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
    let _ = writeln!(
        s,
        "git:          {}",
        if git.is_some() { "found" } else { "MISSING" }
    );
    // Windows: which shell Claude Code's command tool and hooks use. Not a problem either way;
    // it tells the owner what a run's commands are written in.
    #[cfg(windows)]
    if cfg.runtimes.values().any(|rt| rt.kind == "claude") {
        let _ = writeln!(
            s,
            "claude shell: {}",
            match crate::sys::git_bash() {
                Some(bash) => format!("Git Bash ({})", bash.display()),
                None => "PowerShell (Git for Windows was not found)".into(),
            }
        );
    }
    let h = crate::health::health(cfg, 0);
    let _ = writeln!(
        s,
        "disk free:    {:.1} GiB of {:.1} GiB",
        h.disk_free_bytes as f64 / (1u64 << 30) as f64,
        h.disk_total_bytes as f64 / (1u64 << 30) as f64
    );
    let _ = writeln!(
        s,
        "load:         {} {} {} ({} cpus)",
        h.load1, h.load5, h.load15, h.cpu_count
    );
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
            if c.available {
                c.version.clone().unwrap_or_else(|| "found".into())
            } else {
                "NOT FOUND".into()
            }
        );
        if c.models.is_some() || c.efforts.is_some() {
            let models: Vec<&str> = c.models.iter().flatten().map(|m| m.id.as_str()).collect();
            let efforts = c.efforts.as_deref().unwrap_or_default();
            let _ = writeln!(
                s,
                "             models: {}; efforts: {}",
                if models.is_empty() {
                    "-".into()
                } else {
                    models.join(", ")
                },
                if efforts.is_empty() {
                    "-".into()
                } else {
                    efforts.join(", ")
                }
            );
        }
        // A Claude Code whose runs this machine cannot stop at gated commands (on Windows one
        // older than the tested version, or one that does not say its version): the owner
        // should know, the team's rules reach the bot only as text then.
        if c.kind == "claude" && c.available && c.gates != Some(true) {
            let [a, b, patch] = crate::gates::WINDOWS_MIN_CLAUDE;
            let _ = writeln!(
                s,
                "             gated actions: off. Commands that need a person's approval are not \
stopped on this machine: update Claude Code to {a}.{b}.{patch} or later"
            );
        }
        ok &= c.available;
    }
    if h.disk_free_bytes < cfg.min_free_gb * (1 << 30) {
        let _ = writeln!(
            s,
            "warning: free disk is below min_free_gb ({})",
            cfg.min_free_gb
        );
    }
    if let Some(vm) = cfg.vm.as_ref().filter(|vm| crate::vm_home::images_on(vm))
        && crate::vm_home::helper_has_homes(vm) == Some(false)
    {
        let _ = writeln!(s, "warning: {}", crate::vm_home::OLD_HELPER);
    }
    for p in &problems {
        let _ = writeln!(s, "problem: {p}");
    }
    let _ = writeln!(s, "{}", if ok { "OK" } else { "NOT OK" });
    (s, ok)
}
