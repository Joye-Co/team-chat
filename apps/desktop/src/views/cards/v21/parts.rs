//! 引用提问 on v2 blocks (REFS §A2.2, §A3): the affordances that hand one part (or a
//! whole block) to [`refs::ask_about`]. The 引用提问 track decides where the chip goes and
//! who is asked; this file says what the part is.
//!
//! The affordances themselves are the 引用提问 track's ([`refs::affordance::part`]: hover
//! 问这里 after 300 ms, ⌥-click, the context menu 就这里提问 / 单独问这里 / 复制), so
//! v1 cards, Markdown and v2 blocks look and behave the same. A plain table gets cell,
//! row and column parts ([`table_refs`]) instead of the whole block. Nothing shows when
//! the server does not take references (`welcome.features` without `refs/1`).
//!
//! [`refs::ask_about`]: crate::views::refs::ask_about

use gpui::{AnyElement, App, InteractiveElement as _, ParentElement, Styled, Window, div, px};
use serde_json::{Value, json};

use super::Node;
use crate::{
    copy::cards21 as c,
    data::cards_v2::{CardV2, CardV2Block, PartRef},
    views::{
        cards::v2::Ctx,
        refs::{self, PartTarget, RefSource, affordance::PartSpec, markdown::TableRefs},
    },
};

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
    refs::label::build(&card.title, block_segment(card, b).as_deref(), part).text
}

/// The `{block}` segment: the block's title, else its type name when the card has more
/// than one block of that type, else omitted.
pub(crate) fn block_segment(card: &CardV2, b: &CardV2Block) -> Option<String> {
    let same_type = card
        .blocks
        .iter()
        .filter(|x| x.type_name() == b.type_name())
        .count();
    match block_title(b) {
        Some(t) => Some(t),
        None if same_type > 1 && !default_name(b).is_empty() => Some(default_name(b).to_string()),
        None => None,
    }
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

/// True where the 引用提问 affordances show: a 话题 or the desk, and the server takes
/// references.
fn shown(node: &Node, cx: &App) -> bool {
    use crate::views::cards::v2::CardMode;
    matches!(node.ctx.mode, CardMode::Topic | CardMode::Desk) && refs::enabled(cx)
}

/// A table drawn with cell, row and column parts (the plain table: no selection or sort).
fn table_parts(b: &CardV2Block) -> bool {
    matches!(
        b,
        CardV2Block::Table {
            select: None,
            sortable: None | Some(false),
            ..
        }
    )
}

/// The cell, row and column references of a plain table block (R4: `rowKeys`, else the
/// row index), or `None` where the affordances do not show.
pub fn table_refs(node: &Node, b: &CardV2Block, cx: &App) -> Option<TableRefs> {
    if !shown(node, cx) || !table_parts(b) {
        return None;
    }
    refs::cards::CardParts::of(node.ctx, cx)?
        .block(b)?
        .table_refs()
}

/// The cell references of a table with row selection or sort (its cells are controls:
/// ⌥-click and the context menu only, REFS §A3), or `None` where the affordances do not show.
pub fn control_table_refs(node: &Node, b: &CardV2Block, cx: &App) -> Option<TableRefs> {
    if !shown(node, cx) || table_parts(b) {
        return None;
    }
    refs::cards::CardParts::of(node.ctx, cx)?
        .block(b)?
        .control_table_refs()
}

/// Adds the affordances to one block (a plain table has its own per cell).
pub fn wrap(
    node: &Node,
    b: &CardV2Block,
    el: AnyElement,
    window: &mut Window,
    cx: &mut App,
) -> AnyElement {
    if !referenceable(b) || !shown(node, cx) {
        return el;
    }
    let ctx = node.ctx;
    let Some(id) = b.id() else { return el };
    // Tables and the blocks with parts (items, steps, points, metrics, proofs, images)
    // reference their parts; a whole-block 问这里 there would show a second button.
    let el = if matches!(b, CardV2Block::Table { .. }) || refs::cards::has_parts(b) {
        el
    } else {
        let whole = target(ctx, b, None, None, snapshot(b));
        let spec = PartSpec::new(format!("{}-b-{id}", ctx.msg.message_id), move |_| {
            whole.clone()
        })
        .control(control(node, b))
        .radius(px(6.));
        refs::affordance::part(
            div()
                .id(ctx.id(format!("part-{id}")))
                .min_w(px(0.))
                .child(el),
            spec,
            window,
            cx,
        )
    };
    // The keyboard part focus (REFS §A3: Tab, arrows, ⌥↵, ⇧⌥↵, Esc).
    match refs::cards::CardParts::of(ctx, cx).and_then(|c| c.block(b)) {
        Some(bp) => refs::cards::focus_run(&[bp], el, window, cx),
        None => el,
    }
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
        assert!(label(&card, m, Some(&long)).chars().count() <= refs::label::LABEL_MAX);
    }

    #[test]
    fn diagram_blocks_take_their_title_else_the_type_name_when_there_are_several() {
        let card = |blocks: serde_json::Value| -> CardV2 {
            serde_json::from_value(json!({
                "v": 2, "catalog": "tc.card/2.1", "rev": 1, "intent": "done", "title": "周报",
                "fallback": "f", "blocks": blocks
            }))
            .unwrap()
        };
        let one = card(
            json!([{ "id": "d", "type": "diagram", "syntax": "mermaid", "source": "graph TD\nA-->B" }]),
        );
        assert_eq!(block_segment(&one, one.block("d").unwrap()), None);
        let titled = card(json!([
            { "id": "d", "type": "diagram", "syntax": "mermaid", "title": "上线流程", "source": "graph TD\nA-->B" }
        ]));
        assert_eq!(
            block_segment(&titled, titled.block("d").unwrap()).as_deref(),
            Some("上线流程")
        );
        let two = card(json!([
            { "id": "d", "type": "diagram", "syntax": "mermaid", "source": "graph TD\nA-->B" },
            { "id": "e", "type": "diagram", "syntax": "mermaid", "source": "graph TD\nC-->D" }
        ]));
        assert_eq!(
            block_segment(&two, two.block("e").unwrap()).as_deref(),
            Some(c::PART_DIAGRAM)
        );
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
