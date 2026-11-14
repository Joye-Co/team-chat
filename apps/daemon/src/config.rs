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
    /// Variables a run gets only after a person approves (`chat-run secret`, see
    /// [`crate::secrets`]); an assignment's `envAllowlist` picks from these too.
    #[serde(default)]
    pub gated_env: Vec<String>,
    #[serde(default = "default_heartbeat_secs")]
    pub heartbeat_secs: u64,
    /// Screen sharing address people use for 接管 (e.g. `vnc://mac-mini.tailnet`), reported in
    /// `hello`. The owner turns on macOS Screen Sharing by hand; never put credentials here.
    #[serde(default)]
    pub takeover_url: Option<String>,
    /// A daemon installed on its own (a Linux host) updates itself from the server's releases
    /// ([`crate::selfupdate::Updater`]). `false` turns that off; a daemon the app installed is
    /// updated by the app and never updates itself.
    #[serde(default = "default_self_update")]
    pub self_update: bool,
    /// Runtimes by name. When empty, `codex` and `claude` are added if found on PATH.
    #[serde(default)]
    pub runtimes: BTreeMap<String, RuntimeConfig>,
    /// Run each CLI in its own Firecracker microVM (Linux hosts, see
    /// `scripts/runner/host-setup.sh`). Absent: CLIs run as processes of this machine.
    #[serde(default)]
    pub vm: Option<VmConfig>,
    /// Tests only: the daemon's clocks in milliseconds, so a test can freeze the daemon for
    /// seconds instead of over a minute. Never set in a real config.
    #[serde(default)]
    pub test_clocks: Option<TestClocks>,
}

impl Config {
    /// How much later than its period a tick must come to count as a sleep (None: the
    /// default, [`crate::sys::SLEEP_LATE`]).
    pub fn sleep_late(&self) -> Option<std::time::Duration> {
        self.test_clocks
            .and_then(|c| c.late_ms)
            .map(std::time::Duration::from_millis)
    }
}

/// Tests only (see [`Config::test_clocks`]). Each one left out keeps its usual value.
#[derive(Debug, Clone, Copy, Default, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct TestClocks {
    /// The stall limit, instead of `stall_minutes`.
    pub stall_ms: Option<u64>,
    /// The period of a run's supervisor tick (15 s otherwise).
    pub tick_ms: Option<u64>,
    /// How much later than its period a tick must come to count as a sleep (20 s otherwise).
    pub late_ms: Option<u64>,
    /// How long a gated run waits for the CLI's start-up hook (45 s otherwise).
    pub hooks_start_ms: Option<u64>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VmConfig {
    /// Guest kernel (uncompressed vmlinux).
    pub kernel: PathBuf,
    /// Read-only base image (`scripts/runner/build-base-image.sh`).
    pub rootfs: PathBuf,
    /// VM folders (sockets, scratch disks, console logs) and the tools disk.
    #[serde(default = "default_vm_dir")]
    pub dir: PathBuf,
    /// The VM's size when the run does not name one (`run.assign.vm`).
    #[serde(default = "default_vm_vcpus")]
    pub vcpus: u32,
    /// The VM's memory cap; the host gives only the pages the VM touches.
    #[serde(default = "default_vm_mem_mib")]
    pub mem_mib: u32,
    /// The most a run's own size (`run.assign.vm`, set by an admin per machine) may get.
    /// Default: this host's CPU count.
    #[serde(default)]
    pub max_vcpus: Option<u32>,
    /// Default: this host's total memory in MiB.
    #[serde(default)]
    pub max_mem_mib: Option<u32>,
    /// This daemon's first VM slot: its VMs use slots `slot_base..slot_base +
    /// max_parallel_runs` (tap `rvm<slot>`, addresses from the slot). Daemons of different
    /// users on one host get ranges that do not overlap (`/etc/runner-vm.users`).
    #[serde(default)]
    pub slot_base: u32,
    /// Size of each run's writable layer (a sparse file).
    #[serde(default = "default_vm_scratch_gb")]
    pub scratch_gb: u32,
    #[serde(default = "default_vm_firecracker")]
    pub firecracker: String,
    /// Root helper for the tap device and NFS exports, run with `sudo -n`.
    #[serde(default = "default_vm_helper")]
    pub helper: String,
    /// More folders shared with every VM at their host paths. Empty by default: the CLIs'
    /// sign-in folders are never shared (a run gets its member's own CLI folders and a copy of
    /// the sign-in it cannot use to change this host's, see `vm_login`). Folders of the daemon
    /// user's home that start with a dot are skipped.
    #[serde(default)]
    pub shares: Vec<PathBuf>,
    /// Where earlier daemon versions kept each bot's home folder. Still read (an old config
    /// parses): a resumed session is looked for in `<homes>/<bot>`, and the cleanup removes
    /// what is left there.
    #[serde(default = "default_vm_homes")]
    pub homes: PathBuf,
    /// The size of those homes. Ignored; `daemon doctor` says so when it is set.
    #[serde(default)]
    pub home_gb: Option<u32>,
}

/// VM slots: 10.200.0.0/16 holds 16384 /30 networks.
pub const VM_SLOTS: u32 = 16384;
/// Firecracker's most vCPUs per VM.
pub const VM_MAX_VCPUS: u32 = 32;

impl VmConfig {
    /// `max_vcpus`, or this host's CPU count.
    pub fn max_vcpus(&self) -> u32 {
        self.max_vcpus.unwrap_or_else(|| {
            std::thread::available_parallelism()
                .map(|n| n.get() as u32)
                .unwrap_or(1)
        })
    }

    /// `max_mem_mib`, or this host's total memory in MiB (when it cannot be read: no cap
    /// below the protocol's own).
    pub fn max_mem_mib(&self) -> u32 {
        self.max_mem_mib.unwrap_or_else(|| {
            std::fs::read_to_string("/proc/meminfo")
                .ok()
                .and_then(|t| crate::metrics::parse_meminfo(&t))
                .map(|(total, _)| u32::try_from(total >> 20).unwrap_or(u32::MAX))
                .unwrap_or(u32::MAX)
        })
    }

    /// The size of a run's VM (vCPUs, MiB): the run's own size capped at `max_*`, or
    /// `vcpus` / `mem_mib` when it names none.
    pub fn size(&self, asked: Option<crate::proto::VmSize>) -> (u32, u32) {
        vm_size(
            asked,
            (self.vcpus, self.mem_mib),
            (self.max_vcpus(), self.max_mem_mib()),
        )
    }

    /// This daemon's VM slots.
    pub fn slots(&self, max_parallel_runs: u32) -> std::ops::Range<u32> {
        self.slot_base..self.slot_base.saturating_add(max_parallel_runs.max(1))
    }
}

/// [`VmConfig::size`] with the host's limits given.
pub fn vm_size(
    asked: Option<crate::proto::VmSize>,
    default: (u32, u32),
    max: (u32, u32),
) -> (u32, u32) {
    match asked {
        None => default,
        Some(a) => (
            a.vcpus.clamp(1, max.0.max(1)),
            a.mem_mib.min(max.1).max(128),
        ),
    }
}

fn default_vm_dir() -> PathBuf {
    PathBuf::from("/srv/runner/vms")
}
fn default_vm_vcpus() -> u32 {
    2
}
fn default_vm_mem_mib() -> u32 {
    4096
}
fn default_vm_scratch_gb() -> u32 {
    32
}
fn default_vm_firecracker() -> String {
    "firecracker".into()
}
fn default_vm_helper() -> String {
    "/usr/local/sbin/runner-vm".into()
}
fn default_vm_homes() -> PathBuf {
    PathBuf::from("/srv/runner/homes")
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
    /// Model ids this runtime reports in `hello` (feature `effort`), replacing the list the daemon
    /// finds: for ACP agents and the PTY, which report none otherwise, or to narrow a list.
    /// Empty reports no list.
    #[serde(default)]
    pub models: Option<Vec<String>>,
    /// Efforts this runtime reports and runs with (`low`, `high`, ...), replacing the ones the
    /// daemon finds. Empty reports none.
    #[serde(default)]
    pub efforts: Option<Vec<String>>,
    /// Ignored since 1.2.1: an AI member always has every permission (codex approval `never`,
    /// sandbox `danger-full-access`; claude `bypassPermissions`). Kept so old configs parse.
    #[serde(default)]
    pub approval_policy: Option<String>,
    /// Ignored since 1.2.1, see `approval_policy`.
    #[serde(default)]
    pub sandbox: Option<String>,
    /// Ignored since 1.2.1, see `approval_policy`.
    #[serde(default)]
    pub permission_mode: Option<String>,
    /// Ignored since 1.2.1, see `approval_policy`.
    #[serde(default)]
    pub allowed_tools: Option<String>,
    /// ACP only: the agent sends `session/request_permission` before the tool calls it runs (its
    /// own permissions are not set to allow everything). Only then are its runs reported as
    /// gated (`CliInfo.gates`); otherwise the server keeps the team's rules in the prompt.
    #[serde(default)]
    pub asks_permission: bool,
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
    crate::sys::host_name().unwrap_or_else(|| "machine".into())
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
fn default_self_update() -> bool {
    true
}

pub fn home() -> PathBuf {
    let home = std::env::var_os("HOME").filter(|h| !h.is_empty());
    // Windows names the profile folder USERPROFILE; HOME is set only by some shells.
    #[cfg(windows)]
    let home = home.or_else(|| std::env::var_os("USERPROFILE").filter(|h| !h.is_empty()));
    home.map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/"))
}

pub fn expand(p: &Path) -> PathBuf {
    match p.to_str() {
        Some("~") => home(),
        Some(s) if s.starts_with("~/") || (cfg!(windows) && s.starts_with("~\\")) => {
            home().join(&s[2..])
        }
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
        if let Some(vm) = c.vm.as_mut() {
            vm.kernel = expand(&vm.kernel);
            vm.rootfs = expand(&vm.rootfs);
            vm.dir = expand(&vm.dir);
            vm.shares = vm.shares.iter().map(|p| expand(p)).collect();
            vm.homes = expand(&vm.homes);
        }
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
                            models: None,
                            efforts: None,
                            approval_policy: None,
                            sandbox: None,
                            permission_mode: None,
                            allowed_tools: None,
                            asks_permission: false,
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
                if crate::sys::open_to_others(&m) {
                    out.push(format!(
                        "token file {} must have mode 600",
                        self.token_file.display()
                    ));
                }
                // Windows: the same question, asked of the owner and the access list.
                if let Some(why) = crate::sys::acl_problem(&self.token_file) {
                    out.push(format!(
                        "token file {} must be private to this user: {why}",
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
        for k in self
            .gated_env
            .iter()
            .filter(|k| self.env_passthrough.contains(k))
        {
            out.push(format!(
                "{k} is in both env_passthrough and gated_env: a gated variable must not be passed through"
            ));
        }
        if let Some(p) = self.takeover_url.as_deref().and_then(takeover_url_problem) {
            out.push(p.into());
        }
        if self.runtimes.is_empty() {
            out.push("no runtimes: install codex or claude, or add [runtimes.*]".into());
        }
        if let Some(vm) = &self.vm {
            if !cfg!(target_os = "linux") {
                out.push("[vm] runs only on Linux".into());
            }
            for (what, p) in [("kernel", &vm.kernel), ("rootfs", &vm.rootfs)] {
                if !p.exists() {
                    out.push(format!("[vm] {what} {} does not exist", p.display()));
                }
            }
            if !Path::new(&vm.helper).is_file() {
                out.push(format!("[vm] helper {} does not exist", vm.helper));
            }
            if crate::health::which(&vm.firecracker).is_none()
                && !Path::new(&vm.firecracker).is_file()
            {
                out.push(format!("[vm] {} not found", vm.firecracker));
            }
            if self.max_parallel_runs > 1000 {
                out.push("[vm] max_parallel_runs must be at most 1000".into());
            }
            out.extend(vm_problems(vm, self.max_parallel_runs));
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

/// The `[vm]` settings that cannot work, whatever the host has.
pub fn vm_problems(vm: &VmConfig, max_parallel_runs: u32) -> Vec<String> {
    let mut out = vec![];
    if vm
        .max_vcpus
        .is_some_and(|n| !(1..=VM_MAX_VCPUS).contains(&n))
    {
        out.push(format!("[vm] max_vcpus must be 1 to {VM_MAX_VCPUS}"));
    }
    if vm.max_mem_mib.is_some_and(|n| n < 128) {
        out.push("[vm] max_mem_mib must be at least 128".into());
    }
    if !(1..=VM_MAX_VCPUS).contains(&vm.vcpus) {
        out.push(format!("[vm] vcpus must be 1 to {VM_MAX_VCPUS}"));
    }
    if vm.mem_mib < 128 {
        out.push("[vm] mem_mib must be at least 128".into());
    }
    if u64::from(vm.slot_base) + u64::from(max_parallel_runs.max(1)) > u64::from(VM_SLOTS) {
        out.push(format!(
            "[vm] slot_base + max_parallel_runs must be at most {VM_SLOTS}"
        ));
    }
    out
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
gated_env = []                      # e.g. ["CLOUDFLARE_API_TOKEN"]; only after approval (chat-run secret)
# takeover_url = "vnc://mac-mini.tailnet"   # 接管: Screen Sharing address (turn Sharing on by hand; no passwords)
# self_update = false               # a Linux daemon installed on its own updates itself from releases; false turns it off

# AI members always run with every permission (no approvals); there is nothing to set.
[runtimes.codex]
kind = "codex"

[runtimes.claude]
kind = "claude"

# [runtimes.gemini]
# kind = "acp"
# command = "gemini"
# args = ["--acp"]
# models = ["gemini-2.5-pro"]       # what settings offer; replaces the list the daemon finds
# efforts = ["low", "high"]
# asks_permission = true            # the agent asks before running tools: the daemon stops
#                                   # gated commands itself (leave out for one that allows all)

# [runtimes.shell]
# kind = "pty"
# command = "/bin/sh"
# args = ["-c", "{prompt}"]

# Linux runner hosts: each run in its own Firecracker microVM (scripts/runner/host-setup.sh).
# [vm]
# kernel = "/srv/runner/kernels/vmlinux"
# rootfs = "/srv/runner/images/base.ext4"
# vcpus = 2
# mem_mib = 4096                     # a cap; the host gives only what the VM touches
#                                   # (vcpus and mem_mib: the size when the server sends none)
# max_vcpus = 8                     # the most a size an admin sets for this machine may get
# max_mem_mib = 16384               #   (run.assign.vm); default: this host's CPUs and memory
# slot_base = 0                     # first VM slot (tap rvm<slot>); slots slot_base..+max_parallel_runs,
#                                   # within this user's range in /etc/runner-vm.users
"#;

#[cfg(test)]
mod tests {
    use super::takeover_url_problem as p;

    /// A config written for the per-member homes still parses; the settings do nothing.
    #[test]
    fn an_old_vm_config_with_homes_parses() {
        let c = super::Config::parse(
            "server_url = \"http://127.0.0.1:1\"\ntoken_file = \"/nonexistent\"\n[runtimes.claude]\nkind = \"claude\"\n[vm]\nkernel = \"/k\"\nrootfs = \"/r\"\nhomes = \"/srv/runner/homes\"\nhome_gb = 20\n",
        )
        .unwrap();
        let vm = c.vm.unwrap();
        assert_eq!(vm.home_gb, Some(20));
        assert_eq!(vm.homes, std::path::PathBuf::from("/srv/runner/homes"));
        assert!(!super::EXAMPLE.contains("home_gb"));
    }

    #[test]
    fn a_runs_size_is_capped_at_the_hosts_max() {
        use crate::proto::VmSize;
        let size = |vcpus, mem_mib| Some(VmSize { vcpus, mem_mib });
        // None: the configured size, even above the max.
        assert_eq!(super::vm_size(None, (2, 4096), (1, 2048)), (2, 4096));
        assert_eq!(
            super::vm_size(size(4, 8192), (2, 4096), (8, 16384)),
            (4, 8192)
        );
        assert_eq!(
            super::vm_size(size(16, 65536), (2, 4096), (8, 16384)),
            (8, 16384)
        );
        assert_eq!(
            super::vm_size(size(1, 1024), (2, 4096), (8, 16384)),
            (1, 1024)
        );
        // A max of 0 still gives the VM one vCPU.
        assert_eq!(super::vm_size(size(4, 4096), (2, 4096), (0, 2048)).0, 1);
        let base = "server_url = \"http://127.0.0.1:1\"\ntoken_file = \"/x\"\n[vm]\nkernel = \"/k\"\nrootfs = \"/r\"\n";
        let vm = super::Config::parse(&format!("{base}max_vcpus = 3\nmax_mem_mib = 3000\n"))
            .unwrap()
            .vm
            .unwrap();
        assert_eq!(vm.size(size(16, 65536)), (3, 3000));
        assert_eq!(vm.size(None), (2, 4096));
        let vm = super::Config::parse(base).unwrap().vm.unwrap();
        assert!(vm.max_vcpus() >= 1 && vm.max_mem_mib() >= 1);
    }

    #[test]
    fn vm_limits_and_slots_are_checked() {
        let parse = |extra: &str| {
            super::Config::parse(&format!(
                "server_url = \"http://127.0.0.1:1\"\ntoken_file = \"/x\"\n[vm]\nkernel = \"/k\"\nrootfs = \"/r\"\n{extra}"
            ))
            .unwrap()
            .vm
            .unwrap()
        };
        assert!(super::vm_problems(&parse(""), 12).is_empty());
        let vm = parse("slot_base = 64\n");
        assert_eq!(vm.slots(1), 64..65);
        assert_eq!(parse("").slots(12), 0..12);
        assert!(super::vm_problems(&vm, 1).is_empty());
        assert!(super::vm_problems(&parse("slot_base = 16383\n"), 1).is_empty());
        assert_eq!(
            super::vm_problems(&parse("slot_base = 16383\n"), 2),
            ["[vm] slot_base + max_parallel_runs must be at most 16384"]
        );
        assert_eq!(super::vm_problems(&parse("max_vcpus = 0\n"), 1).len(), 1);
        assert_eq!(super::vm_problems(&parse("max_vcpus = 33\n"), 1).len(), 1);
        assert_eq!(super::vm_problems(&parse("max_mem_mib = 64\n"), 1).len(), 1);
    }

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

    #[test]
    fn self_update_is_on_unless_turned_off() {
        let base = "server_url = \"https://chat.example.org\"\ntoken_file = \"/tmp/t\"\n";
        assert!(super::Config::parse(base).unwrap().self_update);
        let off = format!("{base}self_update = false\n");
        assert!(!super::Config::parse(&off).unwrap().self_update);
        let example = super::EXAMPLE.replace("# self_update = false", "self_update = false");
        assert!(!super::Config::parse(&example).unwrap().self_update);
    }
}
