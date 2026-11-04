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

/// The (border, shadows) of a field that fills its column: the same ring
/// drawn inside the field edge, so it never leaves the content column of a
/// sheet or group (T1-12, §2.9).
pub fn field_edge_inset(k: &NativeColors, focused: bool, error: bool) -> (Hsla, Vec<BoxShadow>) {
    let (edge, mut shadows) = field_edge(k, focused, error);
    if focused || error {
        for s in &mut shadows {
            s.inset = true;
            s.spread_radius = metrics::FOCUS_RING - metrics::HAIRLINE;
        }
    }
    (edge, shadows)
}

/// The (border, shadows) of a field in its state (§1.8).
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
        // A field that fills its column draws its ring inside (T1-12).
        let (edge, shadows) = if self.width.is_none() {
            field_edge_inset(&k, focused, self.error)
        } else {
            field_edge(&k, focused, self.error)
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
            .bg(k.content)
            .border_1()
            .border_color(edge)
            .shadow(shadows)
            .when(self.disabled, |d| d.opacity(0.5))
            .flex()
            .items_center()
            .gap(space::S8)
            .cursor_text()
            .child(
                // The text clips at the 12 px padding, never mid-glyph at
                // the border (T1-12).
                div()
                    .flex_1()
                    .min_w(px(0.))
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
                    )),
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
        let (edge, shadows) = if self.width.is_none() {
            field_edge_inset(&k, focused, false)
        } else {
            field_edge(&k, focused, false)
        };
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
            .bg(k.content)
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
            // T1-12: a full-width field keeps its ring inside its edge.
            let (_, s) = field_edge_inset(&k, true, false);
            assert!(s.iter().all(|s| s.inset));
        }
    }
}
