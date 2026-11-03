//! 1.1 media metadata of 产出 versions and message files (docs/protocol.md, "Media"). The
//! sender's client computes it; the server checks sizes and limits and never decodes media.

use serde::{Deserialize, Serialize};
use serde_json::Number;

use crate::limits::{Validation, ValidationError, check_chars, check_id};
use crate::message::check_mime;
use crate::serde_util::non_null;

pub const MAX_MEDIA_FRAMES: usize = 12;
pub const MAX_MEDIA_CHAPTERS: usize = 20;
pub const MAX_MEDIA_IMAGES: usize = 30;
pub const MEDIA_MAX_BYTES: u64 = 2_000_000_000;
pub const MEDIA_MAX_SECONDS: f64 = 4.0 * 3600.0;
pub const MEDIA_MAX_IMAGE_BYTES: u64 = 50_000_000;
pub const MEDIA_MAX_IMAGE_PIXELS: u64 = 100_000_000;
pub const MEDIA_MAX_PDF_BYTES: u64 = 500_000_000;
pub const MEDIA_MAX_PDF_PAGES: u64 = 2000;
pub const MEDIA_MAX_HTML_BYTES: u64 = 20_000_000;
pub const MEDIA_MAX_TEXT_BYTES: u64 = 5_000_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
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
    #[default]
    File,
    /// A kind this version does not know; show the file row.
    #[serde(other)]
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MediaPreview {
    Ready,
    #[default]
    Pending,
    Failed,
    #[serde(other)]
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MediaFrame {
    /// Seconds.
    pub t: Number,
    pub file_id: String,
    #[serde(
        default,
        deserialize_with = "non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub label: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MediaChapter {
    pub t: Number,
    pub title: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MediaImage {
    pub file_id: String,
    pub width: u32,
    pub height: u32,
    pub thumb_file_id: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MediaFocus {
    pub from_line: u32,
    pub to_line: u32,
}

/// What a 产出 version or a message file is, for inline previews.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MediaInfo {
    pub kind: MediaKind,
    pub mime: String,
    pub bytes: u64,
    #[serde(
        default,
        deserialize_with = "non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub width: Option<u32>,
    #[serde(
        default,
        deserialize_with = "non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub height: Option<u32>,
    /// Seconds (video, audio).
    #[serde(
        default,
        deserialize_with = "non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub duration: Option<Number>,
    #[serde(
        default,
        deserialize_with = "non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub pages: Option<u64>,
    #[serde(
        default,
        deserialize_with = "non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub rows: Option<u64>,
    #[serde(
        default,
        deserialize_with = "non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub cols: Option<u64>,
    #[serde(
        default,
        deserialize_with = "non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub lines: Option<u64>,
    #[serde(
        default,
        deserialize_with = "non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub language: Option<String>,
    /// Video poster, HTML snapshot, PDF page 1 or image thumbnail.
    #[serde(
        default,
        deserialize_with = "non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub poster_file_id: Option<String>,
    #[serde(
        default,
        deserialize_with = "non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub frames: Option<Vec<MediaFrame>>,
    #[serde(
        default,
        deserialize_with = "non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub chapters: Option<Vec<MediaChapter>>,
    /// Audio waveform peaks (Float16 array).
    #[serde(
        default,
        deserialize_with = "non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub peaks_file_id: Option<String>,
    /// JSON `[{t, end, speaker, text}]`.
    #[serde(
        default,
        deserialize_with = "non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub transcript_file_id: Option<String>,
    #[serde(
        default,
        deserialize_with = "non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub images: Option<Vec<MediaImage>>,
    /// Code: the lines shown inline.
    #[serde(
        default,
        deserialize_with = "non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub focus: Option<MediaFocus>,
    pub preview: MediaPreview,
}

fn fail(field: &'static str, message: impl Into<String>) -> Validation {
    Err(ValidationError {
        field,
        message: message.into(),
    })
}

impl MediaInfo {
    /// The first size limit the metadata breaks, or `None`.
    pub fn problem(&self) -> Option<&'static str> {
        if self.bytes > MEDIA_MAX_BYTES {
            return Some("file exceeds 2 GB");
        }
        if self
            .duration
            .as_ref()
            .and_then(Number::as_f64)
            .is_some_and(|d| d > MEDIA_MAX_SECONDS || d < 0.0)
        {
            return Some("longer than 4 hours");
        }
        match self.kind {
            MediaKind::Image => {
                if self.bytes > MEDIA_MAX_IMAGE_BYTES {
                    return Some("image exceeds 50 MB");
                }
                if let (Some(w), Some(h)) = (self.width, self.height)
                    && u64::from(w) * u64::from(h) > MEDIA_MAX_IMAGE_PIXELS
                {
                    return Some("image exceeds 100 MP");
                }
            }
            MediaKind::Pdf
                if self.bytes > MEDIA_MAX_PDF_BYTES
                    || self.pages.unwrap_or(0) > MEDIA_MAX_PDF_PAGES =>
            {
                return Some("pdf exceeds 500 MB or 2000 pages");
            }
            MediaKind::Html if self.bytes > MEDIA_MAX_HTML_BYTES => {
                return Some("html exceeds 20 MB");
            }
            MediaKind::Markdown | MediaKind::Code if self.bytes > MEDIA_MAX_TEXT_BYTES => {
                return Some("text exceeds 5 MB");
            }
            _ => {}
        }
        if self.focus.is_some_and(|f| f.from_line > f.to_line) {
            return Some("focus.fromLine is after toLine");
        }
        None
    }

    /// Shapes and limits, as a write checks them.
    pub fn validate(&self) -> Validation {
        if self.kind == MediaKind::Unknown {
            return fail("media.kind", "unknown kind");
        }
        if self.preview == MediaPreview::Unknown {
            return fail("media.preview", "unknown preview state");
        }
        check_mime("media.mime", &self.mime)?;
        if let Some(l) = &self.language {
            check_chars("media.language", l, 1, 32)?;
        }
        if self
            .frames
            .as_ref()
            .is_some_and(|f| f.len() > MAX_MEDIA_FRAMES)
            || self
                .chapters
                .as_ref()
                .is_some_and(|c| c.len() > MAX_MEDIA_CHAPTERS)
            || self
                .images
                .as_ref()
                .is_some_and(|i| i.len() > MAX_MEDIA_IMAGES)
        {
            return fail("media", "too many frames, chapters or images");
        }
        for f in self.frames.iter().flatten() {
            check_id("media.frames.fileId", &f.file_id)?;
        }
        if self
            .focus
            .is_some_and(|f| f.from_line == 0 || f.to_line == 0)
        {
            return fail("media.focus", "lines start at 1");
        }
        match self.problem() {
            Some(p) => fail("media", p),
            None => Ok(()),
        }
    }
}

/// Data of `artifact.previewReady` (1.1): a sender attached the preview of a 产出 version.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ArtifactPreviewReadyData {
    pub artifact_id: String,
    pub version: u32,
    pub media: MediaInfo,
}
