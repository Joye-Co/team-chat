//! The owner's machine monitor (docs/protocol.md, "Machine monitor"):
//! `GET /v1/machines/:id/monitor?range=15m|1h|24h&dayStart=<ms>` answers one [`MachineMonitor`].
//! Enum-like strings (`state`, `login`, `range`) are kept as strings so a newer server's value
//! never fails the whole answer; clients map unknown values to a neutral default.

use serde::{Deserialize, Serialize};

use crate::machine::{BootStats, HomeUse, LoginAccount, SecretInfo, UsageWindow};

/// `MonitorRange` values.
pub mod range {
    pub const M15: &str = "15m";
    pub const H1: &str = "1h";
    pub const H24: &str = "24h";
}

/// `MonitorRun.state` values.
pub mod run_state {
    pub const RUNNING: &str = "running";
    pub const WAITING: &str = "waiting";
    pub const BOOTING: &str = "booting";
    pub const PAUSED: &str = "paused";
    pub const QUEUED: &str = "queued";
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MonitorSeries {
    pub range: String,
    pub from: u64,
    pub step_ms: u64,
    pub cpu_pct: Vec<Option<f64>>,
    pub mem_pct: Vec<Option<f64>>,
    pub disk_pct: Vec<Option<f64>>,
    pub net_rx_bps: Vec<Option<f64>>,
    pub net_tx_bps: Vec<Option<f64>>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MonitorHost {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cpu_pct: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cpu_threads: Option<u32>,
    pub load1: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mem_total_bytes: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mem_available_bytes: Option<u64>,
    pub mem_vm_bytes: u64,
    pub disk_total_bytes: u64,
    pub disk_free_bytes: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub net_rx_bps: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub net_tx_bps: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub uptime_secs: Option<u64>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MonitorCapacity {
    pub slots: u32,
    pub running: u32,
    pub can_take: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub avg_vm_bytes: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reserve_bytes: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MonitorRun {
    pub run_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub member_id: Option<String>,
    /// [`run_state`]
    pub state: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub started_at: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub boot_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cpu_cores: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mem_bytes: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scratch_bytes: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MonitorMember {
    pub member_id: String,
    pub running: u32,
    pub waiting: u32,
    pub queued: u32,
    pub cpu_cores: f64,
    pub mem_bytes: u64,
    pub tokens_today: u64,
    pub finished_today: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MonitorCli {
    pub runtime: String,
    pub kind: String,
    pub available: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    /// [`crate::machine::login_state`]
    pub login: String,
    pub failed_today: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub method: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub email: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub plan: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub usage: Option<Vec<UsageWindow>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub usage_at: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MonitorFailure {
    pub run_id: String,
    pub member_id: String,
    pub at: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MonitorMachine {
    pub id: String,
    pub name: String,
    pub connected: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub revoked: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_heartbeat_at: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub connected_at: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub daemon_version: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub os: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub arch: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MonitorQueue {
    pub count: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub oldest_at: Option<u64>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MonitorToday {
    pub finished: u32,
    pub succeeded: u32,
    pub failed: u32,
    pub finished_yesterday: u32,
    pub tokens: u64,
}

/// The monitor of one machine.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MachineMonitor {
    pub machine: MonitorMachine,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sampled_at: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub host: Option<MonitorHost>,
    pub series: MonitorSeries,
    pub capacity: MonitorCapacity,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub boot: Option<BootStats>,
    pub queue: MonitorQueue,
    pub today: MonitorToday,
    pub clis: Vec<MonitorCli>,
    pub runs: Vec<MonitorRun>,
    pub members: Vec<MonitorMember>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_failure: Option<MonitorFailure>,
    /// Usable, feature "homes": each AI member's home folder on a VM host, from the latest
    /// heartbeat; absent on other machines.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub homes: Option<Vec<HomeUse>>,
}

/// `POST /v1/machines/:id/logins`
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StartLoginRequest {
    pub runtime: String,
}

/// `POST /v1/machines/:id/logins/:loginId/code`
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LoginCodeRequest {
    pub code: String,
}

/// A sign-in in progress on a machine (`GET /v1/machines/:id/logins/:loginId`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MachineLogin {
    pub login_id: String,
    pub runtime: String,
    /// [`crate::machine::login_phase`]
    pub state: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub user_code: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub account: Option<LoginAccount>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    pub updated_at: u64,
}

/// `GET /v1/machines/:id/secrets`
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MachineSecretsResponse {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub public_key: Option<String>,
    #[serde(default)]
    pub secrets: Vec<SecretInfo>,
}

/// `PUT /v1/machines/:id/secrets/:name`
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SetSecretRequest {
    pub sealed: String,
    #[serde(default)]
    pub members: Vec<String>,
}
