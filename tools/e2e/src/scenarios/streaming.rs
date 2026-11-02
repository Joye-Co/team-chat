//! Streaming: a bot sends `streaming: true`, deltas, then the final edit. Both humans see
//! the text grow and end with the final body; the log stores one message.updated.

use super::{Ctx, Run, Scenario};
use crate::check;
use crate::harness::TestClient;
use crate::raw::{RawWs, created};
use anyhow::Result;
use chat_core::{EditRequest, SendRequest};
use std::time::Duration;

pub const SCENARIO: Scenario = Scenario {
    name: "streaming",
    about: "a bot streams a reply; humans see the final body; one message.updated is stored",
    in_all: true,
    timeout_secs: 90,
    run: start,
};

fn start(c: &mut Ctx) -> Run<'_> {
    Box::pin(run(c))
}

const PARTS: usize = 12;

fn part(i: usize) -> String {
    format!("第{i}段 part {i}. ")
}

/// Waits for the final body and collects the partial bodies seen on the way.
async fn watch(cl: &TestClient, msg_id: &str, fin: &str) -> Result<Vec<String>> {
    let mut partial = Vec::new();
    cl.wait(Duration::from_secs(30), "the final streamed body", |c| {
        let m = c.store().message(msg_id).ok().flatten()?;
        let shown = c
            .messages(&m.topic_id, None, 10)
            .ok()?
            .into_iter()
            .find(|x| x.id == msg_id)?;
        if shown.streaming != Some(true) && shown.body == fin {
            return Some(());
        }
        if !shown.body.is_empty() && partial.last() != Some(&shown.body) {
            partial.push(shown.body);
        }
        None
    })
    .await?;
    Ok(partial)
}

async fn run(c: &mut Ctx) -> Result<()> {
    let h = c.h.clone();
    let a = h.human("stream-a").await?;
    let b = h.human("stream-b").await?;
    let k = h.bot("stream-bot").await?;
    let g = h.group(&a, "stream", &[&b, &k]).await?;
    let ca = h.client(&a, "a", None)?;
    let cb = h.client(&b, "b", None)?;
    let ck = h.client(&k, "bot", None)?;
    for cl in [&ca, &cb, &ck] {
        cl.ready(&[&g]).await?;
    }
    ca.c.start_topic(
        &g,
        Some("stream please"),
        "@bot write something",
        vec![k.id.clone()],
    )?;
    let topic = ck
        .wait(Duration::from_secs(10), "bot: the topic", |cl| {
            cl.topics(&g, 10)
                .ok()?
                .into_iter()
                .next()
                .map(|t| t.topic.id)
        })
        .await?;
    let cmid = ck.c.send(SendRequest {
        group_id: g.clone(),
        topic_id: Some(topic.clone()),
        body: String::new(),
        streaming: true,
        ..Default::default()
    })?;
    let msg_id = ck
        .wait(
            Duration::from_secs(10),
            "bot: the streaming message acked",
            |cl| {
                cl.messages(&topic, None, 10)
                    .ok()?
                    .into_iter()
                    .find(|m| m.client_msg_id == cmid)
                    .map(|m| m.id)
            },
        )
        .await?;
    let fin: String = (1..=PARTS).map(part).collect::<String>() + "Done.";

    let bot = async {
        for i in 1..=PARTS {
            ck.c.send_delta(&g, &msg_id, &part(i));
            tokio::time::sleep(Duration::from_millis(60)).await;
        }
        ck.c.edit(EditRequest {
            message_id: msg_id.clone(),
            body: fin.clone(),
            streaming: false,
            ..Default::default()
        })
    };
    let (sent, pa, pb) = tokio::join!(bot, watch(&ca, &msg_id, &fin), watch(&cb, &msg_id, &fin));
    sent?;
    let (pa, pb) = (pa?, pb?);
    for (who, p) in [("A", &pa), ("B", &pb)] {
        check!(
            !p.is_empty(),
            "{who} saw no streaming text before the final body"
        );
        check!(
            p.iter().all(|s| fin.starts_with(s.as_str())),
            "{who} saw text that is not a prefix of the final body"
        );
    }

    let mut ra = RawWs::connect(&h.base, a.token()).await?;
    let (events, _) = ra.fetch_all(&g).await?;
    let made = created(&events);
    let m = made.iter().find(|m| m["id"] == msg_id.as_str());
    check!(
        m.is_some_and(|m| m["streaming"] == true),
        "message.created is not streaming: true"
    );
    let updates: Vec<_> = events
        .iter()
        .filter(|e| e["type"] == "message.updated" && e["data"]["messageId"] == msg_id.as_str())
        .collect();
    check!(
        updates.len() == 1,
        "{} message.updated events stored, want 1",
        updates.len()
    );
    check!(
        updates[0]["data"]["streaming"] == false && updates[0]["data"]["body"] == fin.as_str(),
        "stored update differs"
    );
    check!(
        !events.iter().any(|e| e["type"] == "message.delta"),
        "a delta was stored"
    );
    c.metric("deltas", PARTS as u64);
    c.metric("partialBodiesSeenA", pa.len() as u64);
    c.metric("partialBodiesSeenB", pb.len() as u64);
    c.note("both humans saw growing text, then the final body; one message.updated stored");
    for cl in [ca, cb, ck] {
        cl.shutdown().await;
    }
    Ok(())
}
