//! The one token module (docs/design/COMPLETENESS.md §1): every color,
//! height, radius, spacing step, type size and shadow of the app.
//!
//! - [`Palette`] is the one color struct: the neutral black and white values
//!   of §1.2 and the state colors of §1.3. `theme::Colors` and
//!   [`NativeColors`] are views of it with older field names; they hold no
//!   value of their own.
//! - [`space`], [`height`], [`radius`], [`text`], [`icon`], [`avatar`] and
//!   [`shadow`] are the scales of §1.4–§1.8.
//! - [`size`], [`type_scale`], [`layout`], [`metrics`], [`card_v2`] and
//!   [`card_v21`] keep the older names that views use. Each one is an alias
//!   onto a scale step; new code reads the scales.
//!
//! Views read colors with `theme::colors(cx)` or [`native_colors`] and sizes
//! from here. Never write a color or size literal in a view.

use gpui::{App, BoxShadow, FontWeight, Hsla, Pixels, Styled, Window, point, px, rgba};

use crate::theme::{self, FONT_FAMILY, MEDIUM_WEIGHT, tabular};

fn c(hex: u32) -> Hsla {
    rgba(hex).into()
}

// ---------------------------------------------------------------------------
// Colors (§1.2, §1.3)

/// The one color struct: the shadcn new-york-v4 neutral values of §1.2 and
/// the state colors of §1.3, in one mode.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Palette {
    /// Window content, message column.
    pub background: Hsla,
    /// All primary text and icons.
    pub foreground: Hsla,
    /// Card, panel, sheet and popover surface.
    pub card: Hsla,
    /// Sidebar fill and the group box fill in Settings and sheets.
    pub sidebar: Hsla,
    /// Hover, selected row, skeleton, table header fill, icon box.
    pub muted: Hsla,
    /// Menu and palette highlight: the same value as `muted`.
    pub accent: Hsla,
    /// All secondary text, placeholders, meta and timestamps.
    pub muted_foreground: Hsla,
    /// All 1 px lines.
    pub border: Hsla,
    /// Input and outline button border, switch OFF track.
    pub input: Hsla,
    /// The focus ring (drawn 3 px at 50 %).
    pub ring: Hsla,
    /// Primary button, switch ON track, Progress value.
    pub primary: Hsla,
    pub primary_foreground: Hsla,
    /// A pressed fill: one step past `muted`.
    pub pressed: Hsla,
    /// Lines with Increase contrast on (§11 of DESIGN.md).
    pub border_strong: Hsla,
    /// Secondary text with Increase contrast on: one step darker (light) or
    /// lighter (dark) than `muted_foreground`.
    pub muted_foreground_strong: Hsla,
    /// The window dim behind a sheet (light 20 %, dark #121212 on content).
    pub scrim: Hsla,
    /// State: destructive / failed.
    pub destructive: Hsla,
    /// State: warning / needs you.
    pub warning: Hsla,
    /// State: success / done / online.
    pub success: Hsla,
    /// State: info / in progress / unread / link.
    pub info: Hsla,
    pub dark: bool,
}

impl Palette {
    pub const fn light_hex() -> [u32; 20] {
        [
            0xFFFFFFFF, // background
            0x0A0A0AFF, // foreground
            0xFFFFFFFF, // card
            0xFAFAFAFF, // sidebar
            0xF5F5F5FF, // muted
            0xF5F5F5FF, // accent
            0x737373FF, // muted_foreground
            0xE5E5E5FF, // border
            0xE5E5E5FF, // input
            0xA1A1A1FF, // ring
            0x171717FF, // primary
            0xFAFAFAFF, // primary_foreground
            0xE5E5E5FF, // pressed
            0xD4D4D4FF, // border_strong
            0x00000033, // scrim
            0xDC2626FF, // destructive
            0xD97706FF, // warning
            0x16A34AFF, // success
            0x2563EBFF, // info
            0x525252FF, // muted_foreground_strong
        ]
    }

    pub const fn dark_hex() -> [u32; 20] {
        [
            0x0A0A0AFF, // background
            0xFAFAFAFF, // foreground
            0x171717FF, // card
            0x171717FF, // sidebar
            0x262626FF, // muted
            0x262626FF, // accent
            0xA1A1A1FF, // muted_foreground
            0xFFFFFF29, // border: white 16 % (>= 1.5:1 on background and card, §2.22)
            0xFFFFFF33, // input: white 20 %
            0x737373FF, // ring
            0xE5E5E5FF, // primary
            0x171717FF, // primary_foreground
            0x404040FF, // pressed
            0xFFFFFF40, // border_strong: white 25 %
            0x00000066, // scrim
            0xF87171FF, // destructive
            0xFBBF24FF, // warning
            0x4ADE80FF, // success
            0x60A5FAFF, // info
            0xD4D4D4FF, // muted_foreground_strong
        ]
    }

    fn from_hex(h: [u32; 20], dark: bool) -> Self {
        Self {
            background: c(h[0]),
            foreground: c(h[1]),
            card: c(h[2]),
            sidebar: c(h[3]),
            muted: c(h[4]),
            accent: c(h[5]),
            muted_foreground: c(h[6]),
            border: c(h[7]),
            input: c(h[8]),
            ring: c(h[9]),
            primary: c(h[10]),
            primary_foreground: c(h[11]),
            pressed: c(h[12]),
            border_strong: c(h[13]),
            scrim: c(h[14]),
            destructive: c(h[15]),
            warning: c(h[16]),
            success: c(h[17]),
            info: c(h[18]),
            muted_foreground_strong: c(h[19]),
            dark,
        }
    }

    pub fn light() -> Self {
        Self::from_hex(Self::light_hex(), false)
    }

    pub fn dark() -> Self {
        Self::from_hex(Self::dark_hex(), true)
    }

    pub fn for_mode(dark: bool) -> Self {
        if dark { Self::dark() } else { Self::light() }
    }

    /// The focus ring color: `ring` at 50 % (§1.8).
    pub fn focus(&self) -> Hsla {
        self.ring.opacity(0.5)
    }

    /// A state color at `t` alpha: the soft fill of an alert or an error
    /// ring (destructive at 20 %).
    pub fn soft(color: Hsla, t: f32) -> Hsla {
        Hsla {
            a: color.a * t,
            ..color
        }
    }
}

/// The macOS-named view of [`Palette`] that the native kit reads. Field names
/// follow NATIVE.md; every value comes from the palette.
#[derive(Clone, Copy, Debug)]
pub struct NativeColors {
    /// `background`: the content area.
    pub content: Hsla,
    /// The settings window background.
    pub window: Hsla,
    /// Sheets, alerts, popovers and menus: `card`.
    pub sheet: Hsla,
    /// Menus: opaque `card`.
    pub menu: Hsla,
    /// The sidebar panel.
    pub sidebar: Hsla,
    /// The group box fill (§2.8): `muted` in light, a raised step in dark.
    pub group: Hsla,
    /// `foreground`.
    pub label: Hsla,
    /// `muted-foreground`.
    pub secondary: Hsla,
    /// `muted-foreground`: there is one muted text color (§1.2).
    pub tertiary: Hsla,
    /// `input`: switch OFF track, outline control border.
    pub quaternary: Hsla,
    /// The fill of an outline control (outline button, select, input,
    /// checkbox): `background` in light; in dark `input` at 30 % over
    /// `card` (shadcn `dark:bg-input/30`), so a control on a GroupBox or a
    /// sheet reads as a raised control, not a hole (§2.22, R1-17).
    pub control: Hsla,
    /// The hover fill of an outline control: `accent` in light; in dark
    /// `input` at 50 % over `card` (shadcn `dark:hover:bg-input/50`), one
    /// visible step over [`Self::control`]. `accent` (#262626) is only one
    /// level over the dark control fill, so a hover in it does not show
    /// (review 3 K-04).
    pub control_hover: Hsla,
    /// The fill of a push button that is not the primary one (取消, 编辑…,
    /// 更改…): the foreground at 8 %, no border, like a Mac push button. A
    /// tint of the text color, so it shows on the window, a GroupBox and a
    /// sheet in both modes.
    pub push: Hsla,
    /// The hover of a row inside a group box: the foreground at 4 %, a
    /// quiet tint of the group's own fill in both modes.
    pub row_hover: Hsla,
    /// [`Self::push`] on hover: the foreground at 12 %.
    pub push_hover: Hsla,
    /// [`Self::push`] pressed: the foreground at 16 %.
    pub push_pressed: Hsla,
    /// `border`.
    pub separator: Hsla,
    /// `muted`: control fill (secondary button, segmented track).
    pub fill: Hsla,
    /// `accent`: ghost button and menu row hover.
    pub fill_hover: Hsla,
    /// A pressed control fill: one step past `muted`.
    pub pressed: Hsla,
    /// `muted`: the selected row (D10).
    pub selection: Hsla,
    /// `accent` (§1.2): the same value as `muted`. The highlight of a menu,
    /// palette, picker or mention row; never an inverted row (D10).
    pub accent: Hsla,
    /// `primary` (or the system accent the person picked): the primary
    /// button, switch ON track, Progress value, a checked checkbox.
    pub primary: Hsla,
    /// `primary-foreground`: text and symbols on `primary`.
    pub on_primary: Hsla,
    /// Text or a symbol on a selected row: `foreground` (or the system accent).
    pub accent_text: Hsla,
    /// `foreground`.
    pub ink: Hsla,
    /// `foreground`: text and symbols on an `accent` (highlighted) row.
    pub on_accent: Hsla,
    /// The 3 px focus ring: `ring` at 50 %.
    pub focus: Hsla,
    /// The inner edge of a floating surface: `border`.
    pub glass_edge: Hsla,
    /// The fill of a toolbar group: `card`.
    pub capsule: Hsla,
    /// The window dim behind a sheet.
    pub dim: Hsla,
    /// The switch knob when OFF: `background` (dark: `foreground`).
    pub knob: Hsla,
    /// The switch knob when ON: `primary-foreground` (§2.22).
    pub knob_on: Hsla,
    /// The active segmented trigger: `background` (dark: `input` on `muted`).
    pub segment: Hsla,
    pub amber: Hsla,
    pub blue: Hsla,
    pub red: Hsla,
    pub green: Hsla,
    /// The destructive button fill (§1.3, T1-15): one value in both modes.
    pub destructive_fill: Hsla,
    dark: bool,
}

/// The destructive button fill in both modes (COMPLETENESS.md §1.3):
/// #dc2626 with white text, 4.8:1. Dark mode keeps #f87171 for icons and
/// text only; white on #f87171 is 2.8:1.
pub const DESTRUCTIVE_FILL: u32 = 0xDC2626FF;

/// The app accent while the system accent is multicolor. Kept as a name:
/// it is now `primary` of the light palette (#171717).
pub const GRAPHITE: u32 = 0x171717FF;

impl NativeColors {
    pub fn from_palette(p: &Palette) -> Self {
        Self {
            content: p.background,
            window: p.background,
            sheet: p.card,
            menu: p.card,
            sidebar: p.sidebar,
            // The group box has no border, so its fill alone sets it off:
            // `muted` on the white window and sheet in light; in dark the
            // foreground at 5 % over `card`, a step over both the window
            // (#0a0a0a) and a sheet (#171717).
            group: if p.dark {
                p.card.blend(p.foreground.opacity(0.05))
            } else {
                p.muted
            },
            label: p.foreground,
            secondary: p.muted_foreground,
            tertiary: p.muted_foreground,
            quaternary: p.input,
            control: if p.dark {
                p.card.blend(p.input.opacity(0.3))
            } else {
                p.background
            },
            control_hover: if p.dark {
                p.card.blend(p.input.opacity(0.5))
            } else {
                p.accent
            },
            push: p.foreground.opacity(0.08),
            push_hover: p.foreground.opacity(0.12),
            row_hover: p.foreground.opacity(0.04),
            push_pressed: p.foreground.opacity(0.16),
            separator: p.border,
            fill: p.muted,
            fill_hover: p.accent,
            pressed: p.pressed,
            selection: p.muted,
            accent: p.accent,
            primary: p.primary,
            on_primary: p.primary_foreground,
            accent_text: p.foreground,
            ink: p.foreground,
            on_accent: p.foreground,
            focus: p.focus(),
            glass_edge: p.border,
            capsule: p.card,
            dim: p.scrim,
            knob: if p.dark { p.foreground } else { p.background },
            knob_on: p.primary_foreground,
            segment: if p.dark { p.input } else { p.background },
            amber: p.warning,
            blue: p.info,
            red: p.destructive,
            green: p.success,
            destructive_fill: c(DESTRUCTIVE_FILL),
            dark: p.dark,
        }
    }

    pub fn light() -> Self {
        Self::from_palette(&Palette::light())
    }

    pub fn dark() -> Self {
        Self::from_palette(&Palette::dark())
    }

    pub fn for_mode(dark: bool) -> Self {
        if dark { Self::dark() } else { Self::light() }
    }

    pub fn is_dark(&self) -> bool {
        self.dark
    }

    /// The float level (§1.7): shadow md and the 1 px border line.
    pub fn float(&self) -> Vec<BoxShadow> {
        let mut s = shadow::md();
        s.push(shadow::line(self.separator));
        s
    }

    /// Older name of [`NativeColors::float`]: floating surfaces have no
    /// glass edge of their own now.
    pub fn glass(&self) -> Vec<BoxShadow> {
        self.float()
    }

    /// A 1 px line inside an element's edge, drawn as an inset shadow so
    /// the layout does not move.
    pub fn inner_edge(&self, color: Hsla) -> BoxShadow {
        BoxShadow {
            color,
            offset: point(px(0.), px(0.)),
            blur_radius: px(0.),
            spread_radius: metrics::HAIRLINE,
            inset: true,
        }
    }

    /// The active segmented trigger: shadow sm.
    pub fn segment(&self) -> Vec<BoxShadow> {
        shadow::sm()
    }

    /// The switch knob shadow: a soft drop and a hairline edge.
    pub fn knob_shadow(&self) -> Vec<BoxShadow> {
        let (drop, edge) = if self.dark {
            (0x00000059, 0x00000033)
        } else {
            (0x00000026, 0x0000001A)
        };
        vec![
            BoxShadow {
                color: c(drop),
                offset: point(px(0.), px(1.)),
                blur_radius: px(1.5),
                spread_radius: px(0.),
                inset: false,
            },
            BoxShadow {
                color: c(edge),
                offset: point(px(0.), px(0.)),
                blur_radius: px(0.),
                spread_radius: metrics::HAIRLINE,
                inset: false,
            },
        ]
    }

    /// The 3 px focus ring outside a control (§1.8).
    pub fn focus_ring(&self) -> Vec<BoxShadow> {
        vec![BoxShadow {
            color: self.focus,
            offset: point(px(0.), px(0.)),
            blur_radius: px(0.),
            spread_radius: metrics::FOCUS_RING,
            inset: false,
        }]
    }

    /// The (symbol, text) colors of a selected row (D10): foreground on the
    /// muted fill; muted-foreground symbols when the window is inactive.
    pub fn selected_ink(&self, emphasized: bool) -> (Hsla, Hsla) {
        if emphasized {
            (self.accent_text, self.accent_text)
        } else {
            (self.secondary, self.label)
        }
    }
}

/// The native colors of the current mode.
pub fn native_colors(cx: &App) -> NativeColors {
    let dark = theme::is_dark(cx);
    let mut k = NativeColors::for_mode(dark);
    // The system accent the person picked, else `primary`.
    k.primary = theme::app_accent(dark);
    k.on_primary = theme::accent_ink(dark);
    if let Some(accent) = theme::system_accent() {
        k.accent_text = accent;
        k.knob_on = k.on_primary;
    }
    k
}

/// Whether `window` draws its selection with emphasis (§4 Inactive window):
/// key windows only, or every window in the screenshot self-test.
pub fn emphasized(window: &Window) -> bool {
    crate::ui::rows::selection_emphasized(window)
}

/// Find in a 话题 (⌘F): the fill behind the matched words. Neutral like the
/// rest of the palette: `foreground` at a low alpha behind every match, and
/// a stronger one behind the current match, so the text keeps its own color
/// and contrast in both modes.
pub mod find {
    use super::Palette;
    use gpui::Hsla;

    /// The fill behind every match that is not the current one.
    pub fn match_fill(p: &Palette) -> Hsla {
        p.foreground.opacity(if p.dark { 0.18 } else { 0.10 })
    }

    /// The fill behind the current match.
    pub fn current_fill(p: &Palette) -> Hsla {
        p.foreground.opacity(if p.dark { 0.40 } else { 0.28 })
    }
}

// ---------------------------------------------------------------------------
// Scales (§1.4–§1.8)

/// Spacing (§1.5): 2, 4, 6, 8, 12, 16, 24, 32, 48. The older off-scale
/// names map to the nearest step.
pub mod space {
    use gpui::{Pixels, px};
    pub const S2: Pixels = px(2.);
    pub const S4: Pixels = px(4.);
    pub const S6: Pixels = px(6.);
    pub const S8: Pixels = px(8.);
    pub const S12: Pixels = px(12.);
    pub const S16: Pixels = px(16.);
    pub const S24: Pixels = px(24.);
    pub const S32: Pixels = px(32.);
    pub const S48: Pixels = px(48.);
    /// The whole scale, smallest first.
    pub const SCALE: [Pixels; 9] = [S2, S4, S6, S8, S12, S16, S24, S32, S48];

    // Older names, mapped onto the scale.
    pub const S3: Pixels = S4;
    pub const S10: Pixels = S12;
    pub const S14: Pixels = S16;
    pub const S20: Pixels = S24;
    pub const S28: Pixels = S32;
    pub const S44: Pixels = S48;
}

/// Control and row heights (D3, §1.8, §2).
pub mod height {
    use gpui::{Pixels, px};
    /// Control sm: buttons and fields in form rows, table cells, dense cards.
    pub const SM: Pixels = px(28.);
    /// Control default: buttons, inputs, selects, segmented, icon buttons.
    pub const DEFAULT: Pixels = px(32.);
    /// Control lg: login, full-page empty CTA, composer Send.
    pub const LG: Pixels = px(36.);
    /// The toolbar row and every pane header (§2.2): 64, one line centered
    /// in it (y 32), so the title has room above it and sits a little under
    /// the traffic lights. The subtitle sits after the title on that line,
    /// never under it (owner decision, v18).
    pub const TOOLBAR: Pixels = px(64.);
    /// Sidebar item, menu item, palette row, group label, compact row.
    pub const ROW: Pixels = px(32.);
    /// A menu group label.
    pub const MENU_LABEL: Pixels = px(28.);
    /// Form rows (§2.8): single line and two lines.
    pub const FORM_ROW: Pixels = px(44.);
    pub const FORM_ROW_TWO: Pixels = px(64.);
    /// List row with media and subtitle (§2.6).
    pub const LIST_ROW: Pixels = px(56.);
    /// Table rows (§2.7): default, compact, with an h28 control; header.
    pub const TABLE_ROW: Pixels = px(48.);
    pub const TABLE_ROW_COMPACT: Pixels = px(40.);
    pub const TABLE_ROW_CONTROL: Pixels = px(52.);
    pub const TABLE_HEAD: Pixels = px(40.);
    /// A list footer row and the palette footer.
    pub const FOOTER_ROW: Pixels = px(40.);
    /// The palette input row.
    pub const PALETTE_INPUT: Pixels = px(48.);
    /// The pinned sidebar user row: p 8 around a 40 button, so the 32
    /// avatar and the two lines (20 + 16) fit without clipping.
    pub const SIDEBAR_FOOTER: Pixels = px(56.);
    /// Badge, count pill, kbd, segmented trigger.
    pub const BADGE: Pixels = px(22.);
    pub const COUNT: Pixels = px(20.);
    pub const KBD: Pixels = px(20.);
    pub const SEGMENT_TRIGGER: Pixels = px(26.);
    /// Progress (D12).
    pub const PROGRESS: Pixels = px(4.);
    /// The three control heights.
    pub const CONTROLS: [Pixels; 3] = [SM, DEFAULT, LG];
}

/// Radii (D5, §1.6). Window chrome keeps its own (sidebar 18, sheet 22).
pub mod radius {
    use gpui::{Pixels, px};
    /// Checkbox, kbd.
    pub const CHECKBOX: Pixels = px(4.);
    /// Menu item, palette row, 32 media box, small option box.
    pub const ITEM: Pixels = px(6.);
    /// Button, input, select, segmented trigger, sidebar item, popover.
    pub const CONTROL: Pixels = px(8.);
    /// Group box, table frame, tabs list, alert, empty state, toast, composer.
    pub const CONTAINER: Pixels = px(10.);
    /// Card, inspector section card, media viewer.
    pub const CARD: Pixels = px(14.);
    /// Window chrome: the Mac sheet window.
    pub const SHEET: Pixels = px(22.);
    /// Pills: badge, count, avatar, switch, search field.
    pub const FULL: Pixels = px(9999.);
    /// The whole scale (without chrome and full).
    pub const SCALE: [Pixels; 5] = [CHECKBOX, ITEM, CONTROL, CONTAINER, CARD];

    // Older names, mapped onto the scale.
    pub const MARK: Pixels = CHECKBOX;
    pub const SM: Pixels = ITEM;
    pub const MD: Pixels = CONTROL;
    pub const FIELD: Pixels = CONTAINER;
    pub const LG: Pixels = CARD;
    pub const BUTTON_CARD: Pixels = CONTROL;
    pub const XS: Pixels = MARK;
    pub const CHIP: Pixels = SM;
    pub const TILE: Pixels = SM;
    pub const ROW: Pixels = MD;
    pub const COMPOSER: Pixels = CONTAINER;
    pub const XL: Pixels = CARD;

    /// Agent avatars: a squircle at 0.25 × size (8 at 32, 6 at 24).
    pub fn bot(size: Pixels) -> Pixels {
        size * 0.25
    }
}

/// The type scale (§1.4): (size, line height). Six sizes: 12, 14, 16, 18,
/// 24, 30, plus 13 mono.
pub mod text {
    use gpui::{Pixels, px};
    /// KPI number (600, tabular).
    pub const KPI: (Pixels, Pixels) = (px(30.), px(36.));
    /// KPI number in a block narrower than 250.
    pub const KPI_COMPACT: (Pixels, Pixels) = (px(24.), px(32.));
    /// Hero title: login, member profile (600, 700 allowed).
    pub const HERO: (Pixels, Pixels) = (px(24.), px(32.));
    /// Dialog, sheet, alert and empty-state title.
    pub const TITLE: (Pixels, Pixels) = (px(18.), px(28.));
    /// Toolbar, card, pane and section title.
    pub const HEADING: (Pixels, Pixels) = (px(16.), px(24.));
    /// Body, list title, table cell, menu item, input, button, description.
    pub const UI: (Pixels, Pixels) = (px(14.), px(20.));
    /// Message prose.
    pub const PROSE: (Pixels, Pixels) = (px(14.), px(22.));
    /// Meta, timestamp, toolbar subtitle, group label, badge, caption, kbd.
    pub const META: (Pixels, Pixels) = (px(12.), px(16.));
    /// Code (mono).
    pub const MONO: (Pixels, Pixels) = (px(13.), px(20.));
    /// The six UI sizes.
    pub const SIZES: [Pixels; 6] = [px(12.), px(14.), px(16.), px(18.), px(24.), px(30.)];

    // Older native names, mapped onto the scale.
    pub const LARGE_TITLE: (Pixels, Pixels) = HERO;
    pub const TITLE2: (Pixels, Pixels) = TITLE;
    pub const TITLE3: (Pixels, Pixels) = HEADING;
    pub const HEADLINE: (Pixels, Pixels) = UI;
    pub const BODY: (Pixels, Pixels) = UI;
    pub const MESSAGE: (Pixels, Pixels) = PROSE;
    pub const CALLOUT: (Pixels, Pixels) = META;
    pub const SUBHEADLINE: (Pixels, Pixels) = META;
    pub const SECTION: (Pixels, Pixels) = META;
    pub const FOOTNOTE: (Pixels, Pixels) = META;
}

/// Icon sizes: 12 in a badge, 16 everywhere else, 24 in an empty state.
pub mod icon {
    use gpui::{Pixels, px};
    pub const SM: Pixels = px(12.);
    pub const MD: Pixels = px(16.);
    pub const LG: Pixels = px(24.);
    /// The app icon at the top of a welcome sheet (已更新到 …).
    pub const APP: Pixels = px(64.);

    // Older names, mapped onto the sizes.
    pub const HEADER: Pixels = MD;
    pub const ROW: Pixels = MD;
    pub const CHIP: Pixels = SM;
    pub const TAG: Pixels = SM;
    pub const EMPTY: Pixels = LG;
    pub const TOOLBAR: Pixels = HEADER;
    pub const TODO: Pixels = ROW;
    pub const MENU: Pixels = ROW;
    pub const BUTTON: Pixels = ROW;
    pub const SMALL: Pixels = CHIP;
}

/// Avatar sizes (§1.8): 16, 24, 32, 40, 64 only.
pub mod avatar {
    use gpui::{Pixels, px};
    pub const XS: Pixels = px(16.);
    pub const SM: Pixels = px(24.);
    pub const MD: Pixels = px(32.);
    pub const LG: Pixels = px(40.);
    pub const XL: Pixels = px(64.);
    pub const SIZES: [Pixels; 5] = [XS, SM, MD, LG, XL];
    /// The presence dot and its ring of the surface.
    pub const PRESENCE: Pixels = px(8.);
    pub const PRESENCE_RING: Pixels = px(2.);
}

/// Shadows (§1.7). GPUI blurs by the standard deviation and CSS by twice
/// it, so each CSS blur is halved here.
pub mod shadow {
    use gpui::{BoxShadow, Hsla, Pixels, point, px, rgba};

    fn s(alpha: u32, y: f32, blur: f32, spread: f32) -> BoxShadow {
        BoxShadow {
            color: rgba(alpha).into(),
            offset: point(px(0.), px(y)),
            blur_radius: px(blur / 2.),
            spread_radius: px(spread),
            inset: false,
        }
    }

    /// 0 1px 2px / 5 %: input, outline button, KPI card.
    pub fn xs() -> Vec<BoxShadow> {
        vec![s(0x0000000D, 1., 2., 0.)]
    }
    /// 0 1px 3px / 10 %, 0 1px 2px -1px / 10 %: card, active segment.
    pub fn sm() -> Vec<BoxShadow> {
        vec![s(0x0000001A, 1., 3., 0.), s(0x0000001A, 1., 2., -1.)]
    }
    /// 0 4px 6px -1px, 0 2px 4px -2px / 10 %: menu, popover, toast, palette.
    pub fn md() -> Vec<BoxShadow> {
        vec![s(0x0000001A, 4., 6., -1.), s(0x0000001A, 2., 4., -2.)]
    }
    /// 0 10px 15px -3px, 0 4px 6px -4px / 10 %: sheet, alert.
    pub fn lg() -> Vec<BoxShadow> {
        vec![s(0x0000001A, 10., 15., -3.), s(0x0000001A, 4., 6., -4.)]
    }
    /// A 1 px line outside the edge in `color`, drawn as a shadow so the
    /// layout does not move.
    pub fn line(color: Hsla) -> BoxShadow {
        ring(color, px(1.))
    }
    /// A ring of `width` outside the edge.
    pub fn ring(color: Hsla, width: Pixels) -> BoxShadow {
        BoxShadow {
            color,
            offset: point(px(0.), px(0.)),
            blur_radius: px(0.),
            spread_radius: width,
            inset: false,
        }
    }
}

// ---------------------------------------------------------------------------
// Older names (aliases onto the scales above)

/// `theme::size`: control and row heights.
pub mod size {
    use super::{avatar, height};
    use gpui::{Pixels, px};
    pub const BUTTON_SM: Pixels = height::SM;
    pub const BUTTON_CARD: Pixels = height::SM;
    pub const BUTTON_CARD_PAD: Pixels = super::space::S12;
    pub const BUTTON_MD: Pixels = height::DEFAULT;
    pub const BUTTON_PAGE: Pixels = height::DEFAULT;
    pub const BUTTON_LG: Pixels = height::LG;
    pub const TEXT_BUTTON: Pixels = height::SM;
    pub const ICON_BUTTON: Pixels = height::DEFAULT;
    pub const ICON_BUTTON_SM: Pixels = height::SM;
    pub const SEARCH_FIELD: Pixels = height::DEFAULT;
    pub const FORM_FIELD: Pixels = height::DEFAULT;
    pub const SELECT: Pixels = height::DEFAULT;
    /// One width for every select in a column (§2.8).
    pub const SELECT_WIDTH: Pixels = px(160.);
    /// A pop-up button whose choices are names (a computer, with 这台电脑 after it).
    pub const SELECT_WIDE: Pixels = px(280.);
    /// The search field of the find bar of a 话题 (⌘F).
    pub const FIND_FIELD_WIDTH: Pixels = px(240.);
    /// Invite code box: 40 × 48.
    pub const INVITE_BOX: (Pixels, Pixels) = (px(40.), px(48.));
    /// Composer: p 8 + one 20 line + 8 + the h28 action row + p 8.
    pub const COMPOSER_MIN: Pixels = px(72.);
    pub const COMPOSER_TEXT_MIN: Pixels = px(20.);
    pub const COMPOSER_TOOLBAR: Pixels = px(40.);
    pub const FORMAT_BAR: Pixels = height::LG;
    pub const COMPOSER_MAX_SHARE: f32 = 0.4;
    pub const CHIP: Pixels = height::BADGE;
    pub const CHIP_COMPACT: Pixels = height::COUNT;
    pub const CHIP_MAX: Pixels = px(200.);
    pub const CHIP_COMPACT_MAX: Pixels = px(72.);
    pub const BADGE: Pixels = height::BADGE;
    pub const KBD: Pixels = height::KBD;
    pub const ROLE_TAG: Pixels = height::BADGE;
    pub const REACTION: Pixels = height::SM;
    pub const SECTION_HEADER: Pixels = height::ROW;
    pub const SIDEBAR_ROW: Pixels = height::ROW;
    pub const SIDEBAR_FOOTER: Pixels = height::SIDEBAR_FOOTER;
    pub const TOPIC_ROW: Pixels = px(68.);
    pub const MENU_ROW: Pixels = height::ROW;
    pub const POPUP_ROW: Pixels = height::ROW;
    pub const SWITCHER_ROW: Pixels = height::ROW;
    pub const SWITCHER_HIT_ROW: Pixels = height::LIST_ROW;
    pub const SWITCHER_QUERY: Pixels = height::PALETTE_INPUT;
    pub const SETTINGS_ROW: Pixels = height::FORM_ROW_TWO;
    pub const SETTINGS_LIST_ROW: Pixels = height::FORM_ROW;
    pub const SETTINGS_FIELD_WIDTH: Pixels = px(280.);
    pub const SETTINGS_VALUE_MAX: Pixels = px(320.);
    /// The settings window: 592 of content and a 24 gutter each side.
    pub const SETTINGS_COLUMN: Pixels = px(640.);
    pub const SETTINGS_AVATAR: Pixels = avatar::LG;
    pub const OPTION_ROW: Pixels = px(40.);
    pub const PANEL_ROW: Pixels = px(40.);
    pub const PLAN_ROW: Pixels = height::ROW;
    pub const BANNER: Pixels = px(52.);
    pub const TOAST: Pixels = px(48.);
    pub const AVATAR_MESSAGE: Pixels = avatar::MD;
    pub const AVATAR_FOOTER: Pixels = avatar::MD;
    pub const AVATAR_ROW: Pixels = avatar::SM;
    pub const AVATAR_START: Pixels = avatar::LG;
    /// Overlay widths: palette 600, sheets 460 / 560, alert 260 (§2.9–§2.11).
    pub const SWITCHER_WIDTH: Pixels = px(600.);
    pub const DIALOG_WIDTH: Pixels = px(460.);
    pub const DIALOG_WIDE: Pixels = px(560.);
    pub const ALERT_WIDTH: Pixels = px(260.);
    pub const MENTION_POPUP_WIDTH: Pixels = px(320.);
    pub const PALETTE_TOP: Pixels = px(96.);
}

/// `theme::type_scale`: (size, line height) of each older text style.
pub mod type_scale {
    use super::text;
    use gpui::{Pixels, px};
    pub const DISPLAY: (Pixels, Pixels) = text::HERO;
    pub const HEADING: (Pixels, Pixels) = text::TITLE;
    pub const PAGE_TITLE: (Pixels, Pixels) = text::HEADING;
    pub const TITLE: (Pixels, Pixels) = text::TITLE;
    pub const CARD_TITLE: (Pixels, Pixels) = text::HEADING;
    pub const BODY: (Pixels, Pixels) = text::PROSE;
    pub const UI: (Pixels, Pixels) = text::UI;
    pub const SMALL: (Pixels, Pixels) = text::META;
    pub const META: (Pixels, Pixels) = text::META;
    pub const CODE: (Pixels, Pixels) = text::MONO;
    /// Invite code boxes.
    pub const CODE_ENTRY: (Pixels, Pixels) = text::TITLE;
    pub const BUTTON_LG: (Pixels, Pixels) = text::UI;
    /// A message of one to three emoji (not interface text).
    pub const EMOJI: (Pixels, Pixels) = (px(32.), px(40.));
    /// Emoji in reaction pills.
    pub const REACTION_EMOJI: (Pixels, Pixels) = (px(16.), px(16.));
    pub const KPI: (Pixels, Pixels) = text::KPI;

    pub const TOPIC_TITLE: (Pixels, Pixels) = HEADING;
    pub const MESSAGE: (Pixels, Pixels) = BODY;
    pub const QUOTE: (Pixels, Pixels) = SMALL;
    pub const CALLOUT: (Pixels, Pixels) = SMALL;
    pub const ACK: (Pixels, Pixels) = SMALL;
    pub const CAPTION: (Pixels, Pixels) = META;
    pub const INVITE: (Pixels, Pixels) = CODE_ENTRY;
    pub const OTP: (Pixels, Pixels) = CODE_ENTRY;
}

/// The window layout (§2.1): column widths, header rows and older layout
/// names. `theme` re-exports these at its top level.
pub mod layout {
    use super::{avatar, height, space};
    use gpui::{Pixels, px};

    /// The sidebar panel (x 8–256 of a 264 column).
    pub const SIDEBAR_WIDTH: Pixels = px(248.);
    pub const SIDEBAR_MIN_WIDTH: Pixels = px(200.);
    pub const SIDEBAR_MAX_WIDTH: Pixels = px(320.);
    pub const TOPIC_LIST_WIDTH: Pixels = px(320.);
    pub const TOPIC_LIST_MIN_WIDTH: Pixels = px(280.);
    pub const TOPIC_LIST_MAX_WIDTH: Pixels = px(400.);
    /// The right pane (D13).
    pub const RIGHT_PANE: Pixels = px(320.);
    pub const SIDE_PANEL_WIDTH: Pixels = px(300.);
    pub const TOPIC_VIEW_MIN_WIDTH: Pixels = px(480.);
    pub const TOOLBAR_HEIGHT: Pixels = height::TOOLBAR;
    pub const HEADER_SECOND_ROW: Pixels = px(40.);
    pub const HEADER_BAND: Pixels = px(92.);
    pub const SIDE_PANEL_MIN_WINDOW: Pixels = px(1348.);
    pub const WINDOW_MIN_WIDTH: Pixels = px(1100.);
    pub const SIDE_PANEL_INLINE_MIN_WINDOW: Pixels = px(1260.);
    pub const RAIL_WIDTH: Pixels = px(260.);
    pub const TITLE_BAR: Pixels = height::TOOLBAR;
    pub const TOPIC_RAIL_WIDTH: Pixels = RIGHT_PANE;
    pub const SIDE_CHAT_WIDTH: Pixels = RIGHT_PANE;
    pub const LIVE_PANE_SHARE: f32 = 0.5;
    pub const LIVE_PANE_MIN_WIDTH: Pixels = px(460.);
    pub const LIVE_PANE_MAX_WIDTH: Pixels = px(760.);
    /// The live and 产出 panes (owner decision, v14): 400.
    pub const LIVE_PANE_WIDTH: Pixels = px(400.);
    pub const ARTIFACT_PANE_WIDTH: Pixels = px(400.);
    pub const ARTIFACT_RAIL_WIDTH: Pixels = RIGHT_PANE;
    pub const DESK_SIDE_WIDTH: Pixels = RIGHT_PANE;
    pub const MEMBERS_LIST_WIDTH: Pixels = px(480.);
    pub const PAGE_MIN_WIDTH: Pixels = px(480.);
    pub const PAGE_MEASURE: Pixels = px(720.);
    /// Toolbar icon buttons (D4).
    pub const TITLE_BUTTON: Pixels = height::DEFAULT;
    /// Sidebar rows and group labels.
    pub const RAIL_ROW: Pixels = height::ROW;
    pub const RAIL_GROUP_ROW: Pixels = height::ROW;
    pub const RAIL_SLOT: Pixels = px(16.);
    pub const RAIL_STATE_DOT: Pixels = px(8.);
    pub const RAIL_STATE_STROKE: Pixels = px(1.25);
    pub const RAIL_STATE_CHECK: Pixels = px(12.);
    /// The role tag: a badge.
    pub const TAG_HEIGHT: Pixels = height::BADGE;
    pub const TAG_RADIUS: Pixels = px(11.);
    pub const AVATAR_STATUS_CUT: Pixels = avatar::PRESENCE_RING;
    pub const RAIL_SECTION_PAD_Y: Pixels = space::S16;
    pub const RAIL_SECTION_PAD_X: Pixels = space::S4;
    pub const LIST_ROW_BLEED: Pixels = space::S8;
    pub const WINDOW_MIN_HEIGHT: Pixels = px(680.);
    pub const WINDOW_DEFAULT_WIDTH: Pixels = px(1440.);
    pub const WINDOW_DEFAULT_HEIGHT: Pixels = px(900.);
    /// The red light's origin: the lights centre on the toolbar row's centre
    /// line (y 32), with the page title and the sidebar toggle.
    pub const TRAFFIC_LIGHTS: (Pixels, Pixels) = (px(18.), px(25.));
    pub const TRAFFIC_LIGHTS_INSET: Pixels = px(84.);
    /// The sidebar toggle (隐藏侧栏 / 显示侧栏) sits right after the traffic
    /// lights, on the toolbar row's center line, and stays there whether the
    /// sidebar is shown or hidden.
    pub const SIDEBAR_TOGGLE_X: Pixels = px(80.);
    /// In full screen (no traffic lights) it sits at the leading edge.
    pub const SIDEBAR_TOGGLE_X_FULLSCREEN: Pixels = space::S16;
    /// The toggle (32) and the gap before the page title (24): a page's
    /// title starts this far past the toggle's x with the sidebar hidden.
    pub const SIDEBAR_TOGGLE_ROOM: Pixels = px(32. + 24.);
    /// The message avatar (D7).
    pub const AVATAR_SIZE: Pixels = avatar::MD;
    pub const ROW_HEIGHT: Pixels = height::ROW;
    /// The one in-thread width (§2.4, T1): every message body, card,
    /// attachment item, reference block and embed in a thread is
    /// `min(IN_THREAD, text column)` wide. See [`in_thread_width`].
    pub const IN_THREAD: Pixels = px(640.);
    /// Older name of [`IN_THREAD`].
    pub const MEASURE: Pixels = IN_THREAD;

    /// The width of an in-thread item in a text column `column` wide.
    pub fn in_thread_width(column: Pixels) -> Pixels {
        if column < IN_THREAD {
            column
        } else {
            IN_THREAD
        }
    }
    /// The message column gutter (§2.1).
    pub const CONTENT_GUTTER: Pixels = space::S24;
    pub const HAIRLINE: Pixels = px(1.);
    pub const BORDER: Pixels = px(1.);
    pub const EMBED_FADE: Pixels = space::S48;
}

/// The native kit's metrics (NATIVE.md names), on the scales above.
pub mod metrics {
    use super::{height, radius, space};
    use gpui::{Pixels, px};

    pub const HAIRLINE: Pixels = px(1.);
    pub const FOCUS_RING: Pixels = px(3.);

    // Window (§2.1, §2.2)
    pub const TOOLBAR_ROW: Pixels = height::TOOLBAR;
    pub const TOOLBAR_GROUP: Pixels = height::DEFAULT;
    pub const TOOLBAR_ITEM: Pixels = height::DEFAULT;
    pub const TOOLBAR_PILL: Pixels = height::SM;
    pub const TOOLBAR_PILL_INSET: Pixels = space::S4;
    pub const TOOLBAR_GROUP_GAP: Pixels = space::S8;
    /// From the sidebar's trailing edge (x 264) to the content (x 288).
    pub const CONTENT_GAP: Pixels = space::S24;
    /// The gutter from the page column (x 264, the sidebar's trailing edge)
    /// to the content (x 288).
    pub const CONTENT_LEADING: Pixels = space::S24;
    pub const TOOLBAR_LEADING: Pixels = CONTENT_LEADING;
    pub const TOOLBAR_TRAILING: Pixels = space::S16;
    pub const TOOLBAR_SYMBOL: Pixels = px(16.);
    pub const SIDEBAR_INSET: Pixels = space::S8;
    pub const SIDEBAR_WIDTH: Pixels = px(248.);
    pub const SIDEBAR_ROW: Pixels = height::ROW;
    pub const SIDEBAR_ROW_INSET: Pixels = space::S8;
    pub const SIDEBAR_ROW_RADIUS: Pixels = radius::CONTROL;
    pub const SIDEBAR_SLOT: Pixels = px(16.);
    pub const SIDEBAR_SYMBOL: Pixels = px(16.);
    pub const SIDEBAR_SECTION: Pixels = height::ROW;
    /// Every right pane (D13).
    pub const INSPECTOR_WIDTH: Pixels = super::layout::RIGHT_PANE;
    /// The live and 产出 panes.
    pub const WIDE_PANE_WIDTH: Pixels = px(400.);
    pub const PANE_TOP: Pixels = TOOLBAR_ROW;

    // Buttons (§1.8)
    pub const BUTTON_REGULAR: Pixels = height::DEFAULT;
    pub const BUTTON_REGULAR_RADIUS: Pixels = radius::CONTROL;
    pub const BUTTON_REGULAR_PAD: Pixels = space::S16;
    pub const BUTTON_MIN_WIDTH: Pixels = px(64.);
    pub const BUTTON_SMALL: Pixels = height::SM;
    pub const BUTTON_SMALL_RADIUS: Pixels = radius::CONTROL;
    pub const BUTTON_SMALL_PAD: Pixels = space::S12;
    pub const BUTTON_LARGE: Pixels = height::LG;
    pub const BUTTON_LARGE_PAD: Pixels = space::S24;
    pub const SQUARE_BUTTON: Pixels = height::SM;
    pub const POPUP_CHEVRON: Pixels = px(16.);
    pub const POPUP_CHEVRON_SLOT: Pixels = px(16.);

    // Switch, checkbox, radio (§1.8)
    pub const SWITCH_SMALL: (Pixels, Pixels) = (px(32.), px(18.));
    pub const SWITCH_REGULAR: (Pixels, Pixels) = SWITCH_SMALL;
    pub const SWITCH_INSET: Pixels = px(2.);
    pub const CHECKBOX: Pixels = px(16.);
    pub const CHECKBOX_RADIUS: Pixels = radius::CHECKBOX;
    pub const CHECKMARK: Pixels = px(12.);
    pub const RADIO_DOT: Pixels = px(6.);

    // Segmented (§1.8): list h32, p3, trigger h26 px8.
    pub const SEGMENTED: Pixels = height::DEFAULT;
    pub const SEGMENTED_TOOLBAR: Pixels = height::DEFAULT;
    pub const SEGMENT_INSET: Pixels = px(3.);
    pub const SEGMENT_PAD: Pixels = space::S8;

    // Text entry (§1.8)
    pub const FIELD: Pixels = height::DEFAULT;
    pub const FIELD_LARGE: Pixels = height::LG;
    pub const FIELD_RADIUS: Pixels = radius::CONTROL;
    pub const FIELD_PAD: Pixels = space::S12;
    pub const FIELD_NAME_WIDTH: Pixels = px(220.);
    pub const FIELD_CODE_WIDTH: Pixels = px(140.);
    pub const SEARCH_SIDEBAR: Pixels = height::DEFAULT;
    pub const SEARCH: Pixels = height::DEFAULT;
    pub const SEARCH_SYMBOL: Pixels = px(16.);

    // One-time code (§2.20): bordered slots 40 x 44, radius 8, in groups
    // with a dash between them.
    pub const CODE_SLOT: (Pixels, Pixels) = (px(40.), px(44.));
    pub const CODE_SLOT_RADIUS: Pixels = radius::CONTROL;
    pub const CODE_SLOT_GAP: Pixels = space::S6;
    /// The space between two groups; the dash sits in its middle.
    pub const CODE_GROUP_GAP: Pixels = px(20.);
    pub const CODE_DASH: (Pixels, Pixels) = (px(8.), px(2.));
    pub const CODE_CARET: (Pixels, Pixels) = (px(2.), px(20.));
    /// The 3 px ring of the active slot: foreground at 15 %.
    pub const CODE_RING_OPACITY: f32 = 0.15;
    /// A disabled primary button before its form is complete: primary at
    /// 30 %, light, never a mid gray (§2.20).
    pub const PRIMARY_WAITING_OPACITY: f32 = 0.3;

    // Progress (D12)
    pub const BAR: Pixels = height::PROGRESS;
    pub const BAR_RADIUS: Pixels = px(2.);
    pub const SPINNER: Pixels = px(16.);

    // Grouped forms (§2.8)
    pub const GROUP_RADIUS: Pixels = radius::CONTAINER;
    pub const GROUP_MARGIN: Pixels = space::S24;
    pub const FORM_ROW: Pixels = height::FORM_ROW;
    pub const FORM_ROW_INSET: Pixels = space::S16;
    pub const GROUP_GAP: Pixels = space::S24;
    pub const GROUP_HEADER_GAP: Pixels = space::S12;
    pub const INLINE_FIELD_WIDTH: Pixels = px(220.);

    // Inspector and lists (§2.5, §2.6)
    pub const INSPECTOR_ROW: Pixels = height::ROW;
    pub const INSPECTOR_PAD: Pixels = space::S16;
    /// The label column of a DescriptionList and of every sheet.
    pub const LABEL_COLUMN: Pixels = px(112.);
    pub const LABEL_GAP: Pixels = space::S8;
    pub const LIST_ROW: Pixels = height::ROW;
    pub const LIST_ROW_RADIUS: Pixels = radius::ITEM;
    pub const LIST_ROW_INSET: Pixels = space::S8;

    // Overlays (§2.9–§2.11)
    pub const SHEET_WIDTH: Pixels = px(460.);
    pub const SHEET_WIDE: Pixels = px(560.);
    pub const SHEET_RADIUS: Pixels = radius::SHEET;
    pub const SHEET_PAD: Pixels = space::S24;
    pub const SHEET_BUTTON_EDGE: Pixels = space::S24;
    pub const SHEET_BUTTON_GAP: Pixels = space::S8;
    /// A Mac dialog button is never narrower than this, so 「好」 is not a square.
    pub const SHEET_BUTTON_MIN_WIDTH: Pixels = px(80.);
    pub const ALERT_WIDTH: Pixels = px(260.);
    pub const ALERT_ICON: Pixels = px(48.);
    pub const ALERT_PAD: Pixels = space::S16;
    pub const MENU_RADIUS: Pixels = radius::CONTROL;
    pub const MENU_PAD: Pixels = space::S4;
    pub const MENU_ROW: Pixels = height::ROW;
    pub const MENU_ROW_RADIUS: Pixels = radius::ITEM;
    pub const MENU_SYMBOL: Pixels = px(16.);
    pub const MENU_ROW_PAD: Pixels = space::S8;
    pub const POPOVER_RADIUS: Pixels = radius::CONTROL;
    pub const POPOVER_ARROW: Pixels = px(10.);
}

/// Card v2 sizes (DESIGN.md §7.10) on the scales; `theme::card_v2`.
pub mod card_v2 {
    use super::{height, radius, space, text};
    use gpui::{Hsla, Pixels, px};

    pub const STRIP: Pixels = px(32.);
    /// The card title: 16 / 24, 600.
    pub const TITLE: (Pixels, Pixels) = text::HEADING;
    /// The in-thread card width (§2.4): the one in-thread width.
    pub const MAX_WIDTH: Pixels = super::layout::IN_THREAD;
    pub const TILE: Pixels = px(20.);
    pub const PATH_GROUP_MIN: Pixels = px(24.);
    /// Option rows (choice RadioGroup rows, §2.17).
    pub const OPTION: Pixels = px(40.);
    pub const OPTION_RADIUS: Pixels = radius::CONTROL;
    pub const FORM_LABEL: Pixels = px(112.);
    pub const FIELD_KEY: Pixels = px(112.);
    pub const CHIP: Pixels = height::SM;
    pub const CHIP_RADIUS: Pixels = radius::CONTROL;
    pub const PROOF: Pixels = height::SM;
    pub const BUTTON: Pixels = super::size::BUTTON_CARD;
    pub const FOOTER_BOTTOM: Pixels = space::S16;
    pub const PLAN_ROW: Pixels = height::ROW;
    pub const REF_ROW: Pixels = height::LIST_ROW;
    pub const REF_TILE: Pixels = px(32.);
    /// Compact table rows inside a card (§2.7).
    pub const TABLE_ROW: Pixels = height::TABLE_ROW_COMPACT;
    pub const TABLE_NUM_COL: Pixels = px(64.);
    pub const TABLE_PCT_BAR: Pixels = px(48.);
    pub const DIFF_LINE: Pixels = px(20.);
    pub const DIFF_NUMBER: Pixels = px(32.);
    pub const CHART: Pixels = px(100.);
    pub const THUMB: Pixels = px(84.);
    pub const LIVE_BAR: Pixels = px(2.);

    pub const YOU_FILL: f32 = 0.10;
    pub const YOU_EDGE: f32 = 0.24;
    pub const YOU_BORDER: f32 = 0.30;
    pub const OK_FILL: f32 = 0.09;
    pub const OK_EDGE: f32 = 0.22;
    pub const LIVE_FILL: f32 = 0.08;
    pub const UPDATED_FILL: f32 = 0.06;
    pub const LIVE_EDGE: f32 = 0.22;
    pub const QUIET_FILL: f32 = 0.09;
    pub const QUIET_EDGE: f32 = 0.16;
    pub const DIFF_FILL: f32 = 0.10;
    pub const MUTED: f32 = 0.55;

    /// `color-mix(in srgb, a t, b)`.
    pub fn mix(a: Hsla, b: Hsla, t: f32) -> Hsla {
        let (a, b) = (a.to_rgb(), b.to_rgb());
        let l = |x: f32, y: f32| x * t + y * (1. - t);
        gpui::Rgba {
            r: l(a.r, b.r),
            g: l(a.g, b.g),
            b: l(a.b, b.b),
            a: l(a.a, b.a),
        }
        .into()
    }

    /// `a` at alpha `t` (`color-mix(in srgb, a t, transparent)`).
    pub fn alpha(a: Hsla, t: f32) -> Hsla {
        Hsla { a: a.a * t, ..a }
    }
}

/// Card v2.1 sizes and timings (CardsCompose.dc.html); `theme::card_v21`.
pub mod card_v21 {
    use super::{height, radius};
    use std::time::Duration;

    use gpui::{Pixels, px};

    pub const RANK_ROW: Pixels = px(40.);
    pub const SLOT_CELL: Pixels = px(48.);
    pub const POLL_ROW: Pixels = height::FORM_ROW;
    pub const STAR: Pixels = px(24.);
    pub const SCALE_CELL: Pixels = height::DEFAULT;
    pub const IMAGE: Pixels = px(150.);
    pub const PREVIEW: Pixels = px(120.);
    pub const MATRIX_COL: Pixels = px(72.);
    pub const BAR_ROW: Pixels = height::DEFAULT;
    pub const BAR_NAME: Pixels = px(64.);
    pub const BAR_TRACK: Pixels = px(8.);
    pub const BAR_TRACK_RADIUS: Pixels = radius::FULL;
    pub const BAR_VALUE: Pixels = px(24.);
    pub const BAR_ROW_RADIUS: Pixels = radius::CONTROL;
    pub const EMBED_MIN: f32 = 160.;
    pub const EMBED_MAX: f32 = 480.;
    pub const EMBED_DEFAULT: f32 = 320.;
    pub const SURFACE_FILL: f32 = 0.07;
    pub const CHANGE_FILL: f32 = 0.14;
    pub const INSERTED_FILL: f32 = 0.07;
    pub const PENDING_QUIET: Duration = Duration::from_millis(400);
    pub const SOFT_LIVE: Duration = Duration::from_secs(8);
    pub const HARD_LIVE: Duration = Duration::from_secs(45);
    pub const SOFT_RUN: Duration = Duration::from_secs(20);
    pub const HARD_RUN: Duration = Duration::from_secs(120);
    pub const SOFT_STARTING: Duration = Duration::from_secs(45);
    pub const HARD_STARTING: Duration = Duration::from_secs(180);
    pub const OFFLINE_NOTE: Duration = Duration::from_secs(4);
    pub const RESOLVED_NOTE: Duration = Duration::from_secs(5);
    pub const DEBOUNCE_TEXT: Duration = Duration::from_millis(600);
    pub const DEBOUNCE_DRAFT: Duration = Duration::from_millis(1000);
    pub const CHANGE_IN: Duration = Duration::from_millis(200);
    /// The flash of a changed part: 700 ms (T1-17). Its tint is muted.
    pub const CHANGE_FLASH: Duration = Duration::from_millis(700);
    pub const HOVER_ASK: Duration = Duration::from_millis(300);
    pub const PART_FLASH: Duration = Duration::from_millis(600);
    pub const CHART_PICK_FILL: f32 = 0.06;
    pub const CHART_HOVER_FILL: f32 = 0.035;
    pub const REMOVE_OUT: Duration = Duration::from_millis(240);
}

/// The desk terminal thumbnail (a miniature, not reading text);
/// `theme::desk_screen`.
pub mod desk_screen {
    use gpui::{Pixels, px};
    use std::time::Duration;
    pub const HEIGHT: Pixels = px(120.);
    pub const LINE: (Pixels, Pixels) = (px(10.), px(12.));
    pub const LINE_GAP: Pixels = px(4.);
    pub const TOP: Pixels = px(32.);
    pub const SCROLL_PER_LINE: Duration = Duration::from_millis(900);
}

/// The machine monitor window (开发者视图): its frame and the few sizes its parts need beyond
/// the scale.
pub mod monitor {
    use super::space;
    use gpui::{Pixels, px};

    pub const WINDOW_WIDTH: Pixels = px(1216.);
    pub const WINDOW_HEIGHT: Pixels = px(900.);
    pub const MIN_WIDTH: Pixels = px(960.);
    pub const MIN_HEIGHT: Pixels = px(640.);
    /// The sidebar's row column (the panel adds its insets).
    pub const SIDEBAR_WIDTH: Pixels = px(216.);
    /// The machine tile of the switcher.
    pub const MACHINE_TILE: Pixels = px(32.);
    /// A KPI card's sparkline.
    pub const SPARK_HEIGHT: Pixels = space::S48;
    pub const SPARK_STROKE: Pixels = px(1.5);
    /// The capacity bar and its gaps.
    pub const CAPACITY_BAR: Pixels = space::S8;
    /// The legend swatches and the state dots.
    pub const SWATCH: Pixels = space::S8;
    /// The memory bar of a member row.
    pub const MEMBER_BAR: Pixels = space::S6;
    pub const MEMBER_BAR_WIDTH: Pixels = px(96.);
    /// Table columns of 按成员.
    pub const COL_COUNT: Pixels = px(80.);
    pub const COL_CPU: Pixels = px(96.);
    pub const COL_MEM: Pixels = px(200.);
    pub const COL_TOKENS: Pixels = px(120.);
    pub const COL_DONE: Pixels = px(104.);
    /// The 要注意 card's share of the row next to 容量.
    pub const ATTENTION_SHARE: f32 = 0.41;
}

// ---------------------------------------------------------------------------
// Text styles

/// The text styles of §1.4 for any styled element, under the older native
/// names. Weights are 400, 500 and 600 (700 only for the hero title).
pub trait NativeType: Styled + Sized {
    fn n_style(self, role: (Pixels, Pixels)) -> Self {
        self.font_family(FONT_FAMILY)
            .text_size(role.0)
            .line_height(role.1)
    }
    /// 24 / 32, 600: the member and 产出 detail name.
    fn n_large_title(self) -> Self {
        self.n_style(text::HERO).font_weight(FontWeight::SEMIBOLD)
    }
    /// 18 / 28, 600: sheet and alert titles.
    fn n_title2(self) -> Self {
        self.n_style(text::TITLE).font_weight(FontWeight::SEMIBOLD)
    }
    /// 16 / 24, 500: the toolbar and pane title.
    fn n_title3(self) -> Self {
        self.n_style(text::HEADING).font_weight(MEDIUM_WEIGHT)
    }
    /// 14 / 20, 500: sender names, list titles, field labels.
    fn n_headline(self) -> Self {
        self.n_style(text::UI).font_weight(MEDIUM_WEIGHT)
    }
    /// 14 / 20: all interface text.
    fn n_body(self) -> Self {
        self.n_style(text::UI).font_weight(FontWeight::NORMAL)
    }
    /// 14 / 22: message and card body text.
    fn n_message(self) -> Self {
        self.n_style(text::PROSE).font_weight(FontWeight::NORMAL)
    }
    /// 12 / 16: captions, card facts.
    fn n_callout(self) -> Self {
        self.n_style(text::META).font_weight(FontWeight::NORMAL)
    }
    /// 12 / 16: subtitles, times, meta.
    fn n_subheadline(self) -> Self {
        self.n_style(text::META).font_weight(FontWeight::NORMAL)
    }
    /// 12 / 16, 500: group labels.
    fn n_section(self) -> Self {
        self.n_style(text::META).font_weight(MEDIUM_WEIGHT)
    }
    /// 12 / 16: the smallest interface text.
    fn n_footnote(self) -> Self {
        self.n_style(text::META).font_weight(FontWeight::NORMAL)
    }
    /// Tabular figures in the system font.
    fn n_figures(self) -> Self {
        self.font_features(tabular())
    }
}

impl<T: Styled> NativeType for T {}

// ---------------------------------------------------------------------------
// The view literal ratchet (§1.1, G2, G3)

/// The `px(` literals in `src/views` when this ratchet was set. The count
/// can only go down: lower this number when a change removes literals.
pub const VIEWS_PX_LITERALS: usize = 1431;

/// `.rounded(px(`, `.text_size(px(`, `.gap(px(` and `.h(px(` literals in
/// `src/views` when the ratchet was set. Target 0.
pub const VIEWS_SCALE_LITERALS: usize = 0;

#[cfg(test)]
mod tests {
    use super::*;

    fn rel(h: Hsla, over: Hsla) -> f32 {
        let (a, b) = (h.to_rgb(), over.to_rgb());
        let mix = |x: f32, y: f32| x * a.a + y * (1. - a.a);
        let lin = |v: f32| {
            if v <= 0.03928 {
                v / 12.92
            } else {
                ((v + 0.055) / 1.055).powf(2.4)
            }
        };
        0.2126 * lin(mix(a.r, b.r)) + 0.7152 * lin(mix(a.g, b.g)) + 0.0722 * lin(mix(a.b, b.b))
    }

    fn contrast(fg: Hsla, bg: Hsla) -> f32 {
        let (l1, l2) = (rel(fg, bg), rel(bg, bg));
        (l1.max(l2) + 0.05) / (l1.min(l2) + 0.05)
    }

    #[test]
    fn the_palette_is_the_neutral_table() {
        let (l, d) = (Palette::light(), Palette::dark());
        assert_eq!(l.foreground, c(0x0A0A0AFF));
        assert_eq!(l.muted_foreground, c(0x737373FF));
        assert_eq!(d.muted_foreground, c(0xA1A1A1FF));
        assert_eq!(l.primary, c(0x171717FF));
        assert_eq!(d.primary, c(0xE5E5E5FF));
        // `accent` and `muted` are one value (§1.2).
        for p in [l, d] {
            assert_eq!(p.accent, p.muted);
            assert!(contrast(p.muted_foreground, p.background) >= 4.5);
            assert!(contrast(p.muted_foreground, p.sidebar) >= 4.4);
            assert!(contrast(p.primary_foreground, p.primary) >= 12.);
            // State colors carry an icon: 3:1 is the non-text minimum.
            for s in [p.destructive, p.warning, p.success, p.info] {
                assert!(contrast(s, p.background) >= 3.);
            }
        }
    }

    #[test]
    fn dark_lines_hold_one_and_a_half_to_one() {
        // §2.22 (R1-16): every dark line measures >= 1.5:1 on the window
        // background and on the card fill; the input line is not weaker.
        let d = Palette::dark();
        for surface in [d.background, d.card, d.sidebar] {
            assert!(contrast(d.border, surface) >= 1.5);
            assert!(contrast(d.input, surface) >= contrast(d.border, surface));
        }
    }

    #[test]
    fn dark_controls_rise_from_their_surface() {
        // R1-17: an outline control on a GroupBox (#171717) is lighter than
        // the box, not the #0a0a0a window background.
        let d = NativeColors::dark();
        let p = Palette::dark();
        assert!(d.control.l > p.card.l && d.control.a == 1.);
        assert_eq!(NativeColors::light().control, Palette::light().background);
    }

    #[test]
    fn the_destructive_fill_carries_white_text() {
        // T1-15: one destructive fill, 4.5:1 with its white label.
        for k in [NativeColors::light(), NativeColors::dark()] {
            assert_eq!(k.destructive_fill, c(DESTRUCTIVE_FILL));
            assert!(contrast(gpui::white(), k.destructive_fill) >= 4.5);
        }
    }

    #[test]
    fn native_colors_hold_no_value_of_their_own() {
        for p in [Palette::light(), Palette::dark()] {
            let k = NativeColors::from_palette(&p);
            assert_eq!(k.secondary, k.tertiary);
            assert_eq!(k.separator, p.border);
            assert_eq!(k.primary, p.primary);
            // T1-01, D10: the highlight is `muted`, never `primary`.
            assert_eq!(k.accent, p.muted);
            assert_eq!(k.on_accent, p.foreground);
            assert_eq!(k.selection, p.muted);
            assert_eq!(k.menu.a, 1.);
        }
    }

    #[test]
    fn dark_switch_and_avatar_parity() {
        // §2.22: ON knob on the ON track, and the OFF knob on the OFF track.
        for p in [Palette::light(), Palette::dark()] {
            let k = NativeColors::from_palette(&p);
            assert!(contrast(k.knob_on, k.primary) >= 3.);
        }
        let d = NativeColors::dark();
        assert_eq!(d.knob_on, c(0x171717FF));
        assert_eq!(d.primary, c(0xE5E5E5FF));
        assert!(contrast(d.knob, Palette::dark().background) >= 3.);
    }

    #[test]
    fn the_scales_are_closed() {
        assert_eq!(text::BODY.0, px(14.));
        assert_eq!(text::SUBHEADLINE.0, px(12.));
        for role in [
            text::LARGE_TITLE,
            text::TITLE2,
            text::TITLE3,
            text::HEADLINE,
            text::BODY,
            text::MESSAGE,
            text::CALLOUT,
            text::SUBHEADLINE,
            text::SECTION,
            text::FOOTNOTE,
            type_scale::DISPLAY,
            type_scale::HEADING,
            type_scale::PAGE_TITLE,
            type_scale::TITLE,
            type_scale::CARD_TITLE,
            type_scale::BODY,
            type_scale::UI,
            type_scale::SMALL,
            type_scale::META,
            type_scale::CODE_ENTRY,
            type_scale::BUTTON_LG,
            card_v2::TITLE,
        ] {
            assert!(
                text::SIZES.contains(&role.0),
                "{:?} is off the type scale",
                role.0
            );
        }
        for h in [
            metrics::BUTTON_REGULAR,
            metrics::BUTTON_SMALL,
            metrics::BUTTON_LARGE,
            metrics::FIELD,
            metrics::SEARCH,
            metrics::SEARCH_SIDEBAR,
            metrics::SEGMENTED,
            metrics::SQUARE_BUTTON,
            metrics::TOOLBAR_ITEM,
            size::BUTTON_SM,
            size::BUTTON_CARD,
            size::BUTTON_MD,
            size::BUTTON_LG,
            size::TEXT_BUTTON,
            size::ICON_BUTTON,
            size::ICON_BUTTON_SM,
            size::SEARCH_FIELD,
            size::FORM_FIELD,
            size::SELECT,
        ] {
            assert!(
                height::CONTROLS.contains(&h),
                "{h:?} is not a control height"
            );
        }
        for r in [
            metrics::BUTTON_REGULAR_RADIUS,
            metrics::FIELD_RADIUS,
            metrics::MENU_RADIUS,
            metrics::MENU_ROW_RADIUS,
            metrics::GROUP_RADIUS,
            metrics::LIST_ROW_RADIUS,
            radius::MARK,
            radius::SM,
            radius::MD,
            radius::FIELD,
            radius::LG,
            radius::BUTTON_CARD,
            card_v2::OPTION_RADIUS,
            card_v2::CHIP_RADIUS,
        ] {
            assert!(radius::SCALE.contains(&r), "{r:?} is off the radius scale");
        }
        for s in [
            space::S3,
            space::S10,
            space::S14,
            space::S20,
            space::S28,
            space::S44,
            metrics::GROUP_GAP,
            metrics::GROUP_MARGIN,
            metrics::CONTENT_LEADING,
        ] {
            assert!(space::SCALE.contains(&s), "{s:?} is off the spacing scale");
        }
        for a in [
            size::AVATAR_MESSAGE,
            size::AVATAR_START,
            layout::AVATAR_SIZE,
        ] {
            assert!(avatar::SIZES.contains(&a));
        }
    }

    #[test]
    fn content_starts_at_the_gutter() {
        // The page column starts at the sidebar's trailing edge (x 264):
        // content at 264 + 24 = 288.
        assert_eq!(
            metrics::SIDEBAR_WIDTH + metrics::SIDEBAR_INSET * 2. + metrics::CONTENT_LEADING,
            px(288.)
        );
        assert_eq!(metrics::CONTENT_LEADING, metrics::CONTENT_GAP);
        assert_eq!(metrics::TOOLBAR_LEADING, metrics::CONTENT_LEADING);
        assert_eq!(metrics::PANE_TOP, metrics::TOOLBAR_ROW);
        assert_eq!(metrics::INSPECTOR_WIDTH, px(320.));
        // Owner decision (v14): 320 for inspector and side chat, 400 for the
        // live and 产出 panes; one in-thread width of 640.
        assert_eq!(layout::SIDE_CHAT_WIDTH, px(320.));
        assert_eq!(metrics::WIDE_PANE_WIDTH, px(400.));
        assert_eq!(layout::ARTIFACT_PANE_WIDTH, px(400.));
        assert_eq!(layout::IN_THREAD, px(640.));
        assert_eq!(layout::in_thread_width(px(500.)), px(500.));
        assert_eq!(layout::in_thread_width(px(808.)), px(640.));
    }

    #[test]
    fn shadows_are_the_four_levels() {
        assert_eq!(shadow::xs().len(), 1);
        assert_eq!(shadow::sm().len(), 2);
        assert_eq!(shadow::md().len(), 2);
        assert_eq!(shadow::lg().len(), 2);
        // Floating surfaces keep their 1 px line in both modes (§1.7).
        for k in [NativeColors::light(), NativeColors::dark()] {
            assert!(k.float().iter().any(|s| s.color == k.separator));
        }
    }

    /// The literal ratchet (§1.1): `px(` literals in `src/views` only go
    /// down.
    #[test]
    fn view_literals_only_go_down() {
        fn walk(dir: &std::path::Path, out: &mut Vec<String>) {
            for entry in std::fs::read_dir(dir).expect("read views") {
                let path = entry.expect("entry").path();
                if path.is_dir() {
                    walk(&path, out);
                } else if path.extension().is_some_and(|e| e == "rs") {
                    out.push(std::fs::read_to_string(&path).expect("read view"));
                }
            }
        }
        let mut files = Vec::new();
        walk(
            &std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/views"),
            &mut files,
        );
        let count =
            |needle: &str| -> usize { files.iter().map(|f| f.matches(needle).count()).sum() };
        let all = count("px(");
        assert!(
            all <= VIEWS_PX_LITERALS,
            "src/views has {all} `px(` literals, more than the ratchet {VIEWS_PX_LITERALS}: \
             use a token from ui/native/tokens.rs"
        );
        let scale = [".rounded(px(", ".text_size(px(", ".gap(px(", ".h(px("]
            .iter()
            .map(|n| count(n))
            .sum::<usize>();
        assert!(
            VIEWS_SCALE_LITERALS <= scale && scale <= VIEWS_SCALE_BASELINE,
            "src/views has {scale} rounded/text_size/gap/h `px(` literals, more than \
             {VIEWS_SCALE_BASELINE}"
        );
    }
}

/// The current count of `.rounded(px(`, `.text_size(px(`, `.gap(px(` and
/// `.h(px(` in `src/views`: the ratchet on the way to
/// [`VIEWS_SCALE_LITERALS`] (0).
pub const VIEWS_SCALE_BASELINE: usize = 193;
