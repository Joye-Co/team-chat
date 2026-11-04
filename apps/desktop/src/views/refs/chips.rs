//! Reference chips: the composer's chip row (REFS §A4) and the read-only references above a
//! sent message's body (§A6, Refs.dc.html §01 and §03).

use std::{rc::Rc, sync::Arc};

use chat_core::proto::BlockRef;
use gpui::{
    AnyElement, App, ElementId, InteractiveElement as _, IntoElement, ParentElement as _,
    StatefulInteractiveElement as _, Styled as _, Window, div, prelude::FluentBuilder as _, px,
};
use gpui_component::{Sizable as _, tooltip::Tooltip};

use super::{address_of, label};
use crate::{
    copy::refs as c,
    data::{ChatProvider, DraftRef, Message},
    theme::{self, Type as _, radius, space},
    ui::{self, NamedButton as _, Sym},
    views::timeline::extras::{ExtrasContext, MessageExtra, Placement},
};

/// The chip glyph of a reference, from its part kind (or the block for a whole block).
pub fn glyph(r: &chat_core::proto::BlockRefInput) -> Sym {
    let kind = r.part.as_ref().map(|p| p.kind.as_str());
    let has = |k: &str| {
        r.part
            .as_ref()
            .and_then(|p| p.keys.as_ref())
            .is_some_and(|keys| keys.contains_key(k))
    };
    match kind {
        Some("cell" | "row" | "column") => Sym::Table,
        Some("point" | "series") => Sym::Chart,
        Some("bar") => Sym::BarChart,
        Some("metric") => Sym::Topic,
        Some("step") => Sym::Checklist,
        Some("hunk") => Sym::Diff,
        Some("line") if has("side") => Sym::Diff,
        Some("line") => Sym::Code,
        Some("image") => Sym::Image,
        Some("node") => Sym::Diagram,
        Some("element") => Sym::Document,
        Some("item") => Sym::List,
        _ if r.artifact.is_some() => Sym::Document,
        _ => Sym::Quote,
    }
}

/// Display width of text in points at 12 pt (CJK 12, others about 6.6).
fn width(text: &str) -> f32 {
    text.chars()
        .map(|c| if (c as u32) >= 0x2E80 { 12.0 } else { 6.6 })
        .sum()
}

/// Cuts `lead + part` in the middle to fit `max` points: the start of the lead and the end of
/// the part stay (`10 月数据月…渠道表 · B 站 · 新增群 9`).
pub fn fit(lead: &str, part: &str, max: f32) -> (String, String) {
    if width(lead) + width(part) <= max {
        return (lead.to_string(), part.to_string());
    }
    // Keep the part whole when it fits with a short lead; else cut the part's middle too.
    let part_w = width(part);
    if part_w + 40.0 <= max {
        let room = max - part_w - 8.0;
        let mut out = String::new();
        let mut w = 0.0;
        for c in lead.chars() {
            let cw = width(&c.to_string());
            if w + cw > room {
                break;
            }
            out.push(c);
            w += cw;
        }
        out.push('…');
        return (out, part.to_string());
    }
    let chars: Vec<char> = part.chars().collect();
    let half = (max / 2.0 - 6.0).max(12.0);
    let mut head = String::new();
    let mut w = 0.0;
    for c in &chars {
        let cw = width(&c.to_string());
        if w + cw > half {
            break;
        }
        head.push(*c);
        w += cw;
    }
    let mut tail: Vec<char> = Vec::new();
    w = 0.0;
    for c in chars.iter().rev() {
        let cw = width(&c.to_string());
        if w + cw > half {
            break;
        }
        tail.push(*c);
        w += cw;
    }
    tail.reverse();
    (
        String::new(),
        format!("{head}…{}", tail.into_iter().collect::<String>()),
    )
}

/// The chip's text: quiet lead, strong part, cut to 320 pt minus the glyph and ×.
fn chip_text(r: &DraftRef) -> (String, String) {
    let (lead, part) = label::split(&r.input.label, r.lead_chars);
    fit(lead, part, 320.0 - 48.0)
}

/// One composer chip: `--sunken`, 12 pt, glyph, label on one line, `已更新` when the source
/// changed, × to remove. A click on the body goes back to the part.
#[allow(clippy::too_many_arguments)]
pub fn composer_chip(
    id: impl Into<ElementId>,
    r: &DraftRef,
    updated: bool,
    flashing: bool,
    on_open: impl Fn(&mut Window, &mut App) + 'static,
    on_remove: impl Fn(&mut Window, &mut App) + 'static,
    cx: &App,
) -> AnyElement {
    let k = theme::colors(cx);
    let id: ElementId = id.into();
    let (lead, part) = chip_text(r);
    let full = r.input.label.clone();
    div()
        .id(id.clone())
        .flex()
        .items_center()
        .gap(space::S6)
        .max_w(px(320.))
        // Quiet inline text with a symbol (NATIVE.md rule 4): no chip box;
        // a flash marks it with the selection fill only.
        .h(px(22.))
        .px(space::S4)
        .rounded(radius::SM)
        .when(flashing, |d| d.bg(k.sel))
        .text_size(crate::ui::native::tokens::text::UI.0)
        .line_height(px(18.))
        .text_color(k.ink_3)
        .tooltip(move |window, cx| Tooltip::new(full.clone()).build(window, cx))
        .on_click(move |_, window, cx| on_open(window, cx))
        .child(
            div()
                .flex_none()
                .child(ui::icon(glyph(&r.input), px(16.), k.ink_3)),
        )
        .child(
            div()
                .min_w_0()
                .flex()
                .items_center()
                .gap(space::S4)
                .one_line()
                .when(!lead.trim().is_empty(), |d| {
                    d.child(
                        div()
                            .min_w_0()
                            .flex_shrink(1.)
                            .one_line()
                            .child(lead.trim_end().trim_end_matches('·').trim_end().to_string()),
                    )
                })
                // The part stays whole (`fit` already cut it); the lead gives way.
                .child(div().flex_none().one_line().text_color(k.ink).child(part)),
        )
        .when(updated, |d| {
            d.child(div().flex_none().text_color(k.ink_3).child(c::UPDATED))
        })
        .child(
            ui::native::icon_button_bare(
                ElementId::Name(format!("{id:?}-x").into()),
                Sym::Close,
                ui::native::IconSize::Regular,
            )
            .with_size(px(18.))
            .rounded(px(9.))
            .icon(ui::button_glyph(Sym::Close, px(14.), px(18.)))
            .named(c::REMOVE)
            .on_click(move |_, window, cx| {
                cx.stop_propagation();
                on_remove(window, cx)
            }),
        )
        .into_any_element()
}

/// A chip whose source changed since it was added: a newer Card v2 `rev`, or the source
/// message is gone.
pub fn updated(r: &DraftRef, provider: &Arc<dyn ChatProvider>) -> bool {
    let Some(mid) = &r.input.message_id else {
        return false;
    };
    if let Some(rev) = r.input.card_rev
        && let Some(card) = provider.card_v2(&r.group_id, mid)
    {
        return u64::from(card.card.rev) > u64::from(rev);
    }
    false
}

/// Goes back to the part of a chip: opens its 话题 at the message and flashes the part.
pub fn open_source(r: &DraftRef, cx: &mut App) {
    let Some(services) = super::services(cx) else {
        return;
    };
    let address = address_of(&r.input);
    let (g, t) = (r.group_id.clone(), r.topic_id.clone());
    match &r.input.message_id {
        Some(mid) => {
            let seq = services
                .provider
                .messages(&g, &t, 400)
                .messages
                .iter()
                .find(|m| &m.id == mid)
                .map_or(0, |m| m.created_seq);
            let mid = mid.clone();
            services
                .nav
                .update(cx, |nav, cx| nav.open_message(g, t, mid, seq, cx));
        }
        None => {
            if let Some(a) = &r.input.artifact {
                let (id, v) = (a.artifact_id.clone(), a.version);
                services.nav.update(cx, |nav, cx| {
                    nav.set_right_pane(
                        crate::app::RightPane::Artifact {
                            id,
                            version: Some(v),
                        },
                        cx,
                    )
                });
            }
        }
    }
    let h = super::hub(cx);
    h.update(cx, |h, cx| h.flash(address, cx));
}

// ---------------------------------------------------------------------------------------
// Sent messages
// ---------------------------------------------------------------------------------------

/// The references above a sent message's body: one line per reference with a quote bar
/// (Refs.dc.html §01); three or more fold into `引用了 n 处` with the lines under it.
pub struct RefsExtra;

fn sent_line(id: ElementId, r: &BlockRef, topic: (&str, &str), cx: &App) -> AnyElement {
    let k = theme::colors(cx);
    let deleted = r.deleted.is_some();
    let (lead, part) = if deleted {
        (String::new(), c::DELETED.to_string())
    } else {
        let lead = label::lead_of(&r.source.label);
        let (l, p) = label::split(&r.source.label, lead);
        fit(l, p, 480.0)
    };
    let draft = DraftRef {
        input: r.source.clone(),
        group_id: topic.0.to_string(),
        topic_id: if r.source_topic_id.is_empty() {
            topic.1.to_string()
        } else {
            r.source_topic_id.clone()
        },
        lead_chars: 0,
    };
    div()
        .id(id)
        .flex()
        .items_center()
        .gap(space::S6)
        .min_w_0()
        .h(px(22.))
        .t_meta()
        .text_color(k.ink_3)
        .when(!deleted, |d| {
            d.hover(|s| s.text_color(k.ink_2))
                .on_click(move |_, _, cx| open_source(&draft, cx))
        })
        .child(
            div()
                .flex_none()
                .child(ui::icon(glyph(&r.source), theme::icon::TAG, k.ink_3)),
        )
        .when(!lead.trim().is_empty(), |d| {
            d.child(
                div()
                    .min_w_0()
                    .flex_shrink(1.)
                    .one_line()
                    .child(lead.trim_end().to_string()),
            )
        })
        .child(
            div()
                .flex_none()
                .one_line()
                .when(!deleted, |d| d.text_color(k.ink_2))
                .child(part),
        )
        .into_any_element()
}

/// The reference lines of a 单独问 question (its quote lines), read-only.
pub fn side_lines(labels: &[String], cx: &App) -> AnyElement {
    let k = theme::colors(cx);
    div()
        .flex()
        .flex_col()
        .pl(px(8.))
        .border_l_2()
        .border_color(k.line_2)
        .children(labels.iter().map(|l| {
            let lead = label::lead_of(l);
            let (a, b) = label::split(l, lead);
            let (a, b) = fit(a, b, 240.0);
            div()
                .flex()
                .items_center()
                .gap(space::S4)
                .min_w_0()
                .t_meta()
                .text_color(k.ink_3)
                .child(ui::icon(Sym::Quote, theme::icon::TAG, k.ink_3))
                .when(!a.trim().is_empty(), |d| {
                    d.child(div().flex_none().one_line().child(a.trim_end().to_string()))
                })
                .child(div().min_w_0().one_line().text_color(k.ink_2).child(b))
        }))
        .into_any_element()
}

/// The reference block of a sent message.
pub fn sent_refs(message: &Message, refs: &[BlockRef], cx: &App) -> AnyElement {
    let k = theme::colors(cx);
    let key = if message.id.is_empty() {
        message.client_msg_id.clone()
    } else {
        message.id.clone()
    };
    let topic = (message.group_id.as_str(), message.topic_id.as_str());
    let lines: Vec<AnyElement> = refs
        .iter()
        .enumerate()
        .map(|(i, r)| {
            sent_line(
                ElementId::Name(format!("sref-{key}-{i}").into()),
                r,
                topic,
                cx,
            )
        })
        .collect();
    let many = refs.len() >= 3;
    div()
        .mb(px(4.))
        .pl(px(10.))
        .border_l_2()
        .border_color(k.quote_bar)
        .flex()
        .flex_col()
        .min_w(px(0.))
        .when(many, |d| {
            d.child(
                div()
                    .flex()
                    .items_center()
                    .gap(space::S6)
                    .h(px(22.))
                    .t_meta()
                    .text_color(k.ink_3)
                    .child(ui::icon(Sym::Quote, theme::icon::TAG, k.ink_3))
                    .child(super::cited(refs.len()))
                    .child(ui::icon(Sym::ChevronDown, theme::icon::TAG, k.ink_3)),
            )
        })
        .children(lines)
        .into_any_element()
}

impl MessageExtra for RefsExtra {
    fn placement(&self) -> Placement {
        Placement::AboveBody
    }

    fn render(
        &self,
        message: &Message,
        ctx: &ExtrasContext,
        _window: &mut Window,
        cx: &mut App,
    ) -> Option<AnyElement> {
        let refs = ctx.provider.message_block_refs(message);
        if refs.is_empty() {
            return None;
        }
        Some(sent_refs(message, &refs, cx))
    }
}

/// The body a row shows: without the exact prefix of its references.
pub fn shown_body<'a>(message: &'a Message, provider: &Arc<dyn ChatProvider>) -> &'a str {
    let refs = provider.message_block_refs(message);
    super::strip_prefix(&message.body, &refs).unwrap_or(&message.body)
}

/// A chip row (the composer of a 话题 or of 单独问).
pub fn row(chips: Vec<AnyElement>) -> AnyElement {
    div()
        .flex()
        .flex_wrap()
        .gap_x(space::S12)
        .gap_y(space::S2)
        .children(chips)
        .into_any_element()
}

/// Shared by the composers: whether `r` flashes now.
pub fn flashing(r: &DraftRef, cx: &App) -> bool {
    !cx.reduce_motion()
        && cx
            .try_global::<super::HubGlobal>()
            .is_some_and(|h| h.0.read(cx).flashing(&address_of(&r.input)))
}

/// Keeps `Rc` imported for callers that build chip callbacks.
pub type Callback = Rc<dyn Fn(&mut Window, &mut App)>;
