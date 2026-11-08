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
use crate::ui::native::tokens as tk;

// ---------------------------------------------------------------------------
// Layout: the widths and rows live in ui/native/tokens.rs (§2.1).

pub use crate::ui::native::tokens::layout::*;

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
// Layout v3 (rail | page | pane): the widths live in ui/native/tokens.rs.

/// The presence dot on an avatar's corner (§1.8): 8 on every avatar, 6 on
/// a 16 avatar so it stays inside the disc.
pub fn avatar_status_dot(avatar: Pixels) -> Pixels {
    if avatar <= px(16.) {
        px(6.)
    } else {
        crate::ui::native::tokens::avatar::PRESENCE
    }
}

/// The width of the 实时查看 pane in a window `window_w` wide.
pub fn live_pane_width(window_w: Pixels) -> Pixels {
    (window_w * LIVE_PANE_SHARE).clamp(LIVE_PANE_MIN_WIDTH, LIVE_PANE_MAX_WIDTH)
}

/// Spacing, radii, icon sizes and control heights: aliases onto the scales
/// of ui/native/tokens.rs (docs/design/COMPLETENESS.md §1).
pub use crate::ui::native::tokens::{icon, radius, size, space};

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
    set_app_appearance(choice);
}

/// Gives every window the system appearance of the choice, so what AppKit
/// draws itself (the window's edge and background, menus, the traffic
/// lights) matches the theme: a dark theme in a light system left a white
/// line along the top of the settings window. 跟随系统 hands it back.
#[cfg(target_os = "macos")]
fn set_app_appearance(choice: ThemeChoice) {
    use objc2::{
        MainThreadMarker, msg_send,
        runtime::{AnyClass, AnyObject},
    };
    use objc2_app_kit::NSApplication;
    use objc2_foundation::NSString;

    let Some(mtm) = MainThreadMarker::new() else {
        return;
    };
    let app = NSApplication::sharedApplication(mtm);
    let name = match choice {
        ThemeChoice::System => None,
        ThemeChoice::Light => Some("NSAppearanceNameAqua"),
        ThemeChoice::Dark => Some("NSAppearanceNameDarkAqua"),
    };
    // SAFETY: `+[NSAppearance appearanceNamed:]` returns a shared appearance
    // or nil; `setAppearance:` takes one or nil, on the main thread.
    unsafe {
        let appearance: *mut AnyObject = match (name, AnyClass::get(c"NSAppearance")) {
            (Some(name), Some(class)) => {
                let name = NSString::from_str(name);
                msg_send![class, appearanceNamed: &*name]
            }
            _ => std::ptr::null_mut(),
        };
        let _: () = msg_send![&app, setAppearance: appearance];
    }
}

#[cfg(not(target_os = "macos"))]
fn set_app_appearance(_choice: ThemeChoice) {}

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
    apply_menu_style(cx);
}

/// Applies a change of the accessibility display options (Increase
/// contrast, Reduce motion) while the app runs: the colors are loaded again
/// and every window repaints.
pub fn refresh_accessibility(cx: &mut App) {
    let dark = is_dark(cx);
    Theme::update(cx, |theme| apply(theme, dark));
    apply_menu_style(cx);
    cx.refresh_windows();
}

pub fn is_dark(cx: &App) -> bool {
    cx.theme().mode == ThemeMode::Dark
}

fn c(hex: u32) -> Hsla {
    rgba(hex).into()
}

// --- v13/tokens: every menu (COMPLETENESS.md §2.11) ---
/// The style of every gpui-component menu, dropdown and context menu
/// (§2.11): an opaque `card` surface, radius 8, p 4, the border line and
/// shadow md; items h32, px 8, radius 6, 14 text; highlight = `accent`
/// (muted) with foreground text (D10); separators 1 px, my 4.
pub fn apply_menu_style(cx: &mut App) {
    use crate::ui::native::{
        metrics,
        tokens::{Palette, shadow, text},
    };
    let p = Palette::for_mode(is_dark(cx));
    let mut s = shadow::md();
    s.push(shadow::line(p.border));
    gpui_component::menu::MenuStyle {
        fill: p.card,
        radius: metrics::MENU_RADIUS,
        shadow: s,
        padding: metrics::MENU_PAD,
        row_height: metrics::MENU_ROW,
        row_radius: metrics::MENU_ROW_RADIUS,
        row_padding: metrics::MENU_ROW_PAD,
        text_size: text::UI.0,
        line_height: text::UI.1,
        highlight: p.accent,
        highlight_text: p.foreground,
        separator: p.border,
        separator_height: BORDER,
        separator_margin: space::S4,
    }
    .set(cx);
}
// --- end v13/tokens ---

/// Writes every gpui-component `ThemeColor` slot the app can show in one
/// pass from the palette (COMPLETENESS.md §1.2, §1.3, D1): rem 16, so
/// gpui-component draws its own shadcn sizes.
fn apply(t: &mut Theme, dark: bool) {
    use crate::ui::native::tokens::{Palette, radius as r, text};
    let p = Palette::for_mode(dark);
    let k = if dark { dark_colors() } else { light_colors() };
    let clear = c(0x00000000);
    let soft = Palette::soft;
    // The system accent the person picked, else `primary`.
    let primary = app_accent(dark);
    let on_primary = accent_ink(dark);
    let primary_hover = primary.opacity(0.9);
    let primary_active = primary.opacity(0.8);
    let white = c(0xFFFFFFFF);

    // Base.
    t.background = p.background;
    t.foreground = p.foreground;
    t.muted = p.muted;
    t.muted_foreground = k.text_2;
    t.border = k.border;
    t.input = k.control_border;
    t.ring = p.ring;
    // Text selection is neutral (T1-15, §1.3): foreground at 15 %.
    t.selection = soft(p.foreground, 0.15);
    t.caret = p.foreground;
    t.overlay = p.scrim;
    t.accent = p.accent;
    t.accent_foreground = p.foreground;
    t.accordion = p.background;
    t.window_border = k.border;
    t.drag_border = p.ring;
    t.drop_target = p.muted;

    // Buttons: default = primary; secondary = muted fill; outline and ghost
    // draw on `background` and hover in `accent` (§1.8).
    t.primary = primary;
    t.primary_hover = primary_hover;
    t.primary_active = primary_active;
    t.primary_foreground = on_primary;
    t.button_primary = primary;
    t.button_primary_hover = primary_hover;
    t.button_primary_active = primary_active;
    t.button_primary_foreground = on_primary;
    t.secondary = p.muted;
    t.secondary_hover = p.accent;
    t.secondary_active = p.pressed;
    t.secondary_foreground = p.foreground;
    t.button = p.background;
    t.button_hover = p.accent;
    t.button_active = p.pressed;
    t.button_foreground = p.foreground;
    t.button_secondary = p.muted;
    t.button_secondary_hover = p.accent;
    t.button_secondary_active = p.pressed;
    t.button_secondary_foreground = p.foreground;

    // State colors (§1.3): fills and their white text.
    // One destructive fill in both modes (T1-15): white text at 4.8:1.
    let danger_fill = c(crate::ui::native::tokens::DESTRUCTIVE_FILL);
    t.danger = danger_fill;
    t.danger_hover = danger_fill.opacity(0.9);
    t.danger_active = danger_fill.opacity(0.8);
    t.danger_foreground = white;
    t.button_danger = danger_fill;
    t.button_danger_hover = danger_fill.opacity(0.9);
    t.button_danger_active = danger_fill.opacity(0.8);
    t.button_danger_foreground = white;
    t.success = p.success;
    t.success_hover = p.success;
    t.success_active = p.success;
    t.success_foreground = white;
    t.button_success = p.success;
    t.button_success_hover = p.success.opacity(0.9);
    t.button_success_active = p.success.opacity(0.8);
    t.button_success_foreground = white;
    t.warning = p.warning;
    t.warning_hover = p.warning;
    t.warning_active = p.warning;
    t.warning_foreground = white;
    t.button_warning = p.warning;
    t.button_warning_hover = p.warning.opacity(0.9);
    t.button_warning_active = p.warning.opacity(0.8);
    t.button_warning_foreground = white;
    t.info = p.info;
    t.info_hover = p.info;
    t.info_active = p.info;
    t.info_foreground = white;
    t.button_info = p.info;
    t.button_info_hover = p.info.opacity(0.9);
    t.button_info_active = p.info.opacity(0.8);
    t.button_info_foreground = white;

    // Lists and tables (§2.6, §2.7): hover muted at 50 %, selected muted.
    t.colors.list = p.background;
    t.list_hover = soft(p.muted, 0.5);
    t.list_active = p.muted;
    t.list_active_border = clear;
    t.list_even = p.background;
    t.list_head = p.background;
    t.table = p.background;
    t.table_hover = soft(p.muted, 0.5);
    t.table_active = p.muted;
    t.table_active_border = clear;
    t.table_even = p.background;
    t.table_row_border = k.border;
    t.table_head = p.muted;
    t.table_head_foreground = p.foreground;
    t.table_foot = p.background;
    t.table_foot_foreground = k.text_2;

    // Surfaces.
    t.popover = p.card;
    t.popover_foreground = p.foreground;
    t.group_box = p.sidebar;
    t.group_box_foreground = p.foreground;
    t.description_list_label = clear;
    t.description_list_label_foreground = k.text_2;

    t.sidebar = p.sidebar;
    t.sidebar_foreground = p.foreground;
    t.sidebar_accent = p.muted;
    t.sidebar_accent_foreground = system_accent().unwrap_or(p.foreground);
    t.sidebar_primary = primary;
    t.sidebar_primary_foreground = on_primary;
    t.sidebar_border = k.border;

    t.title_bar = clear;
    t.title_bar_border = clear;
    t.status_bar = p.background;
    t.status_bar_border = k.border;

    // Tabs (segmented): list muted, trigger background.
    t.tab_bar = clear;
    t.tab_bar_segmented = p.muted;
    t.tab = clear;
    t.tab_active = p.background;
    t.tab_foreground = p.foreground.opacity(0.6);
    t.tab_active_foreground = p.foreground;

    // Switch (§1.8): OFF track input, ON track primary.
    t.switch = p.input;
    t.switch_thumb = if dark { p.foreground } else { p.background };
    t.slider_bar = primary;
    t.slider_thumb = p.background;
    // Progress (D12): value primary on primary at 20 %.
    t.progress_bar = primary;
    t.skeleton = p.muted;

    t.scrollbar = clear;
    t.scrollbar_thumb = soft(p.foreground, 0.25);
    t.scrollbar_thumb_hover = soft(p.foreground, 0.4);

    t.link = p.info;
    t.link_hover = p.info.opacity(0.85);
    t.link_active = p.info.opacity(0.7);

    // Named colors: the four state colors; their light variants are the
    // soft fills of an alert.
    t.blue = p.info;
    t.blue_light = soft(p.info, 0.1);
    t.yellow = p.warning;
    t.yellow_light = soft(p.warning, 0.1);
    t.red = p.destructive;
    t.red_light = soft(p.destructive, 0.1);
    t.green = p.success;
    t.green_light = soft(p.success, 0.1);
    t.magenta = p.info;
    t.magenta_light = soft(p.info, 0.1);
    t.cyan = p.info;
    t.cyan_light = soft(p.info, 0.1);

    // Charts: primary first (D12), then neutral steps.
    t.chart_1 = primary;
    t.chart_2 = p.muted_foreground;
    t.chart_3 = p.ring;
    t.chart_4 = p.input;
    t.chart_5 = p.border_strong;
    t.chart_bullish = p.success;
    t.chart_bearish = p.destructive;
    t.chart_grid = k.border;

    // Type and shape (D1, D5): rem 16, controls radius 8, large 10.
    t.font_family = FONT_FAMILY.into();
    t.font_size = px(16.);
    t.mono_font_family = MONO_FONT_FAMILY.into();
    t.mono_font_size = text::MONO.0;
    t.radius = r::CONTROL;
    t.radius_lg = r::CONTAINER;
    // Toasts (§2.16, T1-06): bottom-right, 16 from the window edges, 360.
    t.notification.placement = gpui::Anchor::BottomRight;
    t.notification.width = px(360.);
    t.notification.margins = gpui_component::Edges::all(space::S16);
    // A focused composer lifts toasts over itself (R1-15).
    t.notification.margins.bottom = crate::ui::overlay::toast_bottom();
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

/// The root text size (D1): 14 on a rem of 16.
pub const ROOT_TEXT: Pixels = crate::ui::native::tokens::text::UI.0;

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

/// The type scale (§1.4 of docs/design/COMPLETENESS.md) under the older
/// role names; the values live in ui/native/tokens.rs.
pub use crate::ui::native::tokens::type_scale;

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
    /// 24 / 32, 600: the hero title (login, member profile).
    fn t_display(self) -> Self {
        self.type_role(type_scale::DISPLAY)
            .font_weight(FontWeight::SEMIBOLD)
    }
    /// 18 / 28, 600: dialog, sheet, alert and topic-start titles.
    fn t_heading(self) -> Self {
        self.type_role(type_scale::HEADING)
            .font_weight(FontWeight::SEMIBOLD)
    }
    /// 18 / 28, 600: sheet and dialog titles.
    fn t_title(self) -> Self {
        self.type_role(type_scale::TITLE)
            .font_weight(FontWeight::SEMIBOLD)
    }
    /// 16 / 24, 600: card and section titles.
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
    /// 12 / 16: meta lines, captions, footnotes.
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
    /// 12 / 16, 500: badges and group labels.
    fn t_section(self) -> Self {
        self.type_role(type_scale::META).font_weight(MEDIUM_WEIGHT)
    }
    /// 13 / 20 Menlo.
    fn t_code(self) -> Self {
        self.type_role(type_scale::CODE)
            .font_family(MONO_FONT_FAMILY)
    }
    /// 18 / 28, 500, tabular figures: invite code boxes.
    fn t_code_entry(self) -> Self {
        self.type_role(type_scale::CODE_ENTRY)
            .font_weight(MEDIUM_WEIGHT)
            .font_features(tabular())
    }
    /// 16 / 24, 500: the toolbar title of every page (D9).
    fn t_page_title(self) -> Self {
        self.type_role(type_scale::PAGE_TITLE)
            .font_weight(MEDIUM_WEIGHT)
    }
    /// 16 / 24, 500: the toolbar title of a 话题 page (D9).
    fn t_bar_title(self) -> Self {
        self.type_role(type_scale::PAGE_TITLE)
            .font_weight(MEDIUM_WEIGHT)
    }
    /// 30 / 36, 600, tabular: a KPI number (§2.4).
    fn t_kpi(self) -> Self {
        self.type_role(type_scale::KPI)
            .font_weight(FontWeight::SEMIBOLD)
            .font_features(tabular())
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
    /// A side column (the sidebar, the right pane) sliding open: long enough
    /// to read as a drawer, not a jump (review-animations: drawers 200–500).
    pub const COLUMN_OPEN: Duration = Duration::from_millis(340);
    /// …and shut: exits run a little faster than enters.
    pub const COLUMN_CLOSE: Duration = Duration::from_millis(280);
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
    /// A picker opened by hover closes this long after the pointer leaves
    /// both its trigger and its panel, so crossing the gap keeps it open.
    pub const HOVER_LEAVE: Duration = Duration::from_millis(250);
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

    /// `cubic-bezier(0.32, 0.72, 0, 1)`: a drawer (the iOS sheet curve). It
    /// leaves at once and settles softly, so a column follows the click
    /// without the slow start of `moving`.
    pub fn drawer() -> Easing {
        Easing::cubic_bezier(0.32, 0.72, 0.0, 1.0).expect("valid curve")
    }

    /// The out curve as a function, for `with_animation`.
    pub fn out_fn() -> impl Fn(f32) -> f32 + 'static {
        let e = out();
        move |t| e.sample(t)
    }

    // v3 motion (CONCEPT2 §5). Loops only for live signals; never animate
    // selection, keyboard navigation, screen switches, counts or the theme.

    /// First paint: no stagger (T1-17, review-animations 4): a page opens
    /// after a click, and a list that waits for its last row reads slow.
    pub const STAGGER: Duration = Duration::ZERO;
    /// At most this many items take part in a stagger.
    pub const STAGGER_MAX: usize = 8;
    /// The rise of a card or row on first paint (`tc-rise`): under 200 ms.
    pub const RISE_IN: Duration = Duration::from_millis(180);
    /// Side chat and panes enter: opacity plus [`PANE_TRAVEL`] from the right.
    pub const PANE_ENTER: Duration = Duration::from_millis(200);
    pub const PANE_TRAVEL: gpui::Pixels = gpui::px(12.);
    /// One breath of a live ring or a current plan dot (`tc-breathe`).
    pub const BREATHE: Duration = Duration::from_millis(2400);
    /// The opacity a breathing mark swings between. Both 1 (T1-16): a mark
    /// that fades to 25 % falls to 1.4:1 on `muted`; working is a spinner
    /// (§2.12), and a breathing mark stays at full strength.
    pub const BREATHE_HIGH: f32 = 1.0;
    pub const BREATHE_LOW: f32 = 1.0;
    /// The check of an answered card draws in (`tc-draw`).
    pub const CHECK_DRAW: Duration = Duration::from_millis(300);

    // panes track (1.1.0): 实时查看.
    /// The terminal cursor of 实时查看 blinks once this often (`tc-blink`).
    pub const LIVE_CURSOR_BLINK: Duration = Duration::from_millis(1100);
    /// The 回放 track fills in once (`tc-fill`).
    pub const LIVE_TRACK_FILL: Duration = Duration::from_millis(250);
    /// Chart bars grow from zero once.
    pub const BAR_GROW: Duration = Duration::from_millis(250);
    /// The blink half-period of a live terminal cursor (`tc-blink`).
    pub const CURSOR_BLINK: Duration = Duration::from_millis(500);

    // dfix-motion (1.1.0): chart lines, and what counts as just answered.
    /// A chart line draws in once: under 300 ms with its series stagger
    /// (T1-17, review-animations 4).
    pub const LINE_DRAW: Duration = Duration::from_millis(240);
    /// Each further chart series starts this much later.
    pub const SERIES_STAGGER: Duration = Duration::from_millis(20);
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
    /// The quiet card button (`PushStyle::Secondary`): a soft fill, no border,
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
    /// The one palette every field above comes from.
    pub palette: crate::ui::native::tokens::Palette,
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

    /// Shadow sm under cards (§1.7).
    pub fn lift(&self) -> Vec<BoxShadow> {
        tk::shadow::sm()
    }

    /// Shadow xs under a primary button (§1.7).
    pub fn btn_hi(&self) -> Vec<BoxShadow> {
        tk::shadow::xs()
    }

    /// Shadow xs under an outline or secondary button (§1.7).
    pub fn sec_hi(&self) -> Vec<BoxShadow> {
        tk::shadow::xs()
    }

    /// The 1 px border line inside every agent avatar tile (§1.8, §2.22).
    pub fn tile_edge(&self) -> Vec<BoxShadow> {
        vec![shadow(self.border, 0., 0., 0., 1., true)]
    }

    /// Popovers and the palette: shadow md and the border line (§1.7).
    pub fn pop(&self) -> Vec<BoxShadow> {
        let mut s = tk::shadow::md();
        s.push(tk::shadow::line(self.border));
        s
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

    /// The edge of an overlay: the border token (§1.7). Dialogs draw it as
    /// their 1 px border.
    pub fn overlay_edge(&self) -> Hsla {
        self.border
    }

    /// The shadow list of an elevation level (§1.7): content (none, xs,
    /// sm), float (md) and modal (lg). Every level keeps the 1 px border
    /// line in both modes.
    pub fn shadow(&self, level: Elevation) -> Vec<BoxShadow> {
        let with_line = |mut s: Vec<BoxShadow>| {
            s.push(tk::shadow::line(self.border));
            s
        };
        match level {
            Elevation::Flat => vec![],
            Elevation::Card => with_line(tk::shadow::sm()),
            Elevation::Image => with_line(vec![]),
            Elevation::Control => with_line(tk::shadow::xs()),
            Elevation::Float | Elevation::Capsule => with_line(tk::shadow::md()),
            Elevation::Overlay | Elevation::Popover => with_line(tk::shadow::md()),
            Elevation::Sheet => with_line(tk::shadow::lg()),
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

/// The app colors of one mode: every field is a value of the one
/// [`Palette`](crate::ui::native::tokens::Palette) (COMPLETENESS.md §1.2,
/// §1.3). The older names stay so views build; they point at the palette
/// tokens: one muted text color, one border, a muted selection, `primary`
/// buttons, and state colors only for state.
fn build(dark: bool) -> Colors {
    let p = tk::Palette::for_mode(dark);
    let soft = tk::Palette::soft;
    let x = |l: u32, d: u32| if dark { c(d) } else { c(l) };
    let fg = p.foreground;
    let mfg = p.muted_foreground;
    // Text on a selected row, a link: `foreground` / `info`.
    let accent = p.primary;
    Colors {
        bg: p.background,
        rail: p.sidebar,
        sunken: p.muted,
        ink: fg,
        ink_2: mfg,
        ink_3: mfg,
        line: p.border,
        line_2: p.input,
        sel: p.muted,
        rail_sel: p.muted,
        rail_hover: p.accent,
        btn: accent,
        btn_ink: p.primary_foreground,
        btn_quiet: p.muted,
        btn_quiet_hover: p.accent,
        btn_quiet_pressed: p.pressed,
        you: p.warning,
        you_ink: p.warning,
        live: p.info,
        live_ink: p.info,
        ok: p.success,
        ok_ink: p.success,
        bad: p.destructive,
        bad_ink: p.destructive,
        person: p.muted,
        // The live terminal frame stays dark in both modes.
        screen: x(0x171717FF, 0x0A0A0AFF),
        screen_ink: c(0xD4D4D4FF),
        screen_dim: x(0x737373FF, 0x737373FF),
        tile_ink: fg,
        ph: [p.muted, p.pressed, p.muted],
        // One avatar style for every member (§1.8): no tints.
        tints: [p.muted; 8],

        sidebar: p.sidebar,
        content: p.background,
        raised: p.card,
        overlay: p.card,
        scrim: p.scrim,
        code_bg: p.muted,
        text_1: fg,
        text_2: mfg,
        text_3: mfg,
        placeholder: mfg,
        sep: p.border,
        border: p.border,
        border_strong: p.border_strong,
        hover: p.accent,
        selected: p.muted,
        sidebar_hover: p.accent,
        sidebar_selected: p.muted,
        sel_glyph: fg,
        pressed: p.pressed,
        field: p.muted,
        count_bg: p.muted,
        count_text: fg,
        count_solid: accent,
        tile_ring: p.background,
        opt_bg: p.muted,
        opt_hover: p.accent,
        disabled_bg: soft(p.primary, 0.5),
        disabled_text: soft(p.primary_foreground, 0.5),
        tile: p.muted,
        desk: p.muted,
        accent,
        accent_hover: accent.opacity(0.9),
        accent_pressed: accent.opacity(0.8),
        accent_text: p.info,
        accent_soft: p.muted,
        on_accent: p.primary_foreground,
        focus: p.focus(),
        focus_border: p.ring,
        focus_soft: p.focus(),
        choice_bg: p.muted,
        mention_bg: p.muted,
        me_bg: soft(p.warning, 0.15),
        me_text: fg,
        hl_bg: p.muted,
        unread_dot: p.info,
        you_bg: p.muted,
        you_text: fg,
        you_glyph: p.warning,
        you_bar: p.warning,
        info_bg: p.muted,
        info_text: p.info,
        danger: p.destructive,
        danger_bg: soft(p.destructive, 0.2),
        danger_text: p.destructive,
        success: p.success,
        success_text: p.success,
        badge: p.destructive,
        online: p.success,
        avatar_bg: p.muted,
        avatar_fg: fg,
        bots: [p.muted; 6],
        palette: p,
        dark,
        panel: p.background,
        window_bg: p.background,
        group: p.sidebar,
        elevated: p.card,
        fill_hover: p.accent,
        fill_selected: p.muted,
        fill_pressed: p.pressed,
        fill_field: p.muted,
        fill_active: p.muted,
        control_border: p.input,
        control_border_focus: p.ring,
        quote_bar: p.input,
        code_text: fg,
        warning: p.warning,
        new_divider: p.destructive,
        glyph_progress: p.info,
        glyph_wait: p.warning,
        glyph_done: p.success,
        background: p.background,
        foreground: fg,
        muted_foreground: mfg,
        row_hover: soft(p.muted, 0.5),
        offline: mfg,
        code_background: p.muted,
        search_highlight: p.muted,
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
    // One muted text color still (§1.2), one step stronger.
    let strong = k.palette.muted_foreground_strong;
    k.border = k.border_strong;
    k.sep = k.border_strong;
    k.placeholder = k.text_3;
    k.text_2 = strong;
    k.text_3 = strong;
    k.muted_foreground = strong;
    // v3 names of the same tokens.
    k.line = k.border_strong;
    k.control_border = k.border_strong;
    k.ink_2 = strong;
    k.ink_3 = strong;
    k.offline = strong;
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

/// The desk terminal thumbnail sizes (ui/native/tokens.rs).
#[allow(unused_imports)]
pub use crate::ui::native::tokens::desk_screen;

// --- cards v2 track: Card v2 metrics and tints (DESIGN.md §7.10) ---

/// Card v2 sizes and tints (ui/native/tokens.rs).
pub use crate::ui::native::tokens::card_v2;

// --- end cards v2 track ---

// ---------------------------------------------------------------------------
// --- v12/shell: the app accent (NATIVE.md §4) ---
//
// The accent is graphite (`#6E6E73`, light and dark, white text on it)
// while the system accent is multicolor (`AppleAccentColor` unset). When the person picked an accent
// color in System Settings, the app uses `NSColor.controlAccentColor`, read
// again when the system colors change. Self-tests never read it: they draw
// the ink accent.

/// The system accent as `0xRRGGBBAA`, or 0 for multicolor (graphite).
static SYSTEM_ACCENT: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

/// The accent the person picked in System Settings; `None` for multicolor.
pub fn system_accent() -> Option<Hsla> {
    match SYSTEM_ACCENT.load(Ordering::Relaxed) {
        0 => None,
        v => Some(rgba(v).into()),
    }
}

/// Sets the system accent (`None`: multicolor). True when it changed.
pub fn set_system_accent(accent: Option<Hsla>) -> bool {
    let v = accent.map_or(0, |a| {
        let c = gpui::Rgba::from(a);
        let byte = |x: f32| (x.clamp(0., 1.) * 255.).round() as u32;
        // 0 means multicolor: an opaque color never packs to 0.
        (byte(c.r) << 24) | (byte(c.g) << 16) | (byte(c.b) << 8) | 0xFF
    });
    SYSTEM_ACCENT.swap(v, Ordering::Relaxed) != v
}

/// The app accent of a mode: graphite, or the system accent.
pub fn app_accent(dark: bool) -> Hsla {
    system_accent().unwrap_or_else(|| crate::ui::native::NativeColors::for_mode(dark).primary)
}

/// Text and symbols on the app accent: white on graphite and on a system
/// accent color.
pub fn accent_ink(dark: bool) -> Hsla {
    match system_accent() {
        Some(_) => c(0xFFFFFFFF),
        None => crate::ui::native::NativeColors::for_mode(dark).on_primary,
    }
}

/// The share of the accent in the focus halo.
pub const FOCUS_HALO: f32 = 0.35;

/// gpui-component's `ring` for a soft 3 pt halo of the accent at 35 %:
/// gpui-component paints its outer band in `ring` at 50 %, so the ring is
/// the accent blended toward `under` until half of it is 35 % accent.
pub fn focus_ring_color(accent: Hsla, under: Hsla) -> Hsla {
    let (a, b) = (gpui::Rgba::from(accent), gpui::Rgba::from(under));
    let t = (FOCUS_HALO * 2.).min(1.);
    let mix = |x: f32, y: f32| x * t + y * (1. - t);
    gpui::Rgba {
        r: mix(a.r, b.r),
        g: mix(a.g, b.g),
        b: mix(a.b, b.b),
        a: 1.,
    }
    .into()
}

/// The focus ring drawn by our own controls (§1.8): 3 px of `ring` at 50 %.
pub fn focus_halo(dark: bool) -> Vec<BoxShadow> {
    vec![BoxShadow {
        color: tk::Palette::for_mode(dark).focus(),
        offset: point(px(0.), px(0.)),
        blur_radius: px(0.),
        spread_radius: crate::ui::native::metrics::FOCUS_RING,
        inset: false,
    }]
}

/// Reads the system accent now: `None` while `AppleAccentColor` is unset
/// (multicolor).
#[cfg(target_os = "macos")]
fn read_system_accent() -> Option<Hsla> {
    use objc2_app_kit::{NSColor, NSColorSpace};
    use objc2_foundation::{NSString, NSUserDefaults};

    let defaults = NSUserDefaults::standardUserDefaults();
    defaults.objectForKey(&NSString::from_str("AppleAccentColor"))?;
    let color = NSColor::controlAccentColor();
    let srgb = color.colorUsingColorSpace(&NSColorSpace::sRGBColorSpace())?;
    let (mut r, mut g, mut b, mut a) = (0., 0., 0., 0.);
    // SAFETY: the four pointers are valid for writes for the whole call.
    unsafe { srgb.getRed_green_blue_alpha(&mut r, &mut g, &mut b, &mut a) };
    Some(
        gpui::Rgba {
            r: r as f32,
            g: g as f32,
            b: b as f32,
            a: 1.,
        }
        .into(),
    )
}

#[cfg(not(target_os = "macos"))]
fn read_system_accent() -> Option<Hsla> {
    None
}

/// Reads the system accent and repaints when it changed.
fn sync_system_accent(cx: &mut App) {
    if set_system_accent(read_system_accent()) {
        let dark = is_dark(cx);
        Theme::update(cx, |theme| apply(theme, dark));
        apply_menu_style(cx);
        cx.refresh_windows();
    }
}

/// Follows the accent color of System Settings while the app runs: read at
/// start and again on `NSSystemColorsDidChangeNotification` and when the
/// color preferences change. Call once on the main thread (the app only).
#[cfg(target_os = "macos")]
pub fn watch_system_accent(cx: &mut App) {
    use std::ptr::NonNull;

    use block2::RcBlock;
    use futures::StreamExt as _;
    use objc2_app_kit::NSSystemColorsDidChangeNotification;
    use objc2_foundation::{
        NSDistributedNotificationCenter, NSNotification, NSNotificationCenter, NSString,
    };

    sync_system_accent(cx);
    let (tx, mut rx) = futures::channel::mpsc::unbounded::<()>();
    let block = RcBlock::new(move |_n: NonNull<NSNotification>| {
        let _ = tx.unbounded_send(());
    });
    // SAFETY: valid notification names; the block only sends on a channel.
    let observers = unsafe {
        let local = NSNotificationCenter::defaultCenter()
            .addObserverForName_object_queue_usingBlock(
                Some(NSSystemColorsDidChangeNotification),
                None,
                None,
                &block,
            );
        let distributed = NSDistributedNotificationCenter::defaultCenter()
            .addObserverForName_object_queue_usingBlock(
                Some(&NSString::from_str(
                    "AppleColorPreferencesChangedNotification",
                )),
                None,
                None,
                &block,
            );
        (local, distributed)
    };
    // Observe for the life of the process.
    std::mem::forget(observers);
    std::mem::forget(block);
    cx.spawn(async move |cx| {
        while rx.next().await.is_some() {
            cx.update(sync_system_accent);
        }
    })
    .detach();
}

#[cfg(not(target_os = "macos"))]
pub fn watch_system_accent(_cx: &mut App) {}
// --- end v12/shell ---

#[cfg(test)]
mod tests {
    use super::*;

    // --- v12/shell ---
    #[test]
    fn the_accent_is_graphite_until_a_system_accent_is_set() {
        set_system_accent(None);
        for dark in [false, true] {
            let p = tk::Palette::for_mode(dark);
            assert_eq!(app_accent(dark), p.primary);
            assert_eq!(accent_ink(dark), p.primary_foreground);
        }
        let blue: Hsla = rgba(0x007AFFFF).into();
        assert!(set_system_accent(Some(blue)));
        assert!(
            !set_system_accent(Some(blue)),
            "the same accent is no change"
        );
        let back = gpui::Rgba::from(app_accent(false));
        assert!((back.b - 1.).abs() < 0.01 && back.r < 0.01);
        assert_eq!(accent_ink(false), rgba(0xFFFFFFFF).into());
        set_system_accent(None);
        // The halo: half of the ring over `under` is 35 % accent.
        let ring = gpui::Rgba::from(focus_ring_color(
            rgba(0x000000FF).into(),
            rgba(0xFFFFFFFF).into(),
        ));
        assert!((ring.r - 0.3).abs() < 0.01 && ring.a == 1.);
    }
    // --- end v12/shell ---

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
            // Still one muted text color, one step stronger.
            assert_eq!(h.text_3, h.text_2);
            assert_eq!(h.text_2, k.palette.muted_foreground_strong);
            assert!(contrast(h.text_2, h.bg) > contrast(k.text_2, k.bg));
            assert_eq!(h.placeholder, k.text_3);
            assert_eq!(h.text_1, k.text_1);
            assert_eq!(h.line, k.border_strong);
            assert_eq!(h.ink_3, h.ink_2);
        }
    }

    #[test]
    fn a_disabled_primary_is_primary_at_half() {
        // D11: a disabled primary is the primary at 50 %, never the
        // secondary fill that Cancel could wear.
        for k in [build(false), build(true)] {
            assert_eq!(k.disabled_bg, k.btn.opacity(0.5));
            assert_ne!(k.disabled_bg, k.btn_quiet);
        }
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
    fn primary_is_graphite_and_selection_is_grey() {
        // No white or black filled buttons: the default button is the
        // graphite accent with white text, in both modes.
        for k in [build(false), build(true)] {
            assert_eq!(k.accent, k.btn);
            assert_ne!(k.btn, k.ink);
            assert_eq!(k.btn, k.palette.primary);
            assert_eq!(k.selected, k.sel);
            assert!(k.sel.s < 0.1, "selection has no hue");
            assert!(contrast(k.btn_ink, k.btn) >= 4.5);
        }
    }

    #[test]
    fn state_colors_keep_one_meaning_each() {
        for k in [build(false), build(true)] {
            assert_eq!(k.state(StateTone::You).0, k.you);
            assert_eq!(k.state(StateTone::Live).0, k.live);
            assert_eq!(k.state(StateTone::Problem).0, k.bad);
            // Warning marks an icon (§1.3): the 3:1 non-text minimum.
            assert!(contrast(k.you_ink, k.bg) >= 3.);
            assert!(contrast(k.live_ink, k.bg) >= 4.5);
            assert!(contrast(k.bad_ink, k.bg) >= 4.5);
            assert!(contrast(k.ink_3, k.rail) >= 4.5);
        }
    }

    #[test]
    fn both_modes_draw_the_same_lines() {
        // §1.7: one shadow set and a border line on every level, in light
        // and dark alike.
        for k in [build(false), build(true)] {
            assert_eq!(k.lift(), tk::shadow::sm());
            assert!(k.tile_edge()[0].inset);
            for level in [
                Elevation::Card,
                Elevation::Float,
                Elevation::Overlay,
                Elevation::Sheet,
            ] {
                assert!(k.shadow(level).iter().any(|s| s.color == k.border));
            }
            assert_eq!(k.text_2, k.text_3);
            assert_eq!(k.sep, k.border);
        }
    }
}

// --- inc-cards21: Card v2.1 sizes and timings live in ui/native/tokens.rs ---
pub use crate::ui::native::tokens::card_v21;
// --- end inc-cards21 ---
