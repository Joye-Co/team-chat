//! Posters made on this machine (inc-host track): the frame in the middle of a video, the
//! first page of a PDF, an image itself, and the `MediaInfo` a sender computes for a file
//! before it sends it (R9, MEDIA §3).
//!
//! Generation runs on a background thread once per file; [`poster`] answers `None` until
//! the JPEG is on disk, then calls `done` once so the caller can redraw.

use std::{
    collections::HashMap,
    hash::{Hash as _, Hasher as _},
    path::{Path, PathBuf},
    sync::Mutex,
};

use chat_core::proto::{MediaInfo, MediaKind as WireKind, MediaPreview};
use serde_json::Number;

use super::{MediaKind, file_kind, native};

/// Width of a poster (MEDIA §4: 1280 px, JPEG ≤ 200 KB).
const POSTER_W: f64 = 1280.;

enum State {
    Making,
    Ready(Option<PathBuf>),
}

static POSTERS: Mutex<Option<HashMap<PathBuf, State>>> = Mutex::new(None);

fn out_path(src: &Path) -> PathBuf {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    src.hash(&mut h);
    std::fs::metadata(src)
        .map(|m| m.len())
        .unwrap_or(0)
        .hash(&mut h);
    std::env::temp_dir()
        .join("tc-media")
        .join("posters")
        .join(format!("{:016x}.jpg", h.finish()))
}

fn write_jpeg(b: &native::Bitmap, path: &Path) -> bool {
    let Some(img) = image::RgbaImage::from_raw(b.width, b.height, b.rgba.clone()) else {
        return false;
    };
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let rgb = image::DynamicImage::ImageRgba8(img).to_rgb8();
    let Ok(file) = std::fs::File::create(path) else {
        return false;
    };
    let mut w = std::io::BufWriter::new(file);
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut w, 80)
        .encode_image(&rgb)
        .is_ok()
}

/// Makes the poster of `src` now (blocking). `None` when the kind has none or it failed.
pub fn make(src: &Path, kind: MediaKind) -> Option<PathBuf> {
    match kind {
        MediaKind::Image => Some(src.to_path_buf()),
        MediaKind::Video => {
            let info = native::video_info(src)?;
            let at = (info.duration * 0.5).max(0.05);
            let b = native::video_frames(src, &[at], POSTER_W).pop().flatten()?;
            let out = out_path(src);
            write_jpeg(&b, &out).then_some(out)
        }
        MediaKind::Pdf => {
            let (_, mut pages) = native::pdf_pages(src, POSTER_W as u32, 1)?;
            let b = pages.drain(..).next()?;
            let out = out_path(src);
            write_jpeg(&b, &out).then_some(out)
        }
        _ => None,
    }
}

/// The poster of a local file, made once in the background. `done` runs (on that thread)
/// when a new poster is ready.
pub fn poster(src: &Path, kind: MediaKind, done: impl FnOnce() + Send + 'static) -> Option<PathBuf> {
    if kind == MediaKind::Image {
        return Some(src.to_path_buf());
    }
    if !matches!(kind, MediaKind::Video | MediaKind::Pdf) {
        return None;
    }
    let key = src.to_path_buf();
    {
        let mut g = POSTERS.lock().unwrap_or_else(|e| e.into_inner());
        let map = g.get_or_insert_with(HashMap::new);
        match map.get(&key) {
            Some(State::Ready(p)) => return p.clone(),
            Some(State::Making) => return None,
            None => {
                map.insert(key.clone(), State::Making);
            }
        }
    }
    let spawned = std::thread::Builder::new()
        .name("media-poster".into())
        .spawn(move || {
            let out = make(&key, kind);
            POSTERS
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .get_or_insert_with(HashMap::new)
                .insert(key, State::Ready(out));
            done();
        });
    if spawned.is_err() {
        tracing::warn!("cannot start the poster thread");
    }
    None
}

/// The wire kind of a local kind.
pub fn wire_kind(kind: MediaKind) -> WireKind {
    match kind {
        MediaKind::Html => WireKind::Html,
        MediaKind::Markdown => WireKind::Markdown,
        MediaKind::Image => WireKind::Image,
        MediaKind::ImageSet => WireKind::ImageSet,
        MediaKind::Video => WireKind::Video,
        MediaKind::Audio => WireKind::Audio,
        MediaKind::Pdf => WireKind::Pdf,
        MediaKind::Code => WireKind::Code,
        MediaKind::Table => WireKind::Table,
        MediaKind::File => WireKind::File,
    }
}

/// The local kind of a wire kind (`None` for a kind this app does not know).
pub fn local_kind(kind: WireKind) -> Option<MediaKind> {
    Some(match kind {
        WireKind::Html => MediaKind::Html,
        WireKind::Markdown => MediaKind::Markdown,
        WireKind::Image => MediaKind::Image,
        WireKind::ImageSet => MediaKind::ImageSet,
        WireKind::Video => MediaKind::Video,
        WireKind::Audio => MediaKind::Audio,
        WireKind::Pdf => MediaKind::Pdf,
        WireKind::Code => MediaKind::Code,
        WireKind::Table => MediaKind::Table,
        WireKind::File => MediaKind::File,
        WireKind::Unknown => return None,
    })
}

/// What the sender knows of a local file before it sends it (MEDIA §3): kind, size,
/// pixel size, length, pages. `poster` is the poster to upload (video, PDF). The
/// preview state is `ready` when the kind needs no poster or one was made.
pub fn describe(path: &Path, name: &str, mime: &str) -> (MediaInfo, Option<PathBuf>) {
    let kind = file_kind(name, mime);
    let bytes = std::fs::metadata(path).map(|m| m.len()).unwrap_or(0);
    let mut info = MediaInfo {
        kind: wire_kind(kind),
        mime: mime.to_string(),
        bytes,
        preview: MediaPreview::Ready,
        ..Default::default()
    };
    let mut poster = None;
    match kind {
        MediaKind::Image => {
            if let Ok((w, h)) = image::image_dimensions(path) {
                info.width = Some(w);
                info.height = Some(h);
            }
        }
        MediaKind::Video => match native::video_info(path) {
            Some(v) => {
                info.duration = Number::from_f64((v.duration * 10.).round() / 10.);
                info.width = Some(v.width as u32);
                info.height = Some(v.height as u32);
                poster = make(path, kind);
                if poster.is_none() {
                    info.preview = MediaPreview::Failed;
                }
            }
            None => info.preview = MediaPreview::Failed,
        },
        MediaKind::Audio => match native::audio_peaks(path, 64) {
            Some((d, _)) => info.duration = Number::from_f64((d * 10.).round() / 10.),
            None => info.preview = MediaPreview::Failed,
        },
        MediaKind::Pdf => match native::pdf_pages(path, 320, 1) {
            Some((pages, _)) => {
                info.pages = Some(pages as u64);
                poster = make(path, kind);
            }
            None => info.preview = MediaPreview::Failed,
        },
        _ => {}
    }
    (info, poster)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kinds_round_trip() {
        for k in [
            MediaKind::Html,
            MediaKind::Markdown,
            MediaKind::Image,
            MediaKind::ImageSet,
            MediaKind::Video,
            MediaKind::Audio,
            MediaKind::Pdf,
            MediaKind::Code,
            MediaKind::Table,
            MediaKind::File,
        ] {
            assert_eq!(local_kind(wire_kind(k)), Some(k));
        }
        assert_eq!(local_kind(WireKind::Unknown), None);
    }

    #[test]
    fn the_sender_describes_a_file_it_cannot_decode_as_failed() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("clip.mp4");
        std::fs::write(&p, b"not a video").unwrap();
        let (info, poster) = describe(&p, "clip.mp4", "video/mp4");
        assert_eq!(info.kind, WireKind::Video);
        assert_eq!(info.bytes, 11);
        assert_eq!(info.preview, MediaPreview::Failed);
        assert!(poster.is_none());
        let p = dir.path().join("a.csv");
        std::fs::write(&p, b"a,b\n1,2\n").unwrap();
        let (info, _) = describe(&p, "a.csv", "text/csv");
        assert_eq!((info.kind, info.preview), (WireKind::Table, MediaPreview::Ready));
    }

    #[test]
    fn the_sample_video_gets_a_poster_and_its_length() {
        let clip = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("src/views/artifacts/preview/clip.mp4");
        let (info, poster) = describe(&clip, "clip.mp4", "video/mp4");
        if cfg!(target_os = "macos") {
            assert!(info.duration.is_some());
            let poster = poster.expect("a poster");
            assert!(std::fs::metadata(poster).unwrap().len() > 0);
        }
    }
}
