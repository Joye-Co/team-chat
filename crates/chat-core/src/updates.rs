//! In-app update: check the newest release for this platform, download its file (the DMG,
//! or the Windows installer) with progress and check its SHA-256.

use crate::files::{ProgressFn, TRANSFER_TIMEOUT, stream_to_file};
use crate::http::{ApiError, ApiResult, HttpClient};
use crate::proto::{PLATFORM_MACOS, PLATFORM_WINDOWS, UpdateInfo, parse_release_version};
use reqwest::Method;
use std::path::Path;

/// True when `candidate` is a higher release version than `current`
/// (`MAJOR.MINOR.PATCH`, compared as numbers). A malformed `current` counts as older.
pub fn is_newer(candidate: &str, current: &str) -> bool {
    match (
        parse_release_version(candidate),
        parse_release_version(current),
    ) {
        (Some(c), Some(cur)) => c > cur,
        (Some(_), None) => true,
        _ => false,
    }
}

/// The release platform of this build (`UpdateInfo.platform`).
pub const PLATFORM: &str = if cfg!(windows) {
    PLATFORM_WINDOWS
} else {
    PLATFORM_MACOS
};

/// The newest release for this build's platform when it is newer than `current_version`.
pub async fn check(http: &HttpClient, current_version: &str) -> ApiResult<Option<UpdateInfo>> {
    check_for(http, current_version, PLATFORM).await
}

/// [`check`] for a named platform. A release for another platform is never offered: a
/// server from before the Windows client ignores the platform it is asked for and answers
/// with the DMG, which carries no `platform`.
pub async fn check_for(
    http: &HttpClient,
    current_version: &str,
    platform: &str,
) -> ApiResult<Option<UpdateInfo>> {
    let Some(info) = http.latest_update(platform).await? else {
        return Ok(None);
    };
    info.validate()
        .map_err(|e| ApiError::Decode(e.to_string()))?;
    if info.platform() != platform {
        tracing::info!(
            offered = info.platform(),
            wanted = platform,
            "the server has no release for this platform"
        );
        return Ok(None);
    }
    Ok(is_newer(&info.version, current_version).then_some(info))
}

/// Downloads the file of `info` to `dest` and checks size and SHA-256. The bytes go
/// to `<dest>.part` first; `dest` appears only when they match. `downloadUrl` must be
/// on the server origin (it carries the token), and `info` must be for this build's
/// platform.
pub async fn download(
    http: &HttpClient,
    info: &UpdateInfo,
    dest: &Path,
    progress: &ProgressFn,
) -> ApiResult<()> {
    download_for(http, info, dest, progress, PLATFORM).await
}

/// [`download`] for a named platform.
pub async fn download_for(
    http: &HttpClient,
    info: &UpdateInfo,
    dest: &Path,
    progress: &ProgressFn,
    platform: &str,
) -> ApiResult<()> {
    if info.platform() != platform {
        return Err(ApiError::Decode(format!(
            "the update is for {}, not {platform}",
            info.platform()
        )));
    }
    if !http.same_origin(&info.download_url) {
        return Err(ApiError::BadUrl);
    }
    info.validate()
        .map_err(|e| ApiError::Decode(e.to_string()))?;
    if let Some(p) = dest.parent() {
        tokio::fs::create_dir_all(p).await?;
    }
    let mut part = dest.as_os_str().to_owned();
    part.push(".part");
    let part = std::path::PathBuf::from(part);
    let resp = http
        .send_checked(
            http.authed(Method::GET, &info.download_url)?
                .timeout(TRANSFER_TIMEOUT),
        )
        .await?;
    if let Err(e) = stream_to_file(resp, &part, info.size, Some(&info.sha256), progress).await {
        let _ = tokio::fs::remove_file(&part).await;
        return Err(e);
    }
    tokio::fs::rename(&part, dest).await?;
    Ok(())
}
