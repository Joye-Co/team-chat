//! Files and 产出 (artifacts) over HTTP, and their group events
//! (docs/protocol.md, "Files" and "产出 (artifacts)").

use serde::{Deserialize, Serialize};

use crate::entities::Millis;
use crate::limits::{
    MAX_TITLE_CHARS, Validation, ValidationError, check_chars, check_id, check_title,
};
use crate::message::{MAX_FILE_NAME_CHARS, check_mime, check_positive, check_sha256};
use crate::serde_util::{non_null, nullable};

/// Max bytes of one file upload: 100 MB (decimal), the Workers request body limit.
pub const MAX_FILE_BYTES: u64 = 100_000_000;
/// Max bytes of an avatar image.
pub const MAX_AVATAR_BYTES: u64 = 1_048_576;
pub const MAX_ARTIFACT_FILES: usize = 500;
pub const MAX_ARTIFACT_PATH_CHARS: usize = 255;
pub const MAX_VERSION_NOTE_CHARS: usize = 2000;

fn fail(field: &'static str, message: impl Into<String>) -> Validation {
    Err(ValidationError {
        field,
        message: message.into(),
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FileStatus {
    /// Created, content not uploaded yet.
    Pending,
    /// Uploaded and the SHA-256 matched.
    Ready,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileMeta {
    pub id: String,
    pub group_id: String,
    #[serde(
        default,
        deserialize_with = "nullable",
        skip_serializing_if = "Option::is_none"
    )]
    pub topic_id: Option<Option<String>>,
    pub name: String,
    pub size: u64,
    pub mime: String,
    pub sha256: String,
    #[serde(
        default,
        deserialize_with = "non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub width: Option<u32>,
    #[serde(
        default,
        deserialize_with = "non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub height: Option<u32>,
    pub status: FileStatus,
    pub created_by: String,
    pub created_at: Millis,
}

impl FileMeta {
    /// The attachment form of this file.
    pub fn to_file_ref(&self) -> crate::FileRef {
        crate::FileRef {
            file_id: self.id.clone(),
            name: self.name.clone(),
            size: self.size,
            mime: self.mime.clone(),
            width: self.width,
            height: self.height,
            sha256: self.sha256.clone(),
        }
    }
}

/// `POST /v1/files`
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateFileRequest {
    pub group_id: String,
    #[serde(
        default,
        deserialize_with = "non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub topic_id: Option<String>,
    pub name: String,
    pub size: u64,
    pub mime: String,
    pub sha256: String,
    #[serde(
        default,
        deserialize_with = "non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub width: Option<u32>,
    #[serde(
        default,
        deserialize_with = "non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub height: Option<u32>,
}

impl CreateFileRequest {
    pub fn validate(&self) -> Validation {
        check_id("groupId", &self.group_id)?;
        if let Some(t) = &self.topic_id {
            check_id("topicId", t)?;
        }
        check_chars("name", &self.name, 1, MAX_FILE_NAME_CHARS)?;
        if self.size > MAX_FILE_BYTES {
            return fail("size", format!("at most {MAX_FILE_BYTES} bytes"));
        }
        check_mime("mime", &self.mime)?;
        check_sha256("sha256", &self.sha256)?;
        check_positive("width", self.width)?;
        check_positive("height", self.height)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateFileResponse {
    pub file: FileMeta,
    /// Absolute URL for `PUT` of the raw content (same bearer token).
    pub upload_url: String,
}

/// `PUT /v1/files/:id/content` and `GET /v1/files/:id`
pub type FileResponse = FileMeta;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ArtifactKind {
    Html,
    File,
}

/// True for a safe relative path: segments split by `/`, none empty, `.` or `..`, and no `\`
/// or NUL anywhere.
pub fn is_artifact_path(path: &str) -> bool {
    !path.contains(['\\', '\0'])
        && path
            .split('/')
            .all(|s| !s.is_empty() && s != "." && s != "..")
}

pub fn check_artifact_path(field: &'static str, path: &str) -> Validation {
    check_chars(field, path, 1, MAX_ARTIFACT_PATH_CHARS)?;
    if is_artifact_path(path) {
        Ok(())
    } else {
        fail(field, "unsafe artifact path")
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Artifact {
    pub id: String,
    pub group_id: String,
    pub topic_id: String,
    pub title: String,
    pub kind: ArtifactKind,
    pub current_version: u32,
    pub created_by: String,
    pub created_at: Millis,
    pub updated_at: Millis,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ArtifactFile {
    pub path: String,
    pub file_id: String,
    pub size: u64,
    pub mime: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ArtifactVersion {
    pub artifact_id: String,
    pub version: u32,
    /// Path of the file to open: the HTML page, or the only file.
    pub entry: String,
    pub files: Vec<ArtifactFile>,
    #[serde(
        default,
        deserialize_with = "non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub note: Option<String>,
    pub created_by: String,
    pub created_at: Millis,
}

/// A file of a new version: an uploaded (`ready`) file of the same group, at a path.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VersionFileInput {
    pub path: String,
    pub file_id: String,
}

fn check_version_input(entry: &str, files: &[VersionFileInput], note: Option<&str>) -> Validation {
    check_artifact_path("entry", entry)?;
    if files.is_empty() || files.len() > MAX_ARTIFACT_FILES {
        return fail("files", format!("1..={MAX_ARTIFACT_FILES} files"));
    }
    for f in files {
        check_artifact_path("files.path", &f.path)?;
        check_id("files.fileId", &f.file_id)?;
    }
    note.map_or(Ok(()), |n| {
        check_chars("note", n, 0, MAX_VERSION_NOTE_CHARS)
    })
}

/// `POST /v1/artifacts`: makes the 产出 and its version 1.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateArtifactRequest {
    pub group_id: String,
    pub topic_id: String,
    pub title: String,
    pub kind: ArtifactKind,
    pub entry: String,
    pub files: Vec<VersionFileInput>,
    #[serde(
        default,
        deserialize_with = "non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub note: Option<String>,
}

impl CreateArtifactRequest {
    pub fn validate(&self) -> Validation {
        check_id("groupId", &self.group_id)?;
        check_id("topicId", &self.topic_id)?;
        check_chars("title", &self.title, 1, MAX_TITLE_CHARS)?;
        check_version_input(&self.entry, &self.files, self.note.as_deref())
    }
}

/// `POST /v1/artifacts/:id/versions`
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AddArtifactVersionRequest {
    pub entry: String,
    pub files: Vec<VersionFileInput>,
    #[serde(
        default,
        deserialize_with = "non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub note: Option<String>,
    /// Renames the 产出.
    #[serde(
        default,
        deserialize_with = "non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub title: Option<String>,
}

impl AddArtifactVersionRequest {
    pub fn validate(&self) -> Validation {
        check_version_input(&self.entry, &self.files, self.note.as_deref())?;
        self.title.as_deref().map_or(Ok(()), check_title)
    }
}

/// Response of `POST /v1/artifacts` and `POST /v1/artifacts/:id/versions`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ArtifactVersionResponse {
    pub artifact: Artifact,
    pub version: ArtifactVersion,
}

/// `GET /v1/artifacts/:id`: versions newest first.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ArtifactResponse {
    pub artifact: Artifact,
    pub versions: Vec<ArtifactVersion>,
}

/// `GET /v1/artifacts?groupId=&topicId=`
pub type ArtifactsResponse = Vec<Artifact>;

/// Data of `artifact.created` and `artifact.versionAdded`.
pub type ArtifactEventData = ArtifactVersionResponse;
