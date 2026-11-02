//! Files attached to messages: the timeline rows and image previews
//! ([`extra::AttachmentsExtra`]), and opening a file with the default app
//! or in Finder.
//!
//! Views never read the disk or the network here: the provider downloads a
//! file into its cache ([`ChatProvider::fetch_file`]) and the view opens the
//! path it gets back on the main thread.

pub mod extra;
pub mod preview;

use std::{path::Path, sync::Arc};

use gpui::{App, AsyncApp, Pixels, px};
use gpui_component::{WindowExt as _, notification::Notification};

use crate::{
    copy::{files as f, fill},
    data::{ChatProvider, FileAttachment, FileError, RejectedFile},
};

/// Largest inline preview box (§7.11: 360 x 320).
pub const PREVIEW_MAX_W: Pixels = px(360.);
pub const PREVIEW_MAX_H: Pixels = px(320.);
/// Smallest preview side, so a thin image stays clickable.
pub const PREVIEW_MIN: Pixels = px(48.);
/// Width of a file row in the timeline: the one measure (§4.4).
pub const FILE_ROW_W: Pixels = px(620.);
/// Height of a file row: 12 + 40 pt tile + 12 (fixed, so progress does not
/// change the row height).
pub const FILE_ROW_H: Pixels = px(64.);
/// Height of a 产出 or file row in the side panel (§7.11).
pub const PANEL_ROW_H: Pixels = px(48.);

/// A file name cut in the middle (§15): it keeps the last 12 characters and
/// the extension, and the start as far as `budget` (in pt at 14 pt text)
/// allows, joined by `…`.
pub fn middle_name(name: &str, budget: f32) -> String {
    let width = |c: char| if c.is_ascii() { 7.6 } else { 14.0 };
    let total: f32 = name.chars().map(width).sum();
    if total <= budget {
        return name.to_string();
    }
    let chars: Vec<char> = name.chars().collect();
    let tail_len = chars.len().min(12);
    let tail: String = chars[chars.len() - tail_len..].iter().collect();
    let mut room = budget - tail.chars().map(width).sum::<f32>() - 14.0;
    let mut head = String::new();
    for &c in &chars[..chars.len() - tail_len] {
        room -= width(c);
        if room < 0.0 {
            break;
        }
        head.push(c);
    }
    let head = head.trim_end_matches(|c: char| c.is_whitespace() || "-_.（(【[·".contains(c));
    format!("{head}…{tail}")
}
/// Images up to this size download by themselves for the inline preview.
pub const AUTO_DOWNLOAD_MAX_BYTES: u64 = 20_000_000;

/// "1.2 MB" for a byte count.
pub fn size_text(bytes: u64) -> String {
    const K: f64 = 1000.0;
    let b = bytes as f64;
    let one = |v: f64| {
        if v < 10.0 {
            format!("{v:.1}")
        } else {
            format!("{v:.0}")
        }
    };
    // MB and GB keep one decimal (`112.4 MB`), without a trailing `.0`.
    let mb = |v: f64| {
        let t = format!("{v:.1}");
        t.strip_suffix(".0").map(str::to_string).unwrap_or(t)
    };
    if b < K {
        fill(f::SIZE_B, &[("n", &bytes.to_string())])
    } else if b < K * K {
        fill(f::SIZE_KB, &[("n", &one(b / K))])
    } else if b < K * K * K {
        fill(f::SIZE_MB, &[("n", &mb(b / K / K))])
    } else {
        fill(f::SIZE_GB, &[("n", &mb(b / K / K / K))])
    }
}

/// The reason text for an error.
pub fn error_text(error: FileError) -> &'static str {
    match error {
        FileError::TooLarge => f::ERR_TOO_LARGE,
        FileError::NotAFile => f::ERR_NOT_A_FILE,
        FileError::Unreadable => f::ERR_UNREADABLE,
        FileError::Network => f::ERR_NETWORK,
        FileError::Checksum => f::ERR_CHECKSUM,
        FileError::Rejected => f::ERR_REJECTED,
        FileError::NotFound => f::ERR_NOT_FOUND,
        FileError::NoPage => f::ERR_NO_PAGE,
        FileError::SignedOut => f::ERR_SIGNED_OUT,
        FileError::Other => f::ERR_OTHER,
    }
}

/// The composer's line for a file it could not take.
pub fn rejected_text(r: &RejectedFile) -> String {
    match r.error {
        FileError::TooLarge if r.name.is_empty() => f::REJECT_IMAGE_TOO_LARGE.to_string(),
        FileError::TooLarge => fill(f::REJECT_TOO_LARGE, &[("name", &r.name)]),
        FileError::NotAFile => fill(f::REJECT_FOLDER, &[("name", &r.name)]),
        _ => fill(f::REJECT_UNREADABLE, &[("name", &r.name)]),
    }
}

/// Files that run code or open other places when opened. "Open" shows them
/// in Finder instead, so one click on a teammate's file never runs it.
pub fn opens_unsafely(name: &str) -> bool {
    const RISKY: &[&str] = &[
        "app",
        "command",
        "sh",
        "bash",
        "zsh",
        "csh",
        "ksh",
        "fish",
        "tool",
        "terminal",
        "pkg",
        "mpkg",
        "scpt",
        "scptd",
        "applescript",
        "workflow",
        "action",
        "jar",
        "py",
        "rb",
        "pl",
        "php",
        "webloc",
        "inetloc",
        "fileloc",
        "url",
        "shortcut",
        "dylib",
        "so",
        "bundle",
        "plugin",
        "kext",
        "prefpane",
        "osax",
        "saver",
        "qlgenerator",
        "mdimporter",
        "service",
        "xpc",
        "configprofile",
        "mobileconfig",
        "lnk",
        "exe",
        "bat",
        "cmd",
        "ps1",
        "vbs",
        "jnlp",
        "dmg",
        "iso",
    ];
    let lower = name.to_ascii_lowercase();
    let ext = Path::new(&lower)
        .extension()
        .map(|e| e.to_string_lossy().into_owned())
        .unwrap_or_default();
    // A name without an extension may be an executable script.
    ext.is_empty() || RISKY.contains(&ext.as_str())
}

/// An HTML file: it opens in the isolated 产出 window, not in a browser.
pub fn is_html(name: &str) -> bool {
    let l = name.to_ascii_lowercase();
    l.ends_with(".html") || l.ends_with(".htm")
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OpenAction {
    /// The default app (HTML in the 产出 window; risky files in Finder).
    Open,
    RevealInFinder,
}

/// Downloads the file when needed, then opens it or shows it in Finder.
pub fn open_file(
    provider: &Arc<dyn ChatProvider>,
    group_id: &str,
    topic_id: &str,
    file: &FileAttachment,
    action: OpenAction,
    cx: &mut App,
) {
    let rx = provider.fetch_file(group_id, topic_id, file);
    let file = file.clone();
    cx.spawn(async move |cx: &mut AsyncApp| {
        let result = rx.await.unwrap_or(Err(FileError::Other));
        cx.update(|cx| match result {
            Ok(path) => match action {
                OpenAction::RevealInFinder => cx.reveal_path(&path),
                OpenAction::Open if is_html(&file.name) => {
                    crate::views::artifacts::window::open_html_file(&file, &path, cx)
                }
                OpenAction::Open if opens_unsafely(&file.name) => cx.reveal_path(&path),
                OpenAction::Open => cx.open_with_system(&path),
            },
            Err(error) => notify_error(
                fill(
                    f::OPEN_FAILED,
                    &[("name", &file.name), ("reason", error_text(error))],
                ),
                cx,
            ),
        })
    })
    .detach();
}

/// Shows an error in the active window (or the first window).
pub fn notify_error(message: String, cx: &mut App) {
    notify(Notification::error(message), cx);
}

pub fn notify(note: Notification, cx: &mut App) {
    let target = cx
        .active_window()
        .or_else(|| cx.windows().into_iter().next());
    if let Some(window) = target {
        let _ = window.update(cx, |_, window, cx| window.push_notification(note, cx));
    } else {
        tracing::info!("no window for a notification");
    }
}

/// The size of the inline preview box of an image: its aspect ratio inside
/// [`PREVIEW_MAX_W`] x [`PREVIEW_MAX_H`].
pub fn preview_size(width: Option<u32>, height: Option<u32>) -> (Pixels, Pixels) {
    let (max_w, max_h) = (f32::from(PREVIEW_MAX_W), f32::from(PREVIEW_MAX_H));
    let min = f32::from(PREVIEW_MIN);
    match (width, height) {
        (Some(w), Some(h)) if w > 0 && h > 0 => {
            let (w, h) = (w as f32, h as f32);
            let scale = (max_w / w).min(max_h / h).min(1.0);
            (px((w * scale).max(min)), px((h * scale).max(min)))
        }
        _ => (PREVIEW_MAX_W * 0.66, PREVIEW_MAX_H * 0.66),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sizes_read_well() {
        assert_eq!(size_text(999), "999 B");
        assert_eq!(size_text(1_500), "1.5 KB");
        assert_eq!(size_text(25_000_000), "25 MB");
        assert_eq!(size_text(100_000_000), "100 MB");
        assert_eq!(size_text(112_400_000), "112.4 MB");
        assert_eq!(size_text(25_600_000), "25.6 MB");
        assert_eq!(size_text(2_400_000_000), "2.4 GB");
    }

    #[test]
    fn risky_files_are_not_opened() {
        for name in [
            "run.command",
            "Installer.APP",
            "x.sh",
            "Makefile",
            "a.webloc",
        ] {
            assert!(opens_unsafely(name), "{name}");
        }
        for name in ["报告.pdf", "photo.JPG", "notes.md", "data.csv", "a.zip"] {
            assert!(!opens_unsafely(name), "{name}");
        }
        assert!(is_html("Index.HTM"));
    }

    #[test]
    fn long_names_keep_their_end() {
        let n = "websocket-reconnect-trace-2025-11-03-build-4412.log";
        let cut = middle_name(n, 300.);
        assert!(cut.ends_with("ld-4412.log"), "{cut}");
        assert!(cut.contains('…'));
        assert_eq!(middle_name("a.pdf", 300.), "a.pdf");
    }

    #[test]
    fn previews_keep_the_aspect_ratio() {
        let (w, h) = preview_size(Some(4000), Some(3000));
        assert_eq!((f32::from(w), f32::from(h)), (360., 270.));
        let (w, h) = preview_size(Some(100), Some(50));
        assert_eq!((f32::from(w), f32::from(h)), (100., 50.));
        let (w, h) = preview_size(Some(2000), Some(10));
        assert_eq!((f32::from(w), f32::from(h)), (360., 48.));
    }
}
