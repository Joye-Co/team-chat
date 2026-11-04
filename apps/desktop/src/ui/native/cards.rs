//! Track cards: the native parts of a Card v2 message (NATIVE.md §9.2,
//! DESIGN.md §7.10 semantics). A card is the one container in the
//! transcript: radius 12, `fill`, no border, no shadow, 14 pt insets. Its
//! header is the member name in Headline plus a state symbol and text; its
//! facts are SF Symbol + text runs 12 pt apart, never chips.
//!
//! The kit had no card parts, so they live here (the `cards` track file).

use gpui::{
    AnyElement, App, Hsla, IntoElement, ParentElement, SharedString, Styled, Window, div,
    prelude::FluentBuilder as _, px,
};

use super::tokens::{NativeType as _, native_colors};
use crate::ui::sf_symbols::{Sym, SymSize};

/// Card metrics (§9.2).
pub mod card {
    use gpui::{Pixels, px};

    /// Corner radius of the card container.
    pub const RADIUS: Pixels = px(12.);
    /// Insets on every side.
    pub const INSET: Pixels = px(14.);
    /// Space between the facts of a fact run.
    pub const FACT_GAP: Pixels = px(12.);
    /// Space between a symbol and its text in a run.
    pub const SYMBOL_GAP: Pixels = px(4.);
    /// The header avatar.
    pub const AVATAR: Pixels = px(20.);
    /// Space between the header, the title, the blocks and the footer.
    pub const STACK: Pixels = px(10.);
    /// Inner wells (draft, diff, code): radius 8.
    pub const WELL_RADIUS: Pixels = px(8.);
    /// Thumbnails inside a card: radius 6.
    pub const THUMB_RADIUS: Pixels = px(6.);
    /// The scrubber of a media block.
    pub const SCRUBBER: Pixels = px(4.);
}

/// The card container: radius 12, `fill`, no border, no shadow. Callers add
/// the 14 pt insets where the content starts (a media block runs to the
/// edges).
pub fn card_frame(cx: &App) -> gpui::Div {
    let k = native_colors(cx);
    div()
        .w_full()
        .rounded(card::RADIUS)
        .bg(k.fill)
        .overflow_hidden()
}

/// A quiet well inside a card (draft, diff, code): `fill` on the card's
/// `fill`, radius 8, no border.
pub fn card_well(cx: &App) -> gpui::Div {
    let k = native_colors(cx);
    div().rounded(card::WELL_RADIUS).bg(k.fill)
}

/// A symbol + text run (`arrow.triangle.pull` `#61`): Callout 12, the
/// symbol at the inline small size in `secondary` (or the state color when
/// the symbol is the state), the text in `color`.
pub fn fact(
    sym: Option<Sym>,
    text: impl Into<SharedString>,
    symbol_color: Option<Hsla>,
    color: Option<Hsla>,
    cx: &App,
) -> gpui::Div {
    let k = native_colors(cx);
    div()
        .flex_none()
        .flex()
        .items_center()
        .gap(card::SYMBOL_GAP)
        .n_callout()
        .n_figures()
        .text_color(color.unwrap_or(k.secondary))
        .when_some(sym, |d, s| {
            d.child(
                s.icon(SymSize::INLINE_SMALL)
                    .text_color(symbol_color.unwrap_or(k.secondary)),
            )
        })
        .child(div().whitespace_nowrap().child(text.into()))
}

/// Facts in one run, 12 pt apart, wrapping when they do not fit.
pub fn fact_run(facts: impl IntoIterator<Item = AnyElement>) -> gpui::Div {
    div()
        .flex()
        .flex_wrap()
        .items_center()
        .gap_x(card::FACT_GAP)
        .gap_y(px(4.))
        .children(facts)
}

/// A PR fact run: `arrow.triangle.pull` `#61`, green `+214`, red `−63`.
pub fn pr_facts(number: &str, added: Option<&str>, removed: Option<&str>, cx: &App) -> gpui::Div {
    let k = native_colors(cx);
    let mut parts: Vec<AnyElement> = vec![
        fact(
            Some(Sym::PullRequest),
            number.to_string(),
            None,
            Some(k.label),
            cx,
        )
        .into_any_element(),
    ];
    if let Some(a) = added {
        parts.push(fact(None, a.to_string(), None, Some(k.green), cx).into_any_element());
    }
    if let Some(r) = removed {
        parts.push(fact(None, r.to_string(), None, Some(k.red), cx).into_any_element());
    }
    div()
        .flex_none()
        .flex()
        .items_center()
        .gap(card::SYMBOL_GAP + px(2.))
        .children(parts)
}

/// The state part of a card header: a 13 pt state symbol, the state word in
/// the state color and an optional `secondary` detail, one line.
pub fn state_words(
    mark: AnyElement,
    label: impl Into<SharedString>,
    ink: Hsla,
    detail: Option<SharedString>,
    cx: &App,
) -> gpui::Div {
    let k = native_colors(cx);
    div()
        .min_w(px(0.))
        .flex()
        .items_center()
        .gap(card::SYMBOL_GAP + px(2.))
        .n_body()
        .child(mark)
        .child(
            div()
                .flex_none()
                .whitespace_nowrap()
                .text_color(ink)
                .child(label.into()),
        )
        .when_some(detail.filter(|d| !d.is_empty()), |d, text| {
            d.child(
                div()
                    .min_w(px(0.))
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .text_ellipsis()
                    .text_color(k.secondary)
                    .child(text),
            )
        })
}

/// A 13 pt state symbol in its color.
pub fn state_symbol(sym: Sym, color: Hsla) -> AnyElement {
    sym.icon(SymSize::INLINE)
        .text_color(color)
        .into_any_element()
}

/// Quiet inline reference text (a quote or a reference in the composer or
/// a message): a 13 pt `secondary` symbol and the text, no chip.
pub fn inline_ref(sym: Sym, text: impl Into<SharedString>, cx: &App) -> gpui::Div {
    let k = native_colors(cx);
    div()
        .min_w(px(0.))
        .flex()
        .items_center()
        .gap(card::SYMBOL_GAP)
        .n_body()
        .text_color(k.secondary)
        .child(sym.icon(SymSize::INLINE).text_color(k.secondary))
        .child(
            div()
                .min_w(px(0.))
                .overflow_hidden()
                .whitespace_nowrap()
                .text_ellipsis()
                .child(text.into()),
        )
}

/// A 4 pt media scrubber: `quaternary` track, `label` fill, a 10 pt knob
/// at the position when `knob` is set.
pub fn scrubber(value: f32, knob: bool, _window: &Window, cx: &App) -> gpui::Div {
    let k = native_colors(cx);
    let v = value.clamp(0., 1.);
    div()
        .relative()
        .w_full()
        .h(px(12.))
        .flex()
        .items_center()
        .child(
            div()
                .w_full()
                .h(card::SCRUBBER)
                .rounded(card::SCRUBBER / 2.)
                .bg(k.quaternary)
                .child(
                    div()
                        .h_full()
                        .w(gpui::relative(v))
                        .rounded(card::SCRUBBER / 2.)
                        .bg(k.label),
                ),
        )
        .when(knob, |d| {
            d.child(
                div()
                    .absolute()
                    .top(px(1.))
                    .left(gpui::relative(v))
                    .ml(px(-5.))
                    .size(px(10.))
                    .rounded_full()
                    .bg(k.knob)
                    .shadow(k.knob_shadow()),
            )
        })
}
