//! What a CLI can run: its models and efforts (feature `effort`, docs/machine-protocol.md,
//! "Models and effort"). The daemon asks each CLI when it says `hello`, with [`LIMIT`] per CLI.
//! Finding nothing is never an error: the CLI then reports no list.

use crate::config::RuntimeConfig;
use crate::proto::{CliInfo, CliModel};
use crate::runtime::{Rpc, classify};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

/// How long the daemon asks one CLI for its models and efforts.
pub const LIMIT: Duration = Duration::from_secs(15);
pub const MAX_MODELS: usize = 50;
pub const MAX_EFFORTS: usize = 10;
/// `CliModel.id` and `name` length, in UTF-16 units like the server counts.
const MAX_ID: usize = 200;
const MAX_NAME: usize = 200;
/// `model/list` pages the daemon follows.
const MAX_PAGES: usize = 5;
/// The aliases Claude Code's `--model` takes.
pub const CLAUDE_ALIASES: &[&str] = &["fable", "opus", "sonnet", "haiku"];

/// What one CLI reported.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Found {
    pub models: Option<Vec<CliModel>>,
    pub efforts: Option<Vec<String>>,
}

impl Found {
    /// A model list with the CLI's efforts as the union of its models' efforts, in first-seen
    /// order.
    pub fn from_models(models: Vec<CliModel>) -> Found {
        let mut efforts: Vec<String> = vec![];
        for e in models.iter().flat_map(|m| m.efforts.iter().flatten()) {
            if !efforts.contains(e) {
                efforts.push(e.clone());
            }
        }
        Found {
            models: (!models.is_empty()).then_some(models),
            efforts: (!efforts.is_empty()).then_some(efforts),
        }
    }
}

/// An effort name as the protocol takes it: `^[a-z][a-z0-9_-]{0,31}$`.
pub fn valid_effort(e: &str) -> bool {
    let b = e.as_bytes();
    (1..=32).contains(&b.len())
        && b[0].is_ascii_lowercase()
        && b.iter()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, b'_' | b'-'))
}

fn valid_model_id(id: &str) -> bool {
    let n = id.encode_utf16().count();
    (1..=MAX_ID).contains(&n) && !id.chars().any(char::is_control) && !id.trim().is_empty()
}

/// Whether a run may pass `effort` to this CLI: a well-formed name the CLI reported.
pub fn accepts_effort(cli: Option<&CliInfo>, effort: &str) -> bool {
    valid_effort(effort)
        && cli
            .and_then(|c| c.efforts.as_ref())
            .is_some_and(|l| l.iter().any(|e| e == effort))
}

/// Models and efforts of one runtime: asked from the CLI (`program`, None when it is not
/// installed), then the `models` and `efforts` of `daemon.toml`, then cut to what the protocol
/// takes.
pub async fn discover(rt: &RuntimeConfig, program: Option<&Path>) -> Found {
    let found = match program {
        // Both set in daemon.toml: nothing to ask.
        _ if rt.models.is_some() && rt.efforts.is_some() => Found::default(),
        Some(p) if rt.kind == "codex" => {
            Found::from_models(codex_models(p, rt).await.unwrap_or_default())
        }
        Some(p) if rt.kind == "claude" => claude(p, rt).await,
        _ => Found::default(),
    };
    sanitize(apply_config(found, rt))
}

/// `daemon.toml` `models` and `efforts` replace what the CLI reported. A configured id the CLI
/// also listed keeps its name and efforts.
pub fn apply_config(found: Found, rt: &RuntimeConfig) -> Found {
    let mut out = found;
    if let Some(ids) = &rt.models {
        let listed = out.models.take().unwrap_or_default();
        let models: Vec<CliModel> = ids
            .iter()
            .map(|id| {
                listed
                    .iter()
                    .find(|m| &m.id == id)
                    .cloned()
                    .unwrap_or_else(|| CliModel {
                        id: id.clone(),
                        ..Default::default()
                    })
            })
            .collect();
        out.models = (!models.is_empty()).then_some(models);
    }
    if let Some(e) = &rt.efforts {
        out.efforts = (!e.is_empty()).then(|| e.clone());
    }
    out
}

fn clean_efforts(list: Vec<String>) -> Option<Vec<String>> {
    let mut out: Vec<String> = vec![];
    for e in list {
        if valid_effort(&e) && !out.contains(&e) && out.len() < MAX_EFFORTS {
            out.push(e);
        }
    }
    (!out.is_empty()).then_some(out)
}

/// Cuts to what the protocol takes: valid unique ids, names of at most 200 units, valid
/// efforts, at most 50 models and 10 efforts.
pub fn sanitize(found: Found) -> Found {
    let models = found.models.map(|list| {
        let mut out: Vec<CliModel> = vec![];
        for m in list {
            if out.len() == MAX_MODELS {
                break;
            }
            if !valid_model_id(&m.id) || out.iter().any(|o| o.id == m.id) {
                continue;
            }
            let name = m.name.map(|n| cut_utf16(n.trim(), MAX_NAME));
            out.push(CliModel {
                id: m.id,
                name: name.filter(|n| !n.is_empty()),
                efforts: m.efforts.and_then(clean_efforts),
                default_effort: m.default_effort.filter(|e| valid_effort(e)),
                is_default: m.is_default.filter(|d| *d),
            });
        }
        out
    });
    Found {
        models: models.filter(|m| !m.is_empty()),
        efforts: found.efforts.and_then(clean_efforts),
    }
}

/// The longest prefix of `s` with at most `max` UTF-16 units.
fn cut_utf16(s: &str, max: usize) -> String {
    let mut n = 0;
    s.chars()
        .take_while(|c| {
            n += c.len_utf16();
            n <= max
        })
        .collect()
}

// ---- Codex: `codex app-server`, `model/list` ----

/// One `model/list` page: the models that are not hidden and the next cursor.
pub fn parse_model_page(result: &Value) -> (Vec<CliModel>, Option<String>) {
    let s = |v: &Value| v.as_str().map(str::to_string);
    let models = result["data"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|m| m["hidden"].as_bool() != Some(true))
        .filter_map(|m| {
            let id = s(&m["id"]).or_else(|| s(&m["model"]))?;
            let efforts: Vec<String> = m["supportedReasoningEfforts"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|e| s(&e["reasoningEffort"]))
                .collect();
            Some(CliModel {
                id,
                name: s(&m["displayName"]),
                efforts: (!efforts.is_empty()).then_some(efforts),
                default_effort: s(&m["defaultReasoningEffort"]),
                is_default: (m["isDefault"].as_bool() == Some(true)).then_some(true),
            })
        })
        .collect();
    let cursor = s(&result["nextCursor"]).filter(|c| !c.is_empty());
    (models, cursor)
}

/// What to do after a line from `codex app-server`.
#[derive(Debug, PartialEq)]
pub enum Step {
    /// Write these messages.
    Send(Vec<Value>),
    /// Read the next line.
    Wait,
    /// The list is complete.
    Done(Vec<CliModel>),
    /// Codex did not answer with a list.
    Fail,
}

/// The `model/list` session: `initialize`, `initialized`, then `model/list` pages. Lines that
/// are not the answer it waits for (notifications, other ids) are skipped.
#[derive(Debug, Default)]
pub struct CodexModelList {
    next_id: u64,
    waiting: u64,
    initialized: bool,
    pages: usize,
    models: Vec<CliModel>,
}

impl CodexModelList {
    fn request(&mut self, method: &str, params: Value) -> Value {
        self.next_id += 1;
        self.waiting = self.next_id;
        json!({ "id": self.next_id, "method": method, "params": params })
    }

    /// The first message: `initialize`, with the client info a run sends.
    pub fn start(&mut self) -> Value {
        self.request(
            "initialize",
            json!({
                "clientInfo": { "name": "machine-daemon", "title": null, "version": env!("CARGO_PKG_VERSION") },
                "capabilities": null
            }),
        )
    }

    pub fn on_line(&mut self, line: &str) -> Step {
        let Ok(v) = serde_json::from_str::<Value>(line) else {
            return Step::Wait;
        };
        let Rpc::Response { id, result, error } = classify(&v) else {
            return Step::Wait;
        };
        if id.as_u64() != Some(self.waiting) {
            return Step::Wait;
        }
        if error.is_some() {
            // A later page that fails keeps the pages before it.
            return if self.models.is_empty() {
                Step::Fail
            } else {
                Step::Done(std::mem::take(&mut self.models))
            };
        }
        if !self.initialized {
            self.initialized = true;
            let list = self.request("model/list", json!({}));
            return Step::Send(vec![json!({ "method": "initialized" }), list]);
        }
        let (models, cursor) = parse_model_page(result.unwrap_or(&Value::Null));
        self.models.extend(models);
        self.pages += 1;
        match cursor {
            Some(c) if self.pages < MAX_PAGES => {
                Step::Send(vec![self.request("model/list", json!({ "cursor": c }))])
            }
            _ => Step::Done(std::mem::take(&mut self.models)),
        }
    }
}

/// A short `codex app-server` session for `model/list`, within [`LIMIT`]. The process group is
/// killed after it, however it ends.
async fn codex_models(program: &Path, rt: &RuntimeConfig) -> Option<Vec<CliModel>> {
    let mut cmd = crate::health::command(program);
    cmd.arg("app-server")
        .args(&rt.args)
        .envs(&rt.env)
        .current_dir(crate::config::home())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true);
    #[cfg(unix)]
    cmd.process_group(0);
    let mut child = crate::health::spawn(&mut cmd).ok()?;
    #[cfg(unix)]
    let pgid = child.id().map(|p| p as i32);
    let mut stdin = child.stdin.take()?;
    let stdout = child.stdout.take()?;
    let session = async {
        let mut lines = BufReader::new(stdout).lines();
        let mut list = CodexModelList::default();
        let mut send = vec![list.start()];
        loop {
            for m in send.drain(..) {
                let mut s = m.to_string();
                s.push('\n');
                stdin.write_all(s.as_bytes()).await.ok()?;
            }
            stdin.flush().await.ok()?;
            loop {
                let line = lines.next_line().await.ok()??;
                match list.on_line(&line) {
                    Step::Wait => continue,
                    Step::Send(m) => {
                        send = m;
                        break;
                    }
                    Step::Done(models) => return Some(models),
                    Step::Fail => return None,
                }
            }
        }
    };
    let out = tokio::time::timeout(LIMIT, session).await.ok().flatten();
    // Windows has no process groups to signal: the app-server itself is ended.
    #[cfg(not(unix))]
    let _ = child.start_kill();
    #[cfg(unix)]
    if let Some(pgid) = pgid.filter(|p| *p > 1) {
        // SAFETY: signals the process group this function started.
        unsafe {
            libc::killpg(pgid, libc::SIGKILL);
        }
    }
    let _ = tokio::time::timeout(Duration::from_secs(2), child.wait()).await;
    out
}

// ---- Codex: a default model the account cannot use ----

/// The top-level `model` of Codex's `config.toml` (`$CODEX_HOME`, else `~/.codex`), when set.
pub fn codex_config_model(rt: &RuntimeConfig) -> Option<String> {
    let home = rt
        .env
        .get("CODEX_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("CODEX_HOME").map(PathBuf::from))
        .unwrap_or_else(|| crate::config::home().join(".codex"));
    let text = std::fs::read_to_string(home.join("config.toml")).ok()?;
    top_level_model(&text)
}

fn top_level_model(toml: &str) -> Option<String> {
    for line in toml.lines() {
        let l = line.trim();
        if l.starts_with('[') {
            return None;
        }
        let Some(rest) = l.strip_prefix("model") else {
            continue;
        };
        let Some(v) = rest.trim_start().strip_prefix('=') else {
            continue;
        };
        let v = v
            .split('#')
            .next()
            .unwrap_or("")
            .trim()
            .trim_matches(|c| c == '"' || c == '\'');
        return (!v.is_empty()).then(|| v.to_string());
    }
    None
}

/// A run with no model of its own uses Codex's configured default. When the account's model
/// list does not have that model (Codex then fails with model_not_found), the run gets the
/// list's default model instead. None: keep Codex's own choice.
pub fn codex_fallback_model(cli: Option<&CliInfo>, configured: Option<&str>) -> Option<String> {
    let configured = configured?;
    let models = cli?.models.as_ref().filter(|m| !m.is_empty())?;
    if models.iter().any(|m| m.id == configured) {
        return None;
    }
    models
        .iter()
        .find(|m| m.is_default == Some(true))
        .or_else(|| models.first())
        .map(|m| m.id.clone())
}

#[cfg(test)]
mod codex_default_tests {
    use super::*;

    fn cli(ids: &[(&str, bool)]) -> CliInfo {
        CliInfo {
            runtime: "codex".into(),
            kind: "codex".into(),
            command: "codex".into(),
            version: None,
            available: true,
            models: Some(
                ids.iter()
                    .map(|(id, d)| CliModel {
                        id: (*id).into(),
                        is_default: d.then_some(true),
                        ..Default::default()
                    })
                    .collect(),
            ),
            efforts: None,
            gates: None,
        }
    }

    #[test]
    fn reads_only_the_top_level_model() {
        assert_eq!(
            top_level_model("# c\nmodel = \"gpt-x\" # note\n[profiles.a]\nmodel = \"y\""),
            Some("gpt-x".into())
        );
        assert_eq!(top_level_model("[profiles.a]\nmodel = \"y\""), None);
        assert_eq!(top_level_model("model_provider = \"z\""), None);
    }

    #[test]
    fn falls_back_only_when_the_account_lacks_the_configured_model() {
        let c = cli(&[("gpt-6-luna", true), ("gpt-5.6-terra", false)]);
        assert_eq!(
            codex_fallback_model(Some(&c), Some("gpt-6.1-sol")),
            Some("gpt-6-luna".into())
        );
        assert_eq!(codex_fallback_model(Some(&c), Some("gpt-5.6-terra")), None);
        assert_eq!(codex_fallback_model(Some(&c), None), None);
        assert_eq!(
            codex_fallback_model(Some(&cli(&[])), Some("gpt-6.1-sol")),
            None
        );
    }
}

// ---- Claude Code: aliases, the settings model, `claude --help` ----

/// The CLI's config folder for a runtime: its `CLAUDE_CONFIG_DIR`, else the daemon's, else
/// `~/.claude` (what a run gets, see `run::build_env`).
pub fn claude_config_dir(rt: &RuntimeConfig) -> PathBuf {
    rt.env
        .get("CLAUDE_CONFIG_DIR")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("CLAUDE_CONFIG_DIR").map(PathBuf::from))
        .unwrap_or_else(|| crate::config::home().join(".claude"))
}

/// The `model` of the user's `settings.json`, when set.
pub fn claude_settings_model(config_dir: &Path) -> Option<String> {
    let s = crate::runtime::claude::aside_settings(config_dir)?;
    s["model"].as_str().map(str::to_string)
}

/// The aliases, with the settings model as the default: an alias it names, or one more model.
pub fn claude_models(default: Option<&str>) -> Vec<CliModel> {
    let mut models: Vec<CliModel> = CLAUDE_ALIASES
        .iter()
        .map(|a| CliModel {
            id: a.to_string(),
            ..Default::default()
        })
        .collect();
    if let Some(d) = default.map(str::trim).filter(|d| !d.is_empty()) {
        match models.iter_mut().find(|m| m.id == d) {
            Some(m) => m.is_default = Some(true),
            None => models.push(CliModel {
                id: d.to_string(),
                is_default: Some(true),
                ..Default::default()
            }),
        }
    }
    models
}

/// The efforts `claude --help` lists for `--effort`: the parenthesised comma list in the
/// option's description (`(low, medium, high, xhigh, max)`), which may wrap to the next lines.
/// None when the option or the list is missing.
pub fn claude_help_efforts(help: &str) -> Option<Vec<String>> {
    let mut lines = help.lines();
    let first = lines.find(|l| {
        l.trim_start()
            .split([' ', ',', '=', '<'])
            .any(|w| w == "--effort")
    })?;
    let mut text = first.to_string();
    for l in lines {
        if l.trim_start().starts_with('-') || l.trim().is_empty() {
            break;
        }
        text.push(' ');
        text.push_str(l.trim());
    }
    let mut rest = text.as_str();
    while let Some(open) = rest.find('(') {
        let after = &rest[open + 1..];
        let Some(close) = after.find(')') else {
            break;
        };
        let items: Vec<String> = after[..close]
            .split(',')
            .map(|i| i.trim().trim_matches(['"', '\'']).to_string())
            .collect();
        if items.iter().all(|i| valid_effort(i)) {
            return Some(items);
        }
        rest = &after[close + 1..];
    }
    None
}

async fn claude(program: &Path, rt: &RuntimeConfig) -> Found {
    let default = claude_settings_model(&claude_config_dir(rt));
    let mut cmd = crate::health::command(program);
    cmd.arg("--help")
        .envs(&rt.env)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    let help = match tokio::time::timeout(LIMIT, crate::health::output(&mut cmd)).await {
        Ok(Ok(o)) => format!(
            "{}\n{}",
            String::from_utf8_lossy(&o.stdout),
            String::from_utf8_lossy(&o.stderr)
        ),
        _ => String::new(),
    };
    Found {
        models: Some(claude_models(default.as_deref())),
        efforts: claude_help_efforts(&help),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    fn rt(kind: &str) -> RuntimeConfig {
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
        }
    }

    fn model(id: &str, efforts: &[&str]) -> Value {
        json!({
            "id": id, "model": id, "displayName": id.to_uppercase(), "hidden": false,
            "supportedReasoningEfforts": efforts.iter()
                .map(|e| json!({ "reasoningEffort": e, "description": "…" }))
                .collect::<Vec<_>>(),
            "defaultReasoningEffort": efforts.first(), "isDefault": false
        })
    }

    #[test]
    fn model_page_skips_hidden_and_maps_fields() {
        let mut first = model("gpt-6-astra", &["low", "medium", "high"]);
        first["isDefault"] = json!(true);
        let mut hidden = model("old", &["low"]);
        hidden["hidden"] = json!(true);
        let page = json!({ "data": [first, hidden, model("gpt-6-mini", &["minimal", "low"])],
            "nextCursor": null });
        let (models, cursor) = parse_model_page(&page);
        assert_eq!(cursor, None);
        assert_eq!(models.len(), 2);
        assert_eq!(
            models[0],
            CliModel {
                id: "gpt-6-astra".into(),
                name: Some("GPT-6-ASTRA".into()),
                efforts: Some(vec!["low".into(), "medium".into(), "high".into()]),
                default_effort: Some("low".into()),
                is_default: Some(true),
            }
        );
        assert_eq!(models[1].is_default, None);
        let f = Found::from_models(models);
        assert_eq!(
            f.efforts.unwrap(),
            ["low", "medium", "high", "minimal"].map(String::from)
        );
    }

    /// Feeds the session the answers of a codex app-server and returns what it wrote and the
    /// result.
    fn session(answers: &[Value]) -> (Vec<Value>, Option<Step>) {
        let mut list = CodexModelList::default();
        let mut sent = vec![list.start()];
        for a in answers {
            match list.on_line(&a.to_string()) {
                Step::Wait => {}
                Step::Send(m) => sent.extend(m),
                end => return (sent, Some(end)),
            }
        }
        (sent, None)
    }

    #[test]
    fn model_list_session_skips_notifications_and_follows_pages() {
        let page = |id: &str, cursor: Value| json!({ "result": { "data": [model(id, &["low", "high"])], "nextCursor": cursor } });
        let mut p1 = page("a", json!("c1"));
        p1["id"] = json!(2);
        let mut p2 = page("b", json!(null));
        p2["id"] = json!(3);
        let (sent, end) = session(&[
            json!({ "method": "configWarning", "params": { "summary": "x" } }),
            json!({ "id": 1, "result": { "userAgent": "codex" } }),
            json!({ "method": "configWarning", "params": {} }),
            json!({ "id": 7, "result": {} }),
            p1,
            json!({ "method": "account/updated", "params": {} }),
            p2,
        ]);
        let methods: Vec<&str> = sent.iter().filter_map(|m| m["method"].as_str()).collect();
        assert_eq!(
            methods,
            ["initialize", "initialized", "model/list", "model/list"]
        );
        assert_eq!(sent[2]["params"], json!({}));
        assert_eq!(sent[3]["params"], json!({ "cursor": "c1" }));
        let Some(Step::Done(models)) = end else {
            panic!("{end:?}")
        };
        assert_eq!(
            models.iter().map(|m| m.id.as_str()).collect::<Vec<_>>(),
            ["a", "b"]
        );
    }

    #[test]
    fn model_list_stops_after_five_pages() {
        let mut answers = vec![json!({ "id": 1, "result": {} })];
        for i in 0..10u64 {
            answers.push(json!({ "id": i + 2, "result": {
                "data": [model(&format!("m{i}"), &["low"])], "nextCursor": format!("c{i}") } }));
        }
        let (sent, end) = session(&answers);
        assert_eq!(sent.len(), 2 + MAX_PAGES);
        let Some(Step::Done(models)) = end else {
            panic!("{end:?}")
        };
        assert_eq!(models.len(), MAX_PAGES);
    }

    #[test]
    fn model_list_error_fails_or_keeps_earlier_pages() {
        let err = |id: u64| json!({ "id": id, "error": { "code": -32601, "message": "unknown" } });
        let (_, end) = session(&[json!({ "id": 1, "result": {} }), err(2)]);
        assert_eq!(end, Some(Step::Fail));
        let (_, end) = session(&[err(1)]);
        assert_eq!(end, Some(Step::Fail));
        let (_, end) = session(&[
            json!({ "id": 1, "result": {} }),
            json!({ "id": 2, "result": { "data": [model("a", &[])], "nextCursor": "c" } }),
            err(3),
        ]);
        assert!(matches!(end, Some(Step::Done(m)) if m.len() == 1));
    }

    #[test]
    fn claude_help_lists_efforts() {
        let help = "Options:\n  --debug-file <path>   Write debug logs\n  --effort <level>                      Effort level for the current session\n                                        (low, medium, high, xhigh, max)\n  --environment <environment_id>        Create a new cloud session (on a host)\n";
        assert_eq!(
            claude_help_efforts(help).unwrap(),
            ["low", "medium", "high", "xhigh", "max"].map(String::from)
        );
        let one_line = "  --effort <level>  Effort (default: high) (low, high)\n  --x  y\n";
        assert_eq!(
            claude_help_efforts(one_line).unwrap(),
            ["low", "high"].map(String::from)
        );
        assert_eq!(
            claude_help_efforts("  --model <m>  Model (sonnet, opus)\n"),
            None
        );
        assert_eq!(
            claude_help_efforts("  --effort <level>  Effort level\n  --x (a, b)\n"),
            None
        );
        assert_eq!(claude_help_efforts(""), None);
    }

    #[test]
    fn claude_settings_model_is_the_default() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(claude_settings_model(dir.path()), None);
        let ids = |m: &[CliModel]| {
            m.iter()
                .map(|m| (m.id.clone(), m.is_default == Some(true)))
                .collect::<Vec<_>>()
        };
        let plain = claude_models(None);
        assert_eq!(
            ids(&plain),
            CLAUDE_ALIASES
                .iter()
                .map(|a| (a.to_string(), false))
                .collect::<Vec<_>>()
        );
        std::fs::write(
            dir.path().join("settings.json"),
            r#"{"model":"opus","hooks":{}}"#,
        )
        .unwrap();
        let m = claude_models(claude_settings_model(dir.path()).as_deref());
        assert_eq!(m.len(), 4);
        assert_eq!(m[1].is_default, Some(true));
        assert_eq!(m.iter().filter(|m| m.is_default.is_some()).count(), 1);
        std::fs::write(
            dir.path().join("settings.json"),
            r#"{"model":"claude-opus-9[1m]"}"#,
        )
        .unwrap();
        let m = claude_models(claude_settings_model(dir.path()).as_deref());
        assert_eq!(m.len(), 5);
        assert_eq!(m[4].id, "claude-opus-9[1m]");
        assert_eq!(m[4].is_default, Some(true));
        let mut r = rt("claude");
        r.env
            .insert("CLAUDE_CONFIG_DIR".into(), dir.path().display().to_string());
        assert_eq!(claude_config_dir(&r), dir.path());
    }

    #[test]
    fn config_replaces_what_the_cli_reported() {
        let found = Found::from_models(vec![
            CliModel {
                id: "a".into(),
                name: Some("A".into()),
                efforts: Some(vec!["low".into()]),
                ..Default::default()
            },
            CliModel {
                id: "b".into(),
                ..Default::default()
            },
        ]);
        let mut r = rt("codex");
        assert_eq!(apply_config(found.clone(), &r), found);
        r.models = Some(vec!["a".into(), "custom".into()]);
        r.efforts = Some(vec!["fast".into()]);
        let f = apply_config(found.clone(), &r);
        let models = f.models.unwrap();
        assert_eq!(models[0].name.as_deref(), Some("A"));
        assert_eq!(models[1].id, "custom");
        assert_eq!(models[1].name, None);
        assert_eq!(f.efforts.unwrap(), ["fast"]);
        r.models = Some(vec![]);
        r.efforts = Some(vec![]);
        assert_eq!(apply_config(found, &r), Found::default());
    }

    #[tokio::test]
    async fn config_alone_needs_no_cli() {
        let mut r = rt("acp");
        r.models = Some(vec!["gemini-pro".into()]);
        r.efforts = Some(vec!["low".into(), "High".into(), "high".into()]);
        let f = discover(&r, None).await;
        assert_eq!(f.models.unwrap()[0].id, "gemini-pro");
        assert_eq!(f.efforts.unwrap(), ["low", "high"]);
        assert_eq!(discover(&rt("pty"), None).await, Found::default());
    }

    #[test]
    fn sanitize_keeps_what_the_protocol_takes() {
        assert!(valid_effort("xhigh") && valid_effort("a") && valid_effort("low_2-x"));
        for bad in ["", "High", "1low", "lo w", "-x", &"a".repeat(33), "é"] {
            assert!(!valid_effort(bad), "{bad}");
        }
        let mut models: Vec<CliModel> = (0..60)
            .map(|i| CliModel {
                id: format!("m{i}"),
                ..Default::default()
            })
            .collect();
        models.insert(
            0,
            CliModel {
                id: "x".into(),
                name: Some("名".repeat(250)),
                efforts: Some(vec!["low".into(), "BAD".into(), "low".into()]),
                default_effort: Some("Bad".into()),
                is_default: Some(false),
            },
        );
        models.insert(
            1,
            CliModel {
                id: "x".into(),
                ..Default::default()
            },
        );
        models.insert(
            2,
            CliModel {
                id: "".into(),
                ..Default::default()
            },
        );
        models.insert(
            3,
            CliModel {
                id: "a".repeat(201),
                ..Default::default()
            },
        );
        models.insert(
            4,
            CliModel {
                id: "a\nb".into(),
                ..Default::default()
            },
        );
        let efforts: Vec<String> = (0..15).map(|i| format!("e{i}")).collect();
        let f = sanitize(Found {
            models: Some(models),
            efforts: Some(efforts),
        });
        let m = f.models.unwrap();
        assert_eq!(m.len(), MAX_MODELS);
        assert_eq!(m[0].id, "x");
        assert_eq!(m[0].name.as_ref().unwrap().chars().count(), MAX_NAME);
        assert_eq!(m[0].efforts.as_deref(), Some(&["low".to_string()][..]));
        assert_eq!(m[0].default_effort, None);
        assert_eq!(m[0].is_default, None);
        assert_eq!(m[1].id, "m0");
        assert_eq!(f.efforts.unwrap().len(), MAX_EFFORTS);
    }

    #[test]
    fn runs_take_only_reported_efforts() {
        let cli = CliInfo {
            runtime: "claude".into(),
            kind: "claude".into(),
            command: "claude".into(),
            version: None,
            available: true,
            models: None,
            efforts: Some(vec!["low".into(), "max".into()]),
            gates: None,
        };
        assert!(accepts_effort(Some(&cli), "max"));
        assert!(!accepts_effort(Some(&cli), "medium"));
        assert!(!accepts_effort(None, "max"));
        let none = CliInfo {
            efforts: None,
            ..cli.clone()
        };
        assert!(!accepts_effort(Some(&none), "max"));
        let odd = CliInfo {
            efforts: Some(vec!["--max".into()]),
            ..cli
        };
        assert!(!accepts_effort(Some(&odd), "--max"));
    }
}
