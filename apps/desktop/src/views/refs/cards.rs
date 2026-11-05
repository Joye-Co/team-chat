//! Parts of Card v2 blocks (REFS §A2.2): list items, plan steps, chart points and bars,
//! metrics, evidence items and images. Table cells have their own helper
//! ([`super::markdown::TableRefs`]), diagram nodes theirs (`diagrams`).
//!
//! The Card v2 renderers (2.0 and 2.1 alike) ask [`CardParts::of`] once per block and wrap
//! each part with [`BlockParts::wrap`]. That gives the part the shared affordances
//! ([`super::affordance::part`]: 问这里 after 300 ms, ⌥-click, the context menu). The
//! address, the label and the value come from the builders in [`super::parts`], so a chip
//! reads the same wherever the block renders.

use std::{rc::Rc, sync::Arc};

use gpui::{AnyElement, App, Div, Stateful, Window};

use super::{
    PartTarget, RefSource,
    affordance::{self, Anchor, PartSpec},
    keyboard::{NavKind, NavPart},
    markdown::TableRefs,
    parts::{self, Part, TablePick},
};
use crate::{
    data::cards_v2::{CardV2, CardV2Block, ChartKind, EvidenceState},
    views::cards::v2::{CardMode, Ctx},
};

/// One part of a block.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Pick {
    /// An item of a `list` block (0-based).
    Item(usize),
    /// A step of a `plan` block (0-based).
    Step(usize),
    /// A point (line, area) or a bar of a `chart` block.
    Point { series: usize, x: usize },
    /// A `metric` block: the metric itself.
    Metric,
    /// An `evidence` block: a screenshot (`image`) or a test, log or check (`item`).
    Evidence,
    /// An `image` block: the image itself.
    Image,
}

impl Pick {
    /// The part's suffix of the element id.
    fn key(self) -> String {
        match self {
            Pick::Item(i) => format!("i{i}"),
            Pick::Step(i) => format!("s{i}"),
            Pick::Point { series, x } => format!("p{series}-{x}"),
            Pick::Metric => "m".into(),
            Pick::Evidence => "e".into(),
            Pick::Image => "img".into(),
        }
    }
}

/// The wire word of a serde enum value (`in_progress`, `current`).
fn word<T: serde::Serialize>(v: &T) -> Option<String> {
    serde_json::to_value(v)
        .ok()
        .and_then(|v| v.as_str().map(str::to_string))
}

/// The part `pick` of block `b`, or `None` when the block has no such part.
pub fn part_of(b: &CardV2Block, pick: Pick) -> Option<Part> {
    match (b, pick) {
        (CardV2Block::List { items, .. }, Pick::Item(i)) => {
            let it = items.get(i)?;
            let status = it.status.as_ref().and_then(word);
            Some(parts::list_item(i, None, &it.text, status.as_deref()))
        }
        (CardV2Block::Plan { items, .. }, Pick::Step(i)) => {
            let it = items.as_deref()?.get(i)?;
            let state = word(&it.state).unwrap_or_default();
            Some(parts::step(i, &it.text, &state, None))
        }
        (
            CardV2Block::Chart {
                kind,
                unit,
                x,
                series,
                ..
            },
            Pick::Point { series: s, x: xi },
        ) => {
            let data: Vec<parts::ChartSeriesData> = series
                .iter()
                .map(|s| parts::ChartSeriesData {
                    key: s.key().to_string(),
                    name: s.name.clone(),
                    values: s.values.iter().map(|v| v.as_f64()).collect(),
                })
                .collect();
            let xs = x.clone().unwrap_or_default();
            parts::chart(
                &parts::ChartData {
                    bar: *kind == ChartKind::Bar,
                    x: &xs,
                    series: &data,
                    unit: unit.as_deref(),
                },
                parts::ChartPick::Point { series: s, x: xi },
            )
        }
        (
            CardV2Block::Metric {
                label,
                value,
                unit,
                delta,
                ..
            },
            Pick::Metric,
        ) => Some(parts::metric(
            label,
            value.as_f64().unwrap_or(0.),
            unit.as_deref(),
            delta.as_ref().and_then(|d| d.as_f64()),
            None,
        )),
        (
            CardV2Block::Evidence {
                label,
                state,
                file_id,
                ..
            },
            Pick::Evidence,
        ) => {
            let passed = match state {
                Some(EvidenceState::Pass) => Some(true),
                Some(EvidenceState::Fail) => Some(false),
                _ => None,
            };
            // One evidence block is one proof: its item is 0.
            Some(parts::evidence(
                0,
                file_id.is_some(),
                Some(label),
                file_id.as_deref(),
                passed,
            ))
        }
        (CardV2Block::Image { file_id, alt, .. }, Pick::Image) => {
            Some(parts::image(file_id, alt.as_deref()))
        }
        _ => None,
    }
}

/// Every part of block `b` in source order, each with its place in the block (row, column):
/// a list, a plan and the evidence strip are one column; a chart has a row per series.
pub fn picks(b: &CardV2Block) -> Vec<(Pick, usize, usize)> {
    match b {
        CardV2Block::List { items, .. } => {
            (0..items.len()).map(|i| (Pick::Item(i), i, 0)).collect()
        }
        CardV2Block::Plan { items, .. } => (0..items.as_ref().map_or(0, Vec::len))
            .map(|i| (Pick::Step(i), i, 0))
            .collect(),
        CardV2Block::Chart {
            kind, x, series, ..
        } if *kind != ChartKind::Spark => {
            let n = x.as_ref().map_or(0, Vec::len);
            series
                .iter()
                .enumerate()
                .flat_map(|(s, ser)| {
                    (0..n)
                        .filter(move |xi| ser.values.get(*xi).and_then(|v| v.as_f64()).is_some())
                        .map(move |xi| (Pick::Point { series: s, x: xi }, s, xi))
                })
                .collect()
        }
        CardV2Block::Metric { .. } => vec![(Pick::Metric, 0, 0)],
        CardV2Block::Evidence { .. } => vec![(Pick::Evidence, 0, 0)],
        CardV2Block::Image { .. } => vec![(Pick::Image, 0, 0)],
        _ => Vec::new(),
    }
}

/// The blocks whose parts this file draws (the whole-block affordance stays off for them,
/// so a part does not show two 问这里 buttons).
pub fn has_parts(b: &CardV2Block) -> bool {
    !picks(b).is_empty()
}

/// The reference context of one card: where it lives and what it is called.
#[derive(Clone)]
pub struct CardParts {
    source: RefSource,
    card: Arc<CardV2>,
    /// Element id prefix: the message id.
    prefix: String,
}

impl CardParts {
    /// The card of `ctx`, where the 引用提问 affordances show: a 话题 or the desk, a sent
    /// message, and a server that takes references.
    pub fn of(ctx: &Ctx, cx: &App) -> Option<Self> {
        if !matches!(ctx.mode, CardMode::Topic | CardMode::Desk)
            || ctx.msg.message_id.is_empty()
            || !super::enabled(cx)
        {
            return None;
        }
        Some(CardParts {
            source: RefSource::Message {
                group_id: ctx.msg.group_id.clone(),
                topic_id: ctx.msg.topic_id.clone(),
                message_id: ctx.msg.message_id.clone(),
            },
            card: ctx.card.clone(),
            prefix: ctx.msg.message_id.clone(),
        })
    }

    /// The parts of block `b` (a block without an id cannot be referenced).
    pub fn block(&self, b: &CardV2Block) -> Option<BlockParts> {
        b.id()?;
        Some(BlockParts(Rc::new((self.clone(), b.clone()))))
    }
}

/// One block of a card with its reference context.
#[derive(Clone)]
pub struct BlockParts(Rc<(CardParts, CardV2Block)>);

impl BlockParts {
    fn block_id(&self) -> &str {
        self.0.1.id().unwrap_or_default()
    }

    /// The element id of part `pick`, unique among the visible parts.
    pub fn id(&self, pick: Pick) -> String {
        format!("{}-p-{}-{}", self.0.0.prefix, self.block_id(), pick.key())
    }

    /// The block's element id prefix (keyboard focus).
    pub fn block_key(&self) -> String {
        format!("{}-k-{}", self.0.0.prefix, self.block_id())
    }

    /// The reference to part `pick`, or the whole block when `pick` is `None`.
    pub fn target(&self, pick: Option<Pick>) -> PartTarget {
        let (cp, b) = (&self.0.0, &self.0.1);
        let part = match pick {
            Some(p) => part_of(b, p),
            None => None,
        };
        let part = part.unwrap_or_else(|| Part {
            part: None,
            text: None,
            value: crate::views::cards::v21::parts::snapshot(b),
        });
        part.target(
            cp.source.clone(),
            &cp.card.title,
            self.block_id().to_string(),
            crate::views::cards::v21::parts::block_segment(&cp.card, b).as_deref(),
            Some(cp.card.rev as u64),
        )
    }

    /// The affordance spec of part `pick`.
    pub fn spec(&self, pick: Pick) -> PartSpec {
        let me = self.clone();
        PartSpec::new(self.id(pick), move |_| me.target(Some(pick)))
    }

    /// `el` (the part) with its affordances.
    pub fn wrap(
        &self,
        pick: Pick,
        el: Stateful<Div>,
        anchor: Anchor,
        window: &Window,
        cx: &App,
    ) -> AnyElement {
        affordance::part(el, self.spec(pick).anchor(anchor), window, cx)
    }

    /// `el` (the part) with its affordances; the renderer places 问这里 itself with
    /// [`BlockParts::button`] (after a step's text, in a chart's tooltip).
    pub fn wrap_own(&self, pick: Pick, el: Stateful<Div>, window: &Window, cx: &App) -> AnyElement {
        affordance::part(el, self.spec(pick).own_button(), window, cx)
    }

    /// The cell, row and column references of a table block (R4: `rowKeys`, else the row
    /// index); `None` for other blocks.
    pub fn table_refs(&self) -> Option<TableRefs> {
        // Only the plain table draws its cells as parts with every affordance (no
        // selection, no sort); a table I pick rows in uses [`Self::control_table_refs`].
        if !matches!(
            &self.0.1,
            CardV2Block::Table {
                select: None,
                sortable: None | Some(false),
                ..
            }
        ) {
            return None;
        }
        self.control_table_refs()
    }

    /// The cell, row and column references of any table block, also one with row
    /// selection or sort: its cells are controls, so they take references only by ⌥-click
    /// and the context menu (REFS §A3), never a hover 问这里.
    pub fn control_table_refs(&self) -> Option<TableRefs> {
        let CardV2Block::Table {
            columns,
            rows,
            row_keys,
            ..
        } = &self.0.1
        else {
            return None;
        };
        let cp = &self.0.0;
        let (columns, rows, keys) = (columns.clone(), rows.clone(), row_keys.clone());
        let (source, title, block_id, rev) = (
            cp.source.clone(),
            cp.card.title.clone(),
            self.block_id().to_string(),
            cp.card.rev as u64,
        );
        let block = crate::views::cards::v21::parts::block_segment(&cp.card, &self.0.1);
        Some(TableRefs {
            prefix: format!("{}-t-{block_id}", cp.prefix),
            target: Rc::new(move |pick| {
                let data = parts::TableData {
                    columns: &columns,
                    rows: &rows,
                    row_keys: keys.as_deref().filter(|k| k.len() == rows.len()),
                };
                parts::table(&data, pick).map(|p| {
                    p.target(
                        source.clone(),
                        &title,
                        block_id.clone(),
                        block.as_deref(),
                        Some(rev),
                    )
                })
            }),
        })
    }

    /// The block's parts for the keyboard (REFS §A3), in source order: table cells, rows
    /// and columns; else the parts of [`picks`]. `offset` moves them down by rows (a run
    /// of blocks drawn together), `col` puts a one-part block in a column (a metric row).
    pub fn nav(&self, offset: usize, col: Option<usize>) -> Vec<NavPart> {
        if let Some(t) = self.table_refs() {
            let CardV2Block::Table { columns, rows, .. } = &self.0.1 else {
                return Vec::new();
            };
            let pick = |p: TablePick| -> Rc<dyn Fn(&App) -> PartTarget> {
                let t = t.target.clone();
                Rc::new(move |_| t(p).unwrap_or_else(super::empty_target))
            };
            let mut out = Vec::new();
            for r in 0..rows.len() {
                for c in 0..columns.len() {
                    out.push(NavPart {
                        id: t.cell_id(r, c),
                        row: offset + r,
                        col: c,
                        kind: NavKind::Part,
                        target: pick(TablePick::Cell { row: r, col: c }),
                    });
                }
            }
            out.extend((0..rows.len()).map(|r| NavPart {
                id: t.row_id(r),
                row: offset + r,
                col: 0,
                kind: NavKind::Row,
                target: pick(TablePick::Row(r)),
            }));
            out.extend((0..columns.len()).map(|c| NavPart {
                id: t.header_id(c),
                row: offset,
                col: c,
                kind: NavKind::Column,
                target: pick(TablePick::Column(c)),
            }));
            return out;
        }
        picks(&self.0.1)
            .into_iter()
            .map(|(p, row, c)| {
                let me = self.clone();
                NavPart {
                    id: self.id(p),
                    row: offset + row,
                    col: col.unwrap_or(c),
                    kind: NavKind::Part,
                    target: Rc::new(move |_| me.target(Some(p))),
                }
            })
            .collect()
    }

    /// The whole block (⇧⌥↵).
    pub fn whole(&self) -> Rc<dyn Fn(&App) -> PartTarget> {
        let me = self.clone();
        Rc::new(move |_| me.target(None))
    }

    /// True while the 问这里 of `pick` shows (the pointer rested on it for 300 ms).
    pub fn shown(&self, pick: Pick, cx: &App) -> bool {
        affordance::hover_state(&self.id(pick), cx).1
    }

    /// The 问这里 button of `pick` while it shows, for renderers that place it: after the
    /// pointer's delay, or at once while the part has the keyboard focus (C-03, D2-29).
    pub fn button(&self, pick: Pick, window: &Window, cx: &App) -> Option<AnyElement> {
        if !(self.shown(pick, cx) || super::keyboard::part_focused(&self.id(pick), window, cx)) {
            return None;
        }
        let me = self.clone();
        Some(affordance::inline_button(
            &self.id(pick),
            Rc::new(move |_: &App| me.target(Some(pick))),
            cx,
        ))
    }
}

/// The point parts of a chart, as transparent columns over its plot (one per x, the main
/// series): hover shows a small tooltip with the value and 问这里, ⌥-click and the menu
/// reference the point. `top` is the plot's offset in the chart element.
pub fn chart_overlay(
    parts: &BlockParts,
    top: gpui::Pixels,
    height: gpui::Pixels,
    window: &Window,
    cx: &App,
) -> Option<AnyElement> {
    use gpui::{
        InteractiveElement as _, IntoElement as _, ParentElement as _, Styled as _, div,
        prelude::FluentBuilder as _,
    };
    let CardV2Block::Chart {
        x, series, kind, ..
    } = &parts.0.1
    else {
        return None;
    };
    let n = x.as_ref().map_or(0, Vec::len);
    if n == 0 || series.is_empty() || *kind == ChartKind::Spark {
        return None;
    }
    let k = crate::theme::colors(cx);
    let main = &series[0];
    let left = crate::views::cards::v2::chart_plot_left(*kind, series);
    Some(
        div()
            .absolute()
            .left(left)
            .right_0()
            .top(top)
            .h(height)
            .flex()
            .children((0..n).map(|xi| {
                let pick = Pick::Point { series: 0, x: xi };
                let col = div().id(parts.id(pick)).flex_1().h_full();
                if main.values.get(xi).and_then(|v| v.as_f64()).is_none() {
                    return col.into_any_element();
                }
                let tip = parts.button(pick, window, cx).map(|button| {
                    // `{x} · {value}`: the part text without the series name.
                    let text = part_of(&parts.0.1, pick)
                        .and_then(|p| p.text)
                        .map(|t| match t.split_once(super::label::SEP) {
                            Some((_, rest)) if series.len() == 1 => rest.to_string(),
                            _ => t,
                        })
                        .unwrap_or_default();
                    // Above the point, or under it when the plot has no room above:
                    // the tip never covers the point it describes.
                    let at = main
                        .values
                        .get(xi)
                        .and_then(|v| v.as_f64())
                        .map_or(0., |v| {
                            crate::views::cards::v2::chart_y_from_top(*kind, series, v)
                        });
                    let y = height * at;
                    let top = if y - TIP_H - TIP_GAP >= gpui::px(0.) {
                        y - TIP_H - TIP_GAP
                    } else {
                        y + TIP_GAP
                    };
                    div()
                        .absolute()
                        .top(top)
                        .left_0()
                        .right_0()
                        .flex()
                        // The first and the last tip stay inside the chart.
                        .map(|el| match xi {
                            0 => el.justify_start(),
                            i if i + 1 == n => el.justify_end(),
                            _ => el.justify_center(),
                        })
                        .child(
                            div()
                                .flex_none()
                                .flex()
                                .items_center()
                                .gap(crate::theme::space::S6)
                                .p(crate::theme::space::S4)
                                .pl(crate::theme::space::S8)
                                .rounded(crate::theme::radius::MD)
                                .border_1()
                                .border_color(k.line)
                                .bg(k.bg)
                                .text_size(gpui::px(12.))
                                .line_height(gpui::px(18.))
                                .text_color(k.ink_2)
                                .child(text)
                                .child(button),
                        )
                });
                // The crosshair at the point while it is under the pointer.
                let (hovered, shown) = affordance::hover_state(&parts.id(pick), cx);
                let cross = (shown || (hovered && window.modifiers().alt)).then(|| {
                    div()
                        .absolute()
                        .top_0()
                        .bottom_0()
                        .left_0()
                        .right_0()
                        .flex()
                        .justify_center()
                        .child(div().w(gpui::px(1.)).h_full().bg(k.line_2))
                });
                let col = col.relative().children(cross).children(tip);
                affordance::part(col, parts.spec(pick).own_button().no_fill(), window, cx)
            }))
            .into_any_element(),
    )
}

/// Height of the point tooltip (a 22 pt button, padding and the border), and its gap to
/// the point.
const TIP_H: gpui::Pixels = gpui::px(32.);
const TIP_GAP: gpui::Pixels = gpui::px(8.);

/// The plot's offset in a chart element: below the label row when the chart has a label
/// or a legend (`views::cards::v2::chart`: an 18 pt row and a 12 pt gap).
pub fn chart_plot_top(b: &CardV2Block) -> gpui::Pixels {
    match b {
        CardV2Block::Chart { label, series, .. } if label.is_some() || series.len() > 1 => {
            gpui::px(30.)
        }
        _ => gpui::px(12.),
    }
}

/// A block that takes the keyboard part focus: a referenceable leaf that is not a control
/// (inputs keep Tab for answering) and has no focus of its own (a diagram has its nodes).
pub fn keyboard_block(b: &CardV2Block) -> bool {
    !(b.is_container()
        || b.takes_input()
        || matches!(
            b,
            CardV2Block::Divider { .. }
                | CardV2Block::Buttons { .. }
                | CardV2Block::Input { .. }
                | CardV2Block::Unknown
                | CardV2Block::Embed { .. }
                | CardV2Block::Diagram { .. }
                | CardV2Block::Draft { .. }
                | CardV2Block::Decision { .. }
        ))
}

/// `el` (the blocks of `run`, drawn together) as one keyboard stop with the parts of every
/// block (REFS §A3). `el` is unchanged when no block of the run takes keyboard focus.
pub fn focus_run(run: &[BlockParts], el: AnyElement, window: &Window, cx: &App) -> AnyElement {
    let run: Vec<&BlockParts> = run.iter().filter(|b| keyboard_block(&b.0.1)).collect();
    let Some(first) = run.first() else {
        return el;
    };
    // Metrics share a row; other runs stack.
    let side_by_side = run
        .iter()
        .all(|b| matches!(b.0.1, CardV2Block::Metric { .. }));
    let mut nav = Vec::new();
    let mut offset = 0;
    for (i, b) in run.iter().enumerate() {
        let parts = b.nav(offset, side_by_side.then_some(i));
        if !side_by_side {
            offset = parts.iter().map(|p| p.row + 1).max().unwrap_or(offset);
        }
        nav.extend(parts);
    }
    super::keyboard::focus_block(el, first.block_key(), nav, first.whole(), window, cx)
}

/// `el` with the affordances of `pick` when `parts` is set, else `el` as it is.
pub fn wrap_opt(
    parts: Option<&BlockParts>,
    pick: Pick,
    el: Stateful<Div>,
    anchor: Anchor,
    window: Option<&Window>,
    cx: &App,
) -> AnyElement {
    use gpui::IntoElement as _;
    match (parts, window) {
        (Some(p), Some(w)) => p.wrap(pick, el, anchor, w, cx),
        _ => el.into_any_element(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn card() -> CardV2 {
        serde_json::from_value(json!({
            "v": 2, "rev": 3, "intent": "done", "title": "10 月的三个数", "fallback": "f",
            "blocks": [
                { "type": "metric", "id": "dau", "label": "日活", "value": 2340, "delta": 4 },
                { "type": "chart", "id": "trend", "kind": "bar", "label": "日活 · 近 4 周",
                  "x": ["10-06", "10-13"],
                  "series": [{ "id": "oct", "name": "10 月", "values": [1900, 2060] },
                             { "name": "9 月", "values": [1640] }] },
                { "type": "list", "id": "todo", "items": [
                    { "text": "补留存埋点", "status": "in_progress" }, { "text": "复盘" }] },
                { "type": "plan", "id": "plan", "items": [
                    { "text": "复现", "state": "done" }, { "text": "跑一万条消息的压测", "state": "current" }] },
                { "type": "evidence", "id": "tests", "label": "测试 148 项通过", "state": "pass" },
                { "type": "evidence", "id": "shot", "label": "重连后", "fileId": "f-1" },
                { "type": "image", "id": "pic", "fileId": "f-2", "alt": "新首页" },
                { "type": "text", "id": "why", "text": "说明" }
            ]
        }))
        .expect("sample card")
    }

    fn ctx_parts(c: &CardV2) -> CardParts {
        CardParts {
            source: RefSource::Message {
                group_id: "g".into(),
                topic_id: "t".into(),
                message_id: "m".into(),
            },
            card: Arc::new(c.clone()),
            prefix: "m".into(),
        }
    }

    fn target(c: &CardV2, block: &str, pick: Pick) -> PartTarget {
        ctx_parts(c)
            .block(c.block(block).unwrap())
            .unwrap()
            .target(Some(pick))
    }

    fn keys(t: &PartTarget) -> serde_json::Value {
        serde_json::to_value(t.part.as_ref().unwrap()).unwrap()
    }

    #[test]
    fn a_table_i_pick_rows_in_has_cell_refs_for_the_alt_click_and_the_menu_only() {
        let c: CardV2 = serde_json::from_value(json!({
            "v": 2, "catalog": "tc.card/2.1", "rev": 2, "intent": "info", "title": "B 站 · 按来源",
            "fallback": "f",
            "blocks": [
                { "id": "by", "type": "table", "columns": ["来源", "新增群"],
                  "rows": [["夹夹教程视频", "5"], ["UP 主合作", "3"]],
                  "rowKeys": ["tutorial", "up"], "select": "multiple" }
            ]
        }))
        .unwrap();
        let parts = ctx_parts(&c).block(c.block("by").unwrap()).unwrap();
        assert!(
            parts.table_refs().is_none(),
            "no plain-table parts: no hover 问这里 on a control"
        );
        let refs = parts.control_table_refs().expect("⌥-click and the menu");
        let cell = (refs.target)(TablePick::Cell { row: 1, col: 1 }).expect("a cell");
        assert_eq!(
            serde_json::to_value(cell.part.as_ref().unwrap()).unwrap(),
            json!({ "kind": "cell", "keys": { "row": "up", "col": 1 } })
        );
        assert!(cell.label.contains("UP 主合作"), "{}", cell.label);
    }

    #[test]
    fn list_items_and_plan_steps_are_parts() {
        let c = card();
        let item = target(&c, "todo", Pick::Item(0));
        assert_eq!(
            keys(&item),
            json!({ "kind": "item", "keys": { "item": 0 } })
        );
        assert_eq!(item.label, "10 月的三个数 · 补留存埋点");
        assert_eq!(item.value.as_ref().unwrap()["status"], "in_progress");
        let step = target(&c, "plan", Pick::Step(1));
        assert_eq!(
            keys(&step),
            json!({ "kind": "step", "keys": { "step": 1 } })
        );
        assert_eq!(step.label, "10 月的三个数 · 第 2 步 · 跑一万条消息的压测");
        assert_eq!(step.value.as_ref().unwrap()["state"], "current");
        assert_eq!(step.card_rev, Some(3));
    }

    #[test]
    fn bars_points_and_metrics_carry_their_values() {
        let c = card();
        let bar = target(&c, "trend", Pick::Point { series: 0, x: 1 });
        assert_eq!(
            keys(&bar),
            json!({ "kind": "bar", "keys": { "series": "oct", "x": "10-13" } })
        );
        assert_eq!(bar.block_id, "trend");
        assert!(
            bar.label.starts_with("10 月的三个数 · 日活 · 近 4 周 · "),
            "{}",
            bar.label
        );
        assert_eq!(bar.value.as_ref().unwrap()["value"], 2060.0);
        let metric = target(&c, "dau", Pick::Metric);
        assert_eq!(keys(&metric), json!({ "kind": "metric" }));
        assert_eq!(metric.value.as_ref().unwrap()["delta"], 4.0);
        assert!(metric.label.contains("日活"));
    }

    #[test]
    fn evidence_and_images_are_parts() {
        let c = card();
        let check = target(&c, "tests", Pick::Evidence);
        assert_eq!(
            keys(&check),
            json!({ "kind": "item", "keys": { "item": 0 } })
        );
        assert_eq!(check.value.as_ref().unwrap()["passed"], true);
        let shot = target(&c, "shot", Pick::Evidence);
        assert_eq!(keys(&shot)["kind"], "image");
        assert_eq!(shot.value.as_ref().unwrap()["fileId"], "f-1");
        let pic = target(&c, "pic", Pick::Image);
        assert_eq!(keys(&pic), json!({ "kind": "image" }));
        assert!(pic.label.ends_with("新首页"));
    }

    #[test]
    fn picks_list_every_part_in_source_order() {
        let c = card();
        let p = |id: &str| picks(c.block(id).unwrap());
        assert_eq!(p("todo").len(), 2);
        assert_eq!(p("plan")[1], (Pick::Step(1), 1, 0));
        // The 9 月 series has no value at 10-13: no part there.
        assert_eq!(
            p("trend"),
            vec![
                (Pick::Point { series: 0, x: 0 }, 0, 0),
                (Pick::Point { series: 0, x: 1 }, 0, 1),
                (Pick::Point { series: 1, x: 0 }, 1, 0),
            ]
        );
        assert!(p("why").is_empty());
        assert!(has_parts(c.block("dau").unwrap()));
        assert!(!has_parts(c.block("why").unwrap()));
    }

    #[test]
    fn a_missing_part_falls_back_to_the_whole_block() {
        let c = card();
        let t = target(&c, "todo", Pick::Item(9));
        assert!(t.part.is_none());
        assert_eq!(t.block_id, "todo");
    }
}
