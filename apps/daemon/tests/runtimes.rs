//! Runtime drivers against fake CLIs that follow each CLI's documented protocol.
//!
//! The fake CLIs are Python scripts. Unix starts them by their `#!` line. Windows needs the
//! `fake-cli` launcher of the `test-fakes` feature (`cargo test --features test-fakes`), and
//! `python` on PATH; without the feature this file has no tests there.

#![cfg(any(unix, feature = "test-fakes"))]

use daemon::config::RuntimeConfig;
use daemon::proto::RunEvent;
use daemon::runtime::{self, Control, Outcome, RunSpec, Status};
use std::collections::BTreeMap;
use std::time::Duration;
use tokio::sync::mpsc;

fn fake(name: &str) -> String {
    format!("{}/tests/fake/{name}", env!("CARGO_MANIFEST_DIR"))
}

/// How a fake CLI script is started: the script itself on Unix; on Windows the `fake-cli`
/// launcher, with the script in `FAKE_CLI_SCRIPT`.
fn launch(command: &str) -> (String, Option<(String, String)>) {
    #[cfg(windows)]
    if command.ends_with(".py") {
        return (
            env!("CARGO_BIN_EXE_fake-cli").to_string(),
            Some(("FAKE_CLI_SCRIPT".into(), command.into())),
        );
    }
    (command.to_string(), None)
}

fn spec(dir: &std::path::Path, kind: &str, command: &str, args: &[&str], prompt: &str) -> RunSpec {
    let cwd = dir.join("work");
    std::fs::create_dir_all(&cwd).unwrap();
    let (command, script) = launch(command);
    let mut env = vec![
        ("PATH".into(), std::env::var("PATH").unwrap()),
        ("HOME".into(), dir.to_string_lossy().into()),
    ];
    // What every program on this system needs (Windows: SystemRoot and the like).
    for k in daemon::sys::OS_ENV {
        if let Ok(v) = std::env::var(k) {
            env.push((k.to_string(), v));
        }
    }
    env.extend(script);
    RunSpec {
        run_id: "r1".into(),
        runtime_name: kind.into(),
        cfg: RuntimeConfig {
            kind: kind.into(),
            command: Some(command),
            args: args.iter().map(|s| s.to_string()).collect(),
            env: BTreeMap::new(),
            model: None,
            models: None,
            efforts: None,
            approval_policy: Some("on-request".into()),
            sandbox: None,
            permission_mode: None,
            allowed_tools: None,
            asks_permission: false,
        },
        prompt: prompt.into(),
        model: None,
        effort: None,
        cwd,
        run_dir: dir.to_path_buf(),
        env,
        aside: false,
        note: None,
        resume_session_id: None,
        full_prompt: None,
        gate_hook: None,
        gate_alive: None,
        gates: None,
        exec: daemon::executor::local(),
        vm_shares: vec![],
    }
}

/// Starts a runtime; `react` sees each event and may answer with a control message.
async fn drive(
    spec: RunSpec,
    mut react: impl FnMut(&RunEvent) -> Option<Control>,
) -> (Outcome, Vec<RunEvent>) {
    let rt = runtime::for_kind(&spec.cfg.kind).unwrap();
    let (etx, mut erx) = mpsc::unbounded_channel();
    let (ctx, crx) = mpsc::unbounded_channel();
    let fut = rt.start(spec, etx, crx);
    tokio::pin!(fut);
    let mut evs = vec![];
    let outcome = tokio::time::timeout(Duration::from_secs(30), async {
        loop {
            tokio::select! {
                o = &mut fut => break o,
                Some(e) = erx.recv() => {
                    if let Some(c) = react(&e) { ctx.send(c).unwrap(); }
                    evs.push(e);
                }
            }
        }
    })
    .await
    .expect("runtime finished in time");
    while let Ok(e) = erx.try_recv() {
        evs.push(e);
    }
    (outcome, evs)
}

#[tokio::test]
async fn codex_steer_and_approval() {
    let dir = tempfile::tempdir().unwrap();
    let s = spec(
        dir.path(),
        "codex",
        &fake("fake_codex.py"),
        &[],
        "approve then fix",
    );
    let mut steered = false;
    let (o, evs) = drive(s, |e| match e {
        RunEvent::ApprovalRequest {
            approval_id, title, ..
        } => {
            assert!(title.contains("rm -rf build"));
            Some(Control::Approval {
                approval_id: approval_id.clone(),
                approve: true,
            })
        }
        RunEvent::MessageDelta { .. } if !steered => {
            steered = true;
            Some(Control::Input {
                input_id: "in1".into(),
                text: "also docs".into(),
            })
        }
        _ => None,
    })
    .await;
    assert_eq!(o.status, Status::Succeeded, "{o:?}");
    assert_eq!(
        o.result_text.as_deref(),
        Some("codex: approve then fix + also docs")
    );
    assert_eq!(o.session_id.as_deref(), Some("th-1"));
    assert!(o.session_file.as_ref().unwrap().is_file());
    assert!(
        evs.iter()
            .any(|e| matches!(e, RunEvent::Session { model: Some(m), .. } if m == "fake-codex"))
    );
    assert!(evs.iter().any(
        |e| matches!(e, RunEvent::ApprovalResolved { decision, .. } if decision == "approve")
    ));
    assert!(
        evs.iter()
            .any(|e| matches!(e, RunEvent::ToolResult { ok: true, .. }))
    );
    assert!(
        evs.iter()
            .any(|e| matches!(e, RunEvent::InputAccepted { input_id } if input_id == "in1"))
    );
    assert_eq!(
        evs.iter()
            .filter(|e| matches!(e, RunEvent::TurnCompleted { .. }))
            .count(),
        1
    );
}

/// A steer that reaches codex as the turn ends fails after `turn/completed`; its text must get
/// the next turn instead of being lost.
#[tokio::test]
async fn codex_late_steer_gets_next_turn() {
    let dir = tempfile::tempdir().unwrap();
    let s = spec(
        dir.path(),
        "codex",
        &fake("fake_codex.py"),
        &[],
        "late work",
    );
    let mut steered = false;
    let (o, evs) = drive(s, |e| match e {
        RunEvent::MessageDelta { .. } if !steered => {
            steered = true;
            Some(Control::Input {
                input_id: "in1".into(),
                text: "also docs".into(),
            })
        }
        _ => None,
    })
    .await;
    assert_eq!(o.status, Status::Succeeded, "{o:?}");
    assert_eq!(o.result_text.as_deref(), Some("codex: also docs"));
    assert_eq!(
        evs.iter()
            .filter(|e| matches!(e, RunEvent::TurnCompleted { .. }))
            .count(),
        2
    );
}

/// The run's gates: merging into main, and a GitHub MCP merge.
fn merge_gates() -> std::sync::Arc<daemon::gates::Gates> {
    std::sync::Arc::new(daemon::gates::Gates::new(&[daemon::proto::RunGate {
        kind: "mergeMain".into(),
        patterns: vec![
            r"\bgh pr merge\b".into(),
            r"\bgit push\b.*[\s:](refs/heads/)?(main|master)(\s|$)".into(),
            r"^mcp__github__merge_pull_request\b".into(),
        ],
    }]))
}

/// A gated Codex run: approval policy `untrusted`; a command that is not gated, a file change
/// and requested permissions are accepted at once without a card; a gated command and a gated
/// MCP tool call ask a person through the gates (one card each) and get that person's answer.
#[tokio::test]
async fn codex_gated_run_answers_its_own_approvals() {
    let dir = tempfile::tempdir().unwrap();
    let mut s = spec(
        dir.path(),
        "codex",
        &fake("fake_codex.py"),
        &[],
        "gated work",
    );
    let gates = merge_gates();
    s.gates = Some(gates.clone());
    assert_eq!(runtime::codex::thread_policy(&s).0, "untrusted");
    let work = s.cwd.clone();
    let mut asked = vec![];
    let (o, evs) = drive(s, |e| {
        if let RunEvent::ApprovalRequest {
            approval_id,
            kind,
            command,
            ..
        } = e
        {
            assert_eq!(kind.as_deref(), Some("mergeMain"));
            asked.push(command.clone().unwrap_or_default());
            // Deny the push, approve the MCP merge.
            let approve = asked.len() == 2;
            assert!(gates.answer(approval_id, approve, false).is_some());
        }
        None
    })
    .await;
    assert_eq!(o.status, Status::Succeeded, "{o:?}");
    // The tool's arguments are compared as JSON: their key order depends on serde_json's
    // `preserve_order`, which a build together with the desktop app turns on.
    assert_eq!(asked.len(), 2, "{asked:?}");
    assert_eq!(asked[0], "git push origin main");
    let (tool, args) = asked[1].split_once(' ').expect("tool and arguments");
    assert_eq!(tool, "mcp__github__merge_pull_request");
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(args).expect("arguments are JSON"),
        serde_json::json!({"owner": "o", "pullNumber": 5, "repo": "r"})
    );
    // Only the gated ones were cards; the daemon itself answered every request.
    assert_eq!(
        evs.iter()
            .filter(|e| matches!(e, RunEvent::ApprovalRequest { .. }))
            .count(),
        2
    );
    let answers: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(work.join("gated.json")).unwrap()).unwrap();
    assert_eq!(
        answers,
        serde_json::json!([
            { "decision": "accept" },
            { "decision": "accept" },
            { "decision": "decline" },
            { "action": "accept", "content": null, "_meta": null },
        ])
    );
    let policies = std::fs::read_to_string(work.join("policies.json")).unwrap();
    assert_eq!(policies, r#"["untrusted"]"#);
}

/// What a gated Codex run answers to each kind of app-server request.
#[test]
fn codex_gate_answers() {
    use runtime::codex::{GateAnswer, gate_answer};
    use serde_json::json;
    let g = merge_gates();
    let cmd = |c: &str| json!({ "command": c });
    assert_eq!(
        gate_answer(
            "item/commandExecution/requestApproval",
            &cmd("/bin/zsh -lc 'cargo test'"),
            &g
        ),
        GateAnswer::Now(json!({ "decision": "accept" }))
    );
    // No command (stdin to a running process): accepted.
    assert_eq!(
        gate_answer("item/commandExecution/requestApproval", &json!({}), &g),
        GateAnswer::Now(json!({ "decision": "accept" }))
    );
    assert!(matches!(
        gate_answer("item/commandExecution/requestApproval", &cmd("/bin/bash -lc 'gh pr merge 9'"), &g),
        GateAnswer::Ask { line, .. } if line == "gh pr merge 9"
    ));
    let perms = json!({ "network": { "enabled": true } });
    assert_eq!(
        gate_answer(
            "item/permissions/requestApproval",
            &json!({ "permissions": perms }),
            &g
        ),
        GateAnswer::Now(json!({ "permissions": perms, "scope": "turn" }))
    );
    // An MCP tool approval of another tool: accepted; an elicitation that is not a tool
    // approval, or another request: not the gates' to answer.
    let mcp = |kind: &str| {
        json!({ "serverName": "github", "message": "Allow the github MCP server to run tool \"get_issue\"?",
            "_meta": { "codex_approval_kind": kind, "tool_params": {} } })
    };
    assert_eq!(
        gate_answer("mcpServer/elicitation/request", &mcp("mcp_tool_call"), &g),
        GateAnswer::Now(json!({ "action": "accept", "content": null, "_meta": null }))
    );
    assert_eq!(
        gate_answer("mcpServer/elicitation/request", &mcp("other"), &g),
        GateAnswer::NotMine
    );
    assert_eq!(
        gate_answer("item/tool/requestUserInput", &json!({}), &g),
        GateAnswer::NotMine
    );
}

/// A Codex run without gates keeps approval `never`: nothing to ask about.
#[tokio::test]
async fn codex_run_without_gates_never_asks() {
    let dir = tempfile::tempdir().unwrap();
    let mut s = spec(dir.path(), "codex", &fake("fake_codex.py"), &[], "plain");
    s.gates = Some(std::sync::Arc::new(daemon::gates::Gates::new(&[])));
    let work = s.cwd.clone();
    let (o, _) = drive(s, |_| None).await;
    assert_eq!(o.status, Status::Succeeded, "{o:?}");
    let policies = std::fs::read_to_string(work.join("policies.json")).unwrap();
    assert_eq!(policies, r#"["never"]"#);
}

/// ACP: a permission request that matches a gate asks a person through the gates; the agent
/// gets the person's answer.
#[tokio::test]
async fn acp_permission_flow() {
    let dir = tempfile::tempdir().unwrap();
    let mut s = spec(
        dir.path(),
        "acp",
        &fake("fake_acp.py"),
        &[],
        "approve merge",
    );
    let gates = merge_gates();
    s.gates = Some(gates.clone());
    let (o, evs) = drive(s, |e| {
        if let RunEvent::ApprovalRequest {
            approval_id,
            command,
            ..
        } = e
        {
            assert_eq!(command.as_deref(), Some("gh pr merge 5"));
            assert!(gates.answer(approval_id, false, false).is_some());
        }
        None
    })
    .await;
    assert_eq!(o.status, Status::Succeeded, "{o:?}");
    // A tool call ends the message item; the text after it is the final message.
    assert_eq!(o.result_text.as_deref(), Some("permission no"));
    assert!(
        evs.iter()
            .any(|e| matches!(e, RunEvent::ToolResult { ok: false, .. }))
    );
    assert!(
        evs.iter()
            .any(|e| matches!(e, RunEvent::Session { session_id, .. } if session_id == "s1"))
    );
}

/// ACP: permission is open; a request that matches no gate (here: the run has none) is allowed
/// at once, with no card.
#[tokio::test]
async fn acp_permission_without_a_gate_is_allowed_at_once() {
    let dir = tempfile::tempdir().unwrap();
    let mut s = spec(dir.path(), "acp", &fake("fake_acp.py"), &[], "approve this");
    let (o, evs) = drive(s.clone(), |_| None).await;
    assert_eq!(o.status, Status::Succeeded, "{o:?}");
    assert_eq!(o.result_text.as_deref(), Some("permission yes"));
    assert!(
        !evs.iter()
            .any(|e| matches!(e, RunEvent::ApprovalRequest { .. }))
    );
    // With gates that do not match the request: the same.
    s.gates = Some(merge_gates());
    let (o, evs) = drive(s, |_| None).await;
    assert_eq!(o.result_text.as_deref(), Some("permission yes"));
    assert!(
        !evs.iter()
            .any(|e| matches!(e, RunEvent::ApprovalRequest { .. }))
    );
}

#[tokio::test]
async fn acp_follow_up_is_next_prompt() {
    let dir = tempfile::tempdir().unwrap();
    // A gated command holds the first turn at an approval, so the follow-up surely comes
    // while it runs (a permission without a gate is allowed at once).
    let mut s = spec(
        dir.path(),
        "acp",
        &fake("fake_acp.py"),
        &[],
        "approve merge",
    );
    let gates = merge_gates();
    s.gates = Some(gates.clone());
    let mut sent = false;
    let (o, evs) = drive(s, |e| match e {
        RunEvent::ApprovalRequest { approval_id, .. } => {
            assert!(gates.answer(approval_id, true, false).is_some());
            None
        }
        RunEvent::MessageDelta { .. } if !sent => {
            sent = true;
            Some(Control::Input {
                input_id: "i".into(),
                text: "more".into(),
            })
        }
        _ => None,
    })
    .await;
    assert_eq!(o.status, Status::Succeeded);
    assert_eq!(o.result_text.as_deref(), Some("acp: more"));
    assert_eq!(
        evs.iter()
            .filter(|e| matches!(e, RunEvent::TurnCompleted { .. }))
            .count(),
        2
    );
}

fn messages(evs: &[RunEvent]) -> Vec<String> {
    evs.iter()
        .filter_map(|e| match e {
            RunEvent::Message { text, .. } => Some(text.clone()),
            _ => None,
        })
        .collect()
}

/// ACP resume (接 3.6): the next run of the topic loads the session of the last one, in its own
/// working folder; the agent remembers the first turn and its replay of it is not reported.
#[tokio::test]
async fn acp_resume_loads_the_session() {
    let dir = tempfile::tempdir().unwrap();
    let s = spec(
        dir.path(),
        "acp",
        &fake("fake_acp.py"),
        &[],
        "the code word is PELICAN",
    );
    let (o, evs) = drive(s.clone(), |_| None).await;
    assert_eq!(o.status, Status::Succeeded, "{o:?}");
    assert_eq!(o.session_id.as_deref(), Some("s1"));
    assert!(
        evs.iter()
            .any(|e| matches!(e, RunEvent::Session { session_id, .. } if session_id == "s1"))
    );

    // The follow-up run: another process, another folder, the session id from the server.
    let mut next = s.clone();
    next.cwd = dir.path().join("work2");
    std::fs::create_dir_all(&next.cwd).unwrap();
    next.prompt = "recall".into();
    next.note = Some("NOTE".into());
    next.resume_session_id = Some("s1".into());
    let (o, evs) = drive(next.clone(), |_| None).await;
    assert_eq!(o.status, Status::Succeeded, "{o:?}");
    assert_eq!(o.session_id.as_deref(), Some("s1"));
    assert!(
        evs.iter()
            .any(|e| matches!(e, RunEvent::Session { session_id, .. } if session_id == "s1"))
    );
    // The agent knows the first turn and works in the new folder.
    let want = format!(
        "acp: remembered [the code word is PELICAN] in {}",
        next.cwd.to_string_lossy()
    );
    assert_eq!(o.result_text.as_deref(), Some(want.as_str()));
    // The replay of the old session ("old answer") is not reported; one turn, one message.
    assert_eq!(messages(&evs), vec![want]);
    assert_eq!(
        evs.iter()
            .filter(|e| matches!(e, RunEvent::TurnStarted { .. }))
            .count(),
        1
    );
}

/// ACP resume of a session the agent no longer has: a new session, and the agent is told its
/// earlier conversation is gone.
#[tokio::test]
async fn acp_resume_of_an_unknown_session_starts_a_new_one() {
    let dir = tempfile::tempdir().unwrap();
    let mut s = spec(dir.path(), "acp", &fake("fake_acp.py"), &[], "go on");
    s.resume_session_id = Some("gone".into());
    let (o, _) = drive(s, |_| None).await;
    assert_eq!(o.status, Status::Succeeded, "{o:?}");
    assert_eq!(o.session_id.as_deref(), Some("s1"));
    let text = o.result_text.unwrap();
    assert!(
        text.contains(runtime::acp::LOST_SESSION_NOTE) && text.ends_with("go on"),
        "{text}"
    );
}

/// ACP resume with an agent that does not advertise `loadSession`: it never gets
/// `session/load`; a new session, told so.
#[tokio::test]
async fn acp_resume_without_load_session_starts_a_new_one() {
    let dir = tempfile::tempdir().unwrap();
    let mut s = spec(dir.path(), "acp", &fake("fake_acp.py"), &[], "first");
    let (o, _) = drive(s.clone(), |_| None).await;
    assert_eq!(o.session_id.as_deref(), Some("s1"));
    s.env.push(("FAKE_ACP_NO_LOAD".into(), "1".into()));
    s.resume_session_id = Some("s1".into());
    s.prompt = "recall".into();
    let (o, _) = drive(s, |_| None).await;
    assert_eq!(o.status, Status::Succeeded, "{o:?}");
    assert_eq!(o.session_id.as_deref(), Some("s2"));
    // A new session remembers nothing; the prompt it got said so.
    assert!(o.result_text.unwrap().starts_with("acp: remembered []"));
    let raw = std::fs::read_to_string(dir.path().join("acp-sessions/s2.json")).unwrap();
    assert!(raw.contains("could not be"), "{raw}");
    assert!(!raw.contains("session/load"));
}

/// `CliInfo.resume` of an ACP agent comes from its `initialize` answer.
#[cfg(unix)]
#[tokio::test]
async fn acp_probe_reads_load_session() {
    let dir = tempfile::tempdir().unwrap();
    let s = spec(dir.path(), "acp", &fake("fake_acp.py"), &[], "-");
    let program = std::path::PathBuf::from(fake("fake_acp.py"));
    assert_eq!(
        runtime::acp::probe_load_session(&program, &s.cfg).await,
        Some(true)
    );
    let mut cfg = s.cfg.clone();
    cfg.env.insert("FAKE_ACP_NO_LOAD".into(), "1".into());
    assert_eq!(
        runtime::acp::probe_load_session(&program, &cfg).await,
        Some(false)
    );
    // A program that ends without answering: unknown.
    let mut cfg = s.cfg.clone();
    cfg.args = vec!["-c".into(), "exit 0".into()];
    let sh = std::path::PathBuf::from("/bin/sh");
    assert_eq!(runtime::acp::probe_load_session(&sh, &cfg).await, None);
}

/// The `pty` kind: the prompt reaches the program, what it prints comes back as its lines,
/// text sent to the run while it works is typed into the terminal, and the exit code is the
/// program's. The same program on both systems, in each one's shell.
#[tokio::test]
async fn pty_prompt_input_and_exit_code() {
    let dir = tempfile::tempdir().unwrap();
    #[cfg(unix)]
    let (shell, args) = (
        "/bin/sh",
        vec!["-c", "echo start:{prompt}; read x; echo got:$x; exit 3"],
    );
    #[cfg(windows)]
    let (shell, args) = (
        "cmd.exe",
        vec![
            "/d",
            "/v:on",
            "/c",
            "echo start:{prompt}& set /p x=& echo got:!x!& exit 3",
        ],
    );
    let s = spec(dir.path(), "pty", shell, &args, "hi");
    let mut sent = false;
    let mut accepted = false;
    let (o, _evs) = drive(s, |e| match e {
        RunEvent::MessageDelta { text, .. } if text.contains("start:hi") && !sent => {
            sent = true;
            Some(Control::Input {
                input_id: "i".into(),
                text: "abc".into(),
            })
        }
        RunEvent::InputAccepted { input_id } if input_id == "i" => {
            accepted = true;
            None
        }
        _ => None,
    })
    .await;
    assert!(accepted, "the input was taken");
    assert_eq!(o.status, Status::Failed);
    assert_eq!(o.exit_code, Some(3));
    let text = o.result_text.unwrap();
    assert!(text.contains("got:abc"), "{text:?}");
    // Lines, as the program wrote them (a Windows console sends a screen, not lines).
    let lines: Vec<&str> = text.lines().map(str::trim).collect();
    assert!(lines.contains(&"got:abc"), "{lines:?}");
    assert!(dir.path().join("pty.log").is_file());
}

/// Windows: a batch file is a `pty` program like any other. Its arguments arrive as they
/// were given (a prompt with spaces, `&`, `%` and quotes is one argument, not commands), what
/// is typed goes to the program the batch file reads with, and its exit code is the run's.
#[cfg(windows)]
#[tokio::test]
async fn pty_runs_a_batch_file() {
    let dir = tempfile::tempdir().unwrap();
    let script = dir.path().join("agent with space.cmd");
    std::fs::write(
        &script,
        "@echo off\r\necho arg1:\"%~1\"\r\necho arg2:\"%~2\"\r\nset /p line=\r\necho got:%line%\r\nexit /b 5\r\n",
    )
    .unwrap();
    let prompt = "hello world & echo injected > injected.txt | 100% %PATH% \"q\"";
    let s = spec(
        dir.path(),
        "pty",
        &script.to_string_lossy(),
        &["{prompt}", "second arg"],
        prompt,
    );
    let work = s.cwd.clone();
    let mut sent = false;
    let (o, _evs) = drive(s, |e| match e {
        RunEvent::MessageDelta { text, .. } if text.contains("arg2:") && !sent => {
            sent = true;
            Some(Control::Input {
                input_id: "i".into(),
                text: "typed".into(),
            })
        }
        _ => None,
    })
    .await;
    let raw = std::fs::read_to_string(dir.path().join("pty.log")).unwrap_or_default();
    assert_eq!(o.exit_code, Some(5), "{o:?} raw: {raw:?}");
    assert_eq!(o.status, Status::Failed);
    let text = o.result_text.unwrap();
    let lines: Vec<&str> = text.lines().map(str::trim).collect();
    // `%~1` gives a quote inside the argument back doubled: that is the batch file's own
    // reading; every other character is as it was given.
    let arg1 = format!("arg1:\"{}\"", prompt.replace('"', "\"\""));
    assert!(lines.contains(&arg1.as_str()), "{lines:?} raw: {raw:?}");
    assert!(lines.contains(&"arg2:\"second arg\""), "{lines:?}");
    assert!(lines.contains(&"got:typed"), "{lines:?}");
    assert!(
        !work.join("injected.txt").exists(),
        "the prompt ran as a command"
    );

    // An argument with a line break cannot be written for a batch file: the run says so.
    let s = spec(
        &dir.path().join("r2"),
        "pty",
        &script.to_string_lossy(),
        &["{prompt}"],
        "two\nlines",
    );
    let (o, _) = drive(s, |_| None).await;
    assert_eq!(o.status, Status::Failed);
    assert!(
        o.error
            .as_deref()
            .unwrap_or_default()
            .contains("line break"),
        "{o:?}"
    );
}

#[cfg(unix)]
#[tokio::test]
async fn pty_cancel_kills_group() {
    let dir = tempfile::tempdir().unwrap();
    let s = spec(
        dir.path(),
        "pty",
        "/bin/sh",
        &["-c", "echo $$ > pid; sleep 600 & wait"],
        "x",
    );
    let pidfile = dir.path().join("work/pid");
    let rt = runtime::for_kind("pty").unwrap();
    let (etx, _erx) = mpsc::unbounded_channel();
    let (ctx, crx) = mpsc::unbounded_channel();
    let h = tokio::spawn(rt.start(s, etx, crx));
    for _ in 0..100 {
        if pidfile.exists() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    ctx.send(Control::Cancel).unwrap();
    let o = tokio::time::timeout(Duration::from_secs(10), h)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(o.status, Status::Cancelled);
    let pid: i32 = std::fs::read_to_string(&pidfile)
        .unwrap()
        .trim()
        .parse()
        .unwrap();
    // SAFETY: probing a pid with signal 0.
    assert_ne!(unsafe { libc::kill(pid, 0) }, 0, "shell still alive");
}

/// A side chat run is read-only whatever the runtime config or the CLI's own config says.
/// Codex gets sandbox `read-only`, no configured args, and web search, apps, plugins, browser
/// and computer use and every configured MCP server turned off. Claude Code gets permission
/// mode `plan`, only the read-only tools, the command, edit, web and MCP tools denied, no MCP
/// servers and only the user's settings file, and neither `allowed_tools` nor the configured
/// args.
#[test]
fn aside_runs_ignore_widening_config() {
    let tmp = tempfile::tempdir().unwrap();
    let mut s = spec(
        tmp.path(),
        "codex",
        "codex",
        &["--dangerously-bypass-approvals-and-sandbox"],
        "q",
    );
    s.cfg.sandbox = Some("danger-full-access".into());
    // A normal run always has every permission; the configured `on-request` is ignored.
    assert_eq!(
        runtime::codex::thread_policy(&s),
        ("never".to_string(), "danger-full-access".to_string())
    );
    assert_eq!(
        runtime::codex::app_server_args(&s).unwrap(),
        vec![
            "app-server",
            "--dangerously-bypass-approvals-and-sandbox",
            "-c",
            "features.memories=false"
        ]
    );
    s.aside = true;
    assert_eq!(
        runtime::codex::thread_policy(&s),
        ("never".to_string(), "read-only".to_string())
    );
    // MCP servers from $CODEX_HOME/config.toml (HOME/.codex here) and the project's config.
    std::fs::create_dir_all(tmp.path().join(".codex")).unwrap();
    std::fs::write(
        tmp.path().join(".codex/config.toml"),
        "[mcp_servers.github]\ncommand = \"gh-mcp\"\n[mcp_servers.writer-2]\nurl = \"http://x\"\n",
    )
    .unwrap();
    std::fs::create_dir_all(s.cwd.join(".codex")).unwrap();
    std::fs::write(
        s.cwd.join(".codex/config.toml"),
        "[mcp_servers.repo_tool]\ncommand = \"t\"\n",
    )
    .unwrap();
    let args = runtime::codex::app_server_args(&s).unwrap();
    assert_eq!(args[0], "app-server");
    assert!(!args.iter().any(|a| a.contains("dangerously")), "{args:?}");
    let overrides: Vec<&str> = args
        .windows(2)
        .filter(|w| w[0] == "-c")
        .map(|w| w[1].as_str())
        .collect();
    for want in [
        "web_search=\"disabled\"",
        "features.apps=false",
        "features.plugins=false",
        "features.browser_use=false",
        "features.browser_use_external=false",
        "features.computer_use=false",
        "features.memories=false",
        "mcp_servers.github={command=\"true\",enabled=false}",
        "mcp_servers.writer-2={url=\"http://127.0.0.1/\",enabled=false}",
        "mcp_servers.repo_tool={command=\"true\",enabled=false}",
    ] {
        assert!(overrides.contains(&want), "{want} missing: {args:?}");
    }
    // A server name that `-c` cannot address fails the run instead of leaving it on.
    std::fs::write(
        s.cwd.join(".codex/config.toml"),
        "[mcp_servers.\"a.b\"]\ncommand = \"t\"\n",
    )
    .unwrap();
    assert!(runtime::codex::app_server_args(&s).is_err());

    let mut c = spec(
        tmp.path(),
        "claude",
        "claude",
        &["--dangerously-skip-permissions"],
        "q",
    );
    // A stricter config (1.1.0 docs wrote `acceptEdits`) is ignored: an AI member has every
    // permission, and `-p` has nobody to ask.
    c.cfg.permission_mode = Some("acceptEdits".into());
    c.cfg.allowed_tools = Some("Read".into());
    let normal = runtime::claude::permission_args(&c);
    assert!(
        normal.contains(&"bypassPermissions".to_string()),
        "{normal:?}"
    );
    assert!(!normal.contains(&"acceptEdits".to_string()), "{normal:?}");
    assert!(normal.contains(&"--dangerously-skip-permissions".to_string()));
    let i = normal.iter().position(|a| a == "--allowedTools").unwrap();
    for t in ["Bash", "Edit", "Write", "WebFetch", "Bash(chat-run:*)"] {
        assert!(normal[i + 1].split(' ').any(|x| x == t), "{t}: {normal:?}");
    }
    c.cfg.allowed_tools = None;
    c.cfg.permission_mode = None;
    let plain = runtime::claude::permission_args(&c);
    assert!(
        plain.contains(&"bypassPermissions".to_string()),
        "{plain:?}"
    );
    let i = plain.iter().position(|a| a == "--allowedTools").unwrap();
    for t in ["Bash", "Edit", "Write", "WebFetch"] {
        assert!(plain[i + 1].split(' ').any(|x| x == t), "{t}: {plain:?}");
    }
    c.aside = true;
    let args = runtime::claude::permission_args(&c);
    let value = |flag: &str| {
        let i = args
            .iter()
            .position(|a| a == flag)
            .unwrap_or_else(|| panic!("{flag} missing: {args:?}"));
        args[i + 1].clone()
    };
    assert_eq!(value("--permission-mode"), "plan");
    let tools: Vec<String> = value("--tools").split(',').map(str::to_string).collect();
    assert_eq!(tools, ["Read", "Grep", "Glob"]);
    let denied: Vec<String> = value("--disallowedTools")
        .split(' ')
        .map(str::to_string)
        .collect();
    for t in [
        "Bash",
        "Edit",
        "Write",
        "MultiEdit",
        "NotebookEdit",
        "WebFetch",
        "WebSearch",
        "mcp__*",
    ] {
        assert!(denied.iter().any(|d| d == t), "{t} not denied: {denied:?}");
    }
    assert!(args.iter().any(|a| a == "--strict-mcp-config"));
    assert_eq!(value("--mcp-config"), r#"{"mcpServers":{}}"#);
    // No settings files: the user's hooks could run commands, project and local settings come
    // from the repo. The auth env, apiKeyHelper and model come through --settings (run()).
    assert_eq!(value("--setting-sources"), "");
    for widening in [
        "bypassPermissions",
        "--dangerously-skip-permissions",
        "--allowedTools",
        "acceptEdits",
    ] {
        assert!(!args.iter().any(|a| a == widening), "{widening}: {args:?}");
    }
}

/// The MCP overrides of a side chat run are valid for the real Codex CLI whether or not it
/// loads a server's config file: here an untrusted project's `.codex/config.toml`, which Codex
/// skips, and a user config with a stdio and a URL server. Every server ends up disabled.
/// `codex mcp list` reads the config without a model call.
#[test]
#[ignore = "real CLI: scripts/test-real-cli.sh"]
fn codex_aside_mcp_overrides_load_in_real_cli() {
    let codex = real_codex();
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path().join("codex-home");
    std::fs::create_dir_all(&home).unwrap();
    std::fs::write(
        home.join("config.toml"),
        "[mcp_servers.local]\ncommand = \"echo\"\nargs = [\"a\"]\n\
         [mcp_servers.web]\nurl = \"http://127.0.0.1:9/x\"\nbearer_token_env_var = \"NOPE\"\n",
    )
    .unwrap();
    let mut s = spec(tmp.path(), "codex", "codex", &[], "hi");
    s.env
        .push(("CODEX_HOME".into(), home.to_string_lossy().into()));
    s.aside = true;
    std::fs::create_dir_all(s.cwd.join(".codex")).unwrap();
    std::fs::write(
        s.cwd.join(".codex/config.toml"),
        "[mcp_servers.repo_tool]\ncommand = \"t\"\n",
    )
    .unwrap();
    let args = runtime::codex::app_server_args(&s).unwrap();
    let out = std::process::Command::new(&codex.program)
        .args(&codex.args)
        .args(["mcp", "list", "--json"])
        .args(&args[1..])
        .current_dir(&s.cwd)
        .env("CODEX_HOME", &home)
        .env("HOME", tmp.path())
        .stdin(std::process::Stdio::null())
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "codex rejected {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let list: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    let list = list.as_array().unwrap();
    for name in ["local", "web"] {
        assert!(
            list.iter().any(|e| e["name"] == name),
            "{name} missing: {list:?}"
        );
    }
    for e in list {
        assert_eq!(e["enabled"], false, "{e}");
    }
}

/// The real `codex` of this machine and how to start it (on Windows npm's `codex.cmd` is
/// resolved to what it starts, as for a run). The real-CLI cases are `#[ignore]`d and run by
/// `scripts/test-real-cli.sh`, which checks it is there.
fn real_codex() -> daemon::health::Launch {
    let found = daemon::health::which("codex").expect("codex is not on PATH");
    daemon::health::launch(&found)
}

/// `codex app-server` with `args`: runs `command` through `command/exec` (the server's sandbox,
/// no model call) and returns its result.
fn codex_exec(
    codex: &daemon::health::Launch,
    args: &[String],
    home: &std::path::Path,
    cwd: &std::path::Path,
    command: &[String],
    env: serde_json::Value,
) -> serde_json::Value {
    use std::io::{BufRead, Write};
    let mut p = std::process::Command::new(&codex.program)
        .args(&codex.args)
        .args(args)
        .current_dir(cwd)
        .env("CODEX_HOME", home)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .spawn()
        .unwrap();
    let mut stdin = p.stdin.take().unwrap();
    let mut out = std::io::BufReader::new(p.stdout.take().unwrap());
    for m in [
        serde_json::json!({ "id": 1, "method": "initialize", "params": { "clientInfo": { "name": "daemon-test", "version": "1" } } }),
        serde_json::json!({ "method": "initialized" }),
        serde_json::json!({ "id": 2, "method": "command/exec", "params": {
            "command": command, "cwd": cwd.to_string_lossy(), "env": env, "timeoutMs": 20000 } }),
    ] {
        writeln!(stdin, "{m}").unwrap();
    }
    // The answer, or after two minutes none (the reader is a thread of its own, so a server
    // that says nothing cannot hold the test).
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let mut line = String::new();
        loop {
            line.clear();
            if out.read_line(&mut line).unwrap_or(0) == 0 {
                return;
            }
            let v: serde_json::Value = serde_json::from_str(&line).unwrap_or_default();
            if v["id"] == 2 {
                let _ = tx.send(v);
                return;
            }
        }
    });
    let result = match rx.recv_timeout(Duration::from_secs(120)) {
        Ok(v) => v,
        Err(std::sync::mpsc::RecvTimeoutError::Timeout) => serde_json::json!({
            "id": 2, "error": { "message": "codex app-server gave no answer in 120 s" } }),
        Err(_) => serde_json::json!({
            "id": 2, "error": { "message": "codex app-server ended without an answer" } }),
    };
    let _ = p.kill();
    let _ = p.wait();
    result
}

/// A Codex run in a `workspace-write` sandbox can use `chat-run` without network:
/// the real Codex CLI runs the real client through `command/exec` (the server's sandbox, no
/// model call) with the app-server args the daemon builds. The sandbox blocks the run's unix
/// socket, so a Codex run uses the request folder in its working folder.
#[cfg(unix)]
#[tokio::test(flavor = "multi_thread")]
#[ignore = "real CLI: scripts/test-real-cli.sh"]
async fn codex_sandbox_lets_chat_run_answer_without_network() {
    let codex = real_codex();
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path().join("codex-home");
    std::fs::create_dir_all(&home).unwrap();
    // The network is closed by hand here: chat-run must work without it.
    let closed = ["-c", "sandbox_workspace_write.network_access=false"];
    let mut s = spec(tmp.path(), "codex", "codex", &closed, "hi");
    s.cfg.sandbox = Some("workspace-write".into());
    s.cfg.approval_policy = None;
    s.note = Some(daemon::chat_run::note());
    assert!(runtime::codex::chat_run_works(&s));
    let args = runtime::codex::app_server_args(&s).unwrap();
    assert_eq!(
        args,
        [
            "app-server",
            closed[0],
            closed[1],
            "-c",
            "features.memories=false"
        ],
        "the user's network setting stays"
    );

    daemon::chat_run::set_exe(std::path::PathBuf::from(env!("CARGO_BIN_EXE_daemon")));
    let bin = daemon::chat_run::install_link(tmp.path()).unwrap();
    let outputs = std::sync::Arc::new(daemon::chat_run::Outputs::default());
    let (tx, mut rx) = mpsc::unbounded_channel();
    let tx2 = tx.clone();
    let server = daemon::chat_run::Server::start(
        daemon::chat_run::socket_path(tmp.path()),
        outputs.clone(),
        move |e| {
            let _ = tx2.send(e);
        },
        daemon::chat_run::TIMEOUT,
    )
    .unwrap();
    let spool = daemon::chat_run::Spool::start(
        daemon::chat_run::spool_dir(&s.cwd, "x1"),
        outputs.clone(),
        move |e| {
            let _ = tx.send(e);
        },
        daemon::chat_run::TIMEOUT,
    )
    .unwrap();
    let command: Vec<String> = [
        bin.join(daemon::chat_run::link_name())
            .to_string_lossy()
            .to_string(),
        "resolve".into(),
        "--event".into(),
        "EV1".into(),
        "--result".into(),
        "ignored".into(),
    ]
    .into();
    // The daemon sends the sandbox in `thread/start`; `command/exec` uses the server config.
    let mut sandboxed = args.clone();
    sandboxed.extend([
        "-c".to_string(),
        "sandbox_mode=\"workspace-write\"".to_string(),
    ]);
    let exec = |env: serde_json::Value| {
        let (codex, args, home, cwd, command) = (
            codex.clone(),
            sandboxed.clone(),
            home.clone(),
            s.cwd.clone(),
            command.clone(),
        );
        tokio::task::spawn_blocking(move || codex_exec(&codex, &args, &home, &cwd, &command, env))
    };

    // The socket: the sandbox blocks the connect, so chat-run cannot use it in a Codex run.
    let blocked = exec(serde_json::json!({ "CHAT_RUN_SOCKET": server.path().to_string_lossy() }))
        .await
        .unwrap();
    let stdout = blocked["result"]["stdout"].as_str().unwrap_or_default();
    assert!(stdout.contains("sandbox.blocked"), "{blocked}");

    // The request folder: the same command answers through files.
    let run = exec(serde_json::json!({ "CHAT_RUN_DIR": spool.path().to_string_lossy() }));
    let ev = tokio::time::timeout(Duration::from_secs(30), rx.recv())
        .await
        .expect("chat-run did not reach the daemon")
        .unwrap();
    let RunEvent::Output { output_id, op, .. } = ev else {
        panic!("not an output event: {ev:?}");
    };
    assert_eq!(op, "interaction.resolve");
    assert!(outputs.resolve(daemon::proto::RunOutputResult {
        run_id: "r1".into(),
        output_id,
        ok: true,
        result: Some(serde_json::json!({ "resolved": true })),
        error: None,
    }));
    let done = run.await.unwrap();
    assert_eq!(done["result"]["exitCode"], 0, "{done}");
    assert!(
        done["result"]["stdout"]
            .as_str()
            .unwrap_or_default()
            .contains("resolved"),
        "{done}"
    );
    let left: Vec<_> = std::fs::read_dir(spool.path())
        .unwrap()
        .map(|e| e.unwrap().file_name())
        .collect();
    assert_eq!(left, [std::ffi::OsString::from(".gitignore")]);
    // Network stays closed for the run's commands: a TCP connect to this test fails.
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let probe = format!(
        "import socket\ntry:\n socket.create_connection(('127.0.0.1', {port}), 5)\n print('open')\nexcept OSError as e:\n print('closed', e)"
    );
    let net = tokio::task::spawn_blocking({
        let (codex, args, home, cwd) = (
            codex.clone(),
            sandboxed.clone(),
            home.clone(),
            s.cwd.clone(),
        );
        move || {
            let cmd = vec!["python3".to_string(), "-c".to_string(), probe];
            codex_exec(&codex, &args, &home, &cwd, &cmd, serde_json::json!({}))
        }
    })
    .await
    .unwrap();
    let out = net["result"]["stdout"].as_str().unwrap_or_default();
    assert!(out.starts_with("closed"), "{net}");
}

/// A folder that is not under the temp folder (Codex's sandboxes count the temp folder as
/// writable): under the user's local application data on Windows.
fn outside_temp() -> tempfile::TempDir {
    match std::env::var_os("LOCALAPPDATA").filter(|_| cfg!(windows)) {
        Some(local) => tempfile::tempdir_in(local).unwrap(),
        None => tempfile::tempdir().unwrap(),
    }
}

/// A side chat run of Codex is read-only on every system: with the sandbox the daemon sends
/// for it (`read-only`, approval `never`) the real CLI does not let a command write a file,
/// in the working folder or outside it. Through `command/exec` (the server's sandbox, no
/// model call).
#[tokio::test(flavor = "multi_thread")]
#[ignore = "real CLI: scripts/test-real-cli.sh"]
async fn codex_read_only_sandbox_writes_nothing() {
    let codex = real_codex();
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path().join("codex-home");
    std::fs::create_dir_all(&home).unwrap();
    let mut s = spec(tmp.path(), "codex", "codex", &[], "hi");
    s.aside = true;
    let (approval, sandbox) = runtime::codex::thread_policy(&s);
    assert_eq!(
        (approval.as_str(), sandbox.as_str()),
        ("never", "read-only")
    );
    let mut args = runtime::codex::app_server_args(&s).unwrap();
    args.extend(["-c".to_string(), format!("sandbox_mode=\"{sandbox}\"")]);
    args.extend(["-c".to_string(), format!("approval_policy=\"{approval}\"")]);
    // Reading works: a side chat answers from the files.
    std::fs::write(s.cwd.join("seed.txt"), "seed-text").unwrap();
    let read: Vec<String> = if cfg!(windows) {
        vec!["cmd.exe".into(), "/c".into(), "type seed.txt".into()]
    } else {
        vec!["/bin/sh".into(), "-c".into(), "cat seed.txt".into()]
    };
    let out = {
        let (codex, args, home, cwd) = (codex.clone(), args.clone(), home.clone(), s.cwd.clone());
        tokio::task::spawn_blocking(move || {
            codex_exec(&codex, &args, &home, &cwd, &read, serde_json::json!({}))
        })
        .await
        .unwrap()
    };
    assert!(
        out["result"]["stdout"]
            .as_str()
            .unwrap_or_default()
            .contains("seed-text"),
        "{out}"
    );
    let outside = outside_temp();
    for target in [
        s.cwd.join("written.txt"),
        outside.path().join("written.txt"),
    ] {
        let command: Vec<String> = if cfg!(windows) {
            vec![
                "cmd.exe".into(),
                "/c".into(),
                // No quotes: the path has no space, and cmd.exe reads quotes its own way.
                format!("echo x> {}", target.display()),
            ]
        } else {
            vec![
                "/bin/sh".into(),
                "-c".into(),
                format!("echo x > '{}'", target.display()),
            ]
        };
        let (codex, args, home, cwd) = (codex.clone(), args.clone(), home.clone(), s.cwd.clone());
        let out = tokio::task::spawn_blocking(move || {
            codex_exec(&codex, &args, &home, &cwd, &command, serde_json::json!({}))
        })
        .await
        .unwrap();
        assert!(
            !target.exists(),
            "a read-only run wrote {}: {out}",
            target.display()
        );
    }
}

/// Windows: a Codex run reaches the daemon through the request folder. With the sandbox the
/// daemon sends for a run (`danger-full-access`) the real CLI starts the real `chat-run`
/// client, the request arrives, and the answer comes back; the folder is left clean. Through
/// `command/exec` (no model call).
#[cfg(windows)]
#[tokio::test(flavor = "multi_thread")]
#[ignore = "real CLI: scripts/test-real-cli.sh"]
async fn codex_on_windows_reaches_the_daemon_through_the_request_folder() {
    let codex = real_codex();
    // Also for a user whose own Codex config turns Codex's Windows sandbox on: a run has full
    // access whatever the config says, so the sandbox is not used for it.
    for user_config in ["", "[windows]\nsandbox = \"unelevated\"\n"] {
        codex_reaches_the_daemon_through_the_request_folder(&codex, user_config).await;
    }
}

#[cfg(windows)]
async fn codex_reaches_the_daemon_through_the_request_folder(
    codex: &daemon::health::Launch,
    user_config: &str,
) {
    let codex = codex.clone();
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path().join("codex-home");
    std::fs::create_dir_all(&home).unwrap();
    std::fs::write(home.join("config.toml"), user_config).unwrap();
    let mut s = spec(tmp.path(), "codex", "codex", &[], "hi");
    s.note = Some(daemon::chat_run::note());
    assert!(runtime::codex::chat_run_works(&s));
    let (_, sandbox) = runtime::codex::thread_policy(&s);
    let mut args = runtime::codex::app_server_args(&s).unwrap();
    args.extend(["-c".to_string(), format!("sandbox_mode=\"{sandbox}\"")]);

    daemon::chat_run::set_exe(std::path::PathBuf::from(env!("CARGO_BIN_EXE_daemon")));
    let bin = daemon::chat_run::install_link(tmp.path()).unwrap();
    let outputs = std::sync::Arc::new(daemon::chat_run::Outputs::default());
    let (tx, mut rx) = mpsc::unbounded_channel();
    let spool = daemon::chat_run::Spool::start(
        daemon::chat_run::spool_dir(&s.cwd, "x1"),
        outputs.clone(),
        move |e| {
            let _ = tx.send(e);
        },
        daemon::chat_run::TIMEOUT,
    )
    .unwrap();
    let command: Vec<String> = [
        bin.join(daemon::chat_run::link_name())
            .to_string_lossy()
            .to_string(),
        "resolve".into(),
        "--event".into(),
        "EV1".into(),
        "--result".into(),
        "ignored".into(),
    ]
    .into();
    let run = tokio::task::spawn_blocking({
        let (home, cwd) = (home.clone(), s.cwd.clone());
        let env = serde_json::json!({ "CHAT_RUN_DIR": spool.path().to_string_lossy() });
        move || codex_exec(&codex, &args, &home, &cwd, &command, env)
    });
    let ev = tokio::time::timeout(Duration::from_secs(60), rx.recv())
        .await
        .expect("chat-run did not reach the daemon")
        .unwrap();
    let RunEvent::Output { output_id, op, .. } = ev else {
        panic!("not an output event: {ev:?}");
    };
    assert_eq!(op, "interaction.resolve");
    assert!(outputs.resolve(daemon::proto::RunOutputResult {
        run_id: "r1".into(),
        output_id,
        ok: true,
        result: Some(serde_json::json!({ "resolved": true })),
        error: None,
    }));
    let done = run.await.unwrap();
    assert_eq!(done["result"]["exitCode"], 0, "{done}");
    assert!(
        done["result"]["stdout"]
            .as_str()
            .unwrap_or_default()
            .contains("resolved"),
        "{done}"
    );
    let left: Vec<_> = std::fs::read_dir(spool.path())
        .unwrap()
        .map(|e| e.unwrap().file_name())
        .collect();
    assert_eq!(left, [std::ffi::OsString::from(".gitignore")]);
}

/// A session id is random on every system. (It was read from `/dev/urandom`: on Windows every
/// Claude Code session had the same id, and a follow-up run could resume another topic's.)
#[test]
fn session_ids_are_random_on_every_system() {
    let ids: std::collections::BTreeSet<String> = (0..64).map(|_| runtime::uuid_v4()).collect();
    assert_eq!(ids.len(), 64, "{ids:?}");
    for id in &ids {
        assert!(runtime::valid_session_id(id), "{id}");
        assert_eq!(id.len(), 36);
        assert_eq!(&id[14..15], "4", "version 4: {id}");
    }
}

/// The folder Claude Code keeps a session in is named after the working directory. Unix: the
/// canonical path. Windows: the path as given (the CLI keeps 8.3 short names), then the
/// canonical one; a resume puts the session where the CLI looks under either name.
#[test]
fn the_session_folder_has_the_names_the_cli_can_use() {
    use runtime::claude::{
        find_session_file, prepare_resume, project_dir_name_of, project_dir_names,
    };
    assert_eq!(project_dir_name_of(r"C:\Users\me\work"), "C--Users-me-work");
    assert_eq!(
        project_dir_name_of(r"\\?\C:\Users\RUNNER~1\work"),
        "C--Users-RUNNER-1-work"
    );
    assert_eq!(project_dir_name_of("/Users/张三/a b"), "-Users----a-b");
    // One UTF-16 unit, one `-`: a character outside the basic plane gives two.
    assert_eq!(project_dir_name_of("/x/😀"), "-x---");

    let dir = tempfile::tempdir().unwrap();
    let cwd = dir.path().join("work");
    std::fs::create_dir_all(&cwd).unwrap();
    let names = project_dir_names(&cwd);
    assert!(!names.is_empty() && names.len() <= 2, "{names:?}");
    if cfg!(windows) {
        assert_eq!(names[0], project_dir_name_of(&cwd.to_string_lossy()));
    } else {
        assert_eq!(names.len(), 1);
    }
    // A session of an earlier run, in another project's folder.
    let config = dir.path().join("claude");
    let old = config.join("projects").join("-some-earlier-run");
    std::fs::create_dir_all(&old).unwrap();
    std::fs::write(old.join("s1.jsonl"), "{}").unwrap();
    assert_eq!(
        find_session_file(&config, &cwd, "s1"),
        Some(old.join("s1.jsonl"))
    );
    assert!(prepare_resume(&config, &cwd, "s1"));
    for name in &names {
        assert!(
            config
                .join("projects")
                .join(name)
                .join("s1.jsonl")
                .is_file(),
            "{name}"
        );
    }
    assert_eq!(
        find_session_file(&config, &cwd, "s1"),
        Some(config.join("projects").join(&names[0]).join("s1.jsonl"))
    );
    assert!(!prepare_resume(&config, &cwd, "gone"));
}

/// A Codex run always has full access, whatever `daemon.toml` says, so `chat-run` always works
/// and the daemon adds no sandbox overrides. A user's own args stay as written.
#[test]
fn codex_runs_always_have_full_access() {
    let tmp = tempfile::tempdir().unwrap();
    let mut s = spec(tmp.path(), "codex", "codex", &[], "q");
    s.note = Some("n".into());
    for configured in [
        None,
        Some("workspace-write"),
        Some("read-only"),
        Some("danger-full-access"),
    ] {
        s.cfg.sandbox = configured.map(String::from);
        assert_eq!(
            runtime::codex::thread_policy(&s),
            ("never".to_string(), "danger-full-access".to_string()),
            "{configured:?}"
        );
        assert!(runtime::codex::chat_run_works(&s));
        assert_eq!(
            runtime::codex::app_server_args(&s).unwrap(),
            ["app-server", "-c", "features.memories=false"]
        );
    }
    // Gated actions: still full access, approval `untrusted` so commands come to the daemon;
    // a side chat stays read-only with `never`.
    s.gates = Some(merge_gates());
    assert_eq!(
        runtime::codex::thread_policy(&s),
        ("untrusted".to_string(), "danger-full-access".to_string())
    );
    assert!(runtime::codex::chat_run_works(&s));
    s.aside = true;
    assert_eq!(
        runtime::codex::thread_policy(&s),
        ("never".to_string(), "read-only".to_string())
    );
    s.aside = false;
    s.gates = None;
    s.cfg.args = vec!["-c".into(), "model_reasoning_effort=\"high\"".into()];
    assert_eq!(
        runtime::codex::app_server_args(&s).unwrap(),
        [
            "app-server",
            "-c",
            "model_reasoning_effort=\"high\"",
            "-c",
            "features.memories=false"
        ]
    );
}

/// A side chat run keeps only the auth env, `apiKeyHelper` and model of the user's
/// `settings.json`; hooks, permissions and plugins stay out.
#[test]
fn claude_aside_settings_keep_only_auth_and_model() {
    let dir = tempfile::tempdir().unwrap();
    assert!(runtime::claude::aside_settings(dir.path()).is_none());
    std::fs::write(
        dir.path().join("settings.json"),
        r#"{"env":{"ANTHROPIC_BASE_URL":"https://x.example","N":1},"apiKeyHelper":"~/bin/key",
            "model":"opus","hooks":{"Stop":[{"hooks":[{"type":"command","command":"rm -rf ~"}]}]},
            "permissions":{"allow":["Bash(*)"]},"enabledPlugins":{"p":true},"statusLine":{}}"#,
    )
    .unwrap();
    assert_eq!(
        runtime::claude::aside_settings(dir.path()).unwrap(),
        serde_json::json!({ "env": { "ANTHROPIC_BASE_URL": "https://x.example" }, "apiKeyHelper": "~/bin/key", "model": "opus" })
    );
    std::fs::write(dir.path().join("settings.json"), r#"{"hooks":{}}"#).unwrap();
    assert!(runtime::claude::aside_settings(dir.path()).is_none());
}

/// `run.assign.resumeSessionId` for Codex: `thread/resume` with the thread; a thread Codex no
/// longer has falls back to a new thread.
#[tokio::test]
async fn codex_resumes_the_thread_or_starts_a_new_one() {
    for (tid, want, calls) in [
        (
            "th-old",
            "th-old",
            serde_json::json!([["thread/resume", "th-old"]]),
        ),
        (
            "th-gone",
            "th-1",
            serde_json::json!([["thread/resume", "th-gone"], ["thread/start", null]]),
        ),
    ] {
        let dir = tempfile::tempdir().unwrap();
        let mut s = spec(dir.path(), "codex", &fake("fake_codex.py"), &[], "hello");
        s.resume_session_id = Some(tid.into());
        let (o, evs) = drive(s, |_| None).await;
        assert_eq!(o.status, Status::Succeeded, "{:?}", o.error);
        assert_eq!(o.session_id.as_deref(), Some(want));
        assert!(
            evs.iter()
                .any(|e| matches!(e, RunEvent::Session { session_id, .. } if session_id == want))
        );
        let seen: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string(dir.path().join("work/threads.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(seen, calls);
        // Codex's own memory is off (存 3.6).
        let argv: Vec<String> = serde_json::from_str(
            &std::fs::read_to_string(dir.path().join("work/argv.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(argv, ["app-server", "-c", "features.memories=false"]);
    }
}

/// `run.assign.resumeSessionId` for Claude Code: the earlier run's session file is copied into
/// the project folder of this run's working directory and the CLI gets `--resume`; a session
/// that is gone, or an id that is not a plain id, starts a new session.
#[test]
fn claude_resume_copies_the_session_into_this_project() {
    let dir = tempfile::tempdir().unwrap();
    let cfg = dir.path().join("claude");
    let old = cfg.join("projects/-old-run-work");
    std::fs::create_dir_all(&old).unwrap();
    std::fs::write(old.join("s1.jsonl"), "{}\n").unwrap();
    let cwd = dir.path().join("work");
    std::fs::create_dir_all(&cwd).unwrap();
    assert!(runtime::claude::prepare_resume(&cfg, &cwd, "s1"));
    let copied = cfg
        .join("projects")
        .join(runtime::claude::project_dir_name(&cwd))
        .join("s1.jsonl");
    assert_eq!(std::fs::read_to_string(copied).unwrap(), "{}\n");
    assert!(!runtime::claude::prepare_resume(&cfg, &cwd, "s2"));
    assert!(!runtime::claude::prepare_resume(&cfg, &cwd, "../s1"));
}

/// Feature `effort`: Claude Code gets `--effort <effort>` as its own arguments after `--model`;
/// Codex gets `effort` in each `turn/start`. No effort, no flag.
#[test]
fn effort_reaches_claude_args_and_codex_turns() {
    let dir = tempfile::tempdir().unwrap();
    let mut s = spec(dir.path(), "claude", "claude", &[], "x");
    assert!(runtime::claude::model_args(&s).is_empty());
    s.model = Some("opus".into());
    s.effort = Some("xhigh".into());
    assert_eq!(
        runtime::claude::model_args(&s),
        ["--model", "opus", "--effort", "xhigh"]
    );
    let p = runtime::codex::turn_start_params("th", "hi", Some("high"));
    assert_eq!(p["effort"], "high");
    assert_eq!(p["threadId"], "th");
    assert!(
        runtime::codex::turn_start_params("th", "hi", None)
            .get("effort")
            .is_none()
    );
}

/// Codex gets the run's effort on the first turn and on the turn a late follow-up starts.
#[tokio::test]
async fn codex_effort_on_every_turn() {
    let dir = tempfile::tempdir().unwrap();
    let mut s = spec(
        dir.path(),
        "codex",
        &fake("fake_codex.py"),
        &[],
        "late work",
    );
    s.effort = Some("high".into());
    let mut steered = false;
    let (o, _) = drive(s, |e| match e {
        RunEvent::MessageDelta { .. } if !steered => {
            steered = true;
            Some(Control::Input {
                input_id: "in1".into(),
                text: "also docs".into(),
            })
        }
        _ => None,
    })
    .await;
    assert_eq!(o.status, Status::Succeeded, "{o:?}");
    let seen: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(dir.path().join("work/efforts.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(seen, serde_json::json!(["high", "high"]));
}

/// `hello` discovery: Codex through `model/list` (two pages, a hidden model, a notification in
/// between), Claude Code through its aliases, `settings.json` and `--help`, and daemon.toml
/// lists replacing what a CLI reported.
#[tokio::test]
async fn discovery_reports_models_and_efforts() {
    let dir = tempfile::tempdir().unwrap();
    let claude_dir = dir.path().join("claude");
    std::fs::create_dir_all(&claude_dir).unwrap();
    std::fs::write(claude_dir.join("settings.json"), r#"{"model":"sonnet"}"#).unwrap();
    // The paths are TOML literal strings: a Windows path has backslashes.
    let (codex, codex_script) = launch(&fake("fake_codex.py"));
    let (claude, claude_script) = launch(&fake("fake_claude.py"));
    let script = |s: &Option<(String, String)>| match s {
        Some((k, v)) => format!("{k} = '{v}'"),
        None => String::new(),
    };
    let cfg = daemon::config::Config::parse(&format!(
        r#"
server_url = "http://127.0.0.1:1"
token_file = '{token}'
[runtimes.cx]
kind = "codex"
command = '{codex}'
env = {{ {codex_env} }}
[runtimes.cl]
kind = "claude"
command = '{claude}'
env = {{ CLAUDE_CONFIG_DIR = '{claude_dir}', {claude_env} }}
[runtimes.fixed]
kind = "codex"
command = '{codex}'
env = {{ {codex_env} }}
models = ["fake-codex", "other"]
[runtimes.sh]
kind = "pty"
command = '{shell}'
"#,
        token = dir.path().join("token").display(),
        codex_env = script(&codex_script),
        claude_env = script(&claude_script),
        claude_dir = claude_dir.display(),
        shell = if cfg!(windows) { "cmd.exe" } else { "/bin/sh" },
    ))
    .unwrap();
    let clis = daemon::health::discover_clis(&cfg).await;
    let get = |n: &str| clis.iter().find(|c| c.runtime == n).unwrap();
    let ids = |n: &str| {
        get(n)
            .models
            .iter()
            .flatten()
            .map(|m| m.id.clone())
            .collect::<Vec<_>>()
    };

    assert_eq!(ids("cx"), ["fake-codex", "fake-mini"]);
    let cx = get("cx").models.as_ref().unwrap();
    assert_eq!(cx[0].name.as_deref(), Some("FAKE-CODEX"));
    assert_eq!(cx[0].is_default, Some(true));
    assert_eq!(cx[0].default_effort.as_deref(), Some("low"));
    assert_eq!(cx[1].is_default, None);
    assert_eq!(
        get("cx").efforts.as_deref().unwrap(),
        ["low", "high", "minimal"]
    );

    assert_eq!(ids("cl"), ["fable", "opus", "sonnet", "haiku"]);
    let cl = get("cl").models.as_ref().unwrap();
    assert_eq!(
        cl.iter()
            .filter(|m| m.is_default == Some(true))
            .map(|m| m.id.as_str())
            .collect::<Vec<_>>(),
        ["sonnet"]
    );
    assert_eq!(
        get("cl").efforts.as_deref().unwrap(),
        ["low", "medium", "high", "xhigh", "max"]
    );

    // daemon.toml `models` replaces the list; an id Codex listed keeps its details.
    assert_eq!(ids("fixed"), ["fake-codex", "other"]);
    let fixed = get("fixed").models.as_ref().unwrap();
    assert_eq!(fixed[0].name.as_deref(), Some("FAKE-CODEX"));
    assert_eq!(fixed[1].name, None);

    assert_eq!(get("sh").models, None);
    assert_eq!(get("sh").efforts, None);
}

/// Verification pass 2, problem 4: a turn that ends while a command it put in the background
/// still runs is not the end of the work; the CLI's own turn on the command's notification is.
#[tokio::test]
async fn claude_waits_for_its_background_command_and_its_own_turn_after_it() {
    let dir = tempfile::tempdir().unwrap();
    let s = spec(
        dir.path(),
        "claude",
        &fake("fake_claude.py"),
        &[],
        "background bgself",
    );
    let (o, _) = drive(s, |_| None).await;
    assert_eq!(o.status, Status::Succeeded, "{o:?}");
    assert_eq!(
        o.result_text.as_deref(),
        Some("continued after the background command")
    );
}

/// When the CLI only clears the notification, the daemon tells it to go on.
#[tokio::test]
async fn claude_is_told_to_go_on_when_its_background_command_finished() {
    let dir = tempfile::tempdir().unwrap();
    let s = spec(
        dir.path(),
        "claude",
        &fake("fake_claude.py"),
        &[],
        "background bgidle",
    );
    let (o, _) = drive(s, |_| None).await;
    assert_eq!(o.status, Status::Succeeded, "{o:?}");
    let text = o.result_text.unwrap_or_default();
    assert!(text.starts_with("reply 2: "), "{text}");
    assert!(text.contains(runtime::claude::BACKGROUND_DONE), "{text}");
}

/// A resumed session first clears the notification of a background command the restart
/// stopped (a `result` of no turns and no text): that is not the answer to the prompt.
#[tokio::test]
async fn claude_does_not_take_a_cleared_notification_for_the_answer() {
    let dir = tempfile::tempdir().unwrap();
    let s = spec(
        dir.path(),
        "claude",
        &fake("fake_claude.py"),
        &[],
        "stalenote go on",
    );
    let (o, evs) = drive(s, |_| None).await;
    assert_eq!(o.status, Status::Succeeded, "{o:?}");
    assert_eq!(o.result_text.as_deref(), Some("reply 1: stalenote go on"));
    let ended = evs
        .iter()
        .filter(|e| matches!(e, RunEvent::TurnCompleted { .. }))
        .count();
    assert_eq!(ended, 1);
}

/// A background command that never finishes (a server) gets one line after the wait, and the
/// turn after it ends the run: the run does not sit quiet until the server nudges or restarts it.
#[tokio::test]
async fn claude_is_asked_once_about_a_background_command_that_still_runs() {
    runtime::claude::set_background_wait(std::time::Duration::from_millis(1500));
    let dir = tempfile::tempdir().unwrap();
    let s = spec(
        dir.path(),
        "claude",
        &fake("fake_claude.py"),
        &[],
        "background bgnever",
    );
    let (o, evs) = drive(s, |_| None).await;
    assert_eq!(o.status, Status::Succeeded, "{o:?}");
    let text = o.result_text.unwrap_or_default();
    assert!(text.starts_with("reply 2: "), "{text}");
    assert!(text.contains(runtime::claude::BACKGROUND_STILL), "{text}");
    let ended = evs
        .iter()
        .filter(|e| matches!(e, RunEvent::TurnCompleted { .. }))
        .count();
    assert_eq!(ended, 2);
}

/// `run.assign.fullPrompt` (design-vm-state §3.2): a session Claude Code no longer has starts a
/// new one with `--session-id`, and the CLI gets the whole prompt instead of only what is new;
/// without `fullPrompt`, or when the session is found, the short prompt as before.
#[tokio::test]
async fn claude_gets_the_full_prompt_when_the_session_is_gone() {
    let argv = |dir: &std::path::Path| -> Vec<String> {
        serde_json::from_str(&std::fs::read_to_string(dir.join("work/argv.json")).unwrap()).unwrap()
    };
    // Gone, with the whole prompt.
    let dir = tempfile::tempdir().unwrap();
    let mut s = spec(
        dir.path(),
        "claude",
        &fake("fake_claude.py"),
        &[],
        "only new",
    );
    s.resume_session_id = Some("gone".into());
    s.full_prompt = Some("whole topic then only new".into());
    let (o, _) = drive(s, |_| None).await;
    assert_eq!(o.status, Status::Succeeded, "{o:?}");
    assert_eq!(
        o.result_text.as_deref(),
        Some("reply 1: whole topic then only new")
    );
    assert!(o.resume_missed);
    let args = argv(dir.path());
    assert!(args.contains(&"--session-id".to_string()), "{args:?}");
    assert!(!args.contains(&"--resume".to_string()), "{args:?}");

    // Gone, from an older server: the short prompt, nothing reported.
    let dir = tempfile::tempdir().unwrap();
    let mut s = spec(
        dir.path(),
        "claude",
        &fake("fake_claude.py"),
        &[],
        "only new",
    );
    s.resume_session_id = Some("gone".into());
    let (o, _) = drive(s, |_| None).await;
    assert_eq!(o.result_text.as_deref(), Some("reply 1: only new"));
    assert!(!o.resume_missed);
    assert!(argv(dir.path()).contains(&"--session-id".to_string()));

    // Found: `--resume` and the short prompt, whatever `fullPrompt` holds.
    let dir = tempfile::tempdir().unwrap();
    let mut s = spec(
        dir.path(),
        "claude",
        &fake("fake_claude.py"),
        &[],
        "only new",
    );
    let old = dir.path().join(".claude/projects/-old-run-work");
    std::fs::create_dir_all(&old).unwrap();
    std::fs::write(old.join("s1.jsonl"), "{}\n").unwrap();
    s.resume_session_id = Some("s1".into());
    s.full_prompt = Some("whole topic then only new".into());
    let (o, _) = drive(s, |_| None).await;
    assert_eq!(o.status, Status::Succeeded, "{o:?}");
    assert_eq!(o.result_text.as_deref(), Some("reply 1: only new"));
    assert!(!o.resume_missed);
    assert!(argv(dir.path()).contains(&"--resume".to_string()));
}

/// A thread Codex no longer has: `thread/start`, and the first `turn/start` carries the whole
/// prompt; a thread it has, or no `fullPrompt`, keeps the short prompt.
#[tokio::test]
async fn codex_gets_the_full_prompt_when_the_thread_is_gone() {
    for (tid, full, reply, missed) in [
        ("th-gone", Some("whole"), "codex: whole", true),
        ("th-gone", None, "codex: only new", false),
        ("th-old", Some("whole"), "codex: only new", false),
    ] {
        let dir = tempfile::tempdir().unwrap();
        let mut s = spec(dir.path(), "codex", &fake("fake_codex.py"), &[], "only new");
        s.resume_session_id = Some(tid.into());
        s.full_prompt = full.map(str::to_string);
        let (o, _) = drive(s, |_| None).await;
        assert_eq!(o.status, Status::Succeeded, "{:?}", o.error);
        assert_eq!(o.result_text.as_deref(), Some(reply), "{tid} {full:?}");
        assert_eq!(o.resume_missed, missed, "{tid} {full:?}");
    }
}

/// An ACP session the agent no longer has: the first prompt is the whole prompt (after the run
/// note), not the lost-session note and the short prompt.
#[tokio::test]
async fn acp_gets_the_full_prompt_when_the_session_is_lost() {
    let dir = tempfile::tempdir().unwrap();
    let mut s = spec(dir.path(), "acp", &fake("fake_acp.py"), &[], "go on");
    s.resume_session_id = Some("gone".into());
    s.full_prompt = Some("whole topic, go on".into());
    s.note = Some("NOTE".into());
    let (o, _) = drive(s.clone(), |_| None).await;
    assert_eq!(o.status, Status::Succeeded, "{o:?}");
    let text = o.result_text.unwrap();
    assert!(
        text.ends_with("NOTE\n\nwhole topic, go on")
            && !text.contains(runtime::acp::LOST_SESSION_NOTE),
        "{text}"
    );
    assert!(o.resume_missed);
    assert_eq!(
        runtime::acp::first_prompt(&s, false),
        "NOTE\n\ngo on",
        "a session that loads keeps the short prompt"
    );
    // No `fullPrompt`: told the session is new, as before; nothing reported.
    let dir = tempfile::tempdir().unwrap();
    let mut s = spec(dir.path(), "acp", &fake("fake_acp.py"), &[], "go on");
    s.resume_session_id = Some("gone".into());
    let (o, _) = drive(s, |_| None).await;
    assert!(
        o.result_text
            .unwrap()
            .contains(runtime::acp::LOST_SESSION_NOTE)
    );
    assert!(!o.resume_missed);
}
