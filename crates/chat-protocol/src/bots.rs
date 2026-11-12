//! Bot runs for clients (docs/protocol.md, "Bot runs"): runner-backed bot configuration,
//! machines and their health, the `run.updated` group event and the run log.

use serde::{Deserialize, Serialize};

use crate::entities::{Millis, Seq};
use crate::limits::{Validation, ValidationError, check_chars, check_name};
use crate::machine::{CliInfo, Health, RunLogEntry};
use crate::message::ArtifactRef;
use crate::serde_util::{non_null, nullable};

pub const MAX_LABELS: usize = 20;

/// A 思考强度 name: `[a-z][a-z0-9_-]{0,31}`.
pub fn is_effort(e: &str) -> bool {
    let b = e.as_bytes();
    !b.is_empty()
        && b.len() <= 32
        && b[0].is_ascii_lowercase()
        && b.iter()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || *c == b'_' || *c == b'-')
}

fn fail(field: &'static str, message: impl Into<String>) -> Validation {
    Err(ValidationError {
        field,
        message: message.into(),
    })
}

/// Labels: at most 20, each 1..64 of `[A-Za-z0-9_.:-]`.
pub fn check_labels(field: &'static str, labels: &[String]) -> Validation {
    if labels.len() > MAX_LABELS {
        return fail(field, format!("at most {MAX_LABELS} labels"));
    }
    for l in labels {
        let ok = (1..=64).contains(&l.len())
            && l.bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'.' | b':' | b'-'));
        if !ok {
            return fail(field, "a label is 1..64 of [A-Za-z0-9_.:-]");
        }
    }
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunStatus {
    Created,
    Running,
    Waiting,
    Blocked,
    Done,
    Failed,
    Cancelled,
}

/// Max items of a run plan.
pub const MAX_PLAN_ITEMS: usize = 30;
/// Max length of one plan item's text, in Unicode code points (as a topic title).
pub const MAX_PLAN_TEXT_CHARS: usize = 200;

/// The state of one plan item.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunPlanState {
    /// Finished.
    Done,
    /// The bot works on it now.
    Current,
    /// Waits for a member (an answer, a review).
    Waiting,
    /// Later.
    Pending,
}

/// One item of a run plan: the bot's ordered to-do list.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RunPlanItem {
    pub text: String,
    pub state: RunPlanState,
    /// 1.1: when the item became `current` (ms).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub started_at: Option<u64>,
    /// 1.1: when the item became `done` (ms).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ended_at: Option<u64>,
    /// 1.1: how long the bot expects the item to take (ms).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub estimate_ms: Option<u64>,
}

impl RunPlanItem {
    /// An item with only text and state.
    pub fn new(text: impl Into<String>, state: RunPlanState) -> Self {
        Self {
            text: text.into(),
            state,
            started_at: None,
            ended_at: None,
            estimate_ms: None,
        }
    }
}

/// A plan has at most 30 items, each with 1..200 characters of text.
pub fn check_plan(field: &'static str, plan: &[RunPlanItem]) -> Validation {
    if plan.len() > MAX_PLAN_ITEMS {
        return fail(field, format!("at most {MAX_PLAN_ITEMS} items"));
    }
    for item in plan {
        check_chars(field, &item.text, 1, MAX_PLAN_TEXT_CHARS)?;
    }
    Ok(())
}

/// One run of a bot in a topic: a runner-backed run, or one an external bot started.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BotRun {
    pub id: String,
    pub bot_id: String,
    pub group_id: String,
    pub topic_id: String,
    #[serde(
        default,
        deserialize_with = "non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub machine_id: Option<String>,
    pub status: RunStatus,
    #[serde(
        default,
        deserialize_with = "non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub requested_by: Option<String>,
    pub created_at: Millis,
    #[serde(
        default,
        deserialize_with = "non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub started_at: Option<Millis>,
    #[serde(
        default,
        deserialize_with = "non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub ended_at: Option<Millis>,
    pub updated_at: Millis,
    #[serde(
        default,
        deserialize_with = "non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub summary: Option<String>,
    /// The latest plan the bot or its runner set; absent when there is none.
    #[serde(
        default,
        deserialize_with = "non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub plan: Option<Vec<RunPlanItem>>,
    // 1.1 fields. Absent on older runs.
    /// Tokens and cost the CLI reported in `run.done`.
    #[serde(
        default,
        deserialize_with = "non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub usage: Option<RunUsage>,
    /// Run time the daemon reported in `run.done`.
    #[serde(
        default,
        deserialize_with = "non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub duration_ms: Option<u64>,
    /// The message (usually a Card v2 with intent "done") posted as the result.
    #[serde(
        default,
        deserialize_with = "non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub result_message_id: Option<String>,
    /// 验收标准 the run reports; replaced as a whole.
    #[serde(
        default,
        deserialize_with = "non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub acceptance: Option<Vec<AcceptanceItem>>,
    /// Present while a person has paused the run (接管): who.
    #[serde(
        default,
        deserialize_with = "non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub paused_by: Option<String>,
    /// [`RUN_KIND_ASIDE`] for a side chat run; absent = a normal run. Unknown values read as normal.
    #[serde(
        default,
        deserialize_with = "non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub kind: Option<String>,
    /// 1.1: the run this one retries (`POST /v1/runs/:id/retry`).
    #[serde(
        default,
        deserialize_with = "non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub retry_of: Option<String>,
    /// 1.1 (排队): set while a `created` run with no `machine_id` waits for a free machine.
    #[serde(
        default,
        deserialize_with = "non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub queued_at: Option<Millis>,
    /// 1.1 (排队): 1-based place among the bot's queued runs, oldest first.
    #[serde(
        default,
        deserialize_with = "non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub queue_position: Option<u32>,
    // Usable fields. Absent on older runs and servers.
    /// The person the run works for (交代人); `None` = no person (a hook), a private card
    /// answer, or a run from before (read `requested_by`).
    #[serde(
        default,
        deserialize_with = "non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub requested_for: Option<String>,
    /// The piece of work (chain) the run belongs to.
    #[serde(
        default,
        deserialize_with = "non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub chain_id: Option<String>,
    /// Where the run left its code (`run.done.code`).
    #[serde(
        default,
        deserialize_with = "non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub code: Option<crate::relay::RunCode>,
    /// The model the CLI really ran with (the `session` run event).
    #[serde(
        default,
        deserialize_with = "non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub model: Option<String>,
    /// The 思考强度 the CLI really ran with (the `session` run event); `None` = its default.
    #[serde(
        default,
        deserialize_with = "non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub effort: Option<String>,
    /// Why a failed run failed (`run.done.failure`); `None` = not known.
    #[serde(
        default,
        deserialize_with = "non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub failure: Option<crate::machine::RunFailure>,
    /// The one state people see ([`crate::takeover::RUN_PHASES`], or a newer value).
    #[serde(
        default,
        deserialize_with = "non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub phase: Option<String>,
    /// What a waiting phase waits for ([`crate::takeover::RUN_WAITING_FOR`]).
    #[serde(
        default,
        deserialize_with = "non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub waiting_for: Option<String>,
    /// With `paused_by` only: "pause" | "takeover".
    #[serde(
        default,
        deserialize_with = "non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub hold_kind: Option<String>,
    /// With `paused_by` only: when the hold started.
    #[serde(
        default,
        deserialize_with = "non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub held_at: Option<Millis>,
    /// How the run's machine can be taken over: "commands" | "isolated" | "none".
    #[serde(
        default,
        deserialize_with = "non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub takeover: Option<String>,
    /// The latest hand-back.
    #[serde(
        default,
        deserialize_with = "non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub last_handback: Option<crate::takeover::RunHandback>,
    /// Usable: what the desk did to this run by itself, oldest first (盯 5.5).
    #[serde(
        default,
        deserialize_with = "non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub supervision: Option<Vec<crate::supervise::SuperviseAct>>,
}

impl BotRun {
    /// Waits for a free machine (排队).
    pub fn is_queued(&self) -> bool {
        self.queued_at.is_some() && self.machine_id.is_none() && self.status == RunStatus::Created
    }
}

/// `BotRun.kind` of a read-only side chat run (单独问).
pub const RUN_KIND_ASIDE: &str = "aside";

impl BotRun {
    /// Still working: created, running, waiting or blocked.
    pub fn is_active(&self) -> bool {
        matches!(
            self.status,
            RunStatus::Created | RunStatus::Running | RunStatus::Waiting | RunStatus::Blocked
        )
    }
    pub fn is_aside(&self) -> bool {
        self.kind.as_deref() == Some(RUN_KIND_ASIDE)
    }
    pub fn is_paused(&self) -> bool {
        self.paused_by.is_some()
    }
    pub fn acceptance(&self) -> &[AcceptanceItem] {
        self.acceptance.as_deref().unwrap_or(&[])
    }
}

/// Tokens and cost of a run, as `run.done.usage` reported them.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RunUsage {
    pub input_tokens: u64,
    pub output_tokens: u64,
    /// US dollars; read with [`RunUsage::cost_usd`].
    #[serde(
        default,
        deserialize_with = "non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub cost_usd: Option<serde_json::Number>,
}

impl RunUsage {
    pub fn cost_usd(&self) -> Option<f64> {
        self.cost_usd.as_ref().and_then(serde_json::Number::as_f64)
    }
}

pub const MAX_ACCEPTANCE_ITEMS: usize = 20;

/// State of one acceptance criterion. Closed: new states need a new field.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AcceptanceState {
    Pending,
    Pass,
    Fail,
}

/// Proof of one criterion: a note, a file, a link or a 产出 version.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AcceptanceEvidence {
    #[serde(
        default,
        deserialize_with = "non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub text: Option<String>,
    #[serde(
        default,
        deserialize_with = "non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub file_id: Option<String>,
    #[serde(
        default,
        deserialize_with = "non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub url: Option<String>,
    #[serde(
        default,
        deserialize_with = "non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub artifact: Option<ArtifactRef>,
}

/// One acceptance criterion (验收标准) of a run.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AcceptanceItem {
    pub text: String,
    pub state: AcceptanceState,
    #[serde(
        default,
        deserialize_with = "non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub evidence: Option<AcceptanceEvidence>,
}

/// `POST /v1/runs` (a bot that is not runner-backed): starts a run of the calling bot in the
/// topic, or returns its active run there. `plan`, when given, replaces the plan.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StartRunRequest {
    pub group_id: String,
    pub topic_id: String,
    #[serde(
        default,
        deserialize_with = "non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub plan: Option<Vec<RunPlanItem>>,
}

impl StartRunRequest {
    pub fn validate(&self) -> Validation {
        crate::limits::check_id("groupId", &self.group_id)?;
        crate::limits::check_id("topicId", &self.topic_id)?;
        match &self.plan {
            Some(p) => check_plan("plan", p),
            None => Ok(()),
        }
    }
}

/// `PUT /v1/runs/:id/plan` (the run's bot): replaces the whole plan of an active run.
/// An empty list clears it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SetRunPlanRequest {
    pub plan: Vec<RunPlanItem>,
}

impl SetRunPlanRequest {
    pub fn validate(&self) -> Validation {
        check_plan("plan", &self.plan)
    }
}

/// How an external bot ends its run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EndRunStatus {
    Done,
    Failed,
    Cancelled,
}

/// `POST /v1/runs/:id/end` (the run's bot, runs started with `POST /v1/runs` only).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EndRunRequest {
    pub status: EndRunStatus,
    #[serde(
        default,
        deserialize_with = "non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub summary: Option<String>,
}

impl EndRunRequest {
    pub fn validate(&self) -> Validation {
        match &self.summary {
            Some(s) => check_chars("summary", s, 0, 280),
            None => Ok(()),
        }
    }
}

/// Checks the 1.1 acceptance criteria: at most 20, text 1..500, evidence within its limits.
pub fn check_acceptance(field: &'static str, items: &[AcceptanceItem]) -> Validation {
    if items.len() > MAX_ACCEPTANCE_ITEMS {
        return fail(field, format!("at most {MAX_ACCEPTANCE_ITEMS} items"));
    }
    for item in items {
        check_chars(field, &item.text, 1, 500)?;
        if let Some(e) = &item.evidence {
            if let Some(t) = &e.text {
                check_chars(field, t, 0, 1000)?;
            }
            if let Some(id) = &e.file_id {
                crate::limits::check_id(field, id)?;
            }
            if let Some(u) = &e.url {
                crate::card::check_web_url(field, u)?;
            }
            if let Some(a) = &e.artifact {
                crate::limits::check_id(field, &a.artifact_id)?;
                if a.version == 0 {
                    return fail(field, "artifact version must be positive");
                }
            }
        }
    }
    Ok(())
}

/// `PUT /v1/runs/:id/acceptance` (the run's bot, 1.1): replaces the acceptance criteria of an
/// active run. An empty list clears them.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SetRunAcceptanceRequest {
    pub items: Vec<AcceptanceItem>,
}

impl SetRunAcceptanceRequest {
    pub fn validate(&self) -> Validation {
        check_acceptance("items", &self.items)
    }
}

/// `POST /v1/runs/:id/resume` (1.1): `note` becomes the CLI's next input when set.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResumeRunRequest {
    #[serde(
        default,
        deserialize_with = "non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub note: Option<String>,
}

impl ResumeRunRequest {
    pub fn validate(&self) -> Validation {
        match &self.note {
            Some(n) => check_chars("note", n, 0, 2000),
            None => Ok(()),
        }
    }
}

/// Data of the `run.updated` group event (`actorId` = the bot).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RunUpdatedData {
    pub run: BotRun,
}

/// `GET /v1/runs/:id/events?afterSeq=&limit=`
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RunEventsResponse {
    pub run: BotRun,
    pub entries: Vec<RunLogEntry>,
    pub has_more: bool,
    pub last_seq: Seq,
}

/// What a run's prompt carried from its topic (docs/protocol.md, "Run context").
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RunContextManifest {
    pub resumed: bool,
    pub chars: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub first_seq: Option<Seq>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_seq: Option<Seq>,
    pub messages: u64,
    pub omitted: u64,
    pub files: u64,
    pub cards: u64,
    pub refs: u64,
}

/// `GET /v1/runs/:id/context` (owner).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RunContext {
    pub prompt: String,
    pub manifest: RunContextManifest,
}

/// One message as a run reads it (machine-protocol.md, "Topic context").
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContextMessage {
    pub id: String,
    pub seq: Seq,
    pub author_id: String,
    pub author_name: String,
    pub at: Millis,
    pub text: String,
}

/// `GET /v1/machine/runs/:id/topic/messages`
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TopicHistoryResponse {
    pub messages: Vec<ContextMessage>,
    pub has_more: bool,
}

/// `GET /v1/machine/runs/:id/topic/messages/:messageId`
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TopicMessageResponse {
    pub message: ContextMessage,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContextHit {
    pub message_id: String,
    pub topic_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub topic_title: Option<String>,
    pub seq: Seq,
    pub author_name: String,
    pub at: Millis,
    pub snippet: String,
    /// Another group the run may read (design-visibility): that group; absent for the run's own.
    #[serde(
        default,
        deserialize_with = "non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub group_id: Option<String>,
    #[serde(
        default,
        deserialize_with = "non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub group_name: Option<String>,
}

/// The front desk's work order (交代单) for the executor.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkBrief {
    pub goal: String,
    pub done_when: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub notes: Option<String>,
}

/// "reply" | "dispatch" | "ask" | "skip"
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FrontDeskAction {
    Reply,
    Dispatch,
    Ask,
    Skip,
}

/// One front desk decision (docs/protocol.md, "Front desk").
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FrontDeskDecision {
    pub message_id: String,
    pub topic_id: String,
    pub author_id: String,
    pub action: FrontDeskAction,
    pub reason: String,
    pub confidence: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reply: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub brief: Option<WorkBrief>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fallback: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    pub ms: u64,
    pub input_tokens: u64,
    pub output_tokens: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reply_message_id: Option<String>,
    pub at: Millis,
}

/// `GET /v1/bots/:id/decisions?groupId=&topicId=` (owner), newest first.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FrontDeskDecisionsResponse {
    pub decisions: Vec<FrontDeskDecision>,
}

/// `GET /v1/bots/:id/metrics?days=` (owner).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BotMetrics {
    pub since: Millis,
    pub front_desk: FrontDeskMetrics,
    pub runs: RunMetrics,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FrontDeskMetrics {
    pub decisions: u64,
    pub by_action: FrontDeskActionCounts,
    pub fallbacks: u64,
    pub p50_ms: u64,
    pub p95_ms: u64,
    pub input_tokens: u64,
    pub output_tokens: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FrontDeskActionCounts {
    pub reply: u64,
    pub dispatch: u64,
    pub ask: u64,
    pub skip: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RunMetrics {
    pub total: u64,
    pub done: u64,
    pub failed: u64,
    pub blocked: u64,
    pub cancelled: u64,
    pub restarted: u64,
    pub continued: u64,
    pub interventions: u64,
    pub p50_ms: u64,
    pub p95_ms: u64,
}

/// `GET /v1/machine/runs/:id/search`
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContextSearchResponse {
    pub hits: Vec<ContextHit>,
}

/// `GET /v1/machine/runs/:id/artifacts?scope=topic|group`: the 产出 of the run's topic or group.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RunArtifactsResponse {
    pub artifacts: Vec<crate::files::Artifact>,
}

/// Characters of a 产出's entry text that `artifact get` carries.
pub const MAX_RUN_ARTIFACT_TEXT: usize = 200_000;

/// `GET /v1/machine/runs/:id/artifacts/:artifactId?version=`: one 产出 with its versions (newest
/// first), the version read and its entry text when that is text.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RunArtifactResponse {
    pub artifact: crate::files::Artifact,
    pub versions: Vec<crate::files::ArtifactVersion>,
    pub version: u32,
    #[serde(
        default,
        deserialize_with = "non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub text: Option<String>,
    #[serde(
        default,
        deserialize_with = "non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub truncated: Option<bool>,
}

/// One answer of a card as a run reads it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RunCardAnswer {
    pub actor_id: String,
    pub actor_name: String,
    #[serde(
        default,
        deserialize_with = "non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub at: Option<Millis>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value: Option<serde_json::Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub inputs: Option<serde_json::Value>,
}

/// The open ask of a card as a run reads it: who is waited on.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RunCardAsk {
    pub status: String,
    pub from: Vec<String>,
}

/// `GET /v1/machine/runs/:id/cards/:messageId`: a card of the run's group, whole.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RunCardResponse {
    pub topic_id: String,
    /// Another group the run may read (design-visibility): that group; absent for the run's own.
    #[serde(
        default,
        deserialize_with = "non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub group_id: Option<String>,
    #[serde(
        default,
        deserialize_with = "non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub group_name: Option<String>,
    pub message: ContextMessage,
    pub card: serde_json::Value,
    #[serde(
        default,
        deserialize_with = "non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub ask: Option<RunCardAsk>,
    pub answers: Vec<RunCardAnswer>,
    #[serde(
        default,
        deserialize_with = "non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub answer_count: Option<u64>,
}

/// A group decision with the answerer's name.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RunDecision {
    #[serde(flatten)]
    pub decision: crate::card2::GroupDecision,
    pub answered_by_name: String,
}

/// `GET /v1/machine/runs/:id/decisions?limit=`: the group's decisions, newest first.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RunDecisionsResponse {
    pub decisions: Vec<RunDecision>,
    pub has_more: bool,
    /// Decisions recorded by hand (记一条), newest first.
    #[serde(
        default,
        deserialize_with = "non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub recorded: Option<Vec<RunRecordedDecision>>,
}

/// A decision recorded by hand with the recorder's name.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RunRecordedDecision {
    #[serde(flatten)]
    pub decision: crate::card2::RecordedDecision,
    pub recorded_by_name: String,
}

/// One member of the run's group: name, 岗位 and whether it is an AI member.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RunMember {
    pub id: String,
    pub name: String,
    #[serde(
        default,
        deserialize_with = "non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub role: Option<String>,
    pub ai: bool,
}

/// `GET /v1/machine/runs/:id/members`
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RunMembersResponse {
    pub members: Vec<RunMember>,
}

/// `GET /v1/machine/runs/:id/topic/state?topic=`: one compact state card of a topic.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TopicStateResponse {
    pub topic_id: String,
    /// Another group the run may read (design-visibility): that group; absent for the run's own.
    #[serde(
        default,
        deserialize_with = "non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub group_id: Option<String>,
    #[serde(
        default,
        deserialize_with = "non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub group_name: Option<String>,
    pub title: String,
    pub text: String,
}

/// "group" | "dm"
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RunGroupKind {
    Group,
    Dm,
}

/// One group a run may read: its own (`current`) and the groups of the person it works for.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RunGroup {
    pub id: String,
    pub name: String,
    pub kind: RunGroupKind,
    #[serde(
        default,
        deserialize_with = "non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub purpose: Option<String>,
    pub current: bool,
}

/// `GET /v1/machine/runs/:id/groups` (feature `groups`): the groups the run may read, its own first.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RunGroupsResponse {
    pub groups: Vec<RunGroup>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RunnerRepo {
    #[serde(
        default,
        deserialize_with = "non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub url: Option<String>,
    #[serde(
        default,
        deserialize_with = "non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub path: Option<String>,
}

/// `PUT /v1/bots/:id/runner` (owner)
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SetBotRunnerRequest {
    pub runtime: String,
    #[serde(
        default,
        deserialize_with = "non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub labels: Option<Vec<String>>,
    #[serde(
        default,
        deserialize_with = "non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub repo: Option<RunnerRepo>,
    #[serde(
        default,
        deserialize_with = "non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub base_branch: Option<String>,
    #[serde(
        default,
        deserialize_with = "non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub model: Option<String>,
    /// Absent keeps the stored effort; `Some(None)` = `null` clears it.
    #[serde(
        default,
        deserialize_with = "nullable",
        skip_serializing_if = "Option::is_none"
    )]
    pub effort: Option<Option<String>>,
    #[serde(
        default,
        deserialize_with = "non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub env_allowlist: Option<Vec<String>>,
    #[serde(
        default,
        deserialize_with = "non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub enabled: Option<bool>,
    /// 1.2.2: the one machine that runs this bot (labels then do not count). Absent keeps the
    /// stored machine; `Some(None)` = `null` lets any machine with the labels run it.
    #[serde(
        default,
        deserialize_with = "nullable",
        skip_serializing_if = "Option::is_none"
    )]
    pub machine_id: Option<Option<String>>,
}

impl SetBotRunnerRequest {
    pub fn validate(&self) -> Validation {
        if self.runtime.is_empty() || self.runtime.len() > 64 {
            return fail("runtime", "1..64 characters");
        }
        if let Some(l) = &self.labels {
            check_labels("labels", l)?;
        }
        if let Some(r) = &self.repo
            && r.url.is_some() == r.path.is_some()
        {
            return fail("repo", "repo needs url or path, not both");
        }
        if let Some(Some(e)) = &self.effort
            && !is_effort(e)
        {
            return fail(
                "effort",
                "lower-case letters, digits, _ and -, 1..32 characters",
            );
        }
        Ok(())
    }
}

/// `GET` and `PUT /v1/bots/:id/runner`
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BotRunner {
    pub bot_id: String,
    pub runtime: String,
    pub labels: Vec<String>,
    #[serde(
        default,
        deserialize_with = "non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub repo: Option<RunnerRepo>,
    #[serde(
        default,
        deserialize_with = "non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub base_branch: Option<String>,
    #[serde(
        default,
        deserialize_with = "non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub model: Option<String>,
    #[serde(
        default,
        deserialize_with = "non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub effort: Option<String>,
    pub env_allowlist: Vec<String>,
    pub enabled: bool,
    pub updated_at: Millis,
    /// 1.2.2: the one machine that runs this bot; absent = any machine with the labels.
    #[serde(
        default,
        deserialize_with = "non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub machine_id: Option<String>,
}

/// `Machine.unhealthy` values (机 5.6).
pub mod unhealthy {
    /// No heartbeat for 90 s.
    pub const HEARTBEAT: &str = "heartbeat";
    /// Free disk below the limit.
    pub const DISK: &str = "disk";
}

/// A machine that runs coding CLIs for bots (settings: 机器).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Machine {
    pub id: String,
    pub name: String,
    pub labels: Vec<String>,
    pub created_by: String,
    pub created_at: Millis,
    #[serde(
        default,
        deserialize_with = "non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub revoked: Option<bool>,
    pub connected: bool,
    pub healthy: bool,
    /// Usable (机 5.6): the check that failed when connected and not healthy ([`unhealthy`]
    /// values; an unknown one is a failed check without a name). None otherwise.
    #[serde(
        default,
        deserialize_with = "non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub unhealthy: Option<String>,
    #[serde(
        default,
        deserialize_with = "non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub last_heartbeat_at: Option<Millis>,
    #[serde(
        default,
        deserialize_with = "non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub daemon_version: Option<String>,
    #[serde(
        default,
        deserialize_with = "non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub os: Option<String>,
    #[serde(
        default,
        deserialize_with = "non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub arch: Option<String>,
    #[serde(
        default,
        deserialize_with = "non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub max_parallel_runs: Option<u32>,
    #[serde(
        default,
        deserialize_with = "non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub health: Option<Health>,
    #[serde(
        default,
        deserialize_with = "non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub clis: Option<Vec<CliInfo>>,
    pub running_runs: Vec<String>,
    /// 1.1: what the daemon supports (`hello.machine.features`). Owners only.
    #[serde(
        default,
        deserialize_with = "non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub features: Option<Vec<String>>,
    /// 1.1: screen sharing address for 接管. Owners and run requesters only.
    #[serde(
        default,
        deserialize_with = "non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub takeover_url: Option<String>,
    /// Usable: the person whose own machine this is (个人的电脑); `None` = a team machine.
    #[serde(
        default,
        deserialize_with = "non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub owner_id: Option<String>,
    /// Usable: the AI members whose work for the owner runs here. Sent to the owner only.
    #[serde(
        default,
        deserialize_with = "non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub personal_bots: Option<Vec<PersonalBot>>,
    /// Usable: the computer's own GitHub sign-in (`metrics.github`). Admins and the computer's
    /// owner only.
    #[serde(
        default,
        deserialize_with = "non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub github: Option<crate::machine::MachineGithub>,
}

/// Usable: at most this many AI members on one personal machine.
pub const MAX_PERSONAL_BOTS: usize = 200;

/// Usable: an AI member whose work for the machine's owner runs on that machine.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PersonalBot {
    pub bot_id: String,
    /// The owner's code folder on this machine for that work.
    #[serde(
        default,
        deserialize_with = "non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub repo_path: Option<String>,
}

/// `POST /v1/machines` (owner; usable: any member, see `personal`)
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateMachineRequest {
    pub name: String,
    #[serde(
        default,
        deserialize_with = "non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub labels: Option<Vec<String>>,
    /// Usable: true = my own machine; `None` = a team machine for an admin, my own for a member.
    #[serde(
        default,
        deserialize_with = "non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub personal: Option<bool>,
}

impl CreateMachineRequest {
    pub fn validate(&self) -> Validation {
        check_name("name", &self.name)?;
        if let Some(l) = &self.labels {
            check_labels("labels", l)?;
        }
        Ok(())
    }
}

/// Usable: `UpdateMachineRequest.owner = "me"`.
pub const MACHINE_OWNER_ME: &str = "me";

/// `PATCH /v1/machines/:id` (usable): rename it, or move it between the team and me.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateMachineRequest {
    #[serde(
        default,
        deserialize_with = "non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub name: Option<String>,
    /// `Some(Some("me"))` = make it my own machine; `Some(None)` (`null`) = give it to the team.
    #[serde(
        default,
        deserialize_with = "nullable",
        skip_serializing_if = "Option::is_none"
    )]
    pub owner: Option<Option<String>>,
}

impl UpdateMachineRequest {
    pub fn validate(&self) -> Validation {
        if self.name.is_none() && self.owner.is_none() {
            return fail("name", "name or owner is required");
        }
        if let Some(n) = &self.name {
            check_name("name", n)?;
        }
        if let Some(Some(o)) = &self.owner
            && o != MACHINE_OWNER_ME
        {
            return fail("owner", "owner is \"me\" or null");
        }
        Ok(())
    }
}

/// `PUT /v1/machines/:id/bots/:botId` (the machine's owner, usable). Response: [`Machine`].
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SetMachineBotRequest {
    /// My code folder on this machine; `Some(None)` (`null`) clears it, `None` keeps it.
    #[serde(
        default,
        deserialize_with = "nullable",
        skip_serializing_if = "Option::is_none"
    )]
    pub repo_path: Option<Option<String>>,
}

impl SetMachineBotRequest {
    pub fn validate(&self) -> Validation {
        if let Some(Some(p)) = &self.repo_path {
            check_chars("repoPath", p, 1, 1024)?;
        }
        Ok(())
    }
}

/// The token is shown only in this response.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateMachineResponse {
    pub machine: Machine,
    pub token: String,
}

/// `GET /v1/machines` (owner)
pub type MachinesResponse = Vec<Machine>;
