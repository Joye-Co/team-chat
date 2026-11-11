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
        gates: None,
        github: None,
        run_id: run.into(),
        bot_member_id: "bot1".into(),
        group_id: "g1".into(),
        topic_id: "t1".into(),
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
    let pidfile = tmp.path().join("work/runs/r3/work/fake.pid");
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
    tokio::time::sleep(Duration::from_secs(3)).await;
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
    let pidfile = tmp.path().join("work/runs/r9/work/fake.pid");
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
/// (failed: daemon restarted) and a failed run.session. On a graceful stop it waits for the
/// ack of run.done before it exits.
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
            RunEvent::Status {
                state: "running".into(),
                message: None,
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
    assert!(matches!(frames.last(), Some(Up::RunSession(s)) if !s.ok));
    let meta: serde_json::Value =
        serde_json::from_slice(&std::fs::read(dir.join("run.json")).unwrap()).unwrap();
    assert!(meta["finishedAt"].is_u64());

    // Graceful stop with a running run: run.done (cancelled) goes out, and the daemon exits
    // soon after the server acks it.
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
    assert_eq!(done(&frames).status, "cancelled");
    tokio::time::timeout(Duration::from_secs(8), daemon)
        .await
        .expect("daemon exits once run.done is acked")
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
                        "effort",
                        "gates",
                        "accounts"
                    ]
                    .map(String::from)[..]
                )
            );
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
    let dir = tmp.path().join("work/runs/p1/work");
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
    let pid: i32 = wait_file(&tmp.path().join("work/runs/p2/work/fake.pid"))
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
    let dir = tmp.path().join("work/runs/c1/work");
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
    let report = chat_run_report(&tmp.path().join("work/runs/a1/work"), 1).await;
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

    // 1. chat-run card patch: the output event goes up redacted, the CLI gets the result.
    send(&mut ws, &assign("c1", "fake", "chatrun", None)).await;
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
    let argv: Vec<String> =
        serde_json::from_str(&std::fs::read_to_string(run_dir.join("work/argv.json")).unwrap())
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
    let r = chat_run_report(&run_dir.join("work"), 1).await;
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

    // 2. No run.output.result: chat-run gives up after 15 s with "unconfirmed" (retry is safe).
    send(&mut ws, &assign("c2", "fake", "chatrun", None)).await;
    let fr = collect(&mut ws, "c2", true, |f| matches!(f, Up::RunDone(_))).await;
    assert!(
        events(&fr)
            .iter()
            .any(|e| matches!(e, RunEvent::Output { .. }))
    );
    let r = chat_run_report(&tmp.path().join("work/runs/c2/work"), 1).await;
    assert_eq!(r["code"], 1, "{r}");
    let err: serde_json::Value = serde_json::from_str(r["out"].as_str().unwrap()).unwrap();
    assert_eq!(err["error"]["reason"], "output.unconfirmed");
    assert!(
        err["error"]["message"]
            .as_str()
            .unwrap()
            .contains("may have been applied")
    );

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
    let work = tmp.path().join("work/runs/x1/work");
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

/// `chat-run` while the daemon's link to the server is down: the output never reaches the
/// server, so after 15 s `chat-run` says "not connected", not "unconfirmed".
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn chat_run_without_a_link_is_not_connected() {
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

    send(&mut ws, &assign("n1", "fake", "chatrun", None)).await;
    // The server goes away before it acks the output event.
    collect(&mut ws, "n1", false, |f| {
        matches!(
            f,
            Up::RunEvent {
                event: RunEvent::Output { .. },
                ..
            }
        )
    })
    .await;
    m.refuse.store(true, std::sync::atomic::Ordering::SeqCst);
    drop(ws);
    // chat-run gives up after 15 s; wait_file waits 10 s at a time.
    let report = tmp.path().join("work/runs/n1/work/chatrun-1.json");
    let start = std::time::Instant::now();
    while !report.exists() && start.elapsed() < Duration::from_secs(30) {
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    let r = chat_run_report(&tmp.path().join("work/runs/n1/work"), 1).await;
    assert_eq!(r["code"], 1, "{r}");
    let err: serde_json::Value = serde_json::from_str(r["out"].as_str().unwrap()).unwrap();
    assert_eq!(err["error"]["code"], "unavailable", "{err}");
    assert_eq!(err["error"]["reason"], "server.not_connected", "{err}");
    assert!(
        err["error"]["message"]
            .as_str()
            .unwrap()
            .contains("not connected"),
        "{err}"
    );
    assert!(
        err["error"]["message"]
            .as_str()
            .unwrap()
            .contains("the result is unknown"),
        "{err}"
    );

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
    }
    send(&mut ws, &a).await;
    let fr = collect(&mut ws, "f2", true, |f| matches!(f, Up::RunDone(_))).await;
    assert_eq!(done(&fr).status, "succeeded");
    assert_eq!(done(&fr).session_id.as_deref(), Some(s1.as_str()));
    let argv: Vec<String> = serde_json::from_str(
        &std::fs::read_to_string(tmp.path().join("work/runs/f2/work/argv.json")).unwrap(),
    )
    .unwrap();
    let i = argv.iter().position(|a| a == "--resume").expect("--resume");
    assert_eq!(argv[i + 1], s1);
    assert!(!argv.iter().any(|a| a == "--session-id"));

    // A session the CLI no longer has starts a new one.
    let mut a = assign("f3", "fake", "again", None);
    if let Down::RunAssign(r) = &mut a {
        r.resume_session_id = Some("gone-session".into());
    }
    send(&mut ws, &a).await;
    let fr = collect(&mut ws, "f3", true, |f| matches!(f, Up::RunDone(_))).await;
    assert_eq!(done(&fr).status, "succeeded");
    assert_ne!(done(&fr).session_id.as_deref(), Some("gone-session"));
    let argv: Vec<String> = serde_json::from_str(
        &std::fs::read_to_string(tmp.path().join("work/runs/f3/work/argv.json")).unwrap(),
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
            &std::fs::read_to_string(tmp.path().join(format!("work/runs/{run}/work/argv.json")))
                .unwrap(),
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

/// A tool that daemonized (its parent exited, so it is no longer below the CLI) still stops on
/// pause, continues on resume and is gone after cancel: the daemon finds it by the run's
/// `CHAT_RUN_FOLDER` in its environment.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn pause_and_cancel_cover_orphaned_tools() {
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

    send(&mut ws, &assign("o1", "fake", "orphan", None)).await;
    let dir = tmp.path().join("work/runs/o1/work");
    wait_file(&dir.join("fake.pid")).await;
    let orphan: i32 = wait_file(&dir.join("orphan.txt"))
        .await
        .trim()
        .parse()
        .unwrap();
    let _reap = Reap(vec![orphan]);
    let ticks = dir.join("orphan.ticks");
    wait_file(&ticks).await;

    send(
        &mut ws,
        &Down::RunPause {
            run_id: "o1".into(),
            by: None,
            hold: None,
        },
    )
    .await;
    collect(&mut ws, "o1", true, |f| is_paused(f, true)).await;
    assert!(stopped(orphan), "orphan not stopped");
    let before = file_len(&ticks);
    tokio::time::sleep(Duration::from_millis(500)).await;
    assert_eq!(file_len(&ticks), before, "a stopped orphan made progress");

    send(
        &mut ws,
        &Down::RunResume {
            run_id: "o1".into(),
            note: None,
        },
    )
    .await;
    collect(&mut ws, "o1", true, |f| is_paused(f, false)).await;
    tokio::time::sleep(Duration::from_millis(500)).await;
    assert!(
        file_len(&ticks) > before,
        "orphan did not tick after resume"
    );

    send(
        &mut ws,
        &Down::RunCancel {
            run_id: "o1".into(),
            reason: None,
        },
    )
    .await;
    let fr = collect(&mut ws, "o1", true, |f| matches!(f, Up::RunDone(_))).await;
    assert_eq!(done(&fr).status, "cancelled");
    for _ in 0..100 {
        if !alive(orphan) {
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert!(!alive(orphan), "orphan left after cancel");

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
    let work = |run: &str| tmp.path().join(format!("work/runs/{run}/work"));
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

/// The real `claude` of this machine, or None (skip) when it has none; a failure instead when
/// `TC_REQUIRE_REAL_CLI` is set (CI installs the CLI and sets it).
fn real_claude() -> Option<PathBuf> {
    let found = daemon::health::which("claude");
    if found.is_none() {
        assert!(
            std::env::var_os("TC_REQUIRE_REAL_CLI").is_none(),
            "TC_REQUIRE_REAL_CLI is set and claude is not on PATH"
        );
        eprintln!("skipped: claude is not on PATH");
    }
    found
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
/// command does not run and the model reads why. Approved: it runs. Skipped when `claude` is
/// not on PATH, unless `TC_REQUIRE_REAL_CLI` is set (CI).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_real_cli_waits_for_a_person_at_a_gated_command() {
    let Some(claude) = real_claude() else { return };
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
/// Needs such managed settings on this machine, so it runs only where `TC_MANAGED_HOOKS_ONLY`
/// is set: CI writes the file (`/etc/claude-code/managed-settings.json`, on Windows
/// `C:\Program Files\ClaudeCode\managed-settings.json`) around this one test.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_cli_whose_hooks_are_managed_only_never_starts_a_gated_run() {
    if std::env::var_os("TC_MANAGED_HOOKS_ONLY").is_none() {
        eprintln!("skipped: TC_MANAGED_HOOKS_ONLY is not set");
        return;
    }
    let claude = daemon::health::which("claude").expect("claude on PATH");
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
    let cfg = write_config_with(
        tmp.path(),
        &m.url,
        &fake_runtime(
            "nohooks",
            "claude",
            "fake_claude.py",
            &format!(
                "CLAUDE_CONFIG_DIR = '{}', FAKE_HOOKS_OFF = '1'",
                tmp.path().join("claude").display()
            ),
        ),
    );
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
    let work = tmp.path().join("work/runs/nh1/work");
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
/// process of its own that is frozen for longer than `stall_minutes` (what a closed lid does
/// to it) while a run is in progress. When it runs again it connects to the server again at
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
            "heartbeat_secs = 1\nstall_minutes = 1\n",
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
    let pid: i32 = wait_file(&tmp.path().join("work/runs/z1/work/fake.pid"))
        .await
        .trim()
        .parse()
        .unwrap();

    // Asleep for longer than a silent CLI is given (`stall_minutes = 1`).
    freeze(true);
    tokio::time::sleep(Duration::from_secs(66)).await;
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
    // The run is not called stalled: nothing comes for it (the supervisor looks every 15 s,
    // and looked when it woke).
    let stalled = |f: &Up| matches!(f, Up::RunEvent { event: RunEvent::Status { state, .. }, .. } if state == "stalled");
    let quiet = tokio::time::timeout(
        Duration::from_secs(20),
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
