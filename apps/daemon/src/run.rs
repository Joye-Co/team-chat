//! One run from accept to session upload.

use crate::config::{Config, RuntimeConfig};
use crate::executor::{Executor, Stopped};
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
        /// "takeover": a person takes over (or upgrades a pause): their commands may run.
        hold: Option<String>,
    },
    /// `run.resume`: SIGCONT, move the deadline by the paused time, give `note` to the CLI
    /// (after a take-over: the hand-back note first).
    Resume {
        note: Option<String>,
    },
    /// `run.command`: a command of the person holding the run.
    Command {
        id: String,
        command: String,
        by: String,
    },
    /// `run.command.cancel`.
    CommandCancel {
        id: String,
    },
    /// `run.approval`: the daemon's own approvals (gated secrets and actions) are answered
    /// here, the CLI's go to the runtime. `all` = scope "run" (这件事里都同意).
    Approval {
        approval_id: String,
        approve: bool,
        all: bool,
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

/// What a CLI must know about a run in a microVM: what is kept and what goes with the VM.
/// `cache` is the repository's cache folder; `toolchain` the Rust toolchain the daemon could
/// not install for it.
pub fn vm_note(cache: Option<&std::path::Path>, toolchain: Option<&str>) -> String {
    let cache = match cache {
        Some(c) => format!(
            " This repository's dependency caches and user-level tools (cargo, rustup \
toolchains, `cargo install`, npm and `npm -g`, pnpm, bun, pip, pipx and uv tools, go) are in {} \
and already on PATH; they are kept and shared with the other runs on this repository.",
            c.display()
        ),
        None => String::new(),
    };
    let toolchain = match toolchain {
        Some(t) => format!(
            " The Rust toolchain {t} could not be installed before this run: install it with \
`rustup toolchain install {t}` before you build."
        ),
        None => String::new(),
    };
    format!(
        "You run in a virtual machine that was started for this run and is deleted when you end \
your turn. Your working folder and its build outputs are kept for your next run in this \
topic.{cache} Everything else in $HOME, /tmp and the system folders is gone after this run.\
{toolchain} When a system package (apt) or docker is missing, do not work around it: say in \
your answer which one is missing and that an administrator must add it to the base image. \
Never sign in to anything in this machine. Every process still running when your turn ends \
is killed: when you start something you need the result of (a build, a test run), wait for it \
in the foreground before you finish, never in the background."
    )
}

/// How long a VM run's sign-in must last: the run's limit (2 hours when the server sets none)
/// and half an hour more.
pub fn vm_run_need(a: &RunAssign) -> Duration {
    Duration::from_millis(a.max_duration_ms.unwrap_or(2 * 3_600_000)) + Duration::from_secs(1800)
}

/// The run's working folder. A side chat run gets its own (read-only) checkout; every other
/// run works in its member's line for the topic, so a follow-up run finds what the earlier ones
/// left. When another run holds the line the run gets its own folder too. Also returns the
/// hold on the line and what the CLI must know about the folder.
async fn prepare_work(
    cfg: &Config,
    a: &RunAssign,
) -> Result<
    (
        std::path::PathBuf,
        Option<workspace::LineGuard>,
        Option<String>,
    ),
    String,
> {
    let own = async {
        workspace::prepare(cfg, &a.run_id, a.repo.as_ref())
            .await
            .map(|(cwd, _)| cwd)
    };
    if a.kind.as_deref() == Some(KIND_ASIDE) {
        return Ok((own.await?, None, None));
    }
    let resumed = a.resume_session_id.is_some();
    match workspace::prepare_line(
        cfg,
        &a.run_id,
        &a.bot_member_id,
        &a.topic_id,
        a.repo.as_ref(),
    )
    .await?
    {
        Some((cwd, meta, guard)) => {
            let note = line_note(
                &cwd,
                meta.branch.as_deref(),
                resumed,
                guard.fresh,
                guard.restored,
            );
            Ok((cwd, Some(guard), note))
        }
        None => {
            let cwd = own.await?;
            let note = format!(
                "Another run of yours in this topic is using your usual working folder, so this \
run has a separate one ({}): changes you make here are not in that folder.",
                cwd.display()
            );
            Ok((cwd, None, Some(note)))
        }
    }
}

/// [`prepare_work`] with the run's GitHub (feature "github-host", a VM run): the daemon's own
/// clone and fetch use the host's GitHub sign-in. Without one they go without credentials (a
/// public repository works); when that fails, the run reports that the computer is not signed
/// in and ends before its CLI starts. A run on the computer itself uses the computer's git.
async fn prepare_with_github(
    cfg: &Config,
    a: &RunAssign,
    github: Option<&crate::github::Run>,
    event: &(dyn Fn(RunEvent) + Send + Sync),
) -> Result<
    (
        std::path::PathBuf,
        Option<workspace::LineGuard>,
        Option<String>,
    ),
    String,
> {
    let Some(g) = github.filter(|g| g.vm()) else {
        return prepare_work(cfg, a).await;
    };
    let mut lacking = None;
    let token = if crate::github::fetches_from_github(a.repo.as_ref()) {
        match g.fetch().await {
            crate::github::Got::Token(t) => Some(t),
            crate::github::Got::Missing(reason) => {
                lacking = Some(reason);
                None
            }
            crate::github::Got::Failed(_) => None,
        }
    } else {
        None
    };
    let env = crate::github::fetch_env(token.as_deref());
    match (
        workspace::with_git_env(env, prepare_work(cfg, a)).await,
        lacking,
    ) {
        (Err(e), Some(reason)) => {
            g.missing(reason, event);
            Err(format!("{} ({e})", crate::github::NOT_SIGNED_IN))
        }
        (r, _) => r,
    }
}

/// The run works in a repository (`repo.url` or `repo.path`), so it has a code location.
fn has_repo(a: &RunAssign) -> bool {
    a.repo
        .as_ref()
        .is_some_and(|r| r.url.is_some() || r.path.is_some())
}

/// The environment of the daemon's fetch at the end of a run (the code location's "pushed"):
/// in a VM run the host's GitHub as for [`prepare_with_github`], nothing otherwise. A
/// `repo.path` run whose folder's `origin` is on GitHub gets the token too: the CLI pushed
/// with it.
async fn code_git_env(
    a: &RunAssign,
    work: &std::path::Path,
    github: Option<&crate::github::Run>,
) -> Vec<(String, Option<String>)> {
    let Some(g) = github.filter(|g| g.vm()) else {
        return vec![];
    };
    let origin = workspace::origin_url(work).await;
    let token = if crate::github::code_fetch_from_github(a.repo.as_ref(), origin.as_deref()) {
        match g.fetch().await {
            crate::github::Got::Token(t) => Some(t),
            _ => None,
        }
    } else {
        None
    };
    crate::github::fetch_env(token.as_deref())
}

/// What a resumed CLI session must know when its working folder is not the one its earlier
/// turns used (the line was cleaned up after being idle, or this is the first run since the
/// daemon kept lines). None when the folder is the same one.
pub fn line_note(
    cwd: &std::path::Path,
    branch: Option<&str>,
    resumed: bool,
    fresh: bool,
    restored: bool,
) -> Option<String> {
    if !resumed || !fresh {
        return None;
    }
    let what = match (branch, restored) {
        (Some(b), true) => format!(
            "It is a new checkout of your branch {b}: what you committed there is here, and what \
you left uncommitted was committed for you before the old folder was removed, to {b} (or, if \
that folder was not on a branch, to a branch {b}-kept-<time>). Check before you build on it."
        ),
        (Some(_), false) => "It is a new checkout of the base branch: changes from your earlier \
turns are not in it. Check before you build on them."
            .to_string(),
        (None, _) => "It is empty: files from your earlier turns are not in it.".to_string(),
    };
    Some(format!(
        "Your working folder ({}) is not the one your earlier turns in this conversation used. \
{what}",
        cwd.display()
    ))
}

/// A run that waits for a tool call it started counts as stalled only after this long.
const TOOL_STALL: Duration = Duration::from_secs(30 * 60);

/// 盯 1.4: tools that used this much CPU between two ticks (15 s) are working.
const TOOLS_BUSY_NS: u64 = 100_000_000;

/// 盯 1.4: whether each running run's tools are working, by run id; the heartbeat's
/// `RunCursor.active`. A run that is not here (paused, not started, ended) is not known.
static ACTIVE: std::sync::Mutex<std::collections::BTreeMap<String, bool>> =
    std::sync::Mutex::new(std::collections::BTreeMap::new());

/// `RunCursor.active` of a run: None when not known.
pub fn active(run_id: &str) -> Option<bool> {
    ACTIVE.lock().ok()?.get(run_id).copied()
}

fn set_active(run_id: &str, v: Option<bool>) {
    if let Ok(mut m) = ACTIVE.lock() {
        match v {
            Some(b) => {
                m.insert(run_id.to_string(), b);
            }
            None => {
                m.remove(run_id);
            }
        }
    }
}

/// Whether the tools used at least [`TOOLS_BUSY_NS`] of CPU between two samples: a process
/// that was there before counts what it used since, a new one all it used. Without an earlier
/// sample nothing is known yet: not busy.
fn tools_busy(
    before: Option<&std::collections::HashMap<i32, u64>>,
    now: Option<&std::collections::HashMap<i32, u64>>,
) -> bool {
    let (Some(before), Some(now)) = (before, now) else {
        return false;
    };
    let used: u64 = now
        .iter()
        .map(|(pid, ns)| ns.saturating_sub(before.get(pid).copied().unwrap_or(0)))
        .sum();
    used >= TOOLS_BUSY_NS
}

/// 盯 1.8: the run's deadline after `waited` of waiting for a person's approval: moved by it.
fn wait_moves(
    deadline: Option<tokio::time::Instant>,
    waited: Duration,
) -> Option<tokio::time::Instant> {
    deadline.map(|d| d + waited)
}

pub(crate) const BASE_ENV: &[&str] = &[
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
    for k in BASE_ENV.iter().chain(crate::sys::OS_ENV) {
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
    // Feature "github": who commits, and in a VM git's helper and the gh proxy's name (the
    // run replaces "this computer" with the account's login when it knows it).
    if let Some(g) = a.github.as_ref().filter(|_| !aside) {
        let vm_as = cfg.vm.as_ref().map(|_| crate::github::THIS_COMPUTER);
        crate::github::put_env(g, &mut env, vm_as);
    }
    env
}

/// The CLI's own memory stays off in every run: memory lives only in the product.
pub const NO_CLI_MEMORY: (&str, &str) = ("CLAUDE_CODE_DISABLE_AUTO_MEMORY", "1");

/// The last env step of a run, so neither the runtime's `env` nor an app key turns the CLI's
/// own memory back on.
pub fn seal_env(env: &mut Vec<(String, String)>) {
    let (k, v) = NO_CLI_MEMORY;
    env.retain(|(n, _)| n != k);
    env.push((k.into(), v.into()));
}

/// [`build_env`], then the app's API keys (under the runtime's fixed env, never in a side
/// chat), then [`seal_env`].
fn run_env(
    cfg: &Config,
    rt: &RuntimeConfig,
    a: &RunAssign,
    keys: &[(String, String)],
) -> Vec<(String, String)> {
    let mut env = build_env(cfg, rt, a);
    if a.kind.as_deref() != Some(KIND_ASIDE) {
        for (k, v) in keys {
            if !rt.env.contains_key(k) {
                env.retain(|(n, _)| n != k);
                env.push((k.clone(), v.clone()));
            }
        }
    }
    seal_env(&mut env);
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
    /// Where the run's CLI executes.
    pub exec: Arc<dyn Executor>,
    /// Feature "accounts": the API keys this run's member gets (none for side chats).
    pub keys: Vec<(String, String)>,
    /// 机 4.3: the app's keys this run's member may ask for after a person approves (none for
    /// side chats); never in its environment.
    pub ask_keys: Vec<(String, String)>,
    /// The daemon reported this run's CLI as one whose runs stop at gated commands
    /// (`CliInfo.gates`, [`crate::gates::reported_for`]).
    pub gated: bool,
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
        Some((_, p)) if !p.is_empty() => format!("{}{}{p}", bin.display(), crate::sys::PATH_SEP),
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
        exec,
        keys,
        ask_keys,
        gated,
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
    // Feature "github-host": a VM run's git and gh use the host's GitHub sign-in through the
    // daemon; a run on the computer itself uses the computer's own.
    let github = assign.github.clone().map(|g| {
        Arc::new(crate::github::Run::new(
            g,
            redactor.clone(),
            cfg.vm.is_some(),
        ))
    });
    let in_vm = cfg.vm.is_some() && exec.isolated();
    let prepared = prepare_with_github(&cfg, &assign, github.as_deref(), &event).await;
    let (outcome, timed_out, code) = match prepared {
        Err(e) => (Outcome::failed(e), false, None),
        Ok((cwd, line, line_note)) => {
            if let Some(mut meta) = workspace::read_meta(&dir) {
                meta.bot_member_id = Some(assign.bot_member_id.clone());
                if let Some(sid) = &assign.resume_session_id {
                    meta.resume_session_id = Some(sid.clone());
                }
                workspace::write_meta(&dir, &meta);
            }
            // In a VM the CLI's own folders (its sessions) go with the topic's work line, or
            // with the run when it has no line.
            let cli_root = in_vm.then(|| match &line {
                Some(l) => l.dir().join("cli"),
                None => dir.join("cli"),
            });
            event(RunEvent::Status {
                state: "running".into(),
                message: None,
            });
            let aside = assign.kind.as_deref() == Some(KIND_ASIDE);
            // A side chat run reads only: its checkout is read-only, not just its CLI's modes.
            if aside {
                workspace::set_read_only(&cwd, true);
            }
            let mut env = run_env(&cfg, &rt, &assign, &keys);
            // A VM run uses its repository's caches (design-vm-state 3.3): held until the CLI is
            // done, so cleanup leaves them alone; the Rust toolchain is installed first.
            let mut toolchain_missing = None;
            let cache = if in_vm && !aside {
                match crate::vm_cache::hold(&cfg.work_root, assign.repo.as_ref()).await {
                    Ok(held) => {
                        if let Some(ch) = crate::vm_cache::toolchain_channel(&cwd) {
                            let rustup = std::path::Path::new(crate::vm_cache::HOST_RUSTUP);
                            match crate::vm_cache::install_toolchain(&held.dir, &ch, rustup).await {
                                Ok(ms) => {
                                    eprintln!(
                                        "{} run {run_id}: toolchain {ch} ready ({ms})",
                                        now_ms()
                                    )
                                }
                                Err(e) => {
                                    eprintln!("{} run {run_id}: toolchain {ch}: {e}", now_ms());
                                    toolchain_missing = Some(ch);
                                }
                            }
                        }
                        crate::vm_cache::merge_vars(&mut env, &held.dir, &rt.env);
                        Some(held)
                    }
                    Err(e) => {
                        eprintln!("{} run {run_id}: no repository cache: {e}", now_ms());
                        None
                    }
                }
            } else {
                None
            };
            // A VM run: whose GitHub git and gh work as (the computer's account).
            if let Some(g) = github.as_ref().filter(|g| g.vm() && !aside)
                && env.iter().any(|(k, _)| k == crate::github::ENV_AS)
                && let Some(login) = g.login().await
            {
                env.retain(|(k, _)| k != crate::github::ENV_AS);
                env.push((crate::github::ENV_AS.into(), login));
            }
            // In a VM the CLI uses the topic's own folders and a copy of the sign-in that cannot
            // change this host's (机 3.2).
            let vm_ready = match &cli_root {
                Some(root) => crate::vm_login::prepare(
                    &rt,
                    root,
                    &cwd,
                    assign.resume_session_id.as_deref(),
                    vm_run_need(&assign),
                    &mut env,
                    &assign.bot_member_id,
                    &cfg,
                )
                .await
                .map(|token| {
                    if let Some(t) = token {
                        redactor.add(&t);
                    }
                }),
                None => Ok(()),
            };
            // Marks the run's processes, also tools that leave the CLI's tree (proctree).
            env.push((proctree::ENV_FOLDER.into(), dir.display().to_string()));
            let gate = Arc::new(
                crate::secrets::Gate::for_run(&cfg.gated_env, &assign.env_allowlist, aside)
                    .with_values(if aside { vec![] } else { ask_keys }),
            );
            outputs.set_gate(gate.clone());
            if let Some(g) = github.as_ref().filter(|g| g.vm() && !aside) {
                outputs.set_github(g.clone());
            }
            // Lives until the CLI is done; dropping it removes the socket.
            let chat_run = if aside {
                None
            } else {
                // In a VM `chat-run` always goes through the relayed socket.
                let files = rt.kind == "codex" && !exec.isolated();
                // Files and 产出 go up over HTTP as the run's member, from its working folder.
                outputs.set_local(crate::chat_run_files::Local {
                    server_url: cfg.server_url.clone(),
                    token: token.clone(),
                    run_id: run_id.clone(),
                    root: cwd.clone(),
                    redactor: redactor.clone(),
                    downloads: Some(dir.join(crate::chat_run_context::DOWNLOADS_DIR)),
                });
                let host =
                    start_chat_run(&dir, &cwd, files, &run_id, &out, outputs.clone(), &mut env);
                // `gh` asks the daemon for the host's token (in a VM: the tools disk's).
                if host.is_some()
                    && !exec.isolated()
                    && github.as_ref().is_some_and(|g| g.vm())
                    && let Err(e) = crate::github::install_gh(&dir.join("bin"))
                {
                    eprintln!("{} run {run_id}: no gh proxy: {e}", now_ms());
                }
                host
            };
            // Gated actions: only runtimes that can stop a command get them (the server sends
            // them only for CLIs that report `gates`), never a side chat.
            let mut gates = crate::gates::Gates::new(
                assign
                    .gates
                    .as_deref()
                    .filter(|_| !aside && crate::gates::enforced_by(&rt.kind))
                    .unwrap_or_default(),
            );
            if let Some(ms) = cfg.test_clocks.and_then(|c| c.hooks_start_ms) {
                gates = gates.with_hooks_start(Duration::from_millis(ms));
            }
            let gates = Arc::new(gates);
            outputs.set_gates(gates.clone());
            // Shims first on PATH for the programs the kinds are about: commands inside scripts
            // and spellings the patterns miss stop too (crate::shim). In a VM they are on the
            // tools disk.
            if !gates.is_empty() && chat_run.is_some() {
                let bin = if exec.isolated() {
                    crate::github::GUEST_BIN.to_string()
                } else {
                    let path = env
                        .iter()
                        .find(|(k, _)| k == "PATH")
                        .map(|(_, v)| std::ffi::OsString::from(v))
                        .unwrap_or_default();
                    if let Err(e) = crate::shim::install(&dir.join("bin"), &path) {
                        eprintln!("{} run {run_id}: shims left out: {e}", now_ms());
                    }
                    dir.join("bin").display().to_string()
                };
                env.retain(|(k, _)| k != crate::shim::ENV_GATES);
                env.push((crate::shim::ENV_GATES.into(), "1".into()));
                // Claude Code runs Bash with the PATH of the user's shell profile: put the shims
                // first again before each command (on Windows Git Bash, whose /etc/profile puts
                // /mingw64/bin first).
                if rt.kind == "claude"
                    && !env.iter().any(|(k, _)| k == crate::shim::CLAUDE_ENV_FILE)
                {
                    match crate::shim::claude_env_file(&dir, &bin) {
                        Ok(f) => env.push((crate::shim::CLAUDE_ENV_FILE.into(), f)),
                        Err(e) => eprintln!("{} run {run_id}: no Claude env file: {e}", now_ms()),
                    }
                }
                // Every bash of the run (Codex's `bash -lc` when the user's shell is bash, Git
                // Bash, a `bash -l script`) reads BASH_ENV after the profiles that reset PATH.
                // Codex then skips ~/.bashrc when it captures the shell: the file reads it.
                if !exec.isolated()
                    && let Err(e) =
                        crate::shim::set_bash_env(&mut env, &dir, &bin, rt.kind != "claude")
                {
                    eprintln!("{} run {run_id}: no bash env file: {e}", now_ms());
                }
                // Codex (and an ACP agent) runs each command in the user's login shell
                // (`zsh -lc`): on macOS path_helper, or a profile, would put the shims last.
                // A zsh of the run reads the user's own files, then puts them first again.
                if rt.kind != "claude" && cfg!(unix) && !exec.isolated() {
                    match crate::shim::zsh_dotdir(&dir, &bin) {
                        Ok(zdot) => {
                            let own = env
                                .iter()
                                .find(|(k, _)| k == crate::shim::ZDOTDIR)
                                .map(|(_, v)| v.clone())
                                .or_else(|| std::env::var(crate::shim::ZDOTDIR).ok())
                                .filter(|v| !v.is_empty());
                            env.retain(|(k, _)| {
                                k != crate::shim::ZDOTDIR && k != crate::shim::ENV_USER_ZDOTDIR
                            });
                            if let Some(own) = own {
                                env.push((crate::shim::ENV_USER_ZDOTDIR.into(), own));
                            }
                            env.push((crate::shim::ZDOTDIR.into(), zdot.display().to_string()));
                        }
                        Err(e) => eprintln!("{} run {run_id}: no zsh files: {e}", now_ms()),
                    }
                }
            }
            let gate_hook =
                (!gates.is_empty()).then(|| dir.join("bin").join(crate::chat_run::link_name()));
            let gate_alive = (!gates.is_empty()).then(|| gates.alive());
            // Gates for a CLI the daemon did not report as gated (on Windows one older than
            // the tested version, or one whose version changed since `hello`): its hook could
            // let a command through, so the run does not start.
            let ungated = !gates.is_empty() && !gated;
            let notes: Vec<String> = [
                line_note,
                gate.note(),
                gates.note(),
                cfg.vm.as_ref().map(|_| {
                    vm_note(
                        cache.as_ref().map(|c| c.dir.as_path()),
                        toolchain_missing.as_deref(),
                    )
                }),
                github
                    .as_ref()
                    .filter(|_| !aside)
                    .map(|g| crate::github::note(&g.spec, g.vm())),
            ]
            .into_iter()
            .flatten()
            .collect();
            let spec = RunSpec {
                run_id: run_id.clone(),
                runtime_name: assign.runtime.clone(),
                cfg: rt.clone(),
                prompt: assign.prompt.clone(),
                model: assign.model.clone(),
                effort: assign.effort.clone(),
                cwd,
                run_dir: dir.clone(),
                env,
                aside,
                note: chat_run.as_ref().map(|_| {
                    std::iter::once(crate::chat_run::note())
                        .chain(notes)
                        .collect::<Vec<_>>()
                        .join("\n\n")
                }),
                resume_session_id: assign
                    .resume_session_id
                    .clone()
                    .filter(|s| runtime::valid_session_id(s)),
                full_prompt: assign.full_prompt.clone(),
                gate_hook,
                gate_alive,
                gates: Some(gates.clone()),
                exec: exec.clone(),
                vm_shares: cli_root
                    .iter()
                    .cloned()
                    .chain(cache.iter().map(|c| c.dir.clone()))
                    .collect(),
            };
            let work = spec.cwd.clone();
            // The hook asks through chat-run: a gated run never goes on without it.
            let res = if let Err(e) = vm_ready {
                (Outcome::failed(e), false)
            } else if ungated {
                (
                    Outcome::failed(format!(
                        "this run has commands that need a person's approval, and the {} on \
this machine cannot be made to stop at them (update it)",
                        rt.command()
                    )),
                    false,
                )
            } else if spec.gate_hook.is_some() && chat_run.is_none() {
                (
                    Outcome::failed("cannot ask for approvals: chat-run did not start"),
                    false,
                )
            } else {
                run_runtime(
                    &cfg,
                    &assign,
                    spec,
                    control_rx,
                    &event,
                    exec.as_ref(),
                    Own {
                        secrets: &gate,
                        gates: &gates,
                    },
                )
                .await
            };
            drop(chat_run);
            // The sign-in copies live only while the CLI runs.
            if let Some(root) = &cli_root {
                crate::vm_login::remove_auth(root);
            }
            if aside {
                workspace::set_read_only(&work, false);
            }
            // Where the code is (design-relay R5), read while the run still holds the line.
            let code = if aside || !has_repo(&assign) {
                None
            } else {
                let fallback = workspace::read_meta(&dir).and_then(|m| m.branch);
                let env = code_git_env(&assign, &work, github.as_deref()).await;
                workspace::with_git_env(env, workspace::code_location(&work, fallback.as_deref()))
                    .await
            };
            // The CLI is done: the next run of this member in this topic may use the line.
            drop(line);
            drop(cache);
            (res.0, res.1, code)
        }
    };

    // A token the run fetched may be in what the CLI wrote to stderr.
    if redactor.has_added() {
        redactor.file(&dir.join("stderr.log"));
    }

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
        code,
        failure: (status == "failed")
            .then(|| crate::failure::of_run(outcome.failure.as_ref(), outcome.error.as_deref())),
        resume_missed: outcome.resume_missed.then_some(true),
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

/// A run's two clocks after the machine slept for `asleep`: when the CLI was last heard
/// (`last`) and when the run's time limit ends (`deadline`). The time asleep is no silence
/// and no part of the limit. A paused run is left as it is: handing it back moves both by the
/// whole pause, which has the sleep in it already; moving them here too would count it twice.
fn after_sleep(
    last: Instant,
    deadline: Option<tokio::time::Instant>,
    asleep: Duration,
    now: Instant,
    paused: bool,
) -> (Instant, Option<tokio::time::Instant>) {
    if paused {
        (last, deadline)
    } else {
        ((last + asleep).min(now), deadline.map(|d| d + asleep))
    }
}

/// Pause state of a run (接管).
struct Paused {
    since: Instant,
    /// The stopped CLI and its tools (None while the CLI has not started yet).
    stopped: Option<Box<dyn Stopped>>,
    /// A person took over: their commands may run (design-takeover 3.4).
    takeover: Option<crate::hand::Takeover>,
    /// Inputs for the CLI that came while it was held: given after the hand-back note.
    queued: Vec<Control>,
}

impl Paused {
    /// The id of the person's command that runs now.
    fn running_command(&self) -> Option<String> {
        self.takeover
            .as_ref()
            .and_then(|t| t.running.as_ref())
            .map(|r| r.id.clone())
    }

    /// Lets the CLI and its tools continue.
    fn resume(&self) {
        if let Some(s) = &self.stopped {
            s.resume();
        }
    }
}

/// The approvals the daemon answers itself: gated secrets and gated actions.
struct Own<'a> {
    secrets: &'a crate::secrets::Gate,
    gates: &'a crate::gates::Gates,
}

async fn run_runtime(
    cfg: &Config,
    assign: &RunAssign,
    spec: RunSpec,
    mut control_rx: mpsc::UnboundedReceiver<RunCtl>,
    event: &impl Fn(RunEvent),
    exec: &dyn Executor,
    own: Own<'_>,
) -> (Outcome, bool) {
    let Some(rt) = runtime::for_kind(&spec.cfg.kind) else {
        return (
            Outcome::failed(format!("unknown runtime kind {}", spec.cfg.kind)),
            false,
        );
    };
    let run_dir = spec.run_dir.clone();
    // 接管: a person's commands run in the work folder with the daemon's own env.
    let work = spec.cwd.clone();
    let hand_env = crate::hand::host_env(cfg, &spec.env);
    let (hand_tx, mut hand_rx) = mpsc::unbounded_channel::<crate::hand::HandMsg>();
    let (emit_tx, mut emit_rx) = mpsc::unbounded_channel();
    let (rt_tx, rt_rx) = mpsc::unbounded_channel::<Control>();
    let fut = rt.start(spec, emit_tx, rt_rx);
    tokio::pin!(fut);
    let stall = match cfg.test_clocks.and_then(|c| c.stall_ms) {
        Some(ms) => Duration::from_millis(ms),
        None => Duration::from_secs(cfg.stall_minutes.max(1) * 60),
    };
    // Tool calls without a result: the CLI waits for a step it started (a build, a test run),
    // so its silence is not a stall until much later.
    let mut open_tools: std::collections::HashSet<String> = Default::default();
    let mut last = Instant::now();
    let mut stalled = false;
    let mut timed_out = false;
    let mut deadline = assign
        .max_duration_ms
        .map(|ms| tokio::time::Instant::now() + Duration::from_millis(ms));
    let tick_every = cfg
        .test_clocks
        .and_then(|c| c.tick_ms)
        .map_or(Duration::from_secs(15), Duration::from_millis);
    let mut tick = tokio::time::interval(tick_every);
    let mut ticked = Instant::now();
    let mut tools_before: Option<std::collections::HashMap<i32, u64>> = None;
    let mut paused: Option<Paused> = None;
    // The person's command that ended with the take-over when the run was cancelled: it still
    // gets its `hand.result`.
    let mut ending: Option<String> = None;
    let mut resumes = 0u32;
    let mut control_open = true;
    let mut acceptance = crate::acceptance::AcceptanceTracker::default();
    // Streamed text is released per line, so redaction sees a secret whole.
    let mut gate = DeltaGate::default();
    let outcome = loop {
        // A pause that came before the CLI started stops it as soon as its group is known.
        if let Some(p) = paused.as_mut().filter(|p| p.stopped.is_none()) {
            p.stopped = exec.stop(&run_dir);
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
                match &ev {
                    RunEvent::ToolCall { item_id, .. } => { open_tools.insert(item_id.clone()); }
                    RunEvent::ToolResult { item_id, .. } => { open_tools.remove(item_id); }
                    RunEvent::TurnCompleted { .. } => open_tools.clear(),
                    _ => {}
                }
                let extra = acceptance.on_event(&ev);
                for ev in gate.push(ev).into_iter().chain(extra) {
                    event(ev);
                }
            }
            c = control_rx.recv(), if control_open => {
                let Some(c) = c else { control_open = false; continue };
                match c {
                    RunCtl::Pause { by, hold } => {
                        let take = hold.as_deref() == Some(chat_protocol::takeover::HOLD_TAKEOVER)
                            && exec.takeover().is_some();
                        // Already stopped (the resume before this pause never came, or a pause
                        // becomes a take-over): report the pause again, so the server counts
                        // every pause it sent and a later `paused: false` clears pausedBy
                        // (GAPS SB-16).
                        if let Some(p) = paused.as_mut() {
                            match p.takeover.as_mut() {
                                Some(t) if take => {
                                    if let Some(b) = by.clone() {
                                        t.by = b;
                                    }
                                }
                                None if take => {
                                    p.takeover =
                                        Some(crate::hand::Takeover::start(by.clone(), &work).await);
                                }
                                _ => {}
                            }
                        } else {
                            let stopped = exec.stop(&run_dir);
                            let takeover = if take {
                                Some(crate::hand::Takeover::start(by.clone(), &work).await)
                            } else {
                                None
                            };
                            paused = Some(Paused {
                                since: Instant::now(),
                                stopped,
                                takeover,
                                queued: vec![],
                            });
                        }
                        for ev in gate.push(RunEvent::Paused { paused: true, by }) {
                            event(ev);
                        }
                    }
                    RunCtl::Resume { note } => {
                        let words = note.filter(|n| !n.trim().is_empty());
                        let Some(mut p) = paused.take() else {
                            // Not held (the pause never came, or this daemon restarted): say so,
                            // so the server's state is right, and pass the words on.
                            for ev in gate.push(RunEvent::Paused { paused: false, by: None }) {
                                event(ev);
                            }
                            if let Some(text) = words {
                                resumes += 1;
                                let _ = rt_tx.send(Control::Input {
                                    input_id: format!("resume-{resumes}"),
                                    text,
                                });
                            }
                            continue;
                        };
                        // After a take-over the CLI hears what the person did, before their
                        // words and before what came for it meanwhile.
                        let text = match p.takeover.take() {
                            Some(t) => {
                                let (text, summary) =
                                    t.hand_back(&work, words.as_deref(), &mut hand_rx, &event).await;
                                exec.hand_back(&run_dir);
                                event(RunEvent::HandSummary(summary));
                                Some(text)
                            }
                            None => words,
                        };
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
                        if let Some(text) = text {
                            resumes += 1;
                            let _ = rt_tx.send(Control::Input {
                                input_id: format!("resume-{resumes}"),
                                text,
                            });
                        }
                        for c in std::mem::take(&mut p.queued) {
                            let _ = rt_tx.send(c);
                        }
                    }
                    RunCtl::Command { id, command, by } => {
                        match paused.as_mut().and_then(|p| p.takeover.as_mut()) {
                            Some(t) => t.command(
                                exec,
                                crate::executor::HandReq {
                                    run_dir: run_dir.clone(),
                                    cwd: work.clone(),
                                    command,
                                    env: hand_env.clone(),
                                },
                                id,
                                &by,
                                crate::hand::LIMIT,
                                &hand_tx,
                                &event,
                            ),
                            None => event(crate::hand::refused(&id, "not_held")),
                        }
                    }
                    RunCtl::CommandCancel { id } => {
                        if let Some(t) = paused.as_ref().and_then(|p| p.takeover.as_ref()) {
                            t.cancel(&id);
                        }
                    }
                    // An answer to `chat-run secret` or to a gated command: the daemon's own
                    // approvals, not the CLI's.
                    RunCtl::Approval { approval_id, approve, all } => {
                        let answered = if own.secrets.owns(&approval_id) {
                            Some(own.secrets.answer(&approval_id, approve, Instant::now()))
                        } else if own.gates.owns(&approval_id) {
                            Some(own.gates.answer(&approval_id, approve, all))
                        } else {
                            None
                        };
                        match answered {
                            Some(resolved) => {
                                for ev in resolved.into_iter().flat_map(|ev| gate.push(ev)) {
                                    event(ev);
                                }
                                // The wait is over: the stall clock starts again.
                                last = Instant::now();
                            }
                            None => {
                                let _ = rt_tx.send(Control::Approval { approval_id, approve });
                            }
                        }
                    }
                    // Input for a held CLI waits for the hand-back (design-takeover 3.5).
                    RunCtl::Runtime(c @ Control::Input { .. }) if paused.is_some() => {
                        if let Some(p) = paused.as_mut() {
                            p.queued.push(c);
                        }
                    }
                    RunCtl::Runtime(c) => {
                        // A stopped CLI cannot handle SIGTERM: a cancel resumes it first (and
                        // ends the person's command with the take-over).
                        if c == Control::Cancel && let Some(p) = paused.take() {
                            ending = p.running_command();
                            p.resume();
                            for ev in gate.push(RunEvent::Paused { paused: false, by: None }) {
                                event(ev);
                            }
                        }
                        let _ = rt_tx.send(c);
                    }
                }
            }
            Some(m) = hand_rx.recv() => {
                match paused.as_mut().and_then(|p| p.takeover.as_mut()) {
                    Some(t) => t.on_msg(m, &event),
                    // The take-over is over (the run was cancelled): the command's last events
                    // still go up.
                    None => if let crate::hand::HandMsg::Event(ev) = m {
                        if matches!(&ev, RunEvent::HandResult { command_id, .. }
                            if ending.as_deref() == Some(command_id.as_str()))
                        {
                            ending = None;
                        }
                        event(ev)
                    },
                }
            }
            _ = tokio::time::sleep_until(hold_until), if gate.holding() => {
                for ev in gate.tick(std::time::Instant::now()) {
                    event(ev);
                }
            }
            _ = tick.tick() => {
                // The machine was asleep (or the daemon was not run) since the last tick:
                // the CLI did not run either. That time is no silence of the CLI and no part
                // of the run's time limit.
                let now = Instant::now();
                let step = now.duration_since(ticked);
                let asleep = crate::sys::slept(step, tick_every, cfg.sleep_late());
                if let Some(asleep) = asleep {
                    (last, deadline) = after_sleep(last, deadline, asleep, now, paused.is_some());
                }
                ticked = now;
                if paused.is_some() {
                    set_active(&assign.run_id, None);
                    tools_before = None;
                    continue;
                }
                // A command waiting for a person is not stuck (the server shows waiting_answer),
                // and 盯 1.8: its wait is no part of the run's time limit.
                if own.gates.waiting() || own.secrets.waiting() {
                    last = Instant::now();
                    deadline = wait_moves(deadline, step.saturating_sub(asleep.unwrap_or_default()));
                }
                // 盯 1.4: the tools the CLI started (a build, a test run) are working.
                let tools_now = exec.tools_cpu(&run_dir);
                let busy = tools_busy(tools_before.as_ref(), tools_now.as_ref());
                set_active(&assign.run_id, tools_now.as_ref().map(|_| busy));
                tools_before = tools_now;
                if busy {
                    last = Instant::now();
                }
                let limit = if open_tools.is_empty() { stall } else { stall.max(TOOL_STALL) };
                if !stalled && last.elapsed() >= limit {
                    stalled = true;
                    event(RunEvent::Status { state: "stalled".into(),
                        message: Some(format!("no output for {} minutes", limit.as_secs() / 60)) });
                }
                if let Some(d) = deadline
                    && !timed_out && tokio::time::Instant::now() >= d {
                        timed_out = true;
                        let _ = rt_tx.send(Control::Cancel);
                    }
            }
        }
    };
    set_active(&assign.run_id, None);
    // Never leave a stopped group behind (the runtime already reaped the CLI). A person's
    // command ends with the take-over, and what their commands left with the run.
    if let Some(p) = paused.take() {
        ending = p.running_command().or(ending);
        p.resume();
    }
    exec.hand_end(&run_dir);
    crate::hand::finish_with_run(ending, &mut hand_rx, Duration::from_secs(1), &event).await;
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_vm_run_is_told_what_is_kept_and_its_sign_in_outlasts_its_limit() {
        let c = std::path::Path::new("/w/caches/0123456789abcdef");
        let note = vm_note(Some(c), None);
        assert!(note.contains("build outputs are kept for your next run in this topic"));
        assert!(note.contains("are in /w/caches/0123456789abcdef and already on PATH"));
        assert!(note.contains("$HOME, /tmp and the system folders is gone"));
        assert!(note.contains("an administrator must add it to the base image"));
        assert!(note.contains("Never sign in"));
        assert!(!note.contains("GB") && !note.contains("rustup toolchain install"));
        let note = vm_note(None, Some("1.99.1"));
        assert!(!note.contains("already on PATH"));
        assert!(note.contains("`rustup toolchain install 1.99.1`"));
        let mut a: RunAssign = serde_json::from_value(serde_json::json!({
            "runId": "r1", "botMemberId": "B1", "groupId": "g", "topicId": "t",
            "runtime": "claude", "prompt": "p", "leaseMs": 600000
        }))
        .unwrap();
        assert_eq!(vm_run_need(&a), Duration::from_secs(9000));
        a.max_duration_ms = Some(600_000);
        assert_eq!(vm_run_need(&a), Duration::from_secs(2400));
    }

    /// 存 3.6: the CLI's own memory is off in every run of every kind, side chats too; neither
    /// the runtime's `env` nor an app key turns it back on.
    #[test]
    fn every_run_has_the_cli_memory_off() {
        let mut env = vec![
            ("A".to_string(), "1".to_string()),
            (
                "CLAUDE_CODE_DISABLE_AUTO_MEMORY".to_string(),
                "0".to_string(),
            ),
        ];
        seal_env(&mut env);
        assert_eq!(
            env,
            [("A", "1"), ("CLAUDE_CODE_DISABLE_AUTO_MEMORY", "1")]
                .map(|(k, v)| (k.to_string(), v.to_string()))
        );
        let mut text =
            "server_url = \"http://127.0.0.1:1\"\ntoken_file = \"/nonexistent\"\n".to_string();
        for kind in ["claude", "codex", "acp", "pty"] {
            text.push_str(&format!(
                "[runtimes.{kind}]\nkind = \"{kind}\"\ncommand = \"x\"\nenv = {{ CLAUDE_CODE_DISABLE_AUTO_MEMORY = '0' }}\n"
            ));
        }
        let cfg = Config::parse(&text).unwrap();
        let memory = |env: &[(String, String)]| -> Vec<String> {
            env.iter()
                .filter(|(k, _)| k == NO_CLI_MEMORY.0)
                .map(|(_, v)| v.clone())
                .collect()
        };
        for kind in ["claude", "codex", "acp", "pty"] {
            for aside in [false, true] {
                let mut rt = cfg.runtimes[kind].clone();
                let mut a: RunAssign = serde_json::from_value(serde_json::json!({
                    "runId": "r1", "botMemberId": "B1", "groupId": "g", "topicId": "t",
                    "runtime": kind, "prompt": "p", "leaseMs": 600000
                }))
                .unwrap();
                a.kind = aside.then(|| KIND_ASIDE.to_string());
                let key = [(NO_CLI_MEMORY.0.to_string(), "0".to_string())];
                assert_eq!(
                    memory(&run_env(&cfg, &rt, &a, &key)),
                    ["1"],
                    "{kind} {aside}"
                );
                rt.env.clear();
                assert_eq!(
                    memory(&run_env(&cfg, &rt, &a, &key)),
                    ["1"],
                    "{kind} {aside}"
                );
            }
        }
    }

    /// 盯 1.4: tools are busy when they used 100 ms of CPU since the last sample: what a known
    /// process used since, all that a new one used; nothing is known from one sample.
    #[test]
    fn tools_are_busy_when_they_use_cpu_between_samples() {
        let m = |v: &[(i32, u64)]| {
            v.iter()
                .copied()
                .collect::<std::collections::HashMap<_, _>>()
        };
        let before = m(&[(10, 5_000_000_000), (11, 1_000)]);
        assert!(!tools_busy(None, Some(&before)));
        assert!(!tools_busy(Some(&before), None));
        // A `sleep 600` and an idle shell: nothing moves.
        assert!(!tools_busy(Some(&before), Some(&before)));
        assert!(!tools_busy(
            Some(&before),
            Some(&m(&[(10, 5_050_000_000), (11, 1_000)]))
        ));
        // A compiler at work.
        assert!(tools_busy(
            Some(&before),
            Some(&m(&[(10, 5_200_000_000), (11, 1_000)]))
        ));
        // A new test process that already used 0.3 s; one that ended does not count against.
        assert!(tools_busy(Some(&before), Some(&m(&[(12, 300_000_000)]))));
        // The registry the heartbeat reads.
        set_active("run-busy-1", Some(true));
        assert_eq!(active("run-busy-1"), Some(true));
        set_active("run-busy-1", None);
        assert_eq!(active("run-busy-1"), None);
    }

    /// 盯 1.8: the wait for a person's approval moves the deadline by the time waited.
    #[test]
    fn an_approval_wait_moves_the_deadline() {
        let d = tokio::time::Instant::now() + Duration::from_secs(600);
        assert_eq!(
            wait_moves(Some(d), Duration::from_secs(15)),
            Some(d + Duration::from_secs(15))
        );
        assert_eq!(wait_moves(None, Duration::from_secs(15)), None);
    }

    /// A sleep moves a working run's clocks by the time asleep, once. A paused run's clocks
    /// stay: the hand-back moves them by the whole pause, sleep included.
    #[test]
    fn a_sleep_is_counted_once_also_for_a_paused_run() {
        let start = Instant::now();
        let now = start + Duration::from_secs(1000);
        let last = start + Duration::from_secs(10);
        let deadline = tokio::time::Instant::from_std(start + Duration::from_secs(600));
        let asleep = Duration::from_secs(900);
        // Working: the CLI was silent for 990 s of which 900 asleep, and the limit moves.
        let (l, d) = after_sleep(last, Some(deadline), asleep, now, false);
        assert_eq!(now.duration_since(l), Duration::from_secs(90));
        assert_eq!(d, Some(deadline + asleep));
        // Never into the future.
        let (l, _) = after_sleep(last, None, Duration::from_secs(5000), now, false);
        assert_eq!(l, now);
        // Paused since `last`: nothing moves here; the hand-back adds the pause (990 s, the
        // 900 s of sleep in it), so the limit moves by 990 s in all, not by 1890 s.
        let (l, d) = after_sleep(last, Some(deadline), asleep, now, true);
        assert_eq!((l, d), (last, Some(deadline)));
        let handed_back = d.map(|d| d + now.duration_since(last));
        assert_eq!(handed_back, Some(deadline + Duration::from_secs(990)));
    }

    /// A resumed session is told when its working folder is not the one its earlier turns
    /// used, and what is in the new one; otherwise nothing is said.
    #[test]
    fn a_resumed_session_hears_when_its_folder_is_new() {
        let cwd = std::path::Path::new("/w/lines/k/work");
        assert_eq!(line_note(cwd, Some("bot/k"), true, false, false), None);
        assert_eq!(line_note(cwd, Some("bot/k"), false, true, false), None);
        let restored = line_note(cwd, Some("bot/k"), true, true, true).unwrap();
        assert!(restored.contains("/w/lines/k/work"), "{restored}");
        assert!(restored.contains("branch bot/k"), "{restored}");
        assert!(restored.contains("bot/k-kept-"), "{restored}");
        let new = line_note(cwd, Some("bot/k"), true, true, false).unwrap();
        assert!(new.contains("not in it"), "{new}");
        let scratch = line_note(cwd, None, true, true, false).unwrap();
        assert!(scratch.contains("empty"), "{scratch}");
    }

    /// Feature "github-host": the host is not signed in to GitHub and the daemon's own clone of
    /// a VM run fails without credentials: the run reports `missing` with the reason once and
    /// ends with what the CLI would have been told, before any CLI starts.
    #[cfg(unix)]
    #[tokio::test]
    async fn a_clone_that_needs_the_hosts_missing_github_ends_the_run() {
        let dir = tempfile::tempdir().unwrap();
        let bin = dir.path().join("hostbin");
        std::fs::create_dir(&bin).unwrap();
        let gh = bin.join("gh");
        std::fs::write(&gh, "#!/bin/sh\nexit 1\n").unwrap();
        crate::sys::set_mode(&gh, 0o755).unwrap();
        let cfg = Config::parse(&format!(
            "server_url = \"http://127.0.0.1:1\"\ntoken_file = '{}'\nwork_root = '{}'\n",
            dir.path().join("token").display(),
            dir.path().join("root").display()
        ))
        .unwrap();
        let spec = chat_protocol::github::RunGithub {
            source: "machine".into(),
            member_id: "U1".into(),
            login: None,
            author: None,
            committer: chat_protocol::github::GitIdentity {
                name: "Bot".into(),
                email: "ai-B1@users.noreply.invalid".into(),
            },
            rewrite_ssh: None,
            asked_by: None,
        };
        let a = RunAssign {
            run_id: "rg".into(),
            bot_member_id: "BOTRUN".into(),
            group_id: "G".into(),
            topic_id: "TG".into(),
            runtime: "fake".into(),
            prompt: "p".into(),
            model: None,
            effort: None,
            // Nothing listens there: the clone fails at once, no network needed.
            repo: Some(crate::proto::RepoSpec {
                url: Some("https://127.0.0.1:1/github.com/o/private.git".into()),
                path: None,
                base_branch: None,
            }),
            env_allowlist: vec![],
            lease_ms: 1,
            max_duration_ms: None,
            session_upload: None,
            kind: None,
            resume_session_id: None,
            full_prompt: None,
            gates: None,
            github: Some(spec.clone()),
        };
        let g = crate::github::Run::new(spec, Arc::new(crate::redact::Redactor::default()), true)
            .with_path(bin.as_os_str());
        let events = std::sync::Mutex::new(vec![]);
        let emit = |e: RunEvent| events.lock().unwrap().push(e);
        let err = prepare_with_github(&cfg, &a, Some(&g), &emit)
            .await
            .unwrap_err();
        assert!(
            err.starts_with("This computer is not signed in to GitHub."),
            "{err}"
        );
        // A later use in the same run says nothing more.
        g.missing("signed_out", &emit);
        assert_eq!(
            events.into_inner().unwrap(),
            vec![RunEvent::Github {
                state: "missing".into(),
                login: None,
                reason: Some("signed_out".into()),
            }]
        );
    }

    /// Every non-side-chat run of a member in a topic gets the same line; a side chat gets
    /// its own folder; a second run while the line is held gets its own folder and is told.
    #[tokio::test]
    async fn runs_in_a_topic_share_the_members_line() {
        let dir = tempfile::tempdir().unwrap();
        let cfg = Config::parse(&format!(
            "server_url = \"http://127.0.0.1:1\"\ntoken_file = '{}'\nwork_root = '{}'\n",
            dir.path().join("token").display(),
            dir.path().join("root").display()
        ))
        .unwrap();
        let a = |run: &str, kind: Option<&str>| RunAssign {
            run_id: run.into(),
            bot_member_id: "BOTRUN".into(),
            group_id: "G".into(),
            topic_id: "TRUN".into(),
            runtime: "fake".into(),
            prompt: "p".into(),
            model: None,
            effort: None,
            repo: None,
            env_allowlist: vec![],
            lease_ms: 1,
            max_duration_ms: None,
            session_upload: None,
            kind: kind.map(str::to_string),
            resume_session_id: None,
            full_prompt: None,
            gates: None,
            github: None,
        };
        let (w1, g1, n1) = prepare_work(&cfg, &a("r1", None)).await.unwrap();
        assert!(g1.is_some() && n1.is_none());
        assert!(w1.starts_with(workspace::lines_dir(&cfg)));
        let (w2, g2, n2) = prepare_work(&cfg, &a("r2", None)).await.unwrap();
        assert_eq!(w2, workspace::run_dir(&cfg, "r2").join("work"));
        assert!(g2.is_none());
        assert!(n2.unwrap().contains("separate"));
        drop(g1);
        let (w3, g3, _) = prepare_work(&cfg, &a("r3", None)).await.unwrap();
        assert_eq!(w3, w1);
        drop(g3);
        let (w4, g4, n4) = prepare_work(&cfg, &a("r4", Some(KIND_ASIDE)))
            .await
            .unwrap();
        assert_eq!(w4, workspace::run_dir(&cfg, "r4").join("work"));
        assert!(g4.is_none() && n4.is_none());
    }
}
