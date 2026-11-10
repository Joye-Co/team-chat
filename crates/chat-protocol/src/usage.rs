//! 用量 of 团队管理 (docs/protocol.md "Team usage"): `GET /v1/usage?from=&to=` answers
//! [`TeamUsage`].

use serde::{Deserialize, Serialize};

/// Cost and tokens of a set of runs.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Spend {
    pub cost_usd: f64,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub runs: u32,
    pub topics: u32,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PreviousSpend {
    pub cost_usd: f64,
    pub topics: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BotSpend {
    pub member_id: String,
    #[serde(flatten)]
    pub spend: Spend,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub budget_cents: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MemberSpend {
    pub member_id: String,
    #[serde(flatten)]
    pub spend: Spend,
}

/// What one person asked of one AI member.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PairSpend {
    pub initiator_id: String,
    pub member_id: String,
    #[serde(flatten)]
    pub spend: Spend,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MachineSpend {
    pub machine_id: String,
    #[serde(flatten)]
    pub spend: Spend,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeSpend {
    pub runtime: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub machine_id: Option<String>,
    #[serde(flatten)]
    pub spend: Spend,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DayBot {
    pub member_id: String,
    pub cost_usd: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageDay {
    pub start: u64,
    pub cost_usd: f64,
    pub bots: Vec<DayBot>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TeamUsage {
    pub from: u64,
    pub to: u64,
    pub total: Spend,
    pub previous: PreviousSpend,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub team_budget_cents: Option<u64>,
    pub bots: Vec<BotSpend>,
    pub initiators: Vec<MemberSpend>,
    pub machines: Vec<MachineSpend>,
    /// Who asked which AI member (absent from older servers).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub pairs: Vec<PairSpend>,
    pub runtimes: Vec<RuntimeSpend>,
    pub daily: Vec<UsageDay>,
}
