//! System frameworks behind the media previews, all on the local machine:
//!
//! - video: duration, size and frames with `AVAssetImageGenerator`;
//! - audio: duration and waveform peaks with `AVAudioFile`, playback with `AVPlayer`;
//! - PDF: page count and page pictures drawn by PDFKit into a CoreGraphics bitmap.
//!
//! Everything except [`AudioPlayer`] runs on a background thread. Pictures come back as
//! RGBA bitmaps; the caller encodes and caches them. Windows has the same functions on
//! Media Foundation and WinRT (`native_windows.rs`). On other systems every function
//! answers `None`, so the previews fall back to the file row.

use std::path::Path;

/// An RGBA picture, rows top to bottom.
pub struct Bitmap {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

/// Duration (seconds) and natural size of a video.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct VideoInfo {
    pub duration: f64,
    pub width: u32,
    pub height: u32,
}

#[cfg(target_os = "macos")]
mod mac {
    #![allow(deprecated)]

    use std::{ffi::c_void, path::Path, ptr::NonNull};

    use objc2::{AllocAnyThread as _, rc::Retained};
    use objc2_av_foundation::{
        AVAsset, AVAssetImageGenerator, AVMediaTypeVideo, AVPlayer, AVURLAsset,
    };
    use objc2_avf_audio::{AVAudioFile, AVAudioPCMBuffer};
    use objc2_core_foundation::{CGPoint, CGRect, CGSize};
    use objc2_core_graphics::{
        CGBitmapContextCreate, CGColorSpace, CGContext, CGImage, CGImageAlphaInfo,
    };
    use objc2_core_media::CMTime;
    use objc2_foundation::{MainThreadMarker, NSString, NSURL};
    use objc2_pdf_kit::{PDFDisplayBox, PDFDocument};

    use super::{Bitmap, VideoInfo};

    fn file_url(path: &Path) -> Option<Retained<NSURL>> {
        let s = path.to_str()?;
        Some(NSURL::fileURLWithPath(&NSString::from_str(s)))
    }

    /// `kCGBitmapByteOrder32Big`: RGBA in memory order.
    const BYTE_ORDER_32_BIG: u32 = 4 << 12;

    /// Draws into a fresh RGBA bitmap of `width` x `height`.
    fn draw(width: u32, height: u32, f: impl FnOnce(&CGContext)) -> Option<Bitmap> {
        if width == 0 || height == 0 || width > 8192 || height > 8192 {
            return None;
        }
        let (w, h) = (width as usize, height as usize);
        let mut rgba = vec![0u8; w * h * 4];
        let space = CGColorSpace::new_device_rgb()?;
        let ctx = unsafe {
            CGBitmapContextCreate(
                rgba.as_mut_ptr().cast::<c_void>(),
                w,
                h,
                8,
                w * 4,
                Some(&space),
                CGImageAlphaInfo::PremultipliedLast.0 | BYTE_ORDER_32_BIG,
            )
        }?;
        f(&ctx);
        drop(ctx);
        Some(Bitmap {
            width,
            height,
            rgba,
        })
    }

    fn asset(path: &Path) -> Option<Retained<AVURLAsset>> {
        let url = file_url(path)?;
        Some(unsafe { AVURLAsset::URLAssetWithURL_options(&url, None) })
    }

    pub fn video_info(path: &Path) -> Option<VideoInfo> {
        let asset = asset(path)?;
        let duration = unsafe { asset.duration().seconds() };
        let media = unsafe { AVMediaTypeVideo }?;
        let tracks = unsafe { asset.tracksWithMediaType(media) };
        let track = tracks.firstObject()?;
        let size = unsafe { track.naturalSize() };
        let t = unsafe { track.preferredTransform() };
        // A portrait recording carries a 90° transform.
        let rotated = t.b.abs() > 0.5 && t.c.abs() > 0.5;
        let (w, h) = if rotated {
            (size.height, size.width)
        } else {
            (size.width, size.height)
        };
        (duration.is_finite() && w > 0. && h > 0.).then_some(VideoInfo {
            duration: duration.max(0.),
            width: w.round() as u32,
            height: h.round() as u32,
        })
    }

    pub fn video_frames(path: &Path, times: &[f64], max_width: f64) -> Vec<Option<Bitmap>> {
        let Some(asset) = asset(path) else {
            return times.iter().map(|_| None).collect();
        };
        let asset: &AVAsset = &asset;
        let generator = unsafe { AVAssetImageGenerator::assetImageGeneratorWithAsset(asset) };
        unsafe {
            generator.setAppliesPreferredTrackTransform(true);
            generator.setMaximumSize(CGSize::new(max_width, max_width));
            let tolerance = CMTime::with_seconds(0.1, 600);
            generator.setRequestedTimeToleranceBefore(tolerance);
            generator.setRequestedTimeToleranceAfter(tolerance);
        }
        times
            .iter()
            .map(|&t| {
                let at = unsafe { CMTime::with_seconds(t.max(0.), 600) };
                let image = unsafe {
                    generator.copyCGImageAtTime_actualTime_error(at, std::ptr::null_mut())
                }
                .ok()?;
                cg_bitmap(&image)
            })
            .collect()
    }

    fn cg_bitmap(image: &CGImage) -> Option<Bitmap> {
        let w = CGImage::width(Some(image)) as u32;
        let h = CGImage::height(Some(image)) as u32;
        draw(w, h, |ctx| {
            CGContext::draw_image(
                Some(ctx),
                CGRect::new(CGPoint::new(0., 0.), CGSize::new(w as f64, h as f64)),
                Some(image),
            );
        })
    }

    pub fn audio_peaks(path: &Path, buckets: usize) -> Option<(f64, Vec<f32>)> {
        let url = file_url(path)?;
        let file = unsafe { AVAudioFile::initForReading_error(AVAudioFile::alloc(), &url) }.ok()?;
        let format = unsafe { file.processingFormat() };
        let rate = unsafe { format.sampleRate() };
        let channels = unsafe { format.channelCount() } as usize;
        let frames = unsafe { file.length() }.max(0) as u64;
        if rate <= 0. || frames == 0 || channels == 0 {
            return None;
        }
        let duration = frames as f64 / rate;
        let buckets = buckets.clamp(1, 4000);
        let per_bucket = (frames as f64 / buckets as f64).max(1.);
        let mut peaks = vec![0f32; buckets];
        let chunk: u32 = 65_536;
        let buffer = unsafe {
            AVAudioPCMBuffer::initWithPCMFormat_frameCapacity(
                AVAudioPCMBuffer::alloc(),
                &format,
                chunk,
            )
        }?;
        let mut position: u64 = 0;
        loop {
            if unsafe { file.readIntoBuffer_frameCount_error(&buffer, chunk) }.is_err() {
                break;
            }
            let n = unsafe { buffer.frameLength() } as usize;
            if n == 0 {
                break;
            }
            let data = unsafe { buffer.floatChannelData() };
            if data.is_null() {
                return None;
            }
            for c in 0..channels {
                let ptr: NonNull<f32> = unsafe { *data.add(c) };
                let samples = unsafe { std::slice::from_raw_parts(ptr.as_ptr(), n) };
                for (i, s) in samples.iter().enumerate() {
                    let b = (((position + i as u64) as f64 / per_bucket) as usize).min(buckets - 1);
                    let v = s.abs();
                    if v > peaks[b] {
                        peaks[b] = v;
                    }
                }
            }
            position += n as u64;
        }
        let max = peaks.iter().cloned().fold(0f32, f32::max);
        if max > 0. {
            for p in &mut peaks {
                *p /= max;
            }
        }
        Some((duration, peaks))
    }

    pub fn pdf_pages(path: &Path, width: u32, max_pages: usize) -> Option<(usize, Vec<Bitmap>)> {
        let url = file_url(path)?;
        let doc = unsafe { PDFDocument::initWithURL(PDFDocument::alloc(), &url) }?;
        let count = unsafe { doc.pageCount() };
        let mut out = Vec::new();
        for i in 0..count.min(max_pages) {
            let Some(page) = (unsafe { doc.pageAtIndex(i) }) else {
                break;
            };
            let bounds = unsafe { page.boundsForBox(PDFDisplayBox::CropBox) };
            let rotation = unsafe { page.rotation() }.rem_euclid(360);
            let (pw, ph) = if rotation == 90 || rotation == 270 {
                (bounds.size.height, bounds.size.width)
            } else {
                (bounds.size.width, bounds.size.height)
            };
            if pw <= 0. || ph <= 0. {
                continue;
            }
            let scale = width as f64 / pw;
            let height = (ph * scale).round().max(1.) as u32;
            let bitmap = draw(width, height, |ctx| {
                CGContext::set_rgb_fill_color(Some(ctx), 1., 1., 1., 1.);
                CGContext::fill_rect(
                    Some(ctx),
                    CGRect::new(
                        CGPoint::new(0., 0.),
                        CGSize::new(width as f64, height as f64),
                    ),
                );
                CGContext::scale_ctm(Some(ctx), scale, scale);
                unsafe { page.drawWithBox_toContext(PDFDisplayBox::CropBox, ctx) };
            });
            if let Some(b) = bitmap {
                out.push(b);
            }
        }
        Some((count, out))
    }

    /// Plays one audio file from GPUI controls. Main thread only.
    pub struct AudioPlayer {
        player: Retained<AVPlayer>,
        playing: bool,
        rate: f32,
    }

    impl AudioPlayer {
        pub fn new(path: &Path) -> Option<Self> {
            let mtm = MainThreadMarker::new()?;
            let url = file_url(path)?;
            let player = unsafe { AVPlayer::playerWithURL(&url, mtm) };
            Some(Self {
                player,
                playing: false,
                rate: 1.,
            })
        }

        pub fn play(&mut self) {
            unsafe {
                self.player.play();
                self.player.setRate(self.rate);
            }
            self.playing = true;
        }

        pub fn pause(&mut self) {
            unsafe { self.player.pause() };
            self.playing = false;
        }

        pub fn is_playing(&self) -> bool {
            self.playing && unsafe { self.player.rate() } > 0.
        }

        pub fn position(&self) -> f64 {
            let t = unsafe { self.player.currentTime().seconds() };
            if t.is_finite() { t.max(0.) } else { 0. }
        }

        pub fn seek(&mut self, seconds: f64) {
            unsafe {
                self.player
                    .seekToTime(CMTime::with_seconds(seconds.max(0.), 600))
            };
        }

        pub fn set_rate(&mut self, rate: f32) {
            self.rate = rate;
            if self.playing {
                unsafe { self.player.setRate(rate) };
            }
        }
    }

    impl Drop for AudioPlayer {
        fn drop(&mut self) {
            unsafe { self.player.pause() };
        }
    }
}

#[cfg(target_os = "macos")]
pub use mac::AudioPlayer;

#[cfg(target_os = "windows")]
#[path = "native_windows.rs"]
mod win;

#[cfg(target_os = "windows")]
pub use win::AudioPlayer;

pub fn video_info(path: &Path) -> Option<VideoInfo> {
    #[cfg(target_os = "macos")]
    return mac::video_info(path);
    #[cfg(target_os = "windows")]
    return win::video_info(path);
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        let _ = path;
        None
    }
}

/// Frames at `times` (seconds), at most `max_width` pixels on their long side.
pub fn video_frames(path: &Path, times: &[f64], max_width: f64) -> Vec<Option<Bitmap>> {
    #[cfg(target_os = "macos")]
    return mac::video_frames(path, times, max_width);
    #[cfg(target_os = "windows")]
    return win::video_frames(path, times, max_width);
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        let _ = (path, max_width);
        times.iter().map(|_| None).collect()
    }
}

/// Duration (seconds) and `buckets` peaks between 0 and 1.
pub fn audio_peaks(path: &Path, buckets: usize) -> Option<(f64, Vec<f32>)> {
    #[cfg(target_os = "macos")]
    return mac::audio_peaks(path, buckets);
    #[cfg(target_os = "windows")]
    return win::audio_peaks(path, buckets);
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        let _ = (path, buckets);
        None
    }
}

/// The page count and the first `max_pages` pages drawn `width` pixels wide.
pub fn pdf_pages(path: &Path, width: u32, max_pages: usize) -> Option<(usize, Vec<Bitmap>)> {
    #[cfg(target_os = "macos")]
    return mac::pdf_pages(path, width, max_pages);
    #[cfg(target_os = "windows")]
    return win::pdf_pages(path, width, max_pages);
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        let _ = (path, width, max_pages);
        None
    }
}

/// A stand-in where there is no player: nothing plays.
#[cfg(not(any(target_os = "macos", target_os = "windows")))]
pub struct AudioPlayer;

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
impl AudioPlayer {
    pub fn new(_: &Path) -> Option<Self> {
        None
    }
    pub fn play(&mut self) {}
    pub fn pause(&mut self) {}
    pub fn is_playing(&self) -> bool {
        false
    }
    pub fn position(&self) -> f64 {
        0.
    }
    pub fn seek(&mut self, _: f64) {}
    pub fn set_rate(&mut self, _: f32) {}
}

/// These run against the real system frameworks, on macOS and on Windows CI. The
/// pictures in `fixtures/` are synthetic (made with ffmpeg from colour and sine sources).
#[cfg(all(test, any(target_os = "macos", target_os = "windows")))]
mod tests {
    use std::path::PathBuf;

    use super::*;

    fn fixture(name: &str) -> PathBuf {
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("src/views/media/fixtures")
            .join(name)
    }

    /// The pixel in the middle of a quarter of the picture: (column, row) of the quarter.
    fn quarter(b: &Bitmap, col: u32, row: u32) -> [u8; 3] {
        let x = b.width / 4 + col * b.width / 2;
        let y = b.height / 4 + row * b.height / 2;
        let i = ((y * b.width + x) * 4) as usize;
        [b.rgba[i], b.rgba[i + 1], b.rgba[i + 2]]
    }

    /// Which of red, green, blue are bright: decoders differ a little on the exact colour.
    fn bright(p: [u8; 3]) -> [bool; 3] {
        assert!(
            p.iter().all(|c| !(96..160).contains(c)),
            "a muddy colour: {p:?}"
        );
        [p[0] > 128, p[1] > 128, p[2] > 128]
    }

    const RED: [bool; 3] = [true, false, false];
    const GREEN: [bool; 3] = [false, true, false];
    const BLUE: [bool; 3] = [false, false, true];
    const WHITE: [bool; 3] = [true, true, true];

    /// The four quarters: top left, top right, bottom left, bottom right.
    fn quarters(b: &Bitmap) -> [[bool; 3]; 4] {
        [
            bright(quarter(b, 0, 0)),
            bright(quarter(b, 1, 0)),
            bright(quarter(b, 0, 1)),
            bright(quarter(b, 1, 1)),
        ]
    }

    /// Not a check: what Media Foundation reports for every fixture, in the CI log.
    #[cfg(target_os = "windows")]
    #[test]
    fn media_foundation_describes_the_fixtures() {
        for name in [
            "quadrants.mp4",
            "quadrants-rotated.mp4",
            "quadrants-hevc.mp4",
            "tone.mp3",
            "tone.m4a",
            "tone-stereo.m4a",
        ] {
            println!("--- {name}\n{}", win::describe(&fixture(name)));
        }
        let b = video_frames(&fixture("quadrants-rotated.mp4"), &[0.5], 1920.)
            .pop()
            .flatten()
            .unwrap();
        println!("rotated: {}x{} {:?}", b.width, b.height, quarters(&b));
        for name in ["tone.m4a", "tone-stereo.m4a"] {
            println!(
                "{name}: {:?}",
                audio_peaks(&fixture(name), 10).map(|(d, p)| (d, p.len()))
            );
        }
    }

    #[test]
    fn a_video_gives_its_length_its_size_and_upright_frames() {
        let clip = fixture("quadrants.mp4");
        let info = video_info(&clip).expect("the video's info");
        assert!((info.duration - 1.).abs() < 0.15, "{info:?}");
        assert_eq!((info.width, info.height), (128, 96));
        let frames = video_frames(&clip, &[0.05, 0.5, 30.], 64.);
        assert_eq!(frames.len(), 3);
        for frame in &frames[..2] {
            let b = frame.as_ref().expect("a frame");
            assert_eq!((b.width, b.height), (64, 48));
            assert_eq!(b.rgba.len(), 64 * 48 * 4);
            assert_eq!(quarters(b), [RED, GREEN, BLUE, WHITE]);
        }
        // Never bigger than the video.
        let full = video_frames(&clip, &[0.5], 1920.).pop().flatten().unwrap();
        assert_eq!((full.width, full.height), (128, 96));
    }

    #[test]
    fn a_video_recorded_sideways_comes_back_upright() {
        let clip = fixture("quadrants-rotated.mp4");
        let info = video_info(&clip).expect("the video's info");
        assert_eq!((info.width, info.height), (96, 128));
        let b = video_frames(&clip, &[0.5], 1920.).pop().flatten().unwrap();
        assert_eq!((b.width, b.height), (96, 128));
        assert_eq!(quarters(&b), ROTATED);
    }

    /// `quadrants-rotated.mp4` is `quadrants.mp4` with a display matrix; this is how
    /// AVFoundation shows it.
    const ROTATED: [[bool; 3]; 4] = [GREEN, WHITE, RED, BLUE];

    #[test]
    fn a_file_that_is_not_a_video_has_no_info_and_no_frames() {
        let dir = tempfile::tempdir().unwrap();
        for (name, bytes) in [
            ("坏的 录屏.mp4", &b"not a video"[..]),
            ("empty.mp4", &b""[..]),
            ("cut.mov", &include_bytes!("fixtures/quadrants.mp4")[..200]),
        ] {
            let p = dir.path().join(name);
            std::fs::write(&p, bytes).unwrap();
            assert_eq!(video_info(&p), None, "{name}");
            let frames = video_frames(&p, &[0., 1.], 320.);
            assert_eq!(frames.len(), 2);
            assert!(frames.iter().all(Option::is_none), "{name}");
        }
        assert_eq!(video_info(&dir.path().join("missing.mp4")), None);
    }

    /// Windows has an HEVC decoder only with the store extension, macOS always: with
    /// one the frames come, without one the file reads as "no preview". Never a hang.
    #[test]
    fn a_video_without_a_decoder_reads_as_no_preview() {
        let clip = fixture("quadrants-hevc.mp4");
        let info = video_info(&clip);
        let frame = video_frames(&clip, &[0.5], 320.).pop().flatten();
        println!("hevc: info {info:?}, frame {}", frame.is_some());
        if cfg!(target_os = "macos") {
            assert!(info.is_some());
        }
        match info {
            Some(info) => {
                assert_eq!((info.width, info.height), (128, 96));
                assert_eq!(
                    quarters(&frame.expect("a frame")),
                    [RED, GREEN, BLUE, WHITE]
                );
            }
            None => assert!(frame.is_none()),
        }
    }

    #[test]
    fn audio_gives_its_length_and_peaks() {
        let dir = tempfile::tempdir().unwrap();
        let wav = dir.path().join("周会 录音.wav");
        std::fs::write(&wav, crate::views::media::preview::speech_wav(3)).unwrap();
        let (duration, peaks) = audio_peaks(&wav, 200).expect("the peaks of a WAV file");
        assert!((duration - 3.).abs() < 0.01, "{duration}");
        assert_eq!(peaks.len(), 200);
        assert!(peaks.iter().all(|p| (0. ..=1.).contains(p)));
        assert_eq!(peaks.iter().cloned().fold(0f32, f32::max), 1.);
        // Every bucket of the speech-like sample has sound, and they differ.
        assert!(peaks.iter().all(|p| *p > 0.01), "{peaks:?}");
        assert!(peaks.iter().any(|p| *p < 0.5));

        for name in ["tone.mp3", "tone.m4a"] {
            let (duration, peaks) = audio_peaks(&fixture(name), 50).expect(name);
            assert!((duration - 1.).abs() < 0.2, "{name}: {duration}");
            assert_eq!(peaks.len(), 50);
            // A steady tone: loud from start to end (the codec pads the edges).
            assert!(peaks[5..40].iter().all(|p| *p > 0.2), "{name}: {peaks:?}");
        }
    }

    #[test]
    fn a_file_that_is_not_audio_has_no_peaks() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("broken.mp3");
        std::fs::write(&p, b"not audio at all").unwrap();
        assert!(audio_peaks(&p, 100).is_none());
        let p = dir.path().join("cut.wav");
        std::fs::write(&p, b"RIFF\x24\0\0\0WAVEfmt ").unwrap();
        assert!(audio_peaks(&p, 100).is_none());
        assert!(audio_peaks(&dir.path().join("missing.wav"), 100).is_none());
    }

    #[test]
    fn a_pdf_gives_its_page_count_and_drawn_pages() {
        let dir = tempfile::tempdir().unwrap();
        let pdf = dir.path().join("巡检 报告.pdf");
        std::fs::write(&pdf, crate::views::media::preview::report_pdf()).unwrap();
        let (count, pages) = pdf_pages(&pdf, 306, 2).expect("the pages of a PDF");
        assert_eq!(count, 6);
        assert_eq!(pages.len(), 2);
        for b in &pages {
            // A4, 595 x 842 pt.
            assert_eq!((b.width, b.height), (306, 433));
            assert_eq!(b.rgba.len(), 306 * 433 * 4);
            // White paper with dark text on it, nothing transparent.
            assert_eq!(b.rgba[..4], [255, 255, 255, 255]);
            assert!(b.rgba.chunks_exact(4).all(|p| p[3] == 255));
            let dark = b
                .rgba
                .chunks_exact(4)
                .filter(|p| p[0] < 96 && p[1] < 96 && p[2] < 96)
                .count();
            assert!(dark > 50, "{dark} dark pixels");
            assert!(dark < 306 * 433 / 2, "{dark} dark pixels");
        }
        // The title is on the upper half of the first page: the page is not upside down.
        let dark_rows = |b: &Bitmap, rows: std::ops::Range<usize>| {
            b.rgba[rows.start * 306 * 4..rows.end * 306 * 4]
                .chunks_exact(4)
                .filter(|p| p[0] < 96)
                .count()
        };
        assert!(dark_rows(&pages[0], 0..40) > 0, "no title at the top");
        assert_eq!(
            dark_rows(&pages[0], 420..433),
            0,
            "ink in the bottom margin"
        );
    }

    #[test]
    fn a_file_that_is_not_a_pdf_has_no_pages() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("broken.pdf");
        std::fs::write(&p, b"not a pdf").unwrap();
        assert!(pdf_pages(&p, 300, 3).is_none());
        let whole = crate::views::media::preview::report_pdf();
        let p = dir.path().join("cut.pdf");
        std::fs::write(&p, &whole[..whole.len() / 3]).unwrap();
        // A PDF cut short may be repaired by the reader, but it must answer.
        let _ = pdf_pages(&p, 300, 3);
        assert!(pdf_pages(&dir.path().join("missing.pdf"), 300, 3).is_none());
    }

    /// The CI machine has no sound card: this checks that the controls answer and
    /// nothing blocks or crashes, not that sound comes out.
    #[cfg(target_os = "windows")]
    #[test]
    fn the_audio_player_takes_its_controls_without_a_sound_card() {
        use std::time::{Duration, Instant};
        let dir = tempfile::tempdir().unwrap();
        let wav = dir.path().join("周会 录音.wav");
        std::fs::write(&wav, crate::views::media::preview::speech_wav(5)).unwrap();
        let started = Instant::now();
        let mut player = AudioPlayer::new(&wav).expect("a player");
        player.set_rate(1.5);
        player.seek(2.);
        player.play();
        assert!(player.is_playing(), "playing right after 播放");
        assert!(
            started.elapsed() < Duration::from_millis(500),
            "the controls waited"
        );
        std::thread::sleep(Duration::from_secs(2));
        println!(
            "audio player after 2 s: playing {}, position {:.2}",
            player.is_playing(),
            player.position()
        );
        player.pause();
        assert!(!player.is_playing());
        player.seek(1.);
        std::thread::sleep(Duration::from_millis(300));
        println!(
            "audio player after seek to 1 s: position {:.2}",
            player.position()
        );
        drop(player);

        // A file that is not audio: the player exists and reports that nothing plays.
        let broken = dir.path().join("broken.mp3");
        std::fs::write(&broken, b"not audio").unwrap();
        let mut player = AudioPlayer::new(&broken).expect("a player");
        player.play();
        std::thread::sleep(Duration::from_secs(2));
        println!("broken file: playing {}", player.is_playing());
        assert_eq!(player.position(), 0.);
    }
}
