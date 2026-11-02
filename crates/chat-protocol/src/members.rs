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

/// `PATCH /v1/groups/:id`: renames a group (not a DM). The server trims the name and
/// collapses runs of whitespace to one space before it checks the length. Response:
/// `GroupSummary`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateGroupRequest {
    pub name: String,
}

impl UpdateGroupRequest {
    pub fn validate(&self) -> Validation {
        check_name("name", &self.name)
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
