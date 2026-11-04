//! Loads 产出 versions for the media views and keeps what was read.
//!
//! [`request_artifact`] asks the provider to put a version on disk (the same cache the
//! 产出 window uses), then [`analyze`] reads it once on a background thread: the kind,
//! parsed text, image sizes, video frames and poster, audio peaks, PDF pages and a
//! WebVTT file next to the media (chapters for a video, the transcript for audio). The
//! result is shared by every view of that version until the app quits.
//!
//! Generated pictures go to the system temp folder under `tc-media/<id>-v<n>/`; they are
//! made from the version's own files and never leave the machine.

use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::Arc,
};

use gpui::{App, AsyncApp, Global};

use super::{
    MediaFile, MediaKind, classify, file_kind, mime_for,
    native::{self, Bitmap, VideoInfo},
    text::{self, Heading, Lang, MdStats, Table},
};
use crate::data::{ChatProvider, FileError};

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct MediaKey {
    pub artifact_id: String,
    pub version: u32,
}

impl MediaKey {
    pub fn new(artifact_id: impl Into<String>, version: u32) -> Self {
        Self {
            artifact_id: artifact_id.into(),
            version,
        }
    }
}

#[derive(Clone, Debug)]
pub struct Pic {
    pub path: PathBuf,
    pub name: String,
    pub width: u32,
    pub height: u32,
    pub bytes: u64,
}

/// One timed line of a WebVTT file: a chapter or a transcript sentence.
#[derive(Clone, Debug, PartialEq)]
pub struct Cue {
    pub t: f64,
    pub end: f64,
    pub speaker: Option<String>,
    pub text: String,
}

#[derive(Clone, Debug)]
pub struct Frame {
    pub t: f64,
    pub path: PathBuf,
}

#[derive(Clone, Debug)]
pub struct Video {
    pub path: PathBuf,
    pub info: Option<VideoInfo>,
    pub poster: Option<PathBuf>,
    pub frames: Vec<Frame>,
    pub chapters: Vec<Cue>,
}

#[derive(Clone, Debug)]
pub struct Audio {
    pub path: PathBuf,
    pub duration: f64,
    pub peaks: Vec<f32>,
    pub transcript: Vec<Cue>,
}

#[derive(Clone, Debug)]
pub struct PdfPage {
    pub path: PathBuf,
    pub width: u32,
    pub height: u32,
}

#[derive(Clone, Debug)]
pub struct Pdf {
    pub path: PathBuf,
    pub pages: usize,
    pub images: Vec<PdfPage>,
}

#[derive(Clone, Debug)]
pub struct Code {
    pub language: &'static str,
    pub lang: Lang,
    pub lines: Vec<String>,
}

#[derive(Clone, Debug)]
pub struct Markdown {
    pub text: String,
    pub outline: Vec<Heading>,
    pub stats: MdStats,
}

#[derive(Clone, Debug)]
pub enum Data {
    Html,
    Markdown(Markdown),
    Images(Vec<Pic>),
    Video(Video),
    Audio(Audio),
    Pdf(Pdf),
    Code(Code),
    Table(Table),
    /// Shown as a file row (also when a preview could not be made).
    File,
}

/// One analyzed version.
#[derive(Clone, Debug)]
pub struct Media {
    pub key: MediaKey,
    pub title: String,
    pub kind: MediaKind,
    pub dir: PathBuf,
    pub entry: String,
    pub bytes: u64,
    pub data: Data,
    /// The kind needs a preview but making it failed.
    pub preview_failed: bool,
}

impl Media {
    pub fn entry_path(&self) -> PathBuf {
        self.dir.join(&self.entry)
    }

    /// The entry's file name.
    pub fn file_name(&self) -> &str {
        self.entry.rsplit('/').next().unwrap_or(&self.entry)
    }
}

#[derive(Clone, Debug)]
pub enum Load {
    Loading,
    Ready(Arc<Media>),
    Failed(FileError),
}

#[derive(Default)]
struct Store {
    map: HashMap<MediaKey, Load>,
}

impl Global for Store {}

/// What is known about `key` now.
pub fn get(key: &MediaKey, cx: &App) -> Option<Load> {
    cx.try_global::<Store>()
        .and_then(|s| s.map.get(key).cloned())
}

/// The version ready to show, if it is.
pub fn ready(key: &MediaKey, cx: &App) -> Option<Arc<Media>> {
    match get(key, cx)? {
        Load::Ready(m) => Some(m),
        _ => None,
    }
}

fn set(key: MediaKey, load: Load, cx: &mut App) {
    cx.default_global::<Store>().map.insert(key, load);
    cx.refresh_windows();
}

/// Starts loading a version of a 产出 once (download if needed, then [`analyze`]).
pub fn request_artifact(
    provider: &Arc<dyn ChatProvider>,
    key: &MediaKey,
    title: &str,
    cx: &mut App,
) {
    if get(key, cx).is_some() {
        return;
    }
    cx.default_global::<Store>()
        .map
        .insert(key.clone(), Load::Loading);
    let rx = provider.open_artifact_version(&key.artifact_id, Some(key.version));
    let (key, title) = (key.clone(), title.to_string());
    cx.spawn(
        async move |cx: &mut AsyncApp| match rx.await.unwrap_or(Err(FileError::Other)) {
            Ok(folder) => {
                cx.update(|cx| request_folder(&key, &title, folder.dir, folder.entry, cx));
            }
            Err(e) => {
                cx.update(|cx| set(key, Load::Failed(e), cx));
            }
        },
    )
    .detach();
}

/// Reads a version folder that is already on disk (also the self-test's sample 产出).
pub fn request_folder(key: &MediaKey, title: &str, dir: PathBuf, entry: String, cx: &mut App) {
    if matches!(get(key, cx), Some(Load::Ready(_))) {
        return;
    }
    cx.default_global::<Store>()
        .map
        .insert(key.clone(), Load::Loading);
    let (k, title) = (key.clone(), title.to_string());
    let task = cx
        .background_executor()
        .spawn(async move { analyze(k, title, dir, entry) });
    let key = key.clone();
    cx.spawn(async move |cx: &mut AsyncApp| {
        let media = task.await;
        cx.update(|cx| set(key, Load::Ready(Arc::new(media)), cx));
    })
    .detach();
}

/// Forgets every loaded version (self-tests start clean).
pub fn clear(cx: &mut App) {
    cx.default_global::<Store>().map.clear();
}

// --- analysis (background thread) ---------------------------------------------------

/// Files of a version folder: relative paths with `/`, sizes and guessed MIME types.
fn list_files(dir: &Path) -> Vec<MediaFile> {
    let mut out = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&d) else {
            continue;
        };
        for e in rd.flatten() {
            if out.len() >= 500 {
                return out;
            }
            let p = e.path();
            let Ok(meta) = e.metadata() else { continue };
            if meta.is_dir() {
                stack.push(p);
            } else if let Ok(rel) = p.strip_prefix(dir) {
                let path = rel
                    .components()
                    .map(|c| c.as_os_str().to_string_lossy().into_owned())
                    .collect::<Vec<_>>()
                    .join("/");
                out.push(MediaFile {
                    mime: mime_for(&path).to_string(),
                    path,
                    size: meta.len(),
                });
            }
        }
    }
    out.sort_by(|a, b| a.path.cmp(&b.path));
    out
}

fn cache_dir(key: &MediaKey) -> PathBuf {
    let id: String = key
        .artifact_id
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' {
                c
            } else {
                '_'
            }
        })
        .collect();
    std::env::temp_dir()
        .join("tc-media")
        .join(format!("{id}-v{}", key.version))
}

fn write_jpeg(bitmap: &Bitmap, path: &Path) -> bool {
    let Some(img) = image::RgbaImage::from_raw(bitmap.width, bitmap.height, bitmap.rgba.clone())
    else {
        return false;
    };
    let rgb = image::DynamicImage::ImageRgba8(img).to_rgb8();
    let Ok(file) = std::fs::File::create(path) else {
        return false;
    };
    let mut w = std::io::BufWriter::new(file);
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut w, 82)
        .encode_image(&rgb)
        .is_ok()
}

/// PNG bytes of a bitmap (a frame sent to the 话题 or copied).
pub fn png_bytes(bitmap: &Bitmap) -> Option<Vec<u8>> {
    let img = image::RgbaImage::from_raw(bitmap.width, bitmap.height, bitmap.rgba.clone())?;
    let mut out = std::io::Cursor::new(Vec::new());
    img.write_to(&mut out, image::ImageFormat::Png).ok()?;
    Some(out.into_inner())
}

/// Parses WebVTT cues (`00:24.000 --> 00:33.000`, an optional `<v Name>` speaker).
pub fn parse_vtt(text: &str) -> Vec<Cue> {
    fn ts(s: &str) -> Option<f64> {
        let s = s.split_whitespace().next()?;
        let parts: Vec<&str> = s.split(':').collect();
        let mut secs = 0f64;
        for p in &parts {
            secs = secs * 60. + p.replace(',', ".").parse::<f64>().ok()?;
        }
        (parts.len() >= 2).then_some(secs)
    }
    let mut out = Vec::new();
    let mut lines = text.lines().peekable();
    while let Some(line) = lines.next() {
        let Some((a, b)) = line.split_once("-->") else {
            continue;
        };
        let (Some(t), Some(end)) = (ts(a), ts(b)) else {
            continue;
        };
        let mut body = Vec::new();
        while let Some(l) = lines.peek() {
            if l.trim().is_empty() {
                break;
            }
            body.push(lines.next().unwrap_or_default().trim().to_string());
        }
        let mut text = body.join(" ");
        let mut speaker = None;
        if let Some(rest) = text.strip_prefix("<v ")
            && let Some((name, said)) = rest.split_once('>')
        {
            speaker = Some(name.trim().to_string());
            text = said.replace("</v>", "").trim().to_string();
        }
        if !text.is_empty() {
            out.push(Cue {
                t,
                end,
                speaker,
                text,
            });
        }
    }
    out
}

fn sidecar_vtt(dir: &Path, files: &[MediaFile]) -> Vec<Cue> {
    files
        .iter()
        .find(|f| f.path.to_ascii_lowercase().ends_with(".vtt") && f.size < 2 << 20)
        .and_then(|f| std::fs::read_to_string(dir.join(&f.path)).ok())
        .map(|t| parse_vtt(&t))
        .unwrap_or_default()
}

fn read_text(path: &Path, limit: u64) -> Option<String> {
    let len = std::fs::metadata(path).ok()?.len();
    if len > limit {
        return None;
    }
    let bytes = std::fs::read(path).ok()?;
    Some(String::from_utf8_lossy(&bytes).into_owned())
}

/// Number of frames in the 视频截图 strip.
pub const FRAMES: usize = 6;

/// Reads one version folder. Never fails: a preview that cannot be made becomes
/// [`Data::File`] with `preview_failed`.
pub fn analyze(key: MediaKey, title: String, dir: PathBuf, entry: String) -> Media {
    let files = list_files(&dir);
    let kind = classify(&entry, &files);
    let bytes = files.iter().map(|f| f.size).sum();
    let path = dir.join(&entry);
    let cache = cache_dir(&key);
    let _ = std::fs::create_dir_all(&cache);
    let data = match kind {
        MediaKind::Html => Some(Data::Html),
        MediaKind::Markdown => read_text(&path, 5 << 20).map(|text| {
            Data::Markdown(Markdown {
                outline: text::outline(&text),
                stats: text::md_stats(&text),
                text,
            })
        }),
        MediaKind::Image | MediaKind::ImageSet => {
            let pics: Vec<Pic> = files
                .iter()
                .filter(|f| {
                    file_kind(&f.path, &f.mime) == MediaKind::Image
                        && (kind == MediaKind::ImageSet || f.path == entry)
                })
                .take(30)
                .map(|f| {
                    let p = dir.join(&f.path);
                    let (width, height) = image::image_dimensions(&p).unwrap_or((0, 0));
                    Pic {
                        name: f.path.rsplit('/').next().unwrap_or(&f.path).to_string(),
                        path: p,
                        width,
                        height,
                        bytes: f.size,
                    }
                })
                .collect();
            (!pics.is_empty()).then_some(Data::Images(pics))
        }
        MediaKind::Video => native::video_info(&path).map(|info| {
            let d = info.duration;
            let times: Vec<f64> = (0..FRAMES)
                .map(|i| (i as f64 * d / FRAMES as f64).max(0.05))
                .collect();
            let frames = native::video_frames(&path, &times, 320.)
                .into_iter()
                .zip(&times)
                .enumerate()
                .filter_map(|(i, (b, &t))| {
                    let out = cache.join(format!("frame-{i}.jpg"));
                    (b.is_some_and(|b| write_jpeg(&b, &out))).then_some(Frame { t, path: out })
                })
                .collect();
            let poster = native::video_frames(&path, &[d * 0.5], 1280.)
                .pop()
                .flatten()
                .and_then(|b| {
                    let out = cache.join("poster.jpg");
                    write_jpeg(&b, &out).then_some(out)
                });
            Data::Video(Video {
                path: path.clone(),
                info: Some(info),
                poster,
                frames,
                chapters: sidecar_vtt(&dir, &files),
            })
        }),
        MediaKind::Audio => native::audio_peaks(&path, 2000).map(|(duration, peaks)| {
            Data::Audio(Audio {
                path: path.clone(),
                duration,
                peaks,
                transcript: sidecar_vtt(&dir, &files),
            })
        }),
        MediaKind::Pdf => native::pdf_pages(&path, 1200, 60).map(|(pages, bitmaps)| {
            let images = bitmaps
                .into_iter()
                .enumerate()
                .filter_map(|(i, b)| {
                    let out = cache.join(format!("page-{i}.jpg"));
                    write_jpeg(&b, &out).then_some(PdfPage {
                        path: out,
                        width: b.width,
                        height: b.height,
                    })
                })
                .collect();
            Data::Pdf(Pdf {
                path: path.clone(),
                pages,
                images,
            })
        }),
        MediaKind::Code => {
            let (language, lang) = text::language(&entry).unwrap_or(("Text", Lang::Plain));
            read_text(&path, 1 << 20).map(|t| {
                Data::Code(Code {
                    language,
                    lang,
                    lines: t.lines().take(20_000).map(str::to_string).collect(),
                })
            })
        }
        MediaKind::Table => {
            let delim = if entry.to_ascii_lowercase().ends_with(".tsv") {
                '\t'
            } else {
                ','
            };
            read_text(&path, 200 << 20).map(|t| Data::Table(text::parse_table(&t, delim, 5000)))
        }
        MediaKind::File => Some(Data::File),
    };
    let preview_failed = data.is_none();
    Media {
        key,
        title,
        kind: if preview_failed {
            MediaKind::File
        } else {
            kind
        },
        dir,
        entry,
        bytes,
        data: data.unwrap_or(Data::File),
        preview_failed,
    }
}

/// A frame of a video at full size (for 复制这一帧 / 把这一帧发到话题), as PNG bytes.
pub fn frame_png(video: &Path, t: f64) -> Option<Vec<u8>> {
    let b = native::video_frames(video, &[t], 1920.).pop().flatten()?;
    png_bytes(&b)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_vtt_cues_and_speakers() {
        let cues = parse_vtt(
            "WEBVTT\n\n1\n00:00.000 --> 00:08.000\n两个窗口都在线\n\n00:02:14.000 --> 00:02:31.000\n<v Joye>这周先把 PR 合了。</v>\n",
        );
        assert_eq!(cues.len(), 2);
        assert_eq!(cues[0].text, "两个窗口都在线");
        assert_eq!(cues[1].t, 134.);
        assert_eq!(cues[1].speaker.as_deref(), Some("Joye"));
        assert_eq!(cues[1].text, "这周先把 PR 合了。");
    }

    #[test]
    fn analyzes_text_versions_on_disk() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("数据.csv"), "a,b\n1,2\n3,4\n").unwrap();
        let m = analyze(
            MediaKey::new("t-csv", 1),
            "数据".into(),
            dir.path().to_path_buf(),
            "数据.csv".into(),
        );
        assert_eq!(m.kind, MediaKind::Table);
        let Data::Table(t) = &m.data else {
            panic!("not a table")
        };
        assert_eq!(t.total_rows, 2);
        std::fs::write(dir.path().join("broken.pdf"), "not a pdf").unwrap();
        let m = analyze(
            MediaKey::new("t-pdf", 1),
            "坏的".into(),
            dir.path().to_path_buf(),
            "broken.pdf".into(),
        );
        assert!(m.preview_failed);
        assert_eq!(m.kind, MediaKind::File);
    }
}
