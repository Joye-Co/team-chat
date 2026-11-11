//! Usable: each person's GitHub sign-in (docs/protocol.md, "GitHub sign-in"; machine side in
//! docs/machine-protocol.md, "GitHub (feature `github`)"). States and reasons are open strings.

use serde::{Deserialize, Serialize};

use crate::entities::Millis;
use crate::limits::{Validation, check_chars};
use crate::serde_util::non_null;

/// `hello.features` entry that turns on the `github.status` transient for a connection.
pub const FEATURE_GITHUB: &str = "github/1";
/// Known `GithubAccount.status` values; an unknown value reads as "invalid".
pub const GITHUB_ACCOUNT_STATUSES: [&str; 2] = ["ok", "invalid"];
/// Known `GithubStatusResponse.unavailable_reason` values.
pub const GITHUB_UNAVAILABLE_REASONS: [&str; 2] = ["no_client_id", "no_secret_key"];
/// Known wait reasons (`GithubWait.reason`, `MachineGithubWaitRequest.reason`).
pub const GITHUB_WAIT_REASONS: [&str; 2] = ["not_connected", "invalid"];
/// Known `GithubPollResponse.state` values.
pub const GITHUB_POLL_STATES: [&str; 6] =
    ["pending", "slow_down", "done", "expired", "denied", "taken"];
/// Known `hello.machine.runsIn` values.
pub const MACHINE_RUNS_IN: [&str; 2] = ["host", "vm"];
/// Known `RunGithub.source` values.
pub const RUN_GITHUB_SOURCES: [&str; 2] = ["app", "machine"];
/// Known states of the `github` run event.
pub const RUN_GITHUB_STATES: [&str; 3] = ["used", "missing", "invalid"];

/// A person's connected GitHub account. Never the token.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GithubAccount {
    pub login: String,
    #[serde(
        default,
        deserialize_with = "non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub name: Option<String>,
    /// "ok" | "invalid", or a newer value (read as "invalid").
    pub status: String,
    pub connected_at: Millis,
}

/// A run that ended waiting for this person to connect GitHub.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GithubWait {
    pub run_id: String,
    pub group_id: String,
    pub topic_id: String,
    pub bot_id: String,
    /// "not_connected" | "invalid"
    pub reason: String,
}

/// `GET /v1/me/github`
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GithubStatusResponse {
    pub available: bool,
    #[serde(
        default,
        deserialize_with = "non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub unavailable_reason: Option<String>,
    #[serde(
        default,
        deserialize_with = "non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub account: Option<GithubAccount>,
    pub waits: Vec<GithubWait>,
}

/// `POST /v1/me/github/device`
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GithubDeviceResponse {
    pub device_id: String,
    pub user_code: String,
    pub verification_uri: String,
    /// Seconds between polls.
    pub interval: u32,
    pub expires_at: Millis,
}

/// `POST /v1/me/github/device/:id/poll`
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GithubPollResponse {
    /// One of [`GITHUB_POLL_STATES`], or a newer value.
    pub state: String,
    #[serde(
        default,
        deserialize_with = "non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub account: Option<GithubAccount>,
}

/// Transient `github.status`: only to the person's own connections with feature `github/1`.
/// `account` absent = disconnected.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GithubStatusTransient {
    #[serde(
        default,
        deserialize_with = "non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub account: Option<GithubAccount>,
}

/// A git author or committer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GitIdentity {
    pub name: String,
    pub email: String,
}

/// `run.assign.github` (feature `github`): whose GitHub the run uses and how.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RunGithub {
    /// "app" | "machine", or a newer value (a daemon then uses "machine").
    pub source: String,
    /// The person whose GitHub the run acts as.
    pub member_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub login: Option<String>,
    /// Commit author; `None` = the machine's own git config.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub author: Option<GitIdentity>,
    /// Commit committer: the AI member.
    pub committer: GitIdentity,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rewrite_ssh: Option<bool>,
}

/// `GET /v1/machine/runs/:id/github` (machine token). `Debug` hides the token.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MachineGithubTokenResponse {
    pub token: String,
    pub login: String,
}

impl std::fmt::Debug for MachineGithubTokenResponse {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MachineGithubTokenResponse")
            .field("token", &"<redacted>")
            .field("login", &self.login)
            .finish()
    }
}

/// `POST /v1/machine/runs/:id/github/wait` (machine token).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MachineGithubWaitRequest {
    /// "not_connected" | "invalid"
    pub reason: String,
}

impl MachineGithubWaitRequest {
    pub fn validate(&self) -> Validation {
        check_chars("reason", &self.reason, 1, 32)
    }
}
