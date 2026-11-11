//! Password and one-time code sign-in (docs/protocol.md, "Password sign-in" and "Code
//! sign-in"), over HTTP. The `auth/*` calls need no token.

use reqwest::Method;

use crate::http::{ApiError, ApiResult, HttpClient, enc};
use crate::proto::{
    MyPasswordResponse, PasswordSignInRequest, QrClaimRequest, QrCodeRequest, QrOfferResponse,
    QrOfferStatus, QrPeekResponse, QrPollRequest, QrPollResponse, QrRequestRequest,
    QrRequestResponse, RedeemResponse, RemovePasswordRequest, SetPasswordRequest,
};

fn invalid(e: crate::proto::ValidationError) -> ApiError {
    ApiError::Invalid(e.to_string())
}

impl HttpClient {
    pub async fn my_password(&self) -> ApiResult<MyPasswordResponse> {
        self.call::<(), _>(Method::GET, "/v1/me/password", None, true)
            .await
    }

    /// Sets or changes my password. `current` is needed when one is set, unless this device
    /// signed in within the last 10 minutes.
    pub async fn set_password(
        &self,
        password: &str,
        current: Option<&str>,
    ) -> ApiResult<MyPasswordResponse> {
        let req = SetPasswordRequest {
            password: password.to_string(),
            current_password: current.filter(|c| !c.is_empty()).map(str::to_string),
        };
        req.validate().map_err(invalid)?;
        self.call(Method::PUT, "/v1/me/password", Some(&req), true)
            .await
    }

    pub async fn remove_password(&self, current: Option<&str>) -> ApiResult<MyPasswordResponse> {
        let req = RemovePasswordRequest {
            current_password: current.filter(|c| !c.is_empty()).map(str::to_string),
        };
        req.validate().map_err(invalid)?;
        self.call(Method::POST, "/v1/me/password/remove", Some(&req), true)
            .await
    }

    pub async fn password_sign_in(
        &self,
        email: &str,
        password: &str,
        device_name: &str,
    ) -> ApiResult<RedeemResponse> {
        let req = PasswordSignInRequest {
            email: email.trim().to_lowercase(),
            password: password.to_string(),
            device_name: device_name.to_string(),
        };
        req.validate().map_err(invalid)?;
        self.call(Method::POST, "/v1/auth/password/sign-in", Some(&req), false)
            .await
    }

    /// A code that signs a new device in to my account (shown as a QR image).
    pub async fn qr_offer(&self) -> ApiResult<QrOfferResponse> {
        self.call(
            Method::POST,
            "/v1/me/qr/offer",
            Some(&serde_json::json!({})),
            true,
        )
        .await
    }

    pub async fn qr_offer_status(&self, id: &str) -> ApiResult<QrOfferStatus> {
        self.call::<(), _>(
            Method::GET,
            &format!("/v1/me/qr/offers/{}", enc(id)),
            None,
            true,
        )
        .await
    }

    /// Signs in with a code a signed-in device shows.
    pub async fn qr_claim(&self, code: &str, device_name: &str) -> ApiResult<RedeemResponse> {
        let req = QrClaimRequest {
            code: code.trim().to_string(),
            device_name: device_name.to_string(),
        };
        req.validate().map_err(invalid)?;
        self.call(Method::POST, "/v1/auth/qr/claim", Some(&req), false)
            .await
    }

    /// Asks for a code to show, which a signed-in device then approves.
    pub async fn qr_request(&self, device_name: &str) -> ApiResult<QrRequestResponse> {
        let req = QrRequestRequest {
            device_name: device_name.to_string(),
        };
        req.validate().map_err(invalid)?;
        self.call(Method::POST, "/v1/auth/qr/request", Some(&req), false)
            .await
    }

    pub async fn qr_poll(&self, id: &str, secret: &str) -> ApiResult<QrPollResponse> {
        let req = QrPollRequest {
            id: id.to_string(),
            secret: secret.to_string(),
        };
        self.call(Method::POST, "/v1/auth/qr/poll", Some(&req), false)
            .await
    }

    /// Which device shows `code`, before approving it.
    pub async fn qr_peek(&self, code: &str) -> ApiResult<QrPeekResponse> {
        let req = QrCodeRequest {
            code: code.trim().to_string(),
        };
        req.validate().map_err(invalid)?;
        self.call(Method::POST, "/v1/me/qr/peek", Some(&req), true)
            .await
    }

    /// Lets the device that shows `code` in to my account.
    pub async fn qr_approve(&self, code: &str) -> ApiResult<()> {
        let req = QrCodeRequest {
            code: code.trim().to_string(),
        };
        req.validate().map_err(invalid)?;
        let _: serde_json::Value = self
            .call(Method::POST, "/v1/me/qr/approve", Some(&req), true)
            .await?;
        Ok(())
    }
}
