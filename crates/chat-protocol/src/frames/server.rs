//! Server-to-client frames. To add a frame: add a struct, a variant to [`ServerFrame`] and
//! fixtures.

use serde::{Deserialize, Serialize};

use crate::entities::{GroupSummary, Member, Message, Millis, Seq, Topic};
use crate::errors::ErrorBody;
use crate::events::{Event, TransientEvent};
use crate::serde_util::{ProtocolV1, any_value, non_null};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "t")]
pub enum ServerFrame {
    #[serde(rename = "welcome")]
    Welcome(WelcomeFrame),
    #[serde(rename = "event")]
    Event(EventFrame),
    #[serde(rename = "transient")]
    Transient(TransientFrame),
    #[serde(rename = "ack")]
    Ack(AckFrame),
    #[serde(rename = "page")]
    Page(PageFrame),
    #[serde(rename = "history")]
    History(HistoryFrame),
    #[serde(rename = "directory")]
    Directory(DirectoryFrame),
    #[serde(rename = "error")]
    Error(ErrorFrame),
    #[serde(rename = "pong")]
    Pong,
}

impl ServerFrame {
    /// The wire `t` of this frame.
    pub fn frame_type(&self) -> &'static str {
        match self {
            Self::Welcome(_) => "welcome",
            Self::Event(_) => "event",
            Self::Transient(_) => "transient",
            Self::Ack(_) => "ack",
            Self::Page(_) => "page",
            Self::History(_) => "history",
            Self::Directory(_) => "directory",
            Self::Error(_) => "error",
            Self::Pong => "pong",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WelcomeFrame {
    /// Always 1; any other value fails to parse.
    pub protocol: ProtocolV1,
    pub me: Member,
    pub members: Vec<Member>,
    pub groups: Vec<GroupSummary>,
    pub server_time: Millis,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EventFrame {
    pub event: Event,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TransientFrame {
    pub event: TransientEvent,
}

/// `{ t: "ack", rid, ok: true, result? }` or `{ t: "ack", rid, ok: false, error }`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "RawAck", into = "RawAck")]
pub struct AckFrame {
    pub rid: String,
    pub outcome: AckOutcome,
}

#[derive(Debug, Clone, PartialEq)]
pub enum AckOutcome {
    /// `result` is frame-specific: [`SendResult`] for `send`; decode with [`AckFrame::send_result`].
    Ok {
        result: Option<serde_json::Value>,
    },
    Err {
        error: ErrorBody,
    },
}

impl AckFrame {
    pub fn is_ok(&self) -> bool {
        matches!(self.outcome, AckOutcome::Ok { .. })
    }

    /// Decodes `result` as the answer to a `send` frame.
    pub fn send_result(&self) -> Option<Result<SendResult, serde_json::Error>> {
        match &self.outcome {
            AckOutcome::Ok { result: Some(v) } => Some(serde_json::from_value(v.clone())),
            _ => None,
        }
    }
}

#[derive(Serialize, Deserialize)]
struct RawAck {
    rid: String,
    ok: bool,
    #[serde(
        default,
        deserialize_with = "any_value",
        skip_serializing_if = "Option::is_none"
    )]
    result: Option<serde_json::Value>,
    /// Decoded only when `ok` is false, like the Zod union.
    #[serde(
        default,
        deserialize_with = "any_value",
        skip_serializing_if = "Option::is_none"
    )]
    error: Option<serde_json::Value>,
}

impl TryFrom<RawAck> for AckFrame {
    type Error = String;
    fn try_from(raw: RawAck) -> Result<Self, String> {
        let outcome = if raw.ok {
            AckOutcome::Ok { result: raw.result }
        } else {
            let error = raw.error.ok_or("ack with ok false needs error")?;
            AckOutcome::Err {
                error: serde_json::from_value(error).map_err(|e| e.to_string())?,
            }
        };
        Ok(AckFrame {
            rid: raw.rid,
            outcome,
        })
    }
}

impl From<AckFrame> for RawAck {
    fn from(ack: AckFrame) -> Self {
        match ack.outcome {
            AckOutcome::Ok { result } => RawAck {
                rid: ack.rid,
                ok: true,
                result,
                error: None,
            },
            AckOutcome::Err { error } => RawAck {
                rid: ack.rid,
                ok: false,
                result: None,
                error: Some(serde_json::to_value(error).expect("ErrorBody serializes")),
            },
        }
    }
}

/// `ack.result` of a `send` frame.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SendResult {
    pub message: Message,
    #[serde(
        default,
        deserialize_with = "non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub topic: Option<Topic>,
    #[serde(
        default,
        deserialize_with = "non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub duplicate: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PageFrame {
    pub rid: String,
    pub group_id: String,
    pub events: Vec<Event>,
    pub has_more: bool,
    pub last_seq: Seq,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HistoryFrame {
    pub rid: String,
    pub group_id: String,
    pub topic_id: String,
    pub messages: Vec<Message>,
    pub has_more: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DirectoryFrame {
    #[serde(
        default,
        deserialize_with = "non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub members: Option<Vec<Member>>,
    #[serde(
        default,
        deserialize_with = "non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub groups: Option<Vec<GroupSummary>>,
    #[serde(
        default,
        deserialize_with = "non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub removed_group_ids: Option<Vec<String>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ErrorFrame {
    pub error: ErrorBody,
}
