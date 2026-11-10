//! 沉淀 (memory) over HTTP (docs/protocol.md, "Memory"): the memories of a scope, their
//! versions, a day's 记忆更新 and undoing one of its lines.

use crate::http::{ApiError, ApiResult, HttpClient, enc};
use crate::proto::{
    CreateMemoryRequest, MemoriesResponse, MemoryItem, MemoryUpdate, MemoryUpdatesResponse,
    MemoryVersionsResponse, UpdateMemoryRequest, check_memory_scope,
};
use reqwest::Method;

/// Which memories `GET /v1/memories` lists.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum MemoryStatusFilter {
    #[default]
    Active,
    Pending,
    Retired,
    All,
}

impl MemoryStatusFilter {
    fn wire(self) -> &'static str {
        match self {
            MemoryStatusFilter::Active => "active",
            MemoryStatusFilter::Pending => "pending",
            MemoryStatusFilter::Retired => "retired",
            MemoryStatusFilter::All => "all",
        }
    }
}

/// A query value: unreserved characters as they are, everything else percent-encoded
/// (UTF-8), so `member:b1` and Chinese text survive.
fn q(s: &str) -> String {
    s.bytes()
        .map(|b| {
            if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~') {
                (b as char).to_string()
            } else {
                format!("%{b:02X}")
            }
        })
        .collect()
}

fn scope_ok(scope: &str) -> ApiResult<()> {
    check_memory_scope(scope).map_err(|e| ApiError::Invalid(e.to_string()))
}

impl HttpClient {
    /// The memories of `scope`, newest change first. `query` matches the text; `limit`
    /// is clamped to 1..=500.
    pub async fn memories(
        &self,
        scope: &str,
        status: MemoryStatusFilter,
        query: Option<&str>,
        limit: u32,
    ) -> ApiResult<MemoriesResponse> {
        scope_ok(scope)?;
        let mut path = format!(
            "/v1/memories?scope={}&status={}&limit={}",
            q(scope),
            status.wire(),
            limit.clamp(1, 500)
        );
        if let Some(text) = query.map(str::trim).filter(|t| !t.is_empty()) {
            path.push_str(&format!("&q={}", q(text)));
        }
        self.call::<(), _>(Method::GET, &path, None, true).await
    }

    /// Writers of the scope: a person's memory is active at once.
    pub async fn create_memory(&self, req: &CreateMemoryRequest) -> ApiResult<MemoryItem> {
        req.validate()
            .map_err(|e| ApiError::Invalid(e.to_string()))?;
        self.call(Method::POST, "/v1/memories", Some(req), true)
            .await
    }

    pub async fn memory(&self, id: &str) -> ApiResult<MemoryItem> {
        self.call::<(), _>(
            Method::GET,
            &format!("/v1/memories/{}", enc(id)),
            None,
            true,
        )
        .await
    }

    /// Writers: edits, 停用 (`retired`) or makes active again; a new version.
    pub async fn update_memory(
        &self,
        id: &str,
        req: &UpdateMemoryRequest,
    ) -> ApiResult<MemoryItem> {
        req.validate()
            .map_err(|e| ApiError::Invalid(e.to_string()))?;
        self.call(
            Method::PATCH,
            &format!("/v1/memories/{}", enc(id)),
            Some(req),
            true,
        )
        .await
    }

    /// Every version of a memory, newest first.
    pub async fn memory_versions(&self, id: &str) -> ApiResult<MemoryVersionsResponse> {
        self.call::<(), _>(
            Method::GET,
            &format!("/v1/memories/{}/versions", enc(id)),
            None,
            true,
        )
        .await
    }

    /// Writers: a new version with what `version` held.
    pub async fn restore_memory_version(&self, id: &str, version: u32) -> ApiResult<MemoryItem> {
        self.call::<(), _>(
            Method::POST,
            &format!("/v1/memories/{}/versions/{version}/restore", enc(id)),
            None,
            true,
        )
        .await
    }

    /// The 记忆更新 of `scope` on `day` (`YYYY-MM-DD`), or of the newest day with any.
    pub async fn memory_updates(
        &self,
        scope: &str,
        day: Option<&str>,
    ) -> ApiResult<MemoryUpdatesResponse> {
        scope_ok(scope)?;
        let mut path = format!("/v1/memories/updates?scope={}", q(scope));
        if let Some(d) = day {
            path.push_str(&format!("&day={}", q(d)));
        }
        self.call::<(), _>(Method::GET, &path, None, true).await
    }

    /// Writers of the line's scope: undoes one line of a day's 记忆更新. A `flagged` line is
    /// `invalid`; a second undo is `conflict`.
    pub async fn undo_memory_update(&self, id: &str) -> ApiResult<MemoryUpdate> {
        self.call::<(), _>(
            Method::POST,
            &format!("/v1/memories/updates/{}/undo", enc(id)),
            None,
            true,
        )
        .await
    }
}

#[cfg(test)]
mod tests {
    use super::q;

    #[test]
    fn query_values_are_percent_encoded() {
        assert_eq!(q("member:b-1"), "member%3Ab-1");
        assert_eq!(q("team"), "team");
        assert_eq!(q("先发 staging"), "%E5%85%88%E5%8F%91%20staging");
        assert_eq!(q("a&b=c"), "a%26b%3Dc");
    }
}
