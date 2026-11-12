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
    /// The part of `cost_usd` estimated from tokens at list price (absent: all reported).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub estimated_usd: Option<f64>,
    /// Tokens of models with no known price, left out of `cost_usd` ("价格未知").
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unpriced_tokens: Option<u64>,
}

/// One model's spend: its id, its name from the price list and the CLIs it ran in.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelSpend {
    pub model: String,
    pub name: String,
    #[serde(default)]
    pub runtimes: Vec<String>,
    #[serde(flatten)]
    pub spend: Spend,
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
    /// Runs held at its weekly budget, waiting for an owner.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub held_runs: Option<u32>,
    /// The models this AI member used, by cost.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub models: Vec<String>,
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
    /// By model (absent from older servers).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub models: Vec<ModelSpend>,
    /// Who asked which AI member (absent from older servers).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub pairs: Vec<PairSpend>,
    pub runtimes: Vec<RuntimeSpend>,
    pub daily: Vec<UsageDay>,
    /// Usable: the AI 服务's own calls in the span (absent from older servers). Not in `total`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub service: Option<ServiceUsage>,
}

/// Usable (模 1.9): the server's own model calls (front desk, titles, summaries, review, tidy).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ServiceUsage {
    pub calls: u32,
    /// Calls that failed (they used no tokens).
    pub failures: u32,
    pub cost_usd: f64,
    pub input_tokens: u64,
    pub output_tokens: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cache_read_tokens: Option<u64>,
    /// Tokens of a model with no known price: not in `cost_usd`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unpriced_tokens: Option<u64>,
    /// By use, most tokens first.
    #[serde(default)]
    pub uses: Vec<ServiceUseSpend>,
}

/// One use's share of [`ServiceUsage`].
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ServiceUseSpend {
    #[serde(rename = "use")]
    pub use_: String,
    pub calls: u32,
    pub input_tokens: u64,
    pub output_tokens: u64,
}
