//! The views. Each view is a GPUI entity built from `app::Services`.
//! Shared small pieces (avatar, badge, status chip, time text) live here.

pub mod artifacts;
pub mod cards;
pub mod composer;
pub mod connection_banner;
pub mod files;
pub mod login;
pub mod nav;
pub mod process;
pub mod search;
pub mod settings;
pub mod side_panel;
pub mod sidebar;
pub mod timeline;
pub mod topic_list;
pub mod topic_menu;

use chrono::{Datelike, Local, TimeZone};
use gpui::{App, Div, IntoElement, ParentElement, Pixels, Styled, div, px};

use crate::{
    copy::{self, common, fill},
    data::{Member, Millis, Topic, TopicStatus},
    theme,
};

/// Round avatar with the first letter of the name; bots get a small mark.
pub fn avatar(member: Option<&Member>, size: Pixels, cx: &App) -> Div {
    let colors = theme::colors(cx);
    let initial: String = member
        .and_then(|m| m.name.chars().next())
        .map(|c| c.to_string())
        .unwrap_or_default();
    let is_bot = member.is_some_and(|m| m.is_bot());
    let mut el = div()
        .relative()
        .flex_none()
        .size(size)
        .rounded_full()
        .bg(colors.accent)
        .text_color(colors.accent_foreground)
        .flex()
        .items_center()
        .justify_center()
        .text_size(size * 0.45)
        .child(initial);
    if is_bot {
        el = el.child(
            div()
                .absolute()
                .bottom(px(-2.))
                .right(px(-4.))
                .px(px(2.))
                .rounded(px(3.))
                .bg(colors.foreground)
                .text_color(colors.background)
                .text_size(px(8.))
                .child(common::BOT_MARK),
        );
    }
    el
}

/// Presence dot.
pub fn presence_dot(online: bool, cx: &App) -> Div {
    let colors = theme::colors(cx);
    div().flex_none().size(px(8.)).rounded_full().bg(if online {
        colors.online
    } else {
        colors.offline
    })
}

/// Unread or mention count badge. `None` for zero.
pub fn count_badge(count: u32, mention: bool, cx: &App) -> Option<Div> {
    if count == 0 {
        return None;
    }
    let colors = theme::colors(cx);
    let text = if count > 99 {
        copy::nav::UNREAD_MORE.to_string()
    } else {
        count.to_string()
    };
    let (bg, fg) = if mention {
        (colors.mention, colors.mention_foreground)
    } else {
        (colors.badge, colors.badge_foreground)
    };
    Some(
        div()
            .flex_none()
            .px(px(6.))
            .rounded_full()
            .bg(bg)
            .text_color(fg)
            .text_xs()
            .child(if mention { format!("@{text}") } else { text }),
    )
}

/// The status chip text (docs/PRODUCT.md), or `None` for `open`.
pub fn status_chip_text(topic: &Topic, me: Option<&str>, members: &[Member]) -> Option<String> {
    let waits_on_me = me.is_some_and(|me| topic.waiting_on.iter().any(|m| m == me));
    let first_name = topic
        .waiting_on
        .first()
        .and_then(|id| members.iter().find(|m| &m.id == id))
        .map(|m| m.name.clone());
    let text = match topic.status {
        TopicStatus::Open => return None,
        TopicStatus::InProgress => common::STATUS_IN_PROGRESS.to_string(),
        TopicStatus::Blocked => common::STATUS_BLOCKED.to_string(),
        TopicStatus::Done => common::STATUS_DONE.to_string(),
        TopicStatus::WaitingAnswer if waits_on_me => common::STATUS_WAITING_ANSWER_ME.to_string(),
        TopicStatus::WaitingAnswer => match first_name {
            Some(name) => fill(common::STATUS_WAITING_ANSWER_NAMED, &[("name", &name)]),
            None => common::STATUS_WAITING_ANSWER.to_string(),
        },
        TopicStatus::WaitingReview if waits_on_me => common::STATUS_WAITING_REVIEW_ME.to_string(),
        TopicStatus::WaitingReview => match first_name {
            Some(name) => fill(common::STATUS_WAITING_REVIEW_NAMED, &[("name", &name)]),
            None => common::STATUS_WAITING_REVIEW.to_string(),
        },
    };
    Some(text)
}

pub fn status_chip(
    topic: &Topic,
    me: Option<&str>,
    members: &[Member],
    cx: &App,
) -> Option<impl IntoElement> {
    let text = status_chip_text(topic, me, members)?;
    let (bg, fg) = theme::status_chip_colors(topic.status, cx);
    Some(
        div()
            .flex_none()
            .px(px(6.))
            .py(px(1.))
            .rounded(px(4.))
            .bg(bg)
            .text_color(fg)
            .text_xs()
            .child(text),
    )
}

/// Short local time for lists and the timeline: 刚刚, N 分钟前, HH:MM, 昨天, M月D日.
pub fn time_text(at: Millis) -> String {
    let now = Local::now();
    let Some(then) = Local.timestamp_millis_opt(at).single() else {
        return String::new();
    };
    let ago = now.signed_duration_since(then);
    if ago.num_minutes() < 1 {
        common::JUST_NOW.to_string()
    } else if ago.num_minutes() < 60 {
        fill(
            common::MINUTES_AGO,
            &[("n", &ago.num_minutes().to_string())],
        )
    } else if then.date_naive() == now.date_naive() {
        then.format("%H:%M").to_string()
    } else if then.date_naive().succ_opt() == Some(now.date_naive()) {
        common::YESTERDAY.to_string()
    } else {
        fill(
            common::MONTH_DAY,
            &[
                ("m", &then.month().to_string()),
                ("d", &then.day().to_string()),
            ],
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::ChatProvider;
    use crate::fake::FakeProvider;

    #[test]
    fn status_chips_follow_product_md() {
        let p = FakeProvider::new();
        let members = p.members();
        let mut topic = p.topic("g-dev", "t-api").unwrap();
        assert_eq!(
            status_chip_text(&topic, Some("m-joye"), &members).unwrap(),
            "等你回答"
        );
        assert_eq!(
            status_chip_text(&topic, Some("m-alex"), &members).unwrap(),
            "等Joye回答"
        );
        topic.waiting_on.clear();
        assert_eq!(
            status_chip_text(&topic, Some("m-alex"), &members).unwrap(),
            "等回答"
        );
        topic.status = TopicStatus::WaitingReview;
        assert_eq!(status_chip_text(&topic, None, &members).unwrap(), "等审");
        topic.status = TopicStatus::Open;
        assert!(status_chip_text(&topic, None, &members).is_none());
        topic.status = TopicStatus::Blocked;
        assert_eq!(status_chip_text(&topic, None, &members).unwrap(), "卡住了");
    }
}
