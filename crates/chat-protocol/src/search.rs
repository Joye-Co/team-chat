//! Message search (docs/protocol.md, "Search").

use serde::{Deserialize, Serialize};

use crate::entities::{Millis, Seq};
use crate::limits::{Validation, ValidationError, check_chars, check_id};
use crate::serde_util::non_null;

pub const MAX_SEARCH_LIMIT: u32 = 50;
pub const MAX_QUERY_CHARS: usize = 200;
pub const MAX_SNIPPET_CHARS: usize = 300;

/// `{ t: "search" }`: answered by `searchResults` (or an error `ack`) with the same rid.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchFrame {
    pub rid: String,
    pub query: String,
    /// Absent: every group I am a member of.
    #[serde(
        default,
        deserialize_with = "non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub group_id: Option<String>,
    /// Needs `groupId`.
    #[serde(
        default,
        deserialize_with = "non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub topic_id: Option<String>,
    /// Default 20.
    #[serde(
        default,
        deserialize_with = "non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub limit: Option<u32>,
}

impl SearchFrame {
    pub fn validate(&self) -> Validation {
        check_id("rid", &self.rid)?;
        check_chars("query", &self.query, 1, MAX_QUERY_CHARS)?;
        if let Some(g) = &self.group_id {
            check_id("groupId", g)?;
        }
        if let Some(t) = &self.topic_id {
            check_id("topicId", t)?;
        }
        match self.limit {
            Some(l) if !(1..=MAX_SEARCH_LIMIT).contains(&l) => Err(ValidationError {
                field: "limit",
                message: format!("must be 1..={MAX_SEARCH_LIMIT}"),
            }),
            _ => Ok(()),
        }
    }
}

/// A matched range in `snippet`, in Unicode code points.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct SearchHighlight {
    pub start: u32,
    pub length: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchHit {
    pub group_id: String,
    pub topic_id: String,
    pub message_id: String,
    pub author_id: String,
    /// Plain text around the match.
    pub snippet: String,
    #[serde(
        default,
        deserialize_with = "non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub highlights: Option<Vec<SearchHighlight>>,
    pub created_at: Millis,
    /// The message's `createdSeq`.
    pub seq: Seq,
}

/// `{ t: "searchResults" }`, best match first.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchResultsFrame {
    pub rid: String,
    pub hits: Vec<SearchHit>,
}
