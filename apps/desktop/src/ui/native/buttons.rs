//! Push buttons, pop-up buttons, square `+`/`−` pairs, borderless buttons
//! and toolbar items (NATIVE.md §5.1–§5.3). Every one is a gpui-component
//! `Button` given the native shape, size and states here: no hover fill on
//! push buttons, `fillHover` when pressed, the arrow cursor.

use gpui::{
    AnyElement, App, ElementId, Hsla, IntoElement, ParentElement, SharedString, Styled, Window,
    div, prelude::FluentBuilder as _, px,
};
use gpui_component::{
    Disableable as _, Sizable as _,
    button::{Button, ButtonCustomVariant, ButtonVariants as _},
};

use super::tokens::{NativeColors, NativeType as _, metrics, native_colors};
use crate::ui::sf_symbols::{Sf, Weight, symbol};

/// The look of a push button (§5.2).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PushStyle {
    /// `fill` with `label` text.
    Standard,
    /// One per view at most; Return activates it: `accent` fill, `onAccent`
    /// text.
    Default,
    /// A standard button whose title says what it destroys; the alert that
    /// follows makes the red choice. Same look as `Standard`.
    Destructive,
}

/// The size of a push button (§5.2).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PushSize {
    /// 24 pt, radius 6, 12 pt sides, min width 64, Body 13.
    Regular,
    /// 20 pt, radius 5, 10 pt sides, Callout 12.
    Small,
    /// 28 pt capsule, 16 pt sides: only the bottom buttons of sheets and
    /// alerts and the login window.
    Large,
}

impl PushSize {
    pub fn height(self) -> gpui::Pixels {
        match self {
            PushSize::Regular => metrics::BUTTON_REGULAR,
            PushSize::Small => metrics::BUTTON_SMALL,
            PushSize::Large => metrics::BUTTON_LARGE,
        }
    }

    fn radius(self) -> gpui::Pixels {
        match self {
            PushSize::Regular => metrics::BUTTON_REGULAR_RADIUS,
            PushSize::Small => metrics::BUTTON_SMALL_RADIUS,
            PushSize::Large => metrics::BUTTON_LARGE / 2.,
        }
    }

    fn pad(self) -> gpui::Pixels {
        match self {
            PushSize::Regular => metrics::BUTTON_REGULAR_PAD,
            PushSize::Small => metrics::BUTTON_SMALL_PAD,
            PushSize::Large => metrics::BUTTON_LARGE_PAD,
        }
    }
}

/// The (fill, text, pressed fill) of a push button style.
fn push_colors(style: PushStyle, k: &NativeColors) -> (Hsla, Hsla, Hsla) {
    match style {
        PushStyle::Standard | PushStyle::Destructive => (k.fill, k.label, k.fill_hover),
        PushStyle::Default => (k.accent, k.on_accent, k.accent.opacity(0.82)),
    }
}

/// A borderless gpui-component variant: no fill at rest, `fillHover` on
/// hover, `quaternary` when pressed (§5.1, §10: hover is allowed on toolbar
/// items and borderless buttons only).
pub fn borderless_variant(cx: &App) -> ButtonCustomVariant {
    let k = native_colors(cx);
    ButtonCustomVariant::new(cx)
        .color(gpui::transparent_black())
        .foreground(k.label)
        .hover(k.fill_hover)
        .active(k.quaternary)
}

/// A push button (§5.2). Chain `.on_click`, `.disabled` as on any
/// gpui-component button. A title that opens more UI ends with `…`.
pub fn push_button(
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    style: PushStyle,
    size: PushSize,
    cx: &App,
) -> Button {
    let k = native_colors(cx);
    let (fill, text, pressed) = push_colors(style, &k);
    let variant = ButtonCustomVariant::new(cx)
        .color(fill)
        .foreground(text)
        .hover(fill)
        .active(pressed);
    let b = Button::new(id)
        .custom(variant)
        .label(label)
        .with_size(gpui_component::Size::Small)
        .h(size.height())
        .px(size.pad())
        .rounded(size.radius())
        .bg(fill)
        .text_color(text)
        .border_0();
    let b = match size {
        PushSize::Small => b.n_callout(),
        _ => b.n_body(),
    };
    match size {
        PushSize::Small => b,
        _ => b.min_w(metrics::BUTTON_MIN_WIDTH),
    }
}

/// A disabled push button: `tertiary` text on a quieter fill.
pub fn disabled_push(b: Button, disabled: bool, cx: &App) -> Button {
    let k = native_colors(cx);
    b.disabled(disabled)
        .when(disabled, |b| b.bg(k.fill).text_color(k.tertiary))
}

/// A pop-up button (§5.3): a push button whose title is the chosen option,
/// with a 9 pt `chevron.up.chevron.down` at the trailing edge. Chain
/// `.dropdown_menu(..)` to open the menu.
pub fn popup_button(
    id: impl Into<ElementId>,
    title: impl Into<SharedString>,
    size: PushSize,
    window: &Window,
    cx: &App,
) -> Button {
    chevron_button(id, title, Sf::Popup, size, window, cx)
}

/// A pull-down (§5.3): a push button with a trailing `chevron.down`; its
/// title is the action, not a value.
pub fn pull_down(
    id: impl Into<ElementId>,
    title: impl Into<SharedString>,
    size: PushSize,
    window: &Window,
    cx: &App,
) -> Button {
    chevron_button(id, title, Sf::ChevronDown, size, window, cx)
}

fn chevron_button(
    id: impl Into<ElementId>,
    title: impl Into<SharedString>,
    chevron: Sf,
    size: PushSize,
    window: &Window,
    cx: &App,
) -> Button {
    let k = native_colors(cx);
    push_button(id, "", PushStyle::Standard, size, cx)
        .pl(size.pad())
        .pr(metrics::FIELD_PAD)
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(6.))
                .child(div().flex_1().min_w(px(0.)).child(title.into()))
                .child(symbol(
                    chevron,
                    metrics::POPUP_CHEVRON_SLOT,
                    Weight::Semibold,
                    k.secondary,
                    window,
                )),
        )
}

/// The joined 22 × 22 `+` / `−` pair under a list (§5.2): one `fill`
/// capsule-less rounded rect with two borderless halves.
pub fn square_pair(add: Button, remove: Button, cx: &App) -> gpui::Div {
    let k = native_colors(cx);
    div()
        .flex_none()
        .h(metrics::SQUARE_BUTTON)
        .flex()
        .items_center()
        .rounded(metrics::BUTTON_REGULAR_RADIUS)
        .bg(k.fill)
        .overflow_hidden()
        .child(add)
        .child(div().w(metrics::HAIRLINE).h(px(12.)).bg(k.separator))
        .child(remove)
}

/// One half of a [`square_pair`]: `plus` or the minus sign.
pub fn square_button(
    id: impl Into<ElementId>,
    add: bool,
    name: impl Into<SharedString>,
    window: &Window,
    cx: &App,
) -> Button {
    let k = native_colors(cx);
    let name = name.into();
    let glyph: AnyElement = if add {
        symbol(Sf::Plus, px(12.), Weight::Medium, k.label, window)
    } else {
        // SF Symbols has `minus`, but the shared symbol table does not list
        // it yet: the system font's minus sign draws the same stroke.
        div()
            .n_body()
            .text_color(k.label)
            .child(crate::copy::native_kit::MINUS_SIGN)
            .into_any_element()
    };
    Button::new(id)
        .custom(
            ButtonCustomVariant::new(cx)
                .color(gpui::transparent_black())
                .foreground(k.label)
                .hover(gpui::transparent_black())
                .active(k.fill_hover),
        )
        .with_size(metrics::SQUARE_BUTTON)
        .size(metrics::SQUARE_BUTTON)
        .p_0()
        .rounded(px(0.))
        .accessibility_label(name.clone())
        .tooltip(name)
        .child(glyph)
}

/// A borderless symbol button (inspector `arrow.up.right.square`, row
/// tools): 24 × 24, radius 6, a 13 pt `secondary` symbol, `fillHover` on
/// hover.
pub fn borderless_button(
    id: impl Into<ElementId>,
    sf: Sf,
    name: impl Into<SharedString>,
    window: &Window,
    cx: &App,
) -> Button {
    let k = native_colors(cx);
    let name = name.into();
    Button::new(id)
        .custom(borderless_variant(cx))
        .with_size(metrics::BUTTON_REGULAR)
        .size(metrics::BUTTON_REGULAR)
        .p_0()
        .rounded(metrics::BUTTON_REGULAR_RADIUS)
        .accessibility_label(name.clone())
        .tooltip(name)
        .child(symbol(sf, px(15.), Weight::Regular, k.secondary, window))
}

/// A borderless text button (`编辑…` in a row): Body 13 `label`, 24 pt,
/// hover `fillHover`.
pub fn borderless_text(
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    cx: &App,
) -> Button {
    let k = native_colors(cx);
    Button::new(id)
        .custom(borderless_variant(cx))
        .label(label)
        .with_size(gpui_component::Size::Small)
        .h(metrics::BUTTON_REGULAR)
        .px(px(8.))
        .rounded(metrics::BUTTON_REGULAR_RADIUS)
        .text_color(k.label)
        .n_body()
}

/// How a toolbar item draws (§5.1).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ToolbarItemKind {
    /// A plain item: `label` symbol, `fillHover` hover.
    Plain,
    /// A toggled item (inspector shown): a 28 pt `selection` pill inset 4 pt
    /// with the symbol in `accent`.
    Toggled,
    /// The one primary action of a window: `accent` circle, `onAccent`
    /// symbol, at the trailing edge.
    Prominent,
}

/// A 36 × 36 toolbar item with a 15 pt Medium symbol (§5.1, §6). Name it:
/// the name is its tooltip and what VoiceOver reads.
pub fn toolbar_item(
    id: impl Into<ElementId>,
    sf: Sf,
    name: impl Into<SharedString>,
    kind: ToolbarItemKind,
    emphasized: bool,
    window: &Window,
    cx: &App,
) -> Button {
    let k = native_colors(cx);
    let name = name.into();
    let ink = match kind {
        ToolbarItemKind::Prominent => k.on_accent,
        ToolbarItemKind::Toggled if emphasized => k.accent_text,
        _ if emphasized => k.label,
        _ => k.secondary,
    };
    let glyph = symbol(sf, metrics::TOOLBAR_SYMBOL, Weight::Medium, ink, window);
    let variant = match kind {
        ToolbarItemKind::Prominent => ButtonCustomVariant::new(cx)
            .color(k.accent)
            .foreground(k.on_accent)
            .hover(k.accent)
            .active(k.accent.opacity(0.82)),
        _ => borderless_variant(cx),
    };
    Button::new(id)
        .custom(variant)
        .with_size(metrics::TOOLBAR_ITEM)
        .size(metrics::TOOLBAR_ITEM)
        .p_0()
        .rounded(metrics::TOOLBAR_ITEM / 2.)
        .when(kind == ToolbarItemKind::Prominent, |b| b.bg(k.accent))
        .accessibility_label(name.clone())
        .tooltip(name)
        .map(|b| match kind {
            ToolbarItemKind::Toggled => b.child(
                div()
                    .size(metrics::TOOLBAR_PILL)
                    .rounded_full()
                    .bg(k.selection)
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(glyph),
            ),
            _ => b.child(glyph),
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn push_sizes_follow_tahoe() {
        assert_eq!(PushSize::Regular.height(), px(24.));
        assert_eq!(PushSize::Regular.radius(), px(6.));
        assert_eq!(PushSize::Small.height(), px(20.));
        assert_eq!(PushSize::Small.radius(), px(5.));
        // Large is a capsule.
        assert_eq!(PushSize::Large.radius() * 2., PushSize::Large.height());
    }

    #[test]
    fn destructive_looks_standard_and_default_uses_accent() {
        let k = NativeColors::dark();
        assert_eq!(
            push_colors(PushStyle::Destructive, &k),
            push_colors(PushStyle::Standard, &k)
        );
        assert_eq!(push_colors(PushStyle::Default, &k).0, k.accent);
        // No hover fill on push buttons: hover equals the rest fill.
        assert_ne!(push_colors(PushStyle::Standard, &k).2, k.fill);
    }
}
