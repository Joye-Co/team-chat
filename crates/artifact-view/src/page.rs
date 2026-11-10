//! Platform-independent state of one 产出 page (window or embedded): the version shown,
//! its bridge, and the decisions both backends (WKWebView, WebView2) make the same way.

use std::cell::RefCell;
use std::rc::Rc;
use std::time::{Duration, Instant};

use serde_json::Value;

use crate::resolve::{self, Content};
use crate::{
    ArtifactEvent, Capabilities, Error, RefMenu, Reject, Snapshot, VersionSpec, Viewer,
    bridge::{self, Bridge, Request},
};

/// Removes the WebRTC constructors in every frame: WebRTC (STUN/TURN) goes around the CSP
/// and the request blocking, so a page could reach the network or learn the viewer's IP.
#[cfg(target_os = "macos")]
pub(crate) const NO_NETWORK_SCRIPT: &str = "(()=>{for(const k of ['RTCPeerConnection','webkitRTCPeerConnection','RTCDataChannel','RTCIceCandidate','RTCSessionDescription','RTCRtpSender','RTCRtpReceiver','RTCRtpTransceiver']){try{delete window[k]}catch(e){}try{Object.defineProperty(window,k,{value:undefined,configurable:false,writable:false})}catch(e){}}})();";

/// At most one `ExternalLink` event in this time.
pub(crate) const EXTERNAL_LINK_INTERVAL: Duration = Duration::from_millis(800);

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
    pub(crate) max_payload_bytes: usize,
    pub(crate) closed: bool,
    #[cfg(target_os = "macos")]
    pub(crate) rules_ready: bool,
    pub(crate) last_external: Option<Instant>,
    /// The viewer's last click, drag or key press in this web view (see [`crate::gate`]).
    pub(crate) last_input: Option<Instant>,
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
    let bridge = new_bridge(
        &version,
        caps,
        bridge::MAX_MESSAGE_BYTES,
        bridge::MAX_PAYLOAD_BYTES,
    );
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
        max_payload_bytes: bridge::MAX_PAYLOAD_BYTES,
        closed: false,
        #[cfg(target_os = "macos")]
        rules_ready: false,
        last_external: None,
        last_input: None,
    })))
}

/// A fresh bridge for one version: its declarations, and its data now (a static `data`) or
/// later (a live version: the host pushes the first snapshot).
fn new_bridge(
    spec: &VersionSpec,
    caps: Capabilities,
    max_bytes: usize,
    max_payload_bytes: usize,
) -> Bridge {
    let initial = (!spec.live).then(|| Snapshot::new(spec.data.clone(), 0));
    Bridge::new(caps, spec.events.clone(), initial)
        .with_max_bytes(max_bytes)
        .with_max_payload_bytes(max_payload_bytes)
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
    let fresh = {
        let st = state.borrow();
        new_bridge(&spec, caps, st.max_message_bytes, st.max_payload_bytes)
    };
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

/// Marks the page closed and drops its content and bridge. False when it was closed
/// already.
pub(crate) fn close(state: &RefCell<State>) -> bool {
    let mut st = state.borrow_mut();
    if st.closed {
        return false;
    }
    st.closed = true;
    st.content = Content::memory([]);
    st.bridge.reset();
    st.bridge = Bridge::new(Capabilities::NONE, Some(Vec::new()), None);
    true
}

/// The scripts that give the page of `version` a data snapshot. `None` when another
/// version shows or the page is closed.
pub(crate) fn push_data_scripts(
    state: &RefCell<State>,
    version: u32,
    snap: Snapshot,
) -> Option<Vec<String>> {
    let mut st = state.borrow_mut();
    if st.closed || st.version != version {
        return None;
    }
    Some(st.bridge.push_data(snap))
}

/// The script that finishes an open `emit` or `ask` of the page that shows `version`.
/// `None` when it is no longer open (a late reply after a timeout or reload).
pub(crate) fn finish_script(
    state: &RefCell<State>,
    version: u32,
    reply_id: &Value,
    result: Result<Value, Reject>,
) -> Option<String> {
    let mut st = state.borrow_mut();
    if st.closed || st.version != version {
        return None;
    }
    st.bridge.finish(reply_id, result)
}

/// The host event for a bridge request the page may make.
pub(crate) fn request_event(request: Request, version: u32, user_action: bool) -> ArtifactEvent {
    match request {
        Request::Emit {
            reply_id,
            name,
            payload,
        } => ArtifactEvent::Emit {
            version,
            name,
            payload,
            user_action,
            reply_id,
        },
        Request::Ask {
            reply_id,
            name,
            payload,
            timeout_ms,
        } => ArtifactEvent::Ask {
            version,
            reply_id,
            name,
            payload,
            timeout_ms,
            user_action,
        },
        Request::Ref {
            label,
            value,
            selector,
        } => ArtifactEvent::Ref {
            version,
            label,
            value,
            selector,
        },
    }
}

/// What to do with a navigation.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Nav {
    /// It stays in the origin of the version shown.
    Allow,
    /// Deny it.
    Cancel,
    /// Deny it and tell the host: the viewer activated a link to this external URL.
    External(String),
}

/// True for the URLs a page link may ask the host to open.
pub(crate) fn is_external_url(url: &str) -> bool {
    let lower = url.to_ascii_lowercase();
    lower.starts_with("https://") || lower.starts_with("http://") || lower.starts_with("mailto:")
}

/// Decides a navigation to `url`. `link` says the platform saw a link activation (not a
/// script changing the location). External links are reported at most once per
/// [`EXTERNAL_LINK_INTERVAL`].
pub(crate) fn navigation(state: &RefCell<State>, url: &str, link: bool, now: Instant) -> Nav {
    let mut st = state.borrow_mut();
    if !st.closed
        && let Some((host, _)) = resolve::split_url(url)
        && host == st.host
    {
        return Nav::Allow;
    }
    if !st.closed
        && link
        && is_external_url(url)
        && st
            .last_external
            .is_none_or(|t| now.duration_since(t) >= EXTERNAL_LINK_INTERVAL)
    {
        st.last_external = Some(now);
        return Nav::External(url.to_string());
    }
    Nav::Cancel
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

    #[test]
    fn navigations_stay_in_the_origin_and_links_are_paced() {
        let state = new_state("a1".into(), spec(1), Capabilities::READ).unwrap();
        let t = Instant::now();
        let nav = |url: &str, link: bool, at: Instant| navigation(&state, url, link, at);
        assert_eq!(nav("artifact://a1-v1/other.html", false, t), Nav::Allow);
        assert_eq!(nav("ARTIFACT://A1-V1/", false, t), Nav::Allow);
        // Another version, the network and local files are never opened in the page.
        for url in [
            "artifact://a1-v2/",
            "https://example.com/",
            "file:///etc/passwd",
            "about:blank",
            "javascript:alert(1)",
        ] {
            assert_eq!(nav(url, false, t), Nav::Cancel, "{url}");
        }
        assert_eq!(nav("file:///etc/passwd", true, t), Nav::Cancel);
        assert_eq!(
            nav("https://example.com/", true, t),
            Nav::External("https://example.com/".into())
        );
        assert_eq!(nav("mailto:a@example.com", true, t), Nav::Cancel);
        assert_eq!(
            nav("mailto:a@example.com", true, t + EXTERNAL_LINK_INTERVAL),
            Nav::External("mailto:a@example.com".into())
        );
        assert!(close(&state));
        assert!(!close(&state));
        assert_eq!(nav("artifact://a1-v1/", false, t), Nav::Cancel);
        let late = t + EXTERNAL_LINK_INTERVAL * 3;
        assert_eq!(nav("https://example.com/", true, late), Nav::Cancel);
    }
}
