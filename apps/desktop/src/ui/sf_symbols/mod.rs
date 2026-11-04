//! SF Symbols spike (POLISH-SPEC §7.3, §10.2): the system's symbols drawn by
//! AppKit into a bitmap at the window's scale and shown with GPUI `img()`.
//!
//! The app's icons stay Lucide ([`super::icons`]); this module only exists so
//! the integration can compare the two sets and read the gate numbers
//! (`sf-symbols` scene, `sf-symbols-gates.json`). Nothing outside the scene
//! uses it.
//!
//! How a symbol is drawn:
//! 1. AppKit draws the symbol (point size, weight, medium scale) once per
//!    (symbol, slot, weight, scale) into an `NSBitmapImageRep` at device
//!    scale. Only the alpha channel is kept: a coverage mask.
//! 2. The mask's ink box is moved to the slot center, snapped to whole device
//!    pixels, so a 16 pt slot holds the glyph within half a device pixel.
//!    A symbol whose ink is larger than the slot is drawn again at a smaller
//!    point size (`fitted`).
//! 3. Each color gets its own `RenderImage` (GPUI `img()` does not tint):
//!    the mask times the color, built in Rust in microseconds. A theme switch
//!    changes the color, so the next frame recolors from the cached masks in
//!    that same frame. `ImageSource::Render` paints synchronously, so there is
//!    no blank frame (an `ImageSource::Image` PNG would decode on the
//!    background executor first, `gpui::elements::img` `use_data`).
//!
//! License: SF Symbols are drawn by the system at run time and never exported
//! or bundled.

pub mod gates;
mod raster;
pub mod scene;

use std::{
    cell::RefCell,
    collections::HashMap,
    sync::Arc,
    time::{Duration, Instant},
};

use gpui::{
    AnyElement, Hsla, ImageSource, IntoElement as _, Pixels, RenderImage, Rgba, Styled as _,
    Window, img,
};

use super::icons::{self, Lucide};

pub use raster::Mask;

/// The weight of a symbol (`NSFontWeight`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Weight {
    Light,
    Regular,
    Medium,
    Semibold,
}

macro_rules! symbols {
    ($($variant:ident = $sf:literal, $lucide:ident;)*) => {
        /// The icons of the app's chrome, with their SF Symbol and the Lucide
        /// icon they would replace.
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
        pub enum Sf { $($variant),* }

        impl Sf {
            pub const ALL: &'static [Sf] = &[$(Sf::$variant),*];

            /// The system symbol name.
            pub fn name(self) -> &'static str {
                match self { $(Sf::$variant => $sf),* }
            }

            /// The Lucide icon the app draws today.
            pub fn lucide(self) -> Lucide {
                match self { $(Sf::$variant => Lucide::$lucide),* }
            }
        }
    };
}

symbols! {
    // Title bar and right group (§5).
    Search = "magnifyingglass", Search;
    NewTopic = "square.and.pencil", SquarePen;
    SideChat = "bubble.left", MessageCircle;
    Outputs = "square.3.layers.3d", Layers;
    Computer = "display", Monitor;
    Changes = "arrow.triangle.pull", GitPullRequest;
    PaneRight = "sidebar.right", PanelRight;
    More = "ellipsis", Ellipsis;
    // Rail destinations and rows (§2.3).
    Desk = "house", House;
    Group = "number", Hash;
    Members = "person.2", Users;
    Settings = "gearshape", Settings;
    // Settings sections (§7.1 map, §8.2).
    Bell = "bell", Bell;
    Machines = "server.rack", Server;
    Appearance = "paintpalette", Palette;
    Shortcuts = "keyboard", Keyboard;
    Privacy = "hand.raised", Hand;
    About = "info.circle", Info;
    // Chevrons, checks and marks (§7.1 sizes 10–12).
    Check = "checkmark", Check;
    Close = "xmark", X;
    ChevronDown = "chevron.down", ChevronDown;
    ChevronRight = "chevron.right", ChevronRight;
    ChevronLeft = "chevron.left", ChevronLeft;
    Popup = "chevron.up.chevron.down", ChevronsUpDown;
    Plus = "plus", Plus;
    // States and rows of cards and panes.
    Done = "checkmark.circle", CircleCheck;
    Alert = "exclamationmark.circle", CircleAlert;
    Failed = "xmark.circle", CircleX;
    Running = "smallcircle.filled.circle", CircleDot;
    Clock = "clock", Clock;
    Document = "doc.text", FileText;
    Copy = "doc.on.doc", Copy;
    Open = "arrow.up.right.square", ExternalLink;
    Image = "photo", Image;
    Play = "play", Play;
    Lock = "lock", Lock;
    Eye = "eye", Eye;
    Invite = "person.badge.plus", UserPlus;
    Download = "square.and.arrow.down", Download;
    Options = "slider.horizontal.3", SlidersHorizontal;
}

/// The point size AppKit draws a symbol at for a slot: 13 pt in a 16 pt
/// slot, so a circle symbol is 13 pt wide like Lucide's 20/24 circle.
pub fn point_for_slot(slot: f32) -> f32 {
    slot * 13. / 16.
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
struct MaskKey {
    sf: Sf,
    /// The slot in tenths of a point.
    slot: u16,
    weight: Weight,
    /// The scale factor in hundredths.
    scale: u16,
}

#[derive(Default)]
struct Cache {
    masks: HashMap<MaskKey, Option<Arc<Mask>>>,
    images: HashMap<(MaskKey, u32), Arc<RenderImage>>,
    stats: Stats,
}

/// What the cache did since the last [`take_stats`].
#[derive(Clone, Copy, Debug, Default)]
pub struct Stats {
    /// Symbols asked for.
    pub drawn: u32,
    /// Asks served from a cached image.
    pub hits: u32,
    /// Masks AppKit drew.
    pub masks: u32,
    /// Images colored from a cached mask.
    pub recolors: u32,
    /// Symbols that fell back to Lucide (no such symbol on this system).
    pub missing: u32,
    /// Time spent drawing masks and coloring.
    pub busy: Duration,
}

thread_local! {
    static CACHE: RefCell<Cache> = RefCell::new(Cache::default());
}

/// The counters since the last call, then zeroes them.
pub fn take_stats() -> Stats {
    CACHE.with(|c| std::mem::take(&mut c.borrow_mut().stats))
}

fn key(sf: Sf, slot: f32, weight: Weight, scale: f32) -> MaskKey {
    MaskKey {
        sf,
        slot: (slot * 10.).round() as u16,
        weight,
        scale: (scale * 100.).round() as u16,
    }
}

/// The coverage mask of a symbol in a square slot at `scale`, cached.
pub fn mask(sf: Sf, slot: f32, weight: Weight, scale: f32) -> Option<Arc<Mask>> {
    let k = key(sf, slot, weight, scale);
    if let Some(m) = CACHE.with(|c| c.borrow().masks.get(&k).cloned()) {
        return m;
    }
    let started = Instant::now();
    let m = raster::draw(sf.name(), point_for_slot(slot), weight, slot, scale).map(Arc::new);
    CACHE.with(|c| {
        let mut c = c.borrow_mut();
        c.stats.masks += 1;
        c.stats.busy += started.elapsed();
        c.masks.insert(k, m.clone());
    });
    m
}

/// The image of a symbol in one color, cached per color.
pub fn image(
    sf: Sf,
    slot: f32,
    weight: Weight,
    color: Hsla,
    scale: f32,
) -> Option<Arc<RenderImage>> {
    let k = key(sf, slot, weight, scale);
    let rgba = Rgba::from(color);
    let packed = pack(rgba);
    if let Some(found) = CACHE.with(|c| c.borrow().images.get(&(k, packed)).cloned()) {
        CACHE.with(|c| {
            let mut c = c.borrow_mut();
            c.stats.drawn += 1;
            c.stats.hits += 1;
        });
        return Some(found);
    }
    let m = mask(sf, slot, weight, scale);
    let started = Instant::now();
    let Some(m) = m else {
        CACHE.with(|c| {
            let mut c = c.borrow_mut();
            c.stats.drawn += 1;
            c.stats.missing += 1;
        });
        return None;
    };
    let made = Arc::new(m.colored(rgba));
    CACHE.with(|c| {
        let mut c = c.borrow_mut();
        c.stats.drawn += 1;
        c.stats.recolors += 1;
        c.stats.busy += started.elapsed();
        c.images.insert((k, packed), made.clone());
    });
    Some(made)
}

fn pack(c: Rgba) -> u32 {
    let b = |v: f32| (v.clamp(0., 1.) * 255.).round() as u32;
    (b(c.r) << 24) | (b(c.g) << 16) | (b(c.b) << 8) | b(c.a)
}

/// A symbol in a square slot of `slot` pt, in `color`; Lucide when this
/// system has no such symbol.
pub fn symbol(sf: Sf, slot: Pixels, weight: Weight, color: Hsla, window: &Window) -> AnyElement {
    match image(sf, f32::from(slot), weight, color, window.scale_factor()) {
        Some(image) => img(ImageSource::Render(image))
            .size(slot)
            .flex_none()
            .into_any_element(),
        None => icons::icon(sf.lucide(), slot, color).into_any_element(),
    }
}

#[cfg(all(test, target_os = "macos"))]
mod tests {
    use super::*;

    #[test]
    fn every_symbol_draws_centered_in_its_slot() {
        assert_eq!(Sf::ALL.len(), 40);
        for &scale in &[1., 2.] {
            for s in Sf::ALL {
                let m = raster::draw(s.name(), point_for_slot(16.), Weight::Regular, 16., scale)
                    .unwrap_or_else(|| panic!("{} draws", s.name()));
                assert_eq!(m.side, (16. * scale) as u32);
                let (ex, ey) = m.center_error();
                assert!(ex.abs() <= 0.5 && ey.abs() <= 0.5, "{} at {scale}x: {ex} {ey}", s.name());
                let (x0, y0, x1, y1) = m.ink;
                assert!(x1 - x0 <= m.side && y1 - y0 <= m.side, "{} fits", s.name());
            }
        }
    }

    #[test]
    fn a_color_keeps_the_mask_and_changes_the_ink() {
        let m = raster::draw("bell", 13., Weight::Regular, 16., 2.).expect("bell");
        let red = m.colored(Rgba { r: 1., g: 0., b: 0., a: 0.5 });
        let bytes = red.as_bytes(0).expect("one frame");
        let i = m.alpha.iter().position(|&a| a == 255).expect("a solid pixel");
        // Straight BGRA: blue, green, red, alpha.
        assert_eq!(&bytes[i * 4..i * 4 + 4], &[0, 0, 255, 128]);
        assert!(raster::draw("no.such.symbol.tc", 13., Weight::Regular, 16., 2.).is_none());
    }
}
