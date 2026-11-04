//! Keyboard part focus inside a card block (REFS §A3): Tab moves focus into a card's blocks;
//! arrow keys move a part focus ring inside the focused block (table: cells; ⇧ + arrows: the
//! row or the column; chart: points; list and plan: items and steps); ⌥↵ adds the focused
//! part (the whole block when no part has focus), ⇧⌥↵ the whole block; Esc leaves the block.
//!
//! A renderer wraps a block (or a run of blocks drawn together) in [`focus_block`] with the
//! block's parts as [`NavPart`]s. The parts keep their own element ids, so the affordances
//! ([`super::affordance::part`], the table cells) draw the 2 pt `ink-3` ring of the focused
//! part with [`part_focused`].

use std::{cell::RefCell, collections::HashMap, rc::Rc};

use gpui::{
    AnyElement, App, ElementId, FocusHandle, InteractiveElement as _, IntoElement as _,
    KeyDownEvent, ParentElement as _, Styled as _, Window, div, px,
};

use super::{PartTarget, RefIntent, ask_about, hub};
use crate::theme;

/// What a part of a block stands for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NavKind {
    /// A cell, an item, a step, a point, a metric, a proof.
    Part,
    /// A table row (⇧ + ← or →).
    Row,
    /// A table column (⇧ + ↑ or ↓).
    Column,
}

/// One part of a block for the keyboard.
#[derive(Clone)]
pub struct NavPart {
    /// The part's element id (the affordance's id).
    pub id: String,
    pub row: usize,
    pub col: usize,
    pub kind: NavKind,
    pub target: Rc<dyn Fn(&App) -> PartTarget>,
}

/// The place of one part: what [`next`] moves over.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Place {
    pub row: usize,
    pub col: usize,
    pub kind: NavKind,
}

/// A movement key.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Move {
    Left,
    Right,
    Up,
    Down,
}

impl Move {
    fn of(key: &str) -> Option<Self> {
        Some(match key {
            "left" => Move::Left,
            "right" => Move::Right,
            "up" => Move::Up,
            "down" => Move::Down,
            _ => return None,
        })
    }
}

/// The part focus after `m` (with ⇧ when `shift`) from `current`, as an index into
/// `places`; `None` when nothing has focus or can take it.
pub fn next(places: &[Place], current: Option<usize>, m: Move, shift: bool) -> Option<usize> {
    let parts = || {
        places
            .iter()
            .enumerate()
            .filter(|(_, p)| p.kind == NavKind::Part)
    };
    let Some(cur) = current.and_then(|i| places.get(i).map(|p| (i, *p))) else {
        // Nothing has focus: the first part, or the last one going back.
        return match m {
            Move::Right | Move::Down => parts().map(|(i, _)| i).next(),
            Move::Left | Move::Up => parts().map(|(i, _)| i).next_back(),
        };
    };
    let (i, p) = cur;
    if shift {
        // The whole row or the whole column of the focused part.
        let want = match m {
            Move::Left | Move::Right => (NavKind::Row, p.row, None),
            Move::Up | Move::Down => (NavKind::Column, p.col, Some(())),
        };
        let found = places.iter().position(|q| {
            q.kind == want.0
                && if want.2.is_some() {
                    q.col == want.1
                } else {
                    q.row == want.1
                }
        });
        return Some(found.unwrap_or(i));
    }
    if p.kind != NavKind::Part {
        // From a row or a column back to its first cell.
        let back = parts().find(|(_, q)| match p.kind {
            NavKind::Row => q.row == p.row,
            _ => q.col == p.col,
        });
        return Some(back.map_or(i, |(j, _)| j));
    }
    let found = match m {
        Move::Right => parts()
            .filter(|(_, q)| q.row == p.row && q.col > p.col)
            .min_by_key(|(_, q)| q.col)
            .or_else(|| {
                // Past the end of the row: the first part of the next row.
                parts()
                    .filter(|(_, q)| q.row > p.row)
                    .min_by_key(|(_, q)| (q.row, q.col))
            }),
        Move::Left => parts()
            .filter(|(_, q)| q.row == p.row && q.col < p.col)
            .max_by_key(|(_, q)| q.col)
            .or_else(|| {
                parts()
                    .filter(|(_, q)| q.row < p.row)
                    .max_by_key(|(_, q)| (q.row, q.col))
            }),
        Move::Down => {
            let row = parts().map(|(_, q)| q.row).filter(|r| *r > p.row).min();
            row.and_then(|r| {
                parts()
                    .filter(|(_, q)| q.row == r)
                    .min_by_key(|(_, q)| q.col.abs_diff(p.col))
            })
        }
        Move::Up => {
            let row = parts().map(|(_, q)| q.row).filter(|r| *r < p.row).max();
            row.and_then(|r| {
                parts()
                    .filter(|(_, q)| q.row == r)
                    .min_by_key(|(_, q)| q.col.abs_diff(p.col))
            })
        }
    };
    Some(found.map_or(i, |(j, _)| j))
}

thread_local! {
    /// One focus handle per block key, kept while the app runs.
    static HANDLES: RefCell<HashMap<String, FocusHandle>> = RefCell::new(HashMap::new());
}

/// Most block handles kept; older ones are dropped (their blocks scrolled away long ago).
const MAX_HANDLES: usize = 4096;

fn handle(key: &str, cx: &App) -> FocusHandle {
    HANDLES.with(|h| {
        let mut h = h.borrow_mut();
        if let Some(f) = h.get(key) {
            return f.clone();
        }
        if h.len() >= MAX_HANDLES {
            h.clear();
        }
        let f = cx.focus_handle().tab_stop(true);
        h.insert(key.to_string(), f.clone());
        f
    })
}

/// The block `key` has keyboard focus (or a scene says so).
fn block_focused(key: &str, window: &Window, cx: &App) -> bool {
    let forced = cx
        .try_global::<super::HubGlobal>()
        .is_some_and(|h| h.0.read(cx).forced_focus.as_deref() == Some(key));
    forced || HANDLES.with(|h| h.borrow().get(key).is_some_and(|f| f.is_focused(window)))
}

/// The part `id` has the keyboard part focus (its block has focus too).
pub fn part_focused(id: &str, window: &Window, cx: &App) -> bool {
    let Some(h) = cx.try_global::<super::HubGlobal>() else {
        return false;
    };
    match &h.0.read(cx).part_focus {
        Some((block, part)) => part == id && block_focused(block, window, cx),
        None => false,
    }
}

/// The 2 pt `ink-3` focus ring over a part (outside it, so nothing moves).
pub fn ring(radius: gpui::Pixels, cx: &App) -> gpui::Div {
    let k = theme::colors(cx);
    div()
        .absolute()
        .top(px(-3.))
        .bottom(px(-3.))
        .left(px(-3.))
        .right(px(-3.))
        .rounded(radius + px(3.))
        .border_2()
        .border_color(k.ink_3)
}

/// The focused part of block `key`, as an index into `parts`.
fn current(key: &str, parts: &[NavPart], cx: &App) -> Option<usize> {
    let h = cx.try_global::<super::HubGlobal>()?.0.read(cx);
    let (block, id) = h.part_focus.as_ref()?;
    (block == key)
        .then(|| parts.iter().position(|p| &p.id == id))
        .flatten()
}

fn on_key(
    key: &str,
    parts: &[NavPart],
    whole: &Rc<dyn Fn(&App) -> PartTarget>,
    e: &KeyDownEvent,
    window: &mut Window,
    cx: &mut App,
) {
    let k = &e.keystroke;
    let mods = k.modifiers;
    if mods.platform || mods.control {
        return;
    }
    if let Some(m) = Move::of(k.key.as_str()).filter(|_| !mods.alt) {
        let places: Vec<Place> = parts
            .iter()
            .map(|p| Place {
                row: p.row,
                col: p.col,
                kind: p.kind,
            })
            .collect();
        let cur = current(key, parts, cx);
        if let Some(i) = next(&places, cur, m, mods.shift) {
            let id = parts[i].id.clone();
            let key = key.to_string();
            hub(cx).update(cx, |h, cx| {
                h.part_focus = Some((key, id));
                cx.notify();
            });
            window.refresh();
        }
        cx.stop_propagation();
        return;
    }
    match k.key.as_str() {
        "enter" if mods.alt => {
            let target = match (mods.shift, current(key, parts, cx)) {
                (false, Some(i)) => (parts[i].target)(cx),
                _ => whole(cx),
            };
            cx.stop_propagation();
            ask_about(target, RefIntent::Topic, window, cx);
        }
        "escape" => {
            hub(cx).update(cx, |h, cx| {
                if h.part_focus.as_ref().is_some_and(|(b, _)| b == key) {
                    h.part_focus = None;
                }
                cx.notify();
            });
            window.blur(cx);
            cx.stop_propagation();
        }
        _ => {}
    }
}

/// `el` (a block, or a run of blocks drawn together) as one keyboard stop with part focus.
/// `key` is unique among the visible blocks; `whole` is the block's reference for ⇧⌥↵.
/// Returns `el` unchanged when the server does not take references.
pub fn focus_block(
    el: AnyElement,
    key: String,
    parts: Vec<NavPart>,
    whole: Rc<dyn Fn(&App) -> PartTarget>,
    window: &Window,
    cx: &App,
) -> AnyElement {
    if !super::enabled(cx) {
        return el;
    }
    let f = handle(&key, cx);
    // The block's own ring: keyboard focus with no part focused yet.
    // A click focuses the block too; the ring shows only after a key (or in a scene).
    let own_ring = block_focused(&key, window, cx)
        && (window.last_input_was_keyboard() || !f.is_focused(window))
        && current(&key, &parts, cx).is_none();
    let parts = Rc::new(parts);
    let k = key.clone();
    div()
        .id(ElementId::Name(format!("kb-{key}").into()))
        .relative()
        .min_w(px(0.))
        .track_focus(&f)
        .on_key_down(move |e: &KeyDownEvent, window, cx| on_key(&k, &parts, &whole, e, window, cx))
        .child(el)
        .children(own_ring.then(|| ring(theme::radius::MD, cx)))
        .into_any_element()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn grid(rows: usize, cols: usize) -> Vec<Place> {
        let mut out: Vec<Place> = (0..rows)
            .flat_map(|r| {
                (0..cols).map(move |c| Place {
                    row: r,
                    col: c,
                    kind: NavKind::Part,
                })
            })
            .collect();
        out.extend((0..rows).map(|r| Place {
            row: r,
            col: 0,
            kind: NavKind::Row,
        }));
        out.extend((0..cols).map(|c| Place {
            row: 0,
            col: c,
            kind: NavKind::Column,
        }));
        out
    }

    fn list(n: usize) -> Vec<Place> {
        (0..n)
            .map(|i| Place {
                row: i,
                col: 0,
                kind: NavKind::Part,
            })
            .collect()
    }

    #[test]
    fn the_first_arrow_focuses_the_first_or_the_last_part() {
        let l = list(3);
        assert_eq!(next(&l, None, Move::Down, false), Some(0));
        assert_eq!(next(&l, None, Move::Up, false), Some(2));
        assert_eq!(next(&[], None, Move::Down, false), None);
    }

    #[test]
    fn lists_move_by_item_and_stop_at_the_ends() {
        let l = list(3);
        assert_eq!(next(&l, Some(0), Move::Down, false), Some(1));
        assert_eq!(next(&l, Some(1), Move::Right, false), Some(2));
        assert_eq!(next(&l, Some(2), Move::Down, false), Some(2));
        assert_eq!(next(&l, Some(0), Move::Up, false), Some(0));
        assert_eq!(next(&l, Some(2), Move::Left, false), Some(1));
    }

    #[test]
    fn tables_move_by_cell_and_shift_takes_the_row_or_column() {
        let g = grid(3, 4);
        // (1, 1) is index 5.
        assert_eq!(next(&g, Some(5), Move::Right, false), Some(6));
        assert_eq!(next(&g, Some(5), Move::Down, false), Some(9));
        assert_eq!(next(&g, Some(5), Move::Up, false), Some(1));
        // The end of a row goes on to the next row.
        assert_eq!(next(&g, Some(3), Move::Right, false), Some(4));
        // ⇧ + → is row 1, ⇧ + ↓ column 1.
        let row = next(&g, Some(5), Move::Right, true).unwrap();
        assert_eq!(
            g[row],
            Place {
                row: 1,
                col: 0,
                kind: NavKind::Row
            }
        );
        let col = next(&g, Some(5), Move::Down, true).unwrap();
        assert_eq!(
            g[col],
            Place {
                row: 0,
                col: 1,
                kind: NavKind::Column
            }
        );
        // A plain arrow from the row goes back to its first cell.
        assert_eq!(next(&g, Some(row), Move::Right, false), Some(4));
    }

    #[test]
    fn shift_without_rows_keeps_the_focus() {
        let l = list(2);
        assert_eq!(next(&l, Some(1), Move::Left, true), Some(1));
    }
}
