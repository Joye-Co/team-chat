//! Subscription usage for the owner's monitor (feature "accounts"): Claude Code's 5-hour and
//! weekly windows from `claude -p /usage` (a local command that sends no model request and opens
//! no window), and Codex's windows from ChatGPT's usage endpoint with the machine's own Codex
//! sign-in. Read every 10 minutes and after a sign-in; nothing here leaves the machine but the
//! percentages and reset times.

use crate::config::RuntimeConfig;
use crate::proto::{CliUsage, UsageWindow};
use std::path::PathBuf;
use std::time::Duration;

/// How often usage is read.
pub const EVERY: Duration = Duration::from_secs(600);

/// Parses the text of Claude Code's `/usage`: "Current session: 13% used · resets …" and
/// "Current week (all models): 41% used · resets …". The reset stays in the label (the CLI
/// prints it in local words).
pub fn parse_claude(text: &str) -> Vec<UsageWindow> {
    let re = regex::Regex::new(r"(?m)^\s*(Current (session|week)[^:]*):\s*(\d+(?:\.\d+)?)%\s*used(?:\s*·\s*(resets [^\n]+))?")
        .expect("valid regex");
    re.captures_iter(text)
        .take(8)
        .filter_map(|c| {
            let label = c[1].trim().to_string();
            let kind = if &c[2] == "session" {
                "session"
            } else if label.contains("all models") || !label.contains('(') {
                "week"
            } else {
                "week_model"
            };
            let pct: f64 = c[3].parse().ok()?;
            let label = match c.get(4) {
                Some(r) => format!("{label} · {}", r.as_str().trim()),
                None => label,
            };
            Some(UsageWindow {
                kind: kind.into(),
                label: Some(label.chars().take(80).collect()),
                used_pct: pct.clamp(0., 100.),
                resets_at: None,
            })
        })
        .collect()
}

/// Parses ChatGPT's usage answer: `rate_limit.primary_window` / `secondary_window` with
/// `used_percent` and `reset_at` (unix seconds).
pub fn parse_codex(v: &serde_json::Value) -> Vec<UsageWindow> {
    let rl = &v["rate_limit"];
    [
        ("primary", &rl["primary_window"]),
        ("secondary", &rl["secondary_window"]),
    ]
    .into_iter()
    .filter_map(|(kind, w)| {
        let pct = w.get("used_percent")?.as_f64()?;
        let secs = w.get("limit_window_seconds").and_then(|x| x.as_u64());
        let label = secs.map(|s| {
            if s >= 86_400 {
                format!("{} days", s / 86_400)
            } else {
                format!("{} hours", (s + 1799) / 3600)
            }
        });
        Some(UsageWindow {
            kind: kind.into(),
            label,
            used_pct: pct.clamp(0., 100.),
            resets_at: w.get("reset_at").and_then(|x| x.as_u64()).map(|s| s * 1000),
        })
    })
    .collect()
}

fn env_or(rt: &RuntimeConfig, name: &str, default: PathBuf) -> PathBuf {
    rt.env
        .get(name)
        .cloned()
        .or_else(|| std::env::var(name).ok())
        .map(PathBuf::from)
        .unwrap_or(default)
}

async fn claude(rt: &RuntimeConfig) -> Option<Vec<UsageWindow>> {
    let program = crate::health::which(&rt.command())?;
    let mut cmd = crate::health::command(&program);
    cmd.args([
        "-p",
        "/usage",
        "--output-format",
        "json",
        "--tools",
        "",
        "--strict-mcp-config",
        "--setting-sources",
        "",
        "--no-session-persistence",
    ])
    .stdin(std::process::Stdio::null())
    .kill_on_drop(true);
    for (k, v) in &rt.env {
        cmd.env(k, v);
    }
    let out = tokio::time::timeout(Duration::from_secs(60), cmd.output())
        .await
        .ok()?
        .ok()?;
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).ok()?;
    Some(parse_claude(v.get("result")?.as_str()?))
}

async fn codex(rt: &RuntimeConfig) -> Option<Vec<UsageWindow>> {
    let home = env_or(rt, "CODEX_HOME", crate::config::home().join(".codex"));
    let auth: serde_json::Value =
        serde_json::from_slice(&std::fs::read(home.join("auth.json")).ok()?).ok()?;
    let tokens = auth.get("tokens")?;
    let access = tokens.get("access_token")?.as_str()?;
    let account = tokens.get("account_id").and_then(|x| x.as_str());
    let mut req = reqwest::Client::new()
        .get("https://chatgpt.com/backend-api/wham/usage")
        .bearer_auth(access)
        .header("originator", "codex_cli_rs")
        .header("accept", "application/json")
        .timeout(Duration::from_secs(20));
    if let Some(a) = account {
        req = req.header("chatgpt-account-id", a);
    }
    let resp = req.send().await.ok()?;
    if !resp.status().is_success() {
        return None;
    }
    let body = resp.bytes().await.ok()?;
    Some(parse_codex(&serde_json::from_slice(&body).ok()?))
}

/// Reads every Claude Code and Codex runtime's usage now; runtimes that give none are left out.
pub async fn read_all(runtimes: Vec<(String, RuntimeConfig)>) -> Vec<CliUsage> {
    let mut out = vec![];
    for (name, rt) in runtimes {
        let windows = match rt.kind.as_str() {
            "claude" => claude(&rt).await,
            "codex" => codex(&rt).await,
            _ => None,
        };
        if let Some(windows) = windows.filter(|w| !w.is_empty()) {
            out.push(CliUsage {
                runtime: name,
                at: crate::proto::now_ms(),
                windows,
            });
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn claude_usage_text_becomes_windows() {
        let t = "Current session: 13% used · resets Oct 1 at 3:30pm (Asia/Shanghai)\nCurrent week (all models): 41% used · resets Oct 6\nCurrent week (Opus): 7% used\nTotal cost: $0.00";
        let w = parse_claude(t);
        assert_eq!(w.len(), 3);
        assert_eq!((w[0].kind.as_str(), w[0].used_pct), ("session", 13.));
        assert_eq!(w[1].kind, "week");
        assert_eq!(w[2].kind, "week_model");
        assert!(w[0].label.as_deref().unwrap().contains("resets Oct 1"));
        assert!(parse_claude("Total cost: $0.0000").is_empty());
    }

    #[test]
    fn codex_usage_answer_becomes_windows() {
        let v = serde_json::json!({"plan_type":"plus","rate_limit":{"primary_window":{"used_percent":23.5,"limit_window_seconds":18000,"reset_at":1793640000},"secondary_window":{"used_percent":61,"limit_window_seconds":604800}}});
        let w = parse_codex(&v);
        assert_eq!(w.len(), 2);
        assert_eq!(w[0].resets_at, Some(1_793_640_000_000));
        assert_eq!(w[0].label.as_deref(), Some("5 hours"));
        assert_eq!(w[1].label.as_deref(), Some("7 days"));
    }
}
