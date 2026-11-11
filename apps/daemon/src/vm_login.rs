//! What a run in a microVM gets of this host's CLI sign-ins (机 3.2).
//!
//! The host's sign-in folders (`~/.claude`, `~/.codex`, `~/.pi`) are never shared with a VM.
//! Each run's CLI uses its member's own folders instead (`<member>/.claude`, `.codex`,
//! `.pi/agent`, see [`crate::vm_home::member_dir`]), so session records stay per member, and
//! gets the sign-in in a form it cannot use to change this host's:
//!
//! - **Claude Code**: a token in `CLAUDE_CODE_OAUTH_TOKEN`. A long-lived token
//!   (`claude setup-token`) given to the machine as an API key of that name (or the runtime's
//!   own `ANTHROPIC_API_KEY`) is used as it is; otherwise the host's current access token, never
//!   its refresh token. When that runs out before the run could end, the host CLI refreshes it
//!   first.
//! - **Codex**: a copy of `auth.json` without the refresh token, written for the run (tested
//!   with Codex 0.159: a copy whose `refresh_token` is empty completes a task with the access
//!   token; a copy without the field is not signed in). Access tokens last 10 days; when the
//!   host's runs out before the run could end, the host's Codex refreshes it first
//!   (`account/read` with `refreshToken`), so refreshing stays on this host.
//! - **Pi**: a copy of `auth.json` with each `refresh` emptied, and its settings.
//!
//! The host's settings (`settings.json`, `config.toml`) are copied for each run. A run that
//! resumes a session from before this (kept in the host's folders) gets that one session file
//! copied into its member's folder.

use crate::config::RuntimeConfig;
use base64::Engine as _;
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

/// The variable Claude Code takes a token from.
pub const CLAUDE_TOKEN: &str = "CLAUDE_CODE_OAUTH_TOKEN";
/// A run that has one of these already brings its own Claude credential.
const CLAUDE_OWN: [&str; 3] = [CLAUDE_TOKEN, "ANTHROPIC_API_KEY", "ANTHROPIC_AUTH_TOKEN"];

/// The CLIs' folders of a member, as the run's environment names them.
pub fn cli_env(member: &Path) -> Vec<(String, String)> {
    let p = |rel: &str| member.join(rel).to_string_lossy().to_string();
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

/// Writes a file only the daemon user can read.
fn write_private(path: &Path, text: &str) -> std::io::Result<()> {
    if let Some(d) = path.parent() {
        std::fs::create_dir_all(d)?;
    }
    let tmp = path.with_extension("tmp");
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

/// A session of the member from before per-member folders: copies `<sid>.jsonl` of the host's
/// Claude folder into the member's, under the same project folder. True when it is there now.
pub fn carry_claude_session(host: &Path, member: &Path, cwd: &Path, sid: &str) -> bool {
    use crate::runtime::claude::find_session_file;
    if !crate::runtime::valid_session_id(sid) {
        return false;
    }
    if find_session_file(member, cwd, sid).is_some() {
        return true;
    }
    let Some(found) = find_session_file(host, cwd, sid) else {
        return false;
    };
    let Some(project) = found.parent().and_then(|p| p.file_name()) else {
        return false;
    };
    let dir = member.join("projects").join(project);
    std::fs::create_dir_all(&dir).is_ok()
        && std::fs::copy(&found, dir.join(format!("{sid}.jsonl"))).is_ok()
}

/// A Codex thread's rollout file under `sessions/` (`YYYY/MM/DD/rollout-<time>-<id>.jsonl`).
fn find_rollout(home: &Path, sid: &str) -> Option<PathBuf> {
    fn walk(dir: &Path, suffix: &str, depth: u32) -> Option<PathBuf> {
        for e in std::fs::read_dir(dir).ok()?.flatten() {
            let p = e.path();
            if p.is_dir() {
                if depth < 4
                    && let Some(f) = walk(&p, suffix, depth + 1)
                {
                    return Some(f);
                }
            } else if e
                .file_name()
                .to_str()
                .is_some_and(|n| n.starts_with("rollout-") && n.ends_with(suffix))
            {
                return Some(p);
            }
        }
        None
    }
    walk(&home.join("sessions"), &format!("-{sid}.jsonl"), 0)
}

/// [`carry_claude_session`] for a Codex thread: the rollout file keeps its place under
/// `sessions/`.
pub fn carry_codex_session(host: &Path, member: &Path, sid: &str) -> bool {
    if !crate::runtime::valid_session_id(sid) {
        return false;
    }
    if find_rollout(member, sid).is_some() {
        return true;
    }
    let Some(found) = find_rollout(host, sid) else {
        return false;
    };
    let Ok(rel) = found.strip_prefix(host) else {
        return false;
    };
    let target = member.join(rel);
    target
        .parent()
        .is_some_and(|d| std::fs::create_dir_all(d).is_ok())
        && std::fs::copy(&found, &target).is_ok()
}

/// Readies the CLI's folders and sign-in of a VM run of the member whose folder is `member`:
/// sets the CLIs' folders in `env` and, for Claude Code, the token. Returns the token the run
/// got in its environment (to be masked), if any. `need`: how long the run may take.
pub async fn prepare(
    rt: &RuntimeConfig,
    member: &Path,
    cwd: &Path,
    resume: Option<&str>,
    need: Duration,
    env: &mut Vec<(String, String)>,
) -> Result<Option<String>, String> {
    for (k, v) in cli_env(member) {
        env.retain(|(n, _)| *n != k);
        env.push((k, v));
    }
    let until = now_ms() + need.as_millis() as u64;
    match rt.kind.as_str() {
        "claude" => {
            let host = host_dir(rt, "CLAUDE_CONFIG_DIR", ".claude");
            let own = member.join(".claude");
            std::fs::create_dir_all(&own).map_err(|e| format!("{}: {e}", own.display()))?;
            copy_setting(&host, &own, "settings.json");
            if let Some(sid) = resume {
                carry_claude_session(&host, &own, cwd, sid);
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
            let mut found = read();
            if found
                .as_ref()
                .is_some_and(|(_, exp)| exp.is_some_and(|e| e < until))
            {
                // Claude Code refreshes its sign-in when it talks to the service.
                crate::usage::read_all(vec![("claude".into(), rt.clone())]).await;
                found = read();
            }
            Ok(found.map(|(token, _)| {
                env.push((CLAUDE_TOKEN.into(), token.clone()));
                token
            }))
        }
        "codex" => {
            let host = host_dir(rt, "CODEX_HOME", ".codex");
            let own = member.join(".codex");
            std::fs::create_dir_all(&own).map_err(|e| format!("{}: {e}", own.display()))?;
            copy_setting(&host, &own, "config.toml");
            if let Some(sid) = resume {
                carry_codex_session(&host, &own, sid);
            }
            let read = || std::fs::read_to_string(host.join("auth.json")).ok();
            let mut text = read();
            if text
                .as_deref()
                .and_then(codex_expires_at)
                .is_some_and(|e| e < until)
            {
                refresh_codex(rt).await;
                text = read();
            }
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
                let own = member.join(".pi/agent");
                copy_setting(&host, &own, "settings.json");
                copy_setting(&host, &own, "models.json");
                if let Some(copy) = std::fs::read_to_string(host.join("auth.json"))
                    .ok()
                    .and_then(|t| pi_run_auth(&t))
                {
                    let target = own.join("auth.json");
                    write_private(&target, &copy)
                        .map_err(|e| format!("{}: {e}", target.display()))?;
                }
            }
            Ok(None)
        }
    }
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
    fn a_resumed_session_from_the_shared_folder_is_copied_once() {
        let tmp = tempfile::tempdir().unwrap();
        let (host, member) = (tmp.path().join("host"), tmp.path().join("member"));
        let cwd = tmp.path().join("work");
        std::fs::create_dir_all(&cwd).unwrap();
        let old = host.join("projects").join("-old-work");
        std::fs::create_dir_all(&old).unwrap();
        std::fs::write(old.join("s1.jsonl"), "{}\n").unwrap();
        std::fs::write(old.join("other.jsonl"), "{}\n").unwrap();
        assert!(carry_claude_session(&host, &member, &cwd, "s1"));
        assert!(member.join("projects/-old-work/s1.jsonl").is_file());
        // Only the session the run resumes: no other member's records.
        assert!(!member.join("projects/-old-work/other.jsonl").exists());
        assert!(!carry_claude_session(&host, &member, &cwd, "missing"));
        assert!(!carry_claude_session(&host, &member, &cwd, "../x"));

        let rollout = host.join("sessions/2026/11/09");
        std::fs::create_dir_all(&rollout).unwrap();
        std::fs::write(rollout.join("rollout-2026-11-09T10-00-00-t1.jsonl"), "{}\n").unwrap();
        assert!(carry_codex_session(&host, &member, "t1"));
        assert!(
            member
                .join("sessions/2026/11/09/rollout-2026-11-09T10-00-00-t1.jsonl")
                .is_file()
        );
        assert!(!carry_codex_session(&host, &member, "t2"));
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
        let member = tmp.path().join("homes/01BOT");
        let mut env = vec![("CODEX_HOME".to_string(), host.display().to_string())];
        let r = rt("codex", &[("CODEX_HOME", &host)]);
        let got = prepare(
            &r,
            &member,
            tmp.path(),
            None,
            Duration::from_secs(9000),
            &mut env,
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
    }

    #[tokio::test]
    async fn a_claude_run_gets_the_hosts_access_token_unless_it_brings_its_own() {
        let tmp = tempfile::tempdir().unwrap();
        let host = tmp.path().join("host-claude");
        std::fs::create_dir_all(&host).unwrap();
        let creds = json!({ "claudeAiOauth": { "accessToken": "sk-ant-oat01-host", "refreshToken": "sk-ant-ort01-r", "expiresAt": now_ms() + 86_400_000 } });
        std::fs::write(host.join(".credentials.json"), creds.to_string()).unwrap();
        let member = tmp.path().join("homes/01BOT");
        let r = rt("claude", &[("CLAUDE_CONFIG_DIR", &host)]);
        let mut env = vec![];
        let got = prepare(
            &r,
            &member,
            tmp.path(),
            None,
            Duration::from_secs(9000),
            &mut env,
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
}
