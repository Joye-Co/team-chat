use std::sync::Arc;

use chat_core::proto::*;
use chat_core::{OutboxOp, OutboxState};
use serde_json::json;

use super::hand_off_title;
use crate::ChatClient;

const ME: &str = "01ME000000000000000000000A";
const BOT: &str = "01BOT00000000000000000000A";
const ALICE: &str = "01ALICE0000000000000000000";
const G1: &str = "01G1000000000000000000000A";
const DM: &str = "01DM000000000000000000000A";

fn member(id: &str, name: &str, kind: MemberKind) -> Member {
    Member {
        id: id.into(),
        kind,
        name: name.into(),
        level: MemberLevel::Member,
        ..Default::default()
    }
}

fn summary(id: &str, kind: GroupKind, name: &str, members: &[&str], at: u64) -> GroupSummary {
    GroupSummary {
        group: Group {
            purpose: None,
            id: id.into(),
            kind,
            name: name.into(),
            member_ids: members.iter().map(|s| s.to_string()).collect(),
            created_by: ME.into(),
            created_at: at,
        },
        ..Default::default()
    }
}

fn ev(seq: u64, group: &str, actor: &str, data: EventData) -> Event {
    Event {
        seq,
        group_id: group.into(),
        at: 1_000 + seq,
        actor_id: actor.into(),
        data,
    }
}

fn topic(id: &str, status: TopicStatus, at: u64) -> Topic {
    Topic {
        id: id.into(),
        group_id: G1.into(),
        title: format!("话题 {id}"),
        status,
        created_by: ME.into(),
        created_at: at,
        root_message_id: format!("R{id}"),
        last_seq: 1,
        message_count: 1,
        owner_id: Some(Some(BOT.into())),
        ..Default::default()
    }
}

fn message(id: &str, topic: &str, author: &str, seq: u64, at: u64) -> Message {
    Message {
        id: id.into(),
        client_msg_id: format!("c-{id}"),
        group_id: G1.into(),
        topic_id: topic.into(),
        author_id: author.into(),
        body: "请选择".into(),
        created_seq: seq,
        created_at: at,
        ..Default::default()
    }
}

fn card(v: serde_json::Value) -> CardV2 {
    serde_json::from_value(v).unwrap()
}

fn decision(rev: u32) -> CardV2 {
    card(json!({
        "v": 2, "rev": rev, "intent": "ask", "title": "用哪一版？", "fallback": "请选择",
        "ask": { "from": [ME] },
        "blocks": [{ "type": "decision", "id": "choice", "options": [
            { "id": "a", "label": "A 版" }, { "id": "b", "label": "B 版" }] }],
        "actions": [{ "id": "submit", "label": "确定", "style": "primary" },
                    { "id": "cancel", "label": "不了", "kind": "cancel" }]
    }))
}

fn approval() -> CardV2 {
    card(json!({
        "v": 2, "rev": 1, "intent": "ask", "title": "批准上线？", "fallback": "批准上线？",
        "ask": { "from": [ME], "irreversible": { "verb": "上线", "target": "v2", "consequence": "所有人收到" } },
        "blocks": [],
        "actions": [{ "id": "approve", "label": "批准", "style": "danger" },
                    { "id": "deny", "label": "拒绝", "kind": "cancel" }]
    }))
}

fn done_card() -> CardV2 {
    card(json!({
        "v": 2, "rev": 1, "intent": "done", "title": "修好了", "fallback": "修好了", "blocks": []
    }))
}

fn card_msg(id: &str, topic: &str, seq: u64, c: CardV2) -> Event {
    let mut m = message(id, topic, BOT, seq, 10_000 + seq);
    m.kind = Some(MessageKind::Card);
    m.card_v2 = Some(c);
    ev(
        seq,
        G1,
        BOT,
        EventData::MessageCreated(Box::new(MessageCreatedData { message: m })),
    )
}

fn created(seq: u64, t: Topic) -> Event {
    ev(
        seq,
        G1,
        ME,
        EventData::TopicCreated(TopicCreatedData { topic: t }),
    )
}

fn client() -> (tempfile::TempDir, Arc<ChatClient>) {
    let dir = tempfile::tempdir().unwrap();
    let c = ChatClient::new(
        "http://127.0.0.1:9".into(),
        "test-token".into(),
        dir.path().to_string_lossy().into_owned(),
    )
    .unwrap();
    c.inner
        .store()
        .apply_welcome(
            &member(ME, "我", MemberKind::Human),
            &[
                member(ME, "我", MemberKind::Human),
                member(BOT, "Forge", MemberKind::Bot),
                member(ALICE, "小王", MemberKind::Human),
            ],
            &[
                summary(G1, GroupKind::Group, "开发", &[ME, BOT, ALICE], 1_000),
                summary(DM, GroupKind::Dm, "", &[ME, BOT], 500),
            ],
        )
        .unwrap();
    (dir, c)
}

fn apply(c: &ChatClient, events: &[Event]) {
    c.inner.store().apply_events(G1, events).unwrap();
}

#[test]
fn an_open_question_turns_into_a_record_after_the_answer() {
    let (_d, c) = client();
    apply(
        &c,
        &[
            created(1, topic("T1", TopicStatus::WaitingAnswer, 1)),
            card_msg("Q", "T1", 2, decision(1)),
        ],
    );
    let r = c.card_record("Q".into()).unwrap().unwrap();
    assert_eq!(r.state, "open");
    assert!(r.can_answer);
    assert!(c.card_record("NOPE".into()).unwrap().is_none());

    let id = c
        .answer_card(
            "Q".into(),
            1,
            "submit".into(),
            Some(r#"{"choice":"b"}"#.into()),
            None,
        )
        .unwrap();
    assert!(!id.is_empty());
    let r = c.card_record("Q".into()).unwrap().unwrap();
    assert!(r.sending, "my answer is on its way");
    assert!(!r.can_answer);
    assert_eq!(r.action_label, "确定");
    let op = c.inner.store().outbox_all().unwrap().pop().unwrap().op;
    assert!(matches!(op, OutboxOp::CardAnswer { ref action_id, .. } if action_id == "submit"));

    apply(
        &c,
        &[ev(
            3,
            G1,
            ME,
            EventData::CardAnswered(CardAnsweredData {
                message_id: "Q".into(),
                rev: 1,
                action_id: "submit".into(),
                inputs: Some([("choice".to_string(), json!("b"))].into()),
                ..Default::default()
            }),
        )],
    );
    // The server's ack removes the outbox row; here the cache alone shows the answer.
    for item in c.inner.store().outbox_all().unwrap() {
        c.inner.discard(&item.id).unwrap();
    }
    let r = c.card_record("Q".into()).unwrap().unwrap();
    assert_eq!(r.state, "answered");
    assert!(r.answered_by_me);
    assert_eq!(r.answered_by, "我");
    assert_eq!(r.details, ["B 版"]);
    assert!(!r.declined);
    assert!(!r.can_answer);
}

#[test]
fn a_newer_rev_is_open_again_and_bad_json_is_refused() {
    let (_d, c) = client();
    apply(
        &c,
        &[
            created(1, topic("T1", TopicStatus::WaitingAnswer, 1)),
            card_msg("Q", "T1", 2, decision(2)),
        ],
    );
    apply(
        &c,
        &[ev(
            3,
            G1,
            ALICE,
            EventData::CardAnswered(CardAnsweredData {
                message_id: "Q".into(),
                rev: 1,
                action_id: "cancel".into(),
                ..Default::default()
            }),
        )],
    );
    let r = c.card_record("Q".into()).unwrap().unwrap();
    assert_eq!(
        r.state, "open",
        "an answer to an older rev does not close rev 2"
    );
    assert!(matches!(
        c.answer_card("Q".into(), 2, "submit".into(), Some("{".into()), None),
        Err(crate::CoreError::Invalid)
    ));
}

#[test]
fn a_refused_answer_can_be_forgotten() {
    let (_d, c) = client();
    apply(
        &c,
        &[
            created(1, topic("T1", TopicStatus::WaitingReview, 1)),
            card_msg("A", "T1", 2, approval()),
        ],
    );
    c.answer_card("A".into(), 1, "approve".into(), None, None)
        .unwrap();
    let item = c.inner.store().outbox_all().unwrap().pop().unwrap();
    c.inner
        .store()
        .outbox_set_state(&item.id, OutboxState::Failed, Some("conflict"))
        .unwrap();
    let r = c.card_record("A".into()).unwrap().unwrap();
    assert!(r.refused);
    assert!(r.can_answer, "a refused answer lets me answer again");
    c.forget_card_answer("A".into()).unwrap();
    let r = c.card_record("A".into()).unwrap().unwrap();
    assert!(!r.refused && !r.sending);
}

#[test]
fn a_21_card_is_answered_with_a_card_event_and_a_block_commit() {
    let (_d, c) = client();
    let confirm = card(json!({
        "v": 2, "catalog": "tc.card/2.1", "rev": 1, "intent": "ask", "title": "发布？", "fallback": "发布？",
        "ask": { "from": [ME] },
        "blocks": [{ "type": "confirm", "id": "go", "label": "发布？", "yes": "发", "no": "不发", "fallbackText": "发布？" }],
        "actions": [{ "id": "ok", "label": "好", "style": "primary" }]
    }));
    apply(
        &c,
        &[
            created(1, topic("T1", TopicStatus::WaitingAnswer, 1)),
            card_msg("C", "T1", 2, confirm),
        ],
    );
    c.answer_card("C".into(), 1, "ok".into(), None, None)
        .unwrap();
    let ops: Vec<OutboxOp> = c
        .inner
        .store()
        .outbox_all()
        .unwrap()
        .into_iter()
        .map(|i| i.op)
        .collect();
    assert!(
        matches!(&ops[0], OutboxOp::CardEvent { event, action_id: Some(a), .. } if event == "action.press" && a == "ok")
    );
    assert!(c.card_record("C".into()).unwrap().unwrap().sending);

    c.forget_card_answer("C".into()).unwrap();
    for item in c.inner.store().outbox_all().unwrap() {
        c.inner.discard(&item.id).unwrap();
    }
    c.commit_card_block(
        "C".into(),
        1,
        "go".into(),
        "choice.submit".into(),
        r#"{"value":true}"#.into(),
    )
    .unwrap();
    let ops: Vec<OutboxOp> = c
        .inner
        .store()
        .outbox_all()
        .unwrap()
        .into_iter()
        .map(|i| i.op)
        .collect();
    assert!(matches!(&ops[0], OutboxOp::CardEvent { block_id: Some(b), .. } if b == "go"));
}

#[test]
fn a_v1_card_press_is_queued() {
    let (_d, c) = client();
    let mut m = message("V1", "T1", BOT, 2, 2_000);
    m.kind = Some(MessageKind::Card);
    m.card = Some(
        serde_json::from_value(json!({
            "v": 1, "title": "要部署吗", "blocks": [],
            "actions": [{ "id": "yes", "label": "部署" }, { "id": "no", "label": "先不" }]
        }))
        .unwrap(),
    );
    apply(
        &c,
        &[
            created(1, topic("T1", TopicStatus::WaitingAnswer, 1)),
            ev(
                2,
                G1,
                BOT,
                EventData::MessageCreated(Box::new(MessageCreatedData { message: m })),
            ),
        ],
    );
    assert!(c.card_record("V1".into()).unwrap().unwrap().can_answer);
    c.press_card("V1".into(), "yes".into()).unwrap();
    let r = c.card_record("V1".into()).unwrap().unwrap();
    assert!(r.sending);
    assert_eq!(r.action_label, "部署");
}

#[test]
fn home_lists_what_waits_what_is_being_done_and_what_got_done_today() {
    let (_d, c) = client();
    let mut asked = topic("T1", TopicStatus::WaitingAnswer, 1);
    asked.waiting_on = Some(vec![ME.into()]);
    let mut review = topic("T2", TopicStatus::WaitingReview, 2);
    review.waiting_on = Some(vec![ME.into()]);
    apply(
        &c,
        &[
            created(1, asked),
            card_msg("Q", "T1", 2, decision(1)),
            created(3, review),
            created(4, topic("T3", TopicStatus::InProgress, 4)),
            created(5, topic("T4", TopicStatus::Done, 5)),
            card_msg("D", "T4", 6, done_card()),
        ],
    );
    let home = c.home(0).unwrap();
    let waiting: Vec<&str> = home.waiting.iter().map(|r| r.topic_id.as_str()).collect();
    assert!(
        waiting.contains(&"T1") && waiting.contains(&"T2"),
        "{waiting:?}"
    );
    let t1 = home.waiting.iter().find(|r| r.topic_id == "T1").unwrap();
    assert_eq!(t1.title, "用哪一版？");
    assert_eq!(t1.member_name, "Forge");
    assert!(t1.member_is_bot);
    assert_eq!(t1.group_name, "开发");
    assert_eq!(t1.card.as_ref().map(|m| m.id.as_str()), Some("Q"));
    assert!(
        home.waiting
            .iter()
            .find(|r| r.topic_id == "T2")
            .unwrap()
            .card
            .is_none()
    );

    assert_eq!(
        home.working
            .iter()
            .map(|r| r.topic_id.as_str())
            .collect::<Vec<_>>(),
        ["T3"]
    );
    assert_eq!(home.done_today.len(), 1);
    assert_eq!(home.done_today[0].note, "修好了");
    assert_eq!(home.done_today[0].member_name, "Forge");
    // Done before today: not listed.
    assert!(c.home(u64::MAX).unwrap().done_today.is_empty());

    // Answered: no longer waits on me.
    c.answer_card(
        "Q".into(),
        1,
        "submit".into(),
        Some(r#"{"choice":"a"}"#.into()),
        None,
    )
    .unwrap();
    let home = c.home(0).unwrap();
    let t1 = home.waiting.iter().find(|r| r.topic_id == "T1");
    assert!(
        t1.is_some_and(|r| r.card.is_some()),
        "kept while my answer is on its way"
    );
}

#[test]
fn hand_off_starts_a_topic_that_mentions_the_member() {
    let (_d, c) = client();
    let places = c.hand_off_places(BOT.into()).unwrap();
    assert_eq!(places.len(), 2);
    assert!(places[0].is_dm, "the 私聊 comes first");
    assert_eq!(places[0].name, "Forge");
    assert_eq!(places[1].name, "开发");
    assert_eq!(c.hand_off_places(ALICE.into()).unwrap().len(), 1);

    c.hand_off(
        G1.into(),
        BOT.into(),
        "  把登录页的测试补齐\n要覆盖邀请码  ".into(),
    )
    .unwrap();
    let item = c.inner.store().outbox_all().unwrap().pop().unwrap();
    match item.op {
        OutboxOp::Send {
            topic_id,
            title,
            body,
            mentions,
            ..
        } => {
            assert_eq!(topic_id, None);
            assert_eq!(title.as_deref(), Some("把登录页的测试补齐"));
            assert_eq!(body, "@Forge 把登录页的测试补齐\n要覆盖邀请码");
            assert_eq!(mentions, [BOT]);
        }
        other => panic!("not a send: {other:?}"),
    }
    assert!(matches!(
        c.hand_off(G1.into(), BOT.into(), "  ".into()),
        Err(crate::CoreError::Invalid)
    ));
    assert!(matches!(
        c.hand_off(G1.into(), "nobody".into(), "做".into()),
        Err(crate::CoreError::Invalid)
    ));
}

#[test]
fn a_long_first_line_is_cut_for_the_title() {
    let long = "字".repeat(60);
    let title = hand_off_title(&format!("\n\n{long}\n下一行"));
    assert_eq!(title.chars().count(), 41);
    assert!(title.ends_with('…'));
    assert_eq!(hand_off_title("短"), "短");
}
