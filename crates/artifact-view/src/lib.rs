//! Native window that shows 产出 (HTML artifact) pages in an isolated web view: WKWebView on
//! macOS, WebView2 on Windows. Both backends have the same public API and the same rules.
//!
//! - Each version gets its own origin: `artifact://<artifactId>-v<version>/<path>`, served
//!   from a folder or from memory (`WKURLSchemeHandler`; on Windows a registered custom scheme
//!   answered in `WebResourceRequested`).
//! - Every response carries a strict CSP; http, https, ws, ftp and file loads are blocked
//!   (`WKContentRuleList`; on Windows every request outside the origin gets a 403 and the
//!   browser process resolves no host name); every navigation away from the current origin
//!   and every new window is denied.
//! - The page talks to the host only through one message channel
//!   (`window.webkit.messageHandlers.bridge`, on Windows `chrome.webview.postMessage`) behind
//!   `window.artifact.getData()` / `window.artifact.emitEvent(name, payload)`, checked
//!   against the window's [`Capabilities`].
//!
//! The same page can also live inside a host window ([`EmbeddedArtifact`]): a child view
//! of the host's content view that the host places from its own layout every frame
//! ([`embed`]). It has the same isolation as the window.
//!
//! All window functions run on the main thread inside an app that already runs
//! `NSApplication` (on Windows: the thread that runs the message loop). [`MainThread`] is the
//! token for it.

pub mod bridge;
pub mod diagram;
pub mod embed;
pub mod gate;
mod page;
pub mod resolve;

#[cfg(target_os = "macos")]
mod mac;
#[cfg(target_os = "macos")]
mod mac_embed;
#[cfg(windows)]
mod win;
#[cfg(windows)]
mod win_embed;
#[cfg(windows)]
#[doc(hidden)]
pub mod win_support;

use std::path::PathBuf;

pub use bridge::{
    Capabilities, DataCause, EventDecl, EventKind, RefMenu, Reject, Snapshot, Viewer,
};
pub use embed::{EmbedOptions, Placement, Rect};
pub use resolve::{Content, origin_host};
use serde_json::Value;

#[cfg(target_os = "macos")]
pub use mac::{ArtifactWindow, open_external_url, open_screen_share_url};
#[cfg(windows)]
pub use win::{
    ArtifactWindow, MainThread, open_external_url, open_screen_share_url, set_user_data_dir,
};

/// Proof that the caller is on the main thread.
#[cfg(target_os = "macos")]
pub type MainThread = objc2_foundation::MainThreadMarker;

/// Schemes a machine's screen sharing address (`takeoverUrl`) may use.
pub const SCREEN_SHARE_SCHEMES: &[&str] = &["vnc", "http", "https"];

/// True when `url` is a screen sharing address the app may offer to open: `vnc://`,
/// `http://` or `https://` with a host, at most 2048 bytes, with no spaces or control
/// characters. Anything else (`file:`, `smb:`, other apps' schemes) is never opened.
pub fn is_screen_share_url(url: &str) -> bool {
    if url.is_empty()
        || url.len() > 2048
        || url.chars().any(|c| c.is_whitespace() || c.is_control())
    {
        return false;
    }
    let Some((scheme, rest)) = url.split_once("://") else {
        return false;
    };
    SCREEN_SHARE_SCHEMES
        .iter()
        .any(|s| scheme.eq_ignore_ascii_case(s))
        && rest
            .split(['/', '?', '#'])
            .next()
            .is_some_and(|host| !host.is_empty() && !host.ends_with('@'))
}
#[cfg(target_os = "macos")]
pub use mac_embed::{EmbeddedArtifact, view_from_ptr};
#[cfg(windows)]
pub use win_embed::EmbeddedArtifact;

/// One version of a 产出 to show.
#[derive(Clone, Debug)]
pub struct VersionSpec {
    pub version: u32,
    pub content: Content,
    /// Relative path of the page to open, usually `index.html`.
    pub entry: String,
    /// JSON the page reads with `getData` (when `live` is false).
    pub data: Value,
    /// The host sends the first data snapshot itself (the `GET …/data` when the page
    /// opens): `getData` and `onData` wait for it. When false, `data` is the snapshot.
    pub live: bool,
    /// The events the version declares. `None` puts no declaration rule on `emit`
    /// (self-tests and the diagram renderer); `Some` lets only these names out, each with
    /// its kind.
    pub events: Option<Vec<EventDecl>>,
}

impl VersionSpec {
    pub fn new(version: u32, content: Content) -> Self {
        Self {
            version,
            content,
            entry: "index.html".into(),
            data: Value::Null,
            live: false,
            events: None,
        }
    }

    /// The host pushes the first snapshot (see [`VersionSpec::live`]).
    pub fn live_data(mut self) -> Self {
        self.live = true;
        self
    }

    pub fn with_events(mut self, events: Vec<EventDecl>) -> Self {
        self.events = Some(events);
        self
    }

    pub fn with_data(mut self, data: Value) -> Self {
        self.data = data;
        self
    }

    pub fn with_entry(mut self, entry: impl Into<String>) -> Self {
        self.entry = entry.into();
        self
    }
}

/// One row of the version list in the window title bar. The label comes from the host's copy
/// catalog.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VersionItem {
    pub version: u32,
    pub label: String,
}

/// How the window appears when it opens.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Presentation {
    /// Make it the key window and bring it to the front (normal use).
    #[default]
    Front,
    /// Do not order the window on screen (self-tests and preloading). Call
    /// [`ArtifactWindow::focus`] later to show it.
    Hidden,
    /// Order the window on screen at alpha 0, outside every screen's frame, ignoring the
    /// mouse: for web content that must paint (snapshots) but never be seen.
    Offscreen,
}

/// Everything needed to open an artifact window.
#[derive(Clone, Debug)]
pub struct OpenOptions {
    pub title: String,
    pub width: f64,
    pub height: f64,
    pub artifact_id: String,
    pub version: VersionSpec,
    pub capabilities: Capabilities,
    /// Rows of the version picker; empty hides the picker.
    pub versions: Vec<VersionItem>,
    pub presentation: Presentation,
    /// Allow Safari Web Inspector (developer view only).
    pub inspectable: bool,
    /// Largest bridge message the page may send (serialized JSON). Default
    /// [`bridge::MAX_MESSAGE_BYTES`] (256 KiB); the diagram renderer uses 4 MiB.
    pub max_message_bytes: usize,
    /// Largest `emit` / `ask` payload. Default [`bridge::MAX_PAYLOAD_BYTES`] (16 KiB, an
    /// 产出 page); the diagram renderer sends whole SVGs and uses its message limit.
    pub max_payload_bytes: usize,
    /// The web view draws no background, so snapshots keep transparent pixels
    /// (the diagram renderer).
    pub transparent: bool,
    /// Who looks at the page (`window.teamchat.viewer`); `None` in snapshots.
    pub viewer: Option<Viewer>,
    /// The ⌥-click outline and `就这里提问` menu, when the page may `ref`.
    pub ref_menu: Option<RefMenu>,
}

impl OpenOptions {
    pub fn new(
        title: impl Into<String>,
        artifact_id: impl Into<String>,
        version: VersionSpec,
    ) -> Self {
        Self {
            title: title.into(),
            width: 960.0,
            height: 720.0,
            artifact_id: artifact_id.into(),
            version,
            capabilities: Capabilities::ALL,
            versions: Vec::new(),
            presentation: Presentation::Front,
            inspectable: false,
            max_message_bytes: bridge::MAX_MESSAGE_BYTES,
            max_payload_bytes: bridge::MAX_PAYLOAD_BYTES,
            transparent: false,
            viewer: None,
            ref_menu: None,
        }
    }
}

/// Events the window gives to the host callback, always on the main thread and never
/// re-entrantly: the host may call any window method (also `close`) from the callback.
#[derive(Clone, Debug, PartialEq)]
pub enum ArtifactEvent {
    /// The page of `version` finished loading.
    Loaded { version: u32 },
    /// The page could not load, or its web content process ended.
    LoadFailed { version: u32, reason: String },
    /// The page called `emit(name, payload)` (or the legacy `emitEvent`) with a declared
    /// name. `user_action` is true when the viewer clicked, dragged or typed in this web
    /// view at most [`gate::USER_ACTION_WINDOW`] before; the host forwards only those
    /// events (see [`gate`]). With a `reply_id` the page waits: the host calls `finish`
    /// with `{eventId}` or an error.
    Emit {
        version: u32,
        name: String,
        payload: Value,
        user_action: bool,
        reply_id: Option<Value>,
    },
    /// The page called `ask(name, payload)` with a declared name. The host sends it, then
    /// calls `finish(reply_id, …)` with the member's reply, or [`Reject::Timeout`] after
    /// `timeout_ms`. Same `user_action` rule as `Emit`.
    Ask {
        version: u32,
        reply_id: Value,
        name: String,
        payload: Value,
        timeout_ms: u64,
        user_action: bool,
    },
    /// The page pointed at one of its parts (`ref`, or ⌥-click on `data-ref-label`): the
    /// host puts it into the viewer's composer (REFS §A7). Checked already.
    Ref {
        version: u32,
        label: String,
        value: Option<Value>,
        selector: Option<String>,
    },
    /// The user activated a link to an `http`, `https` or `mailto` URL. The navigation was
    /// cancelled; the host may ask the user and then call [`open_external_url`].
    ExternalLink { url: String },
    /// The user picked a version in the title-bar picker. The host loads that version and
    /// calls `show_version`.
    VersionSelected { version: u32 },
    /// An embedded page got (`true`) or lost (`false`) keyboard focus. Only
    /// [`EmbeddedArtifact`] sends it.
    FocusChanged { focused: bool },
    /// The window closed (by the user or by `close`), or the embedded view was removed.
    Closed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    /// The id is empty, longer than 64 or has characters other than `[A-Za-z0-9_-]`.
    InvalidArtifactId,
    /// The entry path is not a safe relative path.
    InvalidEntry,
    /// The content folder does not exist.
    FolderMissing(PathBuf),
    /// The window was closed.
    Closed,
    /// The host view to embed in has no superview (it is not in a window).
    NoHostView,
    /// The native window or web view could not be created.
    Native(String),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::InvalidArtifactId => write!(f, "invalid artifact id"),
            Error::InvalidEntry => write!(f, "invalid entry path"),
            Error::FolderMissing(p) => write!(f, "artifact folder missing: {}", p.display()),
            Error::Closed => write!(f, "artifact window closed"),
            Error::NoHostView => write!(f, "host view is not in a window"),
            Error::Native(e) => write!(f, "native web view: {e}"),
        }
    }
}

impl std::error::Error for Error {}

#[cfg(test)]
mod tests {
    use super::is_screen_share_url as ok;

    #[test]
    fn screen_share_urls() {
        for u in [
            "vnc://mac-mini.tailnet",
            "VNC://mac-mini.tailnet:5900",
            "vnc://joye@mac-mini",
            "https://share.example.com/s/1",
            "http://10.0.0.2:8080",
        ] {
            assert!(ok(u), "{u}");
        }
        for u in [
            "",
            "file:///Users/me/Downloads/x.command",
            "smb://attacker.example/share",
            "x-man-page://ls",
            "javascript:alert(1)",
            "vnc:mac-mini",
            "vnc://",
            "vnc:///path",
            "vnc://user@",
            "vnc://mac mini",
            "vnc://mac-mini\n",
            "https://a.example/\u{7}",
        ] {
            assert!(!ok(u), "{u}");
        }
        assert!(!ok(&format!("https://a.example/{}", "x".repeat(2048))));
    }
}
