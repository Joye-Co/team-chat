//! Status chips and count badges (DESIGN.md §2.5, §2.6, §12).

use gpui::{
    AbsoluteLength, App, EdgesRefinement, IntoElement, ParentElement, RenderOnce, Styled, Window,
    div, prelude::FluentBuilder as _, px,
};
use gpui_component::tag::Tag;

use crate::{
    copy::{common, nav},
    data::{Member, Topic, TopicStatus},
    theme::{self, ChipKind, Type as _},
    ui::icons::{Lucide, icon},
};

/// Height of chips and badges.
const CHIP_H: gpui::Pixels = px(18.);
/// A name inside 等{name}… truncates here; 等 and the verb stay visible.
const CHIP_NAME_MAX: gpui::Pixels = px(104.);
const CHIP_MAX: gpui::Pixels = px(168.);

/// The status chip of a topic. Renders nothing for `open` topics.
#[derive(IntoElement)]
pub struct StatusChip {
    status: TopicStatus,
    kind: ChipKind,
    /// Text before the name, the name (may be empty), text after it.
    parts: (String, String, String),
}

impl StatusChip {
    /// `None` for `open` topics (they have no chip).
    pub fn for_topic(topic: &Topic, me: Option<&str>, members: &[Member]) -> Option<Self> {
        let waits_on_me = me.is_some_and(|me| topic.waiting_on.iter().any(|m| m == me));
        let name = topic
            .waiting_on
            .first()
            .and_then(|id| members.iter().find(|m| &m.id == id))
            .map(|m| m.name.clone());
        let plain = |s: &str| (s.to_string(), String::new(), String::new());
        let named = |template: &str, name: &str| {
            let (before, after) = template.split_once("{name}").unwrap_or((template, ""));
            (before.to_string(), name.to_string(), after.to_string())
        };
        let (kind, parts) = match topic.status {
            TopicStatus::Open => return None,
            TopicStatus::InProgress => (ChipKind::Quiet, plain(common::STATUS_IN_PROGRESS)),
            TopicStatus::Done => (ChipKind::Quiet, plain(common::STATUS_DONE)),
            TopicStatus::Blocked => (ChipKind::Blocked, plain(common::STATUS_BLOCKED)),
            TopicStatus::WaitingAnswer if waits_on_me => {
                (ChipKind::You, plain(common::STATUS_WAITING_ANSWER_ME))
            }
            TopicStatus::WaitingReview if waits_on_me => {
                (ChipKind::You, plain(common::STATUS_WAITING_REVIEW_ME))
            }
            TopicStatus::WaitingAnswer => match name {
                Some(n) => (
                    ChipKind::Quiet,
                    named(common::STATUS_WAITING_ANSWER_NAMED, &n),
                ),
                None => (ChipKind::Quiet, plain(common::STATUS_WAITING_ANSWER)),
            },
            TopicStatus::WaitingReview => match name {
                Some(n) => (
                    ChipKind::Quiet,
                    named(common::STATUS_WAITING_REVIEW_NAMED, &n),
                ),
                None => (ChipKind::Quiet, plain(common::STATUS_WAITING_REVIEW)),
            },
        };
        Some(Self {
            status: topic.status,
            kind,
            parts,
        })
    }

    /// A quiet chip with a free label and no glyph (for example 等待加入).
    pub fn quiet(label: impl Into<String>) -> Self {
        Self {
            status: TopicStatus::Open,
            kind: ChipKind::Quiet,
            parts: (label.into(), String::new(), String::new()),
        }
    }
}

fn glyph(status: TopicStatus) -> Option<Lucide> {
    Some(match status {
        TopicStatus::Open => return None,
        TopicStatus::InProgress => Lucide::CircleDot,
        TopicStatus::WaitingAnswer => Lucide::CircleQuestionMark,
        TopicStatus::WaitingReview => Lucide::Eye,
        TopicStatus::Blocked => Lucide::CircleAlert,
        TopicStatus::Done => Lucide::CircleCheck,
    })
}

impl RenderOnce for StatusChip {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let colors = theme::chip_colors(self.kind, self.status, cx);
        let glyph = glyph(self.status);
        let (before, name, after) = self.parts;
        let mut tag = Tag::custom(colors.bg, colors.text, colors.ring)
            .rounded(theme::radius::CHIP)
            .flex_none()
            .h(CHIP_H)
            .max_w(CHIP_MAX)
            .py_0()
            .pl(if glyph.is_some() {
                theme::space::S4
            } else {
                theme::space::S6
            })
            .pr(theme::space::S6)
            .gap(px(3.))
            .t_caption()
            .when(self.kind == ChipKind::You, |el| el.semibold());
        // The quiet chip's ring is a hairline; the filled kinds have none.
        let width: AbsoluteLength = if self.kind == ChipKind::Quiet {
            theme::HAIRLINE.into()
        } else {
            px(0.).into()
        };
        tag.style().border_widths = EdgesRefinement {
            top: Some(width),
            right: Some(width),
            bottom: Some(width),
            left: Some(width),
        };
        tag.when_some(glyph, |el, g| {
            el.child(icon(g, theme::icon::CHIP, colors.glyph))
        })
        .child(
            div()
                .min_w(px(0.))
                .flex()
                .items_center()
                .child(div().flex_none().whitespace_nowrap().child(before))
                .when(!name.is_empty(), |el| {
                    el.child(
                        div()
                            .min_w(px(0.))
                            .max_w(CHIP_NAME_MAX)
                            .one_line()
                            .child(name),
                    )
                })
                .when(!after.is_empty(), |el| {
                    el.child(div().flex_none().whitespace_nowrap().child(after))
                }),
        )
    }
}

/// What a count badge counts.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BadgeKind {
    /// Gray unread count, capped at 999+.
    Unread,
    /// Accent `@n`, capped at @99+.
    Mention,
    /// Accent count without `@` (the 待办 count), capped at 99+.
    Todo,
}

/// An 18 pt count capsule. Renders nothing for zero.
#[derive(IntoElement)]
pub struct CountBadge {
    count: u32,
    kind: BadgeKind,
}

impl CountBadge {
    pub fn new(count: u32, kind: BadgeKind) -> Self {
        Self { count, kind }
    }

    pub fn text(count: u32, kind: BadgeKind) -> String {
        let cap = |max: u32| {
            if count > max {
                format!("{max}+")
            } else {
                count.to_string()
            }
        };
        match kind {
            BadgeKind::Unread => cap(999),
            BadgeKind::Todo => cap(99),
            BadgeKind::Mention => format!("{}{}", nav::MENTION_MARK, cap(99)),
        }
    }
}

impl RenderOnce for CountBadge {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        if self.count == 0 {
            return div().into_any_element();
        }
        let k = theme::colors(cx);
        let (bg, fg) = match self.kind {
            BadgeKind::Unread => (k.badge_bg, k.badge_text),
            BadgeKind::Mention | BadgeKind::Todo => (k.badge_mention_bg, k.on_accent),
        };
        div()
            .flex_none()
            .h(CHIP_H)
            .min_w(CHIP_H)
            .px(theme::space::S6)
            .rounded_full()
            .bg(bg)
            .text_color(fg)
            .flex()
            .items_center()
            .justify_center()
            .t_caption()
            .tnum()
            .when(self.kind != BadgeKind::Unread, |el| el.semibold())
            .child(Self::text(self.count, self.kind))
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counts_are_capped() {
        assert_eq!(CountBadge::text(1000, BadgeKind::Unread), "999+");
        assert_eq!(CountBadge::text(999, BadgeKind::Unread), "999");
        assert_eq!(CountBadge::text(100, BadgeKind::Mention), "@99+");
        assert_eq!(CountBadge::text(3, BadgeKind::Mention), "@3");
        assert_eq!(CountBadge::text(12345, BadgeKind::Todo), "99+");
    }
}
