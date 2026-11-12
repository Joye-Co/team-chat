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
    // Windows: a Codex side chat cannot write, but its commands are not cut off from the
    // network as they are on macOS and Linux (see ASIDE_WINDOWS_SANDBOX).
    #[cfg(windows)]
    if cfg.runtimes.values().any(|rt| rt.kind == "codex") {
        let _ = writeln!(s, "codex side chat: {}", CODEX_SIDE_CHAT_ON_WINDOWS);
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
        if c.kind == "acp" && c.available {
            let _ = writeln!(
                s,
                "             continues sessions (session/load): {}",
                if c.resume == Some(true) { "yes" } else { "no" }
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
    let caches = crate::vm_cache::list(&cfg.work_root);
    if !caches.is_empty() {
        let _ = writeln!(
            s,
            "caches:       {} ({:.1} GiB)",
            caches.len(),
            caches.iter().map(|c| c.bytes).sum::<u64>() as f64 / (1u64 << 30) as f64
        );
    }
    for line in cfg.vm.as_ref().map(old_homes).unwrap_or_default() {
        let _ = writeln!(s, "{line}");
    }
    for p in &problems {
        let _ = writeln!(s, "problem: {p}");
    }
    let _ = writeln!(s, "{}", if ok { "OK" } else { "NOT OK" });
    (s, ok)
}

/// What `doctor` says about what is left of the per-member homes of earlier versions (#227).
fn old_homes(vm: &crate::config::VmConfig) -> Vec<String> {
    let mut out = vec![];
    if vm.home_gb.is_some() {
        out.push("note: [vm].home_gb is ignored".to_string());
    }
    let images = (!vm.homes.as_os_str().is_empty())
        .then(|| vm.homes.parent().map(|p| p.join("home-images")))
        .flatten();
    if images.is_some_and(|d| std::fs::read_dir(d).is_ok_and(|mut e| e.next().is_some())) {
        out.push(
            "warning: old home images are still on disk: run scripts/runner/host-setup.sh \
again as root"
                .to_string(),
        );
    }
    out
}

/// What `doctor` says about a side chat of Codex on Windows.
#[cfg(windows)]
pub const CODEX_SIDE_CHAT_ON_WINDOWS: &str =
    "read-only (no writes); its commands can still reach the network on Windows";

#[cfg(test)]
mod tests {
    use super::*;

    fn vm(extra: &str) -> crate::config::VmConfig {
        Config::parse(&format!(
            "server_url = \"http://127.0.0.1:1\"\ntoken_file = \"/nonexistent\"\n[runtimes.claude]\nkind = \"claude\"\n[vm]\nkernel = \"/k\"\nrootfs = \"/r\"\n{extra}"
        ))
        .unwrap()
        .vm
        .unwrap()
    }

    #[test]
    fn doctor_notes_the_old_home_settings_and_images() {
        let tmp = tempfile::tempdir().unwrap();
        let homes = tmp.path().join("homes");
        let at = format!("homes = '{}'\n", homes.display());
        assert!(old_homes(&vm(&at)).is_empty());
        assert_eq!(
            old_homes(&vm(&format!("{at}home_gb = 20\n"))),
            ["note: [vm].home_gb is ignored"]
        );
        let images = tmp.path().join("home-images");
        std::fs::create_dir_all(&images).unwrap();
        // An empty folder is fine.
        assert!(old_homes(&vm(&at)).is_empty());
        std::fs::write(images.join("01BOT.ext4"), b"x").unwrap();
        let said = old_homes(&vm(&at));
        assert_eq!(said.len(), 1);
        assert!(said[0].starts_with("warning: old home images are still on disk"));
    }
}
