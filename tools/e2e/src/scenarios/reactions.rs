//! Reactions: A and B react to one message; each sees the other's reaction live in the
//! cache, a second add is a no-op, removing works, a fresh client rebuilds the list from the
//! log, and the log stores one `reaction.changed` per change.

use super::{Ctx, Run, Scenario};
use crate::check;
use crate::raw::RawWs;
use anyhow::Result;
use chat_core::proto::{EventData, Reaction};
use std::time::{Duration, Instant};

pub const SCENARIO: Scenario = Scenario {
    name: "reactions",
    about: "Two members add and remove reactions; caches follow live, a fresh client rebuilds them",
    in_all: true,
    timeout_secs: 90,
    run: start,
};

fn start(c: &mut Ctx) -> Run<'_> {
    Box::pin(run(c))
}

const WAIT: Duration = Duration::from_secs(10);

fn reactions_of(cl: &chat_core::Client, topic_id: &str, message_id: &str) -> Option<Vec<Reaction>> {
    cl.messages(topic_id, None, 10)
        .ok()?
        .into_iter()
        .find(|m| m.id == message_id)
        .map(|m| m.reactions().to_vec())
}

fn r(emoji: &str, ids: &[&str]) -> Reaction {
    Reaction {
        emoji: emoji.into(),
        member_ids: ids.iter().map(|s| s.to_string()).collect(),
    }
}

async fn run(c: &mut Ctx) -> Result<()> {
    let h = c.h.clone();
    let a = h.human("react-a").await?;
    let b = h.human("react-b").await?;
    let g = h.group(&a, "reactions", &[&b]).await?;
    let ca = h.client(&a, "a", None)?;
    let cb = h.client(&b, "b", None)?;
    ca.ready(&[&g]).await?;
    cb.ready(&[&g]).await?;

    ca.c.start_topic(&g, None, "Ship v0.9.2 tonight?", vec![])?;
    let root = cb
        .wait(WAIT, "B: the root message", |cl| {
            let t = cl.topics(&g, 10).ok()?.into_iter().next()?;
            cl.messages(&t.topic.id, None, 10).ok()?.into_iter().next()
        })
        .await?;
    let (tid, mid) = (root.topic_id.clone(), root.id.clone());

    let t0 = Instant::now();
    cb.c.react(&mid, "👍", true)?;
    let want = vec![r("👍", &[&b.id])];
    ca.wait(WAIT, "A: B's 👍", |cl| {
        (reactions_of(cl, &tid, &mid)? == want).then_some(())
    })
    .await?;
    c.metric(
        "reactionDeliveryMs",
        (t0.elapsed().as_secs_f64() * 10000.0).round() / 10.0,
    );

    ca.c.react(&mid, "👍", true)?;
    ca.c.react(&mid, "✅", true)?;
    cb.c.react(&mid, "👍", true)?; // already on: no event
    let want = vec![r("👍", &[&b.id, &a.id]), r("✅", &[&a.id])];
    cb.wait(WAIT, "B: two 👍 and A's ✅", |cl| {
        (reactions_of(cl, &tid, &mid)? == want).then_some(())
    })
    .await?;
    c.note("both caches show 👍 B、A and ✅ A");

    cb.c.react(&mid, "👍", false)?;
    let want = vec![r("👍", &[&a.id]), r("✅", &[&a.id])];
    ca.wait(WAIT, "A: B removed 👍", |cl| {
        (reactions_of(cl, &tid, &mid)? == want).then_some(())
    })
    .await?;
    ca.wait(WAIT, "A: no pending reactions", |cl| {
        cl.pending_reactions(&tid).ok()?.is_empty().then_some(())
    })
    .await?;

    // A fresh client of B (empty cache) rebuilds the list from the log.
    cb.shutdown().await;
    let cb2 = h.client(&b, "b2", None)?;
    cb2.ready(&[&g]).await?;
    let fresh = cb2
        .wait(WAIT, "B2: reactions from the log", |cl| {
            let got = reactions_of(cl, &tid, &mid)?;
            (got == want).then_some(got)
        })
        .await?;
    check!(fresh.len() == 2, "restart lost reactions");

    // The log has one event per change: B 👍, A 👍, A ✅, B 👍 off.
    let mut raw = RawWs::connect(&h.base, a.token()).await?;
    let (events, _) = raw.fetch_all(&g).await?;
    let changes: Vec<(String, bool)> = events
        .iter()
        .filter_map(|e| serde_json::from_value::<chat_core::proto::Event>(e.clone()).ok())
        .filter_map(|e| match e.data {
            EventData::ReactionChanged(d) => Some((d.emoji, d.on)),
            _ => None,
        })
        .collect();
    check!(
        changes
            == vec![
                ("👍".to_string(), true),
                ("👍".to_string(), true),
                ("✅".to_string(), true),
                ("👍".to_string(), false)
            ],
        "stored changes: {changes:?}"
    );
    c.note("the log stores 4 reaction.changed events; the repeated add stored none");
    ca.shutdown().await;
    cb2.shutdown().await;
    Ok(())
}
