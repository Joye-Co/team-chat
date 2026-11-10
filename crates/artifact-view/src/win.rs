//! Win32 and WebView2 side: the browser environment, one isolated page, and the window.
//!
//! The rules are the ones of `mac.rs`, with WebView2's means:
//!
//! - **Origin**: `artifact` is registered as a custom scheme with an authority, so a page
//!   lives at `artifact://<id>-v<version>/` exactly as on macOS. `WebResourceRequested`
//!   answers it from [`resolve::serve_range`], with the same headers and CSP.
//! - **No network**: the same handler sees every request of the web view (filter `*`) and
//!   answers anything outside the origin of the version shown with 403. Below it, the
//!   browser process starts with `--host-resolver-rules="MAP * ~NOTFOUND"`, so no host name
//!   or IP literal resolves (this also covers WebSocket and WebRTC, which the handler does
//!   not see) and every connection must use a proxy nothing can reach, so WebRTC has no
//!   transport; the WebRTC constructors and `print` are removed in every frame.
//! - **No navigation away**: `NavigationStarting` and `FrameNavigationStarting` cancel
//!   everything outside the origin; `NewWindowRequested` never opens a window; launching
//!   another app's URL scheme is cancelled.
//! - **Bridge**: `chrome.webview.postMessage`, accepted only from the main frame at the
//!   origin of the version shown, then checked by [`bridge::Bridge`]. Host objects are off.
//! - **Browser features off**: downloads, permission prompts, script dialogs, context
//!   menus, dev tools (unless `inspectable`), browser shortcut keys, autofill, password
//!   saving, zoom, swipe navigation, the status bar, the built-in error page, SmartScreen.
//! - **Storage**: every page is InPrivate with a profile of its own, deleted when the page
//!   closes, in a user-data folder of its own ([`set_user_data_dir`]).

use std::cell::{Cell, RefCell};
use std::collections::{HashSet, VecDeque};
use std::marker::PhantomData;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;

use serde_json::{Value, json};
use webview2_com::Microsoft::Web::WebView2::Win32::*;
use webview2_com::{
    AcceleratorKeyPressedEventHandler, AddScriptToExecuteOnDocumentCreatedCompletedHandler,
    CallDevToolsProtocolMethodCompletedHandler, CoreWebView2CustomSchemeRegistration,
    CoreWebView2EnvironmentOptions, CreateCoreWebView2ControllerCompletedHandler,
    CreateCoreWebView2EnvironmentCompletedHandler, DownloadStartingEventHandler,
    ExecuteScriptCompletedHandler, FocusChangedEventHandler,
    LaunchingExternalUriSchemeEventHandler, MoveFocusRequestedEventHandler,
    NavigationCompletedEventHandler, NavigationStartingEventHandler,
    NewWindowRequestedEventHandler, PermissionRequestedEventHandler, ProcessFailedEventHandler,
    WebMessageReceivedEventHandler, WebResourceRequestedEventHandler, take_pwstr,
};
use windows::Win32::Foundation::{
    E_POINTER, HINSTANCE, HWND, LPARAM, LRESULT, POINT, RECT, WPARAM,
};
use windows::Win32::Graphics::Gdi::{
    COLOR_WINDOW, CreateFontIndirectW, GetMonitorInfoW, HBRUSH, HFONT, MONITOR_DEFAULTTONEAREST,
    MONITORINFO, MonitorFromWindow,
};
use windows::Win32::System::Com::{COINIT_APARTMENTTHREADED, CoInitializeEx, IStream};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::HiDpi::{AdjustWindowRectExForDpi, GetDpiForSystem, GetDpiForWindow};
use windows::Win32::UI::Input::KeyboardAndMouse::{GetActiveWindow, SetFocus};
use windows::Win32::UI::Input::{
    GetRawInputData, HRAWINPUT, RAWINPUT, RAWINPUTDEVICE, RAWINPUTHEADER, RID_INPUT,
    RIDEV_INPUTSINK, RIDEV_REMOVE, RIM_TYPEMOUSE, RegisterRawInputDevices,
};
use windows::Win32::UI::Shell::{SHCreateMemStream, ShellExecuteW};
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::core::{BOOL, HSTRING, Interface, PCWSTR, PWSTR, w};

use crate::page::{self, Nav, State};
use crate::resolve::{self, SCHEME};
use crate::win_support::decode_base64;
use crate::{
    ArtifactEvent, Capabilities, Error, OpenOptions, Presentation, Rect, Reject, Snapshot,
    VersionItem, VersionSpec, bridge, gate,
};

type WinResult<T> = windows::core::Result<T>;

/// Proof that the caller is on the thread that runs the message loop (the main thread of
/// the app). Not `Send`.
#[derive(Clone, Copy, Debug)]
pub struct MainThread(PhantomData<*const ()>);

impl MainThread {
    /// The token for the calling thread. Call it only on the thread that runs the app's
    /// message loop: web views are created on it and their events arrive on it. It makes
    /// the thread a COM single-threaded apartment when it is not one yet.
    pub fn current() -> Self {
        // SAFETY: initializes COM on this thread; S_FALSE and RPC_E_CHANGED_MODE (already
        // initialized) are fine.
        unsafe {
            let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        }
        Self(PhantomData)
    }
}

// --- Later-turn dispatch -----------------------------------------------------------------

const WM_RUN_JOBS: u32 = WM_APP + 0x51;

thread_local! {
    static JOBS: RefCell<VecDeque<Box<dyn FnOnce()>>> = const { RefCell::new(VecDeque::new()) };
    static DISPATCHER: Cell<isize> = const { Cell::new(0) };
}

unsafe extern "system" fn dispatch_proc(hwnd: HWND, msg: u32, w: WPARAM, l: LPARAM) -> LRESULT {
    if msg == WM_INPUT {
        route_wheel(l);
        // SAFETY: the default handling frees the input of a foreground WM_INPUT.
        return unsafe { DefWindowProcW(hwnd, msg, w, l) };
    }
    if msg == WM_RUN_JOBS {
        loop {
            let job = JOBS.with(|q| q.borrow_mut().pop_front());
            match job {
                Some(job) => job(),
                None => break,
            }
        }
        return LRESULT(0);
    }
    // SAFETY: forwards to the default procedure with the arguments Windows gave.
    unsafe { DefWindowProcW(hwnd, msg, w, l) }
}

fn module() -> HINSTANCE {
    // SAFETY: the handle of the running executable.
    unsafe { GetModuleHandleW(None) }
        .map(Into::into)
        .unwrap_or_default()
}

fn dispatcher() -> HWND {
    let existing = DISPATCHER.with(Cell::get);
    if existing != 0 {
        return HWND(existing as *mut _);
    }
    // SAFETY: registers a class and creates a message-only window on this thread.
    let hwnd = unsafe {
        let class = w!("ArtifactViewDispatch");
        let wc = WNDCLASSW {
            lpfnWndProc: Some(dispatch_proc),
            hInstance: module(),
            lpszClassName: class,
            ..Default::default()
        };
        RegisterClassW(&wc);
        CreateWindowExW(
            WINDOW_EX_STYLE(0),
            class,
            PCWSTR::null(),
            WINDOW_STYLE(0),
            0,
            0,
            0,
            0,
            Some(HWND_MESSAGE),
            None,
            Some(module()),
            None,
        )
        .unwrap_or_default()
    };
    DISPATCHER.with(|d| d.set(hwnd.0 as isize));
    hwnd
}

/// Runs `job` on a later turn of this thread's message loop, so host code never runs
/// inside a WebView2 or window-procedure call.
pub(crate) fn post(job: impl FnOnce() + 'static) {
    JOBS.with(|q| q.borrow_mut().push_back(Box::new(job)));
    // SAFETY: posts to this thread's own message-only window.
    unsafe {
        let _ = PostMessageW(Some(dispatcher()), WM_RUN_JOBS, WPARAM(0), LPARAM(0));
    }
}

// --- Wheel scrolling that goes to the host -----------------------------------------------

thread_local! {
    /// Containers of embedded pages whose wheel scrolling the host takes, with the host
    /// window each belongs to.
    static WHEEL_ROUTES: RefCell<Vec<(isize, isize)>> = const { RefCell::new(Vec::new()) };
}

/// Gives the wheel over `container` (the window an embedded page lives in) to `host`, or
/// back to the page (`None`).
///
/// The wheel goes to the web view's own window in another process, so the host never
/// sees the message. It reads the system's raw mouse input instead and, when the pointer
/// is over such a container, posts the wheel message the host would have got. Only real
/// input moves the host: a page has no way to produce raw input.
pub(crate) fn set_wheel_route(container: HWND, host: Option<HWND>) {
    let (was_empty, is_empty) = WHEEL_ROUTES.with(|r| {
        let mut routes = r.borrow_mut();
        let was_empty = routes.is_empty();
        routes.retain(|(c, _)| *c != container.0 as isize);
        if let Some(host) = host {
            routes.push((container.0 as isize, host.0 as isize));
        }
        (was_empty, routes.is_empty())
    });
    if was_empty == is_empty {
        return;
    }
    // Generic desktop page, mouse. INPUTSINK: also while another process's window (the
    // web view) has the focus.
    let device = RAWINPUTDEVICE {
        usUsagePage: 1,
        usUsage: 2,
        dwFlags: if is_empty {
            RIDEV_REMOVE
        } else {
            RIDEV_INPUTSINK
        },
        hwndTarget: if is_empty {
            HWND::default()
        } else {
            dispatcher()
        },
    };
    // SAFETY: registers (or removes) this thread's message-only window for raw mouse input.
    unsafe {
        let _ = RegisterRawInputDevices(&[device], std::mem::size_of::<RAWINPUTDEVICE>() as u32);
    }
}

/// Handles one WM_INPUT: a wheel step with the pointer over a routed container becomes a
/// wheel message for its host.
fn route_wheel(input: LPARAM) {
    let mut raw = RAWINPUT::default();
    let mut size = std::mem::size_of::<RAWINPUT>() as u32;
    // SAFETY: reads one raw input record into a buffer of the size given; a record that
    // does not fit (not a mouse) fails and is ignored.
    let read = unsafe {
        GetRawInputData(
            HRAWINPUT(input.0 as *mut _),
            RID_INPUT,
            Some((&mut raw as *mut RAWINPUT).cast()),
            &mut size,
            std::mem::size_of::<RAWINPUTHEADER>() as u32,
        )
    };
    if read == u32::MAX || read == 0 || raw.header.dwType != RIM_TYPEMOUSE.0 {
        return;
    }
    // SAFETY: the record is a mouse record (checked above).
    let (flags, data) = unsafe {
        let buttons = raw.data.mouse.Anonymous.Anonymous;
        (buttons.usButtonFlags as u32, buttons.usButtonData as i16)
    };
    let message = if flags & RI_MOUSE_WHEEL != 0 {
        WM_MOUSEWHEEL
    } else if flags & RI_MOUSE_HWHEEL != 0 {
        WM_MOUSEHWHEEL
    } else {
        return;
    };
    if data == 0 {
        return;
    }
    let mut cursor = POINT::default();
    // SAFETY: queries the pointer and the window under it, then posts to a window of this
    // thread.
    unsafe {
        if GetCursorPos(&mut cursor).is_err() {
            return;
        }
        let mut window = WindowFromPoint(cursor);
        let mut host = None;
        for _ in 0..16 {
            if window.is_invalid() {
                break;
            }
            let found = WHEEL_ROUTES.with(|r| {
                r.borrow()
                    .iter()
                    .find(|(c, _)| *c == window.0 as isize)
                    .map(|(_, h)| *h)
            });
            if found.is_some() {
                host = found;
                break;
            }
            window = GetAncestor(window, GA_PARENT);
        }
        let Some(host) = host else {
            return;
        };
        let position = LPARAM(
            (((cursor.y as i16 as u16 as u32) << 16) | (cursor.x as i16 as u16 as u32)) as isize,
        );
        let wparam = WPARAM(((data as u16 as u32) << 16) as usize);
        let _ = PostMessageW(Some(HWND(host as *mut _)), message, wparam, position);
    }
}

// --- Environment -------------------------------------------------------------------------

static USER_DATA_DIR: OnceLock<PathBuf> = OnceLock::new();

/// Sets the folder that holds the WebView2 user data of 产出 pages (a folder of its own,
/// under the app's cache folder). Call it once before the first page opens; false when it
/// was set already. Without it the folder is `%LOCALAPPDATA%\artifact-view\WebView2`.
pub fn set_user_data_dir(dir: impl Into<PathBuf>) -> bool {
    USER_DATA_DIR.set(dir.into()).is_ok()
}

fn user_data_dir(autoplay: bool) -> PathBuf {
    let base = USER_DATA_DIR.get().cloned().unwrap_or_else(|| {
        std::env::var_os("LOCALAPPDATA")
            .map(PathBuf::from)
            .unwrap_or_else(std::env::temp_dir)
            .join("artifact-view")
            .join("WebView2")
    });
    // The browser arguments belong to the folder: every environment on one folder must
    // use the same ones.
    base.join(if autoplay { "autoplay" } else { "strict" })
}

/// The command line of the browser process: nothing in it reaches the network.
///
/// - No host name resolves (`--host-resolver-rules`).
/// - Every connection, also to an IP literal and to this computer, must go through a proxy
///   at an address nothing can connect to (`0.0.0.0:1`, with the loopback bypass removed).
///   Chromium never falls back to a direct connection for a proxy given this way.
/// - WebRTC may not open UDP sockets outside that proxy, so with the dead proxy it has no
///   transport at all (its TCP sockets use the proxy too). This is the engine-level stop
///   for WebRTC, which the request filter does not see; the script that removes the
///   constructors is only the first layer.
/// - The browser's own background requests are off.
fn browser_arguments(autoplay: bool) -> String {
    let mut args = String::from(
        "--host-resolver-rules=\"MAP * ~NOTFOUND\" \
         --proxy-server=\"http://0.0.0.0:1\" --proxy-bypass-list=\"<-loopback>\" \
         --force-webrtc-ip-handling-policy=disable_non_proxied_udp \
         --disable-background-networking --disable-domain-reliability --no-pings \
         --dns-prefetch-disable",
    );
    args.push_str(if autoplay {
        " --autoplay-policy=no-user-gesture-required"
    } else {
        " --autoplay-policy=user-gesture-required"
    });
    args
}

type EnvWaiter = Box<dyn FnOnce(Result<ICoreWebView2Environment, String>)>;

#[derive(Default)]
enum EnvSlot {
    #[default]
    Empty,
    Pending(Vec<EnvWaiter>),
    Ready(ICoreWebView2Environment),
}

thread_local! {
    static ENVIRONMENTS: RefCell<[EnvSlot; 2]> = RefCell::new([EnvSlot::Empty, EnvSlot::Empty]);
}

/// Calls `done` with the browser environment (created once per thread and autoplay
/// setting), now or on a later turn.
fn with_environment(
    autoplay: bool,
    done: impl FnOnce(Result<ICoreWebView2Environment, String>) + 'static,
) {
    let index = usize::from(autoplay);
    let mut done: Option<EnvWaiter> = Some(Box::new(done));
    let (ready, create) = ENVIRONMENTS.with(|e| {
        let mut slots = e.borrow_mut();
        match &mut slots[index] {
            EnvSlot::Ready(env) => (Some(env.clone()), false),
            EnvSlot::Pending(waiters) => {
                waiters.extend(done.take());
                (None, false)
            }
            EnvSlot::Empty => {
                slots[index] = EnvSlot::Pending(done.take().into_iter().collect());
                (None, true)
            }
        }
    });
    if let (Some(environment), Some(done)) = (ready, done) {
        done(Ok(environment));
    }
    if create && let Err(e) = create_environment(autoplay) {
        finish_environment(autoplay, Err(format!("webview2 environment: {e}")));
    }
}

fn finish_environment(autoplay: bool, result: Result<ICoreWebView2Environment, String>) {
    let index = usize::from(autoplay);
    let waiters = ENVIRONMENTS.with(|e| {
        let mut slots = e.borrow_mut();
        let next = match &result {
            Ok(env) => EnvSlot::Ready(env.clone()),
            Err(_) => EnvSlot::Empty,
        };
        match std::mem::replace(&mut slots[index], next) {
            EnvSlot::Pending(waiters) => waiters,
            _ => Vec::new(),
        }
    });
    for waiter in waiters {
        waiter(result.clone());
    }
}

/// Forgets the environment after its browser process ended: the next page starts a new
/// one.
fn forget_environment(autoplay: bool) {
    ENVIRONMENTS.with(|e| {
        let mut slots = e.borrow_mut();
        if matches!(slots[usize::from(autoplay)], EnvSlot::Ready(_)) {
            slots[usize::from(autoplay)] = EnvSlot::Empty;
        }
    });
}

fn create_environment(autoplay: bool) -> WinResult<()> {
    let dir = user_data_dir(autoplay);
    let _ = std::fs::create_dir_all(&dir);
    let options = CoreWebView2EnvironmentOptions::default();
    // SAFETY: the setters write plain values into the options object before WebView2 reads
    // it.
    unsafe {
        options.set_additional_browser_arguments(browser_arguments(autoplay));
        // Crash reports stay on this computer.
        options.set_is_custom_crash_reporting_enabled(true);
        let scheme = CoreWebView2CustomSchemeRegistration::new(SCHEME.to_string());
        scheme.set_treat_as_secure(true);
        scheme.set_has_authority_component(true);
        // Pages of the scheme may load their own files with CORS-mode requests (fonts,
        // module scripts); no web origin may.
        scheme.set_allowed_origins(vec![format!("{SCHEME}://*")]);
        options.set_scheme_registrations(vec![Some(scheme.into())]);
    }
    let options: ICoreWebView2EnvironmentOptions = options.into();
    let handler = CreateCoreWebView2EnvironmentCompletedHandler::create(Box::new(
        move |result, environment| {
            let result = result
                .and_then(|()| environment.ok_or_else(|| E_POINTER.into()))
                .map_err(|e| format!("webview2 environment: {e}"));
            finish_environment(autoplay, result);
            Ok(())
        },
    ));
    // SAFETY: the loader copies the strings and keeps the options and handler alive itself.
    unsafe {
        CreateCoreWebView2EnvironmentWithOptions(
            PCWSTR::null(),
            &HSTRING::from(dir.as_os_str()),
            &options,
            &handler,
        )
    }
}

// --- One page ----------------------------------------------------------------------------

/// What differs between the pages of the window, the embedded view and the renderer.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct PageConfig {
    pub(crate) autoplay: bool,
    pub(crate) inspectable: bool,
    /// The page draws no background.
    pub(crate) transparent: bool,
    /// One CSS px is one pixel whatever the monitor (hidden windows that are captured).
    pub(crate) fixed_scale: bool,
    /// Send `FocusChanged` events.
    pub(crate) report_focus: bool,
}

/// The virtual-key code of Esc.
const VK_ESCAPE_CODE: u32 = 0x1b;

/// Runs in every frame of every document, before any page script: removes WebRTC and
/// `print` from the window, and does the same for a child frame's window the moment the
/// page reaches for it (a fresh `about:blank` frame has a window of its own).
const FRAME_GUARD_SCRIPT: &str = r#"(function(){
  var seen = new WeakSet();
  var gone = ['RTCPeerConnection','webkitRTCPeerConnection','RTCDataChannel','RTCIceCandidate','RTCSessionDescription','RTCRtpSender','RTCRtpReceiver','RTCRtpTransceiver','RTCIceTransport','RTCDtlsTransport','RTCSctpTransport'];
  function guard(w) {
    try {
      if (!w || seen.has(w)) return;
      seen.add(w);
      gone.forEach(function (k) {
        try { delete w[k]; } catch (e) {}
        try { Object.defineProperty(w, k, { value: undefined, configurable: false, writable: false }); } catch (e) {}
      });
      try { Object.defineProperty(w, 'print', { value: function () {}, configurable: false, writable: false }); } catch (e) {}
      ['HTMLIFrameElement', 'HTMLFrameElement', 'HTMLObjectElement', 'HTMLEmbedElement'].forEach(function (name) {
        var proto = w[name] && w[name].prototype;
        if (!proto) return;
        ['contentWindow', 'contentDocument'].forEach(function (prop) {
          var d = Object.getOwnPropertyDescriptor(proto, prop);
          if (!d || !d.get) return;
          Object.defineProperty(proto, prop, { configurable: false, enumerable: d.enumerable, get: function () {
            var v = d.get.call(this);
            try { guard(prop === 'contentWindow' ? v : (v && v.defaultView)); } catch (e) {}
            return v;
          } });
        });
      });
    } catch (e) {}
  }
  guard(window);
})();"#;

/// For a page whose wheel scrolling goes to the host: the page itself stays still. The
/// host scrolls from the system's own wheel input ([`set_wheel_route`]), never from
/// anything the page says.
const WHEEL_TEMPLATE: &str = r#"(function(){
  var on = __ON__;
  window.addEventListener('artifactpassscroll', function (e) { on = !!e.detail; });
  window.addEventListener('wheel', function (e) {
    if (on && !e.ctrlKey) e.preventDefault();
  }, { capture: true, passive: false });
})();"#;

/// Headers only this backend adds: the features a page never gets, whatever it asks for.
const PERMISSIONS_POLICY: &str = "accelerometer=(), bluetooth=(), camera=(), \
     display-capture=(), fullscreen=(), geolocation=(), gyroscope=(), hid=(), \
     local-fonts=(), magnetometer=(), microphone=(), midi=(), payment=(), \
     publickey-credentials-get=(), screen-wake-lock=(), serial=(), usb=(), \
     window-management=(), xr-spatial-tracking=()";

/// One isolated WebView2 with its controller, in a parent window this crate owns.
pub(crate) struct Page {
    pub(crate) state: Rc<RefCell<State>>,
    cb: Rc<dyn Fn(ArtifactEvent)>,
    parent: HWND,
    config: PageConfig,
    environment: RefCell<Option<ICoreWebView2Environment>>,
    controller: RefCell<Option<ICoreWebView2Controller>>,
    webview: RefCell<Option<ICoreWebView2>>,
    /// The id of the document-start script of the version shown.
    script_id: RefCell<Option<String>>,
    load_generation: Cell<u64>,
    /// Navigations this page cancelled: their completion is not a load failure.
    cancelled: RefCell<HashSet<u64>>,
    bounds: Cell<RECT>,
    visible: Cell<bool>,
    dark: Cell<Option<bool>>,
    focused: Cell<bool>,
    reloads: Cell<u32>,
    /// The host scrolls on the wheel, the page stays still.
    pass_wheel: Cell<bool>,
    /// This page's own InPrivate profile.
    profile: RefCell<Option<ICoreWebView2Profile>>,
    /// Called when Tab leaves the page: the host takes the keyboard focus back.
    on_focus_out: RefCell<Option<Rc<dyn Fn()>>>,
}

macro_rules! text_of {
    ($object:expr, $method:ident) => {{
        let mut value = PWSTR::null();
        // SAFETY: a COM getter that allocates the string it returns; `take_pwstr` frees it.
        #[allow(unused_unsafe)]
        let result = unsafe { $object.$method(&mut value) };
        result.map(|()| take_pwstr(value))
    }};
}

impl Page {
    /// Starts creating the page in `parent` (a window of this crate). The web view appears
    /// on a later turn; until then calls are remembered (bounds, visibility) or dropped
    /// (scripts).
    pub(crate) fn create(
        parent: HWND,
        state: Rc<RefCell<State>>,
        cb: Rc<dyn Fn(ArtifactEvent)>,
        config: PageConfig,
    ) -> Rc<Self> {
        let page = Rc::new(Self {
            state,
            cb,
            parent,
            config,
            environment: RefCell::new(None),
            controller: RefCell::new(None),
            webview: RefCell::new(None),
            script_id: RefCell::new(None),
            load_generation: Cell::new(0),
            cancelled: RefCell::new(HashSet::new()),
            bounds: Cell::new(RECT::default()),
            visible: Cell::new(true),
            dark: Cell::new(None),
            focused: Cell::new(false),
            reloads: Cell::new(0),
            pass_wheel: Cell::new(false),
            profile: RefCell::new(None),
            on_focus_out: RefCell::new(None),
        });
        let weak = Rc::downgrade(&page);
        with_environment(config.autoplay, move |environment| {
            let Some(page) = weak.upgrade() else {
                return;
            };
            if page.is_closed() {
                return;
            }
            let started =
                environment.and_then(|env| page.create_controller(env).map_err(|e| e.to_string()));
            if let Err(reason) = started {
                page.fail(reason);
            }
        });
        page
    }

    pub(crate) fn is_closed(&self) -> bool {
        self.state.borrow().closed
    }

    // The COM pointers are cloned out of their cells before any call: a call into WebView2
    // can run this page's event handlers before it returns, and they use the same cells.
    fn controller(&self) -> Option<ICoreWebView2Controller> {
        self.controller.borrow().clone()
    }

    fn webview(&self) -> Option<ICoreWebView2> {
        self.webview.borrow().clone()
    }

    /// Delivers an event on a later turn of the message loop.
    pub(crate) fn emit(&self, event: ArtifactEvent) {
        let cb = self.cb.clone();
        post(move || cb(event));
    }

    fn fail(&self, reason: String) {
        let version = self.state.borrow().version;
        self.emit(ArtifactEvent::LoadFailed { version, reason });
    }

    fn create_controller(self: &Rc<Self>, environment: ICoreWebView2Environment) -> WinResult<()> {
        // InPrivate needs controller options (WebView2 runtime 101). A runtime without
        // them gets no page rather than a page with a persistent profile.
        let env10: ICoreWebView2Environment10 = environment.cast()?;
        // SAFETY: plain COM calls on a live environment.
        let options = unsafe { env10.CreateCoreWebView2ControllerOptions()? };
        unsafe { options.SetIsInPrivateModeEnabled(true)? };
        // A profile of its own per page, as WKWebView gets a fresh data store per view:
        // storage of an origin never outlives the view that wrote it, and two views of
        // one version do not share it.
        static NEXT_PROFILE: AtomicU64 = AtomicU64::new(1);
        let profile = format!(
            "view-{}-{}",
            std::process::id(),
            NEXT_PROFILE.fetch_add(1, Ordering::Relaxed)
        );
        unsafe { options.SetProfileName(&HSTRING::from(profile))? };
        let weak = Rc::downgrade(self);
        let handler = CreateCoreWebView2ControllerCompletedHandler::create(Box::new(
            move |result, controller| {
                let controller = result.and_then(|()| controller.ok_or_else(|| E_POINTER.into()));
                match (weak.upgrade(), controller) {
                    (Some(page), Ok(controller)) => {
                        if let Err(e) = page.wire(&controller) {
                            // SAFETY: closes the controller this call owns.
                            unsafe {
                                let _ = controller.Close();
                            }
                            page.controller.borrow_mut().take();
                            page.webview.borrow_mut().take();
                            page.fail(format!("webview2 setup: {e}"));
                        }
                    }
                    (Some(page), Err(e)) => page.fail(format!("webview2 controller: {e}")),
                    (None, Ok(controller)) => {
                        // SAFETY: the page is gone; nothing else holds the controller.
                        unsafe {
                            let _ = controller.Close();
                        }
                    }
                    (None, Err(_)) => {}
                }
                Ok(())
            },
        ));
        // SAFETY: `parent` is a live window of this thread; WebView2 keeps the handler.
        unsafe {
            env10.CreateCoreWebView2ControllerWithOptions(self.parent, &options, &handler)?;
        }
        *self.environment.borrow_mut() = Some(environment);
        Ok(())
    }

    /// Applies the settings, installs every handler, then loads the page. Any failure
    /// leaves the page without a web view.
    fn wire(self: &Rc<Self>, controller: &ICoreWebView2Controller) -> WinResult<()> {
        if self.is_closed() {
            // SAFETY: the page closed while the controller was being created.
            unsafe {
                let _ = controller.Close();
            }
            return Ok(());
        }
        let config = self.config;
        // SAFETY: COM calls on the controller and web view WebView2 just handed over, all
        // on the thread that created them.
        unsafe {
            let webview = controller.CoreWebView2()?;

            let settings = webview.Settings()?;
            settings.SetIsScriptEnabled(true)?;
            settings.SetIsWebMessageEnabled(true)?;
            settings.SetAreHostObjectsAllowed(false)?;
            settings.SetAreDefaultScriptDialogsEnabled(false)?;
            settings.SetAreDefaultContextMenusEnabled(config.inspectable)?;
            settings.SetAreDevToolsEnabled(config.inspectable)?;
            settings.SetIsStatusBarEnabled(false)?;
            settings.SetIsZoomControlEnabled(false)?;
            settings.SetIsBuiltInErrorPageEnabled(false)?;
            settings
                .cast::<ICoreWebView2Settings3>()?
                .SetAreBrowserAcceleratorKeysEnabled(config.inspectable)?;
            let autofill = settings.cast::<ICoreWebView2Settings4>()?;
            autofill.SetIsPasswordAutosaveEnabled(false)?;
            autofill.SetIsGeneralAutofillEnabled(false)?;
            settings
                .cast::<ICoreWebView2Settings5>()?
                .SetIsPinchZoomEnabled(false)?;
            settings
                .cast::<ICoreWebView2Settings6>()?
                .SetIsSwipeNavigationEnabled(false)?;
            settings
                .cast::<ICoreWebView2Settings8>()?
                .SetIsReputationCheckingRequired(false)?;

            if config.transparent {
                controller
                    .cast::<ICoreWebView2Controller2>()?
                    .SetDefaultBackgroundColor(COREWEBVIEW2_COLOR {
                        A: 0,
                        R: 0,
                        G: 0,
                        B: 0,
                    })?;
            }
            if config.fixed_scale {
                let scaled = controller.cast::<ICoreWebView2Controller3>()?;
                scaled.SetShouldDetectMonitorScaleChanges(false)?;
                scaled.SetRasterizationScale(1.0)?;
            }

            let mut token = 0i64;

            // Every request of the web view: the origin's files, or 403. Nothing takes the
            // default path: when the page is gone or the request cannot be read, it is 403
            // too.
            let environment = self
                .environment
                .borrow()
                .clone()
                .ok_or_else(|| windows::core::Error::from(E_POINTER))?;
            let weak = Rc::downgrade(self);
            let serve = WebResourceRequestedEventHandler::create(Box::new(move |_, args| {
                let Some(args) = args else {
                    return Ok(());
                };
                let served = weak.upgrade().and_then(|page| {
                    let request = args.Request().ok()?;
                    let url = text_of!(request, Uri).ok()?;
                    let method = text_of!(request, Method).ok()?;
                    let range = request.Headers().ok().and_then(|headers| {
                        let mut value = PWSTR::null();
                        headers
                            .GetHeader(w!("Range"), &mut value)
                            .ok()
                            .map(|()| take_pwstr(value))
                    });
                    page.respond(&url, &method, range.as_deref()).ok()
                });
                let response = match served {
                    Some(response) => response,
                    None => blocked_response(&environment)?,
                };
                args.SetResponse(&response)
            }));
            match webview.cast::<ICoreWebView2_22>() {
                // Also the requests of workers (the CSP allows none).
                Ok(all) => all.AddWebResourceRequestedFilterWithRequestSourceKinds(
                    w!("*"),
                    COREWEBVIEW2_WEB_RESOURCE_CONTEXT_ALL,
                    COREWEBVIEW2_WEB_RESOURCE_REQUEST_SOURCE_KINDS_ALL,
                )?,
                Err(_) => webview.AddWebResourceRequestedFilter(
                    w!("*"),
                    COREWEBVIEW2_WEB_RESOURCE_CONTEXT_ALL,
                )?,
            }
            webview.add_WebResourceRequested(&serve, &mut token)?;

            let weak = Rc::downgrade(self);
            let navigate = NavigationStartingEventHandler::create(Box::new(move |_, args| {
                if let (Some(page), Some(args)) = (weak.upgrade(), args) {
                    let url = text_of!(args, Uri)?;
                    let mut by_user = BOOL::default();
                    args.IsUserInitiated(&mut by_user)?;
                    let mut id = 0u64;
                    args.NavigationId(&mut id)?;
                    let nav =
                        page::navigation(&page.state, &url, by_user.as_bool(), Instant::now());
                    if nav != Nav::Allow {
                        args.SetCancel(true)?;
                        page.cancelled.borrow_mut().insert(id);
                    }
                    if let Nav::External(url) = nav {
                        page.emit(ArtifactEvent::ExternalLink { url });
                    }
                }
                Ok(())
            }));
            webview.add_NavigationStarting(&navigate, &mut token)?;

            // Frames: the CSP allows none, so no frame navigation is ever allowed, not even
            // inside the origin. A frame then only ever holds `about:blank` or `srcdoc`
            // content, whose messages `on_message` drops by their source: the bridge
            // answers the main frame only.
            let frames = NavigationStartingEventHandler::create(Box::new(move |_, args| {
                if let Some(args) = args {
                    args.SetCancel(true)?;
                }
                Ok(())
            }));
            webview.add_FrameNavigationStarting(&frames, &mut token)?;

            let weak = Rc::downgrade(self);
            let completed = NavigationCompletedEventHandler::create(Box::new(move |_, args| {
                if let (Some(page), Some(args)) = (weak.upgrade(), args) {
                    let mut id = 0u64;
                    args.NavigationId(&mut id)?;
                    if page.cancelled.borrow_mut().remove(&id) || page.is_closed() {
                        return Ok(());
                    }
                    let mut ok = BOOL::default();
                    args.IsSuccess(&mut ok)?;
                    let version = page.state.borrow().version;
                    if ok.as_bool() {
                        page.reloads.set(0);
                        page.emit(ArtifactEvent::Loaded { version });
                    } else {
                        let mut status = COREWEBVIEW2_WEB_ERROR_STATUS_UNKNOWN;
                        args.WebErrorStatus(&mut status)?;
                        if status != COREWEBVIEW2_WEB_ERROR_STATUS_OPERATION_CANCELED {
                            page.fail(format!("webview2 error {}", status.0));
                        }
                    }
                }
                Ok(())
            }));
            webview.add_NavigationCompleted(&completed, &mut token)?;

            // No popups. A link the viewer clicked is reported, like a navigation.
            let weak = Rc::downgrade(self);
            let popups = NewWindowRequestedEventHandler::create(Box::new(move |_, args| {
                if let (Some(page), Some(args)) = (weak.upgrade(), args) {
                    args.SetHandled(true)?;
                    let url = text_of!(args, Uri)?;
                    let mut by_user = BOOL::default();
                    args.IsUserInitiated(&mut by_user)?;
                    if let Nav::External(url) =
                        page::navigation(&page.state, &url, by_user.as_bool(), Instant::now())
                    {
                        page.emit(ArtifactEvent::ExternalLink { url });
                    }
                }
                Ok(())
            }));
            webview.add_NewWindowRequested(&popups, &mut token)?;

            let weak = Rc::downgrade(self);
            let messages = WebMessageReceivedEventHandler::create(Box::new(move |_, args| {
                if let (Some(page), Some(args)) = (weak.upgrade(), args) {
                    let source = text_of!(args, Source)?;
                    let message = text_of!(args, WebMessageAsJson)?;
                    page.on_message(&source, message.as_bytes());
                }
                Ok(())
            }));
            webview.add_WebMessageReceived(&messages, &mut token)?;

            let deny = PermissionRequestedEventHandler::create(Box::new(move |_, args| {
                if let Some(args) = args {
                    args.SetState(COREWEBVIEW2_PERMISSION_STATE_DENY)?;
                }
                Ok(())
            }));
            webview.add_PermissionRequested(&deny, &mut token)?;

            let downloads = DownloadStartingEventHandler::create(Box::new(move |_, args| {
                if let Some(args) = args {
                    args.SetCancel(true)?;
                    args.SetHandled(true)?;
                }
                Ok(())
            }));
            webview
                .cast::<ICoreWebView2_4>()?
                .add_DownloadStarting(&downloads, &mut token)?;

            let launches =
                LaunchingExternalUriSchemeEventHandler::create(Box::new(move |_, args| {
                    if let Some(args) = args {
                        args.SetCancel(true)?;
                    }
                    Ok(())
                }));
            webview
                .cast::<ICoreWebView2_18>()?
                .add_LaunchingExternalUriScheme(&launches, &mut token)?;

            let weak = Rc::downgrade(self);
            let failed = ProcessFailedEventHandler::create(Box::new(move |_, args| {
                if let (Some(page), Some(args)) = (weak.upgrade(), args) {
                    let mut kind = COREWEBVIEW2_PROCESS_FAILED_KIND_BROWSER_PROCESS_EXITED;
                    args.ProcessFailedKind(&mut kind)?;
                    page.process_failed(kind);
                }
                Ok(())
            }));
            webview.add_ProcessFailed(&failed, &mut token)?;

            let weak = Rc::downgrade(self);
            let got_focus = FocusChangedEventHandler::create(Box::new(move |_, _| {
                if let Some(page) = weak.upgrade() {
                    page.focus_changed(true);
                }
                Ok(())
            }));
            controller.add_GotFocus(&got_focus, &mut token)?;
            let weak = Rc::downgrade(self);
            let lost_focus = FocusChangedEventHandler::create(Box::new(move |_, _| {
                if let Some(page) = weak.upgrade() {
                    page.focus_changed(false);
                }
                Ok(())
            }));
            controller.add_LostFocus(&lost_focus, &mut token)?;

            // Tab past the last (or before the first) element of the page: the host takes
            // the focus back.
            let weak = Rc::downgrade(self);
            let tab_out = MoveFocusRequestedEventHandler::create(Box::new(move |_, args| {
                if let (Some(page), Some(args)) = (weak.upgrade(), args)
                    && let Some(take) = page.focus_taker()
                {
                    take();
                    args.SetHandled(true)?;
                }
                Ok(())
            }));
            controller.add_MoveFocusRequested(&tab_out, &mut token)?;

            // Keys that are not text (shortcuts, arrows, function keys) count as the
            // viewer's input. Clicks arrive as WM_PARENTNOTIFY in the parent window.
            let weak = Rc::downgrade(self);
            let keys = AcceleratorKeyPressedEventHandler::create(Box::new(move |_, args| {
                if let (Some(page), Some(args)) = (weak.upgrade(), args) {
                    let mut kind = COREWEBVIEW2_KEY_EVENT_KIND_KEY_UP;
                    args.KeyEventKind(&mut kind)?;
                    if kind == COREWEBVIEW2_KEY_EVENT_KIND_KEY_DOWN
                        || kind == COREWEBVIEW2_KEY_EVENT_KIND_SYSTEM_KEY_DOWN
                    {
                        page.note_input();
                        // Esc: the page still gets the key, and the host gets the keyboard
                        // back (an embedded page; the window has no host to give it to).
                        let mut key = 0u32;
                        args.VirtualKey(&mut key)?;
                        if key == VK_ESCAPE_CODE
                            && let Some(take) = page.focus_taker()
                        {
                            take();
                        }
                    }
                }
                Ok(())
            }));
            controller.add_AcceleratorKeyPressed(&keys, &mut token)?;

            *self.profile.borrow_mut() = Some(webview.cast::<ICoreWebView2_13>()?.Profile()?);
            controller.SetBounds(self.bounds.get())?;
            controller.SetIsVisible(self.visible.get())?;
            *self.controller.borrow_mut() = Some(controller.clone());
            *self.webview.borrow_mut() = Some(webview);
        }
        if let Some(dark) = self.dark.take() {
            self.set_dark(dark);
        }
        if config.transparent {
            self.clear_background();
        }
        self.load_current();
        Ok(())
    }

    /// The response for one request: a file of the version shown, or 403 for everything
    /// else (the network, other versions, local files, a closed page).
    fn respond(
        &self,
        url: &str,
        method: &str,
        range: Option<&str>,
    ) -> WinResult<ICoreWebView2WebResourceResponse> {
        let environment = self
            .environment
            .borrow()
            .clone()
            .ok_or_else(|| windows::core::Error::from(E_POINTER))?;
        let response = {
            let st = self.state.borrow();
            match resolve::split_url(url) {
                Some((host, _)) if !st.closed && host == st.host => Some(resolve::serve_range(
                    &st.content,
                    &st.host,
                    &st.entry,
                    method,
                    url,
                    range,
                )),
                _ => None,
            }
        };
        let Some(response) = response else {
            return blocked_response(&environment);
        };
        let mut headers = String::new();
        for (name, value) in &response.headers {
            headers.push_str(name);
            headers.push_str(": ");
            headers.push_str(value);
            headers.push_str("\r\n");
        }
        headers.push_str("Permissions-Policy: ");
        headers.push_str(PERMISSIONS_POLICY);
        let reason = match response.status {
            200 => w!("OK"),
            206 => w!("Partial Content"),
            404 => w!("Not Found"),
            405 => w!("Method Not Allowed"),
            413 => w!("Content Too Large"),
            416 => w!("Range Not Satisfiable"),
            _ => w!("Error"),
        };
        // SAFETY: the stream copies the bytes; the response takes its own reference.
        unsafe {
            let body: Option<IStream> = if response.body.is_empty() {
                None
            } else {
                SHCreateMemStream(Some(&response.body))
            };
            environment.CreateWebResourceResponse(
                body.as_ref(),
                i32::from(response.status),
                reason,
                &HSTRING::from(headers),
            )
        }
    }

    fn on_message(&self, source: &str, raw: &[u8]) {
        {
            let st = self.state.borrow();
            let from_page = resolve::split_url(source).is_some_and(|(host, _)| host == st.host);
            if st.closed || !from_page {
                return;
            }
        }
        let now = Instant::now();
        let (outcome, version, user_action) = {
            let mut st = self.state.borrow_mut();
            let user_action = gate::recent_input(st.last_input, now);
            let version = st.version;
            (st.bridge.handle(raw, now), version, user_action)
        };
        if let Some(reply) = &outcome.reply {
            self.evaluate(reply);
        }
        if let Some(request) = outcome.request {
            self.emit(page::request_event(request, version, user_action));
        }
    }

    fn process_failed(self: &Rc<Self>, kind: COREWEBVIEW2_PROCESS_FAILED_KIND) {
        if self.is_closed() {
            return;
        }
        if kind == COREWEBVIEW2_PROCESS_FAILED_KIND_BROWSER_PROCESS_EXITED {
            forget_environment(self.config.autoplay);
            self.controller.borrow_mut().take();
            self.webview.borrow_mut().take();
            self.fail("web content process ended".into());
        } else if kind == COREWEBVIEW2_PROCESS_FAILED_KIND_RENDER_PROCESS_EXITED {
            self.fail("web content process ended".into());
            let n = self.reloads.get();
            if n < 1 {
                self.reloads.set(n + 1);
                self.load_current();
            }
        }
    }

    fn focus_changed(&self, focused: bool) {
        if self.focused.replace(focused) != focused && self.config.report_focus {
            self.emit(ArtifactEvent::FocusChanged { focused });
        }
    }

    /// Records the viewer's click when it is inside the web view. `position` is the
    /// `lParam` of the parent's WM_PARENTNOTIFY: the pointer in the parent's client pixels.
    /// A click in another child of the parent (the version picker) is not input to the
    /// page.
    pub(crate) fn note_click(&self, position: LPARAM) {
        let x = (position.0 & 0xffff) as u16 as i16 as i32;
        let y = ((position.0 >> 16) & 0xffff) as u16 as i16 as i32;
        let b = self.bounds.get();
        if self.visible.get() && x >= b.left && x < b.right && y >= b.top && y < b.bottom {
            self.note_input();
        }
    }

    /// Records a click or key press of the viewer in this page (see [`gate`]).
    pub(crate) fn note_input(&self) {
        if let Ok(mut st) = self.state.try_borrow_mut()
            && !st.closed
        {
            st.last_input = Some(Instant::now());
        }
    }

    /// Loads the version in the state: replaces the document-start script (the bridge of
    /// this version), then navigates to its entry.
    pub(crate) fn load_current(self: &Rc<Self>) {
        let Some(webview) = self.webview() else {
            // Not created yet: `wire` loads the version the state holds then.
            return;
        };
        let (url, script) = {
            let st = self.state.borrow();
            if st.closed {
                return;
            }
            let init = st.bridge.init(st.viewer.as_ref(), st.ref_menu.as_ref());
            // WebView2 runs the script in every frame of every document: the frame guard
            // everywhere, the bridge only in the main frame of an artifact page.
            let mut script = String::from(FRAME_GUARD_SCRIPT);
            script.push_str("\nif(window===window.top&&location.protocol==='");
            script.push_str(SCHEME);
            script.push_str(":'){\n");
            script.push_str(&bridge::page_script(&init));
            if st.bridge.caps().r#ref
                && let Some(menu) = &st.ref_menu
            {
                script.push('\n');
                script.push_str(&bridge::ref_listener_script(menu));
            }
            script.push('\n');
            let passing = self.pass_wheel.get();
            script.push_str(
                &WHEEL_TEMPLATE.replace("__ON__", if passing { "true" } else { "false" }),
            );
            script.push_str("\n}");
            (
                format!("{SCHEME}://{}/{}", st.host, resolve::encode_path(&st.entry)),
                script,
            )
        };
        let generation = self.load_generation.get() + 1;
        self.load_generation.set(generation);
        // SAFETY: COM calls on this thread's web view.
        unsafe {
            if let Some(old) = self.script_id.borrow_mut().take() {
                let _ = webview.RemoveScriptToExecuteOnDocumentCreated(&HSTRING::from(old));
            }
            let weak = Rc::downgrade(self);
            let target = webview.clone();
            let added = AddScriptToExecuteOnDocumentCreatedCompletedHandler::create(Box::new(
                move |result, id| {
                    let Some(page) = weak.upgrade() else {
                        return Ok(());
                    };
                    if result.is_err() {
                        page.fail("bridge script".into());
                        return Ok(());
                    }
                    if page.load_generation.get() != generation || page.is_closed() {
                        let _ = target.RemoveScriptToExecuteOnDocumentCreated(&HSTRING::from(id));
                        return Ok(());
                    }
                    *page.script_id.borrow_mut() = Some(id);
                    // The navigation starts only now, so the page never runs without the
                    // script.
                    if let Err(e) = target.Navigate(&HSTRING::from(url.as_str())) {
                        page.fail(format!("navigate: {e}"));
                    }
                    Ok(())
                },
            ));
            if let Err(e) =
                webview.AddScriptToExecuteOnDocumentCreated(&HSTRING::from(script), &added)
            {
                self.fail(format!("bridge script: {e}"));
            }
        }
    }

    /// Runs JavaScript in the page; dropped while the web view does not exist.
    pub(crate) fn evaluate(&self, script: &str) {
        if let Some(webview) = self.webview() {
            // SAFETY: a COM call on this thread's web view; no completion handler.
            unsafe {
                let _ = webview.ExecuteScript(
                    &HSTRING::from(script),
                    None::<&ICoreWebView2ExecuteScriptCompletedHandler>,
                );
            }
        }
    }

    /// Runs JavaScript and gives its value when it is a string.
    pub(crate) fn evaluate_text(&self, script: &str, done: impl FnOnce(Option<String>) + 'static) {
        let Some(webview) = self.webview() else {
            post(move || done(None));
            return;
        };
        let done = Rc::new(RefCell::new(Some(done)));
        let on_result = done.clone();
        let handler = ExecuteScriptCompletedHandler::create(Box::new(move |result, json| {
            let text = result
                .ok()
                .and_then(|()| serde_json::from_str::<Value>(&json).ok())
                .and_then(|v| v.as_str().map(str::to_string));
            if let Some(done) = on_result.borrow_mut().take() {
                done(text);
            }
            Ok(())
        }));
        // SAFETY: a COM call on this thread's web view.
        let started = unsafe { webview.ExecuteScript(&HSTRING::from(script), &handler) };
        if started.is_err()
            && let Some(done) = done.borrow_mut().take()
        {
            post(move || done(None));
        }
    }

    pub(crate) fn push_data(&self, version: u32, snap: Snapshot) -> bool {
        let Some(scripts) = page::push_data_scripts(&self.state, version, snap) else {
            return false;
        };
        for script in scripts {
            self.evaluate(&script);
        }
        true
    }

    pub(crate) fn finish(
        &self,
        version: u32,
        reply_id: &Value,
        result: Result<Value, Reject>,
    ) -> bool {
        match page::finish_script(&self.state, version, reply_id, result) {
            Some(script) => {
                self.evaluate(&script);
                true
            }
            None => false,
        }
    }

    /// Places the web view in its parent window, in the parent's pixels.
    pub(crate) fn set_bounds(&self, bounds: RECT) {
        if self.bounds.replace(bounds) == bounds {
            return;
        }
        if let Some(controller) = self.controller() {
            // SAFETY: a COM call on this thread's controller.
            unsafe {
                let _ = controller.SetBounds(bounds);
            }
        }
    }

    pub(crate) fn set_visible(&self, visible: bool) {
        if self.visible.replace(visible) == visible {
            return;
        }
        if let Some(controller) = self.controller() {
            // SAFETY: a COM call on this thread's controller.
            unsafe {
                let _ = controller.SetIsVisible(visible);
            }
        }
    }

    /// Tells the web view that its top-level window moved (it places its popups and the
    /// input method window from it).
    pub(crate) fn parent_moved(&self) {
        if let Some(controller) = self.controller() {
            // SAFETY: a COM call on this thread's controller.
            unsafe {
                let _ = controller.NotifyParentWindowPositionChanged();
            }
        }
    }

    /// `prefers-color-scheme` of the page (WebView2 keeps it per profile; every page has
    /// its own).
    pub(crate) fn set_dark(&self, dark: bool) {
        let Some(profile) = self.profile.borrow().clone() else {
            self.dark.set(Some(dark));
            return;
        };
        // SAFETY: a COM call on this page's profile.
        unsafe {
            let _ = profile.SetPreferredColorScheme(if dark {
                COREWEBVIEW2_PREFERRED_COLOR_SCHEME_DARK
            } else {
                COREWEBVIEW2_PREFERRED_COLOR_SCHEME_LIGHT
            });
        }
    }

    /// Keeps the page still on the wheel (true: the host scrolls) or lets it scroll.
    pub(crate) fn set_pass_wheel(&self, pass: bool) {
        if self.pass_wheel.replace(pass) != pass {
            self.evaluate(&format!(
                "window.dispatchEvent(new CustomEvent('artifactpassscroll',{{detail:{pass}}}));void 0"
            ));
        }
    }

    fn focus_taker(&self) -> Option<Rc<dyn Fn()>> {
        self.on_focus_out.borrow().clone()
    }

    pub(crate) fn set_on_focus_out(&self, take: impl Fn() + 'static) {
        *self.on_focus_out.borrow_mut() = Some(Rc::new(take));
    }

    pub(crate) fn is_focused(&self) -> bool {
        self.focused.get()
    }

    /// Moves keyboard focus into the page.
    pub(crate) fn focus(&self) {
        if let Some(controller) = self.controller() {
            // SAFETY: a COM call on this thread's controller.
            unsafe {
                let _ = controller.MoveFocus(COREWEBVIEW2_MOVE_FOCUS_REASON_PROGRAMMATIC);
            }
        }
    }

    fn call_devtools(
        &self,
        method: PCWSTR,
        params: &Value,
        done: Option<Box<dyn FnOnce(Result<Value, String>)>>,
    ) {
        let Some(webview) = self.webview() else {
            if let Some(done) = done {
                post(move || done(Err("no web view".into())));
            }
            return;
        };
        let done = Rc::new(RefCell::new(done));
        let on_result = done.clone();
        let handler =
            CallDevToolsProtocolMethodCompletedHandler::create(Box::new(move |result, json| {
                if let Some(done) = on_result.borrow_mut().take() {
                    done(match result {
                        Ok(()) => serde_json::from_str(&json).map_err(|e| e.to_string()),
                        Err(e) => Err(e.to_string()),
                    });
                }
                Ok(())
            }));
        // SAFETY: a COM call on this thread's web view. The protocol call works with the
        // dev tools window disabled.
        let started = unsafe {
            webview.CallDevToolsProtocolMethod(method, &HSTRING::from(params.to_string()), &handler)
        };
        if let Err(e) = started
            && let Some(done) = done.borrow_mut().take()
        {
            post(move || done(Err(e.to_string())));
        }
    }

    /// Keeps the page's transparent pixels transparent in captures.
    fn clear_background(&self) {
        self.call_devtools(
            w!("Emulation.setDefaultBackgroundColorOverride"),
            &json!({ "color": { "r": 0, "g": 0, "b": 0, "a": 0 } }),
            None,
        );
    }

    /// The pixels the web view draws per CSS px.
    fn raster_scale(&self) -> f64 {
        self.controller()
            .and_then(|c| c.cast::<ICoreWebView2Controller3>().ok())
            .and_then(|c| {
                let mut scale = 1.0f64;
                // SAFETY: a COM getter with a valid out-pointer.
                unsafe { c.RasterizationScale(&mut scale) }
                    .ok()
                    .map(|()| scale)
            })
            .filter(|s| *s > 0.0)
            .unwrap_or(1.0)
    }

    /// Takes a PNG of the page: of `clip` (CSS px from the top left of the page) at `scale`
    /// output pixels per CSS px, or of what the view shows when `clip` is `None`.
    pub(crate) fn capture(
        &self,
        clip: Option<(Rect, f64)>,
        done: impl FnOnce(Result<Vec<u8>, String>) + 'static,
    ) {
        if self.config.transparent {
            self.clear_background();
        }
        let mut params = json!({ "format": "png", "fromSurface": true });
        if let Some((rect, scale)) = clip {
            params["captureBeyondViewport"] = json!(true);
            params["clip"] = json!({
                "x": rect.x,
                "y": rect.y,
                "width": rect.w.max(1.0),
                "height": rect.h.max(1.0),
                // The capture is in device pixels: take the monitor's scale out.
                "scale": scale / self.raster_scale(),
            });
        }
        self.call_devtools(
            w!("Page.captureScreenshot"),
            &params,
            Some(Box::new(move |result| {
                done(result.and_then(|value| {
                    value["data"]
                        .as_str()
                        .and_then(decode_base64)
                        .filter(|png| !png.is_empty())
                        .ok_or_else(|| "no image".to_string())
                }))
            })),
        );
    }

    /// Stops the page and closes the web view. Runs once; a `Closed` event follows.
    pub(crate) fn teardown(&self) {
        if !page::close(&self.state) {
            return;
        }
        self.on_focus_out.borrow_mut().take();
        self.webview.borrow_mut().take();
        let controller = self.controller.borrow_mut().take();
        let profile = self.profile.borrow_mut().take();
        // SAFETY: closes this page's controller (the page, its media and its handlers
        // end), then deletes its profile with everything the page stored.
        unsafe {
            if let Some(controller) = controller {
                let _ = controller.Close();
            }
            if let Some(profile) = profile.and_then(|p| p.cast::<ICoreWebView2Profile8>().ok()) {
                let _ = profile.Delete();
            }
        }
        self.emit(ArtifactEvent::Closed);
    }
}

/// The answer to every request that is not a file of the version shown.
fn blocked_response(
    environment: &ICoreWebView2Environment,
) -> WinResult<ICoreWebView2WebResourceResponse> {
    // SAFETY: creates a response object with no body.
    unsafe {
        environment.CreateWebResourceResponse(
            None::<&IStream>,
            403,
            w!("Blocked"),
            w!("Content-Type: text/plain\r\nCache-Control: no-store"),
        )
    }
}

/// True for the WM_PARENTNOTIFY of a mouse button or pointer going down in a child window:
/// a click somewhere in the parent's children ([`Page::note_click`] tells where).
pub(crate) fn is_child_click(wparam: WPARAM) -> bool {
    matches!(
        (wparam.0 & 0xffff) as u32,
        WM_LBUTTONDOWN | WM_RBUTTONDOWN | WM_MBUTTONDOWN | WM_XBUTTONDOWN | WM_POINTERDOWN
    )
}

pub(crate) fn client_rect(hwnd: HWND) -> RECT {
    let mut rect = RECT::default();
    // SAFETY: a query with a valid out-pointer.
    unsafe {
        let _ = GetClientRect(hwnd, &mut rect);
    }
    rect
}

/// Pixels per point of the window.
pub(crate) fn window_scale(hwnd: HWND) -> f64 {
    // SAFETY: a query on a window handle; 0 for a dead window.
    let dpi = unsafe { GetDpiForWindow(hwnd) };
    if dpi == 0 { 1.0 } else { dpi as f64 / 96.0 }
}

// --- The window --------------------------------------------------------------------------

const PICKER_ID: usize = 1;
/// Height of the strip that holds the version picker, in points.
const STRIP_HEIGHT: f64 = 40.0;
const PICKER_WIDTH: f64 = 180.0;

struct WindowData {
    page: RefCell<Option<Rc<Page>>>,
    picker: Cell<isize>,
    font: Cell<isize>,
    /// One point is one pixel (a window that is never shown).
    fixed_scale: bool,
}

impl WindowData {
    fn page(&self) -> Option<Rc<Page>> {
        self.page.borrow().clone()
    }

    fn scale(&self, hwnd: HWND) -> f64 {
        if self.fixed_scale {
            1.0
        } else {
            window_scale(hwnd)
        }
    }

    fn picker(&self) -> Option<HWND> {
        let picker = self.picker.get();
        (picker != 0).then(|| HWND(picker as *mut _))
    }

    /// Places the picker strip and the page from the client area.
    fn layout(&self, hwnd: HWND) {
        let client = client_rect(hwnd);
        let scale = self.scale(hwnd);
        let px = |points: f64| (points * scale).round() as i32;
        let mut top = 0;
        // SAFETY: queries and moves a child window of `hwnd`.
        unsafe {
            if let Some(picker) = self.picker()
                && IsWindowVisible(picker).as_bool()
            {
                top = px(STRIP_HEIGHT);
                let _ = SetWindowPos(
                    picker,
                    None,
                    client.right - px(PICKER_WIDTH) - px(12.0),
                    px(8.0),
                    px(PICKER_WIDTH),
                    px(240.0),
                    SWP_NOZORDER | SWP_NOACTIVATE,
                );
            }
        }
        if let Some(page) = self.page() {
            page.set_bounds(RECT {
                left: 0,
                top,
                right: client.right,
                bottom: client.bottom.max(top),
            });
        }
    }
}

fn window_data<'a>(hwnd: HWND) -> Option<&'a WindowData> {
    // SAFETY: the pointer is the box `open` stored; it is cleared before it is freed.
    unsafe { (GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *const WindowData).as_ref() }
}

unsafe extern "system" fn window_proc(hwnd: HWND, msg: u32, w: WPARAM, l: LPARAM) -> LRESULT {
    let data = window_data(hwnd);
    match (msg, data) {
        (WM_SIZE, Some(data)) => {
            data.layout(hwnd);
            LRESULT(0)
        }
        (WM_MOVE | WM_MOVING, Some(data)) => {
            if let Some(page) = data.page() {
                page.parent_moved();
            }
            // SAFETY: the default handling of a message Windows sent.
            unsafe { DefWindowProcW(hwnd, msg, w, l) }
        }
        (WM_PARENTNOTIFY, Some(data)) => {
            if is_child_click(w)
                && let Some(page) = data.page()
            {
                page.note_click(l);
            }
            LRESULT(0)
        }
        (WM_SETFOCUS, Some(data)) => {
            if let Some(page) = data.page() {
                page.focus();
            }
            LRESULT(0)
        }
        (WM_COMMAND, Some(data)) => {
            let code = ((w.0 >> 16) & 0xffff) as u32;
            if (w.0 & 0xffff) == PICKER_ID
                && code == CBN_SELCHANGE
                && let Some(picker) = data.picker()
                && let Some(page) = data.page()
            {
                // SAFETY: combo box messages to this window's own picker.
                let version = unsafe {
                    let index = SendMessageW(picker, CB_GETCURSEL, None, None).0;
                    (index >= 0).then(|| {
                        SendMessageW(picker, CB_GETITEMDATA, Some(WPARAM(index as usize)), None).0
                    })
                };
                if let Some(version) = version.and_then(|v| u32::try_from(v).ok()) {
                    page.emit(ArtifactEvent::VersionSelected { version });
                }
            }
            LRESULT(0)
        }
        (WM_DPICHANGED, Some(data)) => {
            // SAFETY: `l` points at the rectangle Windows suggests for the new DPI.
            unsafe {
                if let Some(rect) = (l.0 as *const RECT).as_ref() {
                    let _ = SetWindowPos(
                        hwnd,
                        None,
                        rect.left,
                        rect.top,
                        rect.right - rect.left,
                        rect.bottom - rect.top,
                        SWP_NOZORDER | SWP_NOACTIVATE,
                    );
                }
            }
            data.layout(hwnd);
            LRESULT(0)
        }
        (WM_CLOSE, Some(data)) => {
            if let Some(page) = data.page() {
                page.teardown();
            }
            // SAFETY: destroys this window.
            unsafe {
                let _ = DestroyWindow(hwnd);
            }
            LRESULT(0)
        }
        (WM_NCDESTROY, Some(_)) => {
            // SAFETY: takes back the box `open` stored; no message for this window follows.
            unsafe {
                let data = SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0) as *mut WindowData;
                if !data.is_null() {
                    let data = Box::from_raw(data);
                    let font = data.font.get();
                    if font != 0 {
                        let _ = windows::Win32::Graphics::Gdi::DeleteObject(
                            HFONT(font as *mut _).into(),
                        );
                    }
                }
                DefWindowProcW(hwnd, msg, w, l)
            }
        }
        // SAFETY: the default handling of a message Windows sent.
        _ => unsafe { DefWindowProcW(hwnd, msg, w, l) },
    }
}

fn register_window_class() -> PCWSTR {
    thread_local! {
        static DONE: Cell<bool> = const { Cell::new(false) };
    }
    let class = w!("ArtifactViewWindow");
    if !DONE.with(|d| d.replace(true)) {
        // SAFETY: registers a window class of this module once per thread.
        unsafe {
            let wc = WNDCLASSW {
                lpfnWndProc: Some(window_proc),
                hInstance: module(),
                lpszClassName: class,
                hCursor: LoadCursorW(None, IDC_ARROW).unwrap_or_default(),
                hbrBackground: HBRUSH((COLOR_WINDOW.0 + 1) as usize as *mut _),
                ..Default::default()
            };
            RegisterClassW(&wc);
        }
    }
    class
}

/// A window that shows one 产出. Main thread only. Dropping it closes the window.
pub struct ArtifactWindow {
    hwnd: HWND,
    page: Rc<Page>,
    state: Rc<RefCell<State>>,
    fixed_scale: bool,
}

impl ArtifactWindow {
    /// Opens a window. `on_event` runs on the main thread, never inside another window
    /// call. The web view is created on a later turn of the message loop; when it cannot
    /// be created or set up, the page does not load and a `LoadFailed` event is sent.
    pub fn open(
        _main: MainThread,
        options: OpenOptions,
        on_event: impl Fn(ArtifactEvent) + 'static,
    ) -> Result<Self, Error> {
        let OpenOptions {
            title,
            width,
            height,
            artifact_id,
            version,
            capabilities,
            versions,
            presentation,
            inspectable,
            max_message_bytes,
            max_payload_bytes,
            transparent,
            viewer,
            ref_menu,
        } = options;
        let state = page::new_state(artifact_id, version, capabilities)?;
        {
            let mut st = state.borrow_mut();
            st.max_message_bytes = max_message_bytes;
            st.max_payload_bytes = max_payload_bytes;
            st.bridge.set_max_bytes(max_message_bytes);
            st.bridge.set_max_payload_bytes(max_payload_bytes);
            st.viewer = viewer;
            st.ref_menu = ref_menu;
        }
        // A window nobody sees is captured: its page uses one pixel per CSS px.
        let fixed_scale = presentation != Presentation::Front;
        let class = register_window_class();
        let style = WS_OVERLAPPEDWINDOW | WS_CLIPCHILDREN;
        let ex_style = if presentation == Presentation::Offscreen {
            WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE | WS_EX_LAYERED
        } else {
            WINDOW_EX_STYLE(0)
        };
        // SAFETY: creates a window of this module's class on the main thread and stores
        // its data box, which WM_NCDESTROY frees.
        let hwnd = unsafe {
            let dpi = GetDpiForSystem().max(96);
            let scale = if fixed_scale { 1.0 } else { dpi as f64 / 96.0 };
            let mut rect = RECT {
                left: 0,
                top: 0,
                right: (width.max(320.0) * scale).round() as i32,
                bottom: (height.max(240.0) * scale).round() as i32,
            };
            let _ = AdjustWindowRectExForDpi(&mut rect, style, false, ex_style, dpi);
            let hwnd = CreateWindowExW(
                ex_style,
                class,
                &HSTRING::from(title.as_str()),
                style,
                CW_USEDEFAULT,
                CW_USEDEFAULT,
                rect.right - rect.left,
                rect.bottom - rect.top,
                None,
                None,
                Some(module()),
                None,
            )
            .map_err(|e| Error::Native(e.to_string()))?;
            let data = Box::new(WindowData {
                page: RefCell::new(None),
                picker: Cell::new(0),
                font: Cell::new(0),
                fixed_scale,
            });
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, Box::into_raw(data) as isize);
            hwnd
        };
        let page = Page::create(
            hwnd,
            state.clone(),
            Rc::new(on_event),
            PageConfig {
                autoplay: false,
                inspectable,
                transparent,
                fixed_scale,
                report_focus: false,
            },
        );
        if let Some(data) = window_data(hwnd) {
            *data.page.borrow_mut() = Some(page.clone());
            data.layout(hwnd);
        }
        let this = Self {
            hwnd,
            page,
            state,
            fixed_scale,
        };
        this.set_versions(&versions);
        match presentation {
            Presentation::Front => {
                this.center();
                this.focus();
            }
            Presentation::Hidden => {}
            Presentation::Offscreen => this.order_offscreen(),
        }
        Ok(this)
    }

    fn data(&self) -> Option<&WindowData> {
        if self.is_closed() {
            return None;
        }
        window_data(self.hwnd)
    }

    fn center(&self) {
        // SAFETY: queries the monitor of this window and moves the window.
        unsafe {
            let monitor = MonitorFromWindow(self.hwnd, MONITOR_DEFAULTTONEAREST);
            let mut info = MONITORINFO {
                cbSize: std::mem::size_of::<MONITORINFO>() as u32,
                ..Default::default()
            };
            let mut frame = RECT::default();
            if GetMonitorInfoW(monitor, &mut info).as_bool()
                && GetWindowRect(self.hwnd, &mut frame).is_ok()
            {
                let work = info.rcWork;
                let (w, h) = (frame.right - frame.left, frame.bottom - frame.top);
                let x = work.left + ((work.right - work.left - w) / 2).max(0);
                let y = work.top + ((work.bottom - work.top - h) / 2).max(0);
                let _ = SetWindowPos(
                    self.hwnd,
                    None,
                    x,
                    y,
                    0,
                    0,
                    SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE,
                );
            }
        }
    }

    /// Shows the window fully transparent, far outside every screen, without activating
    /// it, so its web content paints but nobody sees or clicks it.
    fn order_offscreen(&self) {
        // SAFETY: styles and moves this window.
        unsafe {
            let _ = SetLayeredWindowAttributes(
                self.hwnd,
                windows::Win32::Foundation::COLORREF(0),
                0,
                LWA_ALPHA,
            );
            let _ = SetWindowPos(
                self.hwnd,
                Some(HWND_BOTTOM),
                -32000,
                -32000,
                0,
                0,
                SWP_NOSIZE | SWP_NOACTIVATE,
            );
            let _ = ShowWindow(self.hwnd, SW_SHOWNOACTIVATE);
        }
    }

    /// Shows another version (new origin, content and data) in the same window. `caps` are
    /// that version's bridge capabilities.
    pub fn show_version(&self, spec: VersionSpec, caps: Capabilities) -> Result<(), Error> {
        let version = spec.version;
        page::switch_version(&self.state, spec, caps)?;
        self.select_in_picker(version);
        self.page.load_current();
        Ok(())
    }

    fn select_in_picker(&self, version: u32) {
        let Some(picker) = self.data().and_then(WindowData::picker) else {
            return;
        };
        // SAFETY: combo box messages to this window's own picker.
        unsafe {
            let count = SendMessageW(picker, CB_GETCOUNT, None, None).0.max(0);
            for index in 0..count {
                let item =
                    SendMessageW(picker, CB_GETITEMDATA, Some(WPARAM(index as usize)), None).0;
                if item == version as isize {
                    SendMessageW(picker, CB_SETCURSEL, Some(WPARAM(index as usize)), None);
                    break;
                }
            }
        }
    }

    /// Replaces the rows of the version picker (a strip above the page); an empty list
    /// hides it.
    pub fn set_versions(&self, versions: &[VersionItem]) {
        let Some(data) = self.data() else {
            return;
        };
        // SAFETY: creates and fills a combo box child of this window.
        unsafe {
            if data.picker().is_none() && !versions.is_empty() {
                let Ok(picker) = CreateWindowExW(
                    WINDOW_EX_STYLE(0),
                    w!("COMBOBOX"),
                    PCWSTR::null(),
                    WS_CHILD | WS_VSCROLL | WINDOW_STYLE(CBS_DROPDOWNLIST as u32),
                    0,
                    0,
                    10,
                    10,
                    Some(self.hwnd),
                    Some(HMENU(PICKER_ID as *mut _)),
                    Some(module()),
                    None,
                ) else {
                    return;
                };
                // The system's message font at this window's scale, not the old bitmap font.
                let mut metrics = NONCLIENTMETRICSW {
                    cbSize: std::mem::size_of::<NONCLIENTMETRICSW>() as u32,
                    ..Default::default()
                };
                if windows::Win32::UI::HiDpi::SystemParametersInfoForDpi(
                    SPI_GETNONCLIENTMETRICS.0,
                    metrics.cbSize,
                    Some((&mut metrics as *mut NONCLIENTMETRICSW).cast()),
                    0,
                    GetDpiForWindow(self.hwnd).max(96),
                )
                .is_ok()
                {
                    let font = CreateFontIndirectW(&metrics.lfMessageFont);
                    data.font.set(font.0 as isize);
                    SendMessageW(
                        picker,
                        WM_SETFONT,
                        Some(WPARAM(font.0 as usize)),
                        Some(LPARAM(1)),
                    );
                }
                data.picker.set(picker.0 as isize);
            }
            if let Some(picker) = data.picker() {
                SendMessageW(picker, CB_RESETCONTENT, None, None);
                for item in versions {
                    let label = HSTRING::from(item.label.as_str());
                    let index = SendMessageW(
                        picker,
                        CB_ADDSTRING,
                        None,
                        Some(LPARAM(label.as_ptr() as isize)),
                    )
                    .0;
                    if index >= 0 {
                        SendMessageW(
                            picker,
                            CB_SETITEMDATA,
                            Some(WPARAM(index as usize)),
                            Some(LPARAM(item.version as isize)),
                        );
                    }
                }
                let _ = ShowWindow(
                    picker,
                    if versions.is_empty() {
                        SW_HIDE
                    } else {
                        SW_SHOWNOACTIVATE
                    },
                );
            }
        }
        self.select_in_picker(self.version());
        data.layout(self.hwnd);
    }

    pub fn set_title(&self, title: &str) {
        if !self.is_closed() {
            // SAFETY: sets the title of this window.
            unsafe {
                let _ = SetWindowTextW(self.hwnd, &HSTRING::from(title));
            }
        }
    }

    /// Brings the window to the front and gives it the keyboard.
    pub fn focus(&self) {
        if self.is_closed() {
            return;
        }
        // SAFETY: shows and activates this window.
        unsafe {
            if IsIconic(self.hwnd).as_bool() {
                let _ = ShowWindow(self.hwnd, SW_RESTORE);
            } else {
                let _ = ShowWindow(self.hwnd, SW_SHOW);
            }
            let _ = SetForegroundWindow(self.hwnd);
            let _ = SetFocus(Some(self.hwnd));
        }
        self.page.focus();
    }

    pub fn version(&self) -> u32 {
        self.state.borrow().version
    }

    /// The bridge capabilities of the page that shows now.
    pub fn capabilities(&self) -> Capabilities {
        self.state.borrow().bridge.caps()
    }

    /// Changes the bridge capabilities of the page that shows now.
    pub fn set_capabilities(&self, caps: Capabilities) {
        page::set_capabilities(&self.state, caps);
    }

    /// Gives the page of `version` a data snapshot: the first one answers its waiting
    /// `getData` / `onData`, later ones go to its `onData` callbacks. False when another
    /// version shows or the page is closed.
    pub fn push_data(&self, version: u32, snap: Snapshot) -> bool {
        self.page.push_data(version, snap)
    }

    /// Finishes an open `emit` / `ask` of the page of `version`. False when it is no longer
    /// open (a late reply after a timeout or reload): the host drops it.
    pub fn finish(&self, version: u32, reply_id: &Value, result: Result<Value, Reject>) -> bool {
        self.page.finish(version, reply_id, result)
    }

    pub fn is_closed(&self) -> bool {
        self.state.borrow().closed
    }

    /// Closes the window. A `Closed` event follows once. Safe to call more than once.
    pub fn close(&self) {
        if self.is_closed() {
            return;
        }
        self.page.teardown();
        // SAFETY: destroys this window; its data box is freed in WM_NCDESTROY.
        unsafe {
            let _ = DestroyWindow(self.hwnd);
        }
    }

    /// Resizes the page area (CSS px = points) so content up to this size lays out and can
    /// be captured.
    pub fn set_content_size(&self, width: f64, height: f64) {
        let Some(data) = self.data() else {
            return;
        };
        // SAFETY: resizes this window from its own styles.
        unsafe {
            let dpi = GetDpiForWindow(self.hwnd).max(96);
            let scale = if self.fixed_scale {
                1.0
            } else {
                dpi as f64 / 96.0
            };
            let strip = match data.picker() {
                Some(picker) if IsWindowVisible(picker).as_bool() => STRIP_HEIGHT,
                _ => 0.0,
            };
            let mut rect = RECT {
                left: 0,
                top: 0,
                right: (width.max(1.0) * scale).ceil() as i32,
                bottom: ((height.max(1.0) + strip) * scale).ceil() as i32,
            };
            let style = WINDOW_STYLE(GetWindowLongW(self.hwnd, GWL_STYLE) as u32);
            let ex_style = WINDOW_EX_STYLE(GetWindowLongW(self.hwnd, GWL_EXSTYLE) as u32);
            let _ = AdjustWindowRectExForDpi(&mut rect, style, false, ex_style, dpi);
            let _ = SetWindowPos(
                self.hwnd,
                None,
                0,
                0,
                rect.right - rect.left,
                rect.bottom - rect.top,
                SWP_NOMOVE | SWP_NOZORDER | SWP_NOACTIVATE,
            );
        }
        data.layout(self.hwnd);
    }

    /// Takes a PNG of `rect` of the page (CSS px, from the top-left of the view) at `scale`
    /// output pixels per CSS px. `done` runs on the main thread with the PNG bytes, or an
    /// error text for the log.
    pub fn snapshot(
        &self,
        rect: Rect,
        scale: f64,
        done: impl FnOnce(Result<Vec<u8>, String>) + 'static,
    ) {
        if self.is_closed() {
            post(move || done(Err("closed".into())));
            return;
        }
        self.page.capture(Some((rect, scale)), done);
    }

    /// Runs JavaScript in the page (for self-tests and developer tools).
    pub fn evaluate_script(&self, script: &str) {
        if !self.is_closed() {
            self.page.evaluate(script);
        }
    }
}

impl Drop for ArtifactWindow {
    fn drop(&mut self) {
        self.close();
    }
}

fn shell_open(url: &str) -> bool {
    // SAFETY: asks the shell to open a URL whose scheme the caller checked.
    let result = unsafe {
        ShellExecuteW(
            None,
            w!("open"),
            &HSTRING::from(url),
            PCWSTR::null(),
            PCWSTR::null(),
            SW_SHOWNORMAL,
        )
    };
    result.0 as usize > 32
}

/// Opens a machine's screen sharing address (see [`crate::is_screen_share_url`]) with the
/// system's handler. Returns false for anything else. Call it only after the user agreed.
pub fn open_screen_share_url(url: &str) -> bool {
    crate::is_screen_share_url(url) && shell_open(url)
}

/// Opens an `http`, `https` or `mailto` URL in the default app. Returns false for any other
/// scheme. Call it only after the user agreed.
pub fn open_external_url(url: &str) -> bool {
    page::is_external_url(url)
        && !url
            .chars()
            .any(|c| c.is_control() || c.is_whitespace() || c == '"')
        && shell_open(url)
}

/// Asks the user to confirm with a system message box (确定 / 取消), on a later turn of the
/// message loop so it never runs inside the caller's own work. `done` gets true only for
/// 确定.
pub fn confirm_later(title: &str, detail: &str, done: impl FnOnce(bool) + 'static) {
    let (title, detail) = (HSTRING::from(title), HSTRING::from(detail));
    post(move || {
        // SAFETY: a modal message box owned by this thread's active window.
        let answer = unsafe {
            MessageBoxW(
                Some(GetActiveWindow()),
                &detail,
                &title,
                MB_OKCANCEL | MB_ICONWARNING | MB_DEFBUTTON2,
            )
        };
        done(answer == IDOK);
    });
}
