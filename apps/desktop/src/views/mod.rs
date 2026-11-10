//! The views. Each view is a GPUI entity built from `app::Services`.
//! Shared small pieces (avatar, badge, status chip, time text) live here.

pub mod artifacts;
pub mod banners;
pub mod cards;
pub mod columns;
pub mod composer;
pub mod connection_banner;
pub mod files;
pub mod login;
// --- v11/media ---
pub mod media;
// --- end v11/media ---
pub mod nav;
pub mod process;
pub mod search;
pub mod settings;
pub mod side_panel;
pub mod sidebar;
pub mod timeline;
pub mod topic_list;
pub mod topic_menu;
pub mod welcome;

// The v2 shell (1.1.0) and one folder per feature track.
pub mod artifact_page;
pub mod desk;
pub mod group_page;
pub mod live;
pub mod members_page;
pub mod memory;
pub mod admin;
pub mod rail;
// --- inc-base: 引用提问 entry point (inc-refs implements it).
pub mod refs;
pub mod shell;
pub mod side_chat;
pub mod topic_page;
pub mod topic_rail;
// --- rail track: ⌘K 交代或搜索.
pub mod ask;

use chrono::{Datelike, Local, TimeZone};
use gpui::{App, Div, IntoElement, ParentElement, Pixels, Styled, div, px};

use crate::{
    copy::{common, fill},
    data::{Member, Millis, Topic, TopicStatus},
    theme, ui,
};

/// The member's avatar (`ui::MemberAvatar`: people are circles, bots are
/// rounded squares with the bot mark).
pub fn avatar(member: Option<&Member>, size: Pixels, _cx: &App) -> Div {
    div().flex_none().child(ui::MemberAvatar::new(member, size))
}

/// Presence dot (online green, else `text-3`).
pub fn presence_dot(online: bool, cx: &App) -> Div {
    let colors = theme::colors(cx);
    div().flex_none().size(px(8.)).rounded_full().bg(if online {
        colors.online
    } else {
        colors.offline
    })
}

/// Unread or mention count badge (`ui::CountBadge`). `None` for zero.
pub fn count_badge(count: u32, mention: bool, _cx: &App) -> Option<Div> {
    (count > 0).then(|| {
        let kind = if mention {
            ui::BadgeKind::Mention
        } else {
            ui::BadgeKind::Unread
        };
        div().flex_none().child(ui::CountBadge::new(count, kind))
    })
}

/// A name set between Chinese words (T-09): a space on each side that
/// ends in a Latin letter or digit (`等 Alex Chen 回答`), none beside a
/// Chinese one (`等李雪回答`).
pub fn spaced_name(name: &str) -> String {
    let latin = |c: Option<char>| c.is_some_and(|c| c.is_ascii_alphanumeric());
    let lead = if latin(name.chars().next()) { " " } else { "" };
    let tail = if latin(name.chars().last()) { " " } else { "" };
    format!("{lead}{name}{tail}")
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
            Some(name) => fill(
                common::STATUS_WAITING_ANSWER_NAMED,
                &[("name", &spaced_name(&name))],
            ),
            None => common::STATUS_WAITING_ANSWER.to_string(),
        },
        TopicStatus::WaitingReview if waits_on_me => common::STATUS_WAITING_REVIEW_ME.to_string(),
        TopicStatus::WaitingReview => match first_name {
            Some(name) => fill(
                common::STATUS_WAITING_REVIEW_NAMED,
                &[("name", &spaced_name(&name))],
            ),
            None => common::STATUS_WAITING_REVIEW.to_string(),
        },
    };
    Some(text)
}

pub fn status_chip(
    topic: &Topic,
    me: Option<&str>,
    members: &[Member],
    _cx: &App,
) -> Option<impl IntoElement> {
    ui::StatusChip::for_topic(topic, me, members)
}

/// Short local time for lists and the timeline: 刚刚, N 分钟前, HH:MM, 昨天, M月D日.
pub fn time_text(at: Millis) -> String {
    let now = crate::selftest::clock::local_now();
    let Some(then) = shell::known(at).and_then(|at| Local.timestamp_millis_opt(at).single()) else {
        return common::TIME_UNKNOWN.to_string();
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
    } else if then.year() != now.year() {
        shell::day(at)
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
            "等 Joye 回答"
        );
        assert_eq!(spaced_name("李雪"), "李雪");
        assert_eq!(spaced_name("Alex Chen"), " Alex Chen ");
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
