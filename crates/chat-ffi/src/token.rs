//! The sign-in token in the Keychain: the item `service` + `account` (the bundle id and the
//! server address). `access_group` shares it with the app's extensions and needs the matching
//! entitlement.

use crate::CoreError;

#[cfg(any(target_os = "macos", target_os = "ios"))]
fn store(
    service: String,
    account: String,
    access_group: Option<String>,
) -> Result<Box<dyn chat_core::TokenStore>, CoreError> {
    let store = chat_core::KeychainTokenStore::new(service, account);
    Ok(Box::new(match access_group {
        Some(group) => store.with_access_group(group),
        None => store,
    }))
}

#[cfg(not(any(target_os = "macos", target_os = "ios")))]
fn store(
    _service: String,
    _account: String,
    _access_group: Option<String>,
) -> Result<Box<dyn chat_core::TokenStore>, CoreError> {
    Err(CoreError::Failed {
        message: "no keychain on this platform".into(),
    })
}

#[uniffi::export]
pub fn save_token(
    service: String,
    account: String,
    access_group: Option<String>,
    token: String,
) -> Result<(), CoreError> {
    Ok(store(service, account, access_group)?.save(&token)?)
}

#[uniffi::export]
pub fn load_token(
    service: String,
    account: String,
    access_group: Option<String>,
) -> Result<Option<String>, CoreError> {
    Ok(store(service, account, access_group)?.load()?)
}

#[uniffi::export]
pub fn clear_token(
    service: String,
    account: String,
    access_group: Option<String>,
) -> Result<(), CoreError> {
    Ok(store(service, account, access_group)?.clear()?)
}
