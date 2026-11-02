//! Two members chat: A starts a topic; B gets topic.created then message.created; B replies;
//! A gets the reply.

use super::{Ctx, Run, Scenario};
use crate::check;
use crate::raw::{RawWs, TIMEOUT};
use anyhow::Result;
use std::time::{Duration, Instant};

pub const SCENARIO: Scenario = Scenario {
    name: "chat",
    about: "A starts a topic, B receives it in order and replies, A receives the reply",
    in_all: true,
    timeout_secs: 90,
    run: start,
};

fn start(c: &mut Ctx) -> Run<'_> {
    Box::pin(run(c))
}

async fn run(c: &mut Ctx) -> Result<()> {
    let h = c.h.clone();
    let a = h.human("chat-a").await?;
    let b = h.human("chat-b").await?;
    let g = h.group(&a, "chat", &[&b]).await?;
    let ca = h.client(&a, "a", None)?;
    let cb = h.client(&b, "b", None)?;
    ca.ready(&[&g]).await?;
    cb.ready(&[&g]).await?;
    // A second connection of B reads the raw frames, to check the order on the wire.
    let mut rb = RawWs::connect(&h.base, b.token()).await?;

    let t0 = Instant::now();
    let cmid =
        ca.c.start_topic(&g, None, "你好, B. First topic from A.", vec![b.id.clone()])?;
    let topic = cb
        .wait(Duration::from_secs(10), "B: the new topic", |cl| {
            cl.topics(&g, 10).ok()?.into_iter().next().map(|t| t.topic)
        })
        .await?;
    let root = cb
        .wait(Duration::from_secs(10), "B: the root message", |cl| {
            cl.messages(&topic.id, None, 10).ok()?.into_iter().next()
        })
        .await?;
    let first_ms = t0.elapsed().as_secs_f64() * 1000.0;
    check!(root.client_msg_id == cmid, "root clientMsgId differs");
    check!(
        root.id == topic.root_message_id,
        "root message is not the topic root"
    );
    check!(root.author_id == a.id, "root author is not A");
    check!(
        root.mentions == vec![b.id.clone()],
        "mentions lost: {:?}",
        root.mentions
    );
    check!(
        topic.title == "你好, B. First topic from A.",
        "default title: {:?}",
        topic.title
    );

    let ev1 = rb
        .next_matching(TIMEOUT, |f| {
            f["t"] == "event" && f["event"]["groupId"] == g.as_str()
        })
        .await?;
    let ev2 = rb
        .next_matching(TIMEOUT, |f| {
            f["t"] == "event" && f["event"]["groupId"] == g.as_str()
        })
        .await?;
    check!(
        ev1["event"]["type"] == "topic.created",
        "first event is {}",
        ev1["event"]["type"]
    );
    check!(
        ev2["event"]["type"] == "message.created",
        "second event is {}",
        ev2["event"]["type"]
    );
    let (s1, s2) = (
        ev1["event"]["seq"].as_u64().unwrap_or(0),
        ev2["event"]["seq"].as_u64().unwrap_or(0),
    );
    check!(s2 == s1 + 1, "seqs {s1} then {s2}");
    check!(
        ev2["event"]["data"]["message"]["topicId"] == topic.id.as_str(),
        "message in another topic"
    );
    // Notifications are a V1 extension: when the server marks the event, it must say mention.
    match ev2["notify"].as_str() {
        Some(n) => check!(n == "mention", "B was mentioned but notify is {n}"),
        None => c.note("the server sent no notify reason (notifications not on this server)"),
    }
    c.note(format!(
        "B got topic.created (seq {s1}) then message.created (seq {s2})"
    ));

    let t1 = Instant::now();
    let reply = cb.c.send_message(&g, &topic.id, "Got it, A.", vec![])?;
    let got = ca
        .wait(Duration::from_secs(10), "A: B's reply", |cl| {
            let ms = cl.messages(&topic.id, None, 10).ok()?;
            (ms.len() == 2).then_some(ms)
        })
        .await?;
    let reply_ms = t1.elapsed().as_secs_f64() * 1000.0;
    check!(
        got[0].id == root.id && got[1].client_msg_id == reply,
        "A's order is wrong"
    );
    check!(
        got[1].author_id == b.id && got[1].body == "Got it, A.",
        "reply differs"
    );
    check!(
        got[1].created_seq > got[0].created_seq,
        "reply seq not after root"
    );
    let t = ca.c.store().topic(&topic.id)?.map(|t| t.topic);
    check!(
        t.is_some_and(|t| t.message_count == 2),
        "A's topic messageCount is not 2"
    );
    c.metric("topicDeliveryMs", (first_ms * 10.0).round() / 10.0);
    c.metric("replyDeliveryMs", (reply_ms * 10.0).round() / 10.0);
    c.note("A got B's reply after the root, messageCount 2");
    ca.shutdown().await;
    cb.shutdown().await;
    Ok(())
}
