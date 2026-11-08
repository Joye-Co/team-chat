//! Files attached to messages: the timeline rows and image previews
//! ([`extra::AttachmentsExtra`]), and opening a file with the default app
//! or in Finder.
//!
//! Views never read the disk or the network here: the provider downloads a
//! file into its cache ([`ChatProvider::fetch_file`]) and the view opens the
//! path it gets back on the main thread.

pub mod export;
pub mod extra;
pub mod native;
pub mod preview;

use std::{path::Path, sync::Arc};

use gpui::{App, AsyncApp, Pixels, px};
use gpui_component::{WindowExt as _, notification::Notification};

use crate::{
    copy::{files as f, fill},
    data::{ChatProvider, FileAttachment, FileError, RejectedFile},
};

/// Inline image previews (§2.4, R2-10): one image takes the one in-thread
/// width (640); two or more sit in 2 columns of 312, gap 16. Heights follow
/// the aspect ratio between these bounds; the picture covers its box.
pub const PREVIEW_MAX_W: Pixels = crate::views::timeline::v2::TEXT_MAX;
pub const PREVIEW_HALF_W: Pixels = px(312.);
pub const PREVIEW_MAX_H: Pixels = px(360.);
/// The gap between 2 image columns.
pub const PREVIEW_GAP: Pixels = px(16.);
/// Smallest preview side, so a thin image stays clickable.
pub const PREVIEW_MIN: Pixels = px(48.);
/// Width of a file row in the timeline: the one in-thread width (§2.4).
pub const FILE_ROW_W: Pixels = crate::views::timeline::v2::TEXT_MAX;
/// Height of a file item (§2.6 ListRow): 12 + two lines + the 4 pt bar room + 12 (fixed, so
/// progress does not change the row height).
pub const FILE_ROW_H: Pixels = px(60.);
/// Height of a 产出 or file row in the side panel (§7.11).
pub const PANEL_ROW_H: Pixels = px(48.);

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
        // Web documents that a browser runs with scripts and network, and
        // that the isolated 产出 window cannot show.
        "webarchive",
        "mht",
        "mhtml",
        "svgz",
        "xml",
        "xsl",
        "xslt",
        "shtml",
    ];
    let lower = name.to_ascii_lowercase();
    let ext = Path::new(&lower)
        .extension()
        .map(|e| e.to_string_lossy().into_owned())
        .unwrap_or_default();
    // A name without an extension may be an executable script.
    ext.is_empty() || RISKY.contains(&ext.as_str())
}

/// A page that can run scripts (HTML, XHTML, SVG): it opens in the isolated
/// 产出 window, never in a browser.
pub fn is_html(name: &str) -> bool {
    let l = name.to_ascii_lowercase();
    [".html", ".htm", ".xhtml", ".xht", ".svg"]
        .iter()
        .any(|ext| l.ends_with(ext))
}

/// How an attachment opens.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OpenKind {
    /// In the isolated 产出 window.
    Page,
    /// With the default macOS app.
    System,
    /// Shown in Finder only.
    Reveal,
}

/// Decides from both the attachment's name and the name of the path that opens (macOS
/// picks the app by the path), so a safe name can never open a file with a risky name.
pub fn open_kind(name: &str, path: &Path) -> OpenKind {
    let on_disk = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    if is_html(name) && is_html(&on_disk) {
        OpenKind::Page
    } else if opens_unsafely(name) || opens_unsafely(&on_disk) || is_html(name) || is_html(&on_disk)
    {
        OpenKind::Reveal
    } else {
        OpenKind::System
    }
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
            Ok(path) => match (action, open_kind(&file.name, &path)) {
                (OpenAction::RevealInFinder, _) | (OpenAction::Open, OpenKind::Reveal) => {
                    cx.reveal_path(&path)
                }
                (OpenAction::Open, OpenKind::Page) => {
                    crate::views::artifacts::window::open_html_file(&file, &path, cx)
                }
                (OpenAction::Open, OpenKind::System) => cx.open_with_system(&path),
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

/// Shows an error in the active window (or the first window) as a Tahoe
/// alert (NATIVE.md §7.2, §7.4: errors that block use an alert, never a
/// toast): the app icon, the message and one `好` button (Return or Escape
/// close it). When the window already shows a sheet or an alert, the error
/// goes to the log instead of stacking a second one.
pub fn notify_error(message: String, cx: &mut App) {
    use crate::ui::native::{
        AlertLayout, PushStyle, alert_button, metrics,
        overlays::{app_icon, native_alert},
    };
    use gpui::ParentElement as _;
    let target = cx
        .active_window()
        .or_else(|| cx.windows().into_iter().next());
    let Some(window) = target else {
        tracing::info!("no window for an error alert");
        return;
    };
    let _ = window.update(cx, move |_, window, cx| {
        if window.has_active_dialog(cx) {
            tracing::info!("an error came while a dialog was open");
            return;
        }
        let message: gpui::SharedString = message.into();
        window.open_dialog(cx, move |dialog, _, cx| {
            native_alert(dialog, cx).on_ok(|_, _, _| true).child(
                AlertLayout::new(message.clone())
                    .icon(app_icon(metrics::ALERT_ICON))
                    .buttons([alert_button(
                        "files-error-ok",
                        f::ALERT_OK,
                        PushStyle::Default,
                        false,
                        cx,
                    )
                    .on_click(|_, window, cx| window.close_dialog(cx))]),
            )
        });
    });
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

/// The inline preview box of an image (§2.4): the full in-thread width for
/// one image, a 312 column when the message has more; the height from the
/// aspect ratio (4:3 when unknown), between [`PREVIEW_MIN`] x 2 and
/// [`PREVIEW_MAX_H`] (a half column at most 3:4 tall).
pub fn preview_size(width: Option<u32>, height: Option<u32>, images: usize) -> (Pixels, Pixels) {
    let w = if images > 1 {
        PREVIEW_HALF_W
    } else {
        PREVIEW_MAX_W
    };
    let ratio = match (width, height) {
        (Some(w), Some(h)) if w > 0 && h > 0 => h as f32 / w as f32,
        _ => 0.75,
    };
    let max_h = if images > 1 {
        f32::from(PREVIEW_HALF_W) * 4. / 3.
    } else {
        f32::from(PREVIEW_MAX_H)
    };
    let h = (f32::from(w) * ratio).clamp(f32::from(PREVIEW_MIN) * 2., max_h);
    (w, px(h.round()))
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
    fn the_open_decision_reads_both_names() {
        let p = |n: &str| std::path::PathBuf::from(format!("/cache/abc/{n}"));
        assert_eq!(open_kind("notes.txt", &p("notes.txt")), OpenKind::System);
        // A safe name that would open a cached copy with a risky name is only revealed.
        assert_eq!(open_kind("notes.txt", &p("a.terminal")), OpenKind::Reveal);
        assert_eq!(open_kind("a.terminal", &p("notes.txt")), OpenKind::Reveal);
        assert_eq!(open_kind("page.html", &p("page.html")), OpenKind::Page);
        assert_eq!(open_kind("notes.txt", &p("page.html")), OpenKind::Reveal);
        assert_eq!(open_kind("page.html", &p("notes.txt")), OpenKind::Reveal);
    }

    #[test]
    fn risky_files_are_not_opened() {
        for name in [
            "run.command",
            "Installer.APP",
            "x.sh",
            "Makefile",
            "a.webloc",
            "page.webarchive",
            "page.MHT",
            "feed.xml",
        ] {
            assert!(opens_unsafely(name), "{name}");
        }
        for name in ["报告.pdf", "photo.JPG", "notes.md", "data.csv", "a.zip"] {
            assert!(!opens_unsafely(name), "{name}");
        }
        assert!(is_html("Index.HTM"));
        for name in ["a.xhtml", "b.XHT", "logo.svg"] {
            assert!(is_html(name), "{name}");
        }
        assert!(!is_html("photo.png"));
    }

    #[test]
    fn previews_take_the_column_or_its_halves() {
        let (w, h) = preview_size(Some(4000), Some(3000), 1);
        assert_eq!((f32::from(w), f32::from(h)), (640., 360.));
        let (w, h) = preview_size(Some(2000), Some(10), 1);
        assert_eq!((f32::from(w), f32::from(h)), (640., 96.));
        let (w, h) = preview_size(Some(400), Some(300), 2);
        assert_eq!((f32::from(w), f32::from(h)), (312., 234.));
        let (w, h) = preview_size(None, None, 3);
        assert_eq!((f32::from(w), f32::from(h)), (312., 234.));
    }
}
