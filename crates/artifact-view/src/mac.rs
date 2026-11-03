//! AppKit and WebKit side: the window, the scheme handler, the bridge and the delegates.

use std::cell::{Cell, RefCell};
use std::ptr::NonNull;
use std::rc::Rc;
use std::time::{Duration, Instant};

use block2::{DynBlock, RcBlock};
use dispatch2::DispatchQueue;
use objc2::rc::{Retained, Weak};
use objc2::runtime::{AnyObject, ProtocolObject, Sel};
use objc2::{AllocAnyThread, DefinedClass, MainThreadOnly, define_class, msg_send, sel};
use objc2_app_kit::{
    NSBackingStoreType, NSEvent, NSEventMask, NSEventType, NSLayoutAttribute, NSPopUpButton,
    NSTitlebarAccessoryViewController, NSView, NSWindow, NSWindowDelegate, NSWindowStyleMask,
    NSWindowTabbingMode, NSWorkspace,
};
use objc2_foundation::{
    MainThreadMarker, NSData, NSDictionary, NSError, NSHTTPURLResponse, NSJSONSerialization,
    NSJSONWritingOptions, NSNotification, NSObject, NSObjectProtocol, NSPoint, NSRect, NSSize,
    NSString, NSURL, NSURLRequest,
};
use objc2_web_kit::{
    WKAudiovisualMediaTypes, WKContentRuleList, WKContentRuleListStore, WKNavigation,
    WKNavigationAction, WKNavigationActionPolicy, WKNavigationDelegate, WKNavigationType,
    WKScriptMessage, WKScriptMessageHandler, WKUIDelegate, WKURLSchemeHandler, WKURLSchemeTask,
    WKUserContentController, WKUserScript, WKUserScriptInjectionTime, WKWebView,
    WKWebViewConfiguration, WKWebsiteDataStore, WKWindowFeatures,
};
use serde_json::Value;

use crate::resolve::{self, Content, SCHEME};
use crate::{
    ArtifactEvent, Capabilities, Error, OpenOptions, Presentation, RefMenu, Reject, Snapshot,
    VersionItem, VersionSpec, Viewer,
    bridge::{self, Bridge, Request},
    gate,
};

const RULE_LIST_ID: &str = "artifact-view-block-network-v1";
/// Removes the WebRTC constructors in every frame (kept with the private
/// preference below, which WebKit may drop in a later release).
const NO_NETWORK_SCRIPT: &str = "(()=>{for(const k of ['RTCPeerConnection','webkitRTCPeerConnection','RTCDataChannel','RTCIceCandidate','RTCSessionDescription','RTCRtpSender','RTCRtpReceiver','RTCRtpTransceiver']){try{delete window[k]}catch(e){}try{Object.defineProperty(window,k,{value:undefined,configurable:false,writable:false})}catch(e){}}})();";

/// Turns a private `WKPreferences` switch off when this WebKit has it. Use
/// it inside an `unsafe` block: the setter must take one BOOL.
macro_rules! disable_private_pref {
    ($prefs:expr, $setter:ident) => {{
        let prefs: &objc2_web_kit::WKPreferences = $prefs;
        let responds: bool = msg_send![prefs, respondsToSelector: sel!($setter:)];
        if responds {
            let _: () = msg_send![prefs, $setter: false];
        }
    }};
}

const EXTERNAL_LINK_INTERVAL: Duration = Duration::from_millis(800);

pub(crate) struct State {
    pub(crate) artifact_id: String,
    pub(crate) host: String,
    pub(crate) version: u32,
    pub(crate) content: Content,
    pub(crate) entry: String,
    /// The page's bridge: capabilities, declarations, data and open requests.
    pub(crate) bridge: Bridge,
    pub(crate) viewer: Option<Viewer>,
    pub(crate) ref_menu: Option<RefMenu>,
    pub(crate) max_message_bytes: usize,
    pub(crate) closed: bool,
    pub(crate) rules_ready: bool,
    pub(crate) last_external: Option<Instant>,
    /// The viewer's last click, drag or key press in this web view (see [`gate`]).
    pub(crate) last_input: Option<Instant>,
}

pub(crate) struct Ivars {
    state: Rc<RefCell<State>>,
    cb: Rc<dyn Fn(ArtifactEvent)>,
    webview: RefCell<Option<Weak<WKWebView>>>,
    window: RefCell<Option<Weak<NSWindow>>>,
    ucc: RefCell<Option<Weak<WKUserContentController>>>,
    reloads: Cell<u32>,
    /// The local event monitor that records the viewer's input in the web view.
    monitor: RefCell<Option<Retained<AnyObject>>>,
}

/// Wrapper that lets main-thread values ride a block onto the main queue.
struct MainOnly<T>(T);
// SAFETY: only ever sent to `DispatchQueue::main()`, so the value is created and used on the
// main thread.
unsafe impl<T> Send for MainOnly<T> {}

define_class!(
    // SAFETY: NSObject has no subclassing requirements and Controller does not implement Drop.
    #[unsafe(super = NSObject)]
    #[thread_kind = MainThreadOnly]
    #[name = "TeamChatArtifactViewController"]
    #[ivars = Ivars]
    pub(crate) struct Controller;

    unsafe impl NSObjectProtocol for Controller {}

    unsafe impl WKURLSchemeHandler for Controller {
        #[unsafe(method(webView:startURLSchemeTask:))]
        fn start_task(&self, _web_view: &WKWebView, task: &ProtocolObject<dyn WKURLSchemeTask>) {
            self.serve_task(task);
        }

        #[unsafe(method(webView:stopURLSchemeTask:))]
        fn stop_task(&self, _web_view: &WKWebView, _task: &ProtocolObject<dyn WKURLSchemeTask>) {
            // Tasks finish synchronously in start_task, so there is nothing to cancel.
        }
    }

    unsafe impl WKScriptMessageHandler for Controller {
        #[unsafe(method(userContentController:didReceiveScriptMessage:))]
        fn did_receive(&self, _ucc: &WKUserContentController, message: &WKScriptMessage) {
            self.on_message(message);
        }
    }

    unsafe impl WKNavigationDelegate for Controller {
        #[unsafe(method(webView:decidePolicyForNavigationAction:decisionHandler:))]
        fn decide_policy(
            &self,
            _web_view: &WKWebView,
            action: &WKNavigationAction,
            handler: &DynBlock<dyn Fn(WKNavigationActionPolicy)>,
        ) {
            let policy = self.navigation_policy(action);
            handler.call((policy,));
        }

        #[unsafe(method(webView:didFinishNavigation:))]
        fn did_finish(&self, _web_view: &WKWebView, _navigation: Option<&WKNavigation>) {
            let version = self.ivars().state.borrow().version;
            self.ivars().reloads.set(0);
            self.emit(ArtifactEvent::Loaded { version });
        }

        #[unsafe(method(webView:didFailProvisionalNavigation:withError:))]
        fn did_fail_provisional(&self, _web_view: &WKWebView, _navigation: Option<&WKNavigation>, error: &NSError) {
            self.load_failed(error);
        }

        #[unsafe(method(webView:didFailNavigation:withError:))]
        fn did_fail(&self, _web_view: &WKWebView, _navigation: Option<&WKNavigation>, error: &NSError) {
            self.load_failed(error);
        }

        #[unsafe(method(webViewWebContentProcessDidTerminate:))]
        fn process_terminated(&self, web_view: &WKWebView) {
            let version = self.ivars().state.borrow().version;
            self.emit(ArtifactEvent::LoadFailed { version, reason: "web content process ended".into() });
            let n = self.ivars().reloads.get();
            if n < 1 {
                self.ivars().reloads.set(n + 1);
                self.load_current(web_view);
            }
        }
    }

    unsafe impl WKUIDelegate for Controller {
        #[unsafe(method_id(webView:createWebViewWithConfiguration:forNavigationAction:windowFeatures:))]
        fn create_web_view(
            &self,
            _web_view: &WKWebView,
            _configuration: &WKWebViewConfiguration,
            _action: &WKNavigationAction,
            _features: &WKWindowFeatures,
        ) -> Option<Retained<WKWebView>> {
            // No popups. External links were already reported by the navigation policy.
            None
        }
    }

    unsafe impl NSWindowDelegate for Controller {
        #[unsafe(method(windowWillClose:))]
        fn window_will_close(&self, _notification: &NSNotification) {
            self.teardown();
        }
    }

    impl Controller {
        #[unsafe(method(versionPicked:))]
        fn version_picked(&self, sender: &NSPopUpButton) {
            let tag = sender.selectedTag();
            if let Ok(version) = u32::try_from(tag) {
                self.emit(ArtifactEvent::VersionSelected { version });
            }
        }
    }
);

impl Controller {
    pub(crate) fn new(
        mtm: MainThreadMarker,
        state: Rc<RefCell<State>>,
        cb: Rc<dyn Fn(ArtifactEvent)>,
    ) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(Ivars {
            state,
            cb,
            webview: RefCell::new(None),
            window: RefCell::new(None),
            ucc: RefCell::new(None),
            reloads: Cell::new(0),
            monitor: RefCell::new(None),
        });
        // SAFETY: NSObject's init has this signature.
        unsafe { msg_send![super(this), init] }
    }

    /// Delivers an event on a later main-queue turn so host code never runs inside a WebKit
    /// or AppKit delegate call.
    pub(crate) fn emit(&self, event: ArtifactEvent) {
        let job = MainOnly((self.ivars().cb.clone(), event));
        DispatchQueue::main().exec_async(move || {
            let job = job;
            let (cb, event) = job.0;
            cb(event);
        });
    }

    fn load_failed(&self, error: &NSError) {
        let version = self.ivars().state.borrow().version;
        // NSURLErrorCancelled: our own policy cancelled the navigation.
        if error.code() == -999 {
            return;
        }
        let reason = format!("{} {}", error.domain(), error.code());
        self.emit(ArtifactEvent::LoadFailed { version, reason });
    }

    fn serve_task(&self, task: &ProtocolObject<dyn WKURLSchemeTask>) {
        let request = unsafe { task.request() };
        let Some(url) = request.URL() else {
            let err = NSError::new(-1000, &NSString::from_str("NSURLErrorDomain"));
            unsafe { task.didFailWithError(&err) };
            return;
        };
        let url_string = url
            .absoluteString()
            .map(|s| s.to_string())
            .unwrap_or_default();
        let method = request
            .HTTPMethod()
            .map(|s| s.to_string())
            .unwrap_or_else(|| "GET".into());
        let range = request
            .valueForHTTPHeaderField(&NSString::from_str("Range"))
            .map(|s| s.to_string());
        let response = {
            let st = self.ivars().state.borrow();
            if st.closed {
                resolve::serve(&Content::memory([]), &st.host, &st.entry, &method, "")
            } else {
                resolve::serve_range(
                    &st.content,
                    &st.host,
                    &st.entry,
                    &method,
                    &url_string,
                    range.as_deref(),
                )
            }
        };
        let keys: Vec<Retained<NSString>> = response
            .headers
            .iter()
            .map(|(k, _)| NSString::from_str(k))
            .collect();
        let vals: Vec<Retained<NSString>> = response
            .headers
            .iter()
            .map(|(_, v)| NSString::from_str(v))
            .collect();
        let key_refs: Vec<&NSString> = keys.iter().map(|k| &**k).collect();
        let val_refs: Vec<&NSString> = vals.iter().map(|v| &**v).collect();
        let headers = NSDictionary::from_slices(&key_refs, &val_refs);
        let Some(http) = NSHTTPURLResponse::initWithURL_statusCode_HTTPVersion_headerFields(
            NSHTTPURLResponse::alloc(),
            &url,
            response.status as isize,
            Some(&NSString::from_str("HTTP/1.1")),
            Some(&headers),
        ) else {
            let err = NSError::new(-1000, &NSString::from_str("NSURLErrorDomain"));
            unsafe { task.didFailWithError(&err) };
            return;
        };
        unsafe {
            task.didReceiveResponse(&http);
            task.didReceiveData(&NSData::with_bytes(&response.body));
            task.didFinish();
        }
    }

    fn on_message(&self, message: &WKScriptMessage) {
        let frame = unsafe { message.frameInfo() };
        if !unsafe { frame.isMainFrame() } {
            return;
        }
        let origin = unsafe { frame.securityOrigin() };
        let (proto, host) = unsafe { (origin.protocol().to_string(), origin.host().to_string()) };
        let (outcome, version, user_action) = {
            let st = self.ivars().state.borrow();
            if st.closed
                || !proto.eq_ignore_ascii_case(SCHEME)
                || !host.eq_ignore_ascii_case(&st.host)
            {
                return;
            }
            let body = unsafe { message.body() };
            let raw = if unsafe { NSJSONSerialization::isValidJSONObject(&body) } {
                unsafe {
                    NSJSONSerialization::dataWithJSONObject_options_error(
                        &body,
                        NSJSONWritingOptions::empty(),
                    )
                }
                .map(|d| d.to_vec())
                .unwrap_or_default()
            } else {
                Vec::new()
            };
            let now = Instant::now();
            let user_action = gate::recent_input(st.last_input, now);
            let version = st.version;
            drop(st);
            let outcome = self.ivars().state.borrow_mut().bridge.handle(&raw, now);
            (outcome, version, user_action)
        };
        if let Some(reply) = &outcome.reply
            && let Some(web_view) = unsafe { message.webView() }
        {
            unsafe {
                web_view.evaluateJavaScript_completionHandler(&NSString::from_str(reply), None)
            };
        }
        let event = match outcome.request {
            None => return,
            Some(Request::Emit {
                reply_id,
                name,
                payload,
            }) => ArtifactEvent::Emit {
                version,
                name,
                payload,
                user_action,
                reply_id,
            },
            Some(Request::Ask {
                reply_id,
                name,
                payload,
                timeout_ms,
            }) => ArtifactEvent::Ask {
                version,
                reply_id,
                name,
                payload,
                timeout_ms,
                user_action,
            },
            Some(Request::Ref {
                label,
                value,
                selector,
            }) => ArtifactEvent::Ref {
                version,
                label,
                value,
                selector,
            },
        };
        self.emit(event);
    }

    fn navigation_policy(&self, action: &WKNavigationAction) -> WKNavigationActionPolicy {
        let request = unsafe { action.request() };
        let url = request
            .URL()
            .and_then(|u| u.absoluteString())
            .map(|s| s.to_string())
            .unwrap_or_default();
        let mut st = self.ivars().state.borrow_mut();
        if !st.closed
            && let Some((host, _)) = resolve::split_url(&url)
            && host == st.host
        {
            return WKNavigationActionPolicy::Allow;
        }
        let lower = url.to_ascii_lowercase();
        let external = lower.starts_with("https://")
            || lower.starts_with("http://")
            || lower.starts_with("mailto:");
        if !st.closed
            && external
            && unsafe { action.navigationType() } == WKNavigationType::LinkActivated
        {
            let now = Instant::now();
            if st
                .last_external
                .is_none_or(|t| now.duration_since(t) >= EXTERNAL_LINK_INTERVAL)
            {
                st.last_external = Some(now);
                drop(st);
                self.emit(ArtifactEvent::ExternalLink { url });
            }
        }
        WKNavigationActionPolicy::Cancel
    }

    pub(crate) fn load_current(&self, web_view: &WKWebView) {
        let (url, scripts) = {
            let st = self.ivars().state.borrow();
            if st.closed || !st.rules_ready {
                return;
            }
            let init = st.bridge.init(st.viewer.as_ref(), st.ref_menu.as_ref());
            let mut scripts = vec![bridge::page_script(&init)];
            if st.bridge.caps().r#ref
                && let Some(menu) = &st.ref_menu
            {
                scripts.push(bridge::ref_listener_script(menu));
            }
            (
                format!("{SCHEME}://{}/{}", st.host, resolve::encode_path(&st.entry)),
                scripts,
            )
        };
        // The bridge script carries this version's viewer, capabilities and declarations:
        // replace the scripts before the navigation starts.
        if let Some(ucc) = self.ivars().ucc.borrow().as_ref().and_then(|w| w.load()) {
            let mtm = MainThreadMarker::from(self);
            unsafe { ucc.removeAllUserScripts() };
            add_script(mtm, &ucc, NO_NETWORK_SCRIPT, false);
            for script in &scripts {
                add_script(mtm, &ucc, script, true);
            }
        }
        if let Some(nsurl) = NSURL::URLWithString(&NSString::from_str(&url)) {
            let request = NSURLRequest::requestWithURL(&nsurl);
            unsafe { web_view.loadRequest(&request) };
        }
    }

    /// Breaks the retain cycles and stops the page. Runs once.
    pub(crate) fn teardown(&self) {
        {
            let mut st = self.ivars().state.borrow_mut();
            if st.closed {
                return;
            }
            st.closed = true;
            st.content = Content::memory([]);
            st.bridge.reset();
            st.bridge = Bridge::new(Capabilities::NONE, Some(Vec::new()), None);
        }
        if let Some(monitor) = self.ivars().monitor.borrow_mut().take() {
            unsafe { NSEvent::removeMonitor(&monitor) };
        }
        if let Some(ucc) = self.ivars().ucc.borrow().as_ref().and_then(|w| w.load()) {
            unsafe {
                ucc.removeAllScriptMessageHandlers();
                ucc.removeAllUserScripts();
            }
        }
        if let Some(web_view) = self
            .ivars()
            .webview
            .borrow()
            .as_ref()
            .and_then(|w| w.load())
        {
            unsafe {
                web_view.stopLoading();
                web_view.setNavigationDelegate(None);
                web_view.setUIDelegate(None);
            }
        }
        if let Some(window) = self.ivars().window.borrow().as_ref().and_then(|w| w.load()) {
            window.setDelegate(None);
        }
        if let Some(web_view) = self
            .ivars()
            .webview
            .borrow()
            .as_ref()
            .and_then(|w| w.load())
        {
            // Stops <video> and <audio> that keep playing in a detached view.
            unsafe { web_view.loadHTMLString_baseURL(&NSString::from_str(""), None) };
        }
        self.emit(ArtifactEvent::Closed);
    }
}

thread_local! {
    static RULES: RefCell<Option<Retained<WKContentRuleList>>> = const { RefCell::new(None) };
}

/// Calls `done` with the compiled rule list (compiled once per process).
fn with_rule_list(
    mtm: MainThreadMarker,
    done: impl Fn(Option<Retained<WKContentRuleList>>) + 'static,
) {
    if let Some(list) = RULES.with(|r| r.borrow().clone()) {
        done(Some(list));
        return;
    }
    let Some(store) = (unsafe { WKContentRuleListStore::defaultStore(mtm) }) else {
        done(None);
        return;
    };
    let block = RcBlock::new(move |list: *mut WKContentRuleList, _err: *mut NSError| {
        // SAFETY: WebKit passes a valid (or null) rule list.
        let list = unsafe { Retained::retain(list) };
        if let Some(l) = &list {
            RULES.with(|r| *r.borrow_mut() = Some(l.clone()));
        }
        done(list);
    });
    unsafe {
        store.compileContentRuleListForIdentifier_encodedContentRuleList_completionHandler(
            Some(&NSString::from_str(RULE_LIST_ID)),
            Some(&NSString::from_str(resolve::CONTENT_RULES)),
            Some(&block),
        );
    }
}

/// Validates the version and builds the shared page state.
pub(crate) fn new_state(
    artifact_id: String,
    version: VersionSpec,
    caps: Capabilities,
) -> Result<Rc<RefCell<State>>, Error> {
    let host = resolve::origin_host(&artifact_id, version.version)?;
    if !resolve::validate_rel(&version.entry) {
        return Err(Error::InvalidEntry);
    }
    let bridge = new_bridge(&version, caps, bridge::MAX_MESSAGE_BYTES);
    let content = version.content.prepared()?;
    Ok(Rc::new(RefCell::new(State {
        artifact_id,
        host,
        version: version.version,
        content,
        entry: version.entry,
        bridge,
        viewer: None,
        ref_menu: None,
        max_message_bytes: bridge::MAX_MESSAGE_BYTES,
        closed: false,
        rules_ready: false,
        last_external: None,
        last_input: None,
    })))
}

/// A fresh bridge for one version: its declarations, and its data now (a static `data`) or
/// later (a live version: the host pushes the first snapshot).
fn new_bridge(spec: &VersionSpec, caps: Capabilities, max_bytes: usize) -> Bridge {
    let initial = (!spec.live).then(|| Snapshot::new(spec.data.clone(), 0));
    Bridge::new(caps, spec.events.clone(), initial).with_max_bytes(max_bytes)
}

fn add_script(mtm: MainThreadMarker, ucc: &WKUserContentController, source: &str, main_only: bool) {
    unsafe {
        let script = WKUserScript::initWithSource_injectionTime_forMainFrameOnly(
            WKUserScript::alloc(mtm),
            &NSString::from_str(source),
            WKUserScriptInjectionTime::AtDocumentStart,
            main_only,
        );
        ucc.addUserScript(&script);
    }
}

pub(crate) struct Config {
    pub(crate) config: Retained<WKWebViewConfiguration>,
    pub(crate) ucc: Retained<WKUserContentController>,
}

/// The isolated configuration every 产出 web view uses (window or embedded): a
/// non-persistent data store, the `artifact:` scheme handler, the bridge script and
/// handler, no WebRTC, no DNS prefetch, no media devices, no element fullscreen.
pub(crate) fn configure(mtm: MainThreadMarker, controller: &Controller, autoplay: bool) -> Config {
    let config = unsafe { WKWebViewConfiguration::new(mtm) };
    let ucc = unsafe { WKUserContentController::new(mtm) };
    unsafe {
        config.setWebsiteDataStore(&WKWebsiteDataStore::nonPersistentDataStore(mtm));
        config.setURLSchemeHandler_forURLScheme(
            Some(ProtocolObject::from_ref(controller)),
            &NSString::from_str(SCHEME),
        );
        // The bridge scripts depend on the version: `load_current` adds them per load.
        ucc.addScriptMessageHandler_name(
            ProtocolObject::from_ref(controller),
            &NSString::from_str("bridge"),
        );
        // WebRTC (STUN/TURN) and DNS prefetch go around the CSP and the
        // content rule list, so a page could reach the network or learn the
        // viewer's IP. Remove the APIs in every frame as well.
        let no_rtc = WKUserScript::initWithSource_injectionTime_forMainFrameOnly(
            WKUserScript::alloc(mtm),
            &NSString::from_str(NO_NETWORK_SCRIPT),
            WKUserScriptInjectionTime::AtDocumentStart,
            false,
        );
        ucc.addUserScript(&no_rtc);
        config.setUserContentController(&ucc);
        // Local media plays inline; it starts only on a click unless the host allows
        // autoplay. AirPlay would hand the media URL to another device.
        config.setAllowsAirPlayForMediaPlayback(false);
        config.setMediaTypesRequiringUserActionForPlayback(if autoplay {
            WKAudiovisualMediaTypes::None
        } else {
            WKAudiovisualMediaTypes::All
        });
        let prefs = config.preferences();
        prefs.setJavaScriptCanOpenWindowsAutomatically(false);
        prefs.setElementFullscreenEnabled(false);
        disable_private_pref!(&prefs, _setPeerConnectionEnabled);
        disable_private_pref!(&prefs, _setDNSPrefetchingEnabled);
        disable_private_pref!(&prefs, _setMediaDevicesEnabled);
        config.setSuppressesIncrementalRendering(false);
    }
    Config { config, ucc }
}

/// Sets the delegates and the view switches shared by every 产出 web view.
pub(crate) fn wire(web_view: &WKWebView, controller: &Controller, inspectable: bool) {
    unsafe {
        web_view.setNavigationDelegate(Some(ProtocolObject::from_ref(controller)));
        web_view.setUIDelegate(Some(ProtocolObject::from_ref(controller)));
        web_view.setAllowsLinkPreview(false);
        web_view.setAllowsBackForwardNavigationGestures(false);
        web_view.setAllowsMagnification(false);
        web_view.setInspectable(inspectable);
    }
    *controller.ivars().webview.borrow_mut() = Some(Weak::new(web_view));
    watch_input(web_view, controller);
}

/// Records the time of each click, drag or key press that goes to `web_view`, so page
/// events can be tied to the viewer's own input. AppKit calls the monitor before it
/// dispatches the event; the event passes on unchanged.
fn watch_input(web_view: &WKWebView, controller: &Controller) {
    let view = Weak::new(web_view);
    let state = Rc::downgrade(&controller.ivars().state);
    let block = RcBlock::new(move |event: NonNull<NSEvent>| -> *mut NSEvent {
        // SAFETY: AppKit passes a valid event for the duration of the call.
        let ev = unsafe { event.as_ref() };
        if let (Some(view), Some(state)) = (view.load(), state.upgrade())
            && input_goes_to(ev, &view)
            && let Ok(mut st) = state.try_borrow_mut()
            && !st.closed
        {
            st.last_input = Some(Instant::now());
        }
        event.as_ptr()
    });
    let mask = NSEventMask::LeftMouseDown
        | NSEventMask::RightMouseDown
        | NSEventMask::OtherMouseDown
        | NSEventMask::LeftMouseDragged
        | NSEventMask::KeyDown;
    let monitor = unsafe { NSEvent::addLocalMonitorForEventsMatchingMask_handler(mask, &block) };
    if let Some(old) = controller.ivars().monitor.replace(monitor) {
        unsafe { NSEvent::removeMonitor(&old) };
    }
}

/// True when `event` goes to `view` or one of its subviews: a mouse event over it (hidden
/// views and views under another view do not count) or a key event while it has focus.
fn input_goes_to(event: &NSEvent, view: &WKWebView) -> bool {
    let mtm = MainThreadMarker::from(view);
    let (Some(win), Some(view_win)) = (event.window(mtm), view.window()) else {
        return false;
    };
    if !std::ptr::eq(&*win, &*view_win) {
        return false;
    }
    let target: Option<Retained<NSView>> = if event.r#type() == NSEventType::KeyDown {
        win.firstResponder()
            .and_then(|r| r.downcast::<NSView>().ok())
    } else {
        win.contentView()
            .and_then(|c| c.hitTest(event.locationInWindow()))
    };
    target.is_some_and(|t| t.isDescendantOf(view))
}

/// Installs the network-blocking rule list, then loads the page. If the list cannot be
/// compiled, the page never loads and a `LoadFailed` event is sent.
pub(crate) fn start_loading(
    mtm: MainThreadMarker,
    controller: &Retained<Controller>,
    web_view: &Retained<WKWebView>,
    ucc: &Retained<WKUserContentController>,
) {
    *controller.ivars().ucc.borrow_mut() = Some(Weak::from_retained(ucc));
    let ctl = controller.clone();
    let wv = web_view.clone();
    let ucc_for_rules = ucc.clone();
    with_rule_list(mtm, move |list| match list {
        Some(list) => {
            unsafe { ucc_for_rules.addContentRuleList(&list) };
            ctl.ivars().state.borrow_mut().rules_ready = true;
            ctl.load_current(&wv);
        }
        None => {
            let version = ctl.ivars().state.borrow().version;
            ctl.emit(ArtifactEvent::LoadFailed {
                version,
                reason: "content rule list".into(),
            });
        }
    });
}

/// Loads another version (new origin, content and data) into an existing page state.
pub(crate) fn switch_version(
    state: &RefCell<State>,
    spec: VersionSpec,
    caps: Capabilities,
) -> Result<(), Error> {
    let host = {
        let st = state.borrow();
        if st.closed {
            return Err(Error::Closed);
        }
        resolve::origin_host(&st.artifact_id, spec.version)?
    };
    if !resolve::validate_rel(&spec.entry) {
        return Err(Error::InvalidEntry);
    }
    let fresh = new_bridge(&spec, caps, state.borrow().max_message_bytes);
    let content = spec.content.prepared()?;
    let mut st = state.borrow_mut();
    // The bridge follows the version shown: another author may grant other capabilities,
    // and the old page's open asks end (the page reloads).
    st.bridge.reset();
    st.bridge = fresh;
    st.host = host;
    st.version = spec.version;
    st.content = content;
    st.entry = spec.entry;
    // Input in the previous page does not count for this one.
    st.last_input = None;
    Ok(())
}

/// Changes what the page bridge allows, for the page that shows now.
pub(crate) fn set_capabilities(state: &RefCell<State>, caps: Capabilities) {
    state.borrow_mut().bridge.set_caps(caps);
}

/// Gives the page a data snapshot (the first `GET …/data`, or a push) if it still shows
/// `version`. False when another version shows or the page is closed.
pub(crate) fn push_data(
    state: &RefCell<State>,
    web_view: &WKWebView,
    version: u32,
    snap: Snapshot,
) -> bool {
    let scripts = {
        let mut st = state.borrow_mut();
        if st.closed || st.version != version {
            return false;
        }
        st.bridge.push_data(snap)
    };
    for s in scripts {
        unsafe { web_view.evaluateJavaScript_completionHandler(&NSString::from_str(&s), None) };
    }
    true
}

/// Finishes an open `emit` or `ask` of the page that shows `version`. False when it is no
/// longer open (a late reply after a timeout or reload): the host drops it.
pub(crate) fn finish(
    state: &RefCell<State>,
    web_view: &WKWebView,
    version: u32,
    reply_id: &Value,
    result: Result<Value, Reject>,
) -> bool {
    let script = {
        let mut st = state.borrow_mut();
        if st.closed || st.version != version {
            return false;
        }
        st.bridge.finish(reply_id, result)
    };
    match script {
        Some(s) => {
            unsafe { web_view.evaluateJavaScript_completionHandler(&NSString::from_str(&s), None) };
            true
        }
        None => false,
    }
}

/// A window that shows one 产出. Main thread only. Dropping it closes the window.
pub struct ArtifactWindow {
    window: Retained<NSWindow>,
    web_view: Retained<WKWebView>,
    controller: Retained<Controller>,
    popup: RefCell<Option<Retained<NSPopUpButton>>>,
    state: Rc<RefCell<State>>,
}

impl ArtifactWindow {
    /// Opens a window. `on_event` runs on the main thread, never inside another window call.
    /// The page loads once the network-blocking rule list is installed; if it cannot be
    /// compiled, the page does not load and a `LoadFailed` event is sent.
    pub fn open(
        mtm: MainThreadMarker,
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
            transparent,
            viewer,
            ref_menu,
        } = options;
        let state = new_state(artifact_id, version, capabilities)?;
        {
            let mut st = state.borrow_mut();
            st.max_message_bytes = max_message_bytes;
            st.bridge.set_max_bytes(max_message_bytes);
            st.viewer = viewer;
            st.ref_menu = ref_menu;
        }
        let controller = Controller::new(mtm, state.clone(), Rc::new(on_event));
        let config = configure(mtm, &controller, false);
        let frame = NSRect::new(
            NSPoint::new(0.0, 0.0),
            NSSize::new(width.max(320.0), height.max(240.0)),
        );
        let window = unsafe {
            NSWindow::initWithContentRect_styleMask_backing_defer(
                NSWindow::alloc(mtm),
                frame,
                NSWindowStyleMask::Titled
                    | NSWindowStyleMask::Closable
                    | NSWindowStyleMask::Miniaturizable
                    | NSWindowStyleMask::Resizable,
                NSBackingStoreType::Buffered,
                false,
            )
        };
        // SAFETY: we keep our own reference, so AppKit must not release the window on close.
        unsafe { window.setReleasedWhenClosed(false) };
        window.setTabbingMode(NSWindowTabbingMode::Disallowed);
        window.setTitle(&NSString::from_str(&title));
        window.center();

        let web_view = unsafe {
            WKWebView::initWithFrame_configuration(WKWebView::alloc(mtm), frame, &config.config)
        };
        wire(&web_view, &controller, inspectable);
        if transparent {
            // SAFETY: `drawsBackground` is a BOOL key WKWebView has answered to since macOS
            // 10.10; with NO the page's transparent pixels stay transparent in snapshots.
            unsafe {
                let no = objc2_foundation::NSNumber::new_bool(false);
                let _: () = msg_send![&*web_view, setValue: &*no, forKey: &*NSString::from_str("drawsBackground")];
            }
        }
        let ucc = config.ucc;
        window.setContentView(Some(&web_view));
        window.setDelegate(Some(ProtocolObject::from_ref(&*controller)));

        *controller.ivars().window.borrow_mut() = Some(Weak::from_retained(&window));

        let this = Self {
            window,
            web_view,
            controller,
            popup: RefCell::new(None),
            state,
        };
        this.set_versions(&versions);

        start_loading(mtm, &this.controller, &this.web_view, &ucc);

        match presentation {
            Presentation::Front => this.window.makeKeyAndOrderFront(None),
            Presentation::Hidden => {}
            Presentation::Offscreen => this.order_offscreen(),
        }
        Ok(this)
    }

    /// Shows another version (new origin, content and data) in the same window. `caps` are
    /// that version's bridge capabilities.
    pub fn show_version(&self, spec: VersionSpec, caps: Capabilities) -> Result<(), Error> {
        let version = spec.version;
        switch_version(&self.state, spec, caps)?;
        if let Some(popup) = self.popup.borrow().as_ref() {
            popup.selectItemWithTag(version as isize);
        }
        self.controller.load_current(&self.web_view);
        Ok(())
    }

    /// Replaces the rows of the title-bar version picker; an empty list hides it.
    pub fn set_versions(&self, versions: &[VersionItem]) {
        let mtm = MainThreadMarker::from(&*self.controller);
        let mut slot = self.popup.borrow_mut();
        if slot.is_none() && !versions.is_empty() {
            let popup = NSPopUpButton::initWithFrame_pullsDown(
                NSPopUpButton::alloc(mtm),
                NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(160.0, 24.0)),
                false,
            );
            unsafe {
                popup.setTarget(Some(&*self.controller as &AnyObject));
                popup.setAction(Some(sel!(versionPicked:) as Sel));
            }
            let accessory = NSTitlebarAccessoryViewController::new(mtm);
            accessory.setView(&popup);
            accessory.setLayoutAttribute(NSLayoutAttribute::Right);
            self.window.addTitlebarAccessoryViewController(&accessory);
            *slot = Some(popup);
        }
        if let Some(popup) = slot.as_ref() {
            popup.removeAllItems();
            for item in versions {
                popup.addItemWithTitle(&NSString::from_str(&item.label));
                if let Some(menu_item) = popup.lastItem() {
                    menu_item.setTag(item.version as isize);
                }
            }
            popup.selectItemWithTag(self.version() as isize);
            popup.setHidden(versions.is_empty());
        }
    }

    pub fn set_title(&self, title: &str) {
        self.window.setTitle(&NSString::from_str(title));
    }

    /// Brings the window to the front and makes it key.
    pub fn focus(&self) {
        if !self.is_closed() {
            self.window.makeKeyAndOrderFront(None);
        }
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
        set_capabilities(&self.state, caps);
    }

    /// Gives the page of `version` a data snapshot (see [`push_data`]).
    pub fn push_data(&self, version: u32, snap: Snapshot) -> bool {
        push_data(&self.state, &self.web_view, version, snap)
    }

    /// Finishes an open `emit` / `ask` of the page of `version` (see [`finish`]).
    pub fn finish(&self, version: u32, reply_id: &Value, result: Result<Value, Reject>) -> bool {
        finish(&self.state, &self.web_view, version, reply_id, result)
    }

    pub fn is_closed(&self) -> bool {
        self.state.borrow().closed
    }

    /// Closes the window. A `Closed` event follows once. Safe to call more than once.
    pub fn close(&self) {
        if self.is_closed() {
            return;
        }
        self.controller.teardown();
        self.window.close();
    }

    /// Puts the window at alpha 0 far outside every screen and orders it on screen without
    /// making it key, so its web content paints but nobody sees or clicks it.
    fn order_offscreen(&self) {
        let mtm = MainThreadMarker::from(&*self.controller);
        let mut far = NSPoint::new(-20000.0, -20000.0);
        for screen in objc2_app_kit::NSScreen::screens(mtm).iter() {
            let f = screen.frame();
            far.x = far.x.min(f.origin.x - f.size.width - 20000.0);
            far.y = far.y.min(f.origin.y - f.size.height - 20000.0);
        }
        self.window.setAlphaValue(0.0);
        self.window.setIgnoresMouseEvents(true);
        self.window.setHasShadow(false);
        self.window.setCollectionBehavior(
            objc2_app_kit::NSWindowCollectionBehavior::Transient
                | objc2_app_kit::NSWindowCollectionBehavior::IgnoresCycle,
        );
        self.window.setFrameOrigin(far);
        self.window.orderBack(None);
    }

    /// Resizes the page area (CSS px = points) so content up to this size lays out and can
    /// be captured.
    pub fn set_content_size(&self, width: f64, height: f64) {
        if !self.is_closed() {
            self.window
                .setContentSize(NSSize::new(width.max(1.0), height.max(1.0)));
        }
    }

    /// Takes a PNG of `rect` of the page (CSS px, from the top-left of the view) at `scale`
    /// output pixels per CSS px. `done` runs on the main thread with the PNG bytes, or an
    /// error text for the log. The view must be at least as large as `rect`.
    pub fn snapshot(
        &self,
        rect: crate::Rect,
        scale: f64,
        done: impl FnOnce(Result<Vec<u8>, String>) + 'static,
    ) {
        if self.is_closed() {
            done(Err("closed".into()));
            return;
        }
        let backing = self.window.backingScaleFactor().max(1.0);
        let config = unsafe {
            objc2_web_kit::WKSnapshotConfiguration::new(MainThreadMarker::from(&*self.controller))
        };
        unsafe {
            config.setRect(objc2_foundation::NSRect::new(
                NSPoint::new(rect.x, rect.y),
                NSSize::new(rect.w.max(1.0), rect.h.max(1.0)),
            ));
            let width_pt = (rect.w * scale / backing).max(1.0);
            config.setSnapshotWidth(Some(&objc2_foundation::NSNumber::new_f64(width_pt)));
            config.setAfterScreenUpdates(true);
        }
        let done = RefCell::new(Some(done));
        let block = RcBlock::new(
            move |image: *mut objc2_app_kit::NSImage, error: *mut NSError| {
                let Some(done) = done.borrow_mut().take() else {
                    return;
                };
                // SAFETY: WebKit passes a valid (or null) image and error.
                let image = unsafe { Retained::retain(image) };
                let result = match image {
                    Some(image) => png_of(&image),
                    None => {
                        let error = unsafe { Retained::retain(error) };
                        Err(error.map_or_else(
                            || "no image".to_string(),
                            |e| format!("{} {}", e.domain(), e.code()),
                        ))
                    }
                };
                done(result);
            },
        );
        unsafe {
            self.web_view
                .takeSnapshotWithConfiguration_completionHandler(Some(&config), &block)
        };
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
}

impl Drop for ArtifactWindow {
    fn drop(&mut self) {
        self.close();
    }
}

/// Opens a machine's screen sharing address (see [`crate::is_screen_share_url`]) with the
/// system's handler (Screen Sharing for `vnc:`). Returns false for anything else. Call it
/// only after the user agreed.
pub fn open_screen_share_url(url: &str) -> bool {
    if !crate::is_screen_share_url(url) {
        return false;
    }
    let Some(nsurl) = NSURL::URLWithString(&NSString::from_str(url)) else {
        return false;
    };
    NSWorkspace::sharedWorkspace().openURL(&nsurl)
}

/// PNG bytes of an image at its full pixel size.
fn png_of(image: &objc2_app_kit::NSImage) -> Result<Vec<u8>, String> {
    use objc2_app_kit::{NSBitmapImageFileType, NSBitmapImageRep};
    let tiff = image.TIFFRepresentation().ok_or("no tiff")?;
    let rep = NSBitmapImageRep::imageRepWithData(&tiff).ok_or("no bitmap")?;
    let png = unsafe {
        rep.representationUsingType_properties(NSBitmapImageFileType::PNG, &NSDictionary::new())
    }
    .ok_or("no png")?;
    Ok(png.to_vec())
}

/// Opens an `http`, `https` or `mailto` URL in the default app. Returns false for any other
/// scheme. Call it only after the user agreed.
pub fn open_external_url(url: &str) -> bool {
    let lower = url.to_ascii_lowercase();
    if !(lower.starts_with("https://")
        || lower.starts_with("http://")
        || lower.starts_with("mailto:"))
    {
        return false;
    }
    let Some(nsurl) = NSURL::URLWithString(&NSString::from_str(url)) else {
        return false;
    };
    NSWorkspace::sharedWorkspace().openURL(&nsurl)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spec(version: u32) -> VersionSpec {
        VersionSpec::new(
            version,
            Content::memory([("index.html".to_string(), b"<p>".to_vec())]),
        )
    }

    #[test]
    fn the_bridge_follows_the_version_shown() {
        // v1 by a person: read only. v2 by a bot that declares an event: emit.
        let state = new_state("a1".into(), spec(1), Capabilities::READ).unwrap();
        let bot = Capabilities::ALL;
        state.borrow_mut().last_input = Some(Instant::now());
        let v2 = spec(2).with_events(vec![crate::EventDecl::new(
            "approve",
            crate::EventKind::Emit,
        )]);
        switch_version(&state, v2, bot).unwrap();
        {
            let st = state.borrow();
            assert_eq!(
                (st.version, st.bridge.caps(), st.last_input),
                (2, bot, None)
            );
            assert_eq!(st.host, resolve::origin_host("a1", 2).unwrap());
        }
        let ask = br#"{"id":1,"method":"emitEvent","params":{"name":"approve"}}"#;
        let now = Instant::now();
        assert!(state.borrow_mut().bridge.handle(ask, now).request.is_some());
        // Back to the person's v1: nothing goes out again.
        switch_version(&state, spec(1), Capabilities::READ).unwrap();
        assert_eq!(state.borrow().bridge.caps(), Capabilities::READ);
        let o = state.borrow_mut().bridge.handle(ask, now);
        assert!(o.request.is_none());
        assert!(o.reply.unwrap().contains("not_allowed"));
        // A failed switch keeps the version and capabilities it had.
        let mut bad = spec(3);
        bad.entry = "../x".into();
        assert_eq!(switch_version(&state, bad, bot), Err(Error::InvalidEntry));
        assert_eq!(state.borrow().bridge.caps(), Capabilities::READ);
    }

    #[test]
    fn a_live_version_waits_for_the_host_s_first_snapshot() {
        let state = new_state("a1".into(), spec(1).live_data(), Capabilities::READ).unwrap();
        let get = br#"{"id":"g","method":"getData"}"#;
        assert_eq!(
            state.borrow_mut().bridge.handle(get, Instant::now()).reply,
            None
        );
        let out = state
            .borrow_mut()
            .bridge
            .push_data(Snapshot::new(serde_json::json!({"n":1}), 4));
        assert_eq!(out.len(), 1);
        // A static version answers at once with its data.
        let state = new_state(
            "a1".into(),
            spec(1).with_data(serde_json::json!([1])),
            Capabilities::READ,
        )
        .unwrap();
        let o = state.borrow_mut().bridge.handle(get, Instant::now());
        assert!(o.reply.unwrap().contains("\"result\":[1]"));
    }
}
