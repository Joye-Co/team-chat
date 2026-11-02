//! Machine protocol (docs/machine-protocol.md): frames between the machine daemon and the
//! server's MachineLink, and the normalized run events that clients read in the run log.
//! JSON, camelCase, frames tagged on `t`, run events on `type`. Unknown fields are ignored.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;

use crate::serde_util::ProtocolV1;

pub const MACHINE_PROTOCOL: u32 = 1;

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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repo: Option<RepoSpec>,
    #[serde(default)]
    pub env_allowlist: Vec<String>,
    pub lease_ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_duration_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_upload: Option<SessionUpload>,
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
    },
    #[serde(rename = "approval.resolved", rename_all = "camelCase")]
    ApprovalResolved {
        approval_id: String,
        decision: String,
    },
    #[serde(rename = "input.accepted", rename_all = "camelCase")]
    InputAccepted { input_id: String },
    /// state: "preparing" | "running" | "retrying" | "stalled" | "warning" | "error"
    #[serde(rename = "status", rename_all = "camelCase")]
    Status {
        state: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        message: Option<String>,
    },
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
}

/// Server to daemon.
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
    },
    #[serde(rename = "run.approval", rename_all = "camelCase")]
    RunApproval {
        run_id: String,
        approval_id: String,
        /// "approve" | "deny"
        decision: String,
    },
    #[serde(rename = "run.cancel", rename_all = "camelCase")]
    RunCancel {
        run_id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        reason: Option<String>,
    },
    #[serde(rename = "ack", rename_all = "camelCase")]
    Ack { run_id: String, seq: u64 },
    #[serde(rename = "error", rename_all = "camelCase")]
    Error { error: MachineErrorBody },
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
