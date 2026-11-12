//! Each person's GitHub sign-in (docs/plans/usable/design-github-identity.zh.md §3.7): what
//! the 账号 page, the connect sheet, the 话题 banner and the 机器 hint read.

use super::{GroupId, MemberId, Millis, TopicId};

/// `GET /v1/me/github`, as the views read it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct GithubState {
    /// The server can connect GitHub (a client id and its secret key are set).
    pub available: bool,
    /// Why not, when `available` is false (`no_client_id`, `no_secret_key`, or newer).
    pub unavailable_reason: Option<String>,
    /// `None` = not connected.
    pub account: Option<GithubAccount>,
    /// Runs that ended waiting for me to connect GitHub.
    pub waits: Vec<GithubWait>,
}

impl GithubState {
    /// The newest wait of a 话题: the banner names its AI member. The server lists one
    /// row per run; a 话题 shows one banner.
    pub fn wait_in(&self, group_id: &str, topic_id: &str) -> Option<&GithubWait> {
        self.waits
            .iter()
            .rfind(|w| w.group_id == group_id && w.topic_id == topic_id)
    }

    /// Connected with a token GitHub accepts.
    pub fn connected(&self) -> bool {
        self.account.as_ref().is_some_and(|a| a.ok)
    }
}

/// A connected GitHub account. Never the token.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GithubAccount {
    pub login: String,
    /// The GitHub profile name; `None` = use the login.
    pub name: Option<String>,
    /// GitHub accepts the token. An `invalid` status (or a newer one) reads as false: 需要重新连接.
    pub ok: bool,
    pub connected_at: Millis,
}

/// A run that waits for me to connect GitHub.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GithubWait {
    pub run_id: String,
    pub group_id: GroupId,
    pub topic_id: TopicId,
    /// The AI member that waits.
    pub bot_id: MemberId,
    /// My connection was there but GitHub refused it (reason `invalid`): 重新连接.
    pub reconnect: bool,
}

/// A device code sign-in the server started: the code I type on GitHub's page.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GithubDevice {
    pub id: String,
    /// What I type on GitHub (`WDJB-MJHT`).
    pub user_code: String,
    /// The page I type it on.
    pub verification_uri: String,
    /// Seconds between polls.
    pub interval: u32,
    pub expires_at: Millis,
}

/// What a poll of a [`GithubDevice`] said.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum GithubPoll {
    /// Not confirmed yet.
    Pending,
    /// Poll less often: add 5 s to the interval.
    SlowDown,
    Done(GithubAccount),
    /// The code ran out (15 minutes).
    Expired,
    /// I cancelled on GitHub.
    Denied,
    /// That GitHub account is connected to another member.
    Taken,
}
