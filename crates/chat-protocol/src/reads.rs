//! Per-topic read positions, unread and mention counts, and follow state
//! (docs/protocol.md, "Read positions per topic").

use serde::{Deserialize, Serialize};

use crate::entities::Seq;
use crate::serde_util::non_null;

/// One member's state in one topic.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TopicState {
    pub topic_id: String,
    /// Messages of this topic with `createdSeq <= readSeq` are read.
    pub read_seq: Seq,
    pub unread: u64,
    pub mentions: u64,
    /// Absent means false.
    #[serde(
        default,
        deserialize_with = "non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub following: Option<bool>,
}

impl TopicState {
    pub fn is_following(&self) -> bool {
        self.following.unwrap_or(false)
    }
}

/// `{ t: "reads" }`: my read state in a group changed. Group totals plus the changed topics.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReadsFrame {
    pub group_id: String,
    pub read_seq: Seq,
    pub unread: u64,
    pub mentions: u64,
    pub topics: Vec<TopicState>,
}
