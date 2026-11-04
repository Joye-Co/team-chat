//! One run from accept to session upload.

use crate::config::{Config, RuntimeConfig};
use crate::proto::{RunAssign, RunDone, RunEvent, RunSession, Up, now_ms, truncate};
use crate::redact::DeltaGate;
use crate::runtime::{self, Control, Outcome, RunSpec, Status};
use crate::{proctree, workspace};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::mpsc;

/// What a run sends to the daemon loop. Frames get their `seq` there.
#[derive(Debug)]
#[allow(clippy::large_enum_variant)] // one message per frame on a channel; boxing gains nothing
pub enum RunMsg {
    Frame(Up),
    Finished,
}

pub type RunOut = mpsc::UnboundedSender<(String, RunMsg)>;

/// What the daemon loop sends to a run. Pause and resume are handled by the supervisor (they
/// act on the CLI's process group); everything else goes to the runtime.
#[derive(Debug, Clone, PartialEq)]
pub enum RunCtl {
    Runtime(Control),
    /// `run.pause`: SIGSTOP the CLI's process group and the tool groups below it, and stop the
    /// stall and deadline clocks.
    Pause {
        by: Option<String>,
    },
    /// `run.resume`: SIGCONT, move the deadline by the paused time, give `note` to the CLI.
    Resume {
        note: Option<String>,
    },
}

impl From<Control> for RunCtl {
    fn from(c: Control) -> RunCtl {
        RunCtl::Runtime(c)
    }
}

/// `run.assign.kind` of a read-only side chat run.
pub const KIND_ASIDE: &str = "aside";

/// Runtime kinds that have a read-only mode the daemon can force (Codex sandbox `read-only`,
/// Claude Code permission mode `plan` with only the read tools). ACP agents and the PTY have
/// none.
pub fn aside_capable(kind: &str) -> bool {
    matches!(kind, "codex" | "claude")
}

const BASE_ENV: &[&str] = &[
    "PATH",
    "HOME",
    "USER",
    "LOGNAME",
    "SHELL",
    "TMPDIR",
    "LANG",
    "LC_ALL",
    "LC_CTYPE",
    "TERM",
    "SSH_AUTH_SOCK",
    "CODEX_HOME",
    "CLAUDE_CONFIG_DIR",
];

pub fn build_env(cfg: &Config, rt: &RuntimeConfig, a: &RunAssign) -> Vec<(String, String)> {
    let mut env: Vec<(String, String)> = vec![];
    let mut put = |k: &str, v: String| {
        env.retain(|(n, _)| n != k);
        env.push((k.to_string(), v));
    };
    for k in BASE_ENV {
        if let Ok(v) = std::env::var(k) {
            put(k, v);
        }
    }
    // A read-only side chat run needs no secrets: it gets none of the passthrough variables.
    let aside = a.kind.as_deref() == Some(KIND_ASIDE);
    for k in a.env_allowlist.iter().filter(|_| !aside) {
        if cfg.env_passthrough.contains(k)
            && let Ok(v) = std::env::var(k)
        {
            put(k, v);
        }
    }
    for (k, v) in &rt.env {
        put(k, v.clone());
    }
    put("CHAT_RUN_ID", a.run_id.clone());
    put("CHAT_GROUP_ID", a.group_id.clone());
    put("CHAT_TOPIC_ID", a.topic_id.clone());
    put("CHAT_BOT_MEMBER_ID", a.bot_member_id.clone());
    env
}

fn summary(text: &str) -> String {
    let first = text
        .lines()
        .map(str::trim)
        .find(|l| !l.is_empty())
        .unwrap_or_default();
    let mut s: String = first.chars().take(280).collect();
    if first.chars().count() > 280 {
        s.push('…');
    }
    s
}

pub struct RunCtx {
    pub cfg: Arc<Config>,
    pub token: Arc<String>,
    pub assign: RunAssign,
    pub rt: RuntimeConfig,
    pub control_rx: mpsc::UnboundedReceiver<RunCtl>,
    pub out: RunOut,
    pub redactor: Arc<crate::redact::Redactor>,
    /// `chat-run` outputs waiting for `run.output.result` (shared with the daemon loop).
    pub outputs: Arc<crate::chat_run::Outputs>,
}

/// The `chat-run` transport of one run. Dropping it removes the socket or the folder.
#[allow(dead_code)] // held only for its Drop
enum ChatRunHost {
    Socket(crate::chat_run::Server),
    Files(crate::chat_run::Spool),
}

/// `chat-run` for one run: the link on PATH and the transport. A Codex run uses the request
/// folder in its working folder (`cwd`): Codex's sandbox blocks unix socket connects. Other
/// runtimes use the socket. Side chat runs get none.
fn start_chat_run(
    run_dir: &std::path::Path,
    cwd: &std::path::Path,
    files: bool,
    run_id: &str,
    out: &RunOut,
    outputs: Arc<crate::chat_run::Outputs>,
    env: &mut Vec<(String, String)>,
) -> Option<ChatRunHost> {
    let bin = match crate::chat_run::install_link(run_dir) {
        Ok(b) => b,
        Err(e) => {
            eprintln!("{} run {run_id}: no chat-run link: {e}", now_ms());
            return None;
        }
    };
    let out = out.clone();
    let id = run_id.to_string();
    let emit = move |ev: RunEvent| {
        let _ = out.send((
            id.clone(),
            RunMsg::Frame(Up::RunEvent {
                run_id: id.clone(),
                seq: 0,
                at: now_ms(),
                event: ev,
            }),
        ));
    };
    let started = if files {
        crate::chat_run::Spool::start(
            crate::chat_run::spool_dir(cwd, run_id),
            outputs,
            emit,
            crate::chat_run::TIMEOUT,
        )
        .map(|s| {
            let v = (crate::chat_run::ENV_DIR, s.path().display().to_string());
            (ChatRunHost::Files(s), v)
        })
    } else {
        crate::chat_run::Server::start(
            crate::chat_run::socket_path(run_dir),
            outputs,
            emit,
            crate::chat_run::TIMEOUT,
        )
        .map(|s| {
            let v = (crate::chat_run::ENV_SOCKET, s.path().display().to_string());
            (ChatRunHost::Socket(s), v)
        })
    };
    let (host, (var, value)) = match started {
        Ok(h) => h,
        Err(e) => {
            eprintln!("{} run {run_id}: no chat-run transport: {e}", now_ms());
            return None;
        }
    };
    let path = match env.iter().find(|(k, _)| k == "PATH") {
        Some((_, p)) if !p.is_empty() => format!("{}:{p}", bin.display()),
        _ => bin.display().to_string(),
    };
    env.retain(|(k, _)| {
        k != "PATH" && k != crate::chat_run::ENV_SOCKET && k != crate::chat_run::ENV_DIR
    });
    env.push(("PATH".into(), path));
    env.push((var.into(), value));
    Some(host)
}

pub async fn supervise(ctx: RunCtx) {
    let RunCtx {
        cfg,
        token,
        assign,
        rt,
        control_rx,
        out,
        redactor,
        outputs,
    } = ctx;
    let run_id = assign.run_id.clone();
    let started = Instant::now();
    let send = |up: Up| {
        let _ = out.send((run_id.clone(), RunMsg::Frame(up)));
    };
    let event = |ev: RunEvent| {
        let _ = out.send((
            run_id.clone(),
            RunMsg::Frame(Up::RunEvent {
                run_id: run_id.clone(),
                seq: 0,
                at: now_ms(),
                event: ev,
            }),
        ));
    };
    event(RunEvent::Status {
        state: "preparing".into(),
        message: None,
    });

    let dir = workspace::run_dir(&cfg, &run_id);
    let (outcome, timed_out) = match workspace::prepare(&cfg, &run_id, assign.repo.as_ref()).await {
        Err(e) => (Outcome::failed(e), false),
        Ok((cwd, _meta)) => {
            event(RunEvent::Status {
                state: "running".into(),
                message: None,
            });
            let aside = assign.kind.as_deref() == Some(KIND_ASIDE);
            // A side chat run reads only: its checkout is read-only, not just its CLI's modes.
            if aside {
                workspace::set_read_only(&cwd, true);
            }
            let mut env = build_env(&cfg, &rt, &assign);
            // Marks the run's processes, also tools that leave the CLI's tree (proctree).
            env.push((proctree::ENV_FOLDER.into(), dir.display().to_string()));
            // Lives until the CLI is done; dropping it removes the socket.
            let chat_run = if aside {
                None
            } else {
                let files = rt.kind == "codex";
                // Files and 产出 go up over HTTP as the run's member, from its working folder.
                outputs.set_local(crate::chat_run_files::Local {
                    server_url: cfg.server_url.clone(),
                    token: token.clone(),
                    run_id: run_id.clone(),
                    root: cwd.clone(),
                    redactor: redactor.clone(),
                });
                start_chat_run(&dir, &cwd, files, &run_id, &out, outputs, &mut env)
            };
            let spec = RunSpec {
                run_id: run_id.clone(),
                runtime_name: assign.runtime.clone(),
                cfg: rt.clone(),
                prompt: assign.prompt.clone(),
                model: assign.model.clone(),
                cwd,
                run_dir: dir.clone(),
                env,
                aside,
                note: chat_run.as_ref().map(|_| crate::chat_run::note()),
                resume_session_id: assign
                    .resume_session_id
                    .clone()
                    .filter(|s| runtime::valid_session_id(s)),
            };
            let work = spec.cwd.clone();
            let res = run_runtime(&cfg, &assign, spec, control_rx, &event).await;
            drop(chat_run);
            if aside {
                workspace::set_read_only(&work, false);
            }
            res
        }
    };

    // Keep a copy of the CLI's own session file next to the run.
    let mut upload_file = None;
    let mut source = "stream";
    if let Some(f) = outcome.session_file.as_ref().filter(|f| f.is_file()) {
        let copy = dir.join("session.jsonl");
        if f.starts_with(&dir) || std::fs::copy(f, &copy).is_ok() {
            upload_file = Some(if f.starts_with(&dir) { f.clone() } else { copy });
            source = if f.starts_with(&dir) { "stream" } else { "cli" };
        }
    }
    if upload_file.is_none() && dir.join("stream.jsonl").is_file() {
        upload_file = Some(dir.join("stream.jsonl"));
    }

    let status = if timed_out {
        "timeout"
    } else {
        outcome.status.as_str()
    };
    send(Up::RunDone(RunDone {
        run_id: run_id.clone(),
        seq: 0,
        at: now_ms(),
        status: status.into(),
        exit_code: outcome.exit_code,
        summary: outcome.result_text.as_deref().map(summary),
        result_text: outcome
            .result_text
            .as_deref()
            .map(|t| truncate(t, 64 * 1024)),
        error: outcome.error.as_deref().map(|e| truncate(e, 4096)),
        session_id: outcome.session_id.clone(),
        usage: outcome.usage.clone(),
        duration_ms: started.elapsed().as_millis() as u64,
    }));
    if let Some(mut meta) = workspace::read_meta(&dir) {
        meta.finished_at = Some(now_ms());
        workspace::write_meta(&dir, &meta);
    }

    if let Some(target) = &assign.session_upload {
        let frame = match &upload_file {
            None => RunSession {
                run_id: run_id.clone(),
                seq: 0,
                at: now_ms(),
                ok: false,
                source: None,
                bytes: None,
                gzip_bytes: None,
                sha256: None,
                error: Some("no session file".into()),
            },
            Some(f) => {
                match crate::upload::upload(f, target, &cfg.server_url, &token, &redactor).await {
                    Ok(u) => RunSession {
                        run_id: run_id.clone(),
                        seq: 0,
                        at: now_ms(),
                        ok: true,
                        source: Some(source.into()),
                        bytes: Some(u.bytes),
                        gzip_bytes: Some(u.gzip_bytes),
                        sha256: Some(u.sha256),
                        error: None,
                    },
                    Err(e) => RunSession {
                        run_id: run_id.clone(),
                        seq: 0,
                        at: now_ms(),
                        ok: false,
                        source: Some(source.into()),
                        bytes: None,
                        gzip_bytes: None,
                        sha256: None,
                        error: Some(e),
                    },
                }
            }
        };
        send(Up::RunSession(frame));
    }
    let _ = out.send((run_id.clone(), RunMsg::Finished));
}

/// Pause state of a run (接管).
struct Paused {
    since: Instant,
    /// The CLI's process group that got SIGSTOP (None while the CLI has not started yet).
    stopped: Option<i32>,
    /// The tool groups below the CLI that got SIGSTOP.
    tools: Vec<proctree::Target>,
}

impl Paused {
    /// SIGCONT the CLI and its tools.
    fn resume(&self) {
        if let Some(pgid) = self.stopped {
            proctree::resume(pgid, &self.tools);
        }
    }
}

/// The CLI's process group, from `run.json` (written when the runtime spawned the CLI).
fn run_pgid(run_dir: &std::path::Path) -> Option<i32> {
    workspace::read_meta(run_dir)?.pgid.filter(|p| *p > 1)
}

async fn run_runtime(
    cfg: &Config,
    assign: &RunAssign,
    spec: RunSpec,
    mut control_rx: mpsc::UnboundedReceiver<RunCtl>,
    event: &impl Fn(RunEvent),
) -> (Outcome, bool) {
    let Some(rt) = runtime::for_kind(&spec.cfg.kind) else {
        return (
            Outcome::failed(format!("unknown runtime kind {}", spec.cfg.kind)),
            false,
        );
    };
    let run_dir = spec.run_dir.clone();
    let (emit_tx, mut emit_rx) = mpsc::unbounded_channel();
    let (rt_tx, rt_rx) = mpsc::unbounded_channel::<Control>();
    let fut = rt.start(spec, emit_tx, rt_rx);
    tokio::pin!(fut);
    let stall = Duration::from_secs(cfg.stall_minutes.max(1) * 60);
    let mut last = Instant::now();
    let mut stalled = false;
    let mut timed_out = false;
    let mut deadline = assign
        .max_duration_ms
        .map(|ms| tokio::time::Instant::now() + Duration::from_millis(ms));
    let mut tick = tokio::time::interval(Duration::from_secs(15));
    let mut paused: Option<Paused> = None;
    let mut resumes = 0u32;
    let mut control_open = true;
    let mut acceptance = crate::acceptance::AcceptanceTracker::default();
    // Streamed text is released per line, so redaction sees a secret whole.
    let mut gate = DeltaGate::default();
    let outcome = loop {
        // A pause that came before the CLI started stops it as soon as its group is known.
        if let Some(p) = paused.as_mut().filter(|p| p.stopped.is_none())
            && let Some(pgid) = run_pgid(&run_dir)
        {
            proctree::stop(pgid, &run_dir, &mut p.tools);
            p.stopped = Some(pgid);
        }
        let hold_until = gate
            .deadline()
            .map(tokio::time::Instant::from_std)
            .unwrap_or_else(|| tokio::time::Instant::now() + Duration::from_secs(3600));
        tokio::select! {
            o = &mut fut => break o,
            Some(ev) = emit_rx.recv() => {
                last = Instant::now();
                stalled = false;
                let extra = acceptance.on_event(&ev);
                for ev in gate.push(ev).into_iter().chain(extra) {
                    event(ev);
                }
            }
            c = control_rx.recv(), if control_open => {
                let Some(c) = c else { control_open = false; continue };
                match c {
                    RunCtl::Pause { by } => {
                        if paused.is_some() {
                            continue;
                        }
                        let pgid = run_pgid(&run_dir);
                        let mut tools = vec![];
                        if let Some(pgid) = pgid {
                            proctree::stop(pgid, &run_dir, &mut tools);
                        }
                        paused = Some(Paused { since: Instant::now(), stopped: pgid, tools });
                        for ev in gate.push(RunEvent::Paused { paused: true, by }) {
                            event(ev);
                        }
                    }
                    RunCtl::Resume { note } => {
                        let Some(p) = paused.take() else { continue };
                        p.resume();
                        // The paused time does not count toward maxDurationMs or the stall.
                        if let Some(d) = deadline.as_mut() {
                            *d += p.since.elapsed();
                        }
                        last = Instant::now();
                        stalled = false;
                        for ev in gate.push(RunEvent::Paused { paused: false, by: None }) {
                            event(ev);
                        }
                        if let Some(text) = note.filter(|n| !n.trim().is_empty()) {
                            resumes += 1;
                            let _ = rt_tx.send(Control::Input {
                                input_id: format!("resume-{resumes}"),
                                text,
                            });
                        }
                    }
                    RunCtl::Runtime(c) => {
                        // A stopped CLI cannot handle SIGTERM: a cancel resumes it first.
                        if c == Control::Cancel && let Some(p) = paused.take() {
                            p.resume();
                            for ev in gate.push(RunEvent::Paused { paused: false, by: None }) {
                                event(ev);
                            }
                        }
                        let _ = rt_tx.send(c);
                    }
                }
            }
            _ = tokio::time::sleep_until(hold_until), if gate.holding() => {
                for ev in gate.tick(std::time::Instant::now()) {
                    event(ev);
                }
            }
            _ = tick.tick() => {
                if paused.is_some() {
                    continue;
                }
                if !stalled && last.elapsed() >= stall {
                    stalled = true;
                    event(RunEvent::Status { state: "stalled".into(),
                        message: Some(format!("no output for {} minutes", stall.as_secs() / 60)) });
                }
                if let Some(d) = deadline
                    && !timed_out && tokio::time::Instant::now() >= d {
                        timed_out = true;
                        let _ = rt_tx.send(Control::Cancel);
                    }
            }
        }
    };
    // Never leave a stopped group behind (the runtime already reaped the CLI).
    if let Some(p) = paused.take() {
        p.resume();
    }
    while let Ok(ev) = emit_rx.try_recv() {
        let extra = acceptance.on_event(&ev);
        for ev in gate.push(ev).into_iter().chain(extra) {
            event(ev);
        }
    }
    for ev in gate.flush() {
        event(ev);
    }
    let mut outcome = outcome;
    if timed_out && outcome.status == Status::Cancelled {
        outcome.error = Some("run took longer than maxDurationMs".into());
    }
    (outcome, timed_out)
}
