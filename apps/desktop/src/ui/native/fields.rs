//! The one field family (COMPLETENESS.md §1.8, §2.8, K04): the Input and
//! the SearchField around gpui-component's `Input`. Input: h32 (sm 28, lg
//! 36), px 12, radius 8, 1 px `input` border, shadow xs, 14 text. Search
//! field: h32, radius full, a 16 search icon at x + 12. Focus: the ring color
//! border plus a 3 px ring at 50 %. Error: the destructive border plus a 3 px
//! destructive ring at 20 %.

use gpui::{
    AnyElement, App, BoxShadow, ElementId, Entity, Focusable as _, Hsla, InteractiveElement as _,
    IntoElement, ParentElement, Pixels, RenderOnce, Styled, Window, div, point,
    prelude::FluentBuilder as _, px,
};
use gpui_component::{
    Sizable as _,
    input::{Input, InputState},
};

use super::tokens::{NativeColors, NativeType as _, icon, metrics, native_colors, shadow, space};
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
}

impl FieldSize {
    pub fn height(self) -> Pixels {
        match self {
            FieldSize::Regular => metrics::FIELD,
            FieldSize::Small => metrics::BUTTON_SMALL,
            FieldSize::Large => metrics::FIELD_LARGE,
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

/// The 12 px fade at the end of a field's text box at rest: a value that
/// runs past the box ends in a fade, not on a glyph cut in half. A value that
/// fits ends before it, so it shows nothing.
fn rest_fade(fill: Hsla) -> impl IntoElement {
    div()
        .absolute()
        .top_0()
        .bottom_0()
        .right_0()
        .w(space::S12)
        .bg(gpui::linear_gradient(
            90.,
            gpui::linear_color_stop(fill.opacity(0.), 0.),
            gpui::linear_color_stop(fill, 1.),
        ))
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
    trailing: Option<AnyElement>,
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
            trailing: None,
        }
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
            .child(
                // The text clips at the 12 px padding, never at the border
                // (T1-12); at rest a value longer than the field fades out
                // over its last 12 px instead of ending on a cut glyph.
                div()
                    .flex_1()
                    .min_w(px(0.))
                    .relative()
                    .overflow_hidden()
                    .child(placeholder_input(
                        Input::new(&self.state)
                            .appearance(false)
                            .disabled(self.disabled)
                            .with_size(gpui_component::Size::Small)
                            .px_0()
                            .py_0()
                            .n_body()
                            .text_color(k.label),
                        window,
                        cx,
                    ))
                    .when(!focused, |d| d.child(rest_fade(k.control))),
            )
            .children(self.trailing)
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
}

impl SearchField {
    pub fn new(id: impl Into<ElementId>, state: &Entity<InputState>) -> Self {
        Self {
            id: id.into(),
            state: state.clone(),
            sidebar: false,
            width: None,
            focused: None,
        }
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
    fn fields_are_on_the_control_scale() {
        assert_eq!(FieldSize::Regular.height(), px(32.));
        assert_eq!(FieldSize::Small.height(), px(28.));
        assert_eq!(FieldSize::Large.height(), px(36.));
        assert_eq!(metrics::SEARCH, px(32.));
        assert_eq!(metrics::FIELD_RADIUS, px(8.));
    }

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
