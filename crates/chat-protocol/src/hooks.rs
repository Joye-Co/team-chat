//! Integrations: incoming webhooks and the GitHub adapter (docs/protocol.md, "Integrations").

use serde::{Deserialize, Serialize};

use crate::card::Card;
use crate::entities::Millis;
use crate::limits::{
    MAX_CLIENT_MSG_ID_CHARS, Validation, ValidationError, check_body, check_chars, check_id,
    check_name, check_title,
};
use crate::serde_util::{non_null, nullable};

fn fail(field: &'static str, message: impl Into<String>) -> Validation {
    Err(ValidationError {
        field,
        message: message.into(),
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HookKind {
    Generic,
    Github,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Hook {
    pub id: String,
    pub name: String,
    pub kind: HookKind,
    pub group_id: String,
    /// Default topic.
    #[serde(
        default,
        deserialize_with = "nullable",
        skip_serializing_if = "Option::is_none"
    )]
    pub topic_id: Option<Option<String>>,
    /// The bot member the hook posts as.
    pub member_id: String,
    pub created_by: String,
    pub created_at: Millis,
    #[serde(
        default,
        deserialize_with = "nullable",
        skip_serializing_if = "Option::is_none"
    )]
    pub last_used_at: Option<Option<Millis>>,
}

/// `POST /v1/hooks` (owner)
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateHookRequest {
    pub name: String,
    pub group_id: String,
    #[serde(
        default,
        deserialize_with = "non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub topic_id: Option<String>,
    pub kind: HookKind,
}

impl CreateHookRequest {
    pub fn validate(&self) -> Validation {
        check_name("name", &self.name)?;
        check_id("groupId", &self.group_id)?;
        self.topic_id
            .as_deref()
            .map_or(Ok(()), |t| check_id("topicId", t))
    }
}

/// The secret is shown only in this response.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateHookResponse {
    pub hook: Hook,
    pub url: String,
    pub secret: String,
}

/// `GET /v1/hooks`
pub type HooksResponse = Vec<Hook>;

/// Topic keys: letters, digits and `_ . : # / -`, 1..=128 characters.
pub fn check_topic_key(field: &'static str, key: &str) -> Validation {
    check_chars(field, key, 1, 128)?;
    if key
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | ':' | '#' | '/' | '-'))
    {
        Ok(())
    } else {
        fail(field, "only letters, digits and _ . : # / -")
    }
}

/// `POST /v1/hooks/:id/in` with header `X-Hook-Secret`. Response: `SendResult`.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HookInRequest {
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
    pub topic_key: Option<String>,
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
    pub body: Option<String>,
    #[serde(
        default,
        deserialize_with = "non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub card: Option<Card>,
    #[serde(
        default,
        deserialize_with = "non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub client_msg_id: Option<String>,
}

impl HookInRequest {
    pub fn validate(&self) -> Validation {
        if let Some(t) = &self.topic_id {
            check_id("topicId", t)?;
        }
        if let Some(k) = &self.topic_key {
            check_topic_key("topicKey", k)?;
        }
        if self.topic_id.is_some() && self.topic_key.is_some() {
            return fail("topicKey", "topicId and topicKey exclude each other");
        }
        if let Some(t) = &self.title {
            check_title(t)?;
        }
        if let Some(b) = &self.body {
            check_body("body", b)?;
        }
        if let Some(c) = &self.card {
            c.validate()?;
        }
        if self.body.as_deref().is_none_or(str::is_empty) && self.card.is_none() {
            return fail("body", "body or card is required");
        }
        self.client_msg_id.as_deref().map_or(Ok(()), |c| {
            check_chars("clientMsgId", c, 1, MAX_CLIENT_MSG_ID_CHARS)
        })
    }
}
