//! Audit log (docs/protocol.md, "Audit").

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::entities::Millis;
use crate::serde_util::{non_null, nullable};

pub const MAX_AUDIT_LIMIT: u32 = 200;
/// Audit actions of the 1.1 interaction loop (live card events, page events, data pushes).
pub const AUDIT_ACTIONS_INTERACT: [&str; 3] = ["card.event", "artifact.event", "artifact.data"];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CredentialKind {
    Token,
    Admin,
    Hook,
    Invite,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuditCredential {
    pub kind: CredentialKind,
    /// Token, hook or invite record id. Never the secret or its digest.
    #[serde(
        default,
        deserialize_with = "non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuditTarget {
    pub kind: String,
    pub id: String,
}

/// One audited write.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AuditEntry {
    pub id: String,
    pub at: Millis,
    /// `None` for the admin secret.
    pub actor_id: Option<String>,
    #[serde(
        default,
        deserialize_with = "nullable",
        skip_serializing_if = "Option::is_none"
    )]
    pub requested_by: Option<Option<String>>,
    pub credential: AuditCredential,
    /// Dotted action name, such as `member.update`.
    pub action: String,
    #[serde(
        default,
        deserialize_with = "non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub target: Option<AuditTarget>,
    #[serde(
        default,
        deserialize_with = "non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub group_id: Option<String>,
    #[serde(
        default,
        deserialize_with = "non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub details: Option<BTreeMap<String, Value>>,
}

/// `GET /v1/audit?limit=&before=`: newest first.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AuditResponse {
    pub entries: Vec<AuditEntry>,
    pub has_more: bool,
}
