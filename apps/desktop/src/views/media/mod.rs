//! 产出 media (v11/media): every version of a 产出 shows by what it holds.
//!
//! - [`classify`] reads a version's entry and files (paths and MIME types, existing data)
//!   and picks a [`MediaKind`]: HTML page, Markdown, image or image set, video, audio,
//!   PDF, code, table, or a plain file.
//! - [`store`] puts the version on disk through the provider, reads it once in the
//!   background ([`store::analyze`]: frames, peaks, pages, parsed text) and keeps the
//!   result for every view.
//! - [`inline`] draws the height-limited preview in the 话题 timeline; [`full`] draws the
//!   complete view in the 产出 viewer window ([`viewer`]) and the 产出 pane.
//! - [`states`] are the upload and preview-generation rows.
//!
//! Everything renders on this machine: HTML through the isolated [`WebEmbed`]
//! (`crate::ui::web_embed`), Markdown with the app's own Markdown view, video frames,
//! audio peaks and PDF pages with the system frameworks ([`native`]).
//!
//! [`WebEmbed`]: crate::ui::web_embed::WebEmbed

pub mod full;
pub mod inline;
pub mod native;
pub mod parts;
// --- inc-host track ---
pub mod poster;
// --- end inc-host track ---
pub mod preview;
pub mod state;
pub mod states;
pub mod store;
pub mod text;
pub mod viewer;

use gpui::{App, Hsla, Pixels, px, rgb};

use crate::{copy::media as cm, ui::Sym};

/// What a 产出 version shows as.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum MediaKind {
    Html,
    Markdown,
    Image,
    ImageSet,
    Video,
    Audio,
    Pdf,
    Code,
    Table,
    File,
}

impl MediaKind {
    pub fn label(self) -> &'static str {
        match self {
            MediaKind::Html => cm::KIND_HTML,
            MediaKind::Markdown => cm::KIND_MARKDOWN,
            MediaKind::Image | MediaKind::ImageSet => cm::KIND_IMAGE,
            MediaKind::Video => cm::KIND_VIDEO,
            MediaKind::Audio => cm::KIND_AUDIO,
            MediaKind::Pdf => cm::KIND_PDF,
            MediaKind::Code => cm::KIND_CODE,
            MediaKind::Table => cm::KIND_TABLE,
            MediaKind::File => cm::KIND_FILE,
        }
    }

    pub fn icon(self) -> Sym {
        match self {
            MediaKind::Html | MediaKind::Markdown | MediaKind::Pdf => Sym::Document,
            MediaKind::Image => Sym::Image,
            MediaKind::ImageSet => Sym::Images,
            MediaKind::Video => Sym::Video,
            MediaKind::Audio => Sym::Audio,
            MediaKind::Code => Sym::CodeFile,
            MediaKind::Table => Sym::Table,
            MediaKind::File => Sym::File,
        }
    }

    /// True when the timeline shows a preview of this kind (not only the file row).
    pub fn previews_inline(self) -> bool {
        self != MediaKind::File
    }

    /// Largest version the timeline loads without a click.
    pub fn auto_load_limit(self) -> u64 {
        match self {
            MediaKind::Video | MediaKind::Audio | MediaKind::Pdf => 300 << 20,
            MediaKind::Image | MediaKind::ImageSet => 100 << 20,
            _ => 20 << 20,
        }
    }
}

/// One file of a version: its path in the version, size and MIME type.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MediaFile {
    pub path: String,
    pub size: u64,
    pub mime: String,
}

fn ext(path: &str) -> String {
    path.rsplit_once('.')
        .map(|(_, e)| e.to_ascii_lowercase())
        .unwrap_or_default()
}

/// The kind of one file from its MIME type and extension. The extension must agree with
/// the MIME type for media the app decodes (the server does not check the bytes).
pub fn file_kind(path: &str, mime: &str) -> MediaKind {
    let e = ext(path);
    let mime = mime.to_ascii_lowercase();
    let image_ext = matches!(
        e.as_str(),
        "png" | "jpg" | "jpeg" | "gif" | "webp" | "heic" | "bmp" | "tiff"
    );
    match e.as_str() {
        "html" | "htm" => MediaKind::Html,
        "md" | "markdown" => MediaKind::Markdown,
        "mp4" | "mov" | "m4v" | "webm" if mime.starts_with("video/") || mime.is_empty() => {
            MediaKind::Video
        }
        "m4a" | "mp3" | "wav" | "aac" | "flac" | "aiff" | "caf"
            if mime.starts_with("audio/") || mime.is_empty() =>
        {
            MediaKind::Audio
        }
        "pdf" if mime == "application/pdf" || mime.is_empty() => MediaKind::Pdf,
        "csv" | "tsv" => MediaKind::Table,
        _ if image_ext && (mime.starts_with("image/") || mime.is_empty()) => MediaKind::Image,
        _ if text::language(path).is_some() => MediaKind::Code,
        _ => MediaKind::File,
    }
}

/// The kind of a version from its entry and files. Several images (and nothing else that
/// shows) are an image set.
pub fn classify(entry: &str, files: &[MediaFile]) -> MediaKind {
    let entry_mime = files
        .iter()
        .find(|f| f.path == entry)
        .map(|f| f.mime.as_str())
        .unwrap_or("");
    let kind = file_kind(entry, entry_mime);
    if kind == MediaKind::Image {
        let images = files
            .iter()
            .filter(|f| file_kind(&f.path, &f.mime) == MediaKind::Image)
            .count();
        if images > 1 {
            return MediaKind::ImageSet;
        }
    }
    kind
}

/// A guess of the MIME type from an extension, for files read from disk.
pub fn mime_for(path: &str) -> &'static str {
    match ext(path).as_str() {
        "html" | "htm" => "text/html",
        "md" | "markdown" => "text/markdown",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "heic" => "image/heic",
        "mp4" | "m4v" => "video/mp4",
        "mov" => "video/quicktime",
        "webm" => "video/webm",
        "m4a" => "audio/mp4",
        "mp3" => "audio/mpeg",
        "wav" => "audio/wav",
        "aac" => "audio/aac",
        "flac" => "audio/flac",
        "pdf" => "application/pdf",
        "csv" => "text/csv",
        "tsv" => "text/tab-separated-values",
        "vtt" => "text/vtt",
        _ => "application/octet-stream",
    }
}

// --- formatting -------------------------------------------------------------------

/// `1.2 MB`, `820 KB`, `3.4 GB`.
pub fn size_text(bytes: u64) -> String {
    let b = bytes as f64;
    if b >= 1e9 {
        format!("{:.1} GB", b / 1e9).replace(".0 GB", " GB")
    } else if b >= 1e6 {
        format!("{:.1} MB", b / 1e6)
    } else if b >= 1e3 {
        format!("{:.0} KB", b / 1e3)
    } else {
        format!("{bytes} B")
    }
}

/// `0:42`, `9:40`, `1:02:03`.
pub fn time_text(seconds: f64) -> String {
    let s = seconds.max(0.).round() as u64;
    let (h, m, s) = (s / 3600, (s / 60) % 60, s % 60);
    if h > 0 {
        format!("{h}:{m:02}:{s:02}")
    } else {
        format!("{m}:{s:02}")
    }
}

/// `09:40` (minutes always two digits), for transcripts and audio positions.
pub fn clock_text(seconds: f64) -> String {
    let s = seconds.max(0.).round() as u64;
    if s >= 3600 {
        time_text(seconds)
    } else {
        format!("{:02}:{:02}", s / 60, s % 60)
    }
}

// --- the dark "screen" behind video and the lightbox (Media.dc.html `--screen`) ---------

pub struct Screen {
    pub bg: Hsla,
    pub ink: Hsla,
    pub dim: Hsla,
    pub line: Hsla,
}

pub fn screen(cx: &App) -> Screen {
    let dark = crate::theme::is_dark(cx);
    Screen {
        bg: if dark { rgb(0x0B0B0D) } else { rgb(0x16161A) }.into(),
        ink: rgb(0xC9C9D1).into(),
        dim: if dark { rgb(0x6E6E78) } else { rgb(0x7A7A84) }.into(),
        line: gpui::hsla(0., 0., 1., 0.14),
    }
}

/// Width of an inline preview in the timeline (Media.dc.html: the message column).
pub const INLINE_W: Pixels = px(560.);

#[cfg(test)]
mod tests {
    use super::*;

    fn f(path: &str, mime: &str) -> MediaFile {
        MediaFile {
            path: path.into(),
            size: 1,
            mime: mime.into(),
        }
    }

    #[test]
    fn classifies_versions() {
        assert_eq!(
            classify("index.html", &[f("index.html", "text/html")]),
            MediaKind::Html
        );
        assert_eq!(
            classify("复盘.md", &[f("复盘.md", "text/markdown")]),
            MediaKind::Markdown
        );
        assert_eq!(
            classify("a.mov", &[f("a.mov", "video/quicktime")]),
            MediaKind::Video
        );
        assert_eq!(
            classify("a.m4a", &[f("a.m4a", "audio/mp4")]),
            MediaKind::Audio
        );
        assert_eq!(
            classify("r.pdf", &[f("r.pdf", "application/pdf")]),
            MediaKind::Pdf
        );
        assert_eq!(
            classify("x.csv", &[f("x.csv", "text/csv")]),
            MediaKind::Table
        );
        assert_eq!(
            classify("src/a.ts", &[f("src/a.ts", "video/mp2t")]),
            MediaKind::Code
        );
        assert_eq!(
            classify("1.png", &[f("1.png", "image/png"), f("2.png", "image/png")]),
            MediaKind::ImageSet
        );
        assert_eq!(
            classify("1.png", &[f("1.png", "image/png")]),
            MediaKind::Image
        );
        // A MIME type that disagrees with the extension shows the file row only.
        assert_eq!(
            classify("a.png", &[f("a.png", "application/zip")]),
            MediaKind::File
        );
        assert_eq!(
            classify("a.key", &[f("a.key", "application/x-iwork")]),
            MediaKind::File
        );
    }

    #[test]
    fn formats_sizes_and_times() {
        assert_eq!(size_text(820_000), "820 KB");
        assert_eq!(size_text(2_000_000_000), "2 GB");
        assert_eq!(size_text(14_200_000), "14.2 MB");
        assert_eq!(time_text(42.), "0:42");
        assert_eq!(time_text(1868.), "31:08");
        assert_eq!(time_text(3723.), "1:02:03");
        assert_eq!(clock_text(580.), "09:40");
    }
}
