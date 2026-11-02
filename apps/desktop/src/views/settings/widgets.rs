//! Small pieces the settings pages share: section headers, rows, the error
//! text of a request, the one-time secret block, and helpers for replies.

use gpui::{
    App, Div, ElementId, IntoElement, ParentElement, SharedString, Styled, Window, div,
    prelude::FluentBuilder as _, px,
};
use gpui_component::{
    Disableable as _, Selectable as _, Sizable as _,
    button::{Button, ButtonGroup},
    clipboard::Clipboard,
};

use crate::{
    copy::{fill, settings as s},
    data::{AdminError, AdminReply, MemberLevel, Millis},
    theme::{self, Type as _, space},
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

/// A page title.
pub fn page_title(text: &'static str, cx: &App) -> Div {
    let colors = theme::colors(cx);
    div()
        .pb(space::S12)
        .t_title()
        .text_color(colors.text_1)
        .child(text)
}

/// A group of rows with a small heading.
pub fn section(title: impl Into<SharedString>, cx: &App) -> Div {
    let colors = theme::colors(cx);
    div().flex().flex_col().gap(space::S8).pt(space::S20).child(
        div()
            .t_section()
            .text_color(colors.text_3)
            .child(title.into()),
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
