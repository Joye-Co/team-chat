//! 活动记录 of 团队管理 (docs/protocol.md "Activity"): `GET /v1/activity` answers
//! [`ActivityResponse`]. `kind` stays a string so a newer server's kind never fails the answer.

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

/// `ActivityEntry.kind` values.
pub mod kind {
    pub const MEMBER: &str = "member";
    pub const LEVEL: &str = "level";
    pub const INVITE: &str = "invite";
    pub const KEY: &str = "key";
    pub const APPROVAL: &str = "approval";
    pub const BUDGET: &str = "budget";
    pub const LOGIN: &str = "login";
    pub const MACHINE: &str = "machine";
    /// A task that did not finish (`run.failed`).
    pub const RUN: &str = "run";
    pub const ALL: [&str; 9] = [
        MEMBER, LEVEL, INVITE, KEY, APPROVAL, BUDGET, LOGIN, MACHINE, RUN,
    ];
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ActivityEntry {
    pub id: u64,
    pub at: u64,
    pub kind: String,
    pub action: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub actor_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub machine_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub details: Option<Map<String, Value>>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ActivityResponse {
    pub entries: Vec<ActivityEntry>,
    pub has_more: bool,
}
