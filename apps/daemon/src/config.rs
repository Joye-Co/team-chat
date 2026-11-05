//! Daemon config file (TOML).

use serde::Deserialize;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    /// Base URL of the API Worker, e.g. https://chat-api-staging.example.workers.dev
    pub server_url: String,
    /// File that holds the machine token (mode 600).
    pub token_file: PathBuf,
    #[serde(default)]
    pub machine_id: Option<String>,
    #[serde(default = "default_name")]
    pub name: String,
    #[serde(default)]
    pub labels: Vec<String>,
    #[serde(default = "default_work_root")]
    pub work_root: PathBuf,
    #[serde(default = "default_max_parallel")]
    pub max_parallel_runs: u32,
    /// Finished run folders older than this are removed.
    #[serde(default = "default_keep_hours")]
    pub keep_runs_hours: u64,
    /// Below this free disk space the daemon removes old run folders first, then rejects runs.
    #[serde(default = "default_min_free_gb")]
    pub min_free_gb: u64,
    /// Emit a `stalled` status after this many minutes without output from the CLI.
    #[serde(default = "default_stall_minutes")]
    pub stall_minutes: u64,
    /// When not empty, `repo.path` of an assignment must be inside one of these folders.
    #[serde(default)]
    pub repo_roots: Vec<PathBuf>,
    /// Environment variables an assignment may ask for through `envAllowlist`.
    #[serde(default)]
    pub env_passthrough: Vec<String>,
    #[serde(default = "default_heartbeat_secs")]
    pub heartbeat_secs: u64,
    /// Screen sharing address people use for 接管 (e.g. `vnc://mac-mini.tailnet`), reported in
    /// `hello`. The owner turns on macOS Screen Sharing by hand; never put credentials here.
    #[serde(default)]
    pub takeover_url: Option<String>,
    /// Runtimes by name. When empty, `codex` and `claude` are added if found on PATH.
    #[serde(default)]
    pub runtimes: BTreeMap<String, RuntimeConfig>,
}

/// Why a `takeover_url` cannot be reported, or None when it is fine: it must be a
/// `scheme://host...` address of at most 2048 bytes, without spaces and without a password.
pub fn takeover_url_problem(url: &str) -> Option<&'static str> {
    if url.len() > 2048 {
        return Some("takeover_url must be at most 2048 bytes");
    }
    if url.chars().any(|c| c.is_whitespace() || c.is_control()) {
        return Some("takeover_url must not contain spaces");
    }
    let Some((scheme, rest)) = url.split_once("://") else {
        return Some("takeover_url must look like vnc://host");
    };
    let scheme_ok = !scheme.is_empty()
        && scheme.starts_with(|c: char| c.is_ascii_alphabetic())
        && scheme
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'));
    let authority = rest.split(['/', '?', '#']).next().unwrap_or_default();
    if !scheme_ok || authority.is_empty() {
        return Some("takeover_url must look like vnc://host");
    }
    if authority
        .rsplit_once('@')
        .is_some_and(|(user, _)| user.contains(':'))
    {
        return Some("takeover_url must not contain a password");
    }
    None
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RuntimeConfig {
    /// "codex" | "claude" | "acp" | "pty"
    pub kind: String,
    #[serde(default)]
    pub command: Option<String>,
    #[serde(default)]
    pub args: Vec<String>,
    /// Fixed environment for this runtime (not secrets: put those in env_passthrough).
    #[serde(default)]
    pub env: BTreeMap<String, String>,
    #[serde(default)]
    pub model: Option<String>,
    /// codex: approval policy ("never", "on-request", "untrusted"). Default "never".
    #[serde(default)]
    pub approval_policy: Option<String>,
    /// codex: sandbox ("read-only", "workspace-write", "danger-full-access"). Default "danger-full-access".
    #[serde(default)]
    pub sandbox: Option<String>,
    /// claude: permission mode. Default "bypassPermissions".
    #[serde(default)]
    pub permission_mode: Option<String>,
    /// claude: --allowedTools value.
    #[serde(default)]
    pub allowed_tools: Option<String>,
}

impl RuntimeConfig {
    pub fn command(&self) -> String {
        self.command
            .clone()
            .unwrap_or_else(|| match self.kind.as_str() {
                "codex" => "codex".into(),
                "claude" => "claude".into(),
                _ => String::new(),
            })
    }
}

fn default_name() -> String {
    let mut buf = [0u8; 256];
    // SAFETY: buf is valid for writes of buf.len() bytes.
    let rc = unsafe { libc::gethostname(buf.as_mut_ptr() as *mut libc::c_char, buf.len()) };
    if rc == 0 {
        let end = buf.iter().position(|b| *b == 0).unwrap_or(buf.len());
        String::from_utf8_lossy(&buf[..end]).into_owned()
    } else {
        "machine".into()
    }
}
fn default_work_root() -> PathBuf {
    home().join(format!("{}-runs", crate::product::slug()))
}
fn default_max_parallel() -> u32 {
    2
}
fn default_keep_hours() -> u64 {
    72
}
fn default_min_free_gb() -> u64 {
    10
}
fn default_stall_minutes() -> u64 {
    5
}
fn default_heartbeat_secs() -> u64 {
    30
}

pub fn home() -> PathBuf {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/"))
}

pub fn expand(p: &Path) -> PathBuf {
    match p.to_str() {
        Some("~") => home(),
        Some(s) if s.starts_with("~/") => home().join(&s[2..]),
        _ => p.to_path_buf(),
    }
}

pub fn default_config_path() -> PathBuf {
    if let Some(p) = std::env::var_os("CHAT_DAEMON_CONFIG") {
        return PathBuf::from(p);
    }
    home()
        .join(".config")
        .join(crate::product::slug())
        .join("daemon.toml")
}

impl Config {
    pub fn load(path: &Path) -> Result<Config, String> {
        let text = std::fs::read_to_string(path)
            .map_err(|e| format!("cannot read config {}: {e}", path.display()))?;
        Self::parse(&text)
    }

    pub fn parse(text: &str) -> Result<Config, String> {
        let mut c: Config = toml::from_str(text).map_err(|e| format!("config: {e}"))?;
        c.token_file = expand(&c.token_file);
        c.work_root = expand(&c.work_root);
        c.repo_roots = c.repo_roots.iter().map(|p| expand(p)).collect();
        if c.runtimes.is_empty() {
            for kind in ["codex", "claude"] {
                if crate::health::which(kind).is_some() {
                    c.runtimes.insert(
                        kind.into(),
                        RuntimeConfig {
                            kind: kind.into(),
                            command: None,
                            args: vec![],
                            env: BTreeMap::new(),
                            model: None,
                            approval_policy: None,
                            sandbox: None,
                            permission_mode: None,
                            allowed_tools: None,
                        },
                    );
                }
            }
        }
        Ok(c)
    }

    /// Problems that stop the daemon from working. Never includes secret values.
    pub fn problems(&self) -> Vec<String> {
        let mut out = vec![];
        let url = self.server_url.as_str();
        let local = url.starts_with("http://127.0.0.1")
            || url.starts_with("http://localhost")
            || url.starts_with("http://[::1]");
        if !(url.starts_with("https://") || local) {
            out.push("server_url must use https:// (http:// only for localhost)".into());
        }
        if self.max_parallel_runs == 0 {
            out.push("max_parallel_runs must be at least 1".into());
        }
        match std::fs::metadata(&self.token_file) {
            Err(_) => out.push(format!(
                "token file {} does not exist",
                self.token_file.display()
            )),
            Ok(m) => {
                use std::os::unix::fs::PermissionsExt;
                if m.permissions().mode() & 0o077 != 0 {
                    out.push(format!(
                        "token file {} must have mode 600",
                        self.token_file.display()
                    ));
                }
            }
        }
        for (name, rt) in &self.runtimes {
            if !matches!(rt.kind.as_str(), "codex" | "claude" | "acp" | "pty") {
                out.push(format!("runtime {name}: unknown kind {}", rt.kind));
            }
            if rt.command().is_empty() {
                out.push(format!("runtime {name}: command is required"));
            }
        }
        if let Some(p) = self.takeover_url.as_deref().and_then(takeover_url_problem) {
            out.push(p.into());
        }
        if self.runtimes.is_empty() {
            out.push("no runtimes: install codex or claude, or add [runtimes.*]".into());
        }
        out
    }

    pub fn read_token(&self) -> Result<String, String> {
        let t = std::fs::read_to_string(&self.token_file)
            .map_err(|e| format!("cannot read token file {}: {e}", self.token_file.display()))?;
        let t = t.trim().to_string();
        if t.is_empty() {
            return Err("token file is empty".into());
        }
        Ok(t)
    }

    pub fn ws_url(&self) -> String {
        let base = self.server_url.trim_end_matches('/');
        let base = if let Some(r) = base.strip_prefix("https://") {
            format!("wss://{r}")
        } else if let Some(r) = base.strip_prefix("http://") {
            format!("ws://{r}")
        } else {
            base.to_string()
        };
        format!("{base}/v1/machine/ws")
    }
}

pub const EXAMPLE: &str = r#"# Machine daemon config
server_url = "https://chat-api-staging.example.workers.dev"
token_file = "~/.config/team-chat/staging/machine-token"   # mode 600
name = "mac-mini"
labels = ["mac", "main"]
work_root = "~/team-chat-runs"
max_parallel_runs = 2
keep_runs_hours = 72
min_free_gb = 10
stall_minutes = 5
repo_roots = []                     # e.g. ["~/code"]; empty allows any local repo path
env_passthrough = []                # e.g. ["GITHUB_TOKEN"]; assignments pick from these
# takeover_url = "vnc://mac-mini.tailnet"   # 接管: Screen Sharing address (turn Sharing on by hand; no passwords)

[runtimes.codex]
kind = "codex"
sandbox = "danger-full-access"
approval_policy = "never"

[runtimes.claude]
kind = "claude"
permission_mode = "bypassPermissions"

# [runtimes.gemini]
# kind = "acp"
# command = "gemini"
# args = ["--acp"]

# [runtimes.shell]
# kind = "pty"
# command = "/bin/sh"
# args = ["-c", "{prompt}"]
"#;

#[cfg(test)]
mod tests {
    use super::takeover_url_problem as p;

    #[test]
    fn takeover_url_rules() {
        assert_eq!(p("vnc://mac-mini.tailnet"), None);
        assert_eq!(p("vnc://joye@mac-mini.local:5900/"), None);
        assert!(p("vnc://joye:hunter2@mac-mini.local").is_some());
        assert!(p("mac-mini.local").is_some());
        assert!(p("vnc://").is_some());
        assert!(p("vnc://mac mini").is_some());
        assert!(p(&format!("vnc://{}", "a".repeat(2050))).is_some());
    }
}
