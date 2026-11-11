//! The system's preview handlers inside the app's preview sheet, for the few document
//! types where that is safe: what Quick Look's plug-ins are on macOS, but narrower.
//!
//! macOS previews any file in a sandboxed system service. Windows has no such thing: a
//! preview handler (`IPreviewHandler`, the type's `{8895b1c6-…}` shell extension, what
//! Explorer's preview pane uses) is whatever program registered itself for the type, and
//! Explorer itself stopped previewing files from the internet because a handler can
//! fetch remote resources. Attachments come from other members and bots, so a file goes
//! to a handler only when all of this holds ([`allowed`]):
//!
//! - its type is on a short list of documents ([`TYPES`]): RTF and the Office formats;
//! - the handler registered for the type is the one Microsoft ships for it (its class id
//!   is the known one), never a third party's;
//! - that handler runs isolated: its surrogate is the low-integrity `prevhost.exe` (its
//!   AppID does not set `DisableLowILProcessIsolation`);
//! - 打开 would hand the file to the system at all (never a program or a script).
//!
//! Everything else shows the sheet's "no preview" state with 用系统应用打开.
//!
//! A handler is another program and may be slow or hang, so the app never calls it from
//! its own thread: a dedicated thread with its own apartment and message pump creates
//! it, gives it the file (a read-only stream) and the window, and takes the later calls
//! (resize, unload). The app's thread only looks at a shared state each frame; a handler
//! that has not started within [`START_LIMIT`] is abandoned and the sheet says that
//! there is no preview.
//!
//! The handler draws into a popup the app's window owns, placed over the sheet's body
//! every frame, not into a child of the app's window: GPUI presents through a flip-model
//! swap chain, which covers what a child window draws with GDI. The popup never takes
//! activation. The handler gets a site ([`Frame`], an `IPreviewHandlerFrame`): when the
//! focus is in the preview, Space and Esc close the sheet and the app's own shortcuts go
//! back to the app's window.

use std::{
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    time::{Duration, Instant},
};

use gpui::{AnyElement, App, ElementId, IntoElement as _, Styled as _, Window, canvas};
use windows::{
    Win32::{
        Foundation::{E_NOTIMPL, HWND, LPARAM, POINT, RECT, S_FALSE, WPARAM},
        Graphics::Gdi::ClientToScreen,
        System::{
            Com::{
                CLSCTX_LOCAL_SERVER, CLSIDFromString, COINIT_APARTMENTTHREADED, CoCreateInstance,
                CoInitializeEx, CoUninitialize, IStream, STGM_READ, STGM_SHARE_DENY_NONE,
            },
            Ole::IObjectWithSite,
        },
        UI::{
            Input::KeyboardAndMouse::{GetAsyncKeyState, VK_CONTROL, VK_ESCAPE, VK_MENU, VK_SPACE},
            Shell::{
                ASSOCF_INIT_DEFAULTTOSTAR, ASSOCSTR_SHELLEXTENSION, AssocQueryStringW,
                IInitializeWithItem, IPreviewHandler, IPreviewHandlerFrame,
                IPreviewHandlerFrame_Impl, IShellItem, PREVIEWHANDLERFRAMEINFO,
                PropertiesSystem::{IInitializeWithFile, IInitializeWithStream},
                SHCreateItemFromParsingName, SHCreateStreamOnFileEx,
            },
            WindowsAndMessaging::{
                CreateWindowExW, DispatchMessageW, MSG, PM_REMOVE, PeekMessageW, PostMessageW,
                SWP_ASYNCWINDOWPOS, SWP_HIDEWINDOW, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE,
                SWP_NOZORDER, SWP_SHOWWINDOW, SetWindowPos, TranslateMessage, WM_CLOSE, WM_KEYDOWN,
                WM_SYSKEYDOWN, WS_CLIPCHILDREN, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_POPUP,
            },
        },
    },
    core::{GUID, HSTRING, Interface as _, PCWSTR, PWSTR, implement, w},
};

use crate::win32::wide;

/// The shell extension id under which a file type names its preview handler.
const PREVIEW_HANDLER: PCWSTR = w!("{8895b1c6-b41f-4c1c-a562-0d564250836f}");

/// How long a handler may take to show its first preview before it is abandoned.
pub const START_LIMIT: Duration = Duration::from_secs(8);

/// Microsoft's own preview handlers, by class id.
const RTF: GUID = GUID::from_u128(0xa42c2ccb_67d3_46fa_abe6_7d2f3488c7a3);
const WORD: GUID = GUID::from_u128(0x84f66100_ff7c_4fb4_b0c0_02cd7fb668fe);
const EXCEL: GUID = GUID::from_u128(0x00020827_0000_0000_c000_000000000046);
const POWERPOINT: GUID = GUID::from_u128(0x65235197_874b_4a07_bdc5_e65ea825b718);

/// The document types a handler may preview, each with the one handler trusted for it:
/// Windows' own RTF previewer and the previewers of Microsoft Office. Formats that carry
/// macros (`docm`, …) are not here (打开 does not open them either), nor is mail (its
/// handler loads remote content) or any type a third party's handler would serve.
pub const TYPES: &[(&str, GUID)] = &[
    ("rtf", RTF),
    ("doc", WORD),
    ("docx", WORD),
    ("xls", EXCEL),
    ("xlsx", EXCEL),
    ("ppt", POWERPOINT),
    ("pptx", POWERPOINT),
];

/// The handler trusted for files named like `path`, when the type is on the list.
pub fn trusted_handler(path: &Path) -> Option<GUID> {
    let ext = path.extension()?.to_string_lossy().to_ascii_lowercase();
    TYPES
        .iter()
        .find(|(known, _)| *known == ext)
        .map(|(_, clsid)| *clsid)
}

/// The class id of the preview handler Windows has registered for files named like
/// `path` (anyone's).
pub fn registered_handler(path: &Path) -> Option<GUID> {
    let ext = wide(&format!(".{}", path.extension()?.to_string_lossy()));
    let mut out = [0u16; 64];
    let mut len = out.len() as u32;
    // SAFETY: the strings are NUL-terminated; `out` holds `len` units.
    unsafe {
        AssocQueryStringW(
            ASSOCF_INIT_DEFAULTTOSTAR,
            ASSOCSTR_SHELLEXTENSION,
            PCWSTR(ext.as_ptr()),
            PREVIEW_HANDLER,
            Some(PWSTR(out.as_mut_ptr())),
            &mut len,
        )
        .ok()
        .ok()?;
        CLSIDFromString(PCWSTR(out.as_ptr())).ok()
    }
}

/// Whether the handler `clsid` runs in the low-integrity preview host: its AppID does not
/// turn that isolation off. A handler without an AppID is not a surrogate-hosted one and
/// does not count.
pub fn runs_isolated(clsid: &GUID) -> bool {
    let classes = windows_registry::CLASSES_ROOT;
    let Ok(app_id) = classes
        .open(format!(r"CLSID\{{{clsid:?}}}"))
        .and_then(|key| key.get_string("AppID"))
    else {
        return false;
    };
    !classes
        .open(format!(r"AppID\{app_id}"))
        .and_then(|key| key.get_u32("DisableLowILProcessIsolation"))
        .is_ok_and(|off| off != 0)
}

/// The handler `path` may be given to, when every condition of the module holds.
pub fn allowed(path: &Path) -> Option<GUID> {
    if !crate::views::media::parts::opens_with_system(path) {
        return None;
    }
    let trusted = trusted_handler(path)?;
    (registered_handler(path) == Some(trusted) && runs_isolated(&trusted)).then_some(trusted)
}

/// What a key pressed inside the preview does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Accelerator {
    /// Space or Esc: the sheet closes, as when the focus is in the app.
    Close,
    /// A shortcut of the app (with Ctrl or Alt): it goes to the app's window.
    Forward,
    /// The preview keeps it (arrows, Page Down, …).
    Keep,
}

/// Decides for a key-down message of the preview.
pub fn accelerator(message: u32, key: u16, ctrl: bool, alt: bool) -> Accelerator {
    if message != WM_KEYDOWN && message != WM_SYSKEYDOWN {
        return Accelerator::Keep;
    }
    if !ctrl && !alt && (key == VK_ESCAPE.0 || key == VK_SPACE.0) {
        Accelerator::Close
    } else if ctrl || alt {
        Accelerator::Forward
    } else {
        Accelerator::Keep
    }
}

/// The handler's site: where it hands back the keys it does not use.
#[implement(IPreviewHandlerFrame)]
struct Frame {
    owner: isize,
    close: Arc<AtomicBool>,
}

impl IPreviewHandlerFrame_Impl for Frame_Impl {
    fn GetWindowContext(&self) -> windows::core::Result<PREVIEWHANDLERFRAMEINFO> {
        Err(E_NOTIMPL.into())
    }

    fn TranslateAccelerator(&self, message: *const MSG) -> windows::core::Result<()> {
        // SAFETY: the handler passes a message of its own queue, or null.
        let Some(message) = (unsafe { message.as_ref() }) else {
            return Err(S_FALSE.into());
        };
        // SAFETY: reads the keyboard state.
        let down = |key: u16| unsafe { GetAsyncKeyState(i32::from(key)) < 0 };
        match accelerator(
            message.message,
            message.wParam.0 as u16,
            down(VK_CONTROL.0),
            down(VK_MENU.0),
        ) {
            Accelerator::Close => {
                self.close.store(true, Ordering::SeqCst);
                Ok(())
            }
            Accelerator::Forward => {
                // SAFETY: posts a copy of the key to the app's window.
                unsafe {
                    let _ = PostMessageW(
                        Some(HWND(self.owner as *mut _)),
                        message.message,
                        message.wParam,
                        message.lParam,
                    );
                }
                Ok(())
            }
            Accelerator::Keep => Err(S_FALSE.into()),
        }
    }
}

/// Where a preview is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    /// The handler's thread is starting it.
    Starting,
    /// The handler shows the file.
    Showing,
    /// The handler refused the file, or took too long and was abandoned.
    Failed,
}

/// The phase to show for a handler's own report and the time since it was asked: one that
/// has not answered within the limit counts as failed.
pub fn phase_after(reported: Phase, waited: Duration) -> Phase {
    if reported == Phase::Starting && waited >= START_LIMIT {
        Phase::Failed
    } else {
        reported
    }
}

enum Call {
    Resize(i32, i32),
    Unload,
}

/// A preview handler at work for one file: its thread, and the popup it draws into.
pub struct Host {
    owner: HWND,
    popup: HWND,
    calls: mpsc::Sender<Call>,
    reported: Arc<Mutex<Phase>>,
    close: Arc<AtomicBool>,
    started: Instant,
    placed: std::cell::Cell<(i32, i32, i32, i32)>,
}

impl Host {
    /// Starts the trusted handler for `path` on its own thread, drawing into a popup that
    /// `owner` owns. `None` when no handler may preview the file. Calls no handler.
    pub fn start(owner: HWND, path: &Path) -> Option<Self> {
        let clsid = allowed(path)?;
        // SAFETY: a popup of this thread, owned by `owner` (for a popup the "parent"
        // argument is its owner); it is hidden until it is placed.
        let popup = unsafe {
            CreateWindowExW(
                WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE,
                w!("STATIC"),
                PCWSTR::null(),
                WS_POPUP | WS_CLIPCHILDREN,
                0,
                0,
                1,
                1,
                Some(owner),
                None,
                None,
                None,
            )
            .ok()?
        };
        let (calls, inbox) = mpsc::channel();
        let reported = Arc::new(Mutex::new(Phase::Starting));
        let close = Arc::new(AtomicBool::new(false));
        let job = Job {
            clsid,
            path: path.to_path_buf(),
            owner: owner.0 as isize,
            popup: popup.0 as isize,
            inbox,
            reported: reported.clone(),
            close: close.clone(),
        };
        let spawned = std::thread::Builder::new()
            .name("preview-handler".into())
            .spawn(move || job.run());
        if spawned.is_err() {
            *reported.lock().unwrap_or_else(|e| e.into_inner()) = Phase::Failed;
        }
        Some(Self {
            owner,
            popup,
            calls,
            reported,
            close,
            started: Instant::now(),
            placed: std::cell::Cell::new((0, 0, 1, 1)),
        })
    }

    /// Where the preview is now. Never waits for the handler.
    pub fn phase(&self) -> Phase {
        let reported = *self.reported.lock().unwrap_or_else(|e| e.into_inner());
        phase_after(reported, self.started.elapsed())
    }

    /// True once: a key in the preview asked to close the sheet.
    pub fn take_close(&self) -> bool {
        self.close.swap(false, Ordering::SeqCst)
    }

    /// Puts the preview over this rectangle of the owner's client area, in pixels, and
    /// shows it. Called every frame: the popup follows the owner when it moves. The
    /// handler hears of a new size on its own thread.
    pub fn place(&self, x: i32, y: i32, width: i32, height: i32) {
        let (width, height) = (width.max(1), height.max(1));
        let mut origin = POINT { x, y };
        // SAFETY: the owner is a live window of this thread; the point is valid.
        unsafe {
            let _ = ClientToScreen(self.owner, &mut origin);
        }
        PLACED.set(self.popup.0 as isize);
        let now = (origin.x, origin.y, width, height);
        let before = self.placed.replace(now);
        if before == now {
            return;
        }
        // SAFETY: the popup is this host's window. The call is asynchronous: a window
        // of the handler's process is inside the popup, and a handler that hangs must
        // not hold up this thread.
        unsafe {
            let _ = SetWindowPos(
                self.popup,
                None,
                origin.x,
                origin.y,
                width,
                height,
                SWP_NOZORDER | SWP_NOACTIVATE | SWP_SHOWWINDOW | SWP_ASYNCWINDOWPOS,
            );
        }
        if (before.2, before.3) != (width, height) {
            let _ = self.calls.send(Call::Resize(width, height));
        }
    }

    #[cfg(test)]
    fn popup(&self) -> HWND {
        self.popup
    }
}

impl Drop for Host {
    fn drop(&mut self) {
        // The popup goes out of sight at once, without waiting for anyone; the handler's
        // thread unloads the handler and then closes the popup. A handler that hangs
        // leaves a hidden popup and its thread behind, never a frozen app.
        // SAFETY: the popup is this host's window; the call does not wait.
        unsafe {
            let _ = SetWindowPos(
                self.popup,
                None,
                0,
                0,
                0,
                0,
                SWP_HIDEWINDOW
                    | SWP_NOMOVE
                    | SWP_NOSIZE
                    | SWP_NOZORDER
                    | SWP_NOACTIVATE
                    | SWP_ASYNCWINDOWPOS,
            );
        }
        let _ = self.calls.send(Call::Unload);
    }
}

/// What the handler's thread needs.
struct Job {
    clsid: GUID,
    path: PathBuf,
    owner: isize,
    popup: isize,
    inbox: mpsc::Receiver<Call>,
    reported: Arc<Mutex<Phase>>,
    close: Arc<AtomicBool>,
}

impl Job {
    fn report(&self, phase: Phase) {
        *self.reported.lock().unwrap_or_else(|e| e.into_inner()) = phase;
    }

    /// The handler's thread: a single-threaded apartment that creates the handler, shows
    /// the preview and then pumps messages (the handler calls its site through them) and
    /// takes the host's calls until it is told to unload.
    fn run(self) {
        // SAFETY: COM for this thread, balanced at the end; every COM object of the
        // handler lives and dies on this thread.
        unsafe {
            if CoInitializeEx(None, COINIT_APARTMENTTHREADED).is_err() {
                self.report(Phase::Failed);
                self.close_popup();
                return;
            }
            let handler = self.show();
            self.report(if handler.is_some() {
                Phase::Showing
            } else {
                Phase::Failed
            });
            if let Some(handler) = &handler {
                self.serve(handler);
                let _ = handler.Unload();
            }
            drop(handler);
            self.close_popup();
            CoUninitialize();
        }
    }

    /// Creates the handler, gives it the file, its site and the popup, and shows the
    /// preview.
    unsafe fn show(&self) -> Option<IPreviewHandler> {
        let popup = HWND(self.popup as *mut _);
        // SAFETY: the caller holds COM on this thread; the pointers outlive the calls.
        unsafe {
            let handler: IPreviewHandler =
                CoCreateInstance(&self.clsid, None, CLSCTX_LOCAL_SERVER).ok()?;
            initialize(&handler, &self.path)?;
            if let Ok(sited) = handler.cast::<IObjectWithSite>() {
                let frame: IPreviewHandlerFrame = Frame {
                    owner: self.owner,
                    close: self.close.clone(),
                }
                .into();
                let _ = sited.SetSite(&frame);
            }
            let rect = RECT {
                left: 0,
                top: 0,
                right: 1,
                bottom: 1,
            };
            let shown = handler
                .SetWindow(popup, &rect)
                .and_then(|()| handler.DoPreview());
            if let Err(error) = shown {
                tracing::debug!(%error, "the preview handler did not start");
                let _ = handler.Unload();
                return None;
            }
            Some(handler)
        }
    }

    /// Pumps this thread's messages and takes the host's calls until the host is gone or
    /// asks to unload.
    unsafe fn serve(&self, handler: &IPreviewHandler) {
        loop {
            let mut message = MSG::default();
            // SAFETY: a plain message pump on this thread.
            unsafe {
                while PeekMessageW(&mut message, None, 0, 0, PM_REMOVE).as_bool() {
                    let _ = TranslateMessage(&message);
                    DispatchMessageW(&message);
                }
            }
            match self.inbox.recv_timeout(Duration::from_millis(8)) {
                Ok(Call::Resize(width, height)) => {
                    let rect = RECT {
                        left: 0,
                        top: 0,
                        right: width,
                        bottom: height,
                    };
                    // SAFETY: the handler is this thread's; the rectangle is valid.
                    unsafe {
                        let _ = handler.SetRect(&rect);
                    }
                }
                Ok(Call::Unload) | Err(mpsc::RecvTimeoutError::Disconnected) => return,
                Err(mpsc::RecvTimeoutError::Timeout) => {}
            }
        }
    }

    /// Asks the app's thread to destroy the popup (its window procedure does, on close).
    fn close_popup(&self) {
        // SAFETY: posts to a window of the app's thread; a window that is gone is fine.
        unsafe {
            let _ = PostMessageW(
                Some(HWND(self.popup as *mut _)),
                WM_CLOSE,
                WPARAM(0),
                LPARAM(0),
            );
        }
    }
}

/// Gives the handler the file: as a read-only stream (what handlers in the low-integrity
/// host take), else by path, else as a shell item.
unsafe fn initialize(handler: &IPreviewHandler, path: &Path) -> Option<()> {
    let name = HSTRING::from(path.as_os_str());
    // SAFETY: the caller holds COM on this thread; the strings outlive the calls.
    unsafe {
        if let Ok(init) = handler.cast::<IInitializeWithStream>() {
            let stream: IStream =
                SHCreateStreamOnFileEx(&name, (STGM_READ | STGM_SHARE_DENY_NONE).0, 0, false, None)
                    .ok()?;
            return init.Initialize(&stream, STGM_READ.0).ok();
        }
        if let Ok(init) = handler.cast::<IInitializeWithFile>() {
            return init.Initialize(&name, STGM_READ.0).ok();
        }
        let init = handler.cast::<IInitializeWithItem>().ok()?;
        let item: IShellItem = SHCreateItemFromParsingName(&name, None).ok()?;
        init.Initialize(&item, STGM_READ.0).ok()
    }
}

thread_local! {
    /// The popup of the preview that was placed last, for [`screen_probe`].
    static PLACED: std::cell::Cell<isize> = const { std::cell::Cell::new(0) };
}

/// For the screenshot self-test: what the screen shows where the last preview is, as a
/// PNG. The self-test captures the app's window, which does not include the popup the
/// handler draws into; the screen does. Brings the app's window above the others first.
pub fn screen_probe() -> Option<Vec<u8>> {
    use windows::Win32::UI::WindowsAndMessaging::{GW_OWNER, GetWindow, HWND_TOPMOST, IsWindow};
    let popup = HWND(PLACED.get() as *mut _);
    // SAFETY: the handle is checked before use; the call only raises the app's window.
    unsafe {
        if !IsWindow(Some(popup)).as_bool() {
            return None;
        }
        let _ = SetWindowPos(
            GetWindow(popup, GW_OWNER).unwrap_or(popup),
            Some(HWND_TOPMOST),
            0,
            0,
            0,
            0,
            SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
        );
    }
    std::thread::sleep(Duration::from_millis(300));
    let (x, y, w, h) = artifact_view::win_support::client_rect_on_screen(popup.0 as isize);
    artifact_view::win_support::capture_screen_png(x, y, w, h)
}

/// The handler of one open preview, kept while its sheet is drawn.
struct Shown(Option<Host>);

/// What the sheet's body is for a file without a viewer of the app's own.
pub enum Body {
    /// The handler is starting: nothing to show yet.
    Starting(AnyElement),
    /// The handler draws the body (the element keeps its popup in place).
    Showing(AnyElement),
    /// No handler may preview the file, or it failed: the sheet says "no preview".
    None,
}

/// The body of a preview sheet for `path`. Starts the handler on first use; never waits
/// for it.
pub fn body(id: impl Into<ElementId>, path: &Path, window: &mut Window, cx: &mut App) -> Body {
    let Some(owner) = crate::win32::hwnd(window) else {
        return Body::None;
    };
    let file = path.to_path_buf();
    let shown = window.use_keyed_state(id.into(), cx, move |_, _| Shown(Host::start(owner, &file)));
    let phase = match &shown.read(cx).0 {
        Some(host) => host.phase(),
        None => return Body::None,
    };
    if phase == Phase::Failed {
        // Abandon it: the popup hides and the thread is told to unload.
        shown.update(cx, |shown, _| shown.0 = None);
        return Body::None;
    }
    let element = canvas(
        move |bounds, window, cx| {
            let scale = window.scale_factor();
            let px = |v: gpui::Pixels| (f32::from(v) * scale).round() as i32;
            let mut close = false;
            if let Some(host) = &shown.read(cx).0 {
                if host.phase() == Phase::Showing {
                    // Not over the sheet's rounded bottom corners.
                    let corner = crate::ui::native::metrics::SHEET_RADIUS;
                    host.place(
                        px(bounds.origin.x),
                        px(bounds.origin.y),
                        px(bounds.size.width),
                        px(bounds.size.height - corner),
                    );
                }
                close = host.take_close();
            }
            if close {
                // Space or Esc inside the preview: close the sheet, after this frame.
                window.defer(cx, |window, cx| {
                    use gpui_component::WindowExt as _;
                    window.close_dialog(cx);
                });
            }
            // The popup is a window of its own and the handler answers on its own time:
            // look again next frame while the preview is open.
            window.request_animation_frame();
        },
        |_, _, _, _| {},
    )
    .size_full()
    .into_any_element();
    match phase {
        Phase::Starting => Body::Starting(element),
        _ => Body::Showing(element),
    }
}

#[cfg(test)]
mod tests {
    use windows::Win32::{
        Foundation::LPARAM,
        UI::WindowsAndMessaging::{
            DestroyWindow, EnumChildWindows, HWND_TOPMOST, IsWindow, WINDOW_EX_STYLE,
            WS_OVERLAPPEDWINDOW, WS_VISIBLE,
        },
    };

    use super::*;

    const SAMPLE: &str = r"{\rtf1\ansi\deff0{\fonttbl{\f0 Segoe UI;}}\f0\fs36 Quarterly report\par\fs24 Drawn by the system's preview handler.\par}";

    fn children(hwnd: HWND) -> usize {
        unsafe extern "system" fn count(_: HWND, n: LPARAM) -> windows::core::BOOL {
            // SAFETY: `n` is the counter `children` passed for this call.
            unsafe { *(n.0 as *mut usize) += 1 };
            true.into()
        }
        let mut n = 0usize;
        // SAFETY: the callback runs inside this call, while `n` is alive.
        unsafe {
            let _ = EnumChildWindows(Some(hwnd), Some(count), LPARAM(&mut n as *mut _ as isize));
        }
        n
    }

    fn pump(for_ms: u64, mut until: impl FnMut() -> bool) {
        let end = Instant::now() + Duration::from_millis(for_ms);
        let mut message = MSG::default();
        while Instant::now() < end && !until() {
            // SAFETY: a plain message pump on this thread.
            unsafe {
                while PeekMessageW(&mut message, None, 0, 0, PM_REMOVE).as_bool() {
                    DispatchMessageW(&message);
                }
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    /// Only document types on the list, only with the handler Microsoft ships for them,
    /// and never a file 打开 would not hand to the system.
    #[test]
    fn only_listed_documents_and_their_own_handlers_are_trusted() {
        let p = |name: &str| std::path::PathBuf::from(format!(r"C:\cache\abc\{name}"));
        assert_eq!(trusted_handler(&p("a.rtf")), Some(RTF));
        assert_eq!(trusted_handler(&p("a.DOCX")), Some(WORD));
        assert_eq!(trusted_handler(&p("a.xlsx")), Some(EXCEL));
        assert_eq!(trusted_handler(&p("a.pptx")), Some(POWERPOINT));
        for name in [
            "a.zip",
            "a.eml",
            "a.msg",
            "a.ttf",
            "a.txt",
            "a.pdf",
            "a.docm",
            "a.xlsm",
            "a.html",
            "a.svg",
            "a.psd",
            "a.dwg",
            "noext",
            "run.bat",
            "setup.exe",
            "a.lnk",
            "report.js",
        ] {
            assert_eq!(trusted_handler(&p(name)), None, "{name}");
            assert_eq!(allowed(&p(name)), None, "{name}");
        }
        // A handler someone else registered for a listed type is not the trusted one.
        assert_ne!(Some(GUID::from_u128(1)), trusted_handler(&p("a.docx")));
    }

    #[test]
    fn keys_in_the_preview_go_back_to_the_app() {
        let (space, esc, w, down) = (VK_SPACE.0, VK_ESCAPE.0, b'W' as u16, 0x28u16);
        assert_eq!(
            accelerator(WM_KEYDOWN, space, false, false),
            Accelerator::Close
        );
        assert_eq!(
            accelerator(WM_KEYDOWN, esc, false, false),
            Accelerator::Close
        );
        assert_eq!(
            accelerator(WM_KEYDOWN, w, true, false),
            Accelerator::Forward
        );
        assert_eq!(
            accelerator(WM_SYSKEYDOWN, w, false, true),
            Accelerator::Forward
        );
        assert_eq!(
            accelerator(WM_KEYDOWN, down, false, false),
            Accelerator::Keep
        );
        assert_eq!(accelerator(WM_KEYDOWN, w, false, false), Accelerator::Keep);
        assert_eq!(
            accelerator(0x0101, esc, false, false),
            Accelerator::Keep,
            "key up"
        );
        // The same through the COM object a handler calls.
        let close = Arc::new(AtomicBool::new(false));
        let frame: IPreviewHandlerFrame = Frame {
            owner: 0,
            close: close.clone(),
        }
        .into();
        let key = |vk: u16| MSG {
            message: WM_KEYDOWN,
            wParam: WPARAM(vk as usize),
            ..Default::default()
        };
        // SAFETY: calls on a live object with valid messages.
        unsafe {
            // A key the preview keeps closes nothing (the wrapper folds S_FALSE into
            // success, so the answer itself is checked through `accelerator` above).
            let _ = frame.TranslateAccelerator(&key(down));
            assert!(!close.load(Ordering::SeqCst));
            assert!(
                frame.TranslateAccelerator(&key(esc)).is_ok(),
                "Esc is handled"
            );
            assert!(close.swap(false, Ordering::SeqCst), "Esc asks to close");
            assert!(frame.TranslateAccelerator(&key(space)).is_ok());
            assert!(close.load(Ordering::SeqCst), "Space asks to close");
        }
    }

    #[test]
    fn a_handler_that_does_not_answer_is_given_up() {
        let soon = Duration::from_secs(1);
        assert_eq!(phase_after(Phase::Starting, soon), Phase::Starting);
        assert_eq!(phase_after(Phase::Starting, START_LIMIT), Phase::Failed);
        assert_eq!(
            phase_after(Phase::Showing, START_LIMIT * 10),
            Phase::Showing
        );
        assert_eq!(phase_after(Phase::Failed, soon), Phase::Failed);
    }

    /// Windows' own RTF handler on this system: registered, the trusted one, isolated. It
    /// is started on its own thread while this thread only pumps its messages, takes the
    /// file and draws the document on the screen, in the popup. Closing unloads it and the
    /// popup goes. A system without the handler says so and is skipped.
    #[test]
    fn the_system_handler_draws_a_document_from_its_own_thread() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("季度 报告.rtf");
        std::fs::write(&file, SAMPLE).unwrap();
        println!(
            "rtf: registered {:?}, trusted {:?}, isolated {}",
            registered_handler(&file),
            trusted_handler(&file),
            runs_isolated(&RTF)
        );
        for ext in ["docx", "xlsx", "pptx", "eml", "ttf", "txt"] {
            let other = dir.path().join(format!("a.{ext}"));
            println!(
                "{ext}: registered {:?}, allowed {}",
                registered_handler(&other),
                allowed(&other).is_some()
            );
        }
        // A type with a handler that is not on the list is never previewed.
        assert_eq!(allowed(&dir.path().join("a.ttf")), None);
        assert_eq!(allowed(&dir.path().join("a.txt")), None);
        if registered_handler(&file) != Some(RTF) {
            println!("no RTF preview handler on this system");
            assert_eq!(allowed(&file), None);
            return;
        }
        assert!(
            runs_isolated(&RTF),
            "Windows' RTF handler runs in the low IL host"
        );
        assert_eq!(allowed(&file), Some(RTF));
        // SAFETY: plain windows of this thread.
        unsafe {
            let owner = CreateWindowExW(
                WINDOW_EX_STYLE(0),
                w!("STATIC"),
                w!("preview test"),
                WS_OVERLAPPEDWINDOW | WS_VISIBLE,
                60,
                60,
                640,
                480,
                None,
                None,
                None,
                None,
            )
            .expect("a window");
            // Above the windows other tests of this process show, so the screen has it.
            let _ = SetWindowPos(owner, Some(HWND_TOPMOST), 60, 60, 640, 480, SWP_NOACTIVATE);
            let started = Instant::now();
            let host = Host::start(owner, &file).expect("a host");
            let start_took = started.elapsed();
            println!("Host::start returned after {start_took:?}");
            assert!(
                start_took < Duration::from_millis(500),
                "starting does not wait for the handler"
            );
            pump(START_LIMIT.as_millis() as u64, || {
                host.phase() != Phase::Starting
            });
            println!(
                "handler phase after {:?}: {:?}",
                started.elapsed(),
                host.phase()
            );
            assert_eq!(host.phase(), Phase::Showing);
            host.place(16, 64, 600, 380);
            pump(3000, || false);
            let drawn = children(host.popup());
            println!("windows the handler made in the popup: {drawn}");
            assert!(drawn > 0, "the handler put its view into the popup");
            let (x, y, w, h) =
                artifact_view::win_support::client_rect_on_screen(host.popup().0 as isize);
            let png = artifact_view::win_support::capture_screen_png(x, y, w, h);
            let ink = png
                .as_deref()
                .and_then(|png| image::load_from_memory(png).ok())
                .map(|image| {
                    image
                        .to_rgb8()
                        .pixels()
                        .filter(|p| p.0.iter().all(|c| *c < 96))
                        .count()
                });
            println!("dark pixels where the preview is: {ink:?} of {}", w * h);
            if let (Some(png), Some(dir)) = (&png, std::env::var_os("TC_PROBE_DIR")) {
                let _ = std::fs::create_dir_all(&dir);
                let _ = std::fs::write(Path::new(&dir).join("preview-handler-rtf.png"), png);
            }
            assert!(
                ink.is_some_and(|ink| ink > 200),
                "the handler drew the document's text on screen"
            );
            // Closing: this thread does not wait; the handler's thread unloads it and
            // the popup is destroyed.
            let popup = host.popup();
            let closing = Instant::now();
            drop(host);
            assert!(
                closing.elapsed() < Duration::from_millis(200),
                "drop does not wait"
            );
            pump(5000, || !IsWindow(Some(popup)).as_bool());
            assert!(!IsWindow(Some(popup)).as_bool(), "the popup is gone");
            let _ = DestroyWindow(owner);
        }
    }
}
