//! Space on a file off the Mac. macOS has the Quick Look panel: an instant preview that
//! does not leave the app. Windows has no such panel, so the app shows the file itself, in
//! the viewer it already has for 产出 ([`crate::views::media::viewer`]: image, video,
//! audio, PDF, Markdown, text and code, table), as a sheet on the window the file is in.
//! Space again or Esc closes it, as in Quick Look.
//!
//! A 产出 opens in its own viewer (its page runs in the isolated view). An attachment is
//! read from its local copy: the viewer only decodes or prints it, so even a script
//! previews safely as text. Only a file no viewer shows, and an attached page (it could
//! run only in the 产出 view), go on to the system, behind the same guard as 打开
//! ([`super::native::preview_opens`]): what Windows would run is shown in Explorer.
#![cfg_attr(target_os = "macos", allow(dead_code))]

use std::{cell::RefCell, path::Path};

use gpui::{App, WeakEntity, Window};
use gpui_component::WindowExt as _;

use super::export::{FileRef, Local};
use crate::views::media::{
    self, MediaKind,
    state::Target,
    store::{self, MediaKey},
    viewer::{self, MediaViewer, Opening},
};

thread_local! {
    /// The preview sheet that is open, if one is.
    static OPEN: RefCell<Option<(gpui::AnyWindowHandle, WeakEntity<MediaViewer>)>> =
        const { RefCell::new(None) };
}

/// Closes the preview when it is open (Space toggles it). False when none is open.
pub fn close_if_open(cx: &mut App) -> bool {
    let Some((handle, viewer)) = OPEN.with_borrow_mut(Option::take) else {
        return false;
    };
    // The sheet was closed another way (Esc, its close button): nothing to toggle.
    if viewer.upgrade().is_none() {
        return false;
    }
    // Space arrives while its window is being updated: close on a later turn.
    cx.defer(move |cx| {
        let _ = handle.update(cx, |_, window, cx| {
            if window.has_active_dialog(cx) {
                window.close_dialog(cx);
            }
        });
    });
    true
}

/// What previews an attachment whose local copy is `path`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Preview {
    /// The app's viewer for this kind.
    Viewer(MediaKind),
    /// No viewer shows it: the system does, behind the guard of 打开.
    System,
}

/// Decides by the name on disk (what is read), never by the name the sender gave.
pub fn preview_of(path: &Path) -> Preview {
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    match media::file_kind(&name, media::mime_for(&name)) {
        // A page needs the isolated 产出 view; a plain file has no viewer.
        MediaKind::Html | MediaKind::File => Preview::System,
        kind => Preview::Viewer(kind),
    }
}

/// The store key of an attachment's preview: one per file content. No characters a
/// folder name cannot have (generated pictures go to a folder named after it).
fn attachment_key(sha256: &str, path: &Path) -> MediaKey {
    let id = if sha256.is_empty() {
        use std::hash::{Hash as _, Hasher as _};
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        path.hash(&mut hasher);
        format!("file-{:016x}", hasher.finish())
    } else {
        format!("file-{sha256}")
    };
    MediaKey::new(id, 0)
}

/// Previews `target`, whose local copy is `local`.
pub fn show(target: &FileRef, local: Local, cx: &mut App) {
    match target {
        FileRef::Artifact {
            provider,
            id,
            title,
        } => {
            let Some(detail) = provider.artifact(id) else {
                return system_preview(&local.preview);
            };
            let key = MediaKey::new(id.clone(), detail.summary.version);
            let target = viewer::target_for(provider, id);
            open_sheet(key, title.clone(), target, cx);
        }
        FileRef::Attachment { file, .. } => {
            match request_attachment(&file.name, &file.sha256, &local.preview, cx) {
                Some(key) => open_sheet(key, file.name.clone(), Target::none(), cx),
                None => system_preview(&local.preview),
            }
        }
    }
}

/// Starts reading the attachment at `path` for the viewer. `None` when no viewer shows it.
pub fn request_attachment(name: &str, sha256: &str, path: &Path, cx: &mut App) -> Option<MediaKey> {
    let Preview::Viewer(_) = preview_of(path) else {
        return None;
    };
    let key = attachment_key(sha256, path);
    let (dir, entry) = stage(&key.artifact_id, path)?;
    store::request_folder(&key, name, dir, entry, cx);
    Some(key)
}

/// A folder that holds only this file, for the viewer to read: the viewer takes a folder
/// as one 产出 version and shows every picture in it as a set, and the cache keeps the
/// same content under every name it was sent with in one folder. A hard link (a copy
/// when the temp folder is on another volume), made once per content.
fn stage(key: &str, path: &Path) -> Option<(std::path::PathBuf, String)> {
    stage_in(&std::env::temp_dir().join("tc-quick-look"), key, path)
}

fn stage_in(root: &Path, key: &str, path: &Path) -> Option<(std::path::PathBuf, String)> {
    let name = path.file_name()?;
    let dir = root.join(key);
    let staged = dir.join(name);
    if !staged.is_file() {
        // Only this file: an older name of the same content goes.
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).ok()?;
        if std::fs::hard_link(path, &staged).is_err() {
            std::fs::copy(path, &staged).ok()?;
        }
    }
    Some((dir, name.to_string_lossy().into_owned()))
}

/// Opens the viewer sheet on the active window, after the current update.
fn open_sheet(key: MediaKey, title: String, target: Target, cx: &mut App) {
    let Some(handle) = cx
        .active_window()
        .or_else(|| cx.windows().into_iter().next())
    else {
        return;
    };
    cx.defer(move |cx| {
        let _ = handle.update(cx, |_, window, cx| {
            open_sheet_in(window, key, title, target, cx);
        });
    });
}

/// Opens the viewer sheet on `window` and remembers it for the Space toggle.
pub fn open_sheet_in(
    window: &mut Window,
    key: MediaKey,
    title: String,
    target: Target,
    cx: &mut App,
) {
    let viewer = viewer::open_in(window, key, title, target, Opening::default(), cx);
    OPEN.with_borrow_mut(|open| *open = Some((window.window_handle(), viewer.downgrade())));
}

/// A file no viewer shows: its default app, or Explorer when opening it is not safe.
fn system_preview(path: &Path) {
    #[cfg(target_os = "windows")]
    super::native::system_preview(path);
    #[cfg(not(target_os = "windows"))]
    let _ = path;
}

#[cfg(test)]
mod tests {
    use super::*;

    /// What the app can show previews in the app; a script is shown as text, never run; a
    /// page and an unknown type go to the system's guard.
    #[test]
    fn files_the_app_can_show_preview_in_the_app() {
        let p = |n: &str| preview_of(Path::new(&format!("/cache/abc/{n}")));
        assert_eq!(p("photo.PNG"), Preview::Viewer(MediaKind::Image));
        assert_eq!(p("clip.mp4"), Preview::Viewer(MediaKind::Video));
        assert_eq!(p("talk.m4a"), Preview::Viewer(MediaKind::Audio));
        assert_eq!(p("a.pdf"), Preview::Viewer(MediaKind::Pdf));
        assert_eq!(p("notes.md"), Preview::Viewer(MediaKind::Markdown));
        assert_eq!(p("data.csv"), Preview::Viewer(MediaKind::Table));
        assert_eq!(p("main.rs"), Preview::Viewer(MediaKind::Code));
        assert_eq!(p("notes.txt"), Preview::Viewer(MediaKind::Code));
        for name in [
            "page.html",
            "page.HTM",
            "setup.exe",
            "deck.pptx",
            "a.msi",
            "noext",
        ] {
            assert_eq!(p(name), Preview::System, "{name}");
        }
        assert_eq!(preview_of(Path::new("/")), Preview::System);
    }

    #[test]
    fn a_preview_key_is_a_folder_name() {
        let key = attachment_key(&"a".repeat(64), Path::new("/x/y.png"));
        assert_eq!(key.artifact_id, format!("file-{}", "a".repeat(64)));
        let unnamed = attachment_key("", Path::new(r"C:\cache\y.png"));
        assert!(unnamed.artifact_id.starts_with("file-"));
        for key in [key, unnamed] {
            assert!(
                key.artifact_id
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '-'),
                "{}",
                key.artifact_id
            );
        }
        assert_ne!(
            attachment_key("", Path::new("/a.png")),
            attachment_key("", Path::new("/b.png"))
        );
    }

    /// The cache keeps one content under every name it was sent with in one folder; the
    /// viewer gets a folder with the one file, so one picture never shows as a set of two.
    #[test]
    fn the_viewer_reads_a_folder_with_only_the_file() {
        let root = tempfile::tempdir().unwrap();
        let cache = root.path().join("cache");
        std::fs::create_dir_all(&cache).unwrap();
        std::fs::write(cache.join("a.png"), b"png").unwrap();
        std::fs::write(cache.join("b.png"), b"png").unwrap();
        let stage_root = root.path().join("stage");
        let (dir, entry) = stage_in(&stage_root, "file-abc", &cache.join("a.png")).unwrap();
        assert_eq!(entry, "a.png");
        let names = |dir: &Path| {
            let mut names: Vec<String> = std::fs::read_dir(dir)
                .unwrap()
                .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
                .collect();
            names.sort();
            names
        };
        assert_eq!(names(&dir), ["a.png"]);
        assert_eq!(std::fs::read(dir.join("a.png")).unwrap(), b"png");
        // The same content under its other name replaces it.
        let (dir, entry) = stage_in(&stage_root, "file-abc", &cache.join("b.png")).unwrap();
        assert_eq!(
            (names(&dir), entry.as_str()),
            (vec!["b.png".to_string()], "b.png")
        );
        assert!(stage_in(&stage_root, "file-x", &cache.join("gone.png")).is_none());
    }

    #[test]
    fn nothing_closes_when_no_preview_is_open() {
        // No app is needed to say so: the slot is empty.
        assert!(OPEN.with_borrow(|open| open.is_none()));
    }
}
