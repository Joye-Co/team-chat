//! A 产出 page inside a host window: a clipping container with an isolated WKWebView,
//! added as a child of the host's content view, above the host's own view.
//!
//! The page gets the same isolation as the 产出 window (per-version origin, scheme handler,
//! strict CSP, the network-blocking rule list, the navigation policy and the bridge
//! capabilities): both use the builders in `mac.rs`.

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};

use block2::RcBlock;
use dispatch2::DispatchQueue;
use objc2::rc::{Retained, Weak};
use objc2::runtime::{AnyClass, AnyObject};
use objc2::{ClassType, DefinedClass, MainThreadOnly, define_class, msg_send, sel};
use objc2_app_kit::{
    NSAppearance, NSAppearanceCustomization, NSAppearanceNameAqua, NSAppearanceNameDarkAqua,
    NSApplication, NSEvent, NSEventModifierFlags, NSEventType, NSImage, NSResponder, NSView,
    NSWindow, NSWindowOrderingMode,
};
use objc2_foundation::{
    MainThreadMarker, NSData, NSDictionary, NSError, NSObject, NSPoint, NSProcessInfo, NSRect,
    NSSize, NSString,
};
use objc2_web_kit::{WKWebView, WKWebViewConfiguration};

use crate::embed::{EmbedOptions, Placement, Rect, place};
use crate::mac::{self, Controller, State};
use crate::{ArtifactEvent, Capabilities, Error, VersionSpec};

/// Wrapper that lets main-thread values ride a block onto the main queue.
struct MainOnly<T>(T);
// SAFETY: only ever sent to `DispatchQueue::main()`, so the value is created and used on the
// main thread.
unsafe impl<T> Send for MainOnly<T> {}

fn emit_async(cb: &Rc<dyn Fn(ArtifactEvent)>, event: ArtifactEvent) {
    let job = MainOnly((cb.clone(), event));
    DispatchQueue::main().exec_async(move || {
        let job = job;
        let (cb, event) = job.0;
        cb(event);
    });
}

define_class!(
    /// The clipping container: flipped, so the web view frame inside it uses the host's
    /// top-left coordinates.
    // SAFETY: NSView has no subclassing requirements and ClipView does not implement Drop.
    #[unsafe(super(NSView, NSResponder, NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "TeamChatArtifactEmbedClip"]
    struct ClipView;

    impl ClipView {
        #[unsafe(method(isFlipped))]
        fn is_flipped(&self) -> bool {
            true
        }
    }
);

struct WebIvars {
    cb: Rc<dyn Fn(ArtifactEvent)>,
    pass_scroll: Cell<bool>,
    host: Weak<NSView>,
}

define_class!(
    /// The page view. It reports keyboard focus changes, and with `pass_scroll` it gives
    /// wheel and trackpad scrolling to the host view.
    // SAFETY: WKWebView may be subclassed; the overrides call the superclass and the class
    // does not implement Drop.
    #[unsafe(super(WKWebView, NSView, NSResponder, NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "TeamChatArtifactEmbedWebView"]
    #[ivars = WebIvars]
    struct EmbedWebView;

    impl EmbedWebView {
        #[unsafe(method(becomeFirstResponder))]
        fn become_first_responder(&self) -> bool {
            // SAFETY: NSResponder's method with this signature.
            let ok: bool = unsafe { msg_send![super(self), becomeFirstResponder] };
            if ok {
                emit_async(&self.ivars().cb, ArtifactEvent::FocusChanged { focused: true });
            }
            ok
        }

        #[unsafe(method(resignFirstResponder))]
        fn resign_first_responder(&self) -> bool {
            // SAFETY: NSResponder's method with this signature.
            let ok: bool = unsafe { msg_send![super(self), resignFirstResponder] };
            if ok {
                emit_async(&self.ivars().cb, ArtifactEvent::FocusChanged { focused: false });
            }
            ok
        }

        #[unsafe(method(acceptsFirstMouse:))]
        fn accepts_first_mouse(&self, event: Option<&NSEvent>) -> bool {
            FIRST_MOUSE_FOR_SELF_TESTS.load(Ordering::Relaxed) || {
                // SAFETY: NSView's method with this signature.
                let ok: bool = unsafe { msg_send![super(self), acceptsFirstMouse: event] };
                ok
            }
        }

        #[unsafe(method(scrollWheel:))]
        fn scroll_wheel(&self, event: &NSEvent) {
            if self.ivars().pass_scroll.get()
                && let Some(host) = self.ivars().host.load()
            {
                host.scrollWheel(event);
                return;
            }
            // SAFETY: NSResponder's method with this signature.
            unsafe { msg_send![super(self), scrollWheel: event] }
        }
    }
);

/// Set by [`EmbeddedArtifact::post_click`]: see there.
static FIRST_MOUSE_FOR_SELF_TESTS: AtomicBool = AtomicBool::new(false);

/// One 产出 page embedded in a host window. Main thread only. Dropping it removes the view
/// and stops the page (also its media).
pub struct EmbeddedArtifact {
    container: Retained<ClipView>,
    web_view: Retained<EmbedWebView>,
    controller: Retained<Controller>,
    state: Rc<RefCell<State>>,
    host: Weak<NSView>,
    placement: Cell<Option<Placement>>,
    /// The host asked to hide the view (a GPUI overlay covers it).
    hidden_by_host: Cell<bool>,
    dark: Cell<Option<bool>>,
}

impl EmbeddedArtifact {
    /// Adds the page above `host_view`, as a child of its superview (the window's content
    /// view). The view stays hidden until the first [`set_frame`](Self::set_frame).
    /// `on_event` runs on the main thread, never inside another call of this type.
    ///
    /// `host_view` must be in a window and have a superview.
    pub fn attach(
        mtm: MainThreadMarker,
        host_view: &NSView,
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
        } = options;
        // SAFETY: a plain getter on a live view.
        let parent = unsafe { host_view.superview() }.ok_or(Error::NoHostView)?;
        let state = mac::new_state(artifact_id, version, capabilities)?;
        let cb: Rc<dyn Fn(ArtifactEvent)> = Rc::new(on_event);
        let controller = Controller::new(mtm, state.clone(), cb.clone());
        let config = mac::configure(mtm, &controller, autoplay);

        let zero = NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(1.0, 1.0));
        let container: Retained<ClipView> = {
            let this = ClipView::alloc(mtm).set_ivars(());
            // SAFETY: NSView's designated initializer.
            unsafe { msg_send![super(this), initWithFrame: zero] }
        };
        container.setHidden(true);
        container.setWantsLayer(true);
        // SAFETY: `clipsToBounds` (macOS 14) is a BOOL property; older systems clip
        // subviews by default. `layer` is the container's own backing layer.
        unsafe {
            let responds: bool =
                msg_send![&*container, respondsToSelector: sel!(setClipsToBounds:)];
            if responds {
                let _: () = msg_send![&*container, setClipsToBounds: true];
            }
            let layer: *mut AnyObject = msg_send![&*container, layer];
            if !layer.is_null() {
                let _: () = msg_send![layer, setMasksToBounds: true];
                let _: () = msg_send![layer, setCornerRadius: corner_radius.max(0.0)];
            }
        }

        let web_view: Retained<EmbedWebView> = {
            let this = EmbedWebView::alloc(mtm).set_ivars(WebIvars {
                cb: cb.clone(),
                pass_scroll: Cell::new(pass_scroll),
                host: Weak::new(host_view),
            });
            let cfg: &WKWebViewConfiguration = &config.config;
            // SAFETY: WKWebView's designated initializer.
            unsafe { msg_send![super(this), initWithFrame: zero, configuration: cfg] }
        };
        mac::wire(&web_view, &controller, inspectable);
        // SAFETY: `_setDrawsBackground:` takes one BOOL; it is checked first. A page with a
        // transparent body then shows the host's surface instead of white.
        unsafe {
            let responds: bool =
                msg_send![&*web_view, respondsToSelector: sel!(_setDrawsBackground:)];
            if responds {
                let _: () = msg_send![&*web_view, _setDrawsBackground: false];
            }
        }
        container.addSubview(&web_view);
        parent.addSubview_positioned_relativeTo(
            &container,
            NSWindowOrderingMode::Above,
            Some(host_view),
        );

        let wk: Retained<WKWebView> = Retained::into_super(web_view.clone());
        mac::start_loading(mtm, &controller, &wk, &config.ucc);

        let this = Self {
            container,
            web_view,
            controller,
            state,
            host: Weak::new(host_view),
            placement: Cell::new(None),
            hidden_by_host: Cell::new(false),
            dark: Cell::new(None),
        };
        this.set_dark(dark);
        Ok(this)
    }

    /// Places the page from the host element's bounds (`full`) and its unclipped part
    /// (`visible`), both in points from the host view's top-left corner. `scale` is the
    /// window's backing scale. An empty visible part hides the view.
    pub fn set_frame(&self, full: Rect, visible: Rect, scale: f64) {
        if self.is_closed() {
            return;
        }
        // SAFETY: a plain getter on a live view.
        let Some(parent) = (unsafe { self.container.superview() }) else {
            return;
        };
        let bounds = parent.bounds();
        let placement = place(full, visible, bounds.size.height, parent.isFlipped(), scale);
        if placement != self.placement.get() {
            if let Some(p) = placement {
                self.container.setFrame(ns_rect(p.container));
                self.web_view.setFrame(ns_rect(p.web_view));
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
        !self.container.isHidden()
    }

    fn apply_hidden(&self) {
        let hide = self.is_closed() || self.hidden_by_host.get() || self.placement.get().is_none();
        if hide && self.is_focused() {
            self.resign_focus();
        }
        if self.container.isHidden() != hide {
            self.container.setHidden(hide);
        }
    }

    /// Draws the page with the dark or light appearance (`prefers-color-scheme`).
    pub fn set_dark(&self, dark: bool) {
        if self.dark.get() == Some(dark) {
            return;
        }
        self.dark.set(Some(dark));
        // SAFETY: AppKit's appearance name constants.
        let name = unsafe {
            if dark {
                NSAppearanceNameDarkAqua
            } else {
                NSAppearanceNameAqua
            }
        };
        let appearance = NSAppearance::appearanceNamed(name);
        self.web_view.setAppearance(appearance.as_deref());
    }

    /// Gives wheel scrolling to the host view (true) or to the page (false).
    pub fn set_pass_scroll(&self, pass: bool) {
        self.web_view.ivars().pass_scroll.set(pass);
    }

    /// Moves keyboard focus into the page.
    pub fn focus(&self) {
        if let Some(window) = self.container.window()
            && self.is_shown()
        {
            window.makeFirstResponder(Some(&self.web_view));
        }
    }

    /// True while the page has keyboard focus.
    pub fn is_focused(&self) -> bool {
        let Some(window) = self.container.window() else {
            return false;
        };
        let Some(first) = window.firstResponder() else {
            return false;
        };
        let first: *const NSResponder = &*first;
        let mine: *const NSResponder = (&**self.web_view as &NSView as &NSResponder) as *const _;
        // A text field inside the page can be a descendant view of the web view.
        if std::ptr::eq(first, mine) {
            return true;
        }
        // SAFETY: `isKindOfClass:` and `isDescendantOf:` are plain queries.
        unsafe {
            let first_obj: &AnyObject = &*(first as *const AnyObject);
            let is_view: bool = msg_send![first_obj, isKindOfClass: NSView::class()];
            is_view && {
                let v: bool = msg_send![first_obj, isDescendantOf: &**self.web_view as &NSView];
                v
            }
        }
    }

    /// Gives keyboard focus back to the host view when the page has it.
    pub fn resign_focus(&self) {
        if !self.is_focused() {
            return;
        }
        if let (Some(window), Some(host)) = (self.container.window(), self.host.load()) {
            window.makeFirstResponder(Some(&host));
        }
    }

    /// Takes a picture of the page as PNG bytes (for the host to draw while the view is
    /// hidden under an overlay). `done` runs once on the main thread, with `None` on failure.
    pub fn snapshot_png(&self, done: impl FnOnce(Option<Vec<u8>>) + 'static) {
        if self.is_closed() || self.placement.get().is_none() {
            done(None);
            return;
        }
        let done = RefCell::new(Some(done));
        let block = RcBlock::new(move |image: *mut NSImage, _err: *mut NSError| {
            let Some(done) = done.borrow_mut().take() else {
                return;
            };
            // SAFETY: WebKit passes a valid (or null) image.
            let image = unsafe { Retained::retain(image) };
            done(image.and_then(|i| png_bytes(&i)));
        });
        unsafe {
            self.web_view
                .takeSnapshotWithConfiguration_completionHandler(None, &block)
        };
    }

    /// Shows another version (new origin, content and data) in the same view. `caps` are
    /// that version's bridge capabilities.
    pub fn show_version(&self, spec: VersionSpec, caps: Capabilities) -> Result<(), Error> {
        mac::switch_version(&self.state, spec, caps)?;
        self.controller.load_current(&self.web_view);
        Ok(())
    }

    pub fn version(&self) -> u32 {
        self.state.borrow().version
    }

    /// The bridge capabilities of the page that shows now.
    pub fn capabilities(&self) -> Capabilities {
        self.state.borrow().caps
    }

    /// Changes the bridge capabilities of the page that shows now.
    pub fn set_capabilities(&self, caps: Capabilities) {
        mac::set_capabilities(&self.state, caps);
    }

    pub fn is_closed(&self) -> bool {
        self.state.borrow().closed
    }

    /// Runs JavaScript in the page (for self-tests and developer tools).
    pub fn evaluate_script(&self, script: &str) {
        if !self.is_closed() {
            unsafe {
                self.web_view
                    .evaluateJavaScript_completionHandler(&NSString::from_str(script), None)
            };
        }
    }

    /// Runs JavaScript in the page and gives its value when it is a string (`None` when it
    /// fails or is something else). `done` runs once on the main thread. For self-tests.
    pub fn evaluate_text(&self, script: &str, done: impl FnOnce(Option<String>) + 'static) {
        if self.is_closed() {
            done(None);
            return;
        }
        let done = RefCell::new(Some(done));
        let block = RcBlock::new(move |value: *mut AnyObject, _err: *mut NSError| {
            let Some(done) = done.borrow_mut().take() else {
                return;
            };
            // SAFETY: WebKit passes a valid (or null) object for the call.
            let text = unsafe { value.as_ref() }
                .and_then(|v| v.downcast_ref::<NSString>())
                .map(|s| s.to_string());
            done(text);
        });
        unsafe {
            self.web_view
                .evaluateJavaScript_completionHandler(&NSString::from_str(script), Some(&block))
        };
    }

    /// Posts a left click at `(x, y)`, in points from the page's top-left corner, to the
    /// app's event queue: a mouse-down and a mouse-up that AppKit dispatches like the
    /// viewer's own click (event monitors, hit testing, WebKit). False when the page is not
    /// in a window. For self-tests.
    ///
    /// The self-test never activates the app (it must not take the viewer's focus), and
    /// AppKit gives a click in an inactive app's window to a view only when the view takes
    /// the first mouse. WebKit does not for a button, so from the first call on, every
    /// embedded page in this process takes it, as when the app is active.
    pub fn post_click(&self, x: f64, y: f64) -> bool {
        if self.is_closed() {
            return false;
        }
        FIRST_MOUSE_FOR_SELF_TESTS.store(true, Ordering::Relaxed);
        let Some(window) = self.web_view.window() else {
            return false;
        };
        let local = if self.web_view.isFlipped() {
            NSPoint::new(x, y)
        } else {
            NSPoint::new(x, self.web_view.bounds().size.height - y)
        };
        let at = self.web_view.convertPoint_toView(local, None);
        post_click_at(&window, at)
    }

    /// Posts a left click at `(x, y)`, in points from the top-left corner of the window's
    /// content, like [`post_click`](Self::post_click). For self-tests of input outside the
    /// page.
    pub fn post_window_click(&self, x: f64, y: f64) -> bool {
        let Some(window) = self.container.window() else {
            return false;
        };
        let Some(content) = window.contentView() else {
            return false;
        };
        let height = content.bounds().size.height;
        let local = if content.isFlipped() {
            NSPoint::new(x, y)
        } else {
            NSPoint::new(x, height - y)
        };
        let at = content.convertPoint_toView(local, None);
        post_click_at(&window, at)
    }

    /// Removes the view and stops the page. A `Closed` event follows once. Safe to call
    /// more than once.
    pub fn close(&self) {
        if self.is_closed() {
            return;
        }
        self.resign_focus();
        self.controller.teardown();
        self.container.setHidden(true);
        self.container.removeFromSuperview();
    }
}

impl Drop for EmbeddedArtifact {
    fn drop(&mut self) {
        self.close();
    }
}

/// Posts a left mouse-down and mouse-up at `at` (window coordinates) to the end of the
/// app's event queue.
fn post_click_at(window: &NSWindow, at: NSPoint) -> bool {
    let mtm = MainThreadMarker::from(window);
    let app = NSApplication::sharedApplication(mtm);
    let time = NSProcessInfo::processInfo().systemUptime();
    let number = window.windowNumber();
    for (i, (kind, pressure)) in [
        (NSEventType::LeftMouseDown, 1.0),
        (NSEventType::LeftMouseUp, 0.0),
    ]
    .into_iter()
    .enumerate()
    {
        let Some(event) =
            NSEvent::mouseEventWithType_location_modifierFlags_timestamp_windowNumber_context_eventNumber_clickCount_pressure(
                kind,
                at,
                NSEventModifierFlags::empty(),
                time + i as f64 * 0.05,
                number,
                None,
                0,
                1,
                pressure,
            )
        else {
            return false;
        };
        app.postEvent_atStart(&event, false);
    }
    true
}

fn ns_rect(r: Rect) -> NSRect {
    NSRect::new(NSPoint::new(r.x, r.y), NSSize::new(r.w, r.h))
}

/// PNG bytes of an image (its best representation, 2x on a Retina screen).
fn png_bytes(image: &NSImage) -> Option<Vec<u8>> {
    // SAFETY: `TIFFRepresentation`, `imageRepWithData:` and
    // `representationUsingType:properties:` are plain conversions; 4 is
    // NSBitmapImageFileTypePNG.
    unsafe {
        let tiff: Option<Retained<NSData>> = msg_send![image, TIFFRepresentation];
        let tiff = tiff?;
        let class = AnyClass::get(c"NSBitmapImageRep")?;
        let rep: Option<Retained<AnyObject>> = msg_send![class, imageRepWithData: &*tiff];
        let rep = rep?;
        let props = NSDictionary::<NSString, AnyObject>::new();
        let png: Option<Retained<NSData>> =
            msg_send![&*rep, representationUsingType: 4usize, properties: &*props];
        png.map(|d| d.to_vec())
    }
}

/// The NSView behind a raw AppKit window handle pointer (GPUI's own view).
///
/// # Safety
/// `ptr` must point at a live NSView, and the call must run on the main thread.
pub unsafe fn view_from_ptr<'a>(ptr: *mut std::ffi::c_void) -> Option<&'a NSView> {
    unsafe { ptr.cast::<NSView>().as_ref() }
}
