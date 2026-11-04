//! AppKit draws one symbol into a bitmap; the rest is plain Rust on bytes.

use gpui::{RenderImage, Rgba};

use super::Weight;

/// The coverage of one symbol in a square slot, at device pixels.
#[derive(Clone, Debug)]
pub struct Mask {
    /// The side of the slot in device pixels.
    pub side: u32,
    /// Row-major coverage, top row first.
    pub alpha: Vec<u8>,
    /// The point size AppKit drew at (smaller than asked when `fitted`).
    pub point: f32,
    /// The ink was larger than the slot at the asked size.
    pub fitted: bool,
    /// Ink box in device pixels: x0, y0, x1, y1 (exclusive), in the slot.
    pub ink: (u32, u32, u32, u32),
    /// The scale factor the mask is drawn at.
    pub scale: f32,
}

/// Coverage above this counts as ink (5 %).
pub const INK: u8 = 12;

impl Mask {
    /// How far the ink box center is from the slot center, in points
    /// (x, y; positive is right and down).
    pub fn center_error(&self) -> (f32, f32) {
        let (x0, y0, x1, y1) = self.ink;
        let half = self.side as f32 / 2.;
        let cx = (x0 + x1) as f32 / 2. - half;
        let cy = (y0 + y1) as f32 / 2. - half;
        (cx / self.scale, cy / self.scale)
    }

    /// The mask times a color: straight BGRA, as GPUI's own decoders give.
    pub fn colored(&self, color: Rgba) -> RenderImage {
        let b = |v: f32| (v.clamp(0., 1.) * 255.).round() as u8;
        let (r, g, bl, a) = (b(color.r), b(color.g), b(color.b), color.a.clamp(0., 1.));
        let mut bytes = Vec::with_capacity(self.alpha.len() * 4);
        for &cov in &self.alpha {
            bytes.extend_from_slice(&[bl, g, r, (cov as f32 * a).round() as u8]);
        }
        let buffer =
            image::RgbaImage::from_raw(self.side, self.side, bytes).expect("side * side * 4 bytes");
        RenderImage::new(vec![image::Frame::new(buffer)])
    }
}

/// A canvas of coverage: what AppKit drew, before it is put in the slot.
struct Canvas {
    w: u32,
    h: u32,
    alpha: Vec<u8>,
}

impl Canvas {
    fn ink(&self) -> Option<(u32, u32, u32, u32)> {
        ink_box(&self.alpha, self.w, self.h)
    }
}

/// The box of the pixels with coverage over [`INK`]: x0, y0, x1, y1
/// (exclusive).
pub fn ink_box(alpha: &[u8], w: u32, h: u32) -> Option<(u32, u32, u32, u32)> {
    let (mut x0, mut y0, mut x1, mut y1) = (u32::MAX, u32::MAX, 0, 0);
    for y in 0..h {
        for x in 0..w {
            if alpha[(y * w + x) as usize] > INK {
                x0 = x0.min(x);
                y0 = y0.min(y);
                x1 = x1.max(x + 1);
                y1 = y1.max(y + 1);
            }
        }
    }
    (x0 != u32::MAX).then_some((x0, y0, x1, y1))
}

/// Draws `name` at `point` and centers its ink in a `slot` pt square at
/// `scale`. When the ink is larger than the slot it is drawn again at the
/// point size that fits.
pub fn draw(name: &str, point: f32, weight: Weight, slot: f32, scale: f32) -> Option<Mask> {
    draw_ink(name, point, weight, slot, scale, None)
}

/// [`draw`], and with `ink` (points) the point size is chosen so that the
/// larger side of the ink box is `ink`, whatever the symbol's own
/// proportions: one per-symbol scale, so a row of symbols (the Settings
/// tabs, §2.19) all fill the same square (review 3 K-06).
pub fn draw_ink(
    name: &str,
    point: f32,
    weight: Weight,
    slot: f32,
    scale: f32,
    ink: Option<f32>,
) -> Option<Mask> {
    let side = (slot * scale).round() as u32;
    let limit = ink.map_or(side, |i| ((i * scale).round() as u32).min(side));
    let mut point = point;
    let mut fitted = false;
    let mut canvas = appkit::draw(name, point, weight, scale)?;
    let (x0, y0, x1, y1) = canvas.ink()?;
    let big = (x1 - x0).max(y1 - y0);
    if big > limit || (ink.is_some() && big != limit) {
        // Ink grows linearly with the point size: start from the size that
        // scales the ink to the limit, then a quarter point lower until it
        // fits.
        let mut fit = point * limit as f32 / big as f32;
        fit = (fit * 4.).floor() / 4.;
        loop {
            let c = appkit::draw(name, fit, weight, scale)?;
            let (a, b, c1, d) = c.ink()?;
            if (c1 - a).max(d - b) <= limit || fit <= 4. {
                canvas = c;
                break;
            }
            fit -= 0.25;
        }
        point = fit;
        fitted = true;
    }
    let (x0, y0, x1, y1) = canvas.ink()?;
    // Whole device pixels: the glyph stays as AppKit drew it (crisp), off
    // center by at most half a device pixel.
    let dx = ((side as f32 - (x1 - x0) as f32) / 2.).round() as i64 - x0 as i64;
    let dy = ((side as f32 - (y1 - y0) as f32) / 2.).round() as i64 - y0 as i64;
    let mut alpha = vec![0u8; (side * side) as usize];
    for y in 0..canvas.h {
        let ty = y as i64 + dy;
        if ty < 0 || ty >= side as i64 {
            continue;
        }
        for x in 0..canvas.w {
            let tx = x as i64 + dx;
            if tx < 0 || tx >= side as i64 {
                continue;
            }
            alpha[(ty as u32 * side + tx as u32) as usize] =
                canvas.alpha[(y * canvas.w + x) as usize];
        }
    }
    let ink = ink_box(&alpha, side, side)?;
    Some(Mask {
        side,
        alpha,
        point,
        fitted,
        ink,
        scale,
    })
}

/// The first lookup of a symbol in this process (AppKit loads the symbol
/// catalog then): true when the symbol exists.
pub fn lookup(name: &str) -> bool {
    appkit::lookup(name)
}

#[cfg(target_os = "macos")]
mod appkit {
    use objc2::{AllocAnyThread as _, rc::Retained};
    use objc2_app_kit::{
        NSBitmapImageRep, NSDeviceRGBColorSpace, NSFontWeight, NSFontWeightLight,
        NSFontWeightMedium, NSFontWeightRegular, NSFontWeightSemibold, NSGraphicsContext, NSImage,
        NSImageSymbolConfiguration, NSImageSymbolScale,
    };
    use objc2_foundation::{NSPoint, NSRect, NSSize, NSString};

    use super::{Canvas, Weight};

    fn weight(w: Weight) -> NSFontWeight {
        // SAFETY: AppKit's weight constants are plain CGFloat statics.
        unsafe {
            match w {
                Weight::Light => NSFontWeightLight,
                Weight::Regular => NSFontWeightRegular,
                Weight::Medium => NSFontWeightMedium,
                Weight::Semibold => NSFontWeightSemibold,
            }
        }
    }

    fn symbol(name: &str, point: f32, w: Weight) -> Option<Retained<NSImage>> {
        let image = NSImage::imageWithSystemSymbolName_accessibilityDescription(
            &NSString::from_str(name),
            None,
        )?;
        let config = NSImageSymbolConfiguration::configurationWithPointSize_weight_scale(
            point as f64,
            weight(w),
            NSImageSymbolScale::Medium,
        );
        // Some symbols default to hierarchical (`display` draws its screen at
        // a lower opacity, which reads as a grey fill in a mask): one ink.
        let config = config.configurationByApplyingConfiguration(
            &NSImageSymbolConfiguration::configurationPreferringMonochrome(),
        );
        image.imageWithSymbolConfiguration(&config)
    }

    pub fn lookup(name: &str) -> bool {
        NSImage::imageWithSystemSymbolName_accessibilityDescription(&NSString::from_str(name), None)
            .is_some()
    }

    pub(super) fn draw(name: &str, point: f32, w: Weight, scale: f32) -> Option<Canvas> {
        let image = symbol(name, point, w)?;
        let size = image.size();
        // Some symbols draw outside their image box (`arrow.triangle.pull`
        // keeps only its arrow in it): a margin of half the point size on
        // every side keeps all of the ink.
        let pad = (point as f64 / 2.).ceil();
        let (pw, ph) = (size.width.ceil() + 2. * pad, size.height.ceil() + 2. * pad);
        let (wpx, hpx) = (
            (pw * scale as f64).round() as isize,
            (ph * scale as f64).round() as isize,
        );
        if wpx <= 0 || hpx <= 0 {
            return None;
        }
        // SAFETY: a fresh RGBA bitmap that owns its planes (null planes), 8 bits
        // per sample; AppKit draws into it through a context that is current
        // only between the save and restore below, on this thread.
        let rep = unsafe {
            NSBitmapImageRep::initWithBitmapDataPlanes_pixelsWide_pixelsHigh_bitsPerSample_samplesPerPixel_hasAlpha_isPlanar_colorSpaceName_bytesPerRow_bitsPerPixel(
                NSBitmapImageRep::alloc(),
                std::ptr::null_mut(),
                wpx,
                hpx,
                8,
                4,
                true,
                false,
                NSDeviceRGBColorSpace,
                0,
                0,
            )
        }?;
        rep.setSize(NSSize::new(pw, ph));
        let context = NSGraphicsContext::graphicsContextWithBitmapImageRep(&rep)?;
        NSGraphicsContext::saveGraphicsState_class();
        NSGraphicsContext::setCurrentContext(Some(&context));
        // The symbol's own box is its image size; the ink sits inside it.
        let x = ((pw - size.width) / 2.).round();
        let y = ((ph - size.height) / 2.).round();
        image.drawInRect(NSRect::new(NSPoint::new(x, y), size));
        context.flushGraphics();
        NSGraphicsContext::restoreGraphicsState_class();
        let data = rep.bitmapData();
        if data.is_null() {
            return None;
        }
        let row = rep.bytesPerRow() as usize;
        let (w, h) = (wpx as usize, hpx as usize);
        // SAFETY: the bitmap holds `row * h` bytes and outlives this read.
        let bytes = unsafe { std::slice::from_raw_parts(data, row * h) };
        let mut alpha = Vec::with_capacity(w * h);
        for y in 0..h {
            for x in 0..w {
                alpha.push(bytes[y * row + x * 4 + 3]);
            }
        }
        Some(Canvas {
            w: w as u32,
            h: h as u32,
            alpha,
        })
    }
}

#[cfg(not(target_os = "macos"))]
mod appkit {
    use super::{Canvas, Weight};

    pub fn lookup(_name: &str) -> bool {
        false
    }

    pub(super) fn draw(_name: &str, _point: f32, _w: Weight, _scale: f32) -> Option<Canvas> {
        None
    }
}
