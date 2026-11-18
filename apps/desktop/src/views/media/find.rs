//! Find in a document of the viewer sheet (⌘F), with the find bar of a 话题
//! (`timeline::find`): a field, `n / m`, the previous and the next match and
//! close. A Markdown document marks every match in its text; a code file and
//! a table mark the lines and rows that hold one. The view scrolls to the
//! current match. Matching is the timeline's: case-insensitive, by characters.

use std::{cell::Cell, ops::Range, rc::Rc};

use gpui::{
    App, AppContext as _, Context, Entity, InteractiveElement as _, IntoElement as _,
    ParentElement as _, SharedString, Styled as _, Subscription, Window, div,
};
use gpui_component::input::{InputEvent, InputState};

use super::state::MediaState;
use crate::{
    copy::{fill, timeline as t},
    theme::{Type as _, space},
    ui::Sym,
    views::timeline::find::match_ranges,
};

/// The open find bar of one media view.
pub struct Find {
    pub input: Entity<InputState>,
    /// The current match, an index into the matches of the last frame.
    pub current: usize,
    /// How many matches the last frame drew (the step wraps at it).
    pub total: Rc<Cell<usize>>,
    /// The current match changed: the next frame scrolls to it.
    pub reveal: Rc<Cell<bool>>,
    _subscription: Subscription,
}

impl MediaState {
    /// Opens the find bar (or goes back to its field) with the text selected.
    pub fn open_find(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.find.is_none() {
            let input = cx.new(|cx| InputState::new(window, cx).placeholder(t::FIND_PLACEHOLDER));
            let subscription = cx.subscribe_in(
                &input,
                window,
                |this, _, event: &InputEvent, _, cx| match event {
                    InputEvent::Change => {
                        if let Some(find) = &mut this.find {
                            find.current = 0;
                            find.reveal.set(true);
                        }
                        cx.notify();
                    }
                    InputEvent::PressEnter { shift, .. } => this.step_find(!*shift, cx),
                    _ => {}
                },
            );
            self.find = Some(Find {
                input,
                current: 0,
                total: Rc::new(Cell::new(0)),
                reveal: Rc::new(Cell::new(false)),
                _subscription: subscription,
            });
        }
        if let Some(find) = &self.find {
            find.input.update(cx, |input, cx| {
                input.focus(window, cx);
                input.select_all(window, cx);
            });
        }
        cx.notify();
    }

    /// Scene: the find bar with `query` typed.
    pub fn find_with(&mut self, query: &str, window: &mut Window, cx: &mut Context<Self>) {
        self.open_find(window, cx);
        if let Some(find) = &self.find {
            find.input
                .update(cx, |input, cx| input.set_value(query, window, cx));
            find.reveal.set(true);
        }
    }

    pub fn close_find(&mut self, cx: &mut Context<Self>) {
        self.find = None;
        cx.notify();
    }

    /// The next (or previous) match, wrapping around.
    pub fn step_find(&mut self, forward: bool, cx: &mut Context<Self>) {
        let Some(find) = &mut self.find else {
            return;
        };
        if let Some(next) = step(find.current, find.total.get(), forward) {
            find.current = next;
            find.reveal.set(true);
            cx.notify();
        }
    }

    /// The typed words, when the bar is open and holds any.
    pub fn find_query(&self, cx: &App) -> Option<String> {
        let find = self.find.as_ref()?;
        let query = find.input.read(cx).value().to_string();
        (!query.is_empty()).then_some(query)
    }
}

/// The match after (or before) `current` of `total`, wrapping; `None` with no match.
pub fn step(current: usize, total: usize, forward: bool) -> Option<usize> {
    if total == 0 {
        return None;
    }
    let current = current.min(total - 1);
    Some(if forward {
        (current + 1) % total
    } else {
        (current + total - 1) % total
    })
}

/// The matches of `query` in each of `texts` (the parts of a document, the
/// lines of a file, the rows of a table as one text each).
pub fn matches_in<S: AsRef<str>>(texts: &[S], query: &str) -> Vec<Vec<Range<usize>>> {
    texts
        .iter()
        .map(|text| match_ranges(text.as_ref(), query))
        .collect()
}

/// Where match `current` (counted over all texts) is: the text's index and
/// the match's index in that text.
pub fn locate(matches: &[Vec<Range<usize>>], current: usize) -> Option<(usize, usize)> {
    let mut left = current;
    for (i, m) in matches.iter().enumerate() {
        if left < m.len() {
            return Some((i, left));
        }
        left -= m.len();
    }
    None
}

/// What one frame of a view found: the matches per text, the text that holds
/// the current match and that match's index in it.
pub struct Found {
    pub matches: Vec<Vec<Range<usize>>>,
    pub current: Option<(usize, usize)>,
    /// The current match moved since the last frame: scroll to it.
    pub reveal: bool,
}

impl Found {
    pub fn total(&self) -> usize {
        self.matches.iter().map(Vec::len).sum()
    }

    /// The fill behind text `i`: stronger for the one with the current match.
    pub fn line_fill(&self, i: usize, cx: &App) -> Option<gpui::Hsla> {
        use crate::ui::native::tokens::find;
        let palette = crate::theme::colors(cx).palette;
        if self.matches.get(i).is_none_or(Vec::is_empty) {
            return None;
        }
        Some(if self.current.map(|(at, _)| at) == Some(i) {
            find::current_fill(&palette)
        } else {
            find::match_fill(&palette)
        })
    }
}

/// The matches of the open find bar in `texts`, and the bookkeeping of the
/// frame: the total for the next step, and whether to scroll. `None` while
/// the bar is closed or empty.
pub fn found<S: AsRef<str>>(state: &Entity<MediaState>, texts: &[S], cx: &App) -> Option<Found> {
    let st = state.read(cx);
    let find = st.find.as_ref()?;
    let Some(query) = st.find_query(cx) else {
        find.total.set(0);
        return None;
    };
    let matches = matches_in(texts, &query);
    let total: usize = matches.iter().map(Vec::len).sum();
    find.total.set(total);
    let current = (total > 0)
        .then(|| locate(&matches, find.current.min(total - 1)))
        .flatten();
    Some(Found {
        matches,
        current,
        reveal: find.reveal.replace(false),
    })
}

/// The view could not scroll to the current match in this frame (nothing was
/// laid out yet): the next frame tries again.
pub fn reveal_again(state: &Entity<MediaState>, cx: &App) {
    if let Some(find) = &state.read(cx).find {
        find.reveal.set(true);
    }
}

/// The find bar under a view's header, when it is open: h40 with a line under
/// it, as in a 话题.
pub fn bar(id: &SharedString, state: &Entity<MediaState>, cx: &App) -> Option<gpui::AnyElement> {
    use crate::ui::{
        NamedButton as _,
        native::{IconSize, NativeType as _, SearchField, icon_button_bare, native_colors, tokens},
    };
    use gpui_component::Disableable as _;
    let st = state.read(cx);
    let find = st.find.as_ref()?;
    let n = native_colors(cx);
    let total = find.total.get();
    let count = st.find_query(cx).map(|_| {
        if total == 0 {
            t::FIND_NOT_FOUND.to_string()
        } else {
            fill(
                t::FIND_POSITION,
                &[
                    ("n", &(find.current.min(total - 1) + 1).to_string()),
                    ("total", &total.to_string()),
                ],
            )
        }
    });
    let button = |part: &str, sym: Sym, name: &'static str| {
        icon_button_bare(
            gpui::ElementId::Name(format!("{id}-find-{part}").into()),
            sym,
            IconSize::Regular,
        )
        .named(name)
    };
    let step = |part: &str, sym: Sym, name: &'static str, forward: bool| {
        let state = state.clone();
        button(part, sym, name)
            .disabled(total == 0)
            .when(total == 0, |b| b.opacity(0.5))
            .on_click(move |_, _, cx| state.update(cx, |s, cx| s.step_find(forward, cx)))
    };
    let (close, escape) = (state.clone(), state.clone());
    Some(
        div()
            .id(gpui::ElementId::Name(format!("{id}-find-bar").into()))
            .flex_none()
            .h(tokens::height::FOOTER_ROW)
            .pl(space::S16)
            .pr(space::S12)
            .border_b_1()
            .border_color(n.separator)
            .flex()
            .items_center()
            .gap(space::S8)
            // Escape closes the bar, not the sheet under it.
            .capture_key_down(move |e: &gpui::KeyDownEvent, _, cx| {
                if e.keystroke.key == "escape" && !e.keystroke.modifiers.modified() {
                    escape.update(cx, |s, cx| s.close_find(cx));
                    cx.stop_propagation();
                }
            })
            .child(
                SearchField::new(
                    gpui::ElementId::Name(format!("{id}-find-field").into()),
                    &find.input,
                )
                .small()
                .width(tokens::size::FIND_FIELD_WIDTH),
            )
            .children(count.map(|count| {
                div()
                    .flex_none()
                    .n_body()
                    .tnum()
                    .text_color(n.secondary)
                    .child(count)
            }))
            .child(div().flex_1())
            .child(step("previous", Sym::ChevronUp, t::FIND_PREVIOUS, false))
            .child(step("next", Sym::ChevronDown, t::FIND_NEXT, true))
            .child(
                button("close", Sym::Close, crate::copy::common::CLOSE)
                    .on_click(move |_, _, cx| close.update(cx, |s, cx| s.close_find(cx))),
            )
            .into_any_element(),
    )
}

use gpui::prelude::FluentBuilder as _;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn steps_wrap_and_stop_without_matches() {
        assert_eq!(step(0, 0, true), None);
        assert_eq!(step(0, 3, true), Some(1));
        assert_eq!(step(2, 3, true), Some(0));
        assert_eq!(step(0, 3, false), Some(2));
        // A current match past a shorter result list starts from its end.
        assert_eq!(step(9, 3, true), Some(0));
    }

    #[test]
    fn matches_are_counted_over_parts_and_located_in_one() {
        let parts = [
            "## 原因\n重连后从旧的 seq 拉取",
            "没有",
            "按 SEQ 去重，seq 只增",
        ];
        let m = matches_in(&parts, "seq");
        assert_eq!(m.iter().map(Vec::len).collect::<Vec<_>>(), [1, 0, 2]);
        assert_eq!(locate(&m, 0), Some((0, 0)));
        assert_eq!(locate(&m, 1), Some((2, 0)));
        assert_eq!(locate(&m, 2), Some((2, 1)));
        assert_eq!(locate(&m, 3), None);
        // CJK text matches as typed.
        assert_eq!(matches_in(&parts, "重连")[0].len(), 1);
        assert!(matches_in(&parts, "").iter().all(Vec::is_empty));
    }
}
