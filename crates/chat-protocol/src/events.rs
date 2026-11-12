//! The group event log and transient events.
//!
//! To add an event type: add a data struct and a variant to [`EventData`] (or a struct and a
//! variant to [`TransientEvent`]), then add fixtures.

use serde::{Deserialize, Serialize};

use crate::bots::RunUpdatedData;
use crate::card::Card;
use crate::card2::{
    CardAnsweredData, CardEventTransient, CardRetractedData, CardState, CardV2, Cause,
    DecisionRecordedData, InteractionResolvedTransient, lenient_card_v2,
};
use crate::entities::{Message, Millis, Seq, Topic, TopicStatus};
use crate::files::{
    ArtifactDataTransient, ArtifactEventData, ArtifactEventTransient, ArtifactReplyTransient,
};
use crate::links::TopicLinkedData;
use crate::media::ArtifactPreviewReadyData;
use crate::message::{ArtifactRef, BlockRef, FileRef};
use crate::reactions::ReactionChangedData;
use crate::serde_util::{any_value, non_null, nullable};
use crate::topic::TopicContext;

/// One stored event. The shared fields sit next to `type` and `data` on the wire.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Event {
    pub seq: Seq,
    pub group_id: String,
    pub at: Millis,
    pub actor_id: String,
    #[serde(flatten)]
    pub data: EventData,
}

impl Event {
    /// The wire `type` of this event.
    pub fn event_type(&self) -> &'static str {
        self.data.event_type()
    }
}

/// `type` and `data` of an [`Event`].
#[allow(clippy::large_enum_variant)] // events are short-lived; boxing would change the public API
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", content = "data")]
pub enum EventData {
    #[serde(rename = "topic.created")]
    TopicCreated(TopicCreatedData),
    #[serde(rename = "topic.updated")]
    TopicUpdated(TopicUpdatedData),
    #[serde(rename = "message.created")]
    MessageCreated(Box<MessageCreatedData>),
    #[serde(rename = "message.updated")]
    MessageUpdated(MessageUpdatedData),
    #[serde(rename = "message.deleted")]
    MessageDeleted(MessageDeletedData),
    #[serde(rename = "group.updated")]
    GroupUpdated(GroupUpdatedData),
    #[serde(rename = "group.memberAdded")]
    GroupMemberAdded(GroupMemberChangeData),
    #[serde(rename = "group.memberRemoved")]
    GroupMemberRemoved(GroupMemberChangeData),
    #[serde(rename = "card.actioned")]
    CardActioned(CardActionedData),
    #[serde(rename = "artifact.created")]
    ArtifactCreated(ArtifactEventData),
    #[serde(rename = "artifact.versionAdded")]
    ArtifactVersionAdded(ArtifactEventData),
    #[serde(rename = "run.updated")]
    RunUpdated(RunUpdatedData),
    #[serde(rename = "reaction.changed")]
    ReactionChanged(ReactionChangedData),
    // 1.1
    #[serde(rename = "card.answered")]
    CardAnswered(CardAnsweredData),
    #[serde(rename = "topic.linked")]
    TopicLinked(TopicLinkedData),
    /// 2.1: an answer was taken back.
    #[serde(rename = "card.retracted")]
    CardRetracted(CardRetractedData),
    /// 1.1: the preview of a 产出 version is ready.
    #[serde(rename = "artifact.previewReady")]
    ArtifactPreviewReady(ArtifactPreviewReadyData),
    /// 1.1: a decision recorded by hand (记一条).
    #[serde(rename = "decision.recorded")]
    DecisionRecorded(DecisionRecordedData),
    /// 1.1: a bot member set its desk note (首页).
    #[serde(rename = "desk.noted")]
    DeskNoted(crate::desk::DeskNotedData),
    /// A topic was deleted for everyone; clients drop it and its messages.
    #[serde(rename = "topic.deleted")]
    TopicDeleted(crate::topic::TopicDeleteResult),
}

impl EventData {
    pub fn event_type(&self) -> &'static str {
        match self {
            Self::TopicCreated(_) => "topic.created",
            Self::TopicUpdated(_) => "topic.updated",
            Self::MessageCreated(_) => "message.created",
            Self::MessageUpdated(_) => "message.updated",
            Self::MessageDeleted(_) => "message.deleted",
            Self::GroupUpdated(_) => "group.updated",
            Self::GroupMemberAdded(_) => "group.memberAdded",
            Self::GroupMemberRemoved(_) => "group.memberRemoved",
            Self::CardActioned(_) => "card.actioned",
            Self::ArtifactCreated(_) => "artifact.created",
            Self::ArtifactVersionAdded(_) => "artifact.versionAdded",
            Self::RunUpdated(_) => "run.updated",
            Self::ReactionChanged(_) => "reaction.changed",
            Self::CardAnswered(_) => "card.answered",
            Self::TopicLinked(_) => "topic.linked",
            Self::CardRetracted(_) => "card.retracted",
            Self::ArtifactPreviewReady(_) => "artifact.previewReady",
            Self::DecisionRecorded(_) => "decision.recorded",
            Self::DeskNoted(_) => "desk.noted",
            Self::TopicDeleted(_) => "topic.deleted",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TopicCreatedData {
    pub topic: Topic,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TopicUpdatedData {
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
    /// Present = the new `waitingOn` (replaces the old list).
    #[serde(
        default,
        deserialize_with = "non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub waiting_on: Option<Vec<String>>,
    /// Present = the new owner; `Some(None)` = cleared.
    #[serde(
        default,
        deserialize_with = "nullable",
        skip_serializing_if = "Option::is_none"
    )]
    pub owner_id: Option<Option<String>>,
    /// Present when the server recomputed it (for example after a delete).
    #[serde(
        default,
        deserialize_with = "non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub last_message_at: Option<Millis>,
    /// Present = the side chat's new context (a later `send.context`).
    #[serde(
        default,
        deserialize_with = "non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub context_of: Option<TopicContext>,
    /// Usable: present = the topic's participants now.
    #[serde(
        default,
        deserialize_with = "non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub participants: Option<Vec<crate::relay::TopicParticipant>>,
    /// Usable: present = the topic's relay now; `Some(None)` = it has none any more.
    #[serde(
        default,
        deserialize_with = "nullable",
        skip_serializing_if = "Option::is_none"
    )]
    pub relay: Option<Option<crate::relay::TopicRelay>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MessageCreatedData {
    pub message: Message,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MessageUpdatedData {
    pub message_id: String,
    pub body: String,
    pub edited_at: Millis,
    pub streaming: bool,
    /// Present = replaced with this value; absent = unchanged.
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
    /// Read leniently, as `Message.cardV2`.
    #[serde(
        default,
        deserialize_with = "lenient_card_v2",
        skip_serializing_if = "Option::is_none"
    )]
    pub card_v2: Option<CardV2>,
    /// 2.1: the card's server-owned state after this rev.
    #[serde(
        default,
        deserialize_with = "non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub card_state: Option<CardState>,
    /// 2.1: the event this rev answered.
    #[serde(
        default,
        deserialize_with = "non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub cause: Option<Cause>,
    /// 1.1: present = the message's refs changed (a source was deleted).
    #[serde(
        default,
        deserialize_with = "non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub block_refs: Option<Vec<BlockRef>>,
}

/// Data of `card.actioned`. The event's `actorId` is the member who pressed.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CardActionedData {
    pub message_id: String,
    pub action_id: String,
    /// The action's `value`.
    #[serde(
        default,
        deserialize_with = "any_value",
        skip_serializing_if = "Option::is_none"
    )]
    pub value: Option<serde_json::Value>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MessageDeletedData {
    pub message_id: String,
    pub deleted_at: Millis,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GroupUpdatedData {
    /// The group's name after the change.
    pub name: String,
    /// 1.1: set when the purpose line changed (`""` = cleared).
    #[serde(
        default,
        deserialize_with = "crate::serde_util::non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub purpose: Option<String>,
}

/// Data of `group.memberAdded` and `group.memberRemoved`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GroupMemberChangeData {
    pub member_id: String,
}

/// Delivered once, never stored, no `seq`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum TransientEvent {
    #[serde(rename = "typing")]
    Typing(TypingEvent),
    #[serde(rename = "presence")]
    Presence(PresenceEvent),
    #[serde(rename = "message.delta")]
    MessageDelta(MessageDeltaEvent),
    // 1.1: sent only to connections whose hello has feature "interact/1".
    #[serde(rename = "card.event")]
    CardEvent(CardEventTransient),
    #[serde(rename = "interaction.resolved")]
    InteractionResolved(InteractionResolvedTransient),
    #[serde(rename = "artifact.event")]
    ArtifactEvent(ArtifactEventTransient),
    #[serde(rename = "artifact.data")]
    ArtifactData(ArtifactDataTransient),
    #[serde(rename = "artifact.reply")]
    ArtifactReply(ArtifactReplyTransient),
    // Settings: sent only to connections whose hello has feature "settings/1".
    #[serde(rename = "digest")]
    Digest(crate::settings::DigestTransient),
    // Usable: sent only to the person's own connections whose hello has feature "github/1".
    #[serde(rename = "github.status")]
    GithubStatus(crate::github::GithubStatusTransient),
    // Usable: sent only to admins' connections whose hello has feature "settings/1".
    #[serde(rename = "ai.failing")]
    AiFailing(crate::settings::AiFailingTransient),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TypingEvent {
    pub group_id: String,
    pub topic_id: String,
    pub member_id: String,
    pub at: Millis,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PresenceEvent {
    pub member_id: String,
    pub online: bool,
    pub at: Millis,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MessageDeltaEvent {
    pub group_id: String,
    pub topic_id: String,
    pub message_id: String,
    pub append: String,
}
