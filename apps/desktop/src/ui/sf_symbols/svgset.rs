//! The SVG icon set of systems without SF Symbols (docs/design/WINDOWS.md §5).
//!
//! SF Symbols may only be drawn by Apple's systems, so on Windows every
//! concept ([`Sym`]) draws a Lucide icon (ISC, `third_party/lucide/LICENSE`;
//! the files come with `gpui-kit`), or one of the few icons drawn for this
//! app on Lucide's grid (`assets/icons/drawn/`) where Lucide has no filled
//! or matching shape.
//!
//! The icon comes behind the same asset path as the symbol
//! (`sf/<name>/<weight>/<point>/<slot>/…`, [`super::asset`]), so no call site
//! knows which system it is on. To sit where the symbol sits and weigh what
//! it weighs:
//! - **Optical size**: the icon is as large as fits in the symbol's ink box at
//!   that point size, so it is never larger than the symbol in either
//!   direction (the `@` numbers of the table: the width and height of the
//!   symbol's ink per em, measured on macOS 26; `TC_SVG_TABLE=1` with the
//!   test `the_table_matches_sf_symbols` measures them again).
//! - **Weight**: the stroke is as thick as the symbol's at that point size
//!   and weight ([`stroke_em`]), whatever the icon's scale.
//! - **Alignment**: the ink box is centered in the slot, like the symbol's.
//!
//! `TC_SVG_ICONS=1` draws this set on macOS too (developers only): the way to
//! look at the Windows icons from a Mac.

use std::{
    collections::HashMap,
    sync::{Arc, LazyLock, Mutex, OnceLock},
};

use gpui::AssetSource as _;

use super::{Weight, raster, sym::Sym};
use crate::ui::icons::Lucide;

/// Where a concept's icon comes from.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Source {
    /// A Lucide icon, as bundled by `gpui-kit`.
    Lucide(Lucide),
    /// An icon drawn for this app: the SVG file's text.
    Drawn(&'static str),
}

macro_rules! svg_set {
    (
        lucide { $($ls:ident = $l:ident @ $lw:literal, $lh:literal;)* }
        drawn { $($ds:ident = $file:literal @ $dw:literal, $dh:literal;)* }
    ) => {
        gpui_kit::assets::icon_assets!(LucideSet, [$($l),*]);

        /// The icon of a concept. The match has no wildcard: a new concept
        /// does not compile until it has a row here.
        pub fn source(sym: Sym) -> Option<Source> {
            match sym {
                $(Sym::$ls => Some(Source::Lucide(Lucide::$l)),)*
                $(Sym::$ds => Some(Source::Drawn(include_str!(concat!(
                    "../../../../../assets/icons/drawn/", $file, ".svg"
                )))),)*
                Sym::Missing => None,
            }
        }

        /// The SF Symbol's ink box per em (Regular): width, height. The
        /// icon is drawn as large as fits in it.
        pub fn ink_em(sym: Sym) -> (f32, f32) {
            match sym {
                $(Sym::$ls => ($lw, $lh),)*
                $(Sym::$ds => ($dw, $dh),)*
                Sym::Missing => (0., 0.),
            }
        }
    };
}

svg_set! {
    lucide {
        Home = Inbox @ 1.16, 0.80;
        Outputs = SquareStack @ 0.86, 1.12;
        Members = Users @ 1.37, 0.91;
        Topic = Hash @ 0.88, 0.95;
        Compose = SquarePen @ 0.97, 0.97;
        Search = Search @ 0.96, 0.94;
        Filter = ListFilter @ 0.99, 0.56;
        AskAside = MessagesSquare @ 1.41, 1.09;
        OpenTopic = MessageSquareText @ 1.08, 1.02;
        PullRequest = GitPullRequest @ 0.51, 0.93;
        Branch = GitBranch @ 0.94, 0.91;
        Merge = GitMerge @ 0.59, 0.93;
        Inspector = PanelRight @ 1.16, 0.88;
        SidebarToggle = PanelLeft @ 1.16, 0.88;
        Attach = Plus @ 0.81, 0.78;
        Plus = Plus @ 0.81, 0.78;
        Minus = Minus @ 0.81, 0.08;
        Send = ArrowUp @ 0.74, 0.90;
        Alert = CircleAlert @ 1.00, 0.97;
        Warning = TriangleAlert @ 0.99, 0.90;
        Waiting = Clock @ 1.00, 0.97;
        Todo = CircleDashed @ 1.00, 0.97;
        Circle = Circle @ 1.00, 0.97;
        Stopped = Ban @ 1.00, 0.97;
        Hourglass = Hourglass @ 0.58, 0.87;
        Computer = Monitor @ 1.16, 0.98;
        Laptop = Laptop @ 1.41, 0.80;
        Server = Server @ 1.16, 0.88;
        Cloud = Cloud @ 1.17, 0.74;
        Offline = WifiOff @ 1.13, 0.91;
        Phone = Smartphone @ 0.63, 1.04;
        General = Settings @ 1.08, 1.05;
        Account = CircleUserRound @ 1.00, 0.97;
        Bell = Bell @ 0.95, 1.02;
        Muted = BellOff @ 0.93, 0.98;
        BellBadge = BellDot @ 0.95, 0.98;
        Appearance = Palette @ 1.18, 0.98;
        // No ⌘ on a Windows keyboard: 快捷键 is a keyboard (WINDOWS.md §5).
        Shortcuts = Keyboard @ 0.92, 0.92;
        Keyboard = Keyboard @ 1.23, 0.74;
        Privacy = Hand @ 0.81, 1.06;
        Lock = Lock @ 0.67, 0.93;
        Shield = ShieldCheck @ 0.81, 0.96;
        OpenWindow = AppWindow @ 1.16, 0.88;
        More = Ellipsis @ 0.95, 0.17;
        Check = Check @ 0.84, 0.80;
        Close = X @ 0.81, 0.78;
        ChevronDown = ChevronDown @ 0.85, 0.48;
        ChevronUp = ChevronUp @ 0.85, 0.48;
        ChevronLeft = ChevronLeft @ 0.48, 0.85;
        ChevronRight = ChevronRight @ 0.48, 0.85;
        PageFirst = ChevronsLeft @ 0.91, 0.85;
        PageLast = ChevronsRight @ 0.91, 0.85;
        PopUp = ChevronsUpDown @ 0.66, 0.95;
        OpenExternal = ArrowUpRight @ 0.75, 0.73;
        Document = FileText @ 0.83, 1.02;
        File = File @ 0.83, 1.02;
        CodeFile = FileCode @ 0.83, 1.02;
        MissingFile = FileQuestionMark @ 0.83, 1.02;
        Diff = Diff @ 0.93, 0.90;
        Image = Image @ 1.16, 0.88;
        Images = Images @ 1.22, 0.96;
        Copy = Copy @ 1.00, 1.23;
        AiMember = Sparkles @ 0.88, 1.09;
        Person = User @ 0.86, 0.88;
        Invite = UserPlus @ 1.09, 1.07;
        Quote = TextQuote @ 0.98, 0.85;
        List = List @ 0.98, 0.69;
        NumberedList = ListOrdered @ 0.98, 0.83;
        Checklist = ListChecks @ 1.10, 0.96;
        Outline = ListTree @ 1.23, 0.69;
        Grid = LayoutGrid @ 0.91, 0.88;
        Table = Table @ 1.16, 0.88;
        Download = Download @ 0.87, 1.05;
        // One symbol on the Mac (`square.and.arrow.up`), so one icon here: an
        // asset path names the symbol, not the concept.
        Upload = Share @ 0.87, 1.07;
        Share = Share @ 0.87, 1.07;
        Diagram = Workflow @ 1.05, 0.85;
        Video = Video @ 1.21, 0.77;
        Audio = AudioLines @ 0.91, 0.99;
        Play = Play @ 0.71, 0.82;
        Pause = Pause @ 0.41, 0.78;
        Volume = Volume2 @ 1.09, 0.84;
        PictureInPicture = PictureInPicture2 @ 1.27, 0.99;
        ZoomIn = ZoomIn @ 0.96, 0.94;
        ZoomOut = ZoomOut @ 0.96, 0.94;
        Pointer = MousePointer2 @ 0.56, 0.98;
        Watch = Eye @ 1.30, 0.81;
        Hidden = EyeOff @ 1.30, 0.90;
        NewChat = MessageSquarePlus @ 1.08, 1.02;
        Info = Info @ 1.00, 0.97;
        Help = CircleQuestionMark @ 1.00, 0.97;
        Return = CornerDownLeft @ 1.03, 0.87;
        ReplyIndent = CornerDownRight @ 0.92, 0.74;
        Reply = Reply @ 1.05, 0.87;
        ArrowUp = ArrowUp @ 0.74, 0.90;
        ArrowDown = ArrowDown @ 0.74, 0.90;
        ArrowRight = ArrowRight @ 0.91, 0.71;
        Sort = ArrowUpDown @ 1.16, 0.95;
        Terminal = SquareTerminal @ 1.16, 0.88;
        Retry = RotateCcw @ 0.87, 1.02;
        Refresh = RotateCw @ 0.87, 1.02;
        Undo = Undo2 @ 0.93, 0.91;
        Link = Link @ 1.05, 1.05;
        Code = Code @ 1.38, 0.90;
        Mention = AtSign @ 1.05, 1.04;
        TextFormat = ALargeSmall @ 1.12, 0.73;
        Bold = Bold @ 0.59, 0.70;
        Italic = Italic @ 0.51, 0.70;
        Strikethrough = Strikethrough @ 0.81, 0.73;
        AlignLeft = TextAlignStart @ 0.98, 0.85;
        Options = SlidersHorizontal @ 0.98, 0.80;
        Edit = Pencil @ 0.78, 0.78;
        Expand = Maximize2 @ 0.94, 0.91;
        Collapse = Minimize2 @ 0.95, 0.98;
        SignOut = LogOut @ 1.14, 1.02;
        Emoji = FaceSlightlySmiling @ 1.00, 0.97;
        Chart = ChartLine @ 1.05, 0.88;
        BarChart = ChartColumn @ 1.24, 0.92;
        Activity = Activity @ 0.98, 0.98;
        Delete = Trash @ 0.95, 1.06;
        Star = Star @ 1.09, 1.05;
        ScreenShare = ScreenShare @ 1.37, 0.88;
        Scan = Scan @ 0.96, 0.93;
        Folder = Folder @ 1.11, 0.87;
        Calendar = Calendar @ 0.98, 0.88;
        Target = Crosshair @ 1.19, 1.16;
        Mic = Mic @ 0.70, 1.05;
        Book = BookOpen @ 1.09, 0.91;
        Square = Square @ 0.91, 0.88;
        Asterisk = Asterisk @ 0.88, 1.01;
        Cpu = Cpu @ 0.98, 0.98;
        Memory = MemoryStick @ 1.05, 0.85;
        Disk = HardDrive @ 1.16, 0.80;
        Network = Globe @ 1.00, 0.97;
        Key = KeyRound @ 0.66, 1.23;
        Layers = Layers @ 1.03, 1.05;
        Logs = FileSearch @ 0.90, 1.13;
        History = RotateCcwClock @ 1.12, 0.97;
        Spinner = Loader @ 1.06, 1.03;
        Replace = ArrowLeftRight @ 0.98, 1.12;
        BottomPanel = PanelBottom @ 1.16, 0.88;
    }
    drawn {
        // Filled states: the mark is cut out of the shape (§6: the symbol is
        // the state), which Lucide does not draw.
        NeedsYou = "circle-alert-fill" @ 1.00, 0.97;
        Live = "circle-fill" @ 1.00, 0.97;
        Done = "circle-check-fill" @ 1.00, 0.97;
        Problem = "triangle-alert-fill" @ 0.99, 0.90;
        Failed = "circle-x-fill" @ 1.00, 0.97;
        StarFill = "star-fill" @ 1.09, 1.05;
        // A small desktop computer seen from the front: a team machine.
        MacMini = "desktop-mini" @ 1.27, 0.50;
    }
}

/// The thickness of an SF Symbol's stroke per em at a weight (measured on
/// `circle`): an icon at 13 pt Regular has a 1.07 pt line.
pub fn stroke_em(weight: Weight) -> f32 {
    match weight {
        Weight::Light => 0.065,
        Weight::Regular => 0.082,
        Weight::Medium => 0.101,
        Weight::Semibold => 0.112,
    }
}

/// True where the SVG set draws the icons: every system but macOS, and macOS
/// with `TC_SVG_ICONS` set (developers: look at the Windows icons on a Mac).
pub fn active() -> bool {
    static ACTIVE: OnceLock<bool> = OnceLock::new();
    *ACTIVE.get_or_init(|| {
        !cfg!(target_os = "macos") || std::env::var_os("TC_SVG_ICONS").is_some_and(|v| v != "0")
    })
}

/// A Lucide icon's grid: 24 units, drawn at a stroke of 2.
const GRID: f32 = 24.;
const LUCIDE_STROKE: f32 = 2.;

/// Where an icon's ink is on the 24-unit grid, as a function of the stroke
/// width `w`: a side is `a + b·w` (`b` is 1 for stroked shapes, 0 for filled
/// ones), around a center that does not move.
#[derive(Clone, Copy, Debug)]
struct Geometry {
    cx: f32,
    cy: f32,
    aw: f32,
    bw: f32,
    ah: f32,
    bh: f32,
}

/// What is between `<svg …>` and `</svg>`.
fn body(svg: &str) -> Option<&str> {
    let open = svg.find("<svg")?;
    let start = open + svg[open..].find('>')? + 1;
    let end = svg.rfind("</svg>")?;
    (start <= end).then(|| svg[start..end].trim())
}

fn source_body(source: Source) -> Option<String> {
    match source {
        Source::Lucide(l) => {
            let bytes = LucideSet.load(&l.path()).ok()??;
            body(std::str::from_utf8(&bytes).ok()?).map(str::to_owned)
        }
        Source::Drawn(svg) => body(svg).map(str::to_owned),
    }
}

/// The body with every `stroke-width` of its own (written against Lucide's
/// stroke of 2: a cut-out mark is 2.5) scaled to the stroke `w`.
fn restroke(body: &str, w: f32) -> String {
    const ATTR: &str = "stroke-width=\"";
    let mut out = String::with_capacity(body.len());
    let mut rest = body;
    while let Some(at) = rest.find(ATTR) {
        let value = &rest[at + ATTR.len()..];
        let Some(end) = value.find('"') else { break };
        out.push_str(&rest[..at + ATTR.len()]);
        match value[..end].parse::<f32>() {
            Ok(v) => out.push_str(&format!("{:.3}", v / LUCIDE_STROKE * w)),
            Err(_) => out.push_str(&value[..end]),
        }
        rest = &value[end..];
    }
    out.push_str(rest);
    out
}

/// The icon on the 24-unit viewBox: its grid scaled by `k` and moved by
/// (`tx`, `ty`), with a stroke of `w` grid units. Black: GPUI tints by alpha.
fn compose(body: &str, k: f32, tx: f32, ty: f32, w: f32) -> String {
    format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"24\" height=\"24\" \
         viewBox=\"0 0 24 24\"><g transform=\"translate({tx:.4} {ty:.4}) scale({k:.5})\" \
         fill=\"none\" stroke=\"#000\" stroke-width=\"{w:.3}\" stroke-linecap=\"round\" \
         stroke-linejoin=\"round\">{}</g></svg>",
        restroke(body, w)
    )
}

/// The coverage of an SVG in a square of `side` device pixels.
fn rasterize(svg: &str, side: u32) -> Option<Vec<u8>> {
    let renderer = gpui::SvgRenderer::new(Arc::new(()));
    let tree = renderer.parse_svg(svg.as_bytes()).ok()?;
    let px = gpui::DevicePixels(side as i32);
    let image = renderer
        .render_parsed(&tree, gpui::SvgSize::ExactSize(gpui::size(px, px)))
        .ok()?;
    let bytes = image.as_bytes(0)?;
    Some(bytes.chunks_exact(4).map(|p| p[3]).collect())
}

/// The ink box of the icon on its own grid at a stroke of `w`: x0, y0, x1,
/// y1 in grid units.
fn ink_at(body: &str, w: f32) -> Option<(f32, f32, f32, f32)> {
    // 16 device pixels a unit: the box is good to a sixteenth of a unit.
    const PER_UNIT: u32 = 16;
    let side = GRID as u32 * PER_UNIT;
    let alpha = rasterize(&compose(body, 1., 0., 0., w), side)?;
    let (x0, y0, x1, y1) = raster::ink_box(&alpha, side, side)?;
    let u = |v: u32| v as f32 / PER_UNIT as f32;
    Some((u(x0), u(y0), u(x1), u(y1)))
}

fn geometry(sym: Sym) -> Option<Geometry> {
    static CACHE: LazyLock<Mutex<HashMap<Sym, Option<Geometry>>>> =
        LazyLock::new(|| Mutex::new(HashMap::new()));
    if let Some(hit) = CACHE.lock().ok()?.get(&sym) {
        return *hit;
    }
    let measured = (|| {
        let body = source_body(source(sym)?)?;
        let (a0, b0, a1, b1) = ink_at(&body, 1.)?;
        let (c0, d0, c1, d1) = ink_at(&body, 2.)?;
        let (w1, h1, w2, h2) = (a1 - a0, b1 - b0, c1 - c0, d1 - d0);
        let (bw, bh) = ((w2 - w1).clamp(0., 1.), (h2 - h1).clamp(0., 1.));
        Some(Geometry {
            cx: (c0 + c1) / 2.,
            cy: (d0 + d1) / 2.,
            aw: w2 - 2. * bw,
            bw,
            ah: h2 - 2. * bh,
            bh,
        })
    })();
    if let Ok(mut cache) = CACHE.lock() {
        cache.insert(sym, measured);
    }
    measured
}

/// The concept of a symbol name. Concepts with one symbol must have one icon
/// (`concepts_with_one_symbol_have_one_icon`): the asset path carries the
/// symbol's name, so the first concept of a name stands for all of them.
fn concept(name: &str) -> Option<Sym> {
    static NAMES: LazyLock<HashMap<&'static str, Sym>> = LazyLock::new(|| {
        let mut names = HashMap::new();
        for &sym in Sym::ALL {
            names.entry(sym.name()).or_insert(sym);
        }
        names
    });
    NAMES.get(name).copied()
}

/// The SVG of the symbol `name` drawn at `point` and `weight`, centered in a
/// `slot` pt square (the SVG's 24-unit viewBox is the slot). With `ink` the
/// larger side of the ink box is `ink` points ([`super::SymSize::fill`]).
pub fn svg(name: &str, point: f32, weight: Weight, slot: f32, ink: Option<f32>) -> Option<String> {
    let sym = concept(name)?;
    let body = source_body(source(sym)?)?;
    let (ew, eh) = ink_em(sym);
    let em = ew.max(eh);
    let g = geometry(sym);
    let (Some(g), true) = (g, em > 0.) else {
        // Not measured: Lucide's own proportions (a 20-unit glyph is the
        // point size), as it sits on its grid.
        let k = point * 1.2 / slot;
        let at = (GRID - GRID * k) / 2.;
        let w = stroke_em(weight) * point / (k * slot / GRID);
        return Some(compose(&body, k, at, at, w));
    };
    // The point size the symbol has: the one that makes its larger side
    // `ink`, and never one that makes its ink larger than the slot.
    let point = ink.map_or(point, |ink| ink / em).min(slot / em);
    // The box to fit in: the symbol's ink box, or a square of `ink`.
    let (tw, th) = match ink {
        Some(_) => (em * point, em * point),
        None => (ew * point, eh * point),
    };
    let stroke = stroke_em(weight) * point;
    // Points per grid unit: a side is `k·a + b·stroke`.
    let fit = |t: f32, a: f32, b: f32| (a > 0.01).then(|| (t - b * stroke) / a);
    let k = match (fit(tw, g.aw, g.bw), fit(th, g.ah, g.bh)) {
        (Some(x), Some(y)) => x.min(y),
        (Some(k), None) | (None, Some(k)) => k,
        (None, None) => return None,
    };
    if k <= 0. {
        return None;
    }
    // Grid units to viewBox units: the viewBox is the slot.
    let scale = k * GRID / slot;
    Some(compose(
        &body,
        scale,
        GRID / 2. - g.cx * scale,
        GRID / 2. - g.cy * scale,
        stroke / k,
    ))
}

#[cfg(test)]
mod tests {
    use super::super::SymSize;
    use super::*;

    fn concepts() -> impl Iterator<Item = Sym> {
        Sym::ALL.iter().copied().filter(|s| *s != Sym::Missing)
    }

    /// The ink box of a drawn icon in points: x0, y0, x1, y1 in the slot.
    fn ink_of(svg: &str, slot: f32) -> (f32, f32, f32, f32) {
        const SCALE: f32 = 8.;
        let side = (slot * SCALE) as u32;
        let alpha = rasterize(svg, side).expect("rasterizes");
        let (x0, y0, x1, y1) = raster::ink_box(&alpha, side, side).expect("has ink");
        (
            x0 as f32 / SCALE,
            y0 as f32 / SCALE,
            x1 as f32 / SCALE,
            y1 as f32 / SCALE,
        )
    }

    /// Every concept the code can name has an icon in the set, and it draws
    /// at every §6 size: the symbol's ink size, centered, inside its slot.
    #[test]
    fn every_concept_has_an_svg_icon() {
        for sym in concepts() {
            let source = source(sym).unwrap_or_else(|| panic!("{sym:?} has no SVG icon"));
            assert!(source_body(source).is_some(), "{sym:?}: {source:?} loads");
            let (ew, eh) = ink_em(sym);
            assert!(ew > 0. && eh > 0., "{sym:?} has no ink size");
            for size in SymSize::CONTEXTS {
                let point = size.point * sym.factor();
                let ink = (size.ink > 0.).then_some(size.ink);
                let svg = svg(sym.name(), point, size.weight, size.slot, ink)
                    .unwrap_or_else(|| panic!("{sym:?} draws at {size:?}"));
                let (x0, y0, x1, y1) = ink_of(&svg, size.slot);
                // The symbol's ink box (a square of `ink` for a filled size),
                // no larger than the slot: the icon touches it on one axis
                // and stays inside on the other.
                let em = ew.max(eh);
                let point = ink.map_or(point, |i| i / em).min(size.slot / em);
                let (tw, th) = match ink {
                    Some(_) => (em * point, em * point),
                    None => (ew * point, eh * point),
                };
                let (w, h) = (x1 - x0, y1 - y0);
                // A line is never thinner than the weight's stroke.
                let stroke = stroke_em(size.weight) * point;
                let (tw, th) = (tw.max(stroke), th.max(stroke));
                assert!(
                    w <= tw + 0.3 && h <= th + 0.3,
                    "{sym:?} at {size:?}: ink {w} × {h} in {tw} × {th}"
                );
                assert!(
                    (w - tw).abs() <= 0.3 || (h - th).abs() <= 0.3,
                    "{sym:?} at {size:?}: ink {w} × {h} fills {tw} × {th}"
                );
                let (ex, ey) = ((x0 + x1 - size.slot) / 2., (y0 + y1 - size.slot) / 2.);
                assert!(
                    ex.abs() <= 0.2 && ey.abs() <= 0.2,
                    "{sym:?} at {size:?}: off center by {ex} {ey}"
                );
            }
        }
    }

    /// Every concept draws the icon its table row names: an asset path
    /// carries the symbol name, so two concepts with one symbol (上传 and
    /// 共享…, both `square.and.arrow.up`) must name one icon, or the second
    /// would silently draw the first's.
    #[test]
    fn concepts_with_one_symbol_have_one_icon() {
        for sym in concepts() {
            let drawn = concept(sym.name()).expect("a concept of the name");
            assert_eq!(
                source(drawn),
                source(sym),
                "{sym:?} draws the icon of {drawn:?} (both are `{}`)",
                sym.name()
            );
            assert_eq!(ink_em(drawn), ink_em(sym), "{sym:?} and {drawn:?}");
        }
    }

    /// The Lucide icons gpui-component draws by itself (select chevron,
    /// checkbox check, dialog close …) come from the set too.
    #[test]
    fn lucide_paths_with_a_concept_have_an_icon() {
        for (lucide, sym) in super::super::sym::LUCIDE {
            assert!(source(*sym).is_some(), "{lucide:?} → {sym:?}");
        }
    }

    #[test]
    fn a_cut_out_mark_follows_the_stroke() {
        assert_eq!(
            restroke(
                "<path stroke-width=\"2.5\"/><path stroke-width=\"x\"/>",
                1.6
            ),
            "<path stroke-width=\"2.000\"/><path stroke-width=\"x\"/>"
        );
        assert_eq!(body("<svg a=\"b\">\n  <path/>\n</svg>\n"), Some("<path/>"));
    }

    #[cfg(target_os = "macos")]
    fn sf_ink_em(sym: Sym) -> (f32, f32) {
        const POINT: f32 = 32.;
        const SCALE: f32 = 4.;
        let m = raster::draw(sym.name(), POINT, Weight::Regular, POINT * 3., SCALE)
            .unwrap_or_else(|| panic!("{} draws", sym.name()));
        assert!(!m.fitted, "{}", sym.name());
        let (x0, y0, x1, y1) = m.ink;
        let em = |side: u32| side as f32 / SCALE / POINT;
        (em(x1 - x0), em(y1 - y0))
    }

    /// The thickness of `circle`'s line per em: the coverage of the upper
    /// half of its middle column.
    #[cfg(target_os = "macos")]
    fn sf_stroke_em(weight: Weight) -> f32 {
        const POINT: f32 = 32.;
        const SCALE: f32 = 8.;
        let m = raster::draw("circle", POINT, weight, POINT * 2., SCALE).expect("circle");
        let x = (m.ink.0 + m.ink.2) / 2;
        let column: u32 = (0..m.side / 2)
            .map(|y| m.alpha[(y * m.side + x) as usize] as u32)
            .sum();
        column as f32 / 255. / SCALE / POINT
    }

    /// `TC_SVG_TABLE=1` measures this Mac's SF Symbols, prints the numbers for
    /// the table and [`stroke_em`], and fails where the table is off. Without
    /// it the test does nothing: Apple redraws symbols between system
    /// versions (`arrow.triangle.merge` is 0.79 em tall on macOS 15 and 0.93
    /// on macOS 26), so the numbers are those of the macOS the table was last
    /// measured on (26), not a fact every host agrees on.
    #[cfg(target_os = "macos")]
    #[test]
    fn the_table_matches_sf_symbols() {
        if std::env::var_os("TC_SVG_TABLE").is_none() {
            return;
        }
        let print = true;
        let mut off = Vec::new();
        for sym in concepts() {
            let ((mw, mh), (tw, th)) = (sf_ink_em(sym), ink_em(sym));
            if print {
                println!("{sym:?} @ {mw:.2}, {mh:.2}");
            }
            // Symbols are redrawn a little between system versions.
            if (mw - tw).abs() > 0.08 || (mh - th).abs() > 0.08 {
                off.push(format!(
                    "{sym:?}: table {tw}, {th}; SF Symbols {mw:.2}, {mh:.2}"
                ));
            }
        }
        for weight in [
            Weight::Light,
            Weight::Regular,
            Weight::Medium,
            Weight::Semibold,
        ] {
            let (measured, table) = (sf_stroke_em(weight), stroke_em(weight));
            if print {
                println!("{weight:?} stroke {measured:.4}");
            }
            if (measured - table).abs() > 0.012 {
                off.push(format!(
                    "{weight:?} stroke: {table}, SF Symbols {measured:.4}"
                ));
            }
        }
        assert!(off.is_empty(), "{}", off.join("\n"));
    }

    /// `TC_SVG_SHEET=<dir>` writes `icons-light.png` and `icons-dark.png`:
    /// every concept as its SF Symbol and as its SVG icon, side by side at
    /// the inline, sidebar and toolbar sizes (4x), to compare by eye.
    #[cfg(target_os = "macos")]
    #[test]
    fn contact_sheet() {
        let Some(dir) = std::env::var_os("TC_SVG_SHEET") else {
            return;
        };
        const SCALE: f32 = 4.;
        const COLS: u32 = 6;
        let sizes = [SymSize::INLINE, SymSize::SIDEBAR, SymSize::TOOLBAR];
        let cell = |s: &SymSize| (s.slot * SCALE) as u32;
        let pair_w: u32 = sizes.iter().map(|s| cell(s) * 2 + 12).sum::<u32>() + 28;
        let row_h = cell(&SymSize::TOOLBAR) + 16;
        let syms: Vec<Sym> = concepts().collect();
        let rows = (syms.len() as u32).div_ceil(COLS);
        for (name, bg, fg) in [("light", 255u8, 0u8), ("dark", 20, 240)] {
            let mut sheet = image::RgbaImage::from_pixel(
                COLS * pair_w + 16,
                rows * row_h + 16,
                image::Rgba([bg, bg, bg, 255]),
            );
            let mut put = |alpha: &[u8], side: u32, at: (u32, u32)| {
                for y in 0..side {
                    for x in 0..side {
                        let a = alpha[(y * side + x) as usize] as f32 / 255.;
                        let v = (bg as f32 * (1. - a) + fg as f32 * a).round() as u8;
                        sheet.put_pixel(at.0 + x, at.1 + y, image::Rgba([v, v, v, 255]));
                    }
                }
            };
            for (i, &sym) in syms.iter().enumerate() {
                let (col, row) = (i as u32 % COLS, i as u32 / COLS);
                let mut x = 16 + col * pair_w;
                let y = 16 + row * row_h;
                for size in sizes {
                    let side = cell(&size);
                    let point = size.point * sym.factor();
                    if let Some(m) = raster::draw(sym.name(), point, size.weight, size.slot, SCALE)
                    {
                        put(&m.alpha, side, (x, y));
                    }
                    x += side + 4;
                    if let Some(alpha) = svg(sym.name(), point, size.weight, size.slot, None)
                        .and_then(|svg| rasterize(&svg, side))
                    {
                        put(&alpha, side, (x, y));
                    }
                    x += side + 8;
                }
            }
            let path = std::path::Path::new(&dir).join(format!("icons-{name}.png"));
            sheet.save(&path).expect("writes the sheet");
        }
    }
}
