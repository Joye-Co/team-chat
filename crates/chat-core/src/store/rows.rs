//! Row types returned to the UI and their SQL mapping.
//!
//! Topics and messages keep the full JSON of the entity in a `raw` column, so fields
//! added to the protocol later survive the cache; the typed columns hold what the
//! queries and the event application need and override `raw` on read.

use crate::proto::{
    ArtifactRef, Card, CharacterId, FileRef, Group, Member, Message, NotifyLevel, ReplyToRef,
    Topic, TopicStatus,
};
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
    /// Group totals over all topics.
    pub unread: u64,
    pub mentions: u64,
    /// Time of the newest cached activity, 0 when none is cached.
    pub last_activity_at: u64,
    pub muted: bool,
    /// `None` = the default: all for a DM, mentions for a group.
    pub notify: Option<NotifyLevel>,
}

impl GroupRow {
    pub fn notify_level(&self) -> NotifyLevel {
        self.notify.unwrap_or(match self.group.kind {
            crate::proto::GroupKind::Dm => NotifyLevel::All,
            _ => NotifyLevel::Mentions,
        })
    }
}

/// A topic with my read state, for the topic list.
#[derive(Debug, Clone, PartialEq)]
pub struct TopicRow {
    pub topic: Topic,
    pub last_activity_at: u64,
    /// Every older message of the topic is cached.
    pub history_complete: bool,
    /// Effective read position: max(group cursor, topic cursor).
    pub read_seq: u64,
    /// Unread messages (not mine, not deleted, not system) after `read_seq`.
    pub unread: u64,
    pub mentions: u64,
    pub following: bool,
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

/// One queued write. Sends are keyed by their `clientMsgId`. New optional fields must
/// use `#[serde(default)]` so rows written by older builds still decode.
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
        #[serde(default)]
        reply_to: Option<String>,
        #[serde(default)]
        attachments: Option<Vec<FileRef>>,
        #[serde(default)]
        artifact_refs: Option<Vec<ArtifactRef>>,
        #[serde(default)]
        card: Option<Card>,
    },
    Edit {
        group_id: String,
        topic_id: Option<String>,
        message_id: String,
        body: String,
        #[serde(default)]
        streaming: bool,
        #[serde(default)]
        attachments: Option<Vec<FileRef>>,
        #[serde(default)]
        artifact_refs: Option<Vec<ArtifactRef>>,
        #[serde(default)]
        card: Option<Card>,
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
        #[serde(default)]
        waiting_on: Option<Vec<String>>,
        /// `Some(None)` clears the owner.
        #[serde(default)]
        owner_id: Option<Option<String>>,
    },
    TopicFollow {
        group_id: String,
        topic_id: String,
        follow: bool,
    },
    CardAction {
        group_id: String,
        topic_id: Option<String>,
        message_id: String,
        action_id: String,
    },
    /// Adds (`on`) or removes my reaction.
    React {
        group_id: String,
        topic_id: Option<String>,
        message_id: String,
        emoji: String,
        on: bool,
    },
}

impl OutboxOp {
    pub fn kind(&self) -> &'static str {
        match self {
            OutboxOp::Send { .. } => "send",
            OutboxOp::Edit { .. } => "edit",
            OutboxOp::Delete { .. } => "delete",
            OutboxOp::TopicUpdate { .. } => "topic_update",
            OutboxOp::TopicFollow { .. } => "topic_follow",
            OutboxOp::CardAction { .. } => "card_action",
            OutboxOp::React { .. } => "react",
        }
    }
    pub fn group_id(&self) -> &str {
        match self {
            OutboxOp::Send { group_id, .. }
            | OutboxOp::Edit { group_id, .. }
            | OutboxOp::Delete { group_id, .. }
            | OutboxOp::TopicUpdate { group_id, .. }
            | OutboxOp::TopicFollow { group_id, .. }
            | OutboxOp::CardAction { group_id, .. }
            | OutboxOp::React { group_id, .. } => group_id,
        }
    }
    pub fn topic_id(&self) -> Option<&str> {
        match self {
            OutboxOp::Send { topic_id, .. }
            | OutboxOp::Edit { topic_id, .. }
            | OutboxOp::Delete { topic_id, .. }
            | OutboxOp::CardAction { topic_id, .. }
            | OutboxOp::React { topic_id, .. } => topic_id.as_deref(),
            OutboxOp::TopicUpdate { topic_id, .. } | OutboxOp::TopicFollow { topic_id, .. } => {
                Some(topic_id)
            }
        }
    }
    pub(crate) fn reply_to_ref(id: &Option<String>) -> Option<ReplyToRef> {
        id.as_ref().map(|m| ReplyToRef {
            message_id: m.clone(),
        })
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct OutboxItem {
    /// `clientMsgId` for a send; a local ULID for other writes.
    pub id: String,
    pub op: OutboxOp,
    pub attempts: u64,
    /// Error code of the last refusal.
    pub last_error: Option<String>,
    pub state: OutboxState,
    pub created_at: u64,
}

pub(crate) const OUTBOX_COLS: &str = "id, payload, attempts, last_error, state, created_at";

pub(crate) fn outbox_from_row(r: &Row<'_>) -> rusqlite::Result<OutboxItem> {
    let payload: String = r.get(1)?;
    let state: String = r.get(4)?;
    Ok(OutboxItem {
        id: r.get(0)?,
        op: serde_json::from_str(&payload).map_err(json_err)?,
        attempts: r.get(2)?,
        last_error: r.get(3)?,
        state: if state == "failed" {
            OutboxState::Failed
        } else {
            OutboxState::Pending
        },
        created_at: r.get(5)?,
    })
}

pub(crate) fn json_err(e: serde_json::Error) -> rusqlite::Error {
    rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, Box::new(e))
}

/// The serde string of a unit enum value, such as `"in_progress"`.
pub(crate) fn enum_str<T: Serialize>(v: &T) -> Result<String, serde_json::Error> {
    match serde_json::to_value(v)? {
        serde_json::Value::String(s) => Ok(s),
        other => Ok(other.to_string()),
    }
}

pub(crate) fn enum_from<T: DeserializeOwned>(s: String) -> rusqlite::Result<T> {
    serde_json::from_value(serde_json::Value::String(s)).map_err(json_err)
}

pub(crate) const MEMBER_COLS: &str =
    "id, kind, name, role, level, avatar_url, deactivated, character";

pub(crate) fn member_from_row(r: &Row<'_>) -> rusqlite::Result<Member> {
    let deactivated: bool = r.get(6)?;
    let role: Option<String> = r.get(3)?;
    let avatar_url: Option<String> = r.get(5)?;
    let character: Option<String> = r.get(7)?;
    Ok(Member {
        id: r.get(0)?,
        kind: enum_from(r.get(1)?)?,
        name: r.get(2)?,
        role: role.map(Some),
        level: enum_from(r.get(4)?)?,
        avatar_url: avatar_url.map(Some),
        deactivated: Some(deactivated),
        // An id this build does not know reads as no character.
        character: character.as_deref().and_then(CharacterId::parse).map(Some),
    })
}

pub(crate) const GROUP_COLS: &str = "id, kind, name, member_ids, created_by, created_at, last_seq, read_seq, \
     unread, mentions, last_activity_at, muted, notify";

pub(crate) fn group_from_row(r: &Row<'_>) -> rusqlite::Result<GroupRow> {
    let ids: String = r.get(3)?;
    let notify: Option<String> = r.get(12)?;
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
        muted: r.get(11)?,
        notify: match notify {
            Some(s) => Some(enum_from(s)?),
            None => None,
        },
    })
}

/// Unread condition for message `m`, for member `?2`.
pub(crate) const UNREAD_COND: &str =
    "m.author_id != ?2 AND m.deleted_at IS NULL AND m.kind != 'system'";

/// Topics with read state. `inner` filters, orders and limits `topics t` (it may use
/// `?1` and `?3`); `?2` is my member id.
pub(crate) fn topic_select(inner: &str) -> String {
    format!(
        "SELECT t.raw, t.title, t.status, t.last_seq, t.message_count, t.last_activity_at,
           t.history_complete, t.owner_id, t.waiting_on, t.last_message_at, t.pos,
           (SELECT COUNT(*) FROM messages m WHERE m.topic_id = t.id AND m.created_seq > t.pos AND {cond}),
           (SELECT COUNT(*) FROM messages m WHERE m.topic_id = t.id AND m.created_seq > t.pos AND {cond}
              AND m.mentions LIKE '%\"' || ?2 || '\"%'),
           t.following
         FROM (SELECT t.*,
                 MAX(COALESCE((SELECT g.read_seq FROM groups g WHERE g.id = t.group_id), 0),
                     COALESCE(rc.seq, 0)) AS pos,
                 COALESCE(rc.following, 0) AS following
               FROM topics t
               LEFT JOIN read_cursors rc ON rc.group_id = t.group_id AND rc.topic_id = t.id
               {inner}) t
         ORDER BY t.last_seq DESC",
        cond = UNREAD_COND,
    )
}

pub(crate) fn topic_from_row(r: &Row<'_>) -> rusqlite::Result<TopicRow> {
    let raw: String = r.get(0)?;
    let mut topic: Topic = serde_json::from_str(&raw).map_err(json_err)?;
    topic.title = r.get(1)?;
    topic.status = enum_from(r.get(2)?)?;
    topic.last_seq = r.get(3)?;
    topic.message_count = r.get(4)?;
    let owner: Option<String> = r.get(7)?;
    topic.owner_id = Some(owner);
    let waiting: String = r.get(8)?;
    topic.waiting_on = Some(serde_json::from_str(&waiting).map_err(json_err)?);
    topic.last_message_at = r.get(9)?;
    Ok(TopicRow {
        topic,
        last_activity_at: r.get(5)?,
        history_complete: r.get(6)?,
        read_seq: r.get(10)?,
        unread: r.get(11)?,
        mentions: r.get(12)?,
        following: r.get(13)?,
    })
}

pub(crate) const MESSAGE_COLS: &str = "id, client_msg_id, group_id, topic_id, author_id, body, mentions, \
     created_seq, created_at, edited_at, deleted_at, streaming, kind, raw";

pub(crate) fn message_values(m: &Message) -> Result<Vec<Value>, serde_json::Error> {
    let mut stored = m.clone();
    if m.is_deleted() {
        clear_deleted(&mut stored);
    }
    Ok(vec![
        Value::Text(m.id.clone()),
        Value::Text(m.client_msg_id.clone()),
        Value::Text(m.group_id.clone()),
        Value::Text(m.topic_id.clone()),
        Value::Text(m.author_id.clone()),
        Value::Text(stored.body.clone()),
        Value::Text(serde_json::to_string(&m.mentions)?),
        Value::Integer(m.created_seq as i64),
        Value::Integer(m.created_at as i64),
        m.edited_at()
            .map_or(Value::Null, |v| Value::Integer(v as i64)),
        m.deleted_at()
            .map_or(Value::Null, |v| Value::Integer(v as i64)),
        Value::Integer(m.is_streaming() as i64),
        Value::Text(enum_str(&m.kind.unwrap_or_default())?),
        Value::Text(serde_json::to_string(&stored)?),
    ])
}

/// A deleted message keeps its row with an empty body and no content.
pub(crate) fn clear_deleted(m: &mut Message) {
    m.body.clear();
    m.attachments = None;
    m.artifact_refs = None;
    m.card = None;
    m.reply_to = None;
    m.streaming = None;
}

pub(crate) fn message_from_row(r: &Row<'_>) -> rusqlite::Result<Message> {
    let raw: String = r.get(13)?;
    let mut m: Message = serde_json::from_str(&raw).map_err(json_err)?;
    m.body = r.get(5)?;
    let edited_at: Option<u64> = r.get(9)?;
    let deleted_at: Option<u64> = r.get(10)?;
    let streaming: bool = r.get(11)?;
    m.created_seq = r.get(7)?;
    m.edited_at = edited_at.map(Some);
    m.deleted_at = deleted_at.map(Some);
    m.streaming = streaming.then_some(true);
    Ok(m)
}
