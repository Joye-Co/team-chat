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
        gate_alive: None,
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

/// Windows: 接管 really stops a `pty` run (every thread of the program and of what it started
/// is suspended and nothing is written), handing it back lets it go on, and a cancel ends the
/// program and what it started. Unix: `pty_cancel_kills_group` and the pause cases of
/// `tests/integration.rs`.
#[cfg(windows)]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn pty_pause_holds_and_cancel_ends_the_tree() {
    let dir = tempfile::tempdir().unwrap();
    let script = dir.path().join("ticker.py");
    std::fs::write(
        &script,
        "import os, subprocess, sys, time\n\
kid = subprocess.Popen([sys.executable, '-c', \"import time\\nwhile True:\\n    open('kid.ticks', 'a').write('x\\\\n')\\n    time.sleep(0.05)\"])\n\
open('pids', 'w').write('%d %d' % (os.getpid(), kid.pid))\n\
print('ready', flush=True)\n\
while True:\n    open('own.ticks', 'a').write('x\\n')\n    time.sleep(0.05)\n",
    )
    .unwrap();
    let s = spec(dir.path(), "pty", &script.to_string_lossy(), &[], "go");
    let (work, run_dir, exec) = (s.cwd.clone(), s.run_dir.clone(), s.exec.clone());
    let rt = runtime::for_kind("pty").unwrap();
    let (etx, _erx) = mpsc::unbounded_channel();
    let (ctx, crx) = mpsc::unbounded_channel();
    let run = tokio::spawn(rt.start(s, etx, crx));
    let ticks = [work.join("own.ticks"), work.join("kid.ticks")];
    let sizes = || -> Vec<u64> {
        ticks
            .iter()
            .map(|f| std::fs::metadata(f).map_or(0, |m| m.len()))
            .collect()
    };
    let wait = |what: &'static str, done: &dyn Fn() -> bool| {
        let end = std::time::Instant::now() + Duration::from_secs(60);
        while !done() {
            assert!(std::time::Instant::now() < end, "timed out: {what}");
            std::thread::sleep(Duration::from_millis(50));
        }
    };
    wait("both processes tick", &|| sizes().iter().all(|n| *n > 10));
    let pids: Vec<u32> = std::fs::read_to_string(work.join("pids"))
        .unwrap()
        .split_whitespace()
        .map(|p| p.parse().unwrap())
        .collect();
    assert_eq!(pids.len(), 2);

    let paused = exec.stop(&run_dir).expect("a pty run can be stopped");
    // A write that had begun may still land.
    tokio::time::sleep(Duration::from_millis(400)).await;
    let held = sizes();
    for pid in &pids {
        assert_eq!(
            procs::stopped(*pid),
            Some(true),
            "process {pid} is not stopped"
        );
    }
    tokio::time::sleep(Duration::from_millis(1500)).await;
    assert_eq!(sizes(), held, "a paused run made progress");
    paused.resume();
    wait("both processes tick again", &|| {
        sizes().iter().zip(&held).all(|(now, then)| now > then)
    });
    assert_eq!(procs::stopped(pids[0]), Some(false));

    ctx.send(Control::Cancel).unwrap();
    let o = tokio::time::timeout(Duration::from_secs(20), run)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(o.status, Status::Cancelled, "{o:?}");
    wait("the program and its child end", &|| {
        pids.iter().all(|p| procs::stopped(*p).is_none())
    });
    let end = sizes();
    tokio::time::sleep(Duration::from_millis(600)).await;
    assert_eq!(sizes(), end, "something of the run still writes");
}

/// Windows: whether a process is stopped, for the pause tests.
#[cfg(windows)]
mod procs {
    /// `Some(true)` when every thread of the process `pid` is suspended (a suspend that is
    /// taken back at once tells the count before it), `Some(false)` when one runs, `None` when
    /// the process is gone.
    pub fn stopped(pid: u32) -> Option<bool> {
        use windows_sys::Win32::Foundation::{CloseHandle, INVALID_HANDLE_VALUE};
        use windows_sys::Win32::System::Diagnostics::ToolHelp::{
            CreateToolhelp32Snapshot, TH32CS_SNAPTHREAD, THREADENTRY32, Thread32First, Thread32Next,
        };
        use windows_sys::Win32::System::Threading::{
            OpenThread, ResumeThread, SuspendThread, THREAD_SUSPEND_RESUME,
        };
        if !daemon::winjob::pid_alive(pid) {
            return None;
        }
        let (mut threads, mut running) = (0, 0);
        // SAFETY: a snapshot of the system's threads, read entry by entry into a struct of the
        // size the call is told; each thread handle is closed again.
        unsafe {
            let snap = CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0);
            assert_ne!(snap, INVALID_HANDLE_VALUE);
            let mut e: THREADENTRY32 = std::mem::zeroed();
            e.dwSize = std::mem::size_of::<THREADENTRY32>() as u32;
            let mut more = Thread32First(snap, &mut e) != 0;
            while more {
                if e.th32OwnerProcessID == pid {
                    let t = OpenThread(THREAD_SUSPEND_RESUME, 0, e.th32ThreadID);
                    if !t.is_null() {
                        threads += 1;
                        let before = SuspendThread(t);
                        if before != u32::MAX {
                            ResumeThread(t);
                            if before == 0 {
                                running += 1;
                            }
                        }
                        CloseHandle(t);
                    }
                }
                more = Thread32Next(snap, &mut e) != 0;
            }
            CloseHandle(snap);
        }
        (threads > 0).then_some(running == 0)
    }
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
#[test]
fn codex_aside_mcp_overrides_load_in_real_cli() {
    let Some(codex) = real_codex() else { return };
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
/// resolved to what it starts, as for a run). None (skip) when there is none; a failure
/// instead when `TC_REQUIRE_REAL_CODEX` is set (CI installs the CLI and sets it).
fn real_codex() -> Option<daemon::health::Launch> {
    let Some(found) = daemon::health::which("codex") else {
        assert!(
            std::env::var_os("TC_REQUIRE_REAL_CODEX").is_none(),
            "TC_REQUIRE_REAL_CODEX is set and codex is not on PATH"
        );
        eprintln!("codex not on PATH; skipped");
        return None;
    };
    Some(daemon::health::launch(&found))
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
/// socket, so a Codex run uses the request folder in its working folder. Skipped without
/// `codex` on PATH.
#[cfg(unix)]
#[tokio::test(flavor = "multi_thread")]
async fn codex_sandbox_lets_chat_run_answer_without_network() {
    let Some(codex) = real_codex() else { return };
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

/// A side chat run of Codex is read-only on every system: with the sandbox the daemon sends
/// for it (`read-only`, approval `never`) the real CLI does not let a command write a file,
/// in the working folder or outside it. Through `command/exec` (the server's sandbox, no
/// model call). Skipped without `codex` on PATH.
#[tokio::test(flavor = "multi_thread")]
async fn codex_read_only_sandbox_writes_nothing() {
    let Some(codex) = real_codex() else { return };
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
    let outside = tempfile::tempdir().unwrap();
    for target in [
        s.cwd.join("written.txt"),
        outside.path().join("written.txt"),
    ] {
        let command: Vec<String> = if cfg!(windows) {
            vec![
                "cmd.exe".into(),
                "/c".into(),
                format!("echo x> \"{}\"", target.display()),
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
/// `command/exec` (no model call). Skipped without `codex` on PATH.
#[cfg(windows)]
#[tokio::test(flavor = "multi_thread")]
async fn codex_on_windows_reaches_the_daemon_through_the_request_folder() {
    let Some(codex) = real_codex() else { return };
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

/// Windows: what Codex's own Windows sandbox does when a user turns it on by hand (it is off
/// unless the user's Codex config says otherwise, and the daemon's runs do not use it: they
/// have full access). Both of its modes, with `workspace-write`, through `command/exec` of the
/// real CLI: a write outside the working folder, then the real `chat-run` client against the
/// run's named pipe and against the request folder. What each call answered is written to
/// `codex-windows-sandbox.json` in the test folder (CI prints it).
///
/// This does not prove that `chat-run` works inside that sandbox. It proves what is there to
/// see without a person: no call hangs, and each one either starts nothing (the sandbox
/// refuses to run the command), or `chat-run` answers, or it names the sandbox as what blocks
/// it. Seen with Codex 0.167.1: the unelevated mode starts no command at all ("cannot enforce
/// split writable root sets directly; refusing to run unsandboxed"); what the elevated mode
/// did on the runner is in the file.
#[cfg(windows)]
#[tokio::test(flavor = "multi_thread")]
async fn codex_windows_sandbox_modes_start_nothing_or_name_the_sandbox() {
    let Some(codex) = real_codex() else { return };
    let mut report = serde_json::Map::new();
    for mode in ["unelevated", "elevated"] {
        report.insert(mode.into(), codex_windows_sandbox_mode(&codex, mode).await);
    }
    std::fs::write(
        std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join("codex-windows-sandbox.json"),
        serde_json::to_string_pretty(&report).unwrap(),
    )
    .unwrap();
    for (mode, calls) in &report {
        for what in [
            "chat-run through the named pipe",
            "chat-run through the request folder",
        ] {
            let outcome = calls[what]["outcome"].as_str().unwrap_or_default();
            assert!(
                [
                    "the command was not started",
                    "chat-run answered",
                    "chat-run named the sandbox"
                ]
                .contains(&outcome),
                "{mode}, {what}: {}",
                calls[what]
            );
        }
    }
}

/// One mode of [`codex_windows_sandbox_modes_start_nothing_or_name_the_sandbox`]: each call's
/// answer and what it amounts to.
#[cfg(windows)]
async fn codex_windows_sandbox_mode(
    codex: &daemon::health::Launch,
    mode: &str,
) -> serde_json::Value {
    let codex = codex.clone();
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path().join("codex-home");
    std::fs::create_dir_all(&home).unwrap();
    let mut s = spec(tmp.path(), "codex", "codex", &[], "hi");
    s.note = Some(daemon::chat_run::note());
    let mut args = runtime::codex::app_server_args(&s).unwrap();
    args.extend([
        "-c".to_string(),
        "sandbox_mode=\"workspace-write\"".to_string(),
        "-c".to_string(),
        format!("windows.sandbox=\"{mode}\""),
    ]);
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
    // The daemon's side: every request that arrives is answered.
    let answering = {
        let outputs = outputs.clone();
        tokio::spawn(async move {
            while let Some(ev) = rx.recv().await {
                if let RunEvent::Output { output_id, .. } = ev {
                    outputs.resolve(daemon::proto::RunOutputResult {
                        run_id: "r1".into(),
                        output_id,
                        ok: true,
                        result: Some(serde_json::json!({ "resolved": true })),
                        error: None,
                    });
                }
            }
        })
    };
    let chat_run: Vec<String> = [
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
    let exec = |command: Vec<String>, env: serde_json::Value| {
        let (codex, args, home, cwd) = (codex.clone(), args.clone(), home.clone(), s.cwd.clone());
        tokio::task::spawn_blocking(move || codex_exec(&codex, &args, &home, &cwd, &command, env))
    };
    let outside = tempfile::tempdir().unwrap();
    let target = outside.path().join("written.txt");
    let write = exec(
        vec![
            "cmd.exe".into(),
            "/c".into(),
            format!("echo x> \"{}\"", target.display()),
        ],
        serde_json::json!({}),
    )
    .await
    .unwrap();
    let pipe = exec(
        chat_run.clone(),
        serde_json::json!({ "CHAT_RUN_SOCKET": server.path().to_string_lossy() }),
    )
    .await
    .unwrap();
    let folder = exec(
        chat_run,
        serde_json::json!({ "CHAT_RUN_DIR": spool.path().to_string_lossy() }),
    )
    .await
    .unwrap();
    drop((server, spool));
    let _ = tokio::time::timeout(Duration::from_secs(5), answering).await;
    let outcome = |answer: &serde_json::Value| {
        let stdout = answer["result"]["stdout"].as_str().unwrap_or_default();
        if answer.get("error").is_some_and(|e| !e.is_null()) {
            "the command was not started"
        } else if answer["result"]["exitCode"] == 0 && stdout.contains("resolved") {
            "chat-run answered"
        } else if stdout.contains("sandbox.blocked") {
            "chat-run named the sandbox"
        } else {
            "something else"
        }
    };
    serde_json::json!({
        "a write outside the working folder": {
            "outcome": if write.get("error").is_some_and(|e| !e.is_null()) {
                "the command was not started"
            } else if target.exists() {
                "the file was written: no sandbox was in effect"
            } else {
                "the command ran and the file was not written"
            },
            "answer": write,
        },
        "chat-run through the named pipe": { "outcome": outcome(&pipe), "answer": pipe },
        "chat-run through the request folder": { "outcome": outcome(&folder), "answer": folder },
    })
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
