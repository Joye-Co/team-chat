//! 1.1 runs end to end with a raw machine socket (docs/machine-protocol.md "1.1 features"):
//! a mention starts a run that is acknowledged with a reaction; the requester pauses it
//! (接管) and resumes it (交还) with a note; the bot posts a Card v2 ask and the requester's
//! answer reaches the run as a typed `run.input`; the run ends with a done card carrying proof
//! and the answer is listed in 群里的决定; a side chat (单独问) runs read-only in the asker's
//! DM and posts nothing in the source topic.

use super::{Ctx, Run, Scenario};
use crate::check;
use crate::raw::{RawWs, created};
use anyhow::{Context as _, Result, anyhow};
use chat_core::proto::{CreateMachineRequest, SetBotRunnerRequest};
use reqwest::Method;
use serde_json::{Value, json};
use std::time::Duration;

pub const SCENARIO: Scenario = Scenario {
    name: "runs-v11",
    about: "1.1 runs: reaction ack, pause/resume, card answer to the run, done card, decisions, side chat",
    in_all: true,
    timeout_secs: 120,
    run: start,
};

const WAIT: Duration = Duration::from_secs(20);

fn start(c: &mut Ctx) -> Run<'_> {
    Box::pin(run(c))
}

fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// A raw machine with the 1.1 features and its own run seqs.
struct Machine {
    ws: RawWs,
    seq: u64,
}

impl Machine {
    async fn frame(&mut self, what: &str, pred: impl FnMut(&Value) -> bool) -> Result<Value> {
        self.ws
            .next_matching(WAIT, pred)
            .await
            .with_context(|| format!("machine: {what}"))
    }
    async fn done(&mut self, run_id: &str, extra: Value) -> Result<()> {
        self.seq += 1;
        let mut f = json!({ "t": "run.done", "runId": run_id, "seq": self.seq, "at": now(), "durationMs": 1000 });
        if let (Some(f), Some(extra)) = (f.as_object_mut(), extra.as_object()) {
            f.extend(extra.clone());
        }
        self.ws.send(f).await?;
        let seq = self.seq;
        self.frame("ack of run.done", |f| {
            f["t"] == "ack" && f["runId"] == run_id && f["seq"].as_u64().is_some_and(|s| s >= seq)
        })
        .await?;
        Ok(())
    }
}

/// The first stored bot message of a topic that `pred` accepts, waiting for it.
async fn bot_message(
    ws: &mut RawWs,
    group: &str,
    topic: &str,
    bot: &str,
    what: &str,
    pred: impl Fn(&Value) -> bool,
) -> Result<Value> {
    let deadline = std::time::Instant::now() + WAIT;
    loop {
        let (events, _) = ws.fetch_all(group).await?;
        if let Some(m) = created(&events)
            .into_iter()
            .find(|m| m["topicId"] == topic && m["authorId"] == bot && pred(m))
        {
            return Ok(m.clone());
        }
        check!(
            std::time::Instant::now() < deadline,
            "timed out waiting for {what}"
        );
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
}

async fn run(c: &mut Ctx) -> Result<()> {
    let h = c.h.clone();
    let a = h.human("v11-a").await?;
    let bot = h.bot("v11-bot").await?;
    let g = h.group(&a, "v11", &[&bot]).await?;
    let label = format!("e2e-{}", h.run_id.to_lowercase());
    let label: String = label.chars().take(32).collect();

    // A runner bot on a machine of its own.
    let owner = h.owner_http();
    let runner: SetBotRunnerRequest = serde_json::from_value(json!({
        "runtime": "codex", "labels": [label], "repo": { "path": "/tmp/repo" }, "baseBranch": "main"
    }))?;
    owner
        .set_bot_runner(&bot.id, &runner)
        .await
        .context("set runner")?;
    let made = owner
        .create_machine(&CreateMachineRequest {
            name: h.name("mac"),
            labels: Some(vec![label.clone()]),
            personal: None,
            owner_id: None,
        })
        .await
        .context("create machine")?;
    let machine_id = made.machine.id.clone();
    let result = run_with_machine(c, &h, &a, &bot, &g, &made.token).await;
    if let Err(e) = owner.delete_machine(&machine_id).await {
        c.note(format!("machine not deleted: {e}"));
    }
    result
}

async fn run_with_machine(
    c: &mut Ctx,
    h: &crate::harness::Harness,
    a: &crate::harness::Account,
    bot: &crate::harness::Account,
    g: &str,
    machine_token: &str,
) -> Result<()> {
    let hello = json!({
        "t": "hello", "protocol": 1,
        "machine": { "name": "e2e-mac", "labels": [], "daemonVersion": "1.1.0", "os": "macos", "arch": "aarch64",
                     "maxParallelRuns": 2, "features": ["pause", "answer", "aside"] },
        "clis": [{ "runtime": "codex", "kind": "codex", "command": "codex", "version": "0.159.0", "available": true }],
        "health": { "diskFreeBytes": 100e9, "diskTotalBytes": 500e9, "load1": 0.5, "load5": 0.5, "load15": 0.5, "cpuCount": 8, "activeRuns": 0 },
        "runs": []
    });
    let mut m = Machine {
        ws: RawWs::machine(&h.base, machine_token, hello).await?,
        seq: 0,
    };
    let mut ra = RawWs::connect(&h.base, a.token()).await?;
    let mut rb = RawWs::connect(&h.base, bot.token()).await?;

    // 1. A mention starts a run, acknowledged with a reaction (no text line).
    let sent = ra
        .request(
            json!({ "t": "send", "groupId": g, "clientMsgId": chat_core::new_id(),
                         "body": "@bot 修一下重连测试", "mentions": [bot.id] }),
        )
        .await?;
    check!(sent["ok"] == true, "mention refused: {sent}");
    let topic = sent["result"]["topic"]["id"]
        .as_str()
        .unwrap_or_default()
        .to_string();
    let trigger = sent["result"]["message"]["id"]
        .as_str()
        .unwrap_or_default()
        .to_string();
    let assign = m
        .frame("run.assign", |f| {
            f["t"] == "run.assign" && f["topicId"] == topic.as_str()
        })
        .await?;
    let run_id = assign["runId"]
        .as_str()
        .ok_or_else(|| anyhow!("run.assign without runId"))?
        .to_string();
    check!(
        assign.get("kind").is_none_or(Value::is_null),
        "a normal run has no kind: {}",
        assign["kind"]
    );
    m.ws.send(json!({ "t": "run.accept", "runId": run_id }))
        .await?;
    let deadline = std::time::Instant::now() + WAIT;
    loop {
        let (events, _) = ra.fetch_all(g).await?;
        if events.iter().any(|e| {
            e["type"] == "reaction.changed"
                && e["data"]["messageId"] == trigger.as_str()
                && e["actorId"] == bot.id.as_str()
        }) {
            break;
        }
        check!(
            std::time::Instant::now() < deadline,
            "no reaction ack on the request"
        );
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
    c.note("run acknowledged with a reaction");

    // 2. 接管 and 交还: the requester pauses and resumes; the machine gets both frames.
    let (st, paused) = h
        .api(
            Method::POST,
            &format!("/v1/runs/{run_id}/pause"),
            Some(a.token()),
            Some(json!({})),
        )
        .await?;
    check!(st == 200, "pause: {st} {paused}");
    check!(
        paused["pausedBy"] == a.id.as_str(),
        "pausedBy not set: {paused}"
    );
    m.frame("run.pause", |f| {
        f["t"] == "run.pause" && f["runId"] == run_id.as_str()
    })
    .await?;
    let (st, again) = h
        .api(
            Method::POST,
            &format!("/v1/runs/{run_id}/pause"),
            Some(a.token()),
            Some(json!({})),
        )
        .await?;
    check!(st == 409, "a second pause must conflict: {st} {again}");
    let (st, resumed) = h
        .api(
            Method::POST,
            &format!("/v1/runs/{run_id}/resume"),
            Some(a.token()),
            Some(json!({ "note": "继续，先跑测试" })),
        )
        .await?;
    check!(st == 200, "resume: {st} {resumed}");
    check!(
        resumed.get("pausedBy").is_none_or(Value::is_null),
        "pausedBy not cleared: {resumed}"
    );
    let rf = m
        .frame("run.resume", |f| {
            f["t"] == "run.resume" && f["runId"] == run_id.as_str()
        })
        .await?;
    check!(rf["note"] == "继续，先跑测试", "resume note lost: {rf}");
    c.note("pause and resume reached the machine");

    // 3. A Card v2 ask from the bot; A's answer reaches the active run as a typed run.input.
    let card = json!({
        "v": 2, "rev": 1, "intent": "ask", "title": "先修哪个？", "fallback": "先修哪个？",
        "ask": { "from": [a.id], "record": true },
        "blocks": [{ "type": "decision", "id": "which", "options": [
            { "id": "o1", "label": "重连" }, { "id": "o2", "label": "去重" } ] }],
        "actions": [{ "id": "submit", "label": "确定", "style": "primary" }]
    });
    let posted = rb
        .request(
            json!({ "t": "send", "groupId": g, "topicId": topic, "clientMsgId": chat_core::new_id(),
                         "body": "先修哪个？", "cardV2": card }),
        )
        .await?;
    check!(posted["ok"] == true, "card send refused: {posted}");
    let card_id = posted["result"]["message"]["id"]
        .as_str()
        .unwrap_or_default()
        .to_string();
    check!(
        posted["result"]["message"]["card"]["title"] == "先修哪个？",
        "no derived v1 card: {posted}"
    );
    let answered = ra
        .request(
            json!({ "t": "card.answer", "groupId": g, "messageId": card_id, "rev": 1,
                         "actionId": "submit", "inputs": { "which": "o2" } }),
        )
        .await?;
    check!(answered["ok"] == true, "card answer refused: {answered}");
    // A 1.1.0 daemon takes the typed `run.answer` (machine-protocol "Interaction loop");
    // an older one gets `run.input` with `answer` and the text fallback.
    let input = m
        .frame("run.answer or run.input with the answer", |f| {
            (f["t"] == "run.answer" || f["t"] == "run.input") && f["runId"] == run_id.as_str()
        })
        .await?;
    if input["t"] == "run.answer" {
        check!(
            input["source"]["kind"] == "card" && input["source"]["messageId"] == card_id.as_str(),
            "run.answer without the card: {input}"
        );
        check!(input["actionId"] == "submit", "wrong action: {input}");
        check!(input["inputs"]["which"] == "o2", "wrong inputs: {input}");
        check!(
            input["actor"]["id"] == a.id.as_str(),
            "run.answer without the answerer: {input}"
        );
    } else {
        check!(
            input["answer"]["messageId"] == card_id.as_str(),
            "run.input without the answer: {input}"
        );
        check!(
            input["answer"]["actionId"] == "submit",
            "wrong action: {input}"
        );
        check!(
            input["answer"]["inputs"]["which"] == "o2",
            "wrong inputs: {input}"
        );
        check!(
            input["text"].as_str().is_some_and(|t| t.contains("去重")),
            "text fallback lacks the choice: {input}"
        );
    }
    let twice = ra
        .request(
            json!({ "t": "card.answer", "groupId": g, "messageId": card_id, "rev": 1,
                         "actionId": "submit", "inputs": { "which": "o1" } }),
        )
        .await?;
    check!(
        twice["ok"] != true,
        "a second answer to the same rev must be refused: {twice}"
    );
    c.note("card answer reached the run as a typed answer");

    // 4. The run ends: one done card with proof; usage and duration are kept.
    let result = "修好了：重连后不再重复推送。\ntest result: ok. 12 passed; 0 failed";
    m.done(
        &run_id,
        json!({ "status": "succeeded", "exitCode": 0, "resultText": result, "summary": "修好了",
                            "usage": { "inputTokens": 100, "outputTokens": 20 } }),
    )
    .await?;
    let done = bot_message(&mut ra, g, &topic, &bot.id, "done card", |m| {
        m["cardV2"]["intent"] == "done"
    })
    .await?;
    check!(done["body"] == result, "done card body: {}", done["body"]);
    let blocks = done["cardV2"]["blocks"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    check!(
        blocks
            .iter()
            .any(|b| b["type"] == "evidence" && b["id"] == "tests" && b["state"] == "pass"),
        "done card without test proof: {blocks:?}"
    );
    let (st, run) = h
        .api(
            Method::GET,
            &format!("/v1/runs/{run_id}"),
            Some(a.token()),
            None,
        )
        .await?;
    check!(st == 200, "run: {st}");
    check!(
        run["resultMessageId"] == done["id"],
        "resultMessageId: {run}"
    );
    check!(run["durationMs"] == 1000, "durationMs: {run}");
    check!(run["usage"]["inputTokens"] == 100, "usage: {run}");

    // 5. 群里的决定 lists the answer as it was asked.
    let (st, decisions) = h
        .api(
            Method::GET,
            &format!("/v1/groups/{g}/decisions"),
            Some(a.token()),
            None,
        )
        .await?;
    check!(st == 200, "decisions: {st} {decisions}");
    let list = decisions["decisions"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    check!(
        list.iter()
            .any(|d| d["messageId"] == card_id.as_str() && d["choices"] == json!(["去重"])),
        "decision not listed: {decisions}"
    );
    c.metric("decisions", list.len());

    // 6. 单独问: a read-only side chat in A's DM with the bot, about the topic.
    let (_, last_seq) = ra.fetch_all(g).await?;
    let (st, dm) = h
        .api(
            Method::POST,
            "/v1/dms",
            Some(a.token()),
            Some(json!({ "memberId": bot.id })),
        )
        .await?;
    check!(st == 200, "dm: {st} {dm}");
    let dm_id = dm["group"]["id"].as_str().unwrap_or_default().to_string();
    let side = ra
        .request(json!({ "t": "send", "groupId": dm_id, "clientMsgId": chat_core::new_id(), "body": "现在卡在哪？",
                         "context": { "groupId": g, "topicId": topic, "uptoSeq": last_seq } }))
        .await?;
    check!(side["ok"] == true, "side chat refused: {side}");
    let side_topic = side["result"]["topic"]["id"]
        .as_str()
        .unwrap_or_default()
        .to_string();
    let aside = m
        .frame("aside run.assign", |f| {
            f["t"] == "run.assign" && f["topicId"] == side_topic.as_str()
        })
        .await?;
    check!(
        aside["kind"] == "aside",
        "side chat run kind: {}",
        aside["kind"]
    );
    check!(
        aside["prompt"]
            .as_str()
            .is_some_and(|p| p.contains("修一下重连测试") && p.contains("现在卡在哪？")),
        "side chat prompt lacks the topic or the question"
    );
    let aside_id = aside["runId"].as_str().unwrap_or_default().to_string();
    m.ws.send(json!({ "t": "run.accept", "runId": aside_id }))
        .await?;
    m.done(
        &aside_id,
        json!({ "status": "succeeded", "resultText": "已经修好了，在等你审。" }),
    )
    .await?;
    let answer = bot_message(
        &mut ra,
        &dm_id,
        &side_topic,
        &bot.id,
        "side chat answer",
        |m| m["body"].as_str().is_some_and(|b| b.contains("在等你审")),
    )
    .await?;
    check!(
        answer.get("cardV2").is_none_or(Value::is_null),
        "a side chat answer is plain text"
    );
    let (events, _) = ra.fetch_all(g).await?;
    let after: Vec<_> = events
        .iter()
        .filter(|e| {
            e["seq"].as_u64().is_some_and(|s| s > last_seq) && e["type"] == "message.created"
        })
        .collect();
    check!(
        after.is_empty(),
        "the side chat posted into the source group: {after:?}"
    );
    c.note("side chat answered in the DM only");
    Ok(())
}
