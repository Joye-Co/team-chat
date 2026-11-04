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
    tokens::{NativeType as _, metrics, native_colors, space},
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

/// How a sheet enters from a click: sliding down from under the toolbar
/// while it fades in (160 ms, out curve), unless Reduce Motion is on
/// (gpui-component shows the end state then). A sheet opened from the
/// keyboard many times a day (⌘N 新话题) opens at once: see
/// [`native_sheet_keyboard`] (T1-13, review-animations 9).
pub const SHEET_ENTRANCE: DialogEntrance = DialogEntrance::FadeSlide;

/// [`native_sheet`] for a sheet opened by a keyboard shortcut: no motion,
/// in and out.
pub fn native_sheet_keyboard(dialog: Dialog, width: SheetWidth, cx: &App) -> Dialog {
    native_sheet(dialog, width, cx).entrance(DialogEntrance::None)
}

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
        .border_1()
        .border_color(k.separator)
        .shadow(super::tokens::shadow::lg())
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
    status: Option<AnyElement>,
}

impl SheetLayout {
    pub fn new(title: impl Into<SharedString>) -> Self {
        Self {
            title: title.into(),
            explanation: None,
            body: Vec::new(),
            buttons: Vec::new(),
            leading_button: None,
            status: None,
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

    /// The footer's left slot (§2.9): a count ("已选 2 位", 12 muted) or an
    /// error line, beside the buttons, so the sheet height does not change.
    /// Build an error line with [`sheet_status_error`].
    pub fn status(mut self, el: impl IntoElement) -> Self {
        self.status = Some(el.into_any_element());
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
                    .gap(space::S8)
                    .child(div().n_title2().text_color(k.label).child(self.title))
                    .when_some(self.explanation, |d, t| {
                        d.child(div().n_body().text_color(k.secondary).child(t))
                    }),
            )
            // T1-13: an empty body adds no band; the footer sits 24 under
            // the header.
            .when(!self.body.is_empty(), |d| {
                d.child(
                    div()
                        .px(metrics::SHEET_PAD)
                        .pt(space::S16)
                        .flex()
                        .flex_col()
                        .gap(space::S12)
                        .children(self.body),
                )
            })
            .child(
                div()
                    .px(metrics::SHEET_BUTTON_EDGE)
                    .pt(space::S24)
                    .pb(metrics::SHEET_BUTTON_EDGE)
                    .flex()
                    .items_center()
                    .gap(metrics::SHEET_BUTTON_GAP)
                    .children(self.leading_button)
                    .children(self.status.map(|el| {
                        div()
                            .min_w(px(0.))
                            .n_footnote()
                            .text_color(k.secondary)
                            .child(el)
                    }))
                    .child(div().flex_1())
                    .children(self.buttons),
            )
    }
}

/// The error line of a sheet footer (§2.9, owner decision v14): a 16
/// destructive icon and the message in neutral 12 text.
pub fn sheet_status_error(text: impl Into<SharedString>, cx: &App) -> gpui::Div {
    let k = native_colors(cx);
    div()
        .min_w(px(0.))
        .flex()
        .items_center()
        .gap(space::S6)
        .n_footnote()
        .text_color(k.label)
        .child(crate::ui::icons::icon(
            crate::ui::icons::Sym::Failed,
            super::tokens::icon::MD,
            k.red,
        ))
        .child(crate::ui::truncate::TruncText::new(text))
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
        .border_1()
        .border_color(k.separator)
        .shadow(super::tokens::shadow::lg())
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
            PushStyle::Outline,
            PushSize::Regular,
            cx,
        ),
        push_button(
            format!("{id}-default"),
            default,
            PushStyle::Default,
            PushSize::Regular,
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
            // COMPLETENESS.md §2.10: w 260, p 16; the 48 icon, 12 below the
            // title 14 semibold, 4 below the body 12 muted, 16 below the
            // buttons stacked full width h32, gap 8.
            .w(metrics::ALERT_WIDTH)
            .p(metrics::ALERT_PAD)
            .flex()
            .flex_col()
            .items_center()
            .when_some(self.icon, |d, icon| {
                d.child(
                    div()
                        .size(metrics::ALERT_ICON)
                        .mb(space::S12)
                        .flex_none()
                        .child(icon),
                )
            })
            .child(
                div()
                    .w_full()
                    .text_center()
                    .n_headline()
                    .font_weight(gpui::FontWeight::SEMIBOLD)
                    .text_color(k.label)
                    .child(self.title),
            )
            .when_some(self.message, |d, m| {
                d.child(
                    div()
                        .w_full()
                        .pt(space::S4)
                        .text_center()
                        .n_footnote()
                        .text_color(k.secondary)
                        .child(m),
                )
            })
            .when_some(self.checkbox, |d, c| {
                d.child(div().w_full().pt(space::S12).child(c))
            })
            .child(
                div()
                    .w_full()
                    .pt(space::S16)
                    .flex()
                    .flex_col()
                    .gap(space::S8)
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
    // §2.10: the destructive confirm is the destructive variant (red fill,
    // white text); Cancel is outline; every alert button is h32.
    let label = label.into();
    let style = if destructive {
        PushStyle::Destructive
    } else {
        alert_style(&label, style)
    };
    push_button(id, label, style, PushSize::Regular, cx)
}

/// The style an alert button gets: `取消` is Standard.
fn alert_style(label: &str, style: PushStyle) -> PushStyle {
    if label == crate::copy::common::CANCEL {
        PushStyle::Outline
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
        .border_1()
        .border_color(k.separator)
        .shadow(super::tokens::shadow::lg())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cancel_in_an_alert_is_never_the_default() {
        assert_eq!(
            alert_style(crate::copy::common::CANCEL, PushStyle::Default),
            PushStyle::Outline
        );
        assert_eq!(alert_style("离开", PushStyle::Default), PushStyle::Default);
    }
}
