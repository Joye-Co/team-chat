//! Machine protocol (docs/machine-protocol.md): frames between the machine daemon and the
//! server's MachineLink, and the normalized run events that clients read in the run log.
//! JSON, camelCase, frames tagged on `t`, run events on `type`. Unknown fields are ignored.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;

use crate::serde_util::ProtocolV1;

pub const MACHINE_PROTOCOL: u32 = 1;

/// `hello.machine.features` values of the 1.1 daemon. The server sends a 1.1 frame or field
/// only to a daemon that reports its feature.
pub mod feature {
    /// `run.pause` and `run.resume`.
    pub const PAUSE: &str = "pause";
    /// `run.input.answer` and `run.input.artifactEvent`.
    pub const ANSWER: &str = "answer";
    /// `run.assign.kind = "aside"` (read-only side chat runs).
    pub const ASIDE: &str = "aside";
    /// The `acceptance` run event.
    pub const ACCEPTANCE: &str = "acceptance";
    /// 1.1 interaction loop: `run.answer` and `run.interaction`.
    pub const INTERACT: &str = "interact";
    /// 1.1 `chat-run`: RunEvent `output` and `run.output.result`.
    pub const OUTPUT: &str = "output";
    /// `run.assign.resumeSessionId`: continue the CLI session of the topic's last run.
    pub const RESUME: &str = "resume";
    /// chat-run topic context ops: `topic.history`, `topic.message`, `context.search`, `file.get`,
    /// `topic.state`, `artifact.list`, `artifact.get`, `card.get`, `decisions.list`, `member.list`.
    pub const CONTEXT: &str = "context";
    /// `CliInfo.models` / `efforts` and `run.assign.effort`.
    pub const EFFORT: &str = "effort";
    /// `run.assign.gates`, `CliInfo.gates`, `approval.request.kind`/`command` and
    /// `run.approval.scope`: the run stops at gated commands for a person's approval.
    pub const GATES: &str = "gates";
    /// `machine.publicKey`, `account.login`, `secret.set` / `secret.delete` and the
    /// `secrets` / `usage` metrics: sign in and API keys from the app.
    pub const ACCOUNTS: &str = "accounts";
    /// Usable: `hello.machine.runsIn`, `run.assign.github`, the `github` run event and the
    /// GitHub ops of `chat-run`.
    pub const GITHUB: &str = "github";
    /// Usable: `run.pause.hold`, `run.command`, `run.command.cancel` and the `hand.*` run events.
    pub const TAKEOVER: &str = "takeover";
    /// Usable: with `takeover`, a person's commands run as another user than the run (VM runs).
    pub const TAKEOVER_ISOLATED: &str = "takeover-isolated";
    /// Usable: the `homes` metrics and `home.clear` (VM hosts: each AI member's home folder).
    pub const HOMES: &str = "homes";
}

/// Most entries of `MachineMetrics.homes`.
pub const MAX_HOMES: usize = 500;

/// Usable, feature "homes": one AI member's kept home folder on a VM host.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HomeUse {
    pub member_id: String,
    /// Bytes the home takes on the host disk.
    pub used_bytes: u64,
    /// Its fixed size; absent = no limit.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub size_bytes: Option<u64>,
}

/// `run.approval.scope = "run"` (这件事里都同意): later matches of the same kind in the run pass.
pub const APPROVAL_SCOPE_RUN: &str = "run";

/// One gated action kind of a run (feature "gates"): before a tool runs a shell command that a
/// pattern matches (case-insensitive regex), the daemon asks for approval and waits.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RunGate {
    pub kind: String,
    pub patterns: Vec<String>,
}

/// A model a CLI can run (machine-protocol.md, "Models and effort").
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CliModel {
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub efforts: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_effort: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub is_default: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MachineInfo {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    pub name: String,
    #[serde(default)]
    pub labels: Vec<String>,
    pub daemon_version: String,
    pub os: String,
    pub arch: String,
    pub max_parallel_runs: u32,
    /// 1.1: see [`feature`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub features: Option<Vec<String>>,
    /// 1.1: screen sharing address (e.g. `vnc://<host>`); not the usable take-over (feature
    /// "takeover").
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub takeover_url: Option<String>,
    /// Feature "accounts": the machine's X25519 public key (base64, 32 bytes).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub public_key: Option<String>,
    /// Usable (required with feature "github"): where runs execute, "host" | "vm"
    /// ([`crate::github::MACHINE_RUNS_IN`]).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub runs_in: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CliInfo {
    /// Runtime name as configured on the machine; `run.assign.runtime` names one of these.
    pub runtime: String,
    /// "codex" | "claude" | "acp" | "pty"
    pub kind: String,
    pub command: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    pub available: bool,
    /// Models the CLI can run (feature "effort"); `None` = unknown.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub models: Option<Vec<CliModel>>,
    /// Efforts the CLI takes (feature "effort").
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub efforts: Option<Vec<String>>,
    /// Feature "gates": runs of this CLI stop at `run.assign.gates` matches.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gates: Option<bool>,
    /// Usable (feature "resume"): runs of this CLI continue `run.assign.resumeSessionId`;
    /// `None` = only kinds "claude" and "codex" do.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resume: Option<bool>,
    /// Usable (feature "aside"): this CLI has a read-only mode for side chats; `None` = only
    /// kinds "claude" and "codex" do.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub aside: Option<bool>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Health {
    pub disk_free_bytes: u64,
    pub disk_total_bytes: u64,
    pub load1: f64,
    pub load5: f64,
    pub load15: f64,
    pub cpu_count: u32,
    pub active_runs: u32,
}

/// `VmRunMetrics.phase` values (any short string is read).
pub mod vm_phase {
    pub const BOOTING: &str = "booting";
    pub const RUNNING: &str = "running";
    pub const PAUSED: &str = "paused";
}

/// `CliLogin.state` values (any short string is read).
pub mod login_state {
    pub const LOGGED_IN: &str = "logged_in";
    pub const LOGGED_OUT: &str = "logged_out";
    pub const UNKNOWN: &str = "unknown";
}

/// At most this many runs in `MachineMetrics.runs`.
pub const MAX_METRIC_RUNS: usize = 64;

/// One run's VM (or local process tree) as the daemon measured it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VmRunMetrics {
    pub run_id: String,
    /// [`vm_phase`]
    pub phase: String,
    pub started_at: u64,
    /// Start of the VM to a connected guest agent, once it booted.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub boot_ms: Option<u64>,
    /// CPU since the previous sample, in cores (1.0 = one thread busy).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cpu_cores: Option<f64>,
    /// Resident memory of the VM.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mem_bytes: Option<u64>,
    /// Blocks the run's scratch disk uses.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scratch_bytes: Option<u64>,
}

/// Whether a runtime's CLI has a credential on the machine; never the credential itself.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CliLogin {
    pub runtime: String,
    /// [`login_state`]
    pub state: String,
    /// "subscription" | "api_key" when known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub method: Option<String>,
    /// Feature "accounts": the signed-in account and plan, when the CLI says.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub email: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub plan: Option<String>,
}

/// An API key the machine holds: never the value, only its last 4 characters.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SecretInfo {
    pub name: String,
    pub last4: String,
    /// AI members whose runs get it; empty = every run.
    #[serde(default)]
    pub members: Vec<String>,
    pub updated_at: u64,
}

/// One usage window of a subscription.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageWindow {
    /// "session" | "week" | "week_model" | "primary" | "secondary" | another short word.
    pub kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    pub used_pct: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resets_at: Option<u64>,
}

/// A runtime's subscription usage as the daemon last read it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CliUsage {
    pub runtime: String,
    pub at: u64,
    #[serde(default)]
    pub windows: Vec<UsageWindow>,
}

/// `account.login.status.state` values.
pub mod login_phase {
    pub const STARTING: &str = "starting";
    pub const OPEN_URL: &str = "open_url";
    pub const DEVICE_CODE: &str = "device_code";
    pub const CHECKING: &str = "checking";
    pub const DONE: &str = "done";
    pub const FAILED: &str = "failed";
    pub const CANCELLED: &str = "cancelled";
}

/// The account a sign-in reached.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LoginAccount {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub email: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub plan: Option<String>,
}

/// Daemon to server (feature "accounts"): where a sign-in stands.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LoginStatus {
    pub login_id: String,
    pub runtime: String,
    /// [`login_phase`]
    pub state: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub user_code: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub account: Option<LoginAccount>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// VM boot times of the last (at most 50) runs.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BootStats {
    pub avg_ms: u64,
    pub max_ms: u64,
    pub samples: u32,
}

/// Host and VM numbers sent with each heartbeat (docs/machine-protocol.md, "Metrics"). Every
/// number is optional: a platform may not have it.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MachineMetrics {
    /// When the sample was taken (machine clock).
    pub at: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub uptime_secs: Option<u64>,
    /// Whole-host CPU busy since the previous sample, 0-100.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cpu_pct: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cpu_threads: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mem_total_bytes: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mem_available_bytes: Option<u64>,
    /// Bytes per second over the non-loopback, non-VM interfaces since the previous sample.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub net_rx_bps: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub net_tx_bps: Option<f64>,
    /// VM slots of the machine (`[vm]`), absent without VMs.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub vm_slots: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub boot: Option<BootStats>,
    #[serde(default)]
    pub runs: Vec<VmRunMetrics>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub logins: Option<Vec<CliLogin>>,
    /// Feature "accounts": the API keys the machine holds.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub secrets: Option<Vec<SecretInfo>>,
    /// Feature "accounts": subscription usage.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub usage: Option<Vec<CliUsage>>,
    /// Usable, feature "homes": each AI member's home folder (at most [`MAX_HOMES`]).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub homes: Option<Vec<HomeUse>>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RunCursor {
    pub run_id: String,
    /// Highest seq the daemon has produced for this run.
    pub last_seq: u64,
    /// True when the run has finished on the machine (run.done was produced).
    #[serde(default)]
    pub done: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct KnownRun {
    pub run_id: String,
    /// Highest seq the server has stored; the daemon replays every frame after it.
    pub acked_seq: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RepoSpec {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_branch: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionUpload {
    pub url: String,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub headers: BTreeMap<String, String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RunAssign {
    pub run_id: String,
    pub bot_member_id: String,
    pub group_id: String,
    pub topic_id: String,
    pub runtime: String,
    pub prompt: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    /// Thinking effort (feature "effort").
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub effort: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repo: Option<RepoSpec>,
    #[serde(default)]
    pub env_allowlist: Vec<String>,
    pub lease_ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_duration_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_upload: Option<SessionUpload>,
    /// 1.1: "aside" = a read-only side chat run (feature "aside").
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
    /// 1.1 (feature "resume"): the CLI session of the topic's last run on this machine.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resume_session_id: Option<String>,
    /// Feature "gates": the action kinds the run stops at for a person's approval.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gates: Option<Vec<RunGate>>,
    /// Usable, feature "github": whose GitHub the run uses and how.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub github: Option<crate::github::RunGithub>,
}

/// A Card v2 answer given to a run (`run.input.answer`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RunAnswer {
    pub message_id: String,
    pub rev: u32,
    pub action_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub inputs: Option<BTreeMap<String, Value>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub edited_values: Option<BTreeMap<String, String>>,
    pub actor_id: String,
    /// 1.1 interaction loop: the stored answer.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub answer_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ask_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub event_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub event: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub block_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value: Option<Value>,
}

/// Who made an answer or an event (a person).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RunActor {
    pub id: String,
    pub name: String,
}

/// `run.answer.source`: the stored answer of a card.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum RunAnswerSource {
    #[serde(rename = "card", rename_all = "camelCase")]
    Card {
        message_id: String,
        ask_id: String,
        answer_id: String,
        rev: u32,
    },
}

/// `run.answer` (feature "interact"): a stored Card v2 answer for the run's CLI.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RunAnswerFrame {
    pub run_id: String,
    pub input_id: String,
    pub source: RunAnswerSource,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub action_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub block_id: Option<String>,
    pub event: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub inputs: Option<BTreeMap<String, Value>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub edited_values: Option<BTreeMap<String, String>>,
    pub actor: RunActor,
    /// The interaction's event id, which `card.patch.cause` / `send.causeEventId` name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub event_id: Option<String>,
    /// The block the answer names, as it was at `source.rev` (the CLI has no copy of the card).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub block: Option<Value>,
    /// A poll that shows counts only: the votes per option now; `value`/`inputs` are then absent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tally: Option<BTreeMap<String, u64>>,
}

/// `run.interaction.source`: a live card event or a page event.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum RunInteractionSource {
    #[serde(rename = "card", rename_all = "camelCase")]
    Card {
        message_id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        block_id: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        action_id: Option<String>,
        rev: u32,
    },
    #[serde(rename = "artifact", rename_all = "camelCase")]
    Artifact {
        artifact_id: String,
        version: u32,
        event_id: String,
        ask: bool,
    },
}

/// `run.interaction` (feature "interact"): a live card event or a page event for the CLI.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RunInteractionFrame {
    pub run_id: String,
    pub input_id: String,
    pub source: RunInteractionSource,
    /// A card event name or the page event name.
    pub event: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value: Option<Value>,
    pub actor: RunActor,
    /// The interaction's event id, which `card.patch.cause` / `send.causeEventId` name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub event_id: Option<String>,
    /// Card events: the block (or the action) as it was at `source.rev`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub block: Option<Value>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RunOutputError {
    pub code: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    pub message: String,
}

/// `run.output.result` (feature "output"): the answer to a RunEvent `output`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RunOutputResult {
    pub run_id: String,
    pub output_id: String,
    pub ok: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<RunOutputError>,
}

/// What a `chat-run` output asks for; each one has the rules of the matching frame.
pub mod output_op {
    pub const CARD_POST: &str = "card.post";
    pub const CARD_PATCH: &str = "card.patch";
    pub const INTERACTION_RESOLVE: &str = "interaction.resolve";
    pub const ARTIFACT_DATA: &str = "artifact.data";
    pub const ARTIFACT_REPLY: &str = "artifact.reply";
    /// 1.1: the bot's desk note in the run's group, body `{ text }` (as `PUT /v1/desk-note`).
    pub const DESK_NOTE: &str = "desk.note";
    /// 转交: the whole piece of work goes to another AI member, body `{ member, goal, doneWhen?, notes? }`.
    pub const MEMBER_HANDOFF: &str = "member.handoff";
    /// 分出去: a part goes to another AI member in a new topic; the result comes back.
    pub const MEMBER_ASSIGN: &str = "member.assign";
    /// Usable: how the run ended in the CLI's own words, body `{ state, text }`
    /// ([`super::RUN_REPORT_STATES`]); the latest report counts when the run ends.
    pub const RUN_REPORT: &str = "run.report";
}

/// Known `run.report` states: done, blocked (needs something to go on), waiting (for a person).
pub const RUN_REPORT_STATES: &[&str] = &["done", "blocked", "waiting"];
/// Longest `run.report` text, in characters.
pub const MAX_RUN_REPORT_CHARS: usize = 2000;

/// An event from an interactive 产出 page given to a run (`run.input.artifactEvent`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RunArtifactEvent {
    pub artifact_id: String,
    pub version: u32,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub payload: Option<Value>,
    pub actor_id: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileChange {
    pub path: String,
    /// "add" | "modify" | "delete"
    pub kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub diff: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Usage {
    #[serde(default)]
    pub input_tokens: u64,
    #[serde(default)]
    pub output_tokens: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cost_usd: Option<f64>,
    /// Per model the run used; older daemons send the totals only.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub models: Vec<ModelUsage>,
}

/// What one model of a run used. `input_tokens` leaves out cached input: cache reads and writes
/// are their own counts. `cost_usd` is the CLI's own figure for this model, when it reports one.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelUsage {
    pub model: String,
    #[serde(default)]
    pub input_tokens: u64,
    #[serde(default)]
    pub output_tokens: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cache_read_tokens: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cache_write_tokens: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cost_usd: Option<f64>,
}

/// One normalized item of a run. Session, turn and item ids come from the CLI where it has
/// them; the daemon makes them (`t1`, `m1`, ...) where it does not.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum RunEvent {
    #[serde(rename = "session", rename_all = "camelCase")]
    Session {
        session_id: String,
        runtime: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        model: Option<String>,
    },
    #[serde(rename = "turn.started", rename_all = "camelCase")]
    TurnStarted { turn_id: String },
    #[serde(rename = "turn.completed", rename_all = "camelCase")]
    TurnCompleted {
        turn_id: String,
        /// "completed" | "failed" | "interrupted"
        status: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        error: Option<String>,
    },
    #[serde(rename = "message.delta", rename_all = "camelCase")]
    MessageDelta {
        item_id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        turn_id: Option<String>,
        text: String,
    },
    #[serde(rename = "message", rename_all = "camelCase")]
    Message {
        item_id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        turn_id: Option<String>,
        /// "assistant" | "user"
        role: String,
        text: String,
    },
    #[serde(rename = "reasoning", rename_all = "camelCase")]
    Reasoning {
        item_id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        turn_id: Option<String>,
        text: String,
    },
    #[serde(rename = "tool.call", rename_all = "camelCase")]
    ToolCall {
        item_id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        turn_id: Option<String>,
        tool: String,
        title: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        input: Option<Value>,
    },
    #[serde(rename = "tool.result", rename_all = "camelCase")]
    ToolResult {
        item_id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        turn_id: Option<String>,
        ok: bool,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        output: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        exit_code: Option<i64>,
    },
    #[serde(rename = "file.change", rename_all = "camelCase")]
    FileChange {
        item_id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        turn_id: Option<String>,
        changes: Vec<FileChange>,
    },
    #[serde(rename = "approval.request", rename_all = "camelCase")]
    ApprovalRequest {
        approval_id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        item_id: Option<String>,
        title: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        detail: Option<String>,
        /// Feature "gates": the gate kind the command matched.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        kind: Option<String>,
        /// Feature "gates": the command the CLI was about to run (redacted).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        command: Option<String>,
    },
    #[serde(rename = "approval.resolved", rename_all = "camelCase")]
    ApprovalResolved {
        approval_id: String,
        decision: String,
    },
    #[serde(rename = "input.accepted", rename_all = "camelCase")]
    InputAccepted { input_id: String },
    /// The CLI's to-do list (Claude Code TodoWrite, Codex plan updates); replaces the run plan.
    #[serde(rename = "plan", rename_all = "camelCase")]
    Plan {
        items: Vec<crate::bots::RunPlanItem>,
    },
    /// state: "preparing" | "running" | "retrying" | "stalled" | "warning" | "error"
    #[serde(rename = "status", rename_all = "camelCase")]
    Status {
        state: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        message: Option<String>,
    },
    /// 1.1: the run's acceptance criteria and their state; replaces `BotRun.acceptance`.
    #[serde(rename = "acceptance", rename_all = "camelCase")]
    Acceptance {
        items: Vec<crate::bots::AcceptanceItem>,
    },
    /// 1.1: the CLI is stopped (`paused: true`, after run.pause) or goes on (after run.resume).
    #[serde(rename = "paused", rename_all = "camelCase")]
    Paused {
        paused: bool,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        by: Option<String>,
    },
    /// 1.1 (feature "output", `chat-run`): post or patch a card, resolve an event, or push
    /// data to / reply from a 产出. `op` is one of [`output_op`].
    #[serde(rename = "output", rename_all = "camelCase")]
    Output {
        output_id: String,
        op: String,
        body: Value,
    },
    /// Usable (feature "github"): git or gh used the person's GitHub ("used"), or could not
    /// ("missing", "invalid"; [`crate::github::RUN_GITHUB_STATES`]).
    #[serde(rename = "github", rename_all = "camelCase")]
    Github {
        state: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        login: Option<String>,
    },
    /// Usable (feature "takeover"): a command of the person holding the run started.
    #[serde(rename = "hand.command", rename_all = "camelCase")]
    HandCommand {
        command_id: String,
        command: String,
        by: String,
    },
    /// Usable (feature "takeover"): output of a person's command (<= 16 KiB per event).
    #[serde(rename = "hand.output", rename_all = "camelCase")]
    HandOutput { command_id: String, text: String },
    /// Usable (feature "takeover"): a person's command ended, or was refused (`error`).
    #[serde(rename = "hand.result", rename_all = "camelCase")]
    HandResult {
        command_id: String,
        ok: bool,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        exit_code: Option<i64>,
        ms: u64,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        timed_out: Option<bool>,
        /// "not_held" | "busy" | "unsupported" ([`crate::takeover::HAND_ERRORS`]).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        error: Option<String>,
    },
    /// Usable (feature "takeover"): what the person did while holding the run, at hand-back.
    #[serde(rename = "hand.summary")]
    HandSummary(crate::takeover::RunHandback),
    /// A type this reader does not know (a newer daemon). Readers skip it; never written.
    #[serde(other)]
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RunDone {
    pub run_id: String,
    pub seq: u64,
    pub at: u64,
    /// "succeeded" | "failed" | "cancelled" | "timeout"
    pub status: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exit_code: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result_text: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub usage: Option<Usage>,
    pub duration_ms: u64,
    /// Usable: the run's branch, its head and whether it is pushed; `None` = not known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub code: Option<crate::relay::RunCode>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RunSession {
    pub run_id: String,
    pub seq: u64,
    pub at: u64,
    pub ok: bool,
    /// "cli" (the CLI's own session file) | "stream" (the daemon's copy of the CLI output)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bytes: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gzip_bytes: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sha256: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MachineErrorBody {
    pub code: String,
    pub message: String,
}

/// One stored frame of a run, as `GET /v1/runs/:id/events` returns it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "t")]
pub enum RunLogEntry {
    #[serde(rename = "run.event", rename_all = "camelCase")]
    RunEvent {
        run_id: String,
        seq: u64,
        at: u64,
        event: RunEvent,
    },
    #[serde(rename = "run.done")]
    RunDone(RunDone),
    #[serde(rename = "run.session")]
    RunSession(RunSession),
}

/// Daemon to server.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "t")]
pub enum Up {
    #[serde(rename = "hello", rename_all = "camelCase")]
    Hello {
        protocol: ProtocolV1,
        machine: MachineInfo,
        clis: Vec<CliInfo>,
        health: Health,
        runs: Vec<RunCursor>,
    },
    #[serde(rename = "heartbeat", rename_all = "camelCase")]
    Heartbeat {
        at: u64,
        health: Health,
        runs: Vec<RunCursor>,
        /// Host and VM metrics (daemons since 1.2.7); old servers ignore it.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        metrics: Option<MachineMetrics>,
    },
    #[serde(rename = "run.accept", rename_all = "camelCase")]
    RunAccept { run_id: String },
    #[serde(rename = "run.reject", rename_all = "camelCase")]
    RunReject {
        run_id: String,
        /// "busy" | "runtime_unavailable" | "disk_low" | "invalid" | "draining"
        reason: String,
        message: String,
    },
    #[serde(rename = "run.event", rename_all = "camelCase")]
    RunEvent {
        run_id: String,
        seq: u64,
        at: u64,
        event: RunEvent,
    },
    #[serde(rename = "run.done")]
    RunDone(RunDone),
    #[serde(rename = "run.session")]
    RunSession(RunSession),
    /// Feature "accounts": where a sign-in stands.
    #[serde(rename = "account.login.status")]
    AccountLoginStatus(LoginStatus),
}

/// Server to daemon.
#[allow(clippy::large_enum_variant)] // frames are short-lived; boxing would change the public API
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "t")]
pub enum Down {
    #[serde(rename = "welcome", rename_all = "camelCase")]
    Welcome {
        protocol: ProtocolV1,
        machine_id: String,
        server_time: u64,
        runs: Vec<KnownRun>,
    },
    #[serde(rename = "heartbeat.ack", rename_all = "camelCase")]
    HeartbeatAck { at: u64 },
    #[serde(rename = "run.assign")]
    RunAssign(RunAssign),
    #[serde(rename = "run.input", rename_all = "camelCase")]
    RunInput {
        run_id: String,
        input_id: String,
        text: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        author_id: Option<String>,
        /// 1.1 (feature "answer").
        #[serde(default, skip_serializing_if = "Option::is_none")]
        answer: Option<RunAnswer>,
        /// 1.1 (feature "answer").
        #[serde(default, skip_serializing_if = "Option::is_none")]
        artifact_event: Option<RunArtifactEvent>,
    },
    #[serde(rename = "run.approval", rename_all = "camelCase")]
    RunApproval {
        run_id: String,
        approval_id: String,
        /// "approve" | "deny"
        decision: String,
        /// Feature "gates": "run" = 这件事里都同意 ([`APPROVAL_SCOPE_RUN`]).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        scope: Option<String>,
    },
    #[serde(rename = "run.cancel", rename_all = "camelCase")]
    RunCancel {
        run_id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        reason: Option<String>,
    },
    /// 1.1 (feature "pause"): stop the CLI's process group for 接管; deadlines stop too.
    #[serde(rename = "run.pause", rename_all = "camelCase")]
    RunPause {
        run_id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        by: Option<String>,
        /// Usable, feature "takeover": "takeover" = a person takes over
        /// ([`crate::takeover::HOLD_TAKEOVER`]); `None` = a pause.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        hold: Option<String>,
    },
    /// 1.1 (feature "pause"): go on; `note`, when set, is the CLI's next input.
    #[serde(rename = "run.resume", rename_all = "camelCase")]
    RunResume {
        run_id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        note: Option<String>,
    },
    #[serde(rename = "ack", rename_all = "camelCase")]
    Ack { run_id: String, seq: u64 },
    #[serde(rename = "error", rename_all = "camelCase")]
    Error { error: MachineErrorBody },
    /// 1.1 (feature "interact").
    #[serde(rename = "run.answer")]
    RunAnswer(RunAnswerFrame),
    /// 1.1 (feature "interact").
    #[serde(rename = "run.interaction")]
    RunInteraction(RunInteractionFrame),
    /// 1.1 (feature "output").
    #[serde(rename = "run.output.result")]
    RunOutputResult(RunOutputResult),
    /// Feature "accounts": start, finish (`code`) or cancel a CLI sign-in.
    #[serde(rename = "account.login", rename_all = "camelCase")]
    AccountLogin {
        login_id: String,
        runtime: String,
        /// "start" | "code" | "cancel"
        op: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        code: Option<String>,
    },
    /// Feature "accounts": keep an API key, sealed to the machine's public key.
    #[serde(rename = "secret.set", rename_all = "camelCase")]
    SecretSet {
        name: String,
        sealed: String,
        #[serde(default)]
        members: Vec<String>,
    },
    /// Feature "accounts": forget an API key.
    #[serde(rename = "secret.delete", rename_all = "camelCase")]
    SecretDelete { name: String },
    /// Usable, feature "homes": empty one AI member's home folder. Sent only while connected;
    /// the daemon refuses while that member has a run on the machine.
    #[serde(rename = "home.clear", rename_all = "camelCase")]
    HomeClear { member_id: String },
    /// Usable, feature "takeover": run one command of the person holding the run in its work
    /// folder. Sent only while the link is up; never queued.
    #[serde(rename = "run.command", rename_all = "camelCase")]
    RunCommand {
        run_id: String,
        command_id: String,
        command: String,
        by: String,
    },
    /// Usable, feature "takeover": stop that command.
    #[serde(rename = "run.command.cancel", rename_all = "camelCase")]
    RunCommandCancel { run_id: String, command_id: String },
}

impl Up {
    /// Sequence number of a run-scoped sequenced frame.
    pub fn seq(&self) -> Option<u64> {
        match self {
            Up::RunEvent { seq, .. } => Some(*seq),
            Up::RunDone(d) => Some(d.seq),
            Up::RunSession(s) => Some(s.seq),
            _ => None,
        }
    }
}
