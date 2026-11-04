//! pages track: the native list parts the 首页, 群 and 成员 pages share
//! (NATIVE.md §9.4, §9.5): content section headers, Mail-like rows with an
//! avatar, a Headline title, one `secondary` line and a trailing time, and
//! the toolbar inset of a page that draws its own toolbar row.
//!
//! Rows have no hover fill and the arrow cursor (§10); a row that is
//! selected uses the native list selection of `ui::native::ListRow`.

use gpui::{
    AnyElement, App, ElementId, Hsla, InteractiveElement as _, IntoElement, ParentElement, Pixels,
    SharedString, Stateful, Styled, div, prelude::FluentBuilder as _, px,
};

use crate::{
    app::Services,
    ui::{
        icons,
        native::{NativeType as _, metrics, native_colors},
        sf_symbols::{Sym, SymSize},
    },
};

/// A Mail-like row (§9.4): 56 pt.
pub const ROW: Pixels = px(56.);
/// A 成员 list row (§9.5): 52 pt.
pub const MEMBER_ROW: Pixels = px(52.);
/// The avatar of a list row.
pub const AVATAR: Pixels = px(32.);
/// The accessory slot of a row: wide enough for two small push buttons,
/// so the times of rows with and without buttons line up.
pub const ACCESSORY_MIN: Pixels = px(168.);
/// The gap between a row's avatar and its text.
pub const ROW_GAP: Pixels = px(10.);
/// Where a list's text column starts: the row inset, its padding, the
/// avatar and the gap. Section headers and separators line up with it.
pub const TEXT_START: Pixels = px(8. + 8.);

/// Where a page's toolbar title starts: 20 pt after the sidebar, or after
/// the traffic lights when the sidebar is hidden (§2).
pub fn toolbar_leading(services: &Services, cx: &App) -> Pixels {
    let lights = super::kit::leading(services, cx);
    if lights > px(0.) {
        lights
    } else {
        metrics::TOOLBAR_LEADING
    }
}

/// A content section header (§3 Headline): the name in `label` and an
/// optional `secondary` count, lined up with the rows' text.
pub fn section_header(label: impl Into<SharedString>, count: Option<String>, cx: &App) -> gpui::Div {
    let k = native_colors(cx);
    div()
        .flex_none()
        .h(px(28.))
        .mt(px(12.))
        .px(TEXT_START)
        .flex()
        .items_center()
        .gap(px(6.))
        .child(div().n_headline().text_color(k.label).child(label.into()))
        .children(count.map(|n| {
            div()
                .n_body()
                .n_figures()
                .text_color(k.secondary)
                .child(n)
        }))
}

/// An SF Symbol at a §6 context size in `color`.
pub fn sym(sym: Sym, size: SymSize, color: Hsla) -> AnyElement {
    icons::sym(sym, size, color).into_any_element()
}

/// One line of text cut with `…`.
pub fn line(text: impl Into<SharedString>) -> gpui::Div {
    div()
        .min_w(px(0.))
        .overflow_hidden()
        .whitespace_nowrap()
        .text_ellipsis()
        .child(text.into())
}

/// The ink of a row: (title, secondary, fill) for its selection state.
#[derive(Clone, Copy)]
pub struct RowInk {
    pub title: Hsla,
    pub secondary: Hsla,
    pub fill: Option<Hsla>,
}

impl RowInk {
    pub fn new(selection: Option<(Hsla, Hsla)>, cx: &App) -> Self {
        let k = native_colors(cx);
        match selection {
            // On `accent` the secondary text is `onAccent`, a little quieter.
            Some((fill, text)) if fill == k.accent => Self {
                title: text,
                secondary: text.opacity(0.78),
                fill: Some(fill),
            },
            Some((fill, _)) => Self {
                title: k.label,
                secondary: k.secondary,
                fill: Some(fill),
            },
            None => Self {
                title: k.label,
                secondary: k.secondary,
                fill: None,
            },
        }
    }
}

/// A Mail-like list row: `leading` (an avatar 32), the title (Headline) with
/// an optional trailing time on its line, one `secondary` line under it and
/// an optional `accessory` (small push buttons, a state symbol) at the
/// trailing edge. `separator` draws the hairline under the row, inset to
/// the text column. The caller adds `on_click` and `context_menu`.
#[allow(clippy::too_many_arguments)]
pub fn mail_row(
    id: impl Into<ElementId>,
    height: Pixels,
    leading: AnyElement,
    title: impl Into<SharedString>,
    time: Option<String>,
    second: AnyElement,
    accessory: Option<AnyElement>,
    separator: bool,
    ink: RowInk,
    cx: &App,
) -> Stateful<gpui::Div> {
    let k = native_colors(cx);
    let text_x = metrics::LIST_ROW_INSET + px(8.) + AVATAR + ROW_GAP;
    div()
        .id(id.into())
        .relative()
        .flex_none()
        .h(height)
        .mx(metrics::LIST_ROW_INSET)
        .px(px(8.))
        .rounded(metrics::LIST_ROW_RADIUS)
        .when_some(ink.fill, |d, f| d.bg(f))
        .flex()
        .items_center()
        .gap(ROW_GAP)
        .child(div().flex_none().child(leading))
        .child(
            div()
                .flex_1()
                .min_w(px(0.))
                .flex()
                .flex_col()
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(8.))
                        .child(
                            line(title)
                                .flex_1()
                                .n_headline()
                                .text_color(ink.title),
                        )
                        .children(time.map(|t| {
                            div()
                                .flex_none()
                                .n_subheadline()
                                .n_figures()
                                .text_color(ink.secondary)
                                .child(t)
                        })),
                )
                .child(
                    div()
                        .min_w(px(0.))
                        .n_subheadline()
                        .text_color(ink.secondary)
                        .child(second),
                ),
        )
        .children(accessory.map(|a| {
            div()
                .flex_none()
                .min_w(ACCESSORY_MIN)
                .justify_end()
                .flex()
                .items_center()
                .gap(px(8.))
                .child(a)
        }))
        .when(separator && ink.fill.is_none(), |d| {
            d.child(
                div()
                    .absolute()
                    .bottom_0()
                    .left(text_x - metrics::LIST_ROW_INSET)
                    .right(px(0.))
                    .h(metrics::HAIRLINE)
                    .bg(k.separator),
            )
        })
}

/// The secondary line of a row as runs: an optional state symbol and text
/// in the state color, then `secondary` text.
pub fn state_line(
    state: Option<(Sym, Hsla, String)>,
    rest: Option<String>,
    secondary: Hsla,
) -> AnyElement {
    let has_state = state.is_some();
    div()
        .flex()
        .items_center()
        .gap(px(4.))
        .min_w(px(0.))
        .children(state.map(|(s, color, text)| {
            div()
                .flex_none()
                .flex()
                .items_center()
                .gap(px(3.))
                .child(sym(s, SymSize::INLINE_SMALL, color))
                .child(div().text_color(color).child(text))
        }))
        .children(rest.map(|r| {
            line(if has_state { format!("· {r}") } else { r })
                .flex_1()
                .text_color(secondary)
        }))
        .into_any_element()
}
