//! The Windows side of [`super`], with the frameworks Windows ships:
//!
//! - video and audio samples: Media Foundation's source reader (`IMFSourceReader`);
//! - PDF pages: `Windows.Data.Pdf`;
//! - audio playback: `Windows.Media.Playback.MediaPlayer`.
//!
//! Threads. The process joins the COM multithreaded apartment once
//! (`CoIncrementMTAUsage`), so a thread that never initialized COM can use it. Every
//! decode runs on a thread of its own and the caller waits for it with a time limit: a
//! file that makes a decoder hang costs one parked thread, not the caller. The player
//! is opened on a thread too; the controls only send it short commands.
//!
//! Media Foundation is loaded by name at the first use and not linked: Windows N
//! editions and Windows Server ship without it, and the app must still start there.
//! A file Windows has no decoder for (HEVC without the store extension, for one)
//! answers `None`, like a file that is not a video at all.

use std::{
    ffi::c_void,
    mem::ManuallyDrop,
    path::Path,
    sync::{
        Arc, Mutex, MutexGuard, OnceLock, Weak,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    time::{Duration, Instant},
};

use windows::{
    Data::Pdf::{PdfDocument, PdfPage, PdfPageRenderOptions},
    Foundation::{TimeSpan, TypedEventHandler},
    Media::{
        Core::MediaSource,
        Playback::{MediaPlaybackState, MediaPlayer},
    },
    Storage::{
        StorageFile,
        Streams::{DataReader, DataWriter, InMemoryRandomAccessStream},
    },
    Win32::{
        Media::MediaFoundation::{
            IMF2DBuffer, IMFAttributes, IMFMediaType, IMFSample, IMFSourceReader,
            MF_MT_AUDIO_BITS_PER_SAMPLE, MF_MT_AUDIO_NUM_CHANNELS, MF_MT_AUDIO_SAMPLES_PER_SECOND,
            MF_MT_DEFAULT_STRIDE, MF_MT_FRAME_SIZE, MF_MT_MAJOR_TYPE,
            MF_MT_MINIMUM_DISPLAY_APERTURE, MF_MT_SUBTYPE, MF_MT_VIDEO_ROTATION, MF_PD_DURATION,
            MF_SOURCE_READER_ALL_STREAMS, MF_SOURCE_READER_ENABLE_ADVANCED_VIDEO_PROCESSING,
            MF_SOURCE_READER_FIRST_AUDIO_STREAM, MF_SOURCE_READER_FIRST_VIDEO_STREAM,
            MF_SOURCE_READER_MEDIASOURCE, MF_SOURCE_READERF_CURRENTMEDIATYPECHANGED,
            MF_SOURCE_READERF_ENDOFSTREAM, MF_SOURCE_READERF_ERROR, MF_VERSION,
            MFAudioFormat_Float, MFAudioFormat_PCM, MFMediaType_Audio, MFMediaType_Video,
            MFSTARTUP_FULL, MFVideoFormat_RGB32,
        },
        System::{
            Com::{
                CoIncrementMTAUsage,
                StructuredStorage::{
                    PROPVARIANT, PROPVARIANT_0, PROPVARIANT_0_0, PROPVARIANT_0_0_0,
                },
            },
            LibraryLoader::{GetProcAddress, LOAD_LIBRARY_SEARCH_SYSTEM32, LoadLibraryExW},
            Variant::{VT_I8, VT_UI8},
        },
    },
    core::{GUID, HRESULT, HSTRING, IInspectable, Interface, PCWSTR, Ref, s, w},
};

use super::{Bitmap, VideoInfo};

/// The longest side of a picture this module hands out (the same bound as on macOS).
const MAX_SIDE: u32 = 8192;

const VIDEO: u32 = MF_SOURCE_READER_FIRST_VIDEO_STREAM.0 as u32;
const AUDIO: u32 = MF_SOURCE_READER_FIRST_AUDIO_STREAM.0 as u32;
const ALL_STREAMS: u32 = MF_SOURCE_READER_ALL_STREAMS.0 as u32;
const SOURCE: u32 = MF_SOURCE_READER_MEDIASOURCE.0 as u32;

/// Keeps the multithreaded apartment alive for the life of the process, so objects
/// made on a short-lived thread stay valid after it ends.
fn com() -> bool {
    static MTA: OnceLock<bool> = OnceLock::new();
    *MTA.get_or_init(|| unsafe { CoIncrementMTAUsage() }.is_ok())
}

/// Set when the caller stopped waiting: the worker checks it between samples and pages
/// and gives up, so a decode nobody waits for does not go on using CPU and memory.
type Stop = AtomicBool;

fn stopped(stop: &Stop) -> bool {
    stop.load(Ordering::Relaxed)
}

/// Runs `f` on a thread of its own and waits at most `limit` for the answer. After
/// that the thread is told to stop.
fn run<T: Send + 'static>(
    limit: Duration,
    f: impl FnOnce(&Stop) -> Option<T> + Send + 'static,
) -> Option<T> {
    let (tx, rx) = mpsc::channel();
    let stop = Arc::new(Stop::new(false));
    let theirs = stop.clone();
    std::thread::Builder::new()
        .name("media-native".into())
        .spawn(move || {
            let _ = tx.send(f(&theirs));
        })
        .ok()?;
    let answer = rx.recv_timeout(limit).ok().flatten();
    stop.store(true, Ordering::Relaxed);
    answer
}

/// The absolute path with backslashes and without the `\\?\` prefix: the form the
/// WinRT file APIs and Media Foundation accept.
fn full_path(path: &Path) -> Option<HSTRING> {
    let p = std::path::absolute(path).ok()?;
    let s = p.to_str()?;
    let s = if let Some(rest) = s.strip_prefix(r"\\?\UNC\") {
        format!(r"\\{rest}")
    } else if let Some(rest) = s.strip_prefix(r"\\?\") {
        rest.to_string()
    } else {
        s.to_string()
    };
    Some(HSTRING::from(s))
}

// --- Media Foundation, loaded by name ----------------------------------------------------

struct Mf {
    create_attributes: unsafe extern "system" fn(*mut *mut c_void, u32) -> HRESULT,
    create_media_type: unsafe extern "system" fn(*mut *mut c_void) -> HRESULT,
    create_reader: unsafe extern "system" fn(PCWSTR, *mut c_void, *mut *mut c_void) -> HRESULT,
}

fn mf() -> Option<&'static Mf> {
    static MF: OnceLock<Option<Mf>> = OnceLock::new();
    MF.get_or_init(|| unsafe {
        if !com() {
            return None;
        }
        let plat = LoadLibraryExW(w!("mfplat.dll"), None, LOAD_LIBRARY_SEARCH_SYSTEM32).ok()?;
        let rw = LoadLibraryExW(w!("mfreadwrite.dll"), None, LOAD_LIBRARY_SEARCH_SYSTEM32).ok()?;
        let startup: unsafe extern "system" fn(u32, u32) -> HRESULT =
            std::mem::transmute(GetProcAddress(plat, s!("MFStartup"))?);
        let mf = Mf {
            create_attributes: std::mem::transmute(GetProcAddress(plat, s!("MFCreateAttributes"))?),
            create_media_type: std::mem::transmute(GetProcAddress(plat, s!("MFCreateMediaType"))?),
            create_reader: std::mem::transmute(GetProcAddress(
                rw,
                s!("MFCreateSourceReaderFromURL"),
            )?),
        };
        // Never shut down: the readers live on worker threads until the process ends.
        startup(MF_VERSION, MFSTARTUP_FULL).ok().ok()?;
        Some(mf)
    })
    .as_ref()
}

impl Mf {
    unsafe fn reader(&self, path: &Path, video: bool) -> Option<IMFSourceReader> {
        unsafe {
            let mut raw = std::ptr::null_mut();
            (self.create_attributes)(&mut raw, 1).ok().ok()?;
            let attributes = IMFAttributes::from_raw(raw);
            if video {
                // Lets the reader convert the decoder's YUV picture to RGB32.
                attributes
                    .SetUINT32(&MF_SOURCE_READER_ENABLE_ADVANCED_VIDEO_PROCESSING, 1)
                    .ok()?;
            }
            let url = full_path(path)?;
            let mut raw = std::ptr::null_mut();
            (self.create_reader)(PCWSTR(url.as_ptr()), attributes.as_raw(), &mut raw)
                .ok()
                .ok()?;
            (!raw.is_null()).then(|| IMFSourceReader::from_raw(raw))
        }
    }

    unsafe fn media_type(&self, major: &GUID, subtype: &GUID) -> Option<IMFMediaType> {
        unsafe {
            let mut raw = std::ptr::null_mut();
            (self.create_media_type)(&mut raw).ok().ok()?;
            let ty = IMFMediaType::from_raw(raw);
            ty.SetGUID(&MF_MT_MAJOR_TYPE, major).ok()?;
            ty.SetGUID(&MF_MT_SUBTYPE, subtype).ok()?;
            Some(ty)
        }
    }
}

/// The length of the file in seconds, from the presentation attributes.
unsafe fn duration(reader: &IMFSourceReader) -> Option<f64> {
    unsafe {
        let v = reader
            .GetPresentationAttribute(SOURCE, &MF_PD_DURATION)
            .ok()?;
        let inner = &v.Anonymous.Anonymous;
        (inner.vt == VT_UI8).then(|| inner.Anonymous.uhVal as f64 / 1e7)
    }
}

fn position(ticks: i64) -> PROPVARIANT {
    PROPVARIANT {
        Anonymous: PROPVARIANT_0 {
            Anonymous: ManuallyDrop::new(PROPVARIANT_0_0 {
                vt: VT_I8,
                wReserved1: 0,
                wReserved2: 0,
                wReserved3: 0,
                Anonymous: PROPVARIANT_0_0_0 { hVal: ticks },
            }),
        },
    }
}

// --- video -------------------------------------------------------------------------------

/// The most samples read after a seek to reach the asked time (the reader starts at the
/// key frame before it).
const MAX_SAMPLES_PER_FRAME: usize = 1200;

struct Video {
    reader: IMFSourceReader,
    duration: f64,
    /// `MF_MT_VIDEO_ROTATION` of the file and its size before any rotation.
    rotation: u32,
    coded: (u32, u32),
    /// Degrees to turn the decoded picture clockwise to make it upright.
    turn: u32,
    /// The decoded picture.
    width: u32,
    height: u32,
    stride: i32,
    /// The part of it to show: x, y, width, height.
    crop: (u32, u32, u32, u32),
}

impl Video {
    unsafe fn open(path: &Path) -> Option<Self> {
        unsafe {
            let mf = mf()?;
            let reader = mf.reader(path, true)?;
            let native = reader.GetNativeMediaType(VIDEO, 0).ok()?;
            let rotation = native.GetUINT32(&MF_MT_VIDEO_ROTATION).unwrap_or(0) % 360;
            let coded = native.GetUINT64(&MF_MT_FRAME_SIZE).unwrap_or(0);
            let coded = ((coded >> 32) as u32, coded as u32);
            let _ = reader.SetStreamSelection(ALL_STREAMS, false);
            reader.SetStreamSelection(VIDEO, true).ok()?;
            // Fails here when Windows has no decoder for the stream.
            let rgb = mf.media_type(&MFMediaType_Video, &MFVideoFormat_RGB32)?;
            reader.SetCurrentMediaType(VIDEO, None, &rgb).ok()?;
            let mut video = Self {
                duration: duration(&reader)?,
                reader,
                rotation,
                coded,
                turn: 0,
                width: 0,
                height: 0,
                stride: 0,
                crop: (0, 0, 0, 0),
            };
            video.read_format()?;
            Some(video)
        }
    }

    unsafe fn read_format(&mut self) -> Option<()> {
        unsafe {
            let ty = self.reader.GetCurrentMediaType(VIDEO).ok()?;
            let size = ty.GetUINT64(&MF_MT_FRAME_SIZE).ok()?;
            let (w, h) = ((size >> 32) as u32, size as u32);
            if w == 0 || h == 0 || w > MAX_SIDE || h > MAX_SIDE {
                return None;
            }
            self.width = w;
            self.height = h;
            // The reader's video processor turns a sideways recording upright itself
            // (seen on Windows Server 2025): its picture then has the sides swapped.
            // Only a picture that still has the file's own shape is turned here.
            let sideways = self.rotation == 90 || self.rotation == 270;
            let untouched = (w, h) == self.coded && w != h;
            self.turn = if sideways && untouched {
                self.rotation
            } else {
                0
            };
            self.stride = ty
                .GetUINT32(&MF_MT_DEFAULT_STRIDE)
                .map(|s| s as i32)
                .unwrap_or(w as i32 * 4);
            self.crop = (0, 0, w, h);
            // An `MFVideoArea`: two fixed-point offsets, then the size.
            let mut area = [0u8; 16];
            if ty
                .GetBlob(&MF_MT_MINIMUM_DISPLAY_APERTURE, &mut area, None)
                .is_ok()
            {
                let i16_at = |i: usize| i16::from_le_bytes([area[i], area[i + 1]]);
                let i32_at =
                    |i: usize| i32::from_le_bytes([area[i], area[i + 1], area[i + 2], area[i + 3]]);
                let (x, y, cw, ch) = (i16_at(2), i16_at(6), i32_at(8), i32_at(12));
                if x >= 0
                    && y >= 0
                    && cw > 0
                    && ch > 0
                    && x as u32 + cw as u32 <= w
                    && y as u32 + ch as u32 <= h
                {
                    self.crop = (x as u32, y as u32, cw as u32, ch as u32);
                }
            }
            Some(())
        }
    }

    /// The upright size.
    fn shown_size(&self) -> (u32, u32) {
        let (_, _, w, h) = self.crop;
        if self.turn == 90 || self.turn == 270 {
            (h, w)
        } else {
            (w, h)
        }
    }

    /// The frame shown at `t` seconds.
    unsafe fn frame(&mut self, t: f64, stop: &Stop) -> Option<Bitmap> {
        unsafe {
            let target = (t.max(0.) * 1e7) as i64;
            self.reader
                .SetCurrentPosition(&GUID::zeroed(), &position(target))
                .ok()?;
            let mut last: Option<IMFSample> = None;
            for _ in 0..MAX_SAMPLES_PER_FRAME {
                if stopped(stop) {
                    return None;
                }
                let (mut flags, mut time, mut sample) = (0u32, 0i64, None);
                if self
                    .reader
                    .ReadSample(
                        VIDEO,
                        0,
                        None,
                        Some(&mut flags),
                        Some(&mut time),
                        Some(&mut sample),
                    )
                    .is_err()
                    || flags & MF_SOURCE_READERF_ERROR.0 as u32 != 0
                {
                    break;
                }
                if flags & MF_SOURCE_READERF_CURRENTMEDIATYPECHANGED.0 as u32 != 0 {
                    self.read_format()?;
                }
                if let Some(sample) = sample {
                    let length = sample.GetSampleDuration().unwrap_or(0).max(1);
                    last = Some(sample);
                    // Both come from the file.
                    if time.saturating_add(length) > target {
                        break;
                    }
                }
                if flags & MF_SOURCE_READERF_ENDOFSTREAM.0 as u32 != 0 {
                    break;
                }
            }
            // Past the end the last frame stands in.
            self.bitmap(&last?)
        }
    }

    unsafe fn bitmap(&self, sample: &IMFSample) -> Option<Bitmap> {
        unsafe {
            let buffer = sample.ConvertToContiguousBuffer().ok()?;
            let (x, y, w, h) = self.crop;
            let (x, y, w, h) = (x as usize, y as usize, w as usize, h as usize);
            let min_pitch = self.width as usize * 4;
            let mut rgba = vec![0u8; w * h * 4];
            // Row `row` of the decoded picture starts at `scan0 + row * pitch`; a
            // bottom-up picture has a negative pitch.
            let copy = |scan0: *const u8, pitch: isize, rgba: &mut [u8]| {
                for (row, out) in rgba.chunks_exact_mut(w * 4).enumerate() {
                    let line = scan0.offset((y + row) as isize * pitch).add(x * 4);
                    let line = std::slice::from_raw_parts(line, w * 4);
                    for (px, bgrx) in out.chunks_exact_mut(4).zip(line.chunks_exact(4)) {
                        px.copy_from_slice(&[bgrx[2], bgrx[1], bgrx[0], 255]);
                    }
                }
            };
            if let Ok(two_d) = buffer.cast::<IMF2DBuffer>() {
                let (mut scan0, mut pitch) = (std::ptr::null_mut(), 0i32);
                two_d.Lock2D(&mut scan0, &mut pitch).ok()?;
                // A sample decoded before a change of size is smaller than the size
                // the reader reports now: the buffer must hold every row that is read.
                let fits = !scan0.is_null()
                    && pitch.unsigned_abs() as usize >= min_pitch
                    && two_d
                        .GetContiguousLength()
                        .is_ok_and(|n| n as usize >= min_pitch * self.height as usize);
                if fits {
                    copy(scan0, pitch as isize, &mut rgba);
                }
                let _ = two_d.Unlock2D();
                if !fits {
                    return None;
                }
            } else {
                let (mut data, mut len) = (std::ptr::null_mut(), 0u32);
                buffer.Lock(&mut data, None, Some(&mut len)).ok()?;
                let pitch = self.stride.unsigned_abs() as usize;
                let rows = self.height as usize;
                let fits = !data.is_null() && pitch >= min_pitch && len as usize >= pitch * rows;
                if fits {
                    if self.stride < 0 {
                        copy(data.add(pitch * (rows - 1)), -(pitch as isize), &mut rgba);
                    } else {
                        copy(data, pitch as isize, &mut rgba);
                    }
                }
                let _ = buffer.Unlock();
                if !fits {
                    return None;
                }
            }
            let picture = image::RgbaImage::from_raw(w as u32, h as u32, rgba)?;
            let picture = match self.turn {
                90 => image::imageops::rotate90(&picture),
                180 => image::imageops::rotate180(&picture),
                270 => image::imageops::rotate270(&picture),
                _ => picture,
            };
            Some(Bitmap {
                width: picture.width(),
                height: picture.height(),
                rgba: picture.into_raw(),
            })
        }
    }
}

/// Scales `b` down so that its long side is at most `max` pixels.
fn fit(b: Bitmap, max: f64) -> Option<Bitmap> {
    let long = b.width.max(b.height) as f64;
    if max.is_nan() || max < 1. || long <= max {
        return Some(b);
    }
    let scale = max / long;
    let w = ((b.width as f64 * scale).round() as u32).max(1);
    let h = ((b.height as f64 * scale).round() as u32).max(1);
    let picture = image::RgbaImage::from_raw(b.width, b.height, b.rgba)?;
    let small = image::imageops::resize(&picture, w, h, image::imageops::FilterType::Triangle);
    Some(Bitmap {
        width: w,
        height: h,
        rgba: small.into_raw(),
    })
}

pub fn video_info(path: &Path) -> Option<VideoInfo> {
    let path = path.to_path_buf();
    run(Duration::from_secs(20), move |_| {
        let video = unsafe { Video::open(&path) }?;
        let (width, height) = video.shown_size();
        (video.duration.is_finite() && width > 0 && height > 0).then_some(VideoInfo {
            duration: video.duration.max(0.),
            width,
            height,
        })
    })
}

pub fn video_frames(path: &Path, times: &[f64], max_width: f64) -> Vec<Option<Bitmap>> {
    let path = path.to_path_buf();
    let asked: Vec<f64> = times.to_vec();
    let limit = Duration::from_secs(20 + 10 * times.len() as u64);
    run(limit, move |stop| {
        let mut video = unsafe { Video::open(&path) }?;
        Some(
            asked
                .iter()
                .map(|&t| unsafe { video.frame(t, stop) }.and_then(|b| fit(b, max_width)))
                .collect::<Vec<_>>(),
        )
    })
    .unwrap_or_else(|| times.iter().map(|_| None).collect())
}

// --- audio peaks -------------------------------------------------------------------------

/// How the decoder hands out samples.
#[derive(Clone, Copy)]
struct Pcm {
    float: bool,
    /// Bytes of one sample of one channel.
    bytes: usize,
}

impl Pcm {
    /// The size of one sample, 0 to 1.
    fn level(self, s: &[u8]) -> f32 {
        let v = match (self.float, self.bytes) {
            (true, 4) => f32::from_le_bytes([s[0], s[1], s[2], s[3]]),
            (true, _) => {
                f64::from_le_bytes([s[0], s[1], s[2], s[3], s[4], s[5], s[6], s[7]]) as f32
            }
            (false, 1) => (s[0] as f32 - 128.) / 128.,
            (false, 2) => i16::from_le_bytes([s[0], s[1]]) as f32 / 32_768.,
            (false, 3) => (i32::from_le_bytes([0, s[0], s[1], s[2]]) >> 8) as f32 / 8_388_608.,
            (false, _) => i32::from_le_bytes([s[0], s[1], s[2], s[3]]) as f32 / 2_147_483_648.,
        };
        if v.is_finite() { v.abs() } else { 0. }
    }
}

/// The format of the samples the reader hands out now: the sample layout, the
/// channels and the frames per second.
unsafe fn format(reader: &IMFSourceReader) -> Option<(Pcm, usize, f64)> {
    unsafe {
        let ty = reader.GetCurrentMediaType(AUDIO).ok()?;
        let subtype = ty.GetGUID(&MF_MT_SUBTYPE).ok()?;
        let float = subtype == MFAudioFormat_Float;
        if !float && subtype != MFAudioFormat_PCM {
            return None;
        }
        let channels = ty.GetUINT32(&MF_MT_AUDIO_NUM_CHANNELS).ok()? as usize;
        let rate = ty.GetUINT32(&MF_MT_AUDIO_SAMPLES_PER_SECOND).ok()? as f64;
        let bytes = ty.GetUINT32(&MF_MT_AUDIO_BITS_PER_SAMPLE).ok()? as usize / 8;
        let known = if float {
            matches!(bytes, 4 | 8)
        } else {
            matches!(bytes, 1..=4)
        };
        (channels > 0 && rate > 0. && known).then_some((Pcm { float, bytes }, channels, rate))
    }
}

/// The most peak groups kept while decoding (64 MB; days of audio in groups of 64).
const MAX_GROUPS: usize = 16 << 20;

unsafe fn peaks(path: &Path, buckets: usize, stop: &Stop) -> Option<(f64, Vec<f32>)> {
    unsafe {
        let mf = mf()?;
        let reader = mf.reader(path, false)?;
        let _ = reader.SetStreamSelection(ALL_STREAMS, false);
        reader.SetStreamSelection(AUDIO, true).ok()?;
        // Decoders answer integer PCM; a float WAV file only has float.
        let set = [MFAudioFormat_PCM, MFAudioFormat_Float]
            .iter()
            .any(|subtype| {
                mf.media_type(&MFMediaType_Audio, subtype)
                    .is_some_and(|ty| reader.SetCurrentMediaType(AUDIO, None, &ty).is_ok())
            });
        if !set {
            return None;
        }
        let (mut pcm, mut channels, mut rate) = format(&reader)?;
        let buckets = buckets.clamp(1, 4000);
        // The length is only known at the end (the header of an MP3 file guesses), so
        // the loudest sample of every small group of frames is kept and the groups are
        // spread over the buckets afterwards. A short file gets groups of one frame.
        let guessed_frames = duration(&reader).unwrap_or(0.) * rate;
        let group = if guessed_frames > 0. {
            ((guessed_frames / (buckets as f64 * 4.)) as usize).clamp(1, 1024)
        } else {
            64
        };
        let mut groups: Vec<f32> = Vec::new();
        let (mut loudest, mut in_group, mut frames, mut seconds) = (0f32, 0usize, 0u64, 0f64);
        loop {
            if stopped(stop) {
                return None;
            }
            let (mut flags, mut sample) = (0u32, None);
            if reader
                .ReadSample(AUDIO, 0, None, Some(&mut flags), None, Some(&mut sample))
                .is_err()
                || flags & MF_SOURCE_READERF_ERROR.0 as u32 != 0
            {
                break;
            }
            // The decoder learned the real format from the stream (an AAC file whose
            // header says half the sample rate does this on its first sample).
            if flags & MF_SOURCE_READERF_CURRENTMEDIATYPECHANGED.0 as u32 != 0 {
                (pcm, channels, rate) = format(&reader)?;
            }
            let bytes = pcm.bytes;
            if let Some(sample) = sample {
                let buffer = sample.ConvertToContiguousBuffer().ok()?;
                let (mut data, mut len) = (std::ptr::null_mut(), 0u32);
                buffer.Lock(&mut data, None, Some(&mut len)).ok()?;
                if !data.is_null() {
                    let data = std::slice::from_raw_parts(data, len as usize);
                    for frame in data.chunks_exact(channels * bytes) {
                        for s in frame.chunks_exact(bytes) {
                            loudest = loudest.max(pcm.level(s));
                        }
                        in_group += 1;
                        if in_group == group {
                            groups.push(loudest);
                            (loudest, in_group) = (0., 0);
                        }
                    }
                    let n = len as usize / (channels * bytes);
                    frames += n as u64;
                    seconds += n as f64 / rate;
                }
                let _ = buffer.Unlock();
            }
            if flags & MF_SOURCE_READERF_ENDOFSTREAM.0 as u32 != 0 || groups.len() >= MAX_GROUPS {
                break;
            }
        }
        if in_group > 0 {
            groups.push(loudest);
        }
        if frames == 0 {
            return None;
        }
        let per_bucket = (frames as f64 / buckets as f64).max(1.);
        let mut out = vec![0f32; buckets];
        for (i, v) in groups.iter().enumerate() {
            let b = (((i * group) as f64 / per_bucket) as usize).min(buckets - 1);
            if *v > out[b] {
                out[b] = *v;
            }
        }
        let max = out.iter().cloned().fold(0f32, f32::max);
        if max > 0. {
            for p in &mut out {
                *p /= max;
            }
        }
        Some((seconds, out))
    }
}

pub fn audio_peaks(path: &Path, buckets: usize) -> Option<(f64, Vec<f32>)> {
    let path = path.to_path_buf();
    run(Duration::from_secs(180), move |stop| unsafe {
        peaks(&path, buckets, stop)
    })
}

// --- PDF ---------------------------------------------------------------------------------

fn open_file(path: &Path) -> Option<StorageFile> {
    if !com() {
        return None;
    }
    StorageFile::GetFileFromPathAsync(&full_path(path)?)
        .ok()?
        .join()
        .ok()
}

/// A one-page empty PDF.
fn blank_pdf() -> Vec<u8> {
    let objects = [
        "<< /Type /Catalog /Pages 2 0 R >>",
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 8 8] >>",
    ];
    let mut out = String::from("%PDF-1.4\n");
    let mut offsets = Vec::new();
    for (i, body) in objects.iter().enumerate() {
        offsets.push(out.len());
        out.push_str(&format!("{} 0 obj\n{body}\nendobj\n", i + 1));
    }
    let xref = out.len();
    out.push_str(&format!(
        "xref\n0 {}\n0000000000 65535 f \n",
        objects.len() + 1
    ));
    for offset in offsets {
        out.push_str(&format!("{offset:010} 00000 n \n"));
    }
    out.push_str(&format!(
        "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n",
        objects.len() + 1
    ));
    out.into_bytes()
}

/// Keeps one document and its page open until the process ends.
///
/// `Windows.Data.Pdf.dll` owns a Direct3D device that its documents share. When the
/// last of them is gone, the DLL releases the device in a static destructor while the
/// process exits; on the software renderer (a machine without a GPU, a VM) that
/// release needs threads that are already gone, and the process ends as a crash
/// (0xC000000D in `d3d10warp!Task::ScheduleTask`, seen on Windows Server 2025). A
/// document that is never closed keeps the device out of that destructor.
pub(super) fn keep_pdf_engine() -> bool {
    static KEPT: OnceLock<Option<(PdfDocument, PdfPage)>> = OnceLock::new();
    KEPT.get_or_init(|| {
        let stream = InMemoryRandomAccessStream::new().ok()?;
        let writer = DataWriter::CreateDataWriter(&stream).ok()?;
        writer.WriteBytes(&blank_pdf()).ok()?;
        writer.StoreAsync().ok()?.join().ok()?;
        writer.DetachStream().ok()?;
        stream.Seek(0).ok()?;
        let doc = PdfDocument::LoadFromStreamAsync(&stream)
            .ok()?
            .join()
            .ok()?;
        let page = doc.GetPage(0).ok()?;
        // Drawing is what makes the device.
        draw_page(&page, 8, 8)?;
        Some((doc, page))
    })
    .is_some()
}

/// Draws `page` as a `width` x `height` picture on white.
fn draw_page(page: &PdfPage, width: u32, height: u32) -> Option<Bitmap> {
    let options = PdfPageRenderOptions::new().ok()?;
    options.SetDestinationWidth(width).ok()?;
    options.SetDestinationHeight(height).ok()?;
    let stream = InMemoryRandomAccessStream::new().ok()?;
    page.RenderWithOptionsToStreamAsync(&stream, &options)
        .ok()?
        .join()
        .ok()?;
    // The stream holds an encoded picture (PNG unless asked otherwise).
    let len = u32::try_from(stream.Size().ok()?).ok()?;
    let reader = DataReader::CreateDataReader(&stream.GetInputStreamAt(0).ok()?).ok()?;
    reader.LoadAsync(len).ok()?.join().ok()?;
    let mut encoded = vec![0u8; len as usize];
    reader.ReadBytes(&mut encoded).ok()?;
    let mut picture = image::load_from_memory(&encoded).ok()?.to_rgba8();
    for px in picture.pixels_mut() {
        let a = px[3] as u32;
        if a < 255 {
            for c in 0..3 {
                px[c] = ((px[c] as u32 * a + 255 * (255 - a)) / 255) as u8;
            }
            px[3] = 255;
        }
    }
    Some(Bitmap {
        width: picture.width(),
        height: picture.height(),
        rgba: picture.into_raw(),
    })
}

pub fn pdf_pages(path: &Path, width: u32, max_pages: usize) -> Option<(usize, Vec<Bitmap>)> {
    if width == 0 || width > MAX_SIDE {
        return None;
    }
    let path = path.to_path_buf();
    let limit = Duration::from_secs(30 + 5 * max_pages.min(600) as u64);
    run(limit, move |stop| {
        let file = open_file(&path)?;
        let _ = keep_pdf_engine();
        // Fails for a file that is not a PDF and for one that needs a password.
        let doc = PdfDocument::LoadFromFileAsync(&file).ok()?.join().ok()?;
        let count = doc.PageCount().ok()? as usize;
        let mut out = Vec::new();
        for i in 0..count.min(max_pages) {
            if stopped(stop) {
                return None;
            }
            let Ok(page) = doc.GetPage(i as u32) else {
                break;
            };
            // The size is the page as shown, its rotation applied.
            let Ok(size) = page.Size() else { continue };
            if !(size.Width > 0. && size.Height > 0.) {
                continue;
            }
            let height = (size.Height as f64 * width as f64 / size.Width as f64)
                .round()
                .max(1.);
            if height > MAX_SIDE as f64 {
                continue;
            }
            if let Some(b) = draw_page(&page, width, height as u32) {
                out.push(b);
            }
        }
        Some((count, out))
    })
}

// --- audio playback ----------------------------------------------------------------------

/// How long after 播放 the player counts as playing whatever it reports: it takes a
/// moment to leave "paused".
const STARTING: Duration = Duration::from_millis(600);

#[derive(Default)]
struct Shared {
    /// `None` while the file opens, and after it failed to.
    player: Option<MediaPlayer>,
    /// The player knows the file: seeking works from here on.
    opened: bool,
    failed: bool,
    dropped: bool,
    playing: bool,
    played_at: Option<Instant>,
    rate: f32,
    /// A position asked for before the file was open.
    seek: Option<f64>,
}

fn lock(shared: &Mutex<Shared>) -> MutexGuard<'_, Shared> {
    shared.lock().unwrap_or_else(|e| e.into_inner())
}

fn ticks(seconds: f64) -> TimeSpan {
    TimeSpan {
        Duration: (seconds.max(0.) * 1e7) as i64,
    }
}

fn open_player(path: &Path, shared: &Arc<Mutex<Shared>>) -> Option<()> {
    let file = open_file(path)?;
    let source = MediaSource::CreateFromStorageFile(&file).ok()?;
    let player = MediaPlayer::new().ok()?;
    // No entry in the system's media controls: this is a preview, not a music player.
    let _ = player.CommandManager().and_then(|m| m.SetIsEnabled(false));
    let _ = player.SetAutoPlay(false);
    let weak: Weak<Mutex<Shared>> = Arc::downgrade(shared);
    player
        .MediaOpened(&TypedEventHandler::new(
            move |player: Ref<MediaPlayer>, _: Ref<IInspectable>| {
                let (Some(shared), Some(player)) = (weak.upgrade(), player.as_ref()) else {
                    return Ok(());
                };
                let (seek, rate) = {
                    let mut s = lock(&shared);
                    s.opened = true;
                    (s.seek.take(), s.rate)
                };
                let session = player.PlaybackSession()?;
                if let Some(t) = seek {
                    let _ = session.SetPosition(ticks(t));
                }
                let _ = session.SetPlaybackRate(rate as f64);
                Ok(())
            },
        ))
        .ok()?;
    player.SetSource(&source).ok()?;
    let mut s = lock(shared);
    if s.dropped {
        drop(s);
        let _ = player.Close();
        return Some(());
    }
    if let Ok(session) = player.PlaybackSession() {
        let _ = session.SetPlaybackRate(s.rate as f64);
        // The file may have opened before this thread took the lock.
        if s.opened
            && let Some(t) = s.seek.take()
        {
            let _ = session.SetPosition(ticks(t));
        }
    }
    if s.playing {
        let _ = player.Play();
    }
    s.player = Some(player);
    Some(())
}

/// Plays one audio file from GPUI controls. The file opens on a thread of its own;
/// what is asked before it is open (播放, a position, a speed) applies once it is.
pub struct AudioPlayer {
    shared: Arc<Mutex<Shared>>,
}

impl AudioPlayer {
    pub fn new(path: &Path) -> Option<Self> {
        let path = path.to_path_buf();
        let shared = Arc::new(Mutex::new(Shared {
            rate: 1.,
            ..Shared::default()
        }));
        let theirs = shared.clone();
        std::thread::Builder::new()
            .name("media-audio".into())
            .spawn(move || {
                if open_player(&path, &theirs).is_none() {
                    lock(&theirs).failed = true;
                }
            })
            .ok()?;
        Some(Self { shared })
    }

    fn player(&self) -> Option<MediaPlayer> {
        lock(&self.shared).player.clone()
    }

    pub fn play(&mut self) {
        let player = {
            let mut s = lock(&self.shared);
            s.playing = true;
            s.played_at = Some(Instant::now());
            s.player.clone()
        };
        if let Some(p) = player {
            let _ = p.Play();
        }
    }

    pub fn pause(&mut self) {
        let player = {
            let mut s = lock(&self.shared);
            s.playing = false;
            s.player.clone()
        };
        if let Some(p) = player {
            let _ = p.Pause();
        }
    }

    pub fn is_playing(&self) -> bool {
        let (player, starting) = {
            let s = lock(&self.shared);
            if !s.playing || s.failed {
                return false;
            }
            (
                s.player.clone(),
                s.played_at.is_some_and(|t| t.elapsed() < STARTING),
            )
        };
        // Still opening: it plays as soon as it can.
        let Some(player) = player else { return true };
        starting
            || player
                .PlaybackSession()
                .and_then(|s| s.PlaybackState())
                .is_ok_and(|state| {
                    state == MediaPlaybackState::Playing
                        || state == MediaPlaybackState::Buffering
                        || state == MediaPlaybackState::Opening
                })
    }

    pub fn position(&self) -> f64 {
        let player = {
            let s = lock(&self.shared);
            if !s.opened || s.player.is_none() {
                return s.seek.unwrap_or(0.);
            }
            s.player.clone()
        };
        player
            .and_then(|p| p.PlaybackSession().ok())
            .and_then(|s| s.Position().ok())
            .map(|t| (t.Duration as f64 / 1e7).max(0.))
            .unwrap_or(0.)
    }

    pub fn seek(&mut self, seconds: f64) {
        let player = {
            let mut s = lock(&self.shared);
            if !s.opened || s.player.is_none() {
                s.seek = Some(seconds.max(0.));
                return;
            }
            s.player.clone()
        };
        if let Some(session) = player.and_then(|p| p.PlaybackSession().ok()) {
            let _ = session.SetPosition(ticks(seconds));
        }
    }

    pub fn set_rate(&mut self, rate: f32) {
        lock(&self.shared).rate = rate;
        if let Some(session) = self.player().and_then(|p| p.PlaybackSession().ok()) {
            let _ = session.SetPlaybackRate(rate as f64);
        }
    }
}

impl Drop for AudioPlayer {
    fn drop(&mut self) {
        let player = {
            let mut s = lock(&self.shared);
            s.dropped = true;
            s.player.take()
        };
        if let Some(p) = player {
            let _ = p.Pause();
            // Closing waits for the pipeline: not on the caller's thread.
            let _ = std::thread::Builder::new()
                .name("media-audio".into())
                .spawn(move || {
                    let _ = p.Close();
                });
        }
    }
}

/// What Media Foundation says about a file, step by step (for a failing test's log).
#[cfg(test)]
pub fn describe(path: &Path) -> String {
    let path = path.to_path_buf();
    run(Duration::from_secs(30), move |_| unsafe {
        let mut out = String::new();
        let Some(mf) = mf() else {
            return Some("no Media Foundation".to_string());
        };
        let Some(reader) = mf.reader(&path, true) else {
            return Some("no source reader".to_string());
        };
        for (name, stream) in [("video", VIDEO), ("audio", AUDIO)] {
            for i in 0..4 {
                match reader.GetNativeMediaType(stream, i) {
                    Ok(ty) => out.push_str(&format!(
                        "{name} native {i}: subtype {:?}, rotation {:?}, size {:?}, rate {:?}, channels {:?}\n",
                        ty.GetGUID(&MF_MT_SUBTYPE),
                        ty.GetUINT32(&MF_MT_VIDEO_ROTATION),
                        ty.GetUINT64(&MF_MT_FRAME_SIZE).map(|s| (s >> 32, s as u32)),
                        ty.GetUINT32(&MF_MT_AUDIO_SAMPLES_PER_SECOND),
                        ty.GetUINT32(&MF_MT_AUDIO_NUM_CHANNELS),
                    )),
                    Err(e) => {
                        out.push_str(&format!("{name} native {i}: {e:?}\n"));
                        break;
                    }
                }
            }
        }
        let _ = reader.SetStreamSelection(ALL_STREAMS, true);
        for (name, stream, major, subtype) in [
            ("rgb32", VIDEO, MFMediaType_Video, MFVideoFormat_RGB32),
            ("pcm", AUDIO, MFMediaType_Audio, MFAudioFormat_PCM),
            ("float", AUDIO, MFMediaType_Audio, MFAudioFormat_Float),
        ] {
            let Some(ty) = mf.media_type(&major, &subtype) else {
                continue;
            };
            let set = reader.SetCurrentMediaType(stream, None, &ty);
            out.push_str(&format!("set {name}: {set:?}\n"));
            if set.is_ok() {
                if let Ok(now) = reader.GetCurrentMediaType(stream) {
                    out.push_str(&format!(
                        "  now: subtype {:?}, rotation {:?}, size {:?}, stride {:?}, bits {:?}\n",
                        now.GetGUID(&MF_MT_SUBTYPE),
                        now.GetUINT32(&MF_MT_VIDEO_ROTATION),
                        now.GetUINT64(&MF_MT_FRAME_SIZE).map(|s| (s >> 32, s as u32)),
                        now.GetUINT32(&MF_MT_DEFAULT_STRIDE).map(|s| s as i32),
                        now.GetUINT32(&MF_MT_AUDIO_BITS_PER_SAMPLE),
                    ));
                }
                let (mut flags, mut sample) = (0u32, None);
                let read =
                    reader.ReadSample(stream, 0, None, Some(&mut flags), None, Some(&mut sample));
                out.push_str(&format!(
                    "  read: {read:?}, flags {flags:#x}, sample {}\n",
                    sample.is_some()
                ));
            }
        }
        Some(out)
    })
    .unwrap_or_else(|| "no answer in 30 s".to_string())
}
