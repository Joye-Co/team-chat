//! Soak: members chat at a low rate for `--minutes`, with random connection drops and app
//! restarts (with messages queued while stopped). At the end every member's cache must equal
//! the server log and every sent message must be in it exactly once.

use super::{Ctx, Run, Scenario};
use crate::check;
use crate::harness::{Account, Harness, TestClient, latency_metrics};
use crate::raw::{RawWs, created};
use anyhow::Result;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::time::{Duration, Instant};

pub const SCENARIO: Scenario = Scenario {
    name: "soak",
    about: "low-rate chat with random disconnects for --minutes; consistency at the end",
    in_all: false,
    timeout_secs: 240,
    run: start,
};

fn start(c: &mut Ctx) -> Run<'_> {
    Box::pin(run(c))
}

pub struct Consistency {
    /// `message.created` events over all groups.
    pub messages: usize,
    /// clientMsgId -> authorId of every stored message.
    pub authors: HashMap<String, String>,
}

/// Waits until every client has applied each group's whole log, then checks that each
/// client's topics and messages equal the server's, in order.
pub async fn consistent(
    h: &Harness,
    reader: &Account,
    groups: &[String],
    clients: &[TestClient],
    timeout: Duration,
) -> Result<Consistency> {
    let mut raw = RawWs::connect(&h.base, reader.token()).await?;
    let mut out = Consistency {
        messages: 0,
        authors: HashMap::new(),
    };
    for g in groups {
        let (events, last) = raw.fetch_all(g).await?;
        let mut topics: BTreeMap<String, Vec<String>> = BTreeMap::new();
        for e in events.iter().filter(|e| e["type"] == "topic.created") {
            topics
                .entry(e["data"]["topic"]["id"].as_str().unwrap_or_default().into())
                .or_default();
        }
        for m in created(&events) {
            let topic = m["topicId"].as_str().unwrap_or_default().to_string();
            topics
                .entry(topic)
                .or_default()
                .push(m["id"].as_str().unwrap_or_default().into());
            let key = format!(
                "{}/{}",
                m["authorId"].as_str().unwrap_or_default(),
                m["clientMsgId"].as_str().unwrap_or_default()
            );
            check!(
                out.authors
                    .insert(
                        m["clientMsgId"].as_str().unwrap_or_default().into(),
                        m["authorId"].as_str().unwrap_or_default().into()
                    )
                    .is_none(),
                "clientMsgId stored twice: {key}"
            );
            out.messages += 1;
        }
        for cl in clients {
            cl.wait(timeout, "the whole group log", |c| {
                (c.store().last_applied(g).ok()? >= last).then_some(())
            })
            .await?;
            check!(
                cl.c.store().last_applied(g)? == last,
                "{}: lastApplied beyond the server's",
                cl.label
            );
            let mine: HashSet<String> =
                cl.c.topics(g, 100_000)?
                    .into_iter()
                    .map(|t| t.topic.id)
                    .collect();
            let want: HashSet<String> = topics.keys().cloned().collect();
            check!(
                mine == want,
                "{}: topics differ from the server's",
                cl.label
            );
            for (t, ids) in &topics {
                let got: Vec<String> = cl.messages(t).into_iter().map(|m| m.id).collect();
                check!(
                    &got == ids,
                    "{}: messages of a topic differ from the server's ({} vs {})",
                    cl.label,
                    got.len(),
                    ids.len()
                );
            }
        }
    }
    Ok(out)
}

struct Slot {
    cl: Option<TestClient>,
    dir: String,
    offline_until: Option<Instant>,
    arrivals: HashMap<String, Instant>,
}

impl Slot {
    fn collect(&mut self) {
        if let Some(cl) = &self.cl {
            for (k, v) in cl.arrivals.lock().unwrap().iter() {
                self.arrivals.entry(k.clone()).or_insert(*v);
            }
        }
    }
}

async fn run(c: &mut Ctx) -> Result<()> {
    let h = c.h.clone();
    let mut accs = Vec::new();
    for i in 0..3 {
        accs.push(h.human(&format!("soak-h{i}")).await?);
    }
    for i in 0..2 {
        accs.push(h.bot(&format!("soak-b{i}")).await?);
    }
    let others: Vec<&Account> = accs[1..].iter().collect();
    let groups = vec![
        h.group(&accs[0], "soak-g0", &others).await?,
        h.group(&accs[0], "soak-g1", &others).await?,
    ];
    let gids: Vec<&str> = groups.iter().map(String::as_str).collect();
    let mut slots = Vec::new();
    for (i, a) in accs.iter().enumerate() {
        let dir = format!("m{i}");
        let cl = h.client(a, &dir, None)?;
        cl.ready(&gids).await?;
        slots.push(Slot {
            cl: Some(cl),
            dir,
            offline_until: None,
            arrivals: HashMap::new(),
        });
    }
    let first = slots[0].cl.as_ref().expect("online");
    for (g, title) in [
        (&groups[0], "soak one"),
        (&groups[0], "soak two"),
        (&groups[1], "soak three"),
    ] {
        first.c.start_topic(g, Some(title), "soak topic", vec![])?;
    }
    let mut topics: Vec<(String, String)> = Vec::new();
    for g in &groups {
        let n = if g == &groups[0] { 2 } else { 1 };
        let ts = first
            .wait(Duration::from_secs(20), "soak topics", |cl| {
                let ts = cl.topics(g, 10).ok()?;
                (ts.len() == n).then_some(ts)
            })
            .await?;
        topics.extend(ts.into_iter().map(|t| (g.clone(), t.topic.id)));
    }

    let deadline = Instant::now() + Duration::from_secs_f64(c.minutes * 60.0);
    let mut sent: Vec<(String, usize, Instant)> = Vec::new();
    let (mut blips, mut restarts, mut queued_offline) = (0u64, 0u64, 0u64);
    let mut tick = tokio::time::interval(Duration::from_millis(250));
    while Instant::now() < deadline {
        tick.tick().await;
        for (i, s) in slots.iter_mut().enumerate() {
            if s.offline_until.is_some_and(|t| Instant::now() >= t) {
                s.collect();
                if let Some(old) = s.cl.take() {
                    old.shutdown().await;
                }
                s.cl = Some(h.client(&accs[i], &s.dir, None)?);
                s.offline_until = None;
            }
            let Some(cl) = &s.cl else { continue };
            // About one message per 2 s per member.
            if fastrand::f64() < 0.125 {
                let (g, t) = &topics[fastrand::usize(..topics.len())];
                let id =
                    cl.c.send_message(g, t, &format!("soak {i} {}", sent.len()), vec![])?;
                sent.push((id, i, Instant::now()));
                if s.offline_until.is_some() {
                    queued_offline += 1;
                }
            }
            // About one drop per 40 s per member: a network blip or an app restart.
            if s.offline_until.is_none() && fastrand::f64() < 0.00625 {
                if fastrand::bool() {
                    cl.c.reconnect_now();
                    blips += 1;
                } else {
                    cl.c.stop();
                    s.offline_until =
                        Some(Instant::now() + Duration::from_millis(fastrand::u64(1000..5000)));
                    restarts += 1;
                }
            }
        }
    }
    for (i, s) in slots.iter_mut().enumerate() {
        if s.offline_until.take().is_some() {
            s.collect();
            if let Some(old) = s.cl.take() {
                old.shutdown().await;
            }
            s.cl = Some(h.client(&accs[i], &s.dir, None)?);
        }
    }
    let clients: Vec<TestClient> = slots
        .iter_mut()
        .map(|s| s.cl.take().expect("online"))
        .collect();
    for cl in &clients {
        cl.wait(Duration::from_secs(60), "every send acked", |c| {
            topics
                .iter()
                .all(|(_, t)| c.pending_sends(t).is_ok_and(|p| p.is_empty()))
                .then_some(())
        })
        .await?;
    }
    let res = consistent(&h, &accs[0], &groups, &clients, Duration::from_secs(60)).await?;
    for (id, who, _) in &sent {
        check!(
            res.authors.get(id) == Some(&accs[*who].id),
            "a sent message is missing or has another author"
        );
    }
    check!(
        res.messages == sent.len() + topics.len(),
        "server has {} messages, want {}",
        res.messages,
        sent.len() + topics.len()
    );

    for (s, cl) in slots.iter_mut().zip(&clients) {
        for (k, v) in cl.arrivals.lock().unwrap().iter() {
            s.arrivals.entry(k.clone()).or_insert(*v);
        }
    }
    let mut lat = Vec::new();
    for (m, s) in slots.iter().enumerate() {
        for (id, who, at) in &sent {
            if *who != m
                && let Some(got) = s.arrivals.get(id)
            {
                lat.push(got.saturating_duration_since(*at).as_secs_f64() * 1000.0);
            }
        }
    }
    c.metric("minutes", c.minutes);
    c.metric("members", accs.len() as u64);
    c.metric("messagesSent", sent.len() as u64);
    c.metric("queuedWhileStopped", queued_offline);
    c.metric("networkBlips", blips);
    c.metric("restarts", restarts);
    c.metric("deliveryLatencyInclOffline", latency_metrics(lat));
    c.note(format!(
        "{} messages, {blips} blips, {restarts} restarts; all {} caches equal the server log",
        sent.len(),
        clients.len()
    ));
    for cl in clients {
        cl.shutdown().await;
    }
    Ok(())
}
