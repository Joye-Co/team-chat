//! Offline catch-up: B goes offline, A sends 50 messages, B comes back with the same cache
//! and has all 50 in order with no duplicates.

use super::{Ctx, Run, Scenario};
use crate::check;
use anyhow::Result;
use std::collections::HashSet;
use std::time::{Duration, Instant};

pub const SCENARIO: Scenario = Scenario {
    name: "catchup",
    about: "B offline, A sends 50, B reconnects and has all 50 in order, no duplicates",
    in_all: true,
    timeout_secs: 120,
    run: start,
};

fn start(c: &mut Ctx) -> Run<'_> {
    Box::pin(run(c))
}

const N: usize = 50;

async fn run(c: &mut Ctx) -> Result<()> {
    let h = c.h.clone();
    let a = h.human("catchup-a").await?;
    let b = h.human("catchup-b").await?;
    let g = h.group(&a, "catchup", &[&b]).await?;
    let ca = h.client(&a, "a", None)?;
    let cb = h.client(&b, "b", None)?;
    ca.ready(&[&g]).await?;
    cb.ready(&[&g]).await?;
    ca.c.start_topic(&g, Some("catch-up"), "before B goes offline", vec![])?;
    let topic = cb
        .wait(Duration::from_secs(10), "B: the topic", |cl| {
            let t = cl.topics(&g, 10).ok()?.into_iter().next()?;
            (!cl.messages(&t.topic.id, None, 10).ok()?.is_empty()).then_some(t.topic.id)
        })
        .await?;
    let b_before = cb.c.store().last_applied(&g)?;
    cb.shutdown().await;
    c.note(format!("B offline at seq {b_before}"));

    let t_send = Instant::now();
    let mut sent = Vec::new();
    for i in 1..=N {
        sent.push(ca.c.send_message(&g, &topic, &format!("offline message {i}"), vec![])?);
    }
    ca.wait(Duration::from_secs(60), "A: all 50 acked", |cl| {
        cl.pending_sends(&topic).ok()?.is_empty().then_some(())
    })
    .await?;
    let send_ms = t_send.elapsed().as_secs_f64() * 1000.0;
    let (_, server_last) = crate::raw::RawWs::connect(&h.base, a.token())
        .await?
        .fetch_all(&g)
        .await?;

    let t0 = Instant::now();
    let cb = h.client(&b, "b", None)?;
    let ms = cb
        .wait(Duration::from_secs(30), "B: all 51 messages", |cl| {
            let ms = cl.messages(&topic, None, 1000).ok()?;
            (ms.len() > N && cl.store().last_applied(&g).ok()? >= server_last).then_some(ms)
        })
        .await?;
    let catchup_ms = t0.elapsed().as_secs_f64() * 1000.0;
    check!(
        ms.len() == N + 1,
        "B has {} messages, want {}",
        ms.len(),
        N + 1
    );
    let ids: HashSet<_> = ms.iter().map(|m| &m.id).collect();
    check!(ids.len() == ms.len(), "duplicate message ids in B's cache");
    for w in ms.windows(2) {
        check!(
            w[0].created_seq < w[1].created_seq,
            "B's messages are not in seq order"
        );
    }
    for (i, m) in ms[1..].iter().enumerate() {
        check!(m.client_msg_id == sent[i], "message {} out of order", i + 1);
        check!(
            m.body == format!("offline message {}", i + 1),
            "body {} differs",
            i + 1
        );
    }
    let t = cb.c.store().topic(&topic)?.map(|t| t.topic);
    check!(
        t.is_some_and(|t| t.message_count == (N + 1) as u64),
        "messageCount differs"
    );
    c.metric("messages", N as u64);
    c.metric("allAckedMs", (send_ms * 10.0).round() / 10.0);
    c.metric("catchupMs", (catchup_ms * 10.0).round() / 10.0);
    c.note(format!(
        "B caught up from seq {b_before} to {server_last}: 50 messages in order"
    ));
    ca.shutdown().await;
    cb.shutdown().await;
    Ok(())
}
