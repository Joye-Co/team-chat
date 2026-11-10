//! Sign-in calls that need no token yet. They wait for the server, so call them off the main
//! thread.

use crate::{CoreError, anonymous};

/// A new sign-in: keep `token` in the Keychain ([`crate::save_token`]).
#[derive(Debug, Clone, uniffi::Record)]
pub struct SignedIn {
    pub token: String,
    pub member_name: String,
}

/// Asks the server to send a 6-digit code to `email`. Returns when the code expires
/// (milliseconds since 1970). The answer is the same whether or not the address belongs to a
/// member.
#[uniffi::export]
pub fn request_email_code(base_url: String, email: String) -> Result<u64, CoreError> {
    let http = anonymous(&base_url)?;
    let sent = chat_core::client::background()
        .block_on(async move { http.email_sign_in_code(&email).await })?;
    Ok(sent.expires_at)
}

/// Signs in with the code sent to `email`. A wrong, used or expired code is
/// [`CoreError::Unauthorized`].
#[uniffi::export]
pub fn email_sign_in(
    base_url: String,
    email: String,
    code: String,
    device_name: String,
) -> Result<SignedIn, CoreError> {
    let http = anonymous(&base_url)?;
    let signed = chat_core::client::background()
        .block_on(async move { http.email_sign_in(&email, &code, &device_name).await })?;
    Ok(SignedIn {
        token: signed.token,
        member_name: signed.member.name,
    })
}
