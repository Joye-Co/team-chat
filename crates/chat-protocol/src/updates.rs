//! App updates and DMG downloads (docs/protocol.md, "Updates and downloads").

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
}

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
