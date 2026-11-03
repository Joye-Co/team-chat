//! Status marks, badges, counts, the role tag and kbd (DESIGN.md §7.8, §7.9).
//!
//! | Mark | Shape | Use |
//! | --- | --- | --- |
//! | [`StatusChip`] chip | 22 pt, filled | `you` in the start block, `info` in the topic header |
//! | compact chip | 20 pt, filled, no glyph | 等你答 / 等你审 in topic rows and the switcher |
//! | quiet label | no fill, `you-glyph` glyph, `text-2` text | 等你回答 in headers |
//! | label | no fill | 卡住了 (`danger-text`), 完成 and 等… (`text-3`) |
//! | [`CountBadge`] | 20 pt pill | red for @ only; neutral `count-solid` for unread; grey for tabs |

use gpui::{
    App, IntoElement, ParentElement, Pixels, RenderOnce, SharedString, Styled, Window, div,
    prelude::FluentBuilder as _, px,
};

use crate::{
    copy::{common, nav},
    data::{Member, Topic, TopicStatus},
    theme::{self, ChipKind, Type as _, size, space},
    ui::icons::{Lucide, icon},
};

/// How a status mark is drawn.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChipStyle {
    /// 22 pt filled chip with a glyph.
    Chip,
    /// 20 pt filled chip, no glyph, 600 (topic rows).
    Compact,
    /// No fill, glyph and text (headers, the start block, labels in rows).
    Label,
}

/// The status mark of a topic. Renders nothing for `open` and `in_progress`
/// in rows (§9.1).
#[derive(IntoElement)]
pub struct StatusChip {
    status: TopicStatus,
    kind: ChipKind,
    style: ChipStyle,
    label: SharedString,
    /// The full label for the tooltip (a name never goes inside a label).
    full: Option<SharedString>,
}

impl StatusChip {
    /// The mark of a topic as rows show it: the compact chip when it waits
    /// for the viewer, otherwise a label. `None` for `open` and `in_progress`.
    pub fn for_topic(topic: &Topic, me: Option<&str>, members: &[Member]) -> Option<Self> {
        let waits_on_me = me.is_some_and(|me| topic.waiting_on.iter().any(|m| m == me));
        let name = topic
            .waiting_on
            .first()
            .and_then(|id| members.iter().find(|m| &m.id == id))
            .map(|m| m.name.clone());
        let named = |template: &str| {
            name.as_deref()
                .map(|n| SharedString::from(crate::copy::fill(template, &[("name", n)])))
        };
        let (kind, style, label, full) = match topic.status {
            TopicStatus::Open | TopicStatus::InProgress => return None,
            TopicStatus::Done => (ChipKind::Quiet, ChipStyle::Label, common::STATUS_DONE, None),
            TopicStatus::Blocked => (
                ChipKind::Blocked,
                ChipStyle::Label,
                common::STATUS_BLOCKED,
                None,
            ),
            TopicStatus::WaitingAnswer if waits_on_me => (
                ChipKind::You,
                ChipStyle::Compact,
                common::STATUS_WAITING_ANSWER_ME_SHORT,
                Some(common::STATUS_WAITING_ANSWER_ME.into()),
            ),
            TopicStatus::WaitingReview if waits_on_me => (
                ChipKind::You,
                ChipStyle::Compact,
                common::STATUS_WAITING_REVIEW_ME,
                None,
            ),
            TopicStatus::WaitingAnswer => (
                ChipKind::Quiet,
                ChipStyle::Label,
                common::STATUS_WAITING_ANSWER,
                named(common::STATUS_WAITING_ANSWER_NAMED),
            ),
            TopicStatus::WaitingReview => (
                ChipKind::Quiet,
                ChipStyle::Label,
                common::STATUS_WAITING_REVIEW,
                named(common::STATUS_WAITING_REVIEW_NAMED),
            ),
        };
        Some(Self {
            status: topic.status,
            kind,
            style,
            label: label.into(),
            full,
        })
    }

    /// The mark of a topic in its header (§8.1): a quiet label for 等你…, the
    /// info chip for 进行中, labels for the rest.
    pub fn header_for_topic(topic: &Topic, me: Option<&str>, members: &[Member]) -> Option<Self> {
        if topic.status == TopicStatus::InProgress {
            return Some(Self::labeled(
                common::STATUS_IN_PROGRESS,
                TopicStatus::InProgress,
                ChipKind::Info,
            ));
        }
        let mut chip = Self::for_topic(topic, me, members)?;
        if chip.kind == ChipKind::You {
            chip.style = ChipStyle::Label;
            chip.label = match topic.status {
                TopicStatus::WaitingAnswer => common::STATUS_WAITING_ANSWER_ME.into(),
                _ => common::STATUS_WAITING_REVIEW_ME.into(),
            };
            chip.full = None;
        }
        Some(chip)
    }

    /// A mark with a free label: a filled chip for `You` and `Info`, a label
    /// otherwise (cards use this for their status).
    pub fn labeled(label: impl Into<String>, status: TopicStatus, kind: ChipKind) -> Self {
        let style = match kind {
            ChipKind::You | ChipKind::Info => ChipStyle::Chip,
            _ => ChipStyle::Label,
        };
        Self {
            status,
            kind,
            style,
            label: label.into().into(),
            full: None,
        }
    }

    /// A quiet label with a free text and no glyph (for example 等待加入).
    pub fn quiet(label: impl Into<SharedString>) -> Self {
        Self {
            status: TopicStatus::Open,
            kind: ChipKind::Quiet,
            style: ChipStyle::Label,
            label: label.into(),
            full: None,
        }
    }

    /// Rule of one orange fill per screen (§7.8): a row that waits on the
    /// viewer but is not the first such row on screen draws the quiet label
    /// (`you-glyph` glyph, `text-2` text, no fill) instead of the filled
    /// compact chip. Other marks stay as they are.
    pub fn quiet_you(mut self) -> Self {
        if self.kind == ChipKind::You && self.style == ChipStyle::Compact {
            self.style = ChipStyle::Label;
        }
        self
    }

    /// The mark draws a fill (a chip, not a label).
    pub fn is_filled(&self) -> bool {
        self.style != ChipStyle::Label
    }

    pub fn style(mut self, style: ChipStyle) -> Self {
        self.style = style;
        self
    }

    /// The full text (with a name) for a tooltip.
    pub fn full_label(&self) -> Option<SharedString> {
        self.full.clone()
    }
}

/// The glyph of a status (§7.8).
pub fn status_glyph(status: TopicStatus) -> Option<Lucide> {
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
        let k = theme::colors(cx);
        let colors = theme::chip_colors(self.kind, self.status, cx);
        let glyph = status_glyph(self.status).filter(|_| self.style != ChipStyle::Compact);
        let (height, max_w) = match self.style {
            ChipStyle::Compact => (size::CHIP_COMPACT, size::CHIP_COMPACT_MAX),
            _ => (size::CHIP, size::CHIP_MAX),
        };
        let filled = self.style != ChipStyle::Label;
        // A quiet 等你… label keeps `text-2` text with the `you-glyph` glyph.
        let text = if !filled && self.kind == ChipKind::You {
            k.text_2
        } else {
            colors.text
        };
        div()
            .flex_none()
            .h(height)
            .max_w(max_w)
            .flex()
            .items_center()
            .gap(space::S4)
            .rounded(theme::radius::SM)
            .when(filled, |el| {
                el.bg(colors.bg).map(|el| match (self.style, glyph) {
                    (ChipStyle::Compact, _) => el.px(space::S6),
                    (_, Some(_)) => el.pl(space::S6).pr(space::S8),
                    (_, None) => el.px(space::S8),
                })
            })
            .t_meta_medium()
            .when(
                self.style == ChipStyle::Compact || self.kind == ChipKind::You && filled,
                |el| el.semibold(),
            )
            .text_color(text)
            .when_some(glyph, |el, g| {
                let glyph_color = if !filled && self.kind == ChipKind::You {
                    k.you_glyph
                } else {
                    colors.glyph
                };
                el.child(icon(g, theme::icon::CHIP, glyph_color))
            })
            .child(div().min_w(px(0.)).one_line().child(self.label))
    }
}

/// What a count badge counts (§7.8).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BadgeKind {
    /// Neutral solid badge: unread topics and DMs without a mention.
    Unread,
    /// Red `@n` badge: @ mentions in topic rows.
    Mention,
    /// Red `n` badge without `@` (mentions on a 群 row).
    MentionCount,
    /// Grey count in tabs and section headers (`count-bg`).
    Count,
    /// Older name: the 待办 count, now a grey count.
    Todo,
}

/// A 20 pt count pill. Renders nothing for zero. Every count caps at `99+`.
#[derive(IntoElement)]
pub struct CountBadge {
    count: u32,
    kind: BadgeKind,
}

impl CountBadge {
    pub fn new(count: u32, kind: BadgeKind) -> Self {
        Self { count, kind }
    }

    /// The capped text of a count (`99+` past 99).
    pub fn cap(count: u32) -> String {
        if count > 99 {
            common::COUNT_CAP.to_string()
        } else {
            count.to_string()
        }
    }

    pub fn text(count: u32, kind: BadgeKind) -> String {
        match kind {
            BadgeKind::Mention => format!("{}{}", nav::MENTION_MARK, Self::cap(count)),
            _ => Self::cap(count),
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
            BadgeKind::Mention | BadgeKind::MentionCount => (k.badge, k.on_accent),
            BadgeKind::Unread => (k.count_solid, k.on_accent),
            BadgeKind::Count | BadgeKind::Todo => (k.count_bg, k.count_text),
        };
        div()
            .flex_none()
            .h(size::BADGE)
            .min_w(size::BADGE)
            .px(space::S6)
            .rounded_full()
            .bg(bg)
            .text_color(fg)
            .flex()
            .items_center()
            .justify_center()
            .t_section()
            .tnum()
            .child(Self::text(self.count, self.kind))
            .into_any_element()
    }
}

/// The role tag after a bot's name (§7.9): 20 pt, `field`, `bot` glyph 12 +
/// 岗位 12/16 500 `text-2`.
#[derive(IntoElement)]
pub struct RoleTag {
    label: SharedString,
}

impl RoleTag {
    pub fn new(label: impl Into<SharedString>) -> Self {
        Self {
            label: label.into(),
        }
    }
}

impl RenderOnce for RoleTag {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let k = theme::colors(cx);
        div()
            .flex_none()
            .h(size::ROLE_TAG)
            .px(space::S6)
            .rounded(theme::radius::SM)
            .bg(k.field)
            .flex()
            .items_center()
            .gap(space::S4)
            .t_meta_medium()
            .text_color(k.text_2)
            .child(icon(Lucide::Bot, theme::icon::TAG, k.text_2))
            .child(div().one_line().max_w(px(120.)).child(self.label))
    }
}

/// The v3 岗位 tag after a member's name: 18 pt, radius 5, `sunken`,
/// 12/16 `ink-2`, no glyph (CONCEPT2 §6 "Bot identity in card headers").
#[derive(IntoElement)]
pub struct JobTag {
    label: SharedString,
}

impl JobTag {
    pub fn new(label: impl Into<SharedString>) -> Self {
        Self {
            label: label.into(),
        }
    }
}

/// Widest text of a [`JobTag`].
const JOB_TAG_TEXT_MAX: Pixels = px(120.);

/// `label` cut with `…` to fit `max` at the meta size, measured with the window's font, so a
/// box that holds it ends at the ellipsis.
fn cut_to(label: &str, max: Pixels, window: &Window) -> SharedString {
    let style = window.text_style();
    let font = style.font();
    let width = |s: &str| -> Pixels {
        let run = gpui::TextRun {
            len: s.len(),
            font: font.clone(),
            color: style.color,
            background_color: None,
            underline: None,
            strikethrough: None,
        };
        window
            .text_system()
            .shape_line(
                SharedString::from(s.to_string()),
                theme::type_scale::META.0,
                &[run],
                None,
            )
            .width
    };
    if width(label) <= max {
        return SharedString::from(label.to_string());
    }
    let chars: Vec<char> = label.chars().collect();
    let cut = |n: usize| -> String {
        let head: String = chars[..n].iter().collect();
        format!("{}…", head.trim_end())
    };
    let (mut lo, mut hi) = (0, chars.len());
    while lo < hi {
        let mid = (lo + hi).div_ceil(2);
        if width(&cut(mid)) <= max {
            lo = mid;
        } else {
            hi = mid - 1;
        }
    }
    SharedString::from(cut(lo))
}

impl RenderOnce for JobTag {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let k = theme::colors(cx);
        // The chip holds its text: a long role is cut to the widest size here, so the
        // ellipsis ends at the padding with no empty space after it.
        let text = cut_to(&self.label, JOB_TAG_TEXT_MAX, window);
        div()
            .flex_none()
            .h(theme::TAG_HEIGHT)
            .px(space::S6)
            .rounded(theme::TAG_RADIUS)
            .bg(k.sunken)
            .flex()
            .items_center()
            .t_meta()
            .text_color(k.ink_2)
            .child(div().whitespace_nowrap().child(text))
    }
}

/// A keyboard key (§7.8): 20 pt, min width 20, `field`, 12/16 500 `text-2`.
#[derive(IntoElement)]
pub struct Kbd {
    label: SharedString,
}

impl Kbd {
    pub fn new(label: impl Into<SharedString>) -> Self {
        Self {
            label: label.into(),
        }
    }
}

impl RenderOnce for Kbd {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let k = theme::colors(cx);
        div()
            .flex_none()
            .h(size::KBD)
            .min_w(size::KBD)
            .px(px(5.))
            .rounded(theme::radius::SM)
            .bg(k.field)
            .flex()
            .items_center()
            .justify_center()
            .t_meta_medium()
            .text_color(k.text_2)
            .child(self.label)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counts_cap_at_99() {
        assert_eq!(CountBadge::text(1000, BadgeKind::Unread), "99+");
        assert_eq!(CountBadge::text(99, BadgeKind::Unread), "99");
        assert_eq!(CountBadge::text(100, BadgeKind::Mention), "@99+");
        assert_eq!(CountBadge::text(3, BadgeKind::Mention), "@3");
        assert_eq!(CountBadge::text(12345, BadgeKind::Count), "99+");
    }
}
