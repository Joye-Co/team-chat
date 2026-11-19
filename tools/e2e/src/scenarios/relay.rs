//! A relay (按顺序做, usable design-relay R3) with a raw machine socket standing in for the CLIs:
//! a person writes `@dev 写 → @qa 审 → @dev 改` in a topic; only dev starts; when its run is done
//! the room wakes qa with dev's result and code location; qa fails, the relay stops there and the
//! topic waits; the person retries the step and qa finishes; dev starts the last step and the
//! person stops the relay, which stops dev's run.

use super::{Ctx, Run, Scenario};
use crate::check;
use crate::raw::RawWs;
use anyhow::{Context as _, Result, anyhow};
use chat_core::proto::{CreateMachineRequest, SetBotRunnerRequest};
use reqwest::Method;
use serde_json::{Value, json};
use std::time::Duration;

pub const SCENARIO: Scenario = Scenario {
    name: "relay",
    about: "A relay of three steps: in order, a failure stops it, a retry, a stop",
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

async fn run(c: &mut Ctx) -> Result<()> {
    let h = c.h.clone();
    let a = h.human("relay-a").await?;
    let dev = h.bot("dev").await?;
    let qa = h.bot("qa").await?;
    let g = h.group(&a, "relay", &[&dev, &qa]).await?;
    let label: String = format!("e2e-relay-{}", h.run_id.to_lowercase())
        .chars()
        .take(32)
        .collect();
    let owner = h.owner_http();
    let made = owner
        .create_machine(&CreateMachineRequest {
            name: h.name("relay-mac"),
            labels: Some(vec![label.clone()]),
            personal: None,
            owner_id: None,
        })
        .await
        .context("create machine")?;
    let machine_id = made.machine.id.clone();
    let result = async {
        for bot in [&dev, &qa] {
            let runner: SetBotRunnerRequest = serde_json::from_value(json!({
                "runtime": "claude", "labels": [label], "repo": { "path": "/tmp/repo" }, "baseBranch": "main"
            }))?;
            owner
                .set_bot_runner(&bot.id, &runner)
                .await
                .context("set runner")?;
        }
        let hello = json!({
            "t": "hello", "protocol": 1,
            "machine": { "name": "e2e-relay", "labels": [], "daemonVersion": "1.1.0", "os": "linux", "arch": "x86_64",
                         "maxParallelRuns": 4 },
            "clis": [{ "runtime": "claude", "kind": "claude", "command": "claude", "available": true }],
            "health": { "diskFreeBytes": 100e9, "diskTotalBytes": 500e9, "load1": 0.5, "load5": 0.5, "load15": 0.5, "cpuCount": 8, "activeRuns": 0 },
            "runs": []
        });
        let mut m = RawWs::machine(&h.base, &made.token, hello).await?;
        let mut ra = RawWs::connect(&h.base, a.token()).await?;
        let mut seq = 0u64;

        let body = format!(
            "@{} 写 → @{} 审 → @{} 按意见改\n给 README 加一节安装说明",
            dev.name, qa.name, dev.name
        );
        let sent = ra
            .request(json!({ "t": "send", "groupId": g, "clientMsgId": chat_core::new_id(),
                             "body": body, "mentions": [qa.id, dev.id] }))
            .await?;
        check!(sent["ok"] == true, "arrow message refused: {sent}");
        let topic = sent["result"]["message"]["topicId"]
            .as_str()
            .unwrap_or_default()
            .to_string();

        // Step 1: only dev starts.
        let s1 = assign(&mut m, &dev.id, &[]).await?;
        check!(s1["topicId"] == topic.as_str(), "step 1 in another topic: {s1}");
        check!(
            prompt(&s1).contains("step 1 of 3"),
            "step 1 prompt: {}",
            prompt(&s1)
        );
        let code = json!({ "branch": "readme-install", "head": "abcdef1", "pushed": true });
        finish(&mut m, &mut seq, &s1, "succeeded", "写好了：README 加了安装一节", Some(code)).await?;

        // Step 2: qa gets dev's result and code; it fails, and the relay stops there.
        let s2 = assign(&mut m, &qa.id, &[]).await?;
        check!(
            prompt(&s2).contains("写好了：README 加了安装一节") && prompt(&s2).contains("readme-install"),
            "step 2 prompt lacks step 1: {}",
            prompt(&s2)
        );
        finish(&mut m, &mut seq, &s2, "failed", "", None).await?;
        let paused = relay_event(&mut ra, &topic, |r| r["state"] == "paused").await?;
        check!(
            paused["relay"]["reason"] == "step_failed" && paused["status"] == "blocked",
            "after the failure: {paused}"
        );
        let relay_id = paused["relay"]["id"].as_str().unwrap_or_default().to_string();
        c.note("step 2 failed: the relay paused at qa, the topic is blocked");

        // Retry: qa does step 2 again and finishes.
        let (st, r) = h
            .api(Method::POST, &format!("/v1/groups/{g}/relays/{relay_id}/retry"), Some(a.token()), None)
            .await?;
        check!(st == 200 && r["state"] == "running", "retry: {st} {r}");
        let s2b = assign(&mut m, &qa.id, &[s2["runId"].as_str().unwrap_or_default()]).await?;
        finish(&mut m, &mut seq, &s2b, "succeeded", "审过了：缺 Windows 的步骤", None).await?;

        // Step 3: dev again, with qa's review; the person stops it.
        let s3 = assign(&mut m, &dev.id, &[s1["runId"].as_str().unwrap_or_default()]).await?;
        check!(
            prompt(&s3).contains("step 3 of 3") && prompt(&s3).contains("缺 Windows 的步骤"),
            "step 3 prompt: {}",
            prompt(&s3)
        );
        m.send(json!({ "t": "run.accept", "runId": s3["runId"] })).await?;
        let (st, r) = h
            .api(Method::POST, &format!("/v1/groups/{g}/relays/{relay_id}/stop"), Some(a.token()), None)
            .await?;
        check!(st == 200 && r["state"] == "stopped", "stop: {st} {r}");
        let run3 = s3["runId"].as_str().unwrap_or_default().to_string();
        m.next_matching(WAIT, |f| f["t"] == "run.cancel" && f["runId"] == run3.as_str())
            .await
            .context("machine: run.cancel of step 3")?;
        let stopped = relay_event(&mut ra, &topic, |r| r["state"] == "stopped").await?;
        check!(
            stopped["relay"]["steps"][2]["state"] == "stopped" && stopped["relay"]["stoppedBy"] == a.id.as_str(),
            "after the stop: {stopped}"
        );
        c.note("three steps in order, a failure, a retry and a stop");
        Ok(())
    }
    .await;
    if let Err(e) = owner.delete_machine(&machine_id).await {
        c.note(format!("machine not deleted: {e}"));
    }
    result
}

fn prompt(a: &Value) -> &str {
    a["prompt"].as_str().unwrap_or_default()
}

/// The next `run.assign` for `bot` other than the runs in `not`.
async fn assign(m: &mut RawWs, bot: &str, not: &[&str]) -> Result<Value> {
    let not: Vec<String> = not.iter().map(|s| s.to_string()).collect();
    m.next_matching(WAIT, |f| {
        f["t"] == "run.assign"
            && f["botMemberId"] == bot
            && !not.contains(&f["runId"].as_str().unwrap_or_default().to_string())
    })
    .await
    .context("machine: run.assign")
}

/// The next `topic.updated` of the topic whose relay matches.
async fn relay_event(ra: &mut RawWs, topic: &str, pred: impl Fn(&Value) -> bool) -> Result<Value> {
    let f = ra
        .next_matching(WAIT, |f| {
            f["t"] == "event"
                && f["event"]["type"] == "topic.updated"
                && f["event"]["data"]["topicId"] == topic
                && f["event"]["data"]["relay"].is_object()
                && pred(&f["event"]["data"]["relay"])
        })
        .await
        .context("topic.updated with the relay")?;
    Ok(f["event"]["data"].clone())
}

async fn finish(
    m: &mut RawWs,
    seq: &mut u64,
    a: &Value,
    status: &str,
    result: &str,
    code: Option<Value>,
) -> Result<()> {
    let run_id = a["runId"]
        .as_str()
        .ok_or_else(|| anyhow!("run.assign without runId"))?
        .to_string();
    m.send(json!({ "t": "run.accept", "runId": run_id }))
        .await?;
    *seq += 1;
    let mut done = json!({ "t": "run.done", "runId": run_id, "seq": *seq, "at": now(),
                           "status": status, "durationMs": 1000 });
    if status == "succeeded" {
        done["resultText"] = json!(result);
    } else {
        done["error"] = json!("boom");
    }
    if let Some(c) = code {
        done["code"] = c;
    }
    m.send(done).await?;
    let want = *seq;
    m.next_matching(WAIT, |f| {
        f["t"] == "ack"
            && f["runId"] == run_id.as_str()
            && f["seq"].as_u64().is_some_and(|s| s >= want)
    })
    .await
    .context("machine: ack of run.done")?;
    Ok(())
}
