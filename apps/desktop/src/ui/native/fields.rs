//! The one field family (COMPLETENESS.md §1.8, §2.8, K04): the Input and
//! the SearchField around gpui-component's `Input`. Input: h32 (sm 28, lg
//! 36), px 12, radius 8, 1 px `input` border, shadow xs, 14 text. Search
//! field: h32, radius full, a 16 search icon at x + 12. Focus: the ring color
//! border plus a 3 px ring at 50 %. Error: the destructive border plus a 3 px
//! destructive ring at 20 %. TextArea: the Input recipe over several lines
//! (py 8, a height of whole lines), the same edge, fill and rings.

use gpui::{
    AnyElement, App, BoxShadow, ElementId, Entity, Focusable as _, Hsla, InteractiveElement as _,
    IntoElement, ParentElement, Pixels, RenderOnce, Styled, TextAlign, Window, div, point,
    prelude::FluentBuilder as _, px,
};
use gpui_component::{
    Sizable as _,
    input::{Input, InputState, Textarea, TextareaState},
};

use super::tokens::{
    NativeColors, NativeType as _, icon, layout, metrics, native_colors, shadow, space, text,
};
use crate::ui::{
    controls::placeholder_input,
    sf_symbols::{Sf, Weight, symbol},
};

/// The size of a field (§1.8).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FieldSize {
    /// h32: the default, sheets and settings.
    Regular,
    /// h28: inside a dense row.
    Small,
    /// h36: the login card.
    Large,
    /// h44, the height of a code slot: the one field of a verification
    /// step, beside a full-width button of the same height.
    Hero,
}

impl FieldSize {
    pub fn height(self) -> Pixels {
        match self {
            FieldSize::Regular => metrics::FIELD,
            FieldSize::Small => metrics::BUTTON_SMALL,
            FieldSize::Large => metrics::FIELD_LARGE,
            FieldSize::Hero => metrics::CODE_SLOT.1,
        }
    }
}

fn ring(color: Hsla) -> BoxShadow {
    BoxShadow {
        color,
        offset: point(px(0.), px(0.)),
        blur_radius: px(0.),
        spread_radius: metrics::FOCUS_RING,
        inset: false,
    }
}

/// The (border, shadows) of a field in its state (§1.8). The focus and
/// error rings are 3 px outside the 1 px border, never inside it, where they
/// read as a second border (review 2 R1-07). Every field, a full-width one
/// too, draws them outside; its column keeps 3 px for them.
pub fn field_edge(k: &NativeColors, focused: bool, error: bool) -> (Hsla, Vec<BoxShadow>) {
    let red = k.red;
    match (error, focused) {
        (true, _) => (red, vec![ring(red.opacity(0.2))]),
        (false, true) => (Hsla { a: 1., ..k.focus }, vec![ring(k.focus)]),
        (false, false) => (k.quaternary, shadow::xs()),
    }
}

/// A single-line Input (§1.8).
#[derive(IntoElement)]
pub struct NativeTextField {
    id: ElementId,
    state: Entity<InputState>,
    size: FieldSize,
    width: Option<Pixels>,
    disabled: bool,
    error: bool,
    focused: Option<bool>,
    leading: Option<AnyElement>,
    trailing: Option<AnyElement>,
    align: TextAlign,
}

impl NativeTextField {
    pub fn new(id: impl Into<ElementId>, state: &Entity<InputState>) -> Self {
        Self {
            id: id.into(),
            state: state.clone(),
            size: FieldSize::Regular,
            width: Some(metrics::FIELD_NAME_WIDTH),
            disabled: false,
            error: false,
            focused: None,
            leading: None,
            trailing: None,
            align: TextAlign::Left,
        }
    }

    /// Aligns the text, while editing and at rest: `Right` for numbers
    /// (§2.8 number field).
    pub fn align(mut self, align: TextAlign) -> Self {
        self.align = align;
        self
    }

    pub fn size(mut self, size: FieldSize) -> Self {
        self.size = size;
        self
    }

    pub fn width(mut self, width: Pixels) -> Self {
        self.width = Some(width);
        self
    }

    /// Fills its column (a full-width row in a sheet or the login card).
    pub fn full_width(mut self) -> Self {
        self.width = None;
        self
    }

    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    pub fn error(mut self, error: bool) -> Self {
        self.error = error;
        self
    }

    /// Draws the focus ring whatever the focus (self-test scenes).
    pub fn show_focus(mut self, focused: bool) -> Self {
        self.focused = Some(focused);
        self
    }

    /// A leading element inside the field at x + 12: the "¥" of a number
    /// field (§2.8), 8 before the value.
    pub fn leading(mut self, el: impl IntoElement) -> Self {
        self.leading = Some(el.into_any_element());
        self
    }

    /// A trailing element inside the field (a clear button, a spinner).
    pub fn trailing(mut self, el: impl IntoElement) -> Self {
        self.trailing = Some(el.into_any_element());
        self
    }
}

impl RenderOnce for NativeTextField {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let k = native_colors(cx);
        let focused = self
            .focused
            .unwrap_or_else(|| self.state.read(cx).focus_handle(cx).is_focused(window))
            && !self.disabled;
        let (edge, shadows) = field_edge(&k, focused, self.error);
        // The value drawn at rest; `None` while editing, when empty (the
        // placeholder shows) and for a masked input (a secret never shows).
        let at_rest = {
            let state = self.state.read(cx);
            (!focused && !state.presentation().is_masked())
                .then(|| state.value())
                .filter(|v| !v.is_empty())
        };
        div()
            .id(self.id)
            .flex_none()
            .map(|d| match self.width {
                Some(w) => d.w(w),
                None => d.w_full(),
            })
            .h(self.size.height())
            .px(metrics::FIELD_PAD)
            .rounded(metrics::FIELD_RADIUS)
            .bg(k.control)
            .border_1()
            .border_color(edge)
            .shadow(shadows)
            .when(self.disabled, |d| d.opacity(0.5))
            .flex()
            .items_center()
            .gap(space::S8)
            .cursor_text()
            .children(self.leading)
            .child(
                // The text clips at the 12 px padding, never at the border
                // (T1-12). At rest a value longer than the field ends in
                // "…" with the whole value in a tooltip (§2.21), never on a
                // cut glyph; the input stays mounted under it (invisible),
                // so a click or Tab still lands in it.
                div()
                    .flex_1()
                    .min_w(px(0.))
                    .relative()
                    .overflow_hidden()
                    .child(
                        div()
                            .when(at_rest.is_some(), |d| d.opacity(0.))
                            .child(placeholder_input(
                                Input::new(&self.state)
                                    .appearance(false)
                                    .disabled(self.disabled)
                                    .with_size(gpui_component::Size::Small)
                                    .px_0()
                                    .py_0()
                                    .n_body()
                                    .text_align(self.align)
                                    .text_color(k.label),
                                window,
                                cx,
                            )),
                    )
                    .children(at_rest.map(|value| {
                        div().absolute().inset_0().flex().items_center().child(
                            crate::ui::truncate::TruncText::new(value)
                                .w_full()
                                .n_body()
                                .text_color(k.label)
                                .when(self.align == TextAlign::Right, |t| t.text_right()),
                        )
                    }))
                    // Focused, a value longer than the field fades out at
                    // the edge it runs past, never a glyph cut at the
                    // 12 px text edge (break-ui, review 3 K-07). Painted
                    // after the input's prepaint, so it reads this frame's
                    // scroll offset and caret.
                    .when(focused, |d| {
                        let state = self.state.clone();
                        let fill = k.control;
                        d.child(
                            gpui::canvas(
                                |_, _, _| (),
                                move |bounds, _, window, cx| {
                                    reveal_caret(&state, cx);
                                    let (left, right) =
                                        overflow_edges(&state, bounds.size.width, window, cx);
                                    for (on, at_left) in [(left, true), (right, false)] {
                                        if on {
                                            window.paint_quad(edge_fade(bounds, fill, at_left));
                                        }
                                    }
                                },
                            )
                            .absolute()
                            .inset_0(),
                        )
                    }),
            )
            .children(self.trailing)
    }
}

/// Scrolls a focused single-line value so its caret is in view. A value
/// set by the app (`set_value`) starts scrolled to its start with the caret
/// at its end; the engine follows the caret only when the selection moves,
/// so a long value would keep its caret out of view (review 4 K-16).
fn reveal_caret(state: &Entity<InputState>, cx: &mut App) {
    let (caret, text, offset) = {
        let s = state.read(cx);
        let (Some((caret, _)), Some(text)) = (s.cursor_layout(), s.text_bounds()) else {
            return;
        };
        (caret, text, s.scroll_offset())
    };
    let shift = if caret.right() > text.right() + px(0.5) {
        text.right() - caret.right()
    } else if caret.left() < text.left() - px(0.5) {
        text.left() - caret.left()
    } else {
        return;
    };
    state.update(cx, |s, cx| {
        s.set_scroll_offset(point(offset.x + shift, offset.y), cx)
    });
}

/// The edges a focused single-line value runs past in a text box `room`
/// wide, from this frame's scroll offset: (left, right). When the end of the
/// value (and a caret there) is in view, the right edge is not faded.
fn overflow_edges(
    state: &Entity<InputState>,
    room: Pixels,
    window: &Window,
    cx: &App,
) -> (bool, bool) {
    let state = state.read(cx);
    if state.presentation().is_masked() {
        return (false, false);
    }
    let value = state.value();
    if value.is_empty() {
        return (false, false);
    }
    let style = window.text_style();
    let run = gpui::TextRun {
        len: value.len(),
        font: style.font(),
        color: style.color,
        background_color: None,
        underline: None,
        strikethrough: None,
    };
    let width = window
        .text_system()
        .shape_line(value.clone(), text::UI.0, &[run], None)
        .width;
    let offset = state.scroll_offset().x;
    let left = offset < px(-0.5);
    let right = width + offset > room + px(0.5);
    (left, right)
}

/// A 12 fade from the field fill over one edge of the text box `bounds`.
fn edge_fade(bounds: gpui::Bounds<Pixels>, fill: Hsla, left: bool) -> gpui::PaintQuad {
    let w = metrics::FIELD_PAD;
    let (x, from, to) = if left {
        (bounds.origin.x, fill, fill.opacity(0.))
    } else {
        (
            bounds.origin.x + bounds.size.width - w,
            fill.opacity(0.),
            fill,
        )
    };
    gpui::fill(
        gpui::Bounds::new(point(x, bounds.origin.y), gpui::size(w, bounds.size.height)),
        gpui::linear_gradient(
            90.,
            gpui::linear_color_stop(from, 0.),
            gpui::linear_color_stop(to, 1.),
        ),
    )
}

/// A multi-line field (§1.8, §2.8 Textarea): the Input recipe over
/// `rows` whole lines of 14 / 20 text. px 12, py 8, radius 8, the `control`
/// fill, the 1 px `input` border with shadow xs at rest, and the same focus
/// and error rings as [`NativeTextField`] (review 3 K-02: one field recipe
/// per sheet, no soft halo).
#[derive(IntoElement)]
pub struct NativeTextArea {
    id: ElementId,
    state: Entity<TextareaState>,
    rows: usize,
    disabled: bool,
    error: bool,
    focused: Option<bool>,
}

impl NativeTextArea {
    /// Four lines, full width: the default (§2.8: min 3 lines).
    pub fn new(id: impl Into<ElementId>, state: &Entity<TextareaState>) -> Self {
        Self {
            id: id.into(),
            state: state.clone(),
            rows: 4,
            disabled: false,
            error: false,
            focused: None,
        }
    }

    /// The visible lines (at least 3).
    pub fn rows(mut self, rows: usize) -> Self {
        self.rows = rows.max(3);
        self
    }

    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    pub fn error(mut self, error: bool) -> Self {
        self.error = error;
        self
    }

    /// Draws the focus ring whatever the focus (self-test scenes).
    pub fn show_focus(mut self, focused: bool) -> Self {
        self.focused = Some(focused);
        self
    }

    /// The outer height: whole lines, the padding and the border.
    pub fn height(rows: usize) -> Pixels {
        Self::text_height(rows) + space::S8 * 2. + layout::BORDER * 2.
    }

    /// The height of the text box inside the padding: whole 14 / 20 lines.
    fn text_height(rows: usize) -> Pixels {
        text::UI.1 * rows as f32
    }
}

impl RenderOnce for NativeTextArea {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let k = native_colors(cx);
        let handle = self.state.read(cx).focus_handle(cx);
        let focused = self.focused.unwrap_or_else(|| handle.is_focused(window)) && !self.disabled;
        let (edge, shadows) = field_edge(&k, focused, self.error);
        // For the scene checks: the lines the last frame showed, the wrapped
        // rows of the value that fit in the text box.
        {
            let state = self.state.read(cx);
            if let (Some(bounds), Some(line)) = (state.text_bounds(), state.line_height())
                && line > px(0.)
            {
                let fit = (bounds.size.height / line).floor() as usize;
                let lines = state.display_map().wrap_row_count().min(fit);
                crate::selftest::drawn::mark("textarea-lines", format!("{} {lines}", self.id));
            }
        }
        div()
            .id(self.id)
            .flex_none()
            .w_full()
            .h(Self::height(self.rows))
            // The text sits 12 inside the border, as in the Input (review 4
            // K-17): the multi-line editor pads its text by its own size's
            // input padding (4 at xsmall), so the box pads the rest.
            .px(metrics::FIELD_PAD - gpui_component::Size::XSmall.input_px())
            .py(space::S8)
            .rounded(metrics::FIELD_RADIUS)
            .bg(k.control)
            .border_1()
            .border_color(edge)
            .shadow(shadows)
            .when(self.disabled, |d| d.opacity(0.5))
            .overflow_hidden()
            .cursor_text()
            // A click in the padding lands in the text too.
            .when(!self.disabled, |d| {
                d.on_mouse_down(gpui::MouseButton::Left, move |_, window, cx| {
                    handle.focus(window, cx);
                })
            })
            .child(placeholder_input(
                // The text box is `rows` whole lines tall, whatever rows the
                // state asks for: a state made with the default single row
                // would lay out one line and clip the soft-wrapped rest
                // (review 4 K-12). Longer text scrolls inside the box.
                Textarea::new(&self.state)
                    .h(Self::text_height(self.rows))
                    .appearance(false)
                    .disabled(self.disabled)
                    .xsmall()
                    .p_0()
                    .n_body()
                    .text_color(k.label),
                window,
                cx,
            ))
    }
}

/// The search field (§1.8): h32, radius full, a 16 search icon at x + 12.
#[derive(IntoElement)]
pub struct SearchField {
    id: ElementId,
    state: Entity<InputState>,
    sidebar: bool,
    width: Option<Pixels>,
    focused: Option<bool>,
    clearable: bool,
}

impl SearchField {
    pub fn new(id: impl Into<ElementId>, state: &Entity<InputState>) -> Self {
        Self {
            id: id.into(),
            state: state.clone(),
            sidebar: false,
            width: None,
            focused: None,
            clearable: false,
        }
    }

    /// A clear × at the trailing edge while the field has text (it empties
    /// the field and keeps the focus in it).
    pub fn clearable(mut self) -> Self {
        self.clearable = true;
        self
    }

    /// The sidebar's field: the same shape with no shadow (§2.1).
    pub fn sidebar(mut self) -> Self {
        self.sidebar = true;
        self
    }

    pub fn width(mut self, width: Pixels) -> Self {
        self.width = Some(width);
        self
    }

    pub fn show_focus(mut self, focused: bool) -> Self {
        self.focused = Some(focused);
        self
    }
}

impl RenderOnce for SearchField {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let k = native_colors(cx);
        let focused = self
            .focused
            .unwrap_or_else(|| self.state.read(cx).focus_handle(cx).is_focused(window));
        let (edge, shadows) = field_edge(&k, focused, false);
        let shadows = if self.sidebar && !focused {
            Vec::new()
        } else {
            shadows
        };
        div()
            .id(self.id)
            .flex_none()
            .map(|d| match self.width {
                Some(w) => d.w(w),
                None => d.w_full(),
            })
            .h(metrics::SEARCH)
            .pl(metrics::FIELD_PAD)
            .pr(metrics::FIELD_PAD)
            .rounded_full()
            .bg(k.control)
            .border_1()
            .border_color(edge)
            .shadow(shadows)
            .flex()
            .items_center()
            .gap(space::S8)
            .cursor_text()
            .child(symbol(
                Sf::Search,
                icon::MD,
                Weight::Regular,
                k.secondary,
                window,
            ))
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.))
                    .overflow_hidden()
                    .child(placeholder_input(
                        Input::new(&self.state)
                            .appearance(false)
                            .cleanable(self.clearable)
                            .with_size(gpui_component::Size::Small)
                            .px_0()
                            .py_0()
                            .n_body()
                            .text_color(k.label),
                        window,
                        cx,
                    )),
            )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_and_focus_rings() {
        for k in [NativeColors::light(), NativeColors::dark()] {
            let (rest, s) = field_edge(&k, false, false);
            assert_eq!(rest, k.quaternary);
            assert_eq!(s, shadow::xs());
            let (err, s) = field_edge(&k, true, true);
            assert_eq!(err, k.red);
            assert_eq!(s[0].spread_radius, metrics::FOCUS_RING);
            let (_, s) = field_edge(&k, true, false);
            assert_eq!(s[0].color, k.focus);
            // R1-07: the ring is outside the border, never a second border.
            let (_, s) = field_edge(&k, true, false);
            assert!(s.iter().all(|s| !s.inset));
        }
    }
}
