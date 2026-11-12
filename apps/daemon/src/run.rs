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
    },
    /// `run.resume`: SIGCONT, move the deadline by the paused time, give `note` to the CLI.
    Resume {
        note: Option<String>,
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
pub fn vm_note(vm: &crate::config::VmConfig) -> String {
    let kept = if vm.homes.as_os_str().is_empty() {
        "Your working folder is kept for your next run in this topic; nothing outside it is kept."
            .to_string()
    } else {
        let size = if crate::vm_home::limited(vm) {
            format!(" Your home folder holds {} GB in all.", vm.home_gb)
        } else {
            String::new()
        };
        format!(
            "Your working folder is kept for your next run in this topic, and your home folder \
($HOME) for all your runs: install tools and sign in there, and look there for what a run in \
another topic left. /tmp and everything else outside these two are gone after this run.{size}"
        )
    };
    format!(
        "You run in a virtual machine that was started for this run and is deleted when you end \
your turn. {kept} Every process still running when your turn ends is killed: when you start \
something you need the result of (a sign-in that waits for a person, a build, a test run), wait \
for it in the foreground before you finish, never in the background."
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

/// [`prepare_work`] with the run's GitHub (feature "github", `app` mode): the daemon's own
/// clone and fetch use the person's token. Without one they go without credentials (a public
/// repository works); when that fails, the run reports that it waits for the person and ends
/// before its CLI starts.
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
    let Some(g) = github.filter(|g| g.app()) else {
        return prepare_work(cfg, a).await;
    };
    let mut lacking = None;
    let token = if crate::github::fetches_from_github(a.repo.as_ref()) {
        match g.fetch(event).await {
            crate::github::Got::Token(t) => Some(t.token),
            crate::github::Got::Missing => {
                lacking = Some(false);
                None
            }
            crate::github::Got::Invalid => {
                lacking = Some(true);
                None
            }
            _ => None,
        }
    } else {
        None
    };
    let env = crate::github::fetch_env(&g.spec, token.as_deref());
    match (
        workspace::with_git_env(env, prepare_work(cfg, a)).await,
        lacking,
    ) {
        (Err(e), Some(invalid)) => {
            g.wait(invalid, event).await;
            Err(format!("{} ({e})", g.waiting(invalid)))
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
/// the person's GitHub as for [`prepare_with_github`], nothing without one. A `repo.path` run
/// whose folder's `origin` is on GitHub gets the token too: the CLI pushed with it.
async fn code_git_env(
    a: &RunAssign,
    work: &std::path::Path,
    github: Option<&crate::github::Run>,
    event: &(dyn Fn(RunEvent) + Send + Sync),
) -> Vec<(String, Option<String>)> {
    let Some(g) = github.filter(|g| g.app()) else {
        return vec![];
    };
    let origin = workspace::origin_url(work).await;
    let token = if crate::github::code_fetch_from_github(a.repo.as_ref(), origin.as_deref()) {
        match g.fetch(event).await {
            crate::github::Got::Token(t) => Some(t.token),
            _ => None,
        }
    } else {
        None
    };
    crate::github::fetch_env(&g.spec, token.as_deref())
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
    // Feature "github": who commits, and in `app` mode git's helper and the gh proxy's name.
    if let Some(g) = a.github.as_ref().filter(|_| !aside) {
        crate::github::put_env(g, &mut env);
    }
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
    let github = assign.github.clone().map(|g| {
        Arc::new(crate::github::Run::new(
            &cfg.server_url,
            token.clone(),
            &run_id,
            g,
            redactor.clone(),
        ))
    });
    // A VM member's own folder (its home: a fixed-size disk image), ready before anything uses it.
    let vm_member = match cfg.vm.as_ref().filter(|_| exec.isolated()) {
        Some(vm) => Some(crate::vm_home::ensure(vm, &assign.bot_member_id).await),
        None => None,
    };
    // A VM member's kept home: no GitHub sign-in of an earlier run is used again.
    if let (Some(vm), Some(_)) = (&cfg.vm, &github)
        && crate::github::clear_home(&vm.homes, &assign.bot_member_id)
    {
        eprintln!("{} run {run_id}: removed an old GitHub sign-in", now_ms());
    }
    let prepared = prepare_with_github(&cfg, &assign, github.as_deref(), &event).await;
    let (outcome, timed_out, code) = match prepared {
        Err(e) => (Outcome::failed(e), false, None),
        Ok((cwd, line, line_note)) => {
            if let Some(sid) = &assign.resume_session_id
                && let Some(mut meta) = workspace::read_meta(&dir)
            {
                meta.resume_session_id = Some(sid.clone());
                workspace::write_meta(&dir, &meta);
            }
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
            // API keys from the app: under the runtime's fixed env, and never in a side chat.
            if !aside {
                for (k, v) in &keys {
                    if !rt.env.contains_key(k) {
                        env.retain(|(n, _)| n != k);
                        env.push((k.clone(), v.clone()));
                    }
                }
            }
            // In a VM the CLI uses its member's own folders and a copy of the sign-in that cannot
            // change this host's (机 3.2).
            let vm_ready = match &vm_member {
                Some(Ok(member)) => crate::vm_login::prepare(
                    &rt,
                    member,
                    &cwd,
                    assign.resume_session_id.as_deref(),
                    vm_run_need(&assign),
                    &mut env,
                )
                .await
                .map(|token| {
                    if let Some(t) = token {
                        redactor.add(&t);
                    }
                }),
                Some(Err(e)) => Err(format!("the AI member's home folder is not ready: {e}")),
                None => Ok(()),
            };
            // Marks the run's processes, also tools that leave the CLI's tree (proctree).
            env.push((proctree::ENV_FOLDER.into(), dir.display().to_string()));
            let gate = Arc::new(crate::secrets::Gate::for_run(
                &cfg.gated_env,
                &assign.env_allowlist,
                aside,
            ));
            outputs.set_gate(gate.clone());
            if let Some(g) = github.as_ref().filter(|g| g.app() && !aside) {
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
                // `gh` asks the daemon for the person's token (in a VM: the tools disk's).
                if host.is_some()
                    && !exec.isolated()
                    && github.as_ref().is_some_and(|g| g.app())
                    && let Err(e) = crate::github::install_gh(&dir.join("bin"))
                {
                    eprintln!("{} run {run_id}: no gh proxy: {e}", now_ms());
                }
                host
            };
            // Gated actions: only runtimes that can stop a command get them (the server sends
            // them only for CLIs that report `gates`), never a side chat.
            let gates = Arc::new(crate::gates::Gates::new(
                assign
                    .gates
                    .as_deref()
                    .filter(|_| !aside && crate::gates::enforced_by(&rt.kind))
                    .unwrap_or_default(),
            ));
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
                // first again before each command. (Windows: Git Bash, not done there.)
                if rt.kind == "claude"
                    && cfg!(unix)
                    && !env.iter().any(|(k, _)| k == crate::shim::CLAUDE_ENV_FILE)
                {
                    match crate::shim::claude_env_file(&dir, &bin) {
                        Ok(f) => {
                            env.push((crate::shim::CLAUDE_ENV_FILE.into(), f.display().to_string()))
                        }
                        Err(e) => eprintln!("{} run {run_id}: no Claude env file: {e}", now_ms()),
                    }
                }
                // Codex (and an ACP agent) runs each command in the user's login shell
                // (`zsh -lc`): on macOS path_helper, or a profile, would put the shims last.
                // A zsh of the run reads the user's own files, then puts them first again.
                // (A login bash re-reads the system profile with no such hook: not covered.)
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
                cfg.vm.as_ref().map(vm_note),
                github
                    .as_ref()
                    .filter(|_| !aside)
                    .and_then(|g| crate::github::note(&g.spec)),
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
                gate_hook,
                gate_alive,
                gates: Some(gates.clone()),
                exec: exec.clone(),
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
            if aside {
                workspace::set_read_only(&work, false);
            }
            // Where the code is (design-relay R5), read while the run still holds the line.
            let code = if aside || !has_repo(&assign) {
                None
            } else {
                let fallback = workspace::read_meta(&dir).and_then(|m| m.branch);
                let env = code_git_env(&assign, &work, github.as_deref(), &event).await;
                workspace::with_git_env(env, workspace::code_location(&work, fallback.as_deref()))
                    .await
            };
            // The CLI is done: the next run of this member in this topic may use the line.
            drop(line);
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
}

impl Paused {
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
    let (emit_tx, mut emit_rx) = mpsc::unbounded_channel();
    let (rt_tx, rt_rx) = mpsc::unbounded_channel::<Control>();
    let fut = rt.start(spec, emit_tx, rt_rx);
    tokio::pin!(fut);
    let stall = Duration::from_secs(cfg.stall_minutes.max(1) * 60);
    // Tool calls without a result: the CLI waits for a step it started (a build, a test run),
    // so its silence is not a stall until much later.
    let mut open_tools: std::collections::HashSet<String> = Default::default();
    let mut last = Instant::now();
    let mut stalled = false;
    let mut timed_out = false;
    let mut deadline = assign
        .max_duration_ms
        .map(|ms| tokio::time::Instant::now() + Duration::from_millis(ms));
    const TICK: Duration = Duration::from_secs(15);
    let mut tick = tokio::time::interval(TICK);
    let mut ticked = Instant::now();
    let mut paused: Option<Paused> = None;
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
                    RunCtl::Pause { by } => {
                        // Already stopped (the resume before this pause never came): report the
                        // pause again, so the server counts every pause it sent and a later
                        // `paused: false` clears pausedBy (GAPS SB-16).
                        if paused.is_some() {
                            for ev in gate.push(RunEvent::Paused { paused: true, by }) {
                                event(ev);
                            }
                            continue;
                        }
                        paused = Some(Paused { since: Instant::now(), stopped: exec.stop(&run_dir) });
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
                // The machine was asleep (or the daemon was not run) since the last tick:
                // the CLI did not run either. That time is no silence of the CLI and no part
                // of the run's time limit.
                let now = Instant::now();
                if let Some(asleep) = crate::sys::slept(now.duration_since(ticked), TICK) {
                    (last, deadline) = after_sleep(last, deadline, asleep, now, paused.is_some());
                }
                ticked = now;
                if paused.is_some() {
                    continue;
                }
                // A command waiting for a person is not stuck (the server shows waiting_answer).
                if own.gates.waiting() {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_vm_run_is_told_its_home_size_and_its_sign_in_outlasts_its_limit() {
        let vm = |extra: &str| {
            Config::parse(&format!(
                "server_url = \"http://127.0.0.1:1\"\ntoken_file = \"/nonexistent\"\n[runtimes.claude]\nkind = \"claude\"\n[vm]\nkernel = \"/k\"\nrootfs = \"/r\"\n{extra}"
            ))
            .unwrap()
            .vm
            .unwrap()
        };
        assert!(vm_note(&vm("")).contains("Your home folder holds 20 GB in all."));
        assert!(!vm_note(&vm("home_gb = 0\n")).contains("GB in all"));
        assert!(vm_note(&vm("homes = ''\n")).contains("nothing outside it is kept"));
        let mut a: RunAssign = serde_json::from_value(serde_json::json!({
            "runId": "r1", "botMemberId": "B1", "groupId": "g", "topicId": "t",
            "runtime": "claude", "prompt": "p", "leaseMs": 600000
        }))
        .unwrap();
        assert_eq!(vm_run_need(&a), Duration::from_secs(9000));
        a.max_duration_ms = Some(600_000);
        assert_eq!(vm_run_need(&a), Duration::from_secs(2400));
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

    /// Feature "github": the person has not connected GitHub and the daemon's own clone fails
    /// without credentials: the server is told once, the run reports `missing` and ends with
    /// what the CLI would have said, before any CLI starts.
    #[tokio::test]
    async fn a_clone_that_needs_a_missing_github_ends_the_run_waiting() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", l.local_addr().unwrap());
        let seen = Arc::new(std::sync::Mutex::new(Vec::<String>::new()));
        let s2 = seen.clone();
        tokio::spawn(async move {
            while let Ok((mut s, _)) = l.accept().await {
                let mut buf = vec![0; 4096];
                let n = s.read(&mut buf).await.unwrap_or(0);
                let req = String::from_utf8_lossy(&buf[..n]).to_string();
                let line = req.lines().next().unwrap_or_default().to_string();
                let status = if line.starts_with("GET") {
                    "404 Not Found"
                } else {
                    "200 OK"
                };
                s2.lock().unwrap().push(line);
                let _ = s
                    .write_all(
                        format!("HTTP/1.1 {status}\r\nContent-Length: 2\r\nConnection: close\r\n\r\n{{}}")
                            .as_bytes(),
                    )
                    .await;
            }
        });
        let dir = tempfile::tempdir().unwrap();
        let cfg = Config::parse(&format!(
            "server_url = \"{url}\"\ntoken_file = '{}'\nwork_root = '{}'\n",
            dir.path().join("token").display(),
            dir.path().join("root").display()
        ))
        .unwrap();
        let spec = chat_protocol::github::RunGithub {
            source: "app".into(),
            member_id: "U1".into(),
            login: None,
            author: Some(chat_protocol::github::GitIdentity {
                name: "Ann".into(),
                email: "U1@users.noreply.invalid".into(),
            }),
            committer: chat_protocol::github::GitIdentity {
                name: "Bot".into(),
                email: "ai-B1@users.noreply.invalid".into(),
            },
            rewrite_ssh: Some(true),
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
            gates: None,
            github: Some(spec.clone()),
        };
        let g = crate::github::Run::new(
            &cfg.server_url,
            Arc::new("tm_test_machine_token".into()),
            "rg",
            spec,
            Arc::new(crate::redact::Redactor::default()),
        );
        let events = std::sync::Mutex::new(vec![]);
        let emit = |e: RunEvent| events.lock().unwrap().push(e);
        let err = prepare_with_github(&cfg, &a, Some(&g), &emit)
            .await
            .unwrap_err();
        assert!(err.starts_with("GitHub is not connected for Ann"), "{err}");
        assert_eq!(
            events.into_inner().unwrap(),
            vec![RunEvent::Github {
                state: "missing".into(),
                login: None
            }]
        );
        assert_eq!(
            seen.lock().unwrap().clone(),
            vec![
                "GET /v1/machine/runs/rg/github HTTP/1.1".to_string(),
                "POST /v1/machine/runs/rg/github/wait HTTP/1.1".to_string()
            ]
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
