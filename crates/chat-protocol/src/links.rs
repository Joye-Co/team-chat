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
}

/// A link of a topic. A link with the same kind and `url` replaces the older one.
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
