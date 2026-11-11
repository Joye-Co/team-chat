//! Members, groups, topics and messages.

use serde::{Deserialize, Serialize};

use crate::card::Card;
use crate::card2::{CardV2, lenient_card_v2};
use crate::characters::CharacterId;
use crate::members::NotifyLevel;
use crate::message::{ArtifactRef, FileRef, MessageKind, ReplyTo};
use crate::reactions::Reaction;
use crate::reads::TopicState;
use crate::serde_util::{non_null, nullable};
use crate::topic::TopicContext;

/// Milliseconds since the Unix epoch, UTC.
pub type Millis = u64;
/// Position in a group event log.
pub type Seq = u64;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MemberKind {
    #[default]
    Human,
    Bot,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MemberLevel {
    Owner,
    #[default]
    Member,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Member {
    pub id: String,
    pub kind: MemberKind,
    pub name: String,
    /// `None` = omitted, `Some(None)` = `null`.
    #[serde(
        default,
        deserialize_with = "nullable",
        skip_serializing_if = "Option::is_none"
    )]
    pub role: Option<Option<String>>,
    pub level: MemberLevel,
    /// `None` = omitted, `Some(None)` = `null`.
    #[serde(
        default,
        deserialize_with = "nullable",
        skip_serializing_if = "Option::is_none"
    )]
    pub avatar_url: Option<Option<String>>,
    #[serde(
        default,
        deserialize_with = "non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub deactivated: Option<bool>,
    /// Character id. `None` = omitted, `Some(None)` = `null`.
    #[serde(
        default,
        deserialize_with = "nullable",
        skip_serializing_if = "Option::is_none"
    )]
    pub character: Option<Option<CharacterId>>,
    /// 1.1: who the member reports to (汇报给); `None` = nobody.
    #[serde(
        default,
        deserialize_with = "non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub reports_to: Option<String>,
    /// 1.1: the weekly spending limit in US cents (限额); `None` = none.
    #[serde(
        default,
        deserialize_with = "non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub weekly_budget_cents: Option<u64>,
    /// Usable: `Some(true)` for a 所有者 (always level owner); sent only when true.
    #[serde(
        default,
        deserialize_with = "non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub team_owner: Option<bool>,
}

impl Member {
    /// Usable: a 所有者 (team owner). Every 所有者 is also an admin (`level` owner).
    pub fn is_team_owner(&self) -> bool {
        self.team_owner == Some(true)
    }

    pub fn character(&self) -> Option<CharacterId> {
        self.character.flatten()
    }

    pub fn role(&self) -> Option<&str> {
        self.role.as_ref().and_then(|r| r.as_deref())
    }
    pub fn avatar_url(&self) -> Option<&str> {
        self.avatar_url.as_ref().and_then(|r| r.as_deref())
    }
    pub fn is_deactivated(&self) -> bool {
        self.deactivated.unwrap_or(false)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GroupKind {
    #[default]
    Group,
    Dm,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Group {
    pub id: String,
    pub kind: GroupKind,
    pub name: String,
    pub member_ids: Vec<String>,
    pub created_by: String,
    pub created_at: Millis,
    /// 1.1: the purpose line under the group's name.
    #[serde(
        default,
        deserialize_with = "crate::serde_util::non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub purpose: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GroupSummary {
    pub group: Group,
    pub last_seq: Seq,
    pub read_seq: Seq,
    /// Group totals over the per-topic read positions.
    pub unread: u64,
    pub mentions: u64,
    /// Every topic with unread > 0, mentions > 0, following or muted. Topics not listed have
    /// no unread messages and are neither followed nor muted.
    #[serde(
        default,
        deserialize_with = "non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub topics: Option<Vec<TopicState>>,
    /// Absent means false.
    #[serde(
        default,
        deserialize_with = "non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub muted: Option<bool>,
    /// Absent: `all` for a DM, `mentions` for a group; see [`GroupSummary::notify_level`].
    #[serde(
        default,
        deserialize_with = "non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub notify: Option<NotifyLevel>,
}

impl GroupSummary {
    pub fn is_muted(&self) -> bool {
        self.muted.unwrap_or(false)
    }
    /// The effective notify level, with the default for the group kind.
    pub fn notify_level(&self) -> NotifyLevel {
        self.notify.unwrap_or(match self.group.kind {
            GroupKind::Dm => NotifyLevel::All,
            GroupKind::Group => NotifyLevel::Mentions,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TopicStatus {
    #[default]
    Open,
    InProgress,
    WaitingAnswer,
    WaitingReview,
    Blocked,
    Done,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Topic {
    pub id: String,
    pub group_id: String,
    pub title: String,
    pub status: TopicStatus,
    pub created_by: String,
    pub created_at: Millis,
    pub root_message_id: String,
    pub last_seq: Seq,
    pub message_count: u64,
    /// Members the topic waits on. Absent means empty.
    #[serde(
        default,
        deserialize_with = "non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub waiting_on: Option<Vec<String>>,
    /// The responsible member. `None` = omitted, `Some(None)` = `null`; both mean none.
    #[serde(
        default,
        deserialize_with = "nullable",
        skip_serializing_if = "Option::is_none"
    )]
    pub owner_id: Option<Option<String>>,
    /// `createdAt` of the newest message in the topic.
    #[serde(
        default,
        deserialize_with = "non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub last_message_at: Option<Millis>,
    /// Set on a side chat topic (1.1): the topic it is about.
    #[serde(
        default,
        deserialize_with = "non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub context_of: Option<TopicContext>,
    /// The group's own chat (群聊, 1.1.1); its id is [`crate::main_topic_id`]. Absent = false.
    #[serde(
        default,
        deserialize_with = "non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub main: Option<bool>,
    /// Usable: AI members that ran in the topic; `None` = none.
    #[serde(
        default,
        deserialize_with = "non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub participants: Option<Vec<crate::relay::TopicParticipant>>,
    /// Usable: the topic's latest relay (按顺序做); `None` = none.
    #[serde(
        default,
        deserialize_with = "non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub relay: Option<crate::relay::TopicRelay>,
}

impl Topic {
    pub fn waiting_on(&self) -> &[String] {
        self.waiting_on.as_deref().unwrap_or(&[])
    }
    /// Whether this is the group's 群聊 (the flag, or the fixed id).
    pub fn is_main(&self) -> bool {
        self.main == Some(true) || self.id == crate::main_topic_id(&self.group_id)
    }
    pub fn owner_id(&self) -> Option<&str> {
        self.owner_id.as_ref().and_then(|o| o.as_deref())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Message {
    pub id: String,
    pub client_msg_id: String,
    pub group_id: String,
    pub topic_id: String,
    pub author_id: String,
    /// Markdown. Empty for a deleted message.
    pub body: String,
    pub mentions: Vec<String>,
    pub created_seq: Seq,
    pub created_at: Millis,
    /// `None` = omitted, `Some(None)` = `null`.
    #[serde(
        default,
        deserialize_with = "nullable",
        skip_serializing_if = "Option::is_none"
    )]
    pub edited_at: Option<Option<Millis>>,
    /// `None` = omitted, `Some(None)` = `null`.
    #[serde(
        default,
        deserialize_with = "nullable",
        skip_serializing_if = "Option::is_none"
    )]
    pub deleted_at: Option<Option<Millis>>,
    #[serde(
        default,
        deserialize_with = "non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub streaming: Option<bool>,
    /// Absent means `text`; see [`Message::kind`].
    #[serde(
        default,
        deserialize_with = "non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub kind: Option<MessageKind>,
    #[serde(
        default,
        deserialize_with = "non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub reply_to: Option<ReplyTo>,
    #[serde(
        default,
        deserialize_with = "non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub attachments: Option<Vec<FileRef>>,
    #[serde(
        default,
        deserialize_with = "non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub artifact_refs: Option<Vec<ArtifactRef>>,
    #[serde(
        default,
        deserialize_with = "non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub card: Option<Card>,
    /// Card v2 (1.1). Read leniently: a value this version cannot parse is dropped.
    #[serde(
        default,
        deserialize_with = "lenient_card_v2",
        skip_serializing_if = "Option::is_none"
    )]
    pub card_v2: Option<CardV2>,
    /// 2.1: server-owned state of the card (asks, answers, tallies).
    #[serde(
        default,
        deserialize_with = "non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub card_state: Option<crate::card2::CardState>,
    /// 2.1: the event this message (or its newest rev) answered.
    #[serde(
        default,
        deserialize_with = "non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub cause: Option<crate::card2::Cause>,
    /// 1.1: the parts this message asks about (引用提问).
    #[serde(
        default,
        deserialize_with = "non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub block_refs: Option<Vec<crate::message::BlockRef>>,
    /// Who reacted with which emoji; absent = none.
    #[serde(
        default,
        deserialize_with = "non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub reactions: Option<Vec<Reaction>>,
    /// 1.1: the side chat answer this message was posted from.
    #[serde(
        default,
        deserialize_with = "crate::serde_util::non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub from_aside: Option<crate::topic::AsideOrigin>,
}

impl Message {
    pub fn reactions(&self) -> &[Reaction] {
        self.reactions.as_deref().unwrap_or(&[])
    }
    pub fn kind(&self) -> MessageKind {
        self.kind.unwrap_or(MessageKind::Text)
    }
    pub fn attachments(&self) -> &[FileRef] {
        self.attachments.as_deref().unwrap_or(&[])
    }
    pub fn artifact_refs(&self) -> &[ArtifactRef] {
        self.artifact_refs.as_deref().unwrap_or(&[])
    }
    pub fn edited_at(&self) -> Option<Millis> {
        self.edited_at.flatten()
    }
    pub fn deleted_at(&self) -> Option<Millis> {
        self.deleted_at.flatten()
    }
    pub fn is_deleted(&self) -> bool {
        self.deleted_at().is_some()
    }
    pub fn is_streaming(&self) -> bool {
        self.streaming.unwrap_or(false)
    }
}
