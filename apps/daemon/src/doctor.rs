//! `daemon doctor`: CLIs, versions, health and config problems. Never prints secrets.

use crate::config::Config;
use std::fmt::Write;

pub async fn report(cfg: &Config) -> (String, bool) {
    let mut s = String::new();
    let problems = cfg.problems();
    let _ = writeln!(s, "server:       {}", cfg.server_url);
    let _ = writeln!(s, "token file:   {}", cfg.token_file.display());
    let _ = writeln!(s, "machine:      {} {:?}", cfg.name, cfg.labels);
    let _ = writeln!(s, "work root:    {}", cfg.work_root.display());
    let _ = writeln!(s, "max parallel: {}", cfg.max_parallel_runs);
    let git = crate::health::which("git");
    let _ = writeln!(s, "git:          {}", if git.is_some() { "found" } else { "MISSING" });
    let h = crate::health::health(cfg, 0);
    let _ = writeln!(
        s,
        "disk free:    {:.1} GiB of {:.1} GiB",
        h.disk_free_bytes as f64 / (1u64 << 30) as f64,
        h.disk_total_bytes as f64 / (1u64 << 30) as f64
    );
    let _ = writeln!(s, "load:         {} {} {} ({} cpus)", h.load1, h.load5, h.load15, h.cpu_count);
    let _ = writeln!(s, "runtimes:");
    let clis = crate::health::discover_clis(cfg).await;
    let mut ok = problems.is_empty() && git.is_some();
    for c in &clis {
        let _ = writeln!(
            s,
            "  {:<10} {:<6} {:<10} {}",
            c.runtime,
            c.kind,
            c.command,
            if c.available { c.version.clone().unwrap_or_else(|| "found".into()) } else { "NOT FOUND".into() }
        );
        ok &= c.available;
    }
    if h.disk_free_bytes < cfg.min_free_gb * (1 << 30) {
        let _ = writeln!(s, "warning: free disk is below min_free_gb ({})", cfg.min_free_gb);
    }
    for p in &problems {
        let _ = writeln!(s, "problem: {p}");
    }
    let _ = writeln!(s, "{}", if ok { "OK" } else { "NOT OK" });
    (s, ok)
}
