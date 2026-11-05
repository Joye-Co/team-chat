//! Settings (docs/protocol.md, "Settings"): notification settings, the team's approval rules
//! for AI members, the team's AI 服务 and my signed-in devices, over HTTP.

use reqwest::Method;

use crate::client::{Client, ClientError};
use crate::http::{ApiError, ApiResult, HttpClient, enc};
use crate::proto::{
    AiServiceConfig, AiServiceTestResult, ApprovalPolicy, DevicesResponse, NotifySettings,
    RevokeTokensResponse, UpdateAiServiceRequest, UpdateApprovalPolicyRequest,
    UpdateNotifySettingsRequest,
};

type Handle<T> = tokio::task::JoinHandle<Result<T, ClientError>>;

fn invalid(e: crate::proto::ValidationError) -> ApiError {
    ApiError::Invalid(e.to_string())
}

impl HttpClient {
    pub async fn notify_settings(&self) -> ApiResult<NotifySettings> {
        self.call::<(), _>(Method::GET, "/v1/me/notify-settings", None, true)
            .await
    }

    pub async fn update_notify_settings(
        &self,
        req: &UpdateNotifySettingsRequest,
    ) -> ApiResult<NotifySettings> {
        req.validate().map_err(invalid)?;
        self.call(Method::PATCH, "/v1/me/notify-settings", Some(req), true)
            .await
    }

    /// The team's approval rules for AI members (any member may read them).
    pub async fn approval_policy(&self) -> ApiResult<ApprovalPolicy> {
        self.call::<(), _>(Method::GET, "/v1/settings/approvals", None, true)
            .await
    }

    /// Owner only; others get `Forbidden`.
    pub async fn update_approval_policy(
        &self,
        req: &UpdateApprovalPolicyRequest,
    ) -> ApiResult<ApprovalPolicy> {
        req.validate().map_err(invalid)?;
        self.call(Method::PATCH, "/v1/settings/approvals", Some(req), true)
            .await
    }

    /// The team's AI 服务, without the key. Owner only; others get `Forbidden`.
    pub async fn ai_service(&self) -> ApiResult<AiServiceConfig> {
        self.call::<(), _>(Method::GET, "/v1/settings/ai", None, true)
            .await
    }

    /// Saves the team's AI 服务 (owner only). Without `api_key` the saved key stays.
    pub async fn update_ai_service(
        &self,
        req: &UpdateAiServiceRequest,
    ) -> ApiResult<AiServiceConfig> {
        req.validate().map_err(invalid)?;
        self.call(Method::PUT, "/v1/settings/ai", Some(req), true)
            .await
    }

    /// 测试连接: one tiny call with these values; nothing is saved (owner only).
    pub async fn test_ai_service(
        &self,
        req: &UpdateAiServiceRequest,
    ) -> ApiResult<AiServiceTestResult> {
        req.validate().map_err(invalid)?;
        self.call(Method::POST, "/v1/settings/ai/test", Some(req), true)
            .await
    }

    pub async fn devices(&self) -> ApiResult<DevicesResponse> {
        self.call::<(), _>(Method::GET, "/v1/me/devices", None, true)
            .await
    }

    /// Signs out one of my devices (revokes its token). Returns how many tokens were revoked.
    pub async fn sign_out_device(&self, device_id: &str) -> ApiResult<u64> {
        let r: RevokeTokensResponse = self
            .call::<(), _>(
                Method::DELETE,
                &format!("/v1/me/devices/{}", enc(device_id)),
                None,
                true,
            )
            .await?;
        Ok(r.revoked)
    }
}

impl Client {
    pub fn notify_settings(&self) -> Handle<NotifySettings> {
        let http = self.http();
        self.spawn(async move { Ok(http.notify_settings().await?) })
    }

    pub fn update_notify_settings(
        &self,
        req: UpdateNotifySettingsRequest,
    ) -> Handle<NotifySettings> {
        let http = self.http();
        self.spawn(async move { Ok(http.update_notify_settings(&req).await?) })
    }

    pub fn approval_policy(&self) -> Handle<ApprovalPolicy> {
        let http = self.http();
        self.spawn(async move { Ok(http.approval_policy().await?) })
    }

    pub fn update_approval_policy(
        &self,
        req: UpdateApprovalPolicyRequest,
    ) -> Handle<ApprovalPolicy> {
        let http = self.http();
        self.spawn(async move { Ok(http.update_approval_policy(&req).await?) })
    }

    pub fn devices(&self) -> Handle<DevicesResponse> {
        let http = self.http();
        self.spawn(async move { Ok(http.devices().await?) })
    }

    pub fn sign_out_device(&self, device_id: &str) -> Handle<u64> {
        let http = self.http();
        let id = device_id.to_string();
        self.spawn(async move { Ok(http.sign_out_device(&id).await?) })
    }
}
