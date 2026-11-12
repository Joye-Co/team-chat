//! Each person's GitHub sign-in (docs/protocol.md, "GitHub sign-in"; design
//! docs/plans/usable/design-github-identity.zh.md §3.7). The last `GET /v1/me/github` is kept
//! per account, so the 话题 banner and the 账号 page read it without waiting; it is read again
//! when a view asks (a 话题 opens, the app comes to the front) and on `github.status`.

use std::sync::Mutex;

use chat_core::{
    ApiError,
    proto::{self, ErrorCode},
};

use super::{CoreProvider, lock};
use crate::data::{
    AdminError, AdminReply, Change, GithubAccount, GithubDevice, GithubPoll, GithubState,
    GithubWait,
};

/// The last answer, keyed by the account it belongs to.
#[derive(Default)]
pub(super) struct GithubCache {
    state: Mutex<Option<(String, GithubState)>>,
}

pub(super) fn account(a: &proto::GithubAccount) -> GithubAccount {
    GithubAccount {
        login: a.login.clone(),
        name: a.name.clone().filter(|n| !n.trim().is_empty()),
        // An unknown status reads as "invalid" (docs/protocol.md).
        ok: a.status == "ok",
        connected_at: a.connected_at as i64,
    }
}

pub(super) fn state(r: &proto::GithubStatusResponse) -> GithubState {
    GithubState {
        available: r.available,
        unavailable_reason: r.unavailable_reason.clone(),
        account: r.account.as_ref().map(account),
        waits: r
            .waits
            .iter()
            .map(|w| GithubWait {
                run_id: w.run_id.clone(),
                group_id: w.group_id.clone(),
                topic_id: w.topic_id.clone(),
                bot_id: w.bot_id.clone(),
                reconnect: w.reason == "invalid",
            })
            .collect(),
    }
}

pub(super) fn poll(r: &proto::GithubPollResponse) -> GithubPoll {
    match (r.state.as_str(), r.account.as_ref()) {
        ("done", Some(a)) => GithubPoll::Done(account(a)),
        ("slow_down", _) => GithubPoll::SlowDown,
        ("expired", _) => GithubPoll::Expired,
        ("denied", _) => GithubPoll::Denied,
        ("taken", _) => GithubPoll::Taken,
        // "pending", "done" without an account, or a newer state: ask again.
        _ => GithubPoll::Pending,
    }
}

/// A refusal of the GitHub routes. "Not set up on this server" is a conflict
/// (`github.no_client_id`); a server without the routes answers 404.
fn error(e: &ApiError) -> AdminError {
    match e {
        ApiError::Server {
            code: ErrorCode::Conflict,
            ..
        } => AdminError::Unsupported,
        other => super::settings::api_error(other),
    }
}

impl CoreProvider {
    fn github_key(&self) -> Option<String> {
        self.account().map(|a| a.key.clone())
    }

    pub(super) fn github_inner(&self) -> Option<GithubState> {
        let key = self.github_key()?;
        lock(&self.shared.github.state)
            .as_ref()
            .filter(|(k, _)| *k == key)
            .map(|(_, s)| s.clone())
    }

    /// Changes the kept state of the account `key` (if it is still the one shown) and emits.
    fn github_update(&self, key: &str, f: impl FnOnce(&mut GithubState)) {
        {
            let mut kept = lock(&self.shared.github.state);
            match kept.as_mut() {
                Some((k, s)) if k == key => f(s),
                _ => return,
            }
        }
        self.emit(vec![Change::Github]);
    }

    pub(super) fn refresh_github_inner(&self) {
        let Some(a) = self.account() else {
            return;
        };
        let key = a.key.clone();
        let client = a.client.clone();
        let this = self.clone();
        a.client.spawn(async move {
            match client.http().github_status().await {
                Ok(r) => {
                    let next = state(&r);
                    let changed = {
                        let mut kept = lock(&this.shared.github.state);
                        let same = kept.as_ref().is_some_and(|(k, s)| *k == key && *s == next);
                        *kept = Some((key, next));
                        !same
                    };
                    if changed {
                        this.emit(vec![Change::Github]);
                    }
                }
                // An older server has no route: nothing about GitHub shows.
                Err(e) => tracing::debug!(error = %e, "github status"),
            }
        });
    }

    /// `github.status`: the account changed on another device or on the server. The waits
    /// are read again (a connect resumes them on the server).
    pub(super) fn on_github_status(&self, t: &proto::GithubStatusTransient) {
        if let Some(key) = self.github_key() {
            let next = t.account.as_ref().map(account);
            self.github_update(&key, |s| s.account = next);
        }
        self.refresh_github_inner();
    }

    pub(super) fn start_github_inner(&self) -> AdminReply<GithubDevice> {
        self.request(|client, _| async move {
            let r = client.http().github_device().await.map_err(|e| error(&e))?;
            Ok(GithubDevice {
                id: r.device_id,
                user_code: r.user_code,
                verification_uri: r.verification_uri,
                interval: r.interval.max(1),
                expires_at: r.expires_at as i64,
            })
        })
    }

    pub(super) fn poll_github_inner(&self, device_id: &str) -> AdminReply<GithubPoll> {
        let id = device_id.to_string();
        let key = self.github_key();
        let this = self.clone();
        self.request(move |client, _| async move {
            let r = client
                .http()
                .github_poll(&id)
                .await
                .map_err(|e| error(&e))?;
            let out = poll(&r);
            if let (GithubPoll::Done(a), Some(key)) = (&out, key) {
                let a = a.clone();
                // The server resumed my waits: they go once the status is read again.
                this.github_update(&key, |s| s.account = Some(a));
                this.refresh_github_inner();
            }
            Ok(out)
        })
    }

    pub(super) fn disconnect_github_inner(&self) -> AdminReply<()> {
        let key = self.github_key();
        let this = self.clone();
        self.request(move |client, _| async move {
            client
                .http()
                .github_disconnect()
                .await
                .map_err(|e| error(&e))?;
            if let Some(key) = key {
                this.github_update(&key, |s| s.account = None);
            }
            Ok(())
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wire(status: &str) -> proto::GithubAccount {
        proto::GithubAccount {
            login: "octo".into(),
            name: Some(" ".into()),
            status: status.into(),
            connected_at: 5,
        }
    }

    #[test]
    fn an_unknown_status_reads_as_needing_a_new_connection() {
        assert!(account(&wire("ok")).ok);
        assert!(!account(&wire("invalid")).ok);
        assert!(!account(&wire("revoked-later")).ok);
        // A blank profile name falls back to the login.
        assert_eq!(account(&wire("ok")).name, None);
    }

    #[test]
    fn poll_states_map_and_unknown_ones_keep_polling() {
        let r = |state: &str, account: Option<proto::GithubAccount>| proto::GithubPollResponse {
            state: state.into(),
            account,
        };
        assert_eq!(poll(&r("pending", None)), GithubPoll::Pending);
        assert_eq!(poll(&r("slow_down", None)), GithubPoll::SlowDown);
        assert_eq!(poll(&r("expired", None)), GithubPoll::Expired);
        assert_eq!(poll(&r("denied", None)), GithubPoll::Denied);
        assert_eq!(poll(&r("taken", None)), GithubPoll::Taken);
        assert_eq!(poll(&r("done", None)), GithubPoll::Pending);
        assert_eq!(poll(&r("later", None)), GithubPoll::Pending);
        assert!(matches!(
            poll(&r("done", Some(wire("ok")))),
            GithubPoll::Done(a) if a.login == "octo"
        ));
    }

    #[test]
    fn waits_say_which_need_a_new_connection() {
        let r = proto::GithubStatusResponse {
            available: true,
            unavailable_reason: None,
            account: None,
            waits: vec![
                proto::GithubWait {
                    run_id: "r1".into(),
                    group_id: "g".into(),
                    topic_id: "t".into(),
                    bot_id: "b".into(),
                    reason: "not_connected".into(),
                },
                proto::GithubWait {
                    run_id: "r2".into(),
                    group_id: "g".into(),
                    topic_id: "t".into(),
                    bot_id: "c".into(),
                    reason: "invalid".into(),
                },
            ],
        };
        let s = state(&r);
        assert!(!s.waits[0].reconnect && s.waits[1].reconnect);
        // One banner per 话题: the newest wait.
        assert_eq!(s.wait_in("g", "t").map(|w| w.run_id.as_str()), Some("r2"));
        assert!(s.wait_in("g", "other").is_none());
    }

    #[test]
    fn not_set_up_is_unsupported() {
        let e = ApiError::Server {
            status: 409,
            code: ErrorCode::Conflict,
            message: String::new(),
        };
        assert_eq!(error(&e), AdminError::Unsupported);
        assert_eq!(error(&ApiError::Network("x".into())), AdminError::Offline);
    }
}
