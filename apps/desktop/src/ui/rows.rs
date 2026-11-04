//! The one row helper (DESIGN.md §7.1, §7.6, §7.7): sidebar rows, topic
//! rows, menu and popup rows and settings rows all get their hover, pressed,
//! selected and focus states from here, and section headers share one shape.

use gpui::{
    AnyElement, App, ElementId, InteractiveElement as _, IntoElement, ParentElement, Pixels,
    RenderOnce, SharedString, StatefulInteractiveElement as _, Styled, Window, div,
    prelude::FluentBuilder as _, px,
};

use crate::{
    theme::{self, Type as _, size, space},
    ui::{
        chip::{BadgeKind, CountBadge},
        icons::{Sym, icon},
    },
};

/// The surface a row sits on: it picks the hover and selected fills.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RowSurface {
    /// `sidebar-hover` / `sidebar-selected` (sidebar, settings section list).
    Sidebar,
    /// `hover` / `selected` (topic list, settings tables, side panel).
    Content,
    /// `hover` / `selected` with radius 6 (menus, popups, the switcher).
    Overlay,
    /// v3 rail rows (destinations, 话题, members): `hover` / grey `sel` on
    /// `rail`, radius 8. The selected row's text is `ink` at weight 500
    /// (the caller's).
    Rail,
}

/// An interactive row with the shared states (§7.1): hover fill at once,
/// `pressed` on mouse down, the accent-tint selection, 40 % content when
/// disabled, a pointer cursor. Height, padding and children are the caller's.
///
/// Sidebar and content rows are tab stops (Tab and Shift-Tab, `app.rs`):
/// with keyboard focus they show the 2 pt `focus` ring inside the row, and
/// Enter or Space runs their click. Overlay rows (menus, popups, the
/// switcher) are not: their owner moves a selection with the arrow keys.
pub fn list_row(
    id: impl Into<ElementId>,
    surface: RowSurface,
    selected: bool,
    cx: &App,
) -> gpui::Stateful<gpui::Div> {
    let k = theme::colors(cx);
    let (hover, sel, radius) = match surface {
        RowSurface::Sidebar => (k.sidebar_hover, k.sidebar_selected, theme::radius::MD),
        RowSurface::Content => (k.hover, k.selected, theme::radius::MD),
        RowSurface::Overlay => (k.hover, k.selected, theme::radius::SM),
        RowSurface::Rail => (k.rail_hover, k.rail_sel, theme::radius::MD),
    };
    div()
        .id(id)
        .flex_none()
        .w_full()
        .rounded(radius)
        .flex()
        .items_center()
        // v12/kit (NATIVE.md §1 rule 9, §10): the arrow cursor and no hover
        // fill on rows; the Tahoe gray selection. Only menu rows (overlay)
        // keep a hover highlight.
        .when(selected, |el| el.bg(sel))
        .when(!selected && surface == RowSurface::Overlay, |el| {
            el.hover(|s| s.bg(hover)).active(|s| s.bg(k.pressed))
        })
        .when(surface != RowSurface::Overlay, |el| {
            el.tab_index(0)
                .focus_visible(|s| s.shadow(focus_ring(k.focus)))
        })
}

/// The keyboard focus ring of a row (§7.1): 2 pt of `focus` inside its
/// edge, drawn as an inset shadow so the row's layout does not move.
pub fn focus_ring(color: gpui::Hsla) -> Vec<gpui::BoxShadow> {
    vec![
        gpui::BoxShadow::new(px(0.), px(0.), color)
            .spread_radius(px(2.))
            .inset(),
    ]
}

/// Set by the screenshot self-test: its windows are never key, but the
/// captures show the active look.
static DRAW_AS_KEY: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Draws every window as the key window (screenshot self-test only).
pub fn draw_as_key_window() {
    DRAW_AS_KEY.store(true, std::sync::atomic::Ordering::Relaxed);
}

/// Whether selections in `window` use the accent tint. Like a macOS source
/// list, a window that is not key shows its selection in the unemphasized
/// grey (`pressed`), so the key window is easy to see.
pub fn selection_emphasized(window: &Window) -> bool {
    window.is_window_active() || DRAW_AS_KEY.load(std::sync::atomic::Ordering::Relaxed)
}

/// A row that cannot be used: its content at 40 %, no hover.
pub fn disabled_row(id: impl Into<ElementId>) -> gpui::Stateful<gpui::Div> {
    div()
        .id(id)
        .flex_none()
        .w_full()
        .flex()
        .items_center()
        .opacity(0.4)
}

/// A sidebar-style section header (§7.7): 28 pt, padding 0 8, a chevron 12
/// `text-3` that turns when collapsed, a 12/16 600 `text-2` title, an
/// optional grey count and a trailing element (a `sm` icon button `+`).
#[derive(IntoElement)]
pub struct SectionHeader {
    id: Option<ElementId>,
    label: SharedString,
    count: Option<u32>,
    collapsible: bool,
    collapsed: bool,
    trailing: Option<AnyElement>,
}

impl SectionHeader {
    pub fn new(label: impl Into<SharedString>) -> Self {
        Self {
            id: None,
            label: label.into(),
            count: None,
            collapsible: false,
            collapsed: false,
            trailing: None,
        }
    }

    /// Shows the chevron; `collapsed` turns it to point right.
    pub fn collapsible(mut self, id: impl Into<ElementId>, collapsed: bool) -> Self {
        self.id = Some(id.into());
        self.collapsible = true;
        self.collapsed = collapsed;
        self
    }

    /// The count after the title in `text-3` (`待办 3`), hidden at 0.
    pub fn count(mut self, count: u32) -> Self {
        self.count = Some(count).filter(|&n| n > 0);
        self
    }

    pub fn trailing(mut self, el: impl IntoElement) -> Self {
        self.trailing = Some(el.into_any_element());
        self
    }
}

impl RenderOnce for SectionHeader {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let k = theme::colors(cx);
        let chevron = if self.collapsed {
            Sym::ChevronRight
        } else {
            Sym::ChevronDown
        };
        div()
            .id(self
                .id
                .unwrap_or_else(|| ElementId::Name(self.label.clone())))
            .flex_none()
            .w_full()
            .h(size::SECTION_HEADER)
            .px(space::S8)
            .flex()
            .items_center()
            .gap(space::S6)
            .when(self.collapsible, |el| {
                el.child(icon(chevron, theme::icon::TAG, k.text_3))
            })
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.))
                    .flex()
                    .items_center()
                    .gap(space::S6)
                    .child(
                        div()
                            .min_w(px(0.))
                            .t_section()
                            .text_color(k.text_2)
                            .one_line()
                            .child(self.label),
                    )
                    .when_some(self.count, |el, n| {
                        el.child(
                            div()
                                .flex_none()
                                .t_section()
                                .tnum()
                                .text_color(k.text_3)
                                .child(CountBadge::cap(n)),
                        )
                    }),
            )
            .children(self.trailing)
    }
}

/// A label in a side panel or popup section (§8.1): 12/16 600 `text-3` with
/// the count inline (`产出 · 2`).
pub fn panel_label(label: impl Into<SharedString>, count: Option<u32>, cx: &App) -> gpui::Div {
    let k = theme::colors(cx);
    let text: SharedString = match count {
        Some(n) => format!("{} · {}", label.into(), CountBadge::cap(n)).into(),
        None => label.into(),
    };
    div()
        .t_section()
        .tnum()
        .text_color(k.text_3)
        .one_line()
        .child(text)
}

/// The 8 pt unread dot in the left padding of a topic row (§7.6).
pub fn unread_dot(cx: &App) -> gpui::Div {
    div()
        .flex_none()
        .size(px(8.))
        .rounded_full()
        .bg(theme::colors(cx).unread_dot)
}

/// A count element for a row, by priority: a red `@n` badge, else the
/// neutral unread badge (§7.6).
pub fn row_count(unread: u32, mentions: u32) -> Option<CountBadge> {
    if mentions > 0 {
        Some(CountBadge::new(mentions, BadgeKind::Mention))
    } else if unread > 0 {
        Some(CountBadge::new(unread, BadgeKind::Unread))
    } else {
        None
    }
}

/// A tab in the 40 pt band under the topic list title (§8.1): `ui` 14/20
/// `text-2`, count 13 `text-3`; active `text-1` 600 with a 2 pt underline
/// on the bottom hairline.
pub fn tab(
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    count: u32,
    active: bool,
    cx: &App,
) -> gpui::Stateful<gpui::Div> {
    let k = theme::colors(cx);
    div()
        .id(id)
        .flex_none()
        .h_full()
        .relative()
        .flex()
        .items_center()
        .gap(space::S6)
        .t_ui()
        .text_color(if active { k.text_1 } else { k.text_2 })
        .when(active, |el| el.semibold())
        .child({
            // A hidden 600 copy reserves the bold width, so switching tabs
            // never moves the tabs after this one (§12: tabs never move).
            let label: SharedString = label.into();
            div()
                .flex()
                .flex_col()
                .items_center()
                .child(label.clone())
                .child(div().h_0().overflow_hidden().semibold().child(label))
        })
        .when(count > 0, |el| {
            el.child(
                div()
                    .t_small()
                    .font_weight(gpui::FontWeight::NORMAL)
                    .tnum()
                    .text_color(k.text_3)
                    .child(CountBadge::cap(count)),
            )
        })
        // v12/kit: never underline tabs (NATIVE.md §5.5); the active tab
        // reads by its weight until the screen moves to a segmented control.
}

/// The height of a row kind, for virtual lists.
pub fn row_height(kind: RowKind) -> Pixels {
    match kind {
        RowKind::Sidebar => size::SIDEBAR_ROW,
        RowKind::Topic => size::TOPIC_ROW,
        RowKind::Menu => size::MENU_ROW,
        RowKind::Popup => size::POPUP_ROW,
        RowKind::Switcher => size::SWITCHER_ROW,
        RowKind::Settings => size::SETTINGS_ROW,
        RowKind::Panel => size::PANEL_ROW,
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RowKind {
    Sidebar,
    Topic,
    Menu,
    Popup,
    Switcher,
    Settings,
    Panel,
}
