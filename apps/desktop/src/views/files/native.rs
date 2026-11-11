//! AppKit behind the file actions ([`super::export`]): the system Quick Look panel
//! (`QLPreviewPanel`) and the share picker (`NSSharingServicePicker`).
//!
//! Both run on a later main-queue turn, outside any GPUI update: the panel's key-window
//! change and the picker's menu loop call back into GPUI, which must not be borrowed then.
//!
//! Windows has no Quick Look panel: Space shows the file in the app's own viewer
//! ([`super::quick_look`]). Only a file no viewer shows comes here, to
//! [`system_preview`]: "open with the default app", behind the same guard as 打开
//! ([`super::open_kind`]): a file that Windows would run, install or mount, and any page a
//! browser would run, is shown selected in Explorer instead, so Space on a teammate's file
//! never runs it. 共享… opens the Windows share UI for the window ([`win_share`]).

use std::path::PathBuf;

use gpui::{Pixels, Point, Window};

/// Shows `path` in the Quick Look panel, or closes the panel when it is open (Space
/// toggles it, as in Finder; the panel itself closes on Space and Escape when it is key).
#[cfg(target_os = "macos")]
pub fn toggle_quick_look(path: PathBuf) {
    dispatch2::DispatchQueue::main().exec_async(move || mac::toggle_quick_look(&path));
}

/// Hands a file that no viewer of the app shows to Windows: its default app, or Explorer
/// with the file selected when opening it is not safe.
#[cfg(target_os = "windows")]
pub fn system_preview(path: &std::path::Path) {
    if preview_opens(path, true) {
        win::shell_open(path);
    } else {
        win::reveal(path);
    }
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
    // Windows centers its share UI on the window; `at` has no use there.
    #[cfg(target_os = "windows")]
    {
        let _ = at;
        if let Some(hwnd) = crate::win32::hwnd(window) {
            win_share::share(paths, hwnd.0 as isize);
        }
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    let _ = (paths, at, window);
}

/// The Windows share UI (`DataTransferManager`): the system's picker of nearby devices,
/// contacts and apps that take files, the twin of the macOS share picker.
///
/// A Win32 window has no share contract of its own: `IDataTransferManagerInterop` gives the
/// manager for an HWND and shows the UI for it. The files are opened as storage items on
/// a background thread first (an asynchronous WinRT call that must not block the UI
/// thread); the UI then asks for the data once, on the UI thread, and gets them.
#[cfg(target_os = "windows")]
pub mod win_share {
    use std::{cell::RefCell, path::PathBuf};

    use windows::{
        ApplicationModel::DataTransfer::{DataRequestedEventArgs, DataTransferManager},
        Foundation::TypedEventHandler,
        Storage::{IStorageItem, StorageFile, StorageFolder},
        Win32::{Foundation::HWND, UI::Shell::IDataTransferManagerInterop},
        core::{AgileReference, HSTRING, Interface as _},
    };
    use windows_collections::IIterable;

    thread_local! {
        /// The handler of the share that is showing, with its manager, so the next share
        /// replaces it instead of adding a second one.
        static CURRENT: RefCell<Option<(DataTransferManager, i64)>> = const { RefCell::new(None) };
    }

    /// A file or a folder the share UI can take, as a reference that may cross from the
    /// thread that opened it to the UI thread.
    pub struct Item(AgileReference<IStorageItem>);

    impl Item {
        pub fn storage_item(&self) -> windows::core::Result<IStorageItem> {
            self.0.resolve()
        }
    }

    /// The files and folders as storage items. Blocks: call off the UI thread.
    pub fn storage_items(paths: &[PathBuf]) -> windows::core::Result<Vec<Item>> {
        paths
            .iter()
            .map(|path| {
                let name = HSTRING::from(path.as_os_str());
                let item: IStorageItem = if path.is_dir() {
                    StorageFolder::GetFolderFromPathAsync(&name)?
                        .join()?
                        .cast()?
                } else {
                    StorageFile::GetFileFromPathAsync(&name)?.join()?.cast()?
                };
                AgileReference::new(&item).map(Item)
            })
            .collect()
    }

    /// The share manager of `hwnd`, with `items` as what it shares under `title`.
    pub fn prepare(
        hwnd: HWND,
        title: &str,
        items: Vec<Item>,
    ) -> windows::core::Result<IDataTransferManagerInterop> {
        let interop: IDataTransferManagerInterop =
            windows::core::factory::<DataTransferManager, IDataTransferManagerInterop>()?;
        // SAFETY: `hwnd` is a live top-level window of this thread.
        let manager: DataTransferManager = unsafe { interop.GetForWindow(hwnd)? };
        if let Some((old, token)) = CURRENT.with_borrow_mut(Option::take) {
            let _ = old.RemoveDataRequested(token);
        }
        let title = HSTRING::from(title);
        let token = manager.DataRequested(&TypedEventHandler::<
            DataTransferManager,
            DataRequestedEventArgs,
        >::new(move |_, args| {
            let Some(args) = args.as_ref() else {
                return Ok(());
            };
            let data = args.Request()?.Data()?;
            data.Properties()?.SetTitle(&title)?;
            let items: IIterable<IStorageItem> = items
                .iter()
                .map(|item| item.storage_item().map(Some))
                .collect::<windows::core::Result<Vec<_>>>()?
                .into();
            data.SetStorageItemsReadOnly(&items)
        }))?;
        CURRENT.with_borrow_mut(|current| *current = Some((manager, token)));
        Ok(interop)
    }

    /// Shows the share UI of the window `hwnd` for `paths`. Call on the UI thread.
    pub fn share(paths: Vec<PathBuf>, hwnd: isize) {
        let Some(title) = paths
            .first()
            .and_then(|p| p.file_name())
            .map(|n| n.to_string_lossy().into_owned())
        else {
            return;
        };
        let (tx, rx) = std::sync::mpsc::channel();
        let opened = std::thread::Builder::new()
            .name("share-items".into())
            .spawn(move || {
                let _ = tx.send(storage_items(&paths));
            });
        if opened.is_err() {
            return;
        }
        // Opening a local file takes milliseconds; the picker must open from this turn of
        // the UI thread (it belongs to the click).
        let items = match rx.recv_timeout(std::time::Duration::from_secs(5)) {
            Ok(Ok(items)) => items,
            Ok(Err(error)) => {
                tracing::warn!(%error, "cannot open the files to share");
                return;
            }
            Err(_) => {
                tracing::warn!("opening the files to share took too long");
                return;
            }
        };
        let hwnd = HWND(hwnd as *mut _);
        let shown = prepare(hwnd, &title, items)
            // SAFETY: as in `prepare`.
            .and_then(|interop| unsafe { interop.ShowShareUIForWindow(hwnd) });
        if let Err(error) = shown {
            tracing::warn!(%error, "cannot show the share UI");
        }
    }
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

    /// The share UI gets real storage items for a file and for a page's folder (names
    /// with spaces and Chinese), and the window's share manager takes the request handler.
    /// The picker itself is not opened: nobody is there to close it.
    #[cfg(target_os = "windows")]
    #[test]
    fn the_share_ui_gets_the_files_as_storage_items() {
        use windows::{
            Win32::UI::WindowsAndMessaging::{
                CreateWindowExW, DestroyWindow, WINDOW_EX_STYLE, WS_OVERLAPPEDWINDOW,
            },
            core::w,
        };
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("季度 报告 v2.pdf");
        std::fs::write(&file, b"%PDF-1.4").unwrap();
        let folder = dir.path().join("Q3 复盘");
        std::fs::create_dir(&folder).unwrap();
        std::fs::write(folder.join("index.html"), b"<p>").unwrap();
        let paths = vec![file, folder];
        // Off the test's own thread, as the app does it.
        let items = std::thread::spawn({
            let paths = paths.clone();
            move || super::win_share::storage_items(&paths)
        })
        .join()
        .unwrap()
        .expect("storage items");
        assert_eq!(items.len(), 2);
        let name = |i: usize| items[i].storage_item().unwrap().Name().unwrap().to_string();
        assert_eq!(name(0), "季度 报告 v2.pdf");
        assert_eq!(name(1), "Q3 复盘");
        assert!(
            super::win_share::storage_items(&[dir.path().join("gone.txt")]).is_err(),
            "a missing file is an error, not an empty share"
        );
        // SAFETY: a plain top-level window of this thread, destroyed below.
        unsafe {
            let hwnd = CreateWindowExW(
                WINDOW_EX_STYLE(0),
                w!("STATIC"),
                w!("share test"),
                WS_OVERLAPPEDWINDOW,
                0,
                0,
                200,
                100,
                None,
                None,
                None,
                None,
            )
            .expect("a window");
            let again_items = super::win_share::storage_items(&paths).expect("storage items");
            let prepared = super::win_share::prepare(hwnd, "季度 报告 v2.pdf", items);
            println!(
                "share manager for a window: {:?}",
                prepared.as_ref().map(|_| ())
            );
            // A second share replaces the first one's handler.
            let again = super::win_share::prepare(hwnd, "Q3 复盘", again_items);
            assert_eq!(prepared.is_ok(), again.is_ok());
            // Windows Server without the share contract has no manager; a desktop has.
            if let Err(error) = &prepared {
                println!("no share manager in this session: {error}");
            }
            let _ = DestroyWindow(hwnd);
        }
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
