//! The whole daemon against an in-process mock server and a fake Claude CLI:
//! hello, assign, events, follow-up input, reject, cancel, reconnect with replay, session upload.

use daemon::config::Config;
use daemon::proto::*;
use futures_util::{SinkExt, StreamExt};
use std::collections::BTreeMap;
use std::io::Read;
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

const TOKEN: &str = "tm_test_token_value";

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
            if &head == b"PUT" {
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
    let _ = s
        .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
        .await;
}

async fn send(ws: &mut Ws, d: &Down) {
    ws.send(Message::text(serde_json::to_string(d).unwrap()))
        .await
        .unwrap();
}

/// Next frame that is not a heartbeat.
async fn next(ws: &mut Ws) -> Up {
    loop {
        let m = tokio::time::timeout(Duration::from_secs(20), ws.next())
            .await
            .expect("frame in time")
            .expect("open")
            .expect("ok");
        if let Message::Text(t) = m {
            let up: Up = serde_json::from_str(&t).unwrap_or_else(|e| panic!("{e}: {t}"));
            if !matches!(up, Up::Heartbeat { .. }) {
                return up;
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
        run_id: run.into(),
        bot_member_id: "bot1".into(),
        group_id: "g1".into(),
        topic_id: "t1".into(),
        runtime: runtime.into(),
        prompt: prompt.into(),
        model: None,
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
    let token = dir.join("token");
    std::fs::write(&token, format!("{TOKEN}\n")).unwrap();
    std::fs::set_permissions(&token, std::fs::Permissions::from_mode(0o600)).unwrap();
    let fake = format!("{}/tests/fake/fake_claude.py", env!("CARGO_MANIFEST_DIR"));
    let text = format!(
        r#"
server_url = "{url}"
token_file = "{token}"
name = "test-mac"
labels = ["test"]
work_root = "{work}"
max_parallel_runs = 3
min_free_gb = 0
heartbeat_secs = 1

[runtimes.fake]
kind = "claude"
command = "{fake}"
env = {{ CLAUDE_CONFIG_DIR = "{claude}" }}
"#,
        token = token.display(),
        work = dir.join("work").display(),
        claude = dir.join("claude").display(),
    );
    let cfg = Config::parse(&text).unwrap();
    assert!(cfg.problems().is_empty(), "{:?}", cfg.problems());
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
    for _ in 0..200 {
        if pidfile.exists() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    let pid: i32 = std::fs::read_to_string(&pidfile)
        .unwrap()
        .trim()
        .parse()
        .unwrap();
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
    // SAFETY: probing a pid with signal 0.
    assert_ne!(unsafe { libc::kill(pid, 0) }, 0, "fake CLI still alive");

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
    for _ in 0..200 {
        if pidfile.exists() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    let pid: i32 = std::fs::read_to_string(&pidfile)
        .unwrap()
        .trim()
        .parse()
        .unwrap();
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
        // SAFETY: probing a pid with signal 0.
        if unsafe { libc::kill(pid, 0) } != 0 {
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
    use std::os::unix::process::CommandExt;
    let tmp = tempfile::tempdir().unwrap();
    let mut m = mock().await;
    let cfg = write_config(tmp.path(), &m.url);

    // What the crashed daemon left: a CLI in its own process group, run.json, two frames.
    let mut left = std::process::Command::new("/bin/sleep")
        .arg("600")
        .process_group(0)
        .spawn()
        .unwrap();
    let pid = left.id() as i32;
    let dir = tmp.path().join("work/runs/rc1");
    std::fs::create_dir_all(dir.join("work")).unwrap();
    std::fs::write(
        dir.join("run.json"),
        serde_json::json!({ "runId": "rc1", "startedAt": now_ms() - 5000, "pgid": pid, "program": "/bin/sleep" })
            .to_string(),
    )
    .unwrap();
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
    let mut stopped = false;
    for _ in 0..100 {
        if left.try_wait().unwrap().is_some() {
            stopped = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert!(stopped, "leftover CLI still runs");

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
