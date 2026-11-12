//! Secret redaction of run frames and the delta gate.

use daemon::proto::{FileChange, RunEvent, Up};
use daemon::redact::{DeltaGate, MASK, Redactor};

fn r() -> Redactor {
    Redactor::new(["s3cr3t-passthrough-value".to_string(), "short".to_string()])
}

#[test]
fn masks_known_token_formats_and_exact_values() {
    let red = r();
    let gh = format!("ghp_{}", "a1B2".repeat(9));
    let pat = format!("github_pat_{}", "x9".repeat(20));
    let sk = format!("sk-proj-{}", "Ab3_".repeat(8));
    let ant = format!("sk-ant-api03-{}", "Zz9-".repeat(8));
    let tm = format!("tm_{}", "q".repeat(43));
    let tc = format!("tc_{}", "Q1".repeat(22));
    let cases = [
        format!("Token: {gh}"),
        format!("GH_TOKEN={pat}"),
        format!("export OPENAI_API_KEY={sk}"),
        format!("key {ant} here"),
        format!("machine {tm}"),
        format!("member {tc}"),
        "Authorization: Bearer abc.def-123456789".to_string(),
        "curl -H 'x-api-key: hunter2hunter2'".to_string(),
        "https://deploy:pa55w0rd@github.com/x/y.git".to_string(),
        "https://x.r2.dev/f?X-Amz-Signature=abcdef123&ok=1".to_string(),
        "AWS_SECRET_ACCESS_KEY=wJalrXUtnFEMI/K7MDENG/bPxRfiCYEXAMPLEKEY".to_string(),
        "id AKIAIOSFODNN7EXAMPLE".to_string(),
        "invite code 7K2M-9QXR".to_string(),
        "value is s3cr3t-passthrough-value.".to_string(),
        "-----BEGIN OPENSSH PRIVATE KEY-----\nb3BlbnNzaC1rZXktdjEAAAA\n-----END OPENSSH PRIVATE KEY-----".to_string(),
    ];
    for c in &cases {
        let out = red.text(c);
        assert!(out.contains(MASK), "not masked: {c} -> {out}");
    }
    for secret in [&gh, &pat, &sk, &ant, &tm, &tc] {
        assert!(!red.text(&format!("x {secret} y")).contains(secret.as_str()));
    }
    assert!(!red.text(&cases[13]).contains("s3cr3t"));
    assert!(!red.text(&cases[14]).contains("b3BlbnNz"));
    assert!(!red.text(&cases[8]).contains("pa55w0rd"));
}

#[test]
fn leaves_ordinary_code_and_ids_alone() {
    let red = r();
    for c in [
        "let token: String = read_token(&path)?;",
        "fn password(&self) -> &str { &self.password }",
        "id 550e8400-e29b-41d4-a716-446655440000",
        "this is type-safe and read-only",
        "short words stay short",
        "token = compute()",
        "see ABCD-EFGH in the docs",
    ] {
        assert_eq!(red.text(c), c);
    }
}

#[test]
fn frames_redact_free_text_but_not_ids() {
    let red = r();
    let gh = format!("ghp_{}", "a1B2".repeat(9));
    let up = Up::RunEvent {
        run_id: "run-1".into(),
        seq: 4,
        at: 1,
        event: RunEvent::ToolResult {
            item_id: "item-1".into(),
            turn_id: None,
            ok: true,
            output: Some(format!("GITHUB_TOKEN={gh}\n")),
            exit_code: Some(0),
        },
    };
    let text = red.frame(&up).unwrap();
    assert!(!text.contains(&gh), "{text}");
    let back: Up = serde_json::from_str(&text).unwrap();
    let Up::RunEvent { run_id, event, .. } = back else {
        panic!()
    };
    assert_eq!(run_id, "run-1");
    assert!(matches!(event, RunEvent::ToolResult { item_id, .. } if item_id == "item-1"));

    let diff = Up::RunEvent {
        run_id: "run-1".into(),
        seq: 5,
        at: 1,
        event: RunEvent::FileChange {
            item_id: "f".into(),
            turn_id: None,
            changes: vec![FileChange {
                path: ".env".into(),
                kind: "add".into(),
                diff: Some("+API_KEY=s3cr3t-passthrough-value\n".into()),
            }],
        },
    };
    assert!(!red.frame(&diff).unwrap().contains("s3cr3t"));
    let call = Up::RunEvent {
        run_id: "run-1".into(),
        seq: 6,
        at: 1,
        event: RunEvent::ToolCall {
            item_id: "c".into(),
            turn_id: None,
            tool: "shell".into(),
            title: format!("echo {gh}"),
            input: Some(serde_json::json!({ "command": format!("echo {gh}") })),
        },
    };
    assert!(!red.frame(&call).unwrap().contains(&gh));
}

fn delta(item: &str, text: &str) -> RunEvent {
    RunEvent::MessageDelta {
        item_id: item.into(),
        turn_id: None,
        text: text.into(),
    }
}

fn texts(evs: &[RunEvent]) -> String {
    evs.iter()
        .map(|e| match e {
            RunEvent::MessageDelta { text, .. } => text.clone(),
            _ => String::new(),
        })
        .collect()
}

#[test]
fn delta_gate_keeps_a_split_secret_whole() {
    let red = r();
    let gh = format!("ghp_{}", "a1B2".repeat(9));
    let (a, b) = gh.split_at(10);
    let mut g = DeltaGate::default();
    let mut out = vec![];
    out.extend(g.push(delta("m", "first line\nthe token is ")));
    out.extend(g.push(delta("m", a)));
    out.extend(g.push(delta("m", b)));
    out.extend(g.push(delta("m", " done\nmore")));
    // Each released chunk ends a line, so redacting chunk by chunk masks the whole token.
    let redacted: String = out
        .iter()
        .map(|e| match e {
            RunEvent::MessageDelta { text, .. } => red.text(text),
            _ => String::new(),
        })
        .collect();
    assert!(!redacted.contains(&gh), "{redacted}");
    assert_eq!(texts(&out), format!("first line\nthe token is {gh} done\n"));
    assert!(g.holding());
    // Another event releases the held tail first, in order.
    let rest = g.push(RunEvent::TurnStarted {
        turn_id: "t".into(),
    });
    assert_eq!(texts(&rest), "more");
    assert!(matches!(rest.last(), Some(RunEvent::TurnStarted { .. })));
    assert!(!g.holding());
}

#[test]
fn delta_gate_final_message_replaces_held_text() {
    let mut g = DeltaGate::default();
    assert!(g.push(delta("m", "no line end yet")).is_empty());
    let out = g.push(RunEvent::Message {
        item_id: "m".into(),
        turn_id: None,
        role: "assistant".into(),
        text: "no line end yet, and more".into(),
    });
    assert_eq!(out.len(), 1);
    assert!(matches!(&out[0], RunEvent::Message { .. }));
    // Held text: while the stream goes on, all but the last two words go out after HOLD; once
    // it pauses, all of it (a prompt without a line end).
    assert!(g.push(delta("p", "one two three four")).is_empty());
    let soon = std::time::Instant::now() + std::time::Duration::from_millis(500);
    assert_eq!(texts(&g.tick(soon)), "one two ");
    assert!(g.holding());
    let later = std::time::Instant::now() + daemon::redact::HOLD * 2;
    assert_eq!(texts(&g.tick(later)), "three four");
    assert!(!g.holding());
    assert!(g.push(delta("p", "Password: ")).is_empty());
    assert_eq!(texts(&g.tick(later + daemon::redact::HOLD)), "Password: ");
}

/// `gh auth login` prints a one-time code that looks like an invite code. The person has to
/// read it, so it stays, also in the card the CLI posts afterwards; other codes stay masked.
#[test]
fn device_sign_in_codes_are_shown() {
    let red = Redactor::new(vec![]);
    assert!(red.text("say 7AB5-DFD1 to nobody").contains(MASK));
    let out = red.text("! First copy your one-time code: 7AB5-DFD1\nOpen this URL to continue in your web browser: https://github.com/login/device");
    assert!(out.contains("7AB5-DFD1"), "{out}");
    assert_eq!(red.text("输入验证码 7AB5-DFD1"), "输入验证码 7AB5-DFD1");
    assert!(red.text("invite code 7K2M-9QXR").contains(MASK));
}

/// Feature "github": a run's redactor takes the tokens the run fetched while it runs; its
/// clones (the run's frames, its files, its session upload) see them, other runs do not.
#[test]
fn a_runs_list_grows_and_stays_with_the_run() {
    let daemon_wide = Redactor::new(vec!["machine-secret-1".to_string()]);
    let run = daemon_wide.for_run();
    let other = daemon_wide.for_run();
    let shared = run.clone();
    // Not a pattern the rules know: only the exact value masks it.
    let token = "plainvalue1234567890";
    assert_eq!(run.text(token), token);
    assert!(!run.has_added());
    run.add(token);
    run.add("short");
    assert!(run.has_added());
    assert_eq!(
        shared.text(&format!("pushed with {token}")),
        "pushed with <redacted>"
    );
    assert_eq!(other.text(token), token);
    assert_eq!(daemon_wide.text(token), token);
    // The daemon's own values still apply in the run.
    assert_eq!(run.text("machine-secret-1"), MASK);
    // `stderr.log`, which the CLI writes itself, is masked in place after the run.
    let dir = tempfile::tempdir().unwrap();
    let log = dir.path().join("stderr.log");
    std::fs::write(&log, format!("remote: token {token} rejected\n")).unwrap();
    run.file(&log);
    assert_eq!(
        std::fs::read_to_string(&log).unwrap(),
        "remote: token <redacted> rejected\n"
    );
}

/// Feature "github-host": the token a VM run got from the host's `gh auth token` (any shape,
/// not only `ghp_…`) is masked in the run's frames and its `stderr.log` from then on.
#[cfg(unix)]
#[tokio::test]
async fn the_hosts_github_token_is_masked_in_frames_and_stderr_log() {
    use std::os::unix::fs::PermissionsExt;
    let token = "host-github-token-without-a-known-shape-42";
    let tmp = tempfile::tempdir().unwrap();
    let gh = tmp.path().join("gh");
    std::fs::write(&gh, format!("#!/bin/sh\necho {token}\n")).unwrap();
    std::fs::set_permissions(&gh, std::fs::Permissions::from_mode(0o755)).unwrap();
    let red = std::sync::Arc::new(r().for_run());
    let spec = chat_protocol::github::RunGithub {
        source: "machine".into(),
        member_id: "U1".into(),
        login: None,
        author: None,
        committer: chat_protocol::github::GitIdentity {
            name: "Robo".into(),
            email: "ai-B1@users.noreply.invalid".into(),
        },
        rewrite_ssh: None,
        asked_by: None,
    };
    let g = daemon::github::Run::new(spec, red.clone(), true).with_path(tmp.path().as_os_str());
    assert_eq!(red.text(token), token);
    assert!(matches!(g.fetch().await, daemon::github::Got::Token(t) if t == token));
    let up = Up::RunEvent {
        run_id: "run-1".into(),
        seq: 1,
        at: 1,
        event: RunEvent::ToolResult {
            item_id: "i".into(),
            turn_id: None,
            ok: true,
            output: Some(format!("remote: token {token}\n")),
            exit_code: Some(0),
        },
    };
    let text = red.frame(&up).unwrap();
    assert!(!text.contains(token) && text.contains(MASK), "{text}");
    let log = tmp.path().join("stderr.log");
    std::fs::write(
        &log,
        format!("fatal: https://x-access-token:{token}@github.com\n"),
    )
    .unwrap();
    red.file(&log);
    let logged = std::fs::read_to_string(&log).unwrap();
    assert!(!logged.contains(token) && logged.contains(MASK), "{logged}");
}
