//! Host and VM metrics for the heartbeat (docs/machine-protocol.md, "Metrics").
//!
//! The executor records each run's VM here as it boots, runs, pauses and goes; every heartbeat
//! the daemon takes one [`Sampler::sample`]: whole-host CPU, memory, network and uptime, and per
//! VM its CPU (from the Firecracker process), resident memory and scratch disk use. CPU and
//! network are rates since the previous sample. Linux reads `/proc`; macOS and Windows give CPU,
//! memory and uptime; other numbers are left out. Nothing here reads a credential's value: CLI logins are
//! only checked for presence.

use crate::config::{Config, RuntimeConfig};
use crate::proto::{BootStats, CliLogin, MachineMetrics, VmRunMetrics, login_state, vm_phase};
use std::collections::{BTreeMap, HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::Instant;

/// Boot times kept for [`BootStats`].
const BOOT_SAMPLES: usize = 50;

#[derive(Debug, Clone)]
struct VmEntry {
    phase: &'static str,
    started_at: u64,
    pid: Option<i32>,
    scratch: Option<PathBuf>,
    boot_ms: Option<u64>,
}

static VMS: Mutex<BTreeMap<String, VmEntry>> = Mutex::new(BTreeMap::new());
static BOOTS: Mutex<VecDeque<u64>> = Mutex::new(VecDeque::new());

fn vms() -> std::sync::MutexGuard<'static, BTreeMap<String, VmEntry>> {
    VMS.lock().unwrap_or_else(|e| e.into_inner())
}

/// A run's VM starts booting; `scratch` is its writable disk.
pub fn vm_booting(run_id: &str, scratch: PathBuf) {
    vms().insert(
        run_id.to_string(),
        VmEntry {
            phase: vm_phase::BOOTING,
            started_at: crate::proto::now_ms(),
            pid: None,
            scratch: Some(scratch),
            boot_ms: None,
        },
    );
}

/// The VM's Firecracker process started.
pub fn vm_process(run_id: &str, pid: i32) {
    if let Some(e) = vms().get_mut(run_id) {
        e.pid = (pid > 0).then_some(pid);
    }
}

/// The guest agent answered: the VM runs the CLI.
pub fn vm_booted(run_id: &str, boot_ms: u64) {
    if let Some(e) = vms().get_mut(run_id) {
        e.phase = vm_phase::RUNNING;
        e.boot_ms = Some(boot_ms);
    }
    let mut b = BOOTS.lock().unwrap_or_else(|e| e.into_inner());
    b.push_back(boot_ms);
    while b.len() > BOOT_SAMPLES {
        b.pop_front();
    }
}

/// 接管 paused or resumed the VM.
pub fn vm_paused(run_id: &str, paused: bool) {
    if let Some(e) = vms().get_mut(run_id) {
        e.phase = if paused {
            vm_phase::PAUSED
        } else {
            vm_phase::RUNNING
        };
    }
}

/// The VM is gone (torn down, or it never booted).
pub fn vm_gone(run_id: &str) {
    vms().remove(run_id);
}

/// VMs that booted (running or paused): they hold the memory they use already.
pub fn vms_booted() -> u64 {
    vms()
        .values()
        .filter(|e| e.phase != vm_phase::BOOTING)
        .count() as u64
}

/// `(total, available)` memory of this host, when the platform says.
pub fn host_memory() -> Option<(u64, u64)> {
    host::memory()
}

fn boot_stats() -> Option<BootStats> {
    let b = BOOTS.lock().unwrap_or_else(|e| e.into_inner());
    if b.is_empty() {
        return None;
    }
    let sum: u64 = b.iter().sum();
    Some(BootStats {
        avg_ms: sum / b.len() as u64,
        max_ms: b.iter().copied().max().unwrap_or(0),
        samples: b.len() as u32,
    })
}

/// A run without a VM (the local executor): the daemon's own view of it.
pub struct LocalRun {
    pub run_id: String,
    pub started_at: u64,
}

/// Keeps the previous counters so CPU and network become rates.
#[derive(Default)]
pub struct Sampler {
    cpu: Option<CpuTimes>,
    net: Option<(NetBytes, Instant)>,
    pids: HashMap<i32, (u64, Instant)>,
}

impl Sampler {
    pub fn new() -> Sampler {
        // A first reading, so the first heartbeat already has rates.
        Sampler {
            cpu: host::cpu_times(),
            net: host::net_bytes().map(|n| (n, Instant::now())),
            ..Default::default()
        }
    }

    /// One sample. `vm_slots` is `Some` when runs go to VMs; `local` lists the runs of the
    /// local executor (ignored when VMs run).
    pub fn sample(
        &mut self,
        cfg: &Config,
        vm_slots: Option<u32>,
        local: &[LocalRun],
    ) -> MachineMetrics {
        let now = Instant::now();
        let cpu = host::cpu_times();
        let cpu_pct = match (&self.cpu, &cpu) {
            (Some(a), Some(b)) => busy_pct(a, b),
            _ => None,
        };
        self.cpu = cpu;
        let net = host::net_bytes();
        let (net_rx_bps, net_tx_bps) = match (&self.net, &net) {
            (Some((a, at)), Some(b)) => rate(a, b, now.duration_since(*at).as_secs_f64()),
            _ => (None, None),
        };
        self.net = net.map(|n| (n, now));
        let mem = host::memory();
        let runs = if vm_slots.is_some() {
            self.vm_runs(now)
        } else {
            local
                .iter()
                .map(|r| VmRunMetrics {
                    run_id: r.run_id.clone(),
                    phase: vm_phase::RUNNING.to_string(),
                    started_at: r.started_at,
                    boot_ms: None,
                    cpu_cores: None,
                    mem_bytes: None,
                    scratch_bytes: None,
                })
                .take(crate::proto::MAX_METRIC_RUNS)
                .collect()
        };
        MachineMetrics {
            at: crate::proto::now_ms(),
            uptime_secs: host::uptime_secs(),
            cpu_pct,
            cpu_threads: std::thread::available_parallelism()
                .ok()
                .map(|n| n.get() as u32),
            mem_total_bytes: mem.map(|m| m.0),
            mem_available_bytes: mem.map(|m| m.1),
            net_rx_bps,
            net_tx_bps,
            vm_slots,
            boot: vm_slots.and_then(|_| boot_stats()),
            runs,
            logins: Some(logins(cfg)),
            secrets: None,
            usage: None,
            homes: None,
            problems: None,
            github: None,
        }
    }

    fn vm_runs(&mut self, now: Instant) -> Vec<VmRunMetrics> {
        let entries: Vec<(String, VmEntry)> =
            vms().iter().map(|(k, v)| (k.clone(), v.clone())).collect();
        let tick = host::clock_ticks();
        let mut seen = HashMap::new();
        let mut out = Vec::new();
        for (run_id, e) in entries.into_iter().take(crate::proto::MAX_METRIC_RUNS) {
            let mut cpu_cores = None;
            let mut mem_bytes = None;
            if let Some(pid) = e.pid
                && let Some(p) = host::process(pid)
            {
                if let Some((prev, at)) = self.pids.get(&pid) {
                    let secs = now.duration_since(*at).as_secs_f64();
                    if secs > 0.0 && tick > 0.0 && p.cpu_ticks >= *prev {
                        cpu_cores = Some(round2((p.cpu_ticks - prev) as f64 / tick / secs));
                    }
                }
                seen.insert(pid, (p.cpu_ticks, now));
                mem_bytes = p.rss_bytes;
            }
            out.push(VmRunMetrics {
                run_id,
                phase: e.phase.to_string(),
                started_at: e.started_at,
                boot_ms: e.boot_ms,
                cpu_cores,
                mem_bytes,
                scratch_bytes: e.scratch.as_deref().and_then(allocated_bytes),
            });
        }
        self.pids = seen;
        out
    }
}

fn round2(x: f64) -> f64 {
    (x * 100.0).round() / 100.0
}

/// Bytes a (sparse) file really uses on disk.
fn allocated_bytes(p: &Path) -> Option<u64> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        std::fs::metadata(p).ok().map(|m| m.blocks() * 512)
    }
    // Only VM disks are measured, and VMs run on Linux: the file's length will do.
    #[cfg(not(unix))]
    std::fs::metadata(p).ok().map(|m| m.len())
}

/// Cumulative CPU time of the host in clock ticks.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CpuTimes {
    pub total: u64,
    pub idle: u64,
}

fn busy_pct(a: &CpuTimes, b: &CpuTimes) -> Option<f64> {
    let total = b.total.checked_sub(a.total)?;
    let idle = b.idle.checked_sub(a.idle)?;
    if total == 0 {
        return None;
    }
    Some(round2(
        ((total.saturating_sub(idle)) as f64 / total as f64 * 100.0).clamp(0.0, 100.0),
    ))
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct NetBytes {
    pub rx: u64,
    pub tx: u64,
}

fn rate(a: &NetBytes, b: &NetBytes, secs: f64) -> (Option<f64>, Option<f64>) {
    if secs <= 0.0 || b.rx < a.rx || b.tx < a.tx {
        return (None, None);
    }
    (
        Some(((b.rx - a.rx) as f64 / secs).round()),
        Some(((b.tx - a.tx) as f64 / secs).round()),
    )
}

/// The first line of `/proc/stat`: user nice system idle iowait irq softirq steal.
pub fn parse_proc_stat(text: &str) -> Option<CpuTimes> {
    let line = text.lines().find(|l| l.starts_with("cpu "))?;
    let v: Vec<u64> = line
        .split_whitespace()
        .skip(1)
        .take(8)
        .filter_map(|x| x.parse().ok())
        .collect();
    if v.len() < 4 {
        return None;
    }
    let idle = v[3] + v.get(4).copied().unwrap_or(0);
    Some(CpuTimes {
        total: v.iter().sum(),
        idle,
    })
}

/// `MemTotal` and `MemAvailable` of `/proc/meminfo`, in bytes.
pub fn parse_meminfo(text: &str) -> Option<(u64, u64)> {
    let kb = |key: &str| -> Option<u64> {
        let l = text.lines().find(|l| l.starts_with(key))?;
        l[key.len()..]
            .split_whitespace()
            .next()?
            .parse::<u64>()
            .ok()
    };
    Some((kb("MemTotal:")? * 1024, kb("MemAvailable:")? * 1024))
}

/// Interfaces whose traffic is not the host's own: loopback and the VMs' and containers' links.
fn virtual_iface(name: &str) -> bool {
    name == "lo"
        || ["tap", "fc", "veth", "docker", "virbr", "br-", "tc"]
            .iter()
            .any(|p| name.starts_with(p))
}

/// Received and sent bytes of `/proc/net/dev`, summed over the host's own interfaces.
pub fn parse_net_dev(text: &str) -> Option<NetBytes> {
    let mut out = NetBytes { rx: 0, tx: 0 };
    let mut any = false;
    for l in text.lines().skip(2) {
        let Some((name, rest)) = l.split_once(':') else {
            continue;
        };
        if virtual_iface(name.trim()) {
            continue;
        }
        let v: Vec<u64> = rest
            .split_whitespace()
            .filter_map(|x| x.parse().ok())
            .collect();
        if v.len() >= 9 {
            out.rx += v[0];
            out.tx += v[8];
            any = true;
        }
    }
    any.then_some(out)
}

/// One process: user + system CPU ticks and resident memory.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ProcStat {
    pub cpu_ticks: u64,
    pub rss_bytes: Option<u64>,
}

/// utime + stime of `/proc/<pid>/stat` (fields 14 and 15; the name in parentheses may hold
/// spaces, so fields are counted after its last `)`).
pub fn parse_pid_stat(text: &str) -> Option<u64> {
    let rest = &text[text.rfind(')')? + 1..];
    let v: Vec<&str> = rest.split_whitespace().collect();
    // After ")": state is field 3, so utime (14) is index 11 and stime (15) index 12.
    let utime: u64 = v.get(11)?.parse().ok()?;
    let stime: u64 = v.get(12)?.parse().ok()?;
    Some(utime + stime)
}

/// `VmRSS` of `/proc/<pid>/status`, in bytes.
pub fn parse_vm_rss(text: &str) -> Option<u64> {
    let l = text.lines().find(|l| l.starts_with("VmRSS:"))?;
    Some(
        l["VmRSS:".len()..]
            .split_whitespace()
            .next()?
            .parse::<u64>()
            .ok()?
            * 1024,
    )
}

#[cfg(target_os = "linux")]
mod host {
    use super::*;

    pub fn cpu_times() -> Option<CpuTimes> {
        parse_proc_stat(&std::fs::read_to_string("/proc/stat").ok()?)
    }
    pub fn net_bytes() -> Option<NetBytes> {
        parse_net_dev(&std::fs::read_to_string("/proc/net/dev").ok()?)
    }
    pub fn memory() -> Option<(u64, u64)> {
        parse_meminfo(&std::fs::read_to_string("/proc/meminfo").ok()?)
    }
    pub fn uptime_secs() -> Option<u64> {
        let s = std::fs::read_to_string("/proc/uptime").ok()?;
        s.split_whitespace()
            .next()?
            .parse::<f64>()
            .ok()
            .map(|x| x as u64)
    }
    pub fn clock_ticks() -> f64 {
        // SAFETY: sysconf has no preconditions.
        let t = unsafe { libc::sysconf(libc::_SC_CLK_TCK) };
        if t > 0 { t as f64 } else { 100.0 }
    }
    pub fn process(pid: i32) -> Option<ProcStat> {
        let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
        let status = std::fs::read_to_string(format!("/proc/{pid}/status")).ok();
        Some(ProcStat {
            cpu_ticks: parse_pid_stat(&stat)?,
            rss_bytes: status.as_deref().and_then(parse_vm_rss),
        })
    }
}

#[cfg(target_os = "macos")]
#[allow(deprecated)] // libc points to the mach2 crate for these; the calls are the same
mod host {
    use super::*;

    pub fn cpu_times() -> Option<CpuTimes> {
        // SAFETY: host_statistics fills at most `count` integers of the zeroed struct.
        unsafe {
            let mut info: libc::host_cpu_load_info = std::mem::zeroed();
            let mut count = (std::mem::size_of::<libc::host_cpu_load_info>()
                / std::mem::size_of::<libc::integer_t>())
                as libc::mach_msg_type_number_t;
            let r = libc::host_statistics(
                libc::mach_host_self(),
                libc::HOST_CPU_LOAD_INFO,
                &mut info as *mut _ as libc::host_info_t,
                &mut count,
            );
            if r != 0 {
                return None;
            }
            // user, system, idle, nice
            let t = info.cpu_ticks.map(|x| x as u64);
            Some(CpuTimes {
                total: t.iter().sum(),
                idle: t[2],
            })
        }
    }
    pub fn net_bytes() -> Option<NetBytes> {
        None
    }
    fn sysctl_u64(name: &str) -> Option<u64> {
        let c = std::ffi::CString::new(name).ok()?;
        let mut v: u64 = 0;
        let mut len = std::mem::size_of::<u64>();
        // SAFETY: v has room for len bytes.
        let r = unsafe {
            libc::sysctlbyname(
                c.as_ptr(),
                &mut v as *mut _ as *mut libc::c_void,
                &mut len,
                std::ptr::null_mut(),
                0,
            )
        };
        (r == 0).then_some(v)
    }
    pub fn memory() -> Option<(u64, u64)> {
        let total = sysctl_u64("hw.memsize")?;
        // SAFETY: host_statistics64 fills at most `count` integers of the zeroed struct.
        let avail = unsafe {
            let mut vm: libc::vm_statistics64 = std::mem::zeroed();
            let mut count = (std::mem::size_of::<libc::vm_statistics64>()
                / std::mem::size_of::<libc::integer_t>())
                as libc::mach_msg_type_number_t;
            let r = libc::host_statistics64(
                libc::mach_host_self(),
                libc::HOST_VM_INFO64,
                &mut vm as *mut _ as libc::host_info64_t,
                &mut count,
            );
            if r != 0 {
                return Some((total, 0));
            }
            let page = libc::sysconf(libc::_SC_PAGESIZE).max(4096) as u64;
            (vm.free_count as u64 + vm.inactive_count as u64 + vm.speculative_count as u64) * page
        };
        Some((total, avail.min(total)))
    }
    pub fn uptime_secs() -> Option<u64> {
        let c = std::ffi::CString::new("kern.boottime").ok()?;
        let mut tv: libc::timeval = unsafe { std::mem::zeroed() };
        let mut len = std::mem::size_of::<libc::timeval>();
        // SAFETY: tv has room for len bytes.
        let r = unsafe {
            libc::sysctlbyname(
                c.as_ptr(),
                &mut tv as *mut _ as *mut libc::c_void,
                &mut len,
                std::ptr::null_mut(),
                0,
            )
        };
        if r != 0 {
            return None;
        }
        let now = crate::proto::now_ms() / 1000;
        Some(now.saturating_sub(tv.tv_sec as u64))
    }
    pub fn clock_ticks() -> f64 {
        100.0
    }
    pub fn process(_pid: i32) -> Option<ProcStat> {
        None
    }
}

/// Windows: the same three numbers macOS gives (CPU, memory, uptime), from the system's own
/// counters. No network counters, as on macOS.
#[cfg(windows)]
mod host {
    use super::*;
    use windows_sys::Win32::System::SystemInformation::{
        GetTickCount64, GlobalMemoryStatusEx, MEMORYSTATUSEX,
    };

    pub fn cpu_times() -> Option<CpuTimes> {
        crate::sys::cpu_times().map(|(total, idle)| CpuTimes { total, idle })
    }
    pub fn net_bytes() -> Option<NetBytes> {
        None
    }
    pub fn memory() -> Option<(u64, u64)> {
        // SAFETY: an all-zero MEMORYSTATUSEX with its length set is what the call fills in.
        unsafe {
            let mut m: MEMORYSTATUSEX = std::mem::zeroed();
            m.dwLength = std::mem::size_of::<MEMORYSTATUSEX>() as u32;
            (GlobalMemoryStatusEx(&mut m) != 0)
                .then(|| (m.ullTotalPhys, m.ullAvailPhys.min(m.ullTotalPhys)))
        }
    }
    pub fn uptime_secs() -> Option<u64> {
        // SAFETY: plain call without arguments.
        Some(unsafe { GetTickCount64() } / 1000)
    }
    pub fn clock_ticks() -> f64 {
        100.0
    }
    pub fn process(_pid: i32) -> Option<ProcStat> {
        None
    }
}

#[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
mod host {
    use super::*;
    pub fn cpu_times() -> Option<CpuTimes> {
        None
    }
    pub fn net_bytes() -> Option<NetBytes> {
        None
    }
    pub fn memory() -> Option<(u64, u64)> {
        None
    }
    pub fn uptime_secs() -> Option<u64> {
        None
    }
    pub fn clock_ticks() -> f64 {
        100.0
    }
    pub fn process(_pid: i32) -> Option<ProcStat> {
        None
    }
}

// ---------------------------------------------------------------------------
// CLI logins
// ---------------------------------------------------------------------------

/// The value of `name` the CLI would see: the runtime's fixed env, else the daemon's own.
fn env_of(rt: &RuntimeConfig, name: &str) -> Option<String> {
    rt.env
        .get(name)
        .cloned()
        .or_else(|| std::env::var(name).ok())
        .filter(|v| !v.trim().is_empty())
}

/// An API key the CLI gets: in the runtime's env, or passed through from the daemon's env.
fn has_key(cfg: &Config, rt: &RuntimeConfig, name: &str) -> bool {
    rt.env.get(name).is_some_and(|v| !v.trim().is_empty())
        || (cfg.env_passthrough.iter().any(|n| n == name)
            && std::env::var(name).is_ok_and(|v| !v.trim().is_empty()))
}

fn read_json(p: &Path) -> Option<serde_json::Value> {
    serde_json::from_slice(&std::fs::read(p).ok()?).ok()
}

/// A JSON value that holds something (not null, not an empty string or object).
fn present(v: Option<&serde_json::Value>) -> bool {
    match v {
        None | Some(serde_json::Value::Null) => false,
        Some(serde_json::Value::String(s)) => !s.trim().is_empty(),
        Some(serde_json::Value::Object(o)) => !o.is_empty(),
        Some(_) => true,
    }
}

/// Claude Code: the subscription credential in `$CLAUDE_CONFIG_DIR/.credentials.json`
/// (Linux), the signed-in account in `.claude.json` (macOS keeps the credential in the
/// Keychain), else an `ANTHROPIC_API_KEY` the CLI gets.
pub fn claude_login(cfg: &Config, rt: &RuntimeConfig) -> CliLogin {
    let dir = env_of(rt, "CLAUDE_CONFIG_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| crate::config::home().join(".claude"));
    let creds = read_json(&dir.join(".credentials.json"));
    let oauth = creds.as_ref().and_then(|c| c.get("claudeAiOauth"));
    let token =
        oauth.is_some_and(|o| present(o.get("refreshToken")) || present(o.get("accessToken")));
    // `.claude.json` lives in the config dir when CLAUDE_CONFIG_DIR is set, else in the home.
    let account = [
        dir.join(".claude.json"),
        crate::config::home().join(".claude.json"),
    ]
    .iter()
    .filter_map(|p| read_json(p))
    .any(|j| present(j.get("oauthAccount")));
    let (state, method) = if token || (account && cfg!(target_os = "macos")) {
        (login_state::LOGGED_IN, Some("subscription"))
    } else if has_key(cfg, rt, "ANTHROPIC_API_KEY") {
        (login_state::LOGGED_IN, Some("api_key"))
    } else {
        // On Linux an account without a credential file is a login that expired or was removed.
        (login_state::LOGGED_OUT, None)
    };
    CliLogin {
        runtime: String::new(),
        state: state.to_string(),
        method: method.map(str::to_string),
        email: None,
        plan: None,
        confirmed: None,
    }
}

/// Codex: `$CODEX_HOME/auth.json` with ChatGPT tokens or an API key, else `OPENAI_API_KEY`.
pub fn codex_login(cfg: &Config, rt: &RuntimeConfig) -> CliLogin {
    let home = env_of(rt, "CODEX_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| crate::config::home().join(".codex"));
    let auth = read_json(&home.join("auth.json"));
    let tokens = auth.as_ref().is_some_and(|a| present(a.get("tokens")));
    let key = auth
        .as_ref()
        .is_some_and(|a| present(a.get("OPENAI_API_KEY")))
        || has_key(cfg, rt, "OPENAI_API_KEY");
    let (state, method) = if tokens {
        (login_state::LOGGED_IN, Some("subscription"))
    } else if key {
        (login_state::LOGGED_IN, Some("api_key"))
    } else {
        (login_state::LOGGED_OUT, None)
    };
    CliLogin {
        runtime: String::new(),
        state: state.to_string(),
        method: method.map(str::to_string),
        email: None,
        plan: None,
        confirmed: None,
    }
}

/// The API key a CLI of this kind reads instead of a sign-in.
pub fn api_key_name(kind: &str) -> Option<&'static str> {
    match kind {
        "claude" => Some("ANTHROPIC_API_KEY"),
        "codex" => Some("OPENAI_API_KEY"),
        _ => None,
    }
}

/// The login a heartbeat reports (模 3.2): what the credential files say (`l`), then the CLI's
/// own status check (`check`) over it, an API key saved from the app (`key_saved`) and, last, a
/// run that failed because the CLI is not signed in (`failed_sign_in`), which stands until the
/// status check says it is signed in again.
pub fn apply_sign_in(
    l: &mut CliLogin,
    check: Option<&crate::accounts::SignIn>,
    key_saved: bool,
    failed_sign_in: bool,
) {
    use crate::accounts::SignIn;
    // Feature "sign-in-confirmed": a `logged_in` the CLI's check or an app key vouches for.
    let mut certain = false;
    match check {
        Some(SignIn::In(a)) => {
            certain = true;
            l.email = a.email.clone();
            l.plan = a.plan.clone();
            l.state = login_state::LOGGED_IN.into();
            if a.plan.as_deref() == Some("api_key") {
                l.method = Some("api_key".into());
            } else if l.method.is_none() {
                // The CLI itself says it is signed in (macOS keeps Claude's in the Keychain).
                l.method = Some("subscription".into());
            }
        }
        // The files may still be there after the sign-in expired or was revoked. A key the CLI
        // gets from the daemon's env is not in the check's env, so it still counts.
        Some(SignIn::Out) if l.method.as_deref() != Some("api_key") => {
            l.state = login_state::LOGGED_OUT.into();
            l.method = None;
            l.confirmed = Some(true);
        }
        _ => {}
    }
    if key_saved && l.state == login_state::LOGGED_OUT {
        l.state = login_state::LOGGED_IN.into();
        l.method = Some("api_key".into());
        certain = true;
    }
    if failed_sign_in {
        l.state = login_state::LOGGED_OUT.into();
        l.method = None;
        l.confirmed = Some(true);
    }
    if l.state != login_state::LOGGED_OUT {
        l.confirmed = certain.then_some(true);
    }
}

/// Feature "sign-in-confirmed": the runtime's last run succeeded, so its `logged_in` is certain.
pub fn confirm_worked(l: &mut CliLogin, worked: bool) {
    if worked && l.state == login_state::LOGGED_IN {
        l.confirmed = Some(true);
    }
}

/// The login of every Claude Code and Codex runtime (others have no login the daemon knows).
pub fn logins(cfg: &Config) -> Vec<CliLogin> {
    cfg.runtimes
        .iter()
        .filter_map(|(name, rt)| {
            let mut l = match rt.kind.as_str() {
                "claude" => claude_login(cfg, rt),
                "codex" => codex_login(cfg, rt),
                _ => return None,
            };
            l.runtime = name.clone();
            Some(l)
        })
        .take(64)
        .collect()
}

/// How often the daemon looks at the computer's GitHub sign-in (besides at start and after a
/// push was refused).
pub const GITHUB_EVERY: std::time::Duration = std::time::Duration::from_secs(600);

/// The computer's GitHub for the heartbeat (`metrics.github`, feature "github-host"): `gh auth
/// status`'s exit code for the daemon's own user and, when signed in, the login (`gh api
/// user`). `path` overrides the daemon's `PATH` (tests). The token is never read here.
pub async fn github(path: Option<&std::ffi::OsStr>) -> chat_protocol::machine::MachineGithub {
    let state = match crate::github::host_gh_output(
        path,
        &["auth", "status", "--hostname", crate::github::HOST],
    )
    .await
    {
        Ok(None) => "no_gh",
        Ok(Some(o)) if o.status.success() => "signed_in",
        Ok(Some(o)) if o.status.code().is_some() => "signed_out",
        _ => "unknown",
    };
    let login = if state == "signed_in" {
        crate::github::host_login(path).await
    } else {
        None
    };
    chat_protocol::machine::MachineGithub {
        state: state.into(),
        login,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn host_cpu_is_busy_share_between_two_readings() {
        let a = parse_proc_stat("cpu  100 0 100 700 100 0 0 0 0 0\ncpu0 1 2 3 4\n").unwrap();
        let b = parse_proc_stat("cpu  200 0 200 1300 200 0 0 0 0 0\n").unwrap();
        assert_eq!(a.total, 1000);
        assert_eq!(a.idle, 800);
        // 900 more ticks, 700 of them idle or iowait.
        assert_eq!(busy_pct(&a, &b), Some(22.22));
        assert_eq!(busy_pct(&b, &a), None);
    }

    /// macOS, Linux and Windows all report the host's CPU, memory and uptime.
    #[cfg(any(target_os = "linux", target_os = "macos", windows))]
    #[test]
    fn the_host_reports_cpu_memory_and_uptime() {
        let a = host::cpu_times().expect("cpu times");
        let (total, avail) = host::memory().expect("memory");
        assert!(total > 0 && avail <= total, "{total} {avail}");
        assert!(host::uptime_secs().is_some());
        // Burn some CPU until the counters move (their ticks are coarse), then a second
        // reading gives a share.
        let until = Instant::now() + std::time::Duration::from_secs(10);
        let mut x = 0u64;
        let b = loop {
            for _ in 0..1_000_000 {
                x = x.wrapping_mul(31).wrapping_add(1);
            }
            std::hint::black_box(x);
            let b = host::cpu_times().expect("cpu times");
            if b.total > a.total || Instant::now() > until {
                break b;
            }
        };
        assert!(b.total > a.total, "{a:?} {b:?}");
        let pct = busy_pct(&a, &b).expect("a share");
        assert!((0.0..=100.0).contains(&pct), "{pct}");
        let cfg =
            Config::parse("server_url = \"http://127.0.0.1:1\"\ntoken_file = \"/nonexistent\"\n")
                .unwrap();
        let m = Sampler::new().sample(&cfg, None, &[]);
        assert!(m.mem_total_bytes.is_some() && m.uptime_secs.is_some());
    }

    #[test]
    fn meminfo_gives_total_and_available_bytes() {
        let t = "MemTotal:       65536000 kB\nMemFree:  1 kB\nMemAvailable:   28000000 kB\n";
        assert_eq!(parse_meminfo(t), Some((65536000 * 1024, 28000000 * 1024)));
        assert_eq!(parse_meminfo("MemTotal: 1 kB\n"), None);
    }

    #[test]
    fn net_dev_sums_the_hosts_own_interfaces() {
        let t = "Inter-|   Receive |  Transmit\n face |bytes packets errs drop fifo frame compressed multicast|bytes\n    lo: 500 5 0 0 0 0 0 0 500 5 0 0 0 0 0 0\n  eth0: 1000 10 0 0 0 0 0 0 2000 20 0 0 0 0 0 0\n tap3: 7777 1 0 0 0 0 0 0 7777 1 0 0 0 0 0 0\n  eth1: 10 1 0 0 0 0 0 0 20 1 0 0 0 0 0 0\n";
        assert_eq!(parse_net_dev(t), Some(NetBytes { rx: 1010, tx: 2020 }));
        let (rx, tx) = rate(
            &NetBytes { rx: 1000, tx: 0 },
            &NetBytes { rx: 3000, tx: 500 },
            2.0,
        );
        assert_eq!((rx, tx), (Some(1000.0), Some(250.0)));
        // A counter that went back (an interface reset) gives no rate.
        assert_eq!(
            rate(&NetBytes { rx: 9, tx: 9 }, &NetBytes { rx: 1, tx: 9 }, 1.0),
            (None, None)
        );
    }

    #[test]
    fn pid_stat_reads_cpu_after_a_name_with_spaces_and_parens() {
        let t = "4242 (fire cracker) (x) S 1 4242 4242 0 -1 4194560 100 0 0 0 1500 250 0 0 20 0 3 0 99 0 0";
        assert_eq!(parse_pid_stat(t), Some(1750));
        assert_eq!(
            parse_vm_rss("Name: fc\nVmRSS:\t  1024 kB\n"),
            Some(1024 * 1024)
        );
    }

    #[test]
    fn vm_registry_follows_a_run_from_boot_to_teardown() {
        let id = "metrics-test-run";
        vm_booting(id, PathBuf::from("/nonexistent/scratch.ext4"));
        vm_process(id, 0);
        let mut s = Sampler::default();
        let cfg =
            Config::parse("server_url = \"http://127.0.0.1:1\"\ntoken_file = \"/nonexistent\"\n")
                .unwrap();
        let m = s.sample(&cfg, Some(4), &[]);
        let r = m.runs.iter().find(|r| r.run_id == id).unwrap();
        assert_eq!(r.phase, vm_phase::BOOTING);
        vm_booted(id, 1300);
        vm_paused(id, true);
        let m = s.sample(&cfg, Some(4), &[]);
        let r = m.runs.iter().find(|r| r.run_id == id).unwrap();
        assert_eq!(
            (r.phase.as_str(), r.boot_ms),
            (vm_phase::PAUSED, Some(1300))
        );
        assert!(m.boot.is_some_and(|b| b.samples >= 1));
        vm_gone(id);
        let m = s.sample(&cfg, Some(4), &[]);
        assert!(m.runs.iter().all(|r| r.run_id != id));
    }

    #[test]
    fn logins_check_presence_only() {
        let dir = tempfile::tempdir().unwrap();
        let claude = dir.path().join("claude");
        let codex = dir.path().join("codex");
        std::fs::create_dir_all(&claude).unwrap();
        std::fs::create_dir_all(&codex).unwrap();
        let toml = format!(
            "server_url = \"http://127.0.0.1:1\"\ntoken_file = \"/nonexistent\"\n[runtimes.claude]\nkind = \"claude\"\nenv = {{ CLAUDE_CONFIG_DIR = '{}' }}\n[runtimes.codex]\nkind = \"codex\"\nenv = {{ CODEX_HOME = '{}' }}\n",
            claude.display(),
            codex.display()
        );
        let cfg = Config::parse(&toml).unwrap();
        let by =
            |cfg: &Config, name: &str| logins(cfg).into_iter().find(|l| l.runtime == name).unwrap();
        assert_eq!(by(&cfg, "codex").state, login_state::LOGGED_OUT);
        std::fs::write(
            claude.join(".credentials.json"),
            r#"{"claudeAiOauth":{"accessToken":"x","refreshToken":"y"}}"#,
        )
        .unwrap();
        std::fs::write(
            codex.join("auth.json"),
            r#"{"OPENAI_API_KEY":null,"tokens":{"id_token":"z"}}"#,
        )
        .unwrap();
        let c = by(&cfg, "claude");
        assert_eq!(
            (c.state.as_str(), c.method.as_deref()),
            (login_state::LOGGED_IN, Some("subscription"))
        );
        let x = by(&cfg, "codex");
        assert_eq!(
            (x.state.as_str(), x.method.as_deref()),
            (login_state::LOGGED_IN, Some("subscription"))
        );
        std::fs::write(codex.join("auth.json"), r#"{"OPENAI_API_KEY":"sk-test"}"#).unwrap();
        assert_eq!(by(&cfg, "codex").method.as_deref(), Some("api_key"));
        // The value never leaves this module: only state and method are reported.
        let json = serde_json::to_string(&logins(&cfg)).unwrap();
        assert!(!json.contains("sk-test") && !json.contains("\"x\""));
    }

    /// 模 3.2: the CLI's own check wins over the credential files.
    #[test]
    fn the_status_check_decides_the_login() {
        use crate::accounts::SignIn;
        let files = |state: &str, method: Option<&str>| CliLogin {
            runtime: "claude".into(),
            state: state.into(),
            method: method.map(str::to_string),
            email: None,
            plan: None,
            confirmed: None,
        };
        // The files are there, the CLI says its sign-in is gone (expired or revoked).
        let mut l = files(login_state::LOGGED_IN, Some("subscription"));
        apply_sign_in(&mut l, Some(&SignIn::Out), false, false);
        assert_eq!(
            (l.state.as_str(), l.method, l.confirmed),
            (login_state::LOGGED_OUT, None, Some(true))
        );
        // The files alone say signed out (an apiKeyHelper or an env token they do not know),
        // and the check could not run: shown, but not confirmed, so runs still go there.
        let mut l = files(login_state::LOGGED_OUT, None);
        apply_sign_in(&mut l, Some(&SignIn::Unknown), false, false);
        assert_eq!(
            (l.state.as_str(), l.confirmed),
            (login_state::LOGGED_OUT, None)
        );
        let mut l = files(login_state::LOGGED_OUT, None);
        apply_sign_in(&mut l, None, false, false);
        assert_eq!(l.confirmed, None);
        // A key the CLI gets from the daemon's env still counts.
        let mut l = files(login_state::LOGGED_IN, Some("api_key"));
        apply_sign_in(&mut l, Some(&SignIn::Out), false, false);
        assert_eq!(l.state, login_state::LOGGED_IN);
        // macOS keeps Claude's credential in the Keychain: the check says signed in.
        let mut l = files(login_state::LOGGED_OUT, None);
        let a = crate::proto::LoginAccount {
            email: Some("a@example.com".into()),
            plan: Some("max".into()),
        };
        apply_sign_in(&mut l, Some(&SignIn::In(a)), false, false);
        assert_eq!(
            (l.state.as_str(), l.method.as_deref(), l.plan.as_deref()),
            (login_state::LOGGED_IN, Some("subscription"), Some("max"))
        );
        // The check vouches for it: a certain `logged_in` (feature "sign-in-confirmed").
        assert_eq!(l.confirmed, Some(true));
        // A check that could not run leaves the files' answer, not a certain one.
        let mut l = files(login_state::LOGGED_IN, Some("subscription"));
        apply_sign_in(&mut l, Some(&SignIn::Unknown), false, false);
        assert_eq!(
            (l.state.as_str(), l.confirmed),
            (login_state::LOGGED_IN, None)
        );
        // ... until a run of it succeeds.
        confirm_worked(&mut l, false);
        assert_eq!(l.confirmed, None);
        confirm_worked(&mut l, true);
        assert_eq!(l.confirmed, Some(true));
        // An API key saved from the app.
        let mut l = files(login_state::LOGGED_OUT, None);
        apply_sign_in(&mut l, Some(&SignIn::Out), true, false);
        assert_eq!(
            (l.state.as_str(), l.method.as_deref(), l.confirmed),
            (login_state::LOGGED_IN, Some("api_key"), Some(true))
        );
        // A run that just failed to sign in wins over everything.
        let mut l = files(login_state::LOGGED_IN, Some("subscription"));
        apply_sign_in(&mut l, None, true, true);
        assert_eq!(
            (l.state.as_str(), l.confirmed),
            (login_state::LOGGED_OUT, Some(true))
        );
        // A worked mark never turns a signed-out CLI into a certain sign-in.
        confirm_worked(&mut l, true);
        assert_eq!(l.state, login_state::LOGGED_OUT);
        assert_eq!(api_key_name("codex"), Some("OPENAI_API_KEY"));
        assert_eq!(api_key_name("acp"), None);
    }

    /// Feature "github-host": `metrics.github` from the host's `gh auth status` exit code, and
    /// the login from `gh api user` when signed in.
    #[cfg(unix)]
    #[tokio::test]
    async fn the_computers_github_comes_from_gh_auth_status() {
        let dir = tempfile::tempdir().unwrap();
        let gh = dir.path().join("gh");
        let path = dir.path().as_os_str();
        let state = |g: chat_protocol::machine::MachineGithub| (g.state, g.login);
        assert_eq!(state(github(Some(path)).await), ("no_gh".into(), None));
        std::fs::write(
            &gh,
            "#!/bin/sh\ncase \"$1 $2\" in\n  \"auth status\") exit 0 ;;\n  \"api user\") echo org-bot; exit 0 ;;\nesac\nexit 1\n",
        )
        .unwrap();
        crate::sys::set_mode(&gh, 0o755).unwrap();
        assert_eq!(
            state(github(Some(path)).await),
            ("signed_in".into(), Some("org-bot".into()))
        );
        std::fs::write(&gh, "#!/bin/sh\nexit 1\n").unwrap();
        assert_eq!(state(github(Some(path)).await), ("signed_out".into(), None));
    }
}
