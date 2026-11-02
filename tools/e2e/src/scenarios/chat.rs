//! Two members chat: A starts a topic; B gets topic.created then message.created; B replies;
//! A gets the reply. Then A renames the group; B gets group.updated live and its cache follows.

use super::{Ctx, Run, Scenario};
use crate::check;
use crate::raw::{RawWs, TIMEOUT};
use anyhow::Result;
use std::time::{Duration, Instant};

pub const SCENARIO: Scenario = Scenario {
    name: "chat",
    about: "A starts a topic, B receives it in order and replies, A receives the reply; A renames the group and B sees group.updated",
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
    check!(
        ev2["notify"] == "mention",
        "B was mentioned but notify is {}",
        ev2["notify"]
    );
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

    // Rename: A renames the group (with spaces the server collapses); B gets group.updated on
    // the wire and its cached group takes the new name; the same name again adds no event.
    let t2 = Instant::now();
    let new_name = format!("{} 改名", h.name("chat"));
    let stored =
        ca.c.rename_group(&g, &format!("  {}  ", new_name.replace(' ', "   ")))
            .await??;
    check!(stored == new_name, "server stored {stored:?}");
    let ev = rb
        .next_matching(TIMEOUT, |f| {
            f["t"] == "event"
                && f["event"]["groupId"] == g.as_str()
                && f["event"]["type"] == "group.updated"
        })
        .await?;
    check!(
        ev["event"]["data"]["name"] == new_name.as_str(),
        "group.updated name {}",
        ev["event"]["data"]["name"]
    );
    check!(
        ev["event"]["actorId"] == a.id.as_str(),
        "group.updated actor {}",
        ev["event"]["actorId"]
    );
    let rename_seq = ev["event"]["seq"].as_u64().unwrap_or(0);
    cb.wait(Duration::from_secs(10), "B: the new group name", |cl| {
        cl.store()
            .group(&g)
            .ok()
            .flatten()
            .filter(|row| row.group.name == new_name)
    })
    .await?;
    let rename_ms = t2.elapsed().as_secs_f64() * 1000.0;
    let again = ca.c.rename_group(&g, &new_name).await??;
    check!(again == new_name, "same-name rename stored {again:?}");
    let (_, groups) = h
        .api(reqwest::Method::GET, "/v1/groups", Some(b.token()), None)
        .await?;
    let listed = groups
        .as_array()
        .and_then(|gs| gs.iter().find(|s| s["group"]["id"] == g.as_str()))
        .map(|s| {
            (
                s["group"]["name"].clone(),
                s["lastSeq"].as_u64().unwrap_or(0),
            )
        });
    check!(
        listed.as_ref().is_some_and(|(n, _)| n == new_name.as_str()),
        "GET /v1/groups name {listed:?}"
    );
    check!(
        listed.is_some_and(|(_, last)| last == rename_seq),
        "the same name again added an event"
    );
    c.metric("renameDeliveryMs", (rename_ms * 10.0).round() / 10.0);
    c.note(format!(
        "B got group.updated (seq {rename_seq}) and its cache shows the new name; the same name again added nothing"
    ));
    ca.shutdown().await;
    cb.shutdown().await;
    Ok(())
}
