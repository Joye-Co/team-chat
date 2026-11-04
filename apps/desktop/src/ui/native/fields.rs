//! Standalone text fields and search fields (NATIVE.md §5.6) around
//! gpui-component's `Input`.

use gpui::{
    App, ElementId, Entity, Focusable as _, InteractiveElement as _, IntoElement, ParentElement,
    Pixels, RenderOnce, Styled, Window, div, prelude::FluentBuilder as _, px,
};
use gpui_component::{
    Sizable as _,
    input::{Input, InputState},
};

use super::tokens::{NativeType as _, metrics, native_colors};
use crate::ui::{
    controls::placeholder_input,
    sf_symbols::{Sf, Weight, symbol},
};

/// The size of a standalone field.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FieldSize {
    /// 24 pt: sheets and settings.
    Regular,
    /// 28 pt: the login window.
    Large,
}

/// A standalone text field (§5.6): 24 pt (28 large), radius 6, `content`
/// fill, 1 px `separator` border, 7 pt side padding, the 3 pt focus ring.
/// Width fits the expected text (name 220, code 140), never the window.
#[derive(IntoElement)]
pub struct NativeTextField {
    id: ElementId,
    state: Entity<InputState>,
    size: FieldSize,
    width: Pixels,
    disabled: bool,
    error: bool,
    focused: Option<bool>,
}

impl NativeTextField {
    pub fn new(id: impl Into<ElementId>, state: &Entity<InputState>) -> Self {
        Self {
            id: id.into(),
            state: state.clone(),
            size: FieldSize::Regular,
            width: metrics::FIELD_NAME_WIDTH,
            disabled: false,
            error: false,
            focused: None,
        }
    }

    pub fn size(mut self, size: FieldSize) -> Self {
        self.size = size;
        self
    }

    pub fn width(mut self, width: Pixels) -> Self {
        self.width = width;
        self
    }

    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    /// The error look: a `red` border (the message goes in the row footer).
    pub fn error(mut self, error: bool) -> Self {
        self.error = error;
        self
    }

    pub fn show_focus(mut self, focused: bool) -> Self {
        self.focused = Some(focused);
        self
    }
}

impl RenderOnce for NativeTextField {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let k = native_colors(cx);
        let focused = self
            .focused
            .unwrap_or_else(|| self.state.read(cx).focus_handle(cx).is_focused(window));
        let h = match self.size {
            FieldSize::Regular => metrics::FIELD,
            FieldSize::Large => metrics::FIELD_LARGE,
        };
        let edge = if self.error { k.red } else { k.separator };
        div()
            .id(self.id)
            .flex_none()
            .w(self.width)
            .h(h)
            .px(metrics::FIELD_PAD)
            .rounded(metrics::FIELD_RADIUS)
            .bg(k.content)
            .border_1()
            .border_color(edge)
            .when(focused && !self.disabled, |d| d.shadow(k.focus_ring()))
            .when(self.disabled, |d| d.opacity(0.5))
            .flex()
            .items_center()
            .cursor_text()
            .child(
                div().flex_1().min_w(px(0.)).child(placeholder_input(
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
    }
}

/// A search field (§5.6): a capsule, 30 pt in the sidebar (24 elsewhere),
/// `fill`, a 13 pt `magnifyingglass` in `secondary`, placeholder `搜索`.
/// Shortcut hints live in the menu bar, not in the field.
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

    /// The 30 pt sidebar size.
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
        let h = if self.sidebar {
            metrics::SEARCH_SIDEBAR
        } else {
            metrics::SEARCH
        };
        div()
            .id(self.id)
            .flex_none()
            .map(|d| match self.width {
                Some(w) => d.w(w),
                None => d.w_full(),
            })
            .h(h)
            .pl(px(8.))
            .pr(px(10.))
            .rounded(h / 2.)
            // A translucent fill would show the ring's shadow through it:
            // a focused field is opaque `content`.
            .bg(if focused { k.content } else { k.fill })
            .when(focused, |d| d.shadow(k.focus_ring()))
            .flex()
            .items_center()
            .gap(px(5.))
            .cursor_text()
            .child(symbol(
                Sf::Search,
                metrics::SEARCH_SYMBOL,
                Weight::Regular,
                k.secondary,
                window,
            ))
            .child(
                div().flex_1().min_w(px(0.)).child(placeholder_input(
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
