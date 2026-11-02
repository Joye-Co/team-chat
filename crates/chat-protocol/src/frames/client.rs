//! Client-to-server frames. To add a frame: add a struct, a variant to [`ClientFrame`], an arm
//! in [`ClientFrame::validate`] and fixtures.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::card::{Card, check_action_id};
use crate::entities::{Seq, TopicStatus};
use crate::limits::*;
use crate::message::{ArtifactRef, FileRef, ReplyToRef, check_refs};
use crate::reactions::ReactFrame;
use crate::search::SearchFrame;
use crate::serde_util::{any_value, non_null, nullable};
use crate::topic::{TopicFollowFrame, check_waiting_on};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "t")]
pub enum ClientFrame {
    #[serde(rename = "hello")]
    Hello(HelloFrame),
    #[serde(rename = "send")]
    Send(SendFrame),
    #[serde(rename = "edit")]
    Edit(EditFrame),
    #[serde(rename = "delta")]
    Delta(DeltaFrame),
    #[serde(rename = "delete")]
    Delete(DeleteFrame),
    #[serde(rename = "topic.update")]
    TopicUpdate(TopicUpdateFrame),
    #[serde(rename = "typing")]
    Typing(TypingFrame),
    #[serde(rename = "read")]
    Read(ReadFrame),
    #[serde(rename = "fetch")]
    Fetch(FetchFrame),
    #[serde(rename = "history")]
    History(HistoryRequestFrame),
    #[serde(rename = "ping")]
    Ping,
    #[serde(rename = "topic.follow")]
    TopicFollow(TopicFollowFrame),
    #[serde(rename = "card.action")]
    CardAction(CardActionFrame),
    #[serde(rename = "search")]
    Search(SearchFrame),
    #[serde(rename = "react")]
    React(ReactFrame),
}

impl ClientFrame {
    /// The wire `t` of this frame.
    pub fn frame_type(&self) -> &'static str {
        match self {
            Self::Hello(_) => "hello",
            Self::Send(_) => "send",
            Self::Edit(_) => "edit",
            Self::Delta(_) => "delta",
            Self::Delete(_) => "delete",
            Self::TopicUpdate(_) => "topic.update",
            Self::Typing(_) => "typing",
            Self::Read(_) => "read",
            Self::Fetch(_) => "fetch",
            Self::History(_) => "history",
            Self::Ping => "ping",
            Self::TopicFollow(_) => "topic.follow",
            Self::CardAction(_) => "card.action",
            Self::Search(_) => "search",
            Self::React(_) => "react",
        }
    }

    /// The request id, for frames that expect an `ack` or `page`.
    pub fn rid(&self) -> Option<&str> {
        match self {
            Self::Send(f) => Some(&f.rid),
            Self::Edit(f) => Some(&f.rid),
            Self::Delete(f) => Some(&f.rid),
            Self::TopicUpdate(f) => Some(&f.rid),
            Self::Fetch(f) => Some(&f.rid),
            Self::History(f) => Some(&f.rid),
            Self::TopicFollow(f) => Some(&f.rid),
            Self::CardAction(f) => Some(&f.rid),
            Self::Search(f) => Some(&f.rid),
            Self::React(f) => Some(&f.rid),
            Self::Hello(_) | Self::Delta(_) | Self::Typing(_) | Self::Read(_) | Self::Ping => None,
        }
    }

    /// Checks the limits that the server enforces: ids not empty, body, title and
    /// `clientMsgId` lengths, page `limit` range and protocol version range.
    pub fn validate(&self) -> Validation {
        match self {
            Self::Hello(f) => {
                if f.protocol == 0 {
                    return Err(ValidationError {
                        field: "protocol",
                        message: "must be positive".into(),
                    });
                }
                Ok(())
            }
            Self::Send(f) => {
                check_id("rid", &f.rid)?;
                check_id("groupId", &f.group_id)?;
                if let Some(id) = &f.topic_id {
                    check_id("topicId", id)?;
                }
                if let Some(title) = &f.title {
                    check_title(title)?;
                }
                check_chars("clientMsgId", &f.client_msg_id, 1, MAX_CLIENT_MSG_ID_CHARS)?;
                check_body("body", &f.body)?;
                if let Some(m) = &f.mentions {
                    check_ids("mentions", m)?;
                }
                if let Some(r) = &f.reply_to {
                    check_id("replyTo.messageId", &r.message_id)?;
                }
                check_refs(f.attachments.as_deref(), f.artifact_refs.as_deref())?;
                f.card.as_ref().map_or(Ok(()), Card::validate)
            }
            Self::Edit(f) => {
                check_id("rid", &f.rid)?;
                check_id("groupId", &f.group_id)?;
                check_id("messageId", &f.message_id)?;
                check_body("body", &f.body)?;
                check_refs(f.attachments.as_deref(), f.artifact_refs.as_deref())?;
                f.card.as_ref().map_or(Ok(()), Card::validate)
            }
            Self::Delta(f) => {
                check_id("groupId", &f.group_id)?;
                check_id("messageId", &f.message_id)?;
                check_body("append", &f.append)
            }
            Self::Delete(f) => {
                check_id("rid", &f.rid)?;
                check_id("groupId", &f.group_id)?;
                check_id("messageId", &f.message_id)
            }
            Self::TopicUpdate(f) => {
                check_id("rid", &f.rid)?;
                check_id("groupId", &f.group_id)?;
                check_id("topicId", &f.topic_id)?;
                if let Some(title) = &f.title {
                    check_title(title)?;
                }
                if let Some(w) = &f.waiting_on {
                    check_waiting_on(w)?;
                }
                if let Some(Some(o)) = &f.owner_id {
                    check_id("ownerId", o)?;
                }
                Ok(())
            }
            Self::Typing(f) => {
                check_id("groupId", &f.group_id)?;
                check_id("topicId", &f.topic_id)
            }
            Self::Read(f) => {
                check_id("groupId", &f.group_id)?;
                f.topic_id
                    .as_deref()
                    .map_or(Ok(()), |t| check_id("topicId", t))
            }
            Self::Fetch(f) => {
                check_id("rid", &f.rid)?;
                check_id("groupId", &f.group_id)?;
                f.limit.map_or(Ok(()), check_limit)
            }
            Self::History(f) => {
                check_id("rid", &f.rid)?;
                check_id("groupId", &f.group_id)?;
                check_id("topicId", &f.topic_id)?;
                f.limit.map_or(Ok(()), check_limit)
            }
            Self::Ping => Ok(()),
            Self::TopicFollow(f) => f.validate(),
            Self::CardAction(f) => {
                check_id("rid", &f.rid)?;
                check_id("groupId", &f.group_id)?;
                check_id("messageId", &f.message_id)?;
                check_action_id("actionId", &f.action_id)
            }
            Self::Search(f) => f.validate(),
            Self::React(f) => f.validate(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClientInfo {
    pub name: String,
    pub version: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HelloFrame {
    /// Any positive value parses; the server answers `unsupported_protocol` for != 1.
    pub protocol: u32,
    pub client: ClientInfo,
    /// Last applied seq per group id.
    #[serde(
        default,
        deserialize_with = "non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub cursors: Option<BTreeMap<String, Seq>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SendFrame {
    pub rid: String,
    pub group_id: String,
    /// `None` starts a new topic whose root is this message.
    #[serde(
        default,
        deserialize_with = "non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub topic_id: Option<String>,
    #[serde(
        default,
        deserialize_with = "non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub title: Option<String>,
    pub client_msg_id: String,
    pub body: String,
    #[serde(
        default,
        deserialize_with = "non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub mentions: Option<Vec<String>>,
    #[serde(
        default,
        deserialize_with = "non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub streaming: Option<bool>,
    /// Quote this message (same group). The server fills author and excerpt.
    #[serde(
        default,
        deserialize_with = "non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub reply_to: Option<ReplyToRef>,
    #[serde(
        default,
        deserialize_with = "non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub attachments: Option<Vec<FileRef>>,
    #[serde(
        default,
        deserialize_with = "non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub artifact_refs: Option<Vec<ArtifactRef>>,
    /// Makes a `card` message; `body` is then the plain-text fallback.
    #[serde(
        default,
        deserialize_with = "non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub card: Option<Card>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EditFrame {
    pub rid: String,
    pub group_id: String,
    pub message_id: String,
    pub body: String,
    #[serde(
        default,
        deserialize_with = "non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub streaming: Option<bool>,
    /// Present = replace; absent = keep.
    #[serde(
        default,
        deserialize_with = "non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub attachments: Option<Vec<FileRef>>,
    #[serde(
        default,
        deserialize_with = "non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub artifact_refs: Option<Vec<ArtifactRef>>,
    #[serde(
        default,
        deserialize_with = "non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub card: Option<Card>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeltaFrame {
    pub group_id: String,
    pub message_id: String,
    pub append: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeleteFrame {
    pub rid: String,
    pub group_id: String,
    pub message_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TopicUpdateFrame {
    pub rid: String,
    pub group_id: String,
    pub topic_id: String,
    #[serde(
        default,
        deserialize_with = "non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub title: Option<String>,
    #[serde(
        default,
        deserialize_with = "non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub status: Option<TopicStatus>,
    /// Present = replace the whole list.
    #[serde(
        default,
        deserialize_with = "non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub waiting_on: Option<Vec<String>>,
    /// `Some(None)` = `null` clears the responsible member.
    #[serde(
        default,
        deserialize_with = "nullable",
        skip_serializing_if = "Option::is_none"
    )]
    pub owner_id: Option<Option<String>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TypingFrame {
    pub group_id: String,
    pub topic_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReadFrame {
    pub group_id: String,
    pub seq: Seq,
    /// With it: only this topic's read position. Without: the whole group up to `seq`.
    #[serde(
        default,
        deserialize_with = "non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub topic_id: Option<String>,
}

/// `{ t: "card.action" }`: press a card button. `ack.result` is omitted.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CardActionFrame {
    pub rid: String,
    pub group_id: String,
    pub message_id: String,
    pub action_id: String,
    /// Ignored in V1: the event carries the action's own `value`.
    #[serde(
        default,
        deserialize_with = "any_value",
        skip_serializing_if = "Option::is_none"
    )]
    pub value: Option<serde_json::Value>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FetchFrame {
    pub rid: String,
    pub group_id: String,
    pub after_seq: Seq,
    #[serde(
        default,
        deserialize_with = "non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub limit: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HistoryRequestFrame {
    pub rid: String,
    pub group_id: String,
    pub topic_id: String,
    #[serde(
        default,
        deserialize_with = "non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub before_seq: Option<Seq>,
    #[serde(
        default,
        deserialize_with = "non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub limit: Option<u32>,
}
