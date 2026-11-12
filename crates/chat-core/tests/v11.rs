//! 1.1 cache: Card v2 answers and decisions, topic links, 产出 page events, side chats,
//! active runs, terminal lines, and the one-time recovery of logs that 1.0.x applied.

mod common;
mod mock;

use chat_core::proto::machine::{RunEvent, RunLogEntry};
use chat_core::proto::*;
use chat_core::{
    Client, ClientConfig, ClientUpdate, LogEntry, OutboxOp, Store, TerminalLine, terminal_lines,
};
use common::*;
use mock::{MockServer, TOKEN};
use serde_json::json;
use std::time::{Duration, Instant};

const WAIT: Duration = Duration::from_secs(10);
const DM: &str = "01DM000000000000000000000A";
const BOT: &str = "01BOT00000000000000000000A";

fn open() -> (tempfile::TempDir, Store) {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open(&dir.path().join("cache.sqlite3")).unwrap();
    store
        .apply_welcome(
            &member(ME, "me"),
            &[member(ME, "me"), member(ALICE, "alice")],
            &[summary(group(G1, &[ME, ALICE]), 0, 0, 0)],
        )
        .unwrap();
    (dir, store)
}

fn ask_card(rev: u32, record: bool) -> CardV2 {
    serde_json::from_value(json!({
        "v": 2, "rev": rev, "intent": "ask", "title": "发布吗？", "fallback": "请选择",
        "ask": { "from": ["owners", ME], "record": record },
        "blocks": [{ "type": "decision", "id": "when", "options": [{ "id": "now", "label": "现在" }, { "id": "later", "label": "晚点" }] }],
        "actions": [{ "id": "ok", "label": "好" }]
    }))
    .unwrap()
}

fn card_message(id: &str, seq: u64, card: CardV2) -> Message {
    Message {
        kind: Some(MessageKind::Card),
        card_v2: Some(card),
        ..message(id, G1, "T1", ALICE, seq, "请选择")
    }
}

fn answered(seq: u64, actor: &str, message_id: &str, rev: u32) -> Event {
    ev(
        seq,
        G1,
        actor,
        EventData::CardAnswered(CardAnsweredData {
            message_id: message_id.into(),
            rev,
            action_id: "ok".into(),
            inputs: Some([("when".to_string(), json!("now"))].into()),
            edited_values: None,
            answer_id: Some(format!("AN{seq}")),
            ask_id: Some("a1".into()),
            ..Default::default()
        }),
    )
}

fn run(id: &str, status: RunStatus, created: u64) -> BotRun {
    BotRun {
        queue_position: None,
        queued_at: None,
        id: id.into(),
        bot_id: BOT.into(),
        group_id: G1.into(),
        topic_id: "T1".into(),
        machine_id: None,
        status,
        requested_by: Some(ME.into()),
        created_at: created,
        started_at: None,
        ended_at: None,
        updated_at: created,
        summary: None,
        plan: None,
        usage: Some(RunUsage {
            input_tokens: 10,
            output_tokens: 5,
            cost_usd: serde_json::Number::from_f64(0.25),
        }),
        duration_ms: Some(1000),
        result_message_id: None,
        acceptance: None,
        paused_by: None,
        kind: None,
        retry_of: None,
        requested_for: None,
        chain_id: None,
        code: None,
        model: None,
        effort: None,
        failure: None,
        phase: None,
        waiting_for: None,
        hold_kind: None,
        held_at: None,
        takeover: None,
        last_handback: None,
        supervision: None,
    }
}

#[test]
fn answers_decisions_links_page_events_and_runs_are_cached() {
    let (_d, s) = open();
    let link = |seq: u64, state: &str| {
        ev(
            seq,
            G1,
            ALICE,
            EventData::TopicLinked(TopicLinkedData {
                topic_id: "T1".into(),
                link: TopicLink::Pr(PrLink {
                    repo: "a/b".into(),
                    number: 1,
                    url: "https://github.com/a/b/pull/1".into(),
                    title: "fix".into(),
                    state: state.into(),
                    checks: vec![PrCheck {
                        name: "CI".into(),
                        conclusion: "success".into(),
                    }],
                    ..Default::default()
                }),
            }),
        )
    };
    let out = s
        .apply_events(
            G1,
            &[
                topic_created(1, G1, topic("T1", G1, "M1", 1)),
                msg_created(2, message("M1", G1, "T1", ME, 2, "root")),
                msg_created(3, card_message("C1", 3, ask_card(1, true))),
                msg_created(4, card_message("C2", 4, ask_card(1, false))),
                answered(5, ME, "C1", 1),
                answered(6, ME, "C2", 1),
                link(7, "open"),
                link(8, "merged"),
                ev(
                    9,
                    G1,
                    ALICE,
                    EventData::CardRetracted(CardRetractedData {
                        message_id: "C2".into(),
                        answer_id: "AN6".into(),
                        ask_id: Some("a1".into()),
                        event_id: None,
                    }),
                ),
                ev(
                    10,
                    G1,
                    BOT,
                    EventData::RunUpdated(RunUpdatedData {
                        run: run("R1", RunStatus::Running, 100),
                    }),
                ),
                ev(
                    11,
                    G1,
                    BOT,
                    EventData::RunUpdated(RunUpdatedData {
                        run: run("R2", RunStatus::Done, 200),
                    }),
                ),
            ],
        )
        .unwrap();
    assert_eq!(out.applied, 11);
    assert!(out.message_topics.contains("T1"));

    // The cached message keeps its v2 card.
    assert_eq!(
        s.message("C1").unwrap().unwrap().card_v2,
        Some(ask_card(1, true))
    );

    let answers = s.card_answers("C1").unwrap();
    assert_eq!(answers.len(), 1);
    assert_eq!((answers[0].rev, answers[0].actor_id.as_str()), (1, ME));
    assert_eq!(answers[0].inputs.as_ref().unwrap()["when"], json!("now"));

    // Only answers to cards with ask.record are decisions.
    let decisions = s.decisions(Some(G1), 10).unwrap();
    assert_eq!(decisions.len(), 1);
    assert_eq!(decisions[0].message_id, "C1");
    assert_eq!(decisions[0].card.title, "发布吗？");
    assert_eq!(s.decisions(None, 10).unwrap().len(), 1);

    // A link keeps its newest state.
    let links = s.topic_links("T1").unwrap();
    assert_eq!(links.len(), 1);
    let TopicLink::Pr(pr) = &links[0] else {
        panic!("not a pr")
    };
    assert_eq!(pr.state, "merged");

    // A retract marks the answer; it is no longer a decision.
    let c2 = s.card_answers("C2").unwrap();
    assert_eq!(c2[0].answer_id.as_deref(), Some("AN6"));
    assert_eq!(c2[0].retracted_at, Some(1_009));
    assert!(s.card_answers("C1").unwrap()[0].retracted_at.is_none());

    let active = s.active_runs().unwrap();
    assert_eq!(
        active.iter().map(|r| r.id.as_str()).collect::<Vec<_>>(),
        ["R1"]
    );
    assert_eq!(active[0].usage.as_ref().unwrap().cost_usd(), Some(0.25));
    assert_eq!(s.runs_since(150).unwrap().len(), 1);

    // Applying the same events again changes nothing.
    let again = s.apply_events(G1, &[answered(5, ME, "C1", 1)]).unwrap();
    assert_eq!(again.applied, 0);
    assert_eq!(s.card_answers("C1").unwrap().len(), 1);
}

#[test]
fn edits_replace_the_v2_card_and_deletes_clear_it() {
    let (_d, s) = open();
    s.apply_events(
        G1,
        &[
            topic_created(1, G1, topic("T1", G1, "M1", 1)),
            msg_created(2, card_message("C1", 2, ask_card(1, true))),
            ev(
                3,
                G1,
                ALICE,
                EventData::MessageUpdated(MessageUpdatedData {
                    message_id: "C1".into(),
                    body: "已定".into(),
                    edited_at: 5,
                    streaming: false,
                    card_v2: Some(ask_card(2, true)),
                    ..Default::default()
                }),
            ),
        ],
    )
    .unwrap();
    assert_eq!(s.message("C1").unwrap().unwrap().card_v2.unwrap().rev, 2);
    s.apply_events(
        G1,
        &[ev(
            4,
            G1,
            ALICE,
            EventData::MessageDeleted(MessageDeletedData {
                message_id: "C1".into(),
                deleted_at: 6,
            }),
        )],
    )
    .unwrap();
    assert_eq!(s.message("C1").unwrap().unwrap().card_v2, None);
}

#[test]
fn topics_that_cite_an_artifact_are_found_by_their_ref_blocks() {
    let (_d, s) = open();
    let cite = |id: &str, topic: &str, seq: u64, artifact: &str| {
        let card: CardV2 = serde_json::from_value(json!({
            "v": 2, "rev": 1, "intent": "info", "title": "周报", "fallback": "周报",
            "blocks": [{ "type": "group", "id": "g", "blocks": [
                { "type": "ref", "id": "r", "label": "报告",
                  "target": { "kind": "artifact", "artifactId": artifact, "version": 2 } }
            ] }]
        }))
        .unwrap();
        Message {
            kind: Some(MessageKind::Card),
            card_v2: Some(card),
            ..message(id, G1, topic, ALICE, seq, "")
        }
    };
    s.apply_events(
        G1,
        &[
            topic_created(1, G1, topic("T1", G1, "M1", 1)),
            topic_created(2, G1, topic("T2", G1, "M2", 2)),
            msg_created(3, cite("C1", "T1", 3, "A1")),
            msg_created(4, cite("C2", "T2", 4, "A1")),
            msg_created(5, cite("C3", "T2", 5, "A1")),
            // Only the id inside the body: not a ref.
            msg_created(6, message("M6", G1, "T1", ALICE, 6, "A9")),
            msg_created(7, cite("C4", "T1", 7, "A9X")),
        ],
    )
    .unwrap();
    assert_eq!(
        s.artifact_citations("A1", 10).unwrap(),
        [
            (G1.to_string(), "T2".to_string()),
            (G1.to_string(), "T1".to_string())
        ]
    );
    assert_eq!(s.artifact_citations("A1", 1).unwrap().len(), 1);
    assert!(s.artifact_citations("A9", 10).unwrap().is_empty());
}

#[test]
fn side_chat_topics_are_listed_by_their_context() {
    let dir = tempfile::tempdir().unwrap();
    let s = Store::open(&dir.path().join("cache.sqlite3")).unwrap();
    let mut dm = group(DM, &[ME, BOT]);
    dm.kind = GroupKind::Dm;
    s.apply_welcome(
        &member(ME, "me"),
        &[member(ME, "me"), member(BOT, "jojo")],
        &[
            summary(group(G1, &[ME, BOT]), 0, 0, 0),
            summary(dm, 0, 0, 0),
        ],
    )
    .unwrap();
    let context = TopicContext {
        group_id: G1.into(),
        topic_id: "T1".into(),
        upto_seq: 2,
    };
    let side = Topic {
        context_of: Some(context.clone()),
        ..topic("S1", DM, "SM1", 1)
    };
    s.apply_events(
        DM,
        &[
            topic_created(1, DM, side),
            topic_created(2, DM, topic("S2", DM, "SM2", 2)),
        ],
    )
    .unwrap();
    let list = s.side_chats(G1, "T1").unwrap();
    assert_eq!(list.len(), 1);
    assert_eq!(list[0].topic.id, "S1");
    assert_eq!(list[0].topic.context_of.as_ref().unwrap().upto_seq, 2);

    // A later send moves the context on.
    s.apply_events(
        DM,
        &[ev(
            3,
            DM,
            ME,
            EventData::TopicUpdated(TopicUpdatedData {
                topic_id: "S1".into(),
                context_of: Some(TopicContext {
                    upto_seq: 9,
                    ..context
                }),
                ..Default::default()
            }),
        )],
    )
    .unwrap();
    let list = s.side_chats(G1, "T1").unwrap();
    assert_eq!(list[0].topic.context_of.as_ref().unwrap().upto_seq, 9);
    assert!(s.side_chats(G1, "T2").unwrap().is_empty());
}

#[test]
fn terminal_lines_come_from_tool_calls_and_results() {
    let e = |seq: u64, event: RunEvent| RunLogEntry::RunEvent {
        run_id: "R1".into(),
        seq,
        at: 100 + seq,
        event,
    };
    let lines = terminal_lines(&[
        e(
            3,
            RunEvent::ToolResult {
                item_id: "t1".into(),
                turn_id: None,
                ok: true,
                output: Some("ok".into()),
                exit_code: Some(0),
            },
        ),
        e(
            1,
            RunEvent::Message {
                item_id: "m1".into(),
                turn_id: None,
                role: "assistant".into(),
                text: "hi".into(),
            },
        ),
        e(
            2,
            RunEvent::ToolCall {
                item_id: "t1".into(),
                turn_id: None,
                tool: "Bash".into(),
                title: "Bash: cargo test".into(),
                input: Some(json!({"command": "cargo test"})),
            },
        ),
        e(
            4,
            RunEvent::Acceptance {
                items: vec![AcceptanceItem {
                    text: "tests pass".into(),
                    state: AcceptanceState::Pass,
                    evidence: None,
                }],
            },
        ),
    ]);
    assert_eq!(lines.len(), 2);
    assert!(
        matches!(&lines[0], TerminalLine::Command { title, at: 102, .. } if title == "Bash: cargo test")
    );
    assert!(matches!(
        &lines[1],
        TerminalLine::Output {
            ok: true,
            exit_code: Some(0),
            ..
        }
    ));

    let mut view = chat_core::ProcessView::default();
    view.apply(&[e(
        4,
        RunEvent::Paused {
            paused: true,
            by: Some(ME.into()),
        },
    )]);
    assert_eq!(view.paused, Some((true, Some(ME.into()))));
}

#[test]
fn a_cache_from_1_0_records_every_synced_group_for_recovery() {
    use chat_core::store::migrations::{MIGRATIONS, run};
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("c.sqlite3");
    {
        let mut conn = rusqlite::Connection::open(&path).unwrap();
        run(&mut conn, &MIGRATIONS[..3]).unwrap();
        conn.execute(
            "INSERT INTO sync_state (group_id, last_applied) VALUES (?1, 7), ('G0', 0)",
            [G1],
        )
        .unwrap();
    }
    let s = Store::open(&path).unwrap();
    let r = s.refetches().unwrap();
    assert_eq!(r.len(), 1);
    assert_eq!((r[0].0.as_str(), r[0].1.after, r[0].1.upto), (G1, 0, 7));
    // A fresh cache has nothing to recover.
    let (_d, fresh) = open();
    assert!(fresh.refetches().unwrap().is_empty());
}

#[test]
fn recovery_keeps_a_newer_card_and_context_applied_before_it() {
    let (_d, s) = open();
    let side = |upto_seq: u64| TopicContext {
        group_id: G1.into(),
        topic_id: "T1".into(),
        upto_seq,
    };
    // As 1.0.x left it: seq 1..3 applied, the card and the context dropped.
    s.apply_events(
        G1,
        &[
            topic_created(1, G1, topic("T1", G1, "M1", 1)),
            topic_created(2, G1, topic("S1", G1, "SM1", 2)),
            msg_created(
                3,
                Message {
                    kind: Some(MessageKind::Card),
                    ..message("C1", G1, "T1", ALICE, 3, "请选择")
                },
            ),
        ],
    )
    .unwrap();
    s.with_writer(|tx| {
        tx.execute(
            "INSERT INTO log_refetch (group_id, upto) VALUES (?1, 3)",
            [G1],
        )?;
        Ok(())
    })
    .unwrap();
    // The catch-up applies newer edits before the recovery page arrives.
    s.apply_events(
        G1,
        &[
            ev(
                4,
                G1,
                ALICE,
                EventData::MessageUpdated(MessageUpdatedData {
                    message_id: "C1".into(),
                    body: "已定".into(),
                    edited_at: 5,
                    streaming: false,
                    card_v2: Some(ask_card(2, true)),
                    ..Default::default()
                }),
            ),
            ev(
                5,
                G1,
                ME,
                EventData::TopicUpdated(TopicUpdatedData {
                    topic_id: "S1".into(),
                    context_of: Some(side(40)),
                    ..Default::default()
                }),
            ),
        ],
    )
    .unwrap();
    let page = [
        topic_created(
            2,
            G1,
            Topic {
                context_of: Some(side(5)),
                ..topic("S1", G1, "SM1", 2)
            },
        ),
        msg_created(3, card_message("C1", 3, ask_card(1, true))),
    ];
    let entries: Vec<LogEntry> = page.into_iter().map(LogEntry::Known).collect();
    s.apply_recovered(G1, &entries, 3, true).unwrap();
    assert_eq!(s.message("C1").unwrap().unwrap().card_v2.unwrap().rev, 2);
    let t = s.topic("S1").unwrap().unwrap();
    assert_eq!(t.topic.context_of.unwrap().upto_seq, 40);
}

async fn until(what: &str, mut f: impl FnMut() -> bool) {
    let t0 = Instant::now();
    while !f() {
        assert!(t0.elapsed() < WAIT, "timed out waiting for {what}");
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn recovery_refetches_the_log_once_and_restores_what_1_0_dropped() {
    let server = MockServer::start(
        member(ME, "me"),
        vec![member(ME, "me"), member(ALICE, "alice")],
    )
    .await;
    server.add_group(group(G1, &[ME, ALICE]));
    let card = ask_card(1, true);
    server.with(|s| {
        s.append(
            G1,
            ME,
            EventData::TopicCreated(TopicCreatedData {
                topic: topic("T1", G1, "M1", 1),
            }),
        );
        s.append(
            G1,
            ME,
            EventData::MessageCreated(Box::new(MessageCreatedData {
                message: message("M1", G1, "T1", ME, 2, "root"),
            })),
        );
        s.append(
            G1,
            ALICE,
            EventData::MessageCreated(Box::new(MessageCreatedData {
                message: card_message("C1", 3, card.clone()),
            })),
        );
        s.append(G1, ME, answered(4, ME, "C1", 1).data);
    });

    // The cache as 1.0.1 left it: the log applied to seq 4, the card dropped, the answer skipped.
    let dir = tempfile::tempdir().unwrap();
    {
        let store = Store::open(&dir.path().join("cache.sqlite3")).unwrap();
        store
            .apply_welcome(
                &member(ME, "me"),
                &[member(ME, "me"), member(ALICE, "alice")],
                &[summary(group(G1, &[ME, ALICE]), 4, 4, 0)],
            )
            .unwrap();
        store
            .apply_events(
                G1,
                &[
                    topic_created(1, G1, topic("T1", G1, "M1", 1)),
                    msg_created(2, message("M1", G1, "T1", ME, 2, "root")),
                    msg_created(
                        3,
                        Message {
                            kind: Some(MessageKind::Card),
                            ..message("C1", G1, "T1", ALICE, 3, "请选择")
                        },
                    ),
                ],
            )
            .unwrap();
        store
            .with_writer(|tx| {
                tx.execute(
                    "UPDATE sync_state SET last_applied = 4 WHERE group_id = ?1",
                    [G1],
                )?;
                tx.execute(
                    "INSERT INTO log_refetch (group_id, upto) VALUES (?1, 4)",
                    [G1],
                )?;
                Ok(())
            })
            .unwrap();
    }
    let mut cfg = ClientConfig::new(server.base_url(), TOKEN, dir.path());
    cfg.fetch_limit = 2;
    cfg.request_timeout = Duration::from_secs(5);
    let client = Client::new(cfg).unwrap();
    let _updates = client.subscribe();
    client.start();
    until("recovery done", || {
        client.store().refetches().unwrap().is_empty()
    })
    .await;
    assert_eq!(
        client.store().message("C1").unwrap().unwrap().card_v2,
        Some(card)
    );
    assert_eq!(client.card_answers("C1").unwrap().len(), 1);
    assert_eq!(client.decisions(Some(G1), 10).unwrap().len(), 1);
    // Counts did not change: the root and the card are still 2 messages.
    let t = client.store().topic("T1").unwrap().unwrap();
    assert_eq!(t.topic.message_count, 2);
    // Pages of 2 from seq 0, and no recovery fetch on the next connection.
    let afters: Vec<u64> = server.with(|s| {
        s.frames_of("fetch")
            .iter()
            .map(|f| f["afterSeq"].as_u64().unwrap())
            .collect()
    });
    assert_eq!(afters, vec![0, 2]);

    // Queuing an answer on a v2 card makes a card_answer outbox row with the rev.
    let id = client.answer_card("C1", 1, "ok", None, None).unwrap();
    let row = client.store().outbox_get(&id).unwrap().unwrap();
    assert!(matches!(row.op, OutboxOp::CardAnswer { rev: 1, .. }));
    let _ = ClientUpdate::CaughtUp;
}

fn report_card() -> CardV2 {
    serde_json::from_value(json!({
        "v": 2, "catalog": "tc.card/2.1", "rev": 1, "intent": "done", "title": "月报", "fallback": "月报",
        "ask": { "id": "verify", "from": [ME], "blocking": false },
        "blocks": [
            { "id": "check", "type": "decision", "options": [{ "id": "ok", "label": "对" }, { "id": "wrong", "label": "有问题" }], "events": ["choice.change"] },
            { "id": "trend", "type": "chart", "kind": "line", "series": [{ "id": "oct", "name": "10 月", "values": [1, 2] }], "events": ["chart.select"] }
        ],
        "actions": [{ "id": "send", "label": "发送", "style": "primary", "kind": "submit", "inputs": ["check"] }]
    }))
    .unwrap()
}

#[test]
fn card_state_cause_refs_retracts_and_previews_reach_the_cache() {
    let (_d, s) = open();
    let card = report_card();
    let cause = Cause {
        event_id: "EV1".into(),
        actor_id: ME.into(),
        at: 5,
        event: "choice.change".into(),
        block_id: Some("check".into()),
        action_id: None,
    };
    let state = CardState {
        ask: Some(CardStateAsk {
            id: "verify".into(),
            status: AskStatus::Open,
            from: vec![ME.into()],
            blocking: false,
            ..Default::default()
        }),
        answers: vec![],
        last_cause: Some(cause.clone()),
        ..Default::default()
    };
    let refs = vec![BlockRef {
        source: BlockRefInput {
            message_id: Some("C1".into()),
            block_id: "check".into(),
            label: String::new(),
            ..Default::default()
        },
        source_topic_id: "T1".into(),
        deleted: Some(True),
        ..Default::default()
    }];
    let version = ArtifactVersion {
        preview: None,
        artifact_id: "A1".into(),
        version: 1,
        entry: "index.html".into(),
        files: vec![ArtifactFile {
            path: "index.html".into(),
            file_id: "F1".into(),
            size: 3,
            mime: "text/html".into(),
        }],
        note: None,
        created_by: BOT.into(),
        created_at: 1,
        run_id: None,
        events: None,
        media: None,
        data_rev: None,
        parent_version: None,
    };
    let artifact = Artifact {
        id: "A1".into(),
        group_id: G1.into(),
        topic_id: "T1".into(),
        title: "看板".into(),
        kind: ArtifactKind::Html,
        current_version: 1,
        created_by: BOT.into(),
        created_at: 1,
        updated_at: 1,
    };
    let media = MediaInfo {
        kind: MediaKind::Html,
        mime: "text/html".into(),
        bytes: 3,
        poster_file_id: Some("F2".into()),
        preview: MediaPreview::Ready,
        ..Default::default()
    };
    s.apply_events(
        G1,
        &[
            topic_created(1, G1, topic("T1", G1, "M1", 1)),
            msg_created(2, message("M1", G1, "T1", ME, 2, "root")),
            msg_created(3, card_message("C1", 3, card.clone())),
            ev(
                4,
                G1,
                BOT,
                EventData::MessageUpdated(MessageUpdatedData {
                    message_id: "C1".into(),
                    body: "月报".into(),
                    edited_at: 4,
                    streaming: false,
                    card_v2: Some(CardV2 { rev: 2, ..card }),
                    card_state: Some(state.clone()),
                    cause: Some(cause.clone()),
                    ..Default::default()
                }),
            ),
            ev(
                5,
                G1,
                ME,
                EventData::MessageUpdated(MessageUpdatedData {
                    message_id: "M1".into(),
                    body: "> ↳ 原消息已删除\n\nroot".into(),
                    edited_at: 5,
                    streaming: false,
                    block_refs: Some(refs.clone()),
                    ..Default::default()
                }),
            ),
            ev(
                6,
                G1,
                BOT,
                EventData::ArtifactCreated(ArtifactVersionResponse { artifact, version }),
            ),
            ev(
                7,
                G1,
                BOT,
                EventData::ArtifactPreviewReady(ArtifactPreviewReadyData {
                    artifact_id: "A1".into(),
                    version: 1,
                    media: media.clone(),
                }),
            ),
        ],
    )
    .unwrap();
    let c1 = s.message("C1").unwrap().unwrap();
    assert_eq!(c1.card_v2.unwrap().rev, 2);
    assert_eq!(c1.card_state, Some(state));
    assert_eq!(c1.cause, Some(cause));
    assert_eq!(s.message("M1").unwrap().unwrap().block_refs, Some(refs));
    let v = s.artifact_version("A1", 1).unwrap().unwrap();
    assert_eq!(v.media, Some(media));
}

async fn next<T>(
    rx: &mut tokio::sync::mpsc::UnboundedReceiver<ClientUpdate>,
    f: impl Fn(&ClientUpdate) -> Option<T>,
) -> T {
    {
        let t0 = Instant::now();
        loop {
            assert!(t0.elapsed() < WAIT, "timed out waiting for an update");
            match tokio::time::timeout(Duration::from_millis(100), rx.recv()).await {
                Ok(Some(u)) => {
                    if let Some(x) = f(&u) {
                        return x;
                    }
                }
                Ok(None) => panic!("updates closed"),
                Err(_) => {}
            }
        }
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn card_events_page_events_and_interact_transients() {
    let server = MockServer::start(
        member(ME, "me"),
        vec![
            member(ME, "me"),
            member(ALICE, "alice"),
            member(BOT, "ledger"),
        ],
    )
    .await;
    server.add_group(group(G1, &[ME, ALICE, BOT]));
    server.with(|s| {
        s.append(
            G1,
            ME,
            EventData::TopicCreated(TopicCreatedData {
                topic: topic("T1", G1, "M1", 1),
            }),
        );
        s.append(
            G1,
            ME,
            EventData::MessageCreated(Box::new(MessageCreatedData {
                message: message("M1", G1, "T1", ME, 2, "root"),
            })),
        );
        s.append(
            G1,
            BOT,
            EventData::MessageCreated(Box::new(MessageCreatedData {
                message: card_message("C1", 3, report_card()),
            })),
        );
    });
    let dir = tempfile::tempdir().unwrap();
    let client = Client::new(ClientConfig::new(server.base_url(), TOKEN, dir.path())).unwrap();

    // Not connected: a live event fails at once and is not queued.
    let mut rx = client.subscribe();
    client
        .store()
        .apply_welcome(
            &member(ME, "me"),
            &[member(ME, "me")],
            &[summary(group(G1, &[ME, ALICE, BOT]), 0, 0, 0)],
        )
        .unwrap();
    client
        .store()
        .apply_events(
            G1,
            &[
                topic_created(1, G1, topic("T1", G1, "M1", 1)),
                msg_created(2, message("M1", G1, "T1", ME, 2, "root")),
                msg_created(3, card_message("C1", 3, report_card())),
            ],
        )
        .unwrap();
    client.start();
    until("connected", || client.server_has(FEATURE_INTERACT)).await;
    let hello = server.with(|s| s.hellos[0].clone());
    assert_eq!(
        hello["features"],
        json!(["interact/1", "refs/1", "settings/1", "github/1"])
    );
    assert_eq!(
        hello["supportedCatalogs"],
        json!(["tc.card/2.0", "tc.card/2.1"])
    );

    // A live event: sent now, acked with eventId and expect.
    let live = client
        .send_card_event(chat_core::CardEventRequest {
            message_id: "C1".into(),
            rev: 1,
            block_id: Some("trend".into()),
            event: "chart.select".into(),
            value: Some(json!({"series": "oct", "x": "10-06", "y": 1, "index": 0})),
            part: Some(PartRef {
                kind: "point".into(),
                keys: Some(
                    [
                        ("series".to_string(), PartKey::Key("oct".into())),
                        ("x".to_string(), PartKey::Key("10-06".into())),
                    ]
                    .into(),
                ),
            }),
            ..Default::default()
        })
        .unwrap();
    assert!(!live.durable);
    let ack = next(&mut rx, |u| match u {
        ClientUpdate::CardEventAcked {
            client_event_id,
            result,
            ..
        } if *client_event_id == live.client_event_id => Some(result.clone()),
        _ => None,
    })
    .await
    .unwrap();
    assert!(!ack.stored);
    assert_eq!(ack.expect.mode, ExpectMode::Live);
    let sent = server.with(|s| s.frames_of("card.event"));
    assert_eq!(sent[0]["part"]["kind"], "point");

    // A submit is an answer: queued, sent with its clientEventId, acked as stored.
    let answer = client
        .send_card_event(chat_core::CardEventRequest {
            message_id: "C1".into(),
            rev: 1,
            action_id: Some("send".into()),
            event: "action.press".into(),
            inputs: Some([("check".to_string(), json!({"value": "wrong"}))].into()),
            ..Default::default()
        })
        .unwrap();
    assert!(answer.durable);
    let ack = next(&mut rx, |u| match u {
        ClientUpdate::CardEventAcked {
            client_event_id,
            result,
            ..
        } if *client_event_id == answer.client_event_id => Some(result.clone()),
        _ => None,
    })
    .await
    .unwrap();
    assert_eq!(ack.answer_id.as_deref(), Some("AN-1"));
    until("answer row gone", || {
        client.pending_card_events("T1").unwrap().is_empty()
    })
    .await;

    // A page event (live) and the interact transients.
    client
        .store()
        .apply_events(
            G1,
            &[ev(
                4,
                G1,
                BOT,
                EventData::ArtifactCreated(ArtifactVersionResponse {
                    artifact: Artifact {
                        id: "A1".into(),
                        group_id: G1.into(),
                        topic_id: "T1".into(),
                        title: "看板".into(),
                        kind: ArtifactKind::Html,
                        current_version: 1,
                        created_by: BOT.into(),
                        created_at: 1,
                        updated_at: 1,
                    },
                    version: ArtifactVersion {
                        preview: None,
                        artifact_id: "A1".into(),
                        version: 1,
                        entry: "index.html".into(),
                        files: vec![],
                        note: None,
                        created_by: BOT.into(),
                        created_at: 1,
                        run_id: None,
                        events: None,
                        media: None,
                        data_rev: None,
                        parent_version: None,
                    },
                }),
            )],
        )
        .unwrap();
    let page = client
        .send_artifact_event(
            "A1",
            1,
            "breakdown",
            ArtifactEventKind::Ask,
            Some(json!({"channel": "douyin"})),
        )
        .unwrap();
    let ack = next(&mut rx, |u| match u {
        ClientUpdate::ArtifactEventAcked {
            client_event_id,
            result,
            ..
        } if *client_event_id == page => Some(result.clone()),
        _ => None,
    })
    .await
    .unwrap();
    assert_eq!(ack.event_id, format!("EV-{page}"));
    server.with(|s| {
        s.transient(json!({"type": "interaction.resolved", "groupId": G1, "eventId": "EV1", "result": "patched", "messageId": "C1"}));
        s.transient(json!({"type": "artifact.data", "groupId": G1, "artifactId": "A1", "version": 1, "dataRev": 2, "data": {"x": 1}}));
        s.transient(json!({"type": "artifact.reply", "groupId": G1, "eventId": ack.event_id, "ok": true, "payload": {"series": [1]}}));
    });
    let r = next(&mut rx, |u| match u {
        ClientUpdate::InteractionResolved(r) => Some(r.clone()),
        _ => None,
    })
    .await;
    assert_eq!(r.result, InteractionResult::Patched);
    let d = next(&mut rx, |u| match u {
        ClientUpdate::ArtifactData(d) => Some(d.clone()),
        _ => None,
    })
    .await;
    assert_eq!((d.data_rev, d.data.clone()), (2, Some(json!({"x": 1}))));
    let reply = next(&mut rx, |u| match u {
        ClientUpdate::ArtifactReply(r) => Some(r.clone()),
        _ => None,
    })
    .await;
    assert!(reply.ok);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_live_card_event_fails_at_once_when_not_connected() {
    let dir = tempfile::tempdir().unwrap();
    let client = Client::new(ClientConfig::new("http://127.0.0.1:9", TOKEN, dir.path())).unwrap();
    client
        .store()
        .apply_welcome(
            &member(ME, "me"),
            &[member(ME, "me")],
            &[summary(group(G1, &[ME, BOT]), 0, 0, 0)],
        )
        .unwrap();
    client
        .store()
        .apply_events(
            G1,
            &[
                topic_created(1, G1, topic("T1", G1, "M1", 1)),
                msg_created(2, message("M1", G1, "T1", ME, 2, "root")),
                msg_created(3, card_message("C1", 3, report_card())),
            ],
        )
        .unwrap();
    let mut rx = client.subscribe();
    client.start();
    let sent = client
        .send_card_event(chat_core::CardEventRequest {
            message_id: "C1".into(),
            rev: 1,
            block_id: Some("check".into()),
            event: "choice.change".into(),
            value: Some(json!({"value": "ok"})),
            ..Default::default()
        })
        .unwrap();
    assert!(!sent.durable);
    let err = next(&mut rx, |u| match u {
        ClientUpdate::CardEventAcked {
            client_event_id,
            result,
            ..
        } if *client_event_id == sent.client_event_id => Some(result.clone()),
        _ => None,
    })
    .await
    .unwrap_err();
    assert_eq!(err.reason.as_deref(), Some("client.offline"));
    assert!(client.store().outbox_all().unwrap().is_empty());
    client.stop();
}

#[test]
fn an_immediate_answer_keeps_its_event_block_and_value() {
    let (_d, s) = open();
    s.apply_events(
        G1,
        &[
            topic_created(1, G1, topic("T1", G1, "M1", 1)),
            msg_created(2, message("M1", G1, "T1", ME, 2, "root")),
            msg_created(3, card_message("C1", 3, ask_card(1, true))),
            ev(
                4,
                G1,
                ME,
                EventData::CardAnswered(CardAnsweredData {
                    message_id: "C1".into(),
                    rev: 1,
                    answer_id: Some("AN4".into()),
                    ask_id: Some("a1".into()),
                    event: Some("choice.submit".into()),
                    block_id: Some("when".into()),
                    value: Some(json!({ "value": "later" })),
                    ..Default::default()
                }),
            ),
        ],
    )
    .unwrap();
    let a = &s.card_answers("C1").unwrap()[0];
    assert_eq!(a.action_id, "");
    assert_eq!(a.event.as_deref(), Some("choice.submit"));
    assert_eq!(a.block_id.as_deref(), Some("when"));
    assert_eq!(a.value, Some(json!({ "value": "later" })));
    // 群里的决定 can show what was chosen from the cache.
    let d = s.decisions(Some(G1), 10).unwrap();
    assert_eq!(d[0].answer.value, Some(json!({ "value": "later" })));
}

fn ask21(blocks: serde_json::Value, ask: bool) -> CardV2 {
    let mut card = json!({
        "v": 2, "catalog": "tc.card/2.1", "rev": 1, "intent": "ask", "title": "选", "fallback": "选",
        "blocks": blocks
    });
    if ask {
        card["ask"] = json!({ "id": "a1", "from": [ME] });
    }
    serde_json::from_value(card).unwrap()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn stored_answers_are_queued_while_offline() {
    let dir = tempfile::tempdir().unwrap();
    let client = Client::new(ClientConfig::new("http://127.0.0.1:9", TOKEN, dir.path())).unwrap();
    client
        .store()
        .apply_welcome(
            &member(ME, "me"),
            &[member(ME, "me")],
            &[summary(group(G1, &[ME, BOT]), 0, 0, 0)],
        )
        .unwrap();
    let opts = json!([{ "id": "a", "label": "A" }, { "id": "b", "label": "B" }]);
    let questionnaire = json!([
        { "id": "qs", "type": "questionnaire", "children": ["q1", "q2"], "fallbackText": "问卷" },
        { "id": "q1", "type": "input", "kind": "text", "label": "一" },
        { "id": "q2", "type": "input", "kind": "text", "label": "二" }
    ]);
    let compare =
        json!([{ "id": "cmp", "type": "compare", "options": opts, "fallbackText": "A / B" }]);
    let poll = json!([{ "id": "vote", "type": "poll", "label": "投", "voters": "group", "closesAt": 9_999, "options": opts, "fallbackText": "投" }]);
    let confirm = json!([{ "id": "go", "type": "confirm", "label": "发布？", "yes": "发", "no": "不发", "fallbackText": "发布？" }]);
    client
        .store()
        .apply_events(
            G1,
            &[
                topic_created(1, G1, topic("T1", G1, "M1", 1)),
                msg_created(2, message("M1", G1, "T1", ME, 2, "root")),
                msg_created(3, card_message("Q", 3, ask21(questionnaire, true))),
                msg_created(4, card_message("CMP", 4, ask21(compare, true))),
                msg_created(5, card_message("P", 5, ask21(poll, true))),
                msg_created(6, card_message("CF", 6, ask21(confirm.clone(), true))),
                msg_created(7, card_message("NOASK", 7, ask21(confirm, false))),
            ],
        )
        .unwrap();
    client.start();
    let send = |message_id: &str,
                block_id: &str,
                event: &str,
                value: Option<serde_json::Value>,
                inputs: Option<AnswerInputs>| {
        client
            .send_card_event(chat_core::CardEventRequest {
                message_id: message_id.into(),
                rev: 1,
                block_id: Some(block_id.into()),
                event: event.into(),
                value,
                inputs,
                ..Default::default()
            })
            .unwrap()
    };
    let sent = [
        send(
            "Q",
            "qs",
            "action.press",
            None,
            Some([("q1".to_string(), json!({"value": "x"}))].into()),
        ),
        send(
            "CMP",
            "cmp",
            "compare.pick",
            Some(json!({"value": "a"})),
            None,
        ),
        send(
            "P",
            "vote",
            "choice.submit",
            Some(json!({"value": "b"})),
            None,
        ),
        send(
            "CF",
            "go",
            "choice.submit",
            Some(json!({"value": "yes"})),
            None,
        ),
    ];
    // Each one is an answer the server stores: queued with its clientEventId for the resend.
    for s in &sent {
        assert!(s.durable, "{s:?}");
        let row = client
            .store()
            .outbox_get(&s.client_event_id)
            .unwrap()
            .unwrap();
        let OutboxOp::CardEvent {
            client_event_id, ..
        } = row.op
        else {
            panic!("not a card event row")
        };
        assert_eq!(client_event_id, s.client_event_id);
    }
    assert_eq!(client.pending_card_events("T1").unwrap().len(), 4);
    // Without an ask the same commit is a live event.
    assert!(
        !send(
            "NOASK",
            "go",
            "choice.submit",
            Some(json!({"value": "yes"})),
            None
        )
        .durable
    );
    client.stop();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_answer_is_acked_when_its_event_arrives_first() {
    let server = MockServer::start(
        member(ME, "me"),
        vec![member(ME, "me"), member(BOT, "ledger")],
    )
    .await;
    server.add_group(group(G1, &[ME, BOT]));
    server.with(|s| {
        s.answered_before_ack = true;
        s.append(
            G1,
            ME,
            EventData::TopicCreated(TopicCreatedData {
                topic: topic("T1", G1, "M1", 1),
            }),
        );
        s.append(
            G1,
            ME,
            EventData::MessageCreated(Box::new(MessageCreatedData {
                message: message("M1", G1, "T1", ME, 2, "root"),
            })),
        );
        s.append(
            G1,
            BOT,
            EventData::MessageCreated(Box::new(MessageCreatedData {
                message: card_message("C1", 3, report_card()),
            })),
        );
    });
    let dir = tempfile::tempdir().unwrap();
    let client = Client::new(ClientConfig::new(server.base_url(), TOKEN, dir.path())).unwrap();
    let mut rx = client.subscribe();
    client.start();
    until("card cached", || {
        client.store().message("C1").unwrap().is_some() && client.server_has(FEATURE_INTERACT)
    })
    .await;
    let answer = client
        .send_card_event(chat_core::CardEventRequest {
            message_id: "C1".into(),
            rev: 1,
            action_id: Some("send".into()),
            event: "action.press".into(),
            inputs: Some([("check".to_string(), json!({"value": "ok"}))].into()),
            ..Default::default()
        })
        .unwrap();
    assert!(answer.durable);
    // card.answered removes the sent row before the ack comes; the ack still reaches me.
    let ack = next(&mut rx, |u| match u {
        ClientUpdate::CardEventAcked {
            client_event_id,
            result,
            ..
        } if *client_event_id == answer.client_event_id => Some(result.clone()),
        _ => None,
    })
    .await
    .unwrap();
    assert_eq!(ack.answer_id.as_deref(), Some("AN-1"));
    assert_eq!(ack.expect.mode, ExpectMode::Live);
    assert_eq!(
        client.card_answers("C1").unwrap()[0].answer_id.as_deref(),
        Some("AN-1")
    );
    assert!(client.pending_card_events("T1").unwrap().is_empty());
    client.stop();
}

#[test]
fn state_updates_of_a_card_and_cleared_refs_are_not_edits() {
    let (_d, s) = open();
    let card = ask_card(1, false);
    let state: CardState = serde_json::from_value(json!({
        "ask": { "id": "a1", "status": "answered", "from": [ME], "blocking": true, "answeredBy": ME, "answeredAt": 1004 },
        "answers": []
    }))
    .unwrap();
    s.apply_events(
        G1,
        &[
            topic_created(1, G1, topic("T1", G1, "M1", 1)),
            msg_created(2, message("M1", G1, "T1", ME, 2, "> ↳ 表格\n\nroot")),
            msg_created(3, card_message("C1", 3, card.clone())),
            // The server's state update of an answer: editedAt is the creation time.
            ev(
                4,
                G1,
                ME,
                EventData::MessageUpdated(MessageUpdatedData {
                    message_id: "C1".into(),
                    body: "请选择".into(),
                    edited_at: 1003,
                    streaming: false,
                    card_state: Some(state.clone()),
                    ..Default::default()
                }),
            ),
            // A cleared ref of a deleted source.
            ev(
                5,
                G1,
                ME,
                EventData::MessageUpdated(MessageUpdatedData {
                    message_id: "M1".into(),
                    body: "> ↳ 原消息已删除\n\nroot".into(),
                    edited_at: 1002,
                    streaming: false,
                    block_refs: Some(vec![]),
                    ..Default::default()
                }),
            ),
        ],
    )
    .unwrap();
    assert_eq!(s.message("C1").unwrap().unwrap().edited_at(), None);
    assert_eq!(s.message("M1").unwrap().unwrap().edited_at(), None);
    // A new rev is an edit.
    s.apply_events(
        G1,
        &[ev(
            6,
            G1,
            ALICE,
            EventData::MessageUpdated(MessageUpdatedData {
                message_id: "C1".into(),
                body: "请选择".into(),
                edited_at: 2000,
                streaming: false,
                card_v2: Some(ask_card(2, false)),
                card_state: Some(state),
                ..Default::default()
            }),
        )],
    )
    .unwrap();
    assert_eq!(s.message("C1").unwrap().unwrap().edited_at(), Some(2000));
}

#[test]
fn every_ask_message_is_found_newest_first_without_deleted_ones() {
    let (_d, s) = open();
    let mut events = vec![
        topic_created(1, G1, topic("T1", G1, "M1", 1)),
        msg_created(2, message("M1", G1, "T1", ME, 2, "root")),
        msg_created(3, card_message("C1", 3, ask_card(1, false))),
    ];
    // Many plain messages after the ask: it is not in the newest 30 of its topic.
    for i in 0..40u64 {
        events.push(msg_created(
            4 + i,
            message(&format!("P{i}"), G1, "T1", ALICE, 4 + i, "plain"),
        ));
    }
    events.push(msg_created(44, card_message("C2", 44, ask_card(1, true))));
    events.push(ev(
        45,
        G1,
        ALICE,
        EventData::MessageDeleted(MessageDeletedData {
            message_id: "C2".into(),
            deleted_at: 50,
        }),
    ));
    events.push(msg_created(46, card_message("C3", 46, ask_card(1, false))));
    s.apply_events(G1, &events).unwrap();
    let ids: Vec<String> = s
        .ask_messages(10)
        .unwrap()
        .into_iter()
        .map(|m| m.id)
        .collect();
    assert_eq!(ids, ["C3", "C1"]);
    assert_eq!(s.ask_messages(1).unwrap().len(), 1);
}
