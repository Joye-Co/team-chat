//! Part addresses, part labels and value snapshots per block type (REFS §A2.2, R3, R4).
//!
//! Each builder returns a [`Part`]: the `PartRef`, the part's label text and the value the
//! person sees. [`Part::target`] adds the source and block and builds the chip label.
//! Renderers call these; they never write a label or a `PartRef` by hand.

use chat_core::proto::card2::{PartKey, PartRef};
use serde_json::{Map, Value, json};

use super::{PartTarget, RefSource, label, value};
use crate::copy::{fill, refs as c};

/// One part of a block (or the whole block when `part` is `None`).
#[derive(Clone, Debug, PartialEq)]
pub struct Part {
    pub part: Option<PartRef>,
    /// The `{part}` segment of the label; `None` for a whole block.
    pub text: Option<String>,
    pub value: Option<Value>,
}

impl Part {
    /// The reference: `source` names where it lives, `source_label` is the `{source}` segment
    /// (card title, author's name or `{title} v{n}`), `block_name` the `{block}` segment
    /// (see [`block_name`]).
    pub fn target(
        self,
        source: RefSource,
        source_label: &str,
        block_id: impl Into<String>,
        block_name: Option<&str>,
        card_rev: Option<u64>,
    ) -> PartTarget {
        let l = label::build(source_label, block_name, self.text.as_deref());
        PartTarget {
            source,
            block_id: block_id.into(),
            part: self.part,
            label: l.text,
            value: self.value.map(value::cap),
            card_rev,
        }
    }
}

fn part(kind: &str, keys: &[(&str, PartKey)]) -> PartRef {
    PartRef {
        kind: kind.into(),
        keys: (!keys.is_empty()).then(|| {
            keys.iter()
                .map(|(k, v)| (k.to_string(), v.clone()))
                .collect()
        }),
    }
}

fn key(s: &str) -> PartKey {
    PartKey::Key(label::cut(s, 256))
}

fn idx(i: usize) -> PartKey {
    PartKey::Index(i as u64)
}

// ---------------------------------------------------------------------------------------
// Block names
// ---------------------------------------------------------------------------------------

/// The block types with a default name.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum BlockKind {
    Table,
    Chart,
    Metric,
    List,
    Plan,
    Diff,
    Evidence,
    Diagram,
    Code,
    Image,
    /// Text, link, fields and other whole blocks: no default name.
    Other,
}

impl BlockKind {
    pub fn default_name(self) -> Option<&'static str> {
        Some(match self {
            BlockKind::Table => c::BLOCK_TABLE,
            BlockKind::Chart => c::BLOCK_CHART,
            BlockKind::Metric => c::BLOCK_METRIC,
            BlockKind::List => c::BLOCK_LIST,
            BlockKind::Plan => c::BLOCK_PLAN,
            BlockKind::Diff => c::BLOCK_DIFF,
            BlockKind::Evidence => c::BLOCK_EVIDENCE,
            BlockKind::Diagram => c::BLOCK_DIAGRAM,
            BlockKind::Code => c::BLOCK_CODE,
            BlockKind::Image => c::BLOCK_IMAGE,
            BlockKind::Other => return None,
        })
    }
}

/// The `{block}` segment: the block's title, else the default name of its type when the card
/// has more than one block of that type (`same_kind` counts this block too), else omitted.
pub fn block_name(title: Option<&str>, kind: BlockKind, same_kind: usize) -> Option<String> {
    if let Some(t) = title.map(str::trim).filter(|t| !t.is_empty()) {
        return Some(t.to_string());
    }
    (same_kind > 1)
        .then(|| kind.default_name().map(str::to_string))
        .flatten()
}

/// The `{source}` segment of a 产出 version.
pub fn artifact_source(title: &str, version: u32) -> String {
    fill(
        c::SOURCE_VERSION,
        &[("title", title), ("n", &version.to_string())],
    )
}

/// The whole block: no part, a short text snapshot.
pub fn whole(text: &str) -> Part {
    Part {
        part: None,
        text: None,
        value: (!text.trim().is_empty()).then(|| json!({ "text": label::cut(text, 500) })),
    }
}

// ---------------------------------------------------------------------------------------
// Tables: v1, v2, Markdown tables and CSV 产出
// ---------------------------------------------------------------------------------------

/// A table as the person sees it.
#[derive(Clone, Copy, Debug)]
pub struct TableData<'a> {
    pub columns: &'a [String],
    pub rows: &'a [Vec<String>],
    /// v2 `rowKeys` (R4): one stable key per row; else rows are addressed by index.
    pub row_keys: Option<&'a [String]>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TablePick {
    Cell { row: usize, col: usize },
    Row(usize),
    Column(usize),
}

impl TableData<'_> {
    /// The row's name: its first column's text (R4 chip rule).
    pub fn row_name(&self, row: usize) -> String {
        self.rows
            .get(row)
            .and_then(|r| r.first())
            .map(|s| label::cut(s, 40))
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| (row + 1).to_string())
    }

    fn row_key(&self, row: usize) -> PartKey {
        match self.row_keys.and_then(|k| k.get(row)) {
            Some(k) => key(k),
            None => idx(row),
        }
    }

    fn col_label(&self, col: usize) -> String {
        self.columns
            .get(col)
            .map(|s| label::cut(s, 40))
            .unwrap_or_else(|| (col + 1).to_string())
    }
}

pub fn table(t: &TableData, pick: TablePick) -> Option<Part> {
    match pick {
        TablePick::Cell { row, col } => {
            let display = t.rows.get(row)?.get(col)?.trim().to_string();
            let (row_name, col_label) = (t.row_name(row), t.col_label(col));
            let mut v = Map::new();
            v.insert("row".into(), Value::from(row_name.clone()));
            v.insert("column".into(), Value::from(col_label.clone()));
            v.insert("display".into(), Value::from(display.clone()));
            if let Some(raw) = value::parse_number(&display) {
                v.insert("raw".into(), raw);
            }
            Some(Part {
                part: Some(part("cell", &[("row", t.row_key(row)), ("col", idx(col))])),
                text: Some(fill(
                    c::PART_CELL,
                    &[("row", &row_name), ("col", &col_label), ("value", &display)],
                )),
                value: Some(Value::Object(v)),
            })
        }
        TablePick::Row(row) => {
            let cells = t.rows.get(row)?;
            let row_name = t.row_name(row);
            let map: Map<String, Value> = cells
                .iter()
                .enumerate()
                .map(|(i, s)| (t.col_label(i), Value::from(label::cut(s, 200))))
                .collect();
            Some(Part {
                part: Some(part("row", &[("row", t.row_key(row))])),
                text: Some(fill(c::PART_ROW, &[("row", &row_name)])),
                value: Some(json!({ "rowKey": row_name, "cells": map })),
            })
        }
        TablePick::Column(col) => {
            if col >= t.columns.len() {
                return None;
            }
            let col_label = t.col_label(col);
            let values: Vec<Value> = t
                .rows
                .iter()
                .take(50)
                .map(|r| Value::from(label::cut(r.get(col).map_or("", String::as_str), 200)))
                .collect();
            Some(Part {
                part: Some(part("column", &[("col", idx(col))])),
                text: Some(fill(c::PART_COLUMN, &[("col", &col_label)])),
                value: Some(
                    json!({ "column": col_label, "values": values, "count": t.rows.len() }),
                ),
            })
        }
    }
}

// ---------------------------------------------------------------------------------------
// Charts
// ---------------------------------------------------------------------------------------

#[derive(Clone, Debug)]
pub struct ChartSeriesData {
    /// `id`, else the name (R3 `series` key).
    pub key: String,
    pub name: String,
    pub values: Vec<Option<f64>>,
}

#[derive(Clone, Copy, Debug)]
pub struct ChartData<'a> {
    /// Bar charts address `bar`, others `point`.
    pub bar: bool,
    pub x: &'a [String],
    pub series: &'a [ChartSeriesData],
    pub unit: Option<&'a str>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChartPick {
    Point { series: usize, x: usize },
    Series(usize),
}

fn with_unit(n: f64, unit: Option<&str>) -> String {
    let n = value::display_number(n);
    match unit.map(str::trim).filter(|u| !u.is_empty()) {
        Some(u) if u == "%" => format!("{n}%"),
        Some(u) => format!("{n} {u}"),
        None => n,
    }
}

pub fn chart(ch: &ChartData, pick: ChartPick) -> Option<Part> {
    match pick {
        ChartPick::Point { series, x } => {
            let s = ch.series.get(series)?;
            let v = (*s.values.get(x)?)?;
            let x_label = ch.x.get(x).cloned().unwrap_or_else(|| (x + 1).to_string());
            let display = with_unit(v, ch.unit);
            let mut val = Map::new();
            val.insert("series".into(), Value::from(s.key.clone()));
            val.insert("x".into(), Value::from(x_label.clone()));
            val.insert("value".into(), json!(v));
            if let Some(u) = ch.unit {
                val.insert("unit".into(), Value::from(u));
            }
            Some(Part {
                part: Some(part(
                    if ch.bar { "bar" } else { "point" },
                    &[("series", key(&s.key)), ("x", key(&x_label))],
                )),
                text: Some(fill(
                    c::PART_POINT,
                    &[("series", &s.name), ("x", &x_label), ("value", &display)],
                )),
                value: Some(Value::Object(val)),
            })
        }
        ChartPick::Series(series) => {
            let s = ch.series.get(series)?;
            let xs: Vec<Value> = ch.x.iter().take(200).cloned().map(Value::from).collect();
            let values: Vec<Value> = s.values.iter().take(200).map(|v| json!(v)).collect();
            let mut val = json!({ "series": s.key, "x": xs, "values": values });
            if let (Some(u), Value::Object(o)) = (ch.unit, &mut val) {
                o.insert("unit".into(), Value::from(u));
            }
            Some(Part {
                part: Some(part("series", &[("series", key(&s.key))])),
                text: Some(fill(c::PART_SERIES, &[("series", &s.name)])),
                value: Some(val),
            })
        }
    }
}

// ---------------------------------------------------------------------------------------
// Metric, list items, fields, plan steps, code lines, images, evidence, diagram nodes
// ---------------------------------------------------------------------------------------

pub fn metric(
    label_text: &str,
    v: f64,
    unit: Option<&str>,
    delta: Option<f64>,
    format: Option<&str>,
) -> Part {
    let display = with_unit(v, unit);
    let mut val = Map::new();
    val.insert("label".into(), Value::from(label_text));
    val.insert("value".into(), json!(v));
    if let Some(u) = unit {
        val.insert("unit".into(), Value::from(u));
    }
    if let Some(f) = format {
        val.insert("format".into(), Value::from(f));
    }
    if let Some(d) = delta {
        val.insert("delta".into(), json!(d));
    }
    Part {
        part: Some(part("metric", &[])),
        text: Some(fill(
            c::PART_METRIC,
            &[("label", label_text), ("value", &display)],
        )),
        value: Some(Value::Object(val)),
    }
}

/// An item of a list (by index, or by the item id when it has one).
pub fn list_item(index: usize, id: Option<&str>, text: &str, status: Option<&str>) -> Part {
    let mut val = json!({ "text": label::cut(text, 500) });
    if let (Some(s), Value::Object(o)) = (status, &mut val) {
        o.insert("status".into(), Value::from(s));
    }
    Part {
        part: Some(part(
            "item",
            &[("item", id.map(key).unwrap_or_else(|| idx(index)))],
        )),
        text: Some(label::cut(text, 40)),
        value: Some(val),
    }
}

/// A field (`fields` block, a `draft` field, an answered input).
pub fn field(index: usize, id: Option<&str>, field_label: &str, field_value: &str) -> Part {
    Part {
        part: Some(part(
            "item",
            &[("item", id.map(key).unwrap_or_else(|| idx(index)))],
        )),
        text: Some(label::cut(
            &fill(
                c::PART_FIELD,
                &[("label", field_label), ("value", field_value)],
            ),
            40,
        )),
        value: Some(json!({ "label": field_label, "value": label::cut(field_value, 500) })),
    }
}

/// A choice option (`decision` option, `rank` item): `picked` is the viewer's own choice.
pub fn option(id: &str, option_label: &str, picked: Option<bool>) -> Part {
    let mut val = json!({ "option": id, "label": option_label });
    if let (Some(p), Value::Object(o)) = (picked, &mut val) {
        o.insert("picked".into(), Value::Bool(p));
    }
    Part {
        part: Some(part("item", &[("item", key(id))])),
        text: Some(label::cut(option_label, 40)),
        value: Some(val),
    }
}

/// A plan step, 0-based `index`; `state` is the plan state word (`done`, `working`, `todo`).
pub fn step(index: usize, text: &str, state: &str, at: Option<i64>) -> Part {
    let mut val = json!({ "text": label::cut(text, 500), "state": state });
    if let (Some(at), Value::Object(o)) = (at, &mut val) {
        o.insert("at".into(), Value::from(at));
    }
    Part {
        part: Some(part("step", &[("step", idx(index))])),
        text: Some(fill(
            c::PART_STEP,
            &[
                ("n", &(index + 1).to_string()),
                ("text", &label::cut(text, 40)),
            ],
        )),
        value: Some(val),
    }
}

/// A code line, 1-based `n` (`file` for a code 产出).
pub fn code_line(n: usize, text: &str, file: Option<&str>) -> Part {
    let mut keys = vec![("n", idx(n))];
    if let Some(f) = file {
        keys.push(("file", key(f)));
    }
    Part {
        part: Some(part("line", &keys)),
        text: Some(fill(c::PART_LINE, &[("n", &n.to_string())])),
        value: Some(json!({ "n": n, "text": label::cut(text, 500) })),
    }
}

/// A v1 `image` block: the image itself.
pub fn image(file_id: &str, alt: Option<&str>) -> Part {
    let mut val = json!({ "fileId": file_id });
    if let (Some(a), Value::Object(o)) = (alt, &mut val) {
        o.insert("alt".into(), Value::from(a));
    }
    Part {
        part: Some(part("image", &[])),
        text: Some(
            alt.map(str::trim)
                .filter(|a| !a.is_empty())
                .map(|a| label::cut(a, 40))
                .unwrap_or_else(|| c::BLOCK_IMAGE.to_string()),
        ),
        value: Some(val),
    }
}

/// One proof of an evidence block: a screenshot (`image`) or a test, log or check (`item`).
/// `index` is 0-based; `passed` is the result when known.
pub fn evidence(
    index: usize,
    shot: bool,
    caption: Option<&str>,
    file_id: Option<&str>,
    passed: Option<bool>,
) -> Part {
    let caption = caption.map(str::trim).filter(|c| !c.is_empty());
    let mut val = Map::new();
    val.insert(
        "kind".into(),
        Value::from(if shot { "screenshot" } else { "check" }),
    );
    if let Some(c) = caption {
        val.insert("caption".into(), Value::from(c));
    }
    if let Some(f) = file_id {
        val.insert("fileId".into(), Value::from(f));
    }
    let result = passed.map(|p| if p { c::RESULT_PASS } else { c::RESULT_FAIL });
    if let Some(p) = passed {
        val.insert("passed".into(), Value::Bool(p));
    }
    if let Some(r) = result {
        val.insert("result".into(), Value::from(r));
    }
    let text = if shot {
        caption
            .map(|c| label::cut(c, 40))
            .unwrap_or_else(|| fill(c::PART_SHOT, &[("k", &(index + 1).to_string())]))
    } else {
        let cap = caption.unwrap_or_default();
        match result {
            Some(r) => fill(
                c::PART_RESULT,
                &[("caption", &label::cut(cap, 40)), ("result", r)],
            ),
            None => label::cut(cap, 40),
        }
    };
    Part {
        part: Some(part(
            if shot { "image" } else { "item" },
            &[("item", idx(index))],
        )),
        text: Some(text),
        value: Some(Value::Object(val)),
    }
}

/// A diagram node (REFS §B5): the node id from the source and its label.
pub fn node(id: &str, node_label: &str, diagram: Option<&str>) -> Part {
    let mut val = json!({ "node": id, "label": label::cut(node_label, 200) });
    if let (Some(d), Value::Object(o)) = (diagram, &mut val) {
        o.insert("diagram".into(), Value::from(d));
    }
    Part {
        part: Some(part("node", &[("node", key(id))])),
        text: Some(label::cut(
            if node_label.trim().is_empty() {
                id
            } else {
                node_label
            },
            60,
        )),
        value: Some(val),
    }
}

// ---------------------------------------------------------------------------------------
// Diffs
// ---------------------------------------------------------------------------------------

/// One line of a unified diff with its numbers.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DiffLine {
    pub file: String,
    /// 0-based hunk index within the file.
    pub hunk: usize,
    /// `old` for a removed line, else `new`.
    pub side: &'static str,
    pub n: usize,
    pub text: String,
}

/// One hunk: its file, header and patch text.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DiffHunk {
    pub file: String,
    pub index: usize,
    pub header: String,
    pub patch: String,
}

/// The lines and hunks of a unified diff. `default_file` names the file when the text has no
/// `+++` header (a diff block with `path`).
pub fn parse_diff(text: &str, default_file: Option<&str>) -> (Vec<DiffLine>, Vec<DiffHunk>) {
    let mut lines = Vec::new();
    let mut hunks: Vec<DiffHunk> = Vec::new();
    let mut file = default_file.unwrap_or_default().to_string();
    let mut file_hunks = 0usize;
    let (mut old, mut new) = (0usize, 0usize);
    let mut in_hunk = false;
    for raw in text.lines() {
        if let Some(p) = raw.strip_prefix("+++ ") {
            let p = p.trim();
            file = p.strip_prefix("b/").unwrap_or(p).to_string();
            file_hunks = 0;
            in_hunk = false;
            continue;
        }
        if raw.starts_with("--- ") || raw.starts_with("diff ") || raw.starts_with("index ") {
            in_hunk = false;
            continue;
        }
        if let Some(rest) = raw.strip_prefix("@@") {
            // @@ -a,b +c,d @@
            let nums: Vec<usize> = rest
                .split_whitespace()
                .take(2)
                .filter_map(|t| {
                    t.trim_start_matches(['-', '+'])
                        .split(',')
                        .next()?
                        .parse()
                        .ok()
                })
                .collect();
            old = nums.first().copied().unwrap_or(1);
            new = nums.get(1).copied().unwrap_or(1);
            hunks.push(DiffHunk {
                file: file.clone(),
                index: file_hunks,
                header: raw.to_string(),
                patch: String::new(),
            });
            file_hunks += 1;
            in_hunk = true;
            continue;
        }
        if !in_hunk {
            continue;
        }
        if let Some(h) = hunks.last_mut() {
            if h.patch.len() < 1500 {
                h.patch.push_str(raw);
                h.patch.push('\n');
            }
        }
        let hunk = file_hunks.saturating_sub(1);
        if let Some(t) = raw.strip_prefix('-') {
            lines.push(DiffLine {
                file: file.clone(),
                hunk,
                side: "old",
                n: old,
                text: t.to_string(),
            });
            old += 1;
        } else if let Some(t) = raw.strip_prefix('+') {
            lines.push(DiffLine {
                file: file.clone(),
                hunk,
                side: "new",
                n: new,
                text: t.to_string(),
            });
            new += 1;
        } else {
            let t = raw.strip_prefix(' ').unwrap_or(raw);
            lines.push(DiffLine {
                file: file.clone(),
                hunk,
                side: "new",
                n: new,
                text: t.to_string(),
            });
            old += 1;
            new += 1;
        }
    }
    for h in &mut hunks {
        if h.patch.len() > 1500 {
            h.patch = label::cut(&h.patch, 1500);
        }
    }
    (lines, hunks)
}

fn basename(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

pub fn diff_line(l: &DiffLine) -> Part {
    Part {
        part: Some(part(
            "line",
            &[
                ("file", key(&l.file)),
                ("side", key(l.side)),
                ("n", idx(l.n)),
            ],
        )),
        text: Some(fill(
            c::PART_DIFF_LINE,
            &[("file", basename(&l.file)), ("n", &l.n.to_string())],
        )),
        value: Some(
            json!({ "file": l.file, "side": l.side, "n": l.n, "text": label::cut(&l.text, 500) }),
        ),
    }
}

pub fn diff_hunk(h: &DiffHunk) -> Part {
    let mut patch = h.patch.clone();
    while patch.len() > 1500 {
        patch = label::cut(&patch, patch.chars().count() * 3 / 4);
    }
    Part {
        part: Some(part(
            "hunk",
            &[("file", key(&h.file)), ("hunk", idx(h.index))],
        )),
        text: Some(fill(
            c::PART_HUNK,
            &[
                ("file", basename(&h.file)),
                ("k", &(h.index + 1).to_string()),
            ],
        )),
        value: Some(json!({ "file": h.file, "header": h.header, "patch": patch })),
    }
}

pub fn diff_file(path: &str, status: &str, additions: u64, deletions: u64) -> Part {
    Part {
        part: Some(part("item", &[("item", key(path))])),
        text: Some(basename(path).to_string()),
        value: Some(
            json!({ "file": path, "status": status, "additions": additions, "deletions": deletions }),
        ),
    }
}

// ---------------------------------------------------------------------------------------
// v1 cards
// ---------------------------------------------------------------------------------------

/// What a v1 card block offers, by its index (`v1:<i>`).
pub mod v1 {
    use super::*;
    use crate::data::card::{Card, CardBlock};

    pub fn block_id(index: usize) -> String {
        format!("v1:{index}")
    }

    fn kind_of(b: &CardBlock) -> BlockKind {
        match b {
            CardBlock::Table { .. } => BlockKind::Table,
            CardBlock::List { .. } => BlockKind::List,
            CardBlock::Code { .. } => BlockKind::Code,
            CardBlock::Image { .. } => BlockKind::Image,
            _ => BlockKind::Other,
        }
    }

    /// The `{block}` segment of block `index` (v1 blocks have no titles).
    pub fn name(card: &Card, index: usize) -> Option<String> {
        let kind = kind_of(card.blocks.get(index)?);
        let same = card.blocks.iter().filter(|b| kind_of(b) == kind).count();
        block_name(None, kind, same)
    }

    /// A v1 block can be referenced (dividers, unknown blocks and empty blocks cannot).
    pub fn referenceable(b: &CardBlock) -> bool {
        !matches!(b, CardBlock::Divider | CardBlock::Unknown)
    }

    /// The whole block `index`.
    pub fn whole_block(card: &Card, index: usize) -> Option<Part> {
        let b = card.blocks.get(index)?;
        if !referenceable(b) {
            return None;
        }
        Some(match b {
            CardBlock::Text { text } => whole(text),
            CardBlock::Code { text, .. } => whole(text),
            CardBlock::Link { label, .. } => whole(label),
            CardBlock::Progress { label, .. } => whole(label.as_deref().unwrap_or_default()),
            CardBlock::Image { file_id, alt } => image(file_id, alt.as_deref()),
            CardBlock::Table { columns, rows } => Part {
                part: None,
                text: None,
                value: Some(
                    json!({ "columns": columns, "rows": rows.iter().take(50).collect::<Vec<_>>() }),
                ),
            },
            CardBlock::List { items } => whole(
                &items
                    .iter()
                    .map(|i| i.text.as_str())
                    .collect::<Vec<_>>()
                    .join("\n"),
            ),
            CardBlock::Fields { fields } => whole(
                &fields
                    .iter()
                    .map(|f| format!("{} {}", f.label, f.value))
                    .collect::<Vec<_>>()
                    .join("\n"),
            ),
            _ => whole(""),
        })
    }

    /// The reference to `part` of block `index` of a v1 card in message `source`.
    pub fn target(card: &Card, index: usize, part: Part, source: RefSource) -> PartTarget {
        let name = name(card, index);
        part.target(source, &card.title, block_id(index), name.as_deref(), None)
    }
}
