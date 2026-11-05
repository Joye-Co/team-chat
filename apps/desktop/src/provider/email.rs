//! Email sign-in (docs/protocol.md, "Email sign-in"): the code for 登录 ›
//! 用邮箱登录, and my sign-in email in 账号 › 登录邮箱.

use std::{future::Future, sync::Arc};

use chat_core::{ApiError, Client, HttpClient, proto::ErrorCode};

use super::{CoreProvider, session};
use crate::data::{EmailError, EmailReply, email_reply_now};

/// What the server's refusal means for an email request. The client checks
/// the address before it sends a code request, so the server's `invalid`
/// there means `email.not_configured`: `code_request` says which call it was.
pub(super) fn email_error(e: &ApiError, code_request: bool) -> EmailError {
    match e {
        ApiError::Network(_) | ApiError::NoToken | ApiError::BadUrl => EmailError::Offline,
        ApiError::Invalid(_) => EmailError::BadAddress,
        ApiError::Server { code, .. } => match code {
            ErrorCode::Unauthorized => EmailError::WrongCode,
            ErrorCode::Conflict => EmailError::Taken,
            ErrorCode::RateLimited => EmailError::RateLimited,
            ErrorCode::Invalid if code_request => EmailError::NotSetUp,
            ErrorCode::Invalid => EmailError::BadAddress,
            _ => EmailError::Other,
        },
        ApiError::Unexpected { .. } | ApiError::Decode(_) => EmailError::Offline,
        _ => EmailError::Other,
    }
}

impl CoreProvider {
    fn email_request<T, F>(&self, work: impl FnOnce(Arc<Client>) -> F) -> EmailReply<T>
    where
        T: Send + 'static,
        F: Future<Output = Result<T, EmailError>> + Send + 'static,
    {
        let Some(a) = self.account() else {
            return email_reply_now(Err(EmailError::Offline));
        };
        let (tx, rx) = futures::channel::oneshot::channel();
        let fut = work(a.client.clone());
        a.client.spawn(async move {
            let _ = tx.send(fut.await);
        });
        rx
    }

    pub(super) fn request_sign_in_code_inner(&self, server_url: &str, email: &str) -> EmailReply<()> {
        let Some(server_url) = session::normalize_server_url(server_url) else {
            return email_reply_now(Err(EmailError::Offline));
        };
        let email = email.to_string();
        let (tx, rx) = futures::channel::oneshot::channel();
        chat_core::background().spawn(async move {
            let result = async {
                let http =
                    HttpClient::new(&server_url, None).map_err(|e| email_error(&e, true))?;
                http.email_sign_in_code(&email)
                    .await
                    .map_err(|e| email_error(&e, true))?;
                Ok(())
            }
            .await;
            let _ = tx.send(result);
        });
        rx
    }

    pub(super) fn my_email_inner(&self) -> EmailReply<Option<String>> {
        self.email_request(|client| async move {
            let r = client.http().my_email().await.map_err(|e| email_error(&e, false))?;
            Ok(r.email)
        })
    }

    pub(super) fn add_email_code_inner(&self, email: &str) -> EmailReply<()> {
        let email = email.to_string();
        self.email_request(move |client| async move {
            client
                .http()
                .add_email_code(&email)
                .await
                .map_err(|e| email_error(&e, true))?;
            Ok(())
        })
    }

    pub(super) fn verify_email_inner(&self, email: &str, code: &str) -> EmailReply<String> {
        let (email, code) = (email.to_string(), code.to_string());
        self.email_request(move |client| async move {
            let r = client
                .http()
                .verify_email(&email, &code)
                .await
                .map_err(|e| email_error(&e, false))?;
            r.email.ok_or(EmailError::Other)
        })
    }

    pub(super) fn remove_email_inner(&self) -> EmailReply<()> {
        self.email_request(|client| async move {
            client.http().remove_email().await.map_err(|e| email_error(&e, false))?;
            Ok(())
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn refusals_map_to_email_errors() {
        let server = |code| ApiError::Server {
            status: 400,
            code,
            message: String::new(),
        };
        let e = |code, code_request| email_error(&server(code), code_request);
        assert_eq!(e(ErrorCode::Unauthorized, false), EmailError::WrongCode);
        assert_eq!(e(ErrorCode::Conflict, true), EmailError::Taken);
        assert_eq!(e(ErrorCode::Invalid, true), EmailError::NotSetUp);
        assert_eq!(e(ErrorCode::Invalid, false), EmailError::BadAddress);
        assert_eq!(
            email_error(&ApiError::Network("x".into()), true),
            EmailError::Offline
        );
    }
}
