//! The one theme module (docs/DESIGN.md §2 to §10). It follows the system
//! light or dark mode, loads our values into gpui-component's theme, and gives
//! views the app's semantic tokens: colors, type, spacing, radii, elevation
//! and motion.
//!
//! Views read colors with `theme::colors(cx)`, sizes from the constants below
//! and text styles through [`Type`]. Never write a color, size or duration
//! literal in a view: add a token here so light and dark both work.

use std::{
    sync::{
        Arc, OnceLock,
        atomic::{AtomicBool, Ordering},
    },
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
pub const TOPIC_LIST_MAX_WIDTH: Pixels = px(400.);
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
/// From this window width every column has its default width with the side
/// panel open: 248 + 320 + 480 + 300.
pub const SIDE_PANEL_MIN_WINDOW: Pixels = px(1348.);
pub const WINDOW_MIN_WIDTH: Pixels = px(1100.);
/// Below this window width an open side panel floats over the topic view:
/// 200 + 280 + 480 + 300.
pub const SIDE_PANEL_INLINE_MIN_WINDOW: Pixels = px(1260.);

/// The column widths of the main window (§4.4 "Narrow window").
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Columns {
    pub sidebar: Pixels,
    pub topic_list: Pixels,
    /// The open side panel floats over the topic view (level 2) instead of
    /// taking its own column.
    pub panel_floats: bool,
}

/// With the side panel open, the topic list shrinks to 280 and then the
/// sidebar to 200, so the topic view keeps 480; below
/// [`SIDE_PANEL_INLINE_MIN_WINDOW`] the panel floats over the topic view.
pub fn columns(window_width: Pixels, panel_open: bool) -> Columns {
    let full = Columns {
        sidebar: SIDEBAR_WIDTH,
        topic_list: TOPIC_LIST_WIDTH,
        panel_floats: false,
    };
    if !panel_open || window_width >= SIDE_PANEL_MIN_WINDOW {
        return full;
    }
    if window_width < SIDE_PANEL_INLINE_MIN_WINDOW {
        return Columns {
            panel_floats: true,
            ..full
        };
    }
    let room = window_width - TOPIC_VIEW_MIN_WIDTH - SIDE_PANEL_WIDTH;
    let topic_list = (room - SIDEBAR_WIDTH)
        .max(TOPIC_LIST_MIN_WIDTH)
        .min(TOPIC_LIST_WIDTH);
    let sidebar = (room - topic_list)
        .max(SIDEBAR_MIN_WIDTH)
        .min(SIDEBAR_WIDTH);
    Columns {
        sidebar,
        topic_list,
        panel_floats: false,
    }
}
// ---------------------------------------------------------------------------
// Layout v3 (the v2 redesign: rail | page | pane)

/// The left rail: title row, search, destinations, 话题 and the team strip.
pub const RAIL_WIDTH: Pixels = px(260.);
/// The title bar of every page: breadcrumb, title, state and pane buttons.
pub const TITLE_BAR: Pixels = px(52.);
/// The topic rail beside a 话题 page (the default pane).
pub const TOPIC_RAIL_WIDTH: Pixels = px(320.);
/// The 单独问 side chat pane (⌘;).
pub const SIDE_CHAT_WIDTH: Pixels = px(380.);
/// The 实时查看 pane: half the window, between these bounds (720 at 1440).
pub const LIVE_PANE_SHARE: f32 = 0.5;
pub const LIVE_PANE_MIN_WIDTH: Pixels = px(460.);
pub const LIVE_PANE_MAX_WIDTH: Pixels = px(760.);
/// The 产出 pane beside a 话题 page.
pub const ARTIFACT_PANE_WIDTH: Pixels = px(440.);
/// The 来龙去脉 rail of the 产出 page.
pub const ARTIFACT_RAIL_WIDTH: Pixels = px(320.);
/// The right column of the 首页 (正在做, 排队) and the 群 page (群里的决定).
pub const DESK_SIDE_WIDTH: Pixels = px(300.);
/// The list of the 成员 page.
pub const MEMBERS_LIST_WIDTH: Pixels = px(480.);
/// A page never gets narrower than this; a pane floats over it instead.
pub const PAGE_MIN_WIDTH: Pixels = px(480.);
/// The reading column of pages (首页 cards, the topic timeline).
pub const PAGE_MEASURE: Pixels = px(720.);
/// Title bar icon buttons (单独问, 产出, 电脑, 改动, rail toggle).
pub const TITLE_BUTTON: Pixels = px(28.);
/// Rail rows (destinations and 话题): 28 pt; group headers 24 pt.
pub const RAIL_ROW: Pixels = px(28.);
pub const RAIL_GROUP_ROW: Pixels = px(24.);
/// The slot at the start of a rail row (state dot, tile, check).
pub const RAIL_SLOT: Pixels = px(16.);
/// The state set of a rail row, one visual size (§7.3): a 6 pt dot (filled
/// `you` for 需要你, hollow `ink-3` for 等别人), a 12 pt `ink-3` check for
/// 做完了, the doing member's tile with its status dot for 在做.
pub const RAIL_STATE_DOT: Pixels = px(6.);
pub const RAIL_STATE_STROKE: Pixels = px(1.25);
pub const RAIL_STATE_CHECK: Pixels = px(12.);
/// The 岗位 tag after a member's name: 18 pt, radius 5.
pub const TAG_HEIGHT: Pixels = px(18.);
pub const TAG_RADIUS: Pixels = px(5.);
/// The status dot on an avatar's corner (working, waits on me): 6 pt on a
/// 16 pt avatar, 7 up to 24, 8 up to 32, 10 above (§7.9).
pub fn avatar_status_dot(avatar: Pixels) -> Pixels {
    if avatar <= px(16.) {
        px(6.)
    } else if avatar <= px(24.) {
        px(7.)
    } else if avatar <= px(32.) {
        px(8.)
    } else {
        px(10.)
    }
}
/// The ring in the surface color that cuts the status dot out of the avatar.
pub const AVATAR_STATUS_CUT: Pixels = px(1.5);
/// A right-pane rail section (topic rail, 正在做, 群资料, 来龙去脉, 工作环境):
/// no border, fill or shadow; sections sit on the rail surface and a 1 pt
/// `line` divides them (§7.8). Its vertical and side padding.
pub const RAIL_SECTION_PAD_Y: Pixels = px(14.);
pub const RAIL_SECTION_PAD_X: Pixels = px(4.);
/// How far the hover fill of a row in a boxless list (refs, links) reaches
/// past the text on each side (`ui::inset_row`).
pub const LIST_ROW_BLEED: Pixels = px(8.);

/// The width of the 实时查看 pane in a window `window_w` wide.
pub fn live_pane_width(window_w: Pixels) -> Pixels {
    (window_w * LIVE_PANE_SHARE).clamp(LIVE_PANE_MIN_WIDTH, LIVE_PANE_MAX_WIDTH)
}

pub const WINDOW_MIN_HEIGHT: Pixels = px(680.);
pub const WINDOW_DEFAULT_WIDTH: Pixels = px(1440.);
pub const WINDOW_DEFAULT_HEIGHT: Pixels = px(900.);
/// Traffic light origin: their center lands on y 26, the title row center.
pub const TRAFFIC_LIGHTS: (Pixels, Pixels) = (px(20.), px(20.));
/// Leading inset of a toolbar band that holds the traffic lights.
pub const TRAFFIC_LIGHTS_INSET: Pixels = px(84.);
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
/// Height of the fade at the cut edge of an inline 产出 page (MEDIA §1 inline HTML).
pub const EMBED_FADE: Pixels = px(48.);

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
    /// Card actions (`size::BUTTON_CARD`): every kind shares it.
    pub const BUTTON_CARD: Pixels = px(7.);

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
    /// Card actions (Card v2 footers, 首页 asks, the pinned card): one height
    /// for primary, quiet and text actions (§7.5).
    pub const BUTTON_CARD: Pixels = px(28.);
    /// Side padding of a card action.
    pub const BUTTON_CARD_PAD: Pixels = px(12.);
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
    /// Composer: 12 top padding + 22 text (one line, it grows with the
    /// text) + 40 toolbar.
    pub const COMPOSER_MIN: Pixels = px(74.);
    pub const COMPOSER_TEXT_MIN: Pixels = px(22.);
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
    /// A settings list row (§8.10): the label at the left, its control at
    /// the right, inside one rounded group.
    pub const SETTINGS_LIST_ROW: Pixels = px(52.);
    /// The text field of a settings list row: fixed, so fields do not
    /// stretch across the window.
    pub const SETTINGS_FIELD_WIDTH: Pixels = px(280.);
    /// The longest value at the right of a settings list row (服务器).
    pub const SETTINGS_VALUE_MAX: Pixels = px(320.);
    /// The settings content column: 600 pt of content plus 32 pt each side.
    pub const SETTINGS_COLUMN: Pixels = px(664.);
    /// The picture in the 头像 row.
    pub const SETTINGS_AVATAR: Pixels = px(40.);
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

/// 设置 → 外观 → 主题: follow the system, or keep light or dark.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum ThemeChoice {
    #[default]
    System,
    Light,
    Dark,
}

impl ThemeChoice {
    pub const ALL: [ThemeChoice; 3] = [ThemeChoice::System, ThemeChoice::Light, ThemeChoice::Dark];

    /// The name in `layout.json`.
    pub fn slug(self) -> &'static str {
        match self {
            ThemeChoice::System => "system",
            ThemeChoice::Light => "light",
            ThemeChoice::Dark => "dark",
        }
    }

    pub fn from_slug(slug: &str) -> Option<ThemeChoice> {
        ThemeChoice::ALL.into_iter().find(|c| c.slug() == slug)
    }

    fn mode(self, appearance: ThemeMode) -> ThemeMode {
        match self {
            ThemeChoice::System => appearance,
            ThemeChoice::Light => ThemeMode::Light,
            ThemeChoice::Dark => ThemeMode::Dark,
        }
    }
}

/// The theme choice of this app run (`layout.json`, app only).
static THEME_CHOICE: std::sync::atomic::AtomicU8 = std::sync::atomic::AtomicU8::new(0);

pub fn theme_choice() -> ThemeChoice {
    match THEME_CHOICE.load(Ordering::Relaxed) {
        1 => ThemeChoice::Light,
        2 => ThemeChoice::Dark,
        _ => ThemeChoice::System,
    }
}

/// Sets the theme choice for the windows that follow the system. The caller
/// saves it and repaints with [`apply_theme_choice`].
pub fn set_theme_choice(choice: ThemeChoice) {
    let v = match choice {
        ThemeChoice::System => 0,
        ThemeChoice::Light => 1,
        ThemeChoice::Dark => 2,
    };
    THEME_CHOICE.store(v, Ordering::Relaxed);
}

/// Applies the theme choice in `window` now.
pub fn apply_theme_choice(window: &mut Window, cx: &mut App) {
    set_mode(theme_choice().mode(window.appearance().into()), cx);
    cx.refresh_windows();
}

/// Sets the theme from the window's appearance and keeps it in step when the
/// user switches light or dark mode. Keep the returned subscription alive.
/// A theme choice other than 跟随系统 wins over the appearance.
pub fn follow_system(window: &mut Window, cx: &mut App) -> Subscription {
    let appearance = window.appearance();
    set_mode(theme_choice().mode(appearance.into()), cx);
    window.observe_window_appearance(|window, cx| {
        set_mode(theme_choice().mode(window.appearance().into()), cx);
    })
}

/// Switches to `mode` and loads our values over gpui-component's defaults.
/// `Theme::change` reloads the registered config, so the overrides are applied
/// again after every change (DESIGN.md §18). The self-test uses this too.
pub fn set_mode(mode: ThemeMode, cx: &mut App) {
    Theme::change(mode, None, cx);
    crate::ui::motion::sync_accessibility(cx);
    let dark = mode.is_dark();
    Theme::update(cx, |theme| apply(theme, dark));
}

/// Applies a change of the accessibility display options (Increase
/// contrast, Reduce motion) while the app runs: the colors are loaded again
/// and every window repaints.
pub fn refresh_accessibility(cx: &mut App) {
    let dark = is_dark(cx);
    Theme::update(cx, |theme| apply(theme, dark));
    cx.refresh_windows();
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
    t.selection = p(0x3B6EF533, 0x6B93F75C);
    t.caret = k.ink;
    t.overlay = k.scrim;
    t.accent = k.hover;
    t.accent_foreground = k.text_1;

    t.primary = k.accent;
    t.primary_hover = k.accent_hover;
    t.primary_active = k.accent_pressed;
    t.primary_foreground = k.on_accent;
    t.button_primary = k.accent;
    t.button_primary_hover = k.accent_hover;
    t.button_primary_active = k.accent_pressed;
    t.button_primary_foreground = k.on_accent;

    let (sec, sec_h, sec_a) = (k.bg, k.hover, k.pressed);
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
    t.danger_foreground = c(0xFFFFFFFF);
    t.button_danger = t.danger;
    t.button_danger_hover = t.danger_hover;
    t.button_danger_active = t.danger_active;
    t.button_danger_foreground = c(0xFFFFFFFF);
    t.success = k.success;
    t.success_hover = k.success;
    t.success_active = k.success;
    t.success_foreground = c(0xFFFFFFFF);
    t.warning = k.you_glyph;
    t.warning_hover = k.you_glyph;
    t.warning_active = k.you_glyph;
    t.warning_foreground = c(0xFFFFFFFF);
    t.info = k.live;
    t.info_hover = k.live;
    t.info_active = k.live_ink;
    t.info_foreground = c(0xFFFFFFFF);

    t.colors.list = k.content;
    t.list_hover = k.hover;
    t.list_active = k.sel;
    t.list_active_border = clear;
    t.list_even = k.content;
    t.list_head = k.content;

    t.table = k.content;
    t.table_hover = k.hover;
    t.table_active = k.sel;
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
    t.sidebar_accent = k.sel;
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
    t.slider_bar = k.ink;
    t.slider_thumb = c(0xFFFFFFFF);
    t.progress_bar = k.live;
    t.skeleton = k.field;

    t.scrollbar = clear;
    t.scrollbar_thumb = p(0x11111340, 0xFFFFFF40);
    t.scrollbar_thumb_hover = p(0x11111366, 0xFFFFFF66);

    t.link = k.accent_text;
    t.link_hover = p(0x1C45B8FF, 0xAFC6FCFF);
    t.link_active = p(0x163A9EFF, 0xC4D5FDFF);

    t.blue = k.live;
    t.blue_light = k.info_bg;
    t.yellow = k.you_text;
    t.yellow_light = k.you_bg;
    t.red = k.danger_text;
    t.red_light = k.danger_bg;
    t.green = k.success_text;
    t.green_light = p(0x1F9D551F, 0x3CC27A29);
    t.magenta = t.blue;
    t.magenta_light = t.blue_light;
    t.cyan = t.blue;
    t.cyan_light = t.blue_light;

    t.chart_1 = k.ink;
    t.chart_2 = k.success;
    t.chart_3 = k.you_glyph;
    t.chart_4 = k.text_2;
    t.chart_5 = k.text_3;
    t.window_border = k.border;
    t.drag_border = k.live;
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
    /// 17 / 24: v3 page titles in the title bar.
    pub const PAGE_TITLE: (Pixels, Pixels) = (px(17.), px(24.));
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
    /// 17 / 24, 600: the title bar of 首页, 产出, 成员 and 群 pages (v3).
    fn t_page_title(self) -> Self {
        self.type_role(type_scale::PAGE_TITLE)
            .font_weight(FontWeight::SEMIBOLD)
    }
    /// 15 / 22, 600: the title bar of a 话题 page (v3).
    fn t_bar_title(self) -> Self {
        self.type_role(type_scale::CARD_TITLE)
            .font_weight(FontWeight::SEMIBOLD)
    }
    /// 12 / 16 figures: counts, times, sizes, versions and costs (`.mono`
    /// in the artboards). The system font with tabular figures, so columns
    /// of numbers line up without reading like a terminal (§3).
    fn t_mono(self) -> Self {
        self.type_role(type_scale::META).figures()
    }
    /// Tabular figures in the system font at the inherited size: times,
    /// counts, versions and costs (§3). Real monospace is [`Type::code_font`].
    fn mono(self) -> Self {
        self.figures()
    }
    /// The system font with tabular figures (`tnum`), set explicitly so a
    /// figure inside a code run still reads as a number.
    fn figures(self) -> Self {
        self.font_family(FONT_FAMILY).font_features(tabular())
    }
    /// Real monospace at the inherited size: code, diffs, terminal lines,
    /// ids and hashes, host names (§3).
    fn code_font(self) -> Self {
        self.font_family(MONO_FONT_FAMILY).font_features(tabular())
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
    /// Frame cap of decorative loops (pending dots): each frame re-renders
    /// the view that holds the loop.
    pub const DECORATIVE_FPS: f32 = 24.0;
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

    // v3 motion (CONCEPT2 §5). Loops only for live signals; never animate
    // selection, keyboard navigation, screen switches, counts or the theme.

    /// First paint: cards and rows rise in one after another this far apart.
    pub const STAGGER: Duration = Duration::from_millis(45);
    /// At most this many items take part in a stagger; the rest come with
    /// the last one, so a long list never waits.
    pub const STAGGER_MAX: usize = 8;
    /// The rise of a card or row on first paint (`tc-rise`).
    pub const RISE_IN: Duration = Duration::from_millis(360);
    /// Side chat and panes enter: opacity plus [`PANE_TRAVEL`] from the right.
    pub const PANE_ENTER: Duration = Duration::from_millis(200);
    pub const PANE_TRAVEL: gpui::Pixels = gpui::px(12.);
    /// One breath of a live ring or a current plan dot (`tc-breathe`).
    pub const BREATHE: Duration = Duration::from_millis(2400);
    /// The opacity a breathing ring swings between.
    pub const BREATHE_HIGH: f32 = 0.8;
    pub const BREATHE_LOW: f32 = 0.25;
    /// The check of an answered card draws in (`tc-draw`).
    pub const CHECK_DRAW: Duration = Duration::from_millis(300);

    // panes track (1.1.0): 实时查看.
    /// The terminal cursor of 实时查看 blinks once this often (`tc-blink`).
    pub const LIVE_CURSOR_BLINK: Duration = Duration::from_millis(1100);
    /// The 回放 track fills in once (`tc-fill`).
    pub const LIVE_TRACK_FILL: Duration = Duration::from_millis(600);
    /// Chart bars grow from zero once.
    pub const BAR_GROW: Duration = Duration::from_millis(400);
    /// The blink half-period of a live terminal cursor (`tc-blink`).
    pub const CURSOR_BLINK: Duration = Duration::from_millis(500);

    // dfix-motion (1.1.0): chart lines, and what counts as just answered.
    /// A chart line draws in once (§12: 600–700 ms).
    pub const LINE_DRAW: Duration = Duration::from_millis(650);
    /// Each further chart series starts this much later.
    pub const SERIES_STAGGER: Duration = Duration::from_millis(50);
    /// An answer this recent draws its check; older ones show it still.
    pub const JUST_ANSWERED: Duration = Duration::from_secs(4);
}

// ---------------------------------------------------------------------------
// Colors (§2)

/// Semantic colors of the app, one value per mode (DESIGN.md §2). Field names
/// follow the token names with `_` for `-`.
#[derive(Clone, Copy, Debug)]
pub struct Colors {
    // v3 tokens (HELMET2): one field per CSS custom property, `_` for `-`.
    /// Page background (`--bg`).
    pub bg: Hsla,
    /// The left rail and the topic rail (`--rail`).
    pub rail: Hsla,
    /// Inset fills: tags, kbd, search keys, quiet blocks (`--sunken`).
    pub sunken: Hsla,
    pub ink: Hsla,
    pub ink_2: Hsla,
    pub ink_3: Hsla,
    pub line: Hsla,
    pub line_2: Hsla,
    /// Selection: grey, never an accent tint (`--sel`).
    pub sel: Hsla,
    /// The selected row of the left rail: a lighter grey than `sel`, so the
    /// current 话题 reads as a soft highlight, not a block (§7.3).
    pub rail_sel: Hsla,
    /// Hover of a left rail row, a step under `rail_sel`.
    pub rail_hover: Hsla,
    /// Primary buttons: ink fill with `btn_ink` text (`--btn`).
    pub btn: Hsla,
    pub btn_ink: Hsla,
    /// The quiet card button (`ButtonKind::Quiet`): a soft fill, no border,
    /// and its hover and pressed fills (`--btn-quiet`).
    pub btn_quiet: Hsla,
    pub btn_quiet_hover: Hsla,
    pub btn_quiet_pressed: Hsla,
    /// State: amber means "you" (needs your decision), only that.
    pub you: Hsla,
    pub you_ink: Hsla,
    /// State: blue means "live" (a member works now).
    pub live: Hsla,
    pub live_ink: Hsla,
    pub ok: Hsla,
    pub ok_ink: Hsla,
    /// State: red means a problem.
    pub bad: Hsla,
    pub bad_ink: Hsla,
    /// A person's circle without a photo (`--person`).
    pub person: Hsla,
    /// The live screen frame (`--screen`, `--screen-ink`, `--screen-dim`).
    pub screen: Hsla,
    pub screen_ink: Hsla,
    pub screen_dim: Hsla,
    /// The initial on a bot tile (`--tile-ink`).
    pub tile_ink: Hsla,
    /// Placeholder tints of thumbnails (`--ph-1..3`).
    pub ph: [Hsla; 3],
    /// Named member tints (`--c-jojo` .. `--c-scout`), see [`MEMBER_TINTS`].
    tints: [Hsla; 8],
    v3_shadows: V3Shadows,

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

/// The v3 shadow colors (`--lift`, `--btn-hi`, `--sec-hi`, `--tile-edge`,
/// `--pop`). `None` where the token is `none` in that mode.
#[derive(Clone, Copy, Debug)]
struct V3Shadows {
    lift: Option<Hsla>,
    /// Inset highlight of a primary button, and its y offset (1 light, -1 dark).
    btn_hi: (Hsla, f32),
    sec_hi: Option<Hsla>,
    tile_edge: Hsla,
    pop: [Hsla; 2],
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

    /// The tint of a bot tile. v3: the named member tint of `key` (see
    /// [`Colors::member_tint`]). The v2.1 six-hue hash stays in
    /// [`Colors::legacy_bot_tint`].
    pub fn bot_tint(&self, key: &str) -> Hsla {
        self.member_tint(key)
    }

    /// The v2.1 tint of a bot tile: one of six by a stable hash.
    pub fn legacy_bot_tint(&self, key: &str) -> Hsla {
        self.bots[bot_tint_index(key)]
    }

    /// The named tint of a member (`--c-<name>`): `key` is a member name, a
    /// roster character id or any other id. Roster names and their
    /// characters get their own tint; anything else one of the eight by a
    /// stable hash.
    pub fn member_tint(&self, key: &str) -> Hsla {
        self.tints[member_tint_index(key)]
    }

    /// `--lift`: the 1 pt drop under cards and rail cards (none in dark).
    pub fn lift(&self) -> Vec<BoxShadow> {
        self.v3_shadows
            .lift
            .map(|c| vec![shadow(c, 0., 1., 2., 0., false)])
            .unwrap_or_default()
    }

    /// `--btn-hi`: the inner highlight of a primary button.
    pub fn btn_hi(&self) -> Vec<BoxShadow> {
        let (c, y) = self.v3_shadows.btn_hi;
        vec![shadow(c, 0., y, 0., 0., true)]
    }

    /// `--sec-hi`: the hairline drop under a secondary button (none in dark).
    pub fn sec_hi(&self) -> Vec<BoxShadow> {
        self.v3_shadows
            .sec_hi
            .map(|c| vec![shadow(c, 0., 1., 1., 0., false)])
            .unwrap_or_default()
    }

    /// `--tile-edge`: the 1 pt inner edge of every bot tile.
    pub fn tile_edge(&self) -> Vec<BoxShadow> {
        vec![shadow(self.v3_shadows.tile_edge, 0., 0., 0., 1., true)]
    }

    /// `--pop`: popovers and the 交代或搜索 dialog.
    pub fn pop(&self) -> Vec<BoxShadow> {
        let [a, b] = self.v3_shadows.pop;
        if self.dark {
            vec![
                shadow(a, 0., 18., 48., 0., false),
                shadow(b, 0., 0., 0., 1., false),
            ]
        } else {
            vec![
                shadow(a, 0., 18., 48., 0., false),
                shadow(b, 0., 2., 8., 0., false),
            ]
        }
    }

    /// The (mark, text) colors of a state: amber you, blue live, red
    /// problem, green done, grey quiet.
    pub fn state(&self, state: StateTone) -> (Hsla, Hsla) {
        match state {
            StateTone::You => (self.you, self.you_ink),
            StateTone::Live => (self.live, self.live_ink),
            StateTone::Problem => (self.bad, self.bad_ink),
            StateTone::Done => (self.ok, self.ok_ink),
            StateTone::Quiet => (self.ink_3, self.ink_3),
        }
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

/// The closed set of state colors (v3): one meaning each.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum StateTone {
    /// Amber: needs you (a decision, a review, an answer).
    You,
    /// Blue: a member works on it now.
    Live,
    /// Red: something went wrong.
    Problem,
    /// Green: done.
    Done,
    /// Grey: waits on someone else, idle, or nothing to say.
    Quiet,
}

/// The names of the eight member tints, in `--c-*` order. A member whose
/// name or roster character matches one of these gets that tint.
pub const MEMBER_TINTS: [&str; 8] = [
    "jojo", "watch", "taro", "forge", "tata", "ledger", "pocket", "scout",
];

/// The index into [`MEMBER_TINTS`] for a member name, a roster character id
/// (`o08` is Forge) or any other key (a stable hash).
pub fn member_tint_index(key: &str) -> usize {
    let lower = key.trim().to_lowercase();
    if let Some(i) = MEMBER_TINTS.iter().position(|n| *n == lower) {
        return i;
    }
    // Roster characters (assets/bots/roster.json): the tint of the name.
    const CHARACTERS: [(&str, usize); 7] = [
        ("o11", 1),
        ("h03", 2),
        ("o08", 3),
        ("o03", 4),
        ("h02", 5),
        ("o05", 6),
        ("o07", 7),
    ];
    if let Some((_, i)) = CHARACTERS.iter().find(|(c, _)| *c == lower) {
        return *i;
    }
    let mut hash: u32 = 0x811c_9dc5;
    for byte in lower.bytes() {
        hash ^= byte as u32;
        hash = hash.wrapping_mul(0x0100_0193);
    }
    hash as usize % MEMBER_TINTS.len()
}

fn shadow(color: Hsla, x: f32, y: f32, blur: f32, spread: f32, inset: bool) -> BoxShadow {
    BoxShadow {
        color,
        offset: point(px(x), px(y)),
        blur_radius: px(blur),
        spread_radius: px(spread),
        inset,
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

/// The v3 colors (HELMET2.html, the accepted v2 artboards). The v2.1 field
/// names are kept and point at the v3 tokens, so views written before v3
/// build and take the new look: ink primary buttons, grey selection, rail
/// and sunken surfaces, amber only for "you", blue only for "live".
fn build(dark: bool) -> Colors {
    let p = |l: u32, d: u32| if dark { c(d) } else { c(l) };
    // --- v3 tokens ---
    let bg = p(0xFFFFFFFF, 0x111113FF);
    let rail = p(0xF7F7F8FF, 0x161618FF);
    let sunken = p(0xF3F3F4FF, 0x1C1C1FFF);
    let raised = p(0xFAFAFAFF, 0x151517FF);
    let ink = p(0x111113FF, 0xF1F1F3FF);
    let ink_2 = p(0x52525BFF, 0xA9A9B2FF);
    let ink_3 = p(0x6E6E76FF, 0x8C8C95FF);
    let line = p(0xEBEBEDFF, 0x252529FF);
    let line_2 = p(0xDCDCE0FF, 0x34343AFF);
    let hover = p(0xF0F0F2FF, 0x1D1D20FF);
    let sel = p(0xE9E9ECFF, 0x26262AFF);
    let rail_sel = p(0xEDEDF0FF, 0x222226FF);
    let rail_hover = p(0xF2F2F4FF, 0x1C1C1FFF);
    let btn = p(0x111113FF, 0xF1F1F3FF);
    let btn_ink = p(0xFFFFFFFF, 0x111113FF);
    let you = p(0xE8590CFF, 0xF27A2EFF);
    let you_ink = p(0xB54708FF, 0xF7A26AFF);
    let you_bg = p(0xFFF4ECFF, 0x2B1C12FF);
    let live = p(0x3B6EF5FF, 0x6B93F7FF);
    let live_ink = p(0x2453D1FF, 0x93B1FAFF);
    let ok = p(0x1F9D55FF, 0x3CC27AFF);
    let ok_ink = p(0x177245FF, 0x6AD69CFF);
    let bad = p(0xE5484DFF, 0xF2555AFF);
    let bad_ink = p(0xC42B30FF, 0xFF8A8EFF);
    let person = p(0xE6E6E9FF, 0x2C2C31FF);
    let tints = if dark {
        [
            0xCFC39EFF, 0xB4C6C9FF, 0xCAB9C6FF, 0xD0BDB0FF, 0xB8C1D2FF, 0xCDB9BEFF, 0xC0BED0FF,
            0xB7C9BCFF,
        ]
    } else {
        [
            0xF3E6BFFF, 0xD8ECEFFF, 0xEFDDEBFF, 0xF6E1D3FF, 0xDCE5F7FF, 0xF4DDE3FF, 0xE4E2F4FF,
            0xDCEFE2FF,
        ]
    }
    .map(c);
    // Translucent tints of the state colors for fills (selection of text,
    // mentions, focus halos, soft chips).
    let live_a = |a: u32| p(0x3B6EF500 | a, 0x6B93F700 | a);
    let you_a = |a: u32| p(0xE8590C00 | a, 0xF27A2E00 | a);
    let bad_a = |a: u32| p(0xE5484D00 | a, 0xF2555A00 | a);
    let overlay = p(0xFFFFFFFF, 0x1C1C1FFF);
    let pressed = p(0xE4E4E8FF, 0x2A2A2FFF);
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
    let accent = btn;
    let accent_text = live_ink;
    let focus_border = live;
    Colors {
        bg,
        rail,
        sunken,
        ink,
        ink_2,
        ink_3,
        line,
        line_2,
        sel,
        rail_sel,
        rail_hover,
        btn,
        btn_ink,
        btn_quiet: p(0xF1F1F3FF, 0x222226FF),
        btn_quiet_hover: p(0xE8E8EBFF, 0x2A2A2FFF),
        btn_quiet_pressed: p(0xE1E1E5FF, 0x313137FF),
        you,
        you_ink,
        live,
        live_ink,
        ok,
        ok_ink,
        bad,
        bad_ink,
        person,
        screen: p(0x16161AFF, 0x0B0B0DFF),
        screen_ink: c(0xC9C9D1FF),
        screen_dim: p(0x7A7A84FF, 0x6E6E78FF),
        tile_ink: p(0x2D3272FF, 0x232858FF),
        ph: [
            p(0xD9D3C7FF, 0x4A463FFF),
            p(0xC9D5D3FF, 0x3D4645FF),
            p(0xE2D2C4FF, 0x4E463FFF),
        ],
        tints,
        v3_shadows: V3Shadows {
            lift: (!dark).then(|| c(0x1111130D)),
            btn_hi: if dark {
                (c(0x00000024), -1.)
            } else {
                (c(0xFFFFFF29), 1.)
            },
            sec_hi: (!dark).then(|| c(0x1111130D)),
            tile_edge: p(0x11111312, 0x00000033),
            pop: if dark {
                [c(0x0000008C), c(0xFFFFFF0F)]
            } else {
                [c(0x11111329), c(0x1111130F)]
            },
        },

        // --- v2.1 names on v3 tokens ---
        sidebar: rail,
        content: bg,
        raised,
        overlay,
        scrim: p(0x1111132E, 0x0000008C),
        code_bg: sunken,
        text_1: ink,
        text_2: ink_2,
        text_3: ink_3,
        placeholder: p(0xA1A1A9FF, 0x6C6C75FF),
        sep: line,
        border: line,
        border_strong: line_2,
        hover,
        selected: sel,
        sidebar_hover: hover,
        sidebar_selected: sel,
        sel_glyph: ink,
        pressed,
        field: sunken,
        count_bg: sunken,
        count_text: ink_2,
        count_solid: ink_3,
        tile_ring: p(0xFFFFFFFF, 0xFFFFFF1A),
        opt_bg: sunken,
        opt_hover: hover,
        disabled_bg: p(0x11111314, 0xFFFFFF0F),
        disabled_text: p(0x8A8A92FF, 0x8A8A92FF),
        tile: sunken,
        desk: sunken,
        accent,
        accent_hover: p(0x2A2A2EFF, 0xDCDCE0FF),
        accent_pressed: p(0x000000FF, 0xC9C9CFFF),
        accent_text,
        accent_soft: live_a(0x1A),
        on_accent: btn_ink,
        focus: live_a(0x8C),
        focus_border,
        focus_soft: live_a(0x33),
        choice_bg: live_a(0x0F),
        mention_bg: live_a(0x1A),
        me_bg: you_a(0x24),
        me_text: you_ink,
        hl_bg: you_a(0x2E),
        unread_dot: live,
        you_bg,
        you_text: you_ink,
        you_glyph: you,
        you_bar: you,
        info_bg: live_a(0x1A),
        info_text: live_ink,
        danger: bad,
        danger_bg: bad_a(0x1F),
        danger_text: bad_ink,
        success: ok,
        success_text: ok_ink,
        badge: bad,
        online: ok,
        avatar_bg: person,
        avatar_fg: ink_2,
        bots,
        shadows: ShadowColors {
            float: [p(0x1111131F, 0xFFFFFF1F), p(0x11111314, 0x00000059)],
            overlay: [
                p(0x11111324, 0xFFFFFF14),
                p(0x11111329, 0x0000008C),
                p(0x1111130F, 0x00000000),
            ],
        },
        dark,
        panel: bg,
        window_bg: bg,
        group: bg,
        elevated: overlay,
        fill_hover: hover,
        fill_selected: sel,
        fill_pressed: pressed,
        fill_field: sunken,
        fill_active: sel,
        control_border: line,
        control_border_focus: focus_border,
        quote_bar: line_2,
        code_text: ink,
        warning: you,
        new_divider: bad,
        glyph_progress: live,
        glyph_wait: you,
        glyph_done: ok,
        background: bg,
        foreground: ink,
        muted_foreground: ink_2,
        row_hover: hover,
        offline: ink_3,
        code_background: sunken,
        search_highlight: you_a(0x2E),
    }
}

/// System Settings › Accessibility › Display › Increase contrast (§11).
static INCREASE_CONTRAST: AtomicBool = AtomicBool::new(false);

/// Sets the Increase contrast flag; `true` when it changed.
pub fn set_increase_contrast(on: bool) -> bool {
    INCREASE_CONTRAST.swap(on, Ordering::Relaxed) != on
}

/// With Increase contrast (§11): hairlines and borders use `border-strong`,
/// `text-3` becomes `text-2` (and the placeholder takes the old `text-3`).
/// Scrim and overlay are opaque already, so Reduce transparency needs no
/// change.
fn high_contrast(mut k: Colors) -> Colors {
    k.border = k.border_strong;
    k.sep = k.border_strong;
    k.placeholder = k.text_3;
    k.text_3 = k.text_2;
    // v3 names of the same tokens.
    k.line = k.line_2;
    k.control_border = k.line_2;
    k.ink_3 = k.ink_2;
    k.offline = k.ink_2;
    k
}

fn light_colors() -> &'static Colors {
    static L: OnceLock<Colors> = OnceLock::new();
    static H: OnceLock<Colors> = OnceLock::new();
    if INCREASE_CONTRAST.load(Ordering::Relaxed) {
        H.get_or_init(|| high_contrast(build(false)))
    } else {
        L.get_or_init(|| build(false))
    }
}

fn dark_colors() -> &'static Colors {
    static D: OnceLock<Colors> = OnceLock::new();
    static H: OnceLock<Colors> = OnceLock::new();
    if INCREASE_CONTRAST.load(Ordering::Relaxed) {
        H.get_or_init(|| high_contrast(build(true)))
    } else {
        D.get_or_init(|| build(true))
    }
}

// --- v11/inc-diagrams: diagrams render every theme id, not only the current one ---
/// Whether Increase contrast is on.
pub fn increase_contrast() -> bool {
    INCREASE_CONTRAST.load(Ordering::Relaxed)
}

/// The colors of one mode and contrast setting, whatever the current mode is.
pub fn colors_for(dark: bool, contrast: bool) -> Colors {
    match (dark, contrast) {
        (false, false) => build(false),
        (true, false) => build(true),
        (d, true) => high_contrast(build(d)),
    }
}
// --- end v11/inc-diagrams ---

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
pub fn chip_colors(kind: ChipKind, _status: TopicStatus, cx: &App) -> ChipColors {
    chip_palette(kind, &colors(cx))
}

fn chip_palette(kind: ChipKind, k: &Colors) -> ChipColors {
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
            // A label that waits on someone else stays grey (§7.8); only
            // the quiet 等你… label (kind `You`) draws `you-glyph`.
            glyph: k.text_3,
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

// ---------------------------------------------------------------------------
// Desk track (1.1.0 首页): appended block

/// The window dots of a live terminal thumbnail on the 首页 (close,
/// minimize, zoom), drawn on `screen` in both modes.
pub fn screen_dots() -> [Hsla; 3] {
    [c(0xFF5F57FF), c(0xFEBC2EFF), c(0x28C840FF)]
}

/// Desk terminal thumbnail: 10/12 mono lines, 5 apart (a miniature, not
/// reading text), and the time each line takes to scroll by.
pub mod desk_screen {
    use gpui::{Pixels, px};
    use std::time::Duration;
    pub const HEIGHT: Pixels = px(120.);
    pub const LINE: (Pixels, Pixels) = (px(10.), px(12.));
    pub const LINE_GAP: Pixels = px(5.);
    pub const TOP: Pixels = px(32.);
    pub const SCROLL_PER_LINE: Duration = Duration::from_millis(900);
}

// --- cards v2 track: Card v2 metrics and tints (DESIGN.md §7.10) ---

/// Sizes of Card v2 (Cards.dc.html). Colors stay in [`Colors`]; the state
/// tints of a card are `color-mix` of a state color into `bg`
/// ([`card_v2::mix`]).
pub mod card_v2 {
    use gpui::{Hsla, Pixels, px};

    /// The status strip at the top of a card.
    pub const STRIP: Pixels = px(34.);
    /// The card title: 16/24 600.
    pub const TITLE: (Pixels, Pixels) = (px(16.), px(24.));
    /// Widest card on the 首页 and in the gallery.
    pub const MAX_WIDTH: Pixels = px(680.);
    /// Header tile and the key letter tile of an option.
    pub const TILE: Pixels = px(20.);
    /// The least width of the group part of the header path (`群 / 话题`):
    /// one 12 pt character and an ellipsis.
    pub const PATH_GROUP_MIN: Pixels = px(24.);
    /// Option rows and buttons, form fields.
    pub const OPTION: Pixels = px(36.);
    pub const OPTION_RADIUS: Pixels = px(9.);
    /// Width of the label column of a form.
    pub const FORM_LABEL: Pixels = px(84.);
    /// Width of the key column of fields.
    pub const FIELD_KEY: Pixels = px(68.);
    /// Chips of a chips input.
    pub const CHIP: Pixels = px(30.);
    pub const CHIP_RADIUS: Pixels = px(7.);
    /// Proof chips (`✓ 24 项通过`).
    pub const PROOF: Pixels = px(26.);
    /// Footer buttons (`size::BUTTON_CARD`).
    pub const BUTTON: Pixels = super::size::BUTTON_CARD;
    /// The footer sits in the card, under the blocks: no band, no rule, no
    /// fill (one container level, §7.10). Its bottom padding.
    pub const FOOTER_BOTTOM: Pixels = px(14.);
    /// Plan rows.
    pub const PLAN_ROW: Pixels = px(28.);
    /// Reference rows and their glyph tile.
    pub const REF_ROW: Pixels = px(60.);
    pub const REF_TILE: Pixels = px(28.);
    /// Table rows (Cards.dc.html 06, Interact.dc.html 02: 32 pt).
    pub const TABLE_ROW: Pixels = px(32.);
    /// The least width of a number column; numbers sit right-aligned at the row end.
    pub const TABLE_NUM_COL: Pixels = px(52.);
    /// The bar in front of a percent cell, and its gap.
    pub const TABLE_PCT_BAR: Pixels = px(48.);
    /// Diff lines and the line-number columns.
    pub const DIFF_LINE: Pixels = px(20.);
    pub const DIFF_NUMBER: Pixels = px(30.);
    /// Chart plot height.
    pub const CHART: Pixels = px(100.);
    /// Evidence thumbnails.
    pub const THUMB: Pixels = px(84.);
    /// The live bar under a working strip.
    pub const LIVE_BAR: Pixels = px(2.);

    /// Strip fill and edge strengths of a state color mixed into `bg`.
    pub const YOU_FILL: f32 = 0.10;
    pub const YOU_EDGE: f32 = 0.24;
    pub const YOU_BORDER: f32 = 0.30;
    pub const OK_FILL: f32 = 0.09;
    pub const OK_EDGE: f32 = 0.22;
    pub const LIVE_FILL: f32 = 0.08;
    /// v11/r3-cards: the band of a rev a member's event caused (live over `sunken`).
    pub const UPDATED_FILL: f32 = 0.06;
    pub const LIVE_EDGE: f32 = 0.22;
    pub const QUIET_FILL: f32 = 0.09;
    pub const QUIET_EDGE: f32 = 0.16;
    /// Diff line fills.
    pub const DIFF_FILL: f32 = 0.10;
    /// The opacity of a muted card body (expired, superseded).
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

// --- end cards v2 track ---

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_side_panel_fits_every_window_width() {
        let c = |w: f32, open| columns(px(w), open);
        assert_eq!(c(1100., false).sidebar, SIDEBAR_WIDTH);
        assert_eq!(c(1440., true), c(1440., false));
        // 1308: only the topic list shrinks.
        let narrow = c(1308., true);
        assert_eq!(
            (narrow.sidebar, narrow.topic_list),
            (SIDEBAR_WIDTH, px(280.))
        );
        assert!(!narrow.panel_floats);
        // 1280: the sidebar shrinks too, and the topic view keeps 480.
        let narrower = c(1280., true);
        assert_eq!(narrower.topic_list, px(280.));
        assert_eq!(narrower.sidebar, px(220.));
        // Below 1260 the panel floats over the topic view.
        assert!(c(1259., true).panel_floats);
        assert!(c(WINDOW_MIN_WIDTH.into(), true).panel_floats);
    }

    #[test]
    fn increase_contrast_strengthens_lines_and_quiet_text() {
        for dark in [false, true] {
            let k = build(dark);
            let h = high_contrast(k);
            assert_eq!(h.border, k.border_strong);
            assert_eq!(h.sep, k.border_strong);
            assert_eq!(h.text_3, k.text_2);
            assert_eq!(h.placeholder, k.text_3);
            assert_eq!(h.text_1, k.text_1);
            assert_eq!(h.line, k.line_2);
            assert_eq!(h.ink_3, k.ink_2);
        }
    }

    #[test]
    fn disabled_fill_sits_above_every_surface_in_dark() {
        // Translucent white: it lightens content and overlay alike, so the
        // disabled primary never reads as a hole in a dialog (§2.3).
        let k = build(true);
        assert!(k.disabled_bg.a < 0.2);
        assert!(k.disabled_bg.l > 0.9);
    }

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
    fn only_you_marks_are_orange() {
        // DESIGN.md §7.8: 等{name}… / 等回答 / 等审 labels are grey.
        for k in [build(false), build(true)] {
            assert_eq!(chip_palette(ChipKind::Quiet, &k).glyph, k.text_3);
            assert_eq!(chip_palette(ChipKind::You, &k).glyph, k.you_glyph);
        }
    }

    #[test]
    fn dark_surfaces_sit_above_the_page() {
        // v3: the page is the darkest surface; rail, raised blocks, sunken
        // fills and popovers sit above it.
        let d = build(true);
        let l = |h: Hsla| h.l;
        assert!(l(d.bg) < l(d.raised));
        assert!(l(d.bg) < l(d.rail));
        assert!(l(d.rail) < l(d.sunken));
        assert!(l(d.bg) < l(d.overlay));
    }

    #[test]
    fn roster_members_wear_their_named_tint() {
        // Names and the roster characters they wear map to the same tint.
        for (name, character) in [
            ("Watch", "o11"),
            ("Taro", "h03"),
            ("Forge", "o08"),
            ("Tata", "o03"),
            ("Ledger", "h02"),
            ("Pocket", "o05"),
            ("Scout", "o07"),
        ] {
            assert_eq!(member_tint_index(name), member_tint_index(character));
        }
        let mut all: Vec<usize> = MEMBER_TINTS.iter().map(|n| member_tint_index(n)).collect();
        all.dedup();
        assert_eq!(all, (0..8).collect::<Vec<_>>());
        assert_eq!(member_tint_index("jojo"), 0);
        assert!(member_tint_index("m-xiaowang") < 8);
    }

    #[test]
    fn primary_is_ink_and_selection_is_grey() {
        for k in [build(false), build(true)] {
            assert_eq!(k.accent, k.btn);
            assert_eq!(k.btn, k.ink);
            assert_eq!(k.selected, k.sel);
            assert!(k.sel.s < 0.1, "selection has no hue");
            assert!(contrast(k.btn_ink, k.btn) >= 12.0);
        }
    }

    #[test]
    fn state_colors_keep_one_meaning_each() {
        for k in [build(false), build(true)] {
            assert_eq!(k.state(StateTone::You).0, k.you);
            assert_eq!(k.state(StateTone::Live).0, k.live);
            assert_eq!(k.state(StateTone::Problem).0, k.bad);
            assert!(contrast(k.you_ink, k.bg) >= 4.5);
            assert!(contrast(k.live_ink, k.bg) >= 4.5);
            assert!(contrast(k.bad_ink, k.bg) >= 4.5);
            assert!(contrast(k.ink_3, k.rail) >= 4.5);
        }
    }

    #[test]
    fn shadows_follow_the_mode() {
        assert_eq!(build(false).lift().len(), 1);
        assert!(build(true).lift().is_empty());
        assert!(build(true).sec_hi().is_empty());
        for k in [build(false), build(true)] {
            assert!(k.btn_hi()[0].inset);
            assert!(k.tile_edge()[0].inset);
        }
    }
}

// --- inc-cards21: Card v2.1 sizes and timings (CardsCompose.dc.html, INTERACT §4.4, §6.4) ---
pub mod card_v21 {
    use std::time::Duration;

    use gpui::{Pixels, px};

    /// Rank rows, slot cells, poll rows.
    pub const RANK_ROW: Pixels = px(40.);
    pub const SLOT_CELL: Pixels = px(52.);
    pub const POLL_ROW: Pixels = px(44.);
    /// Rating stars and scale cells.
    pub const STAR: Pixels = px(22.);
    pub const SCALE_CELL: Pixels = px(32.);
    /// Image choice and compare previews.
    pub const IMAGE: Pixels = px(150.);
    pub const PREVIEW: Pixels = px(120.);
    /// Matrix option columns.
    pub const MATRIX_COL: Pixels = px(72.);
    /// v11/r3-cards: a pickable bar chart as rows (Interact.dc.html 02): row height, the
    /// name column, the track height and radius, the value column, the row radius.
    pub const BAR_ROW: Pixels = px(30.);
    pub const BAR_NAME: Pixels = px(60.);
    pub const BAR_TRACK: Pixels = px(10.);
    pub const BAR_TRACK_RADIUS: Pixels = px(3.);
    pub const BAR_VALUE: Pixels = px(22.);
    pub const BAR_ROW_RADIUS: Pixels = px(7.);
    /// Inline page height range (CARD21 §8).
    pub const EMBED_MIN: f32 = 160.;
    pub const EMBED_MAX: f32 = 480.;
    pub const EMBED_DEFAULT: f32 = 320.;
    /// The amber tint of the decision surface of a non-blocking ask, and the live tint of
    /// a block a new rev changed.
    pub const SURFACE_FILL: f32 = 0.07;
    pub const CHANGE_FILL: f32 = 0.14;
    /// v11/r3-cards: the tint a block the rev inserted keeps after its flash (Interact 02).
    pub const INSERTED_FILL: f32 = 0.07;

    /// Pending (INTERACT §4.4): nothing shows before this.
    pub const PENDING_QUIET: Duration = Duration::from_millis(400);
    /// Soft and hard waits by `expect.mode` (INTERACT §6.4).
    pub const SOFT_LIVE: Duration = Duration::from_secs(8);
    pub const HARD_LIVE: Duration = Duration::from_secs(45);
    pub const SOFT_RUN: Duration = Duration::from_secs(20);
    pub const HARD_RUN: Duration = Duration::from_secs(120);
    pub const SOFT_STARTING: Duration = Duration::from_secs(45);
    pub const HARD_STARTING: Duration = Duration::from_secs(180);
    /// `expect.mode: offline` for a live event: the note shows this long.
    pub const OFFLINE_NOTE: Duration = Duration::from_secs(4);
    /// An `interaction.resolved` note shows this long under the block.
    pub const RESOLVED_NOTE: Duration = Duration::from_secs(5);
    /// Live debounce (INTERACT §3.2): text inputs and draft edits.
    pub const DEBOUNCE_TEXT: Duration = Duration::from_millis(600);
    pub const DEBOUNCE_DRAFT: Duration = Duration::from_millis(1000);
    /// A changed block fades in and its tint fades out.
    pub const CHANGE_IN: Duration = Duration::from_millis(200);
    pub const CHANGE_FLASH: Duration = Duration::from_millis(1600);
    /// 问这里 shows after the pointer rests this long (REFS §A3).
    pub const HOVER_ASK: Duration = Duration::from_millis(300);
    /// A part that was just added flashes `--sel`.
    pub const PART_FLASH: Duration = Duration::from_millis(600);
    // v11/pb-cards
    /// The column tint under a chosen and a hovered chart point (drawn under the series).
    pub const CHART_PICK_FILL: f32 = 0.06;
    pub const CHART_HOVER_FILL: f32 = 0.035;
    /// A block a new rev removed fades out over this.
    pub const REMOVE_OUT: Duration = Duration::from_millis(240);
}
// --- end inc-cards21 ---
