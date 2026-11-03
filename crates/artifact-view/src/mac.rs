//! AppKit and WebKit side: the window, the scheme handler, the bridge and the delegates.

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::{Duration, Instant};

use block2::{DynBlock, RcBlock};
use dispatch2::DispatchQueue;
use objc2::rc::{Retained, Weak};
use objc2::runtime::{AnyObject, ProtocolObject, Sel};
use objc2::{AllocAnyThread, DefinedClass, MainThreadOnly, define_class, msg_send, sel};
use objc2_app_kit::{
    NSBackingStoreType, NSLayoutAttribute, NSPopUpButton, NSTitlebarAccessoryViewController,
    NSWindow, NSWindowDelegate, NSWindowStyleMask, NSWindowTabbingMode, NSWorkspace,
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
    ArtifactEvent, Capabilities, Error, OpenOptions, Presentation, VersionItem, VersionSpec, bridge,
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
    pub(crate) data: Value,
    pub(crate) caps: Capabilities,
    pub(crate) closed: bool,
    pub(crate) rules_ready: bool,
    pub(crate) last_external: Option<Instant>,
}

pub(crate) struct Ivars {
    state: Rc<RefCell<State>>,
    cb: Rc<dyn Fn(ArtifactEvent)>,
    webview: RefCell<Option<Weak<WKWebView>>>,
    window: RefCell<Option<Weak<NSWindow>>>,
    ucc: RefCell<Option<Weak<WKUserContentController>>>,
    reloads: Cell<u32>,
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
        let (outcome, version) = {
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
            (bridge::handle(&raw, st.caps, &st.data), st.version)
        };
        if let Some(web_view) = unsafe { message.webView() } {
            let script = NSString::from_str(&bridge::reply_script(&outcome.reply));
            unsafe { web_view.evaluateJavaScript_completionHandler(&script, None) };
        }
        if let Some((name, payload)) = outcome.event {
            self.emit(ArtifactEvent::Emit {
                version,
                name,
                payload,
            });
        }
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
        let url = {
            let st = self.ivars().state.borrow();
            if st.closed || !st.rules_ready {
                return;
            }
            format!("{SCHEME}://{}/{}", st.host, resolve::encode_path(&st.entry))
        };
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
            st.data = Value::Null;
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
    let content = version.content.prepared()?;
    Ok(Rc::new(RefCell::new(State {
        artifact_id,
        host,
        version: version.version,
        content,
        entry: version.entry,
        data: version.data,
        caps,
        closed: false,
        rules_ready: false,
        last_external: None,
    })))
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
        let script = WKUserScript::initWithSource_injectionTime_forMainFrameOnly(
            WKUserScript::alloc(mtm),
            &NSString::from_str(bridge::PAGE_SCRIPT),
            WKUserScriptInjectionTime::AtDocumentStart,
            true,
        );
        ucc.addUserScript(&script);
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
pub(crate) fn switch_version(state: &RefCell<State>, spec: VersionSpec) -> Result<(), Error> {
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
    let content = spec.content.prepared()?;
    let mut st = state.borrow_mut();
    st.host = host;
    st.version = spec.version;
    st.content = content;
    st.entry = spec.entry;
    st.data = spec.data;
    Ok(())
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
        } = options;
        let state = new_state(artifact_id, version, capabilities)?;
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

        if presentation == Presentation::Front {
            this.window.makeKeyAndOrderFront(None);
        }
        Ok(this)
    }

    /// Shows another version (new origin, content and data) in the same window.
    pub fn show_version(&self, spec: VersionSpec) -> Result<(), Error> {
        let version = spec.version;
        switch_version(&self.state, spec)?;
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
