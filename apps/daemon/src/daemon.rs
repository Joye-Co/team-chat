//! The daemon loop: one task owns all state (link, runs, buffers, timers).

use crate::config::Config;
use crate::health;
use crate::proto::*;
use crate::run::{RunCtl, RunCtx, RunMsg};
use crate::runtime::Control;
use futures_util::{SinkExt, StreamExt};
use std::collections::{HashMap, HashSet, VecDeque};
use std::future::Future;
use std::io::Write;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::mpsc;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;

macro_rules! log {
    ($($a:tt)*) => {{
        eprintln!("{} {}", crate::proto::now_ms(), format!($($a)*));
    }};
}

/// Frames buffered per run before message deltas are dropped (finals are always kept).
const BUFFER_SOFT_CAP: usize = 20_000;

/// `hello.machine.features`: the 1.1 work this daemon does (docs/machine-protocol.md).
pub const FEATURES: &[&str] = &[
    feature::PAUSE,
    feature::ANSWER,
    feature::ASIDE,
    feature::ACCEPTANCE,
    feature::INTERACT,
    feature::OUTPUT,
    feature::RESUME,
    feature::CONTEXT,
    feature::EFFORT,
    feature::GATES,
];

/// `run.resume.note` limit (docs/machine-protocol.md).
const MAX_RESUME_NOTE: usize = 2000;

enum LinkEvent {
    /// The link is up, with the CLIs found for its `hello`.
    Up(u64, mpsc::UnboundedSender<String>, Vec<CliInfo>),
    Frame(u64, String),
    Closed(u64, String),
}

struct RunEntry {
    control: mpsc::UnboundedSender<RunCtl>,
    next_seq: u64,
    /// Sequenced frames the server has not acked yet, in seq order.
    buffer: VecDeque<(u64, bool, String)>,
    done: bool,
    finished: bool,
    orphan: bool,
    log: Option<std::fs::File>,
    /// `chat-run` outputs waiting for `run.output.result`.
    outputs: Arc<crate::chat_run::Outputs>,
}

pub struct Daemon {
    cfg: Arc<Config>,
    token: Arc<String>,
    /// Masks secrets in every frame a run sends (run events, run.done, run.session).
    redactor: Arc<crate::redact::Redactor>,
    /// The CLIs of the last `hello`; a run's effort must be one its CLI reported there.
    clis: Vec<CliInfo>,
    runs: HashMap<String, RunEntry>,
    recent: VecDeque<String>,
    recent_set: HashSet<String>,
    epoch: u64,
    link: Option<mpsc::UnboundedSender<String>>,
    welcomed: bool,
    connecting: bool,
    backoff_ms: u64,
    /// When the last heartbeat tick came, on the clock that counts the time the machine was
    /// asleep and that a change of the date does not move ([`crate::sys::since_boot`]).
    last_beat: Duration,
    next_connect: tokio::time::Instant,
    last_rx: Instant,
    link_tx: mpsc::UnboundedSender<LinkEvent>,
    run_tx: mpsc::UnboundedSender<(String, RunMsg)>,
    /// Where runs execute.
    exec: Arc<dyn crate::executor::Executor>,
    /// Host and VM numbers for the heartbeat.
    sampler: crate::metrics::Sampler,
    /// Feature "accounts": the machine key (None turns the feature off), the API keys, the
    /// sign-ins in progress and their status channel, usage and who each CLI is signed in as.
    machine_key: Option<Arc<crate::accounts::MachineKey>>,
    keys: crate::accounts::SecretStore,
    logins: HashMap<String, mpsc::UnboundedSender<crate::accounts::LoginCtl>>,
    login_tx: mpsc::UnboundedSender<crate::proto::LoginStatus>,
    usage: Arc<std::sync::Mutex<Vec<crate::proto::CliUsage>>>,
    accounts: Arc<std::sync::Mutex<HashMap<String, crate::proto::LoginAccount>>>,
    /// Values masked besides the config's: every key this process has held.
    key_values: Vec<String>,
}

pub async fn run(cfg: Config, shutdown: impl Future<Output = ()>) -> Result<(), String> {
    let token = cfg.read_token()?;
    let _ = std::fs::create_dir_all(crate::workspace::runs_dir(&cfg));
    let (link_tx, mut link_rx) = mpsc::unbounded_channel();
    let (run_tx, mut run_rx) = mpsc::unbounded_channel();
    let keys = crate::accounts::SecretStore::load(&cfg);
    let key_values = keys.values();
    let redactor = Arc::new(crate::redact::Redactor::for_daemon_with(
        &cfg,
        &token,
        &key_values,
    ));
    let machine_key = crate::accounts::MachineKey::load_or_create(&cfg).map(Arc::new);
    let (login_tx, mut login_rx) = mpsc::unbounded_channel();
    let exec = crate::executor::for_config(&cfg);
    let mut d = Daemon {
        cfg: Arc::new(cfg),
        token: Arc::new(token),
        redactor,
        clis: vec![],
        runs: HashMap::new(),
        recent: VecDeque::new(),
        recent_set: HashSet::new(),
        epoch: 0,
        link: None,
        welcomed: false,
        connecting: false,
        backoff_ms: 500,
        last_beat: crate::sys::since_boot(),
        next_connect: tokio::time::Instant::now(),
        last_rx: Instant::now(),
        link_tx,
        run_tx,
        exec,
        sampler: crate::metrics::Sampler::new(),
        machine_key,
        keys,
        logins: HashMap::new(),
        login_tx,
        usage: Arc::new(std::sync::Mutex::new(vec![])),
        accounts: Arc::new(std::sync::Mutex::new(HashMap::new())),
        key_values,
    };
    d.read_accounts();
    d.recover();
    {
        let cfg = d.cfg.clone();
        let active: Vec<String> = d.runs.keys().cloned().collect();
        // No CLI runs yet: every request folder is left from a crash.
        let swept = crate::workspace::sweep_spools(&cfg, &[]);
        if swept > 0 {
            log!("removed {swept} chat-run request folders left from a crash");
        }
        tokio::spawn(async move {
            crate::workspace::cleanup(&cfg, &active).await;
        });
    }
    let hb = Duration::from_secs(d.cfg.heartbeat_secs.max(1));
    let mut heartbeat = tokio::time::interval_at(tokio::time::Instant::now() + hb, hb);
    let mut cleanup = tokio::time::interval_at(
        tokio::time::Instant::now() + Duration::from_secs(1800),
        Duration::from_secs(1800),
    );
    // A new program file (the app updated itself): restart once no run is active.
    let mut exe = crate::selfupdate::Watch::current();
    let mut update_check =
        tokio::time::interval_at(tokio::time::Instant::now() + UPDATE_CHECK, UPDATE_CHECK);
    let mut usage_tick = tokio::time::interval_at(
        tokio::time::Instant::now() + Duration::from_secs(5),
        crate::usage::EVERY,
    );
    tokio::pin!(shutdown);
    log!("daemon started: {} runtimes", d.cfg.runtimes.len());
    loop {
        let reconnect_at = d.next_connect;
        tokio::select! {
            _ = &mut shutdown => break,
            _ = tokio::time::sleep_until(reconnect_at), if d.link.is_none() && !d.connecting => d.connect(),
            Some(ev) = link_rx.recv() => d.on_link(ev),
            Some((id, msg)) = run_rx.recv() => d.on_run(id, msg),
            _ = heartbeat.tick() => d.heartbeat(hb),
            Some(st) = login_rx.recv() => d.on_login_status(st),
            _ = usage_tick.tick(), if d.machine_key.is_some() => d.read_accounts(),
            _ = update_check.tick() => {
                // While runs are active the change stays ready; the next idle check restarts.
                if exe.as_mut().is_some_and(|w| w.check()) && d.active_count() == 0 {
                    log!("a new version of the daemon is installed; restarting");
                    break;
                }
            }
            _ = cleanup.tick() => {
                let cfg = d.cfg.clone();
                let active: Vec<String> = d.runs.keys().cloned().collect();
                crate::workspace::sweep_spools(&cfg, &active);
                tokio::spawn(async move { crate::workspace::cleanup(&cfg, &active).await; });
            }
        }
    }
    // Shutdown: stop every run and give them a moment to report and the server a moment to
    // ack their last frames (run.done), so it shows the runs as stopped, not lost.
    log!("daemon stopping");
    for r in d.runs.values() {
        let _ = r.control.send(Control::Cancel.into());
    }
    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    while d
        .runs
        .values()
        .any(|r| !r.done || (d.welcomed && !r.orphan && !r.buffer.is_empty()))
    {
        tokio::select! {
            Some((id, msg)) = run_rx.recv() => d.on_run(id, msg),
            Some(ev) = link_rx.recv() => d.on_link(ev),
            _ = tokio::time::sleep_until(deadline) => break,
        }
    }
    Ok(())
}

/// How often the daemon looks for a new version of its own program file.
const UPDATE_CHECK: Duration = Duration::from_secs(60);

/// Unfinished runs are recovered at start; finished ones too when they ended this recently
/// (the server may not have acked their last frames).
const RECOVER_FINISHED_MS: u64 = 3_600_000;

impl Daemon {
    /// Rebuilds run state after a restart from `run.json` and `events.jsonl`: stops CLIs left
    /// running, ends unfinished runs with `run.done` (failed: daemon restarted) and a failed
    /// `run.session`, and keeps the frames the server may not have, for replay after welcome.
    fn recover(&mut self) {
        let Ok(rd) = std::fs::read_dir(crate::workspace::runs_dir(&self.cfg)) else {
            return;
        };
        let now = now_ms();
        for e in rd.flatten() {
            let dir = e.path();
            let id = e.file_name().to_string_lossy().to_string();
            if !valid_run_id(&id) {
                continue;
            }
            let Some(mut meta) = crate::workspace::read_meta(&dir) else {
                continue;
            };
            let unfinished = meta.finished_at.is_none();
            if !unfinished
                && meta
                    .finished_at
                    .is_some_and(|f| now.saturating_sub(f) > RECOVER_FINISHED_MS)
            {
                continue;
            }
            if unfinished {
                self.exec.kill_leftover(&meta);
            }
            let log_path = dir.join("events.jsonl");
            let mut buffer: VecDeque<(u64, bool, String)> = VecDeque::new();
            let (mut has_done, mut has_session, mut last) = (false, false, 0u64);
            for line in std::fs::read_to_string(&log_path)
                .unwrap_or_default()
                .lines()
            {
                // A crash can cut the last line: skip what does not parse.
                let Ok(up) = serde_json::from_str::<Up>(line) else {
                    continue;
                };
                let Some(seq) = up.seq() else { continue };
                has_done |= matches!(up, Up::RunDone(_));
                has_session |= matches!(up, Up::RunSession(_));
                let delta = matches!(
                    up,
                    Up::RunEvent {
                        event: RunEvent::MessageDelta { .. },
                        ..
                    }
                );
                last = last.max(seq);
                buffer.push_back((seq, delta, line.to_string()));
            }
            if buffer.len() > BUFFER_SOFT_CAP {
                buffer.retain(|(_, d, _)| !d);
            }
            if !unfinished && buffer.is_empty() {
                continue;
            }
            let mut log = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(&log_path)
                .ok();
            let mut add = |up: Up, buffer: &mut VecDeque<(u64, bool, String)>, seq: u64| {
                if let Ok(text) = serde_json::to_string(&up) {
                    if let Some(f) = log.as_mut() {
                        let _ = writeln!(f, "{text}");
                    }
                    buffer.push_back((seq, false, text));
                }
            };
            if !has_done {
                last += 1;
                add(
                    Up::RunDone(RunDone {
                        run_id: id.clone(),
                        seq: last,
                        at: now,
                        status: "failed".into(),
                        exit_code: None,
                        result_text: None,
                        summary: None,
                        error: Some("daemon restarted".into()),
                        session_id: None,
                        usage: None,
                        duration_ms: now.saturating_sub(meta.started_at),
                    }),
                    &mut buffer,
                    last,
                );
            }
            if !has_session {
                last += 1;
                add(
                    Up::RunSession(RunSession {
                        run_id: id.clone(),
                        seq: last,
                        at: now,
                        ok: false,
                        source: None,
                        bytes: None,
                        gzip_bytes: None,
                        sha256: None,
                        error: Some("daemon restarted".into()),
                    }),
                    &mut buffer,
                    last,
                );
            }
            if unfinished {
                meta.finished_at = Some(now);
                crate::workspace::write_meta(&dir, &meta);
            }
            log!("run {id} recovered: {} frames to replay", buffer.len());
            let (control, _) = mpsc::unbounded_channel();
            self.runs.insert(
                id,
                RunEntry {
                    control,
                    next_seq: last + 1,
                    buffer,
                    done: true,
                    finished: true,
                    orphan: false,
                    log: None,
                    outputs: Arc::default(),
                },
            );
        }
    }

    /// `takeover_url` from the config, only when redaction leaves it unchanged (a URL with a
    /// password or a token never leaves the machine).
    fn takeover_url(&self) -> Option<String> {
        let url = self.cfg.takeover_url.as_deref()?.trim();
        if url.is_empty() || crate::config::takeover_url_problem(url).is_some() {
            return None;
        }
        if self.redactor.text(url) != url {
            log!("takeover_url not reported: it looks like it holds a secret");
            return None;
        }
        Some(url.to_string())
    }

    fn active_count(&self) -> u32 {
        self.runs.values().filter(|r| !r.done).count() as u32
    }

    fn cursors(&self) -> Vec<RunCursor> {
        let mut v: Vec<RunCursor> = self
            .runs
            .iter()
            .filter(|(_, r)| !r.orphan)
            .map(|(id, r)| RunCursor {
                run_id: id.clone(),
                last_seq: r.next_seq.saturating_sub(1),
                done: r.done,
            })
            .collect();
        v.sort_by(|a, b| a.run_id.cmp(&b.run_id));
        v
    }

    fn connect(&mut self) {
        self.connecting = true;
        self.epoch += 1;
        let epoch = self.epoch;
        let url = self.cfg.ws_url();
        let token = self.token.clone();
        let tx = self.link_tx.clone();
        let cfg = self.cfg.clone();
        tokio::spawn(async move {
            // Each hello reports what the CLIs can run now (feature `effort`): asked before the
            // link opens, so the server never waits for the hello.
            let clis = health::discover_clis(&cfg).await;
            let mut req = match url.as_str().into_client_request() {
                Ok(r) => r,
                Err(e) => {
                    let _ = tx.send(LinkEvent::Closed(epoch, format!("bad url: {e}")));
                    return;
                }
            };
            let Ok(auth) = format!("Bearer {token}").parse() else {
                let _ = tx.send(LinkEvent::Closed(epoch, "bad token".into()));
                return;
            };
            req.headers_mut().insert("Authorization", auth);
            let conn = tokio::time::timeout(
                Duration::from_secs(20),
                tokio_tungstenite::connect_async(req),
            )
            .await;
            let ws = match conn {
                Ok(Ok((ws, _))) => ws,
                Ok(Err(e)) => {
                    let _ = tx.send(LinkEvent::Closed(epoch, format!("connect: {e}")));
                    return;
                }
                Err(_) => {
                    let _ = tx.send(LinkEvent::Closed(epoch, "connect timed out".into()));
                    return;
                }
            };
            let (mut sink, mut stream) = ws.split();
            let (out_tx, mut out_rx) = mpsc::unbounded_channel::<String>();
            let _ = tx.send(LinkEvent::Up(epoch, out_tx, clis));
            let writer = tokio::spawn(async move {
                while let Some(s) = out_rx.recv().await {
                    if sink.send(Message::text(s)).await.is_err() {
                        break;
                    }
                }
                let _ = sink.close().await;
            });
            let reason = loop {
                match stream.next().await {
                    Some(Ok(Message::Text(t))) => {
                        let _ = tx.send(LinkEvent::Frame(epoch, t.to_string()));
                    }
                    Some(Ok(Message::Close(_))) | None => break "closed by server".to_string(),
                    Some(Ok(_)) => {}
                    Some(Err(e)) => break format!("read: {e}"),
                }
            };
            writer.abort();
            let _ = tx.send(LinkEvent::Closed(epoch, reason));
        });
    }

    fn drop_link(&mut self, why: &str) {
        if self.link.is_some() || self.connecting {
            log!("link down: {why}");
        }
        self.link = None;
        self.welcomed = false;
        self.connecting = false;
        self.epoch += 1; // ignore late events of the old connection
        let jitter = crate::runtime::uuid_v4().as_bytes()[0] as u64 % (self.backoff_ms / 2 + 1);
        self.next_connect =
            tokio::time::Instant::now() + Duration::from_millis(self.backoff_ms / 2 + jitter);
        self.backoff_ms = (self.backoff_ms * 2).min(30_000);
    }

    fn send_now(&self, up: &Up) {
        if let (Some(l), Ok(s)) = (&self.link, serde_json::to_string(up)) {
            let _ = l.send(s);
        }
    }

    fn on_link(&mut self, ev: LinkEvent) {
        match ev {
            LinkEvent::Up(epoch, tx, clis) if epoch == self.epoch => {
                self.connecting = false;
                self.clis = clis;
                self.link = Some(tx);
                self.last_rx = Instant::now();
                let hello = Up::Hello {
                    protocol: ProtocolV1,
                    machine: MachineInfo {
                        id: self.cfg.machine_id.clone(),
                        name: self.cfg.name.clone(),
                        labels: self.cfg.labels.clone(),
                        daemon_version: env!("CARGO_PKG_VERSION").into(),
                        os: std::env::consts::OS.into(),
                        arch: std::env::consts::ARCH.into(),
                        max_parallel_runs: self.cfg.max_parallel_runs,
                        features: Some(
                            FEATURES
                                .iter()
                                .copied()
                                .chain(self.machine_key.as_ref().map(|_| feature::ACCOUNTS))
                                .map(str::to_string)
                                .collect(),
                        ),
                        takeover_url: self.takeover_url(),
                        public_key: self.machine_key.as_ref().map(|k| k.public_b64()),
                    },
                    clis: self.clis.clone(),
                    health: health::health(&self.cfg, self.active_count()),
                    runs: self.cursors(),
                };
                self.send_now(&hello);
            }
            LinkEvent::Frame(epoch, text) if epoch == self.epoch => {
                self.last_rx = Instant::now();
                match serde_json::from_str::<Down>(&text) {
                    Ok(f) => self.on_frame(f),
                    Err(e) => log!("ignored server frame: {e}"),
                }
            }
            LinkEvent::Closed(epoch, why) if epoch == self.epoch => self.drop_link(&why),
            _ => {}
        }
    }

    fn on_frame(&mut self, f: Down) {
        if !self.welcomed && !matches!(f, Down::Welcome { .. } | Down::Error { .. }) {
            return;
        }
        match f {
            Down::Welcome {
                machine_id, runs, ..
            } => {
                if let Some(id) = &self.cfg.machine_id
                    && id != &machine_id
                {
                    log!("server machine id differs from config machine_id");
                }
                self.welcomed = true;
                self.backoff_ms = 500;
                let known: HashMap<String, u64> =
                    runs.into_iter().map(|r| (r.run_id, r.acked_seq)).collect();
                let ids: Vec<String> = self.runs.keys().cloned().collect();
                for id in ids {
                    match known.get(&id) {
                        Some(acked) => {
                            let r = self.runs.get_mut(&id).expect("run");
                            r.orphan = false;
                            r.outputs.acked(*acked);
                            while r.buffer.front().is_some_and(|(s, _, _)| s <= acked) {
                                r.buffer.pop_front();
                            }
                            if let Some(l) = &self.link {
                                for (_, _, s) in &r.buffer {
                                    let _ = l.send(s.clone());
                                }
                            }
                        }
                        None => {
                            // The server no longer holds this run (lease expired or it was
                            // finished there): stop it and keep nothing.
                            let r = self.runs.get_mut(&id).expect("run");
                            if !r.done {
                                log!("run {id} unknown to server: cancelling");
                                let _ = r.control.send(Control::Cancel.into());
                            }
                            r.orphan = true;
                            r.buffer.clear();
                        }
                    }
                    self.gc(&id);
                }
            }
            Down::HeartbeatAck { .. } => {}
            Down::RunAssign(a) => self.on_assign(a),
            Down::RunInput {
                run_id,
                input_id,
                text,
                answer,
                artifact_event,
                ..
            } => {
                if let Some(r) = self.runs.get(&run_id).filter(|r| !r.done) {
                    let text =
                        crate::input::cli_text(&text, answer.as_ref(), artifact_event.as_ref());
                    let _ = r.control.send(Control::Input { input_id, text }.into());
                }
            }
            Down::RunApproval {
                run_id,
                approval_id,
                decision,
                scope,
            } => {
                if let Some(r) = self.runs.get(&run_id) {
                    let _ = r.control.send(RunCtl::Approval {
                        approval_id,
                        approve: decision == "approve",
                        all: scope.as_deref() == Some(APPROVAL_SCOPE_RUN),
                    });
                }
            }
            Down::RunCancel { run_id, .. } => {
                if let Some(r) = self.runs.get(&run_id) {
                    let _ = r.control.send(Control::Cancel.into());
                }
            }
            Down::RunPause { run_id, by } => {
                if let Some(r) = self.runs.get(&run_id).filter(|r| !r.done) {
                    log!("run {run_id} paused");
                    let _ = r.control.send(RunCtl::Pause { by });
                }
            }
            Down::RunResume { run_id, note } => {
                if let Some(r) = self.runs.get(&run_id).filter(|r| !r.done) {
                    log!("run {run_id} resumed");
                    let note = note.map(|n| n.chars().take(MAX_RESUME_NOTE).collect());
                    let _ = r.control.send(RunCtl::Resume { note });
                }
            }
            Down::Ack { run_id, seq } => {
                if let Some(r) = self.runs.get_mut(&run_id) {
                    r.outputs.acked(seq);
                    while r.buffer.front().is_some_and(|(s, _, _)| *s <= seq) {
                        r.buffer.pop_front();
                    }
                    self.gc(&run_id);
                }
            }
            Down::Error { error } => log!("server error {}: {}", error.code, error.message),
            // 1.1 interaction loop: one `<teamchat-event>` user turn; the runtime answers
            // `input.accepted` as for `run.input`. A run that has ended takes nothing (BotDesk
            // starts a follow-up run).
            Down::RunAnswer(a) => {
                if let Some(r) = self.runs.get(&a.run_id).filter(|r| !r.done) {
                    let text = crate::input::answer_envelope(&a);
                    let input_id = a.input_id;
                    let _ = r.control.send(Control::Input { input_id, text }.into());
                }
            }
            Down::RunInteraction(i) => {
                if let Some(r) = self.runs.get(&i.run_id).filter(|r| !r.done) {
                    let text = crate::input::interaction_envelope(&i);
                    let input_id = i.input_id;
                    let _ = r.control.send(Control::Input { input_id, text }.into());
                }
            }
            Down::AccountLogin {
                login_id,
                runtime,
                op,
                code,
            } => self.on_account_login(login_id, runtime, op, code),
            Down::SecretSet {
                name,
                sealed,
                members,
            } => self.on_secret_set(&name, &sealed, members),
            Down::SecretDelete { name } => {
                if let Err(e) = self.keys.delete(&name) {
                    log!("key {}: {e}", crate::chat_run::safe_word(&name));
                } else {
                    log!("key {} removed", crate::chat_run::safe_word(&name));
                }
            }
            Down::RunOutputResult(res) => {
                let run_id = res.run_id.clone();
                let output_id = res.output_id.clone();
                let taken = self
                    .runs
                    .get(&run_id)
                    .is_some_and(|r| r.outputs.resolve(res));
                if !taken {
                    log!(
                        "run {}: no chat-run waits for output {}",
                        crate::chat_run::safe_word(&run_id),
                        crate::chat_run::safe_word(&output_id)
                    );
                }
            }
        }
    }

    fn reject(&self, run_id: &str, reason: &str, message: impl Into<String>) {
        self.send_now(&Up::RunReject {
            run_id: run_id.into(),
            reason: reason.into(),
            message: message.into(),
        });
    }

    fn on_assign(&mut self, mut a: RunAssign) {
        let id = a.run_id.clone();
        if !valid_run_id(&id) {
            return self.reject(&id, "invalid", "runId must be 1-64 of [A-Za-z0-9_-]");
        }
        if self.runs.contains_key(&id) {
            return self.send_now(&Up::RunAccept { run_id: id });
        }
        if self.recent_set.contains(&id) {
            return self.reject(&id, "invalid", "run already finished on this machine");
        }
        let Some(rt) = self.cfg.runtimes.get(&a.runtime).cloned() else {
            return self.reject(
                &id,
                "runtime_unavailable",
                format!("no runtime {}", a.runtime),
            );
        };
        if health::which(&rt.command()).is_none() {
            return self.reject(
                &id,
                "runtime_unavailable",
                format!("{} is not installed", rt.command()),
            );
        }
        if self.active_count() >= self.cfg.max_parallel_runs {
            return self.reject(&id, "busy", "max_parallel_runs reached");
        }
        let (free, _) = health::disk(&self.cfg.work_root);
        if free < self.cfg.min_free_gb * (1 << 30) / 2 {
            return self.reject(&id, "disk_low", "free disk below half of min_free_gb");
        }
        if let Some(repo) = &a.repo
            && let Err(e) = crate::workspace::validate_repo(&self.cfg, repo)
        {
            return self.reject(&id, "invalid", e);
        }
        if a.prompt.is_empty() {
            return self.reject(&id, "invalid", "empty prompt");
        }
        match a.kind.as_deref() {
            None => {}
            Some(crate::run::KIND_ASIDE) if crate::run::aside_capable(&rt.kind) => {}
            Some(crate::run::KIND_ASIDE) => {
                return self.reject(
                    &id,
                    "runtime_unavailable",
                    format!("runtime {} has no read-only mode for side chats", a.runtime),
                );
            }
            // A kind this daemon does not know could need a mode it cannot enforce.
            Some(_) => return self.reject(&id, "invalid", "unknown run kind"),
        }
        // An effort the CLI did not report is not passed: the run goes on with its default.
        if let Some(e) = a.effort.take() {
            let cli = self.clis.iter().find(|c| c.runtime == a.runtime);
            if crate::models::accepts_effort(cli, &e) {
                a.effort = Some(e);
            } else {
                log!(
                    "run {id}: runtime {} did not report effort {}; it runs with its default",
                    crate::chat_run::safe_word(&a.runtime),
                    crate::chat_run::safe_word(&e)
                );
            }
        }
        // Codex with no model set: its configured default may be one this account cannot use.
        if a.model.is_none() && rt.kind == "codex" && rt.model.is_none() {
            let cli = self.clis.iter().find(|c| c.runtime == a.runtime);
            if let Some(m) = crate::models::codex_fallback_model(
                cli,
                crate::models::codex_config_model(&rt).as_deref(),
            ) {
                log!(
                    "run {id}: Codex's default model is not in this account's list; it runs with {}",
                    crate::chat_run::safe_word(&m)
                );
                a.model = Some(m);
            }
        }
        let (ctl_tx, ctl_rx) = mpsc::unbounded_channel();
        let outputs = Arc::new(crate::chat_run::Outputs::new(&id));
        self.runs.insert(
            id.clone(),
            RunEntry {
                control: ctl_tx,
                next_seq: 1,
                buffer: VecDeque::new(),
                done: false,
                finished: false,
                orphan: false,
                log: None,
                outputs: outputs.clone(),
            },
        );
        log!("run {id} accepted ({})", a.runtime);
        self.send_now(&Up::RunAccept { run_id: id });
        let keys = if a.kind.is_some() {
            vec![]
        } else {
            self.keys.env_for(&a.bot_member_id)
        };
        let gated = self
            .clis
            .iter()
            .any(|c| c.runtime == a.runtime && c.gates == Some(true));
        tokio::spawn(crate::run::supervise(RunCtx {
            gated,
            cfg: self.cfg.clone(),
            token: self.token.clone(),
            assign: a,
            rt,
            control_rx: ctl_rx,
            out: self.run_tx.clone(),
            redactor: self.redactor.clone(),
            outputs,
            exec: self.exec.clone(),
            keys,
        }));
    }

    fn on_run(&mut self, id: String, msg: RunMsg) {
        let cfg = self.cfg.clone();
        let Some(r) = self.runs.get_mut(&id) else {
            return;
        };
        match msg {
            RunMsg::Finished => {
                r.finished = true;
                r.done = true;
            }
            RunMsg::Frame(mut up) => {
                let seq = r.next_seq;
                r.next_seq += 1;
                let is_delta = matches!(
                    up,
                    Up::RunEvent {
                        event: RunEvent::MessageDelta { .. },
                        ..
                    }
                );
                match &mut up {
                    Up::RunEvent {
                        seq: s,
                        event: RunEvent::Output { output_id, .. },
                        ..
                    } => {
                        *s = seq;
                        r.outputs.sent(output_id, seq);
                    }
                    Up::RunEvent { seq: s, .. } => *s = seq,
                    Up::RunDone(d) => {
                        d.seq = seq;
                        r.done = true;
                        log!("run {id} done: {}", d.status);
                    }
                    Up::RunSession(s) => s.seq = seq,
                    _ => {}
                }
                let Some(text) = self.redactor.frame(&up) else {
                    return;
                };
                if r.log.is_none() {
                    r.log = std::fs::OpenOptions::new()
                        .create(true)
                        .append(true)
                        .open(crate::workspace::run_dir(&cfg, &id).join("events.jsonl"))
                        .ok();
                }
                if let Some(f) = r.log.as_mut() {
                    let _ = writeln!(f, "{text}");
                }
                if r.orphan {
                    return;
                }
                if self.welcomed
                    && let Some(l) = &self.link
                {
                    let _ = l.send(text.clone());
                }
                r.buffer.push_back((seq, is_delta, text));
                if r.buffer.len() > BUFFER_SOFT_CAP {
                    r.buffer.retain(|(_, d, _)| !d);
                }
            }
        }
        self.gc(&id);
    }

    /// Forget a run once it has finished and the server has every frame.
    fn gc(&mut self, id: &str) {
        let Some(r) = self.runs.get(id) else { return };
        if r.finished && (r.buffer.is_empty() || r.orphan) {
            self.runs.remove(id);
            self.recent.push_back(id.to_string());
            self.recent_set.insert(id.to_string());
            if self.recent.len() > 1000
                && let Some(old) = self.recent.pop_front()
            {
                self.recent_set.remove(&old);
            }
        }
    }

    fn heartbeat(&mut self, hb: Duration) {
        // The machine was asleep since the last tick: the connection it had is dead on the
        // server's side, whatever the socket still says. Connect again now, not after three
        // silent heartbeats or what is left of a long wait between attempts.
        let now = crate::sys::since_boot();
        let between = now.saturating_sub(self.last_beat);
        self.last_beat = now;
        if let Some(asleep) = crate::sys::slept(between, hb) {
            log!(
                "the machine was asleep for {} s: connecting again",
                asleep.as_secs()
            );
            if self.link.is_some() || self.connecting {
                self.drop_link("the machine was asleep");
            }
            self.backoff_ms = 500;
            self.next_connect = tokio::time::Instant::now();
            return;
        }
        if self.link.is_some() && self.last_rx.elapsed() > hb * 3 {
            self.drop_link("no frames from server");
            return;
        }
        if !self.welcomed {
            return;
        }
        let metrics = self.metrics();
        self.send_now(&Up::Heartbeat {
            at: now_ms(),
            health: health::health(&self.cfg, self.active_count()),
            runs: self.cursors(),
            metrics: Some(metrics),
        });
    }

    /// Feature "accounts": start, finish or cancel a sign-in.
    fn on_account_login(
        &mut self,
        login_id: String,
        runtime: String,
        op: String,
        code: Option<String>,
    ) {
        use crate::accounts::LoginCtl;
        if self.machine_key.is_none() || !crate::proto::valid_run_id(&login_id) {
            return;
        }
        match op.as_str() {
            "start" => {
                let Some(rt) = self.cfg.runtimes.get(&runtime).cloned() else {
                    let _ = self.login_tx.send(crate::proto::LoginStatus {
                        login_id,
                        runtime: runtime.clone(),
                        state: crate::proto::login_phase::FAILED.into(),
                        url: None,
                        user_code: None,
                        account: None,
                        error: Some(format!(
                            "no runtime {}",
                            crate::chat_run::safe_word(&runtime)
                        )),
                    });
                    return;
                };
                // One sign-in per runtime at a time: a new one replaces the old.
                let (tx, rx) = mpsc::unbounded_channel();
                if let Some(old) = self.logins.insert(login_id.clone(), tx) {
                    let _ = old.send(LoginCtl::Cancel);
                }
                log!(
                    "sign-in {} started ({})",
                    login_id,
                    crate::chat_run::safe_word(&runtime)
                );
                tokio::spawn(crate::accounts::run_login(
                    login_id,
                    runtime,
                    rt,
                    rx,
                    self.login_tx.clone(),
                ));
            }
            "code" => {
                if let (Some(tx), Some(code)) = (self.logins.get(&login_id), code) {
                    let _ = tx.send(LoginCtl::Code(code));
                }
            }
            "cancel" => {
                if let Some(tx) = self.logins.remove(&login_id) {
                    let _ = tx.send(LoginCtl::Cancel);
                }
            }
            _ => {}
        }
    }

    /// A sign-in moved on: tell the server; after it ends, read who is signed in and the usage.
    fn on_login_status(&mut self, st: crate::proto::LoginStatus) {
        use crate::proto::login_phase as p;
        let ended = matches!(st.state.as_str(), p::DONE | p::FAILED | p::CANCELLED);
        if ended {
            self.logins.remove(&st.login_id);
            log!("sign-in {} {}", st.login_id, st.state);
        }
        if st.state == p::DONE {
            self.read_accounts();
        }
        // Errors come from the CLI's output: mask them like run frames.
        let st = crate::proto::LoginStatus {
            error: st.error.map(|e| self.redactor.text(&e)),
            ..st
        };
        if self.welcomed {
            self.send_now(&Up::AccountLoginStatus(st));
        }
    }

    /// Feature "accounts": open and keep an API key, then mask its value from now on.
    fn on_secret_set(&mut self, name: &str, sealed: &str, members: Vec<String>) {
        let Some(key) = self.machine_key.clone() else {
            return;
        };
        let safe = crate::chat_run::safe_word(name);
        let value = match key.unseal(sealed) {
            Ok(v) => v,
            Err(e) => return log!("key {safe}: {e}"),
        };
        match self.keys.set(name, value.clone(), members) {
            Ok(()) => {
                log!("key {safe} saved");
                self.key_values.push(value);
                self.redactor = Arc::new(crate::redact::Redactor::for_daemon_with(
                    &self.cfg,
                    &self.token,
                    &self.key_values,
                ));
            }
            Err(e) => log!("key {safe}: {e}"),
        }
    }

    /// Reads who each CLI is signed in as and the subscription usage, in the background.
    fn read_accounts(&self) {
        if self.machine_key.is_none() {
            return;
        }
        let runtimes: Vec<(String, crate::config::RuntimeConfig)> = self
            .cfg
            .runtimes
            .iter()
            .filter(|(_, rt)| rt.kind == "claude" || rt.kind == "codex")
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        let (usage, accounts) = (self.usage.clone(), self.accounts.clone());
        tokio::spawn(async move {
            let mut found = HashMap::new();
            for (name, rt) in &runtimes {
                if let Some(a) = crate::accounts::account_of(rt).await {
                    found.insert(name.clone(), a);
                }
            }
            *accounts.lock().unwrap_or_else(|e| e.into_inner()) = found;
            let u = crate::usage::read_all(runtimes).await;
            *usage.lock().unwrap_or_else(|e| e.into_inner()) = u;
        });
    }

    /// The heartbeat's metrics: VMs from the executor, else the runs this daemon holds.
    fn metrics(&mut self) -> crate::proto::MachineMetrics {
        let vm_slots = self.cfg.vm.as_ref().map(|_| self.cfg.max_parallel_runs);
        let local: Vec<crate::metrics::LocalRun> = if vm_slots.is_some() {
            vec![]
        } else {
            self.runs
                .iter()
                .filter(|(_, r)| !r.done)
                .map(|(id, _)| crate::metrics::LocalRun {
                    run_id: id.clone(),
                    started_at: crate::workspace::read_meta(&crate::workspace::run_dir(
                        &self.cfg, id,
                    ))
                    .map(|m| m.started_at)
                    .unwrap_or(0),
                })
                .collect()
        };
        let mut m = self.sampler.sample(&self.cfg, vm_slots, &local);
        if self.machine_key.is_some() {
            m.secrets = Some(self.keys.infos());
            m.usage = Some(self.usage.lock().unwrap_or_else(|e| e.into_inner()).clone());
            let accounts = self.accounts.lock().unwrap_or_else(|e| e.into_inner());
            for l in m.logins.iter_mut().flatten() {
                if let Some(a) = accounts.get(&l.runtime) {
                    l.email = a.email.clone();
                    l.plan = a.plan.clone();
                    if a.plan.as_deref() == Some("api_key") {
                        l.state = crate::proto::login_state::LOGGED_IN.into();
                        l.method = Some("api_key".into());
                    } else if l.state != crate::proto::login_state::LOGGED_IN {
                        // The CLI itself says it is signed in (macOS keeps Claude's in the Keychain).
                        l.state = crate::proto::login_state::LOGGED_IN.into();
                        l.method = Some("subscription".into());
                    }
                }
            }
        }
        m
    }
}
