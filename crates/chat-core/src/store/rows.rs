//! Row types returned to the UI and their SQL mapping.

use crate::proto::{Group, Member, Message, TopicStatus, Topic};
use rusqlite::Row;
use rusqlite::types::Value;
use serde::{Deserialize, Serialize, de::DeserializeOwned};

/// A group with its summary fields, for the sidebar.
#[derive(Debug, Clone, PartialEq)]
pub struct GroupRow {
    pub group: Group,
    /// Newest seq known for the group (server summary or applied events).
    pub last_seq: u64,
    /// My group read cursor.
    pub read_seq: u64,
    pub unread: u64,
    pub mentions: u64,
    /// Time of the newest cached activity, 0 when none is cached.
    pub last_activity_at: u64,
}

/// A topic with per-topic unread state, for the topic list.
#[derive(Debug, Clone, PartialEq)]
pub struct TopicRow {
    pub topic: Topic,
    pub last_activity_at: u64,
    /// Every older message of the topic is cached.
    pub history_complete: bool,
    /// Messages after my read position that I did not write.
    pub unread: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutboxState {
    /// Waiting to be sent or for its ack.
    Pending,
    /// The server refused it; the UI may retry or discard.
    Failed,
}

impl OutboxState {
    pub fn as_str(self) -> &'static str {
        match self {
            OutboxState::Pending => "pending",
            OutboxState::Failed => "failed",
        }
    }
}

/// One queued write. Sends are keyed by their `clientMsgId`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum OutboxOp {
    Send {
        group_id: String,
        topic_id: Option<String>,
        title: Option<String>,
        body: String,
        mentions: Vec<String>,
        /// A bot reply that `delta` frames and a final edit complete.
        #[serde(default)]
        streaming: bool,
    },
    Edit {
        group_id: String,
        topic_id: Option<String>,
        message_id: String,
        body: String,
        #[serde(default)]
        streaming: bool,
    },
    Delete {
        group_id: String,
        topic_id: Option<String>,
        message_id: String,
    },
    TopicUpdate {
        group_id: String,
        topic_id: String,
        title: Option<String>,
        status: Option<TopicStatus>,
    },
}

impl OutboxOp {
    pub fn kind(&self) -> &'static str {
        match self {
            OutboxOp::Send { .. } => "send",
            OutboxOp::Edit { .. } => "edit",
            OutboxOp::Delete { .. } => "delete",
            OutboxOp::TopicUpdate { .. } => "topic_update",
        }
    }
    pub fn group_id(&self) -> &str {
        match self {
            OutboxOp::Send { group_id, .. }
            | OutboxOp::Edit { group_id, .. }
            | OutboxOp::Delete { group_id, .. }
            | OutboxOp::TopicUpdate { group_id, .. } => group_id,
        }
    }
    pub fn topic_id(&self) -> Option<&str> {
        match self {
            OutboxOp::Send { topic_id, .. }
            | OutboxOp::Edit { topic_id, .. }
            | OutboxOp::Delete { topic_id, .. } => topic_id.as_deref(),
            OutboxOp::TopicUpdate { topic_id, .. } => Some(topic_id),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct OutboxItem {
    /// `clientMsgId` for a send; a local ULID for other writes.
    pub id: String,
    pub op: OutboxOp,
    pub attempts: u64,
    pub last_error: Option<String>,
    pub state: OutboxState,
    pub created_at: u64,
}

pub(crate) fn outbox_from_row(r: &Row<'_>) -> rusqlite::Result<OutboxItem> {
    let payload: String = r.get(1)?;
    let state: String = r.get(4)?;
    Ok(OutboxItem {
        id: r.get(0)?,
        op: serde_json::from_str(&payload).map_err(json_err)?,
        attempts: r.get(2)?,
        last_error: r.get(3)?,
        state: if state == "failed" { OutboxState::Failed } else { OutboxState::Pending },
        created_at: r.get(5)?,
    })
}

fn json_err(e: serde_json::Error) -> rusqlite::Error {
    rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, Box::new(e))
}

/// The serde string of a unit enum value, such as `"in_progress"`.
pub(crate) fn enum_str<T: Serialize>(v: &T) -> Result<String, serde_json::Error> {
    match serde_json::to_value(v)? {
        serde_json::Value::String(s) => Ok(s),
        other => Ok(other.to_string()),
    }
}

fn enum_from<T: DeserializeOwned>(s: String) -> rusqlite::Result<T> {
    serde_json::from_value(serde_json::Value::String(s)).map_err(json_err)
}

pub(crate) const MEMBER_COLS: &str = "id, kind, name, role, level, avatar_url, deactivated";

pub(crate) fn member_from_row(r: &Row<'_>) -> rusqlite::Result<Member> {
    let deactivated: bool = r.get(6)?;
    let role: Option<String> = r.get(3)?;
    let avatar_url: Option<String> = r.get(5)?;
    Ok(Member {
        id: r.get(0)?,
        kind: enum_from(r.get(1)?)?,
        name: r.get(2)?,
        role: role.map(Some),
        level: enum_from(r.get(4)?)?,
        avatar_url: avatar_url.map(Some),
        deactivated: Some(deactivated),
    })
}

pub(crate) const GROUP_COLS: &str =
    "id, kind, name, member_ids, created_by, created_at, last_seq, read_seq, unread, mentions, last_activity_at";

pub(crate) fn group_from_row(r: &Row<'_>) -> rusqlite::Result<GroupRow> {
    let ids: String = r.get(3)?;
    Ok(GroupRow {
        group: Group {
            id: r.get(0)?,
            kind: enum_from(r.get(1)?)?,
            name: r.get(2)?,
            member_ids: serde_json::from_str(&ids).map_err(json_err)?,
            created_by: r.get(4)?,
            created_at: r.get(5)?,
        },
        last_seq: r.get(6)?,
        read_seq: r.get(7)?,
        unread: r.get(8)?,
        mentions: r.get(9)?,
        last_activity_at: r.get(10)?,
    })
}

pub(crate) const TOPIC_COLS: &str = "t.id, t.group_id, t.title, t.status, t.created_by, t.created_at, \
     t.root_message_id, t.last_seq, t.message_count, t.last_activity_at, t.history_complete";

/// Unread messages of topic `t` for member `?2`.
pub(crate) const TOPIC_UNREAD_SQL: &str = "SELECT COUNT(*) FROM messages m WHERE m.topic_id = t.id AND m.author_id != ?2 \
     AND m.created_seq > COALESCE((SELECT g.read_seq FROM groups g WHERE g.id = t.group_id), 0) \
     AND m.created_seq > COALESCE((SELECT rc.seq FROM read_cursors rc WHERE rc.group_id = t.group_id AND rc.topic_id = t.id), 0)";

pub(crate) fn topic_from_row(r: &Row<'_>) -> rusqlite::Result<TopicRow> {
    Ok(TopicRow {
        topic: Topic {
            id: r.get(0)?,
            group_id: r.get(1)?,
            title: r.get(2)?,
            status: enum_from(r.get(3)?)?,
            created_by: r.get(4)?,
            created_at: r.get(5)?,
            root_message_id: r.get(6)?,
            last_seq: r.get(7)?,
            message_count: r.get(8)?,
        },
        last_activity_at: r.get(9)?,
        history_complete: r.get(10)?,
        unread: r.get(11)?,
    })
}

pub(crate) const MESSAGE_COLS: &str = "id, client_msg_id, group_id, topic_id, author_id, body, mentions, \
     created_seq, created_at, edited_at, deleted_at, streaming";

pub(crate) fn message_values(m: &Message) -> Result<Vec<Value>, serde_json::Error> {
    Ok(vec![
        Value::Text(m.id.clone()),
        Value::Text(m.client_msg_id.clone()),
        Value::Text(m.group_id.clone()),
        Value::Text(m.topic_id.clone()),
        Value::Text(m.author_id.clone()),
        Value::Text(if m.is_deleted() { String::new() } else { m.body.clone() }),
        Value::Text(serde_json::to_string(&m.mentions)?),
        Value::Integer(m.created_seq as i64),
        Value::Integer(m.created_at as i64),
        m.edited_at().map_or(Value::Null, |v| Value::Integer(v as i64)),
        m.deleted_at().map_or(Value::Null, |v| Value::Integer(v as i64)),
        Value::Integer(m.is_streaming() as i64),
    ])
}

pub(crate) fn message_from_row(r: &Row<'_>) -> rusqlite::Result<Message> {
    let mentions: String = r.get(6)?;
    let streaming: bool = r.get(11)?;
    let edited_at: Option<u64> = r.get(9)?;
    let deleted_at: Option<u64> = r.get(10)?;
    Ok(Message {
        id: r.get(0)?,
        client_msg_id: r.get(1)?,
        group_id: r.get(2)?,
        topic_id: r.get(3)?,
        author_id: r.get(4)?,
        body: r.get(5)?,
        mentions: serde_json::from_str(&mentions).map_err(json_err)?,
        created_seq: r.get(7)?,
        created_at: r.get(8)?,
        edited_at: edited_at.map(Some),
        deleted_at: deleted_at.map(Some),
        streaming: streaming.then_some(true),
    })
}
