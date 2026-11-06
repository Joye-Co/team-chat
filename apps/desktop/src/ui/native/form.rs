//! Grouped forms (NATIVE.md §1 rule 6, §5.6, §8): one rounded `group` with
//! label/value rows separated by inset hairlines, a header above and a
//! footer below; [`inline_commit`] commits a value edited in place.

use crate::ui::truncate::TruncText;
use gpui::{
    AnyElement, App, AppContext as _, Entity, IntoElement, ParentElement, RenderOnce, SharedString,
    Styled, Window, div, prelude::FluentBuilder as _, px,
};
use gpui_component::input::{InputEvent, InputState};

use super::tokens::{NativeType as _, metrics, native_colors};

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
            // A fill, no frame (owner review: borders everywhere are
            // noise): the group's own tint sets it off the window, as in
            // macOS System Settings; hairlines stay only between rows.
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
            TruncText::new(text)
                .n_body()
                .n_figures()
                .text_color(k.secondary)
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
                    .child(TruncText::new(self.label).n_body().text_color(k.label))
                    .when_some(self.detail, |d, t| {
                        d.child(TruncText::new(t).n_subheadline().text_color(k.secondary))
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
