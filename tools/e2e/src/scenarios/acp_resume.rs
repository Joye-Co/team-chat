//! An AI member on an ACP agent continues its conversation (usable, 接 3.6), with a raw machine
//! socket: the machine reports per CLI whether it continues sessions (`CliInfo.resume`). A
//! follow-up in the topic of an agent that can (`session/load`) carries `resumeSessionId` and
//! only what is new; one that cannot gets no session and the full topic, as a first run does.

use super::{Ctx, Run, Scenario};
use crate::check;
use crate::raw::RawWs;
use anyhow::{Context as _, Result, anyhow};
use chat_core::proto::{CreateMachineRequest, SetBotRunnerRequest};
use reqwest::Method;
use serde_json::{Value, json};
use std::time::Duration;

pub const SCENARIO: Scenario = Scenario {
    name: "acp-resume",
    about: "ACP member: a follow-up continues the session only when the agent can load it",
    in_all: true,
    timeout_secs: 90,
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
    let a = h.human("acp-a").await?;
    let bot = h.bot("acp-bot").await?;
    let g = h.group(&a, "acp", &[&bot]).await?;
    let label: String = format!("e2e-acp-{}", h.run_id.to_lowercase())
        .chars()
        .take(32)
        .collect();
    let owner = h.owner_http();
    let made = owner
        .create_machine(&CreateMachineRequest {
            name: h.name("acp-mac"),
            labels: Some(vec![label.clone()]),
            personal: None,
        })
        .await
        .context("create machine")?;
    let machine_id = made.machine.id.clone();
    let result = async {
        let hello = json!({
            "t": "hello", "protocol": 1,
            "machine": { "name": "e2e-acp", "labels": [], "daemonVersion": "1.1.0", "os": "linux", "arch": "x86_64",
                         "maxParallelRuns": 2, "features": ["resume", "aside"] },
            "clis": [
                { "runtime": "opencode", "kind": "acp", "command": "opencode", "available": true, "resume": true, "aside": false },
                { "runtime": "gemini", "kind": "acp", "command": "gemini", "available": true, "resume": false, "aside": false }
            ],
            "health": { "diskFreeBytes": 100e9, "diskTotalBytes": 500e9, "load1": 0.5, "load5": 0.5, "load15": 0.5, "cpuCount": 8, "activeRuns": 0 },
            "runs": []
        });
        let mut m = RawWs::machine(&h.base, &made.token, hello).await?;
        let mut ra = RawWs::connect(&h.base, a.token()).await?;
        let mut seq = 0u64;
        for (runtime, resumes) in [("opencode", true), ("gemini", false)] {
            let runner: SetBotRunnerRequest = serde_json::from_value(json!({
                "runtime": runtime, "labels": [label], "repo": { "path": "/tmp/repo" }, "baseBranch": "main"
            }))?;
            owner
                .set_bot_runner(&bot.id, &runner)
                .await
                .context("set runner")?;
            let first = format!("@bot 第一件事 {runtime}：修好登录");
            let sent = ra
                .request(json!({ "t": "send", "groupId": g, "clientMsgId": chat_core::new_id(),
                                 "body": first, "mentions": [bot.id] }))
                .await?;
            check!(sent["ok"] == true, "mention refused: {sent}");
            let topic = sent["result"]["topic"]["id"]
                .as_str()
                .unwrap_or_default()
                .to_string();
            let sid = format!("ses_e2e_{runtime}");
            let a1 = assign(&mut m, &topic, None).await?;
            finish(&mut m, &mut seq, &a1, Some(&sid)).await?;

            // The follow-up in the same topic, once the first run is done.
            let path = format!("/v1/runs/{}", a1["runId"].as_str().unwrap_or_default());
            let deadline = std::time::Instant::now() + WAIT;
            loop {
                let (st, r) = h.api(Method::GET, &path, Some(a.token()), None).await?;
                if st == 200 && r["status"] == "done" {
                    break;
                }
                check!(
                    std::time::Instant::now() < deadline,
                    "first run not done: {st} {r}"
                );
                tokio::time::sleep(Duration::from_millis(200)).await;
            }
            let follow = ra
                .request(json!({ "t": "send", "groupId": g, "topicId": topic, "clientMsgId": chat_core::new_id(),
                                 "body": "@bot 接着加测试", "mentions": [bot.id] }))
                .await?;
            check!(follow["ok"] == true, "follow-up refused: {follow}");
            let a2 = assign(&mut m, &topic, Some(&a1)).await?;
            let prompt = a2["prompt"].as_str().unwrap_or_default();
            check!(prompt.contains("接着加测试"), "follow-up prompt: {prompt}");
            if resumes {
                check!(
                    a2["resumeSessionId"] == sid.as_str(),
                    "{runtime}: no resume: {a2}"
                );
                check!(
                    !prompt.contains("修好登录"),
                    "{runtime}: a resumed run got the whole topic"
                );
            } else {
                check!(
                    a2.get("resumeSessionId").is_none_or(Value::is_null),
                    "{runtime}: resume sent to an agent that cannot load: {a2}"
                );
                check!(
                    prompt.contains("修好登录"),
                    "{runtime}: a new session did not get the whole topic"
                );
            }
            finish(&mut m, &mut seq, &a2, None).await?;
            c.note(format!("{runtime}: follow-up resumed = {resumes}"));
        }
        Ok(())
    }
    .await;
    if let Err(e) = owner.delete_machine(&machine_id).await {
        c.note(format!("machine not deleted: {e}"));
    }
    result
}

/// The next `run.assign` of the topic other than `not` (a run id).
async fn assign(m: &mut RawWs, topic: &str, not: Option<&Value>) -> Result<Value> {
    let not = not
        .and_then(|f| f["runId"].as_str())
        .unwrap_or_default()
        .to_string();
    m.next_matching(WAIT, |f| {
        f["t"] == "run.assign" && f["topicId"] == topic && f["runId"] != not.as_str()
    })
    .await
    .context("machine: run.assign")
}

async fn finish(m: &mut RawWs, seq: &mut u64, a: &Value, session: Option<&str>) -> Result<()> {
    let run_id = a["runId"]
        .as_str()
        .ok_or_else(|| anyhow!("run.assign without runId"))?
        .to_string();
    m.send(json!({ "t": "run.accept", "runId": run_id }))
        .await?;
    *seq += 1;
    let mut done = json!({ "t": "run.done", "runId": run_id, "seq": *seq, "at": now(),
                           "status": "succeeded", "resultText": "好了", "durationMs": 1000 });
    if let Some(s) = session {
        done["sessionId"] = json!(s);
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
