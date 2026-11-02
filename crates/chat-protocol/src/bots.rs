//! Bot runs for clients (docs/protocol.md, "Bot runs"): runner-backed bot configuration,
//! machines and their health, the `run.updated` group event and the run log.

use serde::{Deserialize, Serialize};

use crate::entities::{Millis, Seq};
use crate::limits::{Validation, ValidationError, check_name};
use crate::machine::{CliInfo, Health, RunLogEntry};
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

/// One run of a runner-backed bot in a topic.
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
