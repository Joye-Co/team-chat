//! Where the app keeps its files on each system. macOS paths are the ones the app has
//! always used; Windows uses the roaming and local app-data folders.

use std::path::PathBuf;

fn var(name: &str) -> Option<PathBuf> {
    std::env::var_os(name)
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
}

/// The user's home folder: `HOME`, or `USERPROFILE` on Windows.
pub fn home() -> Option<PathBuf> {
    if cfg!(windows) {
        var("USERPROFILE").or_else(|| var("HOME"))
    } else {
        var("HOME")
    }
}

/// The parent of the app's data folder: `~/Library/Application Support`, or `%APPDATA%`
/// on Windows. The temp folder when the system names neither.
pub fn data_base() -> PathBuf {
    let base = if cfg!(windows) {
        var("APPDATA")
    } else {
        home().map(|h| h.join("Library/Application Support"))
    };
    base.unwrap_or_else(std::env::temp_dir)
}

/// The parent of the app's caches: `~/Library/Caches`, or `%LOCALAPPDATA%` on Windows.
pub fn cache_base() -> PathBuf {
    let base = if cfg!(windows) {
        var("LOCALAPPDATA")
    } else {
        home().map(|h| h.join("Library").join("Caches"))
    };
    base.unwrap_or_else(std::env::temp_dir)
}

/// The user's Downloads folder.
pub fn downloads() -> Option<PathBuf> {
    home().map(|h| h.join("Downloads"))
}
