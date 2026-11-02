//! The one theme module. It follows the system light or dark mode and gives
//! views the app's semantic colors and sizes.
//!
//! Views read colors with `theme::colors(cx)` (or `cx.theme()` from
//! gpui-component for component tokens). Never hard-code a color in a view:
//! add a field here so light and dark both work.

use gpui::{App, Hsla, Pixels, Subscription, Window, px};
use gpui_component::{ActiveTheme, Theme, ThemeMode};

use crate::data::TopicStatus;

/// Layout sizes shared by the views.
pub const SIDEBAR_WIDTH: Pixels = px(240.);
pub const TOPIC_LIST_WIDTH: Pixels = px(300.);
pub const SIDE_PANEL_WIDTH: Pixels = px(280.);
pub const AVATAR_SIZE: Pixels = px(32.);
pub const ROW_HEIGHT: Pixels = px(32.);

/// Sets the theme from the window's appearance and keeps it in step when the
/// user switches light or dark mode. Keep the returned subscription alive.
pub fn follow_system(window: &mut Window, cx: &mut App) -> Subscription {
    Theme::sync_system_appearance(Some(window), cx);
    window.observe_window_appearance(|window, cx| {
        Theme::sync_system_appearance(Some(window), cx);
    })
}

pub fn is_dark(cx: &App) -> bool {
    cx.theme().mode == ThemeMode::Dark
}

/// Semantic colors of the app.
#[derive(Clone, Copy, Debug)]
pub struct Colors {
    pub background: Hsla,
    pub foreground: Hsla,
    pub muted_foreground: Hsla,
    pub border: Hsla,
    pub sidebar: Hsla,
    pub sidebar_foreground: Hsla,
    pub row_hover: Hsla,
    pub row_selected: Hsla,
    pub accent: Hsla,
    pub accent_foreground: Hsla,
    /// Unread count badge.
    pub badge: Hsla,
    pub badge_foreground: Hsla,
    /// @mention badge.
    pub mention: Hsla,
    pub mention_foreground: Hsla,
    pub online: Hsla,
    pub offline: Hsla,
    pub danger: Hsla,
    pub panel: Hsla,
    pub code_background: Hsla,
    /// A message opened from search, and matched words in search results.
    pub search_highlight: Hsla,
}

pub fn colors(cx: &App) -> Colors {
    let t = cx.theme();
    Colors {
        background: t.background,
        foreground: t.foreground,
        muted_foreground: t.muted_foreground,
        border: t.border,
        sidebar: t.sidebar,
        sidebar_foreground: t.sidebar_foreground,
        row_hover: t.list_hover,
        row_selected: t.list_active,
        accent: t.primary,
        accent_foreground: t.primary_foreground,
        badge: t.muted,
        badge_foreground: t.foreground,
        mention: t.danger,
        mention_foreground: t.danger_foreground,
        online: t.success,
        offline: t.muted_foreground,
        danger: t.danger,
        panel: t.sidebar,
        code_background: t.muted,
        search_highlight: if t.mode == ThemeMode::Dark {
            t.yellow.opacity(0.22)
        } else {
            t.yellow_light
        },
    }
}

/// Background and text colors of a topic status chip.
pub fn status_chip_colors(status: TopicStatus, cx: &App) -> (Hsla, Hsla) {
    let t = cx.theme();
    let (bg, fg) = match status {
        TopicStatus::Open => (t.muted, t.muted_foreground),
        TopicStatus::InProgress => (t.blue_light, t.blue),
        TopicStatus::WaitingAnswer | TopicStatus::WaitingReview => (t.yellow_light, t.yellow),
        TopicStatus::Blocked => (t.red_light, t.red),
        TopicStatus::Done => (t.green_light, t.green),
    };
    if is_dark(cx) {
        (fg.opacity(0.2), fg)
    } else {
        (bg, fg)
    }
}
