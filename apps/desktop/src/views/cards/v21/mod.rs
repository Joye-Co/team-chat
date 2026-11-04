//! Card v2.1 (CardsCompose.dc.html, Interact.dc.html; V11-INTERACT §2–§4, V11-CARD21).
//!
//! The 2.0 renderer (`super::v2`) draws the card frame, the status band, the header, the
//! title and the footer. A card that uses anything of 2.1 (catalog `tc.card/2.1`, a 2.1
//! block, a container with `children`, or a block this version does not know) gets its body
//! from [`body`] here: the flat block list becomes a tree (containers name their children
//! by id), containers lay out their children, and every leaf is drawn by the 2.0 renderer or
//! by `leaves.rs` / `data.rs`. Interactions go back to the card's author as `card.event`
//! (`interact.rs`); the person's local state (selection, sort, tab, collapse, the chart
//! point, drafts) is kept per `(messageId, blockId)` across revs in [`CardUi`] and the
//! card's form.
//!
//! Files: `form21.rs` (values and the answer payload), `leaves.rs` (choice variants,
//! buttons, embed, diagram, unknown box), `data.rs` (table, chart, metric with events),
//! `interact.rs` (emission, pending, conflicts, attribution, diff motion), `parts.rs`
//! (引用提问 affordances), `band.rs` (status band words), `samples.rs`, `gallery.rs`,
//! `scenes.rs`.

pub mod band;
mod data;
pub mod form21;
pub mod gallery;
pub mod interact;
pub(crate) mod leaves;
pub mod parts;
pub mod samples;
pub mod scenes;

use std::{
    cell::Cell,
    collections::{BTreeSet, HashMap, HashSet},
    rc::Rc,
    sync::Arc,
    time::{Duration, Instant},
};

use gpui::{
    AnyElement, App, Div, FontWeight, InteractiveElement as _, IntoElement, ParentElement,
    SharedString, StatefulInteractiveElement as _, Styled, Window, div,
    prelude::FluentBuilder as _, px,
};

use super::v2::{Ctx, blocks as b20, meta};
use crate::{
    copy::{cards21 as c, fill},
    data::cards_v2::{CardV2, CardV2Block, Cause},
    theme::{self, Type as _, card_v21 as interact_t, space},
    ui::{self, Sym},
};

/// A card narrower than this lays a `row` out as a stack (INTERACT §2.2).
pub const ROW_FOLD: f32 = 560.;
/// Top-level blocks shown before 展开 (CARD21 §3).
pub const FOLD_AFTER: usize = 8;

/// True when the card needs the 2.1 body.
pub fn is_21(card: &CardV2) -> bool {
    card.catalog() == chat_core::proto::CARD_CATALOG_21
        || card.blocks.iter().any(|b| {
            b.is_21()
                || matches!(b, CardV2Block::Unknown)
                || matches!(
                    b,
                    CardV2Block::Group {
                        children: Some(_),
                        ..
                    }
                )
                || b.base().is_some_and(|x| !x.events().is_empty())
                || matches!(
                    b,
                    CardV2Block::Table {
                        select: Some(_),
                        ..
                    } | CardV2Block::Table {
                        sortable: Some(true),
                        ..
                    }
                )
        })
}

// ---------------------------------------------------------------------------
// Local UI state

/// What one viewer did to one block that is not an answer value: a group's collapse, the
/// open tab, the table sort, the chart point (INTERACT §4.4).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct BlockUi {
    pub collapsed: Option<bool>,
    pub tab: Option<String>,
    /// Tabs opened once (`tab.open` goes only the first time).
    pub opened: BTreeSet<String>,
    /// Column and ascending.
    pub sort: Option<(usize, bool)>,
    /// Series key and point index.
    pub point: Option<(String, usize)>,
    /// The chart column under the pointer.
    pub hot: Option<usize>,
    /// v11/r3-cards: I cleared the chart's point, so the pick that caused the rev no longer
    /// marks it.
    pub cleared: bool,
    /// The focused rank item.
    pub focus: Option<String>,
}

/// What changed between the rev the viewer saw and the one drawn now.
#[derive(Clone, Debug, Default)]
pub struct RevDiff {
    pub to: u32,
    /// Inserted or changed block ids, with when the viewer first saw the change.
    pub changed: HashMap<String, Instant>,
    /// The ones among `changed` that the rev inserted (marked 刚加的 until the next rev).
    pub inserted: HashSet<String>,
    pub cause: Option<Cause>,
    /// Top-level blocks the rev removed, after which kept block they stood, and when the
    /// viewer first saw the removal (they fade out).
    pub removed: Vec<(Option<String>, CardV2Block)>,
    pub at: Option<Instant>,
}

/// Per card and window: local UI state that survives revs while the block id exists.
pub struct CardUi {
    pub blocks: HashMap<String, BlockUi>,
    /// The last rev drawn (for the block diff).
    pub seen: Option<Arc<CardV2>>,
    pub diff: Option<RevDiff>,
    /// The next pending threshold a redraw is scheduled for.
    pub wake_at: Option<Instant>,
    /// Width of the card body at the last paint; set in prepaint.
    pub width: Rc<Cell<f32>>,
    /// 展开 of the blocks after the first 8.
    pub unfolded: bool,
    /// A live event waiting for the one in flight on its block (latest wins).
    pub queued: HashMap<String, crate::data::cards_v2::CardEventRequest>,
    /// Debounce generations of text inputs.
    pub debounce: HashMap<String, u64>,
    /// The block under the pointer and since when (问这里 after 300 ms).
    pub hover: Option<(String, Instant)>,
    /// v11/pb-cards: each block's state while none of my live events was on its way, put
    /// back when one comes back offline or busy (INTERACT §4.4).
    pub settled: HashMap<String, (BlockUi, crate::views::cards::v2::form::FormSnap)>,
    /// Live events whose optimistic state was put back, by `clientEventId`.
    pub reverted: HashSet<String>,
    /// Blocks a new rev changed while I had typed in them: my text stays, with 内容变了 ·
    /// 用新的 (INTERACT §4.4).
    pub drafts_kept: HashSet<String>,
    /// v11/pb2-cards: the rev whose removed blocks have their fade-end redraw scheduled
    /// (one timer per removal, not one per frame).
    pub fade_timer: Option<u32>,
}

impl CardUi {
    pub fn new(prev: Option<Arc<CardV2>>) -> Self {
        Self {
            blocks: HashMap::new(),
            seen: prev,
            diff: None,
            wake_at: None,
            width: Rc::new(Cell::new(0.)),
            unfolded: false,
            queued: HashMap::new(),
            debounce: HashMap::new(),
            hover: None,
            settled: HashMap::new(),
            reverted: HashSet::new(),
            drafts_kept: HashSet::new(),
            fade_timer: None,
        }
    }

    pub fn block(&self, id: &str) -> BlockUi {
        self.blocks.get(id).cloned().unwrap_or_default()
    }

    pub fn block_mut(&mut self, id: &str) -> &mut BlockUi {
        self.blocks.entry(id.to_string()).or_default()
    }

    /// Moves to a newer rev: drops state of blocks that are gone or changed type, and
    /// records which blocks the rev inserted or changed (for the motion and the
    /// attribution line).
    pub fn observe(&mut self, card: &Arc<CardV2>, cause: Option<&Cause>) {
        let Some(old) = self.seen.clone() else {
            self.seen = Some(card.clone());
            return;
        };
        if old.rev >= card.rev {
            if old.rev == card.rev {
                self.seen = Some(card.clone());
            }
            return;
        }
        let now = Instant::now();
        let ids = interact::changed_blocks(&old, card);
        self.blocks.retain(|id, _| {
            matches!((old.block(id), card.block(id)), (Some(a), Some(b))
                if a.type_name() == b.type_name())
        });
        // A rev that changed a text input may set its text without a keystroke (an
        // untouched text takes the new default): the next draw only stores that text and
        // sends no `input.change`.
        for id in &ids {
            if matches!(card.block(id), Some(CardV2Block::Input { .. }))
                && let Some(b) = self.blocks.get_mut(id)
            {
                b.tab = None;
            }
        }
        let inserted = ids
            .iter()
            .filter(|id| old.block(id).is_none())
            .cloned()
            .collect();
        let changed = ids.into_iter().map(|id| (id, now)).collect();
        self.diff = Some(RevDiff {
            to: card.rev,
            changed,
            inserted,
            cause: cause.cloned(),
            removed: interact::removed_blocks(&old, card),
            at: Some(now),
        });
        self.seen = Some(card.clone());
    }

    /// Blocks that keep my typed text after a rebase onto `card`: the ones this rev kept
    /// (`kept`), and the ones an earlier rev kept whose text I still hold (`holds`), so
    /// 内容变了 · 用新的 stays on block A when a later rev changes only block B.
    pub fn keep_drafts(&mut self, kept: Vec<String>, card: &CardV2, holds: impl Fn(&str) -> bool) {
        self.drafts_kept
            .retain(|id| card.block(id).is_some() && holds(id));
        self.drafts_kept.extend(kept);
    }

    /// 用新的 on `block`: its text is the card's again, set without a keystroke, so the
    /// next draw sends no `input.change`.
    pub fn use_new(&mut self, block: &str) {
        self.drafts_kept.remove(block);
        if let Some(b) = self.blocks.get_mut(block) {
            b.tab = None;
        }
    }

    pub fn narrow(&self) -> bool {
        let w = self.width.get();
        w > 0. && w < ROW_FOLD
    }
}

// ---------------------------------------------------------------------------
// The tree

/// The card's blocks by id, and the top-level ones in reading order.
pub(crate) struct Tree<'a> {
    by_id: HashMap<&'a str, &'a CardV2Block>,
    pub top: Vec<&'a CardV2Block>,
    parent: HashMap<&'a str, &'a str>,
}

impl<'a> Tree<'a> {
    pub fn new(card: &'a CardV2) -> Self {
        let mut parent = HashMap::new();
        for b in &card.blocks {
            if let Some(id) = b.id() {
                for k in b.child_ids() {
                    parent.entry(k).or_insert(id);
                }
            }
        }
        let by_id = card
            .blocks
            .iter()
            .filter_map(|b| b.id().map(|id| (id, b)))
            .collect();
        let top = card
            .blocks
            .iter()
            .filter(|b| b.id().is_none_or(|id| !parent.contains_key(id)))
            .collect();
        Self { by_id, top, parent }
    }

    pub fn get(&self, id: &str) -> Option<&'a CardV2Block> {
        self.by_id.get(id).copied()
    }

    pub fn parent(&self, id: &str) -> Option<&'a str> {
        self.parent.get(id).copied()
    }

    /// The chain from the top level down to `id`.
    fn chain(&self, id: &'a str) -> Vec<&'a str> {
        let mut out = vec![id];
        let mut cur = id;
        while let Some(p) = self.parent(cur) {
            if out.contains(&p) {
                break;
            }
            out.push(p);
            cur = p;
        }
        out.reverse();
        out
    }

    /// The nodes that hold `ids` together: their lowest common container, or each one's
    /// top-level block when they share none.
    pub fn roots(&self, ids: &[&'a str]) -> Vec<&'a str> {
        let chains: Vec<Vec<&str>> = ids.iter().map(|id| self.chain(id)).collect();
        let Some(first) = chains.first() else {
            return Vec::new();
        };
        let mut common = 0;
        while common < first.len() && chains.iter().all(|c| c.get(common) == first.get(common)) {
            common += 1;
        }
        if common > 0 {
            return vec![first[common - 1]];
        }
        let mut out: Vec<&str> = Vec::new();
        for c in &chains {
            if !out.contains(&c[0]) {
                out.push(c[0]);
            }
        }
        out
    }
}

// ---------------------------------------------------------------------------
// Body

/// What a node renders with.
pub(crate) struct Node<'a, 'b> {
    pub ctx: &'a Ctx<'b>,
    pub tree: &'a Tree<'a>,
    pub ui: &'a CardUi,
    /// The card body is narrow: rows and compares stack.
    pub narrow: bool,
    /// Container depth of this node (top level is 0).
    pub depth: usize,
}

/// The body of a 2.1 card: top-level blocks with their spacing, the first
/// [`FOLD_AFTER`] shown and the rest behind 展开 (`emphasis: high` never folds; `low` folds
/// first).
pub(crate) fn body(ctx: &Ctx, window: &mut Window, cx: &mut App) -> Vec<AnyElement> {
    let card = ctx.card.clone();
    let tree = Tree::new(&card);
    let ui_entity = ctx.ui.clone();
    let ui = ui_entity.read(cx);
    let narrow = ui.narrow();
    let unfolded = ui.unfolded;
    let width = ui.width.clone();
    // A snapshot of the UI state for this frame (the entity is read again by handlers).
    let snapshot = CardUi {
        blocks: ui.blocks.clone(),
        seen: ui.seen.clone(),
        diff: ui.diff.clone(),
        wake_at: None,
        width: width.clone(),
        unfolded,
        queued: HashMap::new(),
        debounce: HashMap::new(),
        hover: None,
        settled: HashMap::new(),
        reverted: HashSet::new(),
        drafts_kept: ui.drafts_kept.clone(),
        fade_timer: None,
    };
    let node = Node {
        ctx,
        tree: &tree,
        ui: &snapshot,
        narrow,
        depth: 0,
    };
    let order = fold_order(&tree.top);
    let hidden: BTreeSet<usize> = if unfolded || order.len() <= FOLD_AFTER {
        BTreeSet::new()
    } else {
        order[FOLD_AFTER..].iter().copied().collect()
    };
    let mut out: Vec<AnyElement> = Vec::new();
    let mut prev: Option<&CardV2Block> = None;
    let fading = fading(ctx, &snapshot, cx);
    let fade_after =
        |after: Option<&str>, out: &mut Vec<AnyElement>, window: &mut Window, cx: &mut App| {
            for (_, b) in fading.iter().filter(|(a, _)| a.as_deref() == after) {
                out.push(removed(&node, b, window, cx));
            }
        };
    fade_after(None, &mut out, window, cx);
    for (i, b) in tree.top.iter().enumerate() {
        if hidden.contains(&i) {
            continue;
        }
        let el = render_node(&node, b, window, cx);
        // Top-level blocks the rev inserted or changed that follow each other share one
        // tinted run: no gap, a hairline between them (`interact::decorate`).
        let tinted = |x: &CardV2Block| {
            snapshot
                .diff
                .as_ref()
                .filter(|d| d.to == ctx.card.rev)
                .zip(x.id())
                .is_some_and(|(d, id)| d.changed.contains_key(id))
        };
        let gap = match (prev, b) {
            (Some(p), b) if tinted(p) && tinted(b) => px(0.),
            (Some(CardV2Block::Row { .. }), _) | (_, CardV2Block::Row { .. }) => space::S16,
            (Some(_), CardV2Block::Group { .. }) => space::S14,
            _ => space::S12,
        };
        out.push(div().mt(gap).min_w(px(0.)).child(el).into_any_element());
        prev = Some(b);
        fade_after(b.id(), &mut out, window, cx);
    }
    if !hidden.is_empty() {
        let k = theme::colors(cx);
        let n = hidden.len();
        let ui = ui_entity.clone();
        out.push(
            div()
                .mt(space::S12)
                .child(
                    b20::quiet_button(
                        ctx.id("fold"),
                        &fill(c::FOLD_MORE, &[("n", &n.to_string())]),
                        cx,
                    )
                    .text_color(k.ink_2)
                    .on_click(move |_, _, cx| {
                        ui.update(cx, |u, cx| {
                            u.unfolded = true;
                            cx.notify();
                        })
                    }),
                )
                .into_any_element(),
        );
    }
    // Measures the body for the row fold; a change of side redraws once.
    let was_narrow = width.get() > 0. && width.get() < ROW_FOLD;
    out.push(
        div()
            .relative()
            .h(px(0.))
            .w_full()
            .child(
                gpui::canvas(
                    move |bounds, window, _| {
                        let w = f32::from(bounds.size.width);
                        let now_narrow = w < ROW_FOLD;
                        let first = width.get() == 0.;
                        width.set(w);
                        if first || now_narrow != was_narrow {
                            window.refresh();
                        }
                    },
                    |_, _, _, _| {},
                )
                .absolute()
                .left_0()
                .top_0()
                .w_full()
                .h(px(0.)),
            )
            .into_any_element(),
    );
    interact::after_body(ctx, window, cx);
    out
}

/// The removed blocks still fading out (none with reduce motion), and a redraw when the
/// fade ends so they go.
fn fading(ctx: &Ctx, ui: &CardUi, cx: &mut App) -> Vec<(Option<String>, CardV2Block)> {
    let Some(d) = ui.diff.as_ref().filter(|d| d.to == ctx.card.rev) else {
        return Vec::new();
    };
    let Some(at) = d.at else { return Vec::new() };
    if d.removed.is_empty() || cx.reduce_motion() {
        return Vec::new();
    }
    let left = interact_t::REMOVE_OUT.saturating_sub(at.elapsed());
    if left.is_zero() {
        return Vec::new();
    }
    if schedule_fade_end(&ctx.ui, d.to, cx) {
        let ui = ctx.ui.clone();
        cx.spawn(async move |cx: &mut gpui::AsyncApp| {
            cx.background_executor()
                .timer(left + Duration::from_millis(20))
                .await;
            cx.update(|cx| ui.update(cx, |_, cx| cx.notify()));
        })
        .detach();
    }
    d.removed.clone()
}

/// True the first time rev `rev`'s removal asks for its fade-end redraw; later frames of
/// the same fade start no timer.
fn schedule_fade_end(ui: &gpui::Entity<CardUi>, rev: u32, cx: &mut App) -> bool {
    if ui.read(cx).fade_timer == Some(rev) {
        return false;
    }
    ui.update(cx, |u, _| u.fade_timer = Some(rev));
    true
}

/// A block the new rev removed, fading out where it stood (opacity only, no transforms).
fn removed(node: &Node, b: &CardV2Block, window: &mut Window, cx: &mut App) -> AnyElement {
    let ctx = node.ctx;
    let inner = match b {
        CardV2Block::Group { .. } | CardV2Block::Stack { .. } | CardV2Block::Row { .. } => {
            b20::block(ctx, b, 0, window, cx)
        }
        _ => leaf(node, b, window, cx),
    };
    let id = b.id().unwrap_or("");
    crate::ui::motion::once(
        ctx.id(format!("gone-{id}-{}", ctx.card.rev)),
        interact_t::REMOVE_OUT,
        div().mt(space::S12).min_w(px(0.)).child(inner),
        |el, p| el.opacity(1. - p),
    )
}

/// Indices of the top-level blocks in fold priority: `high` first, then the rest in order,
/// `low` last.
fn fold_order(top: &[&CardV2Block]) -> Vec<usize> {
    use crate::data::cards_v2::Emphasis;
    let em = |b: &CardV2Block| b.base().and_then(|x| x.emphasis);
    let mut out: Vec<usize> = (0..top.len())
        .filter(|i| em(top[*i]) == Some(Emphasis::High))
        .collect();
    out.extend((0..top.len()).filter(|i| em(top[*i]).is_none_or(|e| e == Emphasis::Unknown)));
    out.extend((0..top.len()).filter(|i| em(top[*i]) == Some(Emphasis::Low)));
    out
}

/// One block and its children, with the attribution line and the change motion.
pub(crate) fn render_node(
    node: &Node,
    b: &CardV2Block,
    window: &mut Window,
    cx: &mut App,
) -> AnyElement {
    let inner = match b {
        CardV2Block::Stack { children, .. } => stack(node, children, window, cx),
        CardV2Block::Row {
            children, weights, ..
        } => row(node, b, children, weights.as_deref(), window, cx),
        CardV2Block::Group {
            children: Some(children),
            ..
        } => group(node, b, children, window, cx),
        CardV2Block::Compare { .. } => leaves::compare(node, b, window, cx),
        CardV2Block::Tabs { .. } => tabs(node, b, window, cx),
        CardV2Block::Questionnaire { .. } => questionnaire(node, b, window, cx),
        _ => leaf(node, b, window, cx),
    };
    let el = interact::decorate(node, b, inner, window, cx);
    parts::wrap(node, b, el, window, cx)
}

/// A leaf block: the 2.1 renderers, else the 2.0 one.
fn leaf(node: &Node, b: &CardV2Block, window: &mut Window, cx: &mut App) -> AnyElement {
    let ctx = node.ctx;
    match b {
        CardV2Block::Decision { .. } => leaves::decision(node, b, window, cx),
        CardV2Block::Rank { .. } => leaves::rank(node, b, cx),
        CardV2Block::Rating { .. } => leaves::rating(node, b, cx),
        CardV2Block::Scale { .. } => leaves::scale(node, b, cx),
        CardV2Block::Confirm { .. } => leaves::confirm(node, b, cx),
        CardV2Block::ImageChoice { .. } => leaves::image_choice(node, b, cx),
        CardV2Block::Matrix { .. } => leaves::matrix(node, b, cx),
        CardV2Block::Slots { .. } => leaves::slots(node, b, cx),
        CardV2Block::Poll { .. } => leaves::poll(node, b, cx),
        CardV2Block::Buttons { .. } => leaves::buttons(node, b, window, cx),
        CardV2Block::Diagram { .. } => leaves::diagram(node, b, window, cx),
        CardV2Block::Embed { .. } => leaves::embed(node, b, window, cx),
        CardV2Block::Unknown => leaves::unknown(node, b, cx),
        CardV2Block::Table { .. } => data::table(node, b, window, cx),
        CardV2Block::Chart { .. } => data::chart(node, b, window, cx),
        CardV2Block::Metric { .. } => data::metric(node, b, window, cx),
        CardV2Block::Input { .. } => leaves::input(node, b, window, cx),
        CardV2Block::Group { .. } => b20::block(ctx, b, 0, window, cx),
        _ => b20::block(ctx, b, 0, window, cx),
    }
}

// ---------------------------------------------------------------------------
// Containers

fn child<'a>(node: &'a Node, id: &str) -> Option<&'a CardV2Block> {
    node.tree.get(id)
}

pub(crate) fn deeper<'a, 'b>(node: &Node<'a, 'b>) -> Node<'a, 'b> {
    Node {
        ctx: node.ctx,
        tree: node.tree,
        ui: node.ui,
        narrow: node.narrow,
        depth: node.depth + 1,
    }
}

/// Children one under the other with the card's block gap. An unknown child id is
/// skipped (the server rejects it on write).
fn stack(node: &Node, children: &[String], window: &mut Window, cx: &mut App) -> AnyElement {
    let inner = deeper(node);
    let mut col = div().flex().flex_col().gap(space::S12).min_w(px(0.));
    for id in children {
        if let Some(b) = child(node, id) {
            col = col.child(render_node(&inner, b, window, cx));
        }
    }
    col.into_any_element()
}

/// Side by side in the weight ratio, with a hairline between; a stack when narrow.
fn row(
    node: &Node,
    b: &CardV2Block,
    children: &[String],
    weights: Option<&[u8]>,
    window: &mut Window,
    cx: &mut App,
) -> AnyElement {
    // Metrics stay one band of numbers; any other row stacks when narrow.
    let metrics_only = children
        .iter()
        .all(|id| matches!(child(node, id), Some(CardV2Block::Metric { .. })));
    if node.narrow && !metrics_only {
        return stack(node, children, window, cx);
    }
    let k = theme::colors(cx);
    let inner = deeper(node);
    let mut line = div().flex().items_start().min_w(px(0.));
    let mut first = true;
    // Metrics in a row read as one band of numbers: no hairlines between them.
    let all_metrics = children
        .iter()
        .all(|id| matches!(child(node, id), Some(CardV2Block::Metric { .. })));
    for (i, id) in children.iter().enumerate() {
        let Some(c) = child(node, id) else { continue };
        let w = weights
            .and_then(|w| w.get(i).copied())
            .unwrap_or(1)
            .clamp(1, 3) as f32;
        if !first {
            line = line.child(if all_metrics {
                div().flex_none().w(space::S16)
            } else {
                div()
                    .flex_none()
                    .mx(space::S16)
                    .w(px(1.))
                    .self_stretch()
                    .bg(k.line)
            });
        }
        first = false;
        line = line.child(
            div()
                .id(node.ctx.id(format!("row-{}-{i}", b.id().unwrap_or(""))))
                .flex_grow(w)
                .flex_shrink(1.)
                .flex_basis(px(0.))
                .min_w(px(0.))
                .child(render_node(&inner, c, window, cx)),
        );
    }
    line.into_any_element()
}

/// A titled section; the collapse is local per viewer after the first render.
fn group(
    node: &Node,
    b: &CardV2Block,
    children: &[String],
    window: &mut Window,
    cx: &mut App,
) -> AnyElement {
    let CardV2Block::Group {
        id,
        title,
        label,
        summary,
        collapsible,
        collapsed,
        ..
    } = b
    else {
        return div().into_any_element();
    };
    let k = theme::colors(cx);
    let can_collapse = collapsible.unwrap_or(true);
    let closed = can_collapse
        && node
            .ui
            .block(id)
            .collapsed
            .unwrap_or(collapsed.unwrap_or(false));
    let title = title.clone().or_else(|| label.clone()).unwrap_or_default();
    let ui = node.ctx.ui.clone();
    let gid = id.clone();
    let head = div()
        .id(node.ctx.id(format!("group-{id}")))
        .flex()
        .items_center()
        .gap(space::S8)
        .min_w(px(0.))
        .h(crate::ui::native::tokens::height::SM)
        .when(can_collapse, |el| {
            el.on_click(move |_, _, cx| {
                ui.update(cx, |u, cx| {
                    let b = u.block_mut(&gid);
                    b.collapsed = Some(!closed);
                    cx.notify();
                })
            })
        })
        .when(can_collapse, |el| {
            el.child(ui::icon(
                if closed {
                    Sym::ChevronRight
                } else {
                    Sym::ChevronDown
                },
                px(14.),
                k.ink_3,
            ))
        })
        .child(
            b20::small(div())
                .flex_none()
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(k.ink)
                .child(title),
        )
        .when_some(summary.clone().filter(|_| !closed), |el, s| {
            el.child(
                meta(div())
                    .min_w(px(0.))
                    .one_line()
                    .text_color(k.ink_3)
                    .child(crate::ui::truncate::TruncText::new(s)),
            )
        })
        .when_some(summary.clone().filter(|_| closed), |el, s| {
            el.child(
                meta(div())
                    .ml_auto()
                    .min_w(px(0.))
                    .one_line()
                    .text_color(k.ink_2)
                    .child(crate::ui::truncate::TruncText::new(s)),
            )
        });
    let mut col = div().flex().flex_col().min_w(px(0.)).child(head);
    if !closed {
        col = col.child(
            div()
                .mt(space::S6)
                .when(can_collapse, |el| el.pl(space::S20))
                .child(stack(node, children, window, cx)),
        );
    }
    col.into_any_element()
}

/// A segmented control and one pane; the tab choice is local. The first open of a tab
/// sends `tab.open` when declared.
fn tabs(node: &Node, b: &CardV2Block, window: &mut Window, cx: &mut App) -> AnyElement {
    let CardV2Block::Tabs {
        id, tabs, selected, ..
    } = b
    else {
        return div().into_any_element();
    };
    let k = theme::colors(cx);
    let current = node
        .ui
        .block(id)
        .tab
        .or_else(|| selected.clone())
        .filter(|t| tabs.iter().any(|x| &x.id == t))
        .or_else(|| tabs.first().map(|t| t.id.clone()))
        .unwrap_or_default();
    let bar = div()
        .flex()
        .flex_none()
        .p(px(2.))
        .gap(crate::ui::native::tokens::space::S2)
        .rounded(theme::radius::MD)
        .bg(k.sunken)
        .children(tabs.iter().map(|t| {
            let on = t.id == current;
            let ui = node.ctx.ui.clone();
            let emit = interact::Emitter::of(node.ctx);
            let (bid, tid) = (id.clone(), t.id.clone());
            let declared = b
                .base()
                .is_some_and(|x| x.events().iter().any(|e| e == "tab.open"));
            div()
                .id(node.ctx.id(format!("tab-{id}-{}", t.id)))
                .h(px(26.))
                .px(space::S10)
                .rounded(crate::ui::native::tokens::radius::ITEM)
                .flex()
                .items_center()
                .text_size(crate::ui::native::tokens::text::UI.0)
                .text_color(if on { k.ink } else { k.ink_2 })
                .when(on, |el| {
                    el.bg(k.bg)
                        .font_weight(theme::MEDIUM_WEIGHT)
                        .shadow(k.sec_hi())
                })
                .when(!on, |el| el.hover(|s| s.text_color(k.ink)))
                .on_click(move |_, _, cx| {
                    let first = ui.update(cx, |u, cx| {
                        let b = u.block_mut(&bid);
                        b.tab = Some(tid.clone());
                        cx.notify();
                        b.opened.insert(tid.clone())
                    });
                    if first && declared {
                        emit.send(
                            &bid,
                            "tab.open",
                            serde_json::json!({ "tabId": tid }),
                            None,
                            cx,
                        );
                    }
                })
                .child(t.label.clone())
        }));
    let pane = tabs
        .iter()
        .find(|t| t.id == current)
        .map(|t| stack(node, &t.children, window, cx));
    div()
        .flex()
        .flex_col()
        .gap(space::S12)
        .child(div().flex().child(bar))
        .children(pane)
        .into_any_element()
}

/// Numbered questions, the kind and 必答 at the right, one submit for the card (the footer).
fn questionnaire(node: &Node, b: &CardV2Block, window: &mut Window, cx: &mut App) -> AnyElement {
    let CardV2Block::Questionnaire {
        children,
        anonymous,
        ..
    } = b
    else {
        return div().into_any_element();
    };
    let k = theme::colors(cx);
    let inner = deeper(node);
    let questions: Vec<&CardV2Block> = children.iter().filter_map(|id| child(node, id)).collect();
    let inputs: Vec<&&CardV2Block> = questions.iter().filter(|q| q.takes_input()).collect();
    let required: Vec<&&&CardV2Block> = inputs.iter().filter(|q| leaves::is_required(q)).collect();
    let done = required
        .iter()
        .filter(|q| {
            form21::problem(node.ctx.card, q, &node.ctx.values).is_none()
                && q.id().is_some_and(|id| node.ctx.values.contains_key(id))
        })
        .count();
    let live = node.ctx.live();
    let mut col = div().flex().flex_col().min_w(px(0.));
    if live {
        // 问卷 · 3 题 with a determinate bar of the answered questions and 必答 1/2
        // (NATIVE.md §5.7: one progress drawing, no dashes).
        let answered = inputs
            .iter()
            .filter(|q| {
                q.id().is_some_and(|id| node.ctx.values.contains_key(id))
                    && form21::problem(node.ctx.card, q, &node.ctx.values).is_none()
            })
            .count();
        let dashes = div().w(px(64.)).child(crate::ui::native::determinate_bar(
            answered as f32 / inputs.len().max(1) as f32,
            None,
            cx,
        ));
        col = col.child(
            meta(div())
                .mb(space::S8)
                .flex()
                .items_center()
                .gap(space::S8)
                .text_color(k.ink_3)
                .child(fill(
                    c::QUESTIONNAIRE_HEAD,
                    &[("n", &inputs.len().to_string())],
                ))
                .child(div().ml_auto().child(dashes))
                .child(fill(
                    c::QUESTIONNAIRE_REQUIRED,
                    &[
                        ("done", &done.to_string()),
                        ("total", &required.len().to_string()),
                    ],
                )),
        );
    }
    let mut n = 0;
    for (i, q) in questions.iter().enumerate() {
        let numbered = q.takes_input();
        if numbered {
            n += 1;
        }
        let head = numbered.then(|| {
            div()
                .flex()
                .items_baseline()
                .gap(space::S8)
                .min_w(px(0.))
                .child(
                    meta(div())
                        .flex_none()
                        .mono()
                        .text_color(k.ink_3)
                        .child(n.to_string()),
                )
                .child(
                    b20::small(div())
                        .min_w(px(0.))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(k.ink)
                        .child(leaves::question_label(q)),
                )
                .child(
                    meta(div())
                        .ml_auto()
                        .flex_none()
                        .text_color(k.ink_3)
                        .child(leaves::question_kind(q)),
                )
        });
        let show_body = live || !numbered;
        col = col
            .when(i > 0 && live, |el| {
                el.child(
                    div()
                        .my(space::S14)
                        .h(crate::ui::native::tokens::layout::BORDER)
                        .bg(k.line),
                )
            })
            .when(i > 0 && !live, |el| el.child(div().h(space::S6)))
            .children(head)
            .when(show_body, |el| {
                el.child(
                    div()
                        .when(numbered, |el| el.mt(space::S8))
                        .child(render_node(&inner, q, window, cx)),
                )
            });
    }
    if anonymous.unwrap_or(false) && live {
        col = col.child(
            meta(div())
                .mt(space::S10)
                .text_color(k.ink_3)
                .child(fill(c::Q_ANONYMOUS, &[("name", &node.ctx.author)])),
        );
    }
    col.into_any_element()
}

/// The id of an element of a block.
pub(crate) fn eid(node: &Node, part: impl std::fmt::Display) -> SharedString {
    node.ctx.id(part)
}

/// A 13/20 semibold question line (a choice's `label`).
pub(crate) fn question(text: &str, cx: &App) -> Div {
    let k = theme::colors(cx);
    b20::small(div())
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(k.ink)
        .child(text.to_string())
}

#[cfg(test)]
mod ui_tests {
    use super::*;
    use gpui::AppContext as _;

    fn card(rev: u32, note: &str, other: &str) -> Arc<CardV2> {
        Arc::new(
            serde_json::from_value(serde_json::json!({
                "v": 2, "catalog": "tc.card/2.1", "rev": rev, "intent": "info", "title": "复盘",
                "fallback": "f",
                "blocks": [
                    { "id": "a", "type": "input", "kind": "text", "label": "看法", "default": note,
                      "events": ["input.change"] },
                    { "id": "b", "type": "input", "kind": "text", "label": "补充", "default": other,
                      "events": ["input.change"] },
                    { "id": "t", "type": "tabs", "tabs": [ { "id": "x", "label": "一", "children": [] },
                                                           { "id": "y", "label": "二", "children": [] } ] }
                ]
            }))
            .unwrap(),
        )
    }

    #[test]
    fn a_rev_that_changes_a_text_input_sends_no_input_change() {
        let mut ui = CardUi::new(Some(card(1, "旧", "补")));
        ui.block_mut("a").tab = Some("旧".into());
        ui.block_mut("b").tab = Some("补".into());
        ui.block_mut("t").tab = Some("y".into());
        ui.observe(&card(2, "新", "补"), None);
        // The changed input forgets the last text it saw, so the draw that shows the new
        // text only stores it (text_change(None, ..) sends nothing).
        assert_eq!(ui.block("a").tab, None);
        assert_eq!(ui.block("b").tab.as_deref(), Some("补"), "untouched block");
        assert_eq!(
            ui.block("t").tab.as_deref(),
            Some("y"),
            "the open tab stays"
        );
        // 用新的 sets the text without a keystroke too.
        ui.block_mut("b").tab = Some("我写的".into());
        ui.drafts_kept.insert("b".into());
        ui.use_new("b");
        assert_eq!(ui.block("b").tab, None);
        assert!(!ui.drafts_kept.contains("b"));
    }

    #[test]
    fn a_kept_draft_stays_marked_when_a_later_rev_changes_another_block() {
        let mut ui = CardUi::new(None);
        // Rev 2 changed block a while I had typed in it.
        ui.keep_drafts(vec!["a".into()], &card(2, "新", "补"), |_| true);
        // Rev 3 changes only block b (untouched by me): a keeps its 内容变了.
        ui.keep_drafts(Vec::new(), &card(3, "新", "补二"), |id| id == "a");
        assert!(ui.drafts_kept.contains("a"));
        // Once a's text is gone (用新的, or the block went), the mark goes.
        ui.keep_drafts(Vec::new(), &card(4, "新", "补二"), |_| false);
        assert!(ui.drafts_kept.is_empty());
    }

    #[gpui::test]
    fn a_removal_starts_one_fade_timer_however_often_it_draws(cx: &mut gpui::TestAppContext) {
        let ui = cx.update(|cx| cx.new(|_| CardUi::new(None)));
        cx.update(|cx| {
            assert!(schedule_fade_end(&ui, 2, cx), "the first frame of the fade");
            for _ in 0..30 {
                assert!(!schedule_fade_end(&ui, 2, cx), "later frames start none");
            }
            assert!(
                schedule_fade_end(&ui, 3, cx),
                "the next rev's removal has its own"
            );
        });
    }
}
