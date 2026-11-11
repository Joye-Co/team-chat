//! Space on a file off the Mac. macOS has the Quick Look panel: an instant preview that
//! does not leave the app. Windows has no such panel, so the app shows the file itself, in
//! the viewer it already has for 产出 ([`crate::views::media::viewer`]: image, video,
//! audio, PDF, Markdown, text and code, table), as a sheet on the window the file is in.
//! Space again or Esc closes it, as in Quick Look.
//!
//! A 产出 opens in its own viewer (its page runs in the isolated view). An attachment is
//! read from its local copy: the viewer only decodes or prints it, so even a script
//! previews safely as text, and an attached page runs in the isolated 产出 view.
//!
//! A type the app has no viewer for (Office documents, archives, …) has no preview: the
//! sheet says so and offers 用系统应用打开, behind the guard of 打开. This is narrower than
//! Quick Look, on purpose. macOS previews any file in a sandboxed system service; the
//! Windows counterpart, a shell preview handler, is whatever program
//! registered for the type, running outside any sandbox the app controls, and it can
//! make Office fetch a remote template or linked object. File Explorer itself no longer
//! previews files that carry the Mark of the Web for that reason. Every file this sheet
//! can show comes from the app's download cache ([`chat_core`] marks each one as from
//! the internet, also the copies of files the person sent) or is a 产出 version, so the
//! app follows Explorer: none is ever handed to a system preview handler, and the app
//! hosts none. Opened with its program instead, an Office file opens in Protected View.
//!
//! A file Windows would run is never previewed: it is shown in Explorer.
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
    /// The app's preview sheet: the viewer for this kind, or for a plain file the "no
    /// preview" state with 用系统应用打开.
    Viewer(MediaKind),
    /// A file Windows would run: never previewed, only shown in Explorer.
    Reveal,
}

/// Decides by the name on disk (what is read), never by the name the sender gave.
///
/// A page previews in the isolated 产出 view (the viewer's HTML view: no network, no
/// bridge capabilities), as 打开 shows it in the 产出 window and as Quick Look renders it;
/// its source would tell the reader less. A plain file gets the sheet only when 打开
/// would hand it to the system: a program, a script host's file or a shortcut is shown in
/// Explorer and nothing else.
pub fn preview_of(path: &Path) -> Preview {
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    match media::file_kind(&name, media::mime_for(&name)) {
        MediaKind::File if !media::parts::opens_with_system(path) => Preview::Reveal,
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

/// Previews `target`, whose local copy is `local`: always the sheet, except for a file
/// Windows would run, which is shown in Explorer. Space never starts another program.
pub fn show(target: &FileRef, local: Local, cx: &mut App) {
    match target {
        FileRef::Artifact {
            provider,
            id,
            title,
        } => {
            // A 产出 the provider no longer knows still gets the sheet: its viewer says
            // that the version could not be read.
            let version = provider.artifact(id).map_or(0, |d| d.summary.version);
            let key = MediaKey::new(id.clone(), version);
            let target = viewer::target_for(provider, id);
            open_sheet(key, title.clone(), target, cx);
        }
        FileRef::Attachment { file, .. } => {
            if preview_of(&local.preview) == Preview::Reveal {
                return reveal(&local.preview);
            }
            let key = request_attachment(&file.name, &file.sha256, &local.preview, cx);
            open_sheet(key, file.name.clone(), Target::none(), cx);
        }
    }
}

/// Starts reading the attachment at `path` for the viewer and returns its key in the
/// store. When the file cannot be put in a folder of its own (the temp folder is full or
/// gone) the store gets a failure for the key, so the sheet opens on its "could not be
/// read" state instead of nothing.
pub fn request_attachment(name: &str, sha256: &str, path: &Path, cx: &mut App) -> MediaKey {
    let key = attachment_key(sha256, path);
    match stage(&key.artifact_id, path) {
        Some((dir, entry)) => store::request_folder(&key, name, dir, entry, cx),
        None => store::fail(&key, crate::data::FileError::Unreadable, cx),
    }
    key
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

/// A file that is never previewed: selected in Explorer, nothing else.
fn reveal(path: &Path) {
    #[cfg(target_os = "windows")]
    super::native::reveal(path);
    #[cfg(not(target_os = "windows"))]
    let _ = path;
}

#[cfg(test)]
mod tests {
    use super::*;

    /// What the app can show previews in the app; a script is shown as text, never run; a
    /// page runs in the isolated view; a type without a viewer gets the sheet's "no
    /// preview" state; what the system would run is never previewed.
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
        // A script is text to the viewer.
        assert_eq!(p("run.sh"), Preview::Viewer(MediaKind::Code));
        // What only Windows runs is only kept from a preview there.
        if cfg!(target_os = "windows") {
            assert_eq!(p("a.msi"), Preview::Reveal);
        }
        assert_eq!(p("notes.txt"), Preview::Viewer(MediaKind::Code));
        assert_eq!(p("page.html"), Preview::Viewer(MediaKind::Html));
        assert_eq!(p("page.HTM"), Preview::Viewer(MediaKind::Html));
        for name in [
            "deck.pptx",
            "report.docx",
            "sheet.xlsx",
            "letter.rtf",
            "a.zip",
        ] {
            assert_eq!(p(name), Preview::Viewer(MediaKind::File), "{name}");
        }
        for name in ["setup.exe", "noext", "a.lnk", "tool.command"] {
            assert_eq!(p(name), Preview::Reveal, "{name}");
        }
        assert_eq!(preview_of(Path::new("/")), Preview::Reveal);
    }

    /// The app hosts no system preview handler: a downloaded file (every file this sheet
    /// shows carries the Mark of the Web) is never given to one, as in File Explorer.
    /// The source has no such call, and the types a handler would serve are plain files
    /// to the sheet.
    #[test]
    fn no_file_is_handed_to_a_system_preview_handler() {
        let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let (handler, clsid) = (
            ["IPreview", "Handler"].concat(),
            ["8895b1c6", "-b41f-4c1c-a562-0d564250836f"].concat(),
        );
        let mut stack = vec![src];
        while let Some(dir) = stack.pop() {
            for entry in std::fs::read_dir(&dir).unwrap().flatten() {
                let path = entry.path();
                if path.is_dir() {
                    stack.push(path);
                } else if path.extension().is_some_and(|e| e == "rs") {
                    let text = std::fs::read_to_string(&path).unwrap_or_default();
                    assert!(
                        !text.contains(&handler) && !text.to_lowercase().contains(&clsid),
                        "{} talks to a preview handler",
                        path.display()
                    );
                }
            }
        }
        // Space never starts another program either: the preview code has no call that
        // opens a file with the system.
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/views/files");
        for file in ["quick_look.rs", "native.rs"] {
            let text = std::fs::read_to_string(dir.join(file)).unwrap();
            let code = text.split("#[cfg(test)]").next().unwrap();
            for opener in [
                ["open_with", "_system"].concat(),
                ["shell", "_open"].concat(),
            ] {
                assert!(!code.contains(&opener), "{file} calls {opener}");
            }
        }
        for name in [
            "a.docx", "a.doc", "a.xlsx", "a.pptx", "a.rtf", "a.eml", "a.ttf",
        ] {
            let path = Path::new("/cache/abc").join(name);
            assert_eq!(
                preview_of(&path),
                Preview::Viewer(MediaKind::File),
                "{name}"
            );
        }
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
