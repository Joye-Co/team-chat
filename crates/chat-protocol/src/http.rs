//! HTTP API request and response bodies.

use serde::{Deserialize, Serialize};

use crate::characters::CharacterId;
use crate::entities::{Group, GroupSummary, Member, MemberKind, MemberLevel, Millis};
use crate::errors::ErrorBody;
use crate::limits::{MAX_NAME_CHARS, Validation, check_chars, check_id, check_ids, check_name};
use crate::serde_util::{True, non_null};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ErrorResponse {
    pub error: ErrorBody,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BootstrapRequest {
    pub owner_name: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BootstrapResponse {
    pub token: String,
    pub member: Member,
    pub group: Group,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RedeemRequest {
    pub invite_code: String,
    pub device_name: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RedeemResponse {
    pub token: String,
    pub member: Member,
}

/// `GET /v1/me`
pub type MeResponse = Member;
/// `GET /v1/members`
pub type MembersResponse = Vec<Member>;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateInviteRequest {
    pub name: String,
    pub kind: MemberKind,
    pub level: MemberLevel,
    #[serde(
        default,
        deserialize_with = "non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub role: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateInviteResponse {
    pub code: String,
    pub expires_at: Millis,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateBotRequest {
    pub name: String,
    #[serde(
        default,
        deserialize_with = "non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub role: Option<String>,
    #[serde(
        default,
        deserialize_with = "non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub character: Option<CharacterId>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateBotResponse {
    pub member: Member,
    pub token: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RevokeTokensRequest {
    #[serde(
        default,
        deserialize_with = "non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub member_id: Option<String>,
    #[serde(
        default,
        deserialize_with = "non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub all: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RevokeTokensResponse {
    pub revoked: u64,
}

/// `GET /v1/groups`
pub type GroupsResponse = Vec<GroupSummary>;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateGroupRequest {
    pub name: String,
    pub member_ids: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AddGroupMemberRequest {
    pub member_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateDmRequest {
    pub member_id: String,
}

/// Response of `POST /v1/groups`, `POST /v1/groups/:id/members` and `POST /v1/dms`.
pub type GroupSummaryResponse = GroupSummary;

/// `{ ok: true }`, e.g. `DELETE /v1/groups/:id/members/:memberId`.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct OkResponse {
    pub ok: True,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HealthResponse {
    pub ok: True,
    pub version: String,
}

impl BootstrapRequest {
    pub fn validate(&self) -> Validation {
        check_name("ownerName", &self.owner_name)
    }
}

impl RedeemRequest {
    pub fn validate(&self) -> Validation {
        check_chars("inviteCode", &self.invite_code, 1, usize::MAX)?;
        check_chars("deviceName", &self.device_name, 1, MAX_NAME_CHARS)
    }
}

impl CreateInviteRequest {
    pub fn validate(&self) -> Validation {
        check_name("name", &self.name)
    }
}

impl CreateBotRequest {
    pub fn validate(&self) -> Validation {
        check_name("name", &self.name)
    }
}

impl RevokeTokensRequest {
    pub fn validate(&self) -> Validation {
        self.member_id
            .as_deref()
            .map_or(Ok(()), |id| check_id("memberId", id))
    }
}

impl CreateGroupRequest {
    pub fn validate(&self) -> Validation {
        check_name("name", &self.name)?;
        check_ids("memberIds", &self.member_ids)
    }
}

impl AddGroupMemberRequest {
    pub fn validate(&self) -> Validation {
        check_id("memberId", &self.member_id)
    }
}

impl CreateDmRequest {
    pub fn validate(&self) -> Validation {
        check_id("memberId", &self.member_id)
    }
}

/// `PATCH /v1/members/:id` and `PUT /v1/members/:id/avatar`
pub type MemberResponse = Member;

/// `POST /v1/groups/:id/messages`: the `send` frame without `t`, `rid` and `groupId`.
/// Response: [`crate::SendResult`].
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PostMessageRequest {
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
    pub title: Option<String>,
    pub client_msg_id: String,
    pub body: String,
    #[serde(
        default,
        deserialize_with = "non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub mentions: Option<Vec<String>>,
    #[serde(
        default,
        deserialize_with = "non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub streaming: Option<bool>,
    #[serde(
        default,
        deserialize_with = "non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub reply_to: Option<crate::ReplyToRef>,
    #[serde(
        default,
        deserialize_with = "non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub attachments: Option<Vec<crate::FileRef>>,
    #[serde(
        default,
        deserialize_with = "non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub artifact_refs: Option<Vec<crate::ArtifactRef>>,
    #[serde(
        default,
        deserialize_with = "non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub card: Option<crate::Card>,
}

impl PostMessageRequest {
    /// The same checks as a `send` frame.
    pub fn validate(&self) -> Validation {
        self.clone()
            .into_send_frame("r".into(), "g".into())
            .validate()
    }

    /// The equivalent `send` frame.
    pub fn into_send_frame(self, rid: String, group_id: String) -> crate::ClientFrame {
        crate::ClientFrame::Send(crate::SendFrame {
            rid,
            group_id,
            topic_id: self.topic_id,
            title: self.title,
            client_msg_id: self.client_msg_id,
            body: self.body,
            mentions: self.mentions,
            streaming: self.streaming,
            reply_to: self.reply_to,
            attachments: self.attachments,
            artifact_refs: self.artifact_refs,
            card: self.card,
        })
    }
}
