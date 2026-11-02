//! Gap handling: B's test hook drops one live event; the next event shows the gap and B
//! fetches the missing one.

use super::{Ctx, Run, Scenario};
use crate::check;
use anyhow::Result;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{Duration, Instant};

pub const SCENARIO: Scenario = Scenario {
    name: "gap",
    about: "drop one delivered event on B's side; B fetches the gap",
    in_all: true,
    timeout_secs: 60,
    run: start,
};

fn start(c: &mut Ctx) -> Run<'_> {
    Box::pin(run(c))
}

async fn run(c: &mut Ctx) -> Result<()> {
    let h = c.h.clone();
    let a = h.human("gap-a").await?;
    let b = h.human("gap-b").await?;
    let g = h.group(&a, "gap", &[&b]).await?;
    let target = Arc::new(AtomicU64::new(0));
    let dropped = Arc::new(AtomicBool::new(false));
    let hook: chat_core::DropLiveEvent = {
        let (g, target, dropped) = (g.clone(), target.clone(), dropped.clone());
        Arc::new(move |gid: &str, seq: u64| {
            gid == g
                && seq == target.load(Ordering::SeqCst)
                && !dropped.swap(true, Ordering::SeqCst)
        })
    };
    let ca = h.client(&a, "a", None)?;
    let cb = h.client(&b, "b", Some(hook))?;
    ca.ready(&[&g]).await?;
    cb.ready(&[&g]).await?;
    ca.c.start_topic(&g, Some("gaps"), "root", vec![])?;
    let topic = cb
        .wait(Duration::from_secs(10), "B: the topic", |cl| {
            let t = cl.topics(&g, 10).ok()?.into_iter().next()?;
            (!cl.messages(&t.topic.id, None, 10).ok()?.is_empty()).then_some(t.topic.id)
        })
        .await?;
    let base = cb.c.store().last_applied(&g)?;
    target.store(base + 1, Ordering::SeqCst);

    let m1 =
        ca.c.send_message(&g, &topic, "this one is dropped", vec![])?;
    cb.wait(
        Duration::from_secs(10),
        "B: the hook dropped seq base+1",
        |_| dropped.load(Ordering::SeqCst).then_some(()),
    )
    .await?;
    // Give B a moment: without a later event it cannot know about the gap.
    tokio::time::sleep(Duration::from_millis(300)).await;
    let has_m1 = cb.messages(&topic).iter().any(|m| m.client_msg_id == m1);
    check!(!has_m1, "B has the dropped message before any fetch");
    check!(
        cb.c.store().last_applied(&g)? == base,
        "B moved past the dropped event"
    );

    let t0 = Instant::now();
    let m2 =
        ca.c.send_message(&g, &topic, "this one shows the gap", vec![])?;
    let ms = cb
        .wait(
            Duration::from_secs(10),
            "B: both messages after the gap fetch",
            |cl| {
                let ms = cl.messages(&topic, None, 10).ok()?;
                (ms.len() == 3).then_some(ms)
            },
        )
        .await?;
    let heal_ms = t0.elapsed().as_secs_f64() * 1000.0;
    check!(
        ms[1].client_msg_id == m1 && ms[2].client_msg_id == m2,
        "B's order after the gap is wrong"
    );
    check!(
        cb.c.store().last_applied(&g)? == base + 2,
        "B's lastApplied is not base+2"
    );
    c.metric("gapHealMs", (heal_ms * 10.0).round() / 10.0);
    c.note(format!(
        "dropped seq {}, B fetched it when seq {} arrived",
        base + 1,
        base + 2
    ));
    ca.shutdown().await;
    cb.shutdown().await;
    Ok(())
}
