//! The rows of the rail: the state slot, 话题 rows and section headers.

use gpui::{
    AnyElement, App, ClickEvent, ElementId, InteractiveElement as _, IntoElement, ParentElement,
    SharedString, StatefulInteractiveElement as _, Styled, Window, div,
    prelude::FluentBuilder as _, px,
};

use super::{RailView, model::RowState, modes::TopicItem};
use crate::{
    data::Member,
    theme::{self, Type as _, space},
    ui::{self, Sym, MemberAvatar, Ring, RowSurface},
};

/// A row's click handler.
pub(super) type ClickHandler = Box<dyn Fn(&ClickEvent, &mut Window, &mut App) + 'static>;

/// The state dot, filled or hollow: one size for every state (§7.3).
const DOT: gpui::Pixels = theme::RAIL_STATE_DOT;
/// The widest a row's meta gets.
const META_MAX: gpui::Pixels = px(96.);
/// Indent of a 话题 under a member (mode C).
pub(super) const NESTED_PAD: gpui::Pixels = px(28.);

/// How a 话题 row shows its slot.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum SlotStyle {
    /// The doing member's tile (modes A and B).
    Tile,
    /// A breathing blue dot (mode C, where the member is the parent row).
    Dot,
}

impl RailView {
    /// The 16 pt slot at the start of a 话题 row; `tile` is the doing
    /// member.
    pub(super) fn slot(
        &self,
        state: &RowState,
        tile: Option<&Member>,
        style: SlotStyle,
        selected: bool,
        cx: &App,
    ) -> AnyElement {
        let k = theme::colors(cx);
        let surface = if selected { k.rail_sel } else { k.rail };
        let center = |el: AnyElement| {
            div()
                .flex_none()
                .w(theme::RAIL_SLOT)
                .h(theme::RAIL_SLOT)
                .flex()
                .items_center()
                .justify_center()
                .child(el)
                .into_any_element()
        };
        match state {
            // NATIVE.md §6: needs you is `exclamationmark.circle.fill` in amber.
            RowState::NeedsYou => {
                center(ui::sym(Sym::NeedsYou, ui::SymSize::INLINE, k.you).into_any_element())
            }
            RowState::Working(_) if style == SlotStyle::Tile => center(
                MemberAvatar::new(tile, theme::RAIL_SLOT)
                    .ring(Some(Ring::Live))
                    .on(surface)
                    .into_any_element(),
            ),
            RowState::Working(_) => {
                center(div().size(DOT).rounded_full().bg(k.live).into_any_element())
            }
            // Done is quiet: an `ink-3` check, not a green one (§7.3).
            RowState::Done => {
                center(ui::sym(Sym::Check, ui::SymSize::INLINE_SMALL, k.ink_3).into_any_element())
            }
            // Waiting on someone: `clock` in `secondary` (§6).
            RowState::Waiting => {
                center(ui::sym(Sym::Waiting, ui::SymSize::INLINE, k.ink_3).into_any_element())
            }
            RowState::Idle => center(div().into_any_element()),
        }
    }

    /// A 话题 row: slot, title (quiet rows in `ink-2`), meta. The indent is
    /// the left padding (10, or 28 under a member).
    pub(super) fn topic_row(&self, item: &TopicItem, cx: &App) -> AnyElement {
        let TopicItem {
            key,
            topic,
            state,
            meta,
            meta_you,
            selected,
            style,
            indent,
            tile,
            ..
        } = item;
        let (meta_you, selected, style, indent) = (*meta_you, *selected, *style, *indent);
        let meta = meta.clone();
        let k = theme::colors(cx);
        let nav = self.services.nav.clone();
        let (g, t) = (topic.group_id.clone(), topic.id.clone());
        let title = crate::views::sidebar::topic_display_title(&topic.title).0;
        let quiet = state.quiet() && style == SlotStyle::Tile;
        ui::list_row(
            SharedString::from(format!("rail-{key}-{}", topic.id)),
            RowSurface::Rail,
            selected,
            cx,
        )
        .h(theme::RAIL_ROW)
        .pl(indent)
        .pr(space::S10)
        .gap(space::S8)
        .child(self.slot(state, tile.as_ref(), style, selected, cx))
        .child(
            div()
                .flex_1()
                .min_w_0()
                .one_line()
                .t_small()
                .text_color(if quiet && !selected { k.ink_2 } else { k.ink })
                .when(selected, |el| el.medium())
                .child(title),
        )
        .children(meta.map(|m| {
            // A long name (`等<long name>`) never takes the title's room.
            div()
                .flex_none()
                .max_w(META_MAX)
                .one_line()
                .map(|el| if m.mono { el.t_mono() } else { el.t_meta() })
                .text_color(if meta_you { k.you_ink } else { k.ink_3 })
                .child(m.text)
        }))
        .on_click(move |_, _, cx| {
            let (g, t) = (g.clone(), t.clone());
            nav.update(cx, |nav, cx| nav.open_topic(g, t, cx))
        })
        .into_any_element()
    }

    /// A section header (`⌄ 开发`, `⌄ • 需要你 4`): 26 pt, 12/500 `ink-3`.
    /// The chevron folds the section; the rest of the row runs `open` when
    /// given (a 群 opens its page), else it folds too.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn section_header(
        &self,
        id: impl Into<ElementId>,
        label: String,
        count: Option<usize>,
        you_dot: bool,
        folded: bool,
        selected: bool,
        fold: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
        open: Option<ClickHandler>,
        cx: &App,
    ) -> AnyElement {
        let k = theme::colors(cx);
        let id: ElementId = id.into();
        let fold = std::rc::Rc::new(fold);
        let chevron_fold = fold.clone();
        let chevron = div()
            .id(ElementId::Name(format!("{id}-fold").into()))
            .flex_none()
            .size(px(14.))
            .flex()
            .items_center()
            .justify_center()
            .child(ui::icon(
                if folded {
                    Sym::ChevronRight
                } else {
                    Sym::ChevronDown
                },
                px(12.),
                k.ink_3,
            ))
            .on_click(move |e, window, cx| {
                cx.stop_propagation();
                chevron_fold(e, window, cx)
            });
        ui::list_row(id, RowSurface::Rail, selected, cx)
            .h(theme::RAIL_GROUP_ROW)
            .pl(space::S8)
            .pr(space::S10)
            .gap(space::S6)
            .rounded(px(7.))
            .t_meta_medium()
            .text_color(if selected { k.ink } else { k.ink_3 })
            .child(chevron)
            .when(you_dot, |el| {
                el.child(div().flex_none().size(px(6.)).rounded_full().bg(k.you))
            })
            .child(div().flex_shrink(1.).min_w_0().one_line().child(label))
            .children(count.map(|n| {
                div()
                    .ml(space::S2)
                    .t_mono()
                    .text_color(k.ink_3)
                    .child(n.to_string())
            }))
            .on_click(move |e, window, cx| match &open {
                Some(open) => open(e, window, cx),
                None => fold(e, window, cx),
            })
            .into_any_element()
    }
}
