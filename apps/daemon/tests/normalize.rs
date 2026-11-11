//! Event normalization from recorded (real, sanitized) and synthetic sample streams.

use daemon::proto::RunEvent;
use daemon::runtime::acp::AcpNormalizer;
use daemon::runtime::claude::ClaudeNormalizer;
use daemon::runtime::codex::CodexNormalizer;
use serde_json::Value;

fn lines(name: &str) -> Vec<Value> {
    let p = format!("{}/tests/fixtures/{name}", env!("CARGO_MANIFEST_DIR"));
    std::fs::read_to_string(p)
        .unwrap()
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| serde_json::from_str(l).unwrap())
        .collect()
}

fn codex(name: &str) -> (Vec<RunEvent>, CodexNormalizer) {
    let mut n = CodexNormalizer::default();
    let mut out = vec![];
    for v in lines(name) {
        out.extend(n.on_notification(v["method"].as_str().unwrap(), &v["params"]));
    }
    (out, n)
}

fn types(evs: &[RunEvent]) -> Vec<String> {
    evs.iter()
        .map(|e| {
            serde_json::to_value(e).unwrap()["type"]
                .as_str()
                .unwrap()
                .to_string()
        })
        .collect()
}

#[test]
fn codex_success_sample() {
    let (evs, n) = codex("codex-synthetic-success.jsonl");
    assert_eq!(
        types(&evs),
        [
            "turn.started",
            "message",
            "reasoning",
            "tool.call",
            "tool.result",
            "file.change",
            "tool.call",
            "tool.result",
            "message.delta",
            "message.delta",
            "message",
            "turn.completed"
        ]
    );
    assert_eq!(
        evs[1],
        RunEvent::Message {
            item_id: "i1".into(),
            turn_id: Some("tu1".into()),
            role: "user".into(),
            text: "Fix the test".into()
        }
    );
    match &evs[4] {
        RunEvent::ToolResult {
            ok,
            exit_code,
            output,
            ..
        } => {
            assert!(*ok);
            assert_eq!(*exit_code, Some(0));
            assert_eq!(output.as_deref(), Some("test ok\n"));
        }
        e => panic!("{e:?}"),
    }
    match &evs[5] {
        RunEvent::FileChange { changes, .. } => {
            assert_eq!(changes.len(), 2);
            assert_eq!(
                (changes[0].path.as_str(), changes[0].kind.as_str()),
                ("src/lib.rs", "modify")
            );
            assert_eq!(changes[1].kind, "add");
        }
        e => panic!("{e:?}"),
    }
    match &evs[7] {
        RunEvent::ToolResult { ok, output, .. } => {
            assert!(!*ok);
            assert_eq!(output.as_deref(), Some("server down"));
        }
        e => panic!("{e:?}"),
    }
    assert_eq!(
        evs[11],
        RunEvent::TurnCompleted {
            turn_id: "tu1".into(),
            status: "completed".into(),
            error: None
        }
    );
    assert_eq!(n.last_message.as_deref(), Some("Fixed the test."));
    assert_eq!(
        n.usage.as_ref().map(|u| (u.input_tokens, u.output_tokens)),
        Some((120, 30))
    );
}

#[test]
fn codex_real_auth_failure_sample() {
    let (evs, n) = codex("codex-real-auth-failure.jsonl");
    let t = types(&evs);
    assert_eq!(t.first().map(String::as_str), Some("turn.started"));
    assert!(t.iter().filter(|x| *x == "status").count() >= 5, "{t:?}");
    assert!(
        evs.iter()
            .any(|e| matches!(e, RunEvent::Status { state, .. } if state == "retrying"))
    );
    match evs.last().unwrap() {
        RunEvent::TurnCompleted { status, error, .. } => {
            assert_eq!(status, "failed");
            assert!(error.as_deref().unwrap().contains("401"));
        }
        e => panic!("{e:?}"),
    }
    assert_eq!(n.last_turn_status.as_deref(), Some("failed"));
}

#[test]
fn claude_success_sample() {
    let mut n = ClaudeNormalizer::default();
    let mut evs = n.queue_turn();
    for v in lines("claude-synthetic-success.jsonl") {
        evs.extend(n.on_line(&v));
    }
    assert_eq!(
        types(&evs),
        [
            "turn.started",
            "session",
            "reasoning",
            "tool.call",
            "tool.result",
            "tool.call",
            "tool.result",
            "file.change",
            "tool.result",
            "message.delta",
            "message.delta",
            "message",
            "status",
            "turn.completed"
        ]
    );
    assert_eq!(
        evs[1],
        RunEvent::Session {
            session_id: "s-1".into(),
            runtime: "claude".into(),
            model: Some("claude-test".into()),
            effort: None
        }
    );
    // Tool use ids from the stream; block indices continue across assistant events.
    match &evs[3] {
        RunEvent::ToolCall {
            item_id,
            tool,
            title,
            ..
        } => {
            assert_eq!((item_id.as_str(), tool.as_str()), ("toolu_1", "Bash"));
            assert_eq!(title, "Bash: ls");
        }
        e => panic!("{e:?}"),
    }
    match &evs[7] {
        RunEvent::FileChange { changes, .. } => assert_eq!(changes[0].path, "/work/a.txt"),
        e => panic!("{e:?}"),
    }
    assert!(matches!(&evs[8], RunEvent::ToolResult { ok: false, .. }));
    // Deltas and the final message share one item id.
    let delta_id = match &evs[9] {
        RunEvent::MessageDelta {
            item_id, turn_id, ..
        } => {
            assert_eq!(turn_id.as_deref(), Some("t1"));
            item_id.clone()
        }
        e => panic!("{e:?}"),
    };
    match &evs[11] {
        RunEvent::Message { item_id, text, .. } => {
            assert_eq!(item_id, &delta_id);
            assert_eq!(text, "Done.");
        }
        e => panic!("{e:?}"),
    }
    assert_eq!(
        evs[13],
        RunEvent::TurnCompleted {
            turn_id: "t1".into(),
            status: "completed".into(),
            error: None
        }
    );
    assert_eq!(n.last_result.as_deref(), Some("Done."));
    let u = n.usage.clone().unwrap();
    assert_eq!((u.input_tokens, u.output_tokens), (110, 20));
    assert_eq!(n.open_turns(), 0);
}

#[test]
fn claude_follow_up_turns() {
    let mut n = ClaudeNormalizer::default();
    let mut evs = n.queue_turn();
    evs.extend(n.queue_turn()); // follow-up written while turn 1 runs
    let result = serde_json::json!({ "type": "result", "subtype": "success", "is_error": false, "result": "a", "session_id": "s" });
    evs.extend(n.on_line(&result));
    evs.extend(n.on_line(&result));
    assert_eq!(
        types(&evs),
        [
            "turn.started",
            "turn.completed",
            "turn.started",
            "turn.completed"
        ]
    );
    assert_eq!(n.open_turns(), 0);
}

/// A follow-up the CLI reads while the turn runs joins that turn: one result ends the run
/// (it used to leave a second turn open for ever).
#[test]
fn claude_follow_up_folded_into_the_running_turn() {
    let mut n = ClaudeNormalizer::default();
    let echo = serde_json::json!({ "type": "user", "isReplay": true, "message": { "role": "user", "content": [{ "type": "text", "text": "x" }] } });
    let result = serde_json::json!({ "type": "result", "subtype": "success", "is_error": false, "result": "a", "session_id": "s" });
    let mut evs = n.queue_turn();
    evs.extend(n.on_line(&echo));
    evs.extend(n.queue_turn()); // written while turn 1 runs
    evs.extend(n.on_line(&echo)); // and read before it ends
    evs.extend(n.on_line(&result));
    assert_eq!(types(&evs), ["turn.started", "turn.completed"]);
    assert_eq!(n.open_turns(), 0);
}

/// A follow-up the CLI has not read when the turn ends gets the next turn.
#[test]
fn claude_follow_up_after_the_turn_gets_its_own() {
    let mut n = ClaudeNormalizer::default();
    let echo = serde_json::json!({ "type": "user", "isReplay": true, "message": { "role": "user", "content": [{ "type": "text", "text": "x" }] } });
    let result = serde_json::json!({ "type": "result", "subtype": "success", "is_error": false, "result": "a", "session_id": "s" });
    let mut evs = n.queue_turn();
    evs.extend(n.on_line(&echo));
    evs.extend(n.queue_turn());
    evs.extend(n.on_line(&result));
    assert_eq!(n.open_turns(), 1);
    evs.extend(n.on_line(&echo));
    evs.extend(n.on_line(&result));
    assert_eq!(
        types(&evs),
        [
            "turn.started",
            "turn.completed",
            "turn.started",
            "turn.completed"
        ]
    );
    assert_eq!(n.open_turns(), 0);
}

#[test]
fn claude_real_auth_failure_sample() {
    let mut n = ClaudeNormalizer::default();
    let mut evs = n.queue_turn();
    for v in lines("claude-real-auth-failure.jsonl") {
        evs.extend(n.on_line(&v));
    }
    assert_eq!(
        types(&evs),
        ["turn.started", "session", "message", "turn.completed"]
    );
    match evs.last().unwrap() {
        RunEvent::TurnCompleted { status, error, .. } => {
            assert_eq!(status, "failed");
            assert!(error.as_deref().unwrap().contains("authenticate"));
        }
        e => panic!("{e:?}"),
    }
    assert!(n.last_is_error);
}

#[test]
fn acp_success_sample() {
    let mut n = AcpNormalizer::default();
    let mut evs = n.start_turn();
    for v in lines("acp-synthetic-success.jsonl") {
        if v["method"] == "session/update" {
            evs.extend(n.on_update(&v["params"]["update"]));
        } else if let Some(r) = v["result"]["stopReason"].as_str() {
            evs.extend(n.end_turn(r));
        }
    }
    assert_eq!(
        types(&evs),
        [
            "turn.started",
            "reasoning",
            "message.delta",
            "message.delta",
            "message",
            "tool.call",
            "tool.result",
            "tool.call",
            "tool.result",
            "file.change",
            "tool.call",
            "tool.result",
            "message.delta",
            "message",
            "turn.completed"
        ]
    );
    match &evs[4] {
        RunEvent::Message { text, .. } => assert_eq!(text, "Let me look."),
        e => panic!("{e:?}"),
    }
    match &evs[6] {
        RunEvent::ToolResult { ok, output, .. } => {
            assert!(*ok);
            assert_eq!(output.as_deref(), Some("hello"));
        }
        e => panic!("{e:?}"),
    }
    match &evs[11] {
        RunEvent::ToolResult { ok, output, .. } => {
            assert!(!*ok);
            assert!(output.as_deref().unwrap().contains("denied"));
        }
        e => panic!("{e:?}"),
    }
    assert_eq!(n.last_message.as_deref(), Some("Changed it."));
}

#[test]
fn acp_real_empty_turn() {
    let mut n = AcpNormalizer::default();
    let mut evs = n.start_turn();
    for v in lines("acp-real-opencode-empty.jsonl") {
        if v["method"] == "session/update" {
            evs.extend(n.on_update(&v["params"]["update"]));
        } else if let Some(r) = v["result"]["stopReason"].as_str() {
            evs.extend(n.end_turn(r));
        }
    }
    assert_eq!(types(&evs), ["turn.started", "turn.completed"]);
    assert!(n.last_message.is_none());
}

#[test]
fn ansi_is_stripped() {
    use daemon::runtime::pty::strip_ansi;
    assert_eq!(
        strip_ansi("\x1b[1;32mok\x1b[0m\r\n\x1b]0;title\x07x"),
        "ok\nx"
    );
}

fn plans(evs: &[RunEvent]) -> Vec<Vec<(String, &'static str)>> {
    evs.iter()
        .filter_map(|e| match e {
            RunEvent::Plan { items } => Some(
                items
                    .iter()
                    .map(|i| {
                        let state = match i.state {
                            daemon::proto::RunPlanState::Done => "done",
                            daemon::proto::RunPlanState::Current => "current",
                            daemon::proto::RunPlanState::Waiting => "waiting",
                            daemon::proto::RunPlanState::Pending => "pending",
                        };
                        (i.text.clone(), state)
                    })
                    .collect(),
            ),
            _ => None,
        })
        .collect()
}

fn owned(items: &[(&str, &'static str)]) -> Vec<(String, &'static str)> {
    items.iter().map(|(t, s)| (t.to_string(), *s)).collect()
}

#[test]
fn claude_todowrite_and_task_tools_become_the_run_plan() {
    let mut n = ClaudeNormalizer::default();
    let mut evs = n.queue_turn();
    for v in lines("claude-synthetic-plan.jsonl") {
        evs.extend(n.on_line(&v));
    }
    let got = plans(&evs);
    let long: String = "长".repeat(200);
    assert_eq!(
        got,
        vec![
            owned(&[
                ("写标题和正文", "current"),
                ("生成 3 张配图", "pending"),
                ("周四 20:00 发出", "pending"),
            ]),
            owned(&[
                ("写标题和正文", "done"),
                ("生成 3 张配图", "current"),
                ("周四 20:00 发出", "pending"),
            ]),
            // The unchanged third TodoWrite sends nothing; blank texts are dropped and long
            // ones cut to 200 characters.
            vec![
                ("写标题和正文".to_string(), "done"),
                ("生成 3 张配图".to_string(), "done"),
                (long, "current"),
            ],
            // TaskCreate adds a pending task; TaskUpdate (by the id from its result) starts it.
            owned(&[("读失败的测试", "pending")]),
            owned(&[("读失败的测试", "current")]),
        ]
    );
    // The tool calls are still reported, each followed by its plan.
    let t = types(&evs);
    let first_plan = t.iter().position(|x| x == "plan").unwrap();
    assert_eq!(t[first_plan - 1], "tool.call");
    assert_eq!(t.iter().filter(|x| *x == "tool.call").count(), 6);
    assert_eq!(n.last_result.as_deref(), Some("计划好了。"));
}

#[test]
fn codex_plan_updates_become_the_run_plan() {
    let (evs, n) = codex("codex-synthetic-plan.jsonl");
    assert_eq!(
        plans(&evs),
        vec![
            owned(&[
                ("Read the failing test", "current"),
                ("Fix the reconnect in sync", "pending"),
                ("Run cargo test", "pending"),
            ]),
            owned(&[
                ("Read the failing test", "done"),
                ("Fix the reconnect in sync", "current"),
                ("Run cargo test", "pending"),
            ]),
            // A new explanation with the same steps sends nothing.
            owned(&[
                ("Read the failing test", "done"),
                ("Fix the reconnect in sync", "done"),
                ("Run cargo test", "done"),
            ]),
        ]
    );
    assert_eq!(
        types(&evs),
        [
            "turn.started",
            "plan",
            "tool.call",
            "tool.result",
            "plan",
            "plan",
            "message",
            "turn.completed"
        ]
    );
    assert_eq!(n.last_message.as_deref(), Some("Fixed the reconnect."));
}

#[test]
fn plans_keep_at_most_30_items_and_every_plan_round_trips() {
    let mut t = daemon::runtime::PlanTracker::default();
    let many: Vec<String> = (0..40).map(|i| format!("step {i}")).collect();
    let ev = t
        .update(many.iter().map(|s| (s.as_str(), "pending")))
        .unwrap();
    let RunEvent::Plan { items } = &ev else {
        panic!("{ev:?}")
    };
    assert_eq!(items.len(), 30);
    // Unknown states are dropped; the frame passes the protocol's own checks.
    assert!(t.update([("x", "blocked")]).is_some());
    let back: RunEvent = serde_json::from_value(serde_json::to_value(&ev).unwrap()).unwrap();
    assert_eq!(back, ev);
    chat_protocol::check_plan("plan", items).unwrap();
}

#[test]
fn plan_items_get_their_start_and_end_times() {
    use chat_protocol::RunPlanState;
    let mut t = daemon::runtime::PlanTracker::default();
    let plan = |ev: Option<RunEvent>| match ev {
        Some(RunEvent::Plan { items }) => items,
        other => panic!("{other:?}"),
    };
    let a = plan(t.update_at([("读代码", "in_progress"), ("改", "pending")], 1_000));
    assert_eq!(a[0].started_at, Some(1_000));
    assert_eq!(a[1].started_at, None);
    // Unchanged texts and states send nothing, and keep the first start.
    assert!(
        t.update_at([("读代码", "in_progress"), ("改", "pending")], 2_000)
            .is_none()
    );
    let b = plan(t.update_at([("读代码", "completed"), ("改", "in_progress")], 61_000));
    assert_eq!(
        (b[0].state, b[0].started_at, b[0].ended_at),
        (RunPlanState::Done, Some(1_000), Some(61_000))
    );
    assert_eq!(b[1].started_at, Some(61_000));
    let c = plan(t.update_at([("读代码", "completed"), ("改", "completed")], 90_000));
    assert_eq!(c[0].ended_at, Some(61_000));
    assert_eq!(
        (c[1].started_at, c[1].ended_at),
        (Some(61_000), Some(90_000))
    );
    chat_protocol::check_plan("plan", &c).unwrap();
}

#[test]
fn claude_reports_each_models_tokens_and_cost() {
    let mut n = ClaudeNormalizer::default();
    let result: Value = serde_json::from_str(
        r#"{"type":"result","subtype":"success","is_error":false,"result":"ok","session_id":"s","total_cost_usd":0.31,
            "usage":{"input_tokens":10,"cache_read_input_tokens":100,"cache_creation_input_tokens":5,"output_tokens":20},
            "modelUsage":{"claude-opus-5-5":{"inputTokens":10,"outputTokens":18,"cacheReadInputTokens":100,"cacheCreationInputTokens":5,"costUSD":0.3},
                          "claude-haiku-4-5-20251001":{"inputTokens":0,"outputTokens":2,"cacheReadInputTokens":0,"cacheCreationInputTokens":0,"costUSD":0.01}}}"#,
    )
    .unwrap();
    n.on_line(&result);
    let u = n.usage.clone().unwrap();
    assert_eq!(u.cost_usd, Some(0.31));
    let m: Vec<_> = u
        .models
        .iter()
        .map(|m| {
            (
                m.model.as_str(),
                m.input_tokens,
                m.cache_read_tokens,
                m.cache_write_tokens,
                m.cost_usd,
            )
        })
        .collect();
    assert_eq!(
        m,
        [
            ("claude-haiku-4-5-20251001", 0, None, None, Some(0.01)),
            ("claude-opus-5-5", 10, Some(100), Some(5), Some(0.3)),
        ]
    );
}

#[test]
fn codex_reports_its_model_with_cached_input_apart() {
    let mut n = CodexNormalizer::default();
    n.model = Some("gpt-5.1-codex".into());
    let p: Value = serde_json::from_str(
        r#"{"threadId":"th","turnId":"tu","tokenUsage":{"total":{"totalTokens":1100,"inputTokens":1000,"cachedInputTokens":600,"cacheWriteInputTokens":0,"outputTokens":100,"reasoningOutputTokens":40}}}"#,
    )
    .unwrap();
    n.on_notification("thread/tokenUsage/updated", &p);
    let u = n.usage.clone().unwrap();
    assert_eq!(
        (u.input_tokens, u.output_tokens, u.cost_usd),
        (1000, 100, None)
    );
    assert_eq!(u.models.len(), 1);
    let m = &u.models[0];
    assert_eq!(
        (
            m.model.as_str(),
            m.input_tokens,
            m.output_tokens,
            m.cache_read_tokens
        ),
        ("gpt-5.1-codex", 400, 100, Some(600))
    );
}
