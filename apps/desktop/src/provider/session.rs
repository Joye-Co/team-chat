//! Where the provider keeps its files, the saved session, token storage and
//! the sign-in calls.
//!
//! Layout under the data root (`~/Library/Application Support/<bundle id>`):
//! - `session.json`: the signed-in server and member (no secret).
//! - `accounts/<server>/<member id>/cache.sqlite3`: the chat-core cache of one
//!   account, so two accounts never share a cache.
//! - `logs/`: chat-core and app logs (secrets are never logged).
//!
//! The token lives in the macOS Keychain (service = bundle id, account =
//! server URL). Self-tests use a memory store.

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
}

impl TokenStorage {
    pub fn memory() -> Self {
        TokenStorage::Memory(Arc::new(Mutex::new(None)))
    }

    pub fn store(&self, server_url: &str) -> Box<dyn TokenStore> {
        match self {
            #[cfg(target_os = "macos")]
            TokenStorage::Keychain => Box::new(chat_core::KeychainTokenStore::new(
                env!("PRODUCT_BUNDLE_ID"),
                server_url,
            )),
            #[cfg(not(target_os = "macos"))]
            TokenStorage::Keychain => Box::new(chat_core::FileTokenStore::new(
                default_data_root().join("token"),
            )),
            TokenStorage::Memory(slot) => Box::new(MemoryTokenStore(slot.clone())),
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

/// `~/Library/Application Support/<bundle id>`.
pub fn default_data_root() -> PathBuf {
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir);
    home.join("Library/Application Support")
        .join(env!("PRODUCT_BUNDLE_ID"))
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

/// What `session.json` holds. No secret.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SavedSession {
    pub server_url: String,
    /// The member as the server last described it.
    pub member: proto::Member,
}

pub fn session_path(root: &Path) -> PathBuf {
    root.join("session.json")
}

pub fn load_session(root: &Path) -> Option<SavedSession> {
    let text = std::fs::read_to_string(session_path(root)).ok()?;
    serde_json::from_str(&text).ok()
}

pub fn save_session(root: &Path, session: &SavedSession) -> std::io::Result<()> {
    std::fs::create_dir_all(root)?;
    let path = session_path(root);
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, serde_json::to_vec_pretty(session)?)?;
    std::fs::rename(tmp, path)
}

pub fn clear_session(root: &Path) {
    let _ = std::fs::remove_file(session_path(root));
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
}
