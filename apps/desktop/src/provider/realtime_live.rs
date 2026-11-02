//! Live check of the real-time features with two signed-in members, through
//! two chat-core providers against a real server (`wrangler dev`):
//!
//! ```sh
//! TC_LIVE_URL=http://127.0.0.1:8801 \
//! TC_LIVE_TOKEN_A=~/.config/team-chat/<env>/owner-token \
//! TC_LIVE_TOKEN_B=~/.config/team-chat/<env>/<member>-token \
//! cargo test -p desktop realtime_live -- --ignored --nocapture
//! ```
//!
//! Member A and member B must be different members; A creates a new 群 with
//! B, so every run starts clean. Tokens stay in memory; caches go to a
//! temporary folder that is removed at the end. The test never signs out on
//! the server, so the tokens stay valid.

use std::{
    path::PathBuf,
    sync::Arc,
    time::{Duration, Instant},
};

use futures::channel::mpsc::UnboundedReceiver;

use super::{CoreProvider, Notice, ProviderConfig, persist, session, session::TokenStorage};
use crate::{
    copy::{fill, notify as n},
    data::{
        ChatProvider, ConnectionState, CreateGroupRequest, Credential, DraftKey, LoginRequest,
        Message, SavedNavigation, SendRequest, SessionState, StartTopicRequest, TodoReason,
        TopicStatus,
    },
};

const TIMEOUT: Duration = Duration::from_secs(20);

fn wait<T>(what: &str, mut check: impl FnMut() -> Option<T>) -> T {
    let deadline = Instant::now() + TIMEOUT;
    loop {
        if let Some(v) = check() {
            eprintln!("ok: {what}");
            return v;
        }
        assert!(Instant::now() < deadline, "timed out: {what}");
        std::thread::sleep(Duration::from_millis(20));
    }
}

fn read_token(var: &str) -> Option<String> {
    let path = std::env::var(var).ok()?;
    let path = match path.strip_prefix("~/") {
        Some(rest) => PathBuf::from(std::env::var("HOME").ok()?).join(rest),
        None => PathBuf::from(path),
    };
    Some(std::fs::read_to_string(path).ok()?.trim().to_string())
}

struct Side {
    p: Arc<CoreProvider>,
    tokens: TokenStorage,
    root: PathBuf,
    me: String,
    notices: UnboundedReceiver<Notice>,
}

fn sign_in(url: &str, token: String, root: PathBuf) -> Side {
    let tokens = TokenStorage::memory();
    let p = CoreProvider::new(ProviderConfig {
        data_root: root.clone(),
        tokens: tokens.clone(),
        restore: false,
    });
    let notices = p.notices();
    p.login(LoginRequest {
        server_url: url.to_string(),
        credential: Credential::Token(token),
        device_name: "realtime-live".into(),
    });
    let me = wait("signed in", || match p.session() {
        SessionState::SignedIn { me, .. } => Some(me.id),
        _ => None,
    });
    wait("online", || {
        (p.connection() == ConnectionState::Online).then_some(())
    });
    Side {
        p,
        tokens,
        root,
        me,
        notices,
    }
}

impl Side {
    /// The next notification for `topic_id` with this title, skipping others.
    fn notice(&mut self, what: &str, topic_id: &str, title: &str) -> Notice {
        wait(what, || {
            while let Ok(n) = self.notices.try_recv() {
                eprintln!("  notice: {} / {}", n.title, n.body);
                if n.topic_id.as_deref() == Some(topic_id) && n.title == title {
                    return Some(n);
                }
            }
            None
        })
    }

    fn drain_notices(&mut self) -> Vec<Notice> {
        let mut out = Vec::new();
        while let Ok(n) = self.notices.try_recv() {
            out.push(n);
        }
        out
    }

    fn message(&self, g: &str, t: &str, id: &str) -> Option<Message> {
        self.p
            .messages(g, t, 200)
            .messages
            .into_iter()
            .find(|m| m.id == id)
    }
}

#[test]
#[ignore = "needs a server: set TC_LIVE_URL, TC_LIVE_TOKEN_A and TC_LIVE_TOKEN_B"]
fn realtime_live() {
    let (Ok(url), Some(token_a), Some(token_b)) = (
        std::env::var("TC_LIVE_URL"),
        read_token("TC_LIVE_TOKEN_A"),
        read_token("TC_LIVE_TOKEN_B"),
    ) else {
        panic!("set TC_LIVE_URL, TC_LIVE_TOKEN_A and TC_LIVE_TOKEN_B");
    };
    let base = std::env::temp_dir().join(format!("realtime-live-{}", std::process::id()));
    let a = sign_in(&url, token_a, base.join("a"));
    let mut b = sign_in(&url, token_b, base.join("b"));
    assert_ne!(a.me, b.me, "A and B must be different members");
    let (me_a, me_b) = (a.me.clone(), b.me.clone());

    // Presence.
    wait("A sees B online", || {
        a.p.member(&me_b).filter(|m| m.online).map(|_| ())
    });

    // A new 群 with both members.
    let name = format!("rt-{}", chat_core::now_ms());
    a.p.create_group(CreateGroupRequest {
        name: name.clone(),
        member_ids: vec![me_b.clone()],
    });
    let g = wait("B sees the new group", || {
        b.p.groups()
            .into_iter()
            .find(|g| g.name == name)
            .map(|g| g.id)
    });
    wait("A sees the new group", || {
        a.p.groups().iter().any(|x| x.id == g).then_some(())
    });
    let group_name = name.clone();

    // Topic 1: unread for B.
    let cid = a.p.start_topic(StartTopicRequest {
        group_id: g.clone(),
        title: Some("话题一".into()),
        body: "第一条".into(),
        mentions: vec![],
    });
    let root_id = wait("topic 1 acked", || a.p.acked_message(&cid));
    let t1 = wait("B sees topic 1 unread", || {
        b.p.topics(&g)
            .into_iter()
            .find(|t| t.title == "话题一" && t.unread == 1)
            .map(|t| t.id)
    });

    // A mention: unread, mention count, 待办 and a notification for B.
    let cid = a.p.send(SendRequest {
        group_id: g.clone(),
        topic_id: t1.clone(),
        body: "@Alex 看一下".into(),
        mentions: vec![me_b.clone()],
        quote_of: None,
    });
    let mention_id = wait("mention acked", || a.p.acked_message(&cid));
    wait("B: topic 1 has 2 unread and 1 mention", || {
        b.p.topic(&g, &t1)
            .filter(|t| t.unread == 2 && t.mentions == 1)
            .map(|_| ())
    });
    wait("B: 待办 lists topic 1 for the mention", || {
        b.p.todo()
            .iter()
            .any(|i| i.topic.id == t1 && i.reason == TodoReason::Mentioned)
            .then_some(())
    });
    let a_name = a.p.member(&me_a).map(|m| m.name).unwrap_or_default();
    b.notice(
        "B: mention notification",
        &t1,
        &fill(
            n::MENTION_TITLE,
            &[("name", &a_name), ("group", &group_name)],
        ),
    );

    // Topic 2, then B reads topic 1 only.
    let cid = a.p.start_topic(StartTopicRequest {
        group_id: g.clone(),
        title: Some("话题二".into()),
        body: "第二个话题".into(),
        mentions: vec![],
    });
    wait("topic 2 acked", || a.p.acked_message(&cid));
    let t2 = wait("B sees topic 2 unread", || {
        b.p.topics(&g)
            .into_iter()
            .find(|t| t.title == "话题二" && t.unread == 1)
            .map(|t| t.id)
    });
    let read_to = b.p.topic(&g, &t1).unwrap().last_seq;
    assert!(b.p.topic_read_seq(&g, &t1).unwrap() < read_to);
    b.p.mark_read(&g, &t1, read_to);
    wait("B: topic 1 read, topic 2 still unread", || {
        let t1s = b.p.topic(&g, &t1)?;
        let t2s = b.p.topic(&g, &t2)?;
        (t1s.unread == 0 && t1s.mentions == 0 && t2s.unread == 1).then_some(())
    });
    assert!(b.p.topic_read_seq(&g, &t1).unwrap() >= read_to);
    wait("B: 待办 drops the read mention", || {
        (!b.p.todo().iter().any(|i| i.topic.id == t1)).then_some(())
    });

    // Status and waitingOn: chip state, 待办 and a notification for B.
    a.p.set_topic_status(&g, &t1, TopicStatus::WaitingAnswer, vec![me_b.clone()]);
    wait("B: topic 1 waits on B", || {
        b.p.topic(&g, &t1)
            .filter(|t| t.status == TopicStatus::WaitingAnswer && t.waiting_on == [me_b.clone()])
            .map(|_| ())
    });
    let topic = b.p.topic(&g, &t1).unwrap();
    let members = b.p.members();
    assert_eq!(
        crate::views::status_chip_text(&topic, Some(&me_b), &members).as_deref(),
        Some("等你回答")
    );
    assert_eq!(
        crate::views::status_chip_text(&topic, Some(&me_a), &members),
        Some(format!("等{}回答", b.p.member(&me_b).unwrap().name))
    );
    wait("B: 待办 lists topic 1 as waiting on me", || {
        b.p.todo()
            .iter()
            .any(|i| i.topic.id == t1 && i.reason == TodoReason::WaitingOnMe)
            .then_some(())
    });
    b.notice(
        "B: waiting notification",
        &t1,
        &fill(n::WAITING_TITLE, &[("group", &group_name)]),
    );

    // Owner.
    a.p.set_topic_owner(&g, &t1, Some(me_b.clone()));
    wait("B: B owns topic 1", || {
        b.p.topic(&g, &t1)
            .filter(|t| t.owner_id.as_deref() == Some(me_b.as_str()))
            .map(|_| ())
    });

    // Typing.
    b.p.set_typing(&g, &t1);
    wait("A sees B typing in topic 1", || {
        a.p.typing(&g, &t1).contains(&me_b).then_some(())
    });

    // Quote: B replies to the root message; A sees the quote.
    let cid = b.p.send(SendRequest {
        group_id: g.clone(),
        topic_id: t1.clone(),
        body: "回复第一条".into(),
        mentions: vec![],
        quote_of: Some(root_id.clone()),
    });
    let reply_id = wait("reply acked", || b.p.acked_message(&cid));
    wait("A sees the reply with its quote", || {
        a.message(&g, &t1, &reply_id)
            .filter(|m| m.quote_of.as_deref() == Some(root_id.as_str()))
            .map(|_| ())
    });

    // Edit and delete.
    a.p.edit(&g, &mention_id, "已修改".into());
    wait("B sees the edit", || {
        b.message(&g, &t1, &mention_id)
            .filter(|m| m.body == "已修改" && m.edited_at.is_some())
            .map(|_| ())
    });
    a.p.delete(&g, &mention_id);
    wait("B sees the delete", || {
        b.message(&g, &t1, &mention_id)
            .filter(|m| m.deleted_at.is_some())
            .map(|_| ())
    });

    // A streaming reply from B, live for A.
    let bc = b.p.client().unwrap();
    let cid = bc
        .send(chat_core::SendRequest {
            group_id: g.clone(),
            topic_id: Some(t1.clone()),
            body: String::new(),
            streaming: true,
            ..Default::default()
        })
        .unwrap();
    let stream_id = wait("streaming message acked", || b.p.acked_message(&cid));
    wait("A sees the streaming message", || {
        a.message(&g, &t1, &stream_id)
            .filter(|m| m.streaming)
            .map(|_| ())
    });
    bc.send_delta(&g, &stream_id, "第一段");
    wait("A sees the delta while streaming", || {
        a.message(&g, &t1, &stream_id)
            .filter(|m| m.streaming && m.body.contains("第一段"))
            .map(|_| ())
    });
    bc.edit(chat_core::EditRequest {
        message_id: stream_id.clone(),
        body: "第一段，完成".into(),
        streaming: false,
        ..Default::default()
    })
    .unwrap();
    wait("A sees the final body", || {
        a.message(&g, &t1, &stream_id)
            .filter(|m| !m.streaming && m.body == "第一段，完成")
            .map(|_| ())
    });

    // Mute: a muted 群 does not notify that a topic waits on me.
    b.p.set_group_muted(&g, true);
    wait("B: group muted", || b.p.group_muted(&g).then_some(()));
    b.drain_notices();
    a.p.set_topic_status(&g, &t2, TopicStatus::WaitingReview, vec![me_b.clone()]);
    wait("B: topic 2 waits for review", || {
        b.p.topic(&g, &t2)
            .filter(|t| t.status == TopicStatus::WaitingReview)
            .map(|_| ())
    });
    std::thread::sleep(Duration::from_millis(500));
    let muted_notices = b.drain_notices();
    assert!(
        muted_notices
            .iter()
            .all(|n| n.topic_id.as_deref() != Some(&t2)),
        "a muted group notified: {muted_notices:?}"
    );
    b.p.set_group_muted(&g, false);
    wait("B: group unmuted", || (!b.p.group_muted(&g)).then_some(()));

    // Follow: a followed topic that is done notifies.
    b.p.set_topic_following(&g, &t2, true);
    wait("B follows topic 2", || {
        b.p.topic_following(&g, &t2).then_some(())
    });
    a.p.set_topic_status(&g, &t2, TopicStatus::Done, vec![]);
    b.notice(
        "B: result notification for a followed topic",
        &t2,
        &fill(n::RESULT_TITLE, &[("group", &group_name)]),
    );
    b.p.set_topic_following(&g, &t1, false);
    wait("B unfollows topic 1", || {
        (!b.p.topic_following(&g, &t1)).then_some(())
    });

    // Drafts and the saved place survive a restart.
    let key = DraftKey::topic(g.clone(), t1.clone());
    b.p.save_draft_text(&key, "未发送的草稿".into(), vec![me_a.clone()]);
    b.p.set_quote(&key, Some(root_id.clone()));
    let place = SavedNavigation {
        todo: false,
        group_id: Some(g.clone()),
        topic_id: Some(t1.clone()),
    };
    b.p.save_navigation(place.clone());
    let dir = session::account_dir(&b.root, &url, &me_b);
    wait("ui state written", || {
        let state = persist::load(&dir);
        (state.navigation.is_some() && state.drafts.iter().any(|d| d.key() == key)).then_some(())
    });
    b.p.sign_out_local_for_selftest();
    assert!(b.p.draft(&key).body.is_empty());
    let restored = CoreProvider::new(ProviderConfig {
        data_root: b.root.clone(),
        tokens: b.tokens.clone(),
        restore: true,
    });
    let draft = restored.draft(&key);
    assert_eq!(draft.body, "未发送的草稿");
    assert_eq!(draft.mentions, vec![me_a.clone()]);
    assert_eq!(draft.quote_of.as_deref(), Some(root_id.as_str()));
    assert_eq!(restored.saved_navigation(), Some(place));
    eprintln!("ok: drafts and place restored");
    wait("restored provider online", || {
        (restored.connection() == ConnectionState::Online).then_some(())
    });

    restored.sign_out_local_for_selftest();
    a.p.sign_out_local_for_selftest();
    let _ = std::fs::remove_dir_all(&base);
}
