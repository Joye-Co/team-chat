//! One card of 需要你拍板 (Main.dc.html `article`): the asker, the 话题 path
//! and the time; the question; the body (facts, a reason, proof chips, a
//! draft preview, lettered options); and a footer with the answers on
//! `raised`. Once answered, the footer (or the options) collapses into the
//! record: a drawn check, `你选了「…」`, and 撤回 while it can be taken back.
//!
//! A v2 card (`Message.cardV2`) is drawn from its blocks; a v1 card from its
//! blocks and buttons; an ask without a card (a 话题 that waits on me) gets
//! one line and 打开话题.

use std::collections::BTreeMap;

use chat_core::proto::{
    CardActionKind, CardV2, CardV2Action, CardV2ActionStyle, CardV2Block, EvidenceState, RefTarget,
};
use gpui::{
    AnyElement, App, Context, ElementId, InteractiveElement as _, IntoElement, ParentElement,
    SharedString, StatefulInteractiveElement as _, Styled, StyledImage as _, Window, div,
    prelude::FluentBuilder as _, px,
};
use gpui_component::{WindowExt as _, button::ButtonVariant};

use super::{
    DeskPage,
    done::{chip, mono_lead},
    model::{Answer, AskCard},
};
use crate::{
    app::Services,
    copy::{common, desk as c, fill},
    data::{
        Millis,
        card::{ActionStyle, Card, CardAction, CardBlock, CardField},
        v2::AskKind,
    },
    theme::{self, Type as _, space},
    ui::{self, ButtonKind, ButtonSize, Lucide},
    views::shell::clock,
};

/// `你选了「批准合并」`.
pub fn button_record(label: &str) -> String {
    fill(c::DESK_ANSWERED, &[("label", label)])
}

/// `你选了 A：一个输入框，粘贴自动识别`.
pub fn option_record(key: &str, label: &str) -> String {
    fill(c::DESK_ANSWERED_OPTION, &[("key", key), ("label", label)])
}

/// The letter and label of option `ix`. A label written as `A 一个输入框`
/// keeps its own letter.
pub fn option_parts(ix: usize, label: &str) -> (String, String) {
    let mut chars = label.chars();
    if let (Some(first), Some(' ')) = (chars.next(), chars.next())
        && first.is_ascii_alphanumeric()
    {
        return (
            first.to_ascii_uppercase().to_string(),
            label[2..].trim().to_string(),
        );
    }
    let letter = (b'A' + (ix.min(25) as u8)) as char;
    (letter.to_string(), label.to_string())
}

/// The record of a v1 choice press, when `action_id` is one of its options.
pub fn v1_option_record(card: &Card, action_id: &str) -> Option<String> {
    let actions = card.actions.as_deref().unwrap_or(&[]);
    let ix = actions.iter().position(|a| a.id == action_id)?;
    let (key, label) = option_parts(ix, &actions[ix].label);
    Some(option_record(&key, &label))
}

/// The record of a v2 answer: the picked option, else the action's label.
pub fn v2_record(
    card: &CardV2,
    action_id: &str,
    inputs: &BTreeMap<String, serde_json::Value>,
) -> String {
    for b in all_blocks(&card.blocks) {
        if let CardV2Block::Decision { id, options, .. } = b
            && let Some(serde_json::Value::String(picked)) = inputs.get(id)
            && let Some(ix) = options.iter().position(|o| &o.id == picked)
        {
            let (key, label) = option_parts(ix, &options[ix].label);
            return option_record(&key, &label);
        }
    }
    let label = card
        .actions
        .as_deref()
        .unwrap_or(&[])
        .iter()
        .find(|a| a.id == action_id)
        .map(|a| a.label.clone())
        .unwrap_or_else(|| action_id.to_string());
    button_record(&label)
}

fn all_blocks(blocks: &[CardV2Block]) -> Vec<&CardV2Block> {
    let mut out = Vec::new();
    for b in blocks {
        out.push(b);
        if let CardV2Block::Group { blocks, .. } = b {
            out.extend(blocks.iter().flatten());
        }
    }
    out
}

/// What the record of an answered card shows.
struct Answered {
    record: String,
    at: Millis,
    undoable: bool,
    sent: bool,
}

fn answered(a: &AskCard, page: &DeskPage) -> Option<Answered> {
    if let Some(local) = page.local_answer(&a.key()) {
        return Some(Answered {
            record: local.record.clone(),
            at: local.at,
            undoable: local.undoable,
            sent: !local.undoable,
        });
    }
    a.answered.as_ref().map(|r| Answered {
        record: r.record.clone(),
        at: r.at,
        undoable: false,
        sent: !r.pending,
    })
}

/// Draws one ask.
pub fn render(
    a: &AskCard,
    page: &DeskPage,
    _window: &mut Window,
    cx: &mut Context<DeskPage>,
) -> gpui::Div {
    let key = a.key();
    let done = answered(a, page);
    let s = &page.services().clone();
    let asker = page.snap().name(&a.item.from);
    let top = ui::card_padding(div())
        .flex()
        .flex_col()
        .child(header(a, page, cx))
        .child(title(a, page, cx));

    let (body, footer): (Vec<AnyElement>, Option<AnyElement>) = if let Some(v2) = &a.card_v2 {
        v2_parts(a, s, v2, &key, done.as_ref(), &asker, cx)
    } else if let Some(v1) = &a.card {
        v1_parts(a, s, v1, &key, done.as_ref(), &asker, cx)
    } else {
        topic_parts(a, s, cx)
    };

    ui::card(cx)
        .overflow_hidden()
        .flex()
        .flex_col()
        .child(top.children(body))
        .children(footer)
}

/// The asker, its 岗位, `群 / 话题` and the time.
fn header(a: &AskCard, page: &DeskPage, cx: &mut Context<DeskPage>) -> impl IntoElement {
    let k = theme::colors(cx);
    let services = page.services();
    let snap = page.snap();
    let member = snap.member(&a.item.from);
    let (group, topic) = snap.places.get(&a.key()).cloned().unwrap_or_default();
    let nav = services.nav.clone();
    div()
        .flex()
        .items_center()
        .gap(space::S8)
        .t_meta()
        .text_color(k.ink_3)
        .child(ui::MemberAvatar::new(member.as_ref(), px(20.)))
        .child(
            div()
                .flex_shrink(0.)
                .max_w(NAME_MAX)
                .one_line()
                .t_small()
                .medium()
                .text_color(k.ink)
                .child(member.as_ref().map(|m| m.name.clone()).unwrap_or_default()),
        )
        .children(
            member
                .and_then(|m| m.role)
                .filter(|r| !r.trim().is_empty())
                .map(|r| {
                    div()
                        .min_w(px(0.))
                        .overflow_hidden()
                        .child(ui::JobTag::new(r))
                }),
        )
        .child(path(a, group, topic, nav, cx))
        .child(div().flex_none().ml_auto().mono().child(if a.item.at > 0 {
            clock(a.item.at)
        } else {
            String::new()
        }))
}

/// The widest the asker's name gets in the header; a longer name ends with
/// an ellipsis.
const NAME_MAX: gpui::Pixels = px(180.);

/// `群 / 话题` in two parts, as in the Card v2 header: the path takes the
/// room up to the time, the 群 gives way first (it shrinks ten times faster,
/// down to one character and an ellipsis, and takes at most 45 %), then the
/// 话题 shrinks with its own ellipsis. A 群 of one or two characters never
/// shrinks. Either part opens the 话题.
fn path(
    a: &AskCard,
    group: String,
    topic: String,
    nav: gpui::Entity<crate::app::Navigation>,
    cx: &App,
) -> impl IntoElement {
    let k = theme::colors(cx);
    let key = a.key();
    let part = |which: &str, text: String| {
        let nav = nav.clone();
        let (g, t) = (a.item.group_id.clone(), a.item.topic_id.clone());
        div()
            .id(SharedString::from(format!("desk-path-{which}-{key}")))
            .min_w(px(0.))
            .one_line()
            .cursor_pointer()
            .hover(|s| s.text_color(k.ink))
            .on_click(move |_, _, cx| {
                let (g, t) = (g.clone(), t.clone());
                nav.update(cx, |nav, cx| nav.open_topic(g, t, cx));
            })
            .child(text)
    };
    let short = group.chars().count() <= 2;
    let has_group = !group.trim().is_empty();
    let has_topic = !topic.trim().is_empty();
    div()
        .ml(px(2.))
        .flex_1()
        .min_w(px(0.))
        .flex()
        .overflow_hidden()
        .text_color(k.ink_3)
        .when(has_group, |el| {
            el.child(
                part("group", group)
                    .when(short, |el| el.flex_none())
                    .when(!short, |el| {
                        el.min_w(theme::card_v2::PATH_GROUP_MIN)
                            .max_w(gpui::relative(0.45))
                            .flex_shrink(10.)
                    }),
            )
        })
        .when(has_group && has_topic, |el| {
            el.child(div().flex_none().px(space::S4).child("/"))
        })
        .when(has_topic, |el| {
            el.child(part("topic", topic).flex_shrink(1.).text_color(k.ink_2))
        })
}

fn title(a: &AskCard, page: &DeskPage, cx: &mut Context<DeskPage>) -> impl IntoElement {
    let k = theme::colors(cx);
    let nav = page.services().nav.clone();
    let (g, t) = (a.item.group_id.clone(), a.item.topic_id.clone());
    let text = a
        .card_v2
        .as_ref()
        .map(|c| c.title.clone())
        .or_else(|| a.card.as_ref().map(|c| c.title.clone()))
        .filter(|t| !t.trim().is_empty())
        .unwrap_or_else(|| a.item.title.clone());
    div()
        .id(SharedString::from(format!("desk-title-{}", a.key())))
        .mt(space::S10)
        .t_title()
        .text_color(k.ink)
        .cursor_pointer()
        .child(text)
        .on_click(move |_, _, cx| {
            let (g, t) = (g.clone(), t.clone());
            nav.update(cx, |nav, cx| nav.open_topic(g, t, cx));
        })
}

// --- bodies -----------------------------------------------------------------

/// `包含 / 验收 / 影响` facts: a 56 pt label column and the values.
fn fields(list: &[CardField], cx: &App) -> AnyElement {
    let k = theme::colors(cx);
    div()
        .mt(space::S10)
        .flex()
        .flex_col()
        .gap(space::S6)
        .t_small()
        .children(list.iter().map(|f| {
            div()
                .flex()
                .gap(space::S12)
                .child(
                    div()
                        .flex_none()
                        .w(px(56.))
                        .text_color(k.ink_3)
                        .child(f.label.clone()),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w(px(0.))
                        .text_color(k.ink)
                        .child(f.value.clone()),
                )
        }))
        .into_any_element()
}

/// A reason or explanation: 13/20 `ink-2`.
fn paragraph(text: &str, cx: &App) -> AnyElement {
    let k = theme::colors(cx);
    div()
        .mt(space::S6)
        .t_small()
        .text_color(k.ink_2)
        .child(text.to_string())
        .into_any_element()
}

/// A recommendation under the options: 12/18 `ink-3`.
fn note_line(text: &str, cx: &App) -> AnyElement {
    let k = theme::colors(cx);
    div()
        .mt(space::S10)
        .text_size(px(12.))
        .line_height(px(18.))
        .text_color(k.ink_3)
        .child(text.to_string())
        .into_any_element()
}

/// The draft preview: up to three image tiles and the copy (`sunken`).
fn draft_preview(
    a: &AskCard,
    s: &Services,
    images: &[String],
    label: Option<&str>,
    text: &str,
    cx: &mut Context<DeskPage>,
) -> AnyElement {
    let k = theme::colors(cx);
    let p = s.provider.clone();
    let tile = |ix: usize, w: f32, h: f32, cx: &App| {
        let path = images
            .get(ix)
            .and_then(|f| p.card_image(&a.item.group_id, &a.item.topic_id, f));
        div()
            .flex_none()
            .w(px(w))
            .h(px(h))
            .rounded(px(6.))
            .overflow_hidden()
            .bg(theme::colors(cx).ph[ix % 3])
            .children(path.map(|path| {
                gpui::img(path)
                    .size_full()
                    .object_fit(gpui::ObjectFit::Cover)
            }))
    };
    let grid = (!images.is_empty()).then(|| {
        if images.len() >= 3 {
            div()
                .flex_none()
                .flex()
                .gap(px(4.))
                .child(tile(0, 54., 74., cx))
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap(px(4.))
                        .child(tile(1, 54., 35., cx))
                        .child(tile(2, 54., 35., cx)),
                )
                .into_any_element()
        } else {
            tile(0, 112., 74., cx).into_any_element()
        }
    });
    div()
        .mt(space::S12)
        .flex()
        .gap(space::S14)
        .p(space::S12)
        .rounded(px(10.))
        .bg(k.sunken)
        .children(grid)
        .child(
            div()
                .flex_1()
                .min_w(px(0.))
                .flex()
                .flex_col()
                .gap(px(2.))
                .children(label.map(|l| {
                    div()
                        .text_size(px(14.))
                        .line_height(px(20.))
                        .semibold()
                        .text_color(k.ink)
                        .child(l.to_string())
                }))
                .child(
                    div()
                        .t_small()
                        .text_color(k.ink_2)
                        .line_clamp(2)
                        .child(text.to_string()),
                ),
        )
        .into_any_element()
}

/// Lettered options (`A 一个输入框…`): a press answers at once.
fn options(
    a: &AskCard,
    list: Vec<(String, String, Answer)>,
    cx: &mut Context<DeskPage>,
) -> AnyElement {
    let k = theme::colors(cx);
    div()
        .mt(space::S12)
        .flex()
        .flex_wrap()
        .gap(space::S8)
        .children(
            list.into_iter()
                .enumerate()
                .map(|(ix, (key, label, answer))| {
                    let ask = a.clone();
                    let record = option_record(&key, &label);
                    div()
                        .id(SharedString::from(format!("desk-opt-{}-{ix}", a.key())))
                        .flex()
                        .items_center()
                        .gap(space::S10)
                        .h(px(36.))
                        .pl(space::S8)
                        .pr(space::S14)
                        .border_1()
                        .border_color(k.line_2)
                        .rounded(px(9.))
                        .bg(k.bg)
                        .shadow(k.sec_hi())
                        .cursor_pointer()
                        .hover(|s| s.border_color(k.ink_3))
                        .t_small()
                        .text_color(k.ink)
                        .child(
                            div()
                                .flex_none()
                                .size(px(20.))
                                .rounded(px(6.))
                                .bg(k.sunken)
                                .flex()
                                .items_center()
                                .justify_center()
                                .mono()
                                .text_size(px(11.))
                                .line_height(px(20.))
                                .text_color(k.ink_2)
                                .child(key),
                        )
                        .child(label)
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.answer(ask.clone(), answer.clone(), record.clone(), cx);
                        }))
                }),
        )
        .into_any_element()
}

// --- footers and records ------------------------------------------------------

/// The footer band of a card: answers at the start, a quiet note at the end.
fn footer_band(cx: &App) -> gpui::Div {
    let k = theme::colors(cx);
    div()
        .flex()
        .items_center()
        .gap(space::S8)
        .px(space::S16)
        .py(space::S10)
        .border_t_1()
        .border_color(k.line)
        .bg(k.raised)
}

/// The note at the end of a footer, with an optional glyph.
fn footer_note(text: String, glyph: Option<Lucide>, mono: bool, cx: &App) -> AnyElement {
    let k = theme::colors(cx);
    div()
        .ml_auto()
        .flex_none()
        .flex()
        .items_center()
        .gap(space::S6)
        .t_meta()
        .text_color(k.ink_3)
        .when(mono, |el| el.mono())
        .children(glyph.map(|g| ui::icon(g, px(13.), k.ink_3)))
        .child(text)
        .into_any_element()
}

/// The record parts: a drawn check, the record, the quiet meta and 撤回.
fn record_parts(
    key: &str,
    done: &Answered,
    asker: &str,
    cx: &mut Context<DeskPage>,
) -> Vec<AnyElement> {
    let k = theme::colors(cx);
    // Only the answer I just made draws; yesterday's shows at rest.
    let check = ui::motion::check_mark(
        SharedString::from(format!("desk-check-{key}")),
        ui::icon(Lucide::Check, px(16.), k.ok),
        px(16.),
        ui::motion::just_answered(done.at),
        cx,
    );
    let meta = if done.sent {
        format!("· {}", fill(c::DESK_SENT_TO, &[("name", asker)]))
    } else {
        c::DESK_JUST_NOW.to_string()
    };
    let mut parts = vec![
        check,
        div()
            .min_w(px(0.))
            .one_line()
            .text_color(k.ink)
            .child(done.record.clone())
            .into_any_element(),
        div()
            .flex_none()
            .text_color(k.ink_3)
            .when(!done.sent, |el| el.mono())
            .child(meta)
            .into_any_element(),
    ];
    if done.undoable {
        let key = key.to_string();
        parts.push(
            ui::button(
                SharedString::from(format!("desk-undo-{key}")),
                c::DESK_UNDO,
                ButtonKind::Ghost,
                ButtonSize::Sm,
                cx,
            )
            .ml_auto()
            .on_click(cx.listener(move |this, _, _, cx| this.undo(&key, cx)))
            .into_any_element(),
        );
    }
    parts
}

/// The record in place of a footer.
fn record_band(key: &str, done: &Answered, asker: &str, cx: &mut Context<DeskPage>) -> AnyElement {
    let parts = record_parts(key, done, asker, cx);
    let band = footer_band(cx).py(space::S12).t_small().children(parts);
    record_fade(key, done, band)
}

/// The record in place of the options.
fn record_inline(
    key: &str,
    done: &Answered,
    asker: &str,
    cx: &mut Context<DeskPage>,
) -> AnyElement {
    let k = theme::colors(cx);
    let parts = record_parts(key, done, asker, cx);
    let row = div()
        .mt(space::S12)
        .flex()
        .items_center()
        .gap(space::S8)
        .h(px(36.))
        .px(space::S12)
        .rounded(px(9.))
        .bg(k.sunken)
        .t_small()
        .children(parts);
    record_fade(key, done, row)
}

/// A record fades in once, when its answer was just made.
fn record_fade(key: &str, done: &Answered, el: gpui::Div) -> AnyElement {
    if !ui::motion::just_answered(done.at) {
        return el.into_any_element();
    }
    ui::motion::once(
        SharedString::from(format!("desk-record-{key}")),
        theme::motion::FADE,
        el,
        |el, t| el.opacity(theme::motion::out_fn()(t)),
    )
}

/// One answer button of a footer. A `confirm` asks first in a dialog.
#[allow(clippy::too_many_arguments)]
fn answer_button(
    a: &AskCard,
    ix: usize,
    label: &str,
    kind: ButtonKind,
    answer: Answer,
    confirm: Option<(String, Option<String>)>,
    cx: &mut Context<DeskPage>,
) -> AnyElement {
    let ask = a.clone();
    let record = button_record(label);
    let ok_label = label.to_string();
    let danger = kind == ButtonKind::Danger;
    let page = cx.entity();
    ui::button(
        ElementId::Name(format!("desk-act-{}-{ix}", a.key()).into()),
        label.to_string(),
        kind,
        ButtonSize::Md,
        cx,
    )
    .when(kind == ButtonKind::Ghost, |b| {
        b.text_color(theme::colors(cx).ink_2)
    })
    .on_click(move |_, window, cx| {
        let Some((question, body)) = confirm.clone() else {
            page.update(cx, |this, cx| {
                this.answer(ask.clone(), answer.clone(), record.clone(), cx)
            });
            return;
        };
        if window.has_active_dialog(cx) {
            return;
        }
        let (page, ask, answer, record, ok_label) = (
            page.clone(),
            ask.clone(),
            answer.clone(),
            record.clone(),
            ok_label.clone(),
        );
        window.open_alert_dialog(cx, move |dialog, _, cx| {
            let dialog = crate::ui::SheetExt::sheet(dialog, cx);
            let (page, ask, answer, record) =
                (page.clone(), ask.clone(), answer.clone(), record.clone());
            dialog
                .title(question.clone())
                .when_some(body.clone(), |d, b| d.description(b))
                .show_cancel(true)
                .ok_text(ok_label.clone())
                .ok_variant(if danger {
                    ButtonVariant::Danger
                } else {
                    ButtonVariant::Primary
                })
                .cancel_text(common::CANCEL)
                .on_ok(move |_, _, cx| {
                    page.update(cx, |this, cx| {
                        this.answer(ask.clone(), answer.clone(), record.clone(), cx)
                    });
                    true
                })
        });
    })
    .into_any_element()
}

/// A footer button that goes somewhere instead of answering.
fn link_button(
    a: &AskCard,
    s: &Services,
    ix: usize,
    label: &str,
    target: Option<RefTarget>,
    cx: &mut Context<DeskPage>,
) -> AnyElement {
    let k = theme::colors(cx);
    let services = s.clone();
    let (g, t) = (a.item.group_id.clone(), a.item.topic_id.clone());
    ui::button(
        ElementId::Name(format!("desk-link-{}-{ix}", a.key()).into()),
        label.to_string(),
        ButtonKind::Ghost,
        ButtonSize::Md,
        cx,
    )
    .text_color(k.ink_2)
    .on_click(move |_, _, cx| match &target {
        Some(RefTarget::Pr { url, .. }) | Some(RefTarget::Url { url }) => {
            if url.starts_with("https://") || url.starts_with("http://") {
                cx.open_url(url);
            }
        }
        Some(RefTarget::Artifact {
            artifact_id,
            version,
        }) => {
            let (id, v) = (artifact_id.clone(), *version);
            services
                .nav
                .update(cx, |nav, cx| nav.open_artifact(id, v, cx));
        }
        Some(RefTarget::Topic { group_id, topic_id }) => {
            let (g, t) = (group_id.clone(), topic_id.clone());
            services.nav.update(cx, |nav, cx| nav.open_topic(g, t, cx));
        }
        _ => {
            let (g, t) = (g.clone(), t.clone());
            services.nav.update(cx, |nav, cx| nav.open_topic(g, t, cx));
        }
    })
    .into_any_element()
}

// --- v2 -----------------------------------------------------------------------

type Parts = (Vec<AnyElement>, Option<AnyElement>);

fn v2_parts(
    a: &AskCard,
    s: &Services,
    card: &CardV2,
    key: &str,
    done: Option<&Answered>,
    asker: &str,
    cx: &mut Context<DeskPage>,
) -> Parts {
    let k = theme::colors(cx);
    let mut body = Vec::new();
    let mut chips: Vec<AnyElement> = Vec::new();
    let actions = card.actions.as_deref().unwrap_or(&[]);
    let submit = actions
        .iter()
        .find(|x| x.kind() == CardActionKind::Submit)
        .map(|x| x.id.clone());
    let mut has_choice = false;
    let mut needs_form = false;

    // A PR ref and its diff make one chip: `#61 +214 −63`.
    let blocks = all_blocks(&card.blocks);
    let pr = blocks.iter().find_map(|b| match b {
        CardV2Block::Ref {
            label,
            target: RefTarget::Pr { number, .. },
            ..
        } => Some(
            number
                .map(|n| format!("#{n}"))
                .unwrap_or_else(|| label.clone()),
        ),
        _ => None,
    });
    let diff = blocks.iter().find_map(|b| match b {
        CardV2Block::Diff {
            additions,
            deletions,
            ..
        } => Some((additions.unwrap_or(0), deletions.unwrap_or(0))),
        _ => None,
    });
    if pr.is_some() || diff.is_some() {
        let mut parts: Vec<AnyElement> = vec![
            div()
                .mono()
                .child(pr.clone().unwrap_or_default())
                .into_any_element(),
        ];
        if let Some((add, del)) = diff {
            parts.push(
                div()
                    .mono()
                    .text_color(k.ok_ink)
                    .child(format!("+{add}"))
                    .into_any_element(),
            );
            parts.push(
                div()
                    .mono()
                    .text_color(k.bad_ink)
                    .child(format!("−{del}"))
                    .into_any_element(),
            );
        }
        chips.push(chip(
            format!("desk-chip-pr-{key}"),
            Some((Lucide::GitPullRequest, k.ink_3)),
            parts,
            cx,
        ));
    }

    let mut ix_images: Vec<String> = Vec::new();
    for b in &card.blocks {
        match b {
            CardV2Block::Text { text, .. } => body.push(paragraph(text, cx)),
            CardV2Block::Fields { fields: f, .. } => body.push(fields(f, cx)),
            CardV2Block::List { items, .. } => {
                for it in items {
                    body.push(paragraph(&format!("· {}", it.text), cx));
                }
            }
            CardV2Block::Evidence {
                id,
                label,
                state,
                detail,
                ..
            } => {
                let glyph = match state {
                    Some(EvidenceState::Pass) => Some((Lucide::Check, k.ok)),
                    Some(EvidenceState::Fail) => Some((Lucide::X, k.bad)),
                    _ if detail.as_deref().is_some_and(is_duration) => {
                        Some((Lucide::Play, k.ink_3))
                    }
                    _ => None,
                };
                let mut parts = vec![div().child(label.clone()).into_any_element()];
                if let Some(d) = detail {
                    parts.extend(mono_lead(d, cx));
                }
                chips.push(chip(format!("desk-chip-{key}-{id}"), glyph, parts, cx));
            }
            CardV2Block::Metric {
                label, value, unit, ..
            } => body.push(paragraph(
                &format!("{label} {value}{}", unit.clone().unwrap_or_default()),
                cx,
            )),
            CardV2Block::Draft { label, text, .. } => {
                body.push(draft_preview(a, s, &[], label.as_deref(), text, cx))
            }
            CardV2Block::Group { blocks, .. } => {
                let mut draft = None;
                for inner in blocks.iter().flatten() {
                    match inner {
                        CardV2Block::Image { file_id, .. } => ix_images.push(file_id.clone()),
                        CardV2Block::Draft { label, text, .. } => {
                            draft = Some((label.clone(), text.clone()))
                        }
                        CardV2Block::Text { text, .. } => body.push(paragraph(text, cx)),
                        CardV2Block::Fields { fields: f, .. } => body.push(fields(f, cx)),
                        _ => {}
                    }
                }
                if let Some((label, text)) = draft {
                    body.push(draft_preview(a, s, &ix_images, label.as_deref(), &text, cx));
                }
            }
            CardV2Block::Decision {
                id, options: opts, ..
            } => {
                has_choice = true;
                if let Some(done) = done {
                    body.push(record_inline(key, done, asker, cx));
                } else {
                    let list = opts
                        .iter()
                        .enumerate()
                        .map(|(ix, o)| {
                            let (letter, label) = option_parts(ix, &o.label);
                            let mut inputs = BTreeMap::new();
                            inputs.insert(id.clone(), serde_json::Value::String(o.id.clone()));
                            (
                                letter,
                                label,
                                Answer::V2 {
                                    rev: card.rev,
                                    action_id: submit.clone().unwrap_or_else(|| "submit".into()),
                                    inputs,
                                },
                            )
                        })
                        .collect();
                    body.push(options(a, list, cx));
                    for o in opts {
                        if let Some(d) = &o.detail {
                            body.push(note_line(d, cx));
                            break;
                        }
                    }
                }
            }
            CardV2Block::Input { required, .. } if required.unwrap_or(false) => needs_form = true,
            _ => {}
        }
    }
    if !chips.is_empty() {
        body.push(
            div()
                .mt(space::S12)
                .flex()
                .flex_wrap()
                .gap(space::S6)
                .children(chips)
                .into_any_element(),
        );
    }

    if has_choice {
        return (body, None);
    }
    if let Some(done) = done {
        return (body, Some(record_band(key, done, asker, cx)));
    }

    let irreversible = card.ask.as_ref().and_then(|x| x.irreversible.clone());
    let mut row: Vec<AnyElement> = Vec::new();
    if needs_form {
        row.push(open_topic_button(a, s, cx));
    } else {
        for (ix, act) in actions.iter().enumerate() {
            row.push(v2_action(
                a,
                s,
                card,
                ix,
                act,
                irreversible.as_ref().map(|i| i.consequence.clone()),
                cx,
            ));
        }
    }
    let confirmed = actions.iter().any(|x| x.confirm.is_some());
    let note = match (&irreversible, confirmed, a.files) {
        (Some(i), true, _) => Some(footer_note(
            fill(c::DESK_CONFIRM_FIRST, &[("verb", &i.verb)]),
            None,
            false,
            cx,
        )),
        (Some(i), false, _) => Some(footer_note(
            fill(c::DESK_NO_UNDO, &[("verb", &i.verb)]),
            Some(Lucide::Lock),
            false,
            cx,
        )),
        (None, _, Some(n)) if n > 0 => Some(footer_note(
            fill(c::DESK_FILES, &[("n", &n.to_string())]),
            None,
            true,
            cx,
        )),
        _ => None,
    };
    let footer = footer_band(cx)
        .children(row)
        .children(note)
        .into_any_element();
    (body, Some(footer))
}

fn v2_action(
    a: &AskCard,
    s: &Services,
    card: &CardV2,
    ix: usize,
    act: &CardV2Action,
    consequence: Option<String>,
    cx: &mut Context<DeskPage>,
) -> AnyElement {
    match act.kind() {
        CardActionKind::Link => link_button(a, s, ix, &act.label, act.target.clone(), cx),
        CardActionKind::Unknown => div().into_any_element(),
        kind => {
            let style = act.style.unwrap_or_default();
            let button = match (kind, style) {
                (CardActionKind::Cancel, _) => ButtonKind::Ghost,
                (_, CardV2ActionStyle::Primary) => ButtonKind::Primary,
                (_, CardV2ActionStyle::Danger) => ButtonKind::Danger,
                _ => ButtonKind::Secondary,
            };
            let confirm = act
                .confirm
                .as_ref()
                .map(|cf| (cf.title.clone(), cf.body.clone().or(consequence)));
            answer_button(
                a,
                ix,
                &act.label,
                button,
                Answer::V2 {
                    rev: card.rev,
                    action_id: act.id.clone(),
                    inputs: BTreeMap::new(),
                },
                confirm,
                cx,
            )
        }
    }
}

/// `0:42`, `12:05`.
fn is_duration(s: &str) -> bool {
    let mut parts = s.split(':');
    matches!(
        (parts.next(), parts.next(), parts.next()),
        (Some(m), Some(s), None)
            if !m.is_empty()
                && m.chars().all(|c| c.is_ascii_digit())
                && s.len() == 2
                && s.chars().all(|c| c.is_ascii_digit())
    )
}

// --- v1 -----------------------------------------------------------------------

fn v1_parts(
    a: &AskCard,
    s: &Services,
    card: &Card,
    key: &str,
    done: Option<&Answered>,
    asker: &str,
    cx: &mut Context<DeskPage>,
) -> Parts {
    let actions: Vec<CardAction> = card.actions.clone().unwrap_or_default();
    let choice = a.item.kind == AskKind::Choice && actions.len() >= 2;
    let mut body = Vec::new();
    let mut notes = Vec::new();
    for b in &card.blocks {
        match b {
            CardBlock::Text { text } if choice => notes.push(text.clone()),
            CardBlock::Text { text } => body.push(paragraph(text, cx)),
            CardBlock::Fields { fields: f } => body.push(fields(f, cx)),
            CardBlock::List { items } => {
                for it in items {
                    body.push(paragraph(&format!("· {}", it.text), cx));
                }
            }
            _ => {}
        }
    }
    if choice {
        if let Some(done) = done {
            body.push(record_inline(key, done, asker, cx));
        } else {
            let list = actions
                .iter()
                .enumerate()
                .map(|(ix, x)| {
                    let (letter, label) = option_parts(ix, &x.label);
                    (
                        letter,
                        label,
                        Answer::V1 {
                            action_id: x.id.clone(),
                        },
                    )
                })
                .collect();
            body.push(options(a, list, cx));
            for n in notes {
                body.push(note_line(&n, cx));
            }
        }
        return (body, None);
    }
    if let Some(done) = done {
        return (body, Some(record_band(key, done, asker, cx)));
    }
    if actions.is_empty() {
        let footer = footer_band(cx)
            .child(open_topic_button(a, s, cx))
            .into_any_element();
        return (body, Some(footer));
    }
    let row: Vec<AnyElement> = actions
        .iter()
        .enumerate()
        .map(|(ix, x)| {
            let kind = match x.style.unwrap_or_default() {
                ActionStyle::Primary => ButtonKind::Primary,
                ActionStyle::Danger => ButtonKind::Danger,
                _ => ButtonKind::Secondary,
            };
            answer_button(
                a,
                ix,
                &x.label,
                kind,
                Answer::V1 {
                    action_id: x.id.clone(),
                },
                x.confirm.clone().map(|q| (q, None)),
                cx,
            )
        })
        .collect();
    let footer = footer_band(cx).children(row).into_any_element();
    (body, Some(footer))
}

// --- a 话题 that waits on me -----------------------------------------------------

fn open_topic_button(a: &AskCard, s: &Services, cx: &mut Context<DeskPage>) -> AnyElement {
    let nav = s.nav.clone();
    let (g, t) = (a.item.group_id.clone(), a.item.topic_id.clone());
    ui::button(
        ElementId::Name(format!("desk-open-{}", a.key()).into()),
        c::DESK_OPEN_TOPIC,
        ButtonKind::Primary,
        ButtonSize::Md,
        cx,
    )
    .on_click(move |_, _, cx| {
        let (g, t) = (g.clone(), t.clone());
        nav.update(cx, |nav, cx| nav.open_topic(g, t, cx));
    })
    .into_any_element()
}

fn topic_parts(a: &AskCard, s: &Services, cx: &mut Context<DeskPage>) -> Parts {
    let line = match a.item.kind {
        AskKind::Review => c::DESK_REVIEW_WAITS,
        _ => c::DESK_TOPIC_WAITS,
    };
    let body = vec![paragraph(line, cx)];
    let footer = footer_band(cx)
        .child(open_topic_button(a, s, cx))
        .into_any_element();
    (body, Some(footer))
}
