//! Feature "github" and "github-host": a run's git and gh use this computer's own GitHub
//! sign-in (`run.assign.github`, docs/machine-protocol.md "GitHub"; design: docs/plans/usable/
//! design-github-identity.zh.md 3.3).
//!
//! - Every run: the AI member is the committer (`GIT_COMMITTER_*`, and `GIT_AUTHOR_*` when the
//!   server names an author).
//! - On the computer itself (no `[vm]`): nothing else. git and gh are the computer's own, as
//!   they are for the person who uses it.
//! - In a VM (`[vm]` configured): the VM has no sign-in. git asks `chat-run git-credential`
//!   (set with `GIT_CONFIG_*`, after clearing every other helper for github.com; SSH addresses
//!   go over HTTPS) and `gh` is a proxy (`/opt/runner/bin/gh`). Both ask the daemon, and the
//!   daemon runs `gh auth token` on the host as its own user on every use: the token is never
//!   cached, written to a file, put in the CLI's environment, sent up as a run event or logged.
//!   Each token the run gets is added to the run's redaction list.

use crate::proto::RunEvent;
use crate::redact::Redactor;
use chat_protocol::github::RunGithub;
use serde_json::{Value, json};
use std::ffi::{OsStr, OsString};
use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, OnceLock};
use std::time::Duration;

/// `chat-run` op of the `gh` proxy: the token for one `gh` command.
pub const OP_TOKEN: &str = "github.token";
/// `chat-run` op (and subcommand) of git's credential helper.
pub const OP_CREDENTIAL: &str = "git-credential";
/// The ops the daemon answers itself, never as an `output` run event.
pub const OPS: &[&str] = &[OP_TOKEN, OP_CREDENTIAL];
/// The name the `gh` proxy is started by.
pub const GH: &str = "gh";
/// Set in a VM run only: whose GitHub this run uses (the computer's account, never a secret).
/// The `gh` proxy runs the real `gh` untouched without it.
pub const ENV_AS: &str = "CHAT_GITHUB_AS";
/// [`ENV_AS`] when the computer's GitHub login is not known.
pub const THIS_COMPUTER: &str = "this computer";
/// The GitHub this build talks to.
pub const BASE: &str = "https://github.com";
pub(crate) const HOST: &str = "github.com";
/// The VM's tools folder (see [`crate::executor`]'s firecracker backend), never the real gh.
pub const GUEST_BIN: &str = "/opt/runner/bin";
/// How long one `gh` on the host may take.
const GH_TIMEOUT: Duration = Duration::from_secs(10);
/// What the CLI is told when the computer has no GitHub sign-in.
pub const NOT_SIGNED_IN: &str =
    "This computer is not signed in to GitHub. Stop and report it; do not try to sign in.";

/// Appends `pairs` to the command-line git config of `env` (`GIT_CONFIG_COUNT`,
/// `GIT_CONFIG_KEY_<n>`, `GIT_CONFIG_VALUE_<n>`), after any pairs already there.
fn add_git_config(env: &mut Vec<(String, String)>, pairs: &[(String, String)]) {
    let n: usize = env
        .iter()
        .find(|(k, _)| k == "GIT_CONFIG_COUNT")
        .and_then(|(_, v)| v.trim().parse().ok())
        .unwrap_or(0);
    for (i, (k, v)) in pairs.iter().enumerate() {
        env.push((format!("GIT_CONFIG_KEY_{}", n + i), k.clone()));
        env.push((format!("GIT_CONFIG_VALUE_{}", n + i), v.clone()));
    }
    env.retain(|(k, _)| k != "GIT_CONFIG_COUNT");
    env.push(("GIT_CONFIG_COUNT".into(), (n + pairs.len()).to_string()));
}

/// The config that points github.com at `helper` (None: no helper at all) and sends its SSH
/// addresses over HTTPS (a VM has no SSH key).
fn config_pairs(base: &str, helper: Option<&str>) -> Vec<(String, String)> {
    let key = format!("credential.{base}.helper");
    // The empty value first clears every helper of the system and global config for this
    // host (`gh auth setup-git` in the image, Git for Windows' `credential.helper=manager`).
    let mut pairs = vec![(key.clone(), String::new())];
    if let Some(h) = helper {
        pairs.push((key, h.to_string()));
    }
    let to = format!("url.{base}/.insteadOf");
    pairs.push((to.clone(), format!("git@{HOST}:")));
    pairs.push((to, format!("ssh://git@{HOST}/")));
    pairs
}

/// The run's git identity and, in a VM (`vm_as`: the computer's GitHub login), its helper and
/// `gh` settings (the end of [`crate::run::build_env`]). No token goes into the environment.
pub fn put_env(g: &RunGithub, env: &mut Vec<(String, String)>, vm_as: Option<&str>) {
    put_env_at(g, env, BASE, vm_as)
}

/// [`put_env`] for another GitHub address (tests use a local one).
#[doc(hidden)]
pub fn put_env_at(g: &RunGithub, env: &mut Vec<(String, String)>, base: &str, vm_as: Option<&str>) {
    let mut put = |k: &str, v: &str| {
        env.retain(|(n, _)| n != k);
        env.push((k.to_string(), v.to_string()));
    };
    put("GIT_COMMITTER_NAME", &g.committer.name);
    put("GIT_COMMITTER_EMAIL", &g.committer.email);
    if let Some(a) = &g.author {
        put("GIT_AUTHOR_NAME", &a.name);
        put("GIT_AUTHOR_EMAIL", &a.email);
    }
    let Some(name) = vm_as else { return };
    put(ENV_AS, name);
    let helper = format!("!{} {OP_CREDENTIAL}", crate::chat_run::NAME);
    add_git_config(env, &config_pairs(base, Some(&helper)));
    env.retain(|(k, _)| k != "SSH_AUTH_SOCK");
}

/// The environment of the daemon's own git commands of a VM run (the mirror's clone and
/// fetch; [`crate::workspace::with_git_env`]): the computer's token as an HTTP header for
/// github.com, or no credentials at all when there is none (a public repository still works).
/// A run on the computer itself uses the computer's git and needs none of this.
pub fn fetch_env(token: Option<&str>) -> Vec<(String, Option<String>)> {
    fetch_env_at(token, BASE)
}

#[doc(hidden)]
pub fn fetch_env_at(token: Option<&str>, base: &str) -> Vec<(String, Option<String>)> {
    use base64::Engine;
    let mut pairs = config_pairs(base, None);
    if let Some(t) = token {
        let basic = base64::engine::general_purpose::STANDARD.encode(format!("x-access-token:{t}"));
        pairs.push((
            format!("http.{base}/.extraHeader"),
            format!("Authorization: Basic {basic}"),
        ));
    }
    let mut env = vec![];
    add_git_config(&mut env, &pairs);
    let mut out: Vec<(String, Option<String>)> =
        env.into_iter().map(|(k, v)| (k, Some(v))).collect();
    out.push(("SSH_AUTH_SOCK".into(), None));
    out
}

/// True when the daemon's own git for `repo` goes to GitHub.
pub fn fetches_from_github(repo: Option<&crate::proto::RepoSpec>) -> bool {
    repo.and_then(|r| r.url.as_deref())
        .is_some_and(|u| u.contains(HOST))
}

/// True when the end-of-run fetch of a work folder goes to GitHub: the run's `repo.url` does,
/// or (a `repo.path` run) the folder's `origin` remote does.
pub fn code_fetch_from_github(repo: Option<&crate::proto::RepoSpec>, origin: Option<&str>) -> bool {
    fetches_from_github(repo) || origin.is_some_and(|u| u.to_ascii_lowercase().contains(HOST))
}

/// What the CLI is told about GitHub (its system note): who commits, in a VM that git and gh
/// already work as the computer's account, and whom to name in a pull request.
pub fn note(g: &RunGithub, vm: bool) -> String {
    let mut n = format!(
        "GitHub: your commits are committed as {} <{}>.",
        g.committer.name, g.committer.email
    );
    if vm {
        n.push_str(
            " git and gh work as this computer's GitHub account: do not sign in (no gh auth \
login) and do not look for tokens.",
        );
    }
    if let Some(who) = g
        .asked_by
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        n.push_str(&format!(
            " When you open a pull request, end its body with the line \"交代人：{who}\"."
        ));
    }
    n
}

/// What one look at the computer's GitHub sign-in gave.
#[derive(Debug)]
pub enum Got {
    Token(String),
    /// No sign-in: "no_gh" (gh is not installed) or "signed_out".
    Missing(&'static str),
    Failed(String),
}

/// The real `gh` of the daemon's own `PATH` (`path` overrides it in tests).
fn host_gh(path: Option<&OsStr>) -> Option<PathBuf> {
    let p = path
        .map(OsStr::to_os_string)
        .or_else(|| std::env::var_os("PATH"))
        .unwrap_or_default();
    real_gh(&p)
}

/// Runs the host's `gh <args>` as the daemon's own user with the daemon's environment, at most
/// [`GH_TIMEOUT`]. `Ok(None)`: no gh. Its standard error is never read (nor logged).
pub(crate) async fn host_gh_output(
    path: Option<&OsStr>,
    args: &[&str],
) -> Result<Option<std::process::Output>, String> {
    let Some(gh) = host_gh(path) else {
        return Ok(None);
    };
    let mut c = tokio::process::Command::new(&gh);
    c.args(args)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .kill_on_drop(true);
    if let Some(p) = path {
        c.env("PATH", p);
    }
    let child = match c.spawn() {
        Ok(c) => c,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(format!("cannot start gh: {e}")),
    };
    match tokio::time::timeout(GH_TIMEOUT, child.wait_with_output()).await {
        Ok(Ok(o)) => Ok(Some(o)),
        Ok(Err(e)) => Err(format!("gh failed: {e}")),
        Err(_) => Err("gh did not answer in 10 seconds".into()),
    }
}

/// The token of the computer's GitHub sign-in (`gh auth token`), now.
pub async fn host_token(path: Option<&OsStr>) -> Got {
    match host_gh_output(path, &["auth", "token", "--hostname", HOST]).await {
        Ok(None) => Got::Missing("no_gh"),
        Ok(Some(o)) if !o.status.success() => Got::Missing("signed_out"),
        Ok(Some(o)) => match String::from_utf8(o.stdout).map(|t| t.trim().to_string()) {
            Ok(t) if !t.is_empty() && !t.contains(char::is_whitespace) => Got::Token(t),
            _ => Got::Failed("gh printed no token".into()),
        },
        Err(e) => Got::Failed(e),
    }
}

/// The login of the computer's GitHub account (`gh api user`), if it answers.
pub async fn host_login(path: Option<&OsStr>) -> Option<String> {
    let o = host_gh_output(path, &["api", "user", "--jq", ".login"])
        .await
        .ok()??;
    let login = String::from_utf8(o.stdout).ok()?.trim().to_string();
    let plain = !login.is_empty()
        && login.len() <= 64
        && login
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_' || b == b'[' || b == b']');
    (o.status.success() && plain).then_some(login)
}

/// Asks the daemon to look at the computer's GitHub sign-in again now (a push was refused).
pub fn recheck() -> &'static tokio::sync::Notify {
    static N: OnceLock<tokio::sync::Notify> = OnceLock::new();
    N.get_or_init(tokio::sync::Notify::new)
}

/// One run's GitHub on the daemon side.
pub struct Run {
    pub spec: RunGithub,
    redactor: Arc<Redactor>,
    host: String,
    vm: bool,
    path: Option<OsString>,
    missing: AtomicBool,
    invalid: AtomicBool,
}

impl Run {
    /// `vm`: the run is in a VM and uses the host's sign-in through the daemon.
    pub fn new(spec: RunGithub, redactor: Arc<Redactor>, vm: bool) -> Run {
        Run {
            spec,
            redactor,
            host: HOST.into(),
            vm,
            path: None,
            missing: AtomicBool::new(false),
            invalid: AtomicBool::new(false),
        }
    }

    /// The host the credential helper answers for (tests use a local one).
    #[doc(hidden)]
    pub fn with_host(mut self, host: &str) -> Run {
        self.host = host.to_string();
        self
    }

    /// The `PATH` the host's `gh` is found on (tests use a fake one).
    #[doc(hidden)]
    pub fn with_path(mut self, path: impl Into<OsString>) -> Run {
        self.path = Some(path.into());
        self
    }

    /// True for a VM run: git and gh get the host's sign-in through the daemon.
    pub fn vm(&self) -> bool {
        self.vm
    }

    /// Reads the host's token, once, now; the run masks it from now on.
    pub async fn fetch(&self) -> Got {
        let got = host_token(self.path.as_deref()).await;
        if let Got::Token(t) = &got {
            self.redactor.add(t);
        }
        got
    }

    /// The computer's GitHub login, for [`ENV_AS`].
    pub async fn login(&self) -> Option<String> {
        host_login(self.path.as_deref()).await
    }

    /// Reports once per run that the computer has no GitHub sign-in.
    pub fn missing(&self, reason: &str, emit: &(dyn Fn(RunEvent) + Send + Sync)) {
        if !self.missing.swap(true, Ordering::SeqCst) {
            emit(RunEvent::Github {
                state: "missing".into(),
                login: None,
                reason: Some(reason.into()),
            });
        }
    }

    /// GitHub refused the token (git erased it): reported once per run, and the daemon looks
    /// at the sign-in again now.
    pub fn invalid(&self, emit: &(dyn Fn(RunEvent) + Send + Sync)) {
        if !self.invalid.swap(true, Ordering::SeqCst) {
            emit(RunEvent::Github {
                state: "invalid".into(),
                login: None,
                reason: None,
            });
            recheck().notify_one();
        }
    }

    /// The token as a `chat-run` answer, or the error the CLI sees.
    /// `report`: a missing sign-in is told to the topic and the administrators
    /// ([`Run::missing`]); false when the command only looks at the sign-in.
    async fn token(
        &self,
        report: bool,
        emit: &(dyn Fn(RunEvent) + Send + Sync),
    ) -> Result<String, Value> {
        let e = crate::chat_run_files::err;
        match self.fetch().await {
            Got::Token(t) => Ok(t),
            Got::Missing(reason) => {
                if report {
                    self.missing(reason, emit);
                }
                Err(e(
                    "not_found",
                    Some(&format!("github.{reason}")),
                    NOT_SIGNED_IN,
                ))
            }
            Got::Failed(m) => Err(crate::chat_run_files::err(
                "unavailable",
                None,
                format!("cannot read this computer's GitHub sign-in: {m}"),
            )),
        }
    }

    /// One `chat-run` request of [`OPS`].
    pub async fn handle(
        &self,
        op: &str,
        body: &Value,
        emit: &(dyn Fn(RunEvent) + Send + Sync),
    ) -> Value {
        match op {
            OP_TOKEN => match self.token(body["quiet"] != json!(true), emit).await {
                Ok(t) => json!({ "ok": true, "result": { "token": t } }),
                Err(e) => e,
            },
            OP_CREDENTIAL => {
                let host = body["host"].as_str().unwrap_or_default();
                match body["action"].as_str().unwrap_or_default() {
                    "get" if host.eq_ignore_ascii_case(&self.host) => {
                        match self.token(true, emit).await {
                            Ok(t) => json!({ "ok": true, "result": {
                                "username": "x-access-token", "password": t } }),
                            Err(e) => e,
                        }
                    }
                    "erase" if host.eq_ignore_ascii_case(&self.host) => {
                        self.invalid(emit);
                        json!({ "ok": true, "result": {} })
                    }
                    _ => json!({ "ok": true, "result": {} }),
                }
            }
            _ => crate::chat_run_files::err("invalid", None, "not a GitHub op"),
        }
    }
}

// --- The client side: the daemon binary started as `chat-run git-credential` or `gh`. -------

/// Asks the daemon over the run's `chat-run` transport.
fn ask(op: &str, body: Value) -> Result<Value, String> {
    use crate::chat_run::{ENV_DIR, ENV_SOCKET, TIMEOUT, call, call_files};
    let var = |k: &str| std::env::var_os(k).filter(|s| !s.is_empty());
    let wait = TIMEOUT + Duration::from_secs(5);
    match (var(ENV_SOCKET), var(ENV_DIR)) {
        (Some(s), _) => call(Path::new(&s), op, body, wait),
        (None, Some(d)) => call_files(Path::new(&d), op, body, wait),
        (None, None) => Err(format!(
            "{ENV_SOCKET} is not set: this works only inside a run"
        )),
    }
}

fn error_text(v: &Value) -> String {
    v["error"]["message"]
        .as_str()
        .unwrap_or("the daemon refused")
        .to_string()
}

/// `chat-run git-credential <get|store|erase>`: git's credential helper protocol on standard
/// input and output. Only `get` and `erase` for github.com reach the daemon.
pub fn credential_main(args: &[String]) -> i32 {
    let action = args.first().map(String::as_str).unwrap_or_default();
    let mut fields = serde_json::Map::new();
    for line in std::io::stdin().lock().lines() {
        let Ok(line) = line else { break };
        if line.is_empty() {
            break;
        }
        if let Some((k, v)) = line.split_once('=') {
            fields.insert(k.to_string(), json!(v));
        }
    }
    if action != "get" && action != "erase" {
        return 0;
    }
    let host = fields
        .get("host")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    let body = json!({ "action": action, "host": host,
        "protocol": fields.get("protocol").cloned().unwrap_or(Value::Null) });
    let answer = ask(OP_CREDENTIAL, body);
    if action == "erase" {
        return 0;
    }
    let mut out = std::io::stdout().lock();
    match answer {
        Ok(v) if v["ok"] == json!(true) => {
            let r = &v["result"];
            if let (Some(u), Some(p)) = (r["username"].as_str(), r["password"].as_str()) {
                let _ = write!(out, "username={u}\npassword={p}\n");
            }
            0
        }
        // Fail now: git must not fall back to a prompt or another sign-in.
        Ok(v) => {
            eprintln!("{}", error_text(&v));
            let _ = writeln!(out, "quit=1");
            0
        }
        Err(e) => {
            eprintln!("cannot ask for GitHub credentials: {e}");
            let _ = writeln!(out, "quit=1");
            0
        }
    }
}

/// The real `gh` on `PATH`: not in a folder of the run's tools (where `chat-run` is, or the
/// VM's tools folder) and not this program.
pub fn real_gh(path: &std::ffi::OsStr) -> Option<PathBuf> {
    crate::shim::real_program(GH, path)
}

/// True for the `gh` commands that do not use the sign-in: `gh auth status`, help and the
/// version. A CLI often runs them after a person refused its push; a missing sign-in then is
/// no failure to tell the topic and the administrators about (verify-log 4, problem 4).
fn only_looks(args: &[String]) -> bool {
    let first = args.first().map(String::as_str);
    matches!(first, None | Some("help" | "version" | "--version"))
        || args.iter().any(|a| a == "--help" || a == "-h")
        || (first == Some("auth") && args.get(1).map(String::as_str) == Some("status"))
}

/// True for the `gh` commands a run may not use: signing in or out, and printing the token.
fn refused(args: &[String]) -> bool {
    if args.first().map(String::as_str) != Some("auth") {
        return false;
    }
    match args.get(1).map(String::as_str) {
        Some("login" | "logout" | "refresh" | "setup-git" | "token") => true,
        Some("status") => args[2..]
            .iter()
            .any(|a| a == "-t" || a == "--show-token" || a.starts_with("--show-token=")),
        _ => false,
    }
}

/// `gh <args>` in a VM run: the real `gh` with the computer's token for this one command.
pub fn gh_main(args: Vec<String>) -> i32 {
    // A run with gated actions: `gh` is one of its shims too (crate::shim).
    if let Err(code) = crate::shim::gate(GH, &args) {
        return code;
    }
    let as_person = std::env::var(ENV_AS).ok().filter(|s| !s.is_empty());
    if let Some(name) = &as_person
        && refused(&args)
    {
        eprintln!(
            "gh already works as this computer's GitHub account ({name}); do not sign in or \
read its token here."
        );
        return 1;
    }
    let path = std::env::var_os("PATH").unwrap_or_default();
    let Some(real) = real_gh(&path) else {
        eprintln!("gh: command not found");
        return 127;
    };
    let mut cmd = std::process::Command::new(&real);
    cmd.args(&args);
    let looks = only_looks(&args);
    if as_person.is_some() {
        match ask(OP_TOKEN, json!({ "quiet": looks })) {
            Ok(v) if v["ok"] == json!(true) => {
                cmd.env(
                    "GH_TOKEN",
                    v["result"]["token"].as_str().unwrap_or_default(),
                );
            }
            // gh itself says it is not signed in; nobody is told.
            Ok(_) if looks => {}
            Ok(v) => {
                eprintln!("{}", error_text(&v));
                return 1;
            }
            Err(e) => {
                eprintln!("cannot ask for the GitHub token: {e}");
                return 1;
            }
        }
    }
    match cmd.status() {
        Ok(s) => {
            // Ended by a signal: the shell's number for it.
            #[cfg(unix)]
            let other = {
                use std::os::unix::process::ExitStatusExt;
                128 + s.signal().unwrap_or(0)
            };
            #[cfg(not(unix))]
            let other = 1;
            s.code().unwrap_or(other)
        }
        Err(e) => {
            eprintln!("gh: {}: {e}", real.display());
            126
        }
    }
}

/// `<run folder>/bin/gh` (`gh.exe` on Windows): the proxy, linked to the daemon binary like
/// `chat-run` ([`crate::chat_run::install_link`] made the folder).
pub fn install_gh(bin: &Path) -> std::io::Result<()> {
    let link = bin.join(format!("{GH}{}", std::env::consts::EXE_SUFFIX));
    let _ = std::fs::remove_file(&link);
    let exe = crate::chat_run::exe()?;
    #[cfg(unix)]
    std::os::unix::fs::symlink(exe, &link)?;
    #[cfg(not(unix))]
    if std::fs::hard_link(&exe, &link).is_err() {
        std::fs::copy(&exe, &link)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use chat_protocol::github::GitIdentity;

    pub fn spec(source: &str, author: bool, asked_by: Option<&str>) -> RunGithub {
        RunGithub {
            source: source.into(),
            member_id: "U1".into(),
            login: None,
            author: author.then(|| GitIdentity {
                name: "Ann".into(),
                email: "1+octo@users.noreply.github.com".into(),
            }),
            committer: GitIdentity {
                name: "Bot".into(),
                email: "ai-B1@users.noreply.invalid".into(),
            },
            rewrite_ssh: None,
            asked_by: asked_by.map(str::to_string),
        }
    }

    fn get<'a>(env: &'a [(String, String)], k: &str) -> Option<&'a str> {
        env.iter().find(|(n, _)| n == k).map(|(_, v)| v.as_str())
    }

    fn config(env: &[(String, String)]) -> Vec<(String, String)> {
        let n: usize = get(env, "GIT_CONFIG_COUNT").map_or(0, |v| v.parse().unwrap());
        (0..n)
            .map(|i| {
                (
                    get(env, &format!("GIT_CONFIG_KEY_{i}"))
                        .unwrap()
                        .to_string(),
                    get(env, &format!("GIT_CONFIG_VALUE_{i}"))
                        .unwrap()
                        .to_string(),
                )
            })
            .collect()
    }

    /// A `repo.path` run fetches its code location with the token when its `origin` is on
    /// GitHub (the CLI pushed with it); other remotes stay without one.
    #[test]
    fn the_code_fetch_uses_github_for_a_path_run_whose_origin_is_github() {
        let path = crate::proto::RepoSpec {
            url: None,
            path: Some("/work/repo".into()),
            base_branch: None,
        };
        assert!(!code_fetch_from_github(Some(&path), None));
        assert!(!code_fetch_from_github(
            Some(&path),
            Some("https://git.example.com/o/r.git")
        ));
        assert!(code_fetch_from_github(
            Some(&path),
            Some("https://GitHub.com/o/private.git")
        ));
        assert!(code_fetch_from_github(
            Some(&path),
            Some("git@github.com:o/r.git")
        ));
        let url = crate::proto::RepoSpec {
            url: Some("https://github.com/o/r.git".into()),
            path: None,
            base_branch: None,
        };
        assert!(code_fetch_from_github(Some(&url), None));
        assert!(!code_fetch_from_github(None, None));
    }

    /// On the computer itself (any source, "app" too): only the committer (and an author the
    /// server names); git, gh and SSH stay the computer's own.
    #[test]
    fn on_the_computer_only_the_committer_is_set() {
        for source in ["machine", "app", "team"] {
            let mut env = vec![("SSH_AUTH_SOCK".to_string(), "/s".to_string())];
            put_env(&spec(source, false, None), &mut env, None);
            assert_eq!(get(&env, "GIT_COMMITTER_NAME"), Some("Bot"));
            assert_eq!(
                get(&env, "GIT_COMMITTER_EMAIL"),
                Some("ai-B1@users.noreply.invalid")
            );
            assert_eq!(get(&env, "GIT_AUTHOR_NAME"), None);
            assert_eq!(get(&env, "GIT_CONFIG_COUNT"), None);
            assert_eq!(get(&env, ENV_AS), None);
            assert_eq!(get(&env, "SSH_AUTH_SOCK"), Some("/s"));
        }
        let mut env = vec![];
        put_env(&spec("machine", true, None), &mut env, None);
        assert_eq!(get(&env, "GIT_AUTHOR_NAME"), Some("Ann"));
    }

    /// In a VM: github.com's helpers are cleared, then `chat-run git-credential` is the only
    /// one; config already given on the command line is kept; SSH always goes over HTTPS.
    #[test]
    fn a_vm_run_gets_the_helper_and_never_a_token() {
        let mut env = vec![
            ("GIT_CONFIG_COUNT".to_string(), "1".to_string()),
            ("GIT_CONFIG_KEY_0".to_string(), "core.pager".to_string()),
            ("GIT_CONFIG_VALUE_0".to_string(), "cat".to_string()),
            ("SSH_AUTH_SOCK".to_string(), "/s".to_string()),
        ];
        put_env(&spec("app", false, None), &mut env, Some("org-bot"));
        let key = "credential.https://github.com.helper".to_string();
        assert_eq!(
            config(&env),
            vec![
                ("core.pager".into(), "cat".into()),
                (key.clone(), String::new()),
                (key, "!chat-run git-credential".into()),
                (
                    "url.https://github.com/.insteadOf".into(),
                    "git@github.com:".into()
                ),
                (
                    "url.https://github.com/.insteadOf".into(),
                    "ssh://git@github.com/".into()
                ),
            ]
        );
        assert_eq!(get(&env, "SSH_AUTH_SOCK"), None);
        assert_eq!(get(&env, ENV_AS), Some("org-bot"));
        assert!(
            env.iter()
                .all(|(k, v)| !k.contains("TOKEN") && !v.contains("gho_"))
        );
    }

    /// The daemon's own fetch in a VM run: the token as a header for github.com only, no
    /// helper, and no credentials at all when there is none.
    #[test]
    fn the_daemons_own_fetch_uses_the_token_as_a_header() {
        let env = fetch_env(Some("gho_abc"));
        let flat: Vec<(String, String)> = env
            .iter()
            .filter_map(|(k, v)| v.clone().map(|v| (k.clone(), v)))
            .collect();
        let c = config(&flat);
        assert_eq!(
            c[0],
            ("credential.https://github.com.helper".into(), "".into())
        );
        let header = c
            .iter()
            .find(|(k, _)| k == "http.https://github.com/.extraHeader")
            .unwrap();
        // base64("x-access-token:gho_abc")
        assert_eq!(
            header.1,
            "Authorization: Basic eC1hY2Nlc3MtdG9rZW46Z2hvX2FiYw=="
        );
        assert!(env.contains(&("SSH_AUTH_SOCK".into(), None)));
        let none = fetch_env(None);
        assert!(
            none.iter()
                .all(|(_, v)| !v.as_deref().unwrap_or("").contains("Basic"))
        );
    }

    #[test]
    fn the_note_names_the_committer_the_computer_and_who_asked() {
        let n = note(&spec("machine", false, None), false);
        assert_eq!(
            n,
            "GitHub: your commits are committed as Bot <ai-B1@users.noreply.invalid>."
        );
        let n = note(&spec("machine", false, Some("Ivy Intern")), true);
        assert!(n.contains("work as this computer's GitHub account"), "{n}");
        assert!(n.contains("do not sign in"), "{n}");
        assert!(
            n.ends_with(
                "When you open a pull request, end its body with the line \"交代人：Ivy Intern\"."
            ),
            "{n}"
        );
        assert!(!note(&spec("machine", false, Some("  ")), true).contains("交代人"));
    }

    #[test]
    fn printing_the_token_and_signing_in_are_refused() {
        let a = |s: &str| s.split(' ').map(str::to_string).collect::<Vec<_>>();
        for c in [
            "auth login",
            "auth logout",
            "auth refresh",
            "auth setup-git",
            "auth token",
            "auth token --hostname github.com",
            "auth status --show-token",
            "auth status -t",
            "auth status --hostname github.com -t",
        ] {
            assert!(refused(&a(c)), "{c}");
        }
        for c in ["auth status", "pr create", "api user", "repo view -t x"] {
            assert!(!refused(&a(c)), "{c}");
        }
    }

    #[test]
    fn looking_at_the_sign_in_is_no_use_of_it() {
        let a = |s: &str| s.split_whitespace().map(str::to_string).collect::<Vec<_>>();
        for c in [
            "",
            "--version",
            "version",
            "help",
            "auth status",
            "pr create --help",
        ] {
            assert!(only_looks(&a(c)), "{c}");
        }
        for c in ["pr create", "api user", "repo view", "auth token"] {
            assert!(!only_looks(&a(c)), "{c}");
        }
    }

    #[tokio::test]
    async fn a_quiet_token_ask_tells_nobody_of_a_missing_sign_in() {
        let empty = tempfile::tempdir().unwrap();
        let g = Run::new(
            spec("machine", false, None),
            Arc::new(crate::redact::Redactor::default()),
            true,
        )
        .with_path(empty.path().as_os_str());
        let events = std::sync::Mutex::new(vec![]);
        let emit = |e: RunEvent| events.lock().unwrap().push(e);
        let quiet = g.handle(OP_TOKEN, &json!({ "quiet": true }), &emit).await;
        assert_eq!(quiet["ok"], json!(false));
        assert!(events.lock().unwrap().is_empty());
        let loud = g.handle(OP_TOKEN, &json!({}), &emit).await;
        assert_eq!(loud["ok"], json!(false));
        assert_eq!(events.lock().unwrap().len(), 1);
    }

    #[test]
    fn the_real_gh_is_not_the_proxy() {
        let tmp = tempfile::tempdir().unwrap();
        let tools = tmp.path().join("bin");
        let sys = tmp.path().join("usr");
        std::fs::create_dir_all(&tools).unwrap();
        std::fs::create_dir_all(&sys).unwrap();
        let exe = |d: &Path, n: &str| {
            let p = d.join(format!("{n}{}", std::env::consts::EXE_SUFFIX));
            std::fs::write(&p, b"x").unwrap();
            crate::sys::set_mode(&p, 0o755).unwrap();
            p
        };
        exe(&tools, "chat-run");
        exe(&tools, GH);
        let real = exe(&sys, GH);
        let path = std::env::join_paths([GUEST_BIN.into(), tools.clone(), sys.clone()]).unwrap();
        assert_eq!(real_gh(&path), Some(real));
        let path = std::env::join_paths([tools]).unwrap();
        assert_eq!(real_gh(&path), None);
    }
}
