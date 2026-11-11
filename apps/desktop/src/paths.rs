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

/// The user's Downloads folder. On Windows it is the folder 下载 points at now (a person
/// can move it to another drive in its properties), not always `%USERPROFILE%\Downloads`.
pub fn downloads() -> Option<PathBuf> {
    #[cfg(windows)]
    if let Some(folder) = known_downloads() {
        return Some(folder);
    }
    home().map(|h| h.join("Downloads"))
}

#[cfg(windows)]
fn known_downloads() -> Option<PathBuf> {
    use std::os::windows::ffi::OsStringExt as _;
    use windows::Win32::{
        System::Com::CoTaskMemFree,
        UI::Shell::{FOLDERID_Downloads, KF_FLAG_DEFAULT, SHGetKnownFolderPath},
    };
    // SAFETY: the shell returns a NUL-terminated path it allocated, freed here.
    unsafe {
        let path = SHGetKnownFolderPath(&FOLDERID_Downloads, KF_FLAG_DEFAULT, None).ok()?;
        let folder = PathBuf::from(std::ffi::OsString::from_wide(path.as_wide()));
        CoTaskMemFree(Some(path.as_ptr().cast()));
        (!folder.as_os_str().is_empty()).then_some(folder)
    }
}
