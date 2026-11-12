//! GitHub sign-in of the fake provider, for the screenshot scenes only
//! (design-github-identity.zh.md §3.7). A sign-in shows a fixed made-up code that stays
//! waiting; nothing reaches GitHub.

use std::sync::Mutex;

use crate::data::{GithubAccount, GithubDevice, GithubState};

/// What the fake server says about my GitHub.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum GithubShape {
    /// The server can connect GitHub; I have not.
    #[default]
    NotConnected,
    Connected,
    /// GitHub refused the saved connection: 需要重新连接.
    Invalid,
    /// The server has no GitHub client id.
    Unavailable,
    /// Not connected, and an AI member waits for it in the first 话题 the scenes open.
    Waiting,
}

#[derive(Default)]
pub struct FakeGithub {
    shape: Mutex<GithubShape>,
}

impl FakeGithub {
    pub fn set(&self, shape: GithubShape) {
        *self.shape.lock().unwrap_or_else(|e| e.into_inner()) = shape;
    }

    pub fn shape(&self) -> GithubShape {
        *self.shape.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// The state without waits; `Waiting` gets its wait from the provider's samples.
    pub fn state(&self) -> GithubState {
        let account = |ok| GithubAccount {
            login: "joye-dev".into(),
            name: Some("Joye".into()),
            ok,
            connected_at: crate::selftest::clock::now_ms() - 3 * 86_400_000,
        };
        match self.shape() {
            GithubShape::NotConnected | GithubShape::Waiting => GithubState {
                available: true,
                ..Default::default()
            },
            GithubShape::Connected => GithubState {
                available: true,
                account: Some(account(true)),
                ..Default::default()
            },
            GithubShape::Invalid => GithubState {
                available: true,
                account: Some(account(false)),
                ..Default::default()
            },
            GithubShape::Unavailable => GithubState {
                available: false,
                unavailable_reason: Some("no_client_id".into()),
                ..Default::default()
            },
        }
    }
}

/// The made-up code of every fake sign-in (GitHub's own example shape).
pub fn device() -> GithubDevice {
    GithubDevice {
        id: "fake-device".into(),
        user_code: "WDJB-MJHT".into(),
        verification_uri: "https://github.com/login/device".into(),
        interval: 5,
        expires_at: crate::selftest::clock::now_ms() + 15 * 60_000,
    }
}
