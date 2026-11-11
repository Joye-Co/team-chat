//! Code sign-in for a phone (docs/protocol.md, "Code sign-in"): 账号 › 在手机上登录 shows a
//! code the phone scans, or approves the code a phone shows.

use chat_core::{
    ApiError,
    proto::{ErrorCode, QrOfferState, SignInLink},
};

use super::CoreProvider;
use crate::data::{EmailError, EmailReply, PhoneOffer, PhoneOfferState};

/// A wrong, used or expired code is [`EmailError::WrongCode`].
fn code_error(e: &ApiError) -> EmailError {
    match e {
        ApiError::Server {
            code: ErrorCode::NotFound | ErrorCode::Unauthorized,
            ..
        }
        | ApiError::Invalid(_) => EmailError::WrongCode,
        other => super::email::email_error(other, false),
    }
}

impl CoreProvider {
    pub(super) fn offer_phone_sign_in_inner(&self) -> EmailReply<PhoneOffer> {
        self.email_request(|client| async move {
            let http = client.http();
            let base = http.base().to_string();
            let r = http.qr_offer().await.map_err(|e| code_error(&e))?;
            Ok(PhoneOffer {
                qr_text: SignInLink {
                    code: r.code.clone(),
                    server: Some(base),
                }
                .to_text(),
                id: r.id,
                code: r.code,
                expires_at: r.expires_at,
            })
        })
    }

    pub(super) fn phone_offer_state_inner(&self, id: &str) -> EmailReply<PhoneOfferState> {
        let id = id.to_string();
        self.email_request(move |client| async move {
            let r = client
                .http()
                .qr_offer_status(&id)
                .await
                .map_err(|e| code_error(&e))?;
            Ok(match r.state {
                QrOfferState::Waiting => PhoneOfferState::Waiting,
                QrOfferState::Used => PhoneOfferState::Used {
                    device_name: r.device_name.unwrap_or_default(),
                },
                QrOfferState::Expired => PhoneOfferState::Expired,
            })
        })
    }

    pub(super) fn peek_phone_code_inner(&self, code: &str) -> EmailReply<String> {
        let code = code.to_string();
        self.email_request(move |client| async move {
            let r = client
                .http()
                .qr_peek(&code)
                .await
                .map_err(|e| code_error(&e))?;
            Ok(r.device_name)
        })
    }

    pub(super) fn approve_phone_code_inner(&self, code: &str) -> EmailReply<()> {
        let code = code.to_string();
        self.email_request(move |client| async move {
            client
                .http()
                .qr_approve(&code)
                .await
                .map_err(|e| code_error(&e))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wrong_codes_read_as_wrong_code() {
        let server = |code| ApiError::Server {
            status: 404,
            code,
            message: String::new(),
        };
        assert_eq!(
            code_error(&server(ErrorCode::NotFound)),
            EmailError::WrongCode
        );
        assert_eq!(
            code_error(&server(ErrorCode::RateLimited)),
            EmailError::RateLimited
        );
        assert_eq!(
            code_error(&ApiError::Network("x".into())),
            EmailError::Offline
        );
    }
}
