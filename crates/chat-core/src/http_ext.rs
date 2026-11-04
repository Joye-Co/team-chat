//! V1 HTTP endpoints: members admin, group prefs, avatars, audit, hooks, 产出 metadata,
//! file records and update info. Byte transfers (uploads, downloads with progress)
//! live in [`crate::files`], [`crate::updates`] and [`crate::diagnostics`].

use crate::http::{ApiError, ApiResult, HttpClient, enc};
use crate::proto::{
    AddArtifactVersionRequest, Artifact, ArtifactResponse, ArtifactVersionResponse, AuditResponse,
    CreateArtifactRequest, CreateFileRequest, CreateFileResponse, CreateHookRequest,
    CreateHookResponse, FileMeta, GroupSummary, Hook, Member, UpdateGroupPrefsRequest,
    UpdateGroupRequest, UpdateInfo, UpdateMemberRequest,
};
use reqwest::Method;

fn invalid(e: crate::proto::ValidationError) -> ApiError {
    ApiError::Invalid(e.to_string())
}

impl HttpClient {
    /// Owner: every field; self: `name` and `role`.
    pub async fn update_member(
        &self,
        member_id: &str,
        req: &UpdateMemberRequest,
    ) -> ApiResult<Member> {
        req.validate().map_err(invalid)?;
        self.call(
            Method::PATCH,
            &format!("/v1/members/{}", enc(member_id)),
            Some(req),
            true,
        )
        .await
    }

    /// My mute and notify level for one group.
    pub async fn update_group_prefs(
        &self,
        group_id: &str,
        req: &UpdateGroupPrefsRequest,
    ) -> ApiResult<GroupSummary> {
        self.call(
            Method::PATCH,
            &format!("/v1/groups/{}/prefs", enc(group_id)),
            Some(req),
            true,
        )
        .await
    }

    /// Renames a group (a member of it, or an owner). The server trims the name and collapses
    /// whitespace; a DM answers `forbidden`.
    pub async fn rename_group(&self, group_id: &str, name: &str) -> ApiResult<GroupSummary> {
        let req = UpdateGroupRequest {
            purpose: None,
            name: Some(name.to_string()),
        };
        self.update_group(group_id, &req).await
    }

    /// 1.1: sets a group's purpose line (`""` clears it). Same rights as a rename.
    pub async fn set_group_purpose(
        &self,
        group_id: &str,
        purpose: &str,
    ) -> ApiResult<GroupSummary> {
        let req = UpdateGroupRequest {
            purpose: Some(purpose.to_string()),
            name: None,
        };
        self.update_group(group_id, &req).await
    }

    async fn update_group(
        &self,
        group_id: &str,
        req: &UpdateGroupRequest,
    ) -> ApiResult<GroupSummary> {
        req.validate().map_err(invalid)?;
        self.call(
            Method::PATCH,
            &format!("/v1/groups/{}", enc(group_id)),
            Some(req),
            true,
        )
        .await
    }

    /// Self or an owner. `mime`: image/png, image/jpeg or image/webp; at most 1 MiB.
    pub async fn upload_avatar(
        &self,
        member_id: &str,
        bytes: Vec<u8>,
        mime: &str,
    ) -> ApiResult<Member> {
        if !matches!(mime, "image/png" | "image/jpeg" | "image/webp") {
            return Err(ApiError::Invalid("avatar must be png, jpeg or webp".into()));
        }
        if bytes.len() as u64 > crate::proto::MAX_AVATAR_BYTES {
            return Err(ApiError::Invalid("avatar larger than 1 MiB".into()));
        }
        let req = self
            .authed(
                Method::PUT,
                &self.url(&format!("/v1/members/{}/avatar", enc(member_id))),
            )?
            .header(reqwest::header::CONTENT_TYPE, mime)
            .body(bytes);
        let resp = self.send_checked(req).await?;
        Self::json(resp).await
    }

    /// The bytes of an avatar URL (`Member.avatarUrl`). Only URLs on this server get
    /// the token; others are refused.
    pub async fn avatar_bytes(&self, avatar_url: &str) -> ApiResult<Vec<u8>> {
        if !self.same_origin(avatar_url) {
            return Err(ApiError::BadUrl);
        }
        let resp = self
            .send_checked(self.authed(Method::GET, avatar_url)?)
            .await?;
        let bytes = resp.bytes().await.map_err(crate::http::redact)?;
        if bytes.len() as u64 > crate::proto::MAX_AVATAR_BYTES * 2 {
            return Err(ApiError::Invalid("avatar too large".into()));
        }
        Ok(bytes.to_vec())
    }

    /// Owner. Newest first; `before` is an entry id.
    pub async fn audit(
        &self,
        limit: Option<u32>,
        before: Option<&str>,
    ) -> ApiResult<AuditResponse> {
        let mut q = Vec::new();
        if let Some(l) = limit {
            q.push(format!(
                "limit={}",
                l.clamp(1, crate::proto::MAX_AUDIT_LIMIT)
            ));
        }
        if let Some(b) = before {
            q.push(format!("before={}", enc(b)));
        }
        let path = if q.is_empty() {
            "/v1/audit".to_string()
        } else {
            format!("/v1/audit?{}", q.join("&"))
        };
        self.call::<(), _>(Method::GET, &path, None, true).await
    }

    /// Owner. The response `secret` is shown once: never log it.
    pub async fn create_hook(&self, req: &CreateHookRequest) -> ApiResult<CreateHookResponse> {
        req.validate().map_err(invalid)?;
        self.call(Method::POST, "/v1/hooks", Some(req), true).await
    }

    /// Owner.
    pub async fn hooks(&self) -> ApiResult<Vec<Hook>> {
        self.call::<(), _>(Method::GET, "/v1/hooks", None, true)
            .await
    }

    /// Owner. Deactivates the hook's member.
    pub async fn delete_hook(&self, hook_id: &str) -> ApiResult<()> {
        let _: serde_json::Value = self
            .call::<(), _>(
                Method::DELETE,
                &format!("/v1/hooks/{}", enc(hook_id)),
                None,
                true,
            )
            .await?;
        Ok(())
    }

    /// Creates a `pending` file record. Upload with [`crate::files`].
    pub async fn create_file(&self, req: &CreateFileRequest) -> ApiResult<CreateFileResponse> {
        req.validate().map_err(invalid)?;
        self.call(Method::POST, "/v1/files", Some(req), true).await
    }

    pub async fn file_meta(&self, file_id: &str) -> ApiResult<FileMeta> {
        self.call::<(), _>(
            Method::GET,
            &format!("/v1/files/{}", enc(file_id)),
            None,
            true,
        )
        .await
    }

    /// 产出 of a group (of one topic when `topic_id` is set), newest first.
    pub async fn artifacts(
        &self,
        group_id: &str,
        topic_id: Option<&str>,
    ) -> ApiResult<Vec<Artifact>> {
        let mut path = format!("/v1/artifacts?groupId={}", enc(group_id));
        if let Some(t) = topic_id {
            path.push_str(&format!("&topicId={}", enc(t)));
        }
        self.call::<(), _>(Method::GET, &path, None, true).await
    }

    /// A 产出 with all versions, newest first.
    /// `GET /v1/artifacts/:id/versions/:v/data` (1.1): the latest data snapshot of a version.
    pub async fn artifact_data(
        &self,
        artifact_id: &str,
        version: u32,
    ) -> ApiResult<crate::proto::ArtifactDataResponse> {
        self.call::<(), _>(
            Method::GET,
            &format!("/v1/artifacts/{}/versions/{version}/data", enc(artifact_id)),
            None,
            true,
        )
        .await
    }

    /// 1.1: the page log of a 产出 (页面里的回应), newest first; `before` is an event id.
    pub async fn artifact_events(
        &self,
        artifact_id: &str,
        before: Option<&str>,
        limit: u32,
    ) -> ApiResult<crate::proto::ArtifactEventsResponse> {
        let mut path = format!(
            "/v1/artifacts/{}/events?limit={}",
            enc(artifact_id),
            limit.clamp(1, crate::proto::MAX_ARTIFACT_EVENTS_PAGE)
        );
        if let Some(b) = before {
            path.push_str(&format!("&before={}", enc(b)));
        }
        self.call::<(), _>(Method::GET, &path, None, true).await
    }

    pub async fn artifact(&self, artifact_id: &str) -> ApiResult<ArtifactResponse> {
        self.call::<(), _>(
            Method::GET,
            &format!("/v1/artifacts/{}", enc(artifact_id)),
            None,
            true,
        )
        .await
    }

    pub async fn create_artifact(
        &self,
        req: &CreateArtifactRequest,
    ) -> ApiResult<ArtifactVersionResponse> {
        req.validate().map_err(invalid)?;
        self.call(Method::POST, "/v1/artifacts", Some(req), true)
            .await
    }

    // --- inc-host track ---
    /// `PUT /v1/artifacts/:id/versions/:v/media` (1.1): stores `artifact.previewReady` with
    /// the version's media metadata (the creator, or a member while the preview is not
    /// ready).
    pub async fn put_artifact_media(
        &self,
        artifact_id: &str,
        version: u32,
        media: &crate::proto::MediaInfo,
    ) -> ApiResult<serde_json::Value> {
        media.validate().map_err(invalid)?;
        self.call(
            Method::PUT,
            &format!(
                "/v1/artifacts/{}/versions/{version}/media",
                enc(artifact_id)
            ),
            Some(&serde_json::json!({ "media": media })),
            true,
        )
        .await
    }
    // --- end inc-host track ---

    pub async fn add_artifact_version(
        &self,
        artifact_id: &str,
        req: &AddArtifactVersionRequest,
    ) -> ApiResult<ArtifactVersionResponse> {
        req.validate().map_err(invalid)?;
        self.call(
            Method::POST,
            &format!("/v1/artifacts/{}/versions", enc(artifact_id)),
            Some(req),
            true,
        )
        .await
    }

    /// The newest release, or `None` when the server has none (404).
    pub async fn latest_update(&self) -> ApiResult<Option<UpdateInfo>> {
        match self
            .call::<(), UpdateInfo>(Method::GET, "/v1/updates/latest", None, true)
            .await
        {
            Ok(u) => Ok(Some(u)),
            Err(ApiError::Server { status: 404, .. } | ApiError::Unexpected { status: 404 }) => {
                Ok(None)
            }
            Err(e) => Err(e),
        }
    }

    /// Owner: the gzip body of an uploaded diagnostics report.
    pub async fn diagnostics_report(&self, id: &str) -> ApiResult<Vec<u8>> {
        let resp = self
            .send_checked(self.authed(
                Method::GET,
                &self.url(&format!("/v1/diagnostics/{}", enc(id))),
            )?)
            .await?;
        Ok(resp.bytes().await.map_err(crate::http::redact)?.to_vec())
    }
}

/// Encodes an artifact file path for a URL: each segment escaped, `/` kept.
pub(crate) fn enc_path(path: &str) -> String {
    path.split('/')
        .map(|seg| {
            seg.bytes()
                .map(|b| {
                    if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~') {
                        (b as char).to_string()
                    } else {
                        format!("%{b:02X}")
                    }
                })
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("/")
}
