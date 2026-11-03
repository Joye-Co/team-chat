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
        aside: false,
        note: None,
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
    assert_eq!(
        runtime::codex::thread_policy(&s),
        ("on-request".to_string(), "danger-full-access".to_string())
    );
    assert_eq!(
        runtime::codex::app_server_args(&s).unwrap(),
        vec!["app-server", "--dangerously-bypass-approvals-and-sandbox"]
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
    c.cfg.permission_mode = Some("bypassPermissions".into());
    c.cfg.allowed_tools = Some("Bash".into());
    let normal = runtime::claude::permission_args(&c);
    assert!(normal.contains(&"bypassPermissions".to_string()));
    assert!(normal.contains(&"--dangerously-skip-permissions".to_string()));
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
    // Only the user's settings (auth env, apiKeyHelper, model); project and local settings
    // come from the repo. The tool set above keeps a user allow rule from widening the run.
    assert_eq!(value("--setting-sources"), "user");
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
/// `codex mcp list` reads the config without a model call. Skipped without `codex` on PATH.
#[test]
fn codex_aside_mcp_overrides_load_in_real_cli() {
    let Some(codex) = std::env::var_os("PATH").and_then(|p| {
        std::env::split_paths(&p)
            .map(|d| d.join("codex"))
            .find(|f| f.is_file())
    }) else {
        eprintln!("codex not on PATH; skipped");
        return;
    };
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
    let out = std::process::Command::new(codex)
        .args(["mcp", "list", "--json"])
        .args(&args[1..])
        .current_dir(&s.cwd)
        .env("CODEX_HOME", &home)
        .env("HOME", tmp.path())
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

/// `codex app-server` with `args`: runs `command` through `command/exec` (the server's sandbox,
/// no model call) and returns its result.
fn codex_exec(
    codex: &std::path::Path,
    args: &[String],
    home: &std::path::Path,
    cwd: &std::path::Path,
    command: &[String],
    env: serde_json::Value,
) -> serde_json::Value {
    use std::io::{BufRead, Write};
    let mut p = std::process::Command::new(codex)
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
    let mut line = String::new();
    let result = loop {
        line.clear();
        assert!(
            out.read_line(&mut line).unwrap() > 0,
            "codex app-server ended"
        );
        let v: serde_json::Value = serde_json::from_str(&line).unwrap_or_default();
        if v["id"] == 2 {
            break v;
        }
    };
    let _ = p.kill();
    let _ = p.wait();
    result
}

/// A Codex run in the default `workspace-write` sandbox can use `chat-run`: the real Codex CLI
/// runs the real client against the run's socket through `command/exec`, which uses the
/// server's sandbox without a model call. Without [`runtime::codex::CHAT_RUN_NETWORK`] the
/// sandbox blocks the connect and `chat-run` says so. Skipped without `codex` on PATH.
#[tokio::test(flavor = "multi_thread")]
async fn codex_sandbox_lets_chat_run_reach_its_socket() {
    let Some(codex) = std::env::var_os("PATH").and_then(|p| {
        std::env::split_paths(&p)
            .map(|d| d.join("codex"))
            .find(|f| f.is_file())
    }) else {
        eprintln!("codex not on PATH; skipped");
        return;
    };
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path().join("codex-home");
    std::fs::create_dir_all(&home).unwrap();
    let mut s = spec(tmp.path(), "codex", "codex", &[], "hi");
    s.cfg.approval_policy = None;
    s.note = Some(daemon::chat_run::note());
    assert!(runtime::codex::chat_run_works(&s));

    daemon::chat_run::set_exe(std::path::PathBuf::from(env!("CARGO_BIN_EXE_daemon")));
    let bin = daemon::chat_run::install_link(tmp.path()).unwrap();
    let outputs = std::sync::Arc::new(daemon::chat_run::Outputs::default());
    let (tx, mut rx) = mpsc::unbounded_channel();
    let server = daemon::chat_run::Server::start(
        daemon::chat_run::socket_path(tmp.path()),
        outputs.clone(),
        move |e| {
            let _ = tx.send(e);
        },
        daemon::chat_run::TIMEOUT,
    )
    .unwrap();
    let command: Vec<String> = [
        bin.join("chat-run").to_string_lossy().to_string(),
        "resolve".into(),
        "--event".into(),
        "EV1".into(),
        "--result".into(),
        "ignored".into(),
    ]
    .into();
    let env = serde_json::json!({ "CHAT_RUN_SOCKET": server.path().to_string_lossy() });
    // The daemon sends the sandbox in `thread/start`; `command/exec` uses the server config.
    let with_sandbox = |mut args: Vec<String>| {
        args.extend([
            "-c".to_string(),
            "sandbox_mode=\"workspace-write\"".to_string(),
        ]);
        args
    };

    let args = runtime::codex::app_server_args(&s).unwrap();
    assert!(
        args.contains(&runtime::codex::CHAT_RUN_NETWORK.to_string()),
        "{args:?}"
    );
    let plain: Vec<String> = args
        .iter()
        .filter(|a| *a != "-c" && *a != runtime::codex::CHAT_RUN_NETWORK)
        .cloned()
        .collect();
    let blocked = tokio::task::spawn_blocking({
        let (codex, home, cwd, command, env) = (
            codex.clone(),
            home.clone(),
            s.cwd.clone(),
            command.clone(),
            env.clone(),
        );
        let args = with_sandbox(plain);
        move || codex_exec(&codex, &args, &home, &cwd, &command, env)
    })
    .await
    .unwrap();
    let stdout = blocked["result"]["stdout"].as_str().unwrap_or_default();
    assert!(stdout.contains("sandbox.blocked"), "{blocked}");

    let run = tokio::task::spawn_blocking({
        let (home, cwd) = (home.clone(), s.cwd.clone());
        let args = with_sandbox(args);
        move || codex_exec(&codex, &args, &home, &cwd, &command, env)
    });
    let ev = tokio::time::timeout(Duration::from_secs(30), rx.recv())
        .await
        .expect("chat-run did not reach the socket")
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
}

/// The `chat-run` note goes only to a Codex run whose sandbox lets `chat-run` connect.
#[test]
fn codex_gets_the_chat_run_note_only_when_it_can_work() {
    let tmp = tempfile::tempdir().unwrap();
    let mut s = spec(tmp.path(), "codex", "codex", &[], "q");
    s.note = Some("n".into());
    assert!(runtime::codex::chat_run_works(&s));
    s.cfg.sandbox = Some("danger-full-access".into());
    assert!(runtime::codex::chat_run_works(&s));
    assert_eq!(runtime::codex::app_server_args(&s).unwrap(), ["app-server"]);
    s.cfg.sandbox = Some("read-only".into());
    assert!(!runtime::codex::chat_run_works(&s));
    assert_eq!(runtime::codex::app_server_args(&s).unwrap(), ["app-server"]);
    s.cfg.sandbox = None;
    s.note = None;
    assert_eq!(runtime::codex::app_server_args(&s).unwrap(), ["app-server"]);
}
