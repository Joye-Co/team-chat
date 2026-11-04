//! Menus and popovers (NATIVE.md §7.3): radius 12, `sheet` at 92 % with the
//! glass edge and `float`, 5 pt inner padding, 24 pt rows with an inset
//! `accent` highlight, a `secondary` shortcut column, separators inset to
//! the text column.
//!
//! gpui-component's `PopupMenu` gets these values through the theme
//! (`theme::apply`); these elements draw a menu the app lays out itself
//! (the mention popup, a pop-up button's list) and the gallery.

use gpui::{
    App, IntoElement, ParentElement, RenderOnce, SharedString, Styled, Window, div,
    prelude::FluentBuilder as _, px,
};

use super::tokens::{NativeType as _, metrics, native_colors};
use crate::ui::sf_symbols::{Sf, Weight, symbol};

/// The menu surface: children are [`MenuRow`]s and [`menu_separator`]s.
pub fn menu_surface(cx: &App) -> gpui::Div {
    let k = native_colors(cx);
    div()
        .flex_none()
        .min_w(px(180.))
        .p(metrics::MENU_PAD)
        .rounded(metrics::MENU_RADIUS)
        .bg(k.menu)
        .shadow(k.glass())
        .flex()
        .flex_col()
}

/// A popover surface (§7.3): radius 12, `sheet` fill, `float`, with an
/// arrow on its top edge pointing at the control (`arrow_x` from the
/// leading edge).
pub fn popover_surface(arrow_x: gpui::Pixels, cx: &App) -> gpui::Div {
    let k = native_colors(cx);
    let arrow = metrics::POPOVER_ARROW;
    div()
        .relative()
        .flex_none()
        .rounded(metrics::POPOVER_RADIUS)
        .bg(k.sheet)
        .shadow(k.glass())
        .child(
            // A square turned 45°: GPUI has no rotation for divs, so the
            // arrow is two stacked bars narrowing upward.
            div()
                .absolute()
                .top(-arrow / 2.)
                .left(arrow_x - arrow / 2.)
                .w(arrow)
                .h(arrow / 2.)
                .flex()
                .flex_col()
                .items_center()
                .child(
                    div()
                        .w(arrow / 2.)
                        .h(arrow / 4.)
                        .rounded_t(px(2.))
                        .bg(k.sheet),
                )
                .child(div().w(arrow).h(arrow / 4.).bg(k.sheet)),
        )
}

/// A 1 px separator inset to the text column.
pub fn menu_separator(with_symbols: bool, cx: &App) -> gpui::Div {
    let k = native_colors(cx);
    div()
        .my(px(4.))
        .ml(if with_symbols {
            metrics::MENU_ROW_PAD + metrics::MENU_SYMBOL + px(6.)
        } else {
            metrics::MENU_ROW_PAD
        })
        .mr(metrics::MENU_ROW_PAD)
        .h(px(1.))
        .bg(k.separator)
}

/// A 24 pt menu row: an optional leading 13 pt symbol (all items of a group
/// or none), Body 13 text, an optional shortcut in `secondary`. The
/// highlighted row is a radius 6 `accent` fill inset 5 pt with `onAccent`
/// text.
#[derive(IntoElement)]
pub struct MenuRow {
    label: SharedString,
    symbol: Option<Sf>,
    /// Reserve the symbol column without a symbol.
    symbol_column: bool,
    shortcut: Option<SharedString>,
    checked: bool,
    highlighted: bool,
    disabled: bool,
    destructive: bool,
}

impl MenuRow {
    pub fn new(label: impl Into<SharedString>) -> Self {
        Self {
            label: label.into(),
            symbol: None,
            symbol_column: false,
            shortcut: None,
            checked: false,
            highlighted: false,
            disabled: false,
            destructive: false,
        }
    }

    pub fn symbol(mut self, sf: Sf) -> Self {
        self.symbol = Some(sf);
        self.symbol_column = true;
        self
    }

    /// The chosen item of a pop-up button: a `checkmark` in the symbol column.
    pub fn checked(mut self, checked: bool) -> Self {
        self.checked = checked;
        self.symbol_column = true;
        self
    }

    pub fn shortcut(mut self, s: impl Into<SharedString>) -> Self {
        self.shortcut = Some(s.into());
        self
    }

    pub fn highlighted(mut self, on: bool) -> Self {
        self.highlighted = on;
        self
    }

    pub fn disabled(mut self, on: bool) -> Self {
        self.disabled = on;
        self
    }

    /// The red text of a destructive item.
    pub fn destructive(mut self, on: bool) -> Self {
        self.destructive = on;
        self
    }
}

impl RenderOnce for MenuRow {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let k = native_colors(cx);
        let hl = self.highlighted && !self.disabled;
        let text = if hl {
            k.on_accent
        } else if self.disabled {
            k.tertiary
        } else if self.destructive {
            k.red
        } else {
            k.label
        };
        let glyph = self
            .symbol
            .or(self.checked.then_some(Sf::Check))
            .map(|sf| symbol(sf, metrics::MENU_SYMBOL, Weight::Regular, text, window));
        div()
            .flex_none()
            .h(metrics::MENU_ROW)
            .px(metrics::MENU_ROW_PAD)
            .rounded(metrics::MENU_ROW_RADIUS)
            .flex()
            .items_center()
            .gap(px(6.))
            .when(hl, |d| d.bg(k.accent))
            .when(self.symbol_column, |d| {
                d.child(
                    div()
                        .flex_none()
                        .size(metrics::MENU_SYMBOL)
                        .flex()
                        .items_center()
                        .justify_center()
                        .children(glyph),
                )
            })
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.))
                    .n_body()
                    .text_color(text)
                    .whitespace_nowrap()
                    .child(self.label),
            )
            .when_some(self.shortcut, |d, s| {
                d.child(
                    div()
                        .flex_none()
                        .pl(px(16.))
                        .n_body()
                        .text_color(if hl { k.on_accent } else { k.secondary })
                        .child(s),
                )
            })
    }
}
