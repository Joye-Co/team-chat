//! `desktop --sandbox <dir>`: the real app on this worktree's local sandbox server
//! (`scripts/sandbox/up.ts`, the sample team of `scripts/sandbox/seed.ts`). The app is the same
//! as in production (chat-core, the real provider); only its files live under `<dir>/app`, its
//! token in a file there instead of the Keychain, and it signs in by itself with the seeded
//! owner token. `bun run dev:desktop` starts the server and opens the app this way.

use std::path::{Path, PathBuf};

use super::{CoreProvider, ProviderConfig, session};
use crate::data::{ChatProvider as _, Credential, LoginRequest, SessionState};

pub struct Sandbox {
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
        let sandbox = Sandbox {
            dir: dir.to_path_buf(),
            server_url: read("server-url")?,
            token: read("owner-token")?,
        };
        session::use_data_root(sandbox.dir.join("app"));
        Ok(sandbox)
    }

    pub fn config(&self) -> ProviderConfig {
        let root = self.dir.join("app");
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
