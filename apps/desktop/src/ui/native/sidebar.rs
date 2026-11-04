//! The floating sidebar (NATIVE.md §2, §9.1): the glass panel frame,
//! section headers with a disclosure chevron on hover, and 32 pt rows with
//! the Tahoe selection (gray fill, accent symbol and text, no hover).

use std::rc::Rc;

use gpui::{
    AnyElement, App, ClickEvent, ElementId, Hsla, InteractiveElement as _, IntoElement,
    ParentElement, RenderOnce, SharedString, StatefulInteractiveElement as _, Styled, Window, div,
    prelude::FluentBuilder as _, px,
};

use super::tokens::{NativeType as _, emphasized, metrics, native_colors};
use crate::ui::sf_symbols::{Sf, Weight, symbol};

type Click = Rc<dyn Fn(&ClickEvent, &mut Window, &mut App)>;

/// The floating sidebar panel (§2): inset 8 pt from the window's top,
/// leading and bottom edges, radius 18, `sidebar` fill, 0.5 pt `glassEdge`
/// and the `float` shadow. Children go inside; the traffic lights sit in
/// its top row.
pub fn floating_sidebar(cx: &App) -> gpui::Div {
    let k = native_colors(cx);
    div()
        .flex_none()
        .m(metrics::SIDEBAR_INSET)
        .w(metrics::SIDEBAR_WIDTH)
        .rounded(metrics::SIDEBAR_RADIUS)
        .bg(k.sidebar)
        .shadow(k.glass())
        .overflow_hidden()
        .flex()
        .flex_col()
}

/// A section header (§9.1): Section style (11 pt semibold, `secondary`)
/// with a disclosure chevron that shows on hover at the trailing edge.
#[derive(IntoElement)]
pub struct SidebarSection {
    id: ElementId,
    label: SharedString,
    collapsed: bool,
    on_toggle: Option<Click>,
    /// Show the chevron without hover (gallery).
    show_chevron: bool,
}

impl SidebarSection {
    pub fn new(id: impl Into<ElementId>, label: impl Into<SharedString>) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            collapsed: false,
            on_toggle: None,
            show_chevron: false,
        }
    }

    pub fn collapsed(mut self, collapsed: bool) -> Self {
        self.collapsed = collapsed;
        self
    }

    pub fn on_toggle(mut self, f: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static) -> Self {
        self.on_toggle = Some(Rc::new(f));
        self
    }

    pub fn show_chevron(mut self, show: bool) -> Self {
        self.show_chevron = show;
        self
    }
}

impl RenderOnce for SidebarSection {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let k = native_colors(cx);
        let group: SharedString = format!("native-section-{:?}", self.id).into();
        let chevron = if self.collapsed {
            Sf::ChevronRight
        } else {
            Sf::ChevronDown
        };
        div()
            .id(self.id)
            .group(group.clone())
            .flex_none()
            .h(metrics::SIDEBAR_SECTION)
            .mx(metrics::SIDEBAR_ROW_INSET)
            .px(px(8.))
            .pt(px(8.))
            .flex()
            .items_center()
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.))
                    .n_section()
                    .text_color(k.secondary)
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .text_ellipsis()
                    .child(self.label),
            )
            .child(
                div()
                    .flex_none()
                    .when(!self.show_chevron, |d| {
                        d.opacity(0.).group_hover(group, |s| s.opacity(1.))
                    })
                    .child(symbol(
                        chevron,
                        px(11.),
                        Weight::Semibold,
                        k.secondary,
                        window,
                    )),
            )
            .when_some(self.on_toggle, |d, f| {
                d.on_click(move |e, w, cx| f(e, w, cx))
            })
    }
}

/// What sits in the 18 pt leading slot of a sidebar row.
pub enum RowLeading {
    None,
    /// A 15 pt symbol: `secondary`, `accent` when selected; or a state color
    /// when the symbol is the state.
    Symbol(Sf, Option<Hsla>),
    /// Any element (a 16 pt member avatar).
    Element(AnyElement),
}

/// A 32 pt sidebar row (§9.1). Selection: a rounded rect inset 10 pt from
/// the panel sides, radius 7, `selection` fill; its symbol and text use
/// `accent`. No hover fill. An inactive window draws the text `label`.
#[derive(IntoElement)]
pub struct SidebarRow {
    id: ElementId,
    title: SharedString,
    leading: RowLeading,
    selected: bool,
    unread: bool,
    trailing: Option<SharedString>,
    dot: Option<Hsla>,
    count: Option<u32>,
    inactive: Option<bool>,
    on_click: Option<Click>,
}

impl SidebarRow {
    pub fn new(id: impl Into<ElementId>, title: impl Into<SharedString>) -> Self {
        Self {
            id: id.into(),
            title: title.into(),
            leading: RowLeading::None,
            selected: false,
            unread: false,
            trailing: None,
            dot: None,
            count: None,
            inactive: None,
            on_click: None,
        }
    }

    pub fn symbol(mut self, sf: Sf) -> Self {
        self.leading = RowLeading::Symbol(sf, None);
        self
    }

    /// A state symbol in its state color (needs you: `amber`).
    pub fn state_symbol(mut self, sf: Sf, color: Hsla) -> Self {
        self.leading = RowLeading::Symbol(sf, Some(color));
        self
    }

    pub fn leading(mut self, el: impl IntoElement) -> Self {
        self.leading = RowLeading::Element(el.into_any_element());
        self
    }

    pub fn selected(mut self, selected: bool) -> Self {
        self.selected = selected;
        self
    }

    /// Unread: the title in Semibold.
    pub fn unread(mut self, unread: bool) -> Self {
        self.unread = unread;
        self
    }

    /// Trailing 11 pt `secondary` text (a time).
    pub fn trailing(mut self, text: impl Into<SharedString>) -> Self {
        self.trailing = Some(text.into());
        self
    }

    /// A 6 pt state dot at the trailing edge (§6: blue live work, amber
    /// needs you).
    pub fn trailing_dot(mut self, color: Hsla) -> Self {
        self.dot = Some(color);
        self
    }

    /// The unread count: a gray capsule, 11 pt (hidden at 0).
    pub fn count(mut self, n: u32) -> Self {
        self.count = Some(n).filter(|&n| n > 0);
        self
    }

    pub fn inactive(mut self, inactive: bool) -> Self {
        self.inactive = Some(inactive);
        self
    }

    pub fn on_click(mut self, f: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static) -> Self {
        self.on_click = Some(Rc::new(f));
        self
    }
}

impl RenderOnce for SidebarRow {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let k = native_colors(cx);
        let active = self
            .inactive
            .map(|i| !i)
            .unwrap_or_else(|| emphasized(window));
        let (sel_symbol, sel_text) = k.selected_ink(active);
        let text = if self.selected { sel_text } else { k.label };
        let leading: Option<AnyElement> = match self.leading {
            RowLeading::None => None,
            RowLeading::Symbol(sf, state) => {
                let color = state.unwrap_or(if self.selected {
                    sel_symbol
                } else {
                    k.secondary
                });
                Some(symbol(
                    sf,
                    metrics::SIDEBAR_SYMBOL,
                    Weight::Regular,
                    color,
                    window,
                ))
            }
            RowLeading::Element(el) => Some(el),
        };
        div()
            .id(self.id)
            .flex_none()
            .h(metrics::SIDEBAR_ROW)
            .mx(metrics::SIDEBAR_ROW_INSET)
            .px(px(6.))
            .rounded(metrics::SIDEBAR_ROW_RADIUS)
            .flex()
            .items_center()
            .gap(px(6.))
            .when(self.selected, |d| d.bg(k.selection))
            .child(
                div()
                    .flex_none()
                    .size(metrics::SIDEBAR_SLOT)
                    .flex()
                    .items_center()
                    .justify_center()
                    .children(leading),
            )
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.))
                    .n_body()
                    .when(self.unread, |d| d.font_weight(gpui::FontWeight::SEMIBOLD))
                    .text_color(text)
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .text_ellipsis()
                    .child(self.title),
            )
            .when_some(self.trailing, |d, t| {
                d.child(
                    div()
                        .flex_none()
                        .n_subheadline()
                        .n_figures()
                        .text_color(k.secondary)
                        .child(t),
                )
            })
            .when_some(self.dot, |d, color| {
                d.child(
                    div()
                        .flex_none()
                        .w(px(16.))
                        .flex()
                        .justify_center()
                        .child(div().size(px(6.)).rounded_full().bg(color)),
                )
            })
            .when_some(self.count, |d, n| {
                d.child(
                    div()
                        .flex_none()
                        .h(px(16.))
                        .min_w(px(16.))
                        .px(px(5.))
                        .rounded_full()
                        .bg(k.quaternary)
                        .flex()
                        .items_center()
                        .justify_center()
                        .n_subheadline()
                        .n_figures()
                        .text_color(k.secondary)
                        .child(crate::ui::chip::CountBadge::cap(n)),
                )
            })
            .when_some(self.on_click, |d, f| {
                d.on_click(move |e, w, cx| f(e, w, cx))
            })
    }
}
