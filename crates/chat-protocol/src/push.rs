//! Phone push: device registration, the 等你 badge, the data a push carries and answering a card
//! from a notification over HTTP (docs/protocol.md, "Push").

use serde::{Deserialize, Serialize};

use crate::entities::Millis;
use crate::limits::{Validation, ValidationError};
use crate::members::NotifyReason;
use crate::serde_util::non_null;
use crate::settings::NotifyEvent;

/// Max quick answers a push offers.
pub const MAX_PUSH_ACTIONS: usize = 3;
/// Prefix of the APNs category of a push that offers answers: `tc.ask.<messageId>.<rev>`.
pub const PUSH_CATEGORY_ASK: &str = "tc.ask";

fn invalid(field: &'static str, message: &str) -> Validation {
    Err(ValidationError {
        field,
        message: message.into(),
    })
}

/// The push service.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PushPlatform {
    Apns,
}

/// APNs gateway the token belongs to: development builds get `sandbox` tokens.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PushEnvironment {
    Production,
    Sandbox,
}

/// True for lower-case hex of 32..=400 digits.
pub fn is_push_token(s: &str) -> bool {
    (32..=400).contains(&s.len())
        && s.bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

/// True for a bundle id such as `dev.example.chat`.
pub fn is_push_app_id(s: &str) -> bool {
    s.len() <= 155
        && s.split('.').count() >= 2
        && s.split('.')
            .all(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-'))
}

fn is_action_id(s: &str) -> bool {
    (1..=64).contains(&s.len())
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'.' | b':' | b'-'))
}

/// `POST /v1/me/push-devices`: this sign-in's phone wants pushes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RegisterPushDeviceRequest {
    pub platform: PushPlatform,
    pub token: String,
    pub app_id: String,
    pub environment: PushEnvironment,
}

impl RegisterPushDeviceRequest {
    pub fn validate(&self) -> Validation {
        if !is_push_token(&self.token) {
            return invalid("token", "lower-case hex");
        }
        if !is_push_app_id(&self.app_id) {
            return invalid("appId", "bundle id");
        }
        Ok(())
    }
}

/// One registered phone. The push token itself is never sent back.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PushDevice {
    pub id: String,
    pub platform: PushPlatform,
    pub app_id: String,
    pub environment: PushEnvironment,
    pub created_at: Millis,
    pub updated_at: Millis,
    pub current: bool,
}

/// `GET /v1/me/push-devices`.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PushDevicesResponse {
    pub devices: Vec<PushDevice>,
}

/// `DELETE /v1/me/push-devices[/:id]`.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RemovePushDeviceResponse {
    pub removed: u64,
}

/// `GET /v1/me/badge`: topics that wait on me (等你).
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BadgeResponse {
    pub waiting: u64,
}

/// An answer the notification offers as a button.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PushAction {
    pub id: String,
    pub label: String,
}

/// An open question on a card that can be answered from the notification.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PushAsk {
    pub message_id: String,
    pub rev: u32,
    pub actions: Vec<PushAction>,
}

/// The `tc` key of an APNs payload, next to `aps`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PushData {
    pub v: u32,
    pub reason: NotifyReason,
    #[serde(
        default,
        deserialize_with = "non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub notify_event: Option<NotifyEvent>,
    pub group_id: String,
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
    pub message_id: Option<String>,
    pub seq: u64,
    #[serde(
        default,
        deserialize_with = "non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub ask: Option<PushAsk>,
}

impl PushData {
    pub fn validate(&self) -> Validation {
        if self.v != 1 {
            return invalid("v", "unknown version");
        }
        if let Some(ask) = &self.ask {
            if ask.rev == 0 {
                return invalid("ask.rev", "at least 1");
            }
            if ask.actions.is_empty() || ask.actions.len() > MAX_PUSH_ACTIONS {
                return invalid("ask.actions", "1 to 3 actions");
            }
            for a in &ask.actions {
                if !is_action_id(&a.id) {
                    return invalid("ask.actions.id", "action id");
                }
                let n = a.label.chars().count();
                if n == 0 || n > 40 {
                    return invalid("ask.actions.label", "1 to 40 characters");
                }
            }
        }
        Ok(())
    }
}

/// `POST /v1/groups/:id/messages/:messageId/answer`: `card.answer` without a socket.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AnswerCardRequest {
    pub rev: u32,
    pub action_id: String,
    #[serde(
        default,
        deserialize_with = "non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub client_event_id: Option<String>,
}

impl AnswerCardRequest {
    pub fn validate(&self) -> Validation {
        if self.rev == 0 {
            return invalid("rev", "at least 1");
        }
        if !is_action_id(&self.action_id) {
            return invalid("actionId", "action id");
        }
        if let Some(id) = &self.client_event_id {
            let n = id.chars().count();
            if n == 0 || n > 64 {
                return invalid("clientEventId", "1 to 64 characters");
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tokens_and_app_ids() {
        assert!(is_push_token(&"ab".repeat(32)));
        assert!(!is_push_token(&"AB".repeat(32)));
        assert!(!is_push_token("abcd"));
        assert!(is_push_app_id("dev.example.chat"));
        assert!(!is_push_app_id("chat"));
        assert!(!is_push_app_id("dev..chat"));
    }
}
