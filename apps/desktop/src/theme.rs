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
// Layout (DESIGN.md §4.4)

/// Sidebar column width (200 to 320, resizable).
pub const SIDEBAR_WIDTH: Pixels = px(248.);
pub const SIDEBAR_MIN_WIDTH: Pixels = px(200.);
pub const SIDEBAR_MAX_WIDTH: Pixels = px(320.);
/// Topic list column width (280 to 400).
pub const TOPIC_LIST_WIDTH: Pixels = px(320.);
pub const TOPIC_LIST_MIN_WIDTH: Pixels = px(280.);
/// Side panel width (260 to 380), closed by default.
pub const SIDE_PANEL_WIDTH: Pixels = px(300.);
/// The topic view never gets narrower than this.
pub const TOPIC_VIEW_MIN_WIDTH: Pixels = px(480.);
/// The title row of every header band: 52 pt, centered with the traffic lights.
pub const TOOLBAR_HEIGHT: Pixels = px(52.);
/// The second row of a header band (tabs, topic meta, panel context).
pub const HEADER_SECOND_ROW: Pixels = px(40.);
/// Header band of the topic list, topic view and side panel: 52 + 40. The
/// three 0.5 pt bottom rules sit on one y.
pub const HEADER_BAND: Pixels = px(92.);
/// Below this window width the side panel closes: 248 + 320 + 480 + 300.
pub const SIDE_PANEL_MIN_WINDOW: Pixels = px(1348.);
pub const WINDOW_MIN_WIDTH: Pixels = px(1100.);
pub const WINDOW_MIN_HEIGHT: Pixels = px(680.);
pub const WINDOW_DEFAULT_WIDTH: Pixels = px(1440.);
pub const WINDOW_DEFAULT_HEIGHT: Pixels = px(900.);
/// Traffic light origin: their center lands on y 26, the title row center.
pub const TRAFFIC_LIGHTS: (Pixels, Pixels) = (px(20.), px(20.));
/// Timeline and topic row avatar size.
pub const AVATAR_SIZE: Pixels = px(36.);
/// Sidebar 群 and 私聊 row height.
pub const ROW_HEIGHT: Pixels = px(32.);
/// One measure for message text, cards, files and images (Slack).
pub const MEASURE: Pixels = px(620.);
/// Equal side gutters of the timeline and composer content box.
pub const CONTENT_GUTTER: Pixels = px(24.);
/// Hairline width. 0.5 pt is exact on Retina; on 1x it becomes 1 px.
pub const HAIRLINE: Pixels = px(0.5);
/// Border width of controls, cards, fields and the composer.
pub const BORDER: Pixels = px(1.);

/// Spacing scale (§4.1): 2, 4, 6, 8, 10, 12, 14, 16, 20, 24, 32, 44. 10 and 14
/// are for row and card insets only.
pub mod space {
    use gpui::{Pixels, px};
    pub const S2: Pixels = px(2.);
    /// Off the scale: the side padding of a mention pill only (mockup `.mn`).
    pub const S3: Pixels = px(3.);
    pub const S4: Pixels = px(4.);
    pub const S6: Pixels = px(6.);
    pub const S8: Pixels = px(8.);
    pub const S10: Pixels = px(10.);
    pub const S12: Pixels = px(12.);
    pub const S14: Pixels = px(14.);
    pub const S16: Pixels = px(16.);
    pub const S20: Pixels = px(20.);
    pub const S24: Pixels = px(24.);
    pub const S32: Pixels = px(32.);
    pub const S44: Pixels = px(44.);
    /// Not on the v2.1 scale; kept until the views that use it are rebuilt.
    pub const S28: Pixels = px(28.);
}

/// Radii (§6). Concentric rule: outer = inner + padding.
pub mod radius {
    use gpui::{Pixels, px};
    /// Inline marks in text: mentions, inline code, search marks.
    pub const MARK: Pixels = px(4.);
    /// Chips, labels, kbd, role tag, `sm` buttons, popup and menu rows,
    /// hover-toolbar buttons.
    pub const SM: Pixels = px(6.);
    /// Buttons, 32 to 36 pt fields, selects, rows, option rows, file tiles.
    pub const MD: Pixels = px(8.);
    /// 40 pt fields, `lg` buttons, invite boxes, the @mention popup.
    pub const FIELD: Pixels = px(10.);
    /// Cards, the composer, popovers, dialogs, the switcher, settings tables.
    pub const LG: Pixels = px(12.);

    // Older names, mapped onto the v2.1 steps.
    pub const XS: Pixels = MARK;
    pub const CHIP: Pixels = SM;
    pub const TILE: Pixels = SM;
    pub const ROW: Pixels = MD;
    pub const COMPOSER: Pixels = LG;
    pub const XL: Pixels = LG;

    /// Bot tiles: 0.25 x size (9 at 36, 8 at 32, 6 at 24, 12 at 48).
    pub fn bot(size: Pixels) -> Pixels {
        size * 0.25
    }
}

/// Icon sizes (§4.3).
pub mod icon {
    use gpui::{Pixels, px};
    /// Headers, toolbars, the composer, icon buttons.
    pub const HEADER: Pixels = px(18.);
    /// Buttons, fields, rows, menus.
    pub const ROW: Pixels = px(16.);
    /// Chips and labels.
    pub const CHIP: Pixels = px(14.);
    /// The role tag and section chevrons.
    pub const TAG: Pixels = px(12.);
    /// Empty states.
    pub const EMPTY: Pixels = px(20.);

    // Older names, mapped onto the v2.1 sizes.
    pub const TOOLBAR: Pixels = HEADER;
    pub const TODO: Pixels = ROW;
    pub const MENU: Pixels = ROW;
    pub const BUTTON: Pixels = ROW;
    pub const SMALL: Pixels = CHIP;
}

/// Control and row heights (§4.2, §7). Views take sizes from here, never a
/// literal.
pub mod size {
    use gpui::{Pixels, px};
    /// Button `sm`: inline in a message or card row (重试).
    pub const BUTTON_SM: Pixels = px(28.);
    /// Button `md`: the default (card actions, dialogs).
    pub const BUTTON_MD: Pixels = px(32.);
    /// Button `md` page primary (settings `添加成员 ▾`).
    pub const BUTTON_PAGE: Pixels = px(36.);
    /// Button `lg`: login, empty-state primary, full-width submit.
    pub const BUTTON_LG: Pixels = px(40.);
    /// Text button inside a message (重试 after a failed send).
    pub const TEXT_BUTTON: Pixels = px(22.);
    pub const ICON_BUTTON: Pixels = px(32.);
    pub const ICON_BUTTON_SM: Pixels = px(28.);
    /// Search and filter fields.
    pub const SEARCH_FIELD: Pixels = px(36.);
    /// Form fields (login, settings, dialogs).
    pub const FORM_FIELD: Pixels = px(40.);
    /// Select in a table row: 36 x 120.
    pub const SELECT: Pixels = px(36.);
    pub const SELECT_WIDTH: Pixels = px(120.);
    /// Invite code box: 42 x 48.
    pub const INVITE_BOX: (Pixels, Pixels) = (px(42.), px(48.));
    /// Composer: 12 top padding + 44 text (two 22 pt lines) + 40 toolbar.
    pub const COMPOSER_MIN: Pixels = px(96.);
    pub const COMPOSER_TEXT_MIN: Pixels = px(44.);
    pub const COMPOSER_TOOLBAR: Pixels = px(40.);
    /// The composer's format bar (§7.2).
    pub const FORMAT_BAR: Pixels = px(36.);
    /// The composer grows with its text up to this share of the topic view.
    pub const COMPOSER_MAX_SHARE: f32 = 0.4;
    pub const CHIP: Pixels = px(22.);
    pub const CHIP_COMPACT: Pixels = px(20.);
    pub const CHIP_MAX: Pixels = px(184.);
    pub const CHIP_COMPACT_MAX: Pixels = px(72.);
    pub const BADGE: Pixels = px(20.);
    pub const KBD: Pixels = px(20.);
    pub const ROLE_TAG: Pixels = px(20.);
    pub const REACTION: Pixels = px(26.);
    pub const SECTION_HEADER: Pixels = px(28.);
    pub const SIDEBAR_ROW: Pixels = px(32.);
    pub const SIDEBAR_FOOTER: Pixels = px(56.);
    pub const TOPIC_ROW: Pixels = px(68.);
    pub const MENU_ROW: Pixels = px(32.);
    pub const POPUP_ROW: Pixels = px(40.);
    pub const SWITCHER_ROW: Pixels = px(44.);
    pub const SWITCHER_HIT_ROW: Pixels = px(60.);
    pub const SWITCHER_QUERY: Pixels = px(56.);
    pub const SETTINGS_ROW: Pixels = px(60.);
    pub const OPTION_ROW: Pixels = px(40.);
    pub const PANEL_ROW: Pixels = px(40.);
    /// Plan and step rows of the side panel's progress (§8.1).
    pub const PLAN_ROW: Pixels = px(28.);
    pub const BANNER: Pixels = px(52.);
    pub const TOAST: Pixels = px(44.);
    /// Avatars (§4.3).
    pub const AVATAR_MESSAGE: Pixels = px(36.);
    pub const AVATAR_FOOTER: Pixels = px(32.);
    pub const AVATAR_ROW: Pixels = px(24.);
    pub const AVATAR_START: Pixels = px(48.);
    /// Widths of overlays (§7.12).
    pub const SWITCHER_WIDTH: Pixels = px(640.);
    pub const DIALOG_WIDTH: Pixels = px(440.);
    pub const DIALOG_WIDE: Pixels = px(560.);
    pub const ALERT_WIDTH: Pixels = px(320.);
    pub const MENTION_POPUP_WIDTH: Pixels = px(320.);
    /// Command-style panels sit this far below the window top.
    pub const PALETTE_TOP: Pixels = px(96.);
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

/// Every gpui-component slot the app can show (DESIGN.md §2.8).
fn apply(t: &mut Theme, dark: bool) {
    let k = if dark { dark_colors() } else { light_colors() };
    let p = |l: u32, d: u32| if dark { c(d) } else { c(l) };
    let clear = c(0x00000000);

    t.background = k.content;
    t.foreground = k.text_1;
    t.muted = k.field;
    t.muted_foreground = k.text_2;
    t.border = k.border;
    t.input = k.border;
    t.ring = k.focus;
    t.selection = p(0x1F6BFF33, 0x4C8DFF5C);
    t.caret = k.accent;
    t.overlay = k.scrim;
    t.accent = p(0x1F232914, 0xFFFFFF16);
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
        p(0xFFFFFFFF, 0xFFFFFF14),
        p(0xF6F7F9FF, 0xFFFFFF1F),
        p(0xECEDF0FF, 0xFFFFFF29),
    );
    t.secondary = sec;
    t.secondary_hover = sec_h;
    t.secondary_active = sec_a;
    // Ghost buttons draw in `secondary_foreground`: `text-2` at rest, and
    // `accent_foreground` (`text-1`) on hover (§7.1).
    t.secondary_foreground = k.text_2;
    t.button = sec;
    t.button_hover = sec_h;
    t.button_active = sec_a;
    t.button_foreground = k.text_1;
    t.button_secondary = sec;
    t.button_secondary_hover = sec_h;
    t.button_secondary_active = sec_a;
    t.button_secondary_foreground = k.text_1;

    t.danger = k.danger;
    t.danger_hover = p(0xD93F44FF, 0xF66A6EFF);
    t.danger_active = p(0xC2272DFF, 0xD9474CFF);
    t.danger_foreground = k.on_accent;
    t.button_danger = t.danger;
    t.button_danger_hover = t.danger_hover;
    t.button_danger_active = t.danger_active;
    t.button_danger_foreground = k.on_accent;
    t.success = k.success;
    t.success_hover = k.success;
    t.success_active = k.success;
    t.success_foreground = k.on_accent;
    t.warning = k.you_glyph;
    t.warning_hover = k.you_glyph;
    t.warning_active = k.you_glyph;
    t.warning_foreground = k.on_accent;
    t.info = k.accent;
    t.info_hover = k.accent_hover;
    t.info_active = k.accent_pressed;
    t.info_foreground = k.on_accent;

    t.colors.list = k.content;
    t.list_hover = k.hover;
    t.list_active = p(0x1F232914, 0xFFFFFF16);
    t.list_active_border = clear;
    t.list_even = k.content;
    t.list_head = k.content;

    t.table = k.content;
    t.table_hover = k.hover;
    t.table_active = p(0x1F232914, 0xFFFFFF16);
    t.table_active_border = clear;
    t.table_even = k.content;
    t.table_row_border = k.sep;
    t.table_head = k.content;
    t.table_head_foreground = k.text_3;
    t.table_foot = k.content;
    t.table_foot_foreground = k.text_3;

    t.popover = k.overlay;
    t.popover_foreground = k.text_1;
    t.group_box = k.content;
    t.group_box_foreground = k.text_1;
    t.description_list_label = clear;
    t.description_list_label_foreground = k.text_2;

    t.sidebar = k.sidebar;
    t.sidebar_foreground = k.text_2;
    t.sidebar_accent = p(0x1F232917, 0xFFFFFF17);
    t.sidebar_accent_foreground = k.text_1;
    t.sidebar_primary = k.accent;
    t.sidebar_primary_foreground = k.on_accent;
    t.sidebar_border = k.sep;

    t.title_bar = clear;
    t.title_bar_border = clear;
    t.status_bar = k.content;
    t.status_bar_border = k.sep;

    t.tab_bar = clear;
    t.tab_bar_segmented = k.field;
    t.tab = clear;
    t.tab_active = clear;
    t.tab_foreground = k.text_2;
    t.tab_active_foreground = k.text_1;

    t.switch = k.border_strong;
    t.switch_thumb = c(0xFFFFFFFF);
    t.slider_bar = k.accent;
    t.slider_thumb = c(0xFFFFFFFF);
    t.progress_bar = k.accent;
    t.skeleton = k.field;

    t.scrollbar = clear;
    t.scrollbar_thumb = p(0x1F232940, 0xFFFFFF40);
    t.scrollbar_thumb_hover = p(0x1F232966, 0xFFFFFF66);

    t.link = k.accent_text;
    t.link_hover = p(0x1250BFFF, 0x94BAFFFF);
    t.link_active = p(0x0E449FFF, 0xAECBFFFF);

    t.blue = k.accent;
    t.blue_light = k.info_bg;
    t.yellow = k.you_text;
    t.yellow_light = k.you_bg;
    t.red = k.danger_text;
    t.red_light = k.danger_bg;
    t.green = k.success_text;
    t.green_light = p(0x1E9E5A1F, 0x3CCB7F29);
    t.magenta = t.blue;
    t.magenta_light = t.blue_light;
    t.cyan = t.blue;
    t.cyan_light = t.blue_light;

    t.chart_1 = k.accent;
    t.chart_2 = k.success;
    t.chart_3 = k.you_glyph;
    t.chart_4 = k.text_2;
    t.chart_5 = k.text_3;
    t.window_border = k.border;
    t.drag_border = k.accent;
    t.drop_target = k.info_bg;

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
// Typography (§3)

/// System font (SF Pro); Core Text falls back to PingFang SC for Han and to
/// Apple Color Emoji for emoji.
pub const FONT_FAMILY: &str = ".SystemUIFont";
/// Weight 500 as drawn: font-kit reads SF Medium's Core Text weight (0.23)
/// as about 530, and its CSS matching sends an exact 500 down to Regular
/// when no face is exactly 500. Just above 500 picks the closest heavier
/// face, Medium.
pub const MEDIUM_WEIGHT: FontWeight = FontWeight(501.);
pub const MONO_FONT_FAMILY: &str = "Menlo";

/// The closed type scale (§3): size and line height of each style. There is
/// no 11 pt text and no text a reader reads under 12.
pub mod type_scale {
    use gpui::{Pixels, px};
    /// 22 / 30: topic start title, page titles, login title, empty-state title.
    pub const DISPLAY: (Pixels, Pixels) = (px(22.), px(30.));
    /// 18 / 26: the topic view title, the one primary title of the window.
    pub const HEADING: (Pixels, Pixels) = (px(18.), px(26.));
    /// 16 / 24: topic list header, side panel header, dialog title.
    pub const TITLE: (Pixels, Pixels) = (px(16.), px(24.));
    /// 15 / 22: card titles.
    pub const CARD_TITLE: (Pixels, Pixels) = (px(15.), px(22.));
    /// 14 / 22: message text, composer text, card text, option rows.
    pub const BODY: (Pixels, Pixels) = (px(14.), px(22.));
    /// 14 / 20: rows, tabs, buttons, menus, fields.
    pub const UI: (Pixels, Pixels) = (px(14.), px(20.));
    /// 13 / 20: previews, live line, inline buttons, footnotes, snippets.
    pub const SMALL: (Pixels, Pixels) = (px(13.), px(20.));
    /// 12 / 16: times, chips, badges, kbd, role tag, file meta, help.
    pub const META: (Pixels, Pixels) = (px(12.), px(16.));
    /// 13 / 20 Menlo.
    pub const CODE: (Pixels, Pixels) = (px(13.), px(20.));
    /// 20 / 28: invite code boxes only.
    pub const CODE_ENTRY: (Pixels, Pixels) = (px(20.), px(28.));
    /// 15 / 20: the `lg` button label (buttons only).
    pub const BUTTON_LG: (Pixels, Pixels) = (px(15.), px(20.));
    /// 32 / 40: a message of one to three emoji.
    pub const EMOJI: (Pixels, Pixels) = (px(32.), px(40.));
    /// 16 / 16: emoji in reaction pills.
    pub const REACTION_EMOJI: (Pixels, Pixels) = (px(16.), px(16.));

    // Older names, mapped onto the closed scale.
    pub const TOPIC_TITLE: (Pixels, Pixels) = HEADING;
    pub const MESSAGE: (Pixels, Pixels) = BODY;
    pub const QUOTE: (Pixels, Pixels) = SMALL;
    pub const CALLOUT: (Pixels, Pixels) = SMALL;
    pub const ACK: (Pixels, Pixels) = SMALL;
    pub const CAPTION: (Pixels, Pixels) = META;
    pub const INVITE: (Pixels, Pixels) = CODE_ENTRY;
    pub const OTP: (Pixels, Pixels) = CODE_ENTRY;
}

/// OpenType tabular figures: every count, time, date, size and version.
pub fn tabular() -> FontFeatures {
    static TNUM: OnceLock<FontFeatures> = OnceLock::new();
    TNUM.get_or_init(|| FontFeatures(Arc::new(vec![("tnum".into(), 1)])))
        .clone()
}

/// Text styles for any styled element (§3). Weights are 400, 500 and 600.
pub trait Type: Styled + Sized {
    fn type_role(self, role: (Pixels, Pixels)) -> Self {
        self.text_size(role.0).line_height(role.1)
    }
    /// 22 / 30, 600.
    fn t_display(self) -> Self {
        self.type_role(type_scale::DISPLAY)
            .font_weight(FontWeight::SEMIBOLD)
    }
    /// 18 / 26, 600: the topic view title.
    fn t_heading(self) -> Self {
        self.type_role(type_scale::HEADING)
            .font_weight(FontWeight::SEMIBOLD)
    }
    /// 16 / 24, 600: column, pane and dialog titles.
    fn t_title(self) -> Self {
        self.type_role(type_scale::TITLE)
            .font_weight(FontWeight::SEMIBOLD)
    }
    /// 15 / 22, 600: card titles.
    fn t_card_title(self) -> Self {
        self.type_role(type_scale::CARD_TITLE)
            .font_weight(FontWeight::SEMIBOLD)
    }
    /// 14 / 22: message, composer and card text.
    fn t_body(self) -> Self {
        self.type_role(type_scale::BODY)
    }
    /// 14 / 20: rows, menus, buttons, fields.
    fn t_ui(self) -> Self {
        self.type_role(type_scale::UI)
    }
    /// 13 / 20: previews, secondary lines, inline buttons, footnotes.
    fn t_small(self) -> Self {
        self.type_role(type_scale::SMALL)
    }
    /// 12 / 16: times, help, file meta.
    fn t_meta(self) -> Self {
        self.type_role(type_scale::META)
    }
    /// 12 / 16, 500: chips, kbd, role tags, field labels of tables.
    fn t_meta_medium(self) -> Self {
        self.type_role(type_scale::META).font_weight(MEDIUM_WEIGHT)
    }
    /// 12 / 16, 600: badges and sidebar section headers.
    fn t_section(self) -> Self {
        self.type_role(type_scale::META)
            .font_weight(FontWeight::SEMIBOLD)
    }
    /// 13 / 20 Menlo.
    fn t_code(self) -> Self {
        self.type_role(type_scale::CODE)
            .font_family(MONO_FONT_FAMILY)
    }
    /// 20 / 28, 500, tabular figures: invite code boxes.
    fn t_code_entry(self) -> Self {
        self.type_role(type_scale::CODE_ENTRY)
            .font_weight(MEDIUM_WEIGHT)
            .font_features(tabular())
    }
    /// Older name of [`Type::t_heading`].
    fn t_topic_title(self) -> Self {
        self.t_heading()
    }
    /// Older name of [`Type::t_body`].
    fn t_message(self) -> Self {
        self.t_body()
    }
    /// Older name of [`Type::t_small`].
    fn t_callout(self) -> Self {
        self.t_small()
    }
    /// Older name of [`Type::t_meta_medium`].
    fn t_caption(self) -> Self {
        self.t_meta_medium()
    }
    fn medium(self) -> Self {
        self.font_weight(MEDIUM_WEIGHT)
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
// Motion (§12)

pub mod motion {
    use super::*;

    pub const INSTANT: Duration = Duration::ZERO;
    pub const FAST: Duration = Duration::from_millis(120);
    pub const NORMAL: Duration = Duration::from_millis(160);
    pub const SLOW: Duration = Duration::from_millis(200);
    /// Exit of menus, popovers and dialogs.
    pub const EXIT: Duration = Duration::from_millis(150);
    /// Menus, popovers and selects enter.
    pub const MENU: Duration = Duration::from_millis(150);
    /// Crossfades: chip changes, character status, side panel content.
    pub const FADE: Duration = Duration::from_millis(160);
    /// Busy indicators wait this long before they show, then stay at least
    /// [`BUSY_MIN`].
    pub const BUSY_DELAY: Duration = Duration::from_millis(400);
    pub const BUSY_MIN: Duration = Duration::from_millis(500);
    /// Tooltip delay the first time.
    pub const TOOLTIP_DELAY: Duration = Duration::from_millis(500);
    /// Streaming caret: idle time before it blinks, and the blink half-period.
    pub const CARET_IDLE: Duration = Duration::from_millis(1000);
    pub const CARET_BLINK: Duration = Duration::from_millis(530);
    /// Pending dots loop (three 4 pt dots, 150 ms apart).
    pub const TYPING_PULSE: Duration = Duration::from_millis(1200);
    pub const SHIMMER: Duration = Duration::from_millis(1600);
    pub const SPINNER_TURN: Duration = Duration::from_millis(800);
    /// Travel of menus and popovers from their trigger.
    pub const TRAVEL: gpui::Pixels = gpui::px(4.);
    /// Rise of new messages and the pending row.
    pub const RISE: gpui::Pixels = gpui::px(6.);

    /// `cubic-bezier(0.23, 1, 0.32, 1)`: every enter and exit.
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

/// Semantic colors of the app, one value per mode (DESIGN.md §2). Field names
/// follow the token names with `_` for `-`.
#[derive(Clone, Copy, Debug)]
pub struct Colors {
    // Surfaces (§2.1)
    pub sidebar: Hsla,
    pub content: Hsla,
    pub raised: Hsla,
    pub overlay: Hsla,
    pub scrim: Hsla,
    pub code_bg: Hsla,
    // Text (§2.2)
    pub text_1: Hsla,
    pub text_2: Hsla,
    pub text_3: Hsla,
    pub placeholder: Hsla,
    // Lines and fills (§2.3)
    pub sep: Hsla,
    pub border: Hsla,
    pub border_strong: Hsla,
    pub hover: Hsla,
    pub selected: Hsla,
    pub sidebar_hover: Hsla,
    pub sidebar_selected: Hsla,
    pub sel_glyph: Hsla,
    pub pressed: Hsla,
    pub field: Hsla,
    pub count_bg: Hsla,
    pub count_text: Hsla,
    pub count_solid: Hsla,
    pub tile_ring: Hsla,
    pub opt_bg: Hsla,
    pub opt_hover: Hsla,
    pub disabled_bg: Hsla,
    pub disabled_text: Hsla,
    pub tile: Hsla,
    pub desk: Hsla,
    // Accent, focus and mentions (§2.4)
    pub accent: Hsla,
    pub accent_hover: Hsla,
    pub accent_pressed: Hsla,
    pub accent_text: Hsla,
    pub accent_soft: Hsla,
    pub on_accent: Hsla,
    pub focus: Hsla,
    pub focus_border: Hsla,
    pub focus_soft: Hsla,
    pub choice_bg: Hsla,
    pub mention_bg: Hsla,
    pub me_bg: Hsla,
    pub me_text: Hsla,
    pub hl_bg: Hsla,
    pub unread_dot: Hsla,
    // Status tones (§2.5)
    pub you_bg: Hsla,
    pub you_text: Hsla,
    pub you_glyph: Hsla,
    pub you_bar: Hsla,
    pub info_bg: Hsla,
    pub info_text: Hsla,
    pub danger: Hsla,
    pub danger_bg: Hsla,
    pub danger_text: Hsla,
    pub success: Hsla,
    pub success_text: Hsla,
    pub badge: Hsla,
    pub online: Hsla,
    // People without a photo (§2.6)
    pub avatar_bg: Hsla,
    pub avatar_fg: Hsla,
    bots: [Hsla; 6],
    shadows: ShadowColors,
    dark: bool,

    // Older names, mapped onto the v2.1 tokens so views written before v2.1
    // still build. New code uses the token names above.
    pub panel: Hsla,
    pub window_bg: Hsla,
    pub group: Hsla,
    pub elevated: Hsla,
    pub fill_hover: Hsla,
    pub fill_selected: Hsla,
    pub fill_pressed: Hsla,
    pub fill_field: Hsla,
    pub fill_active: Hsla,
    pub control_border: Hsla,
    pub control_border_focus: Hsla,
    pub quote_bar: Hsla,
    pub code_text: Hsla,
    pub warning: Hsla,
    pub new_divider: Hsla,
    pub glyph_progress: Hsla,
    pub glyph_wait: Hsla,
    pub glyph_done: Hsla,
    pub background: Hsla,
    pub foreground: Hsla,
    pub muted_foreground: Hsla,
    pub row_hover: Hsla,
    pub offline: Hsla,
    pub code_background: Hsla,
    pub search_highlight: Hsla,
}

#[derive(Clone, Copy, Debug)]
struct ShadowColors {
    /// Level 1 float: ring, drop.
    float: [Hsla; 2],
    /// Level 2 overlay: ring, drop, contact (light only).
    overlay: [Hsla; 3],
}

impl Colors {
    pub fn is_dark(&self) -> bool {
        self.dark
    }

    /// The tint of a bot tile (§2.6): one of six by a stable hash of the
    /// character id.
    pub fn bot_tint(&self, key: &str) -> Hsla {
        self.bots[bot_tint_index(key)]
    }

    /// The avatar pair `(background, foreground)` of a person without a
    /// photo. One neutral pair for everyone (§2.6); the index is ignored.
    pub fn palette(&self, _ix: usize) -> (Hsla, Hsla) {
        (self.avatar_bg, self.avatar_fg)
    }

    /// The edge of a level-2 overlay (§5): the ring color of its shadow.
    /// Dialogs draw it as their 1 pt border, because gpui-component's
    /// dialog paints its own drop shadow.
    pub fn overlay_edge(&self) -> Hsla {
        self.shadows.overlay[0]
    }

    /// The shadow list of an elevation level (§5).
    pub fn shadow(&self, level: Elevation) -> Vec<BoxShadow> {
        let ring = |color, width: Pixels| BoxShadow {
            color,
            offset: point(px(0.), px(0.)),
            blur_radius: px(0.),
            spread_radius: width,
            inset: false,
        };
        let drop = |color, y: f32, blur: f32, spread: f32| BoxShadow {
            color,
            offset: point(px(0.), px(y)),
            blur_radius: px(blur),
            spread_radius: px(spread),
            inset: false,
        };
        let s = &self.shadows;
        match level {
            Elevation::Flat => vec![],
            Elevation::Card => vec![ring(self.border, BORDER)],
            Elevation::Image => vec![ring(self.sep, BORDER)],
            Elevation::Control => vec![ring(self.border, BORDER)],
            Elevation::Float | Elevation::Capsule => {
                vec![ring(s.float[0], HAIRLINE), drop(s.float[1], 2., 8., 0.)]
            }
            Elevation::Overlay | Elevation::Popover | Elevation::Sheet => {
                if self.dark {
                    vec![
                        ring(s.overlay[0], BORDER),
                        drop(s.overlay[1], 16., 40., -8.),
                    ]
                } else {
                    vec![
                        ring(s.overlay[0], HAIRLINE),
                        drop(s.overlay[1], 8., 28., -6.),
                        drop(s.overlay[2], 2., 6., 0.),
                    ]
                }
            }
        }
    }

    /// The 3 pt outer ring of a focused field or composer (§7.1).
    pub fn focus_halo(&self) -> Vec<BoxShadow> {
        vec![BoxShadow {
            color: self.focus_soft,
            offset: point(px(0.), px(0.)),
            blur_radius: px(0.),
            spread_radius: px(3.),
            inset: false,
        }]
    }

    /// The 3 pt outer ring of a field in the error state (§7.1).
    pub fn error_halo(&self) -> Vec<BoxShadow> {
        vec![BoxShadow {
            color: self.danger_bg,
            offset: point(px(0.), px(0.)),
            blur_radius: px(0.),
            spread_radius: px(3.),
            inset: false,
        }]
    }
}

/// Elevation levels (§5). Shadows only on floating layers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Elevation {
    /// Level 0: no shadow.
    Flat,
    /// Level 1 float: hover toolbar, jump pill, selected segment.
    Float,
    /// Level 2 overlay: popovers, menus, mention popup, switcher, dialogs,
    /// toasts.
    Overlay,
    /// A 1 pt `border` drawn as a ring (cards keep level 0).
    Card,
    /// A 1 pt `sep` ring around images.
    Image,
    /// Older name: a 1 pt `border` ring.
    Control,
    /// Older name of [`Elevation::Float`].
    Capsule,
    /// Older name of [`Elevation::Overlay`].
    Popover,
    /// Older name of [`Elevation::Overlay`].
    Sheet,
}

/// FNV-1a 32-bit with offset basis `2166136261 ^ 91`, mod 6 (§2.6).
pub fn bot_tint_index(key: &str) -> usize {
    let mut hash: u32 = 0x811c_9dc5 ^ 91;
    for byte in key.bytes() {
        hash ^= byte as u32;
        hash = hash.wrapping_mul(0x0100_0193);
    }
    hash as usize % 6
}

/// A stable index for an id. Kept for older callers; people share one neutral
/// avatar pair, so the index no longer picks a color.
pub fn palette_index(id: &str) -> usize {
    bot_tint_index(id)
}

fn build(dark: bool) -> Colors {
    let p = |l: u32, d: u32| if dark { c(d) } else { c(l) };
    let sidebar = p(0xF2F3F5FF, 0x18181BFF);
    let content = p(0xFFFFFFFF, 0x1F1F23FF);
    let raised = p(0xF6F7F9FF, 0x28282DFF);
    let overlay = p(0xFFFFFFFF, 0x34343BFF);
    let code_bg = p(0xF4F5F7FF, 0x26262BFF);
    let text_1 = p(0x1F2329FF, 0xEDEDF0FF);
    let text_2 = p(0x5F6670FF, 0xA5A9B1FF);
    let text_3 = p(0x70767FFF, 0x8A8F98FF);
    let sep = p(0x1F232917, 0xFFFFFF14);
    let border = p(0x1F232924, 0xFFFFFF21);
    let border_strong = p(0x1F23293D, 0xFFFFFF38);
    let hover = p(0x1F23290A, 0xFFFFFF0A);
    let selected = p(0xE3ECFEFF, 0x23324FFF);
    let pressed = p(0x1F23291F, 0xFFFFFF1F);
    let field = p(0x1F23290E, 0xFFFFFF11);
    let accent = p(0x1F6BFFFF, 0x2F6FEBFF);
    let accent_text = p(0x175CD9FF, 0x7AA8FFFF);
    let focus_border = p(0x1F6BFFFF, 0x4C8DFFFF);
    let me_bg = p(0xFF8A002E, 0xFF9F0A38);
    let hl_bg = p(0xFF8A002E, 0xFF9F0A3D);
    let you_glyph = p(0xE07800FF, 0xFF9F0AFF);
    let danger = p(0xE5484DFF, 0xF2555AFF);
    let danger_text = p(0xC2272DFF, 0xFF7B7FFF);
    let success = p(0x1E9E5AFF, 0x3CCB7FFF);
    let online = p(0x1FB25AFF, 0x34C76AFF);
    let bots = if dark {
        [
            0x6B7DA6FF, 0x62907BFF, 0xA08562FF, 0x8472A8FF, 0x6293A1FF, 0xA0717FFF,
        ]
    } else {
        [
            0xDCE6FBFF, 0xDDF1E6FF, 0xFBE9D3FF, 0xEFE3FAFF, 0xDDEFF4FF, 0xF7DDE4FF,
        ]
    }
    .map(c);
    Colors {
        sidebar,
        content,
        raised,
        overlay,
        scrim: p(0x0F111452, 0x0000008C),
        code_bg,
        text_1,
        text_2,
        text_3,
        placeholder: p(0x9AA0A8FF, 0x6C7079FF),
        sep,
        border,
        border_strong,
        hover,
        selected,
        sidebar_hover: p(0x1F23290D, 0xFFFFFF0B),
        sidebar_selected: p(0xDCE6FAFF, 0x22304BFF),
        sel_glyph: accent_text,
        pressed,
        field,
        count_bg: p(0xE6E8ECFF, 0x3A3E46FF),
        count_text: p(0x4B5058FF, 0xC9CDD4FF),
        count_solid: p(0x697079FF, 0x5C6370FF),
        tile_ring: p(0xFFFFFFFF, 0xFFFFFF1A),
        opt_bg: p(0xF4F5F7FF, 0x2A2D33FF),
        opt_hover: p(0xECEEF1FF, 0x32353CFF),
        disabled_bg: p(0xECEEF1FF, 0x2E3138FF),
        disabled_text: p(0x8A8F98FF, 0x70767FFF),
        tile: p(0xEBEDF0FF, 0x34343BFF),
        desk: p(0xE9EBEFFF, 0x0E0F12FF),
        accent,
        accent_hover: p(0x165EEBFF, 0x3D7EFFFF),
        accent_pressed: p(0x1252CCFF, 0x2A63D4FF),
        accent_text,
        accent_soft: p(0x1F6BFF1A, 0x4C8DFF29),
        on_accent: c(0xFFFFFFFF),
        focus: p(0x1F6BFF8C, 0x4C8DFFA6),
        focus_border,
        focus_soft: p(0x1F6BFF29, 0x4C8DFF3D),
        choice_bg: p(0x1F6BFF0F, 0x4C8DFF1F),
        mention_bg: p(0x1F6BFF1A, 0x4C8DFF2E),
        me_bg,
        me_text: p(0x9A4A00FF, 0xFFB547FF),
        hl_bg,
        unread_dot: p(0x1F6BFFFF, 0x4C8DFFFF),
        you_bg: p(0xFFE9D1FF, 0x4A3418FF),
        you_text: p(0x9A4A00FF, 0xFFB547FF),
        you_glyph,
        you_bar: c(0xF5A623FF),
        info_bg: p(0x1F6BFF1A, 0x4C8DFF2E),
        info_text: accent_text,
        danger,
        danger_bg: p(0xE5484D1F, 0xF2555A2E),
        danger_text,
        success,
        success_text: p(0x137042FF, 0x5BD893FF),
        badge: c(0xD93A40FF),
        online,
        avatar_bg: p(0xE6EAF2FF, 0x364052FF),
        avatar_fg: p(0x3E4C63FF, 0xD3DCEAFF),
        bots,
        shadows: ShadowColors {
            float: [p(0x1F23291F, 0xFFFFFF1F), p(0x0F111414, 0x00000059)],
            overlay: [
                p(0x1F232924, 0xFFFFFF29),
                p(0x0F111438, 0x000000B3),
                p(0x0F11140F, 0x00000000),
            ],
        },
        dark,
        panel: content,
        window_bg: content,
        group: content,
        elevated: overlay,
        fill_hover: hover,
        fill_selected: selected,
        fill_pressed: pressed,
        fill_field: field,
        fill_active: selected,
        control_border: border,
        control_border_focus: focus_border,
        quote_bar: border_strong,
        code_text: text_1,
        warning: you_glyph,
        new_divider: danger,
        glyph_progress: accent,
        glyph_wait: you_glyph,
        glyph_done: success,
        background: content,
        foreground: text_1,
        muted_foreground: text_2,
        row_hover: hover,
        offline: text_3,
        code_background: code_bg,
        search_highlight: hl_bg,
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

/// The tone of a status mark (§7.8).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChipKind {
    /// No fill, `text-3` (or `text-2`) text: 完成, 等{name}…, 等审, 等回答.
    Quiet,
    /// `you` tone: 等你回答, 等你审.
    You,
    /// `danger-text`: 卡住了.
    Blocked,
    /// `info` tone: 进行中 in the topic header.
    Info,
}

/// Background, text and glyph colors of a status mark.
pub fn chip_colors(kind: ChipKind, status: TopicStatus, cx: &App) -> ChipColors {
    let k = colors(cx);
    let clear = c(0x00000000);
    match kind {
        ChipKind::You => ChipColors {
            bg: k.you_bg,
            text: k.you_text,
            ring: clear,
            glyph: k.you_glyph,
        },
        ChipKind::Blocked => ChipColors {
            bg: clear,
            text: k.danger_text,
            ring: clear,
            glyph: k.danger_text,
        },
        ChipKind::Info => ChipColors {
            bg: k.info_bg,
            text: k.info_text,
            ring: clear,
            glyph: k.info_text,
        },
        ChipKind::Quiet => ChipColors {
            bg: clear,
            text: k.text_3,
            ring: clear,
            glyph: match status {
                TopicStatus::WaitingAnswer | TopicStatus::WaitingReview => k.you_glyph,
                _ => k.text_3,
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

/// Background and text colors of a topic status mark. Kept for older
/// callers; new code uses `ui::StatusChip`.
pub fn status_chip_colors(status: TopicStatus, cx: &App) -> (Hsla, Hsla) {
    let kind = match status {
        TopicStatus::Blocked => ChipKind::Blocked,
        TopicStatus::InProgress => ChipKind::Info,
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
    fn text_meets_wcag_aa() {
        for k in [build(false), build(true)] {
            assert!(contrast(k.text_1, k.content) >= 12.0);
            assert!(contrast(k.text_2, k.content) >= 4.5);
            assert!(contrast(k.text_2, k.sidebar) >= 4.5);
            assert!(contrast(k.text_3, k.content) >= 4.5);
            assert!(contrast(k.danger_text, k.content) >= 4.5);
            assert!(contrast(k.accent_text, k.content) >= 4.5);
            assert!(contrast(k.you_text, k.you_bg) >= 4.5);
            assert!(contrast(k.on_accent, k.accent) >= 4.5);
            assert!(contrast(k.on_accent, k.count_solid) >= 4.5);
            assert!(contrast(k.avatar_fg, k.avatar_bg) >= 4.5);
        }
    }

    #[test]
    fn dark_surfaces_step_up() {
        let d = build(true);
        let l = |h: Hsla| h.l;
        assert!(l(d.sidebar) < l(d.content));
        assert!(l(d.content) < l(d.raised));
        assert!(l(d.raised) < l(d.overlay));
    }

    #[test]
    fn bot_tints_differ_for_the_roster() {
        // DESIGN.md §2.6: this seed gives the six roster bots six hues.
        // Pocket, Forge, Watch, Scout, Ledger, Taro.
        let mut tints = ["o05", "o08", "o11", "o07", "h02", "h03"].map(bot_tint_index);
        tints.sort();
        assert_eq!(tints, [0, 1, 2, 3, 4, 5]);
    }
}
