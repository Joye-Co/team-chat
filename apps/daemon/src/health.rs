//! Machine health and CLI discovery.

use crate::config::{Config, RuntimeConfig};
use crate::proto::{CliInfo, Health};
use std::path::{Path, PathBuf};
use std::time::Duration;

pub fn which(cmd: &str) -> Option<PathBuf> {
    if cmd.contains('/') {
        let p = PathBuf::from(cmd);
        return is_executable(&p).then_some(p);
    }
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path)
        .map(|d| d.join(cmd))
        .find(|p| is_executable(p))
}

fn is_executable(p: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(p)
        .map(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
        .unwrap_or(false)
}

/// First line of `<cmd> --version`, with a 10 s limit.
pub async fn cli_version(cmd: &str) -> Option<String> {
    let mut c = tokio::process::Command::new(cmd);
    c.arg("--version")
        .stdin(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .kill_on_drop(true);
    let out = tokio::time::timeout(Duration::from_secs(10), c.output())
        .await
        .ok()?
        .ok()?;
    let s = String::from_utf8_lossy(&out.stdout);
    s.lines()
        .next()
        .map(|l| l.trim().to_string())
        .filter(|l| !l.is_empty())
}

/// Every configured runtime: whether its CLI is installed, its version and what it can run
/// ([`crate::models`]), asked of all CLIs at once.
pub async fn discover_clis(cfg: &Config) -> Vec<CliInfo> {
    futures_util::future::join_all(cfg.runtimes.iter().map(|(name, rt)| discover_cli(name, rt)))
        .await
}

async fn discover_cli(name: &str, rt: &RuntimeConfig) -> CliInfo {
    let command = rt.command();
    let found = which(&command);
    let version = match &found {
        Some(p) if rt.kind != "pty" => cli_version(&p.to_string_lossy()).await,
        _ => None,
    };
    let can_run = crate::models::discover(rt, found.as_deref()).await;
    CliInfo {
        runtime: name.to_string(),
        kind: rt.kind.clone(),
        command,
        version,
        available: found.is_some(),
        models: can_run.models,
        efforts: can_run.efforts,
    }
}

pub fn disk(path: &Path) -> (u64, u64) {
    let mut p = path.to_path_buf();
    while !p.exists() {
        if !p.pop() {
            break;
        }
    }
    let Ok(c) = std::ffi::CString::new(p.to_string_lossy().as_bytes()) else {
        return (0, 0);
    };
    // SAFETY: statvfs writes into a zeroed struct; c is a valid NUL-terminated path.
    unsafe {
        let mut s: libc::statvfs = std::mem::zeroed();
        if libc::statvfs(c.as_ptr(), &mut s) != 0 {
            return (0, 0);
        }
        let frsize = s.f_frsize as u64;
        (s.f_bavail as u64 * frsize, s.f_blocks as u64 * frsize)
    }
}

pub fn health(cfg: &Config, active_runs: u32) -> Health {
    let (free, total) = disk(&cfg.work_root);
    let mut l = [0f64; 3];
    // SAFETY: l has room for 3 doubles.
    let n = unsafe { libc::getloadavg(l.as_mut_ptr(), 3) };
    if n < 3 {
        l = [0.0; 3];
    }
    let round = |x: f64| (x * 100.0).round() / 100.0;
    Health {
        disk_free_bytes: free,
        disk_total_bytes: total,
        load1: round(l[0]),
        load5: round(l[1]),
        load15: round(l[2]),
        cpu_count: std::thread::available_parallelism()
            .map(|n| n.get() as u32)
            .unwrap_or(1),
        active_runs,
    }
}
