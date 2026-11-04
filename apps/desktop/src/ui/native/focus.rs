//! The focusable row (COMPLETENESS.md §2.6, E3; review 2 R1-01, R1-02): the
//! one keyboard and hover recipe for every row-like control: sidebar rows,
//! page rows, table rows, grid tiles, inspector rows, card options, picker
//! rows and chips.
//!
//! A row built on it:
//! - is a tab stop (Tab and Shift-Tab, `app.rs`), and Enter or Space runs
//!   its `on_click` (gpui fires the click of a focused element);
//! - moves the focus to the next or previous row with ↓ and ↑ (and → / ←
//!   in a grid), so a list is walked with the arrow keys;
//! - shows keyboard focus as a 3 px ring at 50 % with a 1 px `ring` line
//!   inside the row edge: a line, never a fill, so a focused row never looks
//!   like the selected row (muted fill);
//! - draws hover at once as muted at 50 % on a row that is not selected.
//!
//! A pointer click focuses the row too, but the ring shows only after a key
//! press (`focus_visible`), as on macOS.
//!
//! ```ignore
//! div().id("row").focus_row(selected, cx).on_click(...)
//! ```

use gpui::{
    App, BoxShadow, Div, InteractiveElement as _, KeyDownEvent, Stateful, Styled as _, Window,
    point, prelude::FluentBuilder as _, px,
};

use super::{metrics, native_colors};

/// How the arrow keys move the focus from a row.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Arrows {
    /// ↓ / ↑ move to the next / previous row (a list).
    #[default]
    Vertical,
    /// → / ↓ next, ← / ↑ previous (a grid of tiles, a row of chips).
    Grid,
    /// The owner handles the arrow keys (a list that moves a selection).
    Owner,
}

/// The keyboard focus look of a row: a 1 px `ring` line and the 3 px ring
/// at 50 % (§1.8), both inside the edge so a row in a clipped frame keeps
/// all of it and its layout does not move.
pub fn row_focus_ring(cx: &App) -> Vec<BoxShadow> {
    let k = native_colors(cx);
    let line = crate::ui::native::tokens::Palette::for_mode(k.is_dark()).ring;
    vec![
        BoxShadow {
            color: line,
            offset: point(px(0.), px(0.)),
            blur_radius: px(0.),
            spread_radius: metrics::HAIRLINE,
            inset: true,
        },
        BoxShadow {
            color: k.focus,
            offset: point(px(0.), px(0.)),
            blur_radius: px(0.),
            spread_radius: metrics::FOCUS_RING,
            inset: true,
        },
    ]
}

/// The row recipe on a stateful element.
pub trait FocusRow: Sized {
    /// Tab stop, Enter / Space, ↑ / ↓, the focus ring, and hover muted 50 %
    /// when not `selected`. The selected fill is the caller's.
    fn focus_row(self, selected: bool, cx: &App) -> Self {
        self.focus_row_with(selected, Arrows::Vertical, cx)
    }

    /// [`FocusRow::focus_row`] with the arrow behavior named.
    fn focus_row_with(self, selected: bool, arrows: Arrows, cx: &App) -> Self;

    /// Hover only (muted 50 %, at once) for a row that is not a control of
    /// its own.
    fn row_hover(self, selected: bool, cx: &App) -> Self;
}

impl FocusRow for Stateful<Div> {
    fn focus_row_with(self, selected: bool, arrows: Arrows, cx: &App) -> Self {
        let ring = row_focus_ring(cx);
        self.row_hover(selected, cx)
            .tab_index(0)
            .focus_visible(move |s| s.shadow(ring))
            .when(arrows != Arrows::Owner, |d| {
                d.on_key_down(move |e: &KeyDownEvent, window, cx| {
                    if let Some(next) = step(e, arrows) {
                        move_focus(next, window, cx);
                        cx.stop_propagation();
                    }
                })
            })
    }

    fn row_hover(self, selected: bool, cx: &App) -> Self {
        let hover = native_colors(cx).fill.opacity(0.5);
        self.when(!selected, |d| d.hover(move |s| s.bg(hover)))
    }
}

/// `Some(true)` for the next row, `Some(false)` for the previous one.
fn step(e: &KeyDownEvent, arrows: Arrows) -> Option<bool> {
    if e.keystroke.modifiers.modified() {
        return None;
    }
    match (e.keystroke.key.as_str(), arrows) {
        ("down", Arrows::Vertical | Arrows::Grid) | ("right", Arrows::Grid) => Some(true),
        ("up", Arrows::Vertical | Arrows::Grid) | ("left", Arrows::Grid) => Some(false),
        _ => None,
    }
}

fn move_focus(next: bool, window: &mut Window, cx: &mut App) {
    if next {
        window.focus_next(cx);
    } else {
        window.focus_prev(cx);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::Keystroke;

    fn key(k: &'static str) -> KeyDownEvent {
        KeyDownEvent {
            keystroke: Keystroke::parse(k).unwrap(),
            is_held: false,
            prefer_character_input: false,
        }
    }

    #[test]
    fn arrows_walk_a_list_and_a_grid() {
        assert_eq!(step(&key("down"), Arrows::Vertical), Some(true));
        assert_eq!(step(&key("up"), Arrows::Vertical), Some(false));
        assert_eq!(step(&key("right"), Arrows::Vertical), None);
        assert_eq!(step(&key("right"), Arrows::Grid), Some(true));
        assert_eq!(step(&key("left"), Arrows::Grid), Some(false));
        assert_eq!(step(&key("down"), Arrows::Owner), None);
        // ⌘↓ and ⇧↓ belong to the owner (scroll, extend a selection).
        assert_eq!(step(&key("cmd-down"), Arrows::Vertical), None);
        assert_eq!(step(&key("shift-down"), Arrows::Vertical), None);
    }

    struct List {
        clicks: std::rc::Rc<std::cell::RefCell<Vec<usize>>>,
    }

    impl gpui::Render for List {
        fn render(
            &mut self,
            _: &mut Window,
            cx: &mut gpui::Context<Self>,
        ) -> impl gpui::IntoElement {
            use gpui::{ParentElement as _, StatefulInteractiveElement as _};
            gpui::div().children((0..3).map(|i| {
                let clicks = self.clicks.clone();
                gpui::div()
                    .id(("row", i))
                    .h(px(32.))
                    .focus_row(i == 0, cx)
                    .on_click(move |_, _, _| clicks.borrow_mut().push(i))
            }))
        }
    }

    /// Tab reaches the first row, ↓ moves to the next, Enter and Space run
    /// the focused row's click, ↑ goes back (R1-01).
    #[gpui::test]
    fn tab_arrows_and_enter_drive_the_rows(cx: &mut gpui::TestAppContext) {
        cx.update(gpui_component::init);
        let clicks = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
        let (_, cx) = cx.add_window_view({
            let clicks = clicks.clone();
            move |_, _| List { clicks }
        });
        cx.update(|window, cx| {
            window.draw(cx).clear(cx);
            window.focus_next(cx);
            assert!(window.focused(cx).is_some(), "Tab focuses the first row");
        });
        // A click needs the key to go down and up on the focused row.
        let press = |cx: &mut gpui::VisualTestContext, key: &'static str| {
            let keystroke = gpui::Keystroke::parse(key).unwrap();
            cx.simulate_event(KeyDownEvent {
                keystroke: keystroke.clone(),
                is_held: false,
                prefer_character_input: false,
            });
            cx.simulate_event(gpui::KeyUpEvent { keystroke });
            cx.run_until_parked();
        };
        press(cx, "enter");
        press(cx, "down");
        press(cx, "down");
        press(cx, "space");
        press(cx, "up");
        press(cx, "enter");
        assert_eq!(*clicks.borrow(), vec![0, 2, 1]);
    }
}
