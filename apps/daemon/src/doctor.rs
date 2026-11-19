//! `daemon doctor`: CLIs, versions, health and config problems. Never prints secrets.

use crate::config::Config;
use std::fmt::Write;
use std::path::Path;

/// What the server said when `doctor` asked it with the machine token.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Server {
    /// It answered and took the token.
    Accepted,
    /// It answered, on a path that does not look at the token.
    Reachable,
    /// It answered that the token is not a machine's (HTTP 401 or 403).
    Refused(u16),
    /// It answered something else: most likely `server_url` is another site.
    Answered(u16),
    Unreachable(String),
    /// Not asked: there is no token to ask with (a config problem says so).
    NoToken,
}

impl Server {
    fn line(&self, cfg: &Config) -> String {
        match self {
            Server::Accepted => "reachable, the machine token is accepted".into(),
            Server::Reachable => "reachable (the token was not checked)".into(),
            Server::Refused(s) => format!(
                "REFUSED the machine token (HTTP {s}): check token_file {} or add this computer again in the app",
                cfg.token_file.display()
            ),
            Server::Answered(s) => format!("answered HTTP {s}: check server_url"),
            Server::Unreachable(e) => format!("NOT REACHABLE: {e}"),
            Server::NoToken => "not checked (no token)".into(),
        }
    }

    fn ok(&self) -> bool {
        matches!(self, Server::Accepted | Server::Reachable | Server::NoToken)
    }

    /// Reads an HTTP answer of the update check. A 404 of our server is JSON
    /// (`{"error":{"code":"not_found",...}}`): "no release" comes after the token was checked.
    fn from_answer(status: u16, body: &[u8]) -> Server {
        match status {
            200..=299 => Server::Accepted,
            401 | 403 => Server::Refused(status),
            404 => {
                let error = serde_json::from_slice::<serde_json::Value>(body)
                    .ok()
                    .map(|v| v["error"].clone());
                match error {
                    Some(e) if e["message"] == "no release" => Server::Accepted,
                    Some(e) if e["code"] == "not_found" => Server::Reachable,
                    _ => Server::Answered(status),
                }
            }
            s => Server::Answered(s),
        }
    }
}

/// Asks the server over plain HTTP whether it takes the machine token: the request the daemon's
/// self-update makes. Never the machine link: the server keeps one link per machine, so a
/// `doctor` on the link would cut off the daemon that is running.
pub async fn check_server(cfg: &Config) -> Server {
    let Ok(token) = cfg.read_token() else {
        return Server::NoToken;
    };
    let client = match reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()
    {
        Ok(c) => c,
        Err(e) => return Server::Unreachable(e.to_string()),
    };
    let url = format!(
        "{}/v1/machine/updates/latest?platform={}",
        cfg.server_url.trim_end_matches('/'),
        chat_protocol::updates::PLATFORM_LINUX_X86_64
    );
    match client.get(&url).bearer_auth(token).send().await {
        Ok(res) => {
            let status = res.status().as_u16();
            let body = res.bytes().await.unwrap_or_default();
            Server::from_answer(status, &body)
        }
        Err(e) => {
            // reqwest's own text is only "error sending request": the cause says why.
            let e = e.without_url();
            let mut said = e.to_string();
            let mut cause = std::error::Error::source(&e);
            while let Some(c) = cause {
                said = format!("{said}: {c}");
                cause = c.source();
            }
            Server::Unreachable(said)
        }
    }
}

/// The report and whether nothing is wrong. `config_path` is the file `cfg` was read from;
/// `server` is [`check_server`]'s answer (None: not asked, the line is left out).
pub async fn report(cfg: &Config, config_path: &Path, server: Option<Server>) -> (String, bool) {
    let mut s = String::new();
    let problems = cfg.problems_at(config_path);
    let _ = writeln!(s, "daemon:       {}", crate::selfupdate::VERSION);
    let _ = writeln!(s, "config:       {}", config_path.display());
    let _ = writeln!(s, "server:       {}", cfg.server_url);
    if let Some(server) = &server {
        let _ = writeln!(s, "server check: {}", server.line(cfg));
    }
    let _ = writeln!(s, "token file:   {}", cfg.token_file.display());
    let _ = writeln!(
        s,
        "machine:      {}{}",
        cfg.name,
        if cfg.labels.is_empty() {
            String::new()
        } else {
            format!(" (labels: {})", cfg.labels.join(", "))
        }
    );
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
        let _ = writeln!(s, "codex:        {}", CODEX_SIDE_CHAT_ON_WINDOWS);
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
    let mut missing_clis = 0;
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
        missing_clis += usize::from(!c.available);
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
    if let Some(vm) = &cfg.vm {
        let slots = vm.slots(cfg.max_parallel_runs);
        let _ = writeln!(
            s,
            "vm:           slots {}..{} (taps rvm{}..rvm{}); {} vCPU, {} MiB unless the server sends a size, at most {} vCPU, {} MiB",
            slots.start,
            slots.end - 1,
            slots.start,
            slots.end - 1,
            vm.vcpus,
            vm.mem_mib,
            vm.max_vcpus(),
            vm.max_mem_mib()
        );
    }
    for line in cfg.vm.as_ref().map(old_homes).unwrap_or_default() {
        let _ = writeln!(s, "{line}");
    }
    for p in &problems {
        let _ = writeln!(s, "problem: {p}");
    }
    // What made it NOT OK, so the last line says where to look above.
    let count = |n: usize, one: &str, many: &str| match n {
        0 => None,
        1 => Some(format!("1 {one}")),
        n => Some(format!("{n} {many}")),
    };
    let wrong: Vec<String> = [
        count(problems.len(), "problem", "problems"),
        git.is_none().then(|| "git is missing".to_string()),
        count(
            missing_clis,
            "runtime's CLI not found",
            "runtimes' CLIs not found",
        ),
        server
            .as_ref()
            .is_some_and(|sv| !sv.ok())
            .then(|| "the server check failed".to_string()),
    ]
    .into_iter()
    .flatten()
    .collect();
    let ok = wrong.is_empty();
    if ok {
        let _ = writeln!(s, "OK");
    } else {
        let _ = writeln!(s, "NOT OK: {}", wrong.join(", "));
    }
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
    "a side chat is read-only (no writes); its commands can still reach the network on Windows";

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
    fn the_server_check_reads_the_answer() {
        let no_release = br#"{"error":{"code":"not_found","message":"no release"}}"#;
        assert_eq!(Server::from_answer(200, b"{}"), Server::Accepted);
        assert_eq!(Server::from_answer(404, no_release), Server::Accepted);
        assert_eq!(
            Server::from_answer(
                404,
                br#"{"error":{"code":"not_found","message":"no route"}}"#
            ),
            Server::Reachable
        );
        assert_eq!(Server::from_answer(404, b"<html>"), Server::Answered(404));
        assert_eq!(Server::from_answer(401, b""), Server::Refused(401));
        assert_eq!(Server::from_answer(502, b""), Server::Answered(502));
        assert!(Server::Accepted.ok() && Server::Reachable.ok() && Server::NoToken.ok());
        assert!(!Server::Refused(401).ok() && !Server::Unreachable("x".into()).ok());
    }

    #[tokio::test]
    async fn the_report_starts_with_the_version_and_config_and_ends_with_what_is_wrong() {
        let cfg = Config::parse(
            "server_url = \"http://127.0.0.1:1\"\ntoken_file = \"/nonexistent/token\"\nname = \"box\"\nlabels = [\"a\", \"b\"]\n[runtimes.gone]\nkind = \"acp\"\ncommand = \"/nonexistent/agent\"\n",
        )
        .unwrap();
        let at = Path::new("/etc/x/daemon.toml");
        let (report, ok) = report(&cfg, at, Some(Server::Refused(401))).await;
        assert!(!ok);
        let mut lines = report.lines();
        assert_eq!(
            lines.next().unwrap(),
            format!("daemon:       {}", crate::selfupdate::VERSION)
        );
        assert_eq!(lines.next().unwrap(), "config:       /etc/x/daemon.toml");
        assert!(
            report.contains("machine:      box (labels: a, b)"),
            "{report}"
        );
        assert!(
            report.contains("server check: REFUSED the machine token (HTTP 401): check token_file"),
            "{report}"
        );
        let last = report.lines().last().unwrap();
        assert!(last.starts_with("NOT OK: "), "{last}");
        assert!(last.contains("1 runtime's CLI not found"), "{last}");
        assert!(last.contains("the server check failed"), "{last}");
        assert!(last.contains("problem"), "{last}");
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
