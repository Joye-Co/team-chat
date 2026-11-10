//! Token storage: the macOS Keychain or the Windows Credential Manager in the app, a
//! mode-600 file for tests and Linux CI.

use std::path::{Path, PathBuf};

#[derive(Debug, thiserror::Error)]
pub enum TokenStoreError {
    #[error("token store io: {0}")]
    Io(#[from] std::io::Error),
    /// The system store failed: the Keychain on macOS, the Credential Manager on Windows.
    #[error("keychain: {0}")]
    Keychain(String),
}

pub trait TokenStore: Send + Sync {
    fn load(&self) -> Result<Option<String>, TokenStoreError>;
    fn save(&self, token: &str) -> Result<(), TokenStoreError>;
    fn clear(&self) -> Result<(), TokenStoreError>;
}

/// Stores the token in one file with mode 600.
#[derive(Debug, Clone)]
pub struct FileTokenStore {
    path: PathBuf,
}

impl FileTokenStore {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        FileTokenStore { path: path.into() }
    }
    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl TokenStore for FileTokenStore {
    fn load(&self) -> Result<Option<String>, TokenStoreError> {
        match std::fs::read_to_string(&self.path) {
            Ok(s) => {
                let t = s.trim();
                Ok((!t.is_empty()).then(|| t.to_string()))
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    fn save(&self, token: &str) -> Result<(), TokenStoreError> {
        use std::io::Write;
        if let Some(dir) = self.path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let tmp = self.path.with_extension("tmp");
        let mut opts = std::fs::OpenOptions::new();
        opts.write(true).create(true).truncate(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            opts.mode(0o600);
        }
        let mut f = opts.open(&tmp)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            f.set_permissions(std::fs::Permissions::from_mode(0o600))?;
        }
        f.write_all(token.as_bytes())?;
        f.sync_all()?;
        std::fs::rename(&tmp, &self.path)?;
        Ok(())
    }

    fn clear(&self) -> Result<(), TokenStoreError> {
        match std::fs::remove_file(&self.path) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(e.into()),
        }
    }
}

/// Stores the token as a generic password in the login Keychain.
/// `service` comes from the bundle id in product.json; `account` names the server.
#[cfg(target_os = "macos")]
#[derive(Debug, Clone)]
pub struct KeychainTokenStore {
    service: String,
    account: String,
}

#[cfg(target_os = "macos")]
impl KeychainTokenStore {
    pub fn new(service: impl Into<String>, account: impl Into<String>) -> Self {
        KeychainTokenStore {
            service: service.into(),
            account: account.into(),
        }
    }
}

#[cfg(target_os = "macos")]
const ERR_SEC_ITEM_NOT_FOUND: i32 = -25300;

#[cfg(target_os = "macos")]
impl TokenStore for KeychainTokenStore {
    fn load(&self) -> Result<Option<String>, TokenStoreError> {
        use security_framework::passwords::get_generic_password;
        match get_generic_password(&self.service, &self.account) {
            Ok(bytes) => {
                Ok(Some(String::from_utf8(bytes).map_err(|_| {
                    TokenStoreError::Keychain("token is not UTF-8".into())
                })?))
            }
            Err(e) if e.code() == ERR_SEC_ITEM_NOT_FOUND => Ok(None),
            Err(e) => Err(TokenStoreError::Keychain(e.to_string())),
        }
    }

    fn save(&self, token: &str) -> Result<(), TokenStoreError> {
        security_framework::passwords::set_generic_password(
            &self.service,
            &self.account,
            token.as_bytes(),
        )
        .map_err(|e| TokenStoreError::Keychain(e.to_string()))
    }

    fn clear(&self) -> Result<(), TokenStoreError> {
        use security_framework::passwords::delete_generic_password;
        match delete_generic_password(&self.service, &self.account) {
            Ok(()) => Ok(()),
            Err(e) if e.code() == ERR_SEC_ITEM_NOT_FOUND => Ok(()),
            Err(e) => Err(TokenStoreError::Keychain(e.to_string())),
        }
    }
}

/// The Credential Manager target of a token: the same service and account pair as the
/// Keychain item on macOS.
pub fn credential_target(service: &str, account: &str) -> String {
    format!("{service}:{account}")
}

/// Stores the token as a generic credential in the Windows Credential Manager (the
/// signed-in user's vault, encrypted with their logon). `service` comes from the bundle id
/// in product.json; `account` names the server.
#[cfg(target_os = "windows")]
#[derive(Debug, Clone)]
pub struct CredentialTokenStore {
    target: String,
    account: String,
}

#[cfg(target_os = "windows")]
impl CredentialTokenStore {
    pub fn new(service: impl AsRef<str>, account: impl Into<String>) -> Self {
        let account = account.into();
        CredentialTokenStore {
            target: credential_target(service.as_ref(), &account),
            account,
        }
    }

    fn target_wide(&self) -> Vec<u16> {
        wide(&self.target)
    }
}

#[cfg(target_os = "windows")]
fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

/// The error text carries the Win32 code only, never the credential.
#[cfg(target_os = "windows")]
fn credential_error(what: &str, e: &windows::core::Error) -> TokenStoreError {
    TokenStoreError::Keychain(format!("{what}: {:#010x}", e.code().0))
}

#[cfg(target_os = "windows")]
impl TokenStore for CredentialTokenStore {
    fn load(&self) -> Result<Option<String>, TokenStoreError> {
        use windows::{
            Win32::{
                Foundation::ERROR_NOT_FOUND,
                Security::Credentials::{CRED_TYPE_GENERIC, CREDENTIALW, CredFree, CredReadW},
            },
            core::PCWSTR,
        };
        let target = self.target_wide();
        let mut found: *mut CREDENTIALW = std::ptr::null_mut();
        // SAFETY: `target` is a NUL-terminated string that outlives the call; on success
        // `found` points to one credential that CredFree releases below.
        let read =
            unsafe { CredReadW(PCWSTR(target.as_ptr()), CRED_TYPE_GENERIC, None, &mut found) };
        match read {
            Ok(()) => {}
            Err(e) if e.code() == ERROR_NOT_FOUND.to_hresult() => return Ok(None),
            Err(e) => return Err(credential_error("read", &e)),
        }
        // SAFETY: CredReadW succeeded, so `found` is valid and its blob has `CredentialBlobSize`
        // bytes; the bytes are copied before the credential is freed.
        let bytes = unsafe {
            let cred = &*found;
            let bytes = if cred.CredentialBlob.is_null() {
                Vec::new()
            } else {
                std::slice::from_raw_parts(cred.CredentialBlob, cred.CredentialBlobSize as usize)
                    .to_vec()
            };
            CredFree(found.cast());
            bytes
        };
        let token = String::from_utf8(bytes)
            .map_err(|_| TokenStoreError::Keychain("token is not UTF-8".into()))?;
        Ok((!token.is_empty()).then_some(token))
    }

    fn save(&self, token: &str) -> Result<(), TokenStoreError> {
        use windows::{
            Win32::Security::Credentials::{
                CRED_FLAGS, CRED_PERSIST_LOCAL_MACHINE, CRED_TYPE_GENERIC, CREDENTIALW, CredWriteW,
            },
            core::PWSTR,
        };
        let mut target = self.target_wide();
        let mut account = wide(&self.account);
        let mut blob = token.as_bytes().to_vec();
        let cred = CREDENTIALW {
            Flags: CRED_FLAGS(0),
            Type: CRED_TYPE_GENERIC,
            TargetName: PWSTR(target.as_mut_ptr()),
            CredentialBlobSize: blob.len() as u32,
            CredentialBlob: blob.as_mut_ptr(),
            // This computer only: the token does not roam with a domain profile.
            Persist: CRED_PERSIST_LOCAL_MACHINE,
            UserName: PWSTR(account.as_mut_ptr()),
            ..Default::default()
        };
        // SAFETY: every pointer in `cred` is valid for the call, which copies the data.
        unsafe { CredWriteW(&cred, 0) }.map_err(|e| credential_error("write", &e))
    }

    fn clear(&self) -> Result<(), TokenStoreError> {
        use windows::{
            Win32::{
                Foundation::ERROR_NOT_FOUND,
                Security::Credentials::{CRED_TYPE_GENERIC, CredDeleteW},
            },
            core::PCWSTR,
        };
        let target = self.target_wide();
        // SAFETY: `target` is a NUL-terminated string that outlives the call.
        match unsafe { CredDeleteW(PCWSTR(target.as_ptr()), CRED_TYPE_GENERIC, None) } {
            Ok(()) => Ok(()),
            Err(e) if e.code() == ERROR_NOT_FOUND.to_hresult() => Ok(()),
            Err(e) => Err(credential_error("delete", &e)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn credential_targets_keep_service_and_account_apart() {
        assert_eq!(
            credential_target("app.id", "https://chat.example"),
            "app.id:https://chat.example"
        );
        assert_ne!(
            credential_target("app.id", "https://a.example"),
            credential_target("app.id", "https://b.example")
        );
    }

    /// Runs against the real Credential Manager of the signed-in user (Windows CI).
    #[cfg(target_os = "windows")]
    #[test]
    fn credential_store_round_trip() {
        let account = format!("https://test.invalid/{}", crate::new_id());
        let s = CredentialTokenStore::new("chat-core.test", account.clone());
        assert_eq!(s.load().unwrap(), None);
        s.save("tc_test_value").unwrap();
        assert_eq!(s.load().unwrap().as_deref(), Some("tc_test_value"));
        // A second store of the same pair reads it; another account does not.
        let again = CredentialTokenStore::new("chat-core.test", account.clone());
        assert_eq!(again.load().unwrap().as_deref(), Some("tc_test_value"));
        let other = CredentialTokenStore::new("chat-core.test", format!("{account}/other"));
        assert_eq!(other.load().unwrap(), None);
        s.save("tc_test_value_2").unwrap();
        assert_eq!(s.load().unwrap().as_deref(), Some("tc_test_value_2"));
        s.clear().unwrap();
        s.clear().unwrap();
        assert_eq!(s.load().unwrap(), None);
    }

    #[test]
    fn file_store_round_trip_with_mode_600() {
        let dir = tempfile::tempdir().unwrap();
        let s = FileTokenStore::new(dir.path().join("sub/token"));
        assert_eq!(s.load().unwrap(), None);
        s.save("tc_test_value").unwrap();
        assert_eq!(s.load().unwrap().as_deref(), Some("tc_test_value"));
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(s.path()).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode, 0o600);
        }
        s.clear().unwrap();
        s.clear().unwrap();
        assert_eq!(s.load().unwrap(), None);
    }
}
