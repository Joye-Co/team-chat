//! Protocol types used by the core. TEMPORARY: mirrors docs/protocol.md until
//! `crates/chat-protocol` lands on main; then this module only re-exports it.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub const PROTOCOL_VERSION: u32 = 1;
pub const MAX_PAGE_LIMIT: u32 = 500;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MemberKind {
    Human,
    Bot,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MemberLevel {
    Owner,
    Member,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Member {
    pub id: String,
    pub kind: MemberKind,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub role: Option<String>,
    pub level: MemberLevel,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub avatar_url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub deactivated: Option<bool>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum GroupKind {
    Group,
    Dm,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Group {
    pub id: String,
    pub kind: GroupKind,
    pub name: String,
    pub member_ids: Vec<String>,
    pub created_by: String,
    pub created_at: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GroupSummary {
    pub group: Group,
    pub last_seq: i64,
    pub read_seq: i64,
    pub unread: i64,
    pub mentions: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TopicStatus {
    Open,
    InProgress,
    WaitingAnswer,
    WaitingReview,
    Blocked,
    Done,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Topic {
    pub id: String,
    pub group_id: String,
    pub title: String,
    pub status: TopicStatus,
    pub created_by: String,
    pub created_at: i64,
    pub root_message_id: String,
    pub last_seq: i64,
    pub message_count: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Message {
    pub id: String,
    pub client_msg_id: String,
    pub group_id: String,
    pub topic_id: String,
    pub author_id: String,
    pub body: String,
    pub mentions: Vec<String>,
    pub created_seq: i64,
    pub created_at: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub edited_at: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub deleted_at: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub streaming: Option<bool>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorCode {
    Unauthorized,
    Forbidden,
    NotFound,
    Invalid,
    Conflict,
    RateLimited,
    TooLarge,
    UnsupportedProtocol,
    Internal,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ErrorBody {
    pub code: ErrorCode,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Event {
    pub seq: i64,
    pub group_id: String,
    pub at: i64,
    pub actor_id: String,
    #[serde(flatten)]
    pub data: EventData,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", content = "data")]
pub enum EventData {
    #[serde(rename = "topic.created")]
    TopicCreated { topic: Topic },
    #[serde(rename = "topic.updated", rename_all = "camelCase")]
    TopicUpdated {
        topic_id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        title: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        status: Option<TopicStatus>,
    },
    #[serde(rename = "message.created")]
    MessageCreated { message: Message },
    #[serde(rename = "message.updated", rename_all = "camelCase")]
    MessageUpdated {
        message_id: String,
        body: String,
        edited_at: i64,
        streaming: bool,
    },
    #[serde(rename = "message.deleted", rename_all = "camelCase")]
    MessageDeleted { message_id: String, deleted_at: i64 },
    #[serde(rename = "group.updated")]
    GroupUpdated { name: String },
    #[serde(rename = "group.memberAdded", rename_all = "camelCase")]
    GroupMemberAdded { member_id: String },
    #[serde(rename = "group.memberRemoved", rename_all = "camelCase")]
    GroupMemberRemoved { member_id: String },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum TransientEvent {
    #[serde(rename = "typing", rename_all = "camelCase")]
    Typing {
        group_id: String,
        topic_id: String,
        member_id: String,
        at: i64,
    },
    #[serde(rename = "presence", rename_all = "camelCase")]
    Presence {
        member_id: String,
        online: bool,
        at: i64,
    },
    #[serde(rename = "message.delta", rename_all = "camelCase")]
    MessageDelta {
        group_id: String,
        topic_id: String,
        message_id: String,
        append: String,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ClientInfo {
    pub name: String,
    pub version: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "t")]
pub enum ClientFrame {
    #[serde(rename = "hello")]
    Hello {
        protocol: u32,
        client: ClientInfo,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        cursors: Option<BTreeMap<String, i64>>,
    },
    #[serde(rename = "send", rename_all = "camelCase")]
    Send {
        rid: String,
        group_id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        topic_id: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        title: Option<String>,
        client_msg_id: String,
        body: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        mentions: Option<Vec<String>>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        streaming: Option<bool>,
    },
    #[serde(rename = "edit", rename_all = "camelCase")]
    Edit {
        rid: String,
        group_id: String,
        message_id: String,
        body: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        streaming: Option<bool>,
    },
    #[serde(rename = "delta", rename_all = "camelCase")]
    Delta {
        group_id: String,
        message_id: String,
        append: String,
    },
    #[serde(rename = "delete", rename_all = "camelCase")]
    Delete {
        rid: String,
        group_id: String,
        message_id: String,
    },
    #[serde(rename = "topic.update", rename_all = "camelCase")]
    TopicUpdate {
        rid: String,
        group_id: String,
        topic_id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        title: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        status: Option<TopicStatus>,
    },
    #[serde(rename = "typing", rename_all = "camelCase")]
    Typing { group_id: String, topic_id: String },
    #[serde(rename = "read", rename_all = "camelCase")]
    Read { group_id: String, seq: i64 },
    #[serde(rename = "fetch", rename_all = "camelCase")]
    Fetch {
        rid: String,
        group_id: String,
        after_seq: i64,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        limit: Option<u32>,
    },
    #[serde(rename = "history", rename_all = "camelCase")]
    History {
        rid: String,
        group_id: String,
        topic_id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        before_seq: Option<i64>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        limit: Option<u32>,
    },
    #[serde(rename = "ping")]
    Ping,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SendResult {
    pub message: Message,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub topic: Option<Topic>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duplicate: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "t")]
pub enum ServerFrame {
    #[serde(rename = "welcome", rename_all = "camelCase")]
    Welcome {
        protocol: u32,
        me: Member,
        members: Vec<Member>,
        groups: Vec<GroupSummary>,
        server_time: i64,
    },
    #[serde(rename = "event")]
    Event { event: Event },
    #[serde(rename = "transient")]
    Transient { event: TransientEvent },
    #[serde(rename = "ack")]
    Ack {
        rid: String,
        ok: bool,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        result: Option<serde_json::Value>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        error: Option<ErrorBody>,
    },
    #[serde(rename = "page", rename_all = "camelCase")]
    Page {
        rid: String,
        group_id: String,
        events: Vec<Event>,
        has_more: bool,
        last_seq: i64,
    },
    #[serde(rename = "history", rename_all = "camelCase")]
    History {
        rid: String,
        group_id: String,
        topic_id: String,
        messages: Vec<Message>,
        has_more: bool,
    },
    #[serde(rename = "directory", rename_all = "camelCase")]
    Directory {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        members: Option<Vec<Member>>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        groups: Option<Vec<GroupSummary>>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        removed_group_ids: Option<Vec<String>>,
    },
    #[serde(rename = "error")]
    Error { error: ErrorBody },
    #[serde(rename = "pong")]
    Pong,
}

// HTTP bodies.

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RedeemRequest {
    pub invite_code: String,
    pub device_name: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RedeemResponse {
    pub token: String,
    pub member: Member,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateInviteRequest {
    pub name: String,
    pub kind: MemberKind,
    pub level: MemberLevel,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub role: Option<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateInviteResponse {
    pub code: String,
    pub expires_at: i64,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateBotRequest {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub role: Option<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateBotResponse {
    pub member: Member,
    pub token: String,
}
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RevokeTokensRequest {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub member_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub all: Option<bool>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RevokeTokensResponse {
    pub revoked: i64,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateGroupRequest {
    pub name: String,
    pub member_ids: Vec<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MemberIdRequest {
    pub member_id: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ErrorResponse {
    pub error: ErrorBody,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HealthResponse {
    pub ok: bool,
    pub version: String,
}
