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

/// Our own logic around Media Foundation: the decode timeout and the row bounds check.
/// What the system decoders return is checked by hand on a Mac or a Windows machine.
#[cfg(all(test, target_os = "windows"))]
mod tests {
    use super::*;

    #[test]
    fn a_decode_nobody_waits_for_stops() {
        assert!(win::tests_support::a_timed_out_worker_is_told_to_stop());
    }

    #[test]
    fn a_frame_is_only_read_from_inside_its_buffer() {
        win::tests_support::rows_stay_inside_the_buffer();
    }
}
