//! One run from accept to session upload.

use crate::config::{Config, RuntimeConfig};
use crate::proto::{RunAssign, RunDone, RunEvent, RunSession, Up, now_ms, truncate};
use crate::runtime::{self, Control, Outcome, RunSpec, Status};
use crate::workspace;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::mpsc;

/// What a run sends to the daemon loop. Frames get their `seq` there.
#[derive(Debug)]
pub enum RunMsg {
    Frame(Up),
    Finished,
}

pub type RunOut = mpsc::UnboundedSender<(String, RunMsg)>;

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
    for k in &a.env_allowlist {
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
    pub control_tx: mpsc::UnboundedSender<Control>,
    pub control_rx: mpsc::UnboundedReceiver<Control>,
    pub out: RunOut,
}

pub async fn supervise(ctx: RunCtx) {
    let RunCtx {
        cfg,
        token,
        assign,
        rt,
        control_tx,
        control_rx,
        out,
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
            let spec = RunSpec {
                run_id: run_id.clone(),
                runtime_name: assign.runtime.clone(),
                cfg: rt.clone(),
                prompt: assign.prompt.clone(),
                model: assign.model.clone(),
                cwd,
                run_dir: dir.clone(),
                env: build_env(&cfg, &rt, &assign),
            };
            run_runtime(&cfg, &assign, spec, control_tx, control_rx, &event).await
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
            Some(f) => match crate::upload::upload(f, target, &cfg.server_url, &token).await {
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
            },
        };
        send(Up::RunSession(frame));
    }
    let _ = out.send((run_id.clone(), RunMsg::Finished));
}

async fn run_runtime(
    cfg: &Config,
    assign: &RunAssign,
    spec: RunSpec,
    control_tx: mpsc::UnboundedSender<Control>,
    control_rx: mpsc::UnboundedReceiver<Control>,
    event: &impl Fn(RunEvent),
) -> (Outcome, bool) {
    let Some(rt) = runtime::for_kind(&spec.cfg.kind) else {
        return (
            Outcome::failed(format!("unknown runtime kind {}", spec.cfg.kind)),
            false,
        );
    };
    let (emit_tx, mut emit_rx) = mpsc::unbounded_channel();
    let fut = rt.start(spec, emit_tx, control_rx);
    tokio::pin!(fut);
    let stall = Duration::from_secs(cfg.stall_minutes.max(1) * 60);
    let mut last = Instant::now();
    let mut stalled = false;
    let mut timed_out = false;
    let deadline = assign
        .max_duration_ms
        .map(|ms| tokio::time::Instant::now() + Duration::from_millis(ms));
    let mut tick = tokio::time::interval(Duration::from_secs(15));
    let outcome = loop {
        tokio::select! {
            o = &mut fut => break o,
            Some(ev) = emit_rx.recv() => {
                last = Instant::now();
                stalled = false;
                event(ev);
            }
            _ = tick.tick() => {
                if !stalled && last.elapsed() >= stall {
                    stalled = true;
                    event(RunEvent::Status { state: "stalled".into(),
                        message: Some(format!("no output for {} minutes", stall.as_secs() / 60)) });
                }
                if let Some(d) = deadline
                    && !timed_out && tokio::time::Instant::now() >= d {
                        timed_out = true;
                        let _ = control_tx.send(Control::Cancel);
                    }
            }
        }
    };
    while let Ok(ev) = emit_rx.try_recv() {
        event(ev);
    }
    let mut outcome = outcome;
    if timed_out && outcome.status == Status::Cancelled {
        outcome.error = Some("run took longer than maxDurationMs".into());
    }
    (outcome, timed_out)
}
