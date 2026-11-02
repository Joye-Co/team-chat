//! The one theme module (docs/DESIGN.md §2 to §10). It follows the system
//! light or dark mode, loads our values into gpui-component's theme, and gives
//! views the app's semantic tokens: colors, type, spacing, radii, elevation
//! and motion.
//!
//! Views read colors with `theme::colors(cx)`, sizes from the constants below
//! and text styles through [`Type`]. Never write a color, size or duration
//! literal in a view: add a token here so light and dark both work.

use std::{
    sync::{Arc, OnceLock},
    time::Duration,
};

use gpui::{
    App, BoxShadow, FontFeatures, FontWeight, Hsla, Pixels, Styled, Subscription, Window, point,
    px, rgba,
};
use gpui_component::{ActiveTheme, Theme, ThemeMode, scroll::ScrollbarMode};
use gpui_kit::base::Easing;

use crate::data::TopicStatus;

// ---------------------------------------------------------------------------
// Layout (§5)

/// Sidebar column width (§5.1).
pub const SIDEBAR_WIDTH: Pixels = px(236.);
pub const TOPIC_LIST_WIDTH: Pixels = px(300.);
pub const SIDE_PANEL_WIDTH: Pixels = px(280.);
/// The topic view never gets narrower than this.
pub const TOPIC_VIEW_MIN_WIDTH: Pixels = px(480.);
/// Toolbar band at the top of each column; the traffic lights sit in it.
pub const TOOLBAR_HEIGHT: Pixels = px(52.);
/// Below this window width the side panel is hidden: the four columns need
/// 236 + 300 + 480 + 280 pt (DESIGN.md §5.1 names 1100 pt, where they would clip).
pub const SIDE_PANEL_MIN_WINDOW: Pixels = px(1300.);
pub const WINDOW_MIN_WIDTH: Pixels = px(1100.);
pub const WINDOW_MIN_HEIGHT: Pixels = px(680.);
pub const WINDOW_DEFAULT_WIDTH: Pixels = px(1440.);
pub const WINDOW_DEFAULT_HEIGHT: Pixels = px(900.);
/// Traffic light origin inside the 52 pt toolbar band.
pub const TRAFFIC_LIGHTS: (Pixels, Pixels) = (px(20.), px(18.));
/// Timeline avatar size.
pub const AVATAR_SIZE: Pixels = px(32.);
/// Sidebar 群 row height.
pub const ROW_HEIGHT: Pixels = px(28.);
/// Maximum width of message text (about 40 Han characters).
pub const MEASURE: Pixels = px(560.);
/// Hairline width. 0.5 pt is exact on Retina; on 1× it becomes 1 px.
pub const HAIRLINE: Pixels = px(0.5);

/// Spacing scale (§5): 2, 4, 6, 8, 10, 12, 14, 16, 20, 24, 28, 32. Nothing else.
pub mod space {
    use gpui::{Pixels, px};
    pub const S2: Pixels = px(2.);
    pub const S4: Pixels = px(4.);
    pub const S6: Pixels = px(6.);
    pub const S8: Pixels = px(8.);
    pub const S10: Pixels = px(10.);
    pub const S12: Pixels = px(12.);
    pub const S14: Pixels = px(14.);
    pub const S16: Pixels = px(16.);
    pub const S20: Pixels = px(20.);
    pub const S24: Pixels = px(24.);
    pub const S28: Pixels = px(28.);
    pub const S32: Pixels = px(32.);
}

/// Radii (§6). Nested radii: inner = outer − padding.
pub mod radius {
    use gpui::{Pixels, px};
    pub const XS: Pixels = px(4.);
    pub const CHIP: Pixels = px(5.);
    pub const TILE: Pixels = px(5.5);
    pub const SM: Pixels = px(6.);
    pub const MD: Pixels = px(7.);
    pub const ROW: Pixels = px(8.);
    pub const LG: Pixels = px(10.);
    pub const COMPOSER: Pixels = px(12.);
    pub const XL: Pixels = px(14.);
    /// Bot avatars: 28 % of their size.
    pub fn bot(size: Pixels) -> Pixels {
        size * 0.28
    }
}

/// Icon sizes (§9).
pub mod icon {
    use gpui::{Pixels, px};
    pub const TOOLBAR: Pixels = px(16.);
    pub const TODO: Pixels = px(15.);
    pub const MENU: Pixels = px(14.);
    pub const BUTTON: Pixels = px(14.);
    pub const SMALL: Pixels = px(13.);
    pub const CHIP: Pixels = px(11.);
    pub const EMPTY: Pixels = px(20.);
}

// ---------------------------------------------------------------------------
// Theme set (§3)

/// Sets the theme from the window's appearance and keeps it in step when the
/// user switches light or dark mode. Keep the returned subscription alive.
pub fn follow_system(window: &mut Window, cx: &mut App) -> Subscription {
    let appearance = window.appearance();
    set_mode(appearance.into(), cx);
    window.observe_window_appearance(|window, cx| {
        set_mode(window.appearance().into(), cx);
    })
}

/// Switches to `mode` and loads our values over gpui-component's defaults.
/// `Theme::change` reloads the registered config, so the overrides are applied
/// again after every change (DESIGN.md §18). The self-test uses this too.
pub fn set_mode(mode: ThemeMode, cx: &mut App) {
    Theme::change(mode, None, cx);
    crate::ui::motion::sync_reduce_motion(cx);
    let dark = mode.is_dark();
    Theme::update(cx, |theme| apply(theme, dark));
}

pub fn is_dark(cx: &App) -> bool {
    cx.theme().mode == ThemeMode::Dark
}

fn c(hex: u32) -> Hsla {
    rgba(hex).into()
}

/// Every gpui-component slot the app can show (DESIGN.md §3).
fn apply(t: &mut Theme, dark: bool) {
    let k = if dark { dark_colors() } else { light_colors() };
    let p = |l: u32, d: u32| if dark { c(d) } else { c(l) };

    t.background = k.content;
    t.foreground = k.text_1;
    t.muted = k.fill_field;
    t.muted_foreground = k.text_2;
    t.border = k.sep;
    t.input = k.control_border;
    t.ring = k.focus_ring;
    t.selection = k.selection;
    t.caret = k.accent;
    t.overlay = k.scrim;
    t.accent = k.fill_active;
    t.accent_foreground = k.text_1;

    t.primary = k.accent;
    t.primary_hover = k.accent_hover;
    t.primary_active = k.accent_pressed;
    t.primary_foreground = k.on_accent;
    t.button_primary = k.accent;
    t.button_primary_hover = k.accent_hover;
    t.button_primary_active = k.accent_pressed;
    t.button_primary_foreground = k.on_accent;

    let (sec, sec_h, sec_a) = (
        p(0xFFFFFFFF, 0xFFFFFF1A),
        p(0xF7F7F7FF, 0xFFFFFF24),
        p(0xE6E6E6FF, 0xFFFFFF2E),
    );
    t.secondary = sec;
    t.secondary_hover = sec_h;
    t.secondary_active = sec_a;
    t.secondary_foreground = k.text_1;
    t.button = sec;
    t.button_hover = sec_h;
    t.button_active = sec_a;
    t.button_foreground = k.text_1;
    t.button_secondary = sec;
    t.button_secondary_hover = sec_h;
    t.button_secondary_active = sec_a;
    t.button_secondary_foreground = k.text_1;

    t.danger = k.danger;
    t.danger_hover = p(0xF0322AFF, 0xFF5A50FF);
    t.danger_active = p(0xD92B22FF, 0xE03A30FF);
    t.danger_foreground = k.on_accent;
    t.button_danger = t.danger;
    t.button_danger_hover = t.danger_hover;
    t.button_danger_active = t.danger_active;
    t.button_danger_foreground = k.on_accent;
    t.success = k.success;
    t.success_hover = k.success;
    t.success_active = k.success;
    t.success_foreground = k.on_accent;
    t.warning = k.warning;
    t.warning_hover = k.warning;
    t.warning_active = k.warning;
    t.warning_foreground = k.on_accent;
    t.info = k.accent;
    t.info_hover = k.accent_hover;
    t.info_active = k.accent_pressed;
    t.info_foreground = k.on_accent;

    t.colors.list = k.content;
    t.list_hover = c(0x00000000);
    t.list_active = k.fill_selected;
    t.list_active_border = c(0x00000000);
    t.list_even = k.content;
    t.list_head = k.content;

    t.table = k.group;
    t.table_hover = k.fill_hover;
    t.table_active = k.fill_selected;
    t.table_active_border = c(0x00000000);
    t.table_even = k.group;
    t.table_row_border = k.sep;
    t.table_head = k.group;
    t.table_head_foreground = k.text_2;
    t.table_foot = k.group;
    t.table_foot_foreground = k.text_2;

    t.popover = k.elevated;
    t.popover_foreground = k.text_1;
    t.group_box = k.group;
    t.group_box_foreground = k.text_1;
    t.description_list_label = c(0x00000000);
    t.description_list_label_foreground = k.text_2;

    t.sidebar = k.sidebar;
    t.sidebar_foreground = k.text_1;
    t.sidebar_accent = k.sidebar_selected;
    t.sidebar_accent_foreground = k.text_1;
    t.sidebar_primary = k.accent;
    t.sidebar_primary_foreground = k.on_accent;
    t.sidebar_border = k.sep;

    t.title_bar = c(0x00000000);
    t.title_bar_border = c(0x00000000);
    t.status_bar = k.content;
    t.status_bar_border = k.sep;

    t.tab_bar = c(0x00000000);
    t.tab_bar_segmented = k.fill_field;
    t.tab = c(0x00000000);
    t.tab_active = p(0xFFFFFFFF, 0xFFFFFF29);
    t.tab_foreground = k.text_2;
    t.tab_active_foreground = k.text_1;

    t.switch = p(0x00000024, 0xFFFFFF29);
    t.switch_thumb = c(0xFFFFFFFF);
    t.slider_bar = k.accent;
    t.slider_thumb = c(0xFFFFFFFF);
    t.progress_bar = k.accent;
    t.skeleton = k.fill_field;

    t.scrollbar = c(0x00000000);
    t.scrollbar_thumb = k.scrollbar_thumb;
    t.scrollbar_thumb_hover = p(0x00000066, 0xFFFFFF66);

    t.link = k.accent_text;
    t.link_hover = p(0x0055B3FF, 0x6BB3FFFF);
    t.link_active = p(0x004799FF, 0x8CC4FFFF);

    t.blue = k.accent;
    t.blue_light = p(0x007AFF1A, 0x0A84FF33);
    t.yellow = k.st_you_text;
    t.yellow_light = k.st_you_bg;
    t.red = k.st_blocked_text;
    t.red_light = k.st_blocked_bg;
    t.green = k.glyph_done;
    t.green_light = p(0x34C75924, 0x30D1582B);
    t.magenta = t.blue;
    t.magenta_light = t.blue_light;
    t.cyan = t.blue;
    t.cyan_light = t.blue_light;

    t.chart_1 = p(0x007AFFFF, 0x0A84FFFF);
    t.chart_2 = p(0x34C759FF, 0x30D158FF);
    t.chart_3 = p(0xFF9500FF, 0xFF9F0AFF);
    t.chart_4 = p(0xAF52DEFF, 0xBF5AF2FF);
    t.chart_5 = p(0x8E8E93FF, 0x98989DFF);
    t.window_border = p(0x00000024, 0xFFFFFF21);
    t.drag_border = k.accent;
    t.drop_target = t.blue_light;

    t.font_family = FONT_FAMILY.into();
    t.font_size = type_scale::UI.0;
    t.mono_font_family = MONO_FONT_FAMILY.into();
    t.mono_font_size = type_scale::CODE.0;
    t.radius = radius::MD;
    t.radius_lg = radius::LG;
    t.shadow = true;
    t.focus_ring = true;
    t.scrollbar_mode = ScrollbarMode::Scrolling;

    let m = &mut t.motion;
    m.duration_fast = motion::FAST;
    m.duration_normal = motion::NORMAL;
    m.duration_slow = motion::SLOW;
    m.easing_enter = motion::out();
    m.easing_exit = motion::out();
    m.easing_move = motion::moving();
}

// ---------------------------------------------------------------------------
// Typography (§4)

/// System font (SF Pro); Core Text falls back to PingFang SC for Han.
pub const FONT_FAMILY: &str = ".SystemUIFont";
pub const MONO_FONT_FAMILY: &str = "Menlo";

/// Size and line height of each text role.
pub mod type_scale {
    use gpui::{Pixels, px};
    pub const DISPLAY: (Pixels, Pixels) = (px(22.), px(28.));
    pub const TOPIC_TITLE: (Pixels, Pixels) = (px(17.), px(22.));
    pub const TITLE: (Pixels, Pixels) = (px(15.), px(20.));
    pub const MESSAGE: (Pixels, Pixels) = (px(14.), px(24.));
    pub const UI: (Pixels, Pixels) = (px(13.), px(18.));
    pub const QUOTE: (Pixels, Pixels) = (px(13.), px(19.));
    pub const CALLOUT: (Pixels, Pixels) = (px(12.), px(16.));
    pub const ACK: (Pixels, Pixels) = (px(12.5), px(18.));
    pub const CAPTION: (Pixels, Pixels) = (px(11.), px(14.));
    pub const CODE: (Pixels, Pixels) = (px(12.5), px(19.));
    pub const INVITE: (Pixels, Pixels) = (px(24.), px(30.));
    pub const OTP: (Pixels, Pixels) = (px(16.), px(22.));
    pub const EMOJI: (Pixels, Pixels) = (px(32.), px(40.));
}

/// OpenType tabular figures: every count, time, date, size and version.
pub fn tabular() -> FontFeatures {
    static TNUM: OnceLock<FontFeatures> = OnceLock::new();
    TNUM.get_or_init(|| FontFeatures(Arc::new(vec![("tnum".into(), 1)])))
        .clone()
}

/// Text roles for any styled element. Weights are 400, 500 and 600 only.
pub trait Type: Styled + Sized {
    fn type_role(self, role: (Pixels, Pixels)) -> Self {
        self.text_size(role.0).line_height(role.1)
    }
    /// 22 / 28, 600: the login welcome.
    fn t_display(self) -> Self {
        self.type_role(type_scale::DISPLAY)
            .font_weight(FontWeight::SEMIBOLD)
    }
    /// 17 / 22, 600: the topic view toolbar.
    fn t_topic_title(self) -> Self {
        self.type_role(type_scale::TOPIC_TITLE)
            .font_weight(FontWeight::SEMIBOLD)
    }
    /// 15 / 20, 600: column, pane and sheet titles.
    fn t_title(self) -> Self {
        self.type_role(type_scale::TITLE)
            .font_weight(FontWeight::SEMIBOLD)
    }
    /// 14 / 24: message body and composer text.
    fn t_message(self) -> Self {
        self.type_role(type_scale::MESSAGE)
    }
    /// 13 / 18: rows, menus, buttons, fields.
    fn t_ui(self) -> Self {
        self.type_role(type_scale::UI)
    }
    /// 12 / 16: previews, 岗位, metadata, hints.
    fn t_callout(self) -> Self {
        self.type_role(type_scale::CALLOUT)
    }
    /// 11 / 14, 500: chips, badges, timestamps, kbd.
    fn t_caption(self) -> Self {
        self.type_role(type_scale::CAPTION)
            .font_weight(FontWeight::MEDIUM)
    }
    /// 11 / 14, 600: section headers.
    fn t_section(self) -> Self {
        self.type_role(type_scale::CAPTION)
            .font_weight(FontWeight::SEMIBOLD)
    }
    /// 12.5 / 19 Menlo.
    fn t_code(self) -> Self {
        self.type_role(type_scale::CODE)
            .font_family(MONO_FONT_FAMILY)
    }
    fn medium(self) -> Self {
        self.font_weight(FontWeight::MEDIUM)
    }
    fn semibold(self) -> Self {
        self.font_weight(FontWeight::SEMIBOLD)
    }
    /// Tabular figures.
    fn tnum(self) -> Self {
        self.font_features(tabular())
    }
    /// One line with an ellipsis at the end.
    fn one_line(self) -> Self {
        self.overflow_hidden().whitespace_nowrap().text_ellipsis()
    }
}

impl<T: Styled> Type for T {}

// ---------------------------------------------------------------------------
// Motion (§10)

pub mod motion {
    use super::*;

    pub const FAST: Duration = Duration::from_millis(120);
    pub const NORMAL: Duration = Duration::from_millis(180);
    pub const SLOW: Duration = Duration::from_millis(220);
    /// Exit of menus and popovers.
    pub const EXIT: Duration = Duration::from_millis(100);
    /// Crossfades: new messages, chip changes, the jump pill.
    pub const FADE: Duration = Duration::from_millis(150);
    /// Tooltip delay the first time.
    pub const TOOLTIP_DELAY: Duration = Duration::from_millis(500);
    /// Streaming caret: idle time before it blinks, and the blink half-period.
    pub const CARET_IDLE: Duration = Duration::from_millis(1000);
    pub const CARET_BLINK: Duration = Duration::from_millis(530);
    /// Typing dots pulse and shimmer loop.
    pub const TYPING_PULSE: Duration = Duration::from_millis(1200);
    pub const SHIMMER: Duration = Duration::from_millis(1600);
    pub const SPINNER_TURN: Duration = Duration::from_millis(800);
    /// Travel of popovers and the jump pill.
    pub const TRAVEL: gpui::Pixels = gpui::px(4.);

    /// `cubic-bezier(0.23, 1, 0.32, 1)`: enter and exit.
    pub fn out() -> Easing {
        Easing::cubic_bezier(0.23, 1.0, 0.32, 1.0).expect("valid curve")
    }

    /// `cubic-bezier(0.77, 0, 0.175, 1)`: movement on screen.
    pub fn moving() -> Easing {
        Easing::cubic_bezier(0.77, 0.0, 0.175, 1.0).expect("valid curve")
    }

    /// The out curve as a function, for `with_animation`.
    pub fn out_fn() -> impl Fn(f32) -> f32 + 'static {
        let e = out();
        move |t| e.sample(t)
    }
}

// ---------------------------------------------------------------------------
// Colors (§2)

/// Semantic colors of the app, one value per mode.
#[derive(Clone, Copy, Debug)]
pub struct Colors {
    // Surfaces
    pub sidebar: Hsla,
    pub sidebar_tint: Hsla,
    pub content: Hsla,
    pub panel: Hsla,
    pub window_bg: Hsla,
    pub group: Hsla,
    pub elevated: Hsla,
    pub toolbar_capsule: Hsla,
    pub scrim: Hsla,
    // Text
    pub text_1: Hsla,
    pub text_2: Hsla,
    pub text_3: Hsla,
    pub placeholder: Hsla,
    // Lines and fills
    pub sep: Hsla,
    pub sep_strong: Hsla,
    pub fill_hover: Hsla,
    pub fill_selected: Hsla,
    pub sidebar_selected: Hsla,
    pub fill_pressed: Hsla,
    pub fill_field: Hsla,
    pub fill_active: Hsla,
    pub control: Hsla,
    pub control_border: Hsla,
    pub control_border_focus: Hsla,
    pub chip_ring: Hsla,
    pub card_border: Hsla,
    pub image_border: Hsla,
    pub quote_bar: Hsla,
    // Accent
    pub accent: Hsla,
    pub accent_hover: Hsla,
    pub accent_pressed: Hsla,
    pub accent_text: Hsla,
    pub accent_soft: Hsla,
    pub on_accent: Hsla,
    pub focus_ring: Hsla,
    pub selection: Hsla,
    pub mention_bg: Hsla,
    pub mention_me_bg: Hsla,
    pub mention_me_text: Hsla,
    // Status chips
    pub st_quiet_text: Hsla,
    pub st_you_bg: Hsla,
    pub st_you_text: Hsla,
    pub st_blocked_bg: Hsla,
    pub st_blocked_text: Hsla,
    pub glyph_progress: Hsla,
    pub glyph_wait: Hsla,
    pub glyph_done: Hsla,
    // Badges, presence, bot mark
    pub badge_bg: Hsla,
    pub badge_text: Hsla,
    pub badge_mention_bg: Hsla,
    pub online: Hsla,
    pub bot_mark: Hsla,
    // Content blocks and system colors
    pub code_bg: Hsla,
    pub code_text: Hsla,
    pub danger: Hsla,
    pub danger_text: Hsla,
    pub success: Hsla,
    pub warning: Hsla,
    pub new_divider: Hsla,
    pub scrollbar_thumb: Hsla,
    /// Shadow colors of each elevation level, in the order of [`Elevation`].
    shadows: ShadowColors,
    dark: bool,

    // Older names, kept so code written before DESIGN.md still builds. New
    // code uses the token names above.
    pub background: Hsla,
    pub foreground: Hsla,
    pub muted_foreground: Hsla,
    pub border: Hsla,
    pub sidebar_foreground: Hsla,
    pub row_hover: Hsla,
    pub row_selected: Hsla,
    pub accent_foreground: Hsla,
    pub badge: Hsla,
    pub badge_foreground: Hsla,
    pub mention: Hsla,
    pub mention_foreground: Hsla,
    pub offline: Hsla,
    pub code_background: Hsla,
    /// A message opened from search, and matched words in search results.
    pub search_highlight: Hsla,
}

#[derive(Clone, Copy, Debug)]
struct ShadowColors {
    control: [Hsla; 2],
    capsule: [Hsla; 2],
    popover: [Hsla; 3],
    sheet: [Hsla; 3],
}

impl Colors {
    pub fn is_dark(&self) -> bool {
        self.dark
    }

    /// Avatar and group-tile palette `(background, foreground)` for an index
    /// from [`palette_index`] (§2.7).
    pub fn palette(&self, ix: usize) -> (Hsla, Hsla) {
        const L: [(u32, u32); 8] = [
            (0xE3E8EFFF, 0x46566CFF),
            (0xE0ECE2FF, 0x3B6646FF),
            (0xF0E7D8FF, 0x76572AFF),
            (0xF3E2E4FF, 0x874450FF),
            (0xDCE9F6FF, 0x2E5C87FF),
            (0xEFE1D9FF, 0x864D36FF),
            (0xDAEDEDFF, 0x2C6868FF),
            (0xE6E6E9FF, 0x55555CFF),
        ];
        const D: [(u32, u32); 8] = [
            (0x3A4352FF, 0xC3CFDFFF),
            (0x33463AFF, 0xB6D6BEFF),
            (0x4A4031FF, 0xE2CFAEFF),
            (0x4D3539FF, 0xEBC0C7FF),
            (0x2F4257FF, 0xB4D2EEFF),
            (0x4C372EFF, 0xEBC4B2FF),
            (0x2C4646FF, 0xAEDADAFF),
            (0x3C3C41FF, 0xCFCFD6FF),
        ];
        let (bg, fg) = if self.dark { D } else { L }[ix % 8];
        (c(bg), c(fg))
    }

    /// The shadow list of an elevation level (§7). The first shadow is a
    /// 0.5 pt ring that serves as the border.
    pub fn shadow(&self, level: Elevation) -> Vec<BoxShadow> {
        let ring = |color| BoxShadow {
            color,
            offset: point(px(0.), px(0.)),
            blur_radius: px(0.),
            spread_radius: HAIRLINE,
            inset: false,
        };
        let drop = |color, y: f32, blur: f32| BoxShadow {
            color,
            offset: point(px(0.), px(y)),
            blur_radius: px(blur),
            spread_radius: px(0.),
            inset: false,
        };
        let s = &self.shadows;
        match level {
            Elevation::Card => vec![ring(self.card_border)],
            Elevation::Image => vec![ring(self.image_border)],
            Elevation::Control => vec![ring(s.control[0]), drop(s.control[1], 1., 1.5)],
            Elevation::Capsule => vec![ring(s.capsule[0]), drop(s.capsule[1], 1., 3.)],
            Elevation::Popover => vec![
                ring(s.popover[0]),
                drop(s.popover[1], 4., 12.),
                drop(s.popover[2], 14., 36.),
            ],
            Elevation::Sheet => vec![
                ring(s.sheet[0]),
                drop(s.sheet[1], 6., 16.),
                drop(s.sheet[2], 24., 64.),
            ],
        }
    }
}

/// Elevation levels (§7).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Elevation {
    /// Level 1: cards, grouped sections, the decision box.
    Card,
    /// Level 1 for images.
    Image,
    /// Secondary buttons, inputs, OTP boxes, the selected segment.
    Control,
    /// Toolbar capsules.
    Capsule,
    /// Level 2: popovers, menus, hover toolbar, jump pill, tooltips, toasts.
    Popover,
    /// Level 3: sheets, alerts, the switcher.
    Sheet,
}

/// A stable palette index for an id: FNV-1a modulo 8 (§2.7).
pub fn palette_index(id: &str) -> usize {
    let mut hash: u32 = 0x811c_9dc5;
    for byte in id.bytes() {
        hash ^= byte as u32;
        hash = hash.wrapping_mul(0x0100_0193);
    }
    hash as usize % 8
}

#[allow(clippy::too_many_arguments)]
fn build(dark: bool) -> Colors {
    let p = |l: u32, d: u32| if dark { c(d) } else { c(l) };
    let content = p(0xFFFFFFFF, 0x1E1E20FF);
    let text_1 = p(0x000000D9, 0xFFFFFFDB);
    let text_2 = p(0x0000008C, 0xEBEBF594);
    let text_3 = p(0x00000075, 0xEBEBF56B);
    let sep = p(0x00000017, 0xFFFFFF16);
    let fill_hover = p(0x00000009, 0xFFFFFF0B);
    let fill_selected = p(0x00000011, 0xFFFFFF16);
    let accent = p(0x007AFFFF, 0x0A84FFFF);
    let on_accent = c(0xFFFFFFFF);
    let badge_bg = p(0x00000011, 0xFFFFFF1A);
    let badge_text = p(0x00000094, 0xEBEBF59E);
    let sidebar = p(0xF0F0F2FF, 0x262628FF);
    let panel = p(0xF7F7F8FF, 0x232326FF);
    let code_bg = p(0xF5F5F7FF, 0x28282BFF);
    let danger_text = p(0xD70015FF, 0xFF6961FF);
    let online = p(0x34C759FF, 0x30D158FF);
    Colors {
        sidebar,
        sidebar_tint: p(0xEAEAEFBD, 0x242428C2),
        content,
        panel,
        window_bg: p(0xECECEEFF, 0x1E1E20FF),
        group: p(0xFFFFFFFF, 0x2A2A2DFF),
        elevated: p(0xFFFFFFFF, 0x2C2C2FFF),
        toolbar_capsule: p(0xFFFFFFDB, 0x3A3A3EDB),
        scrim: p(0x0000002E, 0x00000061),
        text_1,
        text_2,
        text_3,
        placeholder: p(0x0000005C, 0xEBEBF55C),
        sep,
        sep_strong: p(0x00000024, 0xFFFFFF21),
        fill_hover,
        fill_selected,
        sidebar_selected: p(0x00000013, 0xFFFFFF1A),
        fill_pressed: p(0x0000001A, 0xFFFFFF21),
        fill_field: p(0x0000000B, 0xFFFFFF12),
        fill_active: p(0x007AFF1F, 0x0A84FF42),
        control: p(0xFFFFFFFF, 0xFFFFFF1A),
        control_border: p(0x00000021, 0xFFFFFF1A),
        control_border_focus: p(0x00000042, 0xFFFFFF3D),
        chip_ring: p(0x00000024, 0xFFFFFF29),
        card_border: p(0x0000001A, 0xFFFFFF1A),
        image_border: p(0x00000012, 0xFFFFFF14),
        quote_bar: p(0x00000029, 0xFFFFFF33),
        accent,
        accent_hover: p(0x0A6FE6FF, 0x2B93FFFF),
        accent_pressed: p(0x0062CCFF, 0x0070E0FF),
        accent_text: p(0x0066D6FF, 0x4DA3FFFF),
        accent_soft: p(0x007AFF1C, 0x0A84FF38),
        on_accent,
        focus_ring: p(0x007AFF6B, 0x0A84FF8C),
        selection: p(0x007AFF33, 0x0A84FF5C),
        mention_bg: p(0x007AFF1A, 0x0A84FF33),
        mention_me_bg: p(0xFF95002E, 0xFF9F0A3D),
        mention_me_text: p(0xA35400FF, 0xFFBD5CFF),
        st_quiet_text: p(0x0000009E, 0xEBEBF5A3),
        st_you_bg: p(0xFF95002B, 0xFF9F0A38),
        st_you_text: p(0x9C5100FF, 0xFFB547FF),
        st_blocked_bg: p(0xFF3B301C, 0xFF453A33),
        st_blocked_text: p(0xC4001AFF, 0xFF7A70FF),
        glyph_progress: accent,
        glyph_wait: p(0xE68600FF, 0xFF9F0AFF),
        glyph_done: p(0x1F9D45FF, 0x30D158FF),
        badge_bg,
        badge_text,
        badge_mention_bg: accent,
        online,
        bot_mark: p(0x7C7C82FF, 0x8E8E94FF),
        code_bg,
        code_text: text_1,
        danger: p(0xFF3B30FF, 0xFF453AFF),
        danger_text,
        success: online,
        warning: p(0xFF9500FF, 0xFF9F0AFF),
        new_divider: p(0xFF3B30B3, 0xFF453AB3),
        scrollbar_thumb: p(0x00000040, 0xFFFFFF40),
        shadows: ShadowColors {
            control: [p(0x0000001F, 0xFFFFFF14), p(0x0000000F, 0x00000040)],
            capsule: [p(0x00000014, 0xFFFFFF14), p(0x0000000F, 0x0000004D)],
            popover: [
                p(0x0000001F, 0xFFFFFF1A),
                p(0x00000014, 0x0000004D),
                p(0x0000001F, 0x00000073),
            ],
            sheet: [
                p(0x00000024, 0xFFFFFF1F),
                p(0x0000001A, 0x00000059),
                p(0x00000033, 0x0000008C),
            ],
        },
        dark,
        background: content,
        foreground: text_1,
        muted_foreground: text_2,
        border: sep,
        sidebar_foreground: text_1,
        row_hover: fill_hover,
        row_selected: fill_selected,
        accent_foreground: on_accent,
        badge: badge_bg,
        badge_foreground: badge_text,
        mention: accent,
        mention_foreground: on_accent,
        offline: text_3,
        code_background: code_bg,
        search_highlight: p(0xFF95002E, 0xFF9F0A3D),
    }
}

fn light_colors() -> &'static Colors {
    static L: OnceLock<Colors> = OnceLock::new();
    L.get_or_init(|| build(false))
}

fn dark_colors() -> &'static Colors {
    static D: OnceLock<Colors> = OnceLock::new();
    D.get_or_init(|| build(true))
}

/// The app's colors for the current mode.
pub fn colors(cx: &App) -> Colors {
    if is_dark(cx) {
        *dark_colors()
    } else {
        *light_colors()
    }
}

/// The kind of a status chip (§2.5): three weights only.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChipKind {
    /// Outlined, colored glyph: 进行中, 完成, 等{name}….
    Quiet,
    /// Filled orange, weight 600: 等你….
    You,
    /// Red tint: 卡住了.
    Blocked,
}

/// Background, text, ring and glyph colors of a chip kind; `glyph` is the
/// quiet chip's colored glyph for this status.
pub fn chip_colors(kind: ChipKind, status: TopicStatus, cx: &App) -> ChipColors {
    let k = colors(cx);
    let clear = c(0x00000000);
    match kind {
        ChipKind::You => ChipColors {
            bg: k.st_you_bg,
            text: k.st_you_text,
            ring: clear,
            glyph: k.st_you_text,
        },
        ChipKind::Blocked => ChipColors {
            bg: k.st_blocked_bg,
            text: k.st_blocked_text,
            ring: clear,
            glyph: k.st_blocked_text,
        },
        ChipKind::Quiet => ChipColors {
            bg: clear,
            text: k.st_quiet_text,
            ring: k.chip_ring,
            glyph: match status {
                TopicStatus::InProgress => k.glyph_progress,
                TopicStatus::Done => k.glyph_done,
                TopicStatus::WaitingAnswer | TopicStatus::WaitingReview => k.glyph_wait,
                TopicStatus::Open | TopicStatus::Blocked => k.text_2,
            },
        },
    }
}

#[derive(Clone, Copy, Debug)]
pub struct ChipColors {
    pub bg: Hsla,
    pub text: Hsla,
    pub ring: Hsla,
    pub glyph: Hsla,
}

/// Background and text colors of a topic status chip. Kept for older callers;
/// new code uses `ui::StatusChip`.
pub fn status_chip_colors(status: TopicStatus, cx: &App) -> (Hsla, Hsla) {
    let kind = match status {
        TopicStatus::Blocked => ChipKind::Blocked,
        _ => ChipKind::Quiet,
    };
    let cc = chip_colors(kind, status, cx);
    (cc.bg, cc.text)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rel_luminance(h: Hsla, over: Hsla) -> f32 {
        let a = h.to_rgb();
        let b = over.to_rgb();
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
        let l1 = rel_luminance(fg, bg);
        let l2 = rel_luminance(bg, bg);
        (l1.max(l2) + 0.05) / (l1.min(l2) + 0.05)
    }

    #[test]
    fn content_text_meets_contrast() {
        for k in [build(false), build(true)] {
            assert!(contrast(k.text_1, k.content) >= 12.0);
            assert!(contrast(k.text_2, k.content) >= 4.5);
            assert!(contrast(k.text_2, k.sidebar) >= 4.5);
            assert!(contrast(k.text_3, k.content) >= 3.4);
            assert!(contrast(k.st_quiet_text, k.content) >= 4.5);
            assert!(contrast(k.danger_text, k.content) >= 4.5);
            assert!(contrast(k.accent_text, k.content) >= 4.5);
        }
    }

    #[test]
    fn palette_index_is_stable() {
        assert_eq!(palette_index("m-joye"), palette_index("m-joye"));
        assert!(palette_index("anything") < 8);
    }
}
