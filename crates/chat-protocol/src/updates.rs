//! App updates and downloads of the DMG, the Windows installer and the Linux daemon (docs/protocol.md, "Updates and downloads").

use serde::{Deserialize, Serialize};

use crate::entities::Millis;

/// `GET /v1/updates/latest` and the response of `PUT /v1/admin/releases/:version`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateInfo {
    /// `MAJOR.MINOR.PATCH`.
    pub version: String,
    /// Release notes, Markdown.
    pub notes: String,
    pub size: u64,
    pub sha256: String,
    pub published_at: Millis,
    /// Absolute URL of `GET /v1/updates/:version/download`.
    pub download_url: String,
    /// What the file runs on: `"macos"` (the DMG), `"windows"` (the installer) or
    /// `"linux-x86_64"` (the machine daemon alone). Missing
    /// means `"macos"`: servers older than the Windows client never send it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub platform: Option<String>,
}

/// `UpdateInfo.platform` of the DMG.
pub const PLATFORM_MACOS: &str = "macos";
/// `UpdateInfo.platform` of the Windows installer.
pub const PLATFORM_WINDOWS: &str = "windows";
/// `UpdateInfo.platform` of the machine daemon alone for x86_64 Linux (glibc): what a Linux
/// host installs and what its daemon updates itself from (`GET /v1/machine/updates/latest`).
pub const PLATFORM_LINUX_X86_64: &str = "linux-x86_64";

/// True for `MAJOR.MINOR.PATCH` without leading zeros.
pub fn is_release_version(v: &str) -> bool {
    let parts: Vec<&str> = v.split('.').collect();
    parts.len() == 3
        && parts.iter().all(|p| {
            !p.is_empty()
                && p.bytes().all(|b| b.is_ascii_digit())
                && (p.len() == 1 || !p.starts_with('0'))
        })
}

/// Parses a release version into a comparable triple.
pub fn parse_release_version(v: &str) -> Option<(u64, u64, u64)> {
    if !is_release_version(v) {
        return None;
    }
    let mut it = v.split('.').map(|p| p.parse::<u64>().ok());
    Some((it.next()??, it.next()??, it.next()??))
}

impl UpdateInfo {
    /// The platform of the file; an answer without one is the DMG.
    pub fn platform(&self) -> &str {
        self.platform.as_deref().unwrap_or(PLATFORM_MACOS)
    }

    pub fn validate(&self) -> crate::Validation {
        if !is_release_version(&self.version) {
            return Err(crate::ValidationError {
                field: "version",
                message: "must be MAJOR.MINOR.PATCH".into(),
            });
        }
        crate::check_chars("notes", &self.notes, 0, 20_000)?;
        crate::check_sha256("sha256", &self.sha256)
    }
}
