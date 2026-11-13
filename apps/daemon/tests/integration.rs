//! The whole daemon against an in-process mock server and a fake Claude CLI:
//! hello, assign, events, follow-up input, reject, cancel, reconnect with replay, session upload.
//!
//! The fake CLIs are Python scripts. Unix starts them by their `#!` line. Windows needs the
//! `fake-cli` launcher of the `test-fakes` feature (`cargo test --features test-fakes`) and
//! `python` on PATH; without the feature this file has no tests there.

#![cfg(any(unix, feature = "test-fakes"))]

use daemon::config::Config;
use daemon::proto::*;
use futures_util::{SinkExt, StreamExt};
use std::collections::BTreeMap;
use std::io::Read;
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::mpsc;
use tokio_tungstenite::WebSocketStream;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::tungstenite::handshake::server::{Request, Response};

#[path = "fake/model.rs"]
mod model;

const TOKEN: &str = "tm_test_token_value";

/// The `[runtimes.<name>]` section of a fake CLI script (`fake_claude.py`, `fake_codex.py`):
/// the script itself on Unix; on Windows the `fake-cli` launcher with the script in
/// `FAKE_CLI_SCRIPT`. `env` is more of the runtime's `env` table (`K = 'v'`, or empty).
fn fake_runtime(name: &str, kind: &str, script: &str, env: &str) -> String {
    let script = format!("{}/tests/fake/{script}", env!("CARGO_MANIFEST_DIR"));
    #[cfg(windows)]
    let (command, env) = (
        env!("CARGO_BIN_EXE_fake-cli").to_string(),
        format!(
            "FAKE_CLI_SCRIPT = '{script}'{}{env}",
            if env.is_empty() { "" } else { ", " }
        ),
    );
    #[cfg(not(windows))]
    let command = script;
    format!("[runtimes.{name}]\nkind = \"{kind}\"\ncommand = '{command}'\nenv = {{ {env} }}\n")
}

/// True while the process `pid` is there.
fn alive(pid: i32) -> bool {
    #[cfg(unix)]
    // SAFETY: probing a pid with signal 0.
    unsafe {
        libc::kill(pid, 0) == 0
    }
    #[cfg(windows)]
    daemon::winjob::pid_alive(pid as u32)
}

/// True when the process `pid` is stopped: state `T` on Unix; on Windows every one of its
/// threads is suspended (a suspend that is taken back at once tells the count before it).
fn stopped(pid: i32) -> bool {
    #[cfg(unix)]
    {
        let o = std::process::Command::new("ps")
            .args(["-o", "stat=", "-p", &pid.to_string()])
            .output()
            .unwrap();
        String::from_utf8_lossy(&o.stdout).contains('T')
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
                if e.th32OwnerProcessID == pid as u32 {
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
        threads > 0 && running == 0
    }
}

/// Ends a process the fake CLI started, whatever state it is in.
fn kill(pid: i32) {
    #[cfg(unix)]
    // SAFETY: plain syscalls on a pid that this test's fake CLI started.
    unsafe {
        libc::kill(pid, libc::SIGCONT);
        libc::kill(pid, libc::SIGKILL);
    }
    #[cfg(windows)]
    let _ = std::process::Command::new("taskkill")
        .args(["/F", "/PID", &pid.to_string()])
        .output();
}

#[derive(Debug, Clone)]
struct Put {
    path: String,
    headers: BTreeMap<String, String>,
    body: Vec<u8>,
}

type Ws = WebSocketStream<TcpStream>;

struct Mock {
    conns: mpsc::UnboundedReceiver<(Ws, Option<String>)>,
    puts: Arc<Mutex<Vec<Put>>>,
    url: String,
    /// While set, new connections are closed at once (the server is unreachable).
    refuse: Arc<std::sync::atomic::AtomicBool>,
}

/// One port serves both the WebSocket (GET with Upgrade) and session uploads (PUT).
async fn mock() -> Mock {
    let l = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", l.local_addr().unwrap());
    let (tx, rx) = mpsc::unbounded_channel();
    let puts = Arc::new(Mutex::new(vec![]));
    let puts2 = puts.clone();
    let refuse = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let refuse2 = refuse.clone();
    tokio::spawn(async move {
        loop {
            let Ok((s, _)) = l.accept().await else { return };
            if refuse2.load(std::sync::atomic::Ordering::SeqCst) {
                drop(s);
                continue;
            }
            let mut head = [0u8; 3];
            if s.peek(&mut head).await.is_err() {
                continue;
            }
            if &head == b"PUT" || &head == b"POS" {
                let puts = puts2.clone();
                tokio::spawn(handle_put(s, puts));
                continue;
            }
            let tx = tx.clone();
            tokio::spawn(async move {
                let auth = Arc::new(Mutex::new(None));
                let a2 = auth.clone();
                #[allow(clippy::result_large_err)] // the callback type is tungstenite's
                let cb = move |req: &Request, resp: Response| {
                    *a2.lock().unwrap() = req
                        .headers()
                        .get("authorization")
                        .and_then(|v| v.to_str().ok())
                        .map(str::to_string);
                    Ok(resp)
                };
                if let Ok(ws) = tokio_tungstenite::accept_hdr_async(s, cb).await {
                    let a = auth.lock().unwrap().clone();
                    let _ = tx.send((ws, a));
                }
            });
        }
    });
    Mock {
        conns: rx,
        puts,
        url,
        refuse,
    }
}

async fn handle_put(mut s: TcpStream, puts: Arc<Mutex<Vec<Put>>>) {
    let mut buf = vec![];
    let mut tmp = [0u8; 65536];
    let header_end = loop {
        let n = s.read(&mut tmp).await.unwrap();
        if n == 0 {
            return;
        }
        buf.extend_from_slice(&tmp[..n]);
        if let Some(i) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
            break i + 4;
        }
    };
    let head = String::from_utf8_lossy(&buf[..header_end]).to_string();
    let mut lines = head.split("\r\n");
    let path = lines.next().unwrap().split(' ').nth(1).unwrap().to_string();
    let headers: BTreeMap<String, String> = lines
        .filter_map(|l| l.split_once(": "))
        .map(|(k, v)| (k.to_ascii_lowercase(), v.to_string()))
        .collect();
    let len: usize = headers["content-length"].parse().unwrap();
    while buf.len() < header_end + len {
        let n = s.read(&mut tmp).await.unwrap();
        buf.extend_from_slice(&tmp[..n]);
    }
    puts.lock().unwrap().push(Put {
        path,
        headers,
        body: buf[header_end..header_end + len].to_vec(),
    });
    // chat-run files and 产出 (machine routes) answer JSON; a session upload answers nothing.
    let n = puts.lock().unwrap().len();
    let last = puts
        .lock()
        .unwrap()
        .last()
        .map(|p| p.path.clone())
        .unwrap_or_default();
    let body = if last.ends_with("/files") {
        format!(r#"{{"id":"F{n}","status":"ready"}}"#)
    } else if last.contains("/artifacts") {
        r#"{"artifact":{"id":"A1"},"version":{"artifactId":"A1","version":1,"runId":"a1"}}"#
            .to_string()
    } else {
        String::new()
    };
    let _ = s
        .write_all(
            format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            )
            .as_bytes(),
        )
        .await;
}

async fn send(ws: &mut Ws, d: &Down) {
    ws.send(Message::text(serde_json::to_string(d).unwrap()))
        .await
        .unwrap();
}

/// Next frame that is not a heartbeat; heartbeats get `heartbeat.ack` (as the server does), so
/// a quiet run does not look like a dead link.
async fn next(ws: &mut Ws) -> Up {
    loop {
        let m = tokio::time::timeout(Duration::from_secs(20), ws.next())
            .await
            .expect("frame in time")
            .expect("open")
            .expect("ok");
        if let Message::Text(t) = m {
            let up: Up = serde_json::from_str(&t).unwrap_or_else(|e| panic!("{e}: {t}"));
            match up {
                Up::Heartbeat { at, .. } => send(ws, &Down::HeartbeatAck { at }).await,
                up => return up,
            }
        }
    }
}

/// Frames for one run until `stop` matches (inclusive). Acks every sequenced frame.
async fn collect(ws: &mut Ws, run: &str, ack: bool, stop: impl Fn(&Up) -> bool) -> Vec<Up> {
    let mut out = vec![];
    loop {
        let f = next(ws).await;
        let mine = match &f {
            Up::RunEvent { run_id, .. }
            | Up::RunAccept { run_id }
            | Up::RunReject { run_id, .. } => run_id == run,
            Up::RunDone(d) => d.run_id == run,
            Up::RunSession(s) => s.run_id == run,
            _ => false,
        };
        if !mine {
            continue;
        }
        if ack && let Some(seq) = f.seq() {
            send(
                ws,
                &Down::Ack {
                    run_id: run.into(),
                    seq,
                },
            )
            .await;
        }
        let end = stop(&f);
        out.push(f);
        if end {
            return out;
        }
    }
}

fn assign(run: &str, runtime: &str, prompt: &str, upload: Option<String>) -> Down {
    Down::RunAssign(RunAssign {
        kind: None,
        resume_session_id: None,
        full_prompt: None,
        gates: None,
        github: None,
        run_id: run.into(),
        bot_member_id: "bot1".into(),
        group_id: "g1".into(),
        // Each run its own topic, so each has its own work line (`work_of`).
        topic_id: run.into(),
        runtime: runtime.into(),
        prompt: prompt.into(),
        model: None,
        effort: None,
        repo: None,
        env_allowlist: vec![],
        lease_ms: 600_000,
        max_duration_ms: None,
        session_upload: upload.map(|url| SessionUpload {
            url,
            headers: BTreeMap::new(),
        }),
    })
}

/// The working folder of run `run` of [`assign`]: the line of bot1 in the topic named like
/// the run.
fn work_of(tmp: &Path, run: &str) -> PathBuf {
    line_of(tmp, run).join("work")
}

fn line_of(tmp: &Path, topic: &str) -> PathBuf {
    tmp.join("work/lines")
        .join(daemon::workspace::line_key("bot1", topic, None))
}

fn welcome(runs: Vec<KnownRun>) -> Down {
    Down::Welcome {
        protocol: ProtocolV1,
        machine_id: "m1".into(),
        server_time: now_ms(),
        runs,
    }
}

fn events(frames: &[Up]) -> Vec<RunEvent> {
    frames
        .iter()
        .filter_map(|f| match f {
            Up::RunEvent { event, .. } => Some(event.clone()),
            _ => None,
        })
        .collect()
}

fn done(frames: &[Up]) -> RunDone {
    frames
        .iter()
        .find_map(|f| match f {
            Up::RunDone(d) => Some(d.clone()),
            _ => None,
        })
        .expect("run.done")
}

fn write_config(dir: &Path, url: &str) -> Config {
    write_config_with(dir, url, "")
}

/// `extra` is appended to the config (more runtimes).
fn write_config_with(dir: &Path, url: &str, extra: &str) -> Config {
    let token = dir.join("token");
    std::fs::write(&token, format!("{TOKEN}\n")).unwrap();
    // Only this user may read it: mode 600, on Windows an access list of its own.
    daemon::sys::set_mode(&token, 0o600).unwrap();
    let fake = fake_runtime(
        "fake",
        "claude",
        "fake_claude.py",
        &format!("CLAUDE_CONFIG_DIR = '{}'", dir.join("claude").display()),
    );
    let text = format!(
        r#"
server_url = "{url}"
token_file = '{token}'
name = "test-mac"
labels = ["test"]
work_root = '{work}'
max_parallel_runs = 3
min_free_gb = 0
heartbeat_secs = 1
takeover_url = "vnc://mac-mini.tailnet"

{fake}
{extra}
"#,
        token = token.display(),
        work = dir.join("work").display(),
    );
    let cfg = Config::parse(&text).unwrap();
    assert!(cfg.problems().is_empty(), "{:?}", cfg.problems());
    // The same config as a file, for a daemon started as a process of its own.
    let file = dir.join("daemon.toml");
    std::fs::write(&file, &text).unwrap();
    daemon::sys::set_mode(&file, 0o600).unwrap();
    cfg
}

fn gunzip(b: &[u8]) -> Vec<u8> {
    let mut out = vec![];
    flate2::read::GzDecoder::new(b)
        .read_to_end(&mut out)
        .unwrap();
    out
}

fn find_session(dir: &Path) -> PathBuf {
    let projects = dir.join("claude/projects");
    std::fs::read_dir(projects)
        .unwrap()
        .flatten()
        .find_map(|d| std::fs::read_dir(d.path()).unwrap().flatten().next())
        .expect("no session file")
        .path()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn daemon_end_to_end() {
    let tmp = tempfile::tempdir().unwrap();
    let mut m = mock().await;
    let cfg = write_config(tmp.path(), &m.url);
    let (stop_tx, stop_rx) = tokio::sync::oneshot::channel::<()>();
    let daemon = tokio::spawn(daemon::daemon::run(cfg, async {
        let _ = stop_rx.await;
    }));

    // Hello: token in the upgrade, machine info, the runtime and its availability.
    let (mut ws, auth) = tokio::time::timeout(Duration::from_secs(20), m.conns.recv())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(auth.as_deref(), Some(&*format!("Bearer {TOKEN}")));
    match next(&mut ws).await {
        Up::Hello {
            protocol: _,
            machine,
            clis,
            health,
            runs,
        } => {
            assert_eq!(machine.name, "test-mac");
            assert_eq!(machine.max_parallel_runs, 3);
            assert_eq!(clis.len(), 1);
            assert_eq!(
                (clis[0].runtime.as_str(), clis[0].kind.as_str()),
                ("fake", "claude")
            );
            assert!(clis[0].available);
            assert!(health.disk_total_bytes > 0 && health.cpu_count > 0);
            assert!(runs.is_empty());
        }
        f => panic!("{f:?}"),
    }
    send(&mut ws, &welcome(vec![])).await;

    // 1. Assign, stream, follow-up input, done, session upload.
    let upload = format!("{}/v1/machine/runs/r1/session", m.url);
    send(&mut ws, &assign("r1", "fake", "slow hello", Some(upload))).await;
    let mut frames = vec![];
    let mut sent_input = false;
    loop {
        let f = next(&mut ws).await;
        if let Some(seq) = f.seq() {
            send(
                &mut ws,
                &Down::Ack {
                    run_id: "r1".into(),
                    seq,
                },
            )
            .await;
        }
        if !sent_input
            && matches!(
                f,
                Up::RunEvent {
                    event: RunEvent::MessageDelta { .. },
                    ..
                }
            )
        {
            sent_input = true;
            send(
                &mut ws,
                &Down::RunInput {
                    answer: None,
                    artifact_event: None,
                    run_id: "r1".into(),
                    input_id: "in1".into(),
                    text: "follow up".into(),
                    author_id: Some("u1".into()),
                },
            )
            .await;
        }
        let end = matches!(f, Up::RunSession(_));
        frames.push(f);
        if end {
            break;
        }
    }
    assert!(matches!(frames[0], Up::RunAccept { .. }));
    let seqs: Vec<u64> = frames.iter().filter_map(Up::seq).collect();
    assert_eq!(
        seqs,
        (1..=seqs.len() as u64).collect::<Vec<_>>(),
        "seq is 1, 2, 3, ..."
    );
    let evs = events(&frames);
    assert!(
        evs.iter()
            .any(|e| matches!(e, RunEvent::Session { model: Some(m), .. } if m == "fake-model"))
    );
    assert!(
        evs.iter()
            .any(|e| matches!(e, RunEvent::InputAccepted { input_id } if input_id == "in1"))
    );
    assert!(
        evs.iter()
            .any(|e| matches!(e, RunEvent::ToolCall { tool, .. } if tool == "Bash"))
    );
    assert_eq!(
        evs.iter()
            .filter(|e| matches!(e, RunEvent::TurnCompleted { .. }))
            .count(),
        2
    );
    let d = done(&frames);
    assert_eq!(d.status, "succeeded", "{d:?}");
    assert_eq!(d.result_text.as_deref(), Some("reply 2: follow up"));
    assert_eq!(d.summary.as_deref(), Some("reply 2: follow up"));
    assert_eq!(d.usage.as_ref().map(|u| u.output_tokens), Some(8));
    let session = match frames.last().unwrap() {
        Up::RunSession(s) => s.clone(),
        f => panic!("{f:?}"),
    };
    assert!(session.ok, "{session:?}");
    assert_eq!(session.source.as_deref(), Some("cli"));
    let put = m.puts.lock().unwrap().last().cloned().expect("upload");
    assert_eq!(put.path, "/v1/machine/runs/r1/session");
    assert_eq!(
        put.headers.get("authorization").map(String::as_str),
        Some(&*format!("Bearer {TOKEN}"))
    );
    assert_eq!(put.headers["content-type"], "application/gzip");
    let raw = std::fs::read(find_session(tmp.path())).unwrap();
    assert_eq!(gunzip(&put.body), raw);
    assert_eq!(session.bytes, Some(raw.len() as u64));
    assert_eq!(
        put.headers["x-session-sha256"],
        session.sha256.clone().unwrap()
    );
    // The run folder keeps the normalized log and the copied session.
    let run_dir = tmp.path().join("work/runs/r1");
    assert!(run_dir.join("events.jsonl").is_file());
    assert_eq!(std::fs::read(run_dir.join("session.jsonl")).unwrap(), raw);

    // 2. Rejections.
    send(&mut ws, &assign("r-bad", "nope", "x", None)).await;
    match next(&mut ws).await {
        Up::RunReject { run_id, reason, .. } => {
            assert_eq!(run_id, "r-bad");
            assert_eq!(reason, "runtime_unavailable");
        }
        f => panic!("{f:?}"),
    }
    send(&mut ws, &assign("../evil", "fake", "x", None)).await;
    match next(&mut ws).await {
        Up::RunReject { reason, .. } => assert_eq!(reason, "invalid"),
        f => panic!("{f:?}"),
    }
    send(&mut ws, &assign("r1", "fake", "again", None)).await;
    match next(&mut ws).await {
        Up::RunReject { run_id, reason, .. } => {
            assert_eq!((run_id.as_str(), reason.as_str()), ("r1", "invalid"))
        }
        f => panic!("{f:?}"),
    }

    // 3. Cancel kills the CLI's process group.
    send(&mut ws, &assign("r3", "fake", "hang", None)).await;
    let pidfile = work_of(tmp.path(), "r3").join("fake.pid");
    let start = collect(&mut ws, "r3", true, |f| {
        matches!(
            f,
            Up::RunEvent {
                event: RunEvent::TurnStarted { .. },
                ..
            }
        )
    })
    .await;
    assert!(matches!(start[0], Up::RunAccept { .. }));
    // The file can be there before its content.
    let pid: i32 = wait_file(&pidfile).await.trim().parse().unwrap();
    send(
        &mut ws,
        &Down::RunCancel {
            run_id: "r3".into(),
            reason: None,
        },
    )
    .await;
    let fr = collect(&mut ws, "r3", true, |f| matches!(f, Up::RunDone(_))).await;
    assert_eq!(done(&fr).status, "cancelled");
    assert!(!alive(pid), "fake CLI still alive");

    // 4. Link drop: the run keeps going, then the daemon replays from the server's ackedSeq.
    send(&mut ws, &assign("r4", "fake", "slow replay", None)).await;
    let before = collect(&mut ws, "r4", false, |f| f.seq() == Some(2)).await;
    assert!(matches!(before[0], Up::RunAccept { .. }));
    m.refuse.store(true, std::sync::atomic::Ordering::SeqCst);
    drop(ws); // server side goes away without a close handshake
    // The run finishes while the link is down (5 deltas, 0.3 s apart).
    let r4_dir = tmp.path().join("work/runs/r4");
    tokio::time::timeout(Duration::from_secs(15), async {
        while daemon::workspace::read_meta(&r4_dir)
            .and_then(|m| m.finished_at)
            .is_none()
        {
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    })
    .await
    .expect("r4 finished while offline");
    m.refuse.store(false, std::sync::atomic::Ordering::SeqCst);
    let (mut ws, _) = tokio::time::timeout(Duration::from_secs(40), m.conns.recv())
        .await
        .unwrap()
        .unwrap();
    let r4_last = match next(&mut ws).await {
        Up::Hello { runs, .. } => {
            let c = runs.iter().find(|c| c.run_id == "r4").expect("r4 cursor");
            assert!(c.done, "the run finished while offline: {c:?}");
            c.last_seq
        }
        f => panic!("{f:?}"),
    };
    send(
        &mut ws,
        &welcome(vec![KnownRun {
            run_id: "r4".into(),
            acked_seq: 1,
        }]),
    )
    .await;
    let after = collect(&mut ws, "r4", true, |f| matches!(f, Up::RunDone(_))).await;
    let seqs: Vec<u64> = after.iter().filter_map(Up::seq).collect();
    assert_eq!(seqs.first(), Some(&2), "replay starts after ackedSeq");
    assert_eq!(seqs, (2..=r4_last).collect::<Vec<_>>());
    let d = done(&after);
    assert_eq!(d.status, "succeeded");
    assert_eq!(d.result_text.as_deref(), Some("reply 1: slow replay"));

    // 5. Heartbeat carries health and run cursors.
    let hb = loop {
        let m = tokio::time::timeout(Duration::from_secs(10), ws.next())
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        if let Message::Text(t) = m
            && let Ok(Up::Heartbeat { health, runs, .. }) = serde_json::from_str(&t)
        {
            break (health, runs);
        }
    };
    assert!(hb.0.cpu_count > 0);
    assert!(
        hb.1.iter().all(|c| c.run_id != "r1"),
        "acked finished runs are forgotten"
    );

    stop_tx.send(()).unwrap();
    tokio::time::timeout(Duration::from_secs(20), daemon)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn unknown_run_after_reconnect_is_cancelled() {
    let tmp = tempfile::tempdir().unwrap();
    let mut m = mock().await;
    let cfg = write_config(tmp.path(), &m.url);
    let (stop_tx, stop_rx) = tokio::sync::oneshot::channel::<()>();
    let daemon = tokio::spawn(daemon::daemon::run(cfg, async {
        let _ = stop_rx.await;
    }));
    let (mut ws, _) = m.conns.recv().await.unwrap();
    assert!(matches!(next(&mut ws).await, Up::Hello { .. }));
    send(&mut ws, &welcome(vec![])).await;
    send(&mut ws, &assign("r9", "fake", "hang", None)).await;
    collect(&mut ws, "r9", true, |f| {
        matches!(
            f,
            Up::RunEvent {
                event: RunEvent::TurnStarted { .. },
                ..
            }
        )
    })
    .await;
    let pidfile = work_of(tmp.path(), "r9").join("fake.pid");
    // The file can be there before its content.
    let pid: i32 = wait_file(&pidfile).await.trim().parse().unwrap();
    drop(ws);
    let (mut ws, _) = tokio::time::timeout(Duration::from_secs(40), m.conns.recv())
        .await
        .unwrap()
        .unwrap();
    match next(&mut ws).await {
        Up::Hello { runs, .. } => assert!(runs.iter().any(|c| c.run_id == "r9" && !c.done)),
        f => panic!("{f:?}"),
    }
    // The server lost the run (lease expired): the daemon stops it and sends nothing for it.
    send(&mut ws, &welcome(vec![])).await;
    let mut gone = false;
    for _ in 0..100 {
        if !alive(pid) {
            gone = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    assert!(gone, "orphaned run was not killed");
    stop_tx.send(()).unwrap();
    tokio::time::timeout(Duration::from_secs(20), daemon)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
}

/// A daemon that starts after a crash finds an unfinished run folder: it stops the CLI that was
/// left running, replays the frames the server has not acked, and ends the run with run.done
/// (failed: daemon restarted, with the run's CLI session) and a failed run.session. On a
/// graceful stop the runs it interrupts end the same way, and it waits for the ack of run.done
/// before it exits.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn restart_recovers_unfinished_runs() {
    #[cfg(unix)]
    use std::os::unix::process::CommandExt;
    let tmp = tempfile::tempdir().unwrap();
    let mut m = mock().await;
    let cfg = write_config(tmp.path(), &m.url);

    // What the crashed daemon left: a CLI in its own process group, run.json, two frames.
    // Windows: no CLI can be left. It was in a Job Object whose only handle the daemon held,
    // so it ended with the daemon (`tests/process_tree.rs` has the case); the run folder is
    // all there is.
    #[cfg(unix)]
    let mut left = std::process::Command::new("/bin/sleep")
        .arg("600")
        .process_group(0)
        .spawn()
        .unwrap();
    let dir = tmp.path().join("work/runs/rc1");
    std::fs::create_dir_all(dir.join("work")).unwrap();
    #[cfg(unix)]
    let meta = serde_json::json!({ "runId": "rc1", "startedAt": now_ms() - 5000, "pgid": left.id(), "program": "/bin/sleep" });
    #[cfg(windows)]
    let meta = serde_json::json!({ "runId": "rc1", "startedAt": now_ms() - 5000 });
    std::fs::write(dir.join("run.json"), meta.to_string()).unwrap();
    let frame = |seq: u64, event: RunEvent| {
        serde_json::to_string(&Up::RunEvent {
            run_id: "rc1".into(),
            seq,
            at: now_ms(),
            event,
        })
        .unwrap()
    };
    let lines = [
        frame(
            1,
            RunEvent::Session {
                session_id: "sess-rc1".into(),
                runtime: "claude".into(),
                model: None,
                effort: None,
            },
        ),
        frame(
            2,
            RunEvent::Message {
                item_id: "m1".into(),
                turn_id: None,
                role: "assistant".into(),
                text: "half done".into(),
            },
        ),
    ];
    std::fs::write(
        dir.join("events.jsonl"),
        format!("{}\n{}\n{{\"t\":\"run.ev", lines[0], lines[1]),
    )
    .unwrap();

    let (stop_tx, stop_rx) = tokio::sync::oneshot::channel::<()>();
    let daemon = tokio::spawn(daemon::daemon::run(cfg, async {
        let _ = stop_rx.await;
    }));
    let (mut ws, _) = m.conns.recv().await.unwrap();
    match next(&mut ws).await {
        Up::Hello { runs, .. } => {
            let c = runs.iter().find(|c| c.run_id == "rc1").expect("cursor");
            assert!(c.done);
            assert_eq!(c.last_seq, 4);
        }
        f => panic!("{f:?}"),
    }
    // The leftover CLI is stopped.
    #[cfg(unix)]
    {
        let mut stopped = false;
        for _ in 0..100 {
            if left.try_wait().unwrap().is_some() {
                stopped = true;
                break;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        assert!(stopped, "leftover CLI still runs");
    }

    send(
        &mut ws,
        &welcome(vec![KnownRun {
            run_id: "rc1".into(),
            acked_seq: 1,
        }]),
    )
    .await;
    let frames = collect(&mut ws, "rc1", true, |f| matches!(f, Up::RunSession(_))).await;
    let seqs: Vec<u64> = frames.iter().filter_map(Up::seq).collect();
    assert_eq!(seqs, vec![2, 3, 4]);
    let d = done(&frames);
    assert_eq!(d.status, "failed");
    assert_eq!(d.error.as_deref(), Some("daemon restarted"));
    assert_eq!(d.failure.map(|f| f.kind).as_deref(), Some("other"));
    // The session the run reported: the server continues the run in it (盯 1.7).
    assert_eq!(d.session_id.as_deref(), Some("sess-rc1"));
    assert!(matches!(frames.last(), Some(Up::RunSession(s)) if !s.ok));
    let meta: serde_json::Value =
        serde_json::from_slice(&std::fs::read(dir.join("run.json")).unwrap()).unwrap();
    assert!(meta["finishedAt"].is_u64());

    // Graceful stop with a running run (a computer or service restart): run.done goes out as
    // `daemon restarted`, so the server continues the run in its session (盯 1.7), and the
    // daemon exits soon after the server acks it.
    send(&mut ws, &assign("r2", "fake", "hang", None)).await;
    collect(&mut ws, "r2", true, |f| {
        matches!(
            f,
            Up::RunEvent {
                event: RunEvent::TurnStarted { .. },
                ..
            }
        )
    })
    .await;
    stop_tx.send(()).unwrap();
    let frames = collect(&mut ws, "r2", true, |f| matches!(f, Up::RunDone(_))).await;
    let d = done(&frames);
    assert_eq!(d.status, "failed");
    assert_eq!(d.error.as_deref(), Some("daemon restarted"));
    tokio::time::timeout(Duration::from_secs(8), daemon)
        .await
        .expect("daemon exits once run.done is acked")
        .unwrap()
        .unwrap();
}

/// A run that was itself a resume and stopped before its CLI reported a session: the restart
/// reports the session it was resuming, so the conversation is not lost.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn restart_reports_the_resumed_session_when_none_was_logged() {
    let tmp = tempfile::tempdir().unwrap();
    let mut m = mock().await;
    let cfg = write_config(tmp.path(), &m.url);
    let dir = tmp.path().join("work/runs/rc2");
    std::fs::create_dir_all(dir.join("work")).unwrap();
    let meta = serde_json::json!({ "runId": "rc2", "startedAt": now_ms() - 5000, "resumeSessionId": "sess-old" });
    std::fs::write(dir.join("run.json"), meta.to_string()).unwrap();
    let frame = serde_json::to_string(&Up::RunEvent {
        run_id: "rc2".into(),
        seq: 1,
        at: now_ms(),
        event: RunEvent::Status {
            state: "running".into(),
            message: None,
        },
    })
    .unwrap();
    std::fs::write(dir.join("events.jsonl"), format!("{frame}\n")).unwrap();
    let (stop_tx, stop_rx) = tokio::sync::oneshot::channel::<()>();
    let daemon = tokio::spawn(daemon::daemon::run(cfg, async {
        let _ = stop_rx.await;
    }));
    let (mut ws, _) = m.conns.recv().await.unwrap();
    assert!(matches!(next(&mut ws).await, Up::Hello { .. }));
    send(
        &mut ws,
        &welcome(vec![KnownRun {
            run_id: "rc2".into(),
            acked_seq: 1,
        }]),
    )
    .await;
    let frames = collect(&mut ws, "rc2", true, |f| matches!(f, Up::RunSession(_))).await;
    let d = done(&frames);
    assert_eq!(d.error.as_deref(), Some("daemon restarted"));
    assert_eq!(d.session_id.as_deref(), Some("sess-old"));
    stop_tx.send(()).unwrap();
    tokio::time::timeout(Duration::from_secs(8), daemon)
        .await
        .expect("daemon exits")
        .unwrap()
        .unwrap();
}

async fn wait_file(p: &Path) -> String {
    for _ in 0..200 {
        if let Ok(t) = std::fs::read_to_string(p)
            && !t.trim().is_empty()
        {
            return t;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    panic!("{} never written", p.display());
}

fn is_paused(f: &Up, want: bool) -> bool {
    matches!(f, Up::RunEvent { event: RunEvent::Paused { paused, .. }, .. } if *paused == want)
}

/// 1.1: hello reports the features and takeoverUrl; run.pause stops the CLI's process group and
/// run.resume continues it with the note as the next turn; a Card v2 answer reaches the CLI as
/// a fenced block of untrusted data; an acceptance block in the CLI's text becomes the
/// acceptance event; a cancel while paused still stops the CLI; an aside run gets Claude Code's
/// read-only mode and no repo-less runtime takes one.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn pause_resume_answer_acceptance_and_aside() {
    let tmp = tempfile::tempdir().unwrap();
    let mut m = mock().await;
    let cfg = write_config_with(
        tmp.path(),
        &m.url,
        if cfg!(windows) {
            "[runtimes.sh]\nkind = \"pty\"\ncommand = \"cmd.exe\"\nargs = [\"/c\", \"{prompt}\"]\n"
        } else {
            "[runtimes.sh]\nkind = \"pty\"\ncommand = \"/bin/sh\"\nargs = [\"-c\", \"{prompt}\"]\n"
        },
    );
    let (stop_tx, stop_rx) = tokio::sync::oneshot::channel::<()>();
    let daemon = tokio::spawn(daemon::daemon::run(cfg, async {
        let _ = stop_rx.await;
    }));
    let (mut ws, _) = m.conns.recv().await.unwrap();
    match next(&mut ws).await {
        Up::Hello { machine, .. } => {
            assert_eq!(
                machine.features.as_deref(),
                Some(
                    &[
                        "pause",
                        "answer",
                        "aside",
                        "acceptance",
                        "interact",
                        "output",
                        "resume",
                        "context",
                        "groups",
                        "effort",
                        "gates",
                        "github",
                        "github-host",
                        "takeover",
                        "sign-in-confirmed",
                        "limits",
                        "accounts",
                        "ask-keys"
                    ]
                    .map(String::from)[..]
                )
            );
            // Feature "github": where runs execute (no `[vm]` here).
            assert_eq!(machine.runs_in.as_deref(), Some("host"));
            // Feature "accounts": the machine key made next to the token file.
            assert_eq!(machine.public_key.as_deref().map(str::len), Some(44));
            assert_eq!(
                machine.takeover_url.as_deref(),
                Some("vnc://mac-mini.tailnet")
            );
        }
        f => panic!("{f:?}"),
    }
    send(&mut ws, &welcome(vec![])).await;

    // 1. Pause and resume with a note, then an answer with an acceptance block.
    send(&mut ws, &assign("p1", "fake", "tick", None)).await;
    let dir = work_of(tmp.path(), "p1");
    let mut frames = collect(&mut ws, "p1", true, |f| {
        matches!(
            f,
            Up::RunEvent {
                event: RunEvent::TurnStarted { .. },
                ..
            }
        )
    })
    .await;
    let pid: i32 = wait_file(&dir.join("fake.pid"))
        .await
        .trim()
        .parse()
        .unwrap();
    wait_file(&dir.join("ticks.txt")).await;
    send(
        &mut ws,
        &Down::RunPause {
            run_id: "p1".into(),
            by: Some("u1".into()),
            hold: None,
        },
    )
    .await;
    frames.extend(collect(&mut ws, "p1", true, |f| is_paused(f, true)).await);
    assert!(frames.iter().any(|f| matches!(f,
        Up::RunEvent { event: RunEvent::Paused { paused: true, by: Some(b) }, .. } if b == "u1")));
    assert!(stopped(pid), "CLI not stopped");
    // A second pause while stopped (its resume was lost) is reported again, so the server
    // can clear pausedBy on the next `paused: false` (SB-16).
    send(
        &mut ws,
        &Down::RunPause {
            run_id: "p1".into(),
            by: Some("u2".into()),
            hold: None,
        },
    )
    .await;
    let again = collect(&mut ws, "p1", true, |f| is_paused(f, true)).await;
    assert!(again.iter().any(|f| matches!(f,
        Up::RunEvent { event: RunEvent::Paused { paused: true, by: Some(b) }, .. } if b == "u2")));
    assert!(stopped(pid), "CLI not stopped");
    frames.extend(again);
    let ticks = std::fs::read_to_string(dir.join("ticks.txt")).unwrap();
    tokio::time::sleep(Duration::from_millis(600)).await;
    assert_eq!(
        std::fs::read_to_string(dir.join("ticks.txt")).unwrap(),
        ticks,
        "a stopped CLI made progress"
    );
    send(
        &mut ws,
        &Down::RunResume {
            run_id: "p1".into(),
            note: Some("go on".into()),
        },
    )
    .await;
    frames.extend(collect(&mut ws, "p1", true, |f| is_paused(f, false)).await);
    assert!(!stopped(pid), "CLI still stopped");
    let mut inputs = BTreeMap::new();
    inputs.insert("choice".to_string(), serde_json::json!("B"));
    send(
        &mut ws,
        &Down::RunInput {
            run_id: "p1".into(),
            input_id: "in-answer".into(),
            text: "\n```acceptance\n[{\"text\":\"Tests pass\",\"state\":\"pass\",\"evidence\":{\"text\":\"42 ok\"}}]\n```".into(),
            author_id: Some("u1".into()),
            answer: Some(RunAnswer {
                message_id: "msg1".into(),
                rev: 1,
                action_id: "pick".into(),
                inputs: Some(inputs),
                edited_values: None,
                actor_id: "u1".into(),
                answer_id: None,
                ask_id: None,
                event_id: None,
                event: None,
                block_id: None,
                value: None,
            }),
            artifact_event: None,
        },
    )
    .await;
    frames.extend(collect(&mut ws, "p1", true, |f| matches!(f, Up::RunDone(_))).await);
    let evs = events(&frames);
    assert!(
        evs.iter()
            .any(|e| matches!(e, RunEvent::InputAccepted { input_id } if input_id == "resume-1"))
    );
    assert!(
        evs.iter()
            .any(|e| matches!(e, RunEvent::InputAccepted { input_id } if input_id == "in-answer"))
    );
    let replies: Vec<&str> = evs
        .iter()
        .filter_map(|e| match e {
            RunEvent::Message { role, text, .. } if role == "assistant" => Some(text.as_str()),
            _ => None,
        })
        .collect();
    assert!(replies.contains(&"reply 2: go on"), "{replies:?}");
    let answer = replies.iter().find(|t| t.starts_with("reply 3:")).unwrap();
    assert!(answer.contains("untrusted data"), "{answer}");
    assert!(answer.contains("\"actionId\": \"pick\""), "{answer}");
    let acc = evs
        .iter()
        .find_map(|e| match e {
            RunEvent::Acceptance { items } => Some(items.clone()),
            _ => None,
        })
        .expect("acceptance event");
    assert_eq!(acc.len(), 1);
    assert_eq!(acc[0].text, "Tests pass");
    assert_eq!(
        acc[0].evidence.as_ref().and_then(|e| e.text.as_deref()),
        Some("42 ok")
    );
    assert_eq!(done(&frames).status, "succeeded");
    assert_eq!(
        std::fs::read_to_string(dir.join("ticks.txt"))
            .unwrap()
            .lines()
            .count(),
        30
    );

    // 2. Cancel while paused: the stopped CLI is resumed and killed.
    send(&mut ws, &assign("p2", "fake", "hang", None)).await;
    collect(&mut ws, "p2", true, |f| {
        matches!(
            f,
            Up::RunEvent {
                event: RunEvent::TurnStarted { .. },
                ..
            }
        )
    })
    .await;
    let pid: i32 = wait_file(&work_of(tmp.path(), "p2").join("fake.pid"))
        .await
        .trim()
        .parse()
        .unwrap();
    send(
        &mut ws,
        &Down::RunPause {
            run_id: "p2".into(),
            by: None,
            hold: None,
        },
    )
    .await;
    collect(&mut ws, "p2", true, |f| is_paused(f, true)).await;
    assert!(stopped(pid));
    send(
        &mut ws,
        &Down::RunCancel {
            run_id: "p2".into(),
            reason: None,
        },
    )
    .await;
    let fr = collect(&mut ws, "p2", true, |f| matches!(f, Up::RunDone(_))).await;
    assert_eq!(done(&fr).status, "cancelled");
    assert!(!alive(pid), "fake CLI still alive");

    // 3. Aside: Claude Code in plan mode with only the read tools, no MCP servers and no
    // settings files; no PTY aside; no unknown kinds.
    // The user's settings.json: the run keeps only its env, apiKeyHelper and model.
    std::fs::create_dir_all(tmp.path().join("claude")).unwrap();
    std::fs::write(
        tmp.path().join("claude/settings.json"),
        r#"{"env":{"ASIDE_PROBE":"from-settings"},"apiKeyHelper":"/bin/echo key","model":"m1",
            "hooks":{"PreToolUse":[]},"permissions":{"allow":["Bash"]}}"#,
    )
    .unwrap();
    let mut a = assign("s1", "fake", "what is the status", None);
    if let Down::RunAssign(r) = &mut a {
        r.kind = Some("aside".into());
        r.env_allowlist = vec!["PATH".into()];
    }
    send(&mut ws, &a).await;
    let fr = collect(&mut ws, "s1", true, |f| matches!(f, Up::RunDone(_))).await;
    assert_eq!(done(&fr).status, "succeeded");
    let argv: Vec<String> = serde_json::from_str(
        &std::fs::read_to_string(tmp.path().join("work/runs/s1/argv.json")).unwrap(),
    )
    .unwrap();
    let mode = argv.iter().position(|a| a == "--permission-mode").unwrap();
    assert_eq!(argv[mode + 1], "plan");
    let dis = argv.iter().position(|a| a == "--disallowedTools").unwrap();
    assert!(argv[dis + 1].contains("Write") && argv[dis + 1].contains("Edit"));
    assert!(argv[dis + 1].split(' ').any(|t| t == "Bash"));
    let tools = argv.iter().position(|a| a == "--tools").unwrap();
    assert_eq!(argv[tools + 1], "Read,Grep,Glob");
    let sources = argv.iter().position(|a| a == "--setting-sources").unwrap();
    assert_eq!(argv[sources + 1], "");
    let seen: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(tmp.path().join("work/runs/s1/settings-seen.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(seen["probe"], "from-settings");
    assert_eq!(
        seen["settings"],
        serde_json::json!({ "env": { "ASIDE_PROBE": "from-settings" }, "apiKeyHelper": "/bin/echo key", "model": "m1" })
    );
    // The checkout of a side chat run is read-only to its CLI (EACCES), and writable again after.
    let write: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(tmp.path().join("work/runs/s1/aside-write.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(write["name"], "EACCES", "{write}");
    let work = tmp.path().join("work/runs/s1/work");
    #[cfg(unix)]
    assert!(std::fs::metadata(&work).unwrap().permissions().mode() & 0o200 != 0);
    std::fs::write(work.join("after-the-run.txt"), "x").expect("writable again");
    // The settings file (auth values) is gone after the run and never on the command line.
    let settings = argv.iter().position(|a| a == "--settings").unwrap();
    assert!(!Path::new(&argv[settings + 1]).exists());
    assert!(!argv.iter().any(|a| a.contains("from-settings")));
    assert!(argv.iter().any(|a| a == "--strict-mcp-config"));
    assert!(!argv.iter().any(|a| a == "acceptEdits"));
    for (run, runtime, kind, reason) in [
        ("s2", "sh", "aside", "runtime_unavailable"),
        ("s3", "fake", "admin", "invalid"),
    ] {
        let mut a = assign(run, runtime, "x", None);
        if let Down::RunAssign(r) = &mut a {
            r.kind = Some(kind.into());
        }
        send(&mut ws, &a).await;
        match next(&mut ws).await {
            Up::RunReject {
                run_id, reason: r, ..
            } => assert_eq!((run_id.as_str(), r.as_str()), (run, reason)),
            f => panic!("{f:?}"),
        }
    }

    stop_tx.send(()).unwrap();
    tokio::time::timeout(Duration::from_secs(20), daemon)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
}

/// Kills the fake CLI's tool children when a test ends, so a failed assertion leaks nothing.
struct Reap(Vec<i32>);

impl Drop for Reap {
    fn drop(&mut self) {
        for k in self.0.iter().filter(|k| alive(**k)) {
            kill(*k);
        }
    }
}

fn file_len(p: &Path) -> u64 {
    std::fs::metadata(p).map(|m| m.len()).unwrap_or(0)
}

#[cfg(unix)]
fn hand_result(f: &Up, id: &str) -> bool {
    matches!(f, Up::RunEvent { event: RunEvent::HandResult { command_id, .. }, .. } if command_id == id)
}

#[cfg(unix)]
fn command(run: &str, id: &str, command: &str, by: &str) -> Down {
    Down::RunCommand {
        run_id: run.into(),
        command_id: id.into(),
        command: command.into(),
        by: by.into(),
    }
}

/// Usable, feature "takeover" (design-takeover T4): a pause becomes a take-over; only the
/// person holding the run runs commands, one at a time, in the work folder, without the run's
/// env; input for the CLI waits; at hand-back the server gets `hand.summary` and the CLI the
/// note on what the person did, then their words, then the waiting input. A resume of a run
/// that is not held still reports `paused: false`.
#[cfg(unix)]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_person_takes_over_runs_commands_and_hands_back() {
    let tmp = tempfile::tempdir().unwrap();
    let mut m = mock().await;
    let cfg = write_config(tmp.path(), &m.url);
    let (stop_tx, stop_rx) = tokio::sync::oneshot::channel::<()>();
    let daemon = tokio::spawn(daemon::daemon::run(cfg, async {
        let _ = stop_rx.await;
    }));
    let (mut ws, _) = m.conns.recv().await.unwrap();
    assert!(matches!(next(&mut ws).await, Up::Hello { .. }));
    send(&mut ws, &welcome(vec![])).await;

    send(&mut ws, &assign("h1", "fake", "tick", None)).await;
    let dir = work_of(tmp.path(), "h1");
    let mut frames = collect(&mut ws, "h1", true, |f| {
        matches!(
            f,
            Up::RunEvent {
                event: RunEvent::TurnStarted { .. },
                ..
            }
        )
    })
    .await;
    let pid: i32 = wait_file(&dir.join("fake.pid"))
        .await
        .trim()
        .parse()
        .unwrap();
    wait_file(&dir.join("ticks.txt")).await;

    // A resume while nothing is held is still answered.
    send(
        &mut ws,
        &Down::RunResume {
            run_id: "h1".into(),
            note: None,
        },
    )
    .await;
    frames.extend(collect(&mut ws, "h1", true, |f| is_paused(f, false)).await);

    // Before the take-over, no command runs.
    send(&mut ws, &command("h1", "c0", "echo no", "u1")).await;
    let refused = collect(&mut ws, "h1", true, |f| hand_result(f, "c0")).await;
    assert!(events(&refused).iter().any(|e| matches!(e,
        RunEvent::HandResult { ok: false, error: Some(e), .. } if e == "not_held")));

    // A pause, then a take-over of the same run.
    for hold in [None, Some("takeover".to_string())] {
        send(
            &mut ws,
            &Down::RunPause {
                run_id: "h1".into(),
                by: Some("u1".into()),
                hold,
            },
        )
        .await;
        frames.extend(collect(&mut ws, "h1", true, |f| is_paused(f, true)).await);
    }
    assert!(stopped(pid), "CLI not stopped");

    // Someone else cannot run commands.
    send(&mut ws, &command("h1", "c1", "echo no", "u2")).await;
    let refused = collect(&mut ws, "h1", true, |f| hand_result(f, "c1")).await;
    assert!(events(&refused).iter().any(|e| matches!(e,
        RunEvent::HandResult { ok: false, error: Some(e), .. } if e == "not_held")));

    // Input for the CLI waits for the hand-back.
    send(
        &mut ws,
        &Down::RunInput {
            run_id: "h1".into(),
            input_id: "in-later".into(),
            text: "after the hand-back".into(),
            author_id: Some("u3".into()),
            answer: None,
            artifact_event: None,
        },
    )
    .await;

    // The holder's command: in the work folder, with output, exit code and no run env.
    send(
        &mut ws,
        &command(
            "h1",
            "c2",
            "echo out; echo err >&2; echo \"run=[$CHAT_RUN_ID] path=[$PATH]\"; printf x > made.txt; exit 3",
            "u1",
        ),
    )
    .await;
    let c2 = collect(&mut ws, "h1", true, |f| hand_result(f, "c2")).await;
    let evs = events(&c2);
    assert!(evs.iter().any(|e| matches!(e,
        RunEvent::HandCommand { command_id, by, .. } if command_id == "c2" && by == "u1")));
    let out: String = evs
        .iter()
        .filter_map(|e| match e {
            RunEvent::HandOutput { text, .. } => Some(text.as_str()),
            _ => None,
        })
        .collect();
    assert!(out.contains("out\n") && out.contains("err\n"), "{out}");
    assert!(out.contains("run=[]"), "{out}");
    assert!(!out.contains("/runs/h1/bin"), "{out}");
    assert!(evs.iter().any(|e| matches!(
        e,
        RunEvent::HandResult {
            ok: false,
            exit_code: Some(3),
            ..
        }
    )));
    assert!(dir.join("made.txt").is_file());
    assert!(stopped(pid), "the CLI ran while held");

    // One at a time; a cancel ends a long one.
    send(&mut ws, &command("h1", "c3", "sleep 60", "u1")).await;
    collect(&mut ws, "h1", true, |f| {
        matches!(f, Up::RunEvent { event: RunEvent::HandCommand { command_id, .. }, .. } if command_id == "c3")
    })
    .await;
    send(&mut ws, &command("h1", "c4", "echo no", "u1")).await;
    let busy = collect(&mut ws, "h1", true, |f| hand_result(f, "c4")).await;
    assert!(events(&busy).iter().any(|e| matches!(e,
        RunEvent::HandResult { ok: false, error: Some(e), .. } if e == "busy")));
    send(
        &mut ws,
        &Down::RunCommandCancel {
            run_id: "h1".into(),
            command_id: "c3".into(),
        },
    )
    .await;
    let c3 = collect(&mut ws, "h1", true, |f| hand_result(f, "c3")).await;
    assert!(
        events(&c3)
            .iter()
            .any(|e| matches!(e, RunEvent::HandResult { ok: false, .. }))
    );

    // Hand back with words.
    send(
        &mut ws,
        &Down::RunResume {
            run_id: "h1".into(),
            note: Some("I made made.txt".into()),
        },
    )
    .await;
    frames.extend(collect(&mut ws, "h1", true, |f| matches!(f, Up::RunDone(_))).await);
    let evs = events(&frames);
    let summary = evs
        .iter()
        .find_map(|e| match e {
            RunEvent::HandSummary(s) => Some(s.clone()),
            _ => None,
        })
        .expect("hand.summary");
    assert_eq!(summary.by, "u1");
    assert_eq!(summary.commands, 2);
    assert!(
        summary.files.contains(&"made.txt".to_string()),
        "{summary:?}"
    );
    let accepted: Vec<&str> = evs
        .iter()
        .filter_map(|e| match e {
            RunEvent::InputAccepted { input_id } => Some(input_id.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(accepted, vec!["resume-1", "in-later"], "{evs:?}");
    let replies: Vec<&str> = evs
        .iter()
        .filter_map(|e| match e {
            RunEvent::Message { role, text, .. } if role == "assistant" => Some(text.as_str()),
            _ => None,
        })
        .collect();
    let note = replies
        .iter()
        .find(|t| t.starts_with("reply 2:"))
        .expect("hand-back note");
    assert!(
        note.contains("[team-chat] A person held your computer. Member u1 paused you"),
        "{note}"
    );
    assert!(note.contains("ran 2 commands in"), "{note}");
    assert!(note.contains("→ failed (exit 3)"), "{note}");
    assert!(
        note.contains("`sleep 60` → stopped by the person"),
        "{note}"
    );
    assert!(note.contains("changed: made.txt"), "{note}");
    assert!(note.contains("Member u1's note: I made made.txt"), "{note}");
    assert!(
        replies.contains(&"reply 3: after the hand-back"),
        "{replies:?}"
    );
    assert_eq!(done(&frames).status, "succeeded");

    let _ = stop_tx.send(());
    let _ = daemon.await;
}

/// Tool commands that a CLI starts in their own session or process group (Claude Code's Bash
/// tool, Codex exec) stop with the run on pause, continue on resume and are gone after cancel.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn pause_and_cancel_cover_tool_children() {
    let tmp = tempfile::tempdir().unwrap();
    let mut m = mock().await;
    let cfg = write_config(tmp.path(), &m.url);
    let (stop_tx, stop_rx) = tokio::sync::oneshot::channel::<()>();
    let daemon = tokio::spawn(daemon::daemon::run(cfg, async {
        let _ = stop_rx.await;
    }));
    let (mut ws, _) = m.conns.recv().await.unwrap();
    assert!(matches!(next(&mut ws).await, Up::Hello { .. }));
    send(&mut ws, &welcome(vec![])).await;

    send(&mut ws, &assign("c1", "fake", "spawn", None)).await;
    let dir = work_of(tmp.path(), "c1");
    wait_file(&dir.join("fake.pid")).await;
    let kids: Vec<i32> = wait_file(&dir.join("children.txt"))
        .await
        .split_whitespace()
        .map(|p| p.parse().unwrap())
        .collect();
    assert_eq!(kids.len(), 2);
    let _reap = Reap(kids.clone());
    let ticks = [dir.join("session.ticks"), dir.join("group.ticks")];
    for t in &ticks {
        wait_file(t).await;
    }

    send(
        &mut ws,
        &Down::RunPause {
            run_id: "c1".into(),
            by: None,
            hold: None,
        },
    )
    .await;
    collect(&mut ws, "c1", true, |f| is_paused(f, true)).await;
    for k in &kids {
        assert!(stopped(*k), "child {k} not stopped");
    }
    let before: Vec<u64> = ticks.iter().map(|t| file_len(t)).collect();
    tokio::time::sleep(Duration::from_millis(500)).await;
    let after: Vec<u64> = ticks.iter().map(|t| file_len(t)).collect();
    assert_eq!(before, after, "a stopped tool child made progress");

    send(
        &mut ws,
        &Down::RunResume {
            run_id: "c1".into(),
            note: None,
        },
    )
    .await;
    collect(&mut ws, "c1", true, |f| is_paused(f, false)).await;
    tokio::time::sleep(Duration::from_millis(500)).await;
    for (t, b) in ticks.iter().zip(&after) {
        assert!(
            file_len(t) > *b,
            "{} did not tick after resume",
            t.display()
        );
    }

    send(
        &mut ws,
        &Down::RunPause {
            run_id: "c1".into(),
            by: None,
            hold: None,
        },
    )
    .await;
    collect(&mut ws, "c1", true, |f| is_paused(f, true)).await;
    send(
        &mut ws,
        &Down::RunCancel {
            run_id: "c1".into(),
            reason: None,
        },
    )
    .await;
    let fr = collect(&mut ws, "c1", true, |f| matches!(f, Up::RunDone(_))).await;
    assert_eq!(done(&fr).status, "cancelled");
    let mut left = kids.clone();
    for _ in 0..100 {
        left.retain(|k| alive(*k));
        if left.is_empty() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert!(
        left.is_empty(),
        "tool children outlived the cancel: {left:?}"
    );

    stop_tx.send(()).unwrap();
    tokio::time::timeout(Duration::from_secs(20), daemon)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
}

/// The run's `active` in the heartbeats until `want` (acks them; other frames are dropped).
#[cfg(unix)]
async fn heartbeat_active(ws: &mut Ws, run: &str, want: Option<bool>) {
    let mut seen = vec![];
    for _ in 0..60 {
        loop {
            let m = tokio::time::timeout(Duration::from_secs(20), ws.next())
                .await
                .expect("frame in time")
                .expect("open")
                .expect("ok");
            let Message::Text(t) = m else { continue };
            if let Ok(Up::Heartbeat { at, runs, .. }) = serde_json::from_str::<Up>(&t) {
                send(ws, &Down::HeartbeatAck { at }).await;
                let got = runs.iter().find(|r| r.run_id == run).and_then(|r| r.active);
                if got == want {
                    return;
                }
                seen.push(got);
                break;
            }
        }
    }
    panic!("no heartbeat with active {want:?} for {run}: {seen:?}");
}

/// 盯 1.4: while a tool the CLI started burns CPU (a build), the heartbeat says the run is
/// active; a CLI that only waits is not.
#[cfg(unix)]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_heartbeat_says_when_a_runs_tools_are_working() {
    let tmp = tempfile::tempdir().unwrap();
    let mut m = mock().await;
    // The run's tools are sampled every 500 ms, not every 15 s.
    let cfg = write_config_with(tmp.path(), &m.url, "[test_clocks]\ntick_ms = 500\n");
    let (stop_tx, stop_rx) = tokio::sync::oneshot::channel::<()>();
    let daemon = tokio::spawn(daemon::daemon::run(cfg, async {
        let _ = stop_rx.await;
    }));
    let (mut ws, _) = m.conns.recv().await.unwrap();
    assert!(matches!(next(&mut ws).await, Up::Hello { .. }));
    send(&mut ws, &welcome(vec![])).await;

    // A CLI that only waits: known, and not active.
    send(&mut ws, &assign("w1", "fake", "hang", None)).await;
    wait_file(&work_of(tmp.path(), "w1").join("fake.pid")).await;
    heartbeat_active(&mut ws, "w1", Some(false)).await;
    send(
        &mut ws,
        &Down::RunCancel {
            run_id: "w1".into(),
            reason: None,
        },
    )
    .await;
    collect(&mut ws, "w1", true, |f| matches!(f, Up::RunDone(_))).await;

    // A tool at work: active from the second sample on (one tick).
    send(&mut ws, &assign("b1", "fake", "busy", None)).await;
    let dir = work_of(tmp.path(), "b1");
    let kid: i32 = wait_file(&dir.join("children.txt"))
        .await
        .trim()
        .parse()
        .unwrap();
    let _reap = Reap(vec![kid]);
    heartbeat_active(&mut ws, "b1", Some(true)).await;
    send(
        &mut ws,
        &Down::RunCancel {
            run_id: "b1".into(),
            reason: None,
        },
    )
    .await;
    collect(&mut ws, "b1", true, |f| matches!(f, Up::RunDone(_))).await;

    stop_tx.send(()).unwrap();
    tokio::time::timeout(Duration::from_secs(20), daemon)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
}

/// Feature "limits": the app's number of runs at once wins over the config's (3), until the
/// server sends none again.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_app_sets_how_many_runs_go_at_once() {
    let tmp = tempfile::tempdir().unwrap();
    let mut m = mock().await;
    let cfg = write_config(tmp.path(), &m.url);
    let (stop_tx, stop_rx) = tokio::sync::oneshot::channel::<()>();
    let daemon = tokio::spawn(daemon::daemon::run(cfg, async {
        let _ = stop_rx.await;
    }));
    let (mut ws, _) = m.conns.recv().await.unwrap();
    match next(&mut ws).await {
        Up::Hello { machine, .. } => {
            assert!(
                machine
                    .features
                    .unwrap_or_default()
                    .iter()
                    .any(|f| f == "limits")
            );
        }
        f => panic!("{f:?}"),
    }
    send(&mut ws, &welcome(vec![])).await;
    send(
        &mut ws,
        &Down::MachineLimits {
            max_parallel_runs: Some(1),
        },
    )
    .await;

    send(&mut ws, &assign("l1", "fake", "hang", None)).await;
    wait_file(&work_of(tmp.path(), "l1").join("fake.pid")).await;
    send(&mut ws, &assign("l2", "fake", "hang", None)).await;
    let frames = collect(&mut ws, "l2", false, |f| {
        matches!(f, Up::RunReject { .. } | Up::RunAccept { .. })
    })
    .await;
    assert!(
        frames
            .iter()
            .any(|f| matches!(f, Up::RunReject { reason, .. } if reason == "busy")),
        "{frames:?}"
    );

    // Back to the config's 3: a second run goes.
    send(
        &mut ws,
        &Down::MachineLimits {
            max_parallel_runs: None,
        },
    )
    .await;
    send(&mut ws, &assign("l3", "fake", "hang", None)).await;
    wait_file(&work_of(tmp.path(), "l3").join("fake.pid")).await;

    for run in ["l1", "l3"] {
        send(
            &mut ws,
            &Down::RunCancel {
                run_id: run.into(),
                reason: None,
            },
        )
        .await;
        collect(&mut ws, run, true, |f| matches!(f, Up::RunDone(_))).await;
    }
    stop_tx.send(()).unwrap();
    tokio::time::timeout(Duration::from_secs(20), daemon)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
}

/// 1.1 `chat-run artifact create`: the daemon uploads each file of the folder from the run's
/// working folder (text redacted) and makes the 产出 with the machine token; the CLI gets its id.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn chat_run_artifact_create_uploads_the_folder_and_makes_the_artifact() {
    daemon::chat_run::set_exe(PathBuf::from(env!("CARGO_BIN_EXE_daemon")));
    let tmp = tempfile::tempdir().unwrap();
    let mut m = mock().await;
    let cfg = write_config(tmp.path(), &m.url);
    let (stop_tx, stop_rx) = tokio::sync::oneshot::channel::<()>();
    let daemon = tokio::spawn(daemon::daemon::run(cfg, async {
        let _ = stop_rx.await;
    }));
    let (mut ws, _) = m.conns.recv().await.unwrap();
    assert!(matches!(next(&mut ws).await, Up::Hello { .. }));
    send(&mut ws, &welcome(vec![])).await;
    send(&mut ws, &assign("a1", "fake", "artifact", None)).await;
    let frames = collect(&mut ws, "a1", true, |f| matches!(f, Up::RunDone(_))).await;
    assert_eq!(done(&frames).status, "succeeded");
    let report = chat_run_report(&work_of(tmp.path(), "a1"), 1).await;
    assert_eq!(report["code"], 0, "{report}");
    let out: serde_json::Value = serde_json::from_str(report["out"].as_str().unwrap()).unwrap();
    assert_eq!(out["artifact"]["id"], "A1");
    let puts = m.puts.lock().unwrap().clone();
    let files: Vec<&Put> = puts
        .iter()
        .filter(|p| p.path == "/v1/machine/runs/a1/files")
        .collect();
    assert_eq!(files.len(), 2);
    for p in &files {
        assert_eq!(p.headers["authorization"], format!("Bearer {TOKEN}"));
    }
    let page = files
        .iter()
        .find(|p| p.headers["x-file-name"] == "index.html")
        .unwrap();
    assert_eq!(page.headers["content-type"], "text/html");
    let text = String::from_utf8_lossy(&page.body);
    assert!(
        !text.contains("tm_fakefake") && text.contains("<redacted>"),
        "{text}"
    );
    use sha2::Digest;
    let sha: String = sha2::Sha256::digest(&page.body)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    assert_eq!(page.headers["x-file-sha256"], sha);
    let made = puts
        .iter()
        .find(|p| p.path == "/v1/machine/runs/a1/artifacts")
        .expect("artifact create");
    let body: serde_json::Value = serde_json::from_slice(&made.body).unwrap();
    assert_eq!(body["title"], "看板");
    assert_eq!(body["kind"], "html");
    assert_eq!(body["entry"], "index.html");
    let paths: Vec<&str> = body["files"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| f["path"].as_str().unwrap())
        .collect();
    assert_eq!(paths, ["app.js", "index.html"]);
    assert!(
        body["files"][0]["fileId"]
            .as_str()
            .unwrap()
            .starts_with('F')
    );
    stop_tx.send(()).unwrap();
    tokio::time::timeout(Duration::from_secs(20), daemon)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
}

/// Waits for the fake CLI's `chatrun-<n>.json`.
async fn chat_run_report(dir: &Path, n: u32) -> serde_json::Value {
    serde_json::from_str(&wait_file(&dir.join(format!("chatrun-{n}.json"))).await).unwrap()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn interaction_envelope_and_chat_run() {
    daemon::chat_run::set_exe(PathBuf::from(env!("CARGO_BIN_EXE_daemon")));
    let tmp = tempfile::tempdir().unwrap();
    let mut m = mock().await;
    let cfg = write_config(tmp.path(), &m.url);
    let (stop_tx, stop_rx) = tokio::sync::oneshot::channel::<()>();
    let daemon = tokio::spawn(daemon::daemon::run(cfg, async {
        let _ = stop_rx.await;
    }));
    let (mut ws, _) = m.conns.recv().await.unwrap();
    assert!(matches!(next(&mut ws).await, Up::Hello { .. }));
    send(&mut ws, &welcome(vec![])).await;

    // chat-run card patch: the output event goes up redacted, the CLI gets the result. (No
    // result in time is `chat_run`'s unit tests.)
    send(&mut ws, &assign("c1", "fake", "chatrun", None)).await;
    #[cfg(unix)]
    let run_dir = tmp.path().join("work/runs/c1");
    let mut frames = collect(&mut ws, "c1", true, |f| {
        matches!(
            f,
            Up::RunEvent {
                event: RunEvent::Output { .. },
                ..
            }
        )
    })
    .await;
    let Some(Up::RunEvent {
        event: RunEvent::Output {
            output_id,
            op,
            body,
        },
        ..
    }) = frames.last().cloned()
    else {
        panic!("no output event");
    };
    assert_eq!(op, "card.patch");
    assert_eq!(body["messageId"], "M");
    assert_eq!(body["baseRev"], 1);
    assert_eq!(body["cause"], "EV1");
    let text = body["ops"][0]["props"]["text"].as_str().unwrap();
    assert!(
        !text.contains("tm_fakefake") && text.contains("<redacted>"),
        "{text}"
    );
    #[cfg(unix)]
    let socket = run_dir.join("chat-run.sock");
    #[cfg(unix)]
    assert_eq!(
        std::fs::metadata(&socket).unwrap().permissions().mode() & 0o777,
        0o600
    );
    let argv: Vec<String> = serde_json::from_str(
        &std::fs::read_to_string(work_of(tmp.path(), "c1").join("argv.json")).unwrap(),
    )
    .unwrap();
    let note = argv
        .iter()
        .position(|a| a == "--append-system-prompt")
        .unwrap();
    assert!(
        argv[note + 1].contains("chat-run card patch"),
        "{}",
        argv[note + 1]
    );

    // A stored answer while the CLI is still in its turn: one envelope turn, input.accepted.
    let mut inputs = BTreeMap::new();
    inputs.insert("check".to_string(), serde_json::json!({ "value": "wrong" }));
    send(
        &mut ws,
        &Down::RunAnswer(RunAnswerFrame {
            run_id: "c1".into(),
            input_id: "in-7".into(),
            source: RunAnswerSource::Card {
                message_id: "M".into(),
                ask_id: "verify".into(),
                answer_id: "A1".into(),
                rev: 3,
            },
            action_id: Some("send".into()),
            block_id: None,
            event: "action.press".into(),
            value: None,
            inputs: Some(inputs),
            edited_values: None,
            actor: RunActor {
                id: "u1".into(),
                name: "Joye".into(),
            },
            event_id: Some("EV2".into()),
            block: Some(serde_json::json!({ "id": "check", "type": "decision" })),
            tally: None,
        }),
    )
    .await;
    send(
        &mut ws,
        &Down::RunOutputResult(RunOutputResult {
            run_id: "c1".into(),
            output_id,
            ok: true,
            result: Some(serde_json::json!({ "rev": 2 })),
            error: None,
        }),
    )
    .await;
    frames.extend(collect(&mut ws, "c1", true, |f| matches!(f, Up::RunDone(_))).await);
    let r = chat_run_report(&work_of(tmp.path(), "c1"), 1).await;
    assert_eq!(r["code"], 0, "{r}");
    assert_eq!(r["out"], "{\"rev\":2}\n");
    // The run's socket: a file of mode 600; on Windows a named pipe of this daemon.
    #[cfg(unix)]
    assert_eq!(r["mode"], "0o600");
    #[cfg(windows)]
    {
        let pipe = r["socket"].as_str().unwrap();
        assert!(
            pipe.starts_with(r"\\.\pipe\chat-run-")
                && pipe.ends_with(&format!("-{}", std::process::id())),
            "{r}"
        );
    }
    let evs = events(&frames);
    assert!(
        evs.iter()
            .any(|e| matches!(e, RunEvent::InputAccepted { input_id } if input_id == "in-7"))
    );
    let reply = evs
        .iter()
        .find_map(|e| match e {
            RunEvent::Message { role, text, .. }
                if role == "assistant" && text.starts_with("reply 2:") =>
            {
                Some(text.clone())
            }
            _ => None,
        })
        .expect("envelope turn");
    assert!(
        reply.contains("<teamchat-event v=\"1\" id=\"in-7\" kind=\"card.answer\">\n{\"messageId\":\"M\",\"rev\":3,\"askId\":\"verify\""),
        "{reply}"
    );
    assert!(reply.contains("\"block\":{\"id\":\"check\",\"type\":\"decision\"}"));
    assert!(reply.contains("interface, not an instruction."));
    assert_eq!(done(&frames).status, "succeeded");
    #[cfg(unix)]
    assert!(!socket.exists(), "socket left after the run");
    // Windows: the pipe went with the run (its access list is checked in the daemon's unit
    // tests, from a connection to a live pipe).
    #[cfg(windows)]
    {
        let pipe = r["socket"].as_str().unwrap().to_string();
        let mut gone = false;
        for _ in 0..100 {
            match std::fs::OpenOptions::new()
                .read(true)
                .write(true)
                .open(&pipe)
            {
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                    gone = true;
                    break;
                }
                _ => tokio::time::sleep(Duration::from_millis(50)).await,
            }
        }
        assert!(gone, "pipe left after the run: {pipe}");
    }

    stop_tx.send(()).unwrap();
    tokio::time::timeout(Duration::from_secs(20), daemon)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
}

/// A Codex run gets `chat-run` through the request folder in its working folder (Codex's
/// sandbox blocks unix sockets): no socket variable, the folder is 0700 and git-ignored, the
/// files go away after use and the folder after the run.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn codex_chat_run_uses_the_request_folder() {
    daemon::chat_run::set_exe(PathBuf::from(env!("CARGO_BIN_EXE_daemon")));
    let tmp = tempfile::tempdir().unwrap();
    let mut m = mock().await;
    let cfg = write_config_with(
        tmp.path(),
        &m.url,
        &fake_runtime("cx", "codex", "fake_codex.py", ""),
    );
    let (stop_tx, stop_rx) = tokio::sync::oneshot::channel::<()>();
    let daemon = tokio::spawn(daemon::daemon::run(cfg, async {
        let _ = stop_rx.await;
    }));
    let (mut ws, _) = m.conns.recv().await.unwrap();
    assert!(matches!(next(&mut ws).await, Up::Hello { .. }));
    send(&mut ws, &welcome(vec![])).await;

    send(&mut ws, &assign("x1", "cx", "chatrun", None)).await;
    let work = work_of(tmp.path(), "x1");
    let frames = collect(&mut ws, "x1", true, |f| {
        matches!(
            f,
            Up::RunEvent {
                event: RunEvent::Output { .. },
                ..
            }
        )
    })
    .await;
    let Some(Up::RunEvent {
        event: RunEvent::Output { output_id, op, .. },
        ..
    }) = frames.last().cloned()
    else {
        panic!("no output event");
    };
    assert_eq!(op, "interaction.resolve");
    send(
        &mut ws,
        &Down::RunOutputResult(RunOutputResult {
            run_id: "x1".into(),
            output_id,
            ok: true,
            result: Some(serde_json::json!({ "resolved": true })),
            error: None,
        }),
    )
    .await;
    let r: serde_json::Value =
        serde_json::from_str(&wait_file(&work.join("chatrun.json")).await).unwrap();
    assert_eq!(r["code"], 0, "{r}");
    assert_eq!(r["out"], "{\"resolved\":true}\n");
    assert_eq!(r["socket"], serde_json::Value::Null);
    assert_eq!(
        Path::new(r["dir"].as_str().unwrap()),
        work.join(".chat-run-x1")
    );
    // Private to this user: mode 700; on Windows the folder inherits the run folder's access
    // list (the daemon's unit tests check it).
    #[cfg(unix)]
    assert_eq!(r["mode"], "0o700");
    assert_eq!(r["files"], serde_json::json!([".gitignore"]));
    collect(&mut ws, "x1", true, |f| matches!(f, Up::RunDone(_))).await;
    assert!(!work.join(".chat-run-x1").exists(), "request folder left");

    stop_tx.send(()).unwrap();
    tokio::time::timeout(Duration::from_secs(20), daemon)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
}

/// A follow-up run in the same topic continues the CLI session of the run before it: run 1
/// reports its session, run 2 gets `run.assign.resumeSessionId` and starts the CLI with
/// `--resume` of that session (the session file copied into its own project folder). The daemon
/// reports feature "resume".
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_follow_up_run_resumes_the_session() {
    let tmp = tempfile::tempdir().unwrap();
    let mut m = mock().await;
    let cfg = write_config(tmp.path(), &m.url);
    let (stop_tx, stop_rx) = tokio::sync::oneshot::channel::<()>();
    let daemon = tokio::spawn(daemon::daemon::run(cfg, async {
        let _ = stop_rx.await;
    }));
    let (mut ws, _) = tokio::time::timeout(Duration::from_secs(20), m.conns.recv())
        .await
        .unwrap()
        .unwrap();
    match next(&mut ws).await {
        Up::Hello { machine, .. } => {
            assert!(
                machine
                    .features
                    .unwrap_or_default()
                    .iter()
                    .any(|f| f == "resume")
            );
        }
        f => panic!("{f:?}"),
    }
    send(&mut ws, &welcome(vec![])).await;

    send(&mut ws, &assign("f1", "fake", "first question", None)).await;
    let fr = collect(&mut ws, "f1", true, |f| matches!(f, Up::RunDone(_))).await;
    let s1 = done(&fr).session_id.expect("run 1 session");

    // The CLI has exited; an answer in the topic starts run 2 with the session.
    let mut a = assign("f2", "fake", "the answer", None);
    if let Down::RunAssign(r) = &mut a {
        r.resume_session_id = Some(s1.clone());
        r.topic_id = "f1".into();
    }
    send(&mut ws, &a).await;
    let fr = collect(&mut ws, "f2", true, |f| matches!(f, Up::RunDone(_))).await;
    assert_eq!(done(&fr).status, "succeeded");
    assert_eq!(done(&fr).session_id.as_deref(), Some(s1.as_str()));
    assert_eq!(done(&fr).resume_missed, None);
    let argv: Vec<String> = serde_json::from_str(
        &std::fs::read_to_string(work_of(tmp.path(), "f1").join("argv.json")).unwrap(),
    )
    .unwrap();
    let i = argv.iter().position(|a| a == "--resume").expect("--resume");
    assert_eq!(argv[i + 1], s1);
    assert!(!argv.iter().any(|a| a == "--session-id"));
    // Run 2 worked in run 1's folder: no folder of its own, and no other line.
    assert!(!tmp.path().join("work/runs/f2/work").exists());
    assert!(!line_of(tmp.path(), "f2").exists());

    // A session the CLI no longer has starts a new one, with the whole prompt
    // (`run.assign.fullPrompt`), and `run.done` says so.
    let mut a = assign("f3", "fake", "again", None);
    if let Down::RunAssign(r) = &mut a {
        r.resume_session_id = Some("gone-session".into());
        r.full_prompt = Some("first question, the answer, again".into());
    }
    send(&mut ws, &a).await;
    let fr = collect(&mut ws, "f3", true, |f| matches!(f, Up::RunDone(_))).await;
    assert_eq!(done(&fr).status, "succeeded");
    assert_ne!(done(&fr).session_id.as_deref(), Some("gone-session"));
    assert_eq!(done(&fr).resume_missed, Some(true));
    assert_eq!(
        done(&fr).result_text.as_deref(),
        Some("reply 1: first question, the answer, again")
    );
    let argv: Vec<String> = serde_json::from_str(
        &std::fs::read_to_string(work_of(tmp.path(), "f3").join("argv.json")).unwrap(),
    )
    .unwrap();
    assert!(argv.iter().any(|a| a == "--session-id"));

    stop_tx.send(()).unwrap();
    tokio::time::timeout(Duration::from_secs(20), daemon)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
}

/// An AI member on an ACP agent through acceptance paths 3, 4 and 5 (usable, 接 3.6): hello says
/// the agent continues sessions but has no effort and no read-only mode; the request and the
/// run note reach the agent; a follow-up during a turn becomes the next prompt and a stop ends
/// the run; the topic's next run loads the session in the same work folder and the agent
/// remembers the first turn.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_acp_member_takes_a_task_follows_up_and_continues() {
    let tmp = tempfile::tempdir().unwrap();
    let mut m = mock().await;
    let store = tmp.path().join("acp-sessions");
    let acp = fake_runtime(
        "agent",
        "acp",
        "fake_acp.py",
        &format!("FAKE_ACP_STORE = '{}'", store.display()),
    );
    // A configured effort list is not reported for an agent that takes none.
    let cfg = write_config_with(tmp.path(), &m.url, &format!("{acp}efforts = [\"high\"]\n"));
    let (stop_tx, stop_rx) = tokio::sync::oneshot::channel::<()>();
    let daemon = tokio::spawn(daemon::daemon::run(cfg, async {
        let _ = stop_rx.await;
    }));
    let (mut ws, _) = tokio::time::timeout(Duration::from_secs(30), m.conns.recv())
        .await
        .unwrap()
        .unwrap();
    match next(&mut ws).await {
        Up::Hello { clis, .. } => {
            let agent = clis.iter().find(|c| c.runtime == "agent").expect("agent");
            assert_eq!(agent.resume, Some(true), "{agent:?}");
            assert_eq!(agent.aside, Some(false));
            assert_eq!(agent.efforts, None);
            let claude = clis.iter().find(|c| c.runtime == "fake").expect("fake");
            assert_eq!((claude.resume, claude.aside), (Some(true), Some(true)));
        }
        f => panic!("{f:?}"),
    }
    assert!(!store.exists(), "the probe started no session");
    send(&mut ws, &welcome(vec![])).await;

    // Path 3: the request (with its file line) and the run note reach the agent.
    let request = "@dev fix the login page\n  [file f 01J · shot.png · image/png]";
    send(&mut ws, &assign("a1", "agent", request, None)).await;
    let fr = collect(&mut ws, "a1", true, |f| matches!(f, Up::RunDone(_))).await;
    let d1 = done(&fr);
    assert_eq!(d1.status, "succeeded", "{d1:?}");
    let s1 = d1.session_id.clone().expect("session");
    let first: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(store.join(format!("{s1}.json"))).unwrap())
            .unwrap();
    let got = first["prompts"][0].as_str().unwrap();
    assert!(got.ends_with(request), "{got}");
    assert!(got.contains("chat-run"), "the run note: {got}");
    assert_eq!(
        Path::new(first["cwd"].as_str().unwrap()),
        work_of(tmp.path(), "a1")
    );

    // Path 4: the work is visible as it goes, a follow-up during the turn is the next prompt,
    // and a stop ends the run.
    send(&mut ws, &assign("a2", "agent", "slow job", None)).await;
    let mut fr = collect(&mut ws, "a2", true, |f| {
        matches!(
            f,
            Up::RunEvent {
                event: RunEvent::MessageDelta { .. },
                ..
            }
        )
    })
    .await;
    send(
        &mut ws,
        &Down::RunInput {
            answer: None,
            artifact_event: None,
            run_id: "a2".into(),
            input_id: "in1".into(),
            text: "also the docs".into(),
            author_id: Some("u1".into()),
        },
    )
    .await;
    fr.extend(collect(&mut ws, "a2", true, |f| matches!(f, Up::RunDone(_))).await);
    let evs = events(&fr);
    assert!(
        evs.iter()
            .any(|e| matches!(e, RunEvent::InputAccepted { input_id } if input_id == "in1"))
    );
    assert_eq!(
        evs.iter()
            .filter(|e| matches!(e, RunEvent::TurnCompleted { .. }))
            .count(),
        2
    );
    assert_eq!(done(&fr).result_text.as_deref(), Some("acp: also the docs"));

    send(&mut ws, &assign("a3", "agent", "slow again", None)).await;
    collect(&mut ws, "a3", true, |f| {
        matches!(
            f,
            Up::RunEvent {
                event: RunEvent::TurnStarted { .. },
                ..
            }
        )
    })
    .await;
    send(
        &mut ws,
        &Down::RunCancel {
            run_id: "a3".into(),
            reason: None,
        },
    )
    .await;
    let fr = collect(&mut ws, "a3", true, |f| matches!(f, Up::RunDone(_))).await;
    assert_eq!(done(&fr).status, "cancelled");

    // Path 5: the topic's next run continues the session, in the same work folder.
    let mut a = assign("a4", "agent", "recall", None);
    if let Down::RunAssign(r) = &mut a {
        r.resume_session_id = Some(s1.clone());
        r.topic_id = "a1".into();
    }
    send(&mut ws, &a).await;
    let fr = collect(&mut ws, "a4", true, |f| matches!(f, Up::RunDone(_))).await;
    let d4 = done(&fr);
    assert_eq!(d4.status, "succeeded", "{d4:?}");
    assert_eq!(d4.session_id.as_deref(), Some(s1.as_str()));
    let text = d4.result_text.unwrap();
    assert!(
        text.starts_with("acp: remembered [") && text.contains(request),
        "{text}"
    );
    let cwd = text.rsplit_once(" in ").map(|(_, c)| c).unwrap_or_default();
    assert_eq!(Path::new(cwd), work_of(tmp.path(), "a1"), "{text}");
    // The replayed history is not reported as this run's messages.
    assert!(!events(&fr).iter().any(|e| matches!(
        e,
        RunEvent::Message { text, .. } | RunEvent::MessageDelta { text, .. } if text == "old answer"
    )));
    assert!(!tmp.path().join("work/runs/a4/work").exists());

    stop_tx.send(()).unwrap();
    tokio::time::timeout(Duration::from_secs(20), daemon)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
}

/// Feature `effort`: hello reports the CLI's models and efforts; a run with an effort the CLI
/// reported starts it with `--effort`, one with an effort it did not report runs without.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn runs_get_an_effort_the_cli_reported() {
    let tmp = tempfile::tempdir().unwrap();
    let mut m = mock().await;
    let cfg = write_config(tmp.path(), &m.url);
    let (stop_tx, stop_rx) = tokio::sync::oneshot::channel::<()>();
    let daemon = tokio::spawn(daemon::daemon::run(cfg, async {
        let _ = stop_rx.await;
    }));
    let (mut ws, _) = tokio::time::timeout(Duration::from_secs(20), m.conns.recv())
        .await
        .unwrap()
        .unwrap();
    match next(&mut ws).await {
        Up::Hello { clis, .. } => {
            let ids: Vec<&str> = clis[0]
                .models
                .iter()
                .flatten()
                .map(|m| m.id.as_str())
                .collect();
            assert_eq!(ids, ["fable", "opus", "sonnet", "haiku"]);
            assert_eq!(
                clis[0].efforts.as_deref().unwrap(),
                ["low", "medium", "high", "xhigh", "max"]
            );
        }
        f => panic!("{f:?}"),
    }
    send(&mut ws, &welcome(vec![])).await;

    for (run, effort, want) in [("e1", "max", true), ("e2", "turbo", false)] {
        let mut a = assign(run, "fake", "hello", None);
        if let Down::RunAssign(r) = &mut a {
            r.effort = Some(effort.into());
        }
        send(&mut ws, &a).await;
        let fr = collect(&mut ws, run, true, |f| matches!(f, Up::RunDone(_))).await;
        assert_eq!(done(&fr).status, "succeeded");
        let argv: Vec<String> = serde_json::from_str(
            &std::fs::read_to_string(work_of(tmp.path(), run).join("argv.json")).unwrap(),
        )
        .unwrap();
        let i = argv.iter().position(|a| a == "--effort");
        assert_eq!(i.is_some(), want, "{argv:?}");
        if let Some(i) = i {
            assert_eq!(argv[i + 1], effort);
        }
    }

    stop_tx.send(()).unwrap();
    tokio::time::timeout(Duration::from_secs(20), daemon)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
}

/// The login state of `runtime` in the next heartbeat that has metrics (other frames are
/// skipped).
async fn next_login(ws: &mut Ws, runtime: &str) -> String {
    loop {
        let m = tokio::time::timeout(Duration::from_secs(20), ws.next())
            .await
            .expect("frame in time")
            .expect("open")
            .expect("ok");
        let Message::Text(t) = m else { continue };
        if let Up::Heartbeat { at, metrics, .. } = serde_json::from_str::<Up>(&t).unwrap() {
            send(ws, &Down::HeartbeatAck { at }).await;
            if let Some(l) = metrics
                .and_then(|m| m.logins)
                .and_then(|l| l.into_iter().find(|l| l.runtime == runtime))
            {
                return l.state;
            }
        }
    }
}

/// 模 2.2, 3.2: a run that failed because the CLI is not signed in says so in `run.done`, and
/// the heartbeat reports the CLI signed out at once, whatever its credential files say, until
/// a run of it succeeds.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_signed_out_cli_fails_with_its_kind_and_reports_signed_out() {
    let tmp = tempfile::tempdir().unwrap();
    // A credential file: the files say signed in.
    let claude = tmp.path().join("claude");
    std::fs::create_dir_all(&claude).unwrap();
    std::fs::write(
        claude.join(".credentials.json"),
        r#"{"claudeAiOauth":{"accessToken":"x"}}"#,
    )
    .unwrap();
    let mut m = mock().await;
    let cfg = write_config(tmp.path(), &m.url);
    let (stop_tx, stop_rx) = tokio::sync::oneshot::channel::<()>();
    let daemon = tokio::spawn(daemon::daemon::run(cfg, async {
        let _ = stop_rx.await;
    }));
    let (mut ws, _) = tokio::time::timeout(Duration::from_secs(20), m.conns.recv())
        .await
        .unwrap()
        .unwrap();
    assert!(matches!(next(&mut ws).await, Up::Hello { .. }));
    send(&mut ws, &welcome(vec![])).await;
    assert_eq!(next_login(&mut ws, "fake").await, "logged_in");

    let mut a = assign("so1", "fake", "signedout", None);
    if let Down::RunAssign(r) = &mut a {
        r.effort = Some("high".into());
    }
    send(&mut ws, &a).await;
    let fr = collect(&mut ws, "so1", true, |f| matches!(f, Up::RunDone(_))).await;
    let d = done(&fr);
    assert_eq!(d.status, "failed");
    assert_eq!(
        d.error.as_deref(),
        Some("Not logged in · Please run /login")
    );
    assert_eq!(d.failure.map(|f| f.kind).as_deref(), Some("signed_out"));
    // The session event names the model and the effort the CLI ran with.
    assert!(events(&fr).iter().any(|e| matches!(e,
        RunEvent::Session { model: Some(m), effort: Some(e), .. } if m == "fake-model" && e == "high")));
    assert_eq!(next_login(&mut ws, "fake").await, "logged_out");

    send(&mut ws, &assign("ok1", "fake", "hello", None)).await;
    let fr = collect(&mut ws, "ok1", true, |f| matches!(f, Up::RunDone(_))).await;
    let d = done(&fr);
    assert_eq!(d.status, "succeeded");
    assert_eq!(d.failure, None);
    assert_eq!(next_login(&mut ws, "fake").await, "logged_in");

    stop_tx.send(()).unwrap();
    tokio::time::timeout(Duration::from_secs(20), daemon)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
}

/// 模 3.4, 盯 1.5: a run that stops at the subscription limit fails with kind `limit`, with the
/// reset the CLI named or without one, and keeps its session; the server's continuation after
/// the reset (`resumeSessionId`, same topic) resumes that session in the same work folder.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_run_at_its_limit_says_so_and_goes_on_in_its_session() {
    let tmp = tempfile::tempdir().unwrap();
    let mut m = mock().await;
    let cfg = write_config(tmp.path(), &m.url);
    let (stop_tx, stop_rx) = tokio::sync::oneshot::channel::<()>();
    let daemon = tokio::spawn(daemon::daemon::run(cfg, async {
        let _ = stop_rx.await;
    }));
    let (mut ws, _) = tokio::time::timeout(Duration::from_secs(20), m.conns.recv())
        .await
        .unwrap()
        .unwrap();
    assert!(matches!(next(&mut ws).await, Up::Hello { .. }));
    send(&mut ws, &welcome(vec![])).await;

    send(&mut ws, &assign("lim1", "fake", "limitedreset", None)).await;
    let fr = collect(&mut ws, "lim1", true, |f| matches!(f, Up::RunDone(_))).await;
    let d = done(&fr);
    assert_eq!(d.status, "failed");
    let f = d.failure.expect("failure");
    assert_eq!(f.kind, "limit");
    assert_eq!(f.resets_at, Some(4_102_444_000_000));
    let s1 = d.session_id.expect("session");

    // No reset named: still the limit, without a time.
    send(&mut ws, &assign("lim2", "fake", "limited", None)).await;
    let fr = collect(&mut ws, "lim2", true, |f| matches!(f, Up::RunDone(_))).await;
    let f = done(&fr).failure.expect("failure");
    assert_eq!((f.kind.as_str(), f.resets_at), ("limit", None));

    // After the reset the server continues the work: same topic, the session of lim1.
    let mut a = assign("lim3", "fake", "go on", None);
    if let Down::RunAssign(r) = &mut a {
        r.resume_session_id = Some(s1.clone());
        r.topic_id = "lim1".into();
    }
    send(&mut ws, &a).await;
    let fr = collect(&mut ws, "lim3", true, |f| matches!(f, Up::RunDone(_))).await;
    let d = done(&fr);
    assert_eq!(d.status, "succeeded");
    assert_eq!(d.failure, None);
    assert_eq!(d.session_id.as_deref(), Some(s1.as_str()));
    let argv: Vec<String> = serde_json::from_str(
        &std::fs::read_to_string(work_of(tmp.path(), "lim1").join("argv.json")).unwrap(),
    )
    .unwrap();
    let i = argv.iter().position(|a| a == "--resume").expect("--resume");
    assert_eq!(argv[i + 1], s1);
    assert!(!line_of(tmp.path(), "lim3").exists());

    stop_tx.send(()).unwrap();
    tokio::time::timeout(Duration::from_secs(20), daemon)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
}

fn gated(run: &str, prompt: &str) -> Down {
    let mut a = assign(run, "fake", prompt, None);
    if let Down::RunAssign(r) = &mut a {
        r.gates = Some(vec![RunGate {
            kind: "mergeMain".into(),
            patterns: vec![r"\bgh pr merge\b".into()],
        }]);
    }
    a
}

fn approval(run: &str, approval_id: &str, decision: &str, scope: Option<&str>) -> Down {
    Down::RunApproval {
        run_id: run.into(),
        approval_id: approval_id.into(),
        decision: decision.into(),
        scope: scope.map(str::to_string),
    }
}

/// The approval.request of a gated command (the last frame of `frames`).
fn gate_request(frames: &[Up]) -> (String, Option<String>, Option<String>) {
    match frames.last() {
        Some(Up::RunEvent {
            event:
                RunEvent::ApprovalRequest {
                    approval_id,
                    kind,
                    command,
                    ..
                },
            ..
        }) => (approval_id.clone(), kind.clone(), command.clone()),
        f => panic!("expected approval.request, got {f:?}"),
    }
}

fn is_request(f: &Up) -> bool {
    matches!(
        f,
        Up::RunEvent {
            event: RunEvent::ApprovalRequest { .. },
            ..
        }
    )
}

/// M4: a Claude Code run with gates gets the PreToolUse hook; `gh pr merge` waits for
/// `run.approval` (deny blocks it with a reason the CLI reads, approve runs it), other commands
/// never ask, and 这件事里都同意 (scope "run") lets later merges of the run pass.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn gated_commands_wait_for_a_person() {
    daemon::chat_run::set_exe(PathBuf::from(env!("CARGO_BIN_EXE_daemon")));
    let tmp = tempfile::tempdir().unwrap();
    let mut m = mock().await;
    let cfg = write_config(tmp.path(), &m.url);
    let (stop_tx, stop_rx) = tokio::sync::oneshot::channel::<()>();
    let daemon = tokio::spawn(daemon::daemon::run(cfg, async {
        let _ = stop_rx.await;
    }));
    let (mut ws, _) = m.conns.recv().await.unwrap();
    match next(&mut ws).await {
        Up::Hello { machine, clis, .. } => {
            assert!(machine.features.unwrap().iter().any(|f| f == "gates"));
            assert_eq!(clis[0].gates, Some(true), "a claude runtime is gated");
        }
        f => panic!("{f:?}"),
    }
    send(&mut ws, &welcome(vec![])).await;
    let work = |run: &str| work_of(tmp.path(), run);
    let report = |run: &str| work(run).join("gate-1.json");

    // 1. Deny: the command does not run and the CLI reads why.
    send(&mut ws, &gated("g1", "gated")).await;
    let fr = collect(&mut ws, "g1", true, is_request).await;
    let (id, kind, command) = gate_request(&fr);
    assert_eq!(kind.as_deref(), Some("mergeMain"));
    assert_eq!(command.as_deref(), Some("gh pr merge 12 --squash"));
    let argv: Vec<String> =
        serde_json::from_str(&std::fs::read_to_string(work("g1").join("argv.json")).unwrap())
            .unwrap();
    assert!(
        argv.iter().any(|a| a.ends_with(".gate-settings.json")),
        "{argv:?}"
    );
    send(&mut ws, &approval("g1", &id, "deny", None)).await;
    let fr = collect(&mut ws, "g1", true, |f| matches!(f, Up::RunDone(_))).await;
    assert!(events(&fr).iter().any(|e| matches!(
        e,
        RunEvent::ApprovalResolved { approval_id, decision } if *approval_id == id && decision == "deny"
    )));
    assert_eq!(done(&fr).status, "succeeded");
    let r: serde_json::Value = serde_json::from_str(&wait_file(&report("g1")).await).unwrap();
    assert_eq!(r["hooks"], 1);
    let why = r["commands"][0]["denied"].as_str().expect("merge denied");
    assert!(why.contains("declined"), "{why}");
    assert!(r["commands"][1]["denied"].is_null(), "ls never asks: {r}");
    // The settings file goes with the run.
    assert!(!tmp.path().join("work/runs/g1/.gate-settings.json").exists());

    // 2. 这件事里都同意: the first merge asks, the second passes without a card.
    send(&mut ws, &gated("g2", "gated twice")).await;
    let fr = collect(&mut ws, "g2", true, is_request).await;
    let (id, _, _) = gate_request(&fr);
    send(&mut ws, &approval("g2", &id, "approve", Some("run"))).await;
    let fr = collect(&mut ws, "g2", true, |f| matches!(f, Up::RunDone(_))).await;
    assert_eq!(
        fr.iter().filter(|f| is_request(f)).count(),
        0,
        "asked again"
    );
    let r: serde_json::Value = serde_json::from_str(&wait_file(&report("g2")).await).unwrap();
    let cmds = r["commands"].as_array().unwrap();
    assert_eq!(cmds.len(), 3);
    assert!(cmds.iter().all(|c| c["denied"].is_null()), "{r}");

    // 3. No gates: no hook, nothing asks.
    send(&mut ws, &assign("g3", "fake", "gated", None)).await;
    let fr = collect(&mut ws, "g3", true, |f| matches!(f, Up::RunDone(_))).await;
    assert!(!fr.iter().any(is_request));
    let r: serde_json::Value = serde_json::from_str(&wait_file(&report("g3")).await).unwrap();
    assert_eq!(r["hooks"], 0);

    stop_tx.send(()).unwrap();
    tokio::time::timeout(Duration::from_secs(20), daemon)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
}

/// 盯 2.2 and verification log problem 3: pushes to main written another way. The run's git
/// shim decides on the branch a push would really update. `git -C . push origin main` asks
/// once (the hook asks, its shim does not ask again), a plain `git push` on main and
/// `B=main; git push origin HEAD:$B` ask at the shim, and a declined one does not run. Real git
/// in a repository with no remote: an approved push then fails for want of the remote.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn pushes_to_main_written_another_way_ask_once() {
    daemon::chat_run::set_exe(PathBuf::from(env!("CARGO_BIN_EXE_daemon")));
    if daemon::health::which("git").is_none() {
        eprintln!("skipped: git is not on PATH");
        return;
    }
    let tmp = tempfile::tempdir().unwrap();
    let mut m = mock().await;
    let cfg = write_config(tmp.path(), &m.url);
    let (stop_tx, stop_rx) = tokio::sync::oneshot::channel::<()>();
    let daemon = tokio::spawn(daemon::daemon::run(cfg, async {
        let _ = stop_rx.await;
    }));
    let (mut ws, _) = m.conns.recv().await.unwrap();
    next(&mut ws).await;
    send(&mut ws, &welcome(vec![])).await;
    let mut a = assign("p1", "fake", "pushes", None);
    if let Down::RunAssign(r) = &mut a {
        r.gates = Some(default_gates());
    }
    send(&mut ws, &a).await;

    // `git -C . push origin main`: the hook asks; approved, the shim lets it run.
    let fr = collect(&mut ws, "p1", true, is_request).await;
    let (id, kind, command) = gate_request(&fr);
    assert_eq!(kind.as_deref(), Some("mergeMain"));
    assert_eq!(command.as_deref(), Some("git -C . push origin main"));
    send(&mut ws, &approval("p1", &id, "approve", None)).await;

    // A plain `git push` on main: the hook sees nothing, the shim asks.
    let fr = collect(&mut ws, "p1", true, is_request).await;
    let (id, kind, command) = gate_request(&fr);
    assert_eq!(kind.as_deref(), Some("mergeMain"));
    let command = command.unwrap();
    assert!(command.starts_with("git push\n"), "{command}");
    assert!(
        command.contains("git push origin HEAD:refs/heads/main"),
        "{command}"
    );
    send(&mut ws, &approval("p1", &id, "deny", None)).await;

    // `B=main; git push origin HEAD:$B`: the shim sees `HEAD:main`.
    if cfg!(unix) {
        let fr = collect(&mut ws, "p1", true, is_request).await;
        let (id, _, command) = gate_request(&fr);
        let command = command.unwrap();
        assert!(command.contains("HEAD:refs/heads/main"), "{command}");
        send(&mut ws, &approval("p1", &id, "deny", None)).await;
    }
    let fr = collect(&mut ws, "p1", true, |f| matches!(f, Up::RunDone(_))).await;
    assert!(!fr.iter().any(is_request), "asked a second time");
    assert_eq!(done(&fr).status, "succeeded");

    let r: serde_json::Value =
        serde_json::from_str(&wait_file(&work_of(tmp.path(), "p1").join("push-1.json")).await)
            .unwrap();
    let cmds = r["commands"].as_array().unwrap();
    assert_eq!(cmds[0]["ran"]["code"], 0, "{r}");
    // Approved: git ran and failed only for the missing remote.
    let approved = &cmds[1]["ran"];
    assert_ne!(approved["code"], 0, "{r}");
    assert!(
        !approved["stderr"].as_str().unwrap().contains("approval"),
        "{r}"
    );
    // Declined at the shim: git never ran, and the CLI reads why.
    for c in &cmds[2..] {
        assert!(c["denied"].is_null(), "{r}");
        assert_eq!(c["ran"]["code"], 1, "{r}");
        assert!(
            c["ran"]["stderr"].as_str().unwrap().contains("declined"),
            "{r}"
        );
    }

    stop_tx.send(()).unwrap();
    tokio::time::timeout(Duration::from_secs(20), daemon)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
}

/// The real `claude` of this machine (the real-CLI cases are `#[ignore]`d and run by
/// `scripts/test-real-cli.sh`, which checks it is there).
fn real_claude() -> PathBuf {
    daemon::health::which("claude").expect("claude is not on PATH")
}

/// The `[runtimes.real]` section: the real CLI with the fake model, its config in
/// `<dir>/real-claude`. Windows: with the PowerShell tool, the shell tool of a machine
/// without Git Bash.
fn real_runtime(dir: &Path, claude: &Path, model: &model::Model) -> String {
    let mut env: Vec<String> = model
        .env()
        .into_iter()
        .map(|(k, v)| format!("{k} = '{v}'"))
        .collect();
    env.push(format!(
        "CLAUDE_CONFIG_DIR = '{}'",
        dir.join("real-claude").display()
    ));
    if cfg!(windows) {
        env.push("CLAUDE_CODE_USE_POWERSHELL_TOOL = '1'".into());
    }
    format!(
        "[runtimes.real]\nkind = \"claude\"\ncommand = '{}'\nenv = {{ {} }}\n",
        claude.display(),
        env.join(", ")
    )
}

/// A folder by its path (what `tempfile::TempDir::path` gives).
struct Dir(PathBuf);

impl Dir {
    fn path(&self) -> &Path {
        &self.0
    }
}

/// The server's default patterns (`DEFAULT_GATE_PATTERNS` of packages/protocol), as the
/// server sends them in `run.assign.gates`.
fn default_gates() -> Vec<RunGate> {
    let src = include_str!("../../../packages/protocol/src/settings.ts");
    let body = &src[src.find("export const DEFAULT_GATE_PATTERNS").unwrap()..];
    let body = &body[..body.find("\n};").unwrap()];
    let mut out: Vec<RunGate> = vec![];
    for line in body.lines().skip(1) {
        let t = line.trim();
        if let Some(kind) = t.strip_suffix(": [") {
            out.push(RunGate {
                kind: kind.into(),
                patterns: vec![],
            });
        } else if t.starts_with('"') {
            let p: String = serde_json::from_str(t.trim_end_matches(',')).unwrap();
            out.last_mut().unwrap().patterns.push(p);
        }
    }
    assert_eq!(out.len(), 5);
    out
}

/// The same with the real Claude Code CLI of this machine and a fake model
/// (`tests/fake/model.rs`): the daemon reports the CLI as gated, the model calls the Bash tool,
/// the CLI starts the hook, the hook asks the daemon, the daemon asks a person. Declined: the
/// command does not run and the model reads why. Approved: it runs.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "real CLI: scripts/test-real-cli.sh"]
async fn the_real_cli_waits_for_a_person_at_a_gated_command() {
    let claude = real_claude();
    daemon::chat_run::set_exe(PathBuf::from(env!("CARGO_BIN_EXE_daemon")));
    let tmp = tempfile::tempdir().unwrap();
    // Every path of the run (its folder, the hook in it, the CLI's config) has spaces, text
    // outside ASCII and the characters shells give a meaning.
    let base = tmp.path().join("运行 dir &()^%!");
    std::fs::create_dir_all(base.join("real-claude")).unwrap();
    // The user turned hooks off in their own settings: the run's settings turn them on again.
    std::fs::write(
        base.join("real-claude").join("settings.json"),
        r#"{"disableAllHooks": true}"#,
    )
    .unwrap();
    let mut m = mock().await;
    let model = model::Model::start().await;
    let cfg = write_config_with(&base, &m.url, &real_runtime(&base, &claude, &model));
    let tmp = Dir(base);
    let (stop_tx, stop_rx) = tokio::sync::oneshot::channel::<()>();
    let daemon = tokio::spawn(daemon::daemon::run(cfg, async {
        let _ = stop_rx.await;
    }));
    let (mut ws, _) = m.conns.recv().await.unwrap();
    match next(&mut ws).await {
        Up::Hello { clis, .. } => {
            let real = clis.iter().find(|c| c.runtime == "real").expect("real");
            assert!(real.available);
            let version = real.version.clone().expect("the CLI says its version");
            assert!(version.contains("Claude Code"), "{version}");
            assert_eq!(real.gates, Some(true), "{version} is gated");
        }
        f => panic!("{f:?}"),
    }
    send(&mut ws, &welcome(vec![])).await;
    let gated = |run: &str, marker: &str| {
        let mut a = assign(
            run,
            "real",
            &format!("Run the tool.\nTOOL Bash echo {marker}"),
            None,
        );
        if let Down::RunAssign(r) = &mut a {
            r.gates = Some(vec![RunGate {
                kind: "mergeMain".into(),
                patterns: vec!["gated-marker".into()],
            }]);
        }
        a
    };
    let stderr = |run: &str| {
        std::fs::read_to_string(tmp.path().join(format!("work/runs/{run}/stderr.log")))
            .unwrap_or_default()
    };

    // 1. Declined: the command does not run and the model reads why.
    send(&mut ws, &gated("rg1", "gated-marker-declined")).await;
    let fr = collect(&mut ws, "rg1", true, is_request).await;
    let (id, kind, command) = gate_request(&fr);
    assert_eq!(kind.as_deref(), Some("mergeMain"));
    assert_eq!(command.as_deref(), Some("echo gated-marker-declined"));
    send(&mut ws, &approval("rg1", &id, "deny", None)).await;
    let fr = collect(&mut ws, "rg1", true, |f| matches!(f, Up::RunDone(_))).await;
    assert_eq!(done(&fr).status, "succeeded", "{}", stderr("rg1"));
    let results = model.tool_results();
    assert_eq!(results.len(), 1, "{results:?}");
    assert!(results[0].0, "an error result: {results:?}");
    assert!(
        results[0].1.contains("a person declined it"),
        "the model reads why: {results:?}"
    );
    // The model's words after the result carry it, not the command's output.
    assert!(
        !done(&fr)
            .result_text
            .unwrap_or_default()
            .contains("\ngated-marker-declined"),
        "the command ran"
    );

    // 2. Approved: the command runs.
    send(&mut ws, &gated("rg2", "gated-marker-approved")).await;
    let fr = collect(&mut ws, "rg2", true, is_request).await;
    let (id, _, _) = gate_request(&fr);
    send(&mut ws, &approval("rg2", &id, "approve", None)).await;
    let fr = collect(&mut ws, "rg2", true, |f| matches!(f, Up::RunDone(_))).await;
    assert_eq!(done(&fr).status, "succeeded", "{}", stderr("rg2"));
    let results = model.tool_results();
    assert!(
        results
            .iter()
            .any(|(is_error, text)| !is_error && text.contains("gated-marker-approved")),
        "{results:?}"
    );

    // 3. A command no gate names never asks.
    send(&mut ws, &gated("rg3", "plain-marker")).await;
    let fr = collect(&mut ws, "rg3", true, |f| matches!(f, Up::RunDone(_))).await;
    assert!(!fr.iter().any(is_request));
    assert!(
        model
            .tool_results()
            .iter()
            .any(|(is_error, text)| !is_error && text.contains("plain-marker"))
    );

    // 4. The server's default patterns and the way commands are spelled on Windows: with an
    // extension, a folder, quotes; PowerShell's and cmd's own words for deleting. Each is
    // stopped as its kind (and declined: none of them runs). On Windows the model calls the
    // PowerShell tool, elsewhere Bash: the hook sees the same line.
    let tool = if cfg!(windows) { "PowerShell" } else { "Bash" };
    let spellings = [
        ("mergeMain", "gh.exe pr merge 5"),
        (
            "mergeMain",
            r#"& "C:\Program Files\GitHub CLI\gh.exe" pr merge 5"#,
        ),
        ("mergeMain", "git.exe push origin main"),
        ("deployProd", "wrangler.cmd deploy --env production"),
        ("prodConfig", "fly.exe secrets set A=1"),
        ("deleteData", r"Remove-Item -Recurse -Force C:\"),
        ("deleteData", r"cmd /c rd /s /q C:\"),
        ("externalPublish", "npm.cmd publish"),
    ];
    for (i, (want, command)) in spellings.iter().enumerate() {
        let run = format!("rw{i}");
        let mut a = assign(
            &run,
            "real",
            &format!("Run the tool.\nTOOL {tool} {command}"),
            None,
        );
        if let Down::RunAssign(r) = &mut a {
            r.gates = Some(default_gates());
        }
        send(&mut ws, &a).await;
        let fr = collect(&mut ws, &run, true, |f| {
            is_request(f) || matches!(f, Up::RunDone(_))
        })
        .await;
        assert!(
            fr.last().is_some_and(is_request),
            "{command} was not stopped: {}",
            stderr(&run)
        );
        let (id, kind, asked) = gate_request(&fr);
        assert_eq!(kind.as_deref(), Some(*want), "{command}");
        assert_eq!(asked.as_deref(), Some(*command));
        send(&mut ws, &approval(&run, &id, "deny", None)).await;
        let fr = collect(&mut ws, &run, true, |f| matches!(f, Up::RunDone(_))).await;
        assert_eq!(done(&fr).status, "succeeded", "{}", stderr(&run));
    }

    // 5. A plain `git push` on main, which no pattern names: the CLI's Bash finds the run's
    // git shim first on PATH (after the user's shell profile, see CLAUDE_ENV_FILE), the shim
    // asks, and declined, git never runs and the model reads why.
    if cfg!(unix) && daemon::health::which("git").is_some() {
        let mut a = assign(
            "rs1",
            "real",
            "Run the tool.\nTOOL Bash git init -q -b main shimrepo && cd shimrepo && git -c user.name=t -c user.email=t@example.com commit -q --allow-empty -m x && git push",
            None,
        );
        if let Down::RunAssign(r) = &mut a {
            r.gates = Some(default_gates());
        }
        send(&mut ws, &a).await;
        let fr = collect(&mut ws, "rs1", true, |f| {
            is_request(f) || matches!(f, Up::RunDone(_))
        })
        .await;
        assert!(
            fr.last().is_some_and(is_request),
            "the plain push was not stopped: {}",
            stderr("rs1")
        );
        let (id, kind, asked) = gate_request(&fr);
        assert_eq!(kind.as_deref(), Some("mergeMain"));
        assert!(
            asked
                .as_deref()
                .unwrap()
                .contains("git push origin HEAD:refs/heads/main"),
            "{asked:?}"
        );
        send(&mut ws, &approval("rs1", &id, "deny", None)).await;
        let fr = collect(&mut ws, "rs1", true, |f| matches!(f, Up::RunDone(_))).await;
        assert_eq!(done(&fr).status, "succeeded", "{}", stderr("rs1"));
        assert!(
            model
                .tool_results()
                .iter()
                .any(|(_, text)| text.contains("a person declined it")),
            "{:?}",
            model.tool_results()
        );
    }

    stop_tx.send(()).unwrap();
    tokio::time::timeout(Duration::from_secs(20), daemon)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
}

/// Managed settings that allow only managed hooks drop the run's hook, and nothing in the
/// run's own settings can bring it back. A gated run then never starts: the CLI does not run
/// the start-up hook, so the daemon gives it no prompt and the run fails with the reason.
///
/// Needs such managed settings on this machine, so it does nothing unless
/// `TC_MANAGED_HOOKS_ONLY` is set: `scripts/test-real-cli.sh --managed-hooks` writes the file
/// around this one test.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "real CLI: scripts/test-real-cli.sh"]
async fn a_cli_whose_hooks_are_managed_only_never_starts_a_gated_run() {
    if std::env::var_os("TC_MANAGED_HOOKS_ONLY").is_none() {
        eprintln!("skipped: TC_MANAGED_HOOKS_ONLY is not set");
        return;
    }
    let claude = real_claude();
    daemon::chat_run::set_exe(PathBuf::from(env!("CARGO_BIN_EXE_daemon")));
    let tmp = tempfile::tempdir().unwrap();
    let mut m = mock().await;
    let model = model::Model::start().await;
    let cfg = write_config_with(
        tmp.path(),
        &m.url,
        &real_runtime(tmp.path(), &claude, &model),
    );
    let (stop_tx, stop_rx) = tokio::sync::oneshot::channel::<()>();
    let daemon = tokio::spawn(daemon::daemon::run(cfg, async {
        let _ = stop_rx.await;
    }));
    let (mut ws, _) = m.conns.recv().await.unwrap();
    assert!(matches!(next(&mut ws).await, Up::Hello { .. }));
    send(&mut ws, &welcome(vec![])).await;

    let tool = if cfg!(windows) { "PowerShell" } else { "Bash" };
    let mut a = assign(
        "mh1",
        "real",
        &format!("Run the tool.\nTOOL {tool} echo gated-marker-managed"),
        None,
    );
    if let Down::RunAssign(r) = &mut a {
        r.gates = Some(vec![RunGate {
            kind: "mergeMain".into(),
            patterns: vec!["gated-marker".into()],
        }]);
    }
    send(&mut ws, &a).await;
    let mut frames = vec![];
    let fr = tokio::time::timeout(Duration::from_secs(120), async {
        loop {
            let f = next(&mut ws).await;
            let end = matches!(&f, Up::RunDone(d) if d.run_id == "mh1");
            if let Some(seq) = f.seq() {
                send(
                    &mut ws,
                    &Down::Ack {
                        run_id: "mh1".into(),
                        seq,
                    },
                )
                .await;
            }
            frames.push(f);
            if end {
                break;
            }
        }
        frames
    })
    .await
    .expect("the run ended");
    let d = done(&fr);
    assert_eq!(d.status, "failed", "{d:?}");
    assert!(
        d.error
            .as_deref()
            .unwrap_or_default()
            .contains("the approval hook did not start"),
        "{d:?}"
    );
    assert!(!fr.iter().any(is_request), "nobody was asked");
    // The model was never given the prompt, so no command ran.
    assert!(
        model.tool_results().is_empty(),
        "{:?}",
        model.tool_results()
    );
    assert!(
        !model
            .requests()
            .iter()
            .any(|r| r["messages"].to_string().contains("gated-marker-managed")),
        "the prompt reached the model"
    );

    // A run without gates still works on this machine.
    send(&mut ws, &assign("mh2", "real", "say ok", None)).await;
    let fr = collect(&mut ws, "mh2", true, |f| matches!(f, Up::RunDone(_))).await;
    assert_eq!(done(&fr).status, "succeeded");

    stop_tx.send(()).unwrap();
    tokio::time::timeout(Duration::from_secs(20), daemon)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
}

/// The same rule with the fake CLI, on every system and without managed settings: a CLI that
/// runs none of the run's hooks (`FAKE_HOOKS_OFF`) is given no prompt of a gated run. The run
/// fails with the reason; the same CLI runs a run without gates.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_cli_that_runs_no_hooks_gets_no_gated_prompt() {
    daemon::chat_run::set_exe(PathBuf::from(env!("CARGO_BIN_EXE_daemon")));
    let tmp = tempfile::tempdir().unwrap();
    let mut m = mock().await;
    // The run waits 2 s for the start-up hook, not 45 s.
    let extra = fake_runtime(
        "nohooks",
        "claude",
        "fake_claude.py",
        &format!(
            "CLAUDE_CONFIG_DIR = '{}', FAKE_HOOKS_OFF = '1'",
            tmp.path().join("claude").display()
        ),
    ) + "\n[test_clocks]\nhooks_start_ms = 2000\n";
    let cfg = write_config_with(tmp.path(), &m.url, &extra);
    let (stop_tx, stop_rx) = tokio::sync::oneshot::channel::<()>();
    let daemon = tokio::spawn(daemon::daemon::run(cfg, async {
        let _ = stop_rx.await;
    }));
    let (mut ws, _) = m.conns.recv().await.unwrap();
    assert!(matches!(next(&mut ws).await, Up::Hello { .. }));
    send(&mut ws, &welcome(vec![])).await;

    let mut a = assign("nh1", "nohooks", "gated", None);
    if let Down::RunAssign(r) = &mut a {
        r.gates = Some(vec![RunGate {
            kind: "mergeMain".into(),
            patterns: vec![r"\bgh pr merge\b".into()],
        }]);
    }
    send(&mut ws, &a).await;
    let mut fr = vec![];
    tokio::time::timeout(Duration::from_secs(120), async {
        loop {
            let f = next(&mut ws).await;
            let end = matches!(&f, Up::RunDone(d) if d.run_id == "nh1");
            if let Some(seq) = f.seq() {
                let run_id = "nh1".to_string();
                send(&mut ws, &Down::Ack { run_id, seq }).await;
            }
            fr.push(f);
            if end {
                break;
            }
        }
    })
    .await
    .expect("the run ended");
    let d = done(&fr);
    assert_eq!(d.status, "failed", "{d:?}");
    assert!(
        d.error
            .as_deref()
            .unwrap_or_default()
            .contains("the approval hook did not start"),
        "{d:?}"
    );
    assert!(!fr.iter().any(is_request));
    // The CLI started (it wrote its arguments) and never got the prompt (no command ran).
    let work = work_of(tmp.path(), "nh1");
    assert!(work.join("argv.json").is_file());
    assert!(!work.join("gate-1.json").exists());

    send(&mut ws, &assign("nh2", "nohooks", "hello", None)).await;
    let fr = collect(&mut ws, "nh2", true, |f| matches!(f, Up::RunDone(_))).await;
    assert_eq!(done(&fr).status, "succeeded");

    stop_tx.send(()).unwrap();
    tokio::time::timeout(Duration::from_secs(20), daemon)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
}

/// A machine that slept: every process stops and the clocks go on. Here the daemon is a
/// process of its own that is frozen for longer than its stall limit (what a closed lid does
/// to it) while a run is in progress. Its clocks are in milliseconds (`test_clocks`): a 3 s
/// stall limit, a 100 ms supervisor tick, a tick 500 ms late is a sleep. When it runs again it connects to the server again at
/// once (the old connection is dead on the server's side), the run is still there and is not
/// reported as stalled (the CLI was asleep too, not silent), and a cancel still ends it.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_machine_that_slept_connects_again_at_once_and_its_run_is_not_stalled() {
    let tmp = tempfile::tempdir().unwrap();
    let mut m = mock().await;
    write_config(tmp.path(), &m.url);
    let config = tmp.path().join("daemon.toml");
    let text = std::fs::read_to_string(&config).unwrap();
    assert!(text.contains("heartbeat_secs = 1\n"));
    std::fs::write(
        &config,
        text.replace(
            "heartbeat_secs = 1\n",
            "heartbeat_secs = 1\ntest_clocks = { stall_ms = 3000, tick_ms = 100, late_ms = 500 }\n",
        ),
    )
    .unwrap();
    let log = tmp.path().join("daemon.err");
    let mut cmd = tokio::process::Command::new(env!("CARGO_BIN_EXE_daemon"));
    cmd.arg("run")
        .arg("--config")
        .arg(&config)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::fs::File::create(&log).unwrap())
        .kill_on_drop(true);
    // Frozen and thawed the way each system stops a process: SIGSTOP, or every thread of the
    // job suspended.
    #[cfg(unix)]
    let mut child = cmd.spawn().unwrap();
    #[cfg(unix)]
    let freeze = |on: bool| {
        let pid = child.id().unwrap() as i32;
        // SAFETY: a signal to the daemon process this test started.
        unsafe { libc::kill(pid, if on { libc::SIGSTOP } else { libc::SIGCONT }) };
    };
    #[cfg(windows)]
    let (mut child, job) = daemon::winjob::spawn(&mut cmd, false).unwrap();
    #[cfg(windows)]
    let freeze = |on: bool| {
        if on {
            job.suspend();
        } else {
            job.resume();
        }
    };
    let daemon_log = || std::fs::read_to_string(&log).unwrap_or_default();

    let (mut ws, _) = tokio::time::timeout(Duration::from_secs(30), m.conns.recv())
        .await
        .unwrap_or_else(|_| panic!("the daemon did not connect: {}", daemon_log()))
        .unwrap();
    assert!(matches!(next(&mut ws).await, Up::Hello { .. }));
    send(&mut ws, &welcome(vec![])).await;
    send(&mut ws, &assign("z1", "fake", "hang", None)).await;
    let before = collect(&mut ws, "z1", true, |f| {
        matches!(
            f,
            Up::RunEvent {
                event: RunEvent::TurnStarted { .. },
                ..
            }
        )
    })
    .await;
    let acked = before.iter().filter_map(Up::seq).max().unwrap_or(0);
    let pid: i32 = wait_file(&work_of(tmp.path(), "z1").join("fake.pid"))
        .await
        .trim()
        .parse()
        .unwrap();

    // Asleep for longer than a silent CLI is given (3 s).
    freeze(true);
    tokio::time::sleep(Duration::from_millis(3500)).await;
    drop(ws);
    let woke = std::time::Instant::now();
    freeze(false);

    // Awake: a new connection, at once, with the run still in it.
    let (mut ws, _) = tokio::time::timeout(Duration::from_secs(15), m.conns.recv())
        .await
        .unwrap_or_else(|_| panic!("no new connection after the sleep: {}", daemon_log()))
        .unwrap();
    println!("  connected again {:?} after waking", woke.elapsed());
    match next(&mut ws).await {
        Up::Hello { runs, .. } => {
            assert!(runs.iter().any(|c| c.run_id == "z1" && !c.done), "{runs:?}");
        }
        f => panic!("{f:?}"),
    }
    assert!(
        daemon_log().contains("the machine was asleep"),
        "{}",
        daemon_log()
    );
    send(
        &mut ws,
        &welcome(vec![KnownRun {
            run_id: "z1".into(),
            acked_seq: acked,
        }]),
    )
    .await;
    // The run is not called stalled: nothing comes for it (the supervisor looks every 100 ms,
    // and looked when it woke). Awake, the CLI has been silent for well under 3 s.
    let stalled = |f: &Up| matches!(f, Up::RunEvent { event: RunEvent::Status { state, .. }, .. } if state == "stalled");
    let quiet = tokio::time::timeout(
        Duration::from_secs(1),
        collect(&mut ws, "z1", true, |f| {
            stalled(f) || matches!(f, Up::RunDone(_))
        }),
    )
    .await;
    assert!(quiet.is_err(), "the run after the sleep: {quiet:?}");
    assert!(alive(pid), "the CLI is still there");

    // The run goes on as before: a cancel ends it.
    send(
        &mut ws,
        &Down::RunCancel {
            run_id: "z1".into(),
            reason: None,
        },
    )
    .await;
    let fr = collect(&mut ws, "z1", true, |f| matches!(f, Up::RunDone(_))).await;
    assert_eq!(done(&fr).status, "cancelled");
    assert!(!fr.iter().any(stalled));
    let _ = child.start_kill();
    let _ = child.wait().await;
}

/// Feature "github-host": hello says so, and the heartbeat carries the computer's GitHub
/// sign-in as `gh` on the daemon's own PATH reports it (a computer without `[vm]` too).
#[cfg(unix)]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_heartbeat_carries_the_computers_github() {
    let tmp = tempfile::tempdir().unwrap();
    let mut m = mock().await;
    write_config(tmp.path(), &m.url);
    let bin = tmp.path().join("hostbin");
    std::fs::create_dir(&bin).unwrap();
    let gh = bin.join("gh");
    std::fs::write(
        &gh,
        "#!/bin/sh\ncase \"$1 $2\" in\n  \"auth status\") exit 0 ;;\n  \"api user\") echo org-bot ;;\n  *) exit 1 ;;\nesac\n",
    )
    .unwrap();
    std::fs::set_permissions(&gh, std::fs::Permissions::from_mode(0o755)).unwrap();
    let path = std::env::join_paths(std::iter::once(bin.clone()).chain(std::env::split_paths(
        &std::env::var_os("PATH").unwrap_or_default(),
    )))
    .unwrap();
    let log = tmp.path().join("daemon.err");
    let mut child = tokio::process::Command::new(env!("CARGO_BIN_EXE_daemon"))
        .arg("run")
        .arg("--config")
        .arg(tmp.path().join("daemon.toml"))
        .env("PATH", &path)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::fs::File::create(&log).unwrap())
        .kill_on_drop(true)
        .spawn()
        .unwrap();
    let (mut ws, _) = tokio::time::timeout(Duration::from_secs(30), m.conns.recv())
        .await
        .unwrap_or_else(|_| panic!("no connection: {}", std::fs::read_to_string(&log).unwrap()))
        .unwrap();
    match next(&mut ws).await {
        Up::Hello { machine, .. } => {
            let f = machine.features.unwrap_or_default();
            assert!(f.iter().any(|f| f == "github-host"), "{f:?}");
            assert_eq!(machine.runs_in.as_deref(), Some("host"));
        }
        f => panic!("{f:?}"),
    }
    send(&mut ws, &welcome(vec![])).await;
    let github = tokio::time::timeout(Duration::from_secs(20), async {
        loop {
            let Some(Ok(Message::Text(t))) = ws.next().await else {
                panic!("closed")
            };
            if let Up::Heartbeat { at, metrics, .. } = serde_json::from_str::<Up>(&t).unwrap() {
                send(&mut ws, &Down::HeartbeatAck { at }).await;
                if let Some(g) = metrics.and_then(|m| m.github) {
                    return g;
                }
            }
        }
    })
    .await
    .expect("a heartbeat with github");
    assert_eq!(github.state, "signed_in");
    assert_eq!(github.login.as_deref(), Some("org-bot"));
    let _ = child.start_kill();
    let _ = child.wait().await;
}
