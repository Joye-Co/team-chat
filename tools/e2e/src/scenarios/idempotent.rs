//! Idempotent resend: the same clientMsgId twice gives one message, and the second ack has
//! `duplicate: true`. Checked for a new topic, for a reply, and after a reconnect.

use super::{Ctx, Run, Scenario};
use crate::check;
use crate::raw::{RawWs, created};
use anyhow::Result;
use serde_json::{Value, json};

pub const SCENARIO: Scenario = Scenario {
    name: "idempotent",
    about: "the same clientMsgId twice gives one message; the second ack has duplicate: true",
    in_all: true,
    timeout_secs: 60,
    run: start,
};

fn start(c: &mut Ctx) -> Run<'_> {
    Box::pin(run(c))
}

fn result(ack: &Value) -> Result<&Value> {
    check!(
        ack["t"] == "ack" && ack["ok"] == true,
        "send refused: {ack}"
    );
    Ok(&ack["result"])
}

async fn run(c: &mut Ctx) -> Result<()> {
    let h = c.h.clone();
    let a = h.human("idem-a").await?;
    let b = h.human("idem-b").await?;
    let g = h.group(&a, "idem", &[&b]).await?;
    let mut ra = RawWs::connect(&h.base, a.token()).await?;

    // A new topic sent twice.
    let cm1 = chat_core::new_id();
    let frame = json!({ "t": "send", "groupId": g, "clientMsgId": cm1, "body": "only once" });
    let first = ra.request(frame.clone()).await?;
    let r1 = result(&first)?.clone();
    check!(r1["duplicate"] != true, "first send is marked duplicate");
    check!(r1["topic"]["id"].is_string(), "first send made no topic");
    let second = ra.request(frame).await?;
    let r2 = result(&second)?;
    check!(
        r2["duplicate"] == true,
        "second ack has no duplicate: true: {r2}"
    );
    check!(
        r2["message"]["id"] == r1["message"]["id"],
        "second ack names another message"
    );
    let topic = r1["topic"]["id"].as_str().unwrap_or_default().to_string();

    // A reply sent twice, the second time on a new connection (a resend after a reconnect).
    let cm2 = chat_core::new_id();
    let reply = json!({ "t": "send", "groupId": g, "topicId": topic, "clientMsgId": cm2, "body": "reply once" });
    let r3 = result(&ra.request(reply.clone()).await?)?.clone();
    check!(r3["duplicate"] != true, "first reply is marked duplicate");
    let mut ra2 = RawWs::connect(&h.base, a.token()).await?;
    let r4 = result(&ra2.request(reply).await?)?.clone();
    check!(
        r4["duplicate"] == true,
        "reply resend has no duplicate: true"
    );
    check!(
        r4["message"]["id"] == r3["message"]["id"],
        "reply resend names another message"
    );

    // Another author may use the same clientMsgId: the pair is what is unique.
    let mut rb = RawWs::connect(&h.base, b.token()).await?;
    let r5 = result(
        &rb.request(json!({ "t": "send", "groupId": g, "topicId": topic, "clientMsgId": cm2, "body": "B's own" }))
            .await?,
    )?
    .clone();
    check!(
        r5["duplicate"] != true,
        "B's message with A's clientMsgId is marked duplicate"
    );

    let (events, last) = ra.fetch_all(&g).await?;
    let msgs = created(&events);
    let n1 = msgs
        .iter()
        .filter(|m| m["clientMsgId"] == cm1.as_str())
        .count();
    let n2 = msgs
        .iter()
        .filter(|m| m["clientMsgId"] == cm2.as_str() && m["authorId"] == a.id.as_str())
        .count();
    check!(
        n1 == 1 && n2 == 1,
        "log has {n1} and {n2} copies, want 1 and 1"
    );
    check!(msgs.len() == 3, "log has {} messages, want 3", msgs.len());
    let topics = events
        .iter()
        .filter(|e| e["type"] == "topic.created")
        .count();
    check!(topics == 1, "log has {topics} topics, want 1");
    c.metric("logLastSeq", last);
    c.note("duplicates acked with duplicate: true; log holds one copy of each");
    Ok(())
}
