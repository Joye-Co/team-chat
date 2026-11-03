//! 引用提问 on v2 blocks (REFS §A2.2, §A3): the affordances that hand one part (or a
//! whole block) to [`refs::ask_about`]. The 引用提问 track decides where the chip goes and
//! who is asked; this file says what the part is.
//!
//! - Hover 问这里: after the pointer rests 300 ms on a block, a ghost `问这里` at its
//!   top-right edge (not on the controls of a pending ask).
//! - ⌥-click: adds the block (or the clicked part: a table row, a chart point, a metric)
//!   at once, controls included.
//! - Context menu: 就这里提问, 单独问这里, 复制.
//!
//! [`refs::ask_about`]: crate::views::refs::ask_about

use std::time::Instant;

use gpui::{
    AnyElement, App, InteractiveElement as _, IntoElement, MouseButton, MouseDownEvent,
    ParentElement, StatefulInteractiveElement as _, Styled, Window, div,
    prelude::FluentBuilder as _, px,
};
use gpui_component::menu::{ContextMenuExt as _, PopupMenuItem};
use serde_json::{Value, json};

use super::Node;
use crate::{
    copy::cards21 as c,
    data::cards_v2::{CardV2, CardV2Block, PartRef},
    theme::{self, card_v21 as t},
    ui::{self, ButtonKind, ButtonSize, Lucide},
    views::{
        cards::v2::Ctx,
        refs::{self, PartTarget, RefIntent, RefSource},
    },
};

/// Most code points of each label segment and of the whole label (REFS §A2.2).
const SOURCE_MAX: usize = 24;
const BLOCK_MAX: usize = 16;
const PART_MAX: usize = 60;
const LABEL_MAX: usize = 120;
/// Most bytes of a value snapshot.
const VALUE_MAX: usize = 2048;

fn cut(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let mut out: String = s.chars().take(max.saturating_sub(1)).collect();
        out.push('…');
        out
    }
}

/// The catalog default name of a block type (表格, 图表, 指标, ...).
fn default_name(b: &CardV2Block) -> &'static str {
    match b {
        CardV2Block::Table { .. } => c::PART_TABLE,
        CardV2Block::Chart { .. } => c::PART_CHART,
        CardV2Block::Metric { .. } => c::PART_METRIC,
        CardV2Block::List { .. } | CardV2Block::Fields { .. } => c::PART_LIST,
        CardV2Block::Plan { .. } => c::PART_PLAN,
        CardV2Block::Diff { .. } => c::PART_DIFF,
        CardV2Block::Evidence { .. } => c::PART_EVIDENCE,
        CardV2Block::Diagram { .. } => c::PART_DIAGRAM,
        CardV2Block::Code { .. } => c::PART_CODE,
        CardV2Block::Image { .. } => c::PART_IMAGE,
        _ => "",
    }
}

fn block_title(b: &CardV2Block) -> Option<String> {
    match b {
        CardV2Block::Table { title, .. } | CardV2Block::Diagram { title, .. } => title.clone(),
        CardV2Block::Chart { label, .. } => label.clone(),
        CardV2Block::Decision { label, .. } => label.clone(),
        _ => None,
    }
}

/// `{source} · {block} · {part}`: the card title, the block's title or its type name
/// (omitted when the card has one block of that type and no title), and the part.
pub fn label(card: &CardV2, b: &CardV2Block, part: Option<&str>) -> String {
    let mut segs = vec![cut(&card.title, SOURCE_MAX)];
    let same_type = card
        .blocks
        .iter()
        .filter(|x| x.type_name() == b.type_name())
        .count();
    match block_title(b) {
        Some(t) => segs.push(cut(&t, BLOCK_MAX)),
        None if same_type > 1 && !default_name(b).is_empty() => {
            segs.push(default_name(b).to_string())
        }
        None => {}
    }
    if let Some(p) = part.filter(|p| !p.is_empty()) {
        segs.push(cut(p, PART_MAX));
    }
    cut(&segs.join(" · "), LABEL_MAX)
}

/// A snapshot of the whole block, at most 2048 bytes as JSON.
pub fn snapshot(b: &CardV2Block) -> Option<Value> {
    let v = match b {
        CardV2Block::Text { text, .. } => json!({ "text": cut(text, 500) }),
        CardV2Block::Metric {
            label,
            value,
            unit,
            delta,
            ..
        } => json!({ "label": label, "value": value, "unit": unit, "delta": delta }),
        CardV2Block::Chart {
            label,
            x,
            series,
            unit,
            ..
        } => json!({
            "title": label, "x": x, "unit": unit,
            "series": series.iter().map(|s| json!({ "series": s.key(), "values": s.values })).collect::<Vec<_>>()
        }),
        CardV2Block::Table { columns, rows, .. } => {
            // As many rows as fit.
            return [20, 10, 5, 2, 0].into_iter().find_map(|n| {
                fit(
                    json!({ "columns": columns, "rows": rows.iter().take(n).collect::<Vec<_>>(),
                            "count": rows.len() }),
                )
            });
        }
        CardV2Block::Link { label, target, .. } => json!({ "title": label, "target": target }),
        CardV2Block::Ref { label, target, .. } => json!({ "title": label, "target": target }),
        CardV2Block::Diagram { source, .. } => json!({ "source": cut(source, 500) }),
        CardV2Block::Code { text, .. } => json!({ "text": cut(text, 500) }),
        _ => return None,
    };
    fit(v)
}

/// The value when it fits 2048 bytes; else nothing (the label still names the part).
pub fn fit(v: Value) -> Option<Value> {
    (serde_json::to_string(&v)
        .map(|s| s.len())
        .unwrap_or(usize::MAX)
        <= VALUE_MAX)
        .then_some(v)
}

/// The part target of block `b` (whole block when `part` is `None`).
pub fn target(
    ctx: &Ctx,
    b: &CardV2Block,
    part: Option<PartRef>,
    part_text: Option<&str>,
    value: Option<Value>,
) -> PartTarget {
    PartTarget {
        source: RefSource::Message {
            group_id: ctx.msg.group_id.clone(),
            topic_id: ctx.msg.topic_id.clone(),
            message_id: ctx.msg.message_id.clone(),
        },
        block_id: b.id().unwrap_or("card").to_string(),
        part,
        label: label(ctx.card, b, part_text),
        value: value.and_then(fit),
        card_rev: Some(ctx.card.rev as u64),
    }
}

/// True when a block can be referenced as a whole (REFS §A2.2: not containers, dividers,
/// buttons, inputs or blocks this version does not know).
fn referenceable(b: &CardV2Block) -> bool {
    !(b.is_container()
        || matches!(
            b,
            CardV2Block::Divider { .. }
                | CardV2Block::Buttons { .. }
                | CardV2Block::Input { .. }
                | CardV2Block::Unknown
                | CardV2Block::Embed { .. }
        ))
}

/// True when the block is a control of a pending ask (no hover 问这里 there).
fn control(node: &Node, b: &CardV2Block) -> bool {
    b.takes_input() && node.ctx.live()
}

/// Adds the affordances to one block.
pub fn wrap(
    node: &Node,
    b: &CardV2Block,
    el: AnyElement,
    _window: &mut Window,
    cx: &mut App,
) -> AnyElement {
    if !referenceable(b)
        || node.ctx.mode != crate::views::cards::v2::CardMode::Topic
            && node.ctx.mode != crate::views::cards::v2::CardMode::Desk
    {
        return el;
    }
    let ctx = node.ctx;
    let Some(id) = b.id() else { return el };
    let k = theme::colors(cx);
    let whole = target(ctx, b, None, None, snapshot(b));
    let rested = ctx
        .ui
        .read(cx)
        .hover
        .as_ref()
        .filter(|(h, _)| h == id)
        .is_some_and(|(_, at)| at.elapsed() >= t::HOVER_ASK);
    let show_ask = rested && !control(node, b);
    let (ui, block) = (ctx.ui.clone(), id.to_string());
    let (alt_target, menu_target, side_target, ask_target) =
        (whole.clone(), whole.clone(), whole.clone(), whole.clone());
    let copy_text = whole.label.clone();
    div()
        .id(ctx.id(format!("part-{id}")))
        .relative()
        .min_w(px(0.))
        .on_mouse_down(MouseButton::Left, move |e: &MouseDownEvent, window, cx| {
            if e.modifiers.alt {
                refs::ask_about(alt_target.clone(), RefIntent::Topic, window, cx);
                cx.stop_propagation();
            }
        })
        .on_hover(move |on, _, cx| {
            let block = block.clone();
            let on = *on;
            ui.update(cx, |u, cx| {
                if on {
                    u.hover = Some((block.clone(), Instant::now()));
                } else if u.hover.as_ref().is_some_and(|(h, _)| *h == block) {
                    u.hover = None;
                }
                cx.notify();
            });
            if on {
                let ui = ui.clone();
                cx.spawn(async move |cx: &mut gpui::AsyncApp| {
                    cx.background_executor().timer(t::HOVER_ASK).await;
                    cx.update(|cx| ui.update(cx, |_, cx| cx.notify()));
                })
                .detach();
            }
        })
        .context_menu(move |menu, _, _| {
            let (a, b2, text) = (menu_target.clone(), side_target.clone(), copy_text.clone());
            menu.item(
                PopupMenuItem::new(c::PART_ASK_TOPIC).on_click(move |_, window, cx| {
                    refs::ask_about(a.clone(), RefIntent::Topic, window, cx)
                }),
            )
            .item(
                PopupMenuItem::new(c::PART_ASK_SIDE).on_click(move |_, window, cx| {
                    refs::ask_about(b2.clone(), RefIntent::SideChat, window, cx)
                }),
            )
            .item(PopupMenuItem::new(c::PART_COPY).on_click(move |_, _, cx| {
                cx.write_to_clipboard(gpui::ClipboardItem::new_string(text.clone()))
            }))
        })
        .child(el)
        .when(show_ask, |el| {
            el.child(
                div().absolute().top(px(-4.)).right(px(-4.)).child(
                    ui::button(
                        ctx.id(format!("ask-{id}")),
                        c::PART_ASK_HERE,
                        ButtonKind::Ghost,
                        ButtonSize::Sm,
                        cx,
                    )
                    .icon(ui::icon(Lucide::CornerDownRight, px(12.), k.ink_2))
                    .bg(k.bg)
                    .shadow(k.sec_hi())
                    .on_click(move |_, window, cx| {
                        refs::ask_about(ask_target.clone(), RefIntent::Topic, window, cx)
                    }),
                ),
            )
        })
        .into_any_element()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn labels_follow_the_refs_pattern() {
        let card: CardV2 = serde_json::from_value(json!({
            "v": 2, "catalog": "tc.card/2.1", "rev": 1, "intent": "done", "title": "10 月数据月报",
            "fallback": "f",
            "blocks": [
                { "id": "t", "type": "table", "title": "渠道表", "columns": ["渠道", "新增群"], "rows": [["B 站", "9"]] },
                { "id": "m", "type": "metric", "label": "日活", "value": 2340 }
            ]
        }))
        .unwrap();
        let t = card.block("t").unwrap();
        assert_eq!(
            label(&card, t, Some("B 站 · 新增群 9")),
            "10 月数据月报 · 渠道表 · B 站 · 新增群 9"
        );
        let m = card.block("m").unwrap();
        assert_eq!(
            label(&card, m, Some("日活 2,340")),
            "10 月数据月报 · 日活 2,340"
        );
        let long = "字".repeat(200);
        assert!(label(&card, m, Some(&long)).chars().count() <= LABEL_MAX);
    }

    #[test]
    fn snapshots_stay_under_2048_bytes() {
        let big: Vec<Vec<String>> = (0..200)
            .map(|i| vec![format!("行 {i}"), "x".repeat(200)])
            .collect();
        let b = CardV2Block::Table {
            id: "t".into(),
            base: Default::default(),
            columns: vec!["a".into(), "b".into()],
            rows: big,
            title: None,
            row_keys: None,
            select: None,
            sortable: None,
        };
        let v = snapshot(&b).expect("fewer rows fit");
        assert!(serde_json::to_string(&v).unwrap().len() <= VALUE_MAX);
        assert_eq!(v["count"], 200);
    }
}
