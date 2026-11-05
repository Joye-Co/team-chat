//! SF Symbols, the app's only icon system (NATIVE.md rule 2, §6).
//!
//! - [`Sym`] names a concept and its symbol; [`SymSize`] holds the §6 sizes
//!   and weights. Views draw them through [`super::icons`] (`Sym::Home.sidebar()`,
//!   `icons::sym`, `icons::icon`).
//! - [`asset`] serves each symbol as an `sf/…` asset path, so GPUI's `svg()`
//!   (and every gpui-component `Icon`) draws and tints it; it also answers
//!   gpui-component's own Lucide paths with SF Symbols, and prewarms all
//!   concepts on a background thread at launch.
//! - [`Sf`], [`symbol`] and [`scene`] are the comparison scene (`sf-symbols`):
//!   Lucide next to the `img()` path below and the app's path, with the gate
//!   numbers (`sf-symbols-gates.json`).
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

pub mod asset;
pub mod gates;
mod raster;
pub mod scene;
pub mod sym;

pub use sym::{Sym, SymSize};

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
    ($($variant:ident = $sym:ident, $lucide:ident;)*) => {
        /// The 40 icons of the comparison scene, with their concept and the
        /// Lucide icon the app drew before.
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
        pub enum Sf { $($variant),* }

        impl Sf {
            pub const ALL: &'static [Sf] = &[$(Sf::$variant),*];

            /// The concept.
            pub fn sym(self) -> Sym {
                match self { $(Sf::$variant => Sym::$sym),* }
            }

            /// The system symbol name.
            pub fn name(self) -> &'static str {
                self.sym().name()
            }

            /// The Lucide icon the app drew before.
            pub fn lucide(self) -> Lucide {
                match self { $(Sf::$variant => Lucide::$lucide),* }
            }
        }
    };
}

symbols! {
    // Toolbar (§5.1, §6).
    Search = Search, Search;
    NewTopic = Compose, SquarePen;
    SideChat = AskAside, MessageCircle;
    Outputs = Outputs, Layers;
    Computer = Computer, Monitor;
    Changes = PullRequest, GitPullRequest;
    PaneRight = Inspector, PanelRight;
    More = More, Ellipsis;
    // Sidebar destinations and rows (§9.1).
    Desk = Home, House;
    Group = Topic, Hash;
    Members = Members, Users;
    Settings = General, Settings;
    // Settings tabs (§8).
    Bell = Bell, Bell;
    Machines = MacMini, Server;
    Appearance = Appearance, Palette;
    Shortcuts = Shortcuts, Keyboard;
    Privacy = Privacy, Hand;
    About = Info, Info;
    // Chevrons, checks and marks.
    Check = Check, Check;
    Close = Close, X;
    ChevronDown = ChevronDown, ChevronDown;
    ChevronRight = ChevronRight, ChevronRight;
    ChevronLeft = ChevronLeft, ChevronLeft;
    Popup = PopUp, ChevronsUpDown;
    Plus = Plus, Plus;
    // States and rows of cards and panes.
    Done = Done, CircleCheck;
    Alert = NeedsYou, CircleAlert;
    Failed = Failed, CircleX;
    Running = Live, CircleDot;
    Clock = Waiting, Clock;
    Document = Document, FileText;
    Copy = Copy, Copy;
    Open = OpenExternal, ExternalLink;
    Image = Image, Image;
    Play = Play, Play;
    Lock = Lock, Lock;
    Eye = Watch, Eye;
    Invite = Invite, UserPlus;
    Download = Download, Download;
    Options = Options, SlidersHorizontal;
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
    let point = point_for_slot(slot) * sf.sym().factor();
    let m = raster::draw(sf.name(), point, weight, slot, scale).map(Arc::new);
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
        None => icons::lucide_raw(sf.lucide(), slot, color).into_any_element(),
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
                assert!(
                    ex.abs() <= 0.5 && ey.abs() <= 0.5,
                    "{} at {scale}x: {ex} {ey}",
                    s.name()
                );
                let (x0, y0, x1, y1) = m.ink;
                assert!(x1 - x0 <= m.side && y1 - y0 <= m.side, "{} fits", s.name());
            }
        }
    }

    /// The Settings tab symbols fill the same 18 square (§2.19, review 3
    /// K-06): the larger side of every tab symbol's ink is 18 ± 0.5.
    #[test]
    fn tab_symbols_fill_the_tab_square() {
        let size = SymSize::TAB;
        for sym in [
            Sym::General,
            Sym::Account,
            Sym::Bell,
            Sym::Members,
            Sym::Computer,
            Sym::Appearance,
            Sym::Shortcuts,
        ] {
            let m = raster::draw_ink(
                sym.name(),
                size.point * sym.factor(),
                size.weight,
                size.slot,
                2.,
                Some(size.ink),
            )
            .unwrap_or_else(|| panic!("{} draws", sym.name()));
            let (x0, y0, x1, y1) = m.ink;
            let big = (x1 - x0).max(y1 - y0) as f32 / 2.;
            assert!((17.5..=18.).contains(&big), "{}: {big}", sym.name());
        }
    }

    #[test]
    fn a_color_keeps_the_mask_and_changes_the_ink() {
        let m = raster::draw("bell", 13., Weight::Regular, 16., 2.).expect("bell");
        let red = m.colored(Rgba {
            r: 1.,
            g: 0.,
            b: 0.,
            a: 0.5,
        });
        let bytes = red.as_bytes(0).expect("one frame");
        let i = m
            .alpha
            .iter()
            .position(|&a| a == 255)
            .expect("a solid pixel");
        // Straight BGRA: blue, green, red, alpha.
        assert_eq!(&bytes[i * 4..i * 4 + 4], &[0, 0, 255, 128]);
        assert!(raster::draw("no.such.symbol.tc", 13., Weight::Regular, 16., 2.).is_none());
    }
}
