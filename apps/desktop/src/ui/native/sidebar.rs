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
    // --- v13/ux-pages ---
    /// The right column as one element (a count pill or a StatusBadge).
    trailing_el: Option<AnyElement>,
    /// A row that opens to child rows: `Some(open)`. Open shows the
    /// chevron in the right column; closed shows it on hover.
    disclosure: Option<bool>,
    /// A collapsed tail (`空闲 · 3`): the row at 70 % opacity.
    dim: bool,
    // --- end v13/ux-pages ---
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
            trailing_el: None,
            disclosure: None,
            dim: false,
        }
    }

    // --- v13/ux-pages ---
    /// The right column as one element: a count pill or a StatusBadge
    /// (COMPLETENESS.md §2.1, §2.12).
    pub fn trailing_element(mut self, el: impl IntoElement) -> Self {
        self.trailing_el = Some(el.into_any_element());
        self
    }

    /// The row opens to child rows; `open` says whether it is open now.
    pub fn disclosure(mut self, open: bool) -> Self {
        self.disclosure = Some(open);
        self
    }

    /// A collapsed tail item, drawn at 70 % opacity.
    pub fn dim(mut self, dim: bool) -> Self {
        self.dim = dim;
        self
    }
    // --- end v13/ux-pages ---

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
        // The right column: the chevron of an open row; else the element
        // (or nothing), with the chevron fading in on hover.
        let chevron = |open: bool| {
            symbol(
                if open {
                    Sf::ChevronDown
                } else {
                    Sf::ChevronRight
                },
                px(11.),
                Weight::Semibold,
                k.secondary,
                window,
            )
        };
        // A row with a disclosure keeps its count in the right column, where
        // the chevron replaces it on hover.
        let (count, trailing_el) = match (self.disclosure, self.trailing_el, self.count) {
            (Some(_), None, Some(n)) => (None, Some(count_capsule(n, cx).into_any_element())),
            (_, el, n) => (n, el),
        };
        let right: Option<AnyElement> = match (self.disclosure, trailing_el) {
            (Some(true), _) => Some(
                div()
                    .flex_none()
                    .min_w(px(16.))
                    .flex()
                    .justify_end()
                    .child(chevron(true))
                    .into_any_element(),
            ),
            (Some(false), el) => Some(
                div()
                    .flex_none()
                    .relative()
                    .min_w(px(16.))
                    .h_full()
                    .flex()
                    .items_center()
                    .justify_end()
                    .children(el.map(|el| {
                        div()
                            .group_hover(SIDEBAR_ROW_GROUP, |s| s.opacity(0.))
                            .child(el)
                    }))
                    .child(
                        div()
                            .absolute()
                            .top_0()
                            .bottom_0()
                            .right_0()
                            .flex()
                            .items_center()
                            .opacity(0.)
                            .group_hover(SIDEBAR_ROW_GROUP, |s| s.opacity(1.))
                            .child(chevron(false)),
                    )
                    .into_any_element(),
            ),
            (None, el) => el,
        };
        div()
            .id(self.id)
            .group(SIDEBAR_ROW_GROUP)
            .when(self.dim, |d| d.opacity(0.7))
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
            .when_some(count, |d, n| d.child(count_capsule(n, cx)))
            .children(right)
            .when_some(self.on_click, |d, f| {
                d.on_click(move |e, w, cx| f(e, w, cx))
            })
    }
}

/// The pinned user footer of the sidebar (COMPLETENESS.md §2.1, F04): h48,
/// p 8, a 32 avatar, the name 14 medium over a 12 muted second line (role
/// or workspace, or the offline state), `chevrons-up-down` 16 at the right.
/// It opens account and Settings.
#[derive(IntoElement)]
pub struct SidebarFooter {
    id: ElementId,
    avatar: AnyElement,
    name: SharedString,
    second: Option<AnyElement>,
    on_click: Option<Click>,
}

impl SidebarFooter {
    pub fn new(
        id: impl Into<ElementId>,
        avatar: impl IntoElement,
        name: impl Into<SharedString>,
    ) -> Self {
        Self {
            id: id.into(),
            avatar: avatar.into_any_element(),
            name: name.into(),
            second: None,
            on_click: None,
        }
    }

    /// The second line as 12 muted text.
    pub fn detail(mut self, text: impl Into<SharedString>, cx: &App) -> Self {
        let k = native_colors(cx);
        self.second = Some(
            div()
                .n_footnote()
                .text_color(k.secondary)
                .overflow_hidden()
                .whitespace_nowrap()
                .text_ellipsis()
                .child(text.into())
                .into_any_element(),
        );
        self
    }

    /// The second line as any element (the offline StatusBadge).
    pub fn second(mut self, el: impl IntoElement) -> Self {
        self.second = Some(el.into_any_element());
        self
    }

    pub fn on_click(mut self, f: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static) -> Self {
        self.on_click = Some(Rc::new(f));
        self
    }
}

impl RenderOnce for SidebarFooter {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let k = native_colors(cx);
        div()
            .flex_none()
            .p(metrics::SIDEBAR_ROW_INSET)
            .border_t_1()
            .border_color(k.separator)
            .child(
                div()
                    .id(self.id)
                    .h(super::tokens::height::SIDEBAR_FOOTER - metrics::SIDEBAR_ROW_INSET * 2.)
                    .px(metrics::SIDEBAR_ROW_INSET)
                    .rounded(metrics::SIDEBAR_ROW_RADIUS)
                    .hover(|s| s.bg(k.fill_hover))
                    .flex()
                    .items_center()
                    .gap(super::tokens::space::S8)
                    .child(div().flex_none().child(self.avatar))
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(0.))
                            .flex()
                            .flex_col()
                            .child(
                                div()
                                    .n_headline()
                                    .text_color(k.label)
                                    .overflow_hidden()
                                    .whitespace_nowrap()
                                    .text_ellipsis()
                                    .child(self.name),
                            )
                            .children(self.second),
                    )
                    .child(symbol(
                        Sf::Popup,
                        super::tokens::icon::MD,
                        Weight::Regular,
                        k.secondary,
                        window,
                    ))
                    .when_some(self.on_click, |d, f| {
                        d.on_click(move |e, w, cx| f(e, w, cx))
                    }),
            )
    }
}

/// The sidebar count: the count pill (20, `muted` fill, tabular figures).
fn count_capsule(n: u32, _cx: &App) -> gpui::Div {
    // COMPLETENESS.md §1.8: the one count pill.
    div().flex_none().child(super::badge::CountPill::new(n))
}

/// The hover group of a sidebar row (its disclosure chevron).
const SIDEBAR_ROW_GROUP: &str = "sidebar-row";
