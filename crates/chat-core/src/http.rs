//! HTTP API client (reqwest, rustls with webpki roots). Every method is async; run them
//! on a tokio runtime such as [`crate::background`] or [`crate::Client::spawn`].

use crate::proto::{
    AddGroupMemberRequest, CreateBotRequest, CreateBotResponse, CreateDmRequest,
    CreateGroupRequest, CreateInviteRequest, CreateInviteResponse, ErrorCode, ErrorResponse,
    GroupSummary, HealthResponse, Member, MemberKind, MemberLevel, RedeemRequest, RedeemResponse,
    RevokeTokensRequest, RevokeTokensResponse,
};
use reqwest::{Method, StatusCode};
use serde::{Serialize, de::DeserializeOwned};
use std::time::Duration;

#[derive(Debug, Clone, thiserror::Error)]
pub enum ApiError {
    /// The request did not reach the server or the answer did not arrive.
    #[error("network error: {0}")]
    Network(String),
    /// The server answered with an error body.
    #[error("server error {status} {code:?}: {message}")]
    Server {
        status: u16,
        code: ErrorCode,
        message: String,
    },
    /// The server answered with a status and a body that is not an error body.
    #[error("unexpected response status {status}")]
    Unexpected { status: u16 },
    /// The success body did not match the protocol.
    #[error("invalid response body: {0}")]
    Decode(String),
    /// The call needs a token and the client has none.
    #[error("no token")]
    NoToken,
    #[error("invalid base url")]
    BadUrl,
}

impl ApiError {
    pub fn code(&self) -> Option<ErrorCode> {
        match self {
            ApiError::Server { code, .. } => Some(*code),
            _ => None,
        }
    }
    pub fn is_unauthorized(&self) -> bool {
        matches!(
            self,
            ApiError::Server { status: 401, .. } | ApiError::Unexpected { status: 401 }
        ) || self.code() == Some(ErrorCode::Unauthorized)
    }
}

pub type ApiResult<T> = Result<T, ApiError>;

#[derive(Clone)]
pub struct HttpClient {
    base: String,
    token: Option<String>,
    http: reqwest::Client,
}

impl std::fmt::Debug for HttpClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HttpClient")
            .field("base", &self.base)
            .field("token", &self.token.as_ref().map(|_| "<redacted>"))
            .finish()
    }
}

fn redact(e: reqwest::Error) -> ApiError {
    // reqwest errors may carry the URL; ours carry no secrets, but drop it anyway.
    ApiError::Network(e.without_url().to_string())
}

impl HttpClient {
    /// `base` is the server origin, such as `https://chat-api-staging.example.dev`.
    pub fn new(base: &str, token: Option<String>) -> ApiResult<HttpClient> {
        let base = base.trim_end_matches('/').to_string();
        if !(base.starts_with("http://") || base.starts_with("https://")) {
            return Err(ApiError::BadUrl);
        }
        let http = reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(10))
            .timeout(Duration::from_secs(30))
            .build()
            .map_err(redact)?;
        Ok(HttpClient { base, token, http })
    }

    pub fn with_token(&self, token: Option<String>) -> HttpClient {
        HttpClient {
            base: self.base.clone(),
            token,
            http: self.http.clone(),
        }
    }

    pub fn base(&self) -> &str {
        &self.base
    }

    async fn call<B: Serialize, T: DeserializeOwned>(
        &self,
        method: Method,
        path: &str,
        body: Option<&B>,
        auth: bool,
    ) -> ApiResult<T> {
        let mut req = self.http.request(method, format!("{}{}", self.base, path));
        if auth {
            let token = self.token.as_deref().ok_or(ApiError::NoToken)?;
            req = req.bearer_auth(token);
        }
        if let Some(b) = body {
            req = req.json(b);
        }
        let resp = req.send().await.map_err(redact)?;
        let status = resp.status();
        let bytes = resp.bytes().await.map_err(redact)?;
        if status.is_success() {
            return serde_json::from_slice(&bytes).map_err(|e| ApiError::Decode(e.to_string()));
        }
        match serde_json::from_slice::<ErrorResponse>(&bytes) {
            Ok(er) => Err(ApiError::Server {
                status: status.as_u16(),
                code: er.error.code,
                message: er.error.message,
            }),
            Err(_) if status == StatusCode::UNAUTHORIZED => Err(ApiError::Server {
                status: 401,
                code: ErrorCode::Unauthorized,
                message: String::new(),
            }),
            Err(_) => Err(ApiError::Unexpected {
                status: status.as_u16(),
            }),
        }
    }

    pub async fn health(&self) -> ApiResult<HealthResponse> {
        self.call::<(), _>(Method::GET, "/v1/health", None, false)
            .await
    }

    /// Exchanges an invite code for a token. The caller stores the token in a [`crate::TokenStore`].
    pub async fn redeem(&self, invite_code: &str, device_name: &str) -> ApiResult<RedeemResponse> {
        let body = RedeemRequest {
            invite_code: invite_code.to_string(),
            device_name: device_name.to_string(),
        };
        self.call(Method::POST, "/v1/auth/redeem", Some(&body), false)
            .await
    }

    pub async fn me(&self) -> ApiResult<Member> {
        self.call::<(), _>(Method::GET, "/v1/me", None, true).await
    }

    pub async fn members(&self) -> ApiResult<Vec<Member>> {
        self.call::<(), _>(Method::GET, "/v1/members", None, true)
            .await
    }

    pub async fn groups(&self) -> ApiResult<Vec<GroupSummary>> {
        self.call::<(), _>(Method::GET, "/v1/groups", None, true)
            .await
    }

    pub async fn create_group(&self, name: &str, member_ids: &[String]) -> ApiResult<GroupSummary> {
        let body = CreateGroupRequest {
            name: name.to_string(),
            member_ids: member_ids.to_vec(),
        };
        self.call(Method::POST, "/v1/groups", Some(&body), true)
            .await
    }

    pub async fn add_group_member(
        &self,
        group_id: &str,
        member_id: &str,
    ) -> ApiResult<GroupSummary> {
        let body = AddGroupMemberRequest {
            member_id: member_id.to_string(),
        };
        self.call(
            Method::POST,
            &format!("/v1/groups/{}/members", enc(group_id)),
            Some(&body),
            true,
        )
        .await
    }

    pub async fn remove_group_member(&self, group_id: &str, member_id: &str) -> ApiResult<()> {
        let _: serde_json::Value = self
            .call::<(), _>(
                Method::DELETE,
                &format!("/v1/groups/{}/members/{}", enc(group_id), enc(member_id)),
                None,
                true,
            )
            .await?;
        Ok(())
    }

    /// Finds or creates the DM with a member.
    pub async fn dm(&self, member_id: &str) -> ApiResult<GroupSummary> {
        let body = CreateDmRequest {
            member_id: member_id.to_string(),
        };
        self.call(Method::POST, "/v1/dms", Some(&body), true).await
    }

    /// Owner only. The returned code is a secret: never log it.
    pub async fn create_invite(
        &self,
        name: &str,
        kind: MemberKind,
        level: MemberLevel,
        role: Option<&str>,
    ) -> ApiResult<CreateInviteResponse> {
        let body = CreateInviteRequest {
            name: name.to_string(),
            kind,
            level,
            role: role.map(str::to_string),
        };
        self.call(Method::POST, "/v1/invites", Some(&body), true)
            .await
    }

    /// Owner only. The returned token is a secret: never log it.
    pub async fn create_bot(&self, name: &str, role: Option<&str>) -> ApiResult<CreateBotResponse> {
        let body = CreateBotRequest {
            name: name.to_string(),
            role: role.map(str::to_string),
        };
        self.call(Method::POST, "/v1/bots", Some(&body), true).await
    }

    /// Revokes tokens: with no arguments, the token of this client.
    pub async fn revoke(
        &self,
        member_id: Option<&str>,
        all: bool,
    ) -> ApiResult<RevokeTokensResponse> {
        let body = RevokeTokensRequest {
            member_id: member_id.map(str::to_string),
            all: all.then_some(true),
        };
        self.call(Method::POST, "/v1/tokens/revoke", Some(&body), true)
            .await
    }
}

fn enc(s: &str) -> String {
    // Ids are ULIDs; escape anything else so a path cannot be changed.
    s.bytes()
        .map(|b| {
            if b.is_ascii_alphanumeric() || b == b'-' || b == b'_' {
                (b as char).to_string()
            } else {
                format!("%{b:02X}")
            }
        })
        .collect()
}
