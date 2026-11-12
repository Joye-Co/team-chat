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
    /// A URL that is not on the server origin, or not http(s).
    #[error("invalid base url")]
    BadUrl,
    /// The client refused the input before sending it.
    #[error("invalid input: {0}")]
    Invalid(String),
    /// A local file could not be read or written.
    #[error("io: {0}")]
    Io(String),
    /// Downloaded or uploaded bytes do not match the expected size or SHA-256.
    #[error("content does not match its checksum")]
    Checksum,
}

impl From<std::io::Error> for ApiError {
    fn from(e: std::io::Error) -> Self {
        ApiError::Io(e.to_string())
    }
}

impl ApiError {
    pub fn code(&self) -> Option<ErrorCode> {
        match self {
            ApiError::Server { code, .. } => Some(*code),
            _ => None,
        }
    }
    /// The server refused the request because of the current state (409 `conflict`), for
    /// example a run that already ended.
    pub fn is_conflict(&self) -> bool {
        matches!(
            self,
            ApiError::Server { status: 409, .. } | ApiError::Unexpected { status: 409 }
        ) || self.code() == Some(ErrorCode::Conflict)
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

fn error_from(status: StatusCode, bytes: &[u8]) -> ApiError {
    match serde_json::from_slice::<ErrorResponse>(bytes) {
        Ok(er) => ApiError::Server {
            status: status.as_u16(),
            code: er.error.code,
            message: er.error.message,
        },
        Err(_) if status == StatusCode::UNAUTHORIZED => ApiError::Server {
            status: 401,
            code: ErrorCode::Unauthorized,
            message: String::new(),
        },
        Err(_) => ApiError::Unexpected {
            status: status.as_u16(),
        },
    }
}

pub(crate) fn redact(e: reqwest::Error) -> ApiError {
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

    /// Absolute URL of an API path.
    pub(crate) fn url(&self, path: &str) -> String {
        format!("{}{}", self.base, path)
    }

    /// True when `url` is on this client's server origin, so it may carry the token.
    pub fn same_origin(&self, url: &str) -> bool {
        url.strip_prefix(self.base.as_str())
            .is_some_and(|rest| rest.starts_with('/'))
    }

    /// A request with the bearer token.
    pub(crate) fn authed(&self, method: Method, url: &str) -> ApiResult<reqwest::RequestBuilder> {
        let token = self.token.as_deref().ok_or(ApiError::NoToken)?;
        Ok(self.http.request(method, url).bearer_auth(token))
    }

    /// Sends a request and turns a non-success status into an [`ApiError`].
    pub(crate) async fn send_checked(
        &self,
        req: reqwest::RequestBuilder,
    ) -> ApiResult<reqwest::Response> {
        let resp = req.send().await.map_err(redact)?;
        if resp.status().is_success() {
            return Ok(resp);
        }
        let status = resp.status();
        let bytes = resp.bytes().await.map_err(redact)?;
        Err(error_from(status, &bytes))
    }

    /// Decodes a JSON success body.
    pub(crate) async fn json<T: DeserializeOwned>(resp: reqwest::Response) -> ApiResult<T> {
        let bytes = resp.bytes().await.map_err(redact)?;
        serde_json::from_slice(&bytes).map_err(|e| ApiError::Decode(e.to_string()))
    }

    pub(crate) async fn call<B: Serialize, T: DeserializeOwned>(
        &self,
        method: Method,
        path: &str,
        body: Option<&B>,
        auth: bool,
    ) -> ApiResult<T> {
        let mut req = if auth {
            self.authed(method, &self.url(path))?
        } else {
            self.http.request(method, self.url(path))
        };
        if let Some(b) = body {
            req = req.json(b);
        }
        let resp = self.send_checked(req).await?;
        Self::json(resp).await
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

    /// hide-delete/1: deletes a 群 for everyone (an owner; not a DM, not the default group).
    /// The server pushes `directory { removedGroupIds }` to every member.
    pub async fn delete_group(&self, group_id: &str) -> ApiResult<()> {
        let _: serde_json::Value = self
            .call::<(), _>(
                Method::DELETE,
                &format!("/v1/groups/{}", enc(group_id)),
                None,
                true,
            )
            .await?;
        Ok(())
    }

    /// hide-delete/1: deletes a deactivated AI member for good (an owner). Answers the member
    /// with `deleted: true`; old messages keep it as their author.
    pub async fn delete_member(&self, member_id: &str) -> ApiResult<Member> {
        self.call::<(), _>(
            Method::DELETE,
            &format!("/v1/members/{}", enc(member_id)),
            None,
            true,
        )
        .await
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
            member_id: None,
            expires_in_hours: None,
        };
        self.call(Method::POST, "/v1/invites", Some(&body), true)
            .await
    }

    /// Owner only. The returned token is a secret: never log it.
    pub async fn create_bot(
        &self,
        name: &str,
        role: Option<&str>,
        character: Option<crate::proto::CharacterId>,
    ) -> ApiResult<CreateBotResponse> {
        let body = CreateBotRequest {
            name: name.to_string(),
            role: role.map(str::to_string),
            character,
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

pub(crate) fn enc(s: &str) -> String {
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
