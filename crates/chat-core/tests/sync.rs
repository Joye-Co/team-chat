//! Sync engine against the in-process mock server.

mod common;
mod mock;

use chat_core::proto::*;
use chat_core::{Client, ClientConfig, ClientUpdate, ConnectionState, OutboxState};
use common::*;
use mock::{MockServer, TOKEN};
use serde_json::json;
use std::time::{Duration, Instant};
use tokio::sync::mpsc::UnboundedReceiver;

const WAIT: Duration = Duration::from_secs(10);

struct Harness {
    server: MockServer,
    _dir: tempfile::TempDir,
    client: Client,
    updates: UnboundedReceiver<ClientUpdate>,
}

fn config(server: &MockServer, dir: &std::path::Path) -> ClientConfig {
    let mut c = ClientConfig::new(server.base_url(), TOKEN, dir);
    c.backoff_min = Duration::from_millis(50);
    c.backoff_max = Duration::from_millis(400);
    c.fetch_limit = 100;
    c.request_timeout = Duration::from_secs(5);
    c
}

async fn server_with_group() -> MockServer {
    let server = MockServer::start(
        member(ME, "me"),
        vec![member(ME, "me"), member(ALICE, "alice")],
    )
    .await;
    server.add_group(group(G1, &[ME, ALICE]));
    server
}

fn harness_with(server: MockServer, tweak: impl FnOnce(&mut ClientConfig)) -> Harness {
    let dir = tempfile::tempdir().unwrap();
    let mut cfg = config(&server, dir.path());
    tweak(&mut cfg);
    let client = Client::new(cfg).unwrap();
    let updates = client.subscribe();
    client.start();
    Harness {
        server,
        _dir: dir,
        client,
        updates,
    }
}

/// Polls a condition (test code only; the engine itself never polls).
async fn until(what: &str, mut f: impl FnMut() -> bool) {
    let t0 = Instant::now();
    while !f() {
        assert!(t0.elapsed() < WAIT, "timed out waiting for {what}");
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}

async fn next_update(
    rx: &mut UnboundedReceiver<ClientUpdate>,
    what: &str,
    mut pred: impl FnMut(&ClientUpdate) -> bool,
) -> ClientUpdate {
    let t0 = Instant::now();
    loop {
        let left = WAIT.saturating_sub(t0.elapsed());
        match tokio::time::timeout(left, rx.recv()).await {
            Ok(Some(u)) if pred(&u) => return u,
            Ok(Some(_)) => {}
            _ => panic!("timed out waiting for update: {what}"),
        }
    }
}

fn topic_of(h: &Harness) -> String {
    h.client.topics(G1, 10).unwrap()[0].topic.id.clone()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn catch_up_fetches_every_page_then_resumes_from_cursor() {
    let server = server_with_group().await;
    let tid = server.with(|s| {
        let (_, t) = s.post(G1, ALICE, None, None, "c0", "root", false);
        let tid = t.unwrap().id;
        for i in 1..=250 {
            s.post(
                G1,
                ALICE,
                Some(&tid),
                None,
                &format!("c{i}"),
                &format!("m{i}"),
                false,
            );
        }
        tid
    });
    let mut h = harness_with(server, |_| {});
    next_update(&mut h.updates, "caught up", |u| {
        matches!(u, ClientUpdate::CaughtUp)
    })
    .await;
    assert_eq!(h.client.store().last_applied(G1).unwrap(), 252);
    let fetches = h.server.with(|s| s.frames_of("fetch"));
    let afters: Vec<u64> = fetches
        .iter()
        .map(|f| f["afterSeq"].as_u64().unwrap())
        .collect();
    assert_eq!(afters, vec![0, 100, 200]);
    let window = h.client.messages(&tid, None, 200).unwrap();
    assert_eq!(window.len(), 200);
    assert_eq!(window.last().unwrap().body, "m250");
    let g = &h.client.groups().unwrap()[0];
    assert_eq!(g.unread, 251);

    // A reconnect sends the cursor in hello and fetches nothing.
    h.server.with(|s| s.close_all());
    until("second hello", || h.server.with(|s| s.hellos.len() == 2)).await;
    let hello = h.server.with(|s| s.hellos[1].clone());
    assert_eq!(hello["cursors"][G1], json!(252));
    next_update(&mut h.updates, "caught up again", |u| {
        matches!(u, ClientUpdate::CaughtUp)
    })
    .await;
    assert_eq!(h.server.with(|s| s.frames_of("fetch").len()), 3);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn live_events_apply_and_gaps_are_fetched() {
    let server = server_with_group().await;
    let mut h = harness_with(server, |_| {});
    next_update(&mut h.updates, "caught up", |u| {
        matches!(u, ClientUpdate::CaughtUp)
    })
    .await;
    let tid = h.server.with(|s| {
        s.post(G1, ALICE, None, None, "a0", "root", false)
            .1
            .unwrap()
            .id
    });
    until("topic", || h.client.topics(G1, 10).unwrap().len() == 1).await;
    // Seq 4 is never broadcast live: seq 5 reveals the gap.
    h.server.with(|s| {
        s.hide_live.insert(4);
        s.post(G1, ALICE, Some(&tid), None, "a1", "one", false);
        s.post(G1, ALICE, Some(&tid), None, "a2", "two", false);
        s.post(G1, ALICE, Some(&tid), None, "a3", "three", false);
    });
    until("gap closed", || {
        h.client.store().last_applied(G1).unwrap() == 5
    })
    .await;
    let bodies: Vec<String> = h
        .client
        .messages(&tid, None, 10)
        .unwrap()
        .into_iter()
        .map(|m| m.body)
        .collect();
    assert_eq!(bodies, vec!["root", "one", "two", "three"]);
    let gap_fetch = h
        .server
        .with(|s| s.frames_of("fetch").last().cloned().unwrap());
    assert_eq!(gap_fetch["afterSeq"], json!(3));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn unacked_send_is_resent_with_same_client_msg_id_and_deduped() {
    let server = server_with_group().await;
    let tid = server.with(|s| {
        s.post(G1, ALICE, None, None, "a0", "root", false)
            .1
            .unwrap()
            .id
    });
    let mut h = harness_with(server, |_| {});
    next_update(&mut h.updates, "caught up", |u| {
        matches!(u, ClientUpdate::CaughtUp)
    })
    .await;
    // The server stores the message but the connection drops before the ack.
    h.server.with(|s| {
        s.drop_acks = 1;
        s.hide_live.insert(3);
    });
    let cmid = h
        .client
        .send_message(G1, &tid, "hello once", vec![])
        .unwrap();
    assert_eq!(h.client.pending_sends(&tid).unwrap().len(), 1);
    next_update(&mut h.updates, "sent", |u| {
        matches!(u, ClientUpdate::Sent { .. })
    })
    .await;
    let sends = h.server.with(|s| s.frames_of("send"));
    assert_eq!(sends.len(), 2, "resent after reconnect");
    assert!(sends.iter().all(|f| f["clientMsgId"] == json!(cmid)));
    assert_eq!(h.server.with(|s| s.message_count(G1)), 2);
    until("outbox empty", || {
        h.client.store().outbox_all().unwrap().is_empty()
    })
    .await;
    let msgs = h.client.messages(&tid, None, 10).unwrap();
    assert_eq!(msgs.iter().filter(|m| m.client_msg_id == cmid).count(), 1);
    assert_eq!(msgs.len(), 2);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn duplicate_ack_is_handled() {
    let server = server_with_group().await;
    let tid = server.with(|s| {
        s.post(G1, ALICE, None, None, "a0", "root", false)
            .1
            .unwrap()
            .id
    });
    let mut h = harness_with(server, |_| {});
    next_update(&mut h.updates, "caught up", |u| {
        matches!(u, ClientUpdate::CaughtUp)
    })
    .await;
    // The message already exists on the server under this clientMsgId.
    let cmid = chat_core::new_id();
    h.server
        .with(|s| s.post(G1, ME, Some(&tid), None, &cmid, "already there", false));
    until("event applied", || {
        h.client.messages(&tid, None, 10).unwrap().len() == 2
    })
    .await;
    h.client
        .store()
        .outbox_push(&chat_core::OutboxItem {
            id: cmid.clone(),
            op: chat_core::OutboxOp::Send {
                group_id: G1.into(),
                topic_id: Some(tid.clone()),
                title: None,
                body: "already there".into(),
                mentions: vec![],
                streaming: false,
                reply_to: None,
                attachments: None,
                artifact_refs: None,
                card: None,
            },
            attempts: 0,
            last_error: None,
            state: OutboxState::Pending,
            created_at: 0,
        })
        .unwrap();
    h.client.reconnect_now();
    let u = next_update(&mut h.updates, "duplicate sent", |u| {
        matches!(u, ClientUpdate::Sent { .. })
    })
    .await;
    assert!(
        matches!(
            u,
            ClientUpdate::Sent {
                duplicate: true,
                ..
            }
        ),
        "{u:?}"
    );
    assert!(h.client.store().outbox_all().unwrap().is_empty());
    assert_eq!(h.client.messages(&tid, None, 10).unwrap().len(), 2);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn fifty_offline_sends_arrive_in_order_without_duplicates() {
    let server = server_with_group().await;
    let tid = server.with(|s| {
        s.post(G1, ALICE, None, None, "a0", "root", false)
            .1
            .unwrap()
            .id
    });
    let mut h = harness_with(server, |_| {});
    next_update(&mut h.updates, "caught up", |u| {
        matches!(u, ClientUpdate::CaughtUp)
    })
    .await;
    h.server.with(|s| {
        s.refuse = true;
        s.close_all();
    });
    next_update(&mut h.updates, "disconnected", |u| {
        matches!(
            u,
            ClientUpdate::Connection(ConnectionState::Disconnected { .. })
        )
    })
    .await;
    for i in 0..50 {
        h.client
            .send_message(G1, &tid, &format!("offline {i}"), vec![])
            .unwrap();
    }
    assert_eq!(h.client.pending_sends(&tid).unwrap().len(), 50);
    h.server.with(|s| s.refuse = false);
    h.client.reconnect_now();
    until("all sent", || {
        h.client.store().outbox_all().unwrap().is_empty()
    })
    .await;
    let server_bodies: Vec<String> = h.server.with(|s| {
        let mut v: Vec<&Message> = s.groups[G1]
            .messages
            .values()
            .filter(|m| m.author_id == ME)
            .collect();
        v.sort_by_key(|m| m.created_seq);
        v.iter().map(|m| m.body.clone()).collect()
    });
    let expected: Vec<String> = (0..50).map(|i| format!("offline {i}")).collect();
    assert_eq!(server_bodies, expected);
    until("all applied", || {
        h.client.messages(&tid, None, 100).unwrap().len() == 51
    })
    .await;
    let local: Vec<String> = h
        .client
        .messages(&tid, None, 100)
        .unwrap()
        .into_iter()
        .skip(1)
        .map(|m| m.body)
        .collect();
    assert_eq!(local, expected);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn reconnect_backs_off_and_reconnect_now_skips_the_wait() {
    let server = server_with_group().await;
    server.with(|s| s.refuse = true);
    let mut h = harness_with(server, |c| {
        c.backoff_min = Duration::from_millis(100);
        c.backoff_max = Duration::from_secs(20);
    });
    let mut delays = Vec::new();
    while delays.len() < 6 {
        if let ClientUpdate::Connection(ConnectionState::Disconnected { retry_in }) =
            next_update(&mut h.updates, "disconnected", |u| {
                matches!(
                    u,
                    ClientUpdate::Connection(ConnectionState::Disconnected { .. })
                )
            })
            .await
        {
            delays.push(retry_in);
        }
    }
    println!(
        "reconnect delays: {:?}",
        delays.iter().map(|d| d.as_millis()).collect::<Vec<_>>()
    );
    for (i, d) in delays.iter().enumerate() {
        let base = 100u64 << i;
        assert!(
            d.as_millis() as u64 >= base / 2 && d.as_millis() as u64 <= base * 3 / 2,
            "attempt {i}: {d:?}"
        );
    }
    // The next wait is several seconds; reconnect_now connects at once.
    h.server.with(|s| s.refuse = false);
    let t0 = Instant::now();
    h.client.reconnect_now();
    next_update(&mut h.updates, "connected", |u| {
        matches!(u, ClientUpdate::Connection(ConnectionState::Connected))
    })
    .await;
    println!("reconnect_now connected in {} ms", t0.elapsed().as_millis());
    assert!(t0.elapsed() < Duration::from_millis(1500));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn missing_pong_reconnects() {
    let server = server_with_group().await;
    server.with(|s| s.ignore_pings = true);
    let h = harness_with(server, |c| {
        c.ping_interval = Duration::from_millis(200);
        c.pong_timeout = Duration::from_millis(300);
    });
    until("reconnected after pong timeout", || {
        h.server.with(|s| s.accepted >= 2)
    })
    .await;
    assert!(h.server.with(|s| !s.frames_of("ping").is_empty()));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn bad_token_waits_until_a_new_token() {
    let server = server_with_group().await;
    let dir = tempfile::tempdir().unwrap();
    let mut cfg = config(&server, dir.path());
    cfg.token = "tc_wrong".into();
    let client = Client::new(cfg).unwrap();
    let mut rx = client.subscribe();
    client.start();
    next_update(&mut rx, "unauthorized", |u| {
        matches!(u, ClientUpdate::Connection(ConnectionState::Unauthorized))
    })
    .await;
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(
        client.connection_state(),
        ConnectionState::Unauthorized,
        "no retry loop"
    );
    client.set_token(TOKEN.into());
    next_update(&mut rx, "connected", |u| {
        matches!(u, ClientUpdate::Connection(ConnectionState::Connected))
    })
    .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn start_topic_edit_delete_and_refused_update() {
    let server = server_with_group().await;
    let mut h = harness_with(server, |_| {});
    next_update(&mut h.updates, "caught up", |u| {
        matches!(u, ClientUpdate::CaughtUp)
    })
    .await;
    h.client
        .start_topic(G1, None, "First line\nmore", vec![])
        .unwrap();
    assert_eq!(h.client.pending_new_topics(G1).unwrap().len(), 1);
    next_update(&mut h.updates, "sent", |u| {
        matches!(u, ClientUpdate::Sent { .. })
    })
    .await;
    assert!(h.client.pending_new_topics(G1).unwrap().is_empty());
    let topics = h.client.topics(G1, 10).unwrap();
    assert_eq!(topics[0].topic.title, "First line");
    let tid = topic_of(&h);
    let mid = h.client.messages(&tid, None, 10).unwrap()[0].id.clone();

    h.client.edit_message(&mid, "edited").unwrap();
    until("edit", || {
        h.client.messages(&tid, None, 10).unwrap()[0].body == "edited"
    })
    .await;
    h.client.delete_message(&mid).unwrap();
    until("delete", || {
        h.client.messages(&tid, None, 10).unwrap()[0]
            .deleted_at()
            .is_some()
    })
    .await;
    let m = h.client.messages(&tid, None, 10).unwrap().remove(0);
    assert_eq!(m.body, "");

    h.client
        .update_topic(
            &tid,
            chat_core::TopicPatch {
                status: Some(TopicStatus::Done),
                ..Default::default()
            },
        )
        .unwrap();
    until("status", || {
        h.client.topics(G1, 10).unwrap()[0].topic.status == TopicStatus::Done
    })
    .await;

    h.server
        .with(|s| s.topic_update_error = Some(ErrorCode::Forbidden));
    let id = h
        .client
        .update_topic(
            &tid,
            chat_core::TopicPatch {
                title: Some("nope".into()),
                ..Default::default()
            },
        )
        .unwrap();
    let u = next_update(&mut h.updates, "send failed", |u| {
        matches!(u, ClientUpdate::SendFailed(_))
    })
    .await;
    let ClientUpdate::SendFailed(f) = u else {
        unreachable!()
    };
    assert_eq!(
        (f.id.as_str(), f.code, f.kind),
        (id.as_str(), ErrorCode::Forbidden, "topic_update")
    );
    assert_eq!(
        h.client.store().outbox_get(&id).unwrap().unwrap().state,
        OutboxState::Failed
    );
    h.client.discard(&id).unwrap();
    assert!(h.client.store().outbox_all().unwrap().is_empty());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn typing_presence_and_streaming_text() {
    let server = server_with_group().await;
    let tid = server.with(|s| {
        s.post(G1, ALICE, None, None, "a0", "root", false)
            .1
            .unwrap()
            .id
    });
    let mut h = harness_with(server, |_| {});
    next_update(&mut h.updates, "caught up", |u| {
        matches!(u, ClientUpdate::CaughtUp)
    })
    .await;

    h.server.with(|s| {
        s.transient(
            json!({"type": "typing", "groupId": G1, "topicId": tid, "memberId": ALICE, "at": 1}),
        );
        s.transient(json!({"type": "presence", "memberId": ALICE, "online": true, "at": 1}));
    });
    next_update(&mut h.updates, "typing", |u| {
        matches!(u, ClientUpdate::Typing { .. })
    })
    .await;
    assert_eq!(h.client.typing(G1, &tid), vec![ALICE.to_string()]);
    next_update(&mut h.updates, "presence", |u| {
        matches!(u, ClientUpdate::Presence { online: true, .. })
    })
    .await;
    assert!(h.client.is_online(ALICE));

    // A bot reply: streaming send, deltas, final edit.
    let mid = h
        .server
        .with(|s| s.post(G1, ALICE, Some(&tid), None, "a1", "", true).0.id);
    until("streaming message", || {
        h.client.messages(&tid, None, 10).unwrap().len() == 2
    })
    .await;
    assert!(
        h.client.typing(G1, &tid).is_empty(),
        "a new message clears typing"
    );
    h.server.with(|s| {
        for part in ["Hel", "lo ", "wor"] {
            s.transient(json!({"type": "message.delta", "groupId": G1, "topicId": tid, "messageId": mid, "append": part}));
        }
    });
    until("merged deltas", || {
        h.client.messages(&tid, None, 10).unwrap()[1].body == "Hello wor"
    })
    .await;
    h.server.with(|s| {
        s.append(
            G1,
            ALICE,
            EventData::MessageUpdated(MessageUpdatedData {
                message_id: mid.clone(),
                body: "Hello world".into(),
                edited_at: 9,
                streaming: false,
                ..Default::default()
            }),
        );
    });
    until("final body", || {
        h.client.messages(&tid, None, 10).unwrap()[1].body == "Hello world"
    })
    .await;
    let m = h.client.messages(&tid, None, 10).unwrap().remove(1);
    assert!(!m.is_streaming());
    assert_eq!(h.client.messages(&tid, None, 10).unwrap().len(), 2);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn load_older_fills_history_and_reports_has_more() {
    let server = server_with_group().await;
    let tid = server.with(|s| {
        let tid = s
            .post(G1, ALICE, None, None, "a0", "root", false)
            .1
            .unwrap()
            .id;
        for i in 1..=30 {
            s.post(
                G1,
                ALICE,
                Some(&tid),
                None,
                &format!("a{i}"),
                &format!("m{i}"),
                false,
            );
        }
        tid
    });
    let mut h = harness_with(server, |c| c.history_limit = 20);
    next_update(&mut h.updates, "caught up", |u| {
        matches!(u, ClientUpdate::CaughtUp)
    })
    .await;
    // Drop the older half from the cache, as a partial cache would have.
    h.client
        .store()
        .with_writer(|tx| {
            tx.execute("DELETE FROM messages WHERE created_seq < 20", [])?;
            Ok(())
        })
        .unwrap();
    h.client.load_older(&tid).unwrap();
    let u = next_update(&mut h.updates, "history", |u| {
        matches!(u, ClientUpdate::HistoryLoaded { .. })
    })
    .await;
    assert!(
        matches!(
            u,
            ClientUpdate::HistoryLoaded {
                has_more: false,
                ..
            }
        ),
        "{u:?}"
    );
    assert_eq!(h.client.store().message_count_cached(&tid).unwrap(), 31);
    let hist = h.server.with(|s| s.frames_of("history"));
    assert_eq!(hist[0]["beforeSeq"], json!(20));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn read_cursor_goes_to_server_and_survives_offline() {
    let server = server_with_group().await;
    let tid = server.with(|s| {
        let tid = s
            .post(G1, ALICE, None, None, "a0", "root", false)
            .1
            .unwrap()
            .id;
        s.post(G1, ALICE, Some(&tid), None, "a1", "m1", false);
        tid
    });
    let mut h = harness_with(server, |_| {});
    next_update(&mut h.updates, "caught up", |u| {
        matches!(u, ClientUpdate::CaughtUp)
    })
    .await;
    assert_eq!(h.client.groups().unwrap()[0].unread, 2);
    h.server.with(|s| {
        s.refuse = true;
        s.close_all();
    });
    next_update(&mut h.updates, "disconnected", |u| {
        matches!(
            u,
            ClientUpdate::Connection(ConnectionState::Disconnected { .. })
        )
    })
    .await;
    h.client.mark_topic_read(G1, &tid, 3).unwrap();
    assert_eq!(h.client.groups().unwrap()[0].unread, 0);
    h.server.with(|s| s.refuse = false);
    h.client.reconnect_now();
    until("read synced", || {
        h.server
            .with(|s| s.groups[G1].topic_reads.get(&tid) == Some(&3))
    })
    .await;
    assert_eq!(
        h.server.with(|s| s.groups[G1].read_seq),
        0,
        "a topic read leaves the group cursor"
    );
    next_update(&mut h.updates, "connected", |u| {
        matches!(u, ClientUpdate::CaughtUp)
    })
    .await;
    assert_eq!(h.client.groups().unwrap()[0].unread, 0);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn unknown_events_notify_flags_and_search() {
    let server = server_with_group().await;
    let tid = server.with(|s| {
        s.post(G1, ALICE, None, None, "a0", "root", false)
            .1
            .unwrap()
            .id
    });
    let mut h = harness_with(server, |_| {});
    next_update(&mut h.updates, "caught up", |u| {
        matches!(u, ClientUpdate::CaughtUp)
    })
    .await;
    // seq 3: an event type from a newer server; seq 4: a message marked for a notification.
    h.server.with(|s| {
        let seq = s.groups[G1].log.len() as u64 + 1;
        s.broadcast(
            &json!({"t": "event", "event": {"seq": seq, "groupId": G1, "at": 1, "actorId": ALICE, "type": "future.thing", "data": {}}}),
            None,
        );
        let g = s.groups.get_mut(G1).unwrap();
        g.log.push(ev(seq, G1, ALICE, EventData::GroupUpdated(GroupUpdatedData { name: format!("group {G1}") })));
        s.hide_live.insert(seq + 1);
        let (m, _) = s.post(G1, ALICE, Some(&tid), None, "a1", "@me look", false);
        let event = s.groups[G1].log.last().unwrap().clone();
        assert_eq!(event.seq, m.created_seq);
        s.broadcast(&json!({"t": "event", "event": event, "notify": "mention"}), None);
    });
    let u = next_update(&mut h.updates, "notify", |u| {
        matches!(u, ClientUpdate::Notify { .. })
    })
    .await;
    let ClientUpdate::Notify(n) = u else {
        unreachable!()
    };
    assert_eq!(
        (n.reason, n.topic_id.as_deref()),
        (NotifyReason::Mention, Some(tid.as_str()))
    );
    assert_eq!(n.body, "@me look");
    assert_eq!(n.actor_name.as_deref(), Some("alice"));
    assert_eq!(n.title, format!("group {G1}"));
    assert_eq!(n.subtitle.as_deref(), Some("root"));
    until("applied past the unknown event", || {
        h.client.store().last_applied(G1).unwrap() == 4
    })
    .await;
    assert_eq!(h.client.messages(&tid, None, 10).unwrap().len(), 2);
    assert!(
        h.server.with(|s| s.frames_of("fetch").len() == 1),
        "no gap fetch for the unknown event"
    );

    let rid = h
        .client
        .search(chat_core::SearchRequest {
            query: "你好".into(),
            ..Default::default()
        })
        .unwrap();
    let u = next_update(&mut h.updates, "search", |u| {
        matches!(u, ClientUpdate::SearchResults { .. })
    })
    .await;
    let ClientUpdate::SearchResults { rid: got, hits } = u else {
        unreachable!()
    };
    assert_eq!(
        (got, hits.len(), hits[0].snippet.as_str()),
        (rid, 1, "你好")
    );
}
