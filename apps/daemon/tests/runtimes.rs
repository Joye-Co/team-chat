//! Runtime drivers against fake CLIs that follow each CLI's documented protocol.

use daemon::config::RuntimeConfig;
use daemon::proto::RunEvent;
use daemon::runtime::{self, Control, Outcome, RunSpec, Status};
use std::collections::BTreeMap;
use std::time::Duration;
use tokio::sync::mpsc;

fn fake(name: &str) -> String {
    format!("{}/tests/fake/{name}", env!("CARGO_MANIFEST_DIR"))
}

fn spec(dir: &std::path::Path, kind: &str, command: &str, args: &[&str], prompt: &str) -> RunSpec {
    let cwd = dir.join("work");
    std::fs::create_dir_all(&cwd).unwrap();
    RunSpec {
        run_id: "r1".into(),
        runtime_name: kind.into(),
        cfg: RuntimeConfig {
            kind: kind.into(),
            command: Some(command.into()),
            args: args.iter().map(|s| s.to_string()).collect(),
            env: BTreeMap::new(),
            model: None,
            approval_policy: Some("on-request".into()),
            sandbox: None,
            permission_mode: None,
            allowed_tools: None,
        },
        prompt: prompt.into(),
        model: None,
        cwd,
        run_dir: dir.to_path_buf(),
        env: vec![
            ("PATH".into(), std::env::var("PATH").unwrap()),
            ("HOME".into(), dir.to_string_lossy().into()),
        ],
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

#[tokio::test]
async fn acp_permission_flow() {
    let dir = tempfile::tempdir().unwrap();
    let s = spec(dir.path(), "acp", &fake("fake_acp.py"), &[], "approve this");
    let (o, evs) = drive(s, |e| match e {
        RunEvent::ApprovalRequest { approval_id, .. } => Some(Control::Approval {
            approval_id: approval_id.clone(),
            approve: false,
        }),
        _ => None,
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

#[tokio::test]
async fn acp_follow_up_is_next_prompt() {
    let dir = tempfile::tempdir().unwrap();
    let s = spec(dir.path(), "acp", &fake("fake_acp.py"), &[], "approve");
    let mut sent = false;
    let (o, evs) = drive(s, |e| match e {
        RunEvent::ApprovalRequest { approval_id, .. } => Some(Control::Approval {
            approval_id: approval_id.clone(),
            approve: true,
        }),
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

#[tokio::test]
async fn pty_prompt_input_and_exit_code() {
    let dir = tempfile::tempdir().unwrap();
    let s = spec(
        dir.path(),
        "pty",
        "/bin/sh",
        &["-c", "echo start:{prompt}; read x; echo got:$x; exit 3"],
        "hi",
    );
    let mut sent = false;
    let (o, _evs) = drive(s, |e| match e {
        RunEvent::MessageDelta { text, .. } if text.contains("start:hi") && !sent => {
            sent = true;
            Some(Control::Input {
                input_id: "i".into(),
                text: "abc".into(),
            })
        }
        _ => None,
    })
    .await;
    assert_eq!(o.status, Status::Failed);
    assert_eq!(o.exit_code, Some(3));
    assert!(o.result_text.unwrap().contains("got:abc"));
    assert!(dir.path().join("pty.log").is_file());
}

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
