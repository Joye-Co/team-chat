//! 接管 (pause) with the real Claude Code CLI and a real tool process, without a login: the
//! CLI runs against the fake model (`tests/fake/model.rs`), which has it start a shell loop
//! in its Bash tool that writes a line every second. Paused, every process of the run is
//! stopped (the CLI, its shell, what the shell started) and no line is written; handed back,
//! the loop goes on; a cancel ends every process.
//!
//! `apps/daemon/e2e/real-cli-pause.ts` is the same check with signed-in CLIs and a real
//! model on a Mac or Linux machine (it reads the Unix process state `T`). Here "stopped" is
//! state `T` on Unix and, on Windows, a process whose every thread is suspended.
//!
//! One test in a file of its own: it looks at every process below this test program.
//! Skipped when `claude` is not on PATH; a failure instead when `TC_REQUIRE_REAL_CLI` is set.

#[path = "fake/model.rs"]
mod model;

use daemon::config::RuntimeConfig;
use daemon::runtime::{self, Control, RunSpec, Status};
use daemon::workspace::{self, RunMeta};
use std::collections::BTreeMap;
use std::path::Path;
use std::time::{Duration, Instant};
use tokio::sync::mpsc;

/// (pid, parent pid) of every process.
fn processes() -> Vec<(u32, u32)> {
    #[cfg(unix)]
    {
        let out = std::process::Command::new("ps")
            .args(["-axo", "pid=,ppid="])
            .output()
            .unwrap();
        String::from_utf8_lossy(&out.stdout)
            .lines()
            .filter_map(|l| {
                let mut p = l.split_whitespace();
                Some((p.next()?.parse().ok()?, p.next()?.parse().ok()?))
            })
            .collect()
    }
    #[cfg(windows)]
    {
        use windows_sys::Win32::Foundation::{CloseHandle, INVALID_HANDLE_VALUE};
        use windows_sys::Win32::System::Diagnostics::ToolHelp::{
            CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW,
            TH32CS_SNAPPROCESS,
        };
        let mut out = vec![];
        // SAFETY: a snapshot of the system's processes, read entry by entry into a struct of
        // the size the call is told; the snapshot is closed again.
        unsafe {
            let snap = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
            assert_ne!(snap, INVALID_HANDLE_VALUE);
            let mut e: PROCESSENTRY32W = std::mem::zeroed();
            e.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;
            let mut more = Process32FirstW(snap, &mut e) != 0;
            while more {
                let len = e.szExeFile.iter().position(|c| *c == 0).unwrap_or(0);
                let name = String::from_utf16_lossy(&e.szExeFile[..len]);
                // The console host Windows puts next to a console program is the system's,
                // not a process of the run's work.
                if !name.eq_ignore_ascii_case("conhost.exe") {
                    out.push((e.th32ProcessID, e.th32ParentProcessID));
                }
                more = Process32NextW(snap, &mut e) != 0;
            }
            CloseHandle(snap);
        }
        out
    }
}

/// Every process below this test program.
fn below_me() -> Vec<u32> {
    let all = processes();
    let mut found = vec![std::process::id()];
    let mut at = 0;
    while at < found.len() {
        let parent = found[at];
        for (pid, ppid) in &all {
            if *ppid == parent && *pid != parent && !found.contains(pid) {
                found.push(*pid);
            }
        }
        at += 1;
    }
    found.remove(0);
    found
}

/// True when the process `pid` is stopped: state `T` on Unix; on Windows every one of its
/// threads is suspended (a suspend that is taken back at once tells the count before it).
/// None when the process is gone (or cannot be asked).
fn stopped(pid: u32) -> Option<bool> {
    #[cfg(unix)]
    {
        let o = std::process::Command::new("ps")
            .args(["-o", "stat=", "-p", &pid.to_string()])
            .output()
            .unwrap();
        let stat = String::from_utf8_lossy(&o.stdout).trim().to_string();
        // A zombie has ended: it is not a process that runs.
        (!stat.is_empty() && !stat.starts_with('Z')).then(|| stat.contains('T'))
    }
    #[cfg(windows)]
    {
        use windows_sys::Win32::Foundation::{CloseHandle, INVALID_HANDLE_VALUE};
        use windows_sys::Win32::System::Diagnostics::ToolHelp::{
            CreateToolhelp32Snapshot, TH32CS_SNAPTHREAD, THREADENTRY32, Thread32First, Thread32Next,
        };
        use windows_sys::Win32::System::Threading::{
            OpenThread, ResumeThread, SuspendThread, THREAD_SUSPEND_RESUME,
        };
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

fn lines(file: &Path) -> usize {
    std::fs::read_to_string(file).map_or(0, |t| t.lines().count())
}

async fn wait_for(what: &str, limit: Duration, mut ready: impl FnMut() -> bool) {
    let end = Instant::now() + limit;
    while !ready() {
        assert!(Instant::now() < end, "timed out waiting for {what}");
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn pause_stops_the_real_cli_and_its_tool_and_resume_lets_them_go_on() {
    let Some(claude) = daemon::health::which("claude") else {
        assert!(
            std::env::var_os("TC_REQUIRE_REAL_CLI").is_none(),
            "TC_REQUIRE_REAL_CLI is set and claude is not on PATH"
        );
        eprintln!("skipped: claude is not on PATH");
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    let model = model::Model::start().await;
    let cwd = dir.path().join("work");
    std::fs::create_dir_all(&cwd).unwrap();
    // `run.json`, as the daemon writes it before a run starts (the executor records the CLI
    // in it).
    workspace::write_meta(
        dir.path(),
        &RunMeta {
            run_id: "r1".into(),
            started_at: 1,
            ..Default::default()
        },
    );
    let mut env: Vec<(String, String)> = vec![
        ("PATH".into(), std::env::var("PATH").unwrap()),
        ("HOME".into(), dir.path().to_string_lossy().into()),
        (
            "CLAUDE_CONFIG_DIR".into(),
            dir.path().join("claude").to_string_lossy().into(),
        ),
        (
            daemon::proctree::ENV_FOLDER.into(),
            dir.path().to_string_lossy().into(),
        ),
    ];
    for k in daemon::sys::OS_ENV {
        if let Ok(v) = std::env::var(k) {
            env.push((k.to_string(), v));
        }
    }
    env.extend(model.env().into_iter().map(|(k, v)| (k.to_string(), v)));
    // A shell loop in the CLI's Bash tool (Git Bash on Windows): one line a second.
    let prompt =
        "Run the tool.\nTOOL Bash for i in $(seq 1 90); do echo $i >> ticks.txt; sleep 1; done";
    let spec = RunSpec {
        run_id: "r1".into(),
        runtime_name: "claude".into(),
        cfg: RuntimeConfig {
            kind: "claude".into(),
            command: Some(claude.to_string_lossy().into()),
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
        cwd: cwd.clone(),
        run_dir: dir.path().to_path_buf(),
        env,
        aside: false,
        note: None,
        resume_session_id: None,
        gate_hook: None,
        gate_alive: None,
        gates: None,
        exec: daemon::executor::local(),
    };
    let (run_dir, exec) = (spec.run_dir.clone(), spec.exec.clone());
    let rt = runtime::for_kind("claude").unwrap();
    let (etx, _events) = mpsc::unbounded_channel();
    let (control, crx) = mpsc::unbounded_channel();
    let outcome = tokio::spawn(rt.start(spec, etx, crx));
    let ticks = cwd.join("ticks.txt");
    let stderr = || std::fs::read_to_string(run_dir.join("stderr.log")).unwrap_or_default();
    let end = Instant::now() + Duration::from_secs(120);
    while lines(&ticks) < 3 {
        assert!(
            Instant::now() < end && !outcome.is_finished(),
            "the tool's loop did not start; the CLI's stderr: {}",
            stderr()
        );
        tokio::time::sleep(Duration::from_millis(200)).await;
    }

    for round in 0..2 {
        let paused = exec
            .stop(&run_dir)
            .expect("the CLI runs: it can be stopped");
        // A write that had begun may still land.
        tokio::time::sleep(Duration::from_millis(1500)).await;
        let held = lines(&ticks);
        let tree: Vec<(u32, Option<bool>)> =
            below_me().into_iter().map(|p| (p, stopped(p))).collect();
        let live: Vec<_> = tree.iter().filter(|(_, s)| s.is_some()).collect();
        // The CLI and the shell of its tool at the least.
        assert!(live.len() >= 2, "round {round}: {tree:?}");
        assert!(
            live.iter().all(|(_, s)| *s == Some(true)),
            "round {round}: a process of the paused run is not stopped: {tree:?}"
        );
        tokio::time::sleep(Duration::from_secs(5)).await;
        assert_eq!(
            lines(&ticks),
            held,
            "round {round}: a paused run wrote a line"
        );
        paused.resume();
        wait_for("the loop to go on", Duration::from_secs(30), || {
            lines(&ticks) > held
        })
        .await;
        assert!(
            below_me().into_iter().any(|p| stopped(p) == Some(false)),
            "round {round}: nothing runs after the resume"
        );
    }

    // A cancel ends the CLI and the tool's loop.
    control.send(Control::Cancel).unwrap();
    let o = tokio::time::timeout(Duration::from_secs(60), outcome)
        .await
        .expect("the run ended")
        .unwrap();
    assert_eq!(o.status, Status::Cancelled, "{o:?}");
    wait_for(
        "every process of the run to end",
        Duration::from_secs(30),
        || below_me().into_iter().all(|p| stopped(p).is_none()),
    )
    .await;
    let end = lines(&ticks);
    tokio::time::sleep(Duration::from_secs(3)).await;
    assert_eq!(lines(&ticks), end, "the loop outlived the cancel");
}
