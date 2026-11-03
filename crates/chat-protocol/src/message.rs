//! Message extensions: quotes, attachments, 产出 references and message kinds
//! (docs/protocol.md, "Messages: quotes, attachments, cards").

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::card::check_action_id;
use crate::card2::{PartRef, json_bytes};
use crate::limits::{Validation, ValidationError, check_chars, check_id};
use crate::media::MediaInfo;
use crate::serde_util::{any_value, non_null};

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
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
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
    /// 1.1: what the file is, for inline previews (computed by the sender).
    #[serde(
        default,
        deserialize_with = "non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub media: Option<MediaInfo>,
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

// ---------------------------------------------------------------------------
// References to a part (引用提问), 1.1
// ---------------------------------------------------------------------------

/// Max `blockRefs` of one message.
pub const MAX_BLOCK_REFS: usize = 5;
/// Max code points of a ref label.
pub const MAX_BLOCK_REF_LABEL_CHARS: usize = 120;
/// Max UTF-8 bytes of a ref `value` as JSON.
pub const MAX_BLOCK_REF_VALUE_BYTES: usize = 2048;
/// Max v1 card block index (`v1:<0..49>`) and Markdown block index (`md:<0..199>`).
pub const MAX_BLOCK_REF_V1_INDEX: u32 = 49;
pub const MAX_BLOCK_REF_MD_INDEX: u32 = 199;

/// A ref's `blockId`: a Card v2 block id, `card` (the whole card), `page` (a 产出 page),
/// `v1:<index>` (a v1 card block) or `md:<index>` (a top-level Markdown block of the body).
pub fn is_block_ref_id(id: &str) -> bool {
    for (prefix, max) in [
        ("v1:", MAX_BLOCK_REF_V1_INDEX),
        ("md:", MAX_BLOCK_REF_MD_INDEX),
    ] {
        if let Some(n) = id.strip_prefix(prefix) {
            let digits = !n.is_empty() && n.len() <= 3 && n.bytes().all(|b| b.is_ascii_digit());
            let canonical = n == "0" || !n.starts_with('0');
            return digits && canonical && n.parse::<u32>().is_ok_and(|v| v <= max);
        }
    }
    check_action_id("blockId", id).is_ok()
}

/// A 产出 version a ref points at.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ArtifactVersionRef {
    pub artifact_id: String,
    pub version: u32,
}

/// What a client sends: a reference to one part of a message block or a 产出 page. Exactly
/// one of `message_id` / `artifact`.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BlockRefInput {
    #[serde(
        default,
        deserialize_with = "non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub message_id: Option<String>,
    #[serde(
        default,
        deserialize_with = "non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub artifact: Option<ArtifactVersionRef>,
    /// The Card v2 `rev` the sender saw.
    #[serde(
        default,
        deserialize_with = "non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub card_rev: Option<u32>,
    pub block_id: String,
    /// Absent = the whole block.
    #[serde(
        default,
        deserialize_with = "non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub part: Option<PartRef>,
    pub label: String,
    /// The sender's snapshot of what they saw (<= 2048 bytes as JSON).
    #[serde(
        default,
        deserialize_with = "any_value",
        skip_serializing_if = "Option::is_none"
    )]
    pub value: Option<Value>,
}

/// What the server stores and sends. A deleted source: `label` "", no `value`, `deleted`.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BlockRef {
    #[serde(flatten)]
    pub source: BlockRefInput,
    /// Author of the source message, or creator of the 产出 version.
    #[serde(
        default,
        deserialize_with = "non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub source_author_id: Option<String>,
    pub source_topic_id: String,
    #[serde(
        default,
        deserialize_with = "non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub deleted: Option<crate::serde_util::True>,
}

fn check_ref_label(label: &str, min: usize) -> Validation {
    check_chars("blockRefs.label", label, min, MAX_BLOCK_REF_LABEL_CHARS)?;
    if label.chars().any(|c| (c as u32) < 0x20 || c == '\u{7f}') {
        return fail("blockRefs.label", "no control characters");
    }
    Ok(())
}

impl BlockRefInput {
    fn check(&self, label_min: usize) -> Validation {
        if self.message_id.is_some() == self.artifact.is_some() {
            return fail("blockRefs", "exactly one of messageId and artifact");
        }
        if let Some(m) = &self.message_id {
            check_id("blockRefs.messageId", m)?;
        }
        if let Some(a) = &self.artifact {
            check_id("blockRefs.artifact.artifactId", &a.artifact_id)?;
            check_positive("blockRefs.artifact.version", Some(a.version))?;
        }
        check_positive("blockRefs.cardRev", self.card_rev)?;
        if !is_block_ref_id(&self.block_id) {
            return fail(
                "blockRefs.blockId",
                "a block id, card, page, v1:<n> or md:<n>",
            );
        }
        if let Some(p) = &self.part {
            p.validate()?;
        }
        check_ref_label(&self.label, label_min)?;
        if self
            .value
            .as_ref()
            .is_some_and(|v| json_bytes(v) > MAX_BLOCK_REF_VALUE_BYTES)
        {
            return fail("blockRefs.value", "value exceeds 2048 bytes");
        }
        Ok(())
    }

    pub fn validate(&self) -> Validation {
        self.check(1)
    }
}

impl BlockRef {
    pub fn validate(&self) -> Validation {
        self.source.check(0)?;
        check_id("blockRefs.sourceTopicId", &self.source_topic_id)
    }
}

/// Checks `send.blockRefs`: 1..=5 refs.
pub fn check_block_refs(refs: Option<&[BlockRefInput]>) -> Validation {
    let Some(r) = refs else { return Ok(()) };
    if r.is_empty() || r.len() > MAX_BLOCK_REFS {
        return fail("blockRefs", format!("1..={MAX_BLOCK_REFS} refs"));
    }
    r.iter().try_for_each(BlockRefInput::validate)
}

/// The quote lines the server writes in front of the body, one per ref label.
pub fn block_ref_prefix<'a>(labels: impl IntoIterator<Item = &'a str>) -> String {
    labels
        .into_iter()
        .map(|l| format!("> ↳ {l}"))
        .collect::<Vec<_>>()
        .join("\n")
}
