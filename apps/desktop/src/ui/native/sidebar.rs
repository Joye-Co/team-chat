//! The floating sidebar (NATIVE.md §2, §9.1): the glass panel frame,
//! section headers with a disclosure chevron on hover, and 32 pt rows with
//! the Tahoe selection (gray fill, accent symbol and text) and the row hover
//! (muted 50 %).

use std::rc::Rc;

use gpui::{
    AnyElement, App, Bounds, ClickEvent, Context, ElementId, Hsla, InteractiveElement as _,
    IntoElement, ParentElement, Pixels, RenderOnce, SharedString, StatefulInteractiveElement as _,
    Styled, Window, div, prelude::FluentBuilder as _, px,
};
use gpui_component::menu::{ContextMenuExt as _, PopupMenu};

use super::{
    badge::{BadgeState, StatusBadge},
    focus::{Arrows, FocusRow as _},
    tokens::{NativeType as _, emphasized, metrics, native_colors},
};
use crate::{
    theme::MEDIUM_WEIGHT,
    ui::{
        sf_symbols::{Sf, Weight, symbol},
        truncate::TruncText,
    },
};

type Click = Rc<dyn Fn(&ClickEvent, &mut Window, &mut App)>;

/// Builds the context menu of a sidebar item.
pub type MenuBuilder = Rc<dyn Fn(PopupMenu, &mut Window, &mut Context<PopupMenu>) -> PopupMenu>;

/// A right-click menu on a sidebar item (NATIVE.md §7.3): the menu stays
/// inside `limits` (the sidebar panel, in window coordinates), and while it
/// is open the item it acts on keeps the row focus ring.
#[derive(Clone)]
pub struct RowMenu {
    pub build: MenuBuilder,
    pub limits: Option<Bounds<Pixels>>,
}

impl RowMenu {
    pub fn new(
        build: impl Fn(PopupMenu, &mut Window, &mut Context<PopupMenu>) -> PopupMenu + 'static,
        limits: Option<Bounds<Pixels>>,
    ) -> Self {
        Self {
            build: Rc::new(build),
            limits,
        }
    }

    /// Puts the menu on `trigger`, which must have the item's own id and
    /// the item's shape (the ring follows its corners).
    fn attach<E>(self, trigger: E, cx: &App) -> AnyElement
    where
        E: gpui::InteractiveElement + ParentElement + Styled + IntoElement + 'static,
    {
        let ring = super::focus::row_focus_ring(cx);
        let build = self.build;
        let menu = trigger
            .context_menu(move |menu, window, cx| build(menu, window, cx))
            .open_style(|s| s.shadow(ring));
        match self.limits {
            Some(limits) => menu.menu_limits(limits).into_any_element(),
            None => menu.into_any_element(),
        }
    }
}

/// The floating sidebar panel (§2): inset 8 pt from the window's top,
/// leading and bottom edges, radius 18, `sidebar` fill, 0.5 pt `glassEdge`
/// and the `float` shadow. Children go inside; the traffic lights sit in
/// its top row.
pub fn floating_sidebar(width: gpui::Pixels, cx: &App) -> gpui::Div {
    let k = native_colors(cx);
    div()
        .flex_none()
        .m(metrics::SIDEBAR_INSET)
        .w(width)
        .rounded(metrics::SIDEBAR_RADIUS)
        .bg(k.sidebar)
        .shadow(k.glass())
        .overflow_hidden()
        .flex()
        .flex_col()
}

/// The `bell.slash` after a muted 群, 私聊 or 话题 title.
const MUTED_SYMBOL: Pixels = px(11.);

/// The hit width of a section header's chevron (the trailing 34 pt).
const SECTION_CHEVRON_HIT: Pixels = px(34.);

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
    /// The section's own page is open (a 群 header on its 群 page): the
    /// label row takes the selected look (D10).
    selected: bool,
    /// A click on the chevron runs this instead of `on_toggle`.
    on_chevron: Option<Click>,
    menu: Option<RowMenu>,
    /// I muted this 群: a `bell.slash` after the label.
    muted: bool,
}

impl SidebarSection {
    pub fn new(id: impl Into<ElementId>, label: impl Into<SharedString>) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            collapsed: false,
            on_toggle: None,
            show_chevron: false,
            selected: false,
            on_chevron: None,
            menu: None,
            muted: false,
        }
    }

    /// I muted this 群: the label gets a `bell.slash` after it.
    pub fn muted(mut self, muted: bool) -> Self {
        self.muted = muted;
        self
    }

    /// A click on the chevron runs `f` (fold) while a click on the label
    /// runs `on_toggle` (a 群 header opens its page).
    pub fn on_chevron(mut self, f: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static) -> Self {
        self.on_chevron = Some(Rc::new(f));
        self
    }

    /// A right-click menu on the label row.
    pub fn menu(mut self, menu: RowMenu) -> Self {
        self.menu = Some(menu);
        self
    }

    /// T-12: the selected look (D10): the muted fill under the label row
    /// and the label in the selected ink, as a selected row.
    pub fn selected(mut self, selected: bool) -> Self {
        self.selected = selected;
        self
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
        let (_, sel_text) = k.selected_ink(emphasized(window));
        let label = if self.selected { sel_text } else { k.secondary };
        let row = div()
            .id(ElementId::Name(format!("{group}-label").into()))
            .relative()
            .size_full()
            .px(px(8.))
            .rounded(metrics::SIDEBAR_ROW_RADIUS)
            .when(self.selected, |d| d.bg(k.selection))
            .flex()
            .items_center()
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .items_center()
                    .gap(px(4.))
                    .child(TruncText::new(self.label).n_section().text_color(label))
                    .when(self.muted, |d| {
                        d.child(div().flex_none().child(symbol(
                            Sf::Muted,
                            MUTED_SYMBOL,
                            Weight::Regular,
                            k.secondary,
                            window,
                        )))
                    }),
            )
            .child(
                div()
                    .flex_none()
                    .when(!self.show_chevron, |d| {
                        d.opacity(0.).group_hover(group.clone(), |s| s.opacity(1.))
                    })
                    .child(symbol(
                        chevron,
                        px(11.),
                        Weight::Semibold,
                        k.secondary,
                        window,
                    )),
            )
            // The chevron's own hit area folds the section, whatever the
            // label opens.
            .when_some(self.on_chevron, |d, f| {
                d.child(
                    div()
                        .id(ElementId::Name(format!("{group}-fold").into()))
                        .absolute()
                        .top_0()
                        .bottom_0()
                        .right_0()
                        .w(SECTION_CHEVRON_HIT)
                        .on_click(move |e, window, cx| {
                            cx.stop_propagation();
                            f(e, window, cx)
                        }),
                )
            });
        let row = match self.menu {
            Some(menu) => menu.attach(row, cx),
            None => row.into_any_element(),
        };
        div()
            .id(self.id)
            .group(group.clone())
            .flex_none()
            .h(metrics::SIDEBAR_SECTION)
            .mx(metrics::SIDEBAR_ROW_INSET)
            .pt(px(8.))
            .child(row)
            .when_some(self.on_toggle, |d, f| {
                d.on_click(move |e, w, cx| f(e, w, cx))
            })
    }
}

/// The one media kind of a sidebar group (COMPLETENESS.md §2.1, T1-08):
/// inside one group every item has a 16 media of this kind, or none has.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SidebarMedia {
    /// No media column: the title starts at the slot x.
    #[default]
    None,
    /// A 16 symbol (群, pages).
    Icon,
    /// A 16 avatar (私聊, members).
    Avatar,
}

/// The right column of a sidebar item (§2.1, §2.12, T1-08): one kind at
/// x 238. State goes here as a StatusBadge, never as a colored mark in the
/// media column.
#[derive(Clone, Debug, Default)]
pub enum RightColumn {
    #[default]
    None,
    /// The unread count pill (hidden at 0).
    Count(u32),
    /// One StatusBadge (等你, 在做 3/5, 失败).
    Status(BadgeState, SharedString),
    /// A time, 12 muted tabular.
    Time(SharedString),
}

/// What sits in the 16 leading slot of a sidebar row.
pub enum RowLeading {
    None,
    /// A 15 pt symbol: `secondary`, `accent` when selected; or a state color
    /// when the symbol is the state.
    Symbol(Sf, Option<Hsla>),
    /// Any element (a 16 pt member avatar).
    Element(AnyElement),
}

/// A 32 pt sidebar row (§9.1, COMPLETENESS.md §2.1). Selection: a rounded
/// rect inset 8 from the panel sides, `selection` (muted) fill; its symbol
/// and text use `accent`. Hover: muted at 50 %, drawn at once. Padding 8 and
/// gap 8, so the title of a row without media starts at the x of the group
/// label. A row with `on_click` is a [`FocusRow`]: a tab stop with the focus
/// ring, Enter / Space open it, ↑ / ↓ move the focus. An inactive window
/// draws the text `label`.
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
    /// The one right column (T1-08).
    right: RightColumn,
    /// The media kind of the group the row sits in; set by
    /// [`SidebarGroup`].
    media: Option<SidebarMedia>,
    menu: Option<RowMenu>,
    /// I muted this 群, 私聊 or 话题: a `bell.slash` after the title.
    muted: bool,
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
            right: RightColumn::None,
            media: None,
            menu: None,
            muted: false,
        }
    }

    /// I muted what the row opens: a `bell.slash` after the title.
    pub fn muted(mut self, muted: bool) -> Self {
        self.muted = muted;
        self
    }

    /// A right-click menu on the row; the row keeps its focus ring while
    /// the menu is open.
    pub fn menu(mut self, menu: RowMenu) -> Self {
        self.menu = Some(menu);
        self
    }

    /// The right column: a count pill, a StatusBadge or a time (T1-08).
    pub fn right(mut self, right: RightColumn) -> Self {
        self.right = right;
        self
    }

    /// The media kind of the row's group: a row whose media is of another
    /// kind keeps an empty slot of the group's kind, so titles share one x.
    pub fn media_kind(mut self, media: SidebarMedia) -> Self {
        self.media = Some(media);
        self
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

    /// Older API: a state symbol in its state color. State belongs in the
    /// right column ([`SidebarRow::right`] with a StatusBadge, §2.12); a
    /// [`SidebarGroup`] of icons draws this symbol in `secondary`.
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

    /// Older API: a 6 pt state dot at the trailing edge. A free colored dot
    /// is not a status carrier (§2.12); use [`SidebarRow::right`].
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
        // One media kind per group (§2.1): a row of a group drops media of
        // another kind and keeps an empty slot; state colors leave the
        // media column.
        let in_group = self.media.is_some();
        let leading = match (self.media, self.leading) {
            (Some(SidebarMedia::None), _) => RowLeading::None,
            (Some(SidebarMedia::Icon), RowLeading::Symbol(sf, _)) => RowLeading::Symbol(sf, None),
            (Some(SidebarMedia::Icon), _)
            | (Some(SidebarMedia::Avatar), RowLeading::Symbol(..)) => {
                RowLeading::Element(div().into_any_element())
            }
            (_, l) => l,
        };
        let slot = !matches!(self.media, Some(SidebarMedia::None));
        let _ = in_group;
        let leading: Option<AnyElement> = match leading {
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
        let (trailing_text, count, trailing_el) = match self.right {
            RightColumn::None => (self.trailing, self.count, self.trailing_el),
            RightColumn::Count(n) => (None, Some(n).filter(|&n| n > 0), None),
            RightColumn::Status(state, label) => (
                None,
                None,
                Some(
                    StatusBadge::new(state, label)
                        .max_w(px(120.))
                        .into_any_element(),
                ),
            ),
            RightColumn::Time(t) => (Some(t), None, None),
        };
        let (count, trailing_el) = match (self.disclosure, trailing_el, count) {
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
        let row = div()
            .id(self.id)
            .group(SIDEBAR_ROW_GROUP)
            .when(self.dim, |d| d.opacity(0.7))
            .flex_none()
            .h(metrics::SIDEBAR_ROW)
            .mx(metrics::SIDEBAR_ROW_INSET)
            .px(px(8.))
            .rounded(metrics::SIDEBAR_ROW_RADIUS)
            .flex()
            .items_center()
            .gap(px(8.))
            .when(self.selected, |d| d.bg(k.selection))
            .map(|d| {
                if self.on_click.is_some() {
                    d.focus_row(self.selected, cx)
                } else {
                    d.row_hover(self.selected, cx)
                }
            })
            .when(slot, |d| {
                d.child(
                    div()
                        .flex_none()
                        .size(metrics::SIDEBAR_SLOT)
                        .flex()
                        .items_center()
                        .justify_center()
                        .children(leading),
                )
            })
            .child(
                // D10: the selected item is the muted fill and 500; unread
                // is 600 with the 8 info dot by the title (§2.12, T-20). A
                // cut title shows the whole title in a tooltip.
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .child(
                        TruncText::new(self.title)
                            .n_body()
                            .when(self.selected, |d| d.font_weight(MEDIUM_WEIGHT))
                            .when(self.unread, |d| d.font_weight(gpui::FontWeight::SEMIBOLD))
                            .text_color(text),
                    )
                    .when(self.muted, |d| {
                        d.child(div().flex_none().child(symbol(
                            Sf::Muted,
                            MUTED_SYMBOL,
                            Weight::Regular,
                            if self.selected {
                                sel_symbol
                            } else {
                                k.secondary
                            },
                            window,
                        )))
                    })
                    .when(self.unread, |d| {
                        d.child(div().flex_none().size(px(8.)).rounded_full().bg(k.blue))
                    }),
            )
            .when_some(trailing_text, |d, t| {
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
            });
        match self.menu {
            Some(menu) => menu.attach(row, cx),
            None => row.into_any_element(),
        }
    }
}

/// The pinned user footer of the sidebar (COMPLETENESS.md §2.1, F04): h48,
/// p 8, a 32 avatar, the name 14 medium over a 12 muted second line (role
/// or workspace, or the offline state), the 设置 gear 16 at the right. It
/// opens and closes the settings window (the sidebar's only 设置 entry).
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
            TruncText::new(text)
                .flex_1()
                .n_footnote()
                .text_color(k.secondary)
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
                    .focus_row_with(false, Arrows::Owner, cx)
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
                            .child(TruncText::new(self.name).n_headline().text_color(k.label))
                            // A badge on the second line hugs its text
                            // (T1-08): the row does not stretch it.
                            .children(
                                self.second
                                    .map(|el| div().flex().items_center().min_w(px(0.)).child(el)),
                            ),
                    )
                    .child(symbol(
                        Sf::Settings,
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

/// A sidebar group (§2.1, T1-08): a GroupLabel, then items that share one
/// media kind and one right-column x. An empty group draws nothing (no
/// label with no rows).
#[derive(IntoElement)]
pub struct SidebarGroup {
    header: Option<SidebarSection>,
    media: SidebarMedia,
    rows: Vec<SidebarRow>,
    tail: Vec<AnyElement>,
}

impl SidebarGroup {
    pub fn new(header: SidebarSection, media: SidebarMedia) -> Self {
        Self {
            header: Some(header),
            media,
            rows: Vec::new(),
            tail: Vec::new(),
        }
    }

    /// A group with no label: only for the one group of a list.
    pub fn unlabeled(media: SidebarMedia) -> Self {
        Self {
            header: None,
            media,
            rows: Vec::new(),
            tail: Vec::new(),
        }
    }

    pub fn row(mut self, row: SidebarRow) -> Self {
        self.rows.push(row);
        self
    }

    pub fn rows(mut self, rows: impl IntoIterator<Item = SidebarRow>) -> Self {
        self.rows.extend(rows);
        self
    }

    /// An element after the rows (the "More" item, a skeleton).
    pub fn tail(mut self, el: impl IntoElement) -> Self {
        self.tail.push(el.into_any_element());
        self
    }

    pub fn is_empty(&self) -> bool {
        self.rows.is_empty() && self.tail.is_empty()
    }
}

impl RenderOnce for SidebarGroup {
    fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
        if self.is_empty() {
            return div().into_any_element();
        }
        let media = self.media;
        let collapsed = self.header.as_ref().is_some_and(|h| h.collapsed);
        div()
            .flex()
            .flex_col()
            .children(self.header)
            .when(!collapsed, |d| {
                d.children(self.rows.into_iter().map(|r| r.media_kind(media)))
                    .children(self.tail)
            })
            .into_any_element()
    }
}

/// The sidebar count: the count pill (20, `muted` fill, tabular figures).
fn count_capsule(n: u32, _cx: &App) -> gpui::Div {
    // COMPLETENESS.md §1.8: the one count pill.
    div().flex_none().child(super::badge::CountPill::new(n))
}

/// The hover group of a sidebar row (its disclosure chevron).
const SIDEBAR_ROW_GROUP: &str = "sidebar-row";
