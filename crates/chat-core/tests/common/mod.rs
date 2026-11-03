#![allow(dead_code)]
//! Builders for protocol values used by the tests.

use chat_core::proto::*;

pub const ME: &str = "01ME000000000000000000000A";
pub const ALICE: &str = "01ALICE0000000000000000000";
pub const G1: &str = "01G1000000000000000000000A";

pub fn member(id: &str, name: &str) -> Member {
    Member {
        id: id.into(),
        kind: MemberKind::Human,
        name: name.into(),
        level: MemberLevel::Member,
        ..Default::default()
    }
}

pub fn group(id: &str, members: &[&str]) -> Group {
    Group {
        id: id.into(),
        kind: GroupKind::Group,
        name: format!("group {id}"),
        member_ids: members.iter().map(|s| s.to_string()).collect(),
        created_by: members[0].into(),
        created_at: 1_000,
    }
}

pub fn summary(g: Group, last_seq: u64, read_seq: u64, unread: u64) -> GroupSummary {
    GroupSummary {
        group: g,
        last_seq,
        read_seq,
        unread,
        mentions: 0,
        ..Default::default()
    }
}

pub fn topic(id: &str, group_id: &str, root: &str, seq: u64) -> Topic {
    Topic {
        id: id.into(),
        group_id: group_id.into(),
        title: format!("topic {id}"),
        status: TopicStatus::Open,
        created_by: ME.into(),
        created_at: 1_000 + seq,
        root_message_id: root.into(),
        last_seq: seq,
        message_count: 1,
        ..Default::default()
    }
}

pub fn message(
    id: &str,
    group_id: &str,
    topic_id: &str,
    author: &str,
    seq: u64,
    body: &str,
) -> Message {
    Message {
        id: id.into(),
        client_msg_id: format!("c-{id}"),
        group_id: group_id.into(),
        topic_id: topic_id.into(),
        author_id: author.into(),
        body: body.into(),
        mentions: vec![],
        created_seq: seq,
        created_at: 1_000 + seq,
        ..Default::default()
    }
}

pub fn ev(seq: u64, group_id: &str, actor: &str, data: EventData) -> Event {
    Event {
        seq,
        group_id: group_id.into(),
        at: 1_000 + seq,
        actor_id: actor.into(),
        data,
    }
}

pub fn topic_created(seq: u64, group_id: &str, t: Topic) -> Event {
    ev(
        seq,
        group_id,
        ME,
        EventData::TopicCreated(TopicCreatedData { topic: t }),
    )
}

pub fn msg_created(seq: u64, m: Message) -> Event {
    let g = m.group_id.clone();
    let a = m.author_id.clone();
    ev(
        seq,
        &g,
        &a,
        EventData::MessageCreated(Box::new(MessageCreatedData { message: m })),
    )
}
