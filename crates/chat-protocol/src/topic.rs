//! Topic extensions: `waitingOn`, the responsible member, last activity and following
//! (docs/protocol.md, "Topics: waitingOn, owner, follow"). The fields live on
//! [`crate::Topic`], [`crate::TopicUpdateFrame`] and [`crate::TopicUpdatedData`].

use serde::{Deserialize, Serialize};

use crate::entities::Seq;
use crate::limits::{Validation, ValidationError, check_id, check_ids};

/// Max entries in `waitingOn`.
pub const MAX_WAITING_ON: usize = 20;

/// `{ t: "topic.follow" }`: follow or unfollow a topic. `ack.result` is a [`crate::TopicState`].
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TopicFollowFrame {
    pub rid: String,
    pub group_id: String,
    pub topic_id: String,
    pub follow: bool,
}

impl TopicFollowFrame {
    pub fn validate(&self) -> Validation {
        check_id("rid", &self.rid)?;
        check_id("groupId", &self.group_id)?;
        check_id("topicId", &self.topic_id)
    }
}

/// Checks a `waitingOn` list: at most [`MAX_WAITING_ON`] non-empty ids.
pub fn check_waiting_on(ids: &[String]) -> Validation {
    if ids.len() > MAX_WAITING_ON {
        return Err(ValidationError {
            field: "waitingOn",
            message: format!("at most {MAX_WAITING_ON} members"),
        });
    }
    check_ids("waitingOn", ids)
}

/// A side chat (单独问) points at the topic it is about: the asker's DM topic with a bot reads
/// `topic_id` of `group_id` up to `upto_seq` (docs/protocol.md, "Side chats").
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TopicContext {
    pub group_id: String,
    pub topic_id: String,
    pub upto_seq: Seq,
}

/// 1.1: the 单独问 answer a message was posted from ("post to topic"): the side chat topic in
/// the asker's DM with the bot, and the answer message there.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AsideOrigin {
    pub group_id: String,
    pub topic_id: String,
    pub message_id: String,
}

impl AsideOrigin {
    pub fn validate(&self) -> Validation {
        check_id("fromAside.groupId", &self.group_id)?;
        check_id("fromAside.topicId", &self.topic_id)?;
        check_id("fromAside.messageId", &self.message_id)
    }
}

impl TopicContext {
    pub fn validate(&self) -> Validation {
        check_id("context.groupId", &self.group_id)?;
        check_id("context.topicId", &self.topic_id)
    }
}
