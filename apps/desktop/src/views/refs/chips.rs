//! Reference chips: the composer's chip row (REFS §A4) and the read-only references above a
//! sent message's body (§A6, Refs.dc.html §01 and §03).

use crate::ui::native::FocusRow as _;
use std::{rc::Rc, sync::Arc};

use chat_core::proto::BlockRef;
use gpui::{
    AnyElement, App, ElementId, InteractiveElement as _, IntoElement, ParentElement as _,
    RenderOnce, StatefulInteractiveElement as _, Styled as _, Window, div,
    prelude::FluentBuilder as _, px,
};
use gpui_component::tooltip::Tooltip;

use super::{address_of, label};
use crate::{
    copy::refs as c,
    data::{ChatProvider, DraftRef, Message},
    theme::{self, Type as _, space},
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
        // No separator right before the "…" (`数 ·…`).
        let mut out = out.trim_end_matches([' ', '·']).to_string();
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
/// [`width`] counts at 12 pt and the chip draws at 14, so the room is scaled; the
/// lead then fits by itself and its "…" sits right before the part (no gap).
fn chip_text(r: &DraftRef) -> (String, String) {
    let (lead, part) = label::split(&r.input.label, r.lead_chars);
    fit(lead, part, (320.0 - 48.0) * 12.0 / 14.0)
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
        // §2.17 chip: h28, a 1 px border, radius 6; the × is a 28 ghost button at the
        // chip's right edge. A flash marks it with the selection fill.
        .h(crate::ui::native::tokens::height::SM)
        .pl(space::S8)
        .rounded(crate::ui::native::tokens::radius::ITEM)
        .border_1()
        .border_color(crate::ui::native::native_colors(cx).separator)
        .bg(crate::ui::native::native_colors(cx).content)
        .when(flashing, |d| d.bg(k.sel))
        .text_size(crate::ui::native::tokens::text::UI.0)
        .line_height(px(18.))
        .text_color(k.ink_3)
        .tooltip(move |window, cx| Tooltip::new(full.clone()).build(window, cx))
        // The chip body is a tab stop (Enter goes back to the part); × is its
        // own button (E3).
        .focus_row_with(false, crate::ui::native::Arrows::Grid, cx)
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
                    // The lead gives way to the part; its own ellipsis marks the cut, so
                    // no second "…" leaves a gap before the part.
                    // `fit` already cut the lead to its room and ended it with "…";
                    // the box keeps the text's own width, so the part follows it.
                    d.child(
                        div()
                            .min_w_0()
                            .flex_shrink(1.)
                            .overflow_hidden()
                            .whitespace_nowrap()
                            .text_ellipsis()
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
                ui::native::IconSize::Small,
            )
            .rounded(crate::ui::native::tokens::radius::ITEM)
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
        // A sent ref is a tab stop (Enter opens the part) with the row focus
        // ring (E3); its hover stays a text step.
        .when(!deleted, |d| {
            // `selected: true` leaves the hover to the text step here.
            d.focus_row(true, cx)
                .hover(|s| s.text_color(k.ink_2))
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

/// Chips shown before the rest folds into "+N": with chips at most 320 wide, 3 chips and
/// the "+N" chip stay within 2 rows of a composer (§2.17).
pub const SHOWN_CHIPS: usize = 3;

/// The chips of a composer with the rest folded into one "+N" chip (§2.17: at most 2
/// rows). `labels` are the chips' full labels, in order; the "+N" tooltip lists the
/// folded ones.
pub fn capped(chips: Vec<AnyElement>, labels: &[String]) -> Vec<AnyElement> {
    if chips.len() <= SHOWN_CHIPS + 1 {
        return chips;
    }
    let rest = chips.len() - SHOWN_CHIPS;
    let folded = labels
        .iter()
        .skip(SHOWN_CHIPS)
        .cloned()
        .collect::<Vec<_>>()
        .join("\n");
    let mut out: Vec<AnyElement> = chips.into_iter().take(SHOWN_CHIPS).collect();
    out.push(MoreChip { rest, folded }.into_any_element());
    out
}

/// The "+N" chip: the chip frame with the count of folded chips.
#[derive(IntoElement)]
struct MoreChip {
    rest: usize,
    folded: String,
}

impl RenderOnce for MoreChip {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let n = crate::ui::native::native_colors(cx);
        let folded = self.folded;
        div()
            .id("ref-chips-more")
            .flex_none()
            .h(crate::ui::native::tokens::height::SM)
            .px(space::S8)
            .rounded(crate::ui::native::tokens::radius::ITEM)
            .border_1()
            .border_color(n.separator)
            .bg(n.content)
            .flex()
            .items_center()
            .t_small()
            .tnum()
            .text_color(n.secondary)
            .child(crate::copy::fill(
                c::MORE_CHIPS,
                &[("n", &self.rest.to_string())],
            ))
            .when(!folded.is_empty(), |d| {
                d.tooltip(move |window, cx| Tooltip::new(folded.clone()).build(window, cx))
            })
    }
}

/// A chip row (the composer of a 话题 or of 单独问): gap 8, at most 2 rows (the rest
/// folds into "+N").
pub fn row(chips: Vec<AnyElement>) -> AnyElement {
    div()
        .flex()
        .flex_wrap()
        .gap(space::S8)
        .children(capped(chips, &[]))
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
