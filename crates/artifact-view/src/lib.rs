//! Native window that shows 产出 (HTML artifact) pages in an isolated WKWebView.
//!
//! - Each version gets its own origin: `artifact://<artifactId>-v<version>/<path>`, served by a
//!   `WKURLSchemeHandler` from a folder or from memory.
//! - Every response carries a strict CSP; a `WKContentRuleList` blocks http, https, ws, ftp and
//!   file loads; the navigation delegate denies every navigation away from the current origin.
//! - The page talks to the host only through `window.webkit.messageHandlers.bridge`
//!   (`window.artifact.getData()` / `window.artifact.emitEvent(name, payload)`), checked
//!   against the window's [`Capabilities`].
//!
//! The same page can also live inside a host window ([`EmbeddedArtifact`]): a child view
//! of the host's content view that the host places from its own layout every frame
//! ([`embed`]). It has the same isolation as the window.
//!
//! All window functions run on the main thread inside an app that already runs
//! `NSApplication`.

pub mod bridge;
pub mod embed;
pub mod gate;
pub mod resolve;

#[cfg(target_os = "macos")]
mod mac;
#[cfg(target_os = "macos")]
mod mac_embed;

use std::path::PathBuf;

pub use bridge::Capabilities;
pub use embed::{EmbedOptions, Placement, Rect};
pub use resolve::{Content, origin_host};
use serde_json::Value;

#[cfg(target_os = "macos")]
pub use mac::{ArtifactWindow, open_external_url, open_screen_share_url};

/// Schemes a machine's screen sharing address (`takeoverUrl`) may use.
pub const SCREEN_SHARE_SCHEMES: &[&str] = &["vnc", "http", "https"];

/// True when `url` is a screen sharing address the app may offer to open: `vnc://`,
/// `http://` or `https://` with a host, at most 2048 bytes, with no spaces or control
/// characters. Anything else (`file:`, `smb:`, other apps' schemes) is never opened.
pub fn is_screen_share_url(url: &str) -> bool {
    if url.is_empty() || url.len() > 2048 || url.chars().any(|c| c.is_whitespace() || c.is_control())
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

/// One version of a 产出 to show.
#[derive(Clone, Debug)]
pub struct VersionSpec {
    pub version: u32,
    pub content: Content,
    /// Relative path of the page to open, usually `index.html`.
    pub entry: String,
    /// JSON the page reads with `getData`.
    pub data: Value,
}

impl VersionSpec {
    pub fn new(version: u32, content: Content) -> Self {
        Self {
            version,
            content,
            entry: "index.html".into(),
            data: Value::Null,
        }
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
    /// The page called `emitEvent(name, payload)`. `user_action` is true when the viewer
    /// clicked, dragged or typed in this web view at most [`gate::USER_ACTION_WINDOW`]
    /// before; the host forwards only those events (see [`gate`]).
    Emit {
        version: u32,
        name: String,
        payload: Value,
        user_action: bool,
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
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::InvalidArtifactId => write!(f, "invalid artifact id"),
            Error::InvalidEntry => write!(f, "invalid entry path"),
            Error::FolderMissing(p) => write!(f, "artifact folder missing: {}", p.display()),
            Error::Closed => write!(f, "artifact window closed"),
            Error::NoHostView => write!(f, "host view is not in a window"),
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
