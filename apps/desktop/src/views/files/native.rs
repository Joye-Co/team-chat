//! AppKit behind the file actions ([`super::export`]): the system Quick Look panel
//! (`QLPreviewPanel`) and the share picker (`NSSharingServicePicker`).
//!
//! Both run on a later main-queue turn, outside any GPUI update: the panel's key-window
//! change and the picker's menu loop call back into GPUI, which must not be borrowed then.
//!
//! Windows has no Quick Look panel: Space shows the file in the app's own sheet
//! ([`super::quick_look`]). Only a file that Windows would run, install or mount comes
//! here, to [`reveal`]: it is shown selected in Explorer, so Space on a teammate's file
//! never runs it and never starts another program. 共享… opens the Windows share UI for the window ([`win_share`]).

use std::path::PathBuf;

use gpui::{Pixels, Point, Window};

/// Shows `path` in the Quick Look panel, or closes the panel when it is open (Space
/// toggles it, as in Finder; the panel itself closes on Space and Escape when it is key).
#[cfg(target_os = "macos")]
pub fn toggle_quick_look(path: PathBuf) {
    dispatch2::DispatchQueue::main().exec_async(move || mac::toggle_quick_look(&path));
}

/// Shows `path` selected in an Explorer window: what Space does for a file Windows would
/// run. Nothing here opens a file with a program.
#[cfg(target_os = "windows")]
pub fn reveal(path: &std::path::Path) {
    win::reveal(path);
}

/// Opens the share picker for `paths` at `at` (window points from the top left of the
/// content) in `window`'s content view.
#[cfg_attr(target_os = "windows", allow(dead_code))]
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
    // Windows shows its share UI through [`win_share::share`], which needs the app.
    #[cfg(not(target_os = "macos"))]
    let _ = (paths, at, window);
}

/// The Windows share UI (`DataTransferManager`): the system's picker of nearby devices,
/// contacts and apps that take files, the twin of the macOS share picker.
///
/// A Win32 window has no share contract of its own: `IDataTransferManagerInterop` gives the
/// manager for an HWND and shows the UI for it. The files are opened as storage items on
/// a background thread (an asynchronous WinRT call that must not block the UI thread);
/// then the picker opens, asks for the data once on the UI thread, and gets them.
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

    /// Shows the share UI of the window `hwnd` for `paths`. The files are opened on a
    /// background thread; the UI thread only shows the picker once they are.
    pub fn share(paths: Vec<PathBuf>, hwnd: isize, cx: &mut gpui::App) {
        let Some(title) = paths
            .first()
            .and_then(|p| p.file_name())
            .map(|n| n.to_string_lossy().into_owned())
        else {
            return;
        };
        let opening = cx
            .background_executor()
            .spawn(async move { storage_items(&paths) });
        cx.spawn(async move |_| {
            let items = match opening.await {
                Ok(items) => items,
                Err(error) => {
                    tracing::warn!(%error, "cannot open the files to share");
                    return;
                }
            };
            let hwnd = HWND(hwnd as *mut _);
            let shown = prepare(hwnd, &title, items)
                // SAFETY: as in `prepare`; a window that closed meanwhile is an error.
                .and_then(|interop| unsafe { interop.ShowShareUIForWindow(hwnd) });
            if let Err(error) = shown {
                tracing::warn!(%error, "cannot show the share UI");
            }
        })
        .detach();
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

    /// Shows `path` selected in an Explorer window.
    pub fn reveal(path: &Path) {
        let args = wide(&super::explorer_select_arg(path));
        // SAFETY: both strings are NUL-terminated and outlive the call.
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
