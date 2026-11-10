//! A 产出 page inside a host window: a clipping child window with an isolated WebView2,
//! above the host's own content.
//!
//! The page gets the same isolation as the 产出 window: both use [`Page`] of `win.rs`.
//!
//! The host window must let child windows show: a host that draws through a topmost
//! DirectComposition tree (`CreateTargetForHwnd(hwnd, true)`) covers them.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use serde_json::Value;
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::{ClientToScreen, CreateRoundRectRgn, SetWindowRgn};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    INPUT, INPUT_0, INPUT_MOUSE, MOUSEEVENTF_ABSOLUTE, MOUSEEVENTF_LEFTDOWN, MOUSEEVENTF_LEFTUP,
    MOUSEEVENTF_MOVE, MOUSEEVENTF_VIRTUALDESK, MOUSEINPUT, SendInput, SetFocus,
};
use windows::Win32::UI::Shell::{DefSubclassProc, RemoveWindowSubclass, SetWindowSubclass};
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::core::{PCWSTR, w};

use crate::embed::{EmbedOptions, Placement, Rect, place};
use crate::page::{self, State};
use crate::win::{self, MainThread, Page, PageConfig};
use crate::{ArtifactEvent, Capabilities, Error, Reject, Snapshot, VersionSpec};

/// What the container window and the host's subclass procedure reach the page through.
struct Shared {
    page: RefCell<Option<Rc<Page>>>,
}

impl Shared {
    fn page(&self) -> Option<Rc<Page>> {
        self.page.borrow().clone()
    }
}

unsafe extern "system" fn container_proc(hwnd: HWND, msg: u32, w: WPARAM, l: LPARAM) -> LRESULT {
    // SAFETY: the pointer is the `Shared` the embedded view owns; it is cleared before the
    // view frees it.
    let shared = unsafe { (GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *const Shared).as_ref() };
    match msg {
        // A mouse button went down in the web view: the viewer's input.
        WM_PARENTNOTIFY => {
            if win::is_child_click(w)
                && let Some(page) = shared.and_then(Shared::page)
            {
                page.note_input();
            }
            LRESULT(0)
        }
        // The page draws every pixel of the container.
        WM_ERASEBKGND => LRESULT(1),
        // SAFETY: the default handling of a message Windows sent.
        _ => unsafe { DefWindowProcW(hwnd, msg, w, l) },
    }
}

/// Watches the host window: when it moves, the web view places its popups again.
unsafe extern "system" fn host_proc(
    hwnd: HWND,
    msg: u32,
    w: WPARAM,
    l: LPARAM,
    _id: usize,
    data: usize,
) -> LRESULT {
    if matches!(msg, WM_MOVE | WM_MOVING | WM_WINDOWPOSCHANGED) {
        // SAFETY: `data` is the `Shared` of a live embedded view: the subclass is removed
        // before the view frees it.
        if let Some(page) = unsafe { (data as *const Shared).as_ref() }.and_then(Shared::page) {
            page.parent_moved();
        }
    }
    // SAFETY: passes the message on to the host's own procedure.
    unsafe { DefSubclassProc(hwnd, msg, w, l) }
}

fn register_container_class() -> PCWSTR {
    thread_local! {
        static DONE: Cell<bool> = const { Cell::new(false) };
    }
    let class = w!("ArtifactViewEmbed");
    if !DONE.with(|d| d.replace(true)) {
        // SAFETY: registers a window class of this module once per thread.
        unsafe {
            let wc = WNDCLASSW {
                lpfnWndProc: Some(container_proc),
                hInstance: windows::Win32::System::LibraryLoader::GetModuleHandleW(None)
                    .map(Into::into)
                    .unwrap_or_default(),
                lpszClassName: class,
                ..Default::default()
            };
            RegisterClassW(&wc);
        }
    }
    class
}

/// One 产出 page embedded in a host window. Main thread only. Dropping it removes the view
/// and stops the page (also its media).
pub struct EmbeddedArtifact {
    container: HWND,
    host: HWND,
    page: Rc<Page>,
    state: Rc<RefCell<State>>,
    shared: *mut Shared,
    subclass_id: usize,
    placement: Cell<Option<(Placement, i64)>>,
    /// The host asked to hide the view (a host overlay covers it).
    hidden_by_host: Cell<bool>,
    shown: Cell<bool>,
    corner_radius: f64,
}

impl EmbeddedArtifact {
    /// Adds the page above the content of the host window `host` (its `HWND`), as a child
    /// window. The view stays hidden until the first [`set_frame`](Self::set_frame).
    /// `on_event` runs on the main thread, never inside another call of this type.
    pub fn attach(
        _main: MainThread,
        host: isize,
        options: EmbedOptions,
        on_event: impl Fn(ArtifactEvent) + 'static,
    ) -> Result<Self, Error> {
        let EmbedOptions {
            artifact_id,
            version,
            capabilities,
            dark,
            corner_radius,
            pass_scroll,
            autoplay,
            inspectable,
            viewer,
            ref_menu,
        } = options;
        let host = HWND(host as *mut _);
        // SAFETY: a query on a handle the caller gave.
        if !unsafe { IsWindow(Some(host)) }.as_bool() {
            return Err(Error::NoHostView);
        }
        let state = page::new_state(artifact_id, version, capabilities)?;
        {
            let mut st = state.borrow_mut();
            st.viewer = viewer;
            st.ref_menu = ref_menu;
        }
        let class = register_container_class();
        // SAFETY: creates a hidden child of the host on the main thread.
        let container = unsafe {
            CreateWindowExW(
                WS_EX_NOPARENTNOTIFY,
                class,
                PCWSTR::null(),
                WS_CHILD | WS_CLIPCHILDREN | WS_CLIPSIBLINGS,
                0,
                0,
                1,
                1,
                Some(host),
                None,
                None,
                None,
            )
        }
        .map_err(|e| Error::Native(e.to_string()))?;
        let page = Page::create(
            container,
            state.clone(),
            Rc::new(on_event),
            PageConfig {
                autoplay,
                inspectable,
                // A page with a transparent body shows the host's surface, not white.
                transparent: true,
                fixed_scale: false,
                report_focus: true,
            },
        );
        page.set_visible(false);
        page.set_dark(dark);
        if pass_scroll {
            page.set_scroll_target(host.0 as isize);
        }
        // Tab out of the page: the host window gets the keyboard back.
        page.set_on_focus_out(move || {
            // SAFETY: focuses a window of this thread.
            unsafe {
                let _ = SetFocus(Some(host));
            }
        });
        let shared = Box::into_raw(Box::new(Shared {
            page: RefCell::new(Some(page.clone())),
        }));
        let subclass_id = shared as usize;
        // SAFETY: stores the pointer for `container_proc` and subclasses the host; both
        // are undone in `close` before `shared` is freed.
        unsafe {
            SetWindowLongPtrW(container, GWLP_USERDATA, shared as isize);
            let _ = SetWindowSubclass(host, Some(host_proc), subclass_id, shared as usize);
        }
        Ok(Self {
            container,
            host,
            page,
            state,
            shared,
            subclass_id,
            placement: Cell::new(None),
            hidden_by_host: Cell::new(false),
            shown: Cell::new(false),
            corner_radius,
        })
    }

    /// Places the page from the host element's bounds (`full`) and its unclipped part
    /// (`visible`), both in points from the top-left corner of the host window's client
    /// area. `scale` is the window's pixels per point. An empty visible part hides the
    /// view.
    pub fn set_frame(&self, full: Rect, visible: Rect, scale: f64) {
        if self.is_closed() {
            return;
        }
        let scale = if scale > 0.0 { scale } else { 1.0 };
        let height = win::client_rect(self.host).bottom as f64 / scale;
        let placement =
            place(full, visible, height, true, scale).map(|p| (p, scale.to_bits() as i64));
        if placement != self.placement.get() {
            if let Some((p, _)) = placement {
                let px = |v: f64| (v * scale).round() as i32;
                let (w, h) = (px(p.container.w).max(1), px(p.container.h).max(1));
                // SAFETY: moves and shapes this view's own container window.
                unsafe {
                    let _ = SetWindowPos(
                        self.container,
                        Some(HWND_TOP),
                        px(p.container.x),
                        px(p.container.y),
                        w,
                        h,
                        SWP_NOACTIVATE,
                    );
                    let radius = px(self.corner_radius.max(0.0));
                    if radius > 0 {
                        // The window owns the region after the call.
                        let region = CreateRoundRectRgn(0, 0, w + 1, h + 1, radius * 2, radius * 2);
                        SetWindowRgn(self.container, Some(region), true);
                    }
                }
                let left = px(p.web_view.x);
                let top = px(p.web_view.y);
                self.page.set_bounds(RECT {
                    left,
                    top,
                    right: left + px(p.web_view.w).max(1),
                    bottom: top + px(p.web_view.h).max(1),
                });
            }
            self.placement.set(placement);
        }
        self.apply_hidden();
    }

    /// Hides the view while the host draws something above it (a menu, popover, sheet or
    /// dialog): native views always draw above the host's content.
    pub fn set_hidden(&self, hidden: bool) {
        self.hidden_by_host.set(hidden);
        self.apply_hidden();
    }

    /// True when the view is on screen (placed, not hidden).
    pub fn is_shown(&self) -> bool {
        self.shown.get()
    }

    fn apply_hidden(&self) {
        let hide = self.is_closed() || self.hidden_by_host.get() || self.placement.get().is_none();
        if hide && self.is_focused() {
            self.resign_focus();
        }
        if self.shown.replace(!hide) == hide {
            self.page.set_visible(!hide);
            // SAFETY: shows or hides this view's own container window.
            unsafe {
                let _ = ShowWindow(self.container, if hide { SW_HIDE } else { SW_SHOWNA });
            }
        }
    }

    /// Draws the page with the dark or light appearance (`prefers-color-scheme`).
    pub fn set_dark(&self, dark: bool) {
        self.page.set_dark(dark);
    }

    /// Gives wheel scrolling to the host window (true) or to the page (false).
    pub fn set_pass_scroll(&self, pass: bool) {
        self.page
            .set_scroll_target(if pass { self.host.0 as isize } else { 0 });
    }

    /// Moves keyboard focus into the page.
    pub fn focus(&self) {
        if self.is_shown() {
            self.page.focus();
        }
    }

    /// True while the page has keyboard focus.
    pub fn is_focused(&self) -> bool {
        self.page.is_focused()
    }

    /// Gives keyboard focus back to the host window when the page has it.
    pub fn resign_focus(&self) {
        if self.is_focused() {
            // SAFETY: focuses the host window, which is on this thread.
            unsafe {
                let _ = SetFocus(Some(self.host));
            }
        }
    }

    /// Takes a picture of the page as PNG bytes (for the host to draw while the view is
    /// hidden under an overlay). `done` runs once on the main thread, with `None` on failure.
    pub fn snapshot_png(&self, done: impl FnOnce(Option<Vec<u8>>) + 'static) {
        if self.is_closed() || self.placement.get().is_none() {
            win::post(move || done(None));
            return;
        }
        self.page.capture(None, move |png| done(png.ok()));
    }

    /// Shows another version (new origin, content and data) in the same view. `caps` are
    /// that version's bridge capabilities.
    pub fn show_version(&self, spec: VersionSpec, caps: Capabilities) -> Result<(), Error> {
        page::switch_version(&self.state, spec, caps)?;
        self.page.load_current();
        Ok(())
    }

    pub fn version(&self) -> u32 {
        self.state.borrow().version
    }

    /// The bridge capabilities of the page that shows now.
    pub fn capabilities(&self) -> Capabilities {
        self.state.borrow().bridge.caps()
    }

    /// Gives the page of `version` a data snapshot: the first one answers its waiting
    /// `getData` / `onData`, later ones go to its `onData` callbacks. False when another
    /// version shows.
    pub fn push_data(&self, version: u32, snap: Snapshot) -> bool {
        self.page.push_data(version, snap)
    }

    /// Finishes an open `emit` / `ask` of the page of `version`. False when it is no
    /// longer open (the host drops a late reply).
    pub fn finish(&self, version: u32, reply_id: &Value, result: Result<Value, Reject>) -> bool {
        self.page.finish(version, reply_id, result)
    }

    /// Changes the bridge capabilities of the page that shows now.
    pub fn set_capabilities(&self, caps: Capabilities) {
        page::set_capabilities(&self.state, caps);
    }

    pub fn is_closed(&self) -> bool {
        self.state.borrow().closed
    }

    /// Runs JavaScript in the page (for self-tests and developer tools).
    pub fn evaluate_script(&self, script: &str) {
        if !self.is_closed() {
            self.page.evaluate(script);
        }
    }

    /// Runs JavaScript in the page and gives its value when it is a string (`None` when it
    /// fails or is something else). `done` runs once on the main thread. For self-tests.
    pub fn evaluate_text(&self, script: &str, done: impl FnOnce(Option<String>) + 'static) {
        if self.is_closed() {
            win::post(move || done(None));
            return;
        }
        self.page.evaluate_text(script, done);
    }

    /// Clicks the left mouse button at `(x, y)`, in points from the page's top-left corner,
    /// through the system's input queue (the pointer moves there): Windows dispatches it
    /// like the viewer's own click. False when the page is not on screen. For self-tests.
    pub fn post_click(&self, x: f64, y: f64) -> bool {
        let Some((placement, _)) = self.placement.get() else {
            return false;
        };
        if self.is_closed() || !self.is_shown() {
            return false;
        }
        let scale = win::window_scale(self.host);
        click_in(
            self.container,
            (placement.web_view.x + x) * scale,
            (placement.web_view.y + y) * scale,
        )
    }

    /// Clicks at `(x, y)`, in points from the top-left corner of the host window's client
    /// area, like [`post_click`](Self::post_click). For self-tests of input outside the
    /// page.
    pub fn post_window_click(&self, x: f64, y: f64) -> bool {
        let scale = win::window_scale(self.host);
        click_in(self.host, x * scale, y * scale)
    }

    /// Removes the view and stops the page. A `Closed` event follows once. Safe to call
    /// more than once.
    pub fn close(&self) {
        if self.is_closed() {
            return;
        }
        self.resign_focus();
        self.page.teardown();
        self.shown.set(false);
        // SAFETY: undoes what `attach` set up, then destroys the container. `shared` stays
        // allocated until this value drops, with its page cleared.
        unsafe {
            if let Some(shared) = self.shared.as_ref() {
                shared.page.borrow_mut().take();
            }
            let _ = RemoveWindowSubclass(self.host, Some(host_proc), self.subclass_id);
            if IsWindow(Some(self.container)).as_bool() {
                SetWindowLongPtrW(self.container, GWLP_USERDATA, 0);
                let _ = DestroyWindow(self.container);
            }
        }
    }
}

impl Drop for EmbeddedArtifact {
    fn drop(&mut self) {
        self.close();
        // SAFETY: `close` removed every use of the pointer (the window data and the
        // subclass); it came from `Box::into_raw` in `attach`.
        unsafe {
            drop(Box::from_raw(self.shared));
        }
    }
}

/// Sends a left click at a client-pixel position of `window` through `SendInput`.
fn click_in(window: HWND, x: f64, y: f64) -> bool {
    let mut point = POINT {
        x: x.round() as i32,
        y: y.round() as i32,
    };
    // SAFETY: coordinate queries and synthetic mouse input on the calling desktop.
    unsafe {
        if !ClientToScreen(window, &mut point).as_bool() {
            return false;
        }
        let (left, top) = (
            GetSystemMetrics(SM_XVIRTUALSCREEN),
            GetSystemMetrics(SM_YVIRTUALSCREEN),
        );
        let (width, height) = (
            GetSystemMetrics(SM_CXVIRTUALSCREEN).max(1),
            GetSystemMetrics(SM_CYVIRTUALSCREEN).max(1),
        );
        // Absolute coordinates run from 0 to 65535 over the virtual screen.
        let normalize = |v: i32, origin: i32, size: i32| {
            (((v - origin) as f64 + 0.5) * 65535.0 / size as f64).round() as i32
        };
        let mouse = |flags| INPUT {
            r#type: INPUT_MOUSE,
            Anonymous: INPUT_0 {
                mi: MOUSEINPUT {
                    dx: normalize(point.x, left, width),
                    dy: normalize(point.y, top, height),
                    mouseData: 0,
                    dwFlags: flags | MOUSEEVENTF_ABSOLUTE | MOUSEEVENTF_VIRTUALDESK,
                    time: 0,
                    dwExtraInfo: 0,
                },
            },
        };
        let inputs = [
            mouse(MOUSEEVENTF_MOVE),
            mouse(MOUSEEVENTF_MOVE | MOUSEEVENTF_LEFTDOWN),
            mouse(MOUSEEVENTF_MOVE | MOUSEEVENTF_LEFTUP),
        ];
        SendInput(&inputs, std::mem::size_of::<INPUT>() as i32) == inputs.len() as u32
    }
}
