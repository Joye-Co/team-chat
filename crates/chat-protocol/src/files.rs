//! Files and 产出 (artifacts) over HTTP, and their group events
//! (docs/protocol.md, "Files" and "产出 (artifacts)").

use serde::{Deserialize, Serialize};

use crate::card2::{Expect, is_event_name, json_bytes};
use crate::entities::Millis;
use crate::limits::{
    MAX_TITLE_CHARS, Validation, ValidationError, check_chars, check_id, check_title,
};
use crate::media::MediaInfo;
use crate::message::{MAX_FILE_NAME_CHARS, check_mime, check_positive, check_sha256};
use crate::serde_util::{any_value, non_null, nullable};
use serde_json::Value;

/// Max bytes of one file upload: 100 MB (decimal), the Workers request body limit.
pub const MAX_FILE_BYTES: u64 = 100_000_000;
/// Max bytes of an avatar image.
pub const MAX_AVATAR_BYTES: u64 = 1_048_576;
pub const MAX_ARTIFACT_FILES: usize = 500;
pub const MAX_ARTIFACT_PATH_CHARS: usize = 255;
pub const MAX_VERSION_NOTE_CHARS: usize = 2000;
/// Max events a 产出 version declares.
pub const MAX_ARTIFACT_EVENTS: usize = 20;
/// Max UTF-8 bytes of one `artifact.event` payload as JSON.
pub const MAX_ARTIFACT_EVENT_PAYLOAD_BYTES: usize = 16_384;
/// Max UTF-8 bytes of an `artifact.reply` payload, and of a data snapshot, as JSON.
pub const MAX_ARTIFACT_REPLY_BYTES: usize = 65_536;
pub const MAX_ARTIFACT_DATA_BYTES: usize = 262_144;
/// A data snapshot up to this size travels inline in the `artifact.data` transient.
pub const MAX_ARTIFACT_DATA_INLINE_BYTES: usize = 65_536;
/// Paths under this prefix are reserved for TeamChat (bundled libraries of the page host).
pub const RESERVED_ARTIFACT_PREFIX: &str = "__teamchat/";

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
            media: None,
            source: None,
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
    /// 1.1: the bot run that made this version.
    #[serde(
        default,
        deserialize_with = "non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub run_id: Option<String>,
    /// 1.1: events the page may send back (bot-made versions only).
    #[serde(
        default,
        deserialize_with = "non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub events: Option<Vec<ArtifactEventDecl>>,
    /// 1.1: what the version is, for previews.
    #[serde(
        default,
        deserialize_with = "non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub media: Option<MediaInfo>,
    /// 1.1: rev of the latest data snapshot (0 or absent = none).
    #[serde(
        default,
        deserialize_with = "non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub data_rev: Option<u64>,
}

impl ArtifactVersion {
    /// The declared event `name`, if any.
    pub fn event(&self, name: &str) -> Option<&ArtifactEventDecl> {
        self.events.iter().flatten().find(|e| e.name == name)
    }
}

/// `emit` (default) or `ask` (the page waits for `artifact.reply`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ArtifactEventKind {
    #[default]
    Emit,
    Ask,
    #[serde(other)]
    Unknown,
}

/// An event an interactive 产出 page may send. Only declared names of the declared kind are
/// accepted.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ArtifactEventDecl {
    pub name: String,
    #[serde(
        default,
        deserialize_with = "non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub label: Option<String>,
    /// 1.1: absent = `emit`.
    #[serde(
        default,
        deserialize_with = "non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub kind: Option<ArtifactEventKind>,
    /// 1.1: the payload's shape (a TcSchema).
    #[serde(
        default,
        deserialize_with = "any_value",
        skip_serializing_if = "Option::is_none"
    )]
    pub schema: Option<Value>,
}

impl ArtifactEventDecl {
    pub fn kind(&self) -> ArtifactEventKind {
        self.kind.unwrap_or_default()
    }
}

const TC_SCHEMA_KEYS: [&str; 11] = [
    "type",
    "properties",
    "required",
    "enum",
    "items",
    "maxItems",
    "minimum",
    "maximum",
    "maxLength",
    "pattern",
    "description",
];

/// Checks a TcSchema: a subset of JSON Schema (type, properties, required, enum, items,
/// maxItems, minimum, maximum, maxLength, pattern <= 200 characters), depth <= 4, no `$ref`.
pub fn tc_schema_problem(schema: &Value, depth: usize) -> Option<String> {
    if depth > 4 {
        return Some("schema is nested deeper than 4".into());
    }
    let Value::Object(m) = schema else {
        return Some("schema is an object".into());
    };
    for (k, v) in m {
        if !TC_SCHEMA_KEYS.contains(&k.as_str()) {
            return Some(format!("schema key {k} is not supported"));
        }
        match (k.as_str(), v) {
            ("pattern", Value::String(p)) if p.chars().count() <= 200 => {}
            ("pattern", _) => return Some("pattern is a string of at most 200 characters".into()),
            ("properties", Value::Object(props)) => {
                if let Some(e) = props.values().find_map(|p| tc_schema_problem(p, depth + 1)) {
                    return Some(e);
                }
            }
            ("properties", _) => return Some("properties is an object".into()),
            ("items", x) => {
                if let Some(e) = tc_schema_problem(x, depth + 1) {
                    return Some(e);
                }
            }
            _ => {}
        }
    }
    None
}

fn check_events(events: Option<&[ArtifactEventDecl]>) -> Validation {
    let Some(events) = events else { return Ok(()) };
    if events.len() > MAX_ARTIFACT_EVENTS {
        return fail("events", format!("at most {MAX_ARTIFACT_EVENTS}"));
    }
    let mut names = std::collections::BTreeSet::new();
    for e in events {
        check_chars("events.name", &e.name, 1, 128)?;
        if !is_event_name(&e.name) {
            return fail("events.name", "1..64 of [a-z][A-Za-z0-9_.-]");
        }
        if !names.insert(e.name.as_str()) {
            return fail("events.name", "event names are unique");
        }
        if let Some(l) = &e.label {
            check_chars("events.label", l, 0, 200)?;
        }
        if e.kind == Some(ArtifactEventKind::Unknown) {
            return fail("events.kind", "emit or ask");
        }
        if let Some(p) = e.schema.as_ref().and_then(|s| tc_schema_problem(s, 1)) {
            return fail("events.schema", p);
        }
    }
    Ok(())
}

fn check_version_extras(media: Option<&MediaInfo>, data: Option<&Value>) -> Validation {
    if let Some(m) = media {
        m.validate()?;
    }
    if data.is_some_and(|d| json_bytes(d) > MAX_ARTIFACT_DATA_BYTES) {
        return fail("data", "data exceeds 256 KiB");
    }
    Ok(())
}

/// A file of a new version: an uploaded (`ready`) file of the same group, at a path.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VersionFileInput {
    pub path: String,
    pub file_id: String,
}

fn check_writable_path(field: &'static str, path: &str) -> Validation {
    check_artifact_path(field, path)?;
    if path.starts_with(RESERVED_ARTIFACT_PREFIX) {
        return fail(field, "paths under __teamchat/ are reserved");
    }
    Ok(())
}

fn check_version_input(entry: &str, files: &[VersionFileInput], note: Option<&str>) -> Validation {
    check_writable_path("entry", entry)?;
    if files.is_empty() || files.len() > MAX_ARTIFACT_FILES {
        return fail("files", format!("1..={MAX_ARTIFACT_FILES} files"));
    }
    for f in files {
        check_writable_path("files.path", &f.path)?;
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
    /// 1.1: events the page may send; accepted from bot members only.
    #[serde(
        default,
        deserialize_with = "non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub events: Option<Vec<ArtifactEventDecl>>,
    /// 1.1: media metadata of the version (computed by the sender).
    #[serde(
        default,
        deserialize_with = "non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub media: Option<MediaInfo>,
    /// 1.1: the first data snapshot (<= 256 KiB).
    #[serde(
        default,
        deserialize_with = "any_value",
        skip_serializing_if = "Option::is_none"
    )]
    pub data: Option<Value>,
    /// 1.1: the caller bot's active run that made it.
    #[serde(
        default,
        deserialize_with = "non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub run_id: Option<String>,
}

impl CreateArtifactRequest {
    pub fn validate(&self) -> Validation {
        check_id("groupId", &self.group_id)?;
        check_id("topicId", &self.topic_id)?;
        check_chars("title", &self.title, 1, MAX_TITLE_CHARS)?;
        check_version_input(&self.entry, &self.files, self.note.as_deref())?;
        check_events(self.events.as_deref())?;
        check_version_extras(self.media.as_ref(), self.data.as_ref())?;
        self.run_id
            .as_deref()
            .map_or(Ok(()), |r| check_id("runId", r))
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
    /// 1.1: as in [`CreateArtifactRequest`].
    #[serde(
        default,
        deserialize_with = "non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub events: Option<Vec<ArtifactEventDecl>>,
    #[serde(
        default,
        deserialize_with = "non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub media: Option<MediaInfo>,
    #[serde(
        default,
        deserialize_with = "any_value",
        skip_serializing_if = "Option::is_none"
    )]
    pub data: Option<Value>,
    #[serde(
        default,
        deserialize_with = "non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub run_id: Option<String>,
}

impl AddArtifactVersionRequest {
    pub fn validate(&self) -> Validation {
        check_version_input(&self.entry, &self.files, self.note.as_deref())?;
        check_events(self.events.as_deref())?;
        check_version_extras(self.media.as_ref(), self.data.as_ref())?;
        if let Some(r) = &self.run_id {
            check_id("runId", r)?;
        }
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

/// `{ t: "artifact.event" }` (1.1): an interactive 产出 page sent an event. `ack.result` is an
/// [`ArtifactEventAck`].
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ArtifactEventFrame {
    pub rid: String,
    pub group_id: String,
    pub artifact_id: String,
    pub version: u32,
    /// 1.1: idempotency key (24 h).
    #[serde(
        default,
        deserialize_with = "non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub client_event_id: Option<String>,
    pub name: String,
    /// 1.1: the declared kind; absent = `emit`.
    #[serde(
        default,
        deserialize_with = "non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub kind: Option<ArtifactEventKind>,
    #[serde(
        default,
        deserialize_with = "any_value",
        skip_serializing_if = "Option::is_none"
    )]
    pub payload: Option<serde_json::Value>,
}

impl ArtifactEventFrame {
    pub fn validate(&self) -> Validation {
        check_id("rid", &self.rid)?;
        check_id("groupId", &self.group_id)?;
        check_id("artifactId", &self.artifact_id)?;
        check_positive("version", Some(self.version))?;
        if let Some(c) = &self.client_event_id {
            check_chars("clientEventId", c, 1, 64)?;
        }
        check_chars("name", &self.name, 1, 128)?;
        if self.kind == Some(ArtifactEventKind::Unknown) {
            return fail("kind", "emit or ask");
        }
        if let Some(p) = &self.payload
            && json_bytes(p) > MAX_ARTIFACT_EVENT_PAYLOAD_BYTES
        {
            return fail("payload", "payload exceeds 16 KiB");
        }
        Ok(())
    }
}

/// `ack.result` of `artifact.event`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ArtifactEventAck {
    pub event_id: String,
    #[serde(
        default,
        deserialize_with = "non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub duplicate: Option<bool>,
    pub expect: Expect,
}

/// `{ t: "artifact.data" }` (1.1): the version's creator bot replaces its data snapshot.
/// `ack.result` is an [`ArtifactDataAck`].
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ArtifactDataFrame {
    pub rid: String,
    pub group_id: String,
    pub artifact_id: String,
    pub version: u32,
    pub data: Value,
    /// When set, must equal the current rev (`conflict`, `artifact.stale_data`).
    #[serde(
        default,
        deserialize_with = "non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub base_data_rev: Option<u64>,
    #[serde(
        default,
        deserialize_with = "non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub cause: Option<String>,
    #[serde(
        default,
        deserialize_with = "non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub client_push_id: Option<String>,
}

impl ArtifactDataFrame {
    pub fn validate(&self) -> Validation {
        check_id("rid", &self.rid)?;
        check_id("groupId", &self.group_id)?;
        check_id("artifactId", &self.artifact_id)?;
        check_positive("version", Some(self.version))?;
        if json_bytes(&self.data) > MAX_ARTIFACT_DATA_BYTES {
            return fail("data", "data exceeds 256 KiB");
        }
        if let Some(c) = &self.cause {
            check_id("cause", c)?;
        }
        if let Some(c) = &self.client_push_id {
            check_chars("clientPushId", c, 1, 64)?;
        }
        Ok(())
    }
}

/// `ack.result` of `artifact.data` and `PUT …/data`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ArtifactDataAck {
    pub data_rev: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ArtifactReplyCode {
    Declined,
    Failed,
    #[serde(other)]
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ArtifactReplyError {
    pub code: ArtifactReplyCode,
    pub message: String,
}

/// `{ t: "artifact.reply" }` (1.1): the bot answers a page `ask` (private to the asker).
/// `ok: true` takes a `payload`, `ok: false` an `error`.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ArtifactReplyFrame {
    pub rid: String,
    pub group_id: String,
    pub event_id: String,
    pub ok: bool,
    #[serde(
        default,
        deserialize_with = "any_value",
        skip_serializing_if = "Option::is_none"
    )]
    pub payload: Option<Value>,
    #[serde(
        default,
        deserialize_with = "non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub error: Option<ArtifactReplyError>,
}

impl ArtifactReplyFrame {
    pub fn validate(&self) -> Validation {
        check_id("rid", &self.rid)?;
        check_id("groupId", &self.group_id)?;
        check_id("eventId", &self.event_id)?;
        let shape = if self.ok {
            self.error.is_none()
        } else {
            self.error.is_some() && self.payload.is_none()
        };
        if !shape {
            return fail("ok", "ok: true takes a payload, ok: false an error");
        }
        if let Some(e) = &self.error {
            if e.code == ArtifactReplyCode::Unknown {
                return fail("error.code", "declined or failed");
            }
            check_chars("error.message", &e.message, 0, 200)?;
        }
        if self
            .payload
            .as_ref()
            .is_some_and(|p| json_bytes(p) > MAX_ARTIFACT_REPLY_BYTES)
        {
            return fail("payload", "payload exceeds 64 KiB");
        }
        Ok(())
    }
}

/// Transient (1.1, to the version's creator bot): a page event.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ArtifactEventTransient {
    pub group_id: String,
    pub topic_id: String,
    pub artifact_id: String,
    pub version: u32,
    pub event_id: String,
    pub name: String,
    pub kind: ArtifactEventKind,
    #[serde(
        default,
        deserialize_with = "any_value",
        skip_serializing_if = "Option::is_none"
    )]
    pub payload: Option<Value>,
    pub actor_id: String,
    pub at: Millis,
}

/// Transient (1.1, to every member's connections): a new data snapshot; `data` inline when
/// it is at most 64 KiB, else absent (`GET …/data`).
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ArtifactDataTransient {
    pub group_id: String,
    pub artifact_id: String,
    pub version: u32,
    pub data_rev: u64,
    #[serde(
        default,
        deserialize_with = "any_value",
        skip_serializing_if = "Option::is_none"
    )]
    pub data: Option<Value>,
    #[serde(
        default,
        deserialize_with = "non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub cause: Option<String>,
}

/// Transient (1.1, to the asker's connections only): the bot's answer to a page `ask`.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ArtifactReplyTransient {
    pub group_id: String,
    pub event_id: String,
    pub ok: bool,
    #[serde(
        default,
        deserialize_with = "any_value",
        skip_serializing_if = "Option::is_none"
    )]
    pub payload: Option<Value>,
    #[serde(
        default,
        deserialize_with = "non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub error: Option<ArtifactReplyError>,
}

/// `GET /v1/artifacts/:id/versions/:v/data` (group members).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ArtifactDataResponse {
    /// `null` when the version has no data.
    pub data: Value,
    pub data_rev: u64,
    pub updated_at: Millis,
}

/// `PUT /v1/artifacts/:id/versions/:v/data` (the version's creator bot). The response is an
/// [`ArtifactDataAck`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PutArtifactDataRequest {
    pub data: Value,
    #[serde(
        default,
        deserialize_with = "non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub base_data_rev: Option<u64>,
    #[serde(
        default,
        deserialize_with = "non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub cause: Option<String>,
}

impl PutArtifactDataRequest {
    pub fn validate(&self) -> Validation {
        if json_bytes(&self.data) > MAX_ARTIFACT_DATA_BYTES {
            return fail("data", "data exceeds 256 KiB");
        }
        self.cause
            .as_deref()
            .map_or(Ok(()), |c| check_id("cause", c))
    }
}

/// `PUT /v1/artifacts/:id/versions/:v/media` (the version's creator): the media of a version
/// with its preview, sent after the upload. Stored as `artifact.previewReady`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PutArtifactMediaRequest {
    pub media: MediaInfo,
}

impl PutArtifactMediaRequest {
    pub fn validate(&self) -> Validation {
        check_version_extras(Some(&self.media), None)
    }
}

/// Answer of `PUT /v1/artifacts/:id/versions/:v/media`: the version with the new media.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PutArtifactMediaResponse {
    pub version: ArtifactVersion,
}
