//! pages track: the native list parts the 首页, 群 and 成员 pages share
//! (NATIVE.md §9.4, §9.5): content section headers, Mail-like rows with an
//! avatar, a Headline title, one `secondary` line and a trailing time, and
//! the toolbar inset of a page that draws its own toolbar row.
//!
//! Rows have no hover fill and the arrow cursor (§10); a row that is
//! selected uses the native list selection of `ui::native::CompactRow`.

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
/// The gap between a row's avatar and its text.
pub const ROW_GAP: Pixels = px(10.);
/// The padding inside a row, around its selection fill.
pub const ROW_PAD: Pixels = px(8.);
/// The one leading content inset (§2): the toolbar title starts 20 pt after
/// the sidebar, and section headers, row avatars and detail titles start
/// on that same edge. It follows the toolbar leading token, so a change of
/// the shell's inset moves every page with it.
pub const LEADING: Pixels = metrics::TOOLBAR_LEADING;
/// The gap between the two buttons of a row.
pub const BUTTON_GAP: Pixels = px(6.);

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
pub fn section_header(
    label: impl Into<SharedString>,
    count: Option<String>,
    cx: &App,
) -> gpui::Div {
    let k = native_colors(cx);
    div()
        .flex_none()
        .h(px(28.))
        .mt(px(12.))
        .px(LEADING)
        .flex()
        .items_center()
        .gap(px(6.))
        .child(div().n_headline().text_color(k.label).child(label.into()))
        .children(count.map(|n| div().n_body().n_figures().text_color(k.secondary).child(n)))
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

/// A Mail-like list row (§9.4): `leading` (an avatar 32) on the shared
/// leading edge, the title (Headline) with one trailing slot on its line
/// (the time, or the progress of a run), and one `secondary` line under it
/// with an optional trailing slot (small push buttons, a progress). Rows
/// have no other trailing column, so every time sits at the trailing edge.
/// `separator` draws the hairline under the row, inset to the text column.
/// The caller adds `on_click` and `context_menu`.
#[allow(clippy::too_many_arguments)]
pub fn mail_row(
    id: impl Into<ElementId>,
    height: Pixels,
    leading: AnyElement,
    title: impl Into<SharedString>,
    title_trailing: Option<AnyElement>,
    second: AnyElement,
    second_trailing: Option<AnyElement>,
    separator: bool,
    ink: RowInk,
    cx: &App,
) -> Stateful<gpui::Div> {
    let k = native_colors(cx);
    div()
        .id(id.into())
        .relative()
        .flex_none()
        .h(height)
        .mx(LEADING - ROW_PAD)
        .px(ROW_PAD)
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
                .gap(px(2.))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(8.))
                        .child(line(title).flex_1().n_headline().text_color(ink.title))
                        .children(
                            title_trailing
                                .map(|t| div().flex_none().flex().items_center().child(t)),
                        ),
                )
                .child(
                    // 20 pt, the height of a small push button, so rows with
                    // and without buttons keep one rhythm.
                    div()
                        .h(metrics::BUTTON_SMALL)
                        .min_w(px(0.))
                        .flex()
                        .items_center()
                        .gap(px(8.))
                        .n_subheadline()
                        .text_color(ink.secondary)
                        .child(div().flex_1().min_w(px(0.)).child(second))
                        .children(second_trailing.map(|t| {
                            div()
                                .flex_none()
                                .flex()
                                .items_center()
                                .gap(BUTTON_GAP)
                                .child(t)
                        })),
                ),
        )
        .when(separator && ink.fill.is_none(), |d| {
            d.child(
                div()
                    .absolute()
                    .bottom_0()
                    .left(ROW_PAD + AVATAR + ROW_GAP)
                    .right(px(0.))
                    .h(metrics::HAIRLINE)
                    .bg(k.separator),
            )
        })
}

/// The time in a row's title-line slot: Subheadline, tabular figures.
pub fn time_slot(text: impl Into<SharedString>, color: Hsla) -> AnyElement {
    div()
        .n_subheadline()
        .n_figures()
        .text_color(color)
        .child(text.into())
        .into_any_element()
}

/// The progress of a run in a row's trailing slot: the `blue` live dot and
/// `3/5` (§6 Live work).
pub fn progress_slot(n: usize, m: usize, dot: Hsla, text: Hsla) -> AnyElement {
    div()
        .flex()
        .items_center()
        .gap(px(4.))
        .child(sym(Sym::Live, SymSize::INLINE_SMALL, dot))
        .child(
            div()
                .n_subheadline()
                .n_figures()
                .text_color(text)
                .child(format!("{n}/{m}")),
        )
        .into_any_element()
}

/// A person without a photo as a letter disk (§4): `quaternary` fill with
/// `label` text, mine included, so no bright disk outshines the window.
/// Members with a photo or a character tile use `MemberAvatar`.
pub fn avatar(member: Option<&crate::data::Member>, size: Pixels, cx: &App) -> AnyElement {
    let photo = member
        .and_then(|m| m.avatar_url.as_deref())
        .map(|p| p.strip_prefix("file://").unwrap_or(p))
        .is_some_and(|p| p.starts_with('/'));
    let letter = member.is_some_and(|m| !m.is_bot() && m.character.is_none()) && !photo;
    if !letter {
        return crate::ui::MemberAvatar::new(member, size).into_any_element();
    }
    let k = native_colors(cx);
    let name = member.map(|m| m.name.as_str()).unwrap_or_default();
    let text = crate::ui::avatar::monogram(name, size);
    let share = if text.chars().count() > 1 { 0.38 } else { 0.44 };
    div()
        .flex_none()
        .size(size)
        .rounded_full()
        .bg(k.quaternary)
        .text_color(k.label)
        .when(member.is_some_and(|m| m.deactivated), |d| d.opacity(0.5))
        .flex()
        .items_center()
        .justify_center()
        .text_size((size * share).round().max(px(9.)))
        .line_height(size)
        .font_weight(gpui::FontWeight::MEDIUM)
        .child(text)
        .into_any_element()
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

/// A toolbar group capsule (§5.1) around an item that is not a plain
/// `Button` (a pull-down toolbar item): the look of `ToolbarGroup`.
pub fn toolbar_capsule(item: impl IntoElement, cx: &App) -> gpui::Div {
    let k = native_colors(cx);
    div()
        .flex_none()
        .h(metrics::TOOLBAR_GROUP)
        .flex()
        .items_center()
        .rounded(metrics::TOOLBAR_GROUP / 2.)
        .bg(k.capsule)
        .shadow(k.glass())
        .child(item)
}
