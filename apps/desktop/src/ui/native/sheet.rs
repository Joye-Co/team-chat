//! Sheets and alerts (NATIVE.md §7.1, §7.2).
//!
//! A sheet is attached to its window: it slides down from under the toolbar
//! row, is centered horizontally, 460 (forms) or 560 (lists) wide, radius 22,
//! `sheet` fill, `float` shadow, and dims only that window (black 20 % light,
//! 40 % dark; the theme's `overlay`). Escape cancels, Return runs the default
//! button. [`native_sheet`] configures a gpui-component `Dialog` that way;
//! [`SheetLayout`] lays out what goes inside it.
//!
//! An alert (Tahoe layout) shows the app icon, a bold title, the message,
//! an optional checkbox and full-width stacked buttons: the default on top,
//! `取消` (Standard) below; a destructive choice has red text. A
//! window-modal alert attaches at the top center like a sheet.

use gpui::{
    AnyElement, App, IntoElement, ParentElement, RenderOnce, SharedString, Styled, Window, div,
    prelude::FluentBuilder as _, px,
};
use gpui_component::{
    button::Button,
    dialog::{Dialog, DialogEntrance},
};

use super::{
    buttons::{PushSize, PushStyle, push_button},
    tokens::{NativeType as _, metrics, native_colors},
};

/// The width of a sheet.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SheetWidth {
    /// 460: forms.
    Form,
    /// 560: lists and tables.
    List,
}

impl SheetWidth {
    pub fn px(self) -> gpui::Pixels {
        match self {
            SheetWidth::Form => metrics::SHEET_WIDTH,
            SheetWidth::List => metrics::SHEET_WIDE,
        }
    }
}

/// How a sheet enters: sliding down from under the toolbar while it fades
/// in; a sheet opened from the keyboard still slides (a sheet is not a
/// frequent interaction), unless Reduce Motion is on (gpui-component shows
/// the end state then).
pub const SHEET_ENTRANCE: DialogEntrance = DialogEntrance::FadeSlide;

/// Configures a gpui-component dialog as a native sheet (§7.1). Lay out
/// its content with [`SheetLayout`]; it has no close button (Escape and
/// `取消` close it). The dim covers the whole window, toolbar row
/// included; the sheet hangs from under the toolbar row.
pub fn native_sheet(dialog: Dialog, width: SheetWidth, cx: &App) -> Dialog {
    let k = native_colors(cx);
    dialog
        .entrance(SHEET_ENTRANCE)
        .margin_top(metrics::TOOLBAR_ROW)
        .width(width.px())
        .close_button(false)
        .keyboard(true)
        .overlay_closable(false)
        .bg(k.sheet)
        .border_color(k.glass_edge)
        .rounded(metrics::SHEET_RADIUS)
        .pt(px(0.))
        .pb(px(0.))
}

/// The inside of a sheet (§7.1): the title (Title 2) top-leading at 24 pt
/// insets, an optional one-line explanation (Body, `secondary`), the form,
/// then the buttons at the bottom trailing edge, 12 pt apart, 20 pt from the
/// edges: `取消` then the default button.
#[derive(IntoElement)]
pub struct SheetLayout {
    title: SharedString,
    explanation: Option<SharedString>,
    body: Vec<AnyElement>,
    buttons: Vec<Button>,
    leading_button: Option<Button>,
}

impl SheetLayout {
    pub fn new(title: impl Into<SharedString>) -> Self {
        Self {
            title: title.into(),
            explanation: None,
            body: Vec::new(),
            buttons: Vec::new(),
            leading_button: None,
        }
    }

    pub fn explanation(mut self, text: impl Into<SharedString>) -> Self {
        self.explanation = Some(text.into());
        self
    }

    pub fn body(mut self, el: impl IntoElement) -> Self {
        self.body.push(el.into_any_element());
        self
    }

    /// The trailing buttons in order: `取消` first, the default last.
    pub fn buttons(mut self, buttons: impl IntoIterator<Item = Button>) -> Self {
        self.buttons.extend(buttons);
        self
    }

    /// A button at the bottom-leading edge (a help or secondary action).
    pub fn leading_button(mut self, b: Button) -> Self {
        self.leading_button = Some(b);
        self
    }
}

impl RenderOnce for SheetLayout {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let k = native_colors(cx);
        div()
            .w_full()
            .flex()
            .flex_col()
            .child(
                div()
                    .px(metrics::SHEET_PAD)
                    .pt(metrics::SHEET_PAD)
                    .flex()
                    .flex_col()
                    .gap(px(4.))
                    .child(div().n_title2().text_color(k.label).child(self.title))
                    .when_some(self.explanation, |d, t| {
                        d.child(div().n_body().text_color(k.secondary).child(t))
                    }),
            )
            .child(
                div()
                    .px(metrics::SHEET_PAD)
                    .pt(px(16.))
                    .flex()
                    .flex_col()
                    .gap(px(12.))
                    .children(self.body),
            )
            .child(
                div()
                    .px(metrics::SHEET_BUTTON_EDGE)
                    .pt(px(20.))
                    .pb(metrics::SHEET_BUTTON_EDGE)
                    .flex()
                    .items_center()
                    .gap(metrics::SHEET_BUTTON_GAP)
                    .children(self.leading_button)
                    .child(div().flex_1())
                    .children(self.buttons),
            )
    }
}

/// A sheet surface drawn in place (the gallery, and a sheet a view lays out
/// itself): `sheet` fill, radius 22, the glass edge and `float`.
pub fn sheet_surface(width: SheetWidth, cx: &App) -> gpui::Div {
    let k = native_colors(cx);
    div()
        .flex_none()
        .w(width.px())
        .rounded(metrics::SHEET_RADIUS)
        .bg(k.sheet)
        .shadow(k.glass())
}

/// The large 28 pt capsule buttons at the bottom of a sheet: `取消` and the
/// default action.
pub fn sheet_buttons(
    id: &str,
    cancel: impl Into<SharedString>,
    default: impl Into<SharedString>,
    cx: &App,
) -> [Button; 2] {
    [
        push_button(
            format!("{id}-cancel"),
            cancel,
            PushStyle::Standard,
            PushSize::Large,
            cx,
        ),
        push_button(
            format!("{id}-default"),
            default,
            PushStyle::Default,
            PushSize::Large,
            cx,
        ),
    ]
}

/// The Tahoe alert (§7.2).
#[derive(IntoElement)]
pub struct AlertLayout {
    icon: Option<AnyElement>,
    title: SharedString,
    message: Option<SharedString>,
    checkbox: Option<AnyElement>,
    buttons: Vec<Button>,
}

impl AlertLayout {
    pub fn new(title: impl Into<SharedString>) -> Self {
        Self {
            icon: None,
            title: title.into(),
            message: None,
            checkbox: None,
            buttons: Vec::new(),
        }
    }

    /// The 48 pt app icon.
    pub fn icon(mut self, el: impl IntoElement) -> Self {
        self.icon = Some(el.into_any_element());
        self
    }

    pub fn message(mut self, text: impl Into<SharedString>) -> Self {
        self.message = Some(text.into());
        self
    }

    /// An optional checkbox (`NativeCheckbox` with its label).
    pub fn checkbox(mut self, el: impl IntoElement) -> Self {
        self.checkbox = Some(el.into_any_element());
        self
    }

    /// Stacked full width: the default first, `取消` last.
    pub fn buttons(mut self, buttons: impl IntoIterator<Item = Button>) -> Self {
        self.buttons.extend(buttons);
        self
    }
}

impl RenderOnce for AlertLayout {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let k = native_colors(cx);
        div()
            .w(metrics::ALERT_WIDTH)
            .p(metrics::ALERT_PAD)
            .pt(px(20.))
            .flex()
            .flex_col()
            .items_center()
            .gap(px(10.))
            .when_some(self.icon, |d, icon| {
                d.child(div().size(metrics::ALERT_ICON).flex_none().child(icon))
            })
            .child(
                div()
                    .w_full()
                    .text_center()
                    .n_headline()
                    .font_weight(gpui::FontWeight::BOLD)
                    .text_color(k.label)
                    .child(self.title),
            )
            .when_some(self.message, |d, m| {
                d.child(
                    div()
                        .w_full()
                        .text_center()
                        .n_callout()
                        .text_color(k.label)
                        .child(m),
                )
            })
            .when_some(self.checkbox, |d, c| {
                d.child(div().w_full().pt(px(2.)).child(c))
            })
            .child(
                div()
                    .w_full()
                    .pt(px(6.))
                    .flex()
                    .flex_col()
                    .gap(px(8.))
                    .children(self.buttons.into_iter().map(|b| b.w_full())),
            )
    }
}

/// An alert button: full width, 28 pt capsule. `destructive` draws red
/// text on the standard fill; the default uses `accent`. `取消` is always
/// Standard: next to a destructive choice it must not be the most
/// prominent button, and an alert has at most one Default button (the
/// affirmative one).
pub fn alert_button(
    id: impl Into<gpui::ElementId>,
    label: impl Into<SharedString>,
    style: PushStyle,
    destructive: bool,
    cx: &App,
) -> Button {
    let k = native_colors(cx);
    let label = label.into();
    let style = alert_style(&label, style);
    push_button(id, label, style, PushSize::Large, cx).when(destructive, |b| b.text_color(k.red))
}

/// The style an alert button gets: `取消` is Standard.
fn alert_style(label: &str, style: PushStyle) -> PushStyle {
    if label == crate::copy::common::CANCEL {
        PushStyle::Standard
    } else {
        style
    }
}

/// The alert surface: `sheet` fill, radius 22, glass edge and `float`.
pub fn alert_surface(cx: &App) -> gpui::Div {
    let k = native_colors(cx);
    div()
        .flex_none()
        .rounded(metrics::SHEET_RADIUS)
        .bg(k.sheet)
        .shadow(k.glass())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cancel_in_an_alert_is_never_the_default() {
        assert_eq!(
            alert_style(crate::copy::common::CANCEL, PushStyle::Default),
            PushStyle::Standard
        );
        assert_eq!(alert_style("离开", PushStyle::Default), PushStyle::Default);
    }
}
