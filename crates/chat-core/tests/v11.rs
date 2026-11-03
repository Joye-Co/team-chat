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
        }),
    )
}

fn run(id: &str, status: RunStatus, created: u64) -> BotRun {
    BotRun {
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
                    EventData::ArtifactEvented(ArtifactEventedData {
                        artifact_id: "A1".into(),
                        version: 2,
                        name: "pick".into(),
                        payload: Some(json!({"option": "B"})),
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

    let events = s.artifact_events("A1").unwrap();
    assert_eq!((events[0].name.as_str(), events[0].version), ("pick", 2));

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
            EventData::MessageCreated(MessageCreatedData {
                message: message("M1", G1, "T1", ME, 2, "root"),
            }),
        );
        s.append(
            G1,
            ALICE,
            EventData::MessageCreated(MessageCreatedData {
                message: card_message("C1", 3, card.clone()),
            }),
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
