//! Feature "accounts" (docs/machine-protocol.md, "Accounts and keys"): the owner signs the
//! machine's CLIs in and gives it API keys from the app, without a shell on the machine.
//!
//! - **Machine key**: an X25519 key pair made once, next to the token file (`machine.key`, mode
//!   600). Its public half goes in `hello.machine.publicKey`; the app seals each API key to it, so
//!   the server only ever relays ciphertext.
//! - **Keys**: `secrets.json` next to it (mode 600) holds each key's value, the AI members whose
//!   runs get it (empty = every run) and when it changed. Runs get them as environment variables;
//!   every value is masked in what the daemon sends. Only names and last 4 characters leave.
//! - **Sign-in**: the CLI's own login runs on the machine. Claude Code (`claude auth login
//!   --claudeai`) prints a link and waits for the `code#state` the person pastes; Codex
//!   (`codex login --device-auth`) prints a link and a one-time code. The daemon relays the link
//!   and code as `account.login.status`, and writes a pasted code to the CLI's standard input.
//!   Credentials stay in the CLI's own files on the machine.

use crate::config::{Config, RuntimeConfig};
use crate::proto::{LoginAccount, LoginStatus, SecretInfo, login_phase};
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as B64;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt};
use tokio::sync::mpsc;

macro_rules! log {
    ($($a:tt)*) => {{
        eprintln!("{} {}", crate::proto::now_ms(), format!($($a)*));
    }};
}

/// The folder of the machine key and the key store: the token file's.
fn dir(cfg: &Config) -> PathBuf {
    cfg.token_file
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("."))
}

fn write_private(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let tmp = path.with_extension("tmp");
    {
        let mut f = crate::sys::mode(
            std::fs::OpenOptions::new()
                .write(true)
                .create(true)
                .truncate(true),
            0o600,
        )
        .open(&tmp)?;
        std::io::Write::write_all(&mut f, bytes)?;
        f.sync_all()?;
    }
    std::fs::rename(&tmp, path)
}

/// Group- or world-readable files are refused, like the token file.
fn private_enough(path: &Path) -> bool {
    std::fs::metadata(path)
        .map(|m| !crate::sys::open_to_others(&m))
        .unwrap_or(false)
}

/// The machine's X25519 key pair.
pub struct MachineKey {
    secret: crypto_box::SecretKey,
}

impl MachineKey {
    /// Loads `machine.key`, or makes it on first use. None when it cannot be read or written
    /// safely (the daemon then does not offer the feature).
    pub fn load_or_create(cfg: &Config) -> Option<MachineKey> {
        let path = dir(cfg).join("machine.key");
        if path.exists() {
            if !private_enough(&path) {
                log!("machine.key is readable by others; keys from the app are off");
                return None;
            }
            let bytes = std::fs::read(&path).ok()?;
            let raw: [u8; 32] = bytes.as_slice().try_into().ok()?;
            return Some(MachineKey {
                secret: crypto_box::SecretKey::from(raw),
            });
        }
        let secret = crypto_box::SecretKey::generate(&mut crypto_box::aead::OsRng);
        if let Err(e) = write_private(&path, &secret.to_bytes()) {
            log!("cannot write machine.key: {e}; keys from the app are off");
            return None;
        }
        Some(MachineKey { secret })
    }

    /// For tests.
    pub fn from_bytes(raw: [u8; 32]) -> MachineKey {
        MachineKey {
            secret: crypto_box::SecretKey::from(raw),
        }
    }

    /// base64 of the 32-byte public key.
    pub fn public_b64(&self) -> String {
        B64.encode(self.secret.public_key().as_bytes())
    }

    /// Opens a base64 sealed box made for this key.
    pub fn unseal(&self, sealed_b64: &str) -> Result<String, String> {
        let sealed = B64
            .decode(sealed_b64.trim())
            .map_err(|_| "the key is not base64".to_string())?;
        let plain = self
            .secret
            .unseal(&sealed)
            .map_err(|_| "the key was not sealed for this machine".to_string())?;
        String::from_utf8(plain).map_err(|_| "the key is not text".to_string())
    }
}

/// `[A-Z_][A-Z0-9_]{0,63}`: an environment variable name.
pub fn valid_name(name: &str) -> bool {
    let b = name.as_bytes();
    !b.is_empty()
        && b.len() <= 64
        && (b[0].is_ascii_uppercase() || b[0] == b'_')
        && b.iter()
            .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || *c == b'_')
}

/// Names a key may not take: the daemon's own run variables and what every process needs.
fn reserved(name: &str) -> bool {
    name.starts_with("CHAT_")
        || matches!(
            name,
            "PATH"
                | "HOME"
                | "USER"
                | "LOGNAME"
                | "SHELL"
                | "TMPDIR"
                | "TERM"
                | "CODEX_HOME"
                | "CLAUDE_CONFIG_DIR"
                | "SSH_AUTH_SOCK"
        )
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
struct StoredSecret {
    value: String,
    #[serde(default)]
    members: Vec<String>,
    updated_at: u64,
}

/// The keys the machine holds (`secrets.json`, mode 600).
pub struct SecretStore {
    path: PathBuf,
    keys: BTreeMap<String, StoredSecret>,
}

impl SecretStore {
    pub fn load(cfg: &Config) -> SecretStore {
        Self::at(dir(cfg).join("secrets.json"))
    }

    pub fn at(path: PathBuf) -> SecretStore {
        let keys = if path.exists() && !private_enough(&path) {
            log!("secrets.json is readable by others; its keys are not used");
            BTreeMap::new()
        } else {
            std::fs::read(&path)
                .ok()
                .and_then(|b| serde_json::from_slice(&b).ok())
                .unwrap_or_default()
        };
        SecretStore { path, keys }
    }

    fn save(&self) -> Result<(), String> {
        let bytes = serde_json::to_vec_pretty(&self.keys).map_err(|e| e.to_string())?;
        write_private(&self.path, &bytes).map_err(|e| format!("cannot save the key: {e}"))
    }

    pub fn set(&mut self, name: &str, value: String, members: Vec<String>) -> Result<(), String> {
        if !valid_name(name) || reserved(name) {
            return Err(format!("{name} is not a name a key can take"));
        }
        let value = value.trim().to_string();
        if value.is_empty() || value.len() > 8192 {
            return Err("the key is empty or too long".into());
        }
        self.keys.insert(
            name.to_string(),
            StoredSecret {
                value,
                members,
                updated_at: crate::proto::now_ms(),
            },
        );
        self.save()
    }

    pub fn delete(&mut self, name: &str) -> Result<(), String> {
        if self.keys.remove(name).is_some() {
            self.save()?;
        }
        Ok(())
    }

    /// Names and last 4 characters, for the heartbeat.
    pub fn infos(&self) -> Vec<SecretInfo> {
        self.keys
            .iter()
            .map(|(name, s)| SecretInfo {
                name: name.clone(),
                last4: last4(&s.value),
                members: s.members.clone(),
                updated_at: s.updated_at,
            })
            .collect()
    }

    /// The keys a run of `member` gets.
    pub fn env_for(&self, member: &str) -> Vec<(String, String)> {
        self.keys
            .iter()
            .filter(|(_, s)| s.members.is_empty() || s.members.iter().any(|m| m == member))
            .map(|(k, s)| (k.clone(), s.value.clone()))
            .collect()
    }

    /// Every value, for the redactor.
    pub fn values(&self) -> Vec<String> {
        self.keys.values().map(|s| s.value.clone()).collect()
    }
}

/// The last 4 characters of a value (or fewer when it is short).
pub fn last4(v: &str) -> String {
    let chars: Vec<char> = v.chars().collect();
    if chars.len() < 12 {
        return String::new();
    }
    chars[chars.len() - 4..].iter().collect()
}

// ---------------------------------------------------------------------------
// Sign-in

/// A message to a running sign-in.
pub enum LoginCtl {
    Code(String),
    Cancel,
}

/// How long a sign-in may wait for the person.
const LOGIN_LIMIT: Duration = Duration::from_secs(15 * 60);

/// Removes ANSI escape sequences.
fn plain(s: &str) -> String {
    crate::runtime::pty::strip_ansi(s)
}

/// The first https link in a line.
pub fn find_url(line: &str) -> Option<String> {
    let i = line.find("https://")?;
    let url: String = line[i..]
        .chars()
        .take_while(|c| !c.is_whitespace() && *c != '"' && *c != '\'')
        .collect();
    (url.len() > 8).then_some(url)
}

/// A one-time code such as `ABCD-12345`.
pub fn find_user_code(line: &str) -> Option<String> {
    let re = regex::Regex::new(r"\b([A-Z0-9]{4}-[A-Z0-9]{4,6})\b").ok()?;
    re.captures(line).map(|c| c[1].to_string())
}

/// The environment of a sign-in: what a run gets from the daemon, plus the runtime's own env.
fn login_env(rt: &RuntimeConfig) -> Vec<(String, String)> {
    let mut env: Vec<(String, String)> = [
        "PATH",
        "HOME",
        "USER",
        "LOGNAME",
        "SHELL",
        "TMPDIR",
        "LANG",
        "LC_ALL",
        "CODEX_HOME",
        "CLAUDE_CONFIG_DIR",
    ]
    .iter()
    .filter_map(|k| std::env::var(k).ok().map(|v| (k.to_string(), v)))
    .collect();
    for (k, v) in &rt.env {
        env.retain(|(n, _)| n != k);
        env.push((k.clone(), v.clone()));
    }
    // The link goes to the person through the app, never to a browser on the machine.
    env.push(("BROWSER".into(), "true".into()));
    env.push(("NO_COLOR".into(), "1".into()));
    env
}

/// Runs one sign-in to its end and reports each step with `send`.
pub async fn run_login(
    login_id: String,
    runtime: String,
    rt: RuntimeConfig,
    mut ctl: mpsc::UnboundedReceiver<LoginCtl>,
    send: mpsc::UnboundedSender<LoginStatus>,
) {
    let status = |state: &str| LoginStatus {
        login_id: login_id.clone(),
        runtime: runtime.clone(),
        state: state.to_string(),
        url: None,
        user_code: None,
        account: None,
        error: None,
    };
    let fail = |e: String| LoginStatus {
        error: Some(crate::proto::truncate(&e, 900)),
        ..status(login_phase::FAILED)
    };
    let _ = send.send(status(login_phase::STARTING));
    let args: Vec<&str> = match rt.kind.as_str() {
        "claude" => vec!["auth", "login", "--claudeai"],
        "codex" => vec!["login", "--device-auth"],
        k => {
            let _ = send.send(fail(format!("a {k} runtime has no sign-in")));
            return;
        }
    };
    let Some(program) = crate::health::which(&rt.command()) else {
        let _ = send.send(fail(format!("{} is not installed", rt.command())));
        return;
    };
    let mut child = match crate::health::command(&program)
        .args(&args)
        .env_clear()
        .envs(login_env(&rt))
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true)
        .spawn()
    {
        Ok(c) => c,
        Err(e) => {
            let _ = send.send(fail(format!("cannot start {}: {e}", rt.command())));
            return;
        }
    };
    let mut stdin = child.stdin.take();
    let (lines_tx, mut lines) = mpsc::unbounded_channel::<String>();
    for out in [
        child
            .stdout
            .take()
            .map(|s| Box::new(s) as Box<dyn tokio::io::AsyncRead + Unpin + Send>),
        child
            .stderr
            .take()
            .map(|s| Box::new(s) as Box<dyn tokio::io::AsyncRead + Unpin + Send>),
    ]
    .into_iter()
    .flatten()
    {
        let tx = lines_tx.clone();
        tokio::spawn(async move {
            // The prompt ("Paste code here if prompted >") has no newline: read chunks.
            let mut r = tokio::io::BufReader::new(out);
            let mut buf = vec![0u8; 4096];
            while let Ok(n) = r.read(&mut buf).await {
                if n == 0 {
                    break;
                }
                let _ = tx.send(String::from_utf8_lossy(&buf[..n]).to_string());
            }
        });
    }
    drop(lines_tx);
    let deadline = tokio::time::sleep(LOGIN_LIMIT);
    tokio::pin!(deadline);
    let mut url: Option<String> = None;
    let mut code: Option<String> = None;
    let mut text = String::new();
    let mut reported = false;
    let exit = loop {
        tokio::select! {
            l = lines.recv() => match l {
                Some(chunk) => {
                    text.push_str(&plain(&chunk));
                    if text.len() > 64 * 1024 {
                        text.drain(..text.len() - 32 * 1024);
                    }
                    for line in text.lines() {
                        if url.is_none() && let Some(u) = find_url(line) {
                            // Claude prints the manual link (paste the code) after "visit:".
                            url = Some(u);
                        }
                        if rt.kind == "codex" && code.is_none() && let Some(c) = find_user_code(line) {
                            code = Some(c);
                        }
                    }
                    let ready = match rt.kind.as_str() {
                        "codex" => url.is_some() && code.is_some(),
                        _ => url.is_some(),
                    };
                    if ready && !reported {
                        reported = true;
                        let state = if rt.kind == "codex" { login_phase::DEVICE_CODE } else { login_phase::OPEN_URL };
                        let _ = send.send(LoginStatus { url: url.clone(), user_code: code.clone(), ..status(state) });
                    }
                }
                None => break child.wait().await.ok(),
            },
            c = ctl.recv() => match c {
                Some(LoginCtl::Code(c)) => {
                    let c = c.trim().to_string();
                    if c.is_empty() || c.contains(char::is_whitespace) {
                        continue;
                    }
                    if let Some(i) = stdin.as_mut() {
                        let _ = i.write_all(format!("{c}\n").as_bytes()).await;
                        let _ = i.flush().await;
                        let _ = send.send(status(login_phase::CHECKING));
                    }
                }
                Some(LoginCtl::Cancel) | None => {
                    let _ = child.kill().await;
                    let _ = send.send(status(login_phase::CANCELLED));
                    return;
                }
            },
            _ = &mut deadline => {
                let _ = child.kill().await;
                let _ = send.send(fail("the sign-in waited 15 minutes and stopped".into()));
                return;
            }
        }
    };
    let ok = exit.is_some_and(|s| s.success());
    if !ok {
        let tail: String = text
            .lines()
            .rev()
            .map(str::trim)
            .find(|l| !l.is_empty() && !l.contains("https://"))
            .unwrap_or("the sign-in did not finish")
            .chars()
            .take(300)
            .collect();
        let _ = send.send(fail(tail));
        return;
    }
    let account = account_of(&rt).await;
    let _ = send.send(LoginStatus {
        account,
        ..status(login_phase::DONE)
    });
}

/// Who the CLI is signed in as now, when it says.
pub async fn account_of(rt: &RuntimeConfig) -> Option<LoginAccount> {
    let program = crate::health::which(&rt.command())?;
    match rt.kind.as_str() {
        "claude" => {
            let out = tokio::time::timeout(
                Duration::from_secs(20),
                crate::health::command(&program)
                    .args(["auth", "status", "--json"])
                    .env_clear()
                    .envs(login_env(rt))
                    .stdin(std::process::Stdio::null())
                    .kill_on_drop(true)
                    .output(),
            )
            .await
            .ok()?
            .ok()?;
            let v: serde_json::Value = serde_json::from_slice(&out.stdout).ok()?;
            if v.get("loggedIn").and_then(|x| x.as_bool()) != Some(true) {
                return None;
            }
            let s = |k: &str| v.get(k).and_then(|x| x.as_str()).map(str::to_string);
            Some(LoginAccount {
                email: s("email"),
                plan: s("subscriptionType").or_else(|| s("authMethod")),
            })
        }
        "codex" => {
            let out = tokio::time::timeout(
                Duration::from_secs(20),
                crate::health::command(&program)
                    .args(["login", "status"])
                    .env_clear()
                    .envs(login_env(rt))
                    .stdin(std::process::Stdio::null())
                    .kill_on_drop(true)
                    .output(),
            )
            .await
            .ok()?
            .ok()?;
            let text = format!(
                "{}{}",
                String::from_utf8_lossy(&out.stdout),
                String::from_utf8_lossy(&out.stderr)
            );
            // Only the words are read: the line may hold part of an API key.
            let plan = if text.contains("ChatGPT") {
                "chatgpt"
            } else if text.contains("API key") {
                "api_key"
            } else {
                return None;
            };
            Some(LoginAccount {
                email: None,
                plan: Some(plan.into()),
            })
        }
        _ => None,
    }
}

/// Reads lines from an async reader (used by tests).
pub async fn read_lines<R: tokio::io::AsyncRead + Unpin>(r: R) -> Vec<String> {
    let mut out = vec![];
    let mut lines = tokio::io::BufReader::new(r).lines();
    while let Ok(Some(l)) = lines.next_line().await {
        out.push(l);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_sealed_key_opens_only_with_the_machine_key() {
        let key = MachineKey::from_bytes([7u8; 32]);
        let other = MachineKey::from_bytes([9u8; 32]);
        let public = crypto_box::PublicKey::from(
            <[u8; 32]>::try_from(B64.decode(key.public_b64()).unwrap().as_slice()).unwrap(),
        );
        let sealed = public
            .seal(&mut crypto_box::aead::OsRng, b"sk-test-value-1234")
            .unwrap();
        let b64 = B64.encode(&sealed);
        assert_eq!(key.unseal(&b64).unwrap(), "sk-test-value-1234");
        assert!(other.unseal(&b64).is_err());
        assert!(key.unseal("not base64!").is_err());
    }

    #[test]
    fn the_store_keeps_values_private_and_reports_only_names_and_last4() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("secrets.json");
        let mut s = SecretStore::at(path.clone());
        s.set("OPENAI_API_KEY", "sk-proj-abcdefghij9xQz".into(), vec![])
            .unwrap();
        s.set(
            "GITHUB_TOKEN",
            "ghp_0123456789abcdefa1b2".into(),
            vec!["bot1".into()],
        )
        .unwrap();
        assert!(s.set("PATH", "x".repeat(20), vec![]).is_err());
        assert!(s.set("CHAT_RUN_ID", "x".repeat(20), vec![]).is_err());
        assert!(s.set("lower", "x".repeat(20), vec![]).is_err());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }
        let infos = s.infos();
        assert_eq!(infos.len(), 2);
        assert_eq!(infos[1].last4, "9xQz");
        let json = serde_json::to_string(&infos).unwrap();
        assert!(!json.contains("abcdefghij"));
        let s = SecretStore::at(path);
        assert_eq!(s.env_for("bot2").len(), 1);
        assert_eq!(s.env_for("bot1").len(), 2);
        assert_eq!(last4("short"), "");
    }

    #[test]
    fn links_and_codes_are_found_in_cli_output() {
        let l = "If the browser didn't open, visit: https://claude.com/cai/oauth/authorize?code=true&x=1";
        assert_eq!(
            find_url(l).as_deref(),
            Some("https://claude.com/cai/oauth/authorize?code=true&x=1")
        );
        let c = plain("   \u{1b}[94mABCD-12345\u{1b}[0m");
        assert_eq!(find_user_code(&c).as_deref(), Some("ABCD-12345"));
        assert_eq!(find_user_code("codex-cli 0.167.0"), None);
    }
}
