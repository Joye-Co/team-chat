//! The system's preview handlers inside the app's preview sheet: what Quick Look's plug-ins
//! are on macOS. A file type the app has no viewer for (Word, Excel, PowerPoint, RTF, mail,
//! fonts, whatever an installed program registered) is drawn by the handler Windows has for
//! it: the shell extension `{8895b1c6-b41f-4c1c-a562-0d564250836f}` of the type, an
//! `IPreviewHandler`. Explorer's preview pane and Outlook use the same handlers.
//!
//! The handler is created as a local server: it runs out of process, in the system's
//! `prevhost.exe` at low integrity, which is how Windows previews files it does not
//! trust. It gets the file as a read-only stream (or its path, for the few handlers that
//! want one) and a child window of the app's window to draw in; the app places that child
//! over the sheet's body every frame. Closing the sheet unloads the handler.
//!
//! Only files 打开 would hand to the system come here ([`previewable`]): a type Windows
//! would run is never given to a handler.

use std::path::Path;

use gpui::{AnyElement, App, ElementId, IntoElement as _, Styled as _, Window, canvas};
use windows::{
    Win32::{
        Foundation::{HWND, RECT},
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
                CreateWindowExW, DestroyWindow, SWP_NOACTIVATE, SWP_NOZORDER, SetWindowPos,
                WINDOW_EX_STYLE, WS_CHILD, WS_CLIPCHILDREN, WS_VISIBLE,
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
            let child = CreateWindowExW(
                WINDOW_EX_STYLE(0),
                w!("STATIC"),
                PCWSTR::null(),
                WS_CHILD | WS_VISIBLE | WS_CLIPCHILDREN,
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
                child,
                handler,
                placed: std::cell::Cell::new((0, 0, 1, 1)),
            })
        }
    }

    /// Puts the preview at this rectangle of the parent's client area, in pixels.
    pub fn place(&self, x: i32, y: i32, width: i32, height: i32) {
        let (width, height) = (width.max(1), height.max(1));
        if self.placed.replace((x, y, width, height)) == (x, y, width, height) {
            return;
        }
        let rect = RECT {
            left: 0,
            top: 0,
            right: width,
            bottom: height,
        };
        // SAFETY: the child is this host's live window; the rectangle outlives the call.
        unsafe {
            let _ = SetWindowPos(
                self.child,
                None,
                x,
                y,
                width,
                height,
                SWP_NOZORDER | SWP_NOACTIVATE,
            );
            let _ = self.handler.SetRect(&rect);
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
                    host.place(
                        px(bounds.origin.x),
                        px(bounds.origin.y),
                        px(bounds.size.width),
                        px(bounds.size.height),
                    );
                }
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
            DispatchMessageW, EnumChildWindows, MSG, PM_REMOVE, PeekMessageW, WS_OVERLAPPEDWINDOW,
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
    /// Windows previews) is found, takes the file and draws into the app's child window.
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
            assert!(drawn > 0, "the handler put its view into the child window");
            drop(host);
            let _ = DestroyWindow(parent);
        }
    }
}
