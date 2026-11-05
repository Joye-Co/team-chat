//! Where the provider keeps its files, the saved session, token storage and
//! the sign-in calls.
//!
//! Layout under the data root (`~/Library/Application Support/<app id>`):
//! - `sessions.json`: one signed-in member per server and the current server
//!   (no secret). Switching servers keeps the others signed in. An older
//!   `session.json` (one session) is read once and moved into it.
//! - `accounts/<server>/<member id>/cache.sqlite3`: the chat-core cache of one
//!   account, so two accounts never share a cache.
//! - `logs/`: chat-core and app logs (secrets are never logged).
//!
//! The token lives in the macOS Keychain (service = app id, account = server
//! URL). Self-tests use a memory store.
//!
//! The app id is the bundle id for a release build and `<bundle id>.dev` for
//! a debug build, so a build from a worktree never reads, replaces or signs
//! out the installed app's sessions and tokens.

use std::{
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};

use chat_core::{
    ApiError, HttpClient, TokenStore, TokenStoreError,
    proto::{self, ErrorCode},
};
use serde::{Deserialize, Serialize};

use crate::data::{Credential, LoginError};

/// Where tokens are kept.
#[derive(Clone)]
pub enum TokenStorage {
    /// The login Keychain (the app).
    Keychain,
    /// Process memory only (self-tests).
    Memory(Arc<Mutex<Option<String>>>),
    /// Tests: reads fail while the flag is set, as when the Keychain prompt
    /// is denied; otherwise the slot.
    #[cfg(test)]
    Failing(
        Arc<std::sync::atomic::AtomicBool>,
        Arc<Mutex<Option<String>>>,
    ),
}

impl TokenStorage {
    pub fn memory() -> Self {
        TokenStorage::Memory(Arc::new(Mutex::new(None)))
    }

    pub fn store(&self, server_url: &str) -> Box<dyn TokenStore> {
        match self {
            #[cfg(target_os = "macos")]
            TokenStorage::Keychain => {
                Box::new(chat_core::KeychainTokenStore::new(app_id(), server_url))
            }
            #[cfg(not(target_os = "macos"))]
            TokenStorage::Keychain => Box::new(chat_core::FileTokenStore::new(
                default_data_root().join("token"),
            )),
            TokenStorage::Memory(slot) => Box::new(MemoryTokenStore(slot.clone())),
            #[cfg(test)]
            TokenStorage::Failing(fail, slot) => Box::new(FailingTokenStore(
                fail.clone(),
                MemoryTokenStore(slot.clone()),
            )),
        }
    }
}

struct MemoryTokenStore(Arc<Mutex<Option<String>>>);

impl TokenStore for MemoryTokenStore {
    fn load(&self) -> Result<Option<String>, TokenStoreError> {
        Ok(self.0.lock().unwrap_or_else(|e| e.into_inner()).clone())
    }
    fn save(&self, token: &str) -> Result<(), TokenStoreError> {
        *self.0.lock().unwrap_or_else(|e| e.into_inner()) = Some(token.to_string());
        Ok(())
    }
    fn clear(&self) -> Result<(), TokenStoreError> {
        *self.0.lock().unwrap_or_else(|e| e.into_inner()) = None;
        Ok(())
    }
}

#[cfg(test)]
struct FailingTokenStore(Arc<std::sync::atomic::AtomicBool>, MemoryTokenStore);

#[cfg(test)]
impl TokenStore for FailingTokenStore {
    fn load(&self) -> Result<Option<String>, TokenStoreError> {
        if self.0.load(std::sync::atomic::Ordering::SeqCst) {
            return Err(TokenStoreError::Keychain("user denied".into()));
        }
        self.1.load()
    }
    fn save(&self, token: &str) -> Result<(), TokenStoreError> {
        self.1.save(token)
    }
    fn clear(&self) -> Result<(), TokenStoreError> {
        self.1.clear()
    }
}

/// The bundle id for a release build, `<bundle id>.dev` for a debug build:
/// the data folder and the Keychain service of this build.
pub const fn app_id() -> &'static str {
    if cfg!(debug_assertions) {
        concat!(env!("PRODUCT_BUNDLE_ID"), ".dev")
    } else {
        env!("PRODUCT_BUNDLE_ID")
    }
}

/// `~/Library/Application Support/<app id>`.
pub fn default_data_root() -> PathBuf {
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir);
    home.join("Library/Application Support").join(app_id())
}

/// The cache directory of one account.
pub fn account_dir(root: &Path, server_url: &str, member_id: &str) -> PathBuf {
    let safe = |s: &str| -> String {
        s.chars()
            .map(|c| {
                if c.is_ascii_alphanumeric() || c == '-' || c == '.' {
                    c
                } else {
                    '_'
                }
            })
            .collect()
    };
    let server = server_url
        .trim_end_matches('/')
        .trim_start_matches("https://")
        .trim_start_matches("http://");
    root.join("accounts")
        .join(safe(server))
        .join(safe(member_id))
}

/// One signed-in member on one server. No secret.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SavedSession {
    pub server_url: String,
    /// The member as the server last described it.
    pub member: proto::Member,
}

/// What `sessions.json` holds: a session per server and the server in use.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SavedSessions {
    /// The server the app uses now, signed in or not.
    #[serde(default)]
    pub current: Option<String>,
    #[serde(default)]
    pub sessions: Vec<SavedSession>,
}

impl SavedSessions {
    fn get(&self, server_url: &str) -> Option<&SavedSession> {
        self.sessions.iter().find(|s| same_server(&s.server_url, server_url))
    }
}

fn same_server(a: &str, b: &str) -> bool {
    a.trim_end_matches('/') == b.trim_end_matches('/')
}

/// `sessions.json`.
pub fn sessions_path(root: &Path) -> PathBuf {
    root.join("sessions.json")
}

/// The single session of builds before `sessions.json`.
fn legacy_session_path(root: &Path) -> PathBuf {
    root.join("session.json")
}

/// Every saved session; an older `session.json` counts as the current one.
pub fn load_sessions(root: &Path) -> SavedSessions {
    if let Ok(text) = std::fs::read_to_string(sessions_path(root)) {
        return serde_json::from_str(&text).unwrap_or_default();
    }
    std::fs::read_to_string(legacy_session_path(root))
        .ok()
        .and_then(|text| serde_json::from_str::<SavedSession>(&text).ok())
        .map(|s| SavedSessions {
            current: Some(s.server_url.clone()),
            sessions: vec![s],
        })
        .unwrap_or_default()
}

fn save_sessions(root: &Path, all: &SavedSessions) -> std::io::Result<()> {
    std::fs::create_dir_all(root)?;
    let path = sessions_path(root);
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, serde_json::to_vec_pretty(all)?)?;
    std::fs::rename(tmp, path)?;
    let _ = std::fs::remove_file(legacy_session_path(root));
    Ok(())
}

/// The session of the current server, if it is signed in.
pub fn load_session(root: &Path) -> Option<SavedSession> {
    let all = load_sessions(root);
    all.get(all.current.as_deref()?).cloned()
}

/// The server in use, signed in or not.
pub fn current_server(root: &Path) -> Option<String> {
    load_sessions(root).current
}

/// Saves the session of its server and makes that server the current one.
pub fn save_session(root: &Path, session: &SavedSession) -> std::io::Result<()> {
    let mut all = load_sessions(root);
    all.sessions
        .retain(|s| !same_server(&s.server_url, &session.server_url));
    all.sessions.push(session.clone());
    all.current = Some(session.server_url.clone());
    save_sessions(root, &all)
}

/// Forgets the session of one server; the other servers stay signed in and
/// the current server stays the same.
pub fn forget_session(root: &Path, server_url: &str) {
    let mut all = load_sessions(root);
    all.sessions.retain(|s| !same_server(&s.server_url, server_url));
    let _ = save_sessions(root, &all);
}

/// Makes `server_url` the current server. Returns its saved session.
pub fn set_current_server(root: &Path, server_url: &str) -> Option<SavedSession> {
    let mut all = load_sessions(root);
    all.current = Some(server_url.to_string());
    let _ = save_sessions(root, &all);
    all.get(server_url).cloned()
}

/// The servers that have a saved session.
pub fn signed_in_servers(root: &Path) -> Vec<String> {
    load_sessions(root)
        .sessions
        .into_iter()
        .map(|s| s.server_url)
        .collect()
}

/// The name of this Mac, shown in the member's device list.
pub fn device_name() -> String {
    let mut buf = [0u8; 256];
    // SAFETY: the buffer is valid for its length; gethostname writes a C string.
    let ok = unsafe { libc::gethostname(buf.as_mut_ptr().cast(), buf.len()) } == 0;
    let name = if ok {
        let end = buf.iter().position(|&b| b == 0).unwrap_or(buf.len());
        String::from_utf8_lossy(&buf[..end]).to_string()
    } else {
        String::new()
    };
    let name = name.trim_end_matches(".local").trim();
    if name.is_empty() {
        "Mac".into()
    } else {
        name.chars().take(64).collect()
    }
}

/// Normalizes what the user typed as the server URL.
pub fn normalize_server_url(input: &str) -> Option<String> {
    let s = input.trim();
    if s.is_empty() || s.chars().any(char::is_whitespace) {
        return None;
    }
    let (scheme, rest) = if let Some(rest) = s.strip_prefix("http://") {
        ("http://", rest)
    } else if let Some(rest) = s.strip_prefix("https://") {
        ("https://", rest)
    } else {
        ("https://", s)
    };
    let rest = rest.trim_end_matches('/');
    (!rest.is_empty() && !rest.starts_with('/')).then(|| format!("{scheme}{rest}"))
}

pub fn login_error(e: &ApiError) -> LoginError {
    match e {
        ApiError::Network(_) | ApiError::BadUrl => LoginError::ServerUnreachable,
        ApiError::Server { code, status, .. } => match code {
            ErrorCode::UnsupportedProtocol => LoginError::Unsupported,
            ErrorCode::Unauthorized
            | ErrorCode::Forbidden
            | ErrorCode::NotFound
            | ErrorCode::Invalid
            | ErrorCode::Conflict => LoginError::BadCredential,
            _ if *status == 401 => LoginError::BadCredential,
            _ => LoginError::Other,
        },
        ApiError::Unexpected { status: 401 } => LoginError::BadCredential,
        // Not our server (a web page, a proxy error).
        ApiError::Unexpected { .. } | ApiError::Decode(_) => LoginError::ServerUnreachable,
        ApiError::NoToken | ApiError::Invalid(_) | ApiError::Io(_) | ApiError::Checksum => {
            LoginError::Other
        }
    }
}

/// Signs in on the server: redeems an invite code or checks a pasted token.
/// Returns the token and my member.
pub async fn sign_in(
    server_url: &str,
    credential: &Credential,
    device_name: &str,
) -> Result<(String, proto::Member), LoginError> {
    let http = HttpClient::new(server_url, None).map_err(|e| login_error(&e))?;
    match credential {
        Credential::InviteCode(code) => {
            let code = code.trim();
            let r = http
                .redeem(code, device_name)
                .await
                .map_err(|e| login_error(&e))?;
            Ok((r.token, r.member))
        }
        Credential::Token(token) => {
            let token = token.trim().to_string();
            let me = http
                .with_token(Some(token.clone()))
                .me()
                .await
                .map_err(|e| login_error(&e))?;
            Ok((token, me))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn server_url_is_normalized() {
        assert_eq!(
            normalize_server_url(" example.com/ ").as_deref(),
            Some("https://example.com")
        );
        assert_eq!(
            normalize_server_url("http://127.0.0.1:8791").as_deref(),
            Some("http://127.0.0.1:8791")
        );
        assert_eq!(normalize_server_url("https://"), None);
        assert_eq!(normalize_server_url("a b"), None);
    }

    #[test]
    fn account_dirs_are_per_server_and_member() {
        let root = Path::new("/r");
        let a = account_dir(root, "https://x.dev/", "m1");
        assert_eq!(a, Path::new("/r/accounts/x.dev/m1"));
        assert_ne!(a, account_dir(root, "https://y.dev", "m1"));
        assert!(
            !account_dir(root, "http://h:1/../..", "../m")
                .to_string_lossy()
                .contains("/../")
        );
    }

    #[test]
    fn each_server_keeps_its_own_session() {
        let root = std::env::temp_dir().join(format!("sessions-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let member = |id: &str| -> proto::Member {
            serde_json::from_value(serde_json::json!({
                "id": id, "kind": "human", "name": "A", "level": "member"
            }))
            .unwrap()
        };
        // A session.json of an older build is the current session.
        std::fs::create_dir_all(&root).unwrap();
        let prod = SavedSession {
            server_url: "https://prod.dev".into(),
            member: member("m1"),
        };
        std::fs::write(legacy_session_path(&root), serde_json::to_vec(&prod).unwrap()).unwrap();
        assert_eq!(load_session(&root), Some(prod.clone()));

        // Signing in on staging keeps production signed in.
        let staging = SavedSession {
            server_url: "https://staging.dev".into(),
            member: member("m2"),
        };
        save_session(&root, &staging).unwrap();
        assert!(!legacy_session_path(&root).exists());
        assert_eq!(load_session(&root), Some(staging.clone()));
        assert_eq!(signed_in_servers(&root).len(), 2);

        // Switching back finds the production session.
        assert_eq!(set_current_server(&root, "https://prod.dev/"), Some(prod.clone()));
        assert_eq!(load_session(&root), Some(prod.clone()));

        // Signing out of production leaves staging alone, and production
        // stays the current server for the login window.
        forget_session(&root, "https://prod.dev");
        assert_eq!(load_session(&root), None);
        assert_eq!(current_server(&root).as_deref(), Some("https://prod.dev/"));
        assert_eq!(signed_in_servers(&root), vec!["https://staging.dev".to_string()]);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_debug_build_has_its_own_app_id() {
        assert_eq!(app_id(), concat!(env!("PRODUCT_BUNDLE_ID"), ".dev"));
        assert!(default_data_root().ends_with(app_id()));
    }

    #[test]
    fn errors_map_to_login_errors() {
        let server = |status, code| ApiError::Server {
            status,
            code,
            message: String::new(),
        };
        assert_eq!(
            login_error(&server(401, ErrorCode::Unauthorized)),
            LoginError::BadCredential
        );
        assert_eq!(
            login_error(&server(404, ErrorCode::NotFound)),
            LoginError::BadCredential
        );
        assert_eq!(
            login_error(&ApiError::Network("x".into())),
            LoginError::ServerUnreachable
        );
        assert_eq!(
            login_error(&server(400, ErrorCode::UnsupportedProtocol)),
            LoginError::Unsupported
        );
    }

    #[test]
    fn a_keychain_read_error_keeps_the_session_and_the_token() {
        use std::sync::atomic::{AtomicBool, Ordering};
        use std::time::{Duration, Instant};

        use crate::data::{ChatProvider as _, ConnectionState, SessionState};
        use crate::provider::{CoreProvider, ProviderConfig};

        let root = std::env::temp_dir().join(format!("token-unreadable-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let member: proto::Member = serde_json::from_value(serde_json::json!({
            "id": "m1", "kind": "human", "name": "A", "level": "member"
        }))
        .unwrap();
        save_session(
            &root,
            &SavedSession {
                server_url: "http://127.0.0.1:9".into(),
                member,
            },
        )
        .unwrap();
        let fail = Arc::new(AtomicBool::new(true));
        let slot = Arc::new(Mutex::new(Some("saved-token".to_string())));
        let p = CoreProvider::new(ProviderConfig {
            data_root: root.clone(),
            tokens: TokenStorage::Failing(fail.clone(), slot.clone()),
            restore: true,
        });
        let wait = |what: &str, f: &dyn Fn() -> bool| {
            let deadline = Instant::now() + Duration::from_secs(10);
            while !f() {
                assert!(Instant::now() < deadline, "timed out: {what}");
                std::thread::sleep(Duration::from_millis(10));
            }
        };
        wait("unreadable", &|| {
            p.connection() == ConnectionState::TokenUnreadable
        });
        assert!(matches!(p.session(), SessionState::SignedIn { .. }));
        assert!(load_session(&root).is_some());
        assert!(slot.lock().unwrap().is_some());

        // The prompt is allowed on retry: the client starts.
        fail.store(false, Ordering::SeqCst);
        p.reconnect_now();
        wait("token read again", &|| {
            p.connection() != ConnectionState::TokenUnreadable
        });
        assert!(matches!(p.session(), SessionState::SignedIn { .. }));
        p.sign_out_local_for_selftest();
        let _ = std::fs::remove_dir_all(&root);
    }
}
