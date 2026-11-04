//! SF Symbols as GPUI icons: each (symbol, point, weight, slot) is an asset
//! path (`sf/…`) that [`crate::ui::AppAssets`] serves as a small SVG around
//! the AppKit coverage mask. GPUI's `svg()` (and so gpui-component's `Icon`,
//! buttons and menus) rasterizes it into its sprite atlas and tints it on the
//! GPU, so a theme switch only changes the color: no redraw, no blank frame.
//!
//! - The mask is drawn at the screen's scale times GPUI's SVG supersampling
//!   (2), so the atlas tile is the mask 1:1 and the GPU halves it: the same
//!   anti-aliasing as AppKit's own drawing.
//! - [`prewarm`] draws every concept at the §6 sizes on a background thread at
//!   launch; the asset loader serves from that cache and draws a missing one
//!   on the spot (synchronously, so never a blank frame).
//! - A name the system does not have draws a crossed box in debug builds
//!   (and is logged), and nothing in release builds.

use std::{
    collections::HashMap,
    sync::{
        Arc, LazyLock, Mutex,
        atomic::{AtomicU32, Ordering},
    },
};

use gpui_component::Icon;
use gpui::Styled as _;

use super::{
    Weight, raster,
    sym::{LUCIDE, Sym, SymSize},
};

/// Asset paths of symbols start with this.
pub const PREFIX: &str = "sf/";

/// GPUI rasterizes SVGs at twice the device size (`SMOOTH_SVG_SCALE_FACTOR`).
const SUPERSAMPLE: f32 = 2.;

/// The largest backing scale of the screens, in hundredths (2x by default).
static SCALE: AtomicU32 = AtomicU32::new(200);

/// The SVG bytes of each path drawn so far (`None`: no such symbol).
static CACHE: LazyLock<Mutex<HashMap<String, Option<Arc<[u8]>>>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

/// The scale masks are drawn for.
pub fn scale() -> f32 {
    SCALE.load(Ordering::Relaxed) as f32 / 100.
}

/// Sets the scale masks are drawn for (the largest screen's backing scale).
pub fn set_scale(scale: f32) {
    let s = (scale.clamp(1., 4.) * 100.).round() as u32;
    SCALE.store(s, Ordering::Relaxed);
}

fn weight_code(w: Weight) -> &'static str {
    match w {
        Weight::Light => "l",
        Weight::Regular => "r",
        Weight::Medium => "m",
        Weight::Semibold => "s",
    }
}

fn weight_from(code: &str) -> Option<Weight> {
    Some(match code {
        "l" => Weight::Light,
        "r" => Weight::Regular,
        "m" => Weight::Medium,
        "s" => Weight::Semibold,
        _ => return None,
    })
}

/// `sf/<name>/<weight>/<point>/<slot>/<scale>.svg`, sizes in tenths and the
/// scale in hundredths. A missing name is `-`.
pub fn path(sym: Sym, size: SymSize) -> String {
    path_at(sym, size, scale())
}

fn path_at(sym: Sym, size: SymSize, scale: f32) -> String {
    let name = match sym.name() {
        "" => "-",
        n => n,
    };
    let point = size.point * sym.factor();
    format!(
        "{PREFIX}{name}/{}/{}/{}/{}.svg",
        weight_code(size.weight),
        (point * 10.).round() as u32,
        (size.slot * 10.).round() as u32,
        (scale * 100.).round() as u32,
    )
}

/// The parts of a symbol path.
#[derive(Debug, PartialEq)]
struct Parsed<'a> {
    name: &'a str,
    weight: Weight,
    point: f32,
    slot: f32,
    scale: f32,
}

fn parse(path: &str) -> Option<Parsed<'_>> {
    let rest = path.strip_prefix(PREFIX)?.strip_suffix(".svg")?;
    let mut parts = rest.split('/');
    let name = parts.next()?;
    let weight = weight_from(parts.next()?)?;
    let num = |s: Option<&str>| s.and_then(|s| s.parse::<u32>().ok());
    let point = num(parts.next())? as f32 / 10.;
    let slot = num(parts.next())? as f32 / 10.;
    let scale = num(parts.next())? as f32 / 100.;
    if parts.next().is_some() || !(1. ..=400.).contains(&slot) || point <= 0. {
        return None;
    }
    Some(Parsed {
        name: if name == "-" { "" } else { name },
        weight,
        point,
        slot,
        scale,
    })
}

/// The symbol at a size, in the text color of its parent.
pub fn icon(sym: Sym, size: SymSize) -> Icon {
    Icon::empty().path(path(sym, size)).size(size.slot_px())
}

/// The symbol for a component that sizes its own icons: drawn at the inline
/// size, scaled by GPUI to the component's icon size.
pub fn icon_unsized(sym: Sym) -> Icon {
    Icon::empty().path(path(sym, SymSize::INLINE))
}

/// The SVG of a symbol path, cached; `None` when `path` is not a symbol
/// path.
pub fn load(path: &str) -> Option<Arc<[u8]>> {
    if let Some(hit) = CACHE.lock().ok()?.get(path) {
        return Some(hit.clone().unwrap_or_else(fallback));
    }
    let p = parse(path)?;
    let drawn = draw(&p);
    if drawn.is_none() {
        tracing::warn!(symbol = p.name, "no such SF Symbol on this system");
    }
    if let Ok(mut cache) = CACHE.lock() {
        cache.insert(path.to_owned(), drawn.clone());
    }
    Some(drawn.unwrap_or_else(fallback))
}

/// The SVG of a Lucide asset path (`icons/<name>.svg`) that has a concept:
/// gpui-component's own glyphs (select chevrons, checkbox check, dialog
/// close, input clear …) and Lucide names other code still passes.
pub fn load_lucide(path: &str) -> Option<Arc<[u8]>> {
    static PATHS: LazyLock<HashMap<String, Sym>> = LazyLock::new(|| {
        LUCIDE
            .iter()
            .map(|(l, s)| (l.path().to_string(), *s))
            .collect()
    });
    let sym = *PATHS.get(path)?;
    load(&lucide_path(sym))
}

/// A Lucide glyph sits in a 24-unit box at about 20 units: the symbol in the
/// same proportion (13 pt in a 16 pt slot), at the toolbar's resolution so
/// GPUI scales it down for the component's size.
fn lucide_path(sym: Sym) -> String {
    path_at(sym, SymSize::new(16.25, 20., Weight::Regular), scale())
}

fn draw(p: &Parsed<'_>) -> Option<Arc<[u8]>> {
    if p.name.is_empty() {
        return None;
    }
    let mask = raster::draw(p.name, p.point, p.weight, p.slot, p.scale * SUPERSAMPLE)?;
    Some(Arc::from(svg(&mask).into_bytes()))
}

/// The mask as a white PNG with its coverage as alpha, in an SVG of the
/// slot's size (24 units, so Lucide's widened-viewBox fitting still works).
fn svg(mask: &raster::Mask) -> String {
    let side = mask.side;
    let mut rgba = Vec::with_capacity((side * side * 4) as usize);
    for &a in &mask.alpha {
        rgba.extend_from_slice(&[255, 255, 255, a]);
    }
    let mut png = Vec::new();
    let encoder = image::codecs::png::PngEncoder::new(&mut png);
    let _ = image::ImageEncoder::write_image(
        encoder,
        &rgba,
        side,
        side,
        image::ExtendedColorType::Rgba8,
    );
    format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" xmlns:xlink=\"http://www.w3.org/1999/xlink\" \
         width=\"24\" height=\"24\" viewBox=\"0 0 24 24\"><image width=\"24\" height=\"24\" \
         xlink:href=\"data:image/png;base64,{}\"/></svg>",
        base64(&png)
    )
}

/// What an unknown symbol draws: a crossed box in debug builds, so it shows
/// on screen; nothing in release builds.
fn fallback() -> Arc<[u8]> {
    static SVG: LazyLock<Arc<[u8]>> = LazyLock::new(|| {
        let body = if cfg!(debug_assertions) {
            "<rect x=\"3\" y=\"3\" width=\"18\" height=\"18\" fill=\"none\" stroke=\"#000\" \
             stroke-width=\"2\"/><path d=\"M3 3L21 21M21 3L3 21\" stroke=\"#000\" stroke-width=\"2\"/>"
        } else {
            ""
        };
        Arc::from(
            format!(
                "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"24\" height=\"24\" \
                 viewBox=\"0 0 24 24\">{body}</svg>"
            )
            .into_bytes(),
        )
    });
    SVG.clone()
}

/// Draws every concept at the §6 sizes (and the Lucide override size) on a
/// background thread, so the first frames find them cached.
pub fn prewarm(scale: f32) {
    set_scale(scale);
    let _ = std::thread::Builder::new()
        .name("sf-symbols-prewarm".into())
        .spawn(move || {
            let started = std::time::Instant::now();
            let mut paths: Vec<String> = Vec::new();
            for &sym in Sym::ALL {
                for size in SymSize::CONTEXTS {
                    paths.push(path_at(sym, size, scale));
                }
            }
            // Toolbar and row buttons draw at 75 % of a 32 or 28 pt button.
            for &sym in Sym::ALL {
                for slot in [24., 21.] {
                    for size in [SymSize::TOOLBAR, SymSize::INLINE] {
                        paths.push(path_at(sym, SymSize::new(size.point, slot, size.weight), scale));
                    }
                }
            }
            for (_, sym) in LUCIDE {
                paths.push(lucide_path(*sym));
            }
            paths.sort();
            paths.dedup();
            for p in &paths {
                objc_pool(|| {
                    load(p);
                });
            }
            tracing::debug!(
                count = paths.len(),
                ms = started.elapsed().as_millis() as u64,
                "SF Symbols prewarmed"
            );
        });
}

#[cfg(target_os = "macos")]
fn objc_pool(f: impl FnOnce()) {
    objc2::rc::autoreleasepool(|_| f());
}

#[cfg(not(target_os = "macos"))]
fn objc_pool(f: impl FnOnce()) {
    f();
}

/// The largest backing scale of the screens (main thread only).
#[cfg(target_os = "macos")]
pub fn screens_scale() -> f32 {
    use objc2_app_kit::NSScreen;
    use objc2_foundation::MainThreadMarker;
    let Some(mtm) = MainThreadMarker::new() else {
        return 2.;
    };
    NSScreen::screens(mtm)
        .iter()
        .map(|s| s.backingScaleFactor() as f32)
        .fold(1., f32::max)
}

#[cfg(not(target_os = "macos"))]
pub fn screens_scale() -> f32 {
    2.
}

fn base64(bytes: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b = [
            chunk[0],
            *chunk.get(1).unwrap_or(&0),
            *chunk.get(2).unwrap_or(&0),
        ];
        let n = (b[0] as u32) << 16 | (b[1] as u32) << 8 | b[2] as u32;
        out.push(T[(n >> 18) as usize & 63] as char);
        out.push(T[(n >> 12) as usize & 63] as char);
        out.push(if chunk.len() > 1 { T[(n >> 6) as usize & 63] as char } else { '=' });
        out.push(if chunk.len() > 2 { T[n as usize & 63] as char } else { '=' });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paths_round_trip() {
        let p = path_at(Sym::Home, SymSize::SIDEBAR, 2.);
        assert_eq!(p, "sf/tray/r/150/180/200.svg");
        let parsed = parse(&p).unwrap();
        assert_eq!(parsed.name, "tray");
        assert_eq!(parsed.weight, Weight::Regular);
        assert_eq!((parsed.point, parsed.slot, parsed.scale), (15., 18., 2.));
        assert!(parse("sf/tray/x/150/180/200.svg").is_none());
        assert!(parse("icons/tray.svg").is_none());
        // The live dot draws smaller than its context.
        assert_eq!(path_at(Sym::Live, SymSize::INLINE, 1.), "sf/circle.fill/r/60/160/100.svg");
        assert_eq!(path_at(Sym::Missing, SymSize::INLINE, 1.), "sf/-/r/130/160/100.svg");
    }

    #[test]
    fn base64_matches_the_standard_alphabet() {
        assert_eq!(base64(b"Man"), "TWFu");
        assert_eq!(base64(b"Ma"), "TWE=");
        assert_eq!(base64(b"M"), "TQ==");
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn every_concept_draws_and_gpui_rasterizes_it() {
        let renderer = gpui::SvgRenderer::new(std::sync::Arc::new(crate::ui::AppAssets));
        for &sym in Sym::ALL {
            if sym == Sym::Missing {
                continue;
            }
            for size in SymSize::CONTEXTS {
                let p = path_at(sym, size, 2.);
                let parsed = parse(&p).unwrap();
                let svg = draw(&parsed).unwrap_or_else(|| panic!("{} draws", sym.name()));
                let tree = renderer.parse_svg(&svg).expect("parses");
                // GPUI's tile is the mask 1:1: the slot at 2x, supersampled.
                let side = (size.slot * 2. * SUPERSAMPLE).round() as i32;
                let px = gpui::DevicePixels(side);
                let image = renderer
                    .render_parsed(&tree, gpui::SvgSize::ExactSize(gpui::size(px, px)))
                    .expect("rasterizes");
                let bytes = image.as_bytes(0).unwrap();
                let ink = bytes.chunks_exact(4).filter(|p| p[3] > 128).count();
                assert!(ink > 0, "{} at {size:?} has ink", sym.name());
            }
        }
        // An unknown name loads the fallback, never fails.
        let missing = load(&path_at(Sym::Missing, SymSize::INLINE, 2.)).unwrap();
        assert!(std::str::from_utf8(&missing).unwrap().starts_with("<svg"));
    }

    #[test]
    fn gpui_component_glyphs_have_concepts() {
        use crate::ui::icons::Lucide;
        for l in [
            Lucide::Check,
            Lucide::Close,
            Lucide::ChevronDown,
            Lucide::ChevronRight,
            Lucide::ChevronLeft,
            Lucide::ChevronUp,
            Lucide::ChevronsUpDown,
            Lucide::Plus,
            Lucide::Minus,
            Lucide::Search,
            Lucide::CircleX,
            Lucide::CircleCheck,
            Lucide::Info,
            Lucide::TriangleAlert,
            Lucide::Eye,
            Lucide::EyeOff,
            Lucide::Copy,
            Lucide::Ellipsis,
            Lucide::Inbox,
            Lucide::ArrowUp,
            Lucide::ArrowDown,
            Lucide::Calendar,
        ] {
            assert!(super::super::sym::from_lucide(l).is_some(), "{l:?}");
        }
    }
}
