//! Sign-in calls that need no token yet. They wait for the server, so call them off the main
//! thread.

use chat_core::proto::{QrPollState, RedeemResponse, SignInLink};

use crate::{CoreError, anonymous};

/// A new sign-in: keep `token` in the Keychain ([`crate::save_token`]).
#[derive(Debug, Clone, uniffi::Record)]
pub struct SignedIn {
    pub token: String,
    pub member_name: String,
}

impl From<RedeemResponse> for SignedIn {
    fn from(r: RedeemResponse) -> Self {
        SignedIn {
            token: r.token,
            member_name: r.member.name,
        }
    }
}

fn block_on<T>(f: impl std::future::Future<Output = T>) -> T {
    chat_core::client::background().block_on(f)
}

/// Asks the server to send a 6-digit code to `email`. Returns when the code expires
/// (milliseconds since 1970). The answer is the same whether or not the address belongs to a
/// member.
#[uniffi::export]
pub fn request_email_code(base_url: String, email: String) -> Result<u64, CoreError> {
    let http = anonymous(&base_url)?;
    let sent = block_on(async move { http.email_sign_in_code(&email).await })?;
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
    let signed = block_on(async move { http.email_sign_in(&email, &code, &device_name).await })?;
    Ok(signed.into())
}

/// Signs in with the sign-in email and its password. A wrong address or password is
/// [`CoreError::Unauthorized`]; too many tries is [`CoreError::RateLimited`].
#[uniffi::export]
pub fn password_sign_in(
    base_url: String,
    email: String,
    password: String,
    device_name: String,
) -> Result<SignedIn, CoreError> {
    let http = anonymous(&base_url)?;
    let signed =
        block_on(async move { http.password_sign_in(&email, &password, &device_name).await })?;
    Ok(signed.into())
}

/// Joins with an invite code (or signs in with a sign-in code an owner made).
#[uniffi::export]
pub fn redeem_invite(
    base_url: String,
    invite_code: String,
    device_name: String,
) -> Result<SignedIn, CoreError> {
    let http = anonymous(&base_url)?;
    let code = invite_code.trim().to_string();
    let signed = block_on(async move { http.redeem(&code, &device_name).await })?;
    Ok(signed.into())
}

/// What a scanned or pasted sign-in code holds.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct SignInCode {
    /// `XXXX-XXXX`.
    pub code: String,
    /// The server the showing device uses, when the QR image carries it.
    pub server: Option<String>,
}

/// Reads a scanned QR text or a typed code; `None` when it is not a sign-in code.
#[uniffi::export]
pub fn parse_sign_in_code(text: String) -> Option<SignInCode> {
    SignInLink::parse(&text).map(|l| SignInCode {
        code: l.code,
        server: l.server,
    })
}

/// The text to draw as a QR image for `code` on `server`.
#[uniffi::export]
pub fn sign_in_qr_text(code: String, server: String) -> String {
    SignInLink {
        code,
        server: (!server.is_empty()).then_some(server),
    }
    .to_text()
}

/// Signs in with the code a signed-in device shows. A wrong, used or expired code is
/// [`CoreError::Unauthorized`].
#[uniffi::export]
pub fn qr_claim(
    base_url: String,
    code: String,
    device_name: String,
) -> Result<SignedIn, CoreError> {
    let http = anonymous(&base_url)?;
    let signed = block_on(async move { http.qr_claim(&code, &device_name).await })?;
    Ok(signed.into())
}

/// A code for this device to show; a signed-in device approves it.
#[derive(Debug, Clone, uniffi::Record)]
pub struct SignInRequest {
    pub id: String,
    /// `XXXX-XXXX`, to show as text.
    pub code: String,
    /// Only this device knows it; pass it to [`qr_poll`].
    pub secret: String,
    /// The text of the QR image.
    pub qr_text: String,
    pub expires_at: u64,
}

#[uniffi::export]
pub fn qr_request(base_url: String, device_name: String) -> Result<SignInRequest, CoreError> {
    let http = anonymous(&base_url)?;
    let r = block_on(async move { http.qr_request(&device_name).await })?;
    Ok(SignInRequest {
        qr_text: sign_in_qr_text(r.code.clone(), base_url),
        id: r.id,
        code: r.code,
        secret: r.secret,
        expires_at: r.expires_at,
    })
}

/// Where a [`SignInRequest`] stands.
#[derive(Debug, Clone, uniffi::Enum)]
pub enum SignInPoll {
    Waiting,
    Expired,
    Approved { signed_in: SignedIn },
}

/// Asks once whether the request was approved. Call every 2 seconds while the code shows.
#[uniffi::export]
pub fn qr_poll(base_url: String, id: String, secret: String) -> Result<SignInPoll, CoreError> {
    let http = anonymous(&base_url)?;
    let r = block_on(async move { http.qr_poll(&id, &secret).await })?;
    Ok(match (r.state, r.token, r.member) {
        (QrPollState::Approved, Some(token), Some(member)) => SignInPoll::Approved {
            signed_in: SignedIn {
                token,
                member_name: member.name,
            },
        },
        (QrPollState::Waiting, ..) => SignInPoll::Waiting,
        _ => SignInPoll::Expired,
    })
}
