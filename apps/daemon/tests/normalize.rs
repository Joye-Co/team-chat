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
            model: Some("claude-test".into())
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
