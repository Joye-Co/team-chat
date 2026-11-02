//! Message extensions: quotes, attachments, 产出 references and message kinds
//! (docs/protocol.md, "Messages: quotes, attachments, cards").

use serde::{Deserialize, Serialize};

use crate::limits::{Validation, ValidationError, check_chars, check_id};
use crate::serde_util::non_null;

pub const MAX_ATTACHMENTS: usize = 20;
pub const MAX_ARTIFACT_REFS: usize = 20;
/// `replyTo.excerpt` is the first this-many code points of the quoted body.
pub const MAX_EXCERPT_CHARS: usize = 200;
/// Max file name length, in code points.
pub const MAX_FILE_NAME_CHARS: usize = 255;

fn fail(field: &'static str, message: impl Into<String>) -> Validation {
    Err(ValidationError {
        field,
        message: message.into(),
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MessageKind {
    #[default]
    Text,
    Card,
    System,
}

/// A quoted message. The server fills `authorId` and `excerpt`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReplyTo {
    pub message_id: String,
    pub author_id: String,
    pub excerpt: String,
}

/// What a client sends to quote a message.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReplyToRef {
    pub message_id: String,
}

/// An uploaded file, as a message attachment.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileRef {
    pub file_id: String,
    pub name: String,
    pub size: u64,
    pub mime: String,
    /// Pixel size, images only.
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
    pub sha256: String,
}

/// A reference to one version of a 产出.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ArtifactRef {
    pub artifact_id: String,
    pub version: u32,
}

/// Lowercase hex SHA-256 digest.
pub fn check_sha256(field: &'static str, v: &str) -> Validation {
    if v.len() == 64 && v.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f')) {
        Ok(())
    } else {
        fail(field, "must be 64 lowercase hex digits")
    }
}

/// `type/subtype` with letters, digits, `_ . + -`; 3..=255 characters.
pub fn check_mime(field: &'static str, v: &str) -> Validation {
    check_chars(field, v, 3, 255)?;
    let ok_part = |p: &str| {
        !p.is_empty()
            && p.chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | '+' | '-'))
    };
    match v.split_once('/') {
        Some((a, b)) if ok_part(a) && ok_part(b) => Ok(()),
        _ => fail(field, "must be a MIME type"),
    }
}

pub fn check_positive(field: &'static str, v: Option<u32>) -> Validation {
    if v == Some(0) {
        fail(field, "must be positive")
    } else {
        Ok(())
    }
}

impl FileRef {
    pub fn validate(&self) -> Validation {
        check_id("attachments.fileId", &self.file_id)?;
        check_chars("attachments.name", &self.name, 1, MAX_FILE_NAME_CHARS)?;
        check_mime("attachments.mime", &self.mime)?;
        check_positive("attachments.width", self.width)?;
        check_positive("attachments.height", self.height)?;
        check_sha256("attachments.sha256", &self.sha256)
    }
}

/// Checks `attachments` and `artifactRefs` lists of a send or edit.
pub fn check_refs(
    attachments: Option<&[FileRef]>,
    artifacts: Option<&[ArtifactRef]>,
) -> Validation {
    if let Some(a) = attachments {
        if a.len() > MAX_ATTACHMENTS {
            return fail("attachments", format!("at most {MAX_ATTACHMENTS}"));
        }
        a.iter().try_for_each(FileRef::validate)?;
    }
    if let Some(a) = artifacts {
        if a.len() > MAX_ARTIFACT_REFS {
            return fail("artifactRefs", format!("at most {MAX_ARTIFACT_REFS}"));
        }
        for r in a {
            check_id("artifactRefs.artifactId", &r.artifact_id)?;
            check_positive("artifactRefs.version", Some(r.version))?;
        }
    }
    Ok(())
}
