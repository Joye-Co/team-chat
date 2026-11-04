//! 产出: a version's files in a cache folder per version (for the 产出 window), and
//! new 产出 or versions made from local files.

use crate::files::{ProgressFn, UploadRequest, upload_file};
use crate::http::{ApiError, ApiResult, HttpClient};
use crate::http_ext::enc_path;
use crate::proto::{
    AddArtifactVersionRequest, ArtifactKind, ArtifactVersion, ArtifactVersionResponse,
    CreateArtifactRequest, VersionFileInput, is_artifact_path,
};
use reqwest::Method;
use std::path::{Path, PathBuf};

/// `<root>/<id>/<version>.complete`: written after the version folder is in place.
/// It lives beside the folder, so no 产出 path can clash with it.
fn marker(dir: &Path) -> PathBuf {
    let mut s = dir.as_os_str().to_owned();
    s.push(".complete");
    PathBuf::from(s)
}

/// One local file of a new version: `path` inside the 产出, `local` on disk.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocalArtifactFile {
    pub path: String,
    pub local: PathBuf,
    /// Default: guessed from `path`.
    pub mime: Option<String>,
}

/// Every regular file under `dir` (hidden files and symlinks skipped), with paths
/// relative to `dir` joined by `/`. Fails when a path is not a valid 产出 path or there
/// are more than 500 files.
pub fn local_files_in(dir: &Path) -> ApiResult<Vec<LocalArtifactFile>> {
    let mut out = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        for e in std::fs::read_dir(&d)? {
            let e = e?;
            let name = e.file_name();
            let n = name.to_string_lossy();
            if n.starts_with('.') {
                continue;
            }
            let ft = e.file_type()?;
            if ft.is_dir() {
                stack.push(e.path());
            } else if ft.is_file() {
                let rel = e
                    .path()
                    .strip_prefix(dir)
                    .map_err(|_| ApiError::Invalid("bad path".into()))?
                    .components()
                    .map(|c| c.as_os_str().to_string_lossy().into_owned())
                    .collect::<Vec<_>>()
                    .join("/");
                if !is_artifact_path(&rel) {
                    return Err(ApiError::Invalid(format!("bad 产出 path {rel}")));
                }
                out.push(LocalArtifactFile {
                    path: rel,
                    local: e.path(),
                    mime: None,
                });
            }
        }
    }
    if out.len() > crate::proto::MAX_ARTIFACT_FILES {
        return Err(ApiError::Invalid("too many files".into()));
    }
    out.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(out)
}

/// What to make: a new 产出 (`artifact_id = None`) or a new version of one.
#[derive(Debug, Clone, PartialEq)]
pub struct NewVersion {
    pub group_id: String,
    pub topic_id: String,
    /// Required for a new 产出; for a version it renames the 产出.
    pub title: Option<String>,
    pub kind: ArtifactKind,
    /// One of the file paths. For `html` it must be a `text/html` file.
    pub entry: String,
    pub files: Vec<LocalArtifactFile>,
    pub note: Option<String>,
    pub artifact_id: Option<String>,
}

/// Uploads every file, then creates the 产出 or the version. Progress reports each file
/// in turn.
pub async fn create_version(
    http: &HttpClient,
    req: &NewVersion,
    progress: &ProgressFn,
) -> ApiResult<ArtifactVersionResponse> {
    if req.files.is_empty() || req.files.len() > crate::proto::MAX_ARTIFACT_FILES {
        return Err(ApiError::Invalid("1..=500 files".into()));
    }
    if !req.files.iter().any(|f| f.path == req.entry) {
        return Err(ApiError::Invalid("entry is not one of the files".into()));
    }
    if req.kind == ArtifactKind::File && req.files.len() != 1 {
        return Err(ApiError::Invalid("a file 产出 has exactly one file".into()));
    }
    let mut inputs = Vec::with_capacity(req.files.len());
    for f in &req.files {
        if !is_artifact_path(&f.path) {
            return Err(ApiError::Invalid(format!("bad 产出 path {}", f.path)));
        }
        let name = f.path.rsplit('/').next().unwrap_or(&f.path).to_string();
        let meta = upload_file(
            http,
            &UploadRequest {
                group_id: req.group_id.clone(),
                topic_id: Some(req.topic_id.clone()),
                path: f.local.clone(),
                name: Some(name.clone()),
                mime: Some(
                    f.mime
                        .clone()
                        .unwrap_or_else(|| crate::files::guess_mime(&name).to_string()),
                ),
            },
            progress,
        )
        .await?;
        inputs.push(VersionFileInput {
            path: f.path.clone(),
            file_id: meta.id,
        });
    }
    match &req.artifact_id {
        None => {
            let title = req
                .title
                .clone()
                .ok_or_else(|| ApiError::Invalid("a new 产出 needs a title".into()))?;
            http.create_artifact(&CreateArtifactRequest {
                group_id: req.group_id.clone(),
                topic_id: req.topic_id.clone(),
                title,
                kind: req.kind,
                entry: req.entry.clone(),
                files: inputs,
                note: req.note.clone(),
                events: None,
                media: None,
                data: None,
                run_id: None,
            })
            .await
        }
        Some(id) => {
            http.add_artifact_version(
                id,
                &AddArtifactVersionRequest {
                    entry: req.entry.clone(),
                    files: inputs,
                    note: req.note.clone(),
                    title: req.title.clone(),
                    events: None,
                    media: None,
                    data: None,
                    run_id: None,
                },
            )
            .await
        }
    }
}

/// The folder of one version: `<root>/<artifactId>/<version>/`.
pub fn version_dir(root: &Path, artifact_id: &str, version: u32) -> ApiResult<PathBuf> {
    if artifact_id.is_empty()
        || !artifact_id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
    {
        return Err(ApiError::Invalid("bad artifact id".into()));
    }
    Ok(root.join(artifact_id).join(version.to_string()))
}

/// The folder of a version when every file is on disk.
pub fn cached_version_dir(root: &Path, artifact_id: &str, version: u32) -> Option<PathBuf> {
    let d = version_dir(root, artifact_id, version).ok()?;
    marker(&d).is_file().then_some(d)
}

/// Downloads every file of a version into its folder (once; versions never change)
/// and returns the folder. The entry file is `<folder>/<version.entry>`. Files are
/// written to a temporary folder first and moved in place when all are there.
pub async fn fetch_version(
    http: &HttpClient,
    root: &Path,
    version: &ArtifactVersion,
    progress: &ProgressFn,
) -> ApiResult<PathBuf> {
    let dir = version_dir(root, &version.artifact_id, version.version)?;
    if marker(&dir).is_file() {
        return Ok(dir);
    }
    for f in &version.files {
        if !is_artifact_path(&f.path) {
            return Err(ApiError::Invalid(format!("bad 产出 path {}", f.path)));
        }
    }
    let parent = dir.parent().ok_or(ApiError::BadUrl)?.to_path_buf();
    tokio::fs::create_dir_all(&parent).await?;
    let tmp = parent.join(format!(".tmp-{}-{}", version.version, crate::new_id()));
    let res = download_all(http, &tmp, version, progress).await;
    if let Err(e) = res {
        let _ = tokio::fs::remove_dir_all(&tmp).await;
        return Err(e);
    }
    if tokio::fs::rename(&tmp, &dir).await.is_err() {
        // Another fetch finished first, or an incomplete folder is in the way.
        if marker(&dir).is_file() {
            let _ = tokio::fs::remove_dir_all(&tmp).await;
            return Ok(dir);
        }
        let _ = tokio::fs::remove_dir_all(&dir).await;
        tokio::fs::rename(&tmp, &dir).await?;
    }
    tokio::fs::write(marker(&dir), b"").await?;
    Ok(dir)
}

async fn download_all(
    http: &HttpClient,
    tmp: &Path,
    version: &ArtifactVersion,
    progress: &ProgressFn,
) -> ApiResult<()> {
    tokio::fs::create_dir_all(tmp).await?;
    for f in &version.files {
        let dest = tmp.join(f.path.replace('/', std::path::MAIN_SEPARATOR_STR));
        if let Some(p) = dest.parent() {
            tokio::fs::create_dir_all(p).await?;
        }
        let url = http.url(&format!(
            "/v1/artifacts/{}/versions/{}/files/{}",
            crate::http::enc(&version.artifact_id),
            version.version,
            enc_path(&f.path)
        ));
        let resp = http
            .send_checked(
                http.authed(Method::GET, &url)?
                    .timeout(crate::files::TRANSFER_TIMEOUT),
            )
            .await?;
        crate::files::stream_to_file(resp, &dest, f.size, None, progress).await?;
    }
    Ok(())
}
