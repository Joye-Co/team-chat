//! Maps chat-core cache rows to the plain view types of `crate::data`.

use chat_core::{
    GroupRow, OutboxItem, OutboxOp, OutboxState, TopicRow,
    proto::{self, GroupKind as PGroupKind, MemberKind as PMemberKind, MemberLevel as PLevel},
};

use crate::data::{
    GroupKind, GroupSummary, Member, MemberKind, MemberLevel, Message, SendState, Topic,
    TopicStatus, clean_name,
};

/// Prefix of the id of a topic that exists only as a queued first message.
pub const PENDING_TOPIC_PREFIX: &str = "pending:";

pub fn pending_topic_id(client_msg_id: &str) -> String {
    format!("{PENDING_TOPIC_PREFIX}{client_msg_id}")
}

/// The `clientMsgId` behind a pending topic id.
pub fn pending_cid(topic_id: &str) -> Option<&str> {
    topic_id.strip_prefix(PENDING_TOPIC_PREFIX)
}

fn ms(v: u64) -> i64 {
    v.min(i64::MAX as u64) as i64
}

fn count(v: u64) -> u32 {
    v.min(u32::MAX as u64) as u32
}

pub fn member(m: &proto::Member, online: bool) -> Member {
    Member {
        id: m.id.clone(),
        kind: match m.kind {
            PMemberKind::Human => MemberKind::Human,
            PMemberKind::Bot => MemberKind::Bot,
        },
        name: clean_name(&m.name),
        role: m.role().map(str::to_string),
        level: match m.level {
            PLevel::Owner => MemberLevel::Owner,
            PLevel::Member => MemberLevel::Member,
        },
        avatar_url: m.avatar_url().map(str::to_string),
        deactivated: m.is_deactivated(),
        online,
        character: m.character(),
    }
}

pub fn status(s: proto::TopicStatus) -> TopicStatus {
    match s {
        proto::TopicStatus::Open => TopicStatus::Open,
        proto::TopicStatus::InProgress => TopicStatus::InProgress,
        proto::TopicStatus::WaitingAnswer => TopicStatus::WaitingAnswer,
        proto::TopicStatus::WaitingReview => TopicStatus::WaitingReview,
        proto::TopicStatus::Blocked => TopicStatus::Blocked,
        proto::TopicStatus::Done => TopicStatus::Done,
    }
}

pub fn status_to_wire(s: TopicStatus) -> proto::TopicStatus {
    match s {
        TopicStatus::Open => proto::TopicStatus::Open,
        TopicStatus::InProgress => proto::TopicStatus::InProgress,
        TopicStatus::WaitingAnswer => proto::TopicStatus::WaitingAnswer,
        TopicStatus::WaitingReview => proto::TopicStatus::WaitingReview,
        TopicStatus::Blocked => proto::TopicStatus::Blocked,
        TopicStatus::Done => proto::TopicStatus::Done,
    }
}

/// One sidebar row. `name_of` finds a member's name for a DM.
pub fn group(row: &GroupRow, me: &str, name_of: impl Fn(&str) -> Option<String>) -> GroupSummary {
    let g = &row.group;
    let kind = match g.kind {
        PGroupKind::Group => GroupKind::Group,
        PGroupKind::Dm => GroupKind::Dm,
    };
    let dm_peer = (kind == GroupKind::Dm)
        .then(|| g.member_ids.iter().find(|id| *id != me).cloned())
        .flatten();
    let name = match (&dm_peer, g.name.trim().is_empty()) {
        (Some(peer), _) => name_of(peer).unwrap_or_else(|| clean_name(&g.name)),
        _ => clean_name(&g.name),
    };
    GroupSummary {
        id: g.id.clone(),
        kind,
        name,
        member_ids: g.member_ids.clone(),
        dm_peer,
        last_seq: row.last_seq,
        read_seq: row.read_seq,
        unread: count(row.unread),
        mentions: count(row.mentions),
        last_activity_at: ms(row.last_activity_at.max(g.created_at)),
    }
}

pub fn topic(row: &TopicRow) -> Topic {
    let t = &row.topic;
    Topic {
        id: t.id.clone(),
        group_id: t.group_id.clone(),
        title: t.title.clone(),
        status: status(t.status),
        waiting_on: t.waiting_on.clone().unwrap_or_default(),
        owner_id: t.owner_id.clone().flatten(),
        created_by: t.created_by.clone(),
        created_at: ms(t.created_at),
        root_message_id: t.root_message_id.clone(),
        last_seq: t.last_seq,
        last_activity_at: ms(row.last_activity_at.max(t.created_at)),
        message_count: count(t.message_count),
        unread: count(row.unread),
        mentions: count(row.mentions),
    }
}

pub fn message(m: &proto::Message) -> Message {
    Message {
        id: m.id.clone(),
        client_msg_id: m.client_msg_id.clone(),
        group_id: m.group_id.clone(),
        topic_id: m.topic_id.clone(),
        author_id: m.author_id.clone(),
        body: m.body.clone(),
        mentions: m.mentions.clone(),
        created_seq: m.created_seq,
        created_at: ms(m.created_at),
        edited_at: m.edited_at().map(ms),
        deleted_at: m.deleted_at().map(ms),
        streaming: m.is_streaming(),
        send_state: SendState::Sent,
        quote_of: m.reply_to.as_ref().map(|r| r.message_id.clone()),
        attachments: file_refs(m.attachments.as_deref()),
        artifact_refs: artifact_refs(m.artifact_refs.as_deref()),
        reactions: m
            .reactions()
            .iter()
            .map(|r| crate::data::Reaction {
                emoji: r.emoji.clone(),
                member_ids: r.member_ids.clone(),
            })
            .collect(),
    }
}

pub fn file_ref(f: &proto::FileRef) -> crate::data::FileAttachment {
    crate::data::FileAttachment {
        file_id: f.file_id.clone(),
        name: f.name.clone(),
        size: f.size,
        mime: f.mime.clone(),
        width: f.width,
        height: f.height,
        sha256: f.sha256.clone(),
    }
}

pub fn to_file_ref(f: &crate::data::FileAttachment) -> proto::FileRef {
    proto::FileRef {
        file_id: f.file_id.clone(),
        name: f.name.clone(),
        size: f.size,
        mime: f.mime.clone(),
        width: f.width,
        height: f.height,
        sha256: f.sha256.clone(),
    }
}

fn file_refs(list: Option<&[proto::FileRef]>) -> Vec<crate::data::FileAttachment> {
    list.unwrap_or_default().iter().map(file_ref).collect()
}

fn artifact_refs(list: Option<&[proto::ArtifactRef]>) -> Vec<crate::data::ArtifactRef> {
    list.unwrap_or_default()
        .iter()
        .map(|r| crate::data::ArtifactRef {
            artifact_id: r.artifact_id.clone(),
            version: r.version,
        })
        .collect()
}

fn send_state(s: OutboxState) -> SendState {
    match s {
        OutboxState::Failed => SendState::Failed,
        _ => SendState::Pending,
    }
}

/// A cached message as a quote.
pub fn quoted(m: &chat_core::proto::Message) -> crate::data::QuotedMessage {
    crate::data::QuotedMessage {
        author_id: m.author_id.clone(),
        body: m.body.clone(),
        deleted: m.deleted_at().is_some(),
    }
}

/// A queued send shown as a pending or failed message. `topic_id` is the id
/// the view asked for (a pending topic id for the first message of a topic).
pub fn outbox_message(item: &OutboxItem, topic_id: &str, me: &str) -> Option<Message> {
    let OutboxOp::Send {
        group_id,
        body,
        mentions,
        reply_to,
        attachments,
        artifact_refs: refs,
        ..
    } = &item.op
    else {
        return None;
    };
    Some(Message {
        id: String::new(),
        client_msg_id: item.id.clone(),
        group_id: group_id.clone(),
        topic_id: topic_id.to_string(),
        author_id: me.to_string(),
        body: body.clone(),
        mentions: mentions.clone(),
        created_seq: 0,
        created_at: ms(item.created_at),
        edited_at: None,
        deleted_at: None,
        streaming: false,
        send_state: send_state(item.state),
        quote_of: reply_to.clone(),
        attachments: file_refs(attachments.as_deref()),
        artifact_refs: artifact_refs(refs.as_deref()),
        reactions: Vec::new(),
    })
}

/// The title a new topic gets: the given title, or the first line cut to 80
/// characters.
pub fn topic_title(title: Option<&str>, body: &str) -> String {
    title
        .map(str::trim)
        .filter(|t| !t.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| {
            body.lines()
                .map(str::trim)
                .find(|l| !l.is_empty())
                .unwrap_or("")
                .chars()
                .take(80)
                .collect()
        })
}

/// A topic that waits for its first message to be acked.
pub fn pending_topic(item: &OutboxItem, me: &str) -> Option<Topic> {
    let OutboxOp::Send {
        group_id,
        topic_id: None,
        title,
        body,
        ..
    } = &item.op
    else {
        return None;
    };
    Some(Topic {
        id: pending_topic_id(&item.id),
        group_id: group_id.clone(),
        title: topic_title(title.as_deref(), body),
        status: TopicStatus::Open,
        waiting_on: Vec::new(),
        owner_id: None,
        created_by: me.to_string(),
        created_at: ms(item.created_at),
        root_message_id: String::new(),
        last_seq: 0,
        last_activity_at: ms(item.created_at),
        message_count: 1,
        unread: 0,
        mentions: 0,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn title_uses_first_non_empty_line_cut_to_80() {
        assert_eq!(topic_title(None, "\n  hello\nworld"), "hello");
        assert_eq!(topic_title(Some("  "), "a"), "a");
        assert_eq!(topic_title(Some("T"), "a"), "T");
        let long = "长".repeat(100);
        assert_eq!(topic_title(None, &long).chars().count(), 80);
    }

    #[test]
    fn names_lose_extra_spaces() {
        // break-ui CATALOG: a pasted name never indents or gaps in a view.
        assert_eq!(clean_name("  Sam   Lee "), "Sam Lee");
        assert_eq!(clean_name("王\u{3000}一鸣\t"), "王 一鸣");
        assert_eq!(clean_name("Jo"), "Jo");
    }

    #[test]
    fn pending_ids_round_trip() {
        let id = pending_topic_id("01ABC");
        assert_eq!(pending_cid(&id), Some("01ABC"));
        assert_eq!(pending_cid("t-1"), None);
    }
}
