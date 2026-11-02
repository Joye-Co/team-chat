//! The group event log and transient events.
//!
//! To add an event type: add a data struct and a variant to [`EventData`] (or a struct and a
//! variant to [`TransientEvent`]), then add fixtures.

use serde::{Deserialize, Serialize};

use crate::entities::{Message, Millis, Seq, Topic, TopicStatus};
use crate::serde_util::non_null;

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
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", content = "data")]
pub enum EventData {
    #[serde(rename = "topic.created")]
    TopicCreated(TopicCreatedData),
    #[serde(rename = "topic.updated")]
    TopicUpdated(TopicUpdatedData),
    #[serde(rename = "message.created")]
    MessageCreated(MessageCreatedData),
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
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TopicCreatedData {
    pub topic: Topic,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
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
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MessageCreatedData {
    pub message: Message,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MessageUpdatedData {
    pub message_id: String,
    pub body: String,
    pub edited_at: Millis,
    pub streaming: bool,
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
    pub name: String,
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
