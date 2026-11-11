//! Usable: one state a person sees per run (`BotRun.phase`), pause versus take-over (接管) and
//! a person's commands in the run's work folder (docs/protocol.md, "Run phase and take-over";
//! machine side in docs/machine-protocol.md, "Take-over (feature `takeover`)"). Every value is
//! an open string, so a newer server never fails an older reader.

use serde::{Deserialize, Serialize};

use crate::limits::{Validation, ValidationError, check_chars};
use crate::serde_util::non_null;

/// Known `BotRun.phase` values. Another value (or none): work it out from `status` and
/// `paused_by` as before.
pub const RUN_PHASES: [&str; 10] = [
    "working",
    "waiting_person",
    "needs_reply",
    "waiting_approval",
    "waiting_computer",
    "paused",
    "taken_over",
    "done",
    "failed",
    "stopped",
];
/// Known `BotRun.waiting_for` values.
pub const RUN_WAITING_FOR: [&str; 6] =
    ["question", "stuck", "report", "queue", "offline", "booting"];
/// Known `BotRun.hold_kind` and `run.pause.hold` values.
pub const RUN_HOLD_KINDS: [&str; 2] = ["pause", "takeover"];
/// `run.pause.hold` and `BotRun.hold_kind` of a take-over.
pub const HOLD_TAKEOVER: &str = "takeover";
/// Known `BotRun.takeover` values.
pub const RUN_TAKEOVER: [&str; 3] = ["commands", "isolated", "none"];
/// Known `hand.result.error` values.
pub const HAND_ERRORS: [&str; 3] = ["not_held", "busy", "unsupported"];

pub const MAX_HAND_COMMAND_CHARS: usize = 4000;
/// Max UTF-8 bytes of one `hand.output` event.
pub const MAX_HAND_OUTPUT_BYTES: usize = 16_384;
pub const MAX_HANDBACK_FILES: usize = 20;

/// What a person did while holding a run (`BotRun.last_handback`, run event `hand.summary`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RunHandback {
    pub by: String,
    pub held_ms: u64,
    pub commands: u64,
    /// Changed paths in the work folder, at most 20.
    pub files: Vec<String>,
    #[serde(
        default,
        deserialize_with = "non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub head_from: Option<String>,
    #[serde(
        default,
        deserialize_with = "non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub head_to: Option<String>,
}

/// `[A-Za-z0-9_-]{1,64}`: a person's command in a run.
pub fn is_hand_command_id(id: &str) -> bool {
    (1..=64).contains(&id.len())
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}

/// `POST /v1/runs/:id/commands` (the person holding the run).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RunCommandRequest {
    pub command: String,
}

impl RunCommandRequest {
    pub fn validate(&self) -> Validation {
        check_chars("command", &self.command, 1, MAX_HAND_COMMAND_CHARS)
    }
}

/// Answer of `POST /v1/runs/:id/commands`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RunCommandResponse {
    pub command_id: String,
}

impl RunCommandResponse {
    pub fn validate(&self) -> Validation {
        if is_hand_command_id(&self.command_id) {
            Ok(())
        } else {
            Err(ValidationError {
                field: "commandId",
                message: "1..64 of [A-Za-z0-9_-]".into(),
            })
        }
    }
}
