//! Where the provider keeps its files, the saved session, token storage and
//! the sign-in calls.
//!
//! Layout under the data root (`~/Library/Application Support/<app id>`):
//! - `sessions.json`: the accounts signed in on this Mac (server and member
//!   of each, no secret), the active one and the server in use. Switching
//!   keeps the others signed in. An older `session.json` (one session) is
//!   read once and moved into it.
//! - `accounts/<server>/<member id>/cache.sqlite3`: the chat-core cache of one
//!   account, so two accounts never share a cache.
//! - `logs/`: chat-core and app logs (secrets are never logged).
//!
//! Each token lives in the macOS Keychain (service = app id, account =
//! server URL, or `<server url>#<member id>` for a second member on one
//! server; see [`SavedSession::token_key`]). Self-tests use a memory store.
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

static DATA_ROOT: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();

/// Keeps every file of this run under `root` instead of Application Support (the local
/// sandbox, `desktop --sandbox`). Call it before anything reads the data root.
pub fn use_data_root(root: PathBuf) {
    let _ = DATA_ROOT.set(root);
}

/// Where tokens are kept.
#[derive(Clone)]
pub enum TokenStorage {
    /// The login Keychain (the app).
    Keychain,
    /// Process memory only (self-tests).
    Memory(Arc<Mutex<Option<String>>>),
    /// Files of mode 600 in this folder, one per server (the local sandbox, so a debug
    /// build signs in without Keychain prompts).
    Files(PathBuf),
    /// Tests: reads fail while the flag is set, as when the Keychain prompt
    /// is denied; otherwise the slot.
    /// Tests: one memory slot per Keychain account.
    #[cfg(test)]
    Keyed(Arc<Mutex<std::collections::HashMap<String, String>>>),
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

    /// The token store of one Keychain account ([`SavedSession::token_key`]).
    pub fn store(&self, server_url: &str) -> Box<dyn TokenStore> {
        match self {
            #[cfg(target_os = "macos")]
            TokenStorage::Keychain => {
                Box::new(chat_core::KeychainTokenStore::new(app_id(), server_url))
            }
            // The Credential Manager, one generic credential per server.
            #[cfg(target_os = "windows")]
            TokenStorage::Keychain => {
                Box::new(chat_core::CredentialTokenStore::new(app_id(), server_url))
            }
            #[cfg(not(any(target_os = "macos", target_os = "windows")))]
            TokenStorage::Keychain => Box::new(chat_core::FileTokenStore::new(
                default_data_root().join("token"),
            )),
            TokenStorage::Memory(slot) => Box::new(MemoryTokenStore(slot.clone())),
            TokenStorage::Files(dir) => {
                let name: String = server_url
                    .chars()
                    .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
                    .collect();
                Box::new(chat_core::FileTokenStore::new(dir.join(name)))
            }
            #[cfg(test)]
            TokenStorage::Keyed(map) => Box::new(KeyedTokenStore(map.clone(), server_url.into())),
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
struct KeyedTokenStore(
    Arc<Mutex<std::collections::HashMap<String, String>>>,
    String,
);

#[cfg(test)]
impl TokenStore for KeyedTokenStore {
    fn load(&self) -> Result<Option<String>, TokenStoreError> {
        Ok(self.0.lock().unwrap().get(&self.1).cloned())
    }
    fn save(&self, token: &str) -> Result<(), TokenStoreError> {
        self.0.lock().unwrap().insert(self.1.clone(), token.into());
        Ok(())
    }
    fn clear(&self) -> Result<(), TokenStoreError> {
        self.0.lock().unwrap().remove(&self.1);
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

/// `~/Library/Application Support/<app id>` (`%APPDATA%\<app id>` on Windows).
pub fn default_data_root() -> PathBuf {
    if let Some(root) = DATA_ROOT.get() {
        return root.clone();
    }
    crate::paths::data_base().join(app_id())
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
    /// The Keychain account of the token. `None` is the server URL (the
    /// key of every token saved before there were several accounts); a second
    /// member on the same server gets `<server url>#<member id>`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub token_key: Option<String>,
}

impl SavedSession {
    pub fn new(server_url: impl Into<String>, member: proto::Member) -> Self {
        Self {
            server_url: server_url.into(),
            member,
            token_key: None,
        }
    }

    /// The id of this account in the list: server and member.
    pub fn key(&self) -> String {
        account_key(&self.server_url, &self.member.id)
    }

    /// The Keychain account its token is saved under.
    pub fn token_key(&self) -> &str {
        self.token_key.as_deref().unwrap_or(&self.server_url)
    }
}

/// The id of an account: `<server url>#<member id>`.
pub fn account_key(server_url: &str, member_id: &str) -> String {
    format!("{}#{member_id}", server_url.trim_end_matches('/'))
}

pub fn same_server(a: &str, b: &str) -> bool {
    a.trim_end_matches('/') == b.trim_end_matches('/')
}

/// Every account signed in on this Mac, the one the window shows and the
/// server in use.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SavedAccounts {
    pub accounts: Vec<SavedSession>,
    /// The key of the active account; `None` when signed out.
    pub active: Option<String>,
    /// The server in use, signed in or not (the login window's server).
    pub current: Option<String>,
}

impl SavedAccounts {
    pub fn get(&self, key: &str) -> Option<&SavedSession> {
        self.accounts.iter().find(|a| a.key() == key)
    }

    /// The account to restore at start.
    pub fn active(&self) -> Option<&SavedSession> {
        self.active.as_deref().and_then(|k| self.get(k))
    }

    /// The first account signed in on `server_url`.
    pub fn on_server(&self, server_url: &str) -> Option<&SavedSession> {
        self.accounts
            .iter()
            .find(|a| same_server(&a.server_url, server_url))
    }

    /// Makes `key` the active account and its server the one in use.
    pub fn activate(&mut self, key: &str) {
        if let Some(server) = self.get(key).map(|a| a.server_url.clone()) {
            self.active = Some(key.to_string());
            self.current = Some(server);
        }
    }

    /// Adds `session` or updates the saved one of the same server and
    /// member, makes it active, and returns it with its token key: an
    /// account signed in again keeps its key; a new one takes the server
    /// URL unless another member on that server has it.
    pub fn sign_in(&mut self, mut session: SavedSession) -> SavedSession {
        let key = session.key();
        if let Some(old) = self.get(&key) {
            session.token_key = old.token_key.clone();
        } else {
            let taken = self
                .accounts
                .iter()
                .any(|a| a.token_key() == session.server_url);
            session.token_key = taken.then(|| key.clone());
        }
        match self.accounts.iter_mut().find(|a| a.key() == key) {
            Some(slot) => *slot = session.clone(),
            None => self.accounts.push(session.clone()),
        }
        self.activate(&key);
        session
    }

    /// Forgets one account; the active one leaves no account active (the
    /// server in use stays).
    pub fn remove(&mut self, key: &str) -> Option<SavedSession> {
        let i = self.accounts.iter().position(|a| a.key() == key)?;
        if self.active.as_deref() == Some(key) {
            self.active = None;
        }
        Some(self.accounts.remove(i))
    }

    /// The servers with a saved account, each once.
    pub fn servers(&self) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        for a in &self.accounts {
            if !out.iter().any(|s| same_server(s, &a.server_url)) {
                out.push(a.server_url.clone());
            }
        }
        out
    }
}

/// `sessions.json` on disk: `current` and `sessions` are the per-server
/// format of the first build with several servers (which reads its session
/// of `current` and ignores the rest); `active` names the account the
/// window shows when one server has several.
#[derive(Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SessionsFile {
    #[serde(default)]
    current: Option<String>,
    #[serde(default)]
    sessions: Vec<SavedSession>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    active: Option<String>,
}

/// `sessions.json`.
pub fn sessions_path(root: &Path) -> PathBuf {
    root.join("sessions.json")
}

/// The single session of builds before `sessions.json`.
fn legacy_session_path(root: &Path) -> PathBuf {
    root.join("session.json")
}

/// Reads the saved accounts. Without `sessions.json`, an older
/// `session.json` (one session) is that one account, active, its token
/// still under the server URL.
pub fn load_accounts(root: &Path) -> SavedAccounts {
    if let Ok(text) = std::fs::read_to_string(sessions_path(root)) {
        let Ok(file) = serde_json::from_str::<SessionsFile>(&text) else {
            tracing::warn!("sessions.json cannot be read");
            return SavedAccounts::default();
        };
        let mut accounts = SavedAccounts {
            accounts: file.sessions,
            active: None,
            current: file.current,
        };
        // The named account, or the first one on the server in use.
        let active = file
            .active
            .filter(|k| accounts.get(k).is_some())
            .or_else(|| {
                let current = accounts.current.clone()?;
                accounts.on_server(&current).map(SavedSession::key)
            });
        accounts.active = active;
        return accounts;
    }
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct Legacy {
        server_url: String,
        member: proto::Member,
    }
    std::fs::read_to_string(legacy_session_path(root))
        .ok()
        .and_then(|text| serde_json::from_str::<Legacy>(&text).ok())
        .map(|s| {
            let one = SavedSession::new(s.server_url, s.member);
            SavedAccounts {
                active: Some(one.key()),
                current: Some(one.server_url.clone()),
                accounts: vec![one],
            }
        })
        .unwrap_or_default()
}

/// Writes the accounts (atomically) and removes an older `session.json`.
pub fn save_accounts(root: &Path, accounts: &SavedAccounts) -> std::io::Result<()> {
    std::fs::create_dir_all(root)?;
    let file = SessionsFile {
        current: accounts
            .active()
            .map(|a| a.server_url.clone())
            .or_else(|| accounts.current.clone()),
        sessions: accounts.accounts.clone(),
        active: accounts.active.clone(),
    };
    let path = sessions_path(root);
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, serde_json::to_vec_pretty(&file)?)?;
    std::fs::rename(tmp, path)?;
    let _ = std::fs::remove_file(legacy_session_path(root));
    Ok(())
}

/// The active account.
pub fn load_session(root: &Path) -> Option<SavedSession> {
    load_accounts(root).active().cloned()
}

/// Saves `session` as a signed-in account and makes it active. Returns it
/// with its token key.
#[cfg(test)]
pub fn save_session(root: &Path, session: &SavedSession) -> std::io::Result<SavedSession> {
    let mut accounts = load_accounts(root);
    let saved = accounts.sign_in(session.clone());
    save_accounts(root, &accounts)?;
    Ok(saved)
}

#[cfg(target_os = "macos")]
fn host_name() -> String {
    let mut buf = [0u8; 256];
    // SAFETY: the buffer is valid for its length; gethostname writes a C string.
    let ok = unsafe { libc::gethostname(buf.as_mut_ptr().cast(), buf.len()) } == 0;
    if ok {
        let end = buf.iter().position(|&b| b == 0).unwrap_or(buf.len());
        String::from_utf8_lossy(&buf[..end]).to_string()
    } else {
        String::new()
    }
}

/// Windows sets `COMPUTERNAME` for every process; other systems may set `HOSTNAME`.
#[cfg(not(target_os = "macos"))]
fn host_name() -> String {
    std::env::var("COMPUTERNAME")
        .or_else(|_| std::env::var("HOSTNAME"))
        .unwrap_or_default()
}

/// The name of this computer, shown in the member's device list.
pub fn device_name() -> String {
    let name = host_name();
    let name = name.trim_end_matches(".local").trim();
    if name.is_empty() {
        if cfg!(target_os = "macos") {
            "Mac"
        } else {
            "PC"
        }
        .into()
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
        Credential::EmailCode { email, code } => {
            let r = http
                .email_sign_in(email, code, device_name)
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
        let root = temp_root("sessions");
        // A session.json of an older build is the current session.
        let prod = SavedSession::new("https://prod.dev", member("m1", "A"));
        std::fs::write(
            legacy_session_path(&root),
            serde_json::to_vec(&serde_json::json!({
                "serverUrl": prod.server_url, "member": prod.member
            }))
            .unwrap(),
        )
        .unwrap();
        assert_eq!(load_session(&root), Some(prod.clone()));

        // Signing in on staging keeps production signed in.
        let staging = SavedSession::new("https://staging.dev", member("m2", "B"));
        save_session(&root, &staging).unwrap();
        assert!(!legacy_session_path(&root).exists());
        assert_eq!(load_session(&root), Some(staging.clone()));
        let mut all = load_accounts(&root);
        assert_eq!(all.servers().len(), 2);

        // Switching back finds the production session.
        let key = all.on_server("https://prod.dev/").unwrap().key();
        all.activate(&key);
        save_accounts(&root, &all).unwrap();
        assert_eq!(load_session(&root), Some(prod.clone()));

        // Forgetting production leaves staging alone, and production stays
        // the server in use for the login window.
        all.remove(&key);
        save_accounts(&root, &all).unwrap();
        let all = load_accounts(&root);
        assert_eq!(all.active(), None);
        assert_eq!(all.current.as_deref(), Some("https://prod.dev"));
        assert_eq!(all.servers(), vec!["https://staging.dev".to_string()]);
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
        save_session(&root, &SavedSession::new("http://127.0.0.1:9", member)).unwrap();
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

    fn member(id: &str, name: &str) -> proto::Member {
        serde_json::from_value(serde_json::json!({
            "id": id, "kind": "human", "name": name, "level": "member"
        }))
        .unwrap()
    }

    fn temp_root(name: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!("{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        root
    }

    #[test]
    fn a_single_session_file_becomes_one_active_account() {
        let root = temp_root("session-migrate");
        // The format of every build before several accounts.
        std::fs::write(
            legacy_session_path(&root),
            serde_json::to_vec(&serde_json::json!({
                "serverUrl": "https://team.example.com",
                "member": { "id": "m1", "kind": "human", "name": "Joye", "level": "owner" }
            }))
            .unwrap(),
        )
        .unwrap();
        let accounts = load_accounts(&root);
        assert_eq!(accounts.accounts.len(), 1);
        let active = accounts.active().expect("the old session stays signed in");
        assert_eq!(active.server_url, "https://team.example.com");
        assert_eq!(active.member.id, "m1");
        // Its token is still the one saved under the server URL.
        assert_eq!(active.token_key(), "https://team.example.com");

        // Written again as sessions.json, in the per-server format a build
        // with one session per server reads: `current` and its session.
        save_accounts(&root, &accounts).unwrap();
        assert!(!legacy_session_path(&root).exists());
        #[derive(Deserialize)]
        struct PerServer {
            current: Option<String>,
            sessions: Vec<SavedSession>,
        }
        let file: PerServer =
            serde_json::from_str(&std::fs::read_to_string(sessions_path(&root)).unwrap()).unwrap();
        assert_eq!(file.current.as_deref(), Some("https://team.example.com"));
        assert_eq!(file.sessions[0].member.id, "m1");
        assert_eq!(load_accounts(&root), accounts);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn accounts_keep_their_own_token_keys() {
        let mut a = SavedAccounts::default();
        let prod = a.sign_in(SavedSession::new(
            "https://team.example.com",
            member("m1", "A"),
        ));
        assert_eq!(prod.token_key(), "https://team.example.com");
        let local = a.sign_in(SavedSession::new(
            "http://127.0.0.1:8787",
            member("m9", "L"),
        ));
        assert_eq!(local.token_key(), "http://127.0.0.1:8787");
        // A second member on the same server must not overwrite the first token.
        let other = a.sign_in(SavedSession::new(
            "https://team.example.com",
            member("m2", "B"),
        ));
        assert_eq!(other.token_key(), "https://team.example.com#m2");
        assert_eq!(a.accounts.len(), 3);
        assert_eq!(a.active.as_deref(), Some("https://team.example.com#m2"));
        // Signed in again, an account keeps its key and its place.
        let again = a.sign_in(SavedSession::new(
            "https://team.example.com",
            member("m1", "A2"),
        ));
        assert_eq!(again.token_key(), "https://team.example.com");
        assert_eq!(a.accounts.len(), 3);
        assert_eq!(a.accounts[0].member.name, "A2");
        // Removing the active one leaves none active, the others saved.
        assert!(a.remove(&again.key()).is_some());
        assert_eq!(a.active, None);
        assert_eq!(a.accounts.len(), 2);
    }

    #[test]
    fn no_account_left_keeps_the_server_in_use() {
        let root = temp_root("session-empty");
        let mut a = SavedAccounts::default();
        let one = a.sign_in(SavedSession::new("https://x.dev", member("m1", "A")));
        save_accounts(&root, &a).unwrap();
        a.remove(&one.key());
        save_accounts(&root, &a).unwrap();
        let back = load_accounts(&root);
        assert!(back.accounts.is_empty());
        assert_eq!(back.active, None);
        assert_eq!(back.current.as_deref(), Some("https://x.dev"));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn accounts_switch_and_sign_out_one_at_a_time() {
        use std::time::{Duration, Instant};

        use crate::data::{ChatProvider as _, SessionState};
        use crate::provider::{CoreProvider, ProviderConfig};

        // Production (an old single-session file) and a local server.
        let root = temp_root("accounts-switch");
        let prod = "http://127.0.0.1:9";
        let local = "http://127.0.0.1:10";
        std::fs::write(
            legacy_session_path(&root),
            serde_json::to_vec(&serde_json::json!({
                "serverUrl": prod,
                "member": { "id": "m1", "kind": "human", "name": "Joye", "level": "owner" }
            }))
            .unwrap(),
        )
        .unwrap();
        let mut accounts = load_accounts(&root);
        accounts.sign_in(SavedSession::new(local, member("l1", "Local")));
        // The production account stays the one shown.
        accounts.activate(&account_key(prod, "m1"));
        save_accounts(&root, &accounts).unwrap();
        let tokens = Arc::new(Mutex::new(std::collections::HashMap::from([
            (prod.to_string(), "prod-token".to_string()),
            (local.to_string(), "local-token".to_string()),
        ])));
        let open = || {
            CoreProvider::new(ProviderConfig {
                data_root: root.clone(),
                tokens: TokenStorage::Keyed(tokens.clone()),
                restore: true,
            })
        };
        let me = |p: &CoreProvider| match p.session() {
            SessionState::SignedIn { me, server_url } => Some((server_url, me.id)),
            _ => None,
        };
        let wait = |what: &str, f: &dyn Fn() -> bool| {
            let deadline = Instant::now() + Duration::from_secs(10);
            while !f() {
                assert!(Instant::now() < deadline, "timed out: {what}");
                std::thread::sleep(Duration::from_millis(10));
            }
        };

        let p = open();
        assert_eq!(me(&p), Some((prod.into(), "m1".into())));
        let listed = p.accounts();
        assert_eq!(listed.len(), 2);
        assert!(listed[0].active && !listed[1].active);

        // Switch to the local account and back; both stay signed in.
        p.switch_account(&account_key(local, "l1"));
        assert_eq!(me(&p), Some((local.into(), "l1".into())));
        assert_eq!(
            load_accounts(&root).active.as_deref(),
            Some(account_key(local, "l1").as_str())
        );
        p.switch_account(&account_key(prod, "m1"));
        assert_eq!(me(&p), Some((prod.into(), "m1".into())));
        p.switch_account(&account_key(local, "l1"));

        // 添加账号… shows the login view; 取消 returns to the same account.
        p.add_account();
        assert!(matches!(p.session(), SessionState::SignedOut { .. }));
        assert!(p.adding_account());
        p.cancel_add_account();
        assert_eq!(me(&p), Some((local.into(), "l1".into())));
        assert!(!p.adding_account());

        // Quit and relaunch: both accounts, the last one shown.
        p.sign_out_local_for_selftest();
        drop(p);
        let p = open();
        assert_eq!(me(&p), Some((local.into(), "l1".into())));
        assert_eq!(p.accounts().len(), 2);

        // Signing out of the local account leaves production signed in.
        p.sign_out(false);
        assert_eq!(me(&p), Some((prod.into(), "m1".into())));
        let left = load_accounts(&root);
        assert_eq!(left.accounts.len(), 1);
        assert_eq!(
            left.active.as_deref(),
            Some(account_key(prod, "m1").as_str())
        );
        wait("the local token is deleted", &|| {
            !tokens.lock().unwrap().contains_key(local)
        });
        assert_eq!(
            tokens.lock().unwrap().get(prod).map(String::as_str),
            Some("prod-token")
        );
        p.sign_out_local_for_selftest();
        let _ = std::fs::remove_dir_all(&root);
    }
}
