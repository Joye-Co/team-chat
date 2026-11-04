//! Reactions as Messages-style tapbacks (NATIVE.md rule 4: no pills): an
//! emoji and a count in a quiet `fill` bubble with no outline. The viewer's
//! own reaction has an accent tint. The names show on hover in a popover.

use gpui::{
    AnyElement, App, ElementId, IntoElement, ParentElement as _, SharedString, Styled as _, div,
    prelude::FluentBuilder as _, px,
};
use gpui_component::{
    button::{Button, ButtonVariants as _},
    hover_card::HoverCard,
};

use crate::ui::{
    Sym,
    native::{NativeType as _, native_colors, popover_surface},
};

/// The bubble: 22 pt tall, fully rounded; the emoji is 14 pt.
pub const BUBBLE: gpui::Pixels = crate::ui::native::tokens::height::SM;
const EMOJI: gpui::Pixels = px(14.);
/// The accent tint of my own reaction.
const MINE_TINT: f32 = 0.28;
/// Where the popover arrow points: the middle of a one-emoji bubble.
const ARROW_X: gpui::Pixels = px(16.);

/// One tapback bubble. `count` shows only when more than one member
/// reacted (as in Messages).
pub fn bubble(
    id: impl Into<ElementId>,
    emoji: SharedString,
    count: usize,
    mine: bool,
    cx: &App,
) -> Button {
    // COMPLETENESS.md F30: "mine" is the `muted` fill with a foreground
    // border (the kit's selected chip), never a tint.
    let k = native_colors(cx);
    crate::ui::native::chip_button(id, mine, cx).child(
        div()
            .flex()
            .items_center()
            .gap(crate::ui::native::tokens::space::S4)
            .child(div().text_size(EMOJI).line_height(BUBBLE).child(emoji))
            .when(count > 1, |el| {
                el.child(
                    div()
                        .n_section()
                        .n_figures()
                        .text_color(if mine { k.label } else { k.secondary })
                        .child(crate::ui::native::badge::cap(count as u32)),
                )
            }),
    )
}

/// A tapback with the names of the members who reacted in a hover popover.
pub fn with_names(id: impl Into<ElementId>, trigger: Button, names: SharedString) -> AnyElement {
    HoverCard::new(id)
        .anchor(gpui::Anchor::TopLeft)
        .appearance(false)
        .open_delay(std::time::Duration::from_millis(350))
        .close_delay(std::time::Duration::from_millis(80))
        .trigger(trigger)
        .content(move |_, _, cx| {
            let k = native_colors(cx);
            div().pt(px(6.)).child(
                popover_surface(ARROW_X, cx)
                    .max_w(px(280.))
                    .px(px(10.))
                    .py(px(6.))
                    .child(div().n_body().text_color(k.label).child(names.clone())),
            )
        })
        .into_any_element()
}

/// The bubble after the tapbacks that opens the picker: borderless, the
/// `face.smiling` symbol in `secondary`.
pub fn add_bubble(id: impl Into<ElementId>, cx: &App) -> Button {
    crate::ui::native::icon_button_sym(
        id,
        Sym::Emoji,
        crate::copy::timeline::ADD_REACTION,
        crate::ui::native::IconSize::Small,
        cx,
    )
}
