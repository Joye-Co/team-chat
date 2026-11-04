//! Native tokens (docs/design/NATIVE.md §3, §4): the macOS semantic colors
//! in light and dark, the text styles with CJK line heights, the control
//! metrics of §5 and the one floating shadow.
//!
//! Views read colors with [`native_colors`] and sizes from [`metrics`].
//! Never write a color or size literal in a view.

use gpui::{App, BoxShadow, FontWeight, Hsla, Pixels, Styled, Window, point, px, rgba};

use crate::theme::{self, FONT_FAMILY, tabular};

// ---------------------------------------------------------------------------
// Colors (§4)

/// The macOS semantic colors of one mode (§4). Field names follow the token
/// names of NATIVE.md with `_` for camel case.
#[derive(Clone, Copy, Debug)]
pub struct NativeColors {
    /// `textBackgroundColor`: the content area.
    pub content: Hsla,
    /// The settings window background.
    pub window: Hsla,
    /// Sheets, alerts, popovers and menus.
    pub sheet: Hsla,
    /// Menus: opaque `sheet` (a translucent fill without a real blur lets
    /// the rows behind show through).
    pub menu: Hsla,
    /// Sidebar and inspector glass.
    pub sidebar: Hsla,
    /// Grouped form section fill: quiet gray, never a white card.
    pub group: Hsla,
    pub label: Hsla,
    pub secondary: Hsla,
    pub tertiary: Hsla,
    pub quaternary: Hsla,
    pub separator: Hsla,
    /// Control fill (push button, segmented track, search field).
    pub fill: Hsla,
    /// Borderless and toolbar button hover; push button pressed.
    pub fill_hover: Hsla,
    /// Sidebar selection and unfocused list selection.
    pub selection: Hsla,
    /// The app accent: graphite `#6E6E73` while the system accent is
    /// multicolor, else `NSColor.controlAccentColor` (see `native_colors`).
    pub accent: Hsla,
    /// The accent as text or a symbol on a gray selection (§9.1): the
    /// system accent when one is picked, else `label` (graphite text on a
    /// gray selection does not read).
    pub accent_text: Hsla,
    /// Ink: the strongest text color, `#1D1D1F` / `#F5F5F7` (body text of
    /// the older tokens, never a fill).
    pub ink: Hsla,
    /// Text and symbols on `accent`.
    pub on_accent: Hsla,
    /// The 3 pt focus ring: accent at 50 %.
    pub focus: Hsla,
    /// The 0.5 pt inner edge of floating glass.
    pub glass_edge: Hsla,
    /// The toolbar capsule fill.
    pub capsule: Hsla,
    /// The window dim behind a sheet.
    pub dim: Hsla,
    /// The switch knob.
    pub knob: Hsla,
    /// The raised selected segment (§5.5). NATIVE.md says `group`, but 4–6 %
    /// on the `fill` track does not read as raised; Tahoe draws it lighter.
    pub segment: Hsla,
    pub amber: Hsla,
    pub blue: Hsla,
    pub red: Hsla,
    pub green: Hsla,
    dark: bool,
}

/// The app accent while the system accent is multicolor: graphite, in
/// light and dark (coordinator decision, NATIVE.md §4). White text on it
/// reads at 5.1:1.
pub const GRAPHITE: u32 = 0x6E6E73FF;
/// Graphite at 35 %: the focus halo.
const GRAPHITE_HALO: u32 = 0x6E6E7359;

fn c(hex: u32) -> Hsla {
    rgba(hex).into()
}

impl NativeColors {
    pub fn light() -> Self {
        Self {
            content: c(0xFFFFFFFF),
            window: c(0xFFFFFFFF),
            sheet: c(0xFFFFFFFF),
            menu: c(0xFFFFFFFF),
            sidebar: c(0xF7F7F8FF),
            group: c(0x0000000A),
            label: c(0x000000D9),
            secondary: c(0x00000080),
            tertiary: c(0x00000042),
            quaternary: c(0x0000001A),
            separator: c(0x0000001A),
            fill: c(0x0000000D),
            fill_hover: c(0x00000014),
            selection: c(0x0000000F),
            accent: c(GRAPHITE),
            accent_text: c(0x000000D9),
            ink: c(0x1D1D1FFF),
            on_accent: c(0xFFFFFFFF),
            focus: c(GRAPHITE_HALO),
            glass_edge: c(0xFFFFFFB3),
            capsule: c(0xFFFFFFCC),
            dim: c(0x00000033),
            knob: c(0xFFFFFFFF),
            segment: c(0xFFFFFFFF),
            amber: c(0xFF9500FF),
            blue: c(0x007AFFFF),
            red: c(0xFF3B30FF),
            green: c(0x34C759FF),
            dark: false,
        }
    }

    pub fn dark() -> Self {
        Self {
            content: c(0x1E1E1EFF),
            window: c(0x1E1E1EFF),
            sheet: c(0x2C2C2EFF),
            menu: c(0x2C2C2EFF),
            sidebar: c(0x2A2A2CFF),
            group: c(0xFFFFFF0F),
            label: c(0xFFFFFFD9),
            secondary: c(0xFFFFFF8C),
            tertiary: c(0xFFFFFF40),
            quaternary: c(0xFFFFFF1A),
            separator: c(0xFFFFFF1A),
            fill: c(0xFFFFFF14),
            fill_hover: c(0xFFFFFF1F),
            selection: c(0xFFFFFF1A),
            accent: c(GRAPHITE),
            accent_text: c(0xFFFFFFD9),
            ink: c(0xF5F5F7FF),
            on_accent: c(0xFFFFFFFF),
            focus: c(GRAPHITE_HALO),
            glass_edge: c(0xFFFFFF1F),
            capsule: c(0xFFFFFF14),
            dim: c(0x00000066),
            knob: c(0xFFFFFFFF),
            segment: c(0xFFFFFF33),
            amber: c(0xFF9F0AFF),
            blue: c(0x0A84FFFF),
            red: c(0xFF453AFF),
            green: c(0x30D158FF),
            dark: true,
        }
    }

    pub fn for_mode(dark: bool) -> Self {
        if dark { Self::dark() } else { Self::light() }
    }

    pub fn is_dark(&self) -> bool {
        self.dark
    }

    /// `float` (§4): the shadow of floating things only (sidebar panel,
    /// toolbar groups, menus, popovers, sheets). GPUI blurs by the standard
    /// deviation, CSS by twice it, so the CSS blur is halved.
    pub fn float(&self) -> Vec<BoxShadow> {
        let drop = |a: u32| BoxShadow {
            color: c(a),
            offset: point(px(0.), px(8.)),
            blur_radius: px(12.),
            spread_radius: px(0.),
            inset: false,
        };
        if self.dark {
            vec![drop(0x00000073)]
        } else {
            vec![
                drop(0x0000001F),
                BoxShadow {
                    color: c(0x0000000F),
                    offset: point(px(0.), px(0.)),
                    blur_radius: px(0.),
                    spread_radius: metrics::HAIRLINE,
                    inset: false,
                },
            ]
        }
    }

    /// `float` plus the 0.5 pt `glassEdge` inside the edge.
    pub fn glass(&self) -> Vec<BoxShadow> {
        let mut s = self.float();
        s.push(self.inner_edge(self.glass_edge));
        s
    }

    /// A 0.5 pt line inside an element's edge, drawn as an inset shadow so
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

    /// The raised segment of a segmented control (§5.5): 0 1 2 at 12 %.
    pub fn segment(&self) -> Vec<BoxShadow> {
        vec![BoxShadow {
            color: c(0x0000001F),
            offset: point(px(0.), px(1.)),
            blur_radius: px(1.),
            spread_radius: px(0.),
            inset: false,
        }]
    }

    /// The switch knob shadow: a soft drop and a 0.5 pt black edge, so the
    /// white knob shows on a light track in dark mode (§5.4). Checkbox and
    /// radio marks use the edge alone.
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

    /// The 3 pt focus ring outside a control (§4 `focus`).
    pub fn focus_ring(&self) -> Vec<BoxShadow> {
        vec![BoxShadow {
            color: self.focus,
            offset: point(px(0.), px(0.)),
            blur_radius: px(0.),
            spread_radius: metrics::FOCUS_RING,
            inset: false,
        }]
    }

    /// The (symbol, text) colors of a selected row (§9.1 Tahoe): `accent`
    /// in the key window, `secondary`/`label` when the window is inactive.
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
    // The system accent (or graphite) and the 35 % focus halo (§4).
    let dark = theme::is_dark(cx);
    let mut k = NativeColors::for_mode(dark);
    k.accent = theme::app_accent(dark);
    k.on_accent = theme::accent_ink(dark);
    k.accent_text = theme::system_accent().unwrap_or(k.label);
    k.focus = k.accent.opacity(theme::FOCUS_HALO);
    k
}

/// Whether `window` draws its selection with emphasis (§4 Inactive window):
/// key windows only, or every window in the screenshot self-test.
pub fn emphasized(window: &Window) -> bool {
    crate::ui::rows::selection_emphasized(window)
}

// ---------------------------------------------------------------------------
// Metrics (§2, §5, §7, §8)

/// The visible sizes of native controls and surfaces.
pub mod metrics {
    use gpui::{Pixels, px};

    pub const HAIRLINE: Pixels = px(0.5);
    pub const FOCUS_RING: Pixels = px(3.);

    // Window (§2)
    pub const TOOLBAR_ROW: Pixels = px(52.);
    pub const TOOLBAR_GROUP: Pixels = px(36.);
    pub const TOOLBAR_ITEM: Pixels = px(36.);
    pub const TOOLBAR_PILL: Pixels = px(28.);
    pub const TOOLBAR_PILL_INSET: Pixels = px(4.);
    pub const TOOLBAR_GROUP_GAP: Pixels = px(8.);
    /// The gap between the visible sidebar edge and the content's leading
    /// edge (§2): the toolbar title, lists, transcript avatars and detail
    /// titles all start 20 pt after the sidebar.
    pub const CONTENT_GAP: Pixels = px(20.);
    /// The one leading content inset, measured from the page column's
    /// leading edge. The page column starts in the sidebar's 8 pt gap, so
    /// 12 here is 20 after the visible sidebar edge. Every page, list and
    /// detail starts its content here; with the sidebar hidden the shell
    /// shifts the page so the content still sits 20 pt from the window
    /// edge.
    pub const CONTENT_LEADING: Pixels = px(12.);
    /// Where a toolbar title starts: the content inset.
    pub const TOOLBAR_LEADING: Pixels = CONTENT_LEADING;
    pub const TOOLBAR_TRAILING: Pixels = px(12.);
    pub const TOOLBAR_SYMBOL: Pixels = px(15.);
    pub const SIDEBAR_INSET: Pixels = px(8.);
    pub const SIDEBAR_RADIUS: Pixels = px(18.);
    pub const SIDEBAR_WIDTH: Pixels = px(248.);
    pub const SIDEBAR_ROW: Pixels = px(32.);
    pub const SIDEBAR_ROW_INSET: Pixels = px(10.);
    pub const SIDEBAR_ROW_RADIUS: Pixels = px(7.);
    pub const SIDEBAR_SLOT: Pixels = px(18.);
    pub const SIDEBAR_SYMBOL: Pixels = px(15.);
    pub const SIDEBAR_SECTION: Pixels = px(28.);
    /// The inspector (topic rail, 改动) and every narrow trailing pane.
    pub const INSPECTOR_WIDTH: Pixels = px(280.);
    /// The wider trailing panes: 单独问, the 产出 pane and 实时查看.
    pub const WIDE_PANE_WIDTH: Pixels = px(400.);
    /// Where a trailing pane's content starts: below the toolbar row, on
    /// every screen.
    pub const PANE_TOP: Pixels = TOOLBAR_ROW;

    // Push buttons (§5.2)
    pub const BUTTON_REGULAR: Pixels = px(24.);
    pub const BUTTON_REGULAR_RADIUS: Pixels = px(6.);
    pub const BUTTON_REGULAR_PAD: Pixels = px(12.);
    pub const BUTTON_MIN_WIDTH: Pixels = px(64.);
    pub const BUTTON_SMALL: Pixels = px(20.);
    pub const BUTTON_SMALL_RADIUS: Pixels = px(5.);
    pub const BUTTON_SMALL_PAD: Pixels = px(10.);
    pub const BUTTON_LARGE: Pixels = px(28.);
    pub const BUTTON_LARGE_PAD: Pixels = px(16.);
    pub const SQUARE_BUTTON: Pixels = px(22.);
    pub const POPUP_CHEVRON: Pixels = px(9.);
    pub const POPUP_CHEVRON_SLOT: Pixels = px(12.);

    // Switches, checkboxes, radios (§5.4)
    pub const SWITCH_SMALL: (Pixels, Pixels) = (px(32.), px(18.));
    pub const SWITCH_REGULAR: (Pixels, Pixels) = (px(40.), px(24.));
    pub const SWITCH_INSET: Pixels = px(1.);
    pub const CHECKBOX: Pixels = px(16.);
    pub const CHECKBOX_RADIUS: Pixels = px(4.);
    pub const CHECKMARK: Pixels = px(11.);
    pub const RADIO_DOT: Pixels = px(6.);

    // Segmented (§5.5)
    pub const SEGMENTED: Pixels = px(24.);
    pub const SEGMENTED_TOOLBAR: Pixels = px(32.);
    pub const SEGMENT_INSET: Pixels = px(2.);
    pub const SEGMENT_PAD: Pixels = px(12.);

    // Text entry (§5.6)
    pub const FIELD: Pixels = px(24.);
    pub const FIELD_LARGE: Pixels = px(28.);
    pub const FIELD_RADIUS: Pixels = px(6.);
    pub const FIELD_PAD: Pixels = px(7.);
    pub const FIELD_NAME_WIDTH: Pixels = px(220.);
    pub const FIELD_CODE_WIDTH: Pixels = px(140.);
    pub const SEARCH_SIDEBAR: Pixels = px(30.);
    pub const SEARCH: Pixels = px(24.);
    pub const SEARCH_SYMBOL: Pixels = px(13.);

    // Progress (§5.7)
    pub const BAR: Pixels = px(4.);
    pub const BAR_RADIUS: Pixels = px(2.);
    pub const SPINNER: Pixels = px(16.);

    // Grouped forms (§8)
    pub const GROUP_RADIUS: Pixels = px(10.);
    pub const GROUP_MARGIN: Pixels = px(20.);
    pub const FORM_ROW: Pixels = px(36.);
    pub const FORM_ROW_INSET: Pixels = px(12.);
    pub const GROUP_GAP: Pixels = px(20.);
    pub const GROUP_HEADER_GAP: Pixels = px(6.);
    pub const INLINE_FIELD_WIDTH: Pixels = px(220.);

    // Inspector and lists
    pub const INSPECTOR_ROW: Pixels = px(24.);
    pub const INSPECTOR_PAD: Pixels = px(16.);
    /// The label column of a label/value row (fits five CJK characters).
    pub const LABEL_COLUMN: Pixels = px(72.);
    /// The least space between a label and its value.
    pub const LABEL_GAP: Pixels = px(8.);
    pub const LIST_ROW: Pixels = px(28.);
    pub const LIST_ROW_RADIUS: Pixels = px(6.);
    pub const LIST_ROW_INSET: Pixels = px(8.);

    // Overlays (§7)
    pub const SHEET_WIDTH: Pixels = px(460.);
    pub const SHEET_WIDE: Pixels = px(560.);
    pub const SHEET_RADIUS: Pixels = px(22.);
    pub const SHEET_PAD: Pixels = px(24.);
    pub const SHEET_BUTTON_EDGE: Pixels = px(20.);
    pub const SHEET_BUTTON_GAP: Pixels = px(12.);
    pub const ALERT_WIDTH: Pixels = px(260.);
    pub const ALERT_ICON: Pixels = px(48.);
    pub const ALERT_PAD: Pixels = px(16.);
    pub const MENU_RADIUS: Pixels = px(12.);
    pub const MENU_PAD: Pixels = px(5.);
    pub const MENU_ROW: Pixels = px(24.);
    pub const MENU_ROW_RADIUS: Pixels = px(6.);
    pub const MENU_SYMBOL: Pixels = px(13.);
    pub const MENU_ROW_PAD: Pixels = px(9.);
    pub const POPOVER_RADIUS: Pixels = px(12.);
    pub const POPOVER_ARROW: Pixels = px(10.);
}

// ---------------------------------------------------------------------------
// Typography (§3)

/// The macOS text styles of §3: (size, line height). CJK line heights.
pub mod text {
    use gpui::{Pixels, px};
    pub const LARGE_TITLE: (Pixels, Pixels) = (px(26.), px(32.));
    pub const TITLE2: (Pixels, Pixels) = (px(17.), px(22.));
    pub const TITLE3: (Pixels, Pixels) = (px(15.), px(20.));
    pub const HEADLINE: (Pixels, Pixels) = (px(13.), px(18.));
    pub const BODY: (Pixels, Pixels) = (px(13.), px(18.));
    pub const MESSAGE: (Pixels, Pixels) = (px(14.), px(21.));
    pub const CALLOUT: (Pixels, Pixels) = (px(12.), px(16.));
    pub const SUBHEADLINE: (Pixels, Pixels) = (px(11.), px(14.));
    pub const SECTION: (Pixels, Pixels) = (px(11.), px(14.));
    pub const FOOTNOTE: (Pixels, Pixels) = (px(10.), px(13.));
}

/// The §3 text styles for any styled element. Never bolder than Semibold
/// except Large title.
pub trait NativeType: Styled + Sized {
    fn n_style(self, role: (Pixels, Pixels)) -> Self {
        self.font_family(FONT_FAMILY)
            .text_size(role.0)
            .line_height(role.1)
    }
    /// 26 / 32 Bold: only the member and 产出 detail name.
    fn n_large_title(self) -> Self {
        self.n_style(text::LARGE_TITLE)
            .font_weight(FontWeight::BOLD)
    }
    /// 17 / 22 Semibold: sheet and alert titles.
    fn n_title2(self) -> Self {
        self.n_style(text::TITLE2).font_weight(FontWeight::SEMIBOLD)
    }
    /// 15 / 20 Semibold: the toolbar title.
    fn n_title3(self) -> Self {
        self.n_style(text::TITLE3).font_weight(FontWeight::SEMIBOLD)
    }
    /// 13 / 18 Semibold: sender names, row titles with weight.
    fn n_headline(self) -> Self {
        self.n_style(text::HEADLINE)
            .font_weight(FontWeight::SEMIBOLD)
    }
    /// 13 / 18: all interface text.
    fn n_body(self) -> Self {
        self.n_style(text::BODY).font_weight(FontWeight::NORMAL)
    }
    /// 14 / 21: message and card body text only.
    fn n_message(self) -> Self {
        self.n_style(text::MESSAGE).font_weight(FontWeight::NORMAL)
    }
    /// 12 / 16: card facts, captions.
    fn n_callout(self) -> Self {
        self.n_style(text::CALLOUT).font_weight(FontWeight::NORMAL)
    }
    /// 11 / 14: subtitles, times, secondary lines, group footers.
    fn n_subheadline(self) -> Self {
        self.n_style(text::SUBHEADLINE)
            .font_weight(FontWeight::NORMAL)
    }
    /// 11 / 14 Semibold: sidebar and inspector section headers.
    fn n_section(self) -> Self {
        self.n_style(text::SECTION)
            .font_weight(FontWeight::SEMIBOLD)
    }
    /// 10 / 13: the composer hint and dense table metadata only.
    fn n_footnote(self) -> Self {
        self.n_style(text::FOOTNOTE).font_weight(FontWeight::NORMAL)
    }
    /// Tabular figures in the system font.
    fn n_figures(self) -> Self {
        self.font_features(tabular())
    }
}

impl<T: Styled> NativeType for T {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn modes_differ_and_states_are_system_colors() {
        let (l, d) = (NativeColors::light(), NativeColors::dark());
        assert!(!l.is_dark() && d.is_dark());
        assert_ne!(l.content, d.content);
        assert_eq!(l.amber, c(0xFF9500FF));
        assert_eq!(d.green, c(0x30D158FF));
        // Only floating things cast a shadow; light adds a hairline ring.
        assert_eq!(l.float().len(), 2);
        assert_eq!(d.float().len(), 1);
    }

    #[test]
    fn interface_text_is_13_pt() {
        assert_eq!(text::BODY.0, px(13.));
        assert_eq!(text::SUBHEADLINE.0, px(11.));
        assert_eq!(metrics::BUTTON_REGULAR, px(24.));
        assert_eq!(metrics::SIDEBAR_ROW, px(32.));
    }

    #[test]
    fn content_starts_20_pt_after_the_visible_sidebar() {
        assert_eq!(
            metrics::SIDEBAR_INSET + metrics::CONTENT_LEADING,
            metrics::CONTENT_GAP
        );
        assert_eq!(metrics::CONTENT_GAP, px(20.));
        assert_eq!(metrics::TOOLBAR_LEADING, metrics::CONTENT_LEADING);
        assert_eq!(metrics::PANE_TOP, metrics::TOOLBAR_ROW);
    }

    #[test]
    fn the_accent_is_graphite_with_white_text_and_menus_are_opaque() {
        for k in [NativeColors::light(), NativeColors::dark()] {
            assert_eq!(k.accent, c(GRAPHITE));
            assert_eq!(k.on_accent, c(0xFFFFFFFF));
            assert_eq!(k.menu.a, 1.);
            assert!((k.focus.a - 0.35).abs() < 0.01);
            // The knob has a hairline edge, so it shows on any track.
            assert_eq!(k.knob_shadow().len(), 2);
        }
    }
}
