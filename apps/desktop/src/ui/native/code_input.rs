//! The one-time code input (COMPLETENESS.md §2.20; shadcn InputOTP): one
//! bordered slot per character, 40 x 44, radius 8, in groups with a dash
//! between them (an invite code is 4 – 4). The slot at the caret has the
//! focus ring; characters are upper case, centered, mono with tabular
//! figures. A paste fills every slot, Backspace moves back, the arrow keys
//! move between slots, a click puts the caret in that slot, and typing over
//! a filled slot replaces its character.
//!
//! gpui-component's `OtpInput` takes digits only, from single key presses,
//! with no paste, arrows or IME. So the slots are drawn here over a hidden
//! gpui-component `Input` that owns the text, the caret, the selection, the
//! clipboard and the IME; each edit is folded back into the slots.
//!
//! An owner can keep a pasted value whole (a login credential, an older code
//! format) with [`CodeInput::keep_raw`]; [`CodeInput::set_locked`] then
//! draws one locked line in place of the slots, and Backspace clears it.

use gpui::{
    App, AppContext as _, BoxShadow, Context, Entity, EventEmitter, FocusHandle, Focusable, Hsla,
    InteractiveElement as _, IntoElement, MouseButton, ParentElement, Pixels, Render, SharedString,
    Styled, Subscription, Window, div, prelude::FluentBuilder as _,
};
use gpui_component::{
    ActiveTheme as _,
    input::{Input, InputEvent, InputState},
};

use super::tokens::{NativeColors, NativeType as _, metrics, native_colors, shadow, space, text};
use crate::{
    theme::Type as _,
    ui::{Sym, sf_symbols::SymSize},
};

/// What a code input tells its owner.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CodeInputEvent {
    /// The code changed (typed, pasted, deleted).
    Change,
    /// Return was pressed.
    Submit,
}

/// The width of `slots` slots in groups of `group`.
pub fn code_input_width(slots: usize, group: usize) -> Pixels {
    let group = group.max(1);
    let groups = slots.div_ceil(group).max(1);
    let gaps = slots.saturating_sub(groups);
    metrics::CODE_SLOT.0 * slots as f32
        + metrics::CODE_SLOT_GAP * gaps as f32
        + metrics::CODE_GROUP_GAP * (groups - 1) as f32
}

/// The letters and digits of `text`, upper case: what the slots hold.
pub fn code_chars(text: &str) -> String {
    text.chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .map(|c| c.to_ascii_uppercase())
        .collect()
}

/// The slot characters in front of byte offset `cursor` of `raw`.
fn chars_before(raw: &str, cursor: usize) -> usize {
    raw.char_indices()
        .take_while(|(i, _)| *i < cursor)
        .filter(|(_, c)| c.is_ascii_alphanumeric())
        .count()
}

/// Folds one edit of the hidden field into the slots. `prev` is what the
/// slots held, `raw` the field text after the edit and `cursor` its caret
/// (a byte offset in `raw`). Returns the slot text (at most `slots`
/// characters) and its caret. One character typed over a filled slot
/// replaces it; anything else (a paste, a delete) keeps the order and drops
/// what does not fit.
pub fn fold_edit(prev: &str, raw: &str, cursor: usize, slots: usize) -> (String, usize) {
    let cursor = cursor.min(raw.len());
    let next: Vec<char> = code_chars(raw).chars().collect();
    let caret = chars_before(raw, cursor);
    let prev: Vec<char> = prev.chars().collect();
    // One character typed in front of a filled slot: overwrite it.
    if next.len() == prev.len() + 1 && caret >= 1 && caret <= prev.len() {
        let at = caret - 1;
        if next[..at] == prev[..at] && next[at + 1..] == prev[at..] {
            let mut out = prev.clone();
            out[at] = next[at];
            return (out.into_iter().collect(), caret);
        }
    }
    let out: String = next.into_iter().take(slots).collect();
    let caret = caret.min(out.len());
    (out, caret)
}

/// The (border, shadows) of a slot. The active slot is crisp: a foreground
/// border and a light 3 px foreground ring (the gray ring color at 50 % read
/// as a muddy halo on the login, owner review v18). An error draws the
/// destructive border on every slot and the destructive ring on the active
/// one.
fn slot_edge(k: &NativeColors, active: bool, error: bool) -> (Hsla, Vec<BoxShadow>) {
    let ring = |color: Hsla| vec![shadow::ring(color, metrics::FOCUS_RING)];
    match (error, active) {
        (true, true) => (k.red, ring(k.red.opacity(0.2))),
        (true, false) => (k.red, Vec::new()),
        (false, true) => (k.label, ring(k.label.opacity(metrics::CODE_RING_OPACITY))),
        (false, false) => (k.quaternary, shadow::xs()),
    }
}

pub struct CodeInput {
    state: Entity<InputState>,
    slots: usize,
    group: usize,
    error: bool,
    locked: Option<SharedString>,
    keep: fn(&str) -> bool,
    /// The slot text after the last edit.
    prev: String,
    /// The last edit was kept whole (see [`CodeInput::keep_raw`]).
    kept: bool,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<CodeInputEvent> for CodeInput {}

impl Focusable for CodeInput {
    fn focus_handle(&self, cx: &App) -> FocusHandle {
        self.state.read(cx).focus_handle(cx)
    }
}

impl CodeInput {
    /// `slots` characters in groups of `group` (8 and 4: `XXXX-XXXX`).
    pub fn new(slots: usize, group: usize, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let state = cx.new(|cx| InputState::new(window, cx));
        let subscriptions = vec![
            cx.subscribe_in(&state, window, |this, _, event: &InputEvent, window, cx| {
                match event {
                    InputEvent::PressEnter { .. } => cx.emit(CodeInputEvent::Submit),
                    InputEvent::Change => {
                        this.fold(window, cx);
                        cx.emit(CodeInputEvent::Change);
                    }
                    _ => {}
                }
                cx.notify();
            }),
            // The caret moved (arrows, a click): redraw the active slot.
            cx.observe(&state, |_, _, cx| cx.notify()),
        ];
        Self {
            state,
            slots,
            group: group.max(1),
            error: false,
            locked: None,
            keep: |_| false,
            prev: String::new(),
            kept: false,
            _subscriptions: subscriptions,
        }
    }

    /// Keeps a field text whole, unfolded, when `keep` says so (a pasted
    /// login credential, a code of an older length).
    pub fn keep_raw(mut self, keep: fn(&str) -> bool) -> Self {
        self.keep = keep;
        self
    }

    /// The text: the slot characters, or a value kept whole.
    pub fn value(&self, cx: &App) -> SharedString {
        self.state.read(cx).value()
    }

    /// Sets the code (folded into the slots unless kept whole), caret at the
    /// end.
    pub fn set_value(&mut self, value: &str, window: &mut Window, cx: &mut Context<Self>) {
        let value = value.trim();
        self.kept = (self.keep)(value);
        let text = if self.kept {
            value.to_string()
        } else {
            fold_edit("", value, value.len(), self.slots).0
        };
        self.prev = text.clone();
        self.state.update(cx, |st, cx| {
            st.set_value(text.clone(), window, cx);
            st.set_selected_range(text.len()..text.len(), cx);
        });
        cx.notify();
    }

    /// Selects every character: the next one typed starts a new code.
    pub fn select_all(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.state.update(cx, |st, cx| {
            st.focus(window, cx);
            st.select_all(window, cx);
        });
        cx.notify();
    }

    pub fn focus(&self, window: &mut Window, cx: &mut App) {
        self.state.read(cx).focus_handle(cx).focus(window, cx);
    }

    pub fn is_focused(&self, window: &Window, cx: &App) -> bool {
        self.state.read(cx).focus_handle(cx).is_focused(window)
    }

    /// Draws the error border on every slot.
    pub fn set_error(&mut self, error: bool, cx: &mut Context<Self>) {
        if self.error != error {
            self.error = error;
            cx.notify();
        }
    }

    /// Draws one locked line with `text` in place of the slots (a value
    /// kept whole is never drawn).
    pub fn set_locked(&mut self, text: Option<SharedString>, cx: &mut Context<Self>) {
        if self.locked != text {
            self.locked = text;
            cx.notify();
        }
    }

    /// Folds the field text into the slots after an edit.
    fn fold(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let (raw, cursor) = {
            let st = self.state.read(cx);
            (st.value().to_string(), st.cursor())
        };
        let was_kept = self.kept;
        self.kept = (self.keep)(raw.trim());
        if self.kept {
            self.prev = raw;
            return;
        }
        // Backspace on a value kept whole clears it: it is one unit.
        let (text, caret) = if was_kept && raw.len() < self.prev.len() {
            (String::new(), 0)
        } else {
            fold_edit(&self.prev, &raw, cursor, self.slots)
        };
        let caret_now = chars_before(&raw, cursor);
        if text != raw || caret != caret_now {
            self.state.update(cx, |st, cx| {
                if st.value().as_ref() != text {
                    st.set_value(text.clone(), window, cx);
                }
                st.set_selected_range(caret..caret, cx);
            });
        }
        self.prev = text;
    }

    /// A click in slot `ix`: the caret goes there (or after the last
    /// character when the slot is still empty).
    fn place(&mut self, ix: usize, window: &mut Window, cx: &mut Context<Self>) {
        self.state.update(cx, |st, cx| {
            let at = ix.min(st.value().len());
            st.focus(window, cx);
            st.set_selected_range(at..at, cx);
        });
        cx.notify();
    }
}

impl Render for CodeInput {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let k = native_colors(cx);
        // Opaque, so a ring drawn under the slot never shows through it.
        let selection = k.control.blend(cx.theme().selection);
        let (focused, chars, range, cursor) = {
            let st = self.state.read(cx);
            let value = st.value();
            (
                st.focus_handle(cx).is_focused(window),
                value.chars().collect::<Vec<char>>(),
                st.selected_range(),
                st.cursor(),
            )
        };
        let width = code_input_width(self.slots, self.group);
        let selected_all = !chars.is_empty() && range.start == 0 && range.end >= chars.len();
        let selected = |ix: usize| range.start <= ix && ix < range.end;
        // The active slot is the one at the caret; with every slot filled,
        // the last one.
        let active = cursor.min(self.slots.saturating_sub(1));
        let error = self.error;
        let edge = |on: bool| slot_edge(&k, on && focused, error);
        let body = match &self.locked {
            Some(text) => {
                let (border, shadows) = edge(true);
                div()
                    .size_full()
                    .px(metrics::FIELD_PAD)
                    .rounded(metrics::CODE_SLOT_RADIUS)
                    .bg(if focused && selected_all {
                        selection
                    } else {
                        k.control
                    })
                    .border_1()
                    .border_color(border)
                    .shadow(shadows)
                    .flex()
                    .items_center()
                    .justify_center()
                    .gap(space::S6)
                    .n_body()
                    .text_color(k.secondary)
                    .child(Sym::Lock.icon(SymSize::INLINE).text_color(k.secondary))
                    .child(text.clone())
                    .into_any_element()
            }
            None => {
                let mut row = div().size_full().flex().items_center();
                for ix in 0..self.slots {
                    if ix > 0 {
                        if ix % self.group == 0 {
                            row = row.child(
                                div()
                                    .flex_none()
                                    .w(metrics::CODE_GROUP_GAP)
                                    .flex()
                                    .justify_center()
                                    .child(
                                        div()
                                            .w(metrics::CODE_DASH.0)
                                            .h(metrics::CODE_DASH.1)
                                            .rounded(metrics::CODE_DASH.1)
                                            .bg(k.secondary),
                                    ),
                            );
                        } else {
                            row = row.child(div().flex_none().w(metrics::CODE_SLOT_GAP));
                        }
                    }
                    let ch = chars.get(ix).copied();
                    let is_active = ix == active && range.is_empty();
                    let (border, shadows) = edge(is_active);
                    let caret = focused && is_active && ch.is_none();
                    row = row.child(
                        div()
                            .id(("code-slot", ix))
                            .flex_none()
                            .w(metrics::CODE_SLOT.0)
                            .h(metrics::CODE_SLOT.1)
                            .rounded(metrics::CODE_SLOT_RADIUS)
                            .bg(if focused && ch.is_some() && selected(ix) {
                                selection
                            } else {
                                k.control
                            })
                            .border_1()
                            .border_color(border)
                            .shadow(shadows)
                            .flex()
                            .items_center()
                            .justify_center()
                            .n_style(text::TITLE)
                            .font_weight(gpui::FontWeight::MEDIUM)
                            .code_font()
                            .text_color(k.label)
                            .children(ch.map(|c| c.to_string()))
                            .when(caret, |el| {
                                el.child(
                                    div()
                                        .w(metrics::CODE_CARET.0)
                                        .h(metrics::CODE_CARET.1)
                                        .rounded(metrics::CODE_CARET.0)
                                        .bg(k.label),
                                )
                            })
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(move |this, _, window, cx| {
                                    cx.stop_propagation();
                                    this.place(ix, window, cx);
                                }),
                            ),
                    );
                }
                row.into_any_element()
            }
        };
        let state = self.state.clone();
        div()
            .id(("code-input", cx.entity_id()))
            .relative()
            .flex_none()
            .w(width)
            .h(metrics::CODE_SLOT.1)
            .cursor_text()
            // The hidden field owns the text, caret, clipboard and IME; it
            // is laid under the slots, which take the clicks.
            .child(
                div()
                    .absolute()
                    .inset_0()
                    .opacity(0.)
                    .child(Input::new(&self.state).appearance(false).h_full()),
            )
            .child(div().absolute().inset_0().occlude().child(body))
            .on_mouse_down(MouseButton::Left, move |_, window, cx| {
                state.read(cx).focus_handle(cx).focus(window, cx);
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn typing_fills_slots_and_stops_at_the_last() {
        assert_eq!(fold_edit("", "k", 1, 8), ("K".into(), 1));
        assert_eq!(
            fold_edit("K7QM2X9A", "K7QM2X9AB", 9, 8),
            ("K7QM2X9A".into(), 8)
        );
    }

    #[test]
    fn a_paste_fills_every_slot() {
        assert_eq!(fold_edit("", "k7qm-2x9a", 9, 8), ("K7QM2X9A".into(), 8));
        assert_eq!(fold_edit("", " K7QM 2X9A ", 11, 8), ("K7QM2X9A".into(), 8));
    }

    #[test]
    fn typing_over_a_filled_slot_replaces_it() {
        // Caret after slot 2 (arrow keys), `Z` typed: slot 3 becomes Z.
        assert_eq!(
            fold_edit("K7QM2X9A", "K7ZQM2X9A", 3, 8),
            ("K7ZM2X9A".into(), 3)
        );
    }

    #[test]
    fn backspace_moves_back() {
        assert_eq!(fold_edit("K7QM", "K7Q", 3, 8), ("K7Q".into(), 3));
    }

    #[test]
    fn width_of_two_groups_of_four() {
        // 8 x 40 + 6 x 6 + 20.
        assert_eq!(code_input_width(8, 4), gpui::px(376.));
    }
}
