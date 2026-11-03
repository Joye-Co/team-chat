//! View types of the 群, 成员 and 产出 pages (1.1.0, the pages track).
//!
//! Like [`super::v2`], every [`super::ChatProvider`] method that returns these
//! has a default implementation computed from the synced cache, so a view
//! hides the part whose data is missing.

use super::{GroupId, MemberId, Millis, TopicId, v2::Decision};

/// What a 进行中 card on the 群 page previews of its 话题.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TopicPreview {
    /// The first lines of the change (a PR), with its file and counts.
    Diff {
        path: String,
        additions: u32,
        deletions: u32,
        lines: Vec<DiffLine>,
    },
    /// The options of the ask that waits on me.
    Choice { options: Vec<ChoiceOption> },
    /// The live terminal of an active run (from its tool events).
    Screen { run_id: String },
    /// A search the member tried, with matching lines (`query` is marked).
    Matches {
        query: String,
        count: u32,
        lines: Vec<String>,
    },
}

/// One line of a [`TopicPreview::Diff`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DiffLine {
    pub added: bool,
    pub text: String,
}

/// One option of a [`TopicPreview::Choice`]: its key (`A`), label and an
/// optional sketch of what it looks like.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChoiceOption {
    pub key: String,
    pub label: String,
    pub sketch: Option<ChoiceSketch>,
}

/// A small picture of an option.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ChoiceSketch {
    /// One text field holding `text`.
    Field(String),
    /// `total` boxes, the first ones holding the characters of `filled`.
    Cells { total: u8, filled: String },
    /// A line of detail text.
    Text(String),
}

/// What the 群 page shows about a 话题 beyond its state.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TopicGlance {
    /// After the state line (`分词已验收`, `输入框用 A 还是 B`).
    pub state_note: Option<String>,
    /// In the card footer (`Forge 推荐 A：实习生大多从微信复制`).
    pub footer_note: Option<String>,
    pub preview: Option<TopicPreview>,
    /// `测试 148 项通过`.
    pub tests_passed: Option<u32>,
}

/// One row of 最近完成 on the 群 page.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RecentDone {
    pub group_id: GroupId,
    pub topic_id: TopicId,
    pub member_id: MemberId,
    pub title: String,
    /// The result in a few words (`已发布`).
    pub note: Option<String>,
    pub at: Millis,
}

/// A small picture of a 产出's first lines, for lists: a title line and
/// up to three rows of `(label, value, ok)`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ArtifactThumb {
    pub title: String,
    pub rows: Vec<ThumbRow>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ThumbRow {
    pub label: String,
    pub value: Option<String>,
    /// A green check at the end.
    pub ok: bool,
}

/// A 产出 or a change a member delivered (最近交付).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DeliveryKind {
    Pr {
        number: u32,
        additions: u32,
        deletions: u32,
    },
    Artifact {
        artifact_id: String,
        version: u32,
    },
}

/// Where a delivery stands.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DeliveryState {
    /// Not merged; it waits on my review.
    WaitsOnMe,
    Open,
    Merged,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Delivery {
    pub kind: DeliveryKind,
    pub title: String,
    pub at: Millis,
    pub topic: Option<(GroupId, TopicId)>,
    pub state: Option<DeliveryState>,
}

/// A tool a member may use and how far (`GitHub` · `可以提 PR`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MemberTool {
    pub name: String,
    pub access: Option<String>,
    pub detail: Option<String>,
}

/// A 话题 a member starts after the current one.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct QueuedWork {
    pub topic: Option<(GroupId, TopicId)>,
    pub title: String,
    /// When or after what it starts (`你选好输入框后开始`).
    pub note: Option<String>,
}

/// The 成员 page beyond [`super::v2::MemberStats`].
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MemberExtras {
    /// The second line of the member's list row, when the provider knows
    /// better than the derived one (`离开 · 08:50 后没回来`). For a member
    /// that waits on me it is the part after `等你 · `.
    pub line: Option<String>,
    /// Average time of a finished 话题 this week.
    pub avg_topic_ms: Option<u64>,
    /// 产出 this week by kind: PRs and pages.
    pub prs_this_week: u32,
    pub pages_this_week: u32,
    /// The weekly spending limit.
    pub budget_cents: Option<u64>,
    pub deliveries: Vec<Delivery>,
    pub tools: Vec<MemberTool>,
    pub queued: Vec<QueuedWork>,
    /// 用到的决定: decisions the member's 话题 used.
    pub decisions: Vec<Decision>,
}

/// A response from inside a 产出 page (页面里的回应): a viewer's event, or
/// what the author did with it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ArtifactResponse {
    pub member_id: MemberId,
    pub text: String,
    pub at: Option<Millis>,
    /// The 话题 where it goes on.
    pub topic: Option<(GroupId, TopicId)>,
}
