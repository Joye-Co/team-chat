//! View types of the v2 screens (1.1.0): the 首页 desk, the topic rail, 单独问,
//! 实时查看, the 群 page, the 成员 page and the 产出 page.
//!
//! Most of this is computed on the client from the synced cache (desk
//! buckets, group pages, member stats from runs). The [`super::ChatProvider`]
//! methods that return these types all have default implementations, so a
//! provider can add each one when its data exists; a view hides the part
//! whose data is missing. Protocol changes that carry new data come later,
//! one track at a time (see the 1.1.0 spec, "Protocol").

use super::{GroupId, MemberId, MessageId, Millis, TopicId};

/// Jojo's one-line priority at the top of the 首页 (from its latest desk
/// note). Hidden when there is none.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DeskNote {
    pub member_id: MemberId,
    pub text: String,
    pub at: Millis,
}

/// What kind of decision an ask wants.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum AskKind {
    /// Approve or refuse something that cannot be undone (a release).
    Approval,
    /// Pick one of a few options.
    Choice,
    /// Review a change (a PR).
    Review,
    /// Confirm or edit a draft.
    Draft,
    /// A 话题 that waits on me without a card.
    Topic,
}

/// One entry of 需要你拍板: something addressed to me that waits on my
/// decision. `message_id` points at the card to render when there is one.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AskItem {
    pub group_id: GroupId,
    pub topic_id: TopicId,
    pub message_id: Option<MessageId>,
    /// Who asks.
    pub from: MemberId,
    pub kind: AskKind,
    /// The question, as the asker wrote it ("把 qqbot v2.14 发布到生产吗？").
    pub title: String,
    pub at: Millis,
}

/// A proof chip on a result: a PR with its line counts, test counts, a
/// recording, a 产出.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Proof {
    Pr {
        number: u32,
        additions: u32,
        deletions: u32,
    },
    Tests {
        passed: u32,
        failed: u32,
    },
    Recording {
        duration_ms: u64,
    },
    Artifact {
        artifact_id: String,
        title: String,
        version: u32,
    },
    Checks {
        passed: u32,
        total: u32,
    },
}

/// One entry of 做完了 (today): a result with its proof.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DoneItem {
    pub group_id: GroupId,
    pub topic_id: TopicId,
    /// Who did it.
    pub member_id: MemberId,
    /// The one-line result ("做好了 10 月数据月报").
    pub title: String,
    pub at: Millis,
    pub proof: Vec<Proof>,
}

/// One entry of 排队: work a member will start later.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct QueuedItem {
    pub member_id: MemberId,
    pub title: String,
    /// When it joined the queue (`BotRun.queuedAt`), when known.
    pub at: Option<Millis>,
}

/// The state of a change (PR).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ChangeState {
    Open,
    Merging,
    Merged,
    Closed,
}

/// 改动: the code change of a 话题 (a PR).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TopicChanges {
    pub number: u32,
    pub title: String,
    pub url: Option<String>,
    pub additions: u32,
    pub deletions: u32,
    pub files: u32,
    pub checks_passed: u32,
    pub checks_total: u32,
    pub state: ChangeState,
}

/// The currency a cost was reported in. A cost is shown in the currency the
/// data has; it is never converted.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Currency {
    /// US dollars (`BotRun.usage.costUsd`).
    #[default]
    Usd,
    /// Yuan.
    Cny,
}

/// What a run used (`BotRun.usage` + `durationMs`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RunUsage {
    pub duration_ms: u64,
    /// Cost in cents of [`RunUsage::currency`] (`$0.85` is 85).
    pub cost_cents: Option<u64>,
    pub currency: Currency,
    pub input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
}

/// 用时和成本 of a 话题: all its runs together.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TopicCost {
    pub duration_ms: u64,
    /// Cents of [`TopicCost::currency`]; `None` when no run reported a cost,
    /// or when the runs reported different currencies.
    pub cost_cents: Option<u64>,
    pub currency: Currency,
    pub started_at: Option<Millis>,
    pub ended_at: Option<Millis>,
}

/// A decision: an answered ask marked to keep (群里的决定, 用到的决定).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Decision {
    pub id: String,
    pub group_id: GroupId,
    /// The 话题 it was made in, when it came from one.
    pub topic_id: Option<TopicId>,
    pub text: String,
    pub by: MemberId,
    pub at: Millis,
}

/// A suggestion in a 单独问 answer: a button that posts into the 话题 only
/// when pressed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SideChatSuggestion {
    pub text: String,
    /// When it was posted into the 话题; `None` until pressed.
    pub posted_at: Option<Millis>,
}

/// What a 单独问 answer read, for its `看了 …` line.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SideChatSource {
    /// A PR, with its changed files when known.
    Pr { number: u32, files: Option<u32> },
    /// A 产出 version.
    Artifact { title: String, version: u32 },
}

/// One message of a 单独问 aside.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SideChatMessage {
    pub id: String,
    pub author_id: MemberId,
    pub body: String,
    pub at: Millis,
    /// Short reference chips under the answer (`reconnect.ts`).
    pub refs: Vec<String>,
    /// What the answer read (`看了 PR #61 的 12 个文件和「重连测试报告」v2`).
    /// Empty when unknown: the line is hidden.
    pub sources: Vec<SideChatSource>,
    pub suggestion: Option<SideChatSuggestion>,
    /// The member is still writing it.
    pub streaming: bool,
}

/// A 单独问 aside with one member about one 话题. Private to me.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SideChatThread {
    pub member_id: MemberId,
    pub messages: Vec<SideChatMessage>,
}

/// The kind of a line on the live screen.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ScreenLineKind {
    /// A command the member ran.
    Command,
    /// Its output.
    Output,
    /// A result line worth reading (`平均 118 fps`).
    Result,
    Error,
}

/// One line of the terminal rebuilt from a run's tool events.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ScreenLine {
    pub kind: ScreenLineKind,
    pub text: String,
    pub at: Millis,
    /// The plan step it belongs to (index into the run's plan), for 回放.
    pub step: Option<usize>,
}

/// 实时查看: what a member's computer shows for one run.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MachineScreen {
    pub run_id: String,
    pub machine_name: Option<String>,
    pub lines: Vec<ScreenLine>,
    pub started_at: Option<Millis>,
    /// The machine offers screen sharing (`在屏幕共享里打开`).
    pub takeover_url: Option<String>,
    /// Who took over (the run is paused), when someone did.
    pub paused_by: Option<MemberId>,
}

/// One item of 验收标准, reported by the run.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct AcceptanceItem {
    pub text: String,
    /// `None` while not checked yet.
    pub met: Option<bool>,
    /// What the run measured (`现在 57 fps`), when it said.
    pub evidence: Option<String>,
    /// A file of the 话题 the run gave as proof, when the 话题 has it.
    pub evidence_file: Option<crate::data::FileAttachment>,
    /// A web page the run gave as proof.
    pub evidence_url: Option<String>,
    /// A 产出 version the run gave as proof: id, version and title.
    pub evidence_artifact: Option<(String, u32, String)>,
}

/// A machine as the 成员 page and the topic rail show it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MemberMachine {
    pub name: String,
    pub online: bool,
    /// The member uses it now.
    pub busy: bool,
    pub disk_used_pct: Option<u8>,
}

/// A recent problem of a member (最近的问题).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MemberProblem {
    pub text: String,
    pub at: Millis,
    /// The run to retry, when it can be retried.
    pub run_id: Option<String>,
}

/// The 成员 page detail of one member.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MemberStats {
    /// 话题 the member works on now.
    pub active: Vec<(GroupId, TopicId)>,
    pub queued: u32,
    pub topics_this_week: u32,
    pub done_this_week: u32,
    pub artifacts_this_week: u32,
    pub cost_cents_this_week: Option<u64>,
    /// The currency of [`MemberStats::cost_cents_this_week`] (never converted).
    pub cost_currency: Currency,
    /// 汇报给 (`Member.reportsTo`).
    pub reports_to: Option<MemberId>,
    /// 能用的工具.
    pub tools: Vec<String>,
    pub machine: Option<MemberMachine>,
    pub problems: Vec<MemberProblem>,
}

/// 来龙去脉 of a 产出.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ArtifactStory {
    /// The 话题 it came from.
    pub topic: Option<(GroupId, TopicId)>,
    pub made_by: Option<MemberId>,
    pub asked_by: Option<MemberId>,
    pub decisions: Vec<Decision>,
    /// (version, what changed) newest first.
    pub version_notes: Vec<(u32, String)>,
    /// 话题 that point to it: (group, topic, title).
    pub cited_by: Vec<(GroupId, TopicId, String)>,
}

// --- topic track (v11/topic): the 话题 page, its pinned card and 产出 pane ---

/// One big number of a 产出 preview (`6 / 6 场景 · 全部通过`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PreviewMetric {
    pub label: String,
    pub value: String,
    /// After the value in `ink-3` (`/ 6`, `秒`).
    pub unit: Option<String>,
    pub note: Option<String>,
}

/// One row of a 产出 preview table: a label, a pass or fail mark and a mono
/// value.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PreviewRow {
    pub label: String,
    pub ok: Option<bool>,
    pub value: String,
}

/// One bar of a 产出 preview chart.
#[derive(Clone, Debug, PartialEq)]
pub struct PreviewBar {
    pub label: String,
    /// 0..=1 of the chart width.
    pub share: f32,
    pub value: String,
    /// The bar the chart is about (drawn in `ink`).
    pub highlight: bool,
}

/// A short, real preview of a 产出 version for the topic rail thumbnail and
/// the 产出 pane, read from the page itself. `None` when the provider cannot
/// read the page; the views then show the title and 在窗口打开 only.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct ArtifactPreview {
    /// The mono line over the title (`测试报告 · staging`).
    pub eyebrow: Option<String>,
    pub title: String,
    pub summary: Option<String>,
    pub metrics: Vec<PreviewMetric>,
    pub table_title: Option<String>,
    /// Header of the table: label, mark and value columns.
    pub columns: Vec<String>,
    pub rows: Vec<PreviewRow>,
    pub chart_title: Option<String>,
    pub chart_note: Option<String>,
    pub bars: Vec<PreviewBar>,
    /// Axis labels under the bars (`0 1 2`).
    pub axis: Vec<String>,
}
