//! One run on this machine's executor, from start to end, with a cancel and a pause in
//! between: the Claude Code runtime starts a fake CLI, and the fake CLI starts a tree of
//! processes the way a CLI's tools do. The same cases run on Unix (process groups) and on
//! Windows (a Job Object); CI runs them on both.
//!
//! This test program is its own fake CLI (`harness = false`): started with
//! `PROCESS_TREE_FAKE=cli` it speaks Claude Code's stream-json, and with `--fake-mid` or
//! `--fake-tick` it is one of the processes below the CLI. No real CLI, no login.

// The fake processes start children and never wait for them: ending those is the daemon's job.
#![allow(clippy::zombie_processes)]

use daemon::config::RuntimeConfig;
use daemon::proto::RunEvent;
use daemon::runtime::{self, Control, Outcome, RunSpec, Status};
use daemon::workspace::{self, RunMeta};
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};
use tokio::sync::mpsc;

const FAKE: &str = "PROCESS_TREE_FAKE";
/// Every fake process ends by itself after this long, so a failed case leaves nothing behind.
const FAKE_LIFE: Duration = Duration::from_secs(120);

fn main() {
    let args: Vec<String> = std::env::args().collect();
    match args.get(1).map(String::as_str) {
        Some("--fake-tick") => fake_tick(Path::new(&args[2])),
        Some("--fake-mid") => fake_mid(),
        _ => {}
    }
    if std::env::var(FAKE).as_deref() == Ok("cli") {
        fake_cli(&args[1..]);
    }
    let rt = tokio::runtime::Runtime::new().unwrap();
    let cases: Vec<(&str, runtime::BoxFuture<()>)> = vec![
        (
            "a_run_starts_streams_and_ends",
            Box::pin(a_run_starts_streams_and_ends()),
        ),
        (
            "cancel_leaves_no_process_of_the_tree",
            Box::pin(cancel_leaves_no_process_of_the_tree()),
        ),
        (
            "pause_stops_the_tree_and_resume_lets_it_go_on",
            Box::pin(pause_stops_the_tree_and_resume_lets_it_go_on()),
        ),
        (
            "an_npm_shim_starts_the_cli_without_cmd_exe",
            Box::pin(an_npm_shim_starts_the_cli_without_cmd_exe()),
        ),
        (
            "a_batch_wrapper_gets_no_argument_with_a_line_break",
            Box::pin(a_batch_wrapper_gets_no_argument_with_a_line_break()),
        ),
        (
            "a_helper_process_ends_with_everything_it_started",
            Box::pin(a_helper_process_ends_with_everything_it_started()),
        ),
        (
            "the_pty_runtime_runs_a_program_in_a_pseudoconsole",
            Box::pin(the_pty_runtime_runs_a_program_in_a_pseudoconsole()),
        ),
    ];
    println!("\nrunning {} tests", cases.len());
    for (name, case) in cases {
        rt.block_on(async {
            tokio::time::timeout(Duration::from_secs(90), case)
                .await
                .unwrap_or_else(|_| panic!("{name} did not finish in 90 s"));
        });
        println!("test {name} ... ok");
    }
    println!("\ntest result: ok.\n");
}

// ---- the fake processes ----

/// Appends a line to `file` every 50 ms.
fn fake_tick(file: &Path) -> ! {
    use std::io::Write;
    let end = Instant::now() + FAKE_LIFE;
    while Instant::now() < end {
        if let Ok(mut f) = std::fs::OpenOptions::new()
            .append(true)
            .create(true)
            .open(file)
        {
            let _ = f.write_all(b"x\n");
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    std::process::exit(0)
}

fn ticker(file: &str) -> std::process::Command {
    let mut c = std::process::Command::new(std::env::current_exe().unwrap());
    c.arg("--fake-tick")
        .arg(file)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    c
}

#[cfg(windows)]
const CREATE_NEW_PROCESS_GROUP: u32 = 0x200;
#[cfg(windows)]
const DETACHED_PROCESS: u32 = 0x8;
#[cfg(windows)]
const CREATE_BREAKAWAY_FROM_JOB: u32 = 0x0100_0000;

/// A tool of the CLI (its shell): starts processes that leave its process group, like the
/// commands a shell tool runs, then stays.
fn fake_mid() -> ! {
    std::fs::write("mid.pid", std::process::id().to_string()).unwrap();
    // A grandchild of the CLI in a process group of its own.
    let mut group = ticker("grandchild.ticks");
    #[cfg(unix)]
    std::os::unix::process::CommandExt::process_group(&mut group, 0);
    #[cfg(windows)]
    std::os::windows::process::CommandExt::creation_flags(&mut group, CREATE_NEW_PROCESS_GROUP);
    let group = group.spawn().unwrap();
    std::fs::write("grandchild.pid", group.id().to_string()).unwrap();
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // One without a console (a tool that made itself a background process).
        let detached = ticker("detached.ticks")
            .creation_flags(DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP)
            .spawn()
            .unwrap();
        std::fs::write("detached.pid", detached.id().to_string()).unwrap();
        // One that asks to leave the job. The job does not allow it: Windows refuses to start
        // it. If it did start, it must still end with the run.
        match ticker("breakaway.ticks")
            .creation_flags(CREATE_BREAKAWAY_FROM_JOB | CREATE_NEW_PROCESS_GROUP)
            .spawn()
        {
            Ok(c) => std::fs::write("breakaway.pid", c.id().to_string()).unwrap(),
            Err(e) => std::fs::write("breakaway.refused", e.to_string()).unwrap(),
        }
    }
    std::fs::write("mid.ready", "").unwrap();
    std::thread::sleep(FAKE_LIFE);
    std::process::exit(0)
}

/// Claude Code's headless mode, as far as the runtime uses it: one JSON line per user message
/// on stdin, stream-json events on stdout. A message with the word "spawn" starts the process
/// tree and never answers.
fn fake_cli(args: &[String]) -> ! {
    use std::io::{BufRead, Write};
    std::fs::write("argv.json", serde_json::to_vec(args).unwrap()).unwrap();
    let out = |v: Value| {
        let mut o = std::io::stdout().lock();
        let _ = writeln!(o, "{v}");
        let _ = o.flush();
    };
    let sid = "fake-session";
    let mut n = 0;
    for line in std::io::stdin().lock().lines() {
        let Ok(line) = line else { break };
        let msg: Value = serde_json::from_str(&line).unwrap();
        let text = msg["message"]["content"][0]["text"]
            .as_str()
            .unwrap_or_default()
            .to_string();
        if n == 0 {
            out(
                json!({ "type": "system", "subtype": "init", "cwd": std::env::current_dir().unwrap(),
                "session_id": sid, "model": "fake-model" }),
            );
        }
        n += 1;
        let mid = format!("msg_{n}");
        if text.contains("spawn") {
            std::fs::write("cli.pid", std::process::id().to_string()).unwrap();
            let child = ticker("child.ticks").spawn().unwrap();
            std::fs::write("child.pid", child.id().to_string()).unwrap();
            std::process::Command::new(std::env::current_exe().unwrap())
                .arg("--fake-mid")
                .stdin(std::process::Stdio::null())
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .spawn()
                .unwrap();
            fake_tick(Path::new("cli.ticks"));
        }
        out(
            json!({ "type": "stream_event", "event": { "type": "message_start", "message": { "id": mid } }, "session_id": sid }),
        );
        let reply = format!("reply {n}: {text}");
        let chars: Vec<char> = reply.chars().collect();
        for chunk in chars.chunks(chars.len().div_ceil(3).max(1)) {
            let part: String = chunk.iter().collect();
            out(
                json!({ "type": "stream_event", "event": { "type": "content_block_delta", "index": 0,
                "delta": { "type": "text_delta", "text": part } }, "session_id": sid }),
            );
            std::thread::sleep(Duration::from_millis(100));
        }
        out(
            json!({ "type": "assistant", "message": { "id": mid, "content": [{ "type": "text", "text": reply }] }, "session_id": sid }),
        );
        out(
            json!({ "type": "result", "subtype": "success", "is_error": false, "result": reply,
            "session_id": sid, "usage": { "input_tokens": 3, "output_tokens": 4 }, "total_cost_usd": 0.001 }),
        );
    }
    std::process::exit(0)
}

// ---- the cases ----

/// A run of the Claude runtime in `dir` (the run folder; the CLI works in `dir/work`) whose
/// CLI is `command`.
fn spec(dir: &Path, command: &str, prompt: &str) -> RunSpec {
    let dir = dir.to_path_buf();
    let cwd = dir.join("work");
    std::fs::create_dir_all(&cwd).unwrap();
    // `run.json`, as the daemon writes it before a run starts (the executor records the CLI in it).
    workspace::write_meta(
        &dir,
        &RunMeta {
            run_id: "r1".into(),
            started_at: 1,
            ..Default::default()
        },
    );
    let mut env: Vec<(String, String)> = std::env::vars()
        .filter(|(k, _)| k != FAKE && k != "CLAUDE_CONFIG_DIR")
        .collect();
    env.push((FAKE.into(), "cli".into()));
    env.push((
        "CLAUDE_CONFIG_DIR".into(),
        dir.join("claude").to_string_lossy().into(),
    ));
    env.push((
        daemon::proctree::ENV_FOLDER.into(),
        dir.to_string_lossy().into(),
    ));
    RunSpec {
        run_id: "r1".into(),
        runtime_name: "claude".into(),
        cfg: RuntimeConfig {
            kind: "claude".into(),
            command: Some(command.into()),
            args: vec![],
            env: BTreeMap::new(),
            model: None,
            models: None,
            efforts: None,
            approval_policy: None,
            sandbox: None,
            permission_mode: None,
            allowed_tools: None,
        },
        prompt: prompt.into(),
        model: None,
        effort: None,
        cwd,
        run_dir: dir,
        env,
        aside: false,
        note: None,
        resume_session_id: None,
        gate_hook: None,
        exec: daemon::executor::local(),
    }
}

fn this_program() -> String {
    std::env::current_exe()
        .unwrap()
        .to_string_lossy()
        .into_owned()
}

/// A started run: its events so far, its control channel and its outcome.
struct Run {
    events: mpsc::UnboundedReceiver<RunEvent>,
    control: mpsc::UnboundedSender<Control>,
    outcome: tokio::task::JoinHandle<Outcome>,
}

fn start(spec: RunSpec) -> Run {
    let rt = runtime::for_kind(&spec.cfg.kind).unwrap();
    let (etx, events) = mpsc::unbounded_channel();
    let (control, crx) = mpsc::unbounded_channel();
    Run {
        events,
        control,
        outcome: tokio::spawn(rt.start(spec, etx, crx)),
    }
}

fn size(p: &Path) -> u64 {
    std::fs::metadata(p).map(|m| m.len()).unwrap_or(0)
}

async fn wait_for(what: &str, mut ready: impl FnMut() -> bool) {
    let end = Instant::now() + Duration::from_secs(30);
    while !ready() {
        assert!(Instant::now() < end, "timed out waiting for {what}");
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
}

fn alive(pid: u32) -> bool {
    #[cfg(unix)]
    // SAFETY: signal 0 only checks that the process exists.
    return unsafe { libc::kill(pid as i32, 0) == 0 };
    #[cfg(windows)]
    return daemon::winjob::pid_alive(pid);
}

/// The tick files of the tree that the fake CLI starts for "spawn".
fn tick_files(work: &Path) -> Vec<PathBuf> {
    let mut names = vec!["cli.ticks", "child.ticks", "grandchild.ticks"];
    if cfg!(windows) {
        names.push("detached.ticks");
    }
    names.iter().map(|n| work.join(n)).collect()
}

/// Waits until every process of the "spawn" tree runs (each has written ticks), and returns
/// their pids by name.
async fn tree_is_up(work: &Path) -> Vec<(String, u32)> {
    wait_for("the fake CLI's process tree", || {
        work.join("mid.ready").exists() && tick_files(work).iter().all(|f| size(f) > 0)
    })
    .await;
    let mut pids = vec![];
    for e in std::fs::read_dir(work).unwrap().flatten() {
        let name = e.file_name().to_string_lossy().to_string();
        if let Some(who) = name.strip_suffix(".pid") {
            let pid = std::fs::read_to_string(e.path())
                .unwrap()
                .trim()
                .parse()
                .unwrap();
            pids.push((who.to_string(), pid));
        }
    }
    let want = if cfg!(windows) { 5 } else { 4 };
    assert!(pids.len() >= want, "{pids:?}");
    if cfg!(windows) {
        match std::fs::read_to_string(work.join("breakaway.refused")) {
            Ok(why) => println!("    breakaway from the job was refused: {}", why.trim()),
            Err(_) => println!("    breakaway from the job was NOT refused: {pids:?}"),
        }
    }
    pids
}

/// True when no tick file grows for half a second.
async fn ticks_stand_still(work: &Path) -> bool {
    let files = tick_files(work);
    let before: Vec<u64> = files.iter().map(|f| size(f)).collect();
    tokio::time::sleep(Duration::from_millis(500)).await;
    let after: Vec<u64> = files.iter().map(|f| size(f)).collect();
    before == after
}

async fn a_run_starts_streams_and_ends() {
    let dir = tempfile::tempdir().unwrap();
    let mut run = start(spec(dir.path(), &this_program(), "hello"));
    let o = run.outcome.await.unwrap();
    assert_eq!(o.status, Status::Succeeded, "{o:?}");
    assert_eq!(o.result_text.as_deref(), Some("reply 1: hello"));
    assert_eq!(o.exit_code, Some(0));
    assert_eq!(o.session_id.as_deref(), Some("fake-session"));
    let mut evs = vec![];
    while let Ok(e) = run.events.try_recv() {
        evs.push(e);
    }
    let streamed: String = evs
        .iter()
        .filter_map(|e| match e {
            RunEvent::MessageDelta { text, .. } => Some(text.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(streamed, "reply 1: hello", "{evs:?}");
    let deltas = evs
        .iter()
        .filter(|e| matches!(e, RunEvent::MessageDelta { .. }))
        .count();
    assert!(deltas >= 2, "the reply came in parts: {evs:?}");
    assert!(
        evs.iter()
            .any(|e| matches!(e, RunEvent::Session { model: Some(m), .. } if m == "fake-model")),
        "{evs:?}"
    );
    // The runtime's own arguments reached the CLI, and its raw stream is in the run folder.
    let work = dir.path().join("work");
    let argv: Vec<String> =
        serde_json::from_slice(&std::fs::read(work.join("argv.json")).unwrap()).unwrap();
    assert!(argv.iter().any(|a| a == "stream-json"), "{argv:?}");
    assert!(size(&work.parent().unwrap().join("stream.jsonl")) > 0);
}

async fn cancel_leaves_no_process_of_the_tree() {
    let dir = tempfile::tempdir().unwrap();
    let s = spec(dir.path(), &this_program(), "spawn");
    let work = s.cwd.clone();
    let run = start(s);
    let pids = tree_is_up(&work).await;
    assert!(pids.iter().all(|(_, p)| alive(*p)), "{pids:?}");

    let asked = Instant::now();
    run.control.send(Control::Cancel).unwrap();
    let o = run.outcome.await.unwrap();
    assert_eq!(o.status, Status::Cancelled, "{o:?}");
    println!(
        "    cancelled in {:?}, the CLI's exit code: {:?}",
        asked.elapsed(),
        o.exit_code
    );
    // The CLI, its child, its tool, and the tool's children in groups of their own.
    wait_for("every process of the tree to end", || {
        pids.iter().all(|(_, p)| !alive(*p))
    })
    .await;
    assert!(ticks_stand_still(&work).await, "something still ticks");
}

async fn pause_stops_the_tree_and_resume_lets_it_go_on() {
    let dir = tempfile::tempdir().unwrap();
    let s = spec(dir.path(), &this_program(), "spawn");
    let (work, run_dir, exec) = (s.cwd.clone(), s.run_dir.clone(), s.exec.clone());
    let run = start(s);
    let pids = tree_is_up(&work).await;

    let stopped = exec
        .stop(&run_dir)
        .expect("the CLI runs: it can be stopped");
    // A write that had begun may still land.
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert!(ticks_stand_still(&work).await, "a stopped tree still ticks");
    assert!(
        pids.iter().all(|(_, p)| alive(*p)),
        "stopped, not ended: {pids:?}"
    );

    stopped.resume();
    let before: Vec<u64> = tick_files(&work).iter().map(|f| size(f)).collect();
    wait_for("every process to tick again", || {
        tick_files(&work)
            .iter()
            .zip(&before)
            .all(|(f, b)| size(f) > *b)
    })
    .await;

    // A cancel while stopped still ends everything.
    let stopped = exec.stop(&run_dir).expect("stopped again");
    run.control.send(Control::Cancel).unwrap();
    let o = run.outcome.await.unwrap();
    assert_eq!(o.status, Status::Cancelled, "{o:?}");
    drop(stopped);
    wait_for("every process of the tree to end", || {
        pids.iter().all(|(_, p)| !alive(*p))
    })
    .await;
    assert!(ticks_stand_still(&work).await, "something still ticks");
}

/// Windows: a CLI installed by npm is `<name>.cmd`. The run finds it by its bare name in the
/// run's PATH, starts the program the shim names (not `cmd.exe`), and an argument full of
/// `cmd.exe` syntax arrives as it was given.
#[cfg(windows)]
async fn an_npm_shim_starts_the_cli_without_cmd_exe() {
    let dir = tempfile::tempdir().unwrap();
    let tools = dir.path().join("npm");
    let pkg = tools.join("node_modules").join("fake-cli").join("bin");
    std::fs::create_dir_all(&pkg).unwrap();
    std::fs::copy(std::env::current_exe().unwrap(), pkg.join("fake-cli.exe")).unwrap();
    // What npm's cmd-shim writes for a package whose bin is a program.
    std::fs::write(
        tools.join("fakecli.cmd"),
        "@ECHO off\r\nGOTO start\r\n:find_dp0\r\nSET dp0=%~dp0\r\nEXIT /b\r\n:start\r\nSETLOCAL\r\nCALL :find_dp0\r\n\"%dp0%\\node_modules\\fake-cli\\bin\\fake-cli.exe\"   %*\r\n",
    )
    .unwrap();
    // The shell script npm puts next to it must never be picked.
    std::fs::write(tools.join("fakecli"), "#!/bin/sh\nexit 1\n").unwrap();

    let mut s = spec(dir.path(), "fakecli", "hello");
    let note = "say \"hi\" & echo %PATH% ^ | more > x\nsecond line 'q' !e! (a) %";
    s.note = Some(note.into());
    for (k, v) in s.env.iter_mut() {
        if k.eq_ignore_ascii_case("PATH") {
            *v = format!("{};{v}", tools.display());
        }
    }
    let work = s.cwd.clone();
    let run = start(s);
    let o = run.outcome.await.unwrap();
    assert_eq!(o.status, Status::Succeeded, "{o:?}");
    assert_eq!(o.result_text.as_deref(), Some("reply 1: hello"));
    let argv: Vec<String> =
        serde_json::from_slice(&std::fs::read(work.join("argv.json")).unwrap()).unwrap();
    assert!(argv.iter().any(|a| a == note), "{argv:?}");
}

/// Unix starts the program file the kernel is given: there is no shim to resolve.
#[cfg(not(windows))]
async fn an_npm_shim_starts_the_cli_without_cmd_exe() {}

/// Windows: a `.cmd` that is not an npm shim runs in `cmd.exe`. An argument with a line break
/// cannot get through it: the run fails and says why, before anything starts. Arguments
/// without one arrive unchanged, whatever `cmd.exe` syntax is in them.
#[cfg(windows)]
async fn a_batch_wrapper_gets_no_argument_with_a_line_break() {
    let dir = tempfile::tempdir().unwrap();
    let tools = dir.path().join("tools");
    std::fs::create_dir_all(&tools).unwrap();
    // An absolute path: not what npm writes, so the daemon cannot start the target itself.
    std::fs::write(
        tools.join("wrapped.cmd"),
        format!("@echo off\r\n\"{}\" %*\r\n", this_program()),
    )
    .unwrap();
    let wrapper = tools.join("wrapped.cmd").to_string_lossy().into_owned();

    let mut s = spec(&dir.path().join("r1"), &wrapper, "hello");
    s.note = Some("first line\nsecond line & echo owned".into());
    let work = s.cwd.clone();
    let o = start(s).outcome.await.unwrap();
    assert_eq!(o.status, Status::Failed, "{o:?}");
    let why = o.error.unwrap_or_default();
    assert!(
        why.contains("line break") && why.contains("wrapped.cmd"),
        "{why}"
    );
    assert!(
        !work.join("argv.json").exists(),
        "the CLI must not have started"
    );

    let mut s = spec(&dir.path().join("r2"), &wrapper, "hello");
    let note = "say \"hi\" & echo %PATH% ^ | more > x 'q' !e! (a) %";
    s.note = Some(note.into());
    let work = s.cwd.clone();
    let o = start(s).outcome.await.unwrap();
    assert_eq!(o.status, Status::Succeeded, "{o:?}");
    let argv: Vec<String> =
        serde_json::from_slice(&std::fs::read(work.join("argv.json")).unwrap()).unwrap();
    assert!(argv.iter().any(|a| a == note), "{argv:?}");
}

#[cfg(not(windows))]
async fn a_batch_wrapper_gets_no_argument_with_a_line_break() {}

/// Windows: a probe or a sign-in the daemon starts is in a job of its own, so giving it up
/// (a timeout, a cancel) also ends what it started; killing a `.cmd` wrapper alone would
/// leave the real program running.
#[cfg(windows)]
async fn a_helper_process_ends_with_everything_it_started() {
    let dir = tempfile::tempdir().unwrap();
    let work = dir.path().to_path_buf();
    let mut cmd = tokio::process::Command::new(std::env::current_exe().unwrap());
    cmd.arg("--fake-mid")
        .current_dir(&work)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .kill_on_drop(true);
    let helper = daemon::health::spawn(&mut cmd).unwrap();
    wait_for("the helper's children", || work.join("mid.ready").exists()).await;
    let pid = |name: &str| -> u32 {
        std::fs::read_to_string(work.join(name))
            .unwrap()
            .trim()
            .parse()
            .unwrap()
    };
    let pids = [pid("mid.pid"), pid("grandchild.pid"), pid("detached.pid")];
    assert!(pids.iter().all(|p| alive(*p)), "{pids:?}");
    drop(helper);
    wait_for("the helper and its children to end", || {
        pids.iter().all(|p| !alive(*p))
    })
    .await;
}

/// Unix: a helper is a plain child; the CLIs there are not started through wrappers.
#[cfg(not(windows))]
async fn a_helper_process_ends_with_everything_it_started() {}

/// A run of the `pty` runtime with `cmd.exe` and `args`.
#[cfg(windows)]
fn pty_spec(dir: &Path, args: &[&str], prompt: &str) -> RunSpec {
    let mut s = spec(dir, "cmd.exe", prompt);
    s.runtime_name = "shell".into();
    s.cfg.kind = "pty".into();
    s.cfg.args = args.iter().map(|a| a.to_string()).collect();
    s
}

/// Windows: the `pty` kind starts the program in a pseudoconsole (ConPTY). Its output comes
/// back as text, a prompt without a placeholder is typed into it, and a cancel ends it.
#[cfg(windows)]
async fn the_pty_runtime_runs_a_program_in_a_pseudoconsole() {
    let dir = tempfile::tempdir().unwrap();

    // The prompt as an argument.
    let s = pty_spec(
        &dir.path().join("r1"),
        &["/d", "/c", "echo {prompt}"],
        "conpty-hello",
    );
    let log = s.run_dir.join("pty.log");
    let o = start(s).outcome.await.unwrap();
    assert_eq!(o.status, Status::Succeeded, "{o:?}");
    assert_eq!(o.exit_code, Some(0));
    let text = o.result_text.unwrap_or_default();
    assert!(text.contains("conpty-hello"), "{text:?}");
    assert!(size(&log) > 0, "the raw output is kept");

    // The prompt typed into the console; the exit code is the program's.
    let s = pty_spec(
        &dir.path().join("r2"),
        &["/d", "/q"],
        "echo typed-marker& exit 7",
    );
    let o = start(s).outcome.await.unwrap();
    assert_eq!(o.status, Status::Failed, "{o:?}");
    assert_eq!(o.exit_code, Some(7));
    assert!(
        o.result_text.unwrap_or_default().contains("typed-marker"),
        "the typed command ran"
    );

    // A shell that waits for input is ended by a cancel.
    let s = pty_spec(&dir.path().join("r3"), &["/d", "/q", "/k"], "echo started");
    let run_dir = s.run_dir.clone();
    let mut run = start(s);
    let mut seen = String::new();
    while !seen.contains("started") {
        match run.events.recv().await {
            Some(RunEvent::MessageDelta { text, .. }) => seen.push_str(&text),
            Some(_) => {}
            None => panic!("the run ended before it printed: {seen:?}"),
        }
    }
    let pid = workspace::read_meta(&run_dir).unwrap().pgid.unwrap() as u32;
    assert!(alive(pid));
    run.control.send(Control::Cancel).unwrap();
    let o = run.outcome.await.unwrap();
    assert_eq!(o.status, Status::Cancelled, "{o:?}");
    wait_for("the shell to end", || !alive(pid)).await;
}

/// Unix: `tests/runtimes.rs` covers the pty runtime there.
#[cfg(not(windows))]
async fn the_pty_runtime_runs_a_program_in_a_pseudoconsole() {}
