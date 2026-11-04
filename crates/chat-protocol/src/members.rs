//! Member changes, group notification preferences and the server's notify decision
//! (docs/protocol.md, "Members and preferences" and "Notifications").

use serde::{Deserialize, Serialize};

use crate::characters::CharacterId;
use crate::entities::MemberLevel;
use crate::limits::{Validation, check_chars, check_name};
use crate::serde_util::{non_null, nullable};

/// `PATCH /v1/members/:id`. Owner: every field. Self: `name` and `role` only.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateMemberRequest {
    #[serde(
        default,
        deserialize_with = "non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub name: Option<String>,
    /// `Some(None)` = `null` clears the role.
    #[serde(
        default,
        deserialize_with = "nullable",
        skip_serializing_if = "Option::is_none"
    )]
    pub role: Option<Option<String>>,
    #[serde(
        default,
        deserialize_with = "non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub level: Option<MemberLevel>,
    #[serde(
        default,
        deserialize_with = "non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub deactivated: Option<bool>,
    /// Owner only. `Some(None)` = `null` clears the character.
    #[serde(
        default,
        deserialize_with = "nullable",
        skip_serializing_if = "Option::is_none"
    )]
    pub character: Option<Option<CharacterId>>,
}

impl UpdateMemberRequest {
    pub fn validate(&self) -> Validation {
        if let Some(n) = &self.name {
            check_name("name", n)?;
        }
        if let Some(Some(r)) = &self.role {
            check_chars("role", r, 0, 64)?;
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NotifyLevel {
    All,
    Mentions,
    None,
}

/// `PATCH /v1/groups/:id/prefs`. Response: `GroupSummary`.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateGroupPrefsRequest {
    #[serde(
        default,
        deserialize_with = "non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub muted: Option<bool>,
    #[serde(
        default,
        deserialize_with = "non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub notify: Option<NotifyLevel>,
}

impl UpdateGroupPrefsRequest {
    pub fn validate(&self) -> Validation {
        Ok(())
    }
}

/// Max code points of a group's purpose line (1.1).
pub const MAX_GROUP_PURPOSE_CHARS: usize = 200;

/// Trims and collapses every run of whitespace (`char::is_whitespace`, Unicode White_Space) to
/// one space, as the server stores group names and purposes (TS `normalizeGroupText`).
pub fn normalize_group_text(raw: &str) -> String {
    raw.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// `PATCH /v1/groups/:id`: renames a group (not a DM) and/or sets its purpose line (1.1). The
/// server normalizes both with [`normalize_group_text`], then checks the name (1..64) and the
/// purpose (0..200; empty clears it). At least one of them. Response: `GroupSummary`.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateGroupRequest {
    #[serde(
        default,
        deserialize_with = "non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub name: Option<String>,
    #[serde(
        default,
        deserialize_with = "non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub purpose: Option<String>,
}

impl UpdateGroupRequest {
    pub fn validate(&self) -> Validation {
        if let Some(n) = &self.name {
            check_name("name", n)?;
        }
        if let Some(p) = &self.purpose {
            check_chars("purpose", p, 0, 4 * MAX_GROUP_PURPOSE_CHARS)?;
        }
        if self.name.is_none() && self.purpose.is_none() {
            return Err(crate::limits::ValidationError {
                field: "name",
                message: "name or purpose is required".into(),
            });
        }
        Ok(())
    }
}

/// Why an event should notify the receiving member (`event.notify`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NotifyReason {
    /// The message mentions me.
    Mention,
    /// The topic now waits on me.
    Waiting,
    /// A topic I follow got a result: status done or a new 产出.
    Result,
    /// A new message where my notify level is `all`.
    Message,
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Same vector as packages/protocol/test/checks.test.ts (`normalizeGroupText`).
    #[test]
    fn group_text_uses_unicode_white_space() {
        assert_eq!(
            normalize_group_text("\u{feff}前端\u{85}\u{3000}小组\u{2028} \t"),
            "\u{feff}前端 小组"
        );
    }
}
