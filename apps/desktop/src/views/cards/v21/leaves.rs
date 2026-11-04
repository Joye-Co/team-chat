//! 2.1 leaves (CardsCompose.dc.html §02, §03): every choice variant of CARD21 §4 (`decision`
//! extended, rank, rating, scale, confirm, imageChoice, matrix, slots, poll), the `compare`
//! container, inline `buttons`, the `diagram` block, the `embed` frame, the quiet box of a
//! block this version does not know, and the 2.1 footers.
//!
//! Controls are live only for the viewer the ask names (or who may send the block's
//! events); everyone else sees the same options read-only (CARD21 §5).

use crate::ui::native::FocusRow as _;
use std::{
    cell::RefCell,
    collections::HashMap,
    time::{Duration, Instant},
};

use gpui::{
    AnyElement, App, AppContext as _, Context, Div, Entity, FocusHandle, FontWeight,
    InteractiveElement as _, IntoElement, KeyDownEvent, ObjectFit, ParentElement, Render,
    SharedString, StatefulInteractiveElement as _, Styled, StyledImage as _, Subscription, Window,
    div, img, prelude::FluentBuilder as _, px,
};
use gpui_component::{
    Disableable as _,
    slider::{Slider, SliderEvent, SliderState},
};
use serde_json::{Value, json};

use super::{Node, eid, form21, interact::Emitter, question, render_node};
use crate::{
    copy::{cards21 as c, fill},
    data::{
        Millis,
        cards_v2::{
            ButtonStyle, CardActionKind, CardV2Action, CardV2ActionStyle, CardV2Block,
            ComparePreview, InputKind, PollVoters,
        },
    },
    theme::{self, Type as _, card_v2 as m, card_v2::mix, card_v21 as t, space},
    ui::{self, PushStyle, Sym, native::NativeType as _},
    views::cards::v2::{
        Ctx, blocks as b20, button, disabled_button, form::Val, meta, state::CardState,
    },
};

// ---------------------------------------------------------------------------
// Shared bits

/// Is the block's answer required (CARD21: `required?`, choices default to required).
pub fn is_required(b: &CardV2Block) -> bool {
    match b {
        CardV2Block::Decision { required, .. }
        | CardV2Block::Rank { required, .. }
        | CardV2Block::Rating { required, .. }
        | CardV2Block::Scale { required, .. }
        | CardV2Block::ImageChoice { required, .. }
        | CardV2Block::Matrix { required, .. }
        | CardV2Block::Slots { required, .. }
        | CardV2Block::Poll { required, .. } => required.unwrap_or(true),
        CardV2Block::Input { required, .. } => required.unwrap_or(false),
        CardV2Block::Confirm { .. } | CardV2Block::Compare { .. } => true,
        _ => false,
    }
}

/// The question line of a questionnaire child.
pub fn question_label(b: &CardV2Block) -> String {
    match b {
        CardV2Block::Decision { label, .. } => label.clone().unwrap_or_default(),
        CardV2Block::Rank { label, .. }
        | CardV2Block::Rating { label, .. }
        | CardV2Block::Scale { label, .. }
        | CardV2Block::Confirm { label, .. }
        | CardV2Block::ImageChoice { label, .. }
        | CardV2Block::Matrix { label, .. }
        | CardV2Block::Slots { label, .. }
        | CardV2Block::Poll { label, .. }
        | CardV2Block::Input { label, .. } => label.clone(),
        CardV2Block::Compare { question, .. } => question.clone().unwrap_or_default(),
        _ => String::new(),
    }
}

/// `单选 · 必答`, `表单 · 必答`, `可选`, `1–5 分`, `0–10`.
pub fn question_kind(b: &CardV2Block) -> String {
    let kind = match b {
        CardV2Block::Decision {
            multiple: Some(true),
            ..
        } => c::Q_MULTIPLE.to_string(),
        CardV2Block::Decision { .. } => c::Q_SINGLE.into(),
        CardV2Block::Rank { .. } => c::Q_RANK.into(),
        CardV2Block::Rating { .. } => return c::RATING_RANGE.into(),
        CardV2Block::Scale { min, max, .. } => {
            return fill(
                c::SCALE_RANGE,
                &[("min", &min.to_string()), ("max", &max.to_string())],
            );
        }
        CardV2Block::Confirm { .. } => c::Q_CONFIRM.into(),
        CardV2Block::ImageChoice { .. } => c::Q_IMAGE.into(),
        CardV2Block::Matrix { .. } => c::Q_MATRIX.into(),
        CardV2Block::Slots { .. } => c::Q_SLOTS.into(),
        CardV2Block::Poll { .. } => c::Q_POLL.into(),
        CardV2Block::Input { .. } => c::Q_FORM.into(),
        _ => String::new(),
    };
    if is_required(b) {
        format!("{kind} · {}", c::Q_REQUIRED)
    } else {
        c::Q_OPTIONAL.to_string()
    }
}

/// The question above a variant, unless a questionnaire numbers it.
fn heading(node: &Node, b: &CardV2Block, cx: &App) -> Option<Div> {
    let in_q = node
        .tree
        .parent(b.id()?)
        .is_some_and(|p| matches!(node.tree.get(p), Some(CardV2Block::Questionnaire { .. })));
    let text = question_label(b);
    // A card with one input asks it in its title.
    let inputs = node
        .ctx
        .card
        .blocks
        .iter()
        .filter(|x| x.takes_input())
        .count();
    // A question inside a report (a non-blocking ask, INTERACT §3.6) is not the title.
    let card = node.ctx.card;
    let in_report = card.intent != crate::data::cards_v2::CardIntent::Ask
        && card.ask.as_ref().is_some_and(|a| a.blocking != Some(true));
    (!in_q && (inputs > 1 || in_report) && !text.is_empty() && text != card.title)
        .then(|| question(&text, cx))
}

/// The help line under a variant.
fn help_of(b: &CardV2Block) -> Option<String> {
    match b {
        CardV2Block::Decision { help, .. }
        | CardV2Block::Rank { help, .. }
        | CardV2Block::Rating { help, .. }
        | CardV2Block::Scale { help, .. }
        | CardV2Block::ImageChoice { help, .. }
        | CardV2Block::Matrix { help, .. }
        | CardV2Block::Slots { help, .. }
        | CardV2Block::Poll { help, .. } => help.clone(),
        _ => None,
    }
}

fn hint(text: impl Into<SharedString>, cx: &App) -> Div {
    let k = theme::colors(cx);
    meta(div()).text_color(k.ink_3).child(text.into())
}

/// Wraps a variant with its question above and its help below.
fn framed(node: &Node, b: &CardV2Block, body: AnyElement, cx: &App) -> AnyElement {
    div()
        .flex()
        .flex_col()
        .gap(space::S8)
        .min_w(px(0.))
        .children(heading(node, b, cx))
        .child(body)
        .children(help_of(b).map(|h| hint(h, cx)))
        .into_any_element()
}

/// True when the viewer's controls on this block are live.
fn live(node: &Node, b: &CardV2Block) -> bool {
    let ctx = node.ctx;
    let id = b.id().unwrap_or("");
    if matches!(ctx.state, CardState::Sending { .. }) {
        return false;
    }
    if ctx.card.ask.is_some() && ctx.card.surface_ids().contains(id) {
        return ctx.live();
    }
    super::interact::may_emit(ctx, b)
}

/// After a pick: the block's live `choice.change` (when declared) and its commit (when
/// immediate).
fn after_pick(node: &Node, block: &str, cx: &mut App) {
    let ctx = node.ctx;
    let card = ctx.card.clone();
    let form = ctx.form.clone();
    let emit = Emitter::of(ctx);
    let id = block.to_string();
    let values = form.read(cx).values(cx);
    let Some(b) = card.block(&id) else { return };
    let Some(v) = form21::value_of(&card, b, &values) else {
        return;
    };
    emit.send(&id, "choice.change", v.clone(), None, cx);
    if b.base().is_some_and(|x| x.is_immediate()) {
        emit.commit(&id, "choice.submit", v, cx);
    }
}

/// An option row: the shared choice row of §2.17 (h40, border, the letter
/// box or a checkbox); the row takes the click.
#[allow(clippy::too_many_arguments)]
fn option_row(
    id: SharedString,
    index: usize,
    label: &str,
    detail: Option<&str>,
    recommended: bool,
    picked: bool,
    multiple: bool,
    enabled: bool,
    cx: &App,
) -> gpui::Stateful<Div> {
    b20::choice_row(
        id,
        index,
        label,
        detail,
        recommended,
        picked,
        multiple,
        enabled,
        cx,
    )
}

// ---------------------------------------------------------------------------
// decision (CARD21 `choice`)

pub fn decision(node: &Node, b: &CardV2Block, window: &mut Window, cx: &mut App) -> AnyElement {
    let CardV2Block::Decision {
        id,
        options,
        multiple,
        default,
        min_select,
        max_select,
        allow_other,
        ..
    } = b
    else {
        return div().into_any_element();
    };
    let ctx = node.ctx;
    let k = theme::colors(cx);
    let multiple = multiple.unwrap_or(false);
    let live = live(node, b);
    let picked = ctx.form.read(cx).choice(id).to_vec();
    let max = max_select.unwrap_or(options.len() as u32) as usize;
    let full = multiple && picked.len() >= max;
    let recommended = default.as_deref().unwrap_or(&[]);
    // Every choice is a RadioGroup of h40 rows (§2.17), short or long.
    let mut col = div().flex().flex_col().gap(space::S8).min_w(px(0.));
    let mut all: Vec<(String, String, Option<String>, bool)> = options
        .iter()
        .map(|o| {
            (
                o.id.clone(),
                o.label.clone(),
                o.detail.clone(),
                o.recommended.unwrap_or(false) || recommended.contains(&o.id),
            )
        })
        .collect();
    if let Some(o) = allow_other {
        all.push((form21::OTHER.into(), o.label.clone(), None, false));
    }
    for (i, (oid, label, detail, rec)) in all.iter().enumerate() {
        let on = live && picked.contains(oid);
        let dimmed = live && full && !on;
        let mut row = option_row(
            eid(node, format!("opt-{id}-{oid}")),
            i,
            label,
            detail.as_deref(),
            *rec,
            on,
            multiple,
            live && !dimmed,
            cx,
        );
        if live && !dimmed {
            let (form, block, option) = (ctx.form.clone(), id.clone(), oid.clone());
            let node_ptr = PickTarget::of(node);
            row = row.on_click(move |_, _, cx| {
                form.update(cx, |f, cx| f.choose(&block, &option, multiple, cx));
                node_ptr.after(&block, cx);
            });
        }
        col = col.child(row);
        if on && oid == form21::OTHER {
            let field = ctx.form.update(cx, |f, cx| {
                f.field(
                    &form21::other_key(id),
                    allow_other
                        .as_ref()
                        .and_then(|o| o.placeholder.as_deref())
                        .or(Some(c::OTHER_PLACEHOLDER)),
                    false,
                    window,
                    cx,
                )
            });
            let n = field.text(cx).chars().count();
            let focused = field.is_focused(window, cx);
            col = col.child(
                div()
                    .pl(px(24.))
                    .flex()
                    .items_center()
                    .gap(space::S10)
                    .child(
                        b20::field_box(focused, n > form21::OTHER_MAX, m::OPTION, cx)
                            .flex_1()
                            .child(
                                div()
                                    .flex_1()
                                    .min_w(px(0.))
                                    .child(b20::line_input(&field, !live, window, cx)),
                            ),
                    )
                    // Over the limit: neutral text with a red icon (owner
                    // decision v15, field errors).
                    .child(
                        meta(div())
                            .flex_none()
                            .flex()
                            .items_center()
                            .gap(space::S4)
                            .mono()
                            .text_color(if n > form21::OTHER_MAX {
                                k.ink
                            } else {
                                k.ink_3
                            })
                            .when(n > form21::OTHER_MAX, |el| {
                                el.child(ui::icon(Sym::Failed, px(12.), k.bad))
                            })
                            .child(format!("{n}/{}", form21::OTHER_MAX)),
                    ),
            );
        }
    }
    if multiple && live {
        let min = min_select.unwrap_or(1);
        col = col.child(hint(
            if full {
                fill(c::MULTI_HINT_FULL, &[("max", &max.to_string())])
            } else {
                fill(
                    c::MULTI_HINT,
                    &[
                        ("n", &picked.len().to_string()),
                        ("min", &min.to_string()),
                        ("max", &max.to_string()),
                    ],
                )
            },
            cx,
        ));
    }
    framed(node, b, col.into_any_element(), cx)
}

/// True when `id` sits in a column of a `row` that is drawn side by side (not folded).
fn in_row_column(node: &Node, id: &str) -> bool {
    if node.narrow {
        return false;
    }
    let mut cur = id;
    // Depth is at most 2; the bound also stops a cycle the server would refuse.
    for _ in 0..8 {
        let Some(p) = node.tree.parent(cur) else {
            break;
        };
        if matches!(node.tree.get(p), Some(CardV2Block::Row { .. })) {
            return true;
        }
        cur = p;
    }
    false
}

/// What a pick handler needs after the form changed (a clone of the node's context).
#[derive(Clone)]
pub(crate) struct PickTarget {
    emit: Emitter,
    form: Entity<crate::views::cards::v2::form::CardForm>,
    card: std::sync::Arc<crate::data::cards_v2::CardV2>,
}

impl PickTarget {
    pub(crate) fn of(node: &Node) -> Self {
        Self {
            emit: Emitter::of(node.ctx),
            form: node.ctx.form.clone(),
            card: node.ctx.card.clone(),
        }
    }

    /// The block's `choice.change` (or `input.change`) and its commit when immediate.
    pub(crate) fn after(&self, block: &str, cx: &mut App) {
        let values = self.form.read(cx).values(cx);
        let Some(b) = self.card.block(block) else {
            return;
        };
        let Some(v) = form21::value_of(&self.card, b, &values) else {
            return;
        };
        let live = match b {
            CardV2Block::Rank { .. }
            | CardV2Block::Rating { .. }
            | CardV2Block::Scale { .. }
            | CardV2Block::Matrix { .. }
            | CardV2Block::Input { .. } => "input.change",
            CardV2Block::Table { .. } => "table.rowSelect",
            _ => "choice.change",
        };
        self.emit.send(block, live, v.clone(), None, cx);
        let immediate = b.base().is_some_and(|x| x.is_immediate());
        match b {
            CardV2Block::Confirm { .. } | CardV2Block::Poll { .. } => {
                self.emit.commit(block, "choice.submit", v, cx)
            }
            CardV2Block::Compare { .. } if immediate => {
                self.emit.commit(block, "compare.pick", v, cx)
            }
            CardV2Block::Rating { .. } | CardV2Block::Scale { .. } if immediate => {
                self.emit.commit(block, "input.submit", v, cx)
            }
            _ if immediate => self.emit.commit(block, "choice.submit", v, cx),
            _ => {}
        }
    }
}

// ---------------------------------------------------------------------------
// rank

/// What is dragged in a rank list.
#[derive(Clone)]
struct RankDrag {
    block: String,
    item: String,
    label: String,
}

struct RankGhost(String);

impl Render for RankGhost {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let k = theme::colors(cx);
        div()
            .h(t::RANK_ROW)
            .px(space::S12)
            .rounded(m::OPTION_RADIUS)
            .bg(k.bg)
            .border_1()
            .border_color(k.ink_3)
            .shadow(k.lift())
            .flex()
            .items_center()
            .text_size(crate::ui::native::tokens::text::UI.0)
            .text_color(k.ink)
            .child(self.0.clone())
    }
}

thread_local! {
    /// Focus handles of rank lists (keyboard moves), by element id.
    static RANK_FOCUS: RefCell<HashMap<SharedString, FocusHandle>> = RefCell::new(HashMap::new());
}

fn move_item(order: &mut Vec<String>, item: &str, to: usize) {
    if let Some(from) = order.iter().position(|x| x == item) {
        let it = order.remove(from);
        order.insert(to.min(order.len()), it);
    }
}

pub fn rank(node: &Node, b: &CardV2Block, cx: &mut App) -> AnyElement {
    let CardV2Block::Rank { id, items, cut, .. } = b else {
        return div().into_any_element();
    };
    let ctx = node.ctx;
    let k = theme::colors(cx);
    let live = live(node, b);
    let initial: Vec<String> = items.iter().map(|i| i.id.clone()).collect();
    let order = match ctx.form.read(cx).value21(id) {
        Some(Val::Order(o)) => o.clone(),
        _ => initial.clone(),
    };
    let focus_id = eid(node, format!("rank-{id}"));
    let handle = RANK_FOCUS.with(|f| {
        f.borrow_mut()
            .entry(focus_id.clone())
            .or_insert_with(|| cx.focus_handle())
            .clone()
    });
    let focused_item = node.ui.block(id).focus.clone();
    let target = PickTarget::of(node);
    let set = {
        let (form, block, target) = (ctx.form.clone(), id.clone(), target.clone());
        move |order: Vec<String>, cx: &mut App| {
            form.update(cx, |f, cx| f.set_value21(&block, Val::Order(order), cx));
            target.after(&block, cx);
        }
    };
    let mut col = div()
        .id(focus_id.clone())
        .track_focus(&handle)
        .flex()
        .flex_col()
        .p(space::S4)
        .rounded(crate::ui::native::tokens::radius::CONTROL)
        .border_1()
        .border_color(crate::ui::native::native_colors(cx).separator)
        .bg(crate::ui::native::native_colors(cx).fill)
        .min_w(px(0.));
    if live {
        let (order2, set2, ui) = (order.clone(), set.clone(), ctx.ui.clone());
        let block = id.clone();
        col = col.on_key_down(move |e: &KeyDownEvent, _, cx| {
            let ks = &e.keystroke;
            if !ks.modifiers.alt {
                return;
            }
            let Some(item) = ui.read(cx).block(&block).focus.clone() else {
                return;
            };
            let Some(i) = order2.iter().position(|x| *x == item) else {
                return;
            };
            let to = match ks.key.as_str() {
                "up" if i > 0 => i - 1,
                "down" if i + 1 < order2.len() => i + 1,
                _ => return,
            };
            let mut o = order2.clone();
            move_item(&mut o, &item, to);
            set2(o, cx);
            cx.stop_propagation();
        });
    }
    for (i, item_id) in order.iter().enumerate() {
        let Some(item) = items.iter().find(|x| &x.id == item_id) else {
            continue;
        };
        let is_focus = live && focused_item.as_deref() == Some(item_id.as_str());
        let arrow = |up: bool| {
            let can = live && if up { i > 0 } else { i + 1 < order.len() };
            let (o, set, item) = (order.clone(), set.clone(), item_id.clone());
            div()
                .id(eid(
                    node,
                    format!("rank-{id}-{item_id}-{}", if up { "up" } else { "down" }),
                ))
                .flex_none()
                .size(px(24.))
                .rounded(crate::ui::native::tokens::radius::ITEM)
                .flex()
                .items_center()
                .justify_center()
                .when(can, |el| {
                    el.hover(|s| s.bg(k.hover)).on_click(move |_, _, cx| {
                        let mut o = o.clone();
                        let to = if up { i - 1 } else { i + 1 };
                        move_item(&mut o, &item, to);
                        set(o, cx);
                    })
                })
                .child(ui::icon(
                    if up { Sym::ChevronUp } else { Sym::ChevronDown },
                    px(14.),
                    if can { k.ink_2 } else { k.ink_3 },
                ))
        };
        // One list in one well (NATIVE.md rule 5): 32 pt rows split by hairlines, the
        // focused row on `selection`; no boxed rows.
        let nk = crate::ui::native::native_colors(cx);
        let mut row = div()
            .id(eid(node, format!("rank-{id}-{item_id}")))
            .h(crate::ui::native::tokens::height::DEFAULT)
            .pl(space::S12)
            .pr(space::S8)
            .rounded(crate::ui::native::tokens::radius::ITEM)
            .when(is_focus, |el| el.bg(nk.selection))
            .when(i > 0, |el| el.border_t_1().border_color(nk.separator))
            .flex()
            .items_center()
            .gap(space::S10)
            .text_size(crate::ui::native::tokens::text::UI.0)
            .text_color(k.ink)
            .child(
                meta(div())
                    .flex_none()
                    .w(px(14.))
                    .mono()
                    .text_color(k.ink_3)
                    .child((i + 1).to_string()),
            )
            .when(live, |el| el.child(grip(k.ink_3)))
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.))
                    .one_line()
                    .when(is_focus, |el| el.font_weight(theme::MEDIUM_WEIGHT))
                    .child(crate::ui::truncate::TruncText::new(item.label.clone())),
            )
            .when_some(item.meta.clone(), |el, mt| {
                el.child(meta(div()).flex_none().text_color(k.ink_3).child(mt))
            })
            .when(live, |el| el.child(arrow(true)).child(arrow(false)));
        if live {
            let drag = RankDrag {
                block: id.clone(),
                item: item_id.clone(),
                label: item.label.clone(),
            };
            let (o, set, block, ui, handle) = (
                order.clone(),
                set.clone(),
                id.clone(),
                ctx.ui.clone(),
                handle.clone(),
            );
            let item2 = item_id.clone();
            let line = nk.fill_hover;
            row = row
                .on_click(move |_, window, cx| {
                    window.focus(&handle, cx);
                    ui.update(cx, |u, cx| {
                        u.block_mut(&block).focus = Some(item2.clone());
                        cx.notify();
                    });
                })
                .on_drag(drag, |d, _, _, cx| cx.new(|_| RankGhost(d.label.clone())))
                .drag_over::<RankDrag>(move |s, _, _, _| s.bg(line))
                .on_drop(move |d: &RankDrag, _, cx| {
                    if d.block != id_of(&o, &d.item, &d.block) {
                        return;
                    }
                    let mut o = o.clone();
                    move_item(&mut o, &d.item, i);
                    set(o, cx);
                });
        }
        col = col.child(row);
        if let Some(cut) = cut
            .as_ref()
            .filter(|c| c.after as usize == i + 1 && i + 1 < order.len())
        {
            col = col.child(
                div()
                    .my(space::S2)
                    .flex()
                    .items_center()
                    .gap(space::S8)
                    .child(
                        div()
                            .flex_1()
                            .h(crate::ui::native::tokens::layout::BORDER)
                            .border_t_1()
                            .border_dashed()
                            .border_color(k.line_2),
                    )
                    .child(
                        meta(div())
                            .flex_none()
                            .text_color(k.ink_3)
                            .child(cut.label.clone()),
                    )
                    .child(
                        div()
                            .flex_1()
                            .h(crate::ui::native::tokens::layout::BORDER)
                            .border_t_1()
                            .border_dashed()
                            .border_color(k.line_2),
                    ),
            );
        }
    }
    let changed = order != initial;
    let mut out = div().flex().flex_col().gap(space::S8).child(col);
    if live {
        let mut line = div().flex().items_center().gap(space::S8);
        if changed {
            let (form, block, initial, target) = (
                ctx.form.clone(),
                id.clone(),
                initial.clone(),
                target.clone(),
            );
            line = line.child(
                b20::quiet_button(
                    eid(node, format!("rank-reset-{id}")),
                    &fill(c::RANK_RESET, &[("name", &ctx.author)]),
                    cx,
                )
                .ml_auto()
                .on_click(move |_, _, cx| {
                    form.update(cx, |f, cx| {
                        f.set_value21(&block, Val::Order(initial.clone()), cx)
                    });
                    target.after(&block, cx);
                }),
            );
        }
        out = out.child(line);
    }
    framed(node, b, out.into_any_element(), cx)
}

/// The drag handle: two columns of three dots.
fn grip(color: gpui::Hsla) -> Div {
    let col = || {
        div()
            .flex()
            .flex_col()
            .gap(crate::ui::native::tokens::space::S2)
            .children((0..3).map(|_| div().size(px(2.5)).rounded_full().bg(color)))
    };
    div()
        .flex_none()
        .w(px(14.))
        .flex()
        .justify_center()
        .gap(crate::ui::native::tokens::space::S2)
        .child(col())
        .child(col())
}

/// The block of a drag (a drop on another list is ignored).
fn id_of(_order: &[String], _item: &str, block: &str) -> String {
    block.to_string()
}

// ---------------------------------------------------------------------------
// rating and scale

pub fn rating(node: &Node, b: &CardV2Block, cx: &mut App) -> AnyElement {
    let CardV2Block::Rating { id, labels, .. } = b else {
        return div().into_any_element();
    };
    let ctx = node.ctx;
    let k = theme::colors(cx);
    let live = live(node, b);
    let value = match ctx.form.read(cx).value21(id) {
        Some(Val::Num(n)) => Some(*n as usize),
        _ => None,
    };
    let target = PickTarget::of(node);
    let form = ctx.form.clone();
    let block = id.clone();
    let stars = div()
        .flex()
        .items_center()
        .gap(space::S6)
        .children((1..=5usize).map(|n| {
            let on = value.is_some_and(|v| n <= v);
            let (form, block, target) = (form.clone(), block.clone(), target.clone());
            div()
                .id(eid(node, format!("star-{id}-{n}")))
                .size(t::STAR)
                .flex()
                .items_center()
                .justify_center()
                .when(live, |el| {
                    el.focus_row_with(on, crate::ui::native::Arrows::Grid, cx)
                        .on_click(move |_, _, cx| {
                            form.update(cx, |f, cx| f.set_value21(&block, Val::Num(n as f64), cx));
                            target.after(&block, cx);
                        })
                })
                .child(ui::icon(
                    if on { Sym::StarFill } else { Sym::Star },
                    px(20.),
                    if on { k.ink } else { k.line_2 },
                ))
        }));
    let word = value.map(
        |v| match labels.as_ref().and_then(|l| l.get(&v.to_string())) {
            Some(l) => fill(
                c::RATING_VALUE_LABEL,
                &[("label", l), ("n", &v.to_string())],
            ),
            None => fill(c::RATING_VALUE, &[("n", &v.to_string())]),
        },
    );
    let row = div()
        .flex()
        .items_center()
        .gap(space::S12)
        .child(stars)
        .when_some(word, |el, w| {
            el.child(b20::small(div()).text_color(k.ink_2).child(w))
        });
    framed(node, b, row.into_any_element(), cx)
}

pub fn scale(node: &Node, b: &CardV2Block, cx: &mut App) -> AnyElement {
    let CardV2Block::Scale {
        id, min, max, ends, ..
    } = b
    else {
        return div().into_any_element();
    };
    let ctx = node.ctx;
    let k = theme::colors(cx);
    let live = live(node, b);
    let value = match ctx.form.read(cx).value21(id) {
        Some(Val::Num(n)) => Some(*n as i64),
        _ => None,
    };
    let target = PickTarget::of(node);
    let nk = crate::ui::native::native_colors(cx);
    let cells = div().flex().gap(space::S4).children((*min..=*max).map(|n| {
        let on = value == Some(n);
        let (form, block, target) = (ctx.form.clone(), id.clone(), target.clone());
        div()
            .id(eid(node, format!("scale-{id}-{n}")))
            .flex_1()
            .min_w(px(0.))
            .h(crate::ui::native::tokens::height::SM)
            // An sm h28 cell with a 1 px border (§1.8): the chosen one in `primary`,
            // hover muted at 50%.
            .rounded(crate::ui::native::tokens::radius::ITEM)
            .border_1()
            .border_color(if on { nk.primary } else { nk.separator })
            .bg(if on { nk.primary } else { nk.content })
            // A tab stop with ← → (E3); the hover below keeps its border step.
            // `selected: true` leaves the hover to this cell (one hover style).
            .when(live, |el| {
                el.focus_row_with(true, crate::ui::native::Arrows::Grid, cx)
            })
            .when(live && !on, |el| {
                let hover = theme::card_v2::alpha(nk.fill, 0.5);
                let edge = theme::card_v2::alpha(nk.label, 0.5);
                el.hover(move |s| s.bg(hover).border_color(edge))
            })
            .when(!live, |el| el.opacity(0.5))
            .flex()
            .items_center()
            .justify_center()
            .n_body()
            .n_figures()
            .text_color(if on { nk.on_primary } else { nk.label })
            .when(live, |el| {
                el.on_click(move |_, _, cx| {
                    form.update(cx, |f, cx| f.set_value21(&block, Val::Num(n as f64), cx));
                    target.after(&block, cx);
                })
            })
            .child(n.to_string())
    }));
    let foot = ends.as_ref().map(|(a, z)| {
        meta(div())
            .flex()
            .text_color(k.ink_3)
            .child(fill(c::SCALE_END, &[("n", &min.to_string()), ("label", a)]))
            .child(
                div()
                    .ml_auto()
                    .child(fill(c::SCALE_END, &[("n", &max.to_string()), ("label", z)])),
            )
    });
    let col = div()
        .flex()
        .flex_col()
        .gap(space::S6)
        .child(cells)
        .children(foot);
    framed(node, b, col.into_any_element(), cx)
}

// ---------------------------------------------------------------------------
// confirm

pub fn confirm(node: &Node, b: &CardV2Block, cx: &mut App) -> AnyElement {
    let CardV2Block::Confirm {
        id,
        yes,
        no,
        detail,
        ..
    } = b
    else {
        return div().into_any_element();
    };
    let ctx = node.ctx;
    let k = theme::colors(cx);
    let live = live(node, b);
    let chosen = match ctx.form.read(cx).values(cx).get(id.as_str()) {
        Some(Val::Switch(v)) => Some(*v),
        _ => None,
    };
    let target = PickTarget::of(node);
    let btn = |value: bool, label: &str| {
        let on = chosen == Some(value);
        let (form, block, target) = (ctx.form.clone(), id.clone(), target.clone());
        let switch_now = chosen != Some(value);
        button(
            eid(node, format!("confirm-{id}-{value}")),
            label.to_string(),
            if on {
                PushStyle::Default
            } else {
                PushStyle::Outline
            },
            cx,
        )
        .disabled(!live)
        .on_click(move |_, _, cx| {
            form.update(cx, |f, cx| {
                if switch_now {
                    f.toggle_switch_to(&block, value, cx);
                }
            });
            target.after(&block, cx);
        })
    };
    // My answer that did not get there: what it was, and that it is still here.
    if let CardState::SendFailed { .. } = ctx.state {
        let p = ctx
            .env
            .provider
            .card_v2_pending(&ctx.msg.group_id, &ctx.msg.message_id);
        let v = p
            .as_ref()
            .and_then(|p| p.inputs.as_ref())
            .and_then(|i| i.get(id.as_str()))
            .and_then(|v| v.get("value").cloned().or(Some(v.clone())))
            .and_then(|v| v.as_bool());
        if let Some(v) = v {
            let what = if v { yes.clone() } else { no.clone() };
            return div()
                .h(m::OPTION)
                .px(space::S12)
                .rounded(theme::radius::MD)
                .border_1()
                .border_dashed()
                .border_color(k.line_2)
                .flex()
                .items_center()
                .child(
                    b20::small(div())
                        .text_color(k.ink_2)
                        .child(fill(c::YOUR_ANSWER, &[("what", &what)])),
                )
                .child(
                    meta(div())
                        .ml_auto()
                        .text_color(k.ink_3)
                        .child(fill(c::NOT_SENT_YET, &[("name", &ctx.author)])),
                )
                .into_any_element();
        }
    }
    let row = div()
        .flex()
        .items_center()
        .gap(space::S8)
        .child(btn(true, yes))
        .child(btn(false, no))
        .when_some(detail.clone(), |el, d| {
            el.child(
                meta(div())
                    .ml(space::S8)
                    .min_w(px(0.))
                    .text_color(k.ink_3)
                    .child(d),
            )
        });
    framed(node, b, row.into_any_element(), cx)
}

// ---------------------------------------------------------------------------
// imageChoice

pub fn image_choice(node: &Node, b: &CardV2Block, cx: &mut App) -> AnyElement {
    let CardV2Block::ImageChoice {
        id,
        options,
        multiple,
        ..
    } = b
    else {
        return div().into_any_element();
    };
    let ctx = node.ctx;
    let live = live(node, b);
    let multiple = multiple.unwrap_or(false);
    let picked = ctx.form.read(cx).choice(id).to_vec();
    let target = PickTarget::of(node);
    let grid = div()
        .flex()
        .gap(space::S12)
        .children(options.iter().map(|o| {
            let on = picked.contains(&o.id);
            let path =
                ctx.env
                    .provider
                    .card_image(&ctx.msg.group_id, &ctx.msg.topic_id, &o.file_id);
            let (form, block, option, target) =
                (ctx.form.clone(), id.clone(), o.id.clone(), target.clone());
            // Photos-style (NATIVE.md rule 4): the picture, its title with `推荐` after
            // it; a chosen picture shows a checkmark badge, never an outline.
            div()
                .id(eid(node, format!("img-{id}-{}", o.id)))
                .flex_1()
                .min_w(px(0.))
                .flex()
                .flex_col()
                .gap(space::S6)
                .when(live, |el| {
                    el.focus_row_with(on, crate::ui::native::Arrows::Grid, cx)
                        .on_click(move |_, _, cx| {
                            form.update(cx, |f, cx| f.choose(&block, &option, multiple, cx));
                            target.after(&block, cx);
                        })
                })
                .child(
                    div()
                        .relative()
                        .w_full()
                        .h(t::IMAGE)
                        .rounded(crate::ui::native::tokens::radius::CONTROL)
                        .overflow_hidden()
                        .map(|el| match path {
                            Some(p) => el.child(img(p).size_full().object_fit(ObjectFit::Cover)),
                            None => b20::missing_image(el, cx),
                        })
                        .when(on, |el| {
                            el.child(
                                div()
                                    .absolute()
                                    .top(space::S6)
                                    .right(space::S6)
                                    .child(b20::chosen_badge(cx)),
                            )
                        }),
                )
                .child(b20::option_title(
                    &o.label,
                    o.recommended.unwrap_or(false),
                    cx,
                ))
        }));
    framed(node, b, grid.into_any_element(), cx)
}

// ---------------------------------------------------------------------------
// matrix

pub fn matrix(node: &Node, b: &CardV2Block, cx: &mut App) -> AnyElement {
    let CardV2Block::Matrix {
        id, rows, columns, ..
    } = b
    else {
        return div().into_any_element();
    };
    let ctx = node.ctx;
    let k = theme::colors(cx);
    let live = live(node, b);
    let set = match ctx.form.read(cx).value21(id) {
        Some(Val::Rows(r)) => r.clone(),
        _ => Default::default(),
    };
    let target = PickTarget::of(node);
    let head = div()
        .flex()
        .items_center()
        .h(crate::ui::native::tokens::height::SM)
        .border_b_1()
        .border_color(k.line)
        .child(
            meta(div())
                .flex_1()
                .text_color(k.ink_3)
                .child(c::MATRIX_ROW_HEAD),
        )
        .children(columns.iter().map(|col| {
            meta(div())
                .flex_none()
                .w(t::MATRIX_COL)
                .flex()
                .justify_center()
                .text_color(k.ink_3)
                .child(col.label.clone())
        }));
    let mut table = div().flex().flex_col().child(head);
    for (ri, r) in rows.iter().enumerate() {
        let mut line = div()
            .flex()
            .items_center()
            .h(crate::ui::native::tokens::height::FOOTER_ROW)
            .when(ri + 1 < rows.len(), |el| {
                el.border_b_1().border_color(k.line)
            })
            .child(
                b20::small(div())
                    .flex_1()
                    .min_w(px(0.))
                    .one_line()
                    .text_color(k.ink)
                    .child(crate::ui::truncate::TruncText::new(r.label.clone())),
            );
        for col in columns {
            let on = set.get(&r.id) == Some(&col.id);
            let (form, block, row_id, col_id, target, cur) = (
                ctx.form.clone(),
                id.clone(),
                r.id.clone(),
                col.id.clone(),
                target.clone(),
                set.clone(),
            );
            line = line.child(
                div()
                    .id(eid(node, format!("mx-{id}-{}-{}", r.id, col.id)))
                    .flex_none()
                    .w(t::MATRIX_COL)
                    .h_full()
                    .flex()
                    .items_center()
                    .justify_center()
                    .when(live, |el| {
                        el.focus_row_with(on, crate::ui::native::Arrows::Grid, cx)
                            .on_click(move |_, _, cx| {
                                let mut rows = cur.clone();
                                rows.insert(row_id.clone(), col_id.clone());
                                form.update(cx, |f, cx| f.set_value21(&block, Val::Rows(rows), cx));
                                target.after(&block, cx);
                            })
                    })
                    .child(
                        crate::ui::native::NativeRadio::new(
                            eid(node, format!("matrix-{id}-{}-{}-r", r.id, col.id)),
                            on,
                        )
                        .disabled(!live),
                    ),
            );
        }
        table = table.child(line);
    }
    let note = live.then(|| {
        hint(
            fill(
                c::MATRIX_PICKED,
                &[
                    ("n", &set.len().to_string()),
                    ("total", &rows.len().to_string()),
                ],
            ),
            cx,
        )
    });
    let col = div()
        .flex()
        .flex_col()
        .gap(space::S8)
        .child(table)
        .children(note);
    framed(node, b, col.into_any_element(), cx)
}

// ---------------------------------------------------------------------------
// slots

fn weekday(at: Millis) -> (String, String, String) {
    use chrono::{Datelike as _, Local, TimeZone as _};
    let Some(t) =
        crate::views::shell::known(at).and_then(|at| Local.timestamp_millis_opt(at).single())
    else {
        let unknown = crate::views::shell::clock(at);
        return (unknown.clone(), unknown.clone(), unknown);
    };
    let days: Vec<&str> = c::WEEKDAYS.split(' ').collect();
    let w = days
        .get(t.weekday().num_days_from_sunday() as usize)
        .copied()
        .unwrap_or("");
    (
        format!("{w} {}", t.format("%m-%d")),
        t.format("%H:%M").to_string(),
        t.format("%Y-%m-%d").to_string(),
    )
}

pub fn slots(node: &Node, b: &CardV2Block, cx: &mut App) -> AnyElement {
    let CardV2Block::Slots {
        id,
        slots,
        multiple,
        ..
    } = b
    else {
        return div().into_any_element();
    };
    let ctx = node.ctx;
    let k = theme::colors(cx);
    let live = live(node, b);
    let multiple = multiple.unwrap_or(false);
    let picked = ctx.form.read(cx).choice(id).to_vec();
    let target = PickTarget::of(node);
    // Columns by day, rows by time of day.
    let mut days: Vec<(String, String)> = Vec::new();
    let mut times: Vec<String> = Vec::new();
    for s in slots {
        let (head, time, date) = weekday(s.start as Millis);
        if !days.iter().any(|d| d.1 == date) {
            days.push((head, date));
        }
        if !times.contains(&time) {
            times.push(time);
        }
    }
    times.sort();
    let nk = crate::ui::native::native_colors(cx);
    let cell = |s: &crate::data::cards_v2::Slot| {
        let on = picked.contains(&s.id);
        let (_, time, _) = weekday(s.start as Millis);
        let conflicts = s.conflicts.clone().unwrap_or_default();
        let note = if conflicts.is_empty() {
            None
        } else if conflicts.len() == 1 {
            Some(fill(
                c::SLOT_MISSING,
                &[("names", &ctx.name(&conflicts[0]))],
            ))
        } else {
            Some(fill(
                c::SLOT_MISSING_N,
                &[("n", &conflicts.len().to_string())],
            ))
        };
        let (form, block, option, target) =
            (ctx.form.clone(), id.clone(), s.id.clone(), target.clone());
        div()
            .id(eid(node, format!("slot-{id}-{}", s.id)))
            .flex_1()
            .min_w(px(0.))
            .h(t::SLOT_CELL)
            .px(space::S10)
            // A bordered cell (§1.7); the chosen slot: a foreground border on the
            // muted fill and a checkmark badge (§2.17 selected row).
            .relative()
            .rounded(crate::ui::native::tokens::radius::CONTROL)
            .border_1()
            .border_color(if on { nk.label } else { nk.separator })
            .bg(if on { nk.selection } else { nk.content })
            .when(on, |el| {
                el.child(
                    div()
                        .absolute()
                        .top(space::S6)
                        .right(space::S6)
                        .child(b20::chosen_badge(cx)),
                )
            })
            .flex()
            .flex_col()
            .justify_center()
            .when(live, |el| {
                el.focus_row_with(on, crate::ui::native::Arrows::Grid, cx)
                    .on_click(move |_, _, cx| {
                        form.update(cx, |f, cx| f.choose(&block, &option, multiple, cx));
                        target.after(&block, cx);
                    })
            })
            .child(
                b20::small(div())
                    .mono()
                    .text_color(k.ink)
                    .when(on, |el| el.font_weight(FontWeight::SEMIBOLD))
                    .child(time),
            )
            .child(match note {
                Some(n) => meta(div()).one_line().text_color(k.ink_3).child(n),
                None => meta(div())
                    .flex()
                    .items_center()
                    .gap(space::S4)
                    // Foreground text; the color stays on the 12 icon (§1.3).
                    .text_color(k.ink)
                    .child(ui::icon(Sym::Check, px(12.), k.ok))
                    .child(c::SLOT_ALL_FREE),
            })
    };
    let head = div()
        .flex()
        .gap(space::S6)
        .child(div().flex_none().w(px(44.)))
        .children(days.iter().map(|(h, _)| {
            meta(div())
                .flex_1()
                .min_w(px(0.))
                .flex()
                .justify_center()
                .text_color(k.ink_2)
                .child(h.clone())
        }));
    let mut grid = div().flex().flex_col().gap(space::S6).child(head);
    for time in &times {
        let mut line = div().flex().items_center().gap(space::S6).child(
            meta(div())
                .flex_none()
                .w(px(44.))
                .mono()
                .text_color(k.ink_3)
                .child(time.clone()),
        );
        for (_, date) in &days {
            let s = slots.iter().find(|s| {
                let (_, tm, d) = weekday(s.start as Millis);
                &tm == time && &d == date
            });
            line = line.child(match s {
                Some(s) => cell(s).into_any_element(),
                None => div().flex_1().min_w(px(0.)).into_any_element(),
            });
        }
        grid = grid.child(line);
    }
    framed(node, b, grid.into_any_element(), cx)
}

// ---------------------------------------------------------------------------
// poll

pub fn poll(node: &Node, b: &CardV2Block, cx: &mut App) -> AnyElement {
    let CardV2Block::Poll {
        id,
        options,
        public,
        voters,
        changeable,
        ..
    } = b
    else {
        return div().into_any_element();
    };
    let ctx = node.ctx;
    let me = ctx.me.clone().unwrap_or_default();
    // Votes: the server's tally, else the stored answers of this rev.
    let mut tally: HashMap<String, Vec<String>> = HashMap::new();
    for a in ctx.answers.iter().filter(|a| a.rev == ctx.card.rev) {
        if let Some(v) = a.inputs.as_ref().and_then(|i| i.get(id.as_str())) {
            let opt = v
                .get("value")
                .and_then(Value::as_str)
                .or(v.as_str())
                .unwrap_or_default()
                .to_string();
            tally.entry(opt).or_default().push(a.actor_id.clone());
        }
    }
    if let Some(s) = &ctx.msg.card_state {
        if let Some(by) = &s.tally_by {
            tally = by.iter().map(|(k, v)| (k.clone(), v.clone())).collect();
        } else if let Some(t) = &s.tally {
            for (opt, n) in t {
                let have = tally.get(opt).map_or(0, Vec::len);
                if have < *n as usize {
                    tally
                        .entry(opt.clone())
                        .or_default()
                        .extend(std::iter::repeat_n(String::new(), *n as usize - have));
                }
            }
        }
    }
    let total: usize = tally.values().map(Vec::len).sum();
    let mine = tally
        .iter()
        .find(|(_, v)| v.contains(&me))
        .map(|(k, _)| k.clone())
        .or_else(|| ctx.form.read(cx).choice(id).first().cloned());
    let voted = mine.is_some();
    let can = live(node, b) && (!voted || changeable.unwrap_or(false));
    let target = PickTarget::of(node);
    let mut col = div().flex().flex_col().gap(space::S8);
    for o in options.iter() {
        let who = tally.get(&o.id).cloned().unwrap_or_default();
        let n = who.len();
        let pct = (n * 100 + total / 2).checked_div(total).unwrap_or(0);
        let on = mine.as_deref() == Some(o.id.as_str());
        let show = voted || !live(node, b);
        let (form, block, option, target) =
            (ctx.form.clone(), id.clone(), o.id.clone(), target.clone());
        let faces: Vec<AnyElement> = if public.unwrap_or(false) {
            who.iter()
                .filter(|x| !x.is_empty())
                .take(3)
                .map(|x| {
                    ui::MemberAvatar::new(ctx.env.provider.member(x).as_ref(), px(16.))
                        .into_any_element()
                })
                .collect()
        } else {
            Vec::new()
        };
        // A radio row (NATIVE.md §5.4): before a vote a radio button, after it a
        // checkmark on mine; the votes at the trailing edge and a 4 pt bar under the
        // label once the results show. No tiles, no letters, no 我投的 capsule.
        let n_ink = crate::ui::native::native_colors(cx);
        let lead: AnyElement = if show {
            div()
                .flex_none()
                .w(px(16.))
                .flex()
                .justify_center()
                .when(on, |el| {
                    el.child(Sym::Check.inline().text_color(n_ink.label))
                })
                .into_any_element()
        } else {
            crate::ui::native::NativeRadio::new(eid(node, format!("poll-{id}-{}-c", o.id)), on)
                .disabled(!can)
                .into_any_element()
        };
        col = col.child(
            div()
                .id(eid(node, format!("poll-{id}-{}", o.id)))
                .flex()
                .flex_col()
                .gap(space::S4)
                .when(can, |el| {
                    el.focus_row_with(on, crate::ui::native::Arrows::Vertical, cx)
                        .on_click(move |_, _, cx| {
                            form.update(cx, |f, cx| f.choose(&block, &option, false, cx));
                            target.after(&block, cx);
                        })
                })
                .child(
                    div()
                        .min_h(px(24.))
                        .flex()
                        .items_center()
                        .gap(space::S8)
                        .child(lead)
                        .child(b20::option_title(&o.label, false, cx).flex_1())
                        .child(div().flex().gap(px(-4.)).children(faces))
                        .when(show, |el| {
                            el.child(
                                div()
                                    .flex_none()
                                    .n_subheadline()
                                    .n_figures()
                                    .text_color(n_ink.secondary)
                                    .child(fill(
                                        c::POLL_VOTES,
                                        &[("n", &n.to_string()), ("pct", &pct.to_string())],
                                    )),
                            )
                        }),
                )
                .when(show && total > 0, |el| {
                    el.child(div().pl(px(24.)).child(crate::ui::native::determinate_bar(
                        n as f32 / total as f32,
                        None,
                        cx,
                    )))
                }),
        );
    }
    // 还没投：… (the voters who have not voted).
    let voted_ids: Vec<&String> = tally.values().flatten().collect();
    let missing: Vec<String> = match voters {
        PollVoters::Members(ids) => ids.clone(),
        PollVoters::Keyword(_) => ctx
            .env
            .provider
            .members()
            .into_iter()
            .filter(|m| !m.is_bot() && !m.deactivated)
            .map(|m| m.id)
            .collect(),
    }
    .into_iter()
    .filter(|id| !voted_ids.contains(&id))
    .map(|id| ctx.name(&id))
    .collect();
    if public.unwrap_or(false) && !missing.is_empty() && (voted || !live(node, b)) {
        col = col.child(hint(
            fill(c::POLL_NOT_VOTED, &[("names", &missing.join("、"))]),
            cx,
        ));
    }
    framed(node, b, col.into_any_element(), cx)
}

// ---------------------------------------------------------------------------
// compare

pub fn compare(node: &Node, b: &CardV2Block, window: &mut Window, cx: &mut App) -> AnyElement {
    let CardV2Block::Compare {
        id,
        question: q,
        options,
        note,
        ..
    } = b
    else {
        return div().into_any_element();
    };
    let ctx = node.ctx;
    let k = theme::colors(cx);
    let live = live(node, b);
    let picked = ctx.form.read(cx).choice(id).first().cloned();
    let target = PickTarget::of(node);
    let inner = super::deeper(node);
    let mut cells: Vec<AnyElement> = Vec::new();
    for o in options.iter() {
        let on = picked.as_deref() == Some(o.id.as_str());
        let preview: Option<AnyElement> = o.preview.as_ref().map(|p| {
            let path = match p {
                ComparePreview::File { file_id } => {
                    ctx.env
                        .provider
                        .card_image(&ctx.msg.group_id, &ctx.msg.topic_id, file_id)
                }
                ComparePreview::Artifact { .. } => None,
            };
            div()
                .w_full()
                .h(t::PREVIEW)
                .bg(k.sunken)
                .flex()
                .items_center()
                .justify_center()
                .overflow_hidden()
                .map(|el| match path {
                    Some(p) => el.child(img(p).size_full().object_fit(ObjectFit::Cover)),
                    None => el.child(ui::icon(Sym::Image, px(20.), k.ink_3)),
                })
                .into_any_element()
        });
        let lines = |items: &Option<Vec<String>>, sign: &'static str, color| {
            items.clone().unwrap_or_default().into_iter().map(move |x| {
                meta(div())
                    .flex()
                    .gap(space::S6)
                    .child(div().flex_none().text_color(color).child(sign))
                    .child(div().min_w(px(0.)).text_color(k.ink_2).child(x))
            })
        };
        let kids: Vec<AnyElement> = o
            .children
            .clone()
            .unwrap_or_default()
            .iter()
            .filter_map(|cid| node.tree.get(cid))
            .map(|cb| render_node(&inner, cb, window, cx))
            .collect();
        let (form, block, option, target2) =
            (ctx.form.clone(), id.clone(), o.id.clone(), target.clone());
        // A Regular push button (NATIVE.md §5.2): the recommended column's is the
        // Default button; the chosen one says 已选 after a checkmark. No letters.
        let rec = o.recommended.unwrap_or(false);
        let pick = b20::option_button(
            eid(node, format!("cmp-{id}-{}", o.id)),
            if on {
                c::COMPARE_CHOSEN
            } else {
                c::COMPARE_CHOOSE
            },
            rec,
            on,
            live,
            cx,
        )
        .on_click(move |_, _, cx| {
            form.update(cx, |f, cx| f.choose(&block, &option, false, cx));
            target2.after(&block, cx);
        });
        cells.push(
            crate::ui::native::cards::card_well(cx)
                .border_1()
                .border_color(k.line)
                .flex_1()
                .min_w(px(0.))
                .overflow_hidden()
                .flex()
                .flex_col()
                .children(preview)
                .child(
                    div()
                        .p(space::S12)
                        .flex()
                        .flex_col()
                        .gap(space::S6)
                        .child(
                            b20::option_title(&o.label, rec, cx).font_weight(FontWeight::SEMIBOLD),
                        )
                        // "+" and "−" are muted marks, not state color (D3).
                        .children(lines(&o.pros, "+", k.ink_3))
                        .children(lines(&o.cons, "−", k.ink_3))
                        .children(kids)
                        .child(div().mt(space::S4).child(pick)),
                )
                .into_any_element(),
        );
    }
    let narrow = node.narrow;
    let body = if narrow {
        div().flex().flex_col().gap(space::S10).children(cells)
    } else if cells.len() == 4 {
        let mut it = cells.into_iter();
        let mut a = Vec::new();
        let mut z = Vec::new();
        for (i, c) in it.by_ref().enumerate() {
            if i < 2 { a.push(c) } else { z.push(c) }
        }
        div()
            .flex()
            .flex_col()
            .gap(space::S10)
            .child(div().flex().gap(space::S10).children(a))
            .child(div().flex().gap(space::S10).children(z))
    } else {
        div().flex().gap(space::S10).children(cells)
    };
    let mut col = div().flex().flex_col().gap(space::S10).min_w(px(0.));
    if let Some(q) = q.as_ref().filter(|q| **q != ctx.card.title) {
        col = col.child(question(q, cx));
    }
    col = col.child(body);
    if let Some(n) = note.as_ref().filter(|_| live) {
        let field = ctx.form.update(cx, |f, cx| {
            f.field(&form21::note_key(id), Some(&n.label), false, window, cx)
        });
        let focused = field.is_focused(window, cx);
        col = col.child(
            b20::field_box(focused, false, m::OPTION, cx).child(
                div()
                    .flex_1()
                    .min_w(px(0.))
                    .child(b20::line_input(&field, !live, window, cx)),
            ),
        );
    }
    col.into_any_element()
}

// ---------------------------------------------------------------------------
// buttons

pub fn buttons(node: &Node, b: &CardV2Block, _window: &mut Window, cx: &mut App) -> AnyElement {
    let CardV2Block::Buttons { id, buttons, .. } = b else {
        return div().into_any_element();
    };
    let ctx = node.ctx;
    let live = live(node, b);
    let armed = node.ui.block(id).focus.clone();
    let row = div()
        .flex()
        .flex_wrap()
        .gap(space::S8)
        .children(buttons.iter().map(|x| {
            let danger = x.style == Some(ButtonStyle::Danger);
            let is_armed = armed.as_deref() == Some(x.id.as_str());
            let label = match (&x.confirm, is_armed) {
                (Some(cf), true) => cf.title.clone(),
                _ => x.label.clone(),
            };
            let (emit, ui, block, bid, needs) = (
                Emitter::of(ctx),
                ctx.ui.clone(),
                id.clone(),
                x.id.clone(),
                x.confirm.is_some() && !is_armed,
            );
            button(
                eid(node, format!("btn-{id}-{}", x.id)),
                label,
                if danger || is_armed {
                    PushStyle::Destructive
                } else {
                    PushStyle::Outline
                },
                cx,
            )
            .disabled(!live)
            .on_click(move |_, _, cx| {
                // A confirm asks once in place: the button says the confirm title, a second
                // press sends.
                if needs {
                    ui.update(cx, |u, cx| {
                        u.block_mut(&block).focus = Some(bid.clone());
                        cx.notify();
                    });
                    return;
                }
                ui.update(cx, |u, cx| {
                    u.block_mut(&block).focus = None;
                    cx.notify();
                });
                emit.send(&block, "button.press", json!({ "buttonId": bid }), None, cx);
            })
        }));
    row.into_any_element()
}

// ---------------------------------------------------------------------------
// input (2.1: slider and live `input.change`)

struct SliderHost {
    state: Entity<SliderState>,
    _sub: Subscription,
}

pub fn input(node: &Node, b: &CardV2Block, window: &mut Window, cx: &mut App) -> AnyElement {
    let ctx = node.ctx;
    let CardV2Block::Input {
        id,
        kind,
        min,
        max,
        step,
        unit,
        label,
        ..
    } = b
    else {
        return div().into_any_element();
    };
    let k = theme::colors(cx);
    let live = live(node, b);
    if *kind == InputKind::Slider {
        let lo = min.as_ref().and_then(|n| n.as_f64()).unwrap_or(0.) as f32;
        let hi = max.as_ref().and_then(|n| n.as_f64()).unwrap_or(100.) as f32;
        let st = step.as_ref().and_then(|n| n.as_f64()).unwrap_or(1.) as f32;
        let current = match ctx.form.read(cx).values(cx).get(id.as_str()) {
            Some(Val::Num(n)) => *n as f32,
            Some(Val::Text(t)) => t.trim().parse().unwrap_or(lo),
            _ => lo,
        };
        let (form, block, target) = (ctx.form.clone(), id.clone(), PickTarget::of(node));
        let host = window.use_keyed_state(eid(node, format!("slider-{id}")), cx, |window, cx| {
            let state = cx.new(|_| {
                SliderState::new()
                    .min(lo)
                    .max(hi)
                    .step(st)
                    .default_value(current)
            });
            let sub = cx.subscribe_in(&state, window, move |_, _, e: &SliderEvent, _, cx| {
                let v = match e {
                    SliderEvent::Change(v) | SliderEvent::Release(v) => v.start() as f64,
                };
                form.update(cx, |f, cx| f.set_value21(&block, Val::Num(v), cx));
                // Live events go on release (INTERACT §3.2).
                if matches!(e, SliderEvent::Release(_)) {
                    target.after(&block, cx);
                }
            });
            SliderHost { state, _sub: sub }
        });
        let state = host.read(cx).state.clone();
        let shown = {
            let v = current as f64;
            let n = b20::fmt_number(v);
            match unit {
                Some(u) => format!("{n} {u}"),
                None => n,
            }
        };
        return div()
            .flex()
            .items_center()
            .gap(space::S12)
            .child(
                // FormRow label (§2.8): one line in the 112 column, cut at the end.
                b20::small(div())
                    .flex_none()
                    .w(m::FORM_LABEL)
                    .one_line()
                    .text_color(k.ink)
                    .child(crate::ui::truncate::TruncText::new(label.clone())),
            )
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.))
                    .child(Slider::new(&state).disabled(!live)),
            )
            .child(
                b20::small(div())
                    .flex_none()
                    .min_w(m::TABLE_NUM_COL)
                    .flex()
                    .justify_end()
                    .whitespace_nowrap()
                    .n_figures()
                    .text_color(k.ink)
                    .child(shown),
            )
            .into_any_element();
    }
    // Text: `input.change` 600 ms after the last keystroke, when declared.
    if super::interact::sends(ctx.card, b, "input.change") && live {
        let text = match ctx.form.read(cx).values(cx).get(id.as_str()) {
            Some(Val::Text(t)) => t.clone(),
            _ => String::new(),
        };
        let last = node.ui.block(id).tab.clone();
        let (store, send) = text_change(last.as_deref(), &text);
        if store {
            let ui = ctx.ui.clone();
            let block = id.clone();
            let t2 = text.clone();
            ui.update(cx, |u, _| u.block_mut(&block).tab = Some(t2));
            if send {
                Emitter::of(ctx).send_later(
                    id,
                    "input.change",
                    json!({ "value": text }),
                    t::DEBOUNCE_TEXT,
                    cx,
                );
            }
        }
    }
    // A field with its label above (a column of a row is narrow), the help below.
    let CardV2Block::Input {
        placeholder,
        options,
        help,
        ..
    } = b
    else {
        return div().into_any_element();
    };
    let value_empty = match ctx.form.read(cx).values(cx).get(id.as_str()) {
        Some(Val::Text(t)) => t.trim().is_empty(),
        Some(Val::Choice(c)) => c.is_empty(),
        Some(_) => false,
        None => true,
    };
    if !live && value_empty && !is_required(b) {
        // Read-only and empty: nothing to read.
        return div().into_any_element();
    }
    let problems = crate::views::cards::v2::form::problems(ctx.card, &ctx.values);
    let invalid = problems
        .iter()
        .any(|p| matches!(p, crate::views::cards::v2::form::Problem::Invalid(l) if l == label));
    let in_q = node
        .tree
        .parent(id)
        .is_some_and(|p| matches!(node.tree.get(p), Some(CardV2Block::Questionnaire { .. })));
    let control = b20::input_control(
        ctx,
        id,
        *kind,
        label,
        placeholder.as_deref(),
        options.as_deref().unwrap_or(&[]),
        invalid,
        false,
        // Off the ask's surface, a field takes text only when it sends live events.
        (ctx.card.ask.is_none() || !ctx.card.surface_ids().contains(id.as_str()))
            .then(|| live && b.base().is_some_and(|x| !x.events().is_empty())),
        window,
        cx,
    );
    div()
        .flex()
        .flex_col()
        .gap(space::S6)
        .min_w(px(0.))
        .when(!in_q, |el| {
            el.child(b20::small(div()).text_color(k.ink).child(label.clone()))
        })
        .child(control)
        .children(help.clone().map(|h| hint(h, cx)))
        .into_any_element()
}

// ---------------------------------------------------------------------------
// diagram and embed

pub fn diagram(node: &Node, b: &CardV2Block, window: &mut Window, cx: &mut App) -> AnyElement {
    let CardV2Block::Diagram {
        id,
        source,
        title,
        caption,
        alt,
        ..
    } = b
    else {
        return div().into_any_element();
    };
    let ctx = node.ctx;
    crate::diagrams::view::inline(
        crate::diagrams::view::InlineSpec {
            id: eid(node, format!("diagram-{id}")),
            source: source.clone(),
            title: title.clone(),
            caption: caption.clone(),
            alt: alt.clone(),
            origin: crate::diagrams::DiagramSource {
                message_id: Some(ctx.msg.message_id.clone()),
                topic_id: Some(ctx.msg.topic_id.clone()),
                artifact: None,
                block: crate::diagrams::DiagramBlock::Card {
                    block_id: id.clone(),
                },
                block_name: super::parts::block_segment(ctx.card, b),
            },
        },
        window,
        cx,
    )
}

/// What a draw of a text input does with its text: store it as the last seen, and send
/// `input.change`. The first draw only stores (a default or restored text is no edit).
fn text_change(last: Option<&str>, text: &str) -> (bool, bool) {
    match last {
        None => (true, false),
        Some(l) if l == text => (false, false),
        Some(_) => (true, true),
    }
}

type FolderState = Option<Result<crate::data::ArtifactFolder, crate::data::FileError>>;

/// One cached ask for a version folder: the answer, and when it came.
#[derive(Clone)]
struct Asked {
    state: FolderState,
    at: Instant,
}

thread_local! {
    /// Version folders of embedded pages, by `(产出, version)`. A failure is kept for
    /// [`FOLDER_RETRY`] only, so a later draw asks again (the app came back online).
    static FOLDERS: RefCell<HashMap<(String, u32), Asked>> = RefCell::new(HashMap::new());
}

/// How long a failed folder ask stays before a draw asks again.
const FOLDER_RETRY: Duration = Duration::from_secs(15);

/// True when a cached folder state should be asked for again at `now`.
fn stale(a: &Asked, now: Instant) -> bool {
    matches!(a.state, Some(Err(_))) && now.duration_since(a.at) >= FOLDER_RETRY
}

/// Forgets the folder of `(artifact, version)`, so the next draw asks again (重试).
fn forget_folder(artifact: &str, version: u32) {
    FOLDERS.with(|f| f.borrow_mut().remove(&(artifact.to_string(), version)));
}

fn folder(ctx: &Ctx, artifact: &str, version: u32, cx: &mut App) -> FolderState {
    let key = (artifact.to_string(), version);
    let now = Instant::now();
    if let Some(a) = FOLDERS.with(|f| f.borrow().get(&key).cloned())
        && !stale(&a, now)
    {
        return a.state;
    }
    FOLDERS.with(|f| {
        f.borrow_mut().insert(
            key.clone(),
            Asked {
                state: None,
                at: now,
            },
        )
    });
    let rx = ctx
        .env
        .provider
        .open_artifact_version(artifact, Some(version));
    cx.spawn(async move |cx: &mut gpui::AsyncApp| {
        let result = rx.await.unwrap_or(Err(crate::data::FileError::Other));
        FOLDERS.with(|f| {
            f.borrow_mut().insert(
                key,
                Asked {
                    state: Some(result),
                    at: Instant::now(),
                },
            )
        });
        cx.update(|cx| cx.refresh_windows());
    })
    .detach();
    None
}

/// What the body of an embed shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum EmbedBody {
    Page,
    Loading,
    /// The folder did not load: 页面没有打开 with 重试.
    Failed,
    /// The 产出 is not a web page: say so and offer the window.
    NotPage,
}

fn embed_body(state: &FolderState, has_detail: bool) -> EmbedBody {
    match state {
        Some(Ok(f)) if !f.is_page => EmbedBody::NotPage,
        Some(Ok(_)) if has_detail => EmbedBody::Page,
        Some(Ok(_)) | None => EmbedBody::Loading,
        Some(Err(_)) => EmbedBody::Failed,
    }
}

pub fn embed(node: &Node, b: &CardV2Block, _window: &mut Window, cx: &mut App) -> AnyElement {
    let CardV2Block::Embed {
        id,
        artifact_id,
        version,
        height,
        title,
        ..
    } = b
    else {
        return div().into_any_element();
    };
    let ctx = node.ctx;
    let k = theme::colors(cx);
    let h = height
        .map(|h| h as f32)
        .unwrap_or(t::EMBED_DEFAULT)
        .clamp(t::EMBED_MIN, t::EMBED_MAX);
    let detail = ctx.env.provider.artifact(artifact_id);
    let name = title
        .clone()
        .or_else(|| detail.as_ref().map(|d| d.summary.title.clone()))
        .unwrap_or_default();
    let declared: Vec<String> = b.base().map(|x| x.events().to_vec()).unwrap_or_default();
    let state = folder(ctx, artifact_id, *version, cx);
    let shown = embed_body(&state, detail.is_some());
    // A 产出 that is not a web page is framed as a file: its kind, no sandbox and no page
    // actions (CARD21 §3 embed).
    let file = match (&state, shown) {
        (Some(Ok(f)), EmbedBody::NotPage) => {
            use crate::views::media;
            Some(media::file_kind(&f.entry, media::mime_for(&f.entry)))
        }
        _ => None,
    };
    crate::selftest::drawn::mark(
        "embed-frame",
        format!(
            "{id}:{}",
            file.map_or(c::EMBED_KIND, crate::views::media::MediaKind::label)
        ),
    );
    let body: AnyElement = match (state, shown) {
        (Some(Ok(f)), EmbedBody::Page) => {
            // inc-integrate: the 产出 page host owns the bridge (capabilities by author,
            // live data, asks, refs, the pending band); the block narrows the declared
            // events to its own `events` (embed.events ⊆ the version's events).
            use crate::views::artifacts::host;
            let p = ctx.env.provider.as_ref();
            let (mut spec, caps) =
                host::version_spec(p, artifact_id, f.version, f.dir.clone(), f.entry.clone());
            if !declared.is_empty()
                && let Some(events) = spec.events.take()
            {
                spec.events = Some(
                    events
                        .into_iter()
                        .filter(|e| declared.contains(&e.name))
                        .collect(),
                );
            }
            crate::ui::web_embed::WebEmbed::new(
                eid(node, format!("embed-{id}-{version}")),
                crate::ui::web_embed::WebEmbedSpec::new(artifact_id.clone(), spec)
                    .capabilities(caps)
                    .viewer(host::viewer(p))
                    .ref_menu(Some(host::ref_menu(cx))),
            )
            .height(px(h))
            .rounded(px(0.))
            .pass_scroll(true)
            .on_host(host::embed_handler(artifact_id.clone()))
            .on_event(move |event, _, _| {
                if let artifact_view::ArtifactEvent::ExternalLink { url } = event {
                    crate::views::artifacts::window::ask_before_link(url.clone())
                }
            })
            .into_any_element()
        }
        (_, EmbedBody::Failed) => {
            let (art, v) = (artifact_id.clone(), *version);
            div()
                .h(px(h.min(200.)))
                .bg(k.sunken)
                .flex()
                .flex_col()
                .items_center()
                .justify_center()
                .gap(space::S8)
                .child(hint(c::EMBED_FAILED, cx))
                .child(
                    b20::quiet_button(eid(node, format!("embed-retry-{id}")), c::PENDING_RETRY, cx)
                        .text_color(k.ink)
                        .on_click(move |_, _, cx| {
                            forget_folder(&art, v);
                            cx.refresh_windows();
                        }),
                )
                .into_any_element()
        }
        (_, EmbedBody::NotPage) => {
            let a = ctx.answerer();
            let target = crate::data::cards_v2::RefTarget::Artifact {
                artifact_id: artifact_id.clone(),
                version: Some(*version),
            };
            div()
                .h(px(h.min(200.)))
                .bg(k.sunken)
                .flex()
                .flex_col()
                .items_center()
                .justify_center()
                .gap(space::S8)
                .child(hint(c::EMBED_NOT_PAGE, cx))
                .child(
                    b20::quiet_button(eid(node, format!("embed-window-{id}")), c::EMBED_OPEN, cx)
                        .text_color(k.ink)
                        .on_click(move |_, window, cx| a.open(&target, window, cx)),
                )
                .into_any_element()
        }
        _ => div()
            .h(px(h.min(200.)))
            .bg(k.sunken)
            .flex()
            .items_center()
            .justify_center()
            .child(hint(c::EMBED_LOADING, cx))
            .into_any_element(),
    };
    let (g, tp) = (ctx.msg.group_id.clone(), ctx.msg.topic_id.clone());
    let a = ctx.answerer();
    let target = crate::data::cards_v2::RefTarget::Artifact {
        artifact_id: artifact_id.clone(),
        version: Some(*version),
    };
    let _ = (g, tp);
    div()
        .rounded(theme::radius::LG)
        .border_1()
        .border_color(k.line)
        .overflow_hidden()
        .flex()
        .flex_col()
        .child(
            meta(div())
                .h(crate::ui::native::tokens::height::LG)
                .px(space::S12)
                .flex()
                .items_center()
                .gap(space::S8)
                .bg(k.raised)
                .border_b_1()
                .border_color(k.line)
                .child(ui::icon(
                    file.map_or(Sym::Document, crate::views::media::MediaKind::icon),
                    px(14.),
                    k.ink_3,
                ))
                .child(
                    b20::small(div())
                        .min_w(px(0.))
                        .one_line()
                        .font_weight(theme::MEDIUM_WEIGHT)
                        .text_color(k.ink)
                        .child(name),
                )
                .child(
                    div()
                        .flex_none()
                        .mono()
                        .text_color(k.ink_3)
                        .child(fill(c::EMBED_VERSION, &[("n", &version.to_string())])),
                )
                .child(div().flex_none().text_color(k.ink_3).child(format!(
                    "· {}",
                    file.map_or(c::EMBED_KIND, crate::views::media::MediaKind::label)
                )))
                .when(file.is_none(), |el| {
                    el.child(
                        div()
                            .ml_auto()
                            .flex_none()
                            .flex()
                            .items_center()
                            .gap(space::S4)
                            .text_color(k.ink_3)
                            .child(ui::icon(Sym::Shield, px(12.), k.ink_3))
                            .child(c::EMBED_SANDBOX),
                    )
                })
                .child(
                    div()
                        .id(eid(node, format!("embed-open-{id}")))
                        .when(file.is_some(), |el| el.ml_auto())
                        .flex_none()
                        .size(px(24.))
                        .rounded(crate::ui::native::tokens::radius::ITEM)
                        .flex()
                        .items_center()
                        .justify_center()
                        .hover(|s| s.bg(k.hover))
                        .on_click(move |_, window, cx| a.open(&target, window, cx))
                        .child(ui::icon(Sym::OpenExternal, px(14.), k.ink_2)),
                ),
        )
        .child(body)
        .when(file.is_none(), |el| {
            el.child(
                meta(div())
                    .h(crate::ui::native::tokens::height::DEFAULT)
                    .px(space::S12)
                    .flex()
                    .items_center()
                    .gap(space::S6)
                    .bg(k.raised)
                    .border_t_1()
                    .border_color(k.line)
                    .text_color(k.ink_3)
                    .child(ui::icon(Sym::ArrowRight, px(12.), k.ink_3))
                    .child(fill(c::EMBED_FOOT, &[("name", &ctx.author)]))
                    .child(div().ml_auto().child(c::EMBED_NO_APPROVE)),
            )
        })
        .into_any_element()
}

/// Sends my answer that did not get there again (CARD21 §2: the red 失败 band with 重试):
/// with the same `clientEventId` when the outbox still has it, else as a new event.
pub fn resend_answer(ctx: &Ctx) -> impl Fn() + 'static {
    let provider = ctx.env.provider.clone();
    let (g, tp, msg) = (
        ctx.msg.group_id.clone(),
        ctx.msg.topic_id.clone(),
        ctx.msg.message_id.clone(),
    );
    let rev = ctx.card.rev;
    let pending = provider.card_v2_pending(&g, &msg);
    let last = provider
        .card_v2_events(&g, &msg)
        .into_iter()
        .rev()
        .find(|e| e.durable && !e.dropped);
    move || {
        crate::selftest::drawn::mark("card-resend", msg.clone());
        if let Some(e) = &last
            && provider.retry_card_event(&g, &tp, &msg, &e.client_event_id)
        {
            return;
        }
        let request = match (&last, &pending) {
            (Some(e), _) => e.request.clone(),
            (None, Some(p)) => {
                let value = p.inputs.as_ref().and_then(|i| i.get(&p.action_id)).cloned();
                crate::data::cards_v2::CardEventRequest {
                    rev,
                    block_id: Some(p.action_id.clone()),
                    event: "choice.submit".into(),
                    value,
                    ..Default::default()
                }
            }
            _ => return,
        };
        provider.send_card_event(&g, &tp, &msg, request);
    }
}

// ---------------------------------------------------------------------------
// A block this version does not know (CARD21 §6)

pub fn unknown(node: &Node, b: &CardV2Block, cx: &mut App) -> AnyElement {
    let ctx = node.ctx;
    let k = theme::colors(cx);
    // CARD21 §6: the block's own fallbackText, when the sender gave one.
    let fallback = ctx
        .card
        .blocks
        .iter()
        .position(|x| std::ptr::eq(x, b))
        .and_then(|i| ctx.card.unknown_fallback(i))
        .map(str::to_string);
    div()
        .p(space::S12)
        .rounded(theme::radius::MD)
        .bg(k.sunken)
        .border_1()
        .border_color(k.line)
        .flex()
        .gap(space::S10)
        .child(
            div()
                .pt(px(2.))
                .child(ui::icon(Sym::Info, px(14.), k.ink_3)),
        )
        .child(
            div()
                .flex()
                .flex_col()
                .gap(space::S4)
                .min_w(px(0.))
                .child(
                    div()
                        .flex()
                        .items_baseline()
                        .gap(space::S8)
                        .child(
                            b20::small(div())
                                .font_weight(theme::MEDIUM_WEIGHT)
                                .text_color(k.ink)
                                .child(c::UNKNOWN_BLOCK),
                        )
                        .child(meta(div()).mono().text_color(k.ink_3).child(format!(
                            "{} · {}",
                            ctx.card.catalog(),
                            c::UNKNOWN_KIND
                        ))),
                )
                .when_some(fallback, |el, t| {
                    el.child(b20::small(div()).text_color(k.ink_2).child(t))
                }),
        )
        .into_any_element()
}

/// The card's only input is a block this version does not know: 更新 and no submit.
fn only_input_unknown(ctx: &Ctx) -> bool {
    ctx.card.ask.is_some()
        && ctx
            .card
            .blocks
            .iter()
            .any(|b| matches!(b, CardV2Block::Unknown))
        && !ctx.card.blocks.iter().any(CardV2Block::takes_input)
}

// ---------------------------------------------------------------------------
// Footers

/// The 2.1 footers, when the 2.0 footer does not fit: the submit of a non-blocking ask
/// inside a report, an immediate-only surface (no submit), a poll's note, and 更新 when the
/// only input is unknown. `None`: the 2.0 footer draws.
pub fn footer(ctx: &Ctx, _window: &mut Window, cx: &mut App) -> Option<Option<AnyElement>> {
    if !super::is_21(ctx.card) {
        return None;
    }
    let k = theme::colors(cx);
    let bar = |items: Vec<AnyElement>, note: Option<AnyElement>| {
        crate::views::cards::v2::footer_stack(items, note, cx)
    };
    let note = |t: String| {
        meta(div())
            .min_w(px(0.))
            .one_line()
            .text_color(k.ink_3)
            .child(t)
            .into_any_element()
    };
    if only_input_unknown(ctx) && matches!(ctx.state, CardState::Pending) {
        return Some(Some(bar(
            vec![
                button(
                    ctx.id("update"),
                    fill(c::UPDATE_APP, &[("product", env!("PRODUCT_NAME"))]),
                    PushStyle::Outline,
                    cx,
                )
                .on_click(|_, _, cx| {
                    if let Some(u) = crate::updater::global(cx) {
                        u.update(cx, |u, cx| u.check_now(cx));
                    }
                })
                .into_any_element(),
            ],
            Some(note(c::UNKNOWN_FOOT.into()).into_any_element()),
        )));
    }
    // My answer did not get there: 重试 with the same answer, and it goes again by itself.
    if let CardState::SendFailed { .. } = ctx.state {
        let resend = resend_answer(ctx);
        crate::selftest::drawn::mark("card-band-retry", ctx.msg.message_id.clone());
        return Some(Some(bar(
            vec![
                button(
                    ctx.id("retry21"),
                    c::PENDING_RETRY.to_string(),
                    PushStyle::Outline,
                    cx,
                )
                .icon(ui::icon(Sym::Refresh, px(14.), k.ink))
                .on_click(move |_, _, _| resend())
                .into_any_element(),
            ],
            Some(note(c::SEND_AUTO.into()).into_any_element()),
        )));
    }
    // A poll: the rules, no submit (a press votes).
    if let Some(CardV2Block::Poll {
        public, changeable, ..
    }) = ctx
        .card
        .blocks
        .iter()
        .find(|b| matches!(b, CardV2Block::Poll { .. }))
    {
        let mut items = vec![
            meta(div())
                .text_color(k.ink_3)
                .child(if public.unwrap_or(false) {
                    c::POLL_PUBLIC
                } else {
                    c::POLL_PRIVATE
                })
                .into_any_element(),
        ];
        // No buttons here: the rules read as one line.
        if changeable.unwrap_or(false) {
            items.push(note(c::POLL_CHANGEABLE.into()).into_any_element());
        }
        return Some(Some(bar(items, None)));
    }
    // Only immediate blocks (a confirm, a single pick): the press is the answer.
    let surface = ctx.card.surface_ids();
    let immediate_only = !surface.is_empty()
        && surface.iter().all(|id| {
            ctx.card.block(id).is_some_and(|b| {
                matches!(b, CardV2Block::Confirm { .. })
                    || b.base().is_some_and(|x| x.is_immediate())
            })
        })
        && !ctx
            .card
            .actions()
            .iter()
            .any(|a| a.kind() == CardActionKind::Submit);
    if immediate_only && matches!(ctx.state, CardState::Pending | CardState::WaitingOn) {
        return Some(None);
    }
    // A non-blocking ask inside a report: the submit for the asked viewer.
    if ctx.surface_live && !ctx.state.live() {
        let a = ctx.answerer();
        let mut items: Vec<AnyElement> = Vec::new();
        let problems = crate::views::cards::v2::form::problems(ctx.card, &ctx.values);
        let ready = problems.is_empty();
        for x in ctx
            .card
            .actions()
            .iter()
            .filter(|x| matches!(x.kind(), CardActionKind::Submit | CardActionKind::Reject))
        {
            let primary = x.style == Some(CardV2ActionStyle::Primary);
            let action: CardV2Action = x.clone();
            let a = a.clone();
            items.push(if primary && !ready {
                disabled_button(ctx.id(format!("act-{}", x.id)), x.label.clone(), cx)
                    .into_any_element()
            } else {
                button(
                    ctx.id(format!("act-{}", x.id)),
                    x.label.clone(),
                    if primary {
                        PushStyle::Default
                    } else {
                        PushStyle::Outline
                    },
                    cx,
                )
                .on_click(move |_, window, cx| a.press(&action, window, cx))
                .into_any_element()
            });
        }
        let desc = ctx
            .card
            .ask
            .as_ref()
            .and_then(|a| a.promise.clone())
            .map(|p| note(p).into_any_element());
        return Some((!items.is_empty()).then(|| bar(items, desc)));
    }
    let _ = mix;
    None
}

#[cfg(test)]
mod leaf_tests {
    use super::*;
    use crate::data::{ArtifactFolder, FileError};

    fn folder(is_page: bool) -> FolderState {
        Some(Ok(ArtifactFolder {
            artifact_id: "a".into(),
            version: 1,
            dir: "/tmp".into(),
            entry: "index.html".into(),
            is_page,
        }))
    }

    #[test]
    fn a_text_input_sends_only_after_an_edit() {
        assert_eq!(text_change(None, "默认的话"), (true, false), "first draw");
        assert_eq!(text_change(None, ""), (true, false));
        assert_eq!(text_change(Some("默认的话"), "默认的话"), (false, false));
        assert_eq!(text_change(Some("默认的话"), "默认的"), (true, true));
        assert_eq!(
            text_change(Some("a"), ""),
            (true, true),
            "clearing is an edit"
        );
    }

    #[test]
    fn a_failed_or_odd_folder_never_says_loading() {
        assert_eq!(embed_body(&None, true), EmbedBody::Loading);
        assert_eq!(embed_body(&folder(true), true), EmbedBody::Page);
        assert_eq!(embed_body(&folder(false), true), EmbedBody::NotPage);
        assert_eq!(
            embed_body(&Some(Err(FileError::Other)), true),
            EmbedBody::Failed
        );
    }

    #[test]
    fn a_failure_is_asked_again_later_but_a_page_is_kept() {
        let t0 = Instant::now();
        let failed = Asked {
            state: Some(Err(FileError::Other)),
            at: t0,
        };
        assert!(!stale(&failed, t0 + Duration::from_secs(1)));
        assert!(stale(&failed, t0 + FOLDER_RETRY));
        let ok = Asked {
            state: folder(true),
            at: t0,
        };
        assert!(!stale(&ok, t0 + FOLDER_RETRY * 10));
        FOLDERS.with(|f| f.borrow_mut().insert(("a".into(), 1), failed));
        forget_folder("a", 1);
        assert!(FOLDERS.with(|f| f.borrow().get(&("a".to_string(), 1)).is_none()));
    }
}
