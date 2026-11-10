//! Token storage: the Keychain in the apps (macOS and iOS), a mode-600 file for tests and Linux CI.

use std::path::{Path, PathBuf};

#[derive(Debug, thiserror::Error)]
pub enum TokenStoreError {
    #[error("token store io: {0}")]
    Io(#[from] std::io::Error),
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

/// Stores the token as a generic password in the Keychain (the login Keychain on macOS, the
/// app's Keychain on iOS). `service` comes from the bundle id in product.json; `account` names
/// the server.
#[cfg(any(target_os = "macos", target_os = "ios"))]
#[derive(Debug, Clone)]
pub struct KeychainTokenStore {
    service: String,
    account: String,
    access_group: Option<String>,
}

#[cfg(any(target_os = "macos", target_os = "ios"))]
impl KeychainTokenStore {
    pub fn new(service: impl Into<String>, account: impl Into<String>) -> Self {
        KeychainTokenStore {
            service: service.into(),
            account: account.into(),
            access_group: None,
        }
    }

    /// Shares the item with the app's extensions. The app must be signed with this group in its
    /// entitlements, or every call fails.
    pub fn with_access_group(mut self, group: impl Into<String>) -> Self {
        self.access_group = Some(group.into());
        self
    }

    fn options(&self) -> security_framework::passwords_options::PasswordOptions {
        let mut o = security_framework::passwords_options::PasswordOptions::new_generic_password(
            &self.service,
            &self.account,
        );
        if let Some(group) = &self.access_group {
            o.set_access_group(group);
        }
        o
    }
}

#[cfg(any(target_os = "macos", target_os = "ios"))]
const ERR_SEC_ITEM_NOT_FOUND: i32 = -25300;

#[cfg(any(target_os = "macos", target_os = "ios"))]
impl TokenStore for KeychainTokenStore {
    fn load(&self) -> Result<Option<String>, TokenStoreError> {
        match security_framework::passwords::generic_password(self.options()) {
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
        security_framework::passwords::set_generic_password_options(
            token.as_bytes(),
            self.options(),
        )
        .map_err(|e| TokenStoreError::Keychain(e.to_string()))
    }

    fn clear(&self) -> Result<(), TokenStoreError> {
        match security_framework::passwords::delete_generic_password_options(self.options()) {
            Ok(()) => Ok(()),
            Err(e) if e.code() == ERR_SEC_ITEM_NOT_FOUND => Ok(()),
            Err(e) => Err(TokenStoreError::Keychain(e.to_string())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
