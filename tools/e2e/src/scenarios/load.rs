//! Load: 5 humans and 5 bots in 3 groups each send 100 messages at the same time. Every
//! member ends with the same ordered log as the server; delivery latency p50 and p95.

use super::{Ctx, Run, Scenario};
use crate::harness::{Account, TestClient, latency_metrics};
use anyhow::Result;
use std::time::{Duration, Instant};

pub const SCENARIO: Scenario = Scenario {
    name: "load",
    about: "5 humans + 5 bots in 3 groups send 100 messages each at once; identical logs; p50/p95",
    in_all: true,
    timeout_secs: 300,
    run: start,
};

fn start(c: &mut Ctx) -> Run<'_> {
    Box::pin(run(c))
}

const HUMANS: usize = 5;
const BOTS: usize = 5;
const GROUPS: usize = 3;
const PER_MEMBER: usize = 100;
/// Per sender. The server allows about 20 writes per second per member.
const RATE_PER_SEC: f64 = 15.0;

async fn run(c: &mut Ctx) -> Result<()> {
    let h = c.h.clone();
    let mut accs: Vec<Account> = Vec::new();
    for i in 0..HUMANS {
        accs.push(h.human(&format!("load-h{i}")).await?);
    }
    for i in 0..BOTS {
        accs.push(h.bot(&format!("load-b{i}")).await?);
    }
    let others: Vec<&Account> = accs[1..].iter().collect();
    let mut groups = Vec::new();
    for i in 0..GROUPS {
        groups.push(h.group(&accs[0], &format!("load-g{i}"), &others).await?);
    }
    let gids: Vec<&str> = groups.iter().map(String::as_str).collect();
    let clients: Vec<TestClient> = accs
        .iter()
        .enumerate()
        .map(|(i, a)| h.client(a, &format!("m{i}"), None))
        .collect::<Result<_>>()?;
    for cl in &clients {
        cl.ready(&gids).await?;
    }
    for g in &groups {
        clients[0]
            .c
            .start_topic(g, Some("load"), "load test topic", vec![])?;
    }
    let mut topics = Vec::new();
    for g in &groups {
        let t = clients[0]
            .wait(Duration::from_secs(20), "the load topic", |cl| {
                cl.topics(g, 10)
                    .ok()?
                    .into_iter()
                    .next()
                    .map(|t| t.topic.id)
            })
            .await?;
        topics.push(t);
    }
    for cl in &clients {
        for t in &topics {
            cl.wait(Duration::from_secs(20), "every load topic", |c| {
                (!c.messages(t, None, 1).ok()?.is_empty()).then_some(())
            })
            .await?;
        }
    }

    let t0 = Instant::now();
    let senders = clients.iter().enumerate().map(|(m, cl)| {
        let (groups, topics) = (&groups, &topics);
        async move {
            let gap = Duration::from_secs_f64(1.0 / RATE_PER_SEC);
            let start = Instant::now() + Duration::from_millis(fastrand::u64(0..50));
            let mut sent = Vec::with_capacity(PER_MEMBER);
            for i in 0..PER_MEMBER {
                tokio::time::sleep_until((start + gap * i as u32).into()).await;
                let k = (i + m) % GROUPS;
                let body = format!("load {m}/{i}");
                let id = cl.c.send_message(&groups[k], &topics[k], &body, vec![])?;
                sent.push((id, Instant::now()));
            }
            anyhow::Ok(sent)
        }
    });
    let sent: Vec<Vec<(String, Instant)>> = futures_util::future::try_join_all(senders).await?;
    let send_secs = t0.elapsed().as_secs_f64();
    for cl in &clients {
        cl.wait(Duration::from_secs(120), "every send acked", |c| {
            topics
                .iter()
                .all(|t| c.pending_sends(t).is_ok_and(|p| p.is_empty()))
                .then_some(())
        })
        .await?;
    }
    let acked_secs = t0.elapsed().as_secs_f64();

    let check =
        super::soak::consistent(&h, &accs[0], &groups, &clients, Duration::from_secs(60)).await?;
    let converged_secs = t0.elapsed().as_secs_f64();
    let total = HUMANS + BOTS;
    let want = total * PER_MEMBER + GROUPS;
    anyhow::ensure!(
        check.messages == want,
        "server has {} messages, want {want}",
        check.messages
    );

    let mut lat = Vec::new();
    for (m, cl) in clients.iter().enumerate() {
        let arr = cl.arrivals.lock().unwrap();
        for (s, list) in sent.iter().enumerate() {
            if s == m {
                continue;
            }
            for (id, at) in list {
                let got = arr
                    .get(id)
                    .ok_or_else(|| anyhow::anyhow!("{}: no arrival for a message", cl.label))?;
                lat.push(got.saturating_duration_since(*at).as_secs_f64() * 1000.0);
            }
        }
    }
    c.metric("members", total as u64);
    c.metric("groups", GROUPS as u64);
    c.metric("messagesSent", (total * PER_MEMBER) as u64);
    c.metric("sendSeconds", (send_secs * 100.0).round() / 100.0);
    c.metric("allAckedSeconds", (acked_secs * 100.0).round() / 100.0);
    c.metric("convergedSeconds", (converged_secs * 100.0).round() / 100.0);
    c.metric("deliveryLatency", latency_metrics(lat));
    c.note(format!(
        "{} members x {} messages over {} groups; {} logs identical to the server",
        total,
        PER_MEMBER,
        GROUPS,
        clients.len() * GROUPS
    ));
    for cl in clients {
        cl.shutdown().await;
    }
    Ok(())
}
