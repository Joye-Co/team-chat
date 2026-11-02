//! Members, groups, topics and messages.

use serde::{Deserialize, Serialize};

use crate::serde_util::{non_null, nullable};

/// Milliseconds since the Unix epoch, UTC.
pub type Millis = u64;
/// Position in a group event log.
pub type Seq = u64;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MemberKind {
    Human,
    Bot,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MemberLevel {
    Owner,
    Member,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Member {
    pub id: String,
    pub kind: MemberKind,
    pub name: String,
    /// `None` = omitted, `Some(None)` = `null`.
    #[serde(
        default,
        deserialize_with = "nullable",
        skip_serializing_if = "Option::is_none"
    )]
    pub role: Option<Option<String>>,
    pub level: MemberLevel,
    /// `None` = omitted, `Some(None)` = `null`.
    #[serde(
        default,
        deserialize_with = "nullable",
        skip_serializing_if = "Option::is_none"
    )]
    pub avatar_url: Option<Option<String>>,
    #[serde(
        default,
        deserialize_with = "non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub deactivated: Option<bool>,
}

impl Member {
    pub fn role(&self) -> Option<&str> {
        self.role.as_ref().and_then(|r| r.as_deref())
    }
    pub fn avatar_url(&self) -> Option<&str> {
        self.avatar_url.as_ref().and_then(|r| r.as_deref())
    }
    pub fn is_deactivated(&self) -> bool {
        self.deactivated.unwrap_or(false)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GroupKind {
    Group,
    Dm,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Group {
    pub id: String,
    pub kind: GroupKind,
    pub name: String,
    pub member_ids: Vec<String>,
    pub created_by: String,
    pub created_at: Millis,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GroupSummary {
    pub group: Group,
    pub last_seq: Seq,
    pub read_seq: Seq,
    pub unread: u64,
    pub mentions: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TopicStatus {
    Open,
    InProgress,
    WaitingAnswer,
    WaitingReview,
    Blocked,
    Done,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Topic {
    pub id: String,
    pub group_id: String,
    pub title: String,
    pub status: TopicStatus,
    pub created_by: String,
    pub created_at: Millis,
    pub root_message_id: String,
    pub last_seq: Seq,
    pub message_count: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Message {
    pub id: String,
    pub client_msg_id: String,
    pub group_id: String,
    pub topic_id: String,
    pub author_id: String,
    /// Markdown. Empty for a deleted message.
    pub body: String,
    pub mentions: Vec<String>,
    pub created_seq: Seq,
    pub created_at: Millis,
    /// `None` = omitted, `Some(None)` = `null`.
    #[serde(
        default,
        deserialize_with = "nullable",
        skip_serializing_if = "Option::is_none"
    )]
    pub edited_at: Option<Option<Millis>>,
    /// `None` = omitted, `Some(None)` = `null`.
    #[serde(
        default,
        deserialize_with = "nullable",
        skip_serializing_if = "Option::is_none"
    )]
    pub deleted_at: Option<Option<Millis>>,
    #[serde(
        default,
        deserialize_with = "non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub streaming: Option<bool>,
}

impl Message {
    pub fn edited_at(&self) -> Option<Millis> {
        self.edited_at.flatten()
    }
    pub fn deleted_at(&self) -> Option<Millis> {
        self.deleted_at.flatten()
    }
    pub fn is_deleted(&self) -> bool {
        self.deleted_at().is_some()
    }
    pub fn is_streaming(&self) -> bool {
        self.streaming.unwrap_or(false)
    }
}
