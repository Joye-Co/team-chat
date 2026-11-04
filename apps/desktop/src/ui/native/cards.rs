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
use crate::ui::{
    sf_symbols::{Sym, SymSize},
    truncate::TruncText,
};

/// Card metrics: the compact card of COMPLETENESS.md §2.4, on the scales of
/// ui/native/tokens.rs.
pub mod card {
    use crate::ui::native::tokens::{avatar, height, radius, space};
    use gpui::Pixels;

    /// Corner radius of the card container.
    pub const RADIUS: Pixels = radius::CARD;
    /// Insets on every side (compact card p 16).
    pub const INSET: Pixels = space::S16;
    /// Space between the facts of a fact run.
    pub const FACT_GAP: Pixels = space::S12;
    /// Space between a symbol and its text in a run.
    pub const SYMBOL_GAP: Pixels = space::S4;
    /// The header avatar.
    pub const AVATAR: Pixels = avatar::SM;
    /// Space between the header, the title, the blocks and the footer
    /// (compact card gap 16).
    pub const STACK: Pixels = space::S16;
    /// Inner wells (draft, diff, code).
    pub const WELL_RADIUS: Pixels = radius::CONTROL;
    /// Thumbnails inside a card.
    pub const THUMB_RADIUS: Pixels = radius::ITEM;
    /// The scrubber of a media block.
    pub const SCRUBBER: Pixels = height::PROGRESS;
}

/// The card container (COMPLETENESS.md §2.4, §1.7): the card surface,
/// radius 14, a 1 px border, shadow sm; a fill step alone is not an edge.
/// Callers add the insets where the content starts (a media block runs to
/// the edges).
pub fn card_frame(cx: &App) -> gpui::Div {
    super::blocks::card_surface(cx).w_full().overflow_hidden()
}

/// A well inside a card (draft, diff, code, compare panels, rank lists,
/// time slots, "话题里" rows): radius 8, a 1 px border on the card fill
/// (T1-04, §1.7: a fill step alone is not an edge). No muted fill, so muted
/// text inside keeps 4.5:1.
pub fn card_well(cx: &App) -> gpui::Div {
    let k = native_colors(cx);
    div()
        .rounded(card::WELL_RADIUS)
        .border_1()
        .border_color(k.separator)
        .bg(k.sheet)
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

/// The state part of a card header: the state symbol (the only part in the
/// state color), the state words in `label` and an optional `secondary`
/// detail, one line (T1-05, §1.3, §2.12).
///
/// The words never run into the card edge: the detail shrinks first, then
/// the words cut with `…` and show the whole line in a tooltip. `_ink` is
/// ignored (the words were the state color before); callers drop it when
/// they move to [`state_words_parts`].
pub fn state_words(
    mark: AnyElement,
    label: impl Into<SharedString>,
    _ink: Hsla,
    detail: Option<SharedString>,
    cx: &App,
) -> gpui::Div {
    state_words_parts(mark, label, None, None, detail, cx)
}

/// [`state_words`] with a name inside the words (`等 {name} 拍板`): the
/// name cuts first, so the verb after it stays.
pub fn state_words_parts(
    mark: AnyElement,
    lead: impl Into<SharedString>,
    name: Option<SharedString>,
    tail: Option<SharedString>,
    detail: Option<SharedString>,
    cx: &App,
) -> gpui::Div {
    let k = native_colors(cx);
    let lead: SharedString = lead.into();
    let whole: SharedString = match (&name, &tail) {
        (None, None) => lead.clone(),
        _ => format!(
            "{lead}{}{}",
            name.as_deref().unwrap_or(""),
            tail.as_deref().unwrap_or("")
        )
        .into(),
    };
    div()
        .min_w(px(0.))
        .flex()
        .items_center()
        .gap(card::SYMBOL_GAP + px(2.))
        .n_body()
        .text_color(k.label)
        .child(div().flex_none().child(mark))
        .child(
            div()
                .min_w(px(0.))
                .flex()
                .items_center()
                .child(
                    // With a name, the name cuts first; alone, the words
                    // cut at the card edge.
                    TruncText::new(lead)
                        .when(name.is_some(), |t| t.flex_shrink_0().max_w(px(160.)))
                        .tip(whole.clone()),
                )
                .when_some(name, |d, n| {
                    d.child(TruncText::new(n).min_w(px(24.)).tip(whole.clone()))
                })
                .when_some(tail, |d, t| {
                    d.child(div().flex_none().whitespace_nowrap().child(t))
                }),
        )
        .when_some(detail.filter(|d| !d.is_empty()), |d, text| {
            // The detail takes only the space left, so it cuts before the
            // state words do.
            d.child(
                TruncText::new(text)
                    .flex_1()
                    .flex_basis(px(0.))
                    .text_color(k.secondary),
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
        .child(TruncText::new(text))
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
