//! Parts of message bodies (REFS §A2.1): Markdown tables (cells and columns) and code fences
//! (`md:<index>`), and the shared table affordances that v1 card tables use too.

use std::rc::Rc;

use gpui::{
    AnyElement, App, ClipboardItem, Div, ElementId, InteractiveElement as _, IntoElement,
    ParentElement as _, StatefulInteractiveElement as _, Styled as _, Window, div,
    prelude::FluentBuilder as _, px,
};
use gpui_component::{
    menu::{ContextMenuExt as _, PopupMenuItem},
    text::{MarkdownNode, MarkdownParseContext, MarkdownPlugin, markdown_ast},
};

use super::{
    PartTarget, RefIntent, RefSource, ask_about,
    parts::{self, BlockKind, TableData, TablePick},
};
use crate::{
    copy::refs as c,
    markdown::{MdBlock, top_level_blocks},
    theme::{self, Type as _, space},
};

/// The message a body belongs to, for references to its blocks.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RefsBody {
    pub group_id: String,
    pub topic_id: String,
    pub message_id: String,
    /// The `{source}` segment: the author's name.
    pub author: String,
    /// Blocks of the stored body before the shown one (1 when the quote lines of the
    /// message's own references were stripped), so `md:<n>` counts in the stored body.
    pub block_offset: usize,
}

impl RefsBody {
    fn source(&self) -> RefSource {
        RefSource::Message {
            group_id: self.group_id.clone(),
            topic_id: self.topic_id.clone(),
            message_id: self.message_id.clone(),
        }
    }
}

// ---------------------------------------------------------------------------------------
// Table affordances (v1 card tables and Markdown tables)
// ---------------------------------------------------------------------------------------

/// Width of the slot at the end of each row that holds 问这里.
pub const ROW_BUTTON_SLOT: f32 = 76.;

/// The references of one table: `target` builds the reference of a cell, row or column.
#[derive(Clone)]
pub struct TableRefs {
    /// Unique among the visible tables.
    pub prefix: String,
    pub target: Rc<dyn Fn(TablePick) -> Option<PartTarget>>,
}

impl TableRefs {
    /// The element id of a cell (also its keyboard part id).
    pub fn cell_id(&self, row: usize, col: usize) -> String {
        format!("{}-c{row}-{col}", self.prefix)
    }

    /// The keyboard part id of a whole row (REFS §A3: ⇧ + ← or →).
    pub fn row_id(&self, row: usize) -> String {
        format!("{}-r{row}", self.prefix)
    }

    /// The element id of a header cell: the whole column.
    pub fn header_id(&self, col: usize) -> String {
        format!("{}-h{col}", self.prefix)
    }

    /// The cell under the pointer in `row`, and whether its 问这里 shows.
    fn hovered_in(&self, row: usize, cx: &App) -> Option<(usize, bool)> {
        let h = cx.try_global::<super::HubGlobal>()?.0.read(cx);
        let row_prefix = format!("{}-c{row}-", self.prefix);
        let pick = |id: &str| id.strip_prefix(&row_prefix)?.parse::<usize>().ok();
        if let Some(col) = h.forced_hover.as_deref().and_then(pick) {
            return Some((col, true));
        }
        let (id, _) = h.hover.as_ref()?;
        let col = pick(id)?;
        Some((col, h.hover_shown(id)))
    }

    /// The row's look: the hover fill while one of its cells shows 问这里, and the focus
    /// ring while the keyboard part focus is on the whole row.
    pub fn row(&self, el: Div, row: usize, window: &Window, cx: &App) -> Div {
        let k = theme::colors(cx);
        let shown = self.hovered_in(row, cx).is_some_and(|(_, s)| s);
        let focused = super::keyboard::part_focused(&self.row_id(row), window, cx);
        el.relative()
            .when(shown, |el| el.bg(k.hover))
            .when(focused, |el| {
                el.child(super::keyboard::ring(theme::radius::MARK, cx))
            })
    }

    /// The slot at the end of a row: 问这里 for the hovered cell, at the row's right end
    /// and centered on its height (REFS §A3), however narrow the columns are.
    pub fn row_end(&self, row: usize, cx: &App) -> AnyElement {
        let slot = div()
            .ml_auto()
            .self_stretch()
            .w(px(ROW_BUTTON_SLOT))
            .flex_none()
            .flex()
            .items_center()
            .justify_end()
            .pr(space::S4);
        let Some((col, true)) = self.hovered_in(row, cx) else {
            return slot.into_any_element();
        };
        let target = self.target.clone();
        let id = self.cell_id(row, col);
        slot.child(super::affordance::inline_button(
            &id,
            Rc::new(move |_: &App| {
                target(TablePick::Cell { row, col }).unwrap_or_else(super::empty_target)
            }),
            cx,
        ))
        .into_any_element()
    }

    /// A cell: hover tracking, ⌥-click and the context menu.
    pub fn cell(&self, el: Div, row: usize, col: usize, window: &Window, cx: &App) -> AnyElement {
        let id = self.cell_id(row, col);
        self.interactive(el, id, TablePick::Cell { row, col }, window, cx)
    }

    /// A header cell: the whole column.
    pub fn header(&self, el: Div, col: usize, window: &Window, cx: &App) -> AnyElement {
        let id = self.header_id(col);
        self.interactive(el, id, TablePick::Column(col), window, cx)
    }

    fn interactive(
        &self,
        el: Div,
        id: String,
        pick: TablePick,
        window: &Window,
        cx: &App,
    ) -> AnyElement {
        let k = theme::colors(cx);
        let hovered = super::affordance::hover_state(&id, cx).0;
        let alt = window.modifiers().alt;
        let flashing = !cx.reduce_motion()
            && cx
                .try_global::<super::HubGlobal>()
                .is_some_and(|h| h.0.read(cx).flash.is_some())
            && (self.target)(pick).is_some_and(|t| {
                cx.try_global::<super::HubGlobal>()
                    .is_some_and(|h| h.0.read(cx).flashing(&t.address()))
            });
        let (t1, t2, t3, t4) = (
            self.target.clone(),
            self.target.clone(),
            self.target.clone(),
            self.target.clone(),
        );
        let focused = super::keyboard::part_focused(&id, window, cx);
        el.id(ElementId::Name(id.clone().into()))
            .relative()
            .rounded(theme::radius::MARK)
            .when(focused, |el| {
                el.child(super::keyboard::ring(theme::radius::MARK, cx))
            })
            .when(flashing, |el| el.bg(k.sel))
            .when(hovered && alt && !flashing, |el| el.bg(k.hover))
            .on_hover(super::affordance::track_hover(id))
            .on_click(move |e, window, cx| {
                if e.modifiers().alt
                    && let Some(t) = t1(pick)
                {
                    cx.stop_propagation();
                    ask_about(t, RefIntent::Topic, window, cx);
                }
            })
            .context_menu(move |m, _, _| {
                let (a, b, d) = (t2.clone(), t3.clone(), t4.clone());
                m.item(
                    PopupMenuItem::new(c::ASK_IN_TOPIC).on_click(move |_, window, cx| {
                        if let Some(t) = a(pick) {
                            ask_about(t, RefIntent::Topic, window, cx)
                        }
                    }),
                )
                .item(
                    PopupMenuItem::new(c::ASK_ALONE).on_click(move |_, window, cx| {
                        if let Some(t) = b(pick) {
                            ask_about(t, RefIntent::SideChat, window, cx)
                        }
                    }),
                )
                .separator()
                .item(PopupMenuItem::new(c::COPY).on_click(move |_, _, cx| {
                    if let Some(t) = d(pick) {
                        let text = t
                            .value
                            .as_ref()
                            .and_then(|v| v.get("display").or_else(|| v.get("column")))
                            .and_then(|v| v.as_str())
                            .unwrap_or_default()
                            .to_string();
                        cx.write_to_clipboard(ClipboardItem::new_string(text));
                    }
                }))
            })
            .into_any_element()
    }
}

// ---------------------------------------------------------------------------------------
// Markdown tables
// ---------------------------------------------------------------------------------------

/// Draws the tables of a body natively with cell and column references. Added to a body's
/// `TextView` only when the server takes references and the message has an id.
pub struct TablePlugin {
    pub body: RefsBody,
    /// The top-level blocks of the rendered source (to find `md:<index>`).
    pub blocks: Vec<(MdBlock, std::ops::Range<usize>)>,
}

impl TablePlugin {
    pub fn new(body: RefsBody, source: &str) -> Self {
        TablePlugin {
            body,
            blocks: top_level_blocks(source),
        }
    }
}

struct TableNode {
    index: usize,
    /// More than one table in the body: the block is named 表格.
    named: bool,
    columns: Vec<String>,
    rows: Vec<Vec<String>>,
    right: Vec<bool>,
    body: RefsBody,
}

fn plain(node: &markdown_ast::Node) -> String {
    node.to_string()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

impl MarkdownPlugin for TablePlugin {
    fn is_block(&self) -> bool {
        true
    }

    fn name(&self) -> &str {
        "ref-table"
    }

    fn parse(
        &self,
        node: &markdown_ast::Node,
        cx: &MarkdownParseContext<'_>,
    ) -> Option<MarkdownNode> {
        let markdown_ast::Node::Table(table) = node else {
            return None;
        };
        let at = cx.offset() + node.position()?.start.offset;
        let index = self.blocks.iter().position(|(k, r)| {
            *k == MdBlock::Table && r.start <= at && at < r.end.max(r.start + 1)
        })?;
        let mut rows: Vec<Vec<String>> = table
            .children
            .iter()
            .map(|row| match row {
                markdown_ast::Node::TableRow(r) => r.children.iter().map(plain).collect(),
                _ => Vec::new(),
            })
            .collect();
        if rows.is_empty() {
            return None;
        }
        let columns = rows.remove(0);
        let right = table
            .align
            .iter()
            .map(|a| matches!(a, markdown::mdast::AlignKind::Right))
            .collect();
        let named = self
            .blocks
            .iter()
            .filter(|(k, _)| *k == MdBlock::Table)
            .count()
            > 1;
        let text = std::iter::once(columns.join("\t"))
            .chain(rows.iter().map(|r| r.join("\t")))
            .collect::<Vec<_>>()
            .join("\n");
        Some(
            MarkdownNode::new(
                "ref-table",
                TableNode {
                    index,
                    named,
                    columns,
                    rows,
                    right,
                    body: self.body.clone(),
                },
            )
            .text(text),
        )
    }

    fn render(&self, node: &MarkdownNode, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let Some(t) = node.data::<TableNode>() else {
            return div().into_any_element();
        };
        let block_id = format!("md:{}", t.index + t.body.block_offset);
        let refs = {
            let (columns, rows, body) = (t.columns.clone(), t.rows.clone(), t.body.clone());
            let block_id = block_id.clone();
            let named = t.named;
            TableRefs {
                prefix: format!("{}-{}", t.body.message_id, block_id),
                target: Rc::new(move |pick| {
                    let data = TableData {
                        columns: &columns,
                        rows: &rows,
                        row_keys: None,
                    };
                    let name = parts::block_name(None, BlockKind::Table, if named { 2 } else { 1 });
                    parts::table(&data, pick).map(|p| {
                        p.target(
                            body.source(),
                            &body.author,
                            block_id.clone(),
                            name.as_deref(),
                            None,
                        )
                    })
                }),
            }
        };
        table(
            &refs.prefix.clone(),
            &t.columns,
            &t.rows,
            &t.right,
            Some(&refs),
            window,
            cx,
        )
    }
}

/// Column width from its longest cell, in points.
fn column_width(columns: &[String], rows: &[Vec<String>], col: usize) -> f32 {
    let longest = std::iter::once(columns.get(col).map(String::as_str).unwrap_or(""))
        .chain(
            rows.iter()
                .map(|r| r.get(col).map(String::as_str).unwrap_or("")),
        )
        .map(|s| {
            s.chars()
                .map(|ch| if ch.is_ascii() { 1.0 } else { 1.8 })
                .sum::<f32>()
        })
        .fold(0.0_f32, f32::max);
    (longest * 7.0 + 24.0).clamp(64.0, 280.0)
}

/// A table in the card table's look (header `meta` medium `text-3`, hairlines between rows,
/// `small` tabular text) with cell and column references, scrolling sideways when wide.
pub fn table(
    key: &str,
    columns: &[String],
    rows: &[Vec<String>],
    right: &[bool],
    refs: Option<&TableRefs>,
    window: &Window,
    cx: &App,
) -> AnyElement {
    let k = theme::colors(cx);
    let widths: Vec<f32> = (0..columns.len())
        .map(|i| column_width(columns, rows, i))
        .collect();
    let cell = |text: String, width: f32, right: bool| -> Div {
        div()
            .w(px(width))
            .flex_none()
            .px(space::S10)
            .py(space::S6)
            .flex()
            .when(right, |d| d.justify_end())
            .child(text)
    };
    let mut header = div().flex().t_meta_medium().text_color(k.text_3);
    for (i, (name, w)) in columns.iter().zip(&widths).enumerate() {
        let el = cell(name.clone(), *w, right.get(i).copied().unwrap_or(false));
        header = header.child(match refs {
            Some(r) => r.header(el, i, window, cx),
            None => el.into_any_element(),
        });
    }
    if refs.is_some() {
        header = header.child(div().ml_auto().w(px(ROW_BUTTON_SLOT)).flex_none());
    }
    let body = rows.iter().enumerate().map(|(ri, row)| {
        let mut line = div()
            .flex()
            .border_t(theme::HAIRLINE)
            .border_color(k.sep)
            .t_small()
            .tnum()
            .text_color(k.text_1);
        if let Some(r) = refs {
            line = r.row(line, ri, window, cx);
        }
        for (ci, w) in widths.iter().enumerate() {
            let el = cell(
                row.get(ci).cloned().unwrap_or_default(),
                *w,
                right.get(ci).copied().unwrap_or(false),
            );
            line = line.child(match refs {
                Some(r) => r.cell(el, ri, ci, window, cx),
                None => el.into_any_element(),
            });
        }
        if let Some(r) = refs {
            line = line.child(r.row_end(ri, cx));
        }
        line
    });
    div()
        .w_full()
        .py(space::S4)
        .child(
            div()
                .id(ElementId::Name(format!("{key}-scroll").into()))
                .w_full()
                .overflow_x_scroll()
                .child(div().flex().flex_col().child(header).children(body)),
        )
        .into_any_element()
}

// ---------------------------------------------------------------------------------------
// Code fences
// ---------------------------------------------------------------------------------------

/// The reference to a whole code fence of a body: the fence whose code is `code` (the first
/// such fence), as `md:<index>`.
pub fn code_target(body: &RefsBody, source: &str, code: &str) -> Option<PartTarget> {
    let blocks = top_level_blocks(source);
    let codes: Vec<usize> = blocks
        .iter()
        .enumerate()
        .filter(|(_, (k, _))| *k == MdBlock::Code)
        .map(|(i, _)| i)
        .collect();
    let want = code.trim_end();
    let index = codes.iter().copied().find(|i| {
        let r = &blocks[*i].1;
        source.get(r.clone()).is_some_and(|s| s.contains(want))
    })?;
    let name = parts::block_name(None, BlockKind::Code, codes.len());
    Some(parts::whole(code).target(
        body.source(),
        &body.author,
        format!("md:{}", index + body.block_offset),
        name.as_deref().or(Some(c::BLOCK_CODE)),
        None,
    ))
}
