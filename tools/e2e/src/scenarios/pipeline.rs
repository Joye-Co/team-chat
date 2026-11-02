//! Pipelined writes: 50 sends on one raw socket without waiting for acks. Only the write rate
//! limit may refuse one (refused sends are sent again, pipelined, after a pause). The other
//! member gets all 50 once, and the log keeps the send order of the accepted sends. A deployed server once refused every send after about
//! 8 with `internal` ("Subrequest depth limit exceeded"); `wrangler dev` does not enforce that
//! limit, so run this against a deployed env.

use super::{Ctx, Run, Scenario};
use crate::check;
use crate::raw::{RawWs, created};
use anyhow::Result;
use serde_json::json;
use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant};

pub const SCENARIO: Scenario = Scenario {
    name: "pipeline",
    about: "50 pipelined sends on one socket: no internal errors, each delivered once in order",
    in_all: true,
    timeout_secs: 120,
    run: start,
};

const N: usize = 50;

fn start(c: &mut Ctx) -> Run<'_> {
    Box::pin(run(c))
}

async fn run(c: &mut Ctx) -> Result<()> {
    let h = c.h.clone();
    let a = h.human("pipe-a").await?;
    let b = h.human("pipe-b").await?;
    let g = h.group(&a, "pipe", &[&b]).await?;
    let mut ra = RawWs::connect(&h.base, a.token()).await?;
    let mut rb = RawWs::connect(&h.base, b.token()).await?;
    let root = ra
        .request(json!({ "t": "send", "groupId": g, "clientMsgId": chat_core::new_id(), "body": "pipeline" }))
        .await?;
    check!(root["ok"] == true, "root send refused: {root}");
    let topic = root["result"]["topic"]["id"].as_str().unwrap_or_default().to_string();

    let ids: Vec<String> = (0..N).map(|_| chat_core::new_id()).collect();
    let mut todo: Vec<usize> = (0..N).collect();
    let mut rounds = 0;
    let mut rate_limited = 0;
    let started = Instant::now();
    let mut first_round_ok = 0;
    // (round, index) of each accepted send: the stored order must follow it.
    let mut accepted: HashMap<String, (usize, usize)> = HashMap::new();
    while !todo.is_empty() {
        rounds += 1;
        check!(rounds <= 10, "{} sends still refused after 10 rounds", todo.len());
        let mut rids = HashMap::new();
        for &i in &todo {
            let rid = ra.rid();
            ra.send(json!({ "t": "send", "rid": rid, "groupId": g, "topicId": topic, "clientMsgId": ids[i], "body": format!("p{i}") }))
                .await?;
            rids.insert(rid, i);
        }
        let mut refused = Vec::new();
        for _ in 0..rids.len() {
            let ack = ra
                .next_matching(Duration::from_secs(30), |f| {
                    f["t"] == "ack" && f["rid"].as_str().is_some_and(|r| rids.contains_key(r))
                })
                .await?;
            let i = rids[ack["rid"].as_str().unwrap_or_default()];
            if ack["ok"] == true {
                accepted.insert(ids[i].clone(), (rounds, i));
                if rounds == 1 {
                    first_round_ok += 1;
                }
                continue;
            }
            let code = ack["error"]["code"].as_str().unwrap_or_default();
            check!(code == "rate_limited", "pipelined send {i} refused with {code}: {ack}");
            rate_limited += 1;
            refused.push(i);
        }
        refused.sort_unstable();
        todo = refused;
        if !todo.is_empty() {
            tokio::time::sleep(Duration::from_millis(1100)).await;
        }
    }
    let acked_ms = started.elapsed().as_millis() as u64;

    // B receives every one live, once.
    let want: HashSet<&str> = ids.iter().map(String::as_str).collect();
    let mut seen = HashSet::new();
    while seen.len() < N {
        let f = rb
            .next_matching(Duration::from_secs(30), |f| {
                f["t"] == "event"
                    && f["event"]["type"] == "message.created"
                    && f["event"]["data"]["message"]["clientMsgId"]
                        .as_str()
                        .is_some_and(|m| want.contains(m))
            })
            .await?;
        let m = f["event"]["data"]["message"]["clientMsgId"].as_str().unwrap_or_default().to_string();
        check!(seen.insert(m.clone()), "B got {m} twice live");
    }
    let (events, _) = rb.fetch_all(&g).await?;
    let stored: Vec<&str> = created(&events)
        .iter()
        .filter_map(|m| m["clientMsgId"].as_str())
        .filter(|m| want.contains(m))
        .collect();
    check!(stored.len() == N, "{} of {N} stored", stored.len());
    let order: Vec<(usize, usize)> = stored.iter().filter_map(|m| accepted.get(*m).copied()).collect();
    check!(
        order.windows(2).all(|w| w[0] < w[1]),
        "stored order differs from the send order: {order:?}"
    );
    c.metric("firstRoundOk", first_round_ok as u64);
    c.metric("rateLimited", rate_limited as u64);
    c.metric("rounds", rounds as u64);
    c.metric("allAckedMs", acked_ms);
    c.note(format!(
        "{N} pipelined sends: {first_round_ok} ok in the first round, {rate_limited} rate limited and resent, all acked in {acked_ms} ms; B got each once"
    ));
    Ok(())
}
