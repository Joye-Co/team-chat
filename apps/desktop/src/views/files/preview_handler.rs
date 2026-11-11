//! The system's preview handlers inside the app's preview sheet: what Quick Look's plug-ins
//! are on macOS. A file type the app has no viewer for (Word, Excel, PowerPoint, RTF, mail,
//! fonts, whatever an installed program registered) is drawn by the handler Windows has for
//! it: the shell extension `{8895b1c6-b41f-4c1c-a562-0d564250836f}` of the type, an
//! `IPreviewHandler`. Explorer's preview pane and Outlook use the same handlers.
//!
//! The handler is created as a local server: it runs out of process, in the system's
//! `prevhost.exe` at low integrity, which is how Windows previews files it does not
//! trust. It gets the file as a read-only stream (or its path, for the few handlers that
//! want one) and a window to draw in.
//!
//! That window is a popup the app's window owns, placed exactly over the sheet's body
//! every frame, not a child of the app's window: GPUI presents through a flip-model swap
//! chain, which covers whatever a child window draws with GDI (as these handlers do; a
//! WebView2 child brings its own surface and is not affected). An owned popup has its
//! own surface, stays above its owner, hides with it and never takes the focus or a
//! taskbar button. Closing the sheet unloads the handler and destroys the popup.
//!
//! Only files 打开 would hand to the system come here ([`previewable`]): a type Windows
//! would run is never given to a handler.

use std::path::Path;

use gpui::{AnyElement, App, ElementId, IntoElement as _, Styled as _, Window, canvas};
use windows::{
    Win32::{
        Foundation::{HWND, POINT, RECT},
        Graphics::Gdi::ClientToScreen,
        System::Com::{
            CLSCTX_LOCAL_SERVER, CLSIDFromString, CoCreateInstance, IStream, STGM_READ,
            STGM_SHARE_DENY_NONE,
        },
        UI::{
            Shell::{
                ASSOCF_INIT_DEFAULTTOSTAR, ASSOCSTR_SHELLEXTENSION, AssocQueryStringW,
                IInitializeWithItem, IPreviewHandler, IShellItem,
                PropertiesSystem::{IInitializeWithFile, IInitializeWithStream},
                SHCreateItemFromParsingName, SHCreateStreamOnFileEx,
            },
            WindowsAndMessaging::{
                CreateWindowExW, DestroyWindow, SWP_NOACTIVATE, SWP_NOZORDER, SWP_SHOWWINDOW,
                SetWindowPos, WS_CLIPCHILDREN, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_POPUP,
            },
        },
    },
    core::{GUID, HSTRING, Interface as _, PCWSTR, PWSTR, w},
};

use crate::win32::wide;

/// The shell extension id under which a file type names its preview handler.
const PREVIEW_HANDLER: PCWSTR = w!("{8895b1c6-b41f-4c1c-a562-0d564250836f}");

/// Whether `path` may be given to a preview handler: only what 打开 hands to the system.
pub fn previewable(path: &Path) -> bool {
    crate::views::media::parts::opens_with_system(path)
}

/// The class id of the preview handler Windows has for files named like `path`.
pub fn handler_clsid(path: &Path) -> Option<GUID> {
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

/// A preview handler drawing a file into a child of the app's window.
pub struct Host {
    owner: HWND,
    child: HWND,
    handler: IPreviewHandler,
    placed: std::cell::Cell<(i32, i32, i32, i32)>,
}

impl Host {
    /// Starts the handler for `path` in a child window of `parent`. `None` when the type
    /// has no handler or the handler does not take the file.
    pub fn open(parent: HWND, path: &Path) -> Option<Self> {
        if !previewable(path) {
            return None;
        }
        let clsid = handler_clsid(path)?;
        // SAFETY: COM is initialized on this (the main) thread; the child window belongs
        // to `parent`, a window of this thread; every pointer outlives its call.
        unsafe {
            let handler: IPreviewHandler =
                CoCreateInstance(&clsid, None, CLSCTX_LOCAL_SERVER).ok()?;
            initialize(&handler, path)?;
            // For a popup the "parent" argument is its owner.
            let child = CreateWindowExW(
                WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE,
                w!("STATIC"),
                PCWSTR::null(),
                WS_POPUP | WS_CLIPCHILDREN,
                0,
                0,
                1,
                1,
                Some(parent),
                None,
                None,
                None,
            )
            .ok()?;
            let rect = RECT {
                left: 0,
                top: 0,
                right: 1,
                bottom: 1,
            };
            let started = handler
                .SetWindow(child, &rect)
                .and_then(|()| handler.DoPreview());
            if let Err(error) = started {
                tracing::debug!(%error, "the preview handler did not start");
                let _ = handler.Unload();
                let _ = DestroyWindow(child);
                return None;
            }
            Some(Self {
                owner: parent,
                child,
                handler,
                placed: std::cell::Cell::new((0, 0, 1, 1)),
            })
        }
    }

    /// Puts the preview over this rectangle of the owner's client area, in pixels, and
    /// shows it. Called every frame: the popup follows the owner when it moves.
    pub fn place(&self, x: i32, y: i32, width: i32, height: i32) {
        let (width, height) = (width.max(1), height.max(1));
        let mut origin = POINT { x, y };
        // SAFETY: the owner is a live window of this thread; the point is valid.
        unsafe {
            let _ = ClientToScreen(self.owner, &mut origin);
        }
        PLACED.set(self.child.0 as isize);
        let now = (origin.x, origin.y, width, height);
        let before = self.placed.replace(now);
        if before == now {
            return;
        }
        // SAFETY: the popup is this host's live window; the rectangle outlives the call.
        unsafe {
            let _ = SetWindowPos(
                self.child,
                None,
                origin.x,
                origin.y,
                width,
                height,
                SWP_NOZORDER | SWP_NOACTIVATE | SWP_SHOWWINDOW,
            );
            if (before.2, before.3) != (width, height) {
                let rect = RECT {
                    left: 0,
                    top: 0,
                    right: width,
                    bottom: height,
                };
                let _ = self.handler.SetRect(&rect);
            }
        }
    }

    #[cfg(test)]
    fn child(&self) -> HWND {
        self.child
    }
}

impl Drop for Host {
    fn drop(&mut self) {
        // SAFETY: the handler and the window are this host's.
        unsafe {
            let _ = self.handler.Unload();
            let _ = DestroyWindow(self.child);
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
    /// The child window of the preview that was placed last, for [`screen_probe`].
    static PLACED: std::cell::Cell<isize> = const { std::cell::Cell::new(0) };
}

/// For the screenshot self-test: what the screen shows where the last preview is, as a
/// PNG. The self-test captures the app's window, which does not include what another
/// process (the handler's) draws into it; the screen does. Brings the app's window above
/// the others first.
pub fn screen_probe() -> Option<Vec<u8>> {
    use windows::Win32::UI::WindowsAndMessaging::{
        GW_OWNER, GetWindow, HWND_TOPMOST, IsWindow, SWP_NOMOVE, SWP_NOSIZE,
    };
    let child = HWND(PLACED.get() as *mut _);
    // SAFETY: the handle is checked before use; the calls only raise the app's window.
    unsafe {
        if !IsWindow(Some(child)).as_bool() {
            return None;
        }
        let _ = SetWindowPos(
            GetWindow(child, GW_OWNER).unwrap_or(child),
            Some(HWND_TOPMOST),
            0,
            0,
            0,
            0,
            SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
        );
    }
    std::thread::sleep(std::time::Duration::from_millis(300));
    let (x, y, w, h) = artifact_view::win_support::client_rect_on_screen(child.0 as isize);
    artifact_view::win_support::capture_screen_png(x, y, w, h)
}

/// The handler of one open preview, kept while its sheet is drawn.
struct Shown(Option<Host>);

/// The body of a preview sheet for `path`, drawn by the system's handler; `None` when the
/// type has none (the sheet then says that there is no preview).
pub fn body(
    id: impl Into<ElementId>,
    path: &Path,
    window: &mut Window,
    cx: &mut App,
) -> Option<AnyElement> {
    let parent = crate::win32::hwnd(window)?;
    let file = path.to_path_buf();
    let shown = window.use_keyed_state(id.into(), cx, move |_, _| Shown(Host::open(parent, &file)));
    shown.read(cx).0.as_ref()?;
    Some(
        canvas(
            move |bounds, window, cx| {
                let scale = window.scale_factor();
                let px = |v: gpui::Pixels| (f32::from(v) * scale).round() as i32;
                if let Some(host) = &shown.read(cx).0 {
                    // Not over the sheet's rounded bottom corners.
                    let corner = crate::ui::native::metrics::SHEET_RADIUS;
                    host.place(
                        px(bounds.origin.x),
                        px(bounds.origin.y),
                        px(bounds.size.width),
                        px(bounds.size.height - corner),
                    );
                }
                // The popup is a window of its own: follow the app's window while the
                // preview is open (a move draws no frame by itself).
                window.request_animation_frame();
            },
            |_, _, _, _| {},
        )
        .size_full()
        .into_any_element(),
    )
}

#[cfg(test)]
mod tests {
    use windows::Win32::{
        Foundation::LPARAM,
        System::Com::{COINIT_APARTMENTTHREADED, CoInitializeEx},
        UI::WindowsAndMessaging::{
            DispatchMessageW, EnumChildWindows, MSG, PM_REMOVE, PeekMessageW, WINDOW_EX_STYLE,
            WS_OVERLAPPEDWINDOW, WS_VISIBLE,
        },
    };

    use super::*;

    const RTF: &str = r"{\rtf1\ansi\deff0{\fonttbl{\f0 Segoe UI;}}\f0\fs36 Quarterly report\par\fs24 Drawn by the system's preview handler.\par}";

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

    /// A file Windows would run never reaches a handler, whatever is registered for it.
    #[test]
    fn a_risky_file_is_never_given_to_a_handler() {
        for name in [
            "run.bat",
            "setup.exe",
            "report.js",
            "page.html",
            "noext",
            "a.lnk",
        ] {
            let path = std::path::PathBuf::from(format!(r"C:\cache\abc\{name}"));
            assert!(!previewable(&path), "{name}");
        }
        assert!(previewable(Path::new(r"C:\cache\abc\report.docx")));
        assert!(previewable(Path::new(r"C:\cache\abc\notes.rtf")));
        assert!(handler_clsid(Path::new(r"C:\cache\abc\noext")).is_none());
    }

    /// The system's own handler for a type the app has no viewer for (RTF, which every
    /// Windows previews) is found, takes the file and draws into the app's preview window.
    /// A system without that handler says so and is skipped.
    #[test]
    fn the_system_handler_draws_a_file_the_app_has_no_viewer_for() {
        // SAFETY: COM for this test thread; plain windows of this thread.
        unsafe {
            let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        }
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("季度 报告.rtf");
        std::fs::write(&file, RTF).unwrap();
        let clsid = handler_clsid(&file);
        println!("preview handler for .rtf: {clsid:?}");
        for ext in ["docx", "xlsx", "pptx", "eml", "ttf", "zip", "txt"] {
            let found = handler_clsid(&dir.path().join(format!("a.{ext}"))).is_some();
            println!("preview handler for .{ext}: {found}");
        }
        assert!(
            handler_clsid(&dir.path().join("a.tc-no-such-type")).is_none(),
            "an unknown type has no handler"
        );
        if clsid.is_none() {
            println!("no RTF preview handler on this system");
            return;
        }
        // SAFETY: as above.
        unsafe {
            let parent = CreateWindowExW(
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
            let _ = SetWindowPos(
                parent,
                Some(windows::Win32::UI::WindowsAndMessaging::HWND_TOPMOST),
                60,
                60,
                640,
                480,
                SWP_NOACTIVATE,
            );
            let host = Host::open(parent, &file);
            println!("preview handler started: {}", host.is_some());
            let host = host.expect("the handler took the file");
            host.place(16, 64, 600, 380);
            let until = std::time::Instant::now() + std::time::Duration::from_secs(3);
            let mut msg = MSG::default();
            while std::time::Instant::now() < until {
                while PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE).as_bool() {
                    DispatchMessageW(&msg);
                }
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
            let drawn = children(host.child());
            println!("windows the handler made in the app's child: {drawn}");
            assert!(
                drawn > 0,
                "the handler put its view into the preview window"
            );
            // What is on the screen where the preview is: the page's text, drawn by
            // the handler's process (a capture of the app's window would not show it).
            let (x, y, w, h) =
                artifact_view::win_support::client_rect_on_screen(host.child().0 as isize);
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
                let _ = std::fs::write(
                    std::path::Path::new(&dir).join("preview-handler-rtf.png"),
                    png,
                );
            }
            assert!(
                ink.is_some_and(|ink| ink > 200),
                "the handler drew the document's text on screen"
            );
            drop(host);
            let _ = DestroyWindow(parent);
        }
    }
}
