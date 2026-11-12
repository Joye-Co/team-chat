//! What a run in a microVM gets of this host's CLI sign-ins (机 3.2).
//!
//! The host's sign-in folders (`~/.claude`, `~/.codex`, `~/.pi`) are never shared with a VM.
//! Each run's CLI uses the folders of its topic instead (`<cli root>/.claude`, `.codex`,
//! `.pi/agent`; the cli root is `<line>/cli` for a run in a work line, else `<run>/cli`), so
//! session records go with the topic's work and topics that run at once never share them, and
//! gets the sign-in in a form it cannot use to change this host's:
//!
//! - **Claude Code**: a token in `CLAUDE_CODE_OAUTH_TOKEN`. A long-lived token
//!   (`claude setup-token`) given to the machine as an API key of that name (or the runtime's
//!   own `ANTHROPIC_API_KEY`) is used as it is; otherwise the host's current access token, never
//!   its refresh token. When that runs out before the run could end, the host CLI refreshes it
//!   first.
//! - **Codex**: a copy of `auth.json` without the refresh token, written for the run and removed
//!   when its CLI is done ([`remove_auth`], and [`sweep_auth`] at daemon start) (tested
//!   with Codex 0.159: a copy whose `refresh_token` is empty completes a task with the access
//!   token; a copy without the field is not signed in). Access tokens last 10 days; when the
//!   host's runs out before the run could end, the host's Codex refreshes it first
//!   (`account/read` with `refreshToken`), so refreshing stays on this host.
//! - **Pi**: a copy of `auth.json` with each `refresh` emptied, and its settings. When an OAuth
//!   access token runs out before the run could end, the host's Pi refreshes it first
//!   (`pi auth print-bearer-token --provider <p> --min-expiry <s>s`, its output thrown away); a
//!   run whose every sign-in has run out fails up front.
//!
//! Refreshes on this host take turns (one at a time in the daemon, and each one first looks
//! whether another has just renewed the sign-in), so a refresh token is never used twice.
//!
//! The host's settings (`settings.json`, `config.toml`) are copied for each run. A run that
//! resumes a session its cli root does not have gets that one session file copied in from the
//! first place that has it ([`session_roots`], then the host's own folders).

use crate::config::{Config, RuntimeConfig};
use base64::Engine as _;
use serde_json::{Value, json};
use std::future::Future;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

/// The variable Claude Code takes a token from.
pub const CLAUDE_TOKEN: &str = "CLAUDE_CODE_OAUTH_TOKEN";
/// A run that has one of these already brings its own Claude credential.
const CLAUDE_OWN: [&str; 3] = [CLAUDE_TOKEN, "ANTHROPIC_API_KEY", "ANTHROPIC_AUTH_TOKEN"];

/// The CLIs' folders under a run's cli root, as the run's environment names them.
pub fn cli_env(cli_root: &Path) -> Vec<(String, String)> {
    let p = |rel: &str| cli_root.join(rel).to_string_lossy().to_string();
    vec![
        ("CLAUDE_CONFIG_DIR".into(), p(".claude")),
        ("CODEX_HOME".into(), p(".codex")),
        ("PI_CODING_AGENT_DIR".into(), p(".pi/agent")),
    ]
}

/// A CLI folder of this host: the runtime's env, else the daemon's, else under its home.
fn host_dir(rt: &RuntimeConfig, var: &str, rel: &str) -> PathBuf {
    rt.env
        .get(var)
        .cloned()
        .or_else(|| std::env::var(var).ok().filter(|v| !v.is_empty()))
        .map(PathBuf::from)
        .unwrap_or_else(|| crate::config::home().join(rel))
}

fn now_ms() -> u64 {
    crate::proto::now_ms()
}

/// The `exp` of a JWT, in ms. Only read, never checked: it says when to refresh.
pub fn jwt_exp_ms(token: &str) -> Option<u64> {
    let payload = token.split('.').nth(1)?;
    let bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(payload.trim_end_matches('='))
        .ok()?;
    let v: Value = serde_json::from_slice(&bytes).ok()?;
    v.get("exp")?.as_u64().map(|s| s * 1000)
}

/// When the access token of a Codex `auth.json` runs out (ms); None without one.
pub fn codex_expires_at(text: &str) -> Option<u64> {
    let v: Value = serde_json::from_str(text).ok()?;
    jwt_exp_ms(v.get("tokens")?.get("access_token")?.as_str()?)
}

/// A run's copy of a Codex `auth.json`: the refresh token emptied. Codex reads a file without
/// the field as no sign-in, so the field stays, empty. An API key sign-in is copied as it is.
pub fn codex_run_auth(text: &str) -> Option<String> {
    let mut v: Value = serde_json::from_str(text).ok()?;
    if let Some(t) = v.get_mut("tokens").and_then(Value::as_object_mut)
        && t.contains_key("refresh_token")
    {
        t.insert("refresh_token".into(), json!(""));
    }
    serde_json::to_string(&v).ok()
}

/// The access token of Claude Code's `.credentials.json` and when it runs out (ms).
pub fn claude_access(text: &str) -> Option<(String, Option<u64>)> {
    let v: Value = serde_json::from_str(text).ok()?;
    let o = v.get("claudeAiOauth")?;
    let token = o.get("accessToken")?.as_str()?.trim();
    (!token.is_empty()).then(|| {
        (
            token.to_string(),
            o.get("expiresAt").and_then(Value::as_u64),
        )
    })
}

/// A run's copy of Pi's `auth.json`: every `refresh` emptied.
pub fn pi_run_auth(text: &str) -> Option<String> {
    let mut v: Value = serde_json::from_str(text).ok()?;
    for entry in v.as_object_mut()?.values_mut() {
        if let Some(o) = entry.as_object_mut()
            && o.contains_key("refresh")
        {
            o.insert("refresh".into(), json!(""));
        }
    }
    serde_json::to_string(&v).ok()
}

/// The OAuth sign-ins of Pi's `auth.json` and when each runs out (ms): `(provider, expires)`.
pub fn pi_oauth(text: &str) -> Vec<(String, u64)> {
    let Ok(Value::Object(v)) = serde_json::from_str::<Value>(text) else {
        return vec![];
    };
    v.into_iter()
        .filter(|(_, e)| e.get("type").and_then(Value::as_str) == Some("oauth"))
        .filter_map(|(p, e)| Some((p, e.get("expires")?.as_u64()?)))
        .collect()
}

/// Whether a run can use Pi's `auth.json` at `now`: some sign-in is an API key or has not run
/// out (an empty file is fine: Pi may take a key from the environment).
pub fn pi_usable(text: &str, now: u64) -> bool {
    let Ok(Value::Object(v)) = serde_json::from_str::<Value>(text) else {
        return true;
    };
    v.is_empty()
        || v.values().any(|e| {
            e.get("type").and_then(Value::as_str) != Some("oauth")
                || e.get("expires")
                    .and_then(Value::as_u64)
                    .is_none_or(|x| x > now)
        })
}

/// Refreshes on this host, one at a time.
static HOST_REFRESH: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

/// Refreshes this host's sign-in when `expires` (read again after waiting for its turn) is
/// before `until`. True when it ran `refresh`.
pub async fn renew<F, Fut>(expires: impl Fn() -> Option<u64>, until: u64, refresh: F) -> bool
where
    F: FnOnce() -> Fut,
    Fut: Future<Output = ()>,
{
    let due = || expires().is_some_and(|e| e < until);
    if !due() {
        return false;
    }
    let _turn = HOST_REFRESH.lock().await;
    // Another run may have renewed it while this one waited.
    if !due() {
        return false;
    }
    refresh().await;
    true
}

/// Writes a file only the daemon user can read.
fn write_private(path: &Path, text: &str) -> std::io::Result<()> {
    static N: AtomicU64 = AtomicU64::new(0);
    if let Some(d) = path.parent() {
        std::fs::create_dir_all(d)?;
    }
    // Its own temporary name: two writers never take each other's.
    let tmp = path.with_extension(format!(
        "{}.{}.tmp",
        std::process::id(),
        N.fetch_add(1, Ordering::Relaxed)
    ));
    let _ = std::fs::remove_file(&tmp);
    {
        let mut o = std::fs::OpenOptions::new();
        o.write(true).create_new(true);
        #[cfg(unix)]
        std::os::unix::fs::OpenOptionsExt::mode(&mut o, 0o600);
        let mut f = o.open(&tmp)?;
        std::io::Write::write_all(&mut f, text.as_bytes())?;
    }
    std::fs::rename(&tmp, path)
}

/// Copies `name` from `from` to `to` when the host has it (the host's settings win).
fn copy_setting(from: &Path, to: &Path, name: &str) {
    let src = from.join(name);
    if src.is_file() {
        let _ = std::fs::create_dir_all(to);
        let _ = std::fs::copy(&src, to.join(name));
    }
}

/// The cli roots that may hold a member's earlier sessions, in the order they are searched
/// after the run's own `cli_root`: the member's other work lines (`lines/*/cli`, by
/// `line.json`), its runs without a line (`runs/*/cli`, by `run.json`: side chats and runs
/// whose line was held), then the folders of the daemon versions that kept per-member homes
/// (`<[vm].homes>/<member>`, `<[vm].dir>/cli/<member>`).
pub fn session_roots(cfg: &Config, cli_root: &Path, member: &str) -> Vec<PathBuf> {
    let mut out = vec![];
    if member.is_empty() {
        return out;
    }
    let mut push = |p: PathBuf| {
        if p != cli_root && p.is_dir() && !out.contains(&p) {
            out.push(p);
        }
    };
    let lines = std::fs::read_dir(crate::workspace::lines_dir(cfg));
    for e in lines.into_iter().flatten().flatten() {
        if crate::workspace::read_line(&e.path()).is_some_and(|l| l.bot_member_id == member) {
            push(e.path().join("cli"));
        }
    }
    let runs = std::fs::read_dir(crate::workspace::runs_dir(cfg));
    for e in runs.into_iter().flatten().flatten() {
        if crate::workspace::read_meta(&e.path())
            .is_some_and(|m| m.bot_member_id.as_deref() == Some(member))
        {
            push(e.path().join("cli"));
        }
    }
    if let Some(vm) = &cfg.vm
        && crate::vm_cache::plain_id(member)
    {
        if !vm.homes.as_os_str().is_empty() {
            push(vm.homes.join(member));
        }
        push(vm.dir.join("cli").join(member));
    }
    out
}

/// Copies Claude Code session `<sid>.jsonl` into `own` (a run's Claude folder) from the first
/// folder of `from` that has it, under the same project folder. True when it is in `own` now.
pub fn carry_claude_session(from: &[PathBuf], own: &Path, cwd: &Path, sid: &str) -> bool {
    use crate::runtime::claude::find_session_file;
    if !crate::runtime::valid_session_id(sid) {
        return false;
    }
    if find_session_file(own, cwd, sid).is_some() {
        return true;
    }
    let Some(found) = from.iter().find_map(|d| find_session_file(d, cwd, sid)) else {
        return false;
    };
    let Some(project) = found.parent().and_then(|p| p.file_name()) else {
        return false;
    };
    let dir = own.join("projects").join(project);
    std::fs::create_dir_all(&dir).is_ok()
        && std::fs::copy(&found, dir.join(format!("{sid}.jsonl"))).is_ok()
}

/// A session file under `sessions/` (a few folders deep) whose name `matches`.
fn find_in_sessions(home: &Path, matches: &dyn Fn(&str) -> bool) -> Option<PathBuf> {
    fn walk(dir: &Path, matches: &dyn Fn(&str) -> bool, depth: u32) -> Option<PathBuf> {
        for e in std::fs::read_dir(dir).ok()?.flatten() {
            let p = e.path();
            if p.is_dir() {
                if depth < 4
                    && let Some(f) = walk(&p, matches, depth + 1)
                {
                    return Some(f);
                }
            } else if e.file_name().to_str().is_some_and(matches) {
                return Some(p);
            }
        }
        None
    }
    walk(&home.join("sessions"), matches, 0)
}

/// A Codex thread's rollout file under `sessions/` (`YYYY/MM/DD/rollout-<time>-<id>.jsonl`).
fn find_rollout(home: &Path, sid: &str) -> Option<PathBuf> {
    let suffix = format!("-{sid}.jsonl");
    find_in_sessions(home, &|n| n.starts_with("rollout-") && n.ends_with(&suffix))
}

/// A Pi session file under `sessions/` (`<cwd folder>/<time>_<id>.jsonl`).
fn find_pi_session(home: &Path, sid: &str) -> Option<PathBuf> {
    let (under, plain) = (format!("_{sid}.jsonl"), format!("{sid}.jsonl"));
    find_in_sessions(home, &|n| n.ends_with(&under) || n == plain)
}

/// Copies the session file `find` finds in the first folder of `from` into `own`, at the same
/// place under it. True when `own` has it now.
fn carry_session(
    from: &[PathBuf],
    own: &Path,
    sid: &str,
    find: fn(&Path, &str) -> Option<PathBuf>,
) -> bool {
    if !crate::runtime::valid_session_id(sid) {
        return false;
    }
    if find(own, sid).is_some() {
        return true;
    }
    let Some((root, found)) = from.iter().find_map(|d| Some((d, find(d, sid)?))) else {
        return false;
    };
    let Ok(rel) = found.strip_prefix(root) else {
        return false;
    };
    let target = own.join(rel);
    target
        .parent()
        .is_some_and(|d| std::fs::create_dir_all(d).is_ok())
        && std::fs::copy(&found, &target).is_ok()
}

/// [`carry_claude_session`] for a Codex thread: the rollout file keeps its place under
/// `sessions/`.
pub fn carry_codex_session(from: &[PathBuf], own: &Path, sid: &str) -> bool {
    carry_session(from, own, sid, find_rollout)
}

/// [`carry_claude_session`] for a Pi session: the file keeps its place under `sessions/`.
pub fn carry_pi_session(from: &[PathBuf], own: &Path, sid: &str) -> bool {
    carry_session(from, own, sid, find_pi_session)
}

/// The sign-in copies a run's CLI reads under its cli root.
const AUTH_COPIES: [&str; 2] = [".codex/auth.json", ".pi/agent/auth.json"];

/// Removes the sign-in copies under a run's cli root once its CLI is done. True when one was
/// there.
pub fn remove_auth(cli_root: &Path) -> bool {
    // Every copy goes: no short cut after the first.
    AUTH_COPIES
        .iter()
        .filter(|f| std::fs::remove_file(cli_root.join(f)).is_ok())
        .count()
        > 0
}

/// At daemon start: removes the sign-in copies a crash left in the cli roots of the work lines
/// and runs that no `active` run uses. Returns the cli roots that had one.
pub fn sweep_auth(cfg: &Config, active: &[String]) -> usize {
    use crate::workspace::{lines_dir, read_meta, run_dir, runs_dir};
    let in_use: Vec<PathBuf> = active
        .iter()
        .flat_map(|id| {
            let dir = run_dir(cfg, id);
            let line = read_meta(&dir).and_then(|m| m.line);
            [Some(dir), line].into_iter().flatten()
        })
        .collect();
    let mut removed = 0;
    for parent in [lines_dir(cfg), runs_dir(cfg)] {
        for e in std::fs::read_dir(parent).into_iter().flatten().flatten() {
            if !in_use.contains(&e.path()) && remove_auth(&e.path().join("cli")) {
                removed += 1;
            }
        }
    }
    removed
}

/// Readies the CLI's folders and sign-in of a VM run whose CLI folders are under `cli_root`:
/// sets the CLIs' folders in `env` and, for Claude Code, the token. A resumed session the cli
/// root lacks is copied in from the member's (`member_id`) other folders ([`session_roots`]) or
/// the host's. Returns the token the run got in its environment (to be masked), if any.
/// `need`: how long the run may take. The caller removes the sign-in copies with
/// [`remove_auth`] when the CLI is done.
#[allow(clippy::too_many_arguments)]
pub async fn prepare(
    rt: &RuntimeConfig,
    cli_root: &Path,
    cwd: &Path,
    resume: Option<&str>,
    need: Duration,
    env: &mut Vec<(String, String)>,
    member_id: &str,
    cfg: &Config,
) -> Result<Option<String>, String> {
    for (k, v) in cli_env(cli_root) {
        env.retain(|(n, _)| *n != k);
        env.push((k, v));
    }
    std::fs::create_dir_all(cli_root).map_err(|e| format!("{}: {e}", cli_root.display()))?;
    // Runs that start together never write one folder at once.
    let _turn = crate::vm_cache::lock(&format!("login:{}", cli_root.display())).await;
    // Where a resumed session may be: the member's other folders, then the host's.
    let from = |rel: &str, host: &Path| {
        session_roots(cfg, cli_root, member_id)
            .into_iter()
            .map(|r| r.join(rel))
            .chain([host.to_path_buf()])
            .collect::<Vec<_>>()
    };
    let until = now_ms() + need.as_millis() as u64;
    match rt.kind.as_str() {
        "claude" => {
            let host = host_dir(rt, "CLAUDE_CONFIG_DIR", ".claude");
            let own = cli_root.join(".claude");
            std::fs::create_dir_all(&own).map_err(|e| format!("{}: {e}", own.display()))?;
            copy_setting(&host, &own, "settings.json");
            if let Some(sid) = resume {
                carry_claude_session(&from(".claude", &host), &own, cwd, sid);
            }
            if env
                .iter()
                .any(|(k, v)| CLAUDE_OWN.contains(&k.as_str()) && !v.is_empty())
            {
                return Ok(None);
            }
            let read = || {
                std::fs::read_to_string(host.join(".credentials.json"))
                    .ok()
                    .and_then(|t| claude_access(&t))
            };
            // Claude Code refreshes its sign-in when it talks to the service.
            renew(
                || read().and_then(|(_, e)| e),
                until,
                || async {
                    crate::usage::read_all(vec![("claude".into(), rt.clone())]).await;
                },
            )
            .await;
            Ok(read().map(|(token, _)| {
                env.push((CLAUDE_TOKEN.into(), token.clone()));
                token
            }))
        }
        "codex" => {
            let host = host_dir(rt, "CODEX_HOME", ".codex");
            let own = cli_root.join(".codex");
            std::fs::create_dir_all(&own).map_err(|e| format!("{}: {e}", own.display()))?;
            copy_setting(&host, &own, "config.toml");
            if let Some(sid) = resume {
                carry_codex_session(&from(".codex", &host), &own, sid);
            }
            let read = || std::fs::read_to_string(host.join("auth.json")).ok();
            renew(
                || read().as_deref().and_then(codex_expires_at),
                until,
                || async {
                    refresh_codex(rt).await;
                },
            )
            .await;
            let text = read();
            let target = own.join("auth.json");
            match text.as_deref().and_then(codex_run_auth) {
                Some(copy) => write_private(&target, &copy)
                    .map_err(|e| format!("{}: {e}", target.display()))?,
                None => {
                    let _ = std::fs::remove_file(&target);
                }
            }
            Ok(None)
        }
        _ => {
            // Pi (an ACP or terminal runtime): its sign-in and settings, when the host has it.
            let host = host_dir(rt, "PI_CODING_AGENT_DIR", ".pi/agent");
            if host.is_dir() {
                let own = cli_root.join(".pi/agent");
                copy_setting(&host, &own, "settings.json");
                copy_setting(&host, &own, "models.json");
                if let Some(sid) = resume {
                    carry_pi_session(&from(".pi/agent", &host), &own, sid);
                }
                let read = || std::fs::read_to_string(host.join("auth.json")).ok();
                renew(
                    || {
                        read()
                            .map(|t| pi_oauth(&t))
                            .and_then(|o| o.iter().map(|(_, e)| *e).min())
                    },
                    until,
                    || async {
                        let due: Vec<String> = read()
                            .map(|t| pi_oauth(&t))
                            .unwrap_or_default()
                            .into_iter()
                            .filter(|(_, e)| *e < until)
                            .map(|(p, _)| p)
                            .collect();
                        let secs = until.saturating_sub(now_ms()) / 1000 + 1;
                        for p in due {
                            refresh_pi(rt, &host, &p, secs).await;
                        }
                    },
                )
                .await;
                let text = read();
                if text.as_deref().is_some_and(|t| !pi_usable(t, now_ms())) {
                    return Err(
                        "Pi's sign-in on this computer has run out and could not be \
renewed: sign in to Pi on it again, or give the machine an API key"
                            .into(),
                    );
                }
                if let Some(copy) = text.as_deref().and_then(pi_run_auth) {
                    let target = own.join("auth.json");
                    write_private(&target, &copy)
                        .map_err(|e| format!("{}: {e}", target.display()))?;
                }
            }
            Ok(None)
        }
    }
}

/// Lets this host's Pi refresh one provider's sign-in: it asks for a token that lasts `secs`
/// more seconds and writes the renewed sign-in back to its `auth.json`. The token it prints is
/// thrown away.
async fn refresh_pi(rt: &RuntimeConfig, host: &Path, provider: &str, secs: u64) -> bool {
    let Some(program) = crate::health::which("pi") else {
        return false;
    };
    let mut cmd = crate::health::command(&program);
    cmd.args(["auth", "print-bearer-token", "--provider", provider])
        .arg("--min-expiry")
        .arg(format!("{secs}s"))
        .envs(&rt.env)
        .env("PI_CODING_AGENT_DIR", host)
        .current_dir(crate::config::home())
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .kill_on_drop(true);
    let ok = tokio::time::timeout(Duration::from_secs(60), crate::health::output(&mut cmd))
        .await
        .ok()
        .and_then(Result::ok)
        .is_some_and(|o| o.status.success());
    if !ok {
        eprintln!(
            "{} pi: the sign-in of {} could not be refreshed",
            now_ms(),
            crate::chat_run::safe_word(provider)
        );
    }
    ok
}

/// What to do after a line of `codex app-server` in [`refresh_codex`].
#[derive(Debug, PartialEq)]
pub enum RefreshStep {
    Send(Vec<Value>),
    Wait,
    Done(bool),
}

/// `initialize`, `initialized`, then `account/read` with `refreshToken`: Codex refreshes its
/// sign-in the way it always does and writes it back to its `auth.json`.
#[derive(Debug, Default)]
pub struct CodexRefresh {
    initialized: bool,
}

impl CodexRefresh {
    pub fn start(&self) -> Value {
        json!({
            "id": 1,
            "method": "initialize",
            "params": {
                "clientInfo": { "name": "machine-daemon", "title": null, "version": env!("CARGO_PKG_VERSION") },
                "capabilities": null
            }
        })
    }

    pub fn on_line(&mut self, line: &str) -> RefreshStep {
        let Ok(v) = serde_json::from_str::<Value>(line) else {
            return RefreshStep::Wait;
        };
        let want = if self.initialized { 2 } else { 1 };
        if v.get("id").and_then(Value::as_u64) != Some(want) || v.get("method").is_some() {
            return RefreshStep::Wait;
        }
        if v.get("error").is_some() {
            return RefreshStep::Done(false);
        }
        if self.initialized {
            return RefreshStep::Done(true);
        }
        self.initialized = true;
        RefreshStep::Send(vec![
            json!({ "method": "initialized" }),
            json!({ "id": 2, "method": "account/read", "params": { "refreshToken": true } }),
        ])
    }
}

/// Lets this host's Codex refresh its sign-in (a short `codex app-server` session).
async fn refresh_codex(rt: &RuntimeConfig) -> bool {
    let Some(program) = crate::health::which(&rt.command()) else {
        return false;
    };
    let mut cmd = crate::health::command(&program);
    cmd.arg("app-server")
        .args(&rt.args)
        .envs(&rt.env)
        .current_dir(crate::config::home())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true);
    let Ok(mut child) = crate::health::spawn(&mut cmd) else {
        return false;
    };
    let (Some(mut stdin), Some(stdout)) = (child.stdin.take(), child.stdout.take()) else {
        return false;
    };
    let session = async {
        let mut lines = BufReader::new(stdout).lines();
        let mut s = CodexRefresh::default();
        let mut send = vec![s.start()];
        loop {
            for m in send.drain(..) {
                stdin.write_all(format!("{m}\n").as_bytes()).await.ok()?;
            }
            stdin.flush().await.ok()?;
            loop {
                let line = lines.next_line().await.ok()??;
                match s.on_line(&line) {
                    RefreshStep::Wait => continue,
                    RefreshStep::Send(m) => {
                        send = m;
                        break;
                    }
                    RefreshStep::Done(ok) => return Some(ok),
                }
            }
        }
    };
    let ok = tokio::time::timeout(Duration::from_secs(60), session)
        .await
        .ok()
        .flatten()
        .unwrap_or(false);
    let _ = child.start_kill();
    let _ = tokio::time::timeout(Duration::from_secs(2), child.wait()).await;
    if !ok {
        eprintln!("{} codex: the sign-in could not be refreshed", now_ms());
    }
    ok
}

#[cfg(test)]
mod tests {
    use super::*;

    fn jwt(exp_secs: u64) -> String {
        let e = |v: Value| {
            base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(v.to_string().as_bytes())
        };
        format!(
            "{}.{}.sig",
            e(json!({"alg": "none"})),
            e(json!({"exp": exp_secs}))
        )
    }

    /// A VM host's config with its work root, old homes and VM folder under `root`.
    fn vm_host(root: &Path) -> Config {
        Config::parse(&format!(
            "server_url = \"http://127.0.0.1:1\"\ntoken_file = \"/nonexistent\"\nwork_root = '{}'\n[runtimes.claude]\nkind = \"claude\"\n[vm]\nkernel = \"/k\"\nrootfs = \"/r\"\nhomes = '{}'\ndir = '{}'\n",
            root.join("w").display(),
            root.join("homes").display(),
            root.join("vms").display()
        ))
        .unwrap()
    }

    fn rt(kind: &str, env: &[(&str, &Path)]) -> RuntimeConfig {
        let mut c = crate::config::Config::parse(&format!(
            "server_url = \"http://127.0.0.1:1\"\ntoken_file = \"/nonexistent\"\n[runtimes.x]\nkind = \"{kind}\"\ncommand = \"/nonexistent/cli\"\n"
        ))
        .unwrap();
        let mut r = c.runtimes.remove("x").unwrap();
        for (k, v) in env {
            r.env.insert(k.to_string(), v.display().to_string());
        }
        r
    }

    #[test]
    fn a_codex_copy_has_an_empty_refresh_token_and_keeps_the_rest() {
        let host = json!({
            "auth_mode": "chatgpt",
            "OPENAI_API_KEY": null,
            "last_refresh": "2026-11-09T09:18:18Z",
            "tokens": { "access_token": jwt(2_000_000_000), "id_token": "i", "account_id": "a", "refresh_token": "SECRET-REFRESH" }
        });
        let copy: Value =
            serde_json::from_str(&codex_run_auth(&host.to_string()).unwrap()).unwrap();
        assert_eq!(copy["tokens"]["refresh_token"], "");
        assert_eq!(
            copy["tokens"]["access_token"],
            host["tokens"]["access_token"]
        );
        assert_eq!(copy["tokens"]["account_id"], "a");
        assert_eq!(copy["last_refresh"], host["last_refresh"]);
        assert!(
            !codex_run_auth(&host.to_string())
                .unwrap()
                .contains("SECRET-REFRESH")
        );
        assert_eq!(codex_expires_at(&host.to_string()), Some(2_000_000_000_000));
        // An API key sign-in has no tokens: copied as it is.
        let key = json!({ "OPENAI_API_KEY": "k" }).to_string();
        assert_eq!(codex_run_auth(&key).unwrap(), key);
        assert!(codex_run_auth("not json").is_none());
    }

    #[test]
    fn claude_gives_its_access_token_never_the_refresh_token() {
        let creds = json!({ "claudeAiOauth": { "accessToken": "sk-ant-oat01-a", "refreshToken": "sk-ant-ort01-r", "expiresAt": 1_800_000_000_000u64 } });
        assert_eq!(
            claude_access(&creds.to_string()),
            Some(("sk-ant-oat01-a".into(), Some(1_800_000_000_000)))
        );
        assert!(claude_access(r#"{"claudeAiOauth":{"accessToken":""}}"#).is_none());
        assert!(claude_access("{}").is_none());
    }

    #[test]
    fn a_pi_copy_has_no_refresh_tokens() {
        let host = json!({ "anthropic": { "type": "oauth", "refresh": "R", "access": "A", "expires": 1 }, "openai": { "type": "api_key", "key": "K" } });
        let copy: Value = serde_json::from_str(&pi_run_auth(&host.to_string()).unwrap()).unwrap();
        assert_eq!(copy["anthropic"]["refresh"], "");
        assert_eq!(copy["anthropic"]["access"], "A");
        assert_eq!(copy["openai"]["key"], "K");
    }

    #[test]
    fn pi_says_which_sign_ins_run_out_and_whether_any_is_usable() {
        let t = json!({ "anthropic": { "type": "oauth", "refresh": "R", "access": "A", "expires": 5 }, "openai": { "type": "api_key", "key": "K" } }).to_string();
        assert_eq!(pi_oauth(&t), vec![("anthropic".to_string(), 5)]);
        // An API key is always usable.
        assert!(pi_usable(&t, 10));
        let only =
            json!({ "anthropic": { "type": "oauth", "access": "A", "expires": 5 } }).to_string();
        assert!(pi_usable(&only, 4));
        assert!(!pi_usable(&only, 6));
        assert!(pi_usable("{}", 6));
        assert!(pi_oauth("not json").is_empty());
    }

    #[tokio::test]
    async fn host_refreshes_take_turns_and_never_repeat() {
        use std::sync::Arc;
        use std::sync::atomic::AtomicUsize;
        // The host's token runs out in 10 s; eight runs that need an hour start together.
        let expires = Arc::new(AtomicU64::new(now_ms() + 10_000));
        let refreshes = Arc::new(AtomicUsize::new(0));
        let until = now_ms() + 3_600_000;
        let runs = (0..8).map(|_| {
            let (expires, refreshes) = (expires.clone(), refreshes.clone());
            tokio::spawn(async move {
                let read = expires.clone();
                renew(
                    move || Some(read.load(Ordering::SeqCst)),
                    until,
                    || async move {
                        refreshes.fetch_add(1, Ordering::SeqCst);
                        tokio::time::sleep(Duration::from_millis(20)).await;
                        expires.store(now_ms() + 86_400_000, Ordering::SeqCst);
                    },
                )
                .await
            })
        });
        let ran: Vec<bool> = futures_join(runs).await;
        assert_eq!(refreshes.load(Ordering::SeqCst), 1);
        assert_eq!(ran.iter().filter(|r| **r).count(), 1);
        // A sign-in that lasts is never refreshed.
        assert!(!renew(|| Some(u64::MAX), until, || async { unreachable!() }).await);
        assert!(!renew(|| None, until, || async { unreachable!() }).await);
    }

    async fn futures_join<T: Send + 'static>(
        tasks: impl Iterator<Item = tokio::task::JoinHandle<T>>,
    ) -> Vec<T> {
        let mut out = vec![];
        for t in tasks.collect::<Vec<_>>() {
            out.push(t.await.unwrap());
        }
        out
    }

    #[test]
    fn writers_of_one_sign_in_copy_never_take_each_others_file() {
        let tmp = tempfile::tempdir().unwrap();
        let target = tmp.path().join("m/.codex/auth.json");
        std::thread::scope(|s| {
            for i in 0..16 {
                let target = &target;
                s.spawn(move || write_private(target, &format!("{{\"n\":{i}}}")).unwrap());
            }
        });
        let v: Value = serde_json::from_str(&std::fs::read_to_string(&target).unwrap()).unwrap();
        assert!(v["n"].is_u64());
        // No temporary file is left.
        assert_eq!(
            std::fs::read_dir(target.parent().unwrap()).unwrap().count(),
            1
        );
    }

    #[tokio::test]
    async fn a_pi_run_fails_up_front_when_every_sign_in_ran_out() {
        let tmp = tempfile::tempdir().unwrap();
        let host = tmp.path().join("host-pi");
        std::fs::create_dir_all(&host).unwrap();
        let member = tmp.path().join("w/lines/l1/cli");
        let r = rt("acp", &[("PI_CODING_AGENT_DIR", &host)]);
        let cfg = vm_host(tmp.path());
        let run = |env: &mut Vec<(String, String)>| {
            let (r, member, cwd) = (r.clone(), member.clone(), tmp.path().to_path_buf());
            let cfg = cfg.clone();
            let mut e = std::mem::take(env);
            async move {
                let need = Duration::from_secs(9000);
                prepare(&r, &member, &cwd, None, need, &mut e, "01BOT", &cfg).await
            }
        };
        // A sign-in that lasts: copied without its refresh token, nothing refreshed.
        let fresh = json!({ "none-such": { "type": "oauth", "refresh": "SECRET-R", "access": "A", "expires": now_ms() + 86_400_000 } });
        std::fs::write(host.join("auth.json"), fresh.to_string()).unwrap();
        run(&mut vec![]).await.unwrap();
        let copy = std::fs::read_to_string(member.join(".pi/agent/auth.json")).unwrap();
        assert!(copy.contains("\"access\":\"A\"") && !copy.contains("SECRET-R"));
        // Run out, and this host cannot renew it (no Pi, or a provider it does not know).
        let gone = json!({ "none-such": { "type": "oauth", "refresh": "SECRET-R", "access": "A", "expires": 1 } });
        std::fs::write(host.join("auth.json"), gone.to_string()).unwrap();
        let e = run(&mut vec![]).await.unwrap_err();
        assert!(e.contains("Pi"), "{e}");
    }

    #[test]
    fn a_resumed_session_from_the_shared_folder_is_copied_once() {
        let tmp = tempfile::tempdir().unwrap();
        let (host, member) = (tmp.path().join("host"), tmp.path().join("member"));
        let cwd = tmp.path().join("work");
        std::fs::create_dir_all(&cwd).unwrap();
        let old = host.join("projects").join("-old-work");
        std::fs::create_dir_all(&old).unwrap();
        std::fs::write(old.join("s1.jsonl"), "{}\n").unwrap();
        std::fs::write(old.join("other.jsonl"), "{}\n").unwrap();
        let from = [tmp.path().join("empty"), host.clone()];
        assert!(carry_claude_session(&from, &member, &cwd, "s1"));
        assert!(member.join("projects/-old-work/s1.jsonl").is_file());
        // Only the session the run resumes: no other member's records.
        assert!(!member.join("projects/-old-work/other.jsonl").exists());
        assert!(!carry_claude_session(&from, &member, &cwd, "missing"));
        assert!(!carry_claude_session(&from, &member, &cwd, "../x"));

        let rollout = host.join("sessions/2026/11/09");
        std::fs::create_dir_all(&rollout).unwrap();
        std::fs::write(rollout.join("rollout-2026-11-09T10-00-00-t1.jsonl"), "{}\n").unwrap();
        assert!(carry_codex_session(&from, &member, "t1"));
        assert!(
            member
                .join("sessions/2026/11/09/rollout-2026-11-09T10-00-00-t1.jsonl")
                .is_file()
        );
        assert!(!carry_codex_session(&from, &member, "t2"));

        let pi = host.join("sessions/--work--");
        std::fs::create_dir_all(&pi).unwrap();
        std::fs::write(pi.join("2026-11-09T10-00-00-000Z_p1.jsonl"), "{}\n").unwrap();
        assert!(carry_pi_session(&from, &member, "p1"));
        assert!(
            member
                .join("sessions/--work--/2026-11-09T10-00-00-000Z_p1.jsonl")
                .is_file()
        );
        assert!(!carry_pi_session(&from, &member, "p2"));
    }

    #[tokio::test]
    async fn a_codex_run_gets_its_members_folders_and_a_copy_of_the_sign_in() {
        let tmp = tempfile::tempdir().unwrap();
        let host = tmp.path().join("host-codex");
        std::fs::create_dir_all(&host).unwrap();
        let far = (now_ms() / 1000) + 10 * 86_400;
        let auth = json!({ "auth_mode": "chatgpt", "tokens": { "access_token": jwt(far), "refresh_token": "SECRET-REFRESH" } });
        std::fs::write(host.join("auth.json"), auth.to_string()).unwrap();
        std::fs::write(host.join("config.toml"), "model = \"m\"\n").unwrap();
        let member = tmp.path().join("w/lines/l1/cli");
        let mut env = vec![("CODEX_HOME".to_string(), host.display().to_string())];
        let r = rt("codex", &[("CODEX_HOME", &host)]);
        let cfg = vm_host(tmp.path());
        let got = prepare(
            &r,
            &member,
            tmp.path(),
            None,
            Duration::from_secs(9000),
            &mut env,
            "01BOT",
            &cfg,
        )
        .await
        .unwrap();
        assert!(got.is_none());
        let get = |k: &str| env.iter().find(|(n, _)| n == k).map(|(_, v)| v.clone());
        assert_eq!(
            get("CODEX_HOME"),
            Some(member.join(".codex").display().to_string())
        );
        assert_eq!(
            get("CLAUDE_CONFIG_DIR"),
            Some(member.join(".claude").display().to_string())
        );
        assert_eq!(env.iter().filter(|(k, _)| k == "CODEX_HOME").count(), 1);
        let copy = std::fs::read_to_string(member.join(".codex/auth.json")).unwrap();
        assert!(!copy.contains("SECRET-REFRESH"));
        assert!(member.join(".codex/config.toml").is_file());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(member.join(".codex/auth.json"))
                .unwrap()
                .permissions()
                .mode();
            assert_eq!(mode & 0o777, 0o600);
        }
        // The host's file is untouched.
        assert!(
            std::fs::read_to_string(host.join("auth.json"))
                .unwrap()
                .contains("SECRET-REFRESH")
        );
        // When the CLI is done the copy goes; the settings stay.
        assert!(remove_auth(&member));
        assert!(!member.join(".codex/auth.json").exists());
        assert!(member.join(".codex/config.toml").is_file());
        assert!(!remove_auth(&member));
    }

    #[tokio::test]
    async fn a_claude_run_gets_the_hosts_access_token_unless_it_brings_its_own() {
        let tmp = tempfile::tempdir().unwrap();
        let host = tmp.path().join("host-claude");
        std::fs::create_dir_all(&host).unwrap();
        let creds = json!({ "claudeAiOauth": { "accessToken": "sk-ant-oat01-host", "refreshToken": "sk-ant-ort01-r", "expiresAt": now_ms() + 86_400_000 } });
        std::fs::write(host.join(".credentials.json"), creds.to_string()).unwrap();
        let member = tmp.path().join("w/lines/l1/cli");
        let r = rt("claude", &[("CLAUDE_CONFIG_DIR", &host)]);
        let cfg = vm_host(tmp.path());
        let mut env = vec![];
        let got = prepare(
            &r,
            &member,
            tmp.path(),
            None,
            Duration::from_secs(9000),
            &mut env,
            "01BOT",
            &cfg,
        )
        .await
        .unwrap();
        assert_eq!(got.as_deref(), Some("sk-ant-oat01-host"));
        assert!(env.contains(&(CLAUDE_TOKEN.to_string(), "sk-ant-oat01-host".to_string())));
        // Nothing of the sign-in is in the member's folder.
        assert!(!member.join(".claude/.credentials.json").exists());
        // A long-lived token given as an API key wins.
        let mut env = vec![(CLAUDE_TOKEN.to_string(), "sk-ant-oat01-long".to_string())];
        let got = prepare(
            &r,
            &member,
            tmp.path(),
            None,
            Duration::from_secs(9000),
            &mut env,
            "01BOT",
            &cfg,
        )
        .await
        .unwrap();
        assert!(got.is_none());
        assert_eq!(env.iter().filter(|(k, _)| k == CLAUDE_TOKEN).count(), 1);
    }

    #[test]
    fn the_refresh_session_asks_codex_to_refresh() {
        let mut s = CodexRefresh::default();
        assert_eq!(s.start()["method"], "initialize");
        assert_eq!(
            s.on_line(r#"{"method":"note","params":{}}"#),
            RefreshStep::Wait
        );
        let RefreshStep::Send(m) = s.on_line(r#"{"id":1,"result":{}}"#) else {
            panic!("expected the account/read request");
        };
        assert_eq!(m[0]["method"], "initialized");
        assert_eq!(m[1]["method"], "account/read");
        assert_eq!(m[1]["params"]["refreshToken"], true);
        assert_eq!(
            s.on_line(r#"{"id":2,"result":{"account":null}}"#),
            RefreshStep::Done(true)
        );
        let mut s = CodexRefresh::default();
        s.on_line(r#"{"id":1,"result":{}}"#);
        assert_eq!(
            s.on_line(r#"{"id":2,"error":{"code":-1}}"#),
            RefreshStep::Done(false)
        );
    }

    /// Two topics of one member that run at once each get their own CLI folders and sign-in
    /// copy: the folders of their work lines.
    #[tokio::test]
    async fn two_topics_of_one_member_at_once_get_their_own_cli_folders() {
        let tmp = tempfile::tempdir().unwrap();
        let cfg = vm_host(tmp.path());
        let host = tmp.path().join("host-codex");
        std::fs::create_dir_all(&host).unwrap();
        let far = (now_ms() / 1000) + 10 * 86_400;
        let auth = json!({ "auth_mode": "chatgpt", "tokens": { "access_token": jwt(far), "refresh_token": "R" } });
        std::fs::write(host.join("auth.json"), auth.to_string()).unwrap();
        let r = rt("codex", &[("CODEX_HOME", &host)]);
        let (cwd1, _, l1) = crate::workspace::prepare_line(&cfg, "r1", "01BOT", "t1", None)
            .await
            .unwrap()
            .unwrap();
        let (cwd2, _, l2) = crate::workspace::prepare_line(&cfg, "r2", "01BOT", "t2", None)
            .await
            .unwrap()
            .unwrap();
        let (root1, root2) = (l1.dir().join("cli"), l2.dir().join("cli"));
        assert_ne!(root1, root2);
        let need = Duration::from_secs(9000);
        let (mut e1, mut e2) = (vec![], vec![]);
        let (a, b) = tokio::join!(
            prepare(&r, &root1, &cwd1, None, need, &mut e1, "01BOT", &cfg),
            prepare(&r, &root2, &cwd2, None, need, &mut e2, "01BOT", &cfg),
        );
        a.unwrap();
        b.unwrap();
        let home =
            |e: &[(String, String)]| e.iter().find(|(k, _)| k == "CODEX_HOME").unwrap().1.clone();
        assert_eq!(home(&e1), root1.join(".codex").display().to_string());
        assert_eq!(home(&e2), root2.join(".codex").display().to_string());
        assert!(
            root1.join(".codex/auth.json").is_file() && root2.join(".codex/auth.json").is_file()
        );
        // The first run ends: only its copy goes.
        remove_auth(&root1);
        assert!(!root1.join(".codex/auth.json").exists());
        assert!(root2.join(".codex/auth.json").is_file());
    }

    /// A resumed session is found in the run's own folder, the member's other work lines, its
    /// runs without a line, the folders of the per-member homes, then the host's folder; never
    /// in another member's.
    #[tokio::test]
    async fn a_resumed_session_is_found_in_each_place_it_may_be() {
        let tmp = tempfile::tempdir().unwrap();
        let cfg = vm_host(tmp.path());
        let w = tmp.path().join("w");
        let cwd = tmp.path().join("work");
        std::fs::create_dir_all(&cwd).unwrap();
        let host = tmp.path().join("host-claude");
        std::fs::create_dir_all(&host).unwrap();
        let mine = w.join("lines/mine/cli");
        let line = |key: &str, member: &str| {
            let d = w.join("lines").join(key);
            std::fs::create_dir_all(d.join("cli")).unwrap();
            let meta = json!({ "key": key, "botMemberId": member, "topicId": "t", "createdAt": 0, "lastUsedAt": 0 });
            std::fs::write(d.join("line.json"), meta.to_string()).unwrap();
            d.join("cli")
        };
        let run = |id: &str, member: &str| {
            let d = w.join("runs").join(id);
            std::fs::create_dir_all(d.join("cli")).unwrap();
            let meta = json!({ "runId": id, "startedAt": 0, "botMemberId": member });
            std::fs::write(d.join("run.json"), meta.to_string()).unwrap();
            d.join("cli")
        };
        line("mine", "01BOT");
        let places = [
            ("own", mine.clone()),
            ("line", line("other", "01BOT")),
            ("run", run("r9", "01BOT")),
            ("home", tmp.path().join("homes/01BOT")),
            ("vmcli", tmp.path().join("vms/cli/01BOT")),
        ];
        let put_claude = |root: &Path, sid: &str| {
            let d = root.join("projects/-somewhere");
            std::fs::create_dir_all(&d).unwrap();
            std::fs::write(d.join(format!("{sid}.jsonl")), "{}\n").unwrap();
        };
        for (sid, root) in &places {
            put_claude(&root.join(".claude"), &format!("c-{sid}"));
        }
        // The host's own folder is the last place; another member's line is never searched.
        put_claude(&host, "c-host");
        put_claude(&line("theirs", "01OTHER").join(".claude"), "c-theirs");
        put_claude(&run("r8", "01OTHER").join(".claude"), "c-theirs-run");
        let r = rt("claude", &[("CLAUDE_CONFIG_DIR", &host)]);
        let need = Duration::from_secs(9000);
        for sid in ["own", "line", "run", "home", "vmcli", "host"] {
            let sid = format!("c-{sid}");
            let mut env = vec![(CLAUDE_TOKEN.to_string(), "t".to_string())];
            prepare(&r, &mine, &cwd, Some(&sid), need, &mut env, "01BOT", &cfg)
                .await
                .unwrap();
            assert!(
                crate::runtime::claude::find_session_file(&mine.join(".claude"), &cwd, &sid)
                    .is_some(),
                "{sid}"
            );
        }
        for sid in ["c-theirs", "c-theirs-run"] {
            let mut env = vec![(CLAUDE_TOKEN.to_string(), "t".to_string())];
            prepare(&r, &mine, &cwd, Some(sid), need, &mut env, "01BOT", &cfg)
                .await
                .unwrap();
            let own = mine.join(".claude");
            assert!(crate::runtime::claude::find_session_file(&own, &cwd, sid).is_none());
        }
        // The order: the member's lines, its runs, then the old homes.
        assert_eq!(
            session_roots(&cfg, &mine, "01BOT"),
            places[1..]
                .iter()
                .map(|(_, p)| p.clone())
                .collect::<Vec<_>>()
        );

        // Codex: a thread kept in a run folder of the member.
        let codex_host = tmp.path().join("host-codex");
        std::fs::create_dir_all(&codex_host).unwrap();
        let day = places[2].1.join(".codex/sessions/2026/11/09");
        std::fs::create_dir_all(&day).unwrap();
        std::fs::write(day.join("rollout-2026-11-09T10-00-00-x1.jsonl"), "{}\n").unwrap();
        let r = rt("codex", &[("CODEX_HOME", &codex_host)]);
        prepare(
            &r,
            &mine,
            &cwd,
            Some("x1"),
            need,
            &mut vec![],
            "01BOT",
            &cfg,
        )
        .await
        .unwrap();
        assert!(
            mine.join(".codex/sessions/2026/11/09/rollout-2026-11-09T10-00-00-x1.jsonl")
                .is_file()
        );

        // Pi: a session kept in an old home.
        let pi_host = tmp.path().join("host-pi");
        std::fs::create_dir_all(&pi_host).unwrap();
        let s = places[3].1.join(".pi/agent/sessions/--work--");
        std::fs::create_dir_all(&s).unwrap();
        std::fs::write(s.join("2026-11-09T10-00-00-000Z_p1.jsonl"), "{}\n").unwrap();
        let r = rt("acp", &[("PI_CODING_AGENT_DIR", &pi_host)]);
        prepare(
            &r,
            &mine,
            &cwd,
            Some("p1"),
            need,
            &mut vec![],
            "01BOT",
            &cfg,
        )
        .await
        .unwrap();
        assert!(
            mine.join(".pi/agent/sessions/--work--/2026-11-09T10-00-00-000Z_p1.jsonl")
                .is_file()
        );
    }

    /// At daemon start the sign-in copies a crash left go, except those of runs still active.
    #[test]
    fn the_sign_in_copies_a_crash_left_are_swept() {
        let tmp = tempfile::tempdir().unwrap();
        let cfg = vm_host(tmp.path());
        let w = tmp.path().join("w");
        let put = |f: &str| {
            let p = w.join(f);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(&p, "{}").unwrap();
            p
        };
        let line = put("lines/l1/cli/.codex/auth.json");
        let run = put("runs/r1/cli/.pi/agent/auth.json");
        let kept = put("runs/r1/cli/.codex/config.toml");
        let active = put("runs/r2/cli/.codex/auth.json");
        let active_line = put("lines/l2/cli/.codex/auth.json");
        let meta = json!({ "runId": "r3", "startedAt": 0, "line": w.join("lines/l2") });
        std::fs::create_dir_all(w.join("runs/r3")).unwrap();
        std::fs::write(w.join("runs/r3/run.json"), meta.to_string()).unwrap();
        let active_ids = ["r2".to_string(), "r3".to_string()];
        assert_eq!(sweep_auth(&cfg, &active_ids), 2);
        assert!(!line.exists() && !run.exists());
        assert!(kept.exists() && active.exists() && active_line.exists());
        assert_eq!(sweep_auth(&cfg, &active_ids), 0);
        assert_eq!(sweep_auth(&cfg, &[]), 2);
    }
}
