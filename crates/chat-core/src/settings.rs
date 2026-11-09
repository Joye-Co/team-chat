//! Settings (docs/protocol.md, "Settings"): notification settings, the team's approval rules
//! for AI members, the team's AI 服务 and my signed-in devices, over HTTP.

use reqwest::Method;

use crate::client::{Client, ClientError};
use crate::http::{ApiError, ApiResult, HttpClient, enc};
use crate::proto::{
    AiServiceConfig, AiServiceTestResult, ApprovalPolicy, DevicesResponse, EmailCodeRequest,
    EmailCodeResponse, EmailSignInRequest, MyEmailResponse, NotifySettings, RedeemResponse,
    RevokeTokensResponse, SetWorkerModelRequest, UpdateAiServiceRequest,
    UpdateApprovalPolicyRequest, UpdateNotifySettingsRequest, VerifyEmailRequest, WorkerModel,
    WorkerModelsResponse,
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

    /// 设置 › 模型: the team's model and effort per CLI runtime (members read).
    pub async fn worker_models(&self) -> ApiResult<WorkerModelsResponse> {
        self.call::<(), _>(Method::GET, "/v1/settings/models", None, true)
            .await
    }

    /// Sets the team's model and effort for one runtime (owner only). Absent keeps, `Some(None)` clears.
    pub async fn set_worker_model(
        &self,
        runtime: &str,
        req: &SetWorkerModelRequest,
    ) -> ApiResult<WorkerModel> {
        req.validate().map_err(invalid)?;
        // A runtime name is lower-case letters, digits, `_` and `-`: nothing to escape.
        let probe = WorkerModel {
            runtime: runtime.to_string(),
            ..Default::default()
        };
        probe.validate().map_err(invalid)?;
        let path = format!("/v1/settings/models/{runtime}");
        self.call(Method::PUT, &path, Some(req), true).await
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

    // --- email sign-in (docs/protocol.md, "Email sign-in") ---

    /// Asks for a sign-in code at `email`. No token needed; the answer is the
    /// same whether or not the address belongs to a member.
    pub async fn email_sign_in_code(&self, email: &str) -> ApiResult<EmailCodeResponse> {
        let req = EmailCodeRequest {
            email: email.trim().to_string(),
        };
        req.validate().map_err(invalid)?;
        self.call(Method::POST, "/v1/auth/email/code", Some(&req), false)
            .await
    }

    /// Signs in with the code sent to `email`: a new token for that member.
    pub async fn email_sign_in(
        &self,
        email: &str,
        code: &str,
        device_name: &str,
    ) -> ApiResult<RedeemResponse> {
        let req = EmailSignInRequest {
            email: email.trim().to_string(),
            code: code.trim().to_string(),
            device_name: device_name.to_string(),
        };
        req.validate().map_err(invalid)?;
        self.call(Method::POST, "/v1/auth/email/sign-in", Some(&req), false)
            .await
    }

    pub async fn my_email(&self) -> ApiResult<MyEmailResponse> {
        self.call::<(), _>(Method::GET, "/v1/me/email", None, true)
            .await
    }

    /// Sends a code to `email` before it becomes my sign-in email.
    pub async fn add_email_code(&self, email: &str) -> ApiResult<EmailCodeResponse> {
        let req = EmailCodeRequest {
            email: email.trim().to_string(),
        };
        req.validate().map_err(invalid)?;
        self.call(Method::POST, "/v1/me/email/code", Some(&req), true)
            .await
    }

    /// Confirms the code and makes `email` my sign-in email.
    pub async fn verify_email(&self, email: &str, code: &str) -> ApiResult<MyEmailResponse> {
        let req = VerifyEmailRequest {
            email: email.trim().to_string(),
            code: code.trim().to_string(),
        };
        req.validate().map_err(invalid)?;
        self.call(Method::POST, "/v1/me/email", Some(&req), true)
            .await
    }

    pub async fn remove_email(&self) -> ApiResult<MyEmailResponse> {
        self.call::<(), _>(Method::DELETE, "/v1/me/email", None, true)
            .await
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
