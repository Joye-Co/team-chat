//! 成员 of 团队管理 (docs/protocol.md "Team roster"): `GET /v1/team/roster` answers [`TeamRoster`].

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RosterDevice {
    pub label: String,
    pub created_at: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_used_at: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RosterMember {
    pub member_id: String,
    pub joined_at: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub added_by: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_seen_at: Option<u64>,
    #[serde(default)]
    pub devices: Vec<RosterDevice>,
    /// Usable: `Some(true)` for a 所有者; sent only when true.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub team_owner: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PendingInvite {
    pub id: String,
    pub name: String,
    pub kind: String,
    pub level: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub role: Option<String>,
    pub created_by: String,
    pub created_at: u64,
    pub expires_at: u64,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TeamRoster {
    pub members: Vec<RosterMember>,
    pub invites: Vec<PendingInvite>,
}
