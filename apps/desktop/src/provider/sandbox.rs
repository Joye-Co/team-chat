//! `desktop --sandbox <dir>`: the real app on this worktree's local sandbox server
//! (`scripts/sandbox/up.ts`, the sample team of `scripts/sandbox/seed.ts`). The app is the same
//! as in production (chat-core, the real provider); only its files live under `<dir>/app`, its
//! token in a file there instead of the Keychain, and it signs in by itself with the seeded
//! owner token. `bun run dev:desktop` starts the server and opens the app this way.

use std::path::{Path, PathBuf};

use super::{CoreProvider, ProviderConfig, session};
use crate::data::{ChatProvider as _, Credential, LoginRequest, SessionState};

pub struct Sandbox {
    /// The app's data root: `<dir>/app` (`app-admin`, `app-member` for the other 身份).
    dir: PathBuf,
    server_url: String,
    token: String,
}

impl Sandbox {
    /// Reads `<dir>/server-url` and `<dir>/owner-token`, and moves the data root to `<dir>/app`.
    pub fn open(dir: &Path) -> Result<Self, String> {
        let read = |name: &str| {
            std::fs::read_to_string(dir.join(name))
                .map(|s| s.trim().to_string())
                .map_err(|_| {
                    format!(
                        "{} is missing: start the sandbox with `bun scripts/sandbox/up.ts`",
                        dir.join(name).display()
                    )
                })
        };
        // `TC_SANDBOX_AS=admin`, `member` or `newcomer`: the seeded 管理员, 普通成员 or a member
        // who just joined (首页 从这里开始), in a data root of its own, to try the 身份
        // (design-roles).
        let who = std::env::var("TC_SANDBOX_AS")
            .ok()
            .filter(|w| matches!(w.as_str(), "admin" | "member" | "newcomer"));
        let (token, app) = match &who {
            Some(w) => (format!("{w}-token"), format!("app-{w}")),
            None => ("owner-token".to_string(), "app".to_string()),
        };
        let sandbox = Sandbox {
            dir: dir.join(app),
            server_url: read("server-url")?,
            token: read(&token)?,
        };
        session::use_data_root(sandbox.dir.clone());
        Ok(sandbox)
    }

    pub fn config(&self) -> ProviderConfig {
        let root = self.dir.clone();
        ProviderConfig {
            tokens: session::TokenStorage::Files(root.join("tokens")),
            data_root: root,
            restore: true,
        }
    }

    /// Signs in as the sample team's owner, unless a saved session was restored.
    pub fn sign_in(&self, core: &CoreProvider) {
        if matches!(core.session(), SessionState::SignedOut { .. }) {
            core.login(LoginRequest {
                server_url: self.server_url.clone(),
                credential: Credential::Token(self.token.clone()),
                device_name: "sandbox".into(),
            });
        }
    }
}

#[cfg(test)]
mod tests {
    /// `up.ts --reset` and the restart after a half-finished seed must remove every data root
    /// this file opens, or a role restores a stale token and another server's cache.
    #[test]
    fn up_script_removes_every_data_root() {
        let up = include_str!("../../../../scripts/sandbox/up.ts");
        let line = up
            .lines()
            .find(|l| l.starts_with("const APP_ROOTS = "))
            .expect("up.ts lists APP_ROOTS");
        for root in ["app", "app-admin", "app-member"] {
            assert!(
                line.contains(&format!("\"{root}\"")),
                "{root} missing from APP_ROOTS"
            );
        }
        assert_eq!(
            up.matches("APP_ROOTS").count(),
            3,
            "both reset paths use APP_ROOTS"
        );
    }
}
