//! Client-to-server frames. To add a frame: add a struct, a variant to [`ClientFrame`], an arm
//! in [`ClientFrame::validate`] and fixtures.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::entities::{Seq, TopicStatus};
use crate::limits::*;
use crate::serde_util::non_null;

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
                Ok(())
            }
            Self::Edit(f) => {
                check_id("rid", &f.rid)?;
                check_id("groupId", &f.group_id)?;
                check_id("messageId", &f.message_id)?;
                check_body("body", &f.body)
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
                Ok(())
            }
            Self::Typing(f) => {
                check_id("groupId", &f.group_id)?;
                check_id("topicId", &f.topic_id)
            }
            Self::Read(f) => check_id("groupId", &f.group_id),
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
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
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
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
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TypingFrame {
    pub group_id: String,
    pub topic_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReadFrame {
    pub group_id: String,
    pub seq: Seq,
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
