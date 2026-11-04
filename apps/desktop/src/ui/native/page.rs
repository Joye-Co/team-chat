//! Page parts (COMPLETENESS.md §2.3–§2.15, §2.18): the pieces the 首页, 群,
//! 成员 and 产出 pages share, so every page has the same recipe.
//!
//! | Part | Spec |
//! | --- | --- |
//! | [`section_header`], [`count_pill`] | §2.3 section header with a count pill |
//! | [`frame`], [`PageRow`], [`list_footer`] | §2.6 a framed list of rows with fixed columns and a footer |
//! | [`kpi_card`], [`card`] | §2.4 KPI card and card |
//! | [`status_badge`] | §2.12 StatusBadge: outline badge with a colored 12 icon |
//! | [`page_alert`] | §2.15 Alert: neutral frame, state icon, title, description, action |
//! | [`page_empty`], [`compact_empty`] | §2.13 Empty state, full and compact |
//! | [`skeleton_rows`], [`skeleton_cards`] | §2.14 loading in the final shapes |
//! | [`PageLoad`] | which of ready, loading or failed a page shows |
//!
//! Colors come from [`native_colors`]; text from the [`NativeType`] roles.

use crate::ui::truncate::TruncText;
use gpui::{
    AnyElement, App, ElementId, Hsla, InteractiveElement as _, IntoElement, ParentElement, Pixels,
    SharedString, Stateful, Styled, div, prelude::FluentBuilder as _, px,
};
use gpui_component::tag::Tag;

use super::{FocusRow as _, NativeType as _, feedback::Bone, native_colors};
use crate::{
    app::Services,
    data::ConnectionState,
    ui::{
        icons,
        sf_symbols::{Sym, SymSize, Weight},
    },
};

/// The spacing, radius and size steps of the page parts (§1.5, §1.6, §2.6).
pub mod m {
    use gpui::{Pixels, px};

    /// Card padding, the top and bottom padding of a page and the gap
    /// between sections.
    pub const GUTTER: Pixels = px(24.);
    /// The leading and trailing edge of page content: the toolbar title's
    /// edge, so a page's lists start under its title (one leading edge).
    pub const EDGE: Pixels = crate::ui::native::metrics::CONTENT_LEADING;
    /// From a section header to its list, card or grid.
    pub const HEADER_GAP: Pixels = px(12.);
    /// Card grid and KPI gap.
    pub const GRID_GAP: Pixels = px(16.);
    /// A list frame, alert and empty state.
    pub const FRAME_RADIUS: Pixels = px(10.);
    /// A card.
    pub const CARD_RADIUS: Pixels = px(14.);
    /// A list row with media and a second line.
    pub const ROW: Pixels = px(56.);
    /// A compact single-line row.
    pub const COMPACT_ROW: Pixels = px(36.);
    /// The padding inside a row.
    pub const ROW_PAD: Pixels = px(12.);
    /// The media of a list row.
    pub const MEDIA: Pixels = px(32.);
    /// The footer row of a list.
    pub const FOOTER_ROW: Pixels = px(40.);
    /// A count pill.
    pub const PILL: Pixels = px(20.);
    /// A badge.
    pub const BADGE: Pixels = px(22.);
    /// The time column of a row: `昨天 09:40` in tabular figures.
    pub const TIME_COLUMN: Pixels = px(72.);
    /// The empty-state icon box.
    pub const EMPTY_BOX: Pixels = px(40.);
    /// The longest line of an empty-state description.
    pub const EMPTY_TEXT: Pixels = px(384.);
}

/// A 12 pt state icon (badge) and a 16 pt row icon.
const BADGE_ICON: SymSize = SymSize::new(12., 12., Weight::Regular);
const ROW_ICON: SymSize = SymSize::new(16., 16., Weight::Regular);
const EMPTY_ICON: SymSize = SymSize::new(22., 24., Weight::Regular);
/// The KPI number: 24 / 32 semibold in a block narrower than 250 (§1.4).
const KPI_NUMBER: (Pixels, Pixels) = (px(24.), px(32.));

/// A 16 pt symbol in `color`.
pub fn row_icon(sym: Sym, color: Hsla) -> AnyElement {
    icons::sym(sym, ROW_ICON, color).into_any_element()
}

/// A 32 media box (§2.6): radius 6, border, muted fill, a 16 icon.
pub fn icon_box(sym: Sym, color: Option<Hsla>, cx: &App) -> AnyElement {
    let k = native_colors(cx);
    div()
        .flex_none()
        .size(m::MEDIA)
        .rounded(px(6.))
        .border_1()
        .border_color(k.separator)
        .bg(k.fill)
        .flex()
        .items_center()
        .justify_center()
        .child(row_icon(sym, color.unwrap_or(k.label)))
        .into_any_element()
}

/// A count pill (§1.8): 20 high, 12 medium tabular, muted fill. Caps at 99+.
pub fn count_pill(n: usize, cx: &App) -> gpui::Div {
    let k = native_colors(cx);
    div()
        .flex_none()
        .h(m::PILL)
        .min_w(m::PILL)
        .px(px(6.))
        .rounded_full()
        .bg(k.fill)
        .flex()
        .items_center()
        .justify_center()
        .n_callout()
        .font_weight(gpui::FontWeight::MEDIUM)
        .n_figures()
        .text_color(k.label)
        .child(crate::ui::chip::CountBadge::cap(n as u32))
}

/// A section header (§2.3): title 16 semibold, an optional count pill after
/// it, an optional one-line description under it and an optional action at
/// the right, top-aligned with the title.
pub fn section_header(
    title: impl Into<SharedString>,
    count: Option<usize>,
    description: Option<SharedString>,
    action: Option<AnyElement>,
    cx: &App,
) -> gpui::Div {
    let k = native_colors(cx);
    div()
        .flex_none()
        .flex()
        .items_start()
        .gap(px(8.))
        .child(
            div()
                .flex_1()
                .min_w(px(0.))
                .flex()
                .flex_col()
                .gap(px(4.))
                .child(
                    div()
                        .h(px(28.))
                        .flex()
                        .items_center()
                        .gap(px(8.))
                        .child(
                            div()
                                .n_title3()
                                .text_color(k.label)
                                .whitespace_nowrap()
                                .child(title.into()),
                        )
                        .children(count.map(|n| count_pill(n, cx))),
                )
                .children(description.map(|d| TruncText::new(d).n_body().text_color(k.secondary))),
        )
        .children(action.map(|a| div().flex_none().h(px(28.)).flex().items_center().child(a)))
}

/// A list or table frame (§2.6, §2.7): radius 10, 1 px border, the page
/// background, clipped. Rows draw their own separators.
pub fn frame(cx: &App) -> gpui::Div {
    let k = native_colors(cx);
    div()
        .flex_none()
        .flex()
        .flex_col()
        .rounded(m::FRAME_RADIUS)
        .border_1()
        .border_color(k.separator)
        .bg(k.content)
        .overflow_hidden()
}

/// A card (§2.4): radius 14, 1 px border, card fill, shadow sm.
pub fn card(cx: &App) -> gpui::Div {
    let k = native_colors(cx);
    div()
        .flex()
        .flex_col()
        .rounded(m::CARD_RADIUS)
        .border_1()
        .border_color(k.separator)
        .bg(k.sheet)
        .shadow(shadow_sm(&k))
}

/// The xs and sm shadows of §1.7.
fn shadow_sm(k: &super::NativeColors) -> Vec<gpui::BoxShadow> {
    let a = if k.is_dark() { 0.3 } else { 0.06 };
    vec![gpui::BoxShadow {
        color: gpui::hsla(0., 0., 0., a),
        offset: gpui::point(px(0.), px(1.)),
        blur_radius: px(2.),
        spread_radius: px(0.),
        inset: false,
    }]
}

/// The fixed right columns of one list (§2.6): every row of a list passes
/// the same value, so its status, time and actions sit at one x.
#[derive(Clone, Copy, Debug, Default)]
pub struct Columns {
    pub status: Option<Pixels>,
    pub time: Option<Pixels>,
    pub actions: Option<Pixels>,
}

/// A list row (§2.6): h56, px12, gap 12: media 32, title 14 medium over a
/// 12 muted line, then the right cluster in a fixed order (status, time,
/// actions) at the widths of [`Columns`]. `separator` draws the 1 px line
/// under the row, inset to the title x. The caller adds `on_click`; the row
/// is a [`FocusRow`](super::FocusRow) (Tab, Enter / Space, ↑ / ↓, hover).
#[derive(Default)]
pub struct PageRow {
    pub media: Option<AnyElement>,
    pub title: SharedString,
    pub title_trailing: Option<AnyElement>,
    pub second: Option<AnyElement>,
    pub status: Option<AnyElement>,
    pub time: Option<SharedString>,
    pub actions: Option<AnyElement>,
    pub separator: bool,
    pub selected: bool,
    pub dim: bool,
}

impl PageRow {
    pub fn new(title: impl Into<SharedString>) -> Self {
        Self {
            title: title.into(),
            ..Default::default()
        }
    }

    pub fn render(self, id: impl Into<ElementId>, cols: Columns, cx: &App) -> Stateful<gpui::Div> {
        let k = native_colors(cx);
        let has_media = self.media.is_some();
        let title_x = m::ROW_PAD
            + if has_media {
                m::MEDIA + px(12.)
            } else {
                px(0.)
            };
        let mut row = div()
            .id(id.into())
            .relative()
            .flex_none()
            .h(m::ROW)
            .px(m::ROW_PAD)
            .flex()
            .items_center()
            .gap(px(12.))
            .when(self.selected, |d| d.bg(k.fill))
            // Every page row opens something: a tab stop with the focus
            // ring, Enter / Space run the caller's click, ↑ / ↓ walk the list.
            .focus_row(self.selected, cx)
            .when(self.dim, |d| d.opacity(0.7))
            .children(self.media.map(|m| div().flex_none().child(m)))
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
                            .min_w(px(0.))
                            .child(
                                TruncText::new(self.title)
                                    .n_body()
                                    .font_weight(gpui::FontWeight::MEDIUM)
                                    .text_color(k.label),
                            )
                            .children(self.title_trailing),
                    )
                    .children(self.second.map(|s| {
                        div()
                            .min_w(px(0.))
                            .h(px(16.))
                            .flex()
                            .items_center()
                            .overflow_hidden()
                            .n_callout()
                            .text_color(k.secondary)
                            .child(s)
                    })),
            );
        if let Some(w) = cols.status {
            row = row.child(
                div()
                    .flex_none()
                    .w(w)
                    .flex()
                    .justify_end()
                    .children(self.status),
            );
        }
        if let Some(w) = cols.time {
            row = row.child(
                div()
                    .flex_none()
                    .w(w)
                    .flex()
                    .justify_end()
                    .n_callout()
                    .n_figures()
                    .text_color(k.secondary)
                    .whitespace_nowrap()
                    .children(self.time),
            );
        }
        if let Some(w) = cols.actions {
            row = row.child(
                div()
                    .flex_none()
                    .w(w)
                    .flex()
                    .items_center()
                    .justify_end()
                    .gap(px(8.))
                    .children(self.actions),
            );
        }
        row.when(self.separator, |d| {
            d.child(
                div()
                    .absolute()
                    .bottom_0()
                    .left(title_x)
                    .right(px(0.))
                    .h(px(1.))
                    .bg(k.separator),
            )
        })
    }
}

/// The footer row of a list (§2.6): h40, border-t, 14 muted text (`看全部
/// 23 个话题 →`, or a count). The caller adds `on_click` when it opens more.
pub fn list_footer(
    id: impl Into<ElementId>,
    text: impl Into<SharedString>,
    trailing: Option<AnyElement>,
    cx: &App,
) -> Stateful<gpui::Div> {
    let k = native_colors(cx);
    div()
        .id(id.into())
        .flex_none()
        .h(m::FOOTER_ROW)
        .px(px(16.))
        .border_t_1()
        .border_color(k.separator)
        .flex()
        .items_center()
        .justify_between()
        .n_body()
        .text_color(k.secondary)
        .child(text.into())
        .children(trailing)
}

/// A KPI card (§2.4): label 14 muted, the number 24 semibold tabular, a
/// 14 muted footer line; radius 14, border, shadow xs, p 24.
pub fn kpi_card(
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    value: impl Into<SharedString>,
    foot: impl Into<SharedString>,
    icon: Option<(Sym, Hsla)>,
    cx: &App,
) -> Stateful<gpui::Div> {
    let k = native_colors(cx);
    card(cx)
        .id(id.into())
        .flex_1()
        .min_w(px(0.))
        // §2.4 KPI card (T1-11): p 24, footer 14 muted.
        .p(m::GUTTER)
        .gap(px(4.))
        .child(
            div()
                .flex()
                .items_center()
                .justify_between()
                .gap(px(8.))
                .child(TruncText::new(label).n_body().text_color(k.secondary))
                .children(icon.map(|(s, c)| row_icon(s, c))),
        )
        .child(
            div()
                .n_style(KPI_NUMBER)
                .font_weight(gpui::FontWeight::SEMIBOLD)
                .n_figures()
                .text_color(k.label)
                .child(value.into()),
        )
        .child(
            TruncText::new(foot)
                .mt(px(2.))
                .n_body()
                .text_color(k.secondary),
        )
}

/// The states of §2.12, each with one icon and one color.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    NeedsYou,
    Working,
    Stopping,
    Queued,
    Done,
    Failed,
    Abnormal,
    Online,
    Offline,
    Deactivated,
    Paused,
    Merging,
    /// A quiet waiting word (等李雪回答): clock, muted.
    Waiting,
}

impl Status {
    fn ink(self, k: &super::NativeColors) -> (Option<Sym>, Hsla) {
        match self {
            Status::NeedsYou => (Some(Sym::NeedsYou), k.amber),
            Status::Working => (Some(Sym::Spinner), k.blue),
            Status::Stopping => (Some(Sym::Spinner), k.secondary),
            Status::Queued => (Some(Sym::Waiting), k.secondary),
            Status::Done => (Some(Sym::Done), k.green),
            Status::Failed => (Some(Sym::Failed), k.red),
            Status::Abnormal => (Some(Sym::Warning), k.red),
            Status::Online => (None, k.green),
            Status::Offline => (None, k.secondary),
            Status::Deactivated => (Some(Sym::Stopped), k.secondary),
            Status::Paused => (Some(Sym::Pause), k.secondary),
            Status::Merging => (Some(Sym::Spinner), k.blue),
            Status::Waiting => (Some(Sym::Waiting), k.secondary),
        }
    }
}

/// A StatusBadge (§2.12): an outline badge, h22, 12 medium text in
/// `label`, with the state's 12 icon (or an 8 dot for presence) in its
/// color. Built on gpui-component's `Tag`.
pub fn status_badge(status: Status, text: impl Into<SharedString>, _cx: &App) -> AnyElement {
    // COMPLETENESS.md §2.12: the one StatusBadge of the kit.
    use super::badge::{BadgeState as B, StatusBadge};
    let state = match status {
        Status::NeedsYou => B::NeedsYou,
        Status::Working => B::Working,
        Status::Stopping => B::Stopping,
        Status::Queued => B::Queued,
        Status::Done => B::Done,
        Status::Failed => B::Failed,
        Status::Abnormal => B::Abnormal,
        Status::Online => B::Online,
        Status::Offline => B::Offline,
        Status::Deactivated => B::Deactivated,
        Status::Paused => B::Paused,
        Status::Merging => B::Merging,
        Status::Waiting => B::Quiet,
    };
    StatusBadge::new(state, text).into_any_element()
}

/// An outline badge with text only (a type, `v3`): h22, 12 medium.
pub fn outline_badge(text: impl Into<SharedString>, cx: &App) -> AnyElement {
    let k = native_colors(cx);
    Tag::custom(gpui::transparent_black(), k.label, k.separator)
        .outline()
        .rounded_full()
        .flex_none()
        .h(m::BADGE)
        .px(px(8.))
        .py(px(0.))
        .n_callout()
        .font_weight(gpui::FontWeight::MEDIUM)
        .text_color(k.label)
        .whitespace_nowrap()
        .child(text.into())
        .into_any_element()
}

/// The kind of an [`page_alert`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AlertKind {
    Info,
    Warning,
    Error,
}

/// An Alert (§2.15): radius 10, border, px16 py12, a 16 state icon, title
/// 14 medium, description 14 muted, an action at the right. The error kind
/// tints the icon and border; the title stays neutral (owner decision). gpui-component's `Alert` has no action
/// slot and no muted description, so the frame is drawn here once.
pub fn page_alert(
    id: impl Into<ElementId>,
    kind: AlertKind,
    title: impl Into<SharedString>,
    description: Option<SharedString>,
    action: Option<AnyElement>,
    cx: &App,
) -> Stateful<gpui::Div> {
    let k = native_colors(cx);
    let (sym, color) = match kind {
        AlertKind::Info => (Sym::Info, k.blue),
        AlertKind::Warning => (Sym::NeedsYou, k.amber),
        AlertKind::Error => (Sym::Warning, k.red),
    };
    let error = kind == AlertKind::Error;
    div()
        .id(id.into())
        .flex_none()
        .w_full()
        .px(px(16.))
        .py(px(12.))
        .rounded(m::FRAME_RADIUS)
        .border_1()
        .border_color(if error {
            k.red.opacity(0.5)
        } else {
            k.separator
        })
        .bg(k.content)
        .flex()
        .items_center()
        .gap(px(12.))
        .child(
            div()
                .flex_none()
                .self_start()
                .pt(px(2.))
                .child(row_icon(sym, color)),
        )
        .child(
            div()
                .flex_1()
                .min_w(px(0.))
                .flex()
                .flex_col()
                .gap(px(2.))
                .child(
                    div()
                        .n_body()
                        .font_weight(gpui::FontWeight::MEDIUM)
                        // Owner decision v14 (T1-11): neutral title; the
                        // state color is on the icon and the border.
                        .text_color(k.label)
                        .child(title.into()),
                )
                .children(description.map(|d| div().n_body().text_color(k.secondary).child(d))),
        )
        .children(action.map(|a| div().flex_none().child(a)))
}

/// A full-region Empty state (§2.13): a 40 icon box, title 18 medium,
/// description 14 muted (max 384), then the actions, centered with gap 24.
pub fn page_empty(
    sym: Sym,
    title: impl Into<SharedString>,
    description: impl Into<SharedString>,
    actions: Vec<AnyElement>,
    cx: &App,
) -> gpui::Div {
    let k = native_colors(cx);
    div()
        .flex_1()
        .w_full()
        .p(px(48.))
        .flex()
        .flex_col()
        .items_center()
        .justify_center()
        .gap(m::GUTTER)
        .child(
            div()
                .flex()
                .flex_col()
                .items_center()
                .gap(px(16.))
                .child(
                    div()
                        .size(m::EMPTY_BOX)
                        .rounded(m::FRAME_RADIUS)
                        .bg(k.fill)
                        .flex()
                        .items_center()
                        .justify_center()
                        .child(icons::sym(sym, EMPTY_ICON, k.label)),
                )
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .items_center()
                        .gap(px(8.))
                        .child(
                            div()
                                .n_title2()
                                .font_weight(gpui::FontWeight::MEDIUM)
                                .text_color(k.label)
                                .child(title.into()),
                        )
                        .child(
                            div()
                                .max_w(m::EMPTY_TEXT)
                                .text_center()
                                .n_body()
                                .text_color(k.secondary)
                                .child(description.into()),
                        ),
                ),
        )
        .when(!actions.is_empty(), |d| {
            d.child(div().flex().items_center().gap(px(8.)).children(actions))
        })
}

/// A compact Empty (§2.13): a 16 icon, one 14 muted line and an optional
/// action, left-aligned in its section.
pub fn compact_empty(
    sym: Sym,
    text: impl Into<SharedString>,
    action: Option<AnyElement>,
    cx: &App,
) -> gpui::Div {
    let k = native_colors(cx);
    div()
        .flex_none()
        .min_h(px(32.))
        .flex()
        .items_center()
        .gap(px(8.))
        .child(row_icon(sym, k.secondary))
        .child(
            div()
                .flex_1()
                .min_w(px(0.))
                .n_body()
                .text_color(k.secondary)
                .child(text.into()),
        )
        .children(action)
}

/// The shapes of a loading region (§2.14, T1-07): each kind is the final
/// shape of the list it stands in for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SkeletonKind {
    /// A framed list row (§2.6): h56, 32 box, then lines h14 and h12.
    Row,
    /// A pane or inspector row: 16 square and one line, pitch 32.
    Compact,
    /// A table (§2.7): an h40 header, then h48 rows, in a frame.
    TableRow,
    /// A member list row: unframed, a group label, then 32 avatar circle,
    /// name over role, pitch 56 (the member list row).
    MemberRow,
    /// A sidebar item (§2.14): 16 square and a h14 bar, pitch 32, px 8, on
    /// the sidebar fill.
    SidebarItem,
}

/// The pitch of a member list row.
pub const MEMBER_ROW: Pixels = px(56.);

/// `n` skeleton rows of `kind` at the real row height and frame.
pub fn skeleton_rows(id: &str, kind: SkeletonKind, n: usize, cx: &App) -> AnyElement {
    let k = native_colors(cx);
    let widths = [0.62, 0.45, 0.78, 0.54, 0.7, 0.4];
    let rows = (0..n).map(|i| {
        let w = widths[i % widths.len()];
        let row_id = SharedString::from(format!("{id}-{i}"));
        match kind {
            SkeletonKind::Row => div()
                .id(row_id)
                .relative()
                .h(m::ROW)
                .px(m::ROW_PAD)
                .flex()
                .items_center()
                .gap(px(12.))
                .child(Bone::new().size(m::MEDIA).rounded(px(6.)))
                .child(
                    div()
                        .flex_1()
                        .flex()
                        .flex_col()
                        .gap(px(6.))
                        .child(Bone::new().h(px(14.)).w(gpui::relative(w)))
                        .child(Bone::new().h(px(12.)).w(gpui::relative(w * 0.6))),
                )
                .child(Bone::new().h(px(12.)).w(px(40.)))
                .when(i + 1 < n, |d| {
                    d.child(
                        div()
                            .absolute()
                            .bottom_0()
                            .left(m::ROW_PAD + m::MEDIA + px(12.))
                            .right(px(0.))
                            .h(px(1.))
                            .bg(k.separator),
                    )
                })
                .into_any_element(),
            SkeletonKind::Compact => div()
                .id(row_id)
                .h(px(32.))
                .flex()
                .items_center()
                .gap(px(8.))
                .child(Bone::new().size(px(16.)).rounded(px(4.)))
                .child(Bone::new().h(px(14.)).w(gpui::relative(w)))
                .into_any_element(),
            SkeletonKind::TableRow => div()
                .id(row_id)
                .h(super::tokens::height::TABLE_ROW)
                .px(px(16.))
                .flex()
                .items_center()
                .gap(px(24.))
                .when(i + 1 < n, |d| d.border_b_1().border_color(k.separator))
                .child(
                    div()
                        .flex_1()
                        .child(Bone::new().h(px(14.)).w(gpui::relative(w))),
                )
                .child(Bone::new().h(px(14.)).w(px(64.)))
                .child(Bone::new().h(px(14.)).w(px(96.)))
                .child(Bone::new().h(px(14.)).w(px(56.)))
                .into_any_element(),
            SkeletonKind::MemberRow => div()
                .id(row_id)
                .h(MEMBER_ROW)
                .flex()
                .items_center()
                .gap(px(12.))
                .child(Bone::new().size(m::MEDIA).rounded_full())
                .child(
                    div()
                        .flex_1()
                        .flex()
                        .flex_col()
                        .gap(px(6.))
                        .child(Bone::new().h(px(14.)).w(gpui::relative(w * 0.7)))
                        .child(Bone::new().h(px(12.)).w(gpui::relative(w * 0.4))),
                )
                .into_any_element(),
            SkeletonKind::SidebarItem => div()
                .id(row_id)
                .h(px(32.))
                .px(px(8.))
                .flex()
                .items_center()
                .gap(px(8.))
                .child(Bone::new().size(px(16.)).rounded(px(4.)).on_fill(true))
                .child(
                    Bone::new()
                        .h(px(14.))
                        .w(gpui::relative(0.5 + 0.4 * w))
                        .on_fill(true),
                )
                .into_any_element(),
        }
    });
    match kind {
        SkeletonKind::Row => frame(cx).children(rows).into_any_element(),
        SkeletonKind::TableRow => frame(cx)
            .child(
                div()
                    .h(super::tokens::height::TABLE_HEAD)
                    .px(px(16.))
                    .bg(k.fill)
                    .border_b_1()
                    .border_color(k.separator)
                    .flex()
                    .items_center()
                    .gap(px(24.))
                    .child(
                        div()
                            .flex_1()
                            .child(Bone::new().h(px(12.)).w(px(48.)).on_fill(true)),
                    )
                    .child(Bone::new().h(px(12.)).w(px(64.)).on_fill(true))
                    .child(Bone::new().h(px(12.)).w(px(96.)).on_fill(true))
                    .child(Bone::new().h(px(12.)).w(px(56.)).on_fill(true)),
            )
            .children(rows)
            .into_any_element(),
        SkeletonKind::MemberRow => div()
            .flex()
            .flex_col()
            .child(
                div()
                    .h(px(32.))
                    .flex()
                    .items_center()
                    .child(Bone::new().h(px(12.)).w(px(48.))),
            )
            .children(rows)
            .into_any_element(),
        SkeletonKind::Compact | SkeletonKind::SidebarItem => {
            div().flex().flex_col().children(rows).into_any_element()
        }
    }
}

/// `n` skeleton KPI cards in one row, at the card height.
pub fn skeleton_cards(n: usize, cx: &App) -> AnyElement {
    div()
        .flex()
        .gap(m::GRID_GAP)
        .children((0..n).map(|_| {
            card(cx)
                .flex_1()
                .p(m::GUTTER)
                .gap(px(8.))
                .child(
                    Bone::new()
                        .h(px(14.))
                        .w(gpui::relative(0.5))
                        .rounded(px(8.)),
                )
                .child(Bone::new().h(px(28.)).w(px(48.)).rounded(px(8.)))
                .child(
                    Bone::new()
                        .h(px(12.))
                        .w(gpui::relative(0.7))
                        .rounded(px(8.)),
                )
        }))
        .into_any_element()
}

/// What a page shows: its data, its loading shapes, or a failure.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PageLoad {
    Ready,
    /// The first sync has not finished and the cache holds nothing for the
    /// page yet.
    Loading,
    /// The cache holds nothing for the page and the server cannot be reached.
    Failed,
}

impl PageLoad {
    /// A page with data always shows it (the cache is the source of truth);
    /// an empty page waits for the first sync or reports why it cannot.
    pub fn of(services: &Services, has_data: bool) -> Self {
        if has_data {
            return Self::Ready;
        }
        match services.provider.connection() {
            ConnectionState::Connecting => Self::Loading,
            ConnectionState::Reconnecting { .. } | ConnectionState::TokenUnreadable => Self::Failed,
            ConnectionState::Online | ConnectionState::Offline => Self::Ready,
        }
    }
}

/// The failure of a page with no cached data (§2.15): an Alert with
/// `重试`, which reconnects now.
pub fn load_failed(id: &str, services: &Services, cx: &App) -> AnyElement {
    use crate::copy::common as cc;
    let services = services.clone();
    page_alert(
        SharedString::from(format!("{id}-failed")),
        AlertKind::Error,
        cc::PAGE_LOAD_FAILED,
        Some(cc::PAGE_LOAD_FAILED_LINE.into()),
        Some(
            super::push_button(
                SharedString::from(format!("{id}-retry")),
                cc::PAGE_RETRY,
                super::PushStyle::Outline,
                super::PushSize::Small,
                cx,
            )
            .on_click(move |_, _, _| services.provider.reconnect_now())
            .into_any_element(),
        ),
        cx,
    )
    .into_any_element()
}

/// One Progress (D12): h4, radius full, the value in `label` (primary) on a
/// track of the same color at 20 %. gpui-component's `Progress`.
pub fn progress(id: impl Into<ElementId>, value: f32, cx: &App) -> AnyElement {
    use gpui_component::{Sizable as _, progress::Progress};
    let k = native_colors(cx);
    Progress::new(id)
        .value((value.clamp(0., 1.) * 100.).round())
        .color(k.label)
        .with_size(gpui_component::Size::XSmall)
        .into_any_element()
}

/// A card with its header (§2.4): p24, gap 24; title 16 semibold, an
/// optional description 14 muted and an action at the top right; then the
/// body. `footer` sits under the body after a 1 px line.
pub fn titled_card(
    id: impl Into<ElementId>,
    title: impl Into<SharedString>,
    description: Option<SharedString>,
    action: Option<AnyElement>,
    body: impl IntoElement,
    footer: Option<AnyElement>,
    cx: &App,
) -> Stateful<gpui::Div> {
    let k = native_colors(cx);
    card(cx)
        .id(id.into())
        .min_w(px(0.))
        .p(m::GUTTER)
        .gap(m::GUTTER)
        .child(
            div()
                .flex()
                .items_start()
                .gap(px(8.))
                .child(
                    div()
                        .flex_1()
                        .min_w(px(0.))
                        .flex()
                        .flex_col()
                        .gap(px(4.))
                        .child(div().n_title3().text_color(k.label).child(title.into()))
                        .children(
                            description.map(|d| div().n_body().text_color(k.secondary).child(d)),
                        ),
                )
                .children(action.map(|a| div().flex_none().child(a))),
        )
        .child(div().flex().flex_col().child(body))
        .children(footer.map(|f| {
            div()
                .mx(-m::GUTTER)
                .px(m::GUTTER)
                .pt(m::GUTTER)
                .border_t_1()
                .border_color(k.separator)
                .flex()
                .items_center()
                .gap(px(8.))
                .child(f)
        }))
}

/// A row inside a card (§2.6 compact): an optional leading element, title
/// 14 over an optional 12 muted meta, and a right element; a 1 px line
/// under it unless it is the last.
pub fn card_row(
    id: impl Into<ElementId>,
    leading: Option<AnyElement>,
    title: impl Into<SharedString>,
    meta: Option<SharedString>,
    trailing: Option<AnyElement>,
    last: bool,
    cx: &App,
) -> Stateful<gpui::Div> {
    let k = native_colors(cx);
    div()
        .id(id.into())
        .flex_none()
        .min_h(px(48.))
        .py(px(8.))
        .flex()
        .items_center()
        .gap(px(12.))
        .when(!last, |d| d.border_b_1().border_color(k.separator))
        .children(leading.map(|l| div().flex_none().child(l)))
        .child(
            div()
                .flex_1()
                .min_w(px(0.))
                .flex()
                .flex_col()
                .gap(px(2.))
                .child(TruncText::new(title).n_body().text_color(k.label))
                .children(meta.map(|m| TruncText::new(m).n_callout().text_color(k.secondary))),
        )
        .children(trailing.map(|t| div().flex_none().flex().items_center().gap(px(8.)).child(t)))
}

/// A label/value row of a DescriptionList (§2.5): label 14 muted in a
/// fixed column, value 14 from the next column, one line.
pub fn desc_row(label: impl Into<SharedString>, value: impl IntoElement, cx: &App) -> gpui::Div {
    let k = native_colors(cx);
    div()
        .flex_none()
        .min_h(px(36.))
        .flex()
        .items_center()
        .gap(px(8.))
        .child(
            TruncText::new(label)
                .flex_none()
                .w(px(112.))
                .n_body()
                .text_color(k.secondary),
        )
        .child(
            div()
                .flex_1()
                .min_w(px(0.))
                .overflow_hidden()
                .whitespace_nowrap()
                .text_ellipsis()
                .n_body()
                .n_figures()
                .text_color(k.label)
                .child(value),
        )
}

/// A stat of a KPI strip inside a card: label 12 muted over a 24
/// semibold tabular number and an optional 12 muted note.
pub fn stat(
    label: impl Into<SharedString>,
    value: impl Into<SharedString>,
    note: Option<SharedString>,
    cx: &App,
) -> gpui::Div {
    let k = native_colors(cx);
    div()
        .flex_1()
        .min_w(px(0.))
        .flex()
        .flex_col()
        .gap(px(4.))
        .child(
            div()
                .n_callout()
                .text_color(k.secondary)
                .child(label.into()),
        )
        .child(
            div()
                .n_style(KPI_NUMBER)
                .font_weight(gpui::FontWeight::SEMIBOLD)
                .n_figures()
                .text_color(k.label)
                .whitespace_nowrap()
                .child(value.into()),
        )
        // One line (T1-11, §2.21): the note cuts with "…" and shows the
        // whole note in a tooltip, never a wrap mid-phrase.
        .children(note.map(|n| TruncText::new(n).n_callout().text_color(k.secondary)))
}

/// A column of a [`table`]: its header, its width (`None` = the flexible
/// text column) and whether it right-aligns (numbers, times).
pub struct Col {
    pub header: SharedString,
    pub width: Option<Pixels>,
    pub right: bool,
}

/// A compact table (§2.7): header row h40 with 12 medium muted labels on
/// the muted fill, rows h48 with 1 px lines between them, cells px12
/// (first and last px16), one line each, truncating. The frame is the
/// card's or the caller's.
pub fn table(
    id: &str,
    cols: &[Col],
    rows: Vec<(
        Vec<AnyElement>,
        Option<Box<dyn Fn(&gpui::ClickEvent, &mut gpui::Window, &mut App)>>,
    )>,
    cx: &App,
) -> gpui::Div {
    use gpui::StatefulInteractiveElement as _;
    let k = native_colors(cx);
    let n = cols.len();
    let cell = |i: usize, right: bool, width: Option<Pixels>| {
        div()
            .min_w(px(0.))
            .when_some(width, |d, w| d.flex_none().w(w))
            .when(width.is_none(), |d| d.flex_1())
            .pl(if i == 0 { px(16.) } else { px(12.) })
            .pr(if i + 1 == n { px(16.) } else { px(0.) })
            .flex()
            .items_center()
            .when(right, |d| d.justify_end())
            .overflow_hidden()
            .whitespace_nowrap()
    };
    let header = div()
        .flex_none()
        .h(px(40.))
        .flex()
        .items_center()
        .bg(k.fill)
        .border_b_1()
        .border_color(k.separator)
        .children(cols.iter().enumerate().map(|(i, c)| {
            cell(i, c.right, c.width)
                .n_callout()
                .font_weight(gpui::FontWeight::MEDIUM)
                .text_color(k.secondary)
                .child(c.header.clone())
        }));
    let total = rows.len();
    let body = rows.into_iter().enumerate().map(|(ri, (cells, click))| {
        div()
            .id(SharedString::from(format!("{id}-row-{ri}")))
            .flex_none()
            .h(px(48.))
            .flex()
            .items_center()
            .when(ri + 1 < total, |d| d.border_b_1().border_color(k.separator))
            .map(|d| {
                if click.is_some() {
                    d.focus_row(false, cx)
                } else {
                    d.row_hover(false, cx)
                }
            })
            .n_body()
            .text_color(k.label)
            .children(cells.into_iter().enumerate().map(|(i, el)| {
                let c = &cols[i.min(n - 1)];
                cell(i, c.right, c.width).child(el)
            }))
            .when_some(click, |d, f| d.on_click(move |e, w, cx| f(e, w, cx)))
    });
    div().flex().flex_col().child(header).children(body)
}

/// One line of text that truncates with `…` (a table cell, a meta line).
pub fn cell_text(text: impl Into<SharedString>, muted: bool, cx: &App) -> AnyElement {
    let k = native_colors(cx);
    TruncText::new(text)
        .text_color(if muted { k.secondary } else { k.label })
        .into_any_element()
}
