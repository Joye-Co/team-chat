//! Topic extensions: `waitingOn`, the responsible member, last activity and following
//! (docs/protocol.md, "Topics: waitingOn, owner, follow"). The fields live on
//! [`crate::Topic`], [`crate::TopicUpdateFrame`] and [`crate::TopicUpdatedData`].

use serde::{Deserialize, Serialize};

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
