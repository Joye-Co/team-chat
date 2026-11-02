//! Emoji reactions on messages (docs/protocol.md, "Reactions").

use serde::{Deserialize, Serialize};

use crate::limits::{Validation, ValidationError, check_id};

/// Most distinct emoji on one message.
pub const MAX_REACTIONS: usize = 50;
/// One emoji (with its modifiers and joiners), in Unicode code points.
pub const MAX_EMOJI_CHARS: usize = 16;

/// Who reacted with one emoji, oldest first.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Reaction {
    pub emoji: String,
    pub member_ids: Vec<String>,
}

/// `{ t: "react" }`: add (`on: true`) or remove my reaction. `ack.result` is a
/// [`ReactResult`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReactFrame {
    pub rid: String,
    pub group_id: String,
    pub message_id: String,
    pub emoji: String,
    pub on: bool,
}

/// `ack.result` of `react`: false when my reaction already was as asked (no event).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReactResult {
    pub changed: bool,
}

/// Data of `reaction.changed`. The event's `actorId` is the member who reacted.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReactionChangedData {
    pub message_id: String,
    pub emoji: String,
    pub on: bool,
}

/// One emoji: 1..=16 code points, no whitespace.
pub fn check_emoji(field: &'static str, emoji: &str) -> Validation {
    let n = emoji.chars().count();
    let message = if n == 0 {
        "must not be empty"
    } else if n > MAX_EMOJI_CHARS {
        "at most 16 code points"
    } else if emoji.chars().any(char::is_whitespace) {
        "no whitespace"
    } else {
        return Ok(());
    };
    Err(ValidationError {
        field,
        message: message.into(),
    })
}

impl ReactFrame {
    pub fn validate(&self) -> Validation {
        check_id("rid", &self.rid)?;
        check_id("groupId", &self.group_id)?;
        check_id("messageId", &self.message_id)?;
        check_emoji("emoji", &self.emoji)
    }
}

/// Applies one `reaction.changed` to a message's reaction list: adds the member
/// at the end of the emoji's list (a new emoji goes last) or removes it (an
/// emoji with nobody left goes away). Returns whether the list changed.
pub fn apply_reaction(
    reactions: &mut Vec<Reaction>,
    emoji: &str,
    member_id: &str,
    on: bool,
) -> bool {
    let ix = reactions.iter().position(|r| r.emoji == emoji);
    match (ix, on) {
        (Some(i), true) => {
            let r = &mut reactions[i];
            if r.member_ids.iter().any(|m| m == member_id) {
                return false;
            }
            r.member_ids.push(member_id.to_string());
            true
        }
        (None, true) => {
            reactions.push(Reaction {
                emoji: emoji.to_string(),
                member_ids: vec![member_id.to_string()],
            });
            true
        }
        (Some(i), false) => {
            let r = &mut reactions[i];
            let before = r.member_ids.len();
            r.member_ids.retain(|m| m != member_id);
            let changed = r.member_ids.len() != before;
            if r.member_ids.is_empty() {
                reactions.remove(i);
            }
            changed
        }
        (None, false) => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn toggles_one_per_member() {
        let mut r = Vec::new();
        assert!(apply_reaction(&mut r, "👍", "a", true));
        assert!(!apply_reaction(&mut r, "👍", "a", true));
        assert!(apply_reaction(&mut r, "👍", "b", true));
        assert!(apply_reaction(&mut r, "✅", "a", true));
        assert_eq!(r[0].member_ids, ["a", "b"]);
        assert!(apply_reaction(&mut r, "👍", "a", false));
        assert!(apply_reaction(&mut r, "👍", "b", false));
        assert_eq!(r.len(), 1);
        assert!(!apply_reaction(&mut r, "👍", "b", false));
    }

    #[test]
    fn checks_emoji() {
        assert!(check_emoji("e", "👩‍👩‍👧‍👦").is_ok());
        assert!(check_emoji("e", "").is_err());
        assert!(check_emoji("e", "👍 👍").is_err());
        assert!(check_emoji("e", &"👍".repeat(17)).is_err());
    }
}
