//! Account sign-in beyond invite and email codes: an optional password, and sign-in by a
//! one-time code that a signed-in device shows or approves (docs/protocol.md, "Password
//! sign-in" and "Code sign-in").

use serde::{Deserialize, Serialize};

use crate::entities::{Member, Millis};
use crate::limits::{Validation, ValidationError, check_chars};
use crate::settings::is_email_address;

fn invalid(field: &'static str, message: &str) -> Validation {
    Err(ValidationError {
        field,
        message: message.into(),
    })
}

/// A new password: 8 to 128 characters, not only spaces.
pub fn check_password(field: &'static str, p: &str) -> Validation {
    check_chars(field, p, 8, 128)?;
    if p.trim().is_empty() {
        return invalid(field, "not only spaces");
    }
    Ok(())
}

fn check_device_name(name: &str) -> Validation {
    check_chars("deviceName", name, 1, 128)
}

/// Reads a typed or scanned sign-in code like an invite code: upper case, dashes and spaces
/// dropped, I/L as 1, O as 0.
pub fn normalize_sign_in_code(code: &str) -> String {
    code.chars()
        .filter(|c| !c.is_whitespace() && *c != '-')
        .map(|c| match c.to_ascii_uppercase() {
            'I' | 'L' => '1',
            'O' => '0',
            c => c,
        })
        .collect()
}

/// `GET`, `PUT /v1/me/password` and `POST /v1/me/password/remove`.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MyPasswordResponse {
    pub set: bool,
    pub updated_at: Option<Millis>,
}

/// `PUT /v1/me/password`: sets or changes my password.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SetPasswordRequest {
    pub password: String,
    /// Needed when a password is set, unless this token signed in within the last 10 minutes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub current_password: Option<String>,
}

impl SetPasswordRequest {
    pub fn validate(&self) -> Validation {
        check_password("password", &self.password)?;
        if let Some(c) = &self.current_password {
            check_chars("currentPassword", c, 1, 128)?;
        }
        Ok(())
    }
}

/// `POST /v1/me/password/remove`.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RemovePasswordRequest {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub current_password: Option<String>,
}

impl RemovePasswordRequest {
    pub fn validate(&self) -> Validation {
        if let Some(c) = &self.current_password {
            check_chars("currentPassword", c, 1, 128)?;
        }
        Ok(())
    }
}

/// `POST /v1/auth/password/sign-in`; answers `RedeemResponse`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PasswordSignInRequest {
    pub email: String,
    pub password: String,
    pub device_name: String,
}

impl PasswordSignInRequest {
    pub fn validate(&self) -> Validation {
        if !is_email_address(&self.email) {
            return invalid("email", "not an email address");
        }
        check_chars("password", &self.password, 1, 128)?;
        check_device_name(&self.device_name)
    }
}

/// `POST /v1/me/qr/offer`: a code that signs a new device in to my account.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QrOfferResponse {
    pub id: String,
    pub code: String,
    pub expires_at: Millis,
}

/// Whether a device used my offered code.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum QrOfferState {
    Waiting,
    Used,
    Expired,
}

/// `GET /v1/me/qr/offers/:id`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QrOfferStatus {
    pub state: QrOfferState,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub device_name: Option<String>,
}

/// `POST /v1/auth/qr/claim`; answers `RedeemResponse`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QrClaimRequest {
    pub code: String,
    pub device_name: String,
}

impl QrClaimRequest {
    pub fn validate(&self) -> Validation {
        check_chars("code", &self.code, 1, 32)?;
        check_device_name(&self.device_name)
    }
}

/// `POST /v1/auth/qr/request`: a new device asks a signed-in one to let it in.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QrRequestRequest {
    pub device_name: String,
}

impl QrRequestRequest {
    pub fn validate(&self) -> Validation {
        check_device_name(&self.device_name)
    }
}

/// The code to show and the secret only this device knows, for polling.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QrRequestResponse {
    pub id: String,
    pub code: String,
    pub secret: String,
    pub expires_at: Millis,
}

/// `POST /v1/auth/qr/poll`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QrPollRequest {
    pub id: String,
    pub secret: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum QrPollState {
    Waiting,
    Approved,
    Expired,
}

/// `approved` carries the new token once; then the request is gone.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QrPollResponse {
    pub state: QrPollState,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub token: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub member: Option<Member>,
}

/// `POST /v1/me/qr/peek` and `POST /v1/me/qr/approve`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QrCodeRequest {
    pub code: String,
}

impl QrCodeRequest {
    pub fn validate(&self) -> Validation {
        check_chars("code", &self.code, 1, 32)
    }
}

/// `POST /v1/me/qr/peek`: which device asks, before I approve it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QrPeekResponse {
    pub device_name: String,
    pub expires_at: Millis,
}

/// What a sign-in QR image holds: `tc-sign-in:<CODE>` with `?server=<base URL>` when the
/// shower knows it. A bare typed code parses too.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SignInLink {
    pub code: String,
    pub server: Option<String>,
}

pub const SIGN_IN_LINK_SCHEME: &str = "tc-sign-in:";

impl SignInLink {
    pub fn to_text(&self) -> String {
        match &self.server {
            Some(s) => format!(
                "{SIGN_IN_LINK_SCHEME}{}?server={}",
                self.code,
                percent_encode(s)
            ),
            None => format!("{SIGN_IN_LINK_SCHEME}{}", self.code),
        }
    }

    /// Reads a scanned or pasted text; `None` when it holds no 8-character code.
    pub fn parse(text: &str) -> Option<SignInLink> {
        let text = text.trim();
        let (code, server) = match text.strip_prefix(SIGN_IN_LINK_SCHEME) {
            Some(rest) => match rest.split_once("?server=") {
                Some((c, s)) => (c, percent_decode(s).filter(|s| !s.is_empty())),
                None => (rest, None),
            },
            None => (text, None),
        };
        let norm = normalize_sign_in_code(code);
        let crockford =
            |c: char| c.is_ascii_digit() || (c.is_ascii_uppercase() && !"ILOU".contains(c));
        if norm.chars().count() != 8 || !norm.chars().all(crockford) {
            return None;
        }
        Some(SignInLink {
            code: format!("{}-{}", &norm[..4], &norm[4..]),
            server,
        })
    }
}

fn percent_encode(s: &str) -> String {
    s.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                (b as char).to_string()
            }
            _ => format!("%{b:02X}"),
        })
        .collect()
}

fn percent_decode(s: &str) -> Option<String> {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' {
            let hex = std::str::from_utf8(b.get(i + 1..i + 3)?).ok()?;
            out.push(u8::from_str_radix(hex, 16).ok()?);
            i += 3;
        } else {
            out.push(b[i]);
            i += 1;
        }
    }
    String::from_utf8(out).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn passwords() {
        assert!(check_password("p", "12345678").is_ok());
        assert!(check_password("p", "1234567").is_err());
        assert!(check_password("p", "        ").is_err());
        assert!(check_password("p", "密码密码密码密码").is_ok());
        assert!(check_password("p", &"x".repeat(129)).is_err());
    }

    #[test]
    fn links_round_trip() {
        let l = SignInLink {
            code: "7K2M-9QPX".into(),
            server: Some("https://chat.example.com/x".into()),
        };
        assert_eq!(SignInLink::parse(&l.to_text()), Some(l));
        assert_eq!(
            SignInLink::parse(" 7k2m 9qpx "),
            Some(SignInLink {
                code: "7K2M-9QPX".into(),
                server: None
            })
        );
        assert_eq!(
            SignInLink::parse("tc-sign-in:h3vd-2wq8").unwrap().code,
            "H3VD-2WQ8"
        );
        assert_eq!(SignInLink::parse("7K2M-9QP").map(|l| l.code), None);
        assert_eq!(SignInLink::parse("hello world"), None);
        assert_eq!(normalize_sign_in_code("io-lo"), "1010");
    }
}
