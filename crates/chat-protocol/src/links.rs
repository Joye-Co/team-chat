//! Topic links (docs/protocol.md, "Topic links"): structured state of things a topic points
//! at, such as a pull request with its checks. Carried by the `topic.linked` event.

use serde::{Deserialize, Serialize};

use crate::card::check_web_url;
use crate::limits::{Validation, ValidationError, check_chars, check_id};
use crate::serde_util::non_null;

pub const MAX_LINK_CHECKS: usize = 100;

fn fail(field: &'static str, message: impl Into<String>) -> Validation {
    Err(ValidationError {
        field,
        message: message.into(),
    })
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PrCheck {
    pub name: String,
    /// GitHub conclusion in plain text ("success", "failure", ...); "pending" while it runs.
    pub conclusion: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PrLink {
    /// `owner/name`.
    pub repo: String,
    pub number: u64,
    pub url: String,
    pub title: String,
    /// Plain text: "open", "draft", "merged" or "closed".
    pub state: String,
    pub checks: Vec<PrCheck>,
    #[serde(
        default,
        deserialize_with = "non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub additions: Option<u64>,
    #[serde(
        default,
        deserialize_with = "non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub deletions: Option<u64>,
    #[serde(
        default,
        deserialize_with = "non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub changed_files: Option<u64>,
    /// 1.1: the pull request waits to merge (merge queue or auto-merge); absent = not queued.
    #[serde(
        default,
        deserialize_with = "non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub merge: Option<PrMerge>,
    /// The pull request's diff, uploaded by a run's computer; `None` = not known.
    #[serde(
        default,
        deserialize_with = "non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub diff: Option<PrDiffRef>,
}

/// At most this many files in a [`PrDiff`].
pub const MAX_PR_DIFF_FILES: usize = 300;
/// The longest patch of one file in a [`PrDiff`] (characters); a longer one is left out.
pub const MAX_PR_PATCH_CHARS: usize = 65_536;

/// The diff of a pull request, uploaded as a JSON file ([`PrDiff`]) by the computer of the run
/// that named it. `size` and `sha256` let a client cache it.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PrDiffRef {
    pub file_id: String,
    pub name: String,
    pub size: u64,
    pub sha256: String,
}

impl PrDiffRef {
    pub fn validate(&self) -> Validation {
        check_chars("diff.fileId", &self.file_id, 1, 64)?;
        check_chars("diff.name", &self.name, 1, 200)?;
        crate::message::check_sha256("diff.sha256", &self.sha256)
    }
}

/// One file of a pull request's diff, as GitHub lists it.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PrDiffFile {
    pub path: String,
    /// The old path of a renamed file.
    #[serde(
        default,
        deserialize_with = "non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub previous_path: Option<String>,
    /// GitHub's word: "added", "removed", "modified", "renamed", "copied", "changed", "unchanged".
    pub status: String,
    pub additions: u64,
    pub deletions: u64,
    /// The file's unified diff hunks; `None` for a binary file or one left out for its size.
    #[serde(
        default,
        deserialize_with = "non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub patch: Option<String>,
}

/// The content of a [`PrDiffRef`] file. `truncated`: the pull request has more files.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PrDiff {
    pub files: Vec<PrDiffFile>,
    #[serde(
        default,
        deserialize_with = "non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub truncated: Option<bool>,
}

impl PrDiff {
    pub fn validate(&self) -> Validation {
        if self.files.len() > MAX_PR_DIFF_FILES {
            return fail("diff.files", format!("at most {MAX_PR_DIFF_FILES}"));
        }
        for f in &self.files {
            check_chars("diff.files.path", &f.path, 1, 1024)?;
            if let Some(p) = &f.previous_path {
                check_chars("diff.files.previousPath", p, 1, 1024)?;
            }
            check_chars("diff.files.status", &f.status, 1, 32)?;
            if let Some(p) = &f.patch {
                check_chars("diff.files.patch", p, 0, MAX_PR_PATCH_CHARS)?;
            }
        }
        Ok(())
    }
}

/// 1.1: a queued merge of a pull request, from GitHub (`enqueued`, `auto_merge_enabled`,
/// `dequeued`, `auto_merge_disabled`). `error` is set when the merge stopped without merging.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PrMerge {
    pub queued_at: crate::entities::Millis,
    /// Plain text from GitHub, e.g. "merge conflict", "checks failed".
    #[serde(
        default,
        deserialize_with = "non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub error: Option<String>,
}

/// A link of a topic. A link with the same kind and `url` replaces the older one.
#[allow(clippy::large_enum_variant)] // frames are short-lived; boxing would change the public API
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind")]
pub enum TopicLink {
    #[serde(rename = "pr")]
    Pr(PrLink),
    /// A kind this version does not know. Readers skip it.
    #[serde(other)]
    Unknown,
}

impl TopicLink {
    /// The link's identity within its topic: kind and URL.
    pub fn key(&self) -> Option<(&'static str, &str)> {
        match self {
            Self::Pr(p) => Some(("pr", &p.url)),
            Self::Unknown => None,
        }
    }

    pub fn validate(&self) -> Validation {
        match self {
            Self::Pr(p) => {
                check_chars("link.repo", &p.repo, 1, 200)?;
                if p.number == 0 {
                    return fail("link.number", "must be positive");
                }
                check_web_url("link.url", &p.url)?;
                check_chars("link.title", &p.title, 0, 500)?;
                check_chars("link.state", &p.state, 1, 32)?;
                if p.checks.len() > MAX_LINK_CHECKS {
                    return fail("link.checks", format!("at most {MAX_LINK_CHECKS}"));
                }
                for c in &p.checks {
                    check_chars("link.checks.name", &c.name, 1, 200)?;
                    check_chars("link.checks.conclusion", &c.conclusion, 1, 32)?;
                }
                if let Some(e) = p.merge.as_ref().and_then(|m| m.error.as_deref()) {
                    check_chars("link.merge.error", e, 1, 200)?;
                }
                if let Some(d) = &p.diff {
                    d.validate()?;
                }
                Ok(())
            }
            Self::Unknown => fail("link.kind", "unknown link kind"),
        }
    }
}

/// Data of `topic.linked`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TopicLinkedData {
    pub topic_id: String,
    pub link: TopicLink,
}

impl TopicLinkedData {
    pub fn validate(&self) -> Validation {
        check_id("topicId", &self.topic_id)?;
        self.link.validate()
    }
}
