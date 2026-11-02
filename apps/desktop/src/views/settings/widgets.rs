//! Small pieces the settings pages share: section headers, rows, the error
//! text of a request, the one-time secret block, and helpers for replies.

use gpui::{
    AnyElement, App, Div, ElementId, InteractiveElement as _, IntoElement, MouseButton,
    ParentElement, Pixels, SharedString, Styled, Window, div, prelude::FluentBuilder as _, px,
};
use gpui_component::{
    Disableable as _, Selectable as _, Sizable as _,
    button::{Button, ButtonGroup},
    clipboard::Clipboard,
};

use crate::{
    copy::{fill, settings as s},
    data::{AdminError, AdminReply, MemberLevel, Millis},
    theme::{self, Elevation, Type as _, space},
};

/// Waits for a reply off the UI thread. A dropped sender is [`AdminError::Other`].
pub async fn answer<T>(reply: AdminReply<T>) -> Result<T, AdminError> {
    reply.await.unwrap_or(Err(AdminError::Other))
}

/// The copy text of a failed request.
pub fn error_text(e: AdminError) -> &'static str {
    match e {
        AdminError::Unsupported => s::ERROR_UNSUPPORTED,
        AdminError::Offline => s::ERROR_OFFLINE,
        AdminError::Forbidden => s::ERROR_FORBIDDEN,
        AdminError::NotFound => s::ERROR_NOT_FOUND,
        AdminError::LastOwner => s::ERROR_LAST_OWNER,
        AdminError::Invalid => s::ERROR_INVALID,
        AdminError::TooLarge => s::ERROR_TOO_LARGE,
        AdminError::RateLimited => s::ERROR_RATE_LIMITED,
        AdminError::Other => s::ERROR_OTHER,
    }
}

pub fn level_text(level: MemberLevel) -> &'static str {
    match level {
        MemberLevel::Owner => s::LEVEL_OWNER,
        MemberLevel::Member => s::LEVEL_MEMBER,
    }
}

/// Splits labels typed as `a, b，c d` into trimmed, unique labels.
pub fn split_labels(text: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for label in text.split([',', '，', ' ', '、']) {
        let label = label.trim();
        if !label.is_empty() && !out.iter().any(|l| l == label) {
            out.push(label.to_string());
        }
    }
    out
}

/// `12 GB`, or `512 MB` under 1 GB.
pub fn size_text(bytes: u64) -> String {
    const GB: u64 = 1024 * 1024 * 1024;
    const MB: u64 = 1024 * 1024;
    if bytes >= GB {
        let gb = bytes as f64 / GB as f64;
        let n = if gb >= 10.0 {
            format!("{gb:.0}")
        } else {
            format!("{gb:.1}")
        };
        fill(s::SIZE_GB, &[("n", &n)])
    } else {
        fill(s::SIZE_MB, &[("n", &(bytes / MB).to_string())])
    }
}

/// `2026年11月10日 14:05` in local time.
pub fn date_text(at: Millis) -> String {
    use chrono::TimeZone as _;
    match chrono::Local.timestamp_millis_opt(at) {
        chrono::LocalResult::Single(t) => t.format("%Y年%-m月%-d日 %H:%M").to_string(),
        _ => String::new(),
    }
}

/// The text of `n` characters at most, or the error.
pub fn check_len(text: &str, max: usize) -> Result<(), String> {
    if text.chars().count() > max {
        Err(fill(s::TOO_LONG, &[("n", &max.to_string())]))
    } else {
        Ok(())
    }
}

/// A page title (15 / 20, 600). Pages show it in the toolbar band through
/// [`page_header`]; this form is for a page without a band.
pub fn page_title(text: &'static str, cx: &App) -> Div {
    let colors = theme::colors(cx);
    div()
        .pb(space::S12)
        .t_title()
        .text_color(colors.text_1)
        .child(text)
}

/// The page header that sits in the 52 pt toolbar band of the settings
/// window: the title 15 / 600, an optional count 12 pt `text-2`, and the
/// page actions at the trailing edge (DESIGN.md §13.10).
pub fn page_header(
    title: &'static str,
    count: Option<String>,
    actions: Option<AnyElement>,
    cx: &App,
) -> Div {
    let colors = theme::colors(cx);
    div()
        .size_full()
        .px(space::S20)
        .flex()
        .items_center()
        .gap(space::S8)
        .child(
            div()
                .flex_none()
                .t_title()
                .text_color(colors.text_1)
                .child(title),
        )
        .children(count.map(|n| {
            div()
                .flex_1()
                .min_w_0()
                .t_callout()
                .tnum()
                .text_color(colors.text_2)
                .one_line()
                .child(n)
        }))
        .child(div().flex_1())
        .children(actions.map(|a| {
            div()
                .flex_none()
                .flex()
                .items_center()
                .gap(space::S8)
                // Buttons in the band stop the window drag.
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .child(a)
        }))
}

/// A section header above a grouped section: 11 / 14, 600, `text-3`,
/// aligned with the text inside the group.
pub fn section_header(title: impl Into<SharedString>, cx: &App) -> Div {
    let colors = theme::colors(cx);
    div()
        .px(space::S12)
        .pt(space::S20)
        .pb(space::S6)
        .t_section()
        .text_color(colors.text_3)
        .one_line()
        .child(title.into())
}

/// A grouped (inset) section: `group` surface, radius 10, the level-1 ring.
/// Put [`group_row`]s in it, with [`group_sep`] between them.
pub fn group(cx: &App) -> Div {
    let colors = theme::colors(cx);
    div()
        .w_full()
        .flex()
        .flex_col()
        .rounded(theme::radius::LG)
        .bg(colors.group)
        .shadow(colors.shadow(Elevation::Card))
}

/// One row of a grouped section: at least 52 pt, 12 pt side padding.
pub fn group_row() -> Div {
    div()
        .w_full()
        .min_h(px(52.))
        .px(space::S12)
        .py(space::S10)
        .flex()
        .items_center()
        .gap(space::S10)
}

/// The hairline between two rows, inset to the text by `inset`.
pub fn group_sep(inset: Pixels, cx: &App) -> Div {
    div()
        .w_full()
        .flex_none()
        .pl(inset)
        .child(div().h(theme::HAIRLINE).w_full().bg(theme::colors(cx).sep))
}

/// Rows joined by inset hairlines.
pub fn group_rows(rows: Vec<Div>, inset: Pixels, cx: &App) -> Div {
    let n = rows.len();
    let mut out = group(cx);
    for (ix, row) in rows.into_iter().enumerate() {
        out = out.child(row);
        if ix + 1 < n {
            out = out.child(group_sep(inset, cx));
        }
    }
    out
}

/// A labelled row inside a group: the 13 pt label on the left, the control
/// on the right.
pub fn group_field(label: &'static str, control: impl IntoElement, cx: &App) -> Div {
    let colors = theme::colors(cx);
    group_row()
        .child(
            div()
                .w(px(88.))
                .flex_none()
                .t_ui()
                .text_color(colors.text_1)
                .child(label),
        )
        .child(div().flex_1().min_w_0().child(control))
}

/// A footnote under a group: 12 pt `text-2`, aligned with the row text.
pub fn footnote(text: impl Into<SharedString>, cx: &App) -> Div {
    let colors = theme::colors(cx);
    div()
        .px(space::S12)
        .pt(space::S6)
        .t_callout()
        .text_color(colors.text_2)
        .child(text.into())
}

/// A group row that shows an empty list: one centered line in `text-2`.
pub fn group_empty(text: impl Into<SharedString>, cx: &App) -> Div {
    let colors = theme::colors(cx);
    group(cx).child(
        group_row()
            .justify_center()
            .t_callout()
            .text_color(colors.text_2)
            .text_center()
            .child(text.into()),
    )
}

/// A labelled row: the label on the left, the control on the right.
pub fn field(label: &'static str, control: impl IntoElement, cx: &App) -> Div {
    let colors = theme::colors(cx);
    div()
        .flex()
        .items_center()
        .gap(px(12.))
        .child(
            div()
                .w(px(88.))
                .flex_none()
                .t_ui()
                .text_color(colors.text_2)
                .child(label),
        )
        .child(div().flex_1().min_w_0().child(control))
}

/// A muted hint line.
pub fn hint(text: impl Into<SharedString>, cx: &App) -> Div {
    let colors = theme::colors(cx);
    div()
        .t_callout()
        .text_color(colors.text_2)
        .child(text.into())
}

/// A status line: an error in the danger color, or a quiet success note.
pub fn status_line(status: &Status, cx: &App) -> Option<Div> {
    let colors = theme::colors(cx);
    match status {
        Status::Idle => None,
        Status::Busy(text) | Status::Done(text) => Some(
            div()
                .t_callout()
                .text_color(colors.text_2)
                .child(text.clone()),
        ),
        Status::Failed(text) => Some(
            div()
                .t_callout()
                .text_color(colors.danger_text)
                .child(text.clone()),
        ),
    }
}

/// The state of one request on a page.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum Status {
    #[default]
    Idle,
    Busy(SharedString),
    Done(SharedString),
    Failed(SharedString),
}

impl Status {
    pub fn busy(&self) -> bool {
        matches!(self, Status::Busy(_))
    }

    pub fn from_error(e: AdminError) -> Self {
        Status::Failed(error_text(e).into())
    }
}

/// A one-time secret: its label, the value in a code box with a copy button,
/// and a hint. The value is never logged.
pub fn secret_block(id: impl Into<ElementId>, label: &'static str, value: &str, cx: &App) -> Div {
    let colors = theme::colors(cx);
    let value: SharedString = value.to_string().into();
    div()
        .flex()
        .flex_col()
        .gap(px(4.))
        .child(div().t_ui().medium().child(label))
        .child(
            div()
                .flex()
                .items_start()
                .gap(px(8.))
                .p(space::S10)
                .rounded(theme::radius::LG)
                .bg(colors.fill_field)
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .t_code()
                        .child(value.clone()),
                )
                .child(Clipboard::new(id).tooltip(s::COPY).value(value)),
        )
}

/// A row of mutually exclusive choices; `on_pick` gets the picked index.
pub fn choice(
    id: impl Into<ElementId>,
    options: &[&'static str],
    selected: usize,
    disabled: bool,
    on_pick: impl Fn(usize, &mut Window, &mut App) + 'static,
) -> ButtonGroup {
    let id: ElementId = id.into();
    ButtonGroup::new(id.clone())
        .outline()
        .small()
        .children(options.iter().enumerate().map(|(ix, label)| {
            Button::new(ElementId::NamedInteger(
                SharedString::from(format!("{id}-opt")),
                ix as u64,
            ))
            .label(*label)
            .selected(ix == selected)
            .disabled(disabled)
        }))
        .when(!disabled, |group| {
            group.on_click(move |picked: &Vec<usize>, window, cx| {
                if let Some(ix) = picked.first() {
                    on_pick(*ix, window, cx);
                }
            })
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn labels_split_on_any_separator() {
        assert_eq!(
            split_labels(" mac, office，gpu  mac 、x"),
            vec!["mac", "office", "gpu", "x"]
        );
        assert!(split_labels(" , ").is_empty());
    }

    #[test]
    fn sizes_read_well() {
        assert_eq!(size_text(212 * 1024 * 1024 * 1024), "212 GB");
        assert_eq!(size_text(3 * 1024 * 1024 * 1024 / 2), "1.5 GB");
        assert_eq!(size_text(512 * 1024 * 1024), "512 MB");
    }

    #[test]
    fn every_error_has_text() {
        for e in [
            AdminError::Unsupported,
            AdminError::Offline,
            AdminError::Forbidden,
            AdminError::NotFound,
            AdminError::LastOwner,
            AdminError::Invalid,
            AdminError::TooLarge,
            AdminError::RateLimited,
            AdminError::Other,
        ] {
            assert!(!error_text(e).is_empty());
        }
    }

    #[test]
    fn length_check() {
        assert!(check_len("一二三", 3).is_ok());
        assert_eq!(check_len("一二三四", 3), Err("不能超过 3 个字".to_string()));
    }
}
