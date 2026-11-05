//! Client-to-server frames. To add a frame: add a struct, a variant to [`ClientFrame`], an arm
//! in [`ClientFrame::validate`] and fixtures.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::card::{Card, check_action_id};
use crate::card2::{
    CardAnswerFrame, CardAnswerRetractFrame, CardEventFrame, CardPatchFrame, CardV2,
    InteractionResolveFrame,
};
use crate::entities::{Seq, TopicStatus};
use crate::files::{ArtifactDataFrame, ArtifactEventFrame, ArtifactReplyFrame};
use crate::limits::*;
use crate::message::{
    ArtifactRef, BlockRefInput, FileRef, ReplyToRef, check_block_refs, check_refs,
};
use crate::reactions::ReactFrame;
use crate::search::SearchFrame;
use crate::serde_util::{any_value, non_null, nullable};
use crate::topic::TopicContext;
use crate::topic::{TopicFollowFrame, TopicMuteFrame, check_waiting_on};

#[allow(clippy::large_enum_variant)] // frames are short-lived; boxing would change the public API
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
    #[serde(rename = "topic.mute")]
    TopicMute(TopicMuteFrame),
    #[serde(rename = "card.action")]
    CardAction(CardActionFrame),
    #[serde(rename = "search")]
    Search(SearchFrame),
    #[serde(rename = "react")]
    React(ReactFrame),
    // 1.1
    #[serde(rename = "card.answer")]
    CardAnswer(CardAnswerFrame),
    #[serde(rename = "artifact.event")]
    ArtifactEvent(ArtifactEventFrame),
    // 1.1 interaction loop
    #[serde(rename = "card.event")]
    CardEvent(CardEventFrame),
    #[serde(rename = "card.answer.retract")]
    CardAnswerRetract(CardAnswerRetractFrame),
    #[serde(rename = "card.patch")]
    CardPatch(CardPatchFrame),
    #[serde(rename = "interaction.resolve")]
    InteractionResolve(InteractionResolveFrame),
    #[serde(rename = "artifact.data")]
    ArtifactData(ArtifactDataFrame),
    #[serde(rename = "artifact.reply")]
    ArtifactReply(ArtifactReplyFrame),
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
            Self::TopicMute(_) => "topic.mute",
            Self::CardAction(_) => "card.action",
            Self::Search(_) => "search",
            Self::React(_) => "react",
            Self::CardAnswer(_) => "card.answer",
            Self::ArtifactEvent(_) => "artifact.event",
            Self::CardEvent(_) => "card.event",
            Self::CardAnswerRetract(_) => "card.answer.retract",
            Self::CardPatch(_) => "card.patch",
            Self::InteractionResolve(_) => "interaction.resolve",
            Self::ArtifactData(_) => "artifact.data",
            Self::ArtifactReply(_) => "artifact.reply",
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
            Self::TopicMute(f) => Some(&f.rid),
            Self::CardAction(f) => Some(&f.rid),
            Self::Search(f) => Some(&f.rid),
            Self::React(f) => Some(&f.rid),
            Self::CardAnswer(f) => Some(&f.rid),
            Self::ArtifactEvent(f) => Some(&f.rid),
            Self::CardEvent(f) => Some(&f.rid),
            Self::CardAnswerRetract(f) => Some(&f.rid),
            Self::CardPatch(f) => Some(&f.rid),
            Self::InteractionResolve(f) => Some(&f.rid),
            Self::ArtifactData(f) => Some(&f.rid),
            Self::ArtifactReply(f) => Some(&f.rid),
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
                for (field, list, max) in [
                    ("features", &f.features, 32),
                    ("supportedCatalogs", &f.supported_catalogs, 16),
                ] {
                    if let Some(l) = list {
                        if l.len() > max {
                            return Err(ValidationError {
                                field,
                                message: format!("at most {max}"),
                            });
                        }
                        l.iter().try_for_each(|x| check_chars(field, x, 1, 32))?;
                    }
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
                f.card.as_ref().map_or(Ok(()), Card::validate)?;
                f.card_v2.as_ref().map_or(Ok(()), CardV2::validate)?;
                if let Some(c) = &f.cause_event_id {
                    check_id("causeEventId", c)?;
                }
                check_block_refs(f.block_refs.as_deref())?;
                f.from_aside
                    .as_ref()
                    .map_or(Ok(()), crate::topic::AsideOrigin::validate)?;
                f.context.as_ref().map_or(Ok(()), TopicContext::validate)
            }
            Self::Edit(f) => {
                check_id("rid", &f.rid)?;
                check_id("groupId", &f.group_id)?;
                check_id("messageId", &f.message_id)?;
                check_body("body", &f.body)?;
                check_refs(f.attachments.as_deref(), f.artifact_refs.as_deref())?;
                f.card.as_ref().map_or(Ok(()), Card::validate)?;
                f.card_v2.as_ref().map_or(Ok(()), CardV2::validate)?;
                if f.base_rev == Some(0) {
                    return Err(ValidationError {
                        field: "baseRev",
                        message: "must be positive".into(),
                    });
                }
                f.cause_event_id
                    .as_deref()
                    .map_or(Ok(()), |c| check_id("causeEventId", c))
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
            Self::TopicMute(f) => f.validate(),
            Self::CardAction(f) => {
                check_id("rid", &f.rid)?;
                check_id("groupId", &f.group_id)?;
                check_id("messageId", &f.message_id)?;
                check_action_id("actionId", &f.action_id)
            }
            Self::Search(f) => f.validate(),
            Self::React(f) => f.validate(),
            Self::CardAnswer(f) => f.validate(),
            Self::ArtifactEvent(f) => f.validate(),
            Self::CardEvent(f) => f.validate(),
            Self::CardAnswerRetract(f) => f.validate(),
            Self::CardPatch(f) => f.validate(),
            Self::InteractionResolve(f) => f.validate(),
            Self::ArtifactData(f) => f.validate(),
            Self::ArtifactReply(f) => f.validate(),
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
    /// 1.1: what this client handles ([`crate::CLIENT_FEATURES`]).
    #[serde(
        default,
        deserialize_with = "non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub features: Option<Vec<String>>,
    /// 1.1: Card v2 catalogs this client renders.
    #[serde(
        default,
        deserialize_with = "non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub supported_catalogs: Option<Vec<String>>,
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
    /// 1.1: makes a card message; without `card` the server derives the v1 card.
    #[serde(
        default,
        deserialize_with = "non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub card_v2: Option<CardV2>,
    /// 1.1: start (or move on) a side chat about this topic. Only in a DM with a bot.
    #[serde(
        default,
        deserialize_with = "non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub context: Option<TopicContext>,
    /// 2.1: the event (`eventId`) this message answers; the server fills `Message.cause`.
    #[serde(
        default,
        deserialize_with = "non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub cause_event_id: Option<String>,
    /// 1.1: refs to parts; the server writes their quote lines in front of the body.
    #[serde(
        default,
        deserialize_with = "non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub block_refs: Option<Vec<BlockRefInput>>,
    /// 1.1: this send posts a side chat answer to the side chat's source topic.
    #[serde(
        default,
        deserialize_with = "non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub from_aside: Option<crate::topic::AsideOrigin>,
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
    /// 1.1: a card message only; `rev` must be higher than the stored one.
    #[serde(
        default,
        deserialize_with = "non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub card_v2: Option<CardV2>,
    /// 2.1: with `cardV2`, the rev the edit is based on (`conflict`, `card.stale_rev`).
    #[serde(
        default,
        deserialize_with = "non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub base_rev: Option<u32>,
    /// 2.1: the event this edit answers.
    #[serde(
        default,
        deserialize_with = "non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub cause_event_id: Option<String>,
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
