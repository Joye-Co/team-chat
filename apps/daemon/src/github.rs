//! Feature "github": a run uses the GitHub identity the server gives it (`run.assign.github`,
//! docs/machine-protocol.md "GitHub (feature `github`)"; design: docs/plans/usable/
//! design-github-identity.zh.md 3.6).
//!
//! - Both modes: the AI member is the committer (`GIT_COMMITTER_*`); the person is the author
//!   when the server says so (`GIT_AUTHOR_*`), else the machine's own git config.
//! - `app` mode: git asks `chat-run git-credential` (set with `GIT_CONFIG_*`, after clearing
//!   every other helper for github.com) and `gh` is a proxy (`<run folder>/bin/gh`, in a VM
//!   `/opt/runner/bin/gh`). Both ask the daemon, and the daemon asks the server
//!   (`GET /v1/machine/runs/:id/github`) on every use: the token is never cached, written to a
//!   file, put in the CLI's environment or sent up as a run event. Each token the run gets is
//!   added to the run's redaction list.
//! - `machine` mode (and any newer source): nothing but the committer; git and gh are the
//!   machine's own.

use crate::proto::RunEvent;
use crate::redact::Redactor;
use chat_protocol::github::{MachineGithubTokenResponse, MachineGithubWaitRequest, RunGithub};
use serde_json::{Value, json};
use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

/// `chat-run` op of the `gh` proxy: the token for one `gh` command.
pub const OP_TOKEN: &str = "github.token";
/// `chat-run` op (and subcommand) of git's credential helper.
pub const OP_CREDENTIAL: &str = "git-credential";
/// The ops the daemon answers itself, never as an `output` run event.
pub const OPS: &[&str] = &[OP_TOKEN, OP_CREDENTIAL];
/// The name the `gh` proxy is started by.
pub const GH: &str = "gh";
/// Set in `app` mode only: whose GitHub this run uses (a display name, never a secret). The
/// `gh` proxy runs the real `gh` untouched without it.
pub const ENV_AS: &str = "CHAT_GITHUB_AS";
/// The GitHub this build talks to.
pub const BASE: &str = "https://github.com";
const HOST: &str = "github.com";
/// The VM's tools folder (see [`crate::executor`]'s firecracker backend), never the real gh.
pub const GUEST_BIN: &str = "/opt/runner/bin";
/// How long the daemon waits for the server's answer.
const SERVER_TIMEOUT: Duration = Duration::from_secs(10);

/// Whose name the run's texts use for the person.
pub fn person(g: &RunGithub) -> String {
    g.author
        .as_ref()
        .map(|a| a.name.trim())
        .filter(|n| !n.is_empty())
        .map(str::to_string)
        .or_else(|| g.login.clone().filter(|l| !l.is_empty()))
        .unwrap_or_else(|| "the person who asked".into())
}

/// True for `source: "app"`; any other (or newer) source is the machine's own GitHub.
pub fn app(g: &RunGithub) -> bool {
    g.source == "app"
}

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

/// The config that points github.com at `helper` (None: no helper at all) and, with
/// `rewriteSsh`, sends its SSH addresses over HTTPS.
fn config_pairs(g: &RunGithub, base: &str, helper: Option<&str>) -> Vec<(String, String)> {
    let key = format!("credential.{base}.helper");
    // The empty value first clears every helper of the system and global config for this
    // host (Git for Windows' `credential.helper=manager`, `gh auth setup-git`).
    let mut pairs = vec![(key.clone(), String::new())];
    if let Some(h) = helper {
        pairs.push((key, h.to_string()));
    }
    if g.rewrite_ssh == Some(true) {
        let to = format!("url.{base}/.insteadOf");
        pairs.push((to.clone(), format!("git@{HOST}:")));
        pairs.push((to, format!("ssh://git@{HOST}/")));
    }
    pairs
}

/// The run's git identity and, in `app` mode, its helper and `gh` settings (the end of
/// [`crate::run::build_env`]). No token goes into the environment.
pub fn put_env(g: &RunGithub, env: &mut Vec<(String, String)>) {
    put_env_at(g, env, BASE)
}

/// [`put_env`] for another GitHub address (tests use a local one).
#[doc(hidden)]
pub fn put_env_at(g: &RunGithub, env: &mut Vec<(String, String)>, base: &str) {
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
    if !app(g) {
        return;
    }
    put(ENV_AS, &person(g));
    let helper = format!("!{} {OP_CREDENTIAL}", crate::chat_run::NAME);
    add_git_config(env, &config_pairs(g, base, Some(&helper)));
    if g.rewrite_ssh == Some(true) {
        env.retain(|(k, _)| k != "SSH_AUTH_SOCK");
    }
}

/// The environment of the daemon's own git commands for the run (the mirror's clone and
/// fetch; [`crate::workspace::with_git_env`]): in `app` mode the person's token as an HTTP
/// header for github.com, or no credentials at all when there is none (a public repository
/// still works). Empty in `machine` mode: the machine's own git.
pub fn fetch_env(g: &RunGithub, token: Option<&str>) -> Vec<(String, Option<String>)> {
    fetch_env_at(g, token, BASE)
}

#[doc(hidden)]
pub fn fetch_env_at(
    g: &RunGithub,
    token: Option<&str>,
    base: &str,
) -> Vec<(String, Option<String>)> {
    use base64::Engine;
    if !app(g) {
        return vec![];
    }
    let mut pairs = config_pairs(g, base, None);
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
    if g.rewrite_ssh == Some(true) {
        out.push(("SSH_AUTH_SOCK".into(), None));
    }
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

/// What the CLI is told about GitHub in `app` mode (its system note).
pub fn note(g: &RunGithub) -> Option<String> {
    if !app(g) {
        return None;
    }
    let name = person(g);
    let who = match g.login.as_deref().filter(|l| !l.is_empty()) {
        Some(login) => format!("git and gh act as {login} ({name}, who asked)"),
        None => format!("git and gh act as the GitHub account of {name}, who asked"),
    };
    Some(format!(
        "GitHub: {who}. Do not run gh auth login. If git says GitHub is not connected, stop and \
report that you are waiting for {name}. If GitHub refuses because of \"OAuth App access \
restrictions\", stop and report that an owner of that organization must approve the app at \
https://github.com/organizations/<org>/settings/oauth_application_policy."
    ))
}

/// What one fetch of the token gave.
#[derive(Debug)]
pub enum Got {
    Token(MachineGithubTokenResponse),
    /// The person has not connected GitHub (404).
    Missing,
    /// GitHub refused the person's token (409).
    Invalid,
    /// This machine or run may not use it (403: another machine, an ended run, no right).
    NotAllowed,
    Failed(String),
}

/// One run's GitHub on the daemon side.
pub struct Run {
    server_url: String,
    machine_token: Arc<String>,
    run_id: String,
    pub spec: RunGithub,
    redactor: Arc<Redactor>,
    host: String,
    used: AtomicBool,
    waited: AtomicBool,
}

impl Run {
    pub fn new(
        server_url: &str,
        machine_token: Arc<String>,
        run_id: &str,
        spec: RunGithub,
        redactor: Arc<Redactor>,
    ) -> Run {
        Run {
            server_url: server_url.to_string(),
            machine_token,
            run_id: run_id.to_string(),
            spec,
            redactor,
            host: HOST.into(),
            used: AtomicBool::new(false),
            waited: AtomicBool::new(false),
        }
    }

    /// The host the credential helper answers for (tests use a local one).
    #[doc(hidden)]
    pub fn with_host(mut self, host: &str) -> Run {
        self.host = host.to_string();
        self
    }

    pub fn app(&self) -> bool {
        app(&self.spec)
    }

    fn url(&self, tail: &str) -> Option<String> {
        let u = format!(
            "{}/v1/machine/runs/{}/github{tail}",
            self.server_url.trim_end_matches('/'),
            self.run_id
        );
        // The machine token goes only to the configured server.
        crate::upload::same_origin(&u, &self.server_url).then_some(u)
    }

    /// Asks the server for the token, once, now.
    pub async fn fetch(&self, emit: &(dyn Fn(RunEvent) + Send + Sync)) -> Got {
        let Some(url) = self.url("") else {
            return Got::Failed("bad server url".into());
        };
        let client = match reqwest::Client::builder().timeout(SERVER_TIMEOUT).build() {
            Ok(c) => c,
            Err(e) => return Got::Failed(e.to_string()),
        };
        let res = client
            .get(url)
            .bearer_auth(self.machine_token.as_str())
            .send()
            .await;
        let r = match res {
            Ok(r) => r,
            Err(e) => return Got::Failed(format!("cannot reach the server: {}", e.without_url())),
        };
        match r.status().as_u16() {
            200 => match r
                .bytes()
                .await
                .ok()
                .and_then(|b| serde_json::from_slice::<MachineGithubTokenResponse>(&b).ok())
            {
                Some(t) => {
                    self.redactor.add(&t.token);
                    if !self.used.swap(true, Ordering::SeqCst) {
                        emit(RunEvent::Github {
                            state: "used".into(),
                            login: Some(t.login.clone()),
                            reason: None,
                        });
                    }
                    Got::Token(t)
                }
                None => Got::Failed("the server's answer has no token".into()),
            },
            404 => Got::Missing,
            409 => Got::Invalid,
            403 => Got::NotAllowed,
            s => Got::Failed(format!("the server answered HTTP {s}")),
        }
    }

    /// Reports once per run that it needs a GitHub the person has not connected (or that no
    /// longer works): the server asks them, and the run stops and says it waits.
    pub async fn wait(&self, invalid: bool, emit: &(dyn Fn(RunEvent) + Send + Sync)) {
        if self.waited.swap(true, Ordering::SeqCst) {
            return;
        }
        emit(RunEvent::Github {
            state: if invalid { "invalid" } else { "missing" }.into(),
            login: self.spec.login.clone(),
            reason: None,
        });
        let body = MachineGithubWaitRequest {
            reason: if invalid { "invalid" } else { "not_connected" }.into(),
        };
        self.post("/wait", &json!(body)).await;
    }

    /// git got an authentication failure with the token: the server checks it once.
    pub async fn rejected(&self) {
        self.post("/rejected", &json!({})).await;
    }

    async fn post(&self, tail: &str, body: &Value) {
        let Some(url) = self.url(tail) else { return };
        let Ok(client) = reqwest::Client::builder().timeout(SERVER_TIMEOUT).build() else {
            return;
        };
        let res = client
            .post(url)
            .bearer_auth(self.machine_token.as_str())
            .header("Content-Type", "application/json")
            .body(body.to_string())
            .send()
            .await;
        if let Err(e) = res.and_then(|r| r.error_for_status()) {
            eprintln!(
                "{} run {}: github{tail}: {}",
                crate::proto::now_ms(),
                self.run_id,
                e.without_url()
            );
        }
    }

    /// What the run's CLI is told when it waits for the person.
    pub fn waiting(&self, invalid: bool) -> String {
        let name = person(&self.spec);
        if invalid {
            format!(
                "The GitHub connection of {name} no longer works (they have been asked to connect \
again). Stop and report that you are waiting for {name} to connect GitHub again."
            )
        } else {
            format!(
                "GitHub is not connected for {name} (they have been asked). Stop and report that \
you are waiting for them to connect GitHub."
            )
        }
    }

    /// The token as a `chat-run` answer, or the error the CLI sees.
    async fn token(
        &self,
        emit: &(dyn Fn(RunEvent) + Send + Sync),
    ) -> Result<MachineGithubTokenResponse, Value> {
        let e = crate::chat_run_files::err;
        match self.fetch(emit).await {
            Got::Token(t) => Ok(t),
            Got::Missing => {
                self.wait(false, emit).await;
                Err(e(
                    "not_found",
                    Some("github.not_connected"),
                    self.waiting(false),
                ))
            }
            Got::Invalid => {
                self.wait(true, emit).await;
                Err(e("conflict", Some("github.invalid"), self.waiting(true)))
            }
            Got::NotAllowed => Err(e(
                "forbidden",
                Some("github.not_allowed"),
                format!("this run may not use the GitHub of {}", person(&self.spec)),
            )),
            Got::Failed(m) => Err(e("unavailable", None, m)),
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
            OP_TOKEN => match self.token(emit).await {
                Ok(t) => json!({ "ok": true, "result": { "token": t.token, "login": t.login } }),
                Err(e) => e,
            },
            OP_CREDENTIAL => {
                let host = body["host"].as_str().unwrap_or_default();
                match body["action"].as_str().unwrap_or_default() {
                    "get" if host.eq_ignore_ascii_case(&self.host) => {
                        match self.token(emit).await {
                            Ok(t) => json!({ "ok": true, "result": {
                                "username": "x-access-token", "password": t.token } }),
                            Err(e) => e,
                        }
                    }
                    "erase" if host.eq_ignore_ascii_case(&self.host) => {
                        self.rejected().await;
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

/// `gh <args>` in a run: the real `gh` with the person's token for this one command.
pub fn gh_main(args: Vec<String>) -> i32 {
    // A run with gated actions: `gh` is one of its shims too (crate::shim).
    if let Err(code) = crate::shim::gate(GH, &args) {
        return code;
    }
    let as_person = std::env::var(ENV_AS).ok().filter(|s| !s.is_empty());
    if let Some(name) = &as_person
        && args.first().map(String::as_str) == Some("auth")
        && matches!(
            args.get(1).map(String::as_str),
            Some("login" | "logout" | "refresh" | "setup-git")
        )
    {
        eprintln!("GitHub sign-in is handled by the app for {name}; do not sign in here.");
        return 1;
    }
    let path = std::env::var_os("PATH").unwrap_or_default();
    let Some(real) = real_gh(&path) else {
        eprintln!("gh: command not found");
        return 127;
    };
    let mut cmd = std::process::Command::new(&real);
    cmd.args(&args);
    if as_person.is_some() {
        match ask(OP_TOKEN, json!({})) {
            Ok(v) if v["ok"] == json!(true) => {
                cmd.env(
                    "GH_TOKEN",
                    v["result"]["token"].as_str().unwrap_or_default(),
                );
            }
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

    pub fn spec(source: &str, author: bool, rewrite: bool) -> RunGithub {
        RunGithub {
            source: source.into(),
            member_id: "U1".into(),
            login: Some("octo".into()),
            author: author.then(|| GitIdentity {
                name: "Ann".into(),
                email: "1+octo@users.noreply.github.com".into(),
            }),
            committer: GitIdentity {
                name: "Bot".into(),
                email: "ai-B1@users.noreply.invalid".into(),
            },
            rewrite_ssh: rewrite.then_some(true),
            asked_by: None,
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

    /// 机 4.5: the AI member commits in both modes; the person is the author only when the
    /// server names one, else the machine's git config stays.
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

    #[test]
    fn the_committer_is_the_ai_member_and_the_author_only_when_given() {
        let mut env = vec![("SSH_AUTH_SOCK".to_string(), "/s".to_string())];
        put_env(&spec("machine", false, false), &mut env);
        assert_eq!(get(&env, "GIT_COMMITTER_NAME"), Some("Bot"));
        assert_eq!(
            get(&env, "GIT_COMMITTER_EMAIL"),
            Some("ai-B1@users.noreply.invalid")
        );
        assert_eq!(get(&env, "GIT_AUTHOR_NAME"), None);
        // Machine mode: no helper, no proxy, the machine's SSH.
        assert_eq!(get(&env, "GIT_CONFIG_COUNT"), None);
        assert_eq!(get(&env, ENV_AS), None);
        assert_eq!(get(&env, "SSH_AUTH_SOCK"), Some("/s"));

        let mut env = vec![];
        put_env(&spec("app", true, false), &mut env);
        assert_eq!(get(&env, "GIT_AUTHOR_NAME"), Some("Ann"));
        assert_eq!(
            get(&env, "GIT_AUTHOR_EMAIL"),
            Some("1+octo@users.noreply.github.com")
        );
        // A newer source the daemon does not know is the machine's own GitHub.
        let mut env = vec![];
        put_env(&spec("team", false, true), &mut env);
        assert_eq!(get(&env, "GIT_CONFIG_COUNT"), None);
    }

    /// App mode: github.com's helpers are cleared, then `chat-run git-credential` is the only
    /// one; config already given on the command line is kept; SSH goes over HTTPS when asked.
    #[test]
    fn app_mode_sets_the_helper_and_never_a_token() {
        let mut env = vec![
            ("GIT_CONFIG_COUNT".to_string(), "1".to_string()),
            ("GIT_CONFIG_KEY_0".to_string(), "core.pager".to_string()),
            ("GIT_CONFIG_VALUE_0".to_string(), "cat".to_string()),
            ("SSH_AUTH_SOCK".to_string(), "/s".to_string()),
        ];
        put_env(&spec("app", false, true), &mut env);
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
        assert_eq!(get(&env, ENV_AS), Some("octo"));
        assert!(
            env.iter()
                .all(|(k, v)| !k.contains("TOKEN") && !v.contains("gho_"))
        );
        // Without rewriteSsh the machine's SSH agent stays.
        let mut env = vec![("SSH_AUTH_SOCK".to_string(), "/s".to_string())];
        put_env(&spec("app", false, false), &mut env);
        assert_eq!(get(&env, "SSH_AUTH_SOCK"), Some("/s"));
        assert_eq!(config(&env).len(), 2);
    }

    /// The daemon's own fetch: the token as a header for github.com only, no helper, and no
    /// credentials at all when the person has none.
    #[test]
    fn the_daemons_own_fetch_uses_the_token_as_a_header() {
        assert!(fetch_env(&spec("machine", false, false), Some("t")).is_empty());
        let env = fetch_env(&spec("app", false, true), Some("gho_abc"));
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
        let none = fetch_env(&spec("app", false, false), None);
        assert!(
            none.iter()
                .all(|(_, v)| !v.as_deref().unwrap_or("").contains("Basic"))
        );
    }

    #[test]
    fn the_note_names_the_person_and_the_org_page() {
        assert_eq!(note(&spec("machine", true, false)), None);
        let n = note(&spec("app", true, false)).unwrap();
        assert!(n.contains("act as octo (Ann, who asked)"), "{n}");
        assert!(n.contains("Do not run gh auth login"), "{n}");
        assert!(n.contains("oauth_application_policy"), "{n}");
        let mut s = spec("app", false, false);
        s.login = None;
        let n = note(&s).unwrap();
        assert!(
            n.contains("the GitHub account of the person who asked"),
            "{n}"
        );
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
