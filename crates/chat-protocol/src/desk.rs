//! 1.1 desk notes (首页): a bot member's one-line priority, set by the bot and kept in the
//! group log (docs/protocol.md, "Desk notes").

use serde::{Deserialize, Serialize};

use crate::entities::{Millis, Seq};
use crate::limits::{Validation, check_chars, check_id};

/// Max code points of a desk note (after the server trims it).
pub const MAX_DESK_NOTE_CHARS: usize = 140;

/// `PUT /v1/desk-note` (a bot member of the group): sets its note; `""` clears it.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PutDeskNoteRequest {
    pub group_id: String,
    pub text: String,
}

impl PutDeskNoteRequest {
    pub fn validate(&self) -> Validation {
        check_id("groupId", &self.group_id)?;
        check_chars("text", &self.text, 0, 4 * MAX_DESK_NOTE_CHARS)
    }
}

/// Response of `PUT /v1/desk-note`: the stored event's seq (`None` = same text, nothing stored).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PutDeskNoteResponse {
    pub seq: Option<Seq>,
}

/// Data of the stored `desk.noted` event (`actorId` = the bot); `""` = cleared.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeskNotedData {
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeskNote {
    pub member_id: String,
    pub text: String,
    pub at: Millis,
    pub seq: Seq,
}

/// `GET /v1/groups/:id/desk-notes`: the newest note per bot member, cleared ones left out.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeskNotesResponse {
    pub notes: Vec<DeskNote>,
}
