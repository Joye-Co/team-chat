//! AppKit behind the file actions ([`super::export`]): the system Quick Look panel
//! (`QLPreviewPanel`) and the share picker (`NSSharingServicePicker`).
//!
//! Both run on a later main-queue turn, outside any GPUI update: the panel's key-window
//! change and the picker's menu loop call back into GPUI, which must not be borrowed then.
//!
//! Windows has neither. Quick Look becomes "open with the default app" (the closest thing
//! a stock Windows has to a preview), behind the same guard as 打开 ([`super::open_kind`]):
//! a file that Windows would run, install or mount, and any page a browser would run, is
//! shown selected in Explorer instead, so Space on a teammate's file never runs it. 分享
//! shows the file selected in Explorer, where the system's own 共享 command and drag and
//! drop are. On other systems every call does nothing.

use std::path::PathBuf;

use gpui::{Pixels, Point, Window};

/// Shows `path` in the Quick Look panel, or closes the panel when it is open (Space
/// toggles it, as in Finder; the panel itself closes on Space and Escape when it is key).
pub fn toggle_quick_look(path: PathBuf) {
    #[cfg(target_os = "macos")]
    dispatch2::DispatchQueue::main().exec_async(move || mac::toggle_quick_look(&path));
    #[cfg(target_os = "windows")]
    if preview_opens(&path, true) {
        win::shell_open(&path);
    } else {
        win::reveal(&path);
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    let _ = path;
}

/// Whether previewing `path` may hand it to its default app: only what 打开 would hand to
/// the system too. Anything risky, a page (a browser would run it outside the 产出
/// window), and a path without a file name are shown in the file manager instead.
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
pub fn preview_opens(path: &std::path::Path, windows: bool) -> bool {
    let Some(name) = path.file_name().map(|n| n.to_string_lossy().into_owned()) else {
        return false;
    };
    super::open_kind_on(&name, path, windows) == super::OpenKind::System
}

/// Opens the share picker for `paths` at `at` (window points from the top left of the
/// content) in `window`'s content view.
pub fn share(paths: Vec<PathBuf>, at: Point<Pixels>, window: &Window) {
    #[cfg(target_os = "macos")]
    {
        let Some(view) = mac::content_view(window) else {
            return;
        };
        let job = mac::MainOnly((view, paths, f64::from(at.x), f64::from(at.y)));
        dispatch2::DispatchQueue::main().exec_async(move || {
            let job = job;
            let (view, paths, x, y) = job.0;
            mac::share(&view, &paths, x, y);
        });
    }
    #[cfg(target_os = "windows")]
    {
        let _ = (at, window);
        if let Some(path) = paths.first() {
            win::reveal(path);
        }
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    let _ = (paths, at, window);
}

/// The argument that makes Explorer open a folder with `path` selected. Explorer splits
/// its command line at commas, so the path is always quoted.
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
pub fn explorer_select_arg(path: &std::path::Path) -> String {
    format!("/select,\"{}\"", path.display())
}

#[cfg(target_os = "windows")]
mod win {
    use std::path::Path;

    use windows::{
        Win32::UI::{Shell::ShellExecuteW, WindowsAndMessaging::SW_SHOWNORMAL},
        core::{PCWSTR, w},
    };

    use crate::win32::wide;

    /// Opens `path` with the app Windows has for its type. Only for a path that
    /// [`super::preview_opens`] allowed.
    pub fn shell_open(path: &Path) {
        let file = wide(&path.display().to_string());
        // SAFETY: both strings are NUL-terminated and outlive the call.
        unsafe {
            ShellExecuteW(
                None,
                w!("open"),
                PCWSTR(file.as_ptr()),
                PCWSTR::null(),
                PCWSTR::null(),
                SW_SHOWNORMAL,
            );
        }
    }

    /// Shows `path` selected in an Explorer window.
    pub fn reveal(path: &Path) {
        let args = wide(&super::explorer_select_arg(path));
        // SAFETY: as in `shell_open`.
        unsafe {
            ShellExecuteW(
                None,
                w!("open"),
                w!("explorer.exe"),
                PCWSTR(args.as_ptr()),
                PCWSTR::null(),
                SW_SHOWNORMAL,
            );
        }
    }
}

/// The point in an AppKit view's own coordinates for a point measured from the top left:
/// a view that is not flipped counts y from the bottom.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub fn view_point(x: f64, y: f64, height: f64, flipped: bool) -> (f64, f64) {
    if flipped { (x, y) } else { (x, height - y) }
}

#[cfg(target_os = "macos")]
mod mac {
    use std::{cell::RefCell, path::Path, path::PathBuf};

    use objc2::{
        AnyThread as _, MainThreadOnly, define_class, msg_send,
        rc::Retained,
        runtime::{AnyClass, AnyObject, NSObject, NSObjectProtocol},
    };
    use objc2_app_kit::{NSApplication, NSResponder, NSSharingServicePicker, NSView};
    use objc2_foundation::{
        MainThreadMarker, NSArray, NSInteger, NSPoint, NSRect, NSRectEdge, NSSize, NSString, NSURL,
    };

    // QLPreviewPanel lives in Quartz (QuickLookUI); nothing else links it.
    #[link(name = "Quartz", kind = "framework")]
    unsafe extern "C" {}

    /// Lets main-thread values ride a block onto the main queue.
    pub struct MainOnly<T>(pub T);
    // SAFETY: only ever sent to `DispatchQueue::main()`, so the value is made and used on
    // the main thread.
    unsafe impl<T> Send for MainOnly<T> {}

    thread_local! {
        /// The file the panel shows.
        static ITEM: RefCell<Option<Retained<NSURL>>> = const { RefCell::new(None) };
        /// The panel's controller and data source, made once.
        static CONTROLLER: RefCell<Option<Retained<Controller>>> = const { RefCell::new(None) };
        /// The open share picker, kept alive while it shows.
        static PICKER: RefCell<Option<Retained<NSSharingServicePicker>>> = const { RefCell::new(None) };
    }

    define_class!(
        /// Takes control of the Quick Look panel from the responder chain (it sits after
        /// the key window) and serves it the one file.
        // SAFETY: NSResponder has no subclassing requirements, and the class has no ivars
        // and no Drop.
        #[unsafe(super(NSResponder, NSObject))]
        #[thread_kind = MainThreadOnly]
        #[name = "TeamChatQuickLookController"]
        struct Controller;

        impl Controller {
            #[unsafe(method(acceptsPreviewPanelControl:))]
            fn accepts(&self, _panel: *mut AnyObject) -> bool {
                true
            }

            #[unsafe(method(beginPreviewPanelControl:))]
            fn begin(&self, panel: *mut AnyObject) {
                // SAFETY: AppKit passes the live shared panel; this object outlives it
                // (it is kept in CONTROLLER for the life of the app).
                unsafe {
                    let _: () = msg_send![panel, setDataSource: self];
                    let _: () = msg_send![panel, reloadData];
                }
            }

            #[unsafe(method(endPreviewPanelControl:))]
            fn end(&self, panel: *mut AnyObject) {
                // SAFETY: as in `begin`; nil clears the data source.
                unsafe {
                    let _: () = msg_send![panel, setDataSource: std::ptr::null::<AnyObject>()];
                }
            }

            #[unsafe(method(numberOfPreviewItemsInPreviewPanel:))]
            fn count(&self, _panel: *mut AnyObject) -> NSInteger {
                ITEM.with_borrow(|item| item.is_some() as NSInteger)
            }

            #[unsafe(method(previewPanel:previewItemAtIndex:))]
            fn item(&self, _panel: *mut AnyObject, _index: NSInteger) -> *mut AnyObject {
                // NSURL is a QLPreviewItem. ITEM keeps it alive while the panel shows it.
                ITEM.with_borrow(|item| {
                    item.as_ref()
                        .map(|u| Retained::as_ptr(u) as *mut AnyObject)
                        .unwrap_or(std::ptr::null_mut())
                })
            }
        }

        unsafe impl NSObjectProtocol for Controller {}
    );

    impl Controller {
        fn shared(mtm: MainThreadMarker) -> Retained<Self> {
            CONTROLLER.with_borrow_mut(|c| {
                c.get_or_insert_with(|| {
                    let this = Self::alloc(mtm).set_ivars(());
                    // SAFETY: NSResponder's init on a freshly allocated object.
                    unsafe { msg_send![super(this), init] }
                })
                .clone()
            })
        }
    }

    fn panel_class() -> Option<&'static AnyClass> {
        AnyClass::get(c"QLPreviewPanel")
    }

    /// The shared panel when it was made before (asking for it makes it).
    fn existing_panel() -> Option<Retained<AnyObject>> {
        let class = panel_class()?;
        // SAFETY: class methods of QLPreviewPanel on the main thread.
        unsafe {
            let exists: bool = msg_send![class, sharedPreviewPanelExists];
            if !exists {
                return None;
            }
            msg_send![class, sharedPreviewPanel]
        }
    }

    pub fn toggle_quick_look(path: &Path) {
        let Some(mtm) = MainThreadMarker::new() else {
            return;
        };
        if let Some(panel) = existing_panel() {
            // SAFETY: the live shared panel, on the main thread.
            let visible: bool = unsafe { msg_send![&*panel, isVisible] };
            if visible {
                let _: () = unsafe { msg_send![&*panel, orderOut: std::ptr::null::<AnyObject>()] };
                return;
            }
        }
        let Some(class) = panel_class() else {
            tracing::warn!("Quick Look is not available");
            return;
        };
        let url = NSURL::fileURLWithPath(&NSString::from_str(&path.to_string_lossy()));
        ITEM.with_borrow_mut(|item| *item = Some(url));
        let controller = Controller::shared(mtm);
        // The panel looks for its controller along the key window's responder chain:
        // put the controller right after the window.
        let app = NSApplication::sharedApplication(mtm);
        if let Some(window) = app.keyWindow().or_else(|| app.mainWindow()) {
            // SAFETY: reading and setting responders on the main thread.
            unsafe {
                let next = window.nextResponder();
                let already = next
                    .as_deref()
                    .is_some_and(|n| std::ptr::eq(n, &**controller as &NSResponder));
                if !already {
                    controller.setNextResponder(next.as_deref());
                    window.setNextResponder(Some(&controller));
                }
            }
        }
        // SAFETY: class and instance methods of the shared panel on the main thread.
        unsafe {
            let panel: Option<Retained<AnyObject>> = msg_send![class, sharedPreviewPanel];
            let Some(panel) = panel else { return };
            let _: () = msg_send![&*panel, updateController];
            // Without a controller (no key window), serve the panel directly.
            let source: *mut AnyObject = msg_send![&*panel, dataSource];
            if source.is_null() {
                let _: () = msg_send![&*panel, setDataSource: &*controller];
            }
            let _: () = msg_send![&*panel, reloadData];
            let _: () = msg_send![&*panel, makeKeyAndOrderFront: std::ptr::null::<AnyObject>()];
        }
    }

    /// The window's content view (GPUI's own NSView).
    pub fn content_view(window: &gpui::Window) -> Option<Retained<NSView>> {
        use raw_window_handle::{HasWindowHandle, RawWindowHandle};
        MainThreadMarker::new()?;
        let handle = HasWindowHandle::window_handle(window).ok()?;
        let RawWindowHandle::AppKit(appkit) = handle.as_raw() else {
            return None;
        };
        let ptr = appkit.ns_view.as_ptr() as *mut NSView;
        // SAFETY: GPUI's AppKit handle points at the window's live content NSView; this
        // runs on the main thread, and retaining keeps it valid until the picker shows.
        unsafe { Retained::retain(ptr) }
    }

    pub fn share(view: &NSView, paths: &[PathBuf], x: f64, y: f64) {
        if MainThreadMarker::new().is_none() {
            return;
        }
        let urls: Vec<Retained<NSURL>> = paths
            .iter()
            .map(|p| NSURL::fileURLWithPath(&NSString::from_str(&p.to_string_lossy())))
            .collect();
        if urls.is_empty() {
            return;
        }
        let items: Retained<NSArray<AnyObject>> = NSArray::from_retained_slice(
            &urls
                .into_iter()
                .map(|u| Retained::into_super(Retained::into_super(u)))
                .collect::<Vec<Retained<AnyObject>>>(),
        );
        // SAFETY: an array of file NSURLs, which the picker takes as share items.
        let picker = unsafe {
            NSSharingServicePicker::initWithItems(NSSharingServicePicker::alloc(), &items)
        };
        let bounds = view.bounds();
        let (vx, vy) = super::view_point(x, y, bounds.size.height, view.isFlipped());
        let rect = NSRect::new(NSPoint::new(vx, vy), NSSize::new(1., 1.));
        PICKER.with_borrow_mut(|p| *p = Some(picker.clone()));
        picker.showRelativeToRect_ofView_preferredEdge(rect, view, NSRectEdge::MinY);
    }
}

#[cfg(test)]
mod tests {
    use super::view_point;

    /// Space on a file a teammate posted must never run it or send a page to the browser.
    #[test]
    fn a_preview_never_opens_what_windows_would_run() {
        let p = |n: &str| std::path::PathBuf::from(format!("/cache/abc/{n}"));
        for name in [
            "report.bat",
            "report.BAT",
            "report.bat.",
            "report.bat ",
            "report.exe",
            "a.cmd",
            "a.lnk",
            "a.ps1",
            "a.vbs",
            "a.scr",
            "a.msi",
            "a.hta",
            "a.js",
            "a.url",
            "a.iso",
            "page.html",
            "page.HTM",
            "page.html.",
            "page.svg",
            "noext",
            "a.pdf.exe . ",
        ] {
            assert!(!super::preview_opens(&p(name), true), "{name:?}");
        }
        for name in ["notes.txt", "a.pdf", "photo.PNG", "clip.mp4", "deck.pptx"] {
            assert!(super::preview_opens(&p(name), true), "{name:?}");
        }
        assert!(!super::preview_opens(std::path::Path::new("/"), true));
    }

    #[test]
    fn explorer_selects_a_quoted_path() {
        let path = std::path::Path::new(r"C:\Users\a b\x, y.txt");
        assert_eq!(
            super::explorer_select_arg(path),
            r#"/select,"C:\Users\a b\x, y.txt""#
        );
    }

    #[test]
    fn a_point_from_the_top_left_lands_in_view_coordinates() {
        assert_eq!(view_point(10., 30., 500., true), (10., 30.));
        assert_eq!(view_point(10., 30., 500., false), (10., 470.));
    }
}
