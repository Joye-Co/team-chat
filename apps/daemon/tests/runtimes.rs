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
        gate_hook: None,
        exec: daemon::executor::local(),
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

#[cfg(unix)]
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
/// `codex mcp list` reads the config without a model call. Skipped without `codex` on PATH.
#[cfg(unix)]
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

#[cfg(unix)]
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

/// A Codex run in a `workspace-write` sandbox can use `chat-run` without network:
/// the real Codex CLI runs the real client through `command/exec` (the server's sandbox, no
/// model call) with the app-server args the daemon builds. The sandbox blocks the run's unix
/// socket, so a Codex run uses the request folder in its working folder. Skipped without
/// `codex` on PATH.
#[cfg(unix)]
#[tokio::test(flavor = "multi_thread")]
async fn codex_sandbox_lets_chat_run_answer_without_network() {
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
        ["app-server", closed[0], closed[1]],
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
        bin.join("chat-run").to_string_lossy().to_string(),
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
        assert_eq!(runtime::codex::app_server_args(&s).unwrap(), ["app-server"]);
    }
    s.cfg.args = vec!["-c".into(), "model_reasoning_effort=\"high\"".into()];
    assert_eq!(
        runtime::codex::app_server_args(&s).unwrap(),
        ["app-server", "-c", "model_reasoning_effort=\"high\""]
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
