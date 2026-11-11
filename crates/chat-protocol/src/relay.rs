//! Usable: several AI members in one topic, chains of hand-overs and relays (按顺序做)
//! (docs/protocol.md, "Participants, chains and relays"). States and reasons are open strings:
//! a reader shows an unknown state as stopped and an unknown reason as none.

use serde::{Deserialize, Serialize};

use crate::entities::Millis;
use crate::serde_util::non_null;

/// Max AI members `Topic.participants` lists.
pub const MAX_TOPIC_PARTICIPANTS: usize = 50;
pub const MIN_RELAY_STEPS: usize = 2;
pub const MAX_RELAY_STEPS: usize = 6;

/// Known `TopicRelay.state` values.
pub const RELAY_STATES: [&str; 4] = ["running", "paused", "done", "stopped"];
/// Known `TopicRelayStep.state` values.
pub const RELAY_STEP_STATES: [&str; 6] =
    ["pending", "queued", "working", "done", "failed", "stopped"];
/// Known `TopicRelay.reason` values.
pub const RELAY_REASONS: [&str; 8] = [
    "member_unavailable",
    "step_failed",
    "step_blocked",
    "step_cancelled",
    "limit",
    "undelivered",
    "not_pushed",
    "requester_left",
];

/// One AI member that ran in a topic.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TopicParticipant {
    pub member_id: String,
    /// It has a run in the topic that has not ended.
    pub working: bool,
    /// When its latest run in the topic started.
    pub last_run_at: Millis,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TopicRelayStep {
    pub member_id: String,
    #[serde(
        default,
        deserialize_with = "non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub note: Option<String>,
    /// One of [`RELAY_STEP_STATES`], or a newer value.
    pub state: String,
    #[serde(
        default,
        deserialize_with = "non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub run_id: Option<String>,
}

/// A relay (按顺序做) a person started in a topic.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TopicRelay {
    pub id: String,
    pub started_by: String,
    pub goal: String,
    /// Index of the current step, 0-based.
    pub current: u32,
    /// One of [`RELAY_STATES`], or a newer value.
    pub state: String,
    /// One of [`RELAY_REASONS`], or a newer value.
    #[serde(
        default,
        deserialize_with = "non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub reason: Option<String>,
    #[serde(
        default,
        deserialize_with = "non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub stopped_by: Option<String>,
    pub updated_at: Millis,
    pub steps: Vec<TopicRelayStep>,
}

/// Where a run left its code (`run.done.code`, `BotRun.code`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RunCode {
    pub branch: String,
    /// The commit the branch points at (hex).
    pub head: String,
    /// Every commit of the branch is on its remote.
    pub pushed: bool,
}

/// `POST /v1/groups/:id/chains/:chainId/stop`
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChainStopResponse {
    pub stopped_runs: u64,
}
