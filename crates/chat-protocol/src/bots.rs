//! Bot runs for clients (docs/protocol.md, "Bot runs"): runner-backed bot configuration,
//! machines and their health, the `run.updated` group event and the run log.

use serde::{Deserialize, Serialize};

use crate::entities::{Millis, Seq};
use crate::limits::{Validation, ValidationError, check_chars, check_name};
use crate::machine::{CliInfo, Health, RunLogEntry};
use crate::message::ArtifactRef;
use crate::serde_util::non_null;

pub const MAX_LABELS: usize = 20;

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
    pub env_allowlist: Vec<String>,
    pub enabled: bool,
    pub updated_at: Millis,
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
}

/// `POST /v1/machines` (owner)
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

/// The token is shown only in this response.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateMachineResponse {
    pub machine: Machine,
    pub token: String,
}

/// `GET /v1/machines` (owner)
pub type MachinesResponse = Vec<Machine>;
