//! Grouped forms (NATIVE.md §1 rule 6, §5.6, §8): one rounded `group` with
//! label/value rows separated by inset hairlines, a header above and a
//! footer below; the inline value field that edits a value in place.

use gpui::{
    AnyElement, App, AppContext as _, ElementId, Entity, Focusable as _, InteractiveElement as _,
    IntoElement, KeyDownEvent, ParentElement, RenderOnce, SharedString, Styled, TextAlign, Window,
    div, prelude::FluentBuilder as _, px,
};
use gpui_component::{
    Sizable as _,
    input::{Input, InputEvent, InputState},
};

use super::tokens::{NativeType as _, metrics, native_colors};
use crate::ui::controls::placeholder_input;

/// A grouped form section (§8): optional header (13 pt semibold `label`,
/// 6 pt above), rows on a `group` fill with radius 10, an optional footer
/// (11 pt `secondary`, 6 pt below). No shadow, no border.
#[derive(IntoElement, Default)]
pub struct GroupedForm {
    header: Option<SharedString>,
    footer: Option<SharedString>,
    rows: Vec<AnyElement>,
    trailing_below: Option<AnyElement>,
}

impl GroupedForm {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn header(mut self, header: impl Into<SharedString>) -> Self {
        self.header = Some(header.into());
        self
    }

    pub fn footer(mut self, footer: impl Into<SharedString>) -> Self {
        self.footer = Some(footer.into());
        self
    }

    pub fn row(mut self, row: impl IntoElement) -> Self {
        self.rows.push(row.into_any_element());
        self
    }

    pub fn rows(mut self, rows: impl IntoIterator<Item = impl IntoElement>) -> Self {
        self.rows
            .extend(rows.into_iter().map(IntoElement::into_any_element));
        self
    }

    /// Something right under the group at its leading edge (the `+`/`−`
    /// square pair of a list group).
    pub fn below(mut self, el: impl IntoElement) -> Self {
        self.trailing_below = Some(el.into_any_element());
        self
    }
}

impl RenderOnce for GroupedForm {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let k = native_colors(cx);
        let n = self.rows.len();
        let mut body = div()
            .w_full()
            .rounded(metrics::GROUP_RADIUS)
            .bg(k.group)
            .flex()
            .flex_col();
        for (ix, row) in self.rows.into_iter().enumerate() {
            body = body.child(row);
            if ix + 1 < n {
                body = body.child(div().mx(metrics::FORM_ROW_INSET).h(px(1.)).bg(k.separator));
            }
        }
        div()
            .w_full()
            .flex()
            .flex_col()
            .when_some(self.header, |d, h| {
                d.child(
                    div()
                        .pb(metrics::GROUP_HEADER_GAP)
                        .px(px(2.))
                        .n_headline()
                        .text_color(k.label)
                        .child(h),
                )
            })
            .child(body)
            .when_some(self.footer, |d, f| {
                d.child(
                    div()
                        .pt(metrics::GROUP_HEADER_GAP)
                        .px(px(2.))
                        .n_subheadline()
                        .text_color(k.secondary)
                        .child(f),
                )
            })
            .when_some(self.trailing_below, |d, el| {
                d.child(div().pt(px(8.)).flex().child(el))
            })
    }
}

/// One row of a grouped form (§8): min 36 pt, 12 pt side insets, the label
/// (Body `label`) at the leading edge and the value or control at the
/// trailing edge. A row may have a leading symbol or avatar and a
/// `secondary` second line under the label.
#[derive(IntoElement)]
pub struct FormRow {
    label: SharedString,
    detail: Option<SharedString>,
    leading: Option<AnyElement>,
    value: Option<AnyElement>,
    min_height: gpui::Pixels,
}

impl FormRow {
    pub fn new(label: impl Into<SharedString>) -> Self {
        Self {
            label: label.into(),
            detail: None,
            leading: None,
            value: None,
            min_height: metrics::FORM_ROW,
        }
    }

    /// A `secondary` Subheadline line under the label.
    pub fn detail(mut self, detail: impl Into<SharedString>) -> Self {
        self.detail = Some(detail.into());
        self
    }

    pub fn leading(mut self, el: impl IntoElement) -> Self {
        self.leading = Some(el.into_any_element());
        self
    }

    /// The control at the trailing edge (switch, pop-up, push button,
    /// inline value field).
    pub fn control(mut self, el: impl IntoElement) -> Self {
        self.value = Some(el.into_any_element());
        self
    }

    /// A read-only value: `secondary` Body text at the trailing edge.
    pub fn value(mut self, text: impl Into<SharedString>, cx: &App) -> Self {
        let k = native_colors(cx);
        self.value = Some(
            div()
                .n_body()
                .n_figures()
                .text_color(k.secondary)
                .overflow_hidden()
                .whitespace_nowrap()
                .text_ellipsis()
                .child(text.into())
                .into_any_element(),
        );
        self
    }

    pub fn min_height(mut self, h: gpui::Pixels) -> Self {
        self.min_height = h;
        self
    }
}

impl RenderOnce for FormRow {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let k = native_colors(cx);
        div()
            .w_full()
            .min_h(self.min_height)
            .px(metrics::FORM_ROW_INSET)
            .py(px(6.))
            .flex()
            .items_center()
            .gap(px(10.))
            .children(self.leading)
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.))
                    .flex()
                    .flex_col()
                    .child(
                        div()
                            .n_body()
                            .text_color(k.label)
                            .overflow_hidden()
                            .whitespace_nowrap()
                            .text_ellipsis()
                            .child(self.label),
                    )
                    .when_some(self.detail, |d, t| {
                        d.child(
                            div()
                                .n_subheadline()
                                .text_color(k.secondary)
                                .overflow_hidden()
                                .whitespace_nowrap()
                                .text_ellipsis()
                                .child(t),
                        )
                    }),
            )
            .when_some(self.value, |d, v| {
                d.child(
                    div()
                        .flex_none()
                        .max_w(px(320.))
                        .flex()
                        .justify_end()
                        .child(v),
                )
            })
    }
}

/// The inline value field of a grouped form row (§5.6): no border, no fill,
/// right-aligned, `secondary` text at rest; with focus it shows `label`
/// text, the caret and the focus ring around the text only. Escape puts
/// back `committed`; Return and focus loss commit (see [`inline_commit`]).
#[derive(IntoElement)]
pub struct InlineValueField {
    id: ElementId,
    state: Entity<InputState>,
    committed: SharedString,
    width: gpui::Pixels,
    focused: Option<bool>,
}

impl InlineValueField {
    pub fn new(
        id: impl Into<ElementId>,
        state: &Entity<InputState>,
        committed: impl Into<SharedString>,
    ) -> Self {
        Self {
            id: id.into(),
            state: state.clone(),
            committed: committed.into(),
            width: metrics::INLINE_FIELD_WIDTH,
            focused: None,
        }
    }

    pub fn width(mut self, width: gpui::Pixels) -> Self {
        self.width = width;
        self
    }

    /// Draws the focused look without focus (gallery only).
    pub fn show_focus(mut self, focused: bool) -> Self {
        self.focused = Some(focused);
        self
    }
}

impl RenderOnce for InlineValueField {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let k = native_colors(cx);
        let focused = self
            .focused
            .unwrap_or_else(|| self.state.read(cx).focus_handle(cx).is_focused(window));
        let state = self.state.clone();
        let committed = self.committed.clone();
        div()
            .id(self.id)
            .flex_none()
            .w(self.width)
            .h(px(20.))
            .px(px(4.))
            .rounded(px(4.))
            .flex()
            .items_center()
            .when(focused, |d| d.bg(k.content).shadow(k.focus_ring()))
            .cursor_text()
            .on_key_down(move |e: &KeyDownEvent, window, cx| {
                if e.keystroke.key == "escape" {
                    let committed = committed.clone();
                    state.update(cx, |s, cx| s.set_value(committed, window, cx));
                    window.blur(cx);
                    cx.stop_propagation();
                }
            })
            .child(placeholder_input(
                Input::new(&self.state)
                    .appearance(false)
                    .with_size(gpui_component::Size::Small)
                    .px_0()
                    .py_0()
                    .text_align(TextAlign::Right)
                    .n_body()
                    .text_color(if focused { k.label } else { k.secondary }),
                window,
                cx,
            ))
    }
}

/// Commits an inline value on Return and on focus loss: `commit` gets the
/// new text when it differs from the last committed one. Keep the returned
/// subscription with the view.
pub fn inline_commit<V: 'static>(
    state: &Entity<InputState>,
    window: &mut Window,
    cx: &mut gpui::Context<V>,
    commit: impl Fn(&mut V, SharedString, &mut Window, &mut gpui::Context<V>) + 'static,
) -> gpui::Subscription {
    cx.subscribe_in(
        state,
        window,
        move |this, state, e: &InputEvent, window, cx| match e {
            InputEvent::PressEnter { .. } => {
                let v = state.read(cx).value();
                commit(this, v, window, cx);
                window.blur(cx);
            }
            InputEvent::Blur => {
                let v = state.read(cx).value();
                commit(this, v, window, cx);
            }
            _ => {}
        },
    )
}

/// A new input state for an inline value field.
pub fn inline_state(
    value: impl Into<SharedString>,
    window: &mut Window,
    cx: &mut App,
) -> Entity<InputState> {
    let value = value.into();
    cx.new(|cx| InputState::new(window, cx).default_value(value))
}
