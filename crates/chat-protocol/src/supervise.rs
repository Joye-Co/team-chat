//! Usable: what the desk's supervision (盯活) did to a run (`BotRun.supervision`) and the team's
//! time limit for one run (docs/protocol.md, "Supervision (盯活)" and "Run time limit"). Actions
//! and reasons are open strings, so a newer server never fails an older reader.

use serde::{Deserialize, Serialize};

use crate::limits::{Validation, ValidationError};

/// Known [`SuperviseAct::action`] values.
pub const SUPERVISE_ACTIONS: [&str; 11] = [
    "nudge",
    "loop",
    "continue",
    "retry",
    "move",
    "wait_machine",
    "machine_back",
    "limit",
    "limit_continue",
    "limit_stop",
    "gave_up",
];
/// Known [`SuperviseAct::reason`] values.
pub const SUPERVISE_REASONS: [&str; 9] = [
    "silent",
    "same_step",
    "promise",
    "machine_away",
    "machine_removed",
    "machine_lost",
    "daemon_restarted",
    "time_limit",
    "no_progress",
];
/// At most this many acts are kept per run (the latest).
pub const MAX_SUPERVISE_ACTS: usize = 20;

/// One act of the desk's supervision on a run, with its reason. Server-made.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SuperviseAct {
    pub at: u64,
    /// [`SUPERVISE_ACTIONS`] or a newer value.
    pub action: String,
    /// [`SUPERVISE_REASONS`] or a newer value.
    pub reason: String,
    /// How long the reason lasted, in whole minutes, when it has one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub minutes: Option<u64>,
}

/// Bounds of [`RunSettings::limit_minutes`].
pub const RUN_LIMIT_MIN_MINUTES: u32 = 30;
pub const RUN_LIMIT_MAX_MINUTES: u32 = 24 * 60;
pub const DEFAULT_RUN_LIMIT_MINUTES: u32 = 120;

/// `GET /v1/settings/runs` (member): how long an AI member works on one run before it saves its
/// state, reports and goes on in a new turn of the same session.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RunSettings {
    pub limit_minutes: u32,
    /// 0 = never changed (the default).
    pub updated_at: u64,
}

/// `PATCH /v1/settings/runs` (admin): only the given fields change.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateRunSettingsRequest {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit_minutes: Option<u32>,
}

impl UpdateRunSettingsRequest {
    pub fn validate(&self) -> Validation {
        match self.limit_minutes {
            Some(m) if !(RUN_LIMIT_MIN_MINUTES..=RUN_LIMIT_MAX_MINUTES).contains(&m) => {
                Err(ValidationError {
                    field: "limitMinutes",
                    message: format!("{RUN_LIMIT_MIN_MINUTES}..{RUN_LIMIT_MAX_MINUTES}"),
                })
            }
            _ => Ok(()),
        }
    }
}
