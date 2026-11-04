//! The spike's pass gates (POLISH-SPEC §10.2), measured in the running app:
//!
//! - cold: 40 symbols drawn at 1x and 2x in < 30 ms, in a process that has
//!   not drawn a symbol yet;
//! - crisp at 1x: the 1x masks next to Lucide at 1x (numbers and magnified
//!   sheets);
//! - centered in 16 pt slots within 0.5 pt;
//! - a theme switch recolors in the same frame (no blank frame).

use std::{
    path::Path,
    sync::{Mutex, OnceLock},
    time::Instant,
};

use gpui::{App, DevicePixels, SvgSize, size};
use serde_json::{Value, json};

use super::{Sf, Weight, point_for_slot, raster};

/// The cold-start numbers, measured once per process before the scene draws.
#[derive(Clone, Debug)]
pub struct Cold {
    /// A background thread drew one symbol before the timing started.
    pub prewarmed: bool,
    /// How long that background warm-up took.
    pub prewarm_ms: f64,
    /// The first symbol lookup of the process (AppKit loads the catalog).
    pub first_lookup_ms: f64,
    /// 40 masks at 1x, right after the first lookup.
    pub x1_ms: f64,
    /// 40 masks at 2x.
    pub x2_ms: f64,
    /// 40 colored images from the 2x masks.
    pub color_ms: f64,
    /// The same 40 at 1x and 2x again (AppKit warm).
    pub warm_ms: f64,
    pub missing: Vec<&'static str>,
    /// Symbols drawn smaller to fit the 16 pt slot: name and point size.
    pub fitted: Vec<(&'static str, f32)>,
}

static COLD: OnceLock<Cold> = OnceLock::new();

fn ms(t: Instant) -> f64 {
    t.elapsed().as_secs_f64() * 1000.
}

/// Measures the cold start on the first call of the process.
pub fn cold() -> &'static Cold {
    COLD.get_or_init(|| {
        // `TC_SF_PREWARM=1`: what a launch-time warm-up on a background thread
        // would leave for the main thread (the catalog load and the first draw).
        let prewarmed = std::env::var_os("TC_SF_PREWARM").is_some();
        let mut prewarm_ms = 0.;
        if prewarmed {
            let t = Instant::now();
            let _ = std::thread::spawn(|| {
                raster::lookup(Sf::Bell.name());
                raster::draw(Sf::Bell.name(), point_for_slot(16.), Weight::Regular, 16., 2.)
                    .is_some()
            })
            .join();
            prewarm_ms = ms(t);
        }
        let forty = &Sf::ALL[..40];
        let t = Instant::now();
        raster::lookup(Sf::Bell.name());
        let first_lookup_ms = ms(t);
        let draw_all = |scale: f32| {
            forty
                .iter()
                .map(|s| raster::draw(s.name(), point_for_slot(16.), Weight::Regular, 16., scale))
                .collect::<Vec<_>>()
        };
        let t = Instant::now();
        let one = draw_all(1.);
        let x1_ms = ms(t);
        let t = Instant::now();
        let two = draw_all(2.);
        let x2_ms = ms(t);
        let t = Instant::now();
        let ink = gpui::Rgba {
            r: 0.32,
            g: 0.32,
            b: 0.36,
            a: 1.,
        };
        for m in two.iter().flatten() {
            std::hint::black_box(m.colored(ink));
        }
        let color_ms = ms(t);
        let t = Instant::now();
        std::hint::black_box((draw_all(1.), draw_all(2.)));
        let warm_ms = ms(t);
        let missing = forty
            .iter()
            .zip(&one)
            .filter(|(_, m)| m.is_none())
            .map(|(s, _)| s.name())
            .collect();
        let fitted = forty
            .iter()
            .zip(&two)
            .filter_map(|(s, m)| m.as_ref().filter(|m| m.fitted).map(|m| (s.name(), m.point)))
            .collect();
        Cold {
            prewarmed,
            prewarm_ms,
            first_lookup_ms,
            x1_ms,
            x2_ms,
            color_ms,
            warm_ms,
            missing,
            fitted,
        }
    })
}

/// One theme switch as the scene saw it: the first frame after the switch.
#[derive(Clone, Debug, Default)]
pub struct Switch {
    pub to_dark: bool,
    pub drawn: u32,
    pub hits: u32,
    pub recolors: u32,
    pub masks: u32,
    pub missing: u32,
    pub busy_ms: f64,
    /// The frame drew in the new theme's ink.
    pub new_ink: bool,
}

pub static SWITCHES: Mutex<Vec<Switch>> = Mutex::new(Vec::new());

/// The worst ink-center offset of all 40 in a `slot` pt slot at `scale`,
/// in points, and which symbol it was.
fn centering(slot: f32, scale: f32, weight: Weight) -> Value {
    let mut worst = (0f32, "");
    let mut rows = Vec::new();
    for s in &Sf::ALL[..40] {
        let Some(m) = raster::draw(s.name(), point_for_slot(slot), weight, slot, scale) else {
            continue;
        };
        let (ex, ey) = m.center_error();
        let e = ex.abs().max(ey.abs());
        if e > worst.0 {
            worst = (e, s.name());
        }
        let (x0, y0, x1, y1) = m.ink;
        let clipped = x0 == 0 || y0 == 0 || x1 == m.side || y1 == m.side;
        rows.push(json!({ "symbol": s.name(), "dx_pt": ex, "dy_pt": ey,
            "ink_w_pt": (x1 - x0) as f32 / scale, "ink_h_pt": (y1 - y0) as f32 / scale,
            "touches_slot_edge": clipped, "point": m.point }));
    }
    json!({ "slot": slot, "scale": scale, "max_error_pt": worst.0, "worst": worst.1,
        "pass": worst.0 <= 0.5, "symbols": rows })
}

/// Share of inked pixels that are solid (≥ 75 %) and the mean coverage of
/// inked pixels: higher reads crisper at 1x.
fn crispness(masks: &[Vec<u8>]) -> (f32, f32) {
    let (mut inked, mut solid, mut sum) = (0u32, 0u32, 0u64);
    for m in masks {
        for &a in m {
            if a > raster::INK {
                inked += 1;
                sum += a as u64;
                if a >= 191 {
                    solid += 1;
                }
            }
        }
    }
    let inked = inked.max(1);
    (solid as f32 / inked as f32, sum as f32 / inked as f32 / 255.)
}

/// Lucide at 16 device pixels, as GPUI's `svg()` draws it on a 1x screen.
fn lucide_masks(side: u32, cx: &App) -> Vec<Vec<u8>> {
    let renderer = cx.svg_renderer();
    Sf::ALL[..40]
        .iter()
        .map(|s| {
            let path = s.lucide().path();
            let bytes = gpui::AssetSource::load(&crate::ui::AppAssets, &path)
                .ok()
                .flatten()
                .unwrap_or_default();
            let px = DevicePixels(side as i32);
            renderer
                .parse_svg(&bytes)
                .ok()
                .and_then(|svg| renderer.render_parsed(&svg, SvgSize::ExactSize(size(px, px))).ok())
                .and_then(|img| img.as_bytes(0).map(|b| b.chunks_exact(4).map(|p| p[3]).collect()))
                .unwrap_or_else(|| vec![0; (side * side) as usize])
        })
        .collect()
}

/// 40 masks in a 10 x 4 sheet, ink-2 on white, each pixel `zoom` x `zoom`.
fn write_sheet(masks: &[Vec<u8>], side: u32, zoom: u32, path: &Path) -> Result<(), String> {
    let gap = 4 * side / 16;
    let cell = side + gap;
    let (w, h) = (10 * cell + gap, 4 * cell + gap);
    let mut sheet = image::RgbImage::from_pixel(w * zoom, h * zoom, image::Rgb([255, 255, 255]));
    let ink = [0x52u8, 0x52, 0x5B];
    for (i, m) in masks.iter().enumerate() {
        let (cx0, cy0) = (gap + (i as u32 % 10) * cell, gap + (i as u32 / 10) * cell);
        for y in 0..side {
            for x in 0..side {
                let a = m[(y * side + x) as usize] as u32;
                // The slot shows as a faint square so its edges can be read.
                let bg = 0xF3u32;
                let px = |c: u8| ((c as u32 * a + bg * (255 - a)) / 255) as u8;
                let rgb = image::Rgb([px(ink[0]), px(ink[1]), px(ink[2])]);
                for zy in 0..zoom {
                    for zx in 0..zoom {
                        sheet.put_pixel((cx0 + x) * zoom + zx, (cy0 + y) * zoom + zy, rgb);
                    }
                }
            }
        }
    }
    sheet.save(path).map_err(|e| e.to_string())
}

/// Writes `<scene>-<theme>-gates.json` and the 1x sheets next to the capture.
pub fn write(png: &Path, cx: &mut App) -> Result<(), String> {
    let dir = png.parent().unwrap_or(Path::new("."));
    let stem = png
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("sf-symbols");
    let cold = cold().clone();
    let sf1: Vec<Vec<u8>> = Sf::ALL[..40]
        .iter()
        .map(|s| {
            raster::draw(s.name(), point_for_slot(16.), Weight::Regular, 16., 1.)
                .map(|m| m.alpha)
                .unwrap_or_else(|| vec![0; 256])
        })
        .collect();
    let sf1m: Vec<Vec<u8>> = Sf::ALL[..40]
        .iter()
        .map(|s| {
            raster::draw(s.name(), point_for_slot(16.), Weight::Medium, 16., 1.)
                .map(|m| m.alpha)
                .unwrap_or_else(|| vec![0; 256])
        })
        .collect();
    let lu1 = lucide_masks(16, cx);
    write_sheet(&sf1, 16, 8, &dir.join("sf-1x-sheet.png"))?;
    write_sheet(&sf1m, 16, 8, &dir.join("sf-medium-1x-sheet.png"))?;
    write_sheet(&lu1, 16, 8, &dir.join("lucide-1x-sheet.png"))?;
    let sf2: Vec<Vec<u8>> = Sf::ALL[..40]
        .iter()
        .map(|s| {
            raster::draw(s.name(), point_for_slot(16.), Weight::Regular, 16., 2.)
                .map(|m| m.alpha)
                .unwrap_or_else(|| vec![0; 1024])
        })
        .collect();
    write_sheet(&sf2, 32, 4, &dir.join("sf-2x-sheet.png"))?;
    write_sheet(&lucide_masks(32, cx), 32, 4, &dir.join("lucide-2x-sheet.png"))?;
    let (sf_solid, sf_mean) = crispness(&sf1);
    let (sfm_solid, sfm_mean) = crispness(&sf1m);
    let (lu_solid, lu_mean) = crispness(&lu1);
    let switches: Vec<Value> = SWITCHES
        .lock()
        .map(|s| s.clone())
        .unwrap_or_default()
        .iter()
        .map(|s| {
            json!({ "to_dark": s.to_dark, "drawn": s.drawn, "hits": s.hits,
            "recolors": s.recolors, "masks_drawn": s.masks, "missing": s.missing,
            "busy_ms": s.busy_ms, "new_ink": s.new_ink,
            "blank_symbols": s.missing, "pass": s.missing == 0 && s.drawn > 0 && s.new_ink })
        })
        .collect();
    let cold_total = cold.first_lookup_ms + cold.x1_ms + cold.x2_ms;
    let report = json!({
        "cold": {
            "prewarmed_off_main": cold.prewarmed,
            "prewarm_background_ms": cold.prewarm_ms,
            "first_lookup_ms": cold.first_lookup_ms,
            "x1_40_ms": cold.x1_ms,
            "x2_40_ms": cold.x2_ms,
            "color_40_ms": cold.color_ms,
            "warm_x1_x2_80_ms": cold.warm_ms,
            "total_cold_ms": cold_total,
            "raster_only_ms": cold.x1_ms + cold.x2_ms,
            "pass_total": cold_total < 30.,
            "pass_raster_only": cold.x1_ms + cold.x2_ms < 30.,
            "missing": cold.missing,
            "fitted_16": cold.fitted,
        },
        "crisp_1x": {
            "metric": "inked pixels (coverage > 5 %): share solid (>= 75 %), mean coverage",
            "sf_regular": { "solid": sf_solid, "mean": sf_mean },
            "sf_medium": { "solid": sfm_solid, "mean": sfm_mean },
            "lucide_now": { "solid": lu_solid, "mean": lu_mean },
            "sheets": ["sf-1x-sheet.png", "sf-medium-1x-sheet.png", "lucide-1x-sheet.png",
                "sf-2x-sheet.png", "lucide-2x-sheet.png"],
        },
        "centering": [centering(16., 1., Weight::Regular), centering(16., 2., Weight::Regular),
            centering(12., 2., Weight::Semibold)],
        "theme_switch": switches,
    });
    std::fs::write(
        dir.join(format!("{stem}-gates.json")),
        serde_json::to_string_pretty(&report).unwrap_or_default() + "\n",
    )
    .map_err(|e| e.to_string())
}
