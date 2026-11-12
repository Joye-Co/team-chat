//! Each person's GitHub sign-in (docs/protocol.md, "GitHub sign-in"), over HTTP. The server
//! runs GitHub's device code flow and keeps the token; the app only shows the code.

use reqwest::Method;

use crate::http::{ApiResult, HttpClient, enc};
use crate::proto::{GithubDeviceResponse, GithubPollResponse, GithubStatusResponse};

impl HttpClient {
    /// `GET /v1/me/github`: whether the server can connect GitHub, my account and the runs
    /// that wait for me to connect it.
    pub async fn github_status(&self) -> ApiResult<GithubStatusResponse> {
        self.call::<(), _>(Method::GET, "/v1/me/github", None, true)
            .await
    }

    /// `POST /v1/me/github/device`: starts a device code sign-in.
    pub async fn github_device(&self) -> ApiResult<GithubDeviceResponse> {
        self.call(
            Method::POST,
            "/v1/me/github/device",
            Some(&serde_json::json!({})),
            true,
        )
        .await
    }

    /// `POST /v1/me/github/device/:id/poll`: whether I confirmed the code on GitHub.
    pub async fn github_poll(&self, device_id: &str) -> ApiResult<GithubPollResponse> {
        self.call(
            Method::POST,
            &format!("/v1/me/github/device/{}/poll", enc(device_id)),
            Some(&serde_json::json!({})),
            true,
        )
        .await
    }

    /// `DELETE /v1/me/github`: forgets my GitHub account on the server.
    pub async fn github_disconnect(&self) -> ApiResult<()> {
        self.call::<(), serde_json::Value>(Method::DELETE, "/v1/me/github", None, true)
            .await
            .map(|_| ())
    }
}
