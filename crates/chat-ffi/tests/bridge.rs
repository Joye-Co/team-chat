//! The bridge against chat-core's in-process mock server: what the app reads and writes.

#[path = "../../chat-core/tests/common/mod.rs"]
mod common;
#[path = "../../chat-core/tests/mock/mod.rs"]
mod mock;

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use chat_ffi::{ChatClient, Update, UpdateListener};
use common::*;
use mock::{MockServer, TOKEN};

const WAIT: Duration = Duration::from_secs(10);

#[derive(Default, Clone)]
struct Recorder(Arc<Mutex<Vec<Update>>>);

impl UpdateListener for Recorder {
    fn changed(&self, update: Update) {
        self.0.lock().unwrap().push(update);
    }
}

impl Recorder {
    fn saw(&self, f: impl Fn(&Update) -> bool) -> bool {
        self.0.lock().unwrap().iter().any(f)
    }
}

async fn until(what: &str, mut f: impl FnMut() -> bool) {
    let t0 = Instant::now();
    while !f() {
        assert!(t0.elapsed() < WAIT, "timed out waiting for {what}");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

struct Harness {
    server: MockServer,
    _dir: tempfile::TempDir,
    client: Arc<ChatClient>,
    updates: Recorder,
    topic: String,
}

/// A group with Alice, one topic she started, and a connected client.
async fn harness() -> Harness {
    let server = MockServer::start(
        member(ME, "me"),
        vec![member(ME, "me"), member(ALICE, "Alice")],
    )
    .await;
    server.add_group(group(G1, &[ME, ALICE]));
    let (_, topic) =
        server.with(|s| s.post(G1, ALICE, None, Some("发版"), "c-a1", "今晚发版？", false));
    let topic = topic.unwrap().id;
    let dir = tempfile::tempdir().unwrap();
    let client = ChatClient::new(
        server.base_url(),
        TOKEN.into(),
        dir.path().to_string_lossy().into(),
    )
    .unwrap();
    let updates = Recorder::default();
    client.listen(Box::new(updates.clone()));
    client.start();
    let c = client.clone();
    until("the group", || {
        c.groups().map(|g| !g.is_empty()).unwrap_or(false)
    })
    .await;
    Harness {
        server,
        _dir: dir,
        client,
        updates,
        topic,
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn lists_the_group_with_its_topic_and_names() {
    let h = harness().await;
    let c = h.client.clone();
    until("the topic", || {
        c.groups()
            .unwrap()
            .first()
            .is_some_and(|g| !g.topics.is_empty())
    })
    .await;
    let g = &h.client.groups().unwrap()[0];
    assert_eq!(g.id, G1);
    assert!(!g.is_dm);
    assert_eq!(g.member_count, 2);
    assert_eq!(g.main_topic_id, chat_protocol::main_topic_id(G1));
    assert_eq!(g.topics[0].title, "发版");
    assert!(!g.topics[0].is_main);
    assert_eq!(h.client.connection_state(), "connected");
    assert_eq!(h.client.my_name().as_deref(), Some("me"));
    let members = h.client.members().unwrap();
    assert!(members.iter().any(|m| m.name == "Alice" && !m.is_me));
}

#[tokio::test(flavor = "multi_thread")]
async fn reads_messages_with_authors_and_marks_them_read() {
    let h = harness().await;
    let (c, t) = (h.client.clone(), h.topic.clone());
    until("the message", || {
        c.messages(t.clone(), 50).unwrap().len() == 1
    })
    .await;
    let m = &h.client.messages(h.topic.clone(), 50).unwrap()[0];
    assert_eq!(m.author_name, "Alice");
    assert!(!m.is_mine);
    assert_eq!(m.kind, "text");
    assert_eq!(m.body, "今晚发版？");
    h.client
        .mark_read(G1.into(), h.topic.clone(), m.seq)
        .unwrap();
    let topic = h.client.topic(G1.into(), h.topic.clone()).unwrap().unwrap();
    assert_eq!(topic.read_seq, m.seq);
    assert_eq!(topic.unread, 0);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_send_is_pending_then_a_message_and_the_update_names_the_topic() {
    let h = harness().await;
    let id = h
        .client
        .send(G1.into(), h.topic.clone(), "可以".into(), vec![], None)
        .unwrap();
    assert!(!id.is_empty());
    let (c, t) = (h.client.clone(), h.topic.clone());
    until("the ack", || {
        c.pending(t.clone()).unwrap().is_empty()
            && c.messages(t.clone(), 50)
                .unwrap()
                .iter()
                .any(|m| m.is_mine && m.body == "可以")
    })
    .await;
    let topic = h.topic.clone();
    assert!(
        h.updates
            .saw(|u| matches!(u, Update::Messages { topic_id, .. } if *topic_id == topic))
    );
    assert!(h.updates.saw(|u| matches!(u, Update::Connection)));
}

#[tokio::test(flavor = "multi_thread")]
async fn sends_while_offline_go_out_after_the_reconnect_once() {
    let h = harness().await;
    h.server.with(|s| s.refuse = true);
    h.server.with(|s| s.close_all());
    let c = h.client.clone();
    until("offline", || c.connection_state() != "connected").await;
    h.client
        .send(
            G1.into(),
            h.topic.clone(),
            "离线时写的".into(),
            vec![],
            None,
        )
        .unwrap();
    assert_eq!(h.client.pending(h.topic.clone()).unwrap().len(), 1);
    // Someone else writes while I am away.
    h.server
        .with(|s| s.post(G1, ALICE, Some(&h.topic), None, "c-a2", "我先测一下", false));
    h.server.with(|s| s.refuse = false);
    h.client.reconnect_now();
    let t = h.topic.clone();
    until("both messages", || {
        let ms = c.messages(t.clone(), 50).unwrap();
        ms.iter().any(|m| m.body == "我先测一下") && ms.iter().any(|m| m.body == "离线时写的")
    })
    .await;
    let ms = h.client.messages(h.topic.clone(), 50).unwrap();
    assert_eq!(ms.iter().filter(|m| m.body == "离线时写的").count(), 1);
    assert!(h.client.pending(h.topic.clone()).unwrap().is_empty());
    assert_eq!(h.server.with(|s| s.message_count(G1)), 3);
}

#[tokio::test(flavor = "multi_thread")]
async fn bad_input_is_invalid() {
    let h = harness().await;
    let long = "字".repeat(100_000);
    assert!(matches!(
        h.client
            .send(G1.into(), h.topic.clone(), long, vec![], None),
        Err(chat_ffi::CoreError::Invalid)
    ));
    assert!(h.client.file_path("not json".into()).is_err());
    assert!(matches!(
        chat_ffi::request_email_code("ftp://x".into(), "a@b.co".into()),
        Err(chat_ffi::CoreError::Invalid)
    ));
}

#[tokio::test(flavor = "multi_thread")]
async fn sending_a_missing_file_is_invalid() {
    let h = harness().await;
    let r = h.client.send_file(
        G1.into(),
        h.topic.clone(),
        "/nonexistent/photo.jpg".into(),
        None,
        String::new(),
        None,
    );
    assert!(matches!(r, Err(chat_ffi::CoreError::Invalid)));
}
