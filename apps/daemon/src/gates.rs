//! Gated actions (feature "gates", docs/machine-protocol.md "Gated actions").
//!
//! An AI member runs with every permission. Only the few action kinds the team checked in its
//! settings (合并到 main, 发布到生产, …) need a person, and only at the moment a command would
//! run. `run.assign.gates` lists the kinds with their patterns (regular expressions, matched
//! case-insensitively against each simple command of a shell line). Claude Code runs get a
//! PreToolUse hook (`chat-run gate`, see [`hook_main`]) that sends each command of a shell tool
//! (Bash, PowerShell, Monitor; and each MCP tool call, as `<tool name> <input JSON>`) to the
//! daemon over the run's `chat-run` socket.
//! A match sends `approval.request {kind, command}` and the hook waits until `run.approval`
//! comes: approve lets the command run, deny blocks it and the CLI reads why. `scope: "run"`
//! (这件事里都同意) lets later matches of the same kind in the run pass without asking.
//! Everything else passes at once; nothing else is ever stopped.

use crate::proto::{RunEvent, RunGate, truncate};
use regex::{Regex, RegexBuilder};
use serde_json::{Value, json};
use std::collections::{HashMap, HashSet};
use std::io::Read;
use std::sync::Mutex;
use std::time::Duration;
use tokio::sync::oneshot;

/// The `chat-run` op of the hook.
pub const OP: &str = "gate.check";
/// Approval ids of this module start with this.
const PREFIX: &str = "daemon-gate-";
/// Longest command in `approval.request` (the protocol's limit of `command`).
pub const MAX_COMMAND: usize = 4000;
/// Largest hook input read from standard input.
const MAX_HOOK_INPUT: u64 = 1 << 20;
/// How long the hook waits for a person (the run's own deadline ends it sooner). Claude Code
/// kills a hook after its `timeout`, which the settings set to the same.
pub const HOOK_WAIT: Duration = Duration::from_secs(24 * 3600);

/// Runtime kinds whose runs stop at gated commands. Codex's app-server asks for approval only
/// under an approval policy that asks for (almost) every command, so its runs are not gated
/// yet and the server keeps the rules in the prompt for them.
pub fn enforced_by(kind: &str) -> bool {
    kind == "claude"
}

/// The oldest Claude Code whose runs are gated on Windows: 2.1.295 added `onFailure: "block"`
/// for command hooks (and 2.1.139 the `args` form the hook uses there, see
/// [`claude_settings`]). An older CLI lets a command through when the hook cannot start.
pub const WINDOWS_MIN_CLAUDE: [u32; 3] = [2, 1, 295];

/// Whether runs of this CLI stop at gated commands (`CliInfo.gates`): `version` is the first
/// line of its `--version`. Unix: every Claude Code. Windows: a Claude Code of at least
/// [`WINDOWS_MIN_CLAUDE`]; one that is older, or that does not say its version, is not
/// reported as gated, and the server keeps the rules in the prompt for it.
pub fn reported_for(kind: &str, version: Option<&str>) -> bool {
    enforced_by(kind) && (!cfg!(windows) || version.is_some_and(windows_can_gate))
}

/// True for the `--version` line of a Claude Code that is new enough for [`reported_for`]
/// (`2.1.322 (Claude Code)`).
fn windows_can_gate(version: &str) -> bool {
    let mut parts = version
        .split_whitespace()
        .next()
        .unwrap_or_default()
        .split('.')
        .map(|p| p.parse::<u32>().ok());
    let mut v = [0u32; 3];
    for slot in &mut v {
        match parts.next() {
            Some(Some(n)) => *slot = n,
            _ => return false,
        }
    }
    v >= WINDOWS_MIN_CLAUDE
}

/// The 设置 name of a kind, for the approval title (the server shows its own).
fn label(kind: &str) -> String {
    match kind {
        "mergeMain" => "合并到 main".into(),
        "deployProd" => "发布到生产".into(),
        "prodConfig" => "改生产环境配置".into(),
        "deleteData" => "删除数据或文件".into(),
        "externalPublish" => "对外发布".into(),
        other => other.to_string(),
    }
}

/// The English name of a kind, for what the CLI reads.
fn action(kind: &str) -> String {
    match kind {
        "mergeMain" => "merging into the main branch".into(),
        "deployProd" => "deploying to production".into(),
        "prodConfig" => "changing production configuration or secrets".into(),
        "deleteData" => "deleting data".into(),
        "externalPublish" => "publishing outside the team".into(),
        other => other.to_string(),
    }
}

/// The simple commands of a shell line: split at newlines, `;`, `&&`, `||` and `|`, with
/// runs of whitespace made one space. A pattern then cannot span two commands.
pub fn commands(line: &str) -> Vec<String> {
    line.split(['\n', ';', '|', '&'])
        .map(|c| c.split_whitespace().collect::<Vec<_>>().join(" "))
        .filter(|c| !c.is_empty())
        .collect()
}

struct Kind {
    name: String,
    patterns: Vec<Regex>,
}

#[derive(Default)]
struct State {
    /// The approval that waits: (approval id, kind, the hook's answer).
    pending: Option<(String, String, oneshot::Sender<bool>)>,
    /// Kinds a person approved for the whole run (这件事里都同意).
    approved: HashSet<String>,
    next: u64,
}

/// The gated kinds of one run.
pub struct Gates {
    kinds: Vec<Kind>,
    state: Mutex<State>,
    /// One approval at a time (the server keeps one open approval per run): a second gated
    /// command waits here until the first one is answered.
    asking: tokio::sync::Mutex<()>,
}

impl Gates {
    /// Compiles the assignment's gates. A pattern that does not compile is left out and logged;
    /// a kind left with no pattern is dropped.
    pub fn new(gates: &[RunGate]) -> Gates {
        let mut kinds = vec![];
        let mut seen = HashMap::new();
        for g in gates {
            let mut patterns = vec![];
            for p in &g.patterns {
                match RegexBuilder::new(p)
                    .case_insensitive(true)
                    .size_limit(1 << 20)
                    .build()
                {
                    Ok(r) => patterns.push(r),
                    Err(e) => eprintln!(
                        "{} gate {}: pattern left out: {}",
                        crate::proto::now_ms(),
                        crate::chat_run::safe_word(&g.kind),
                        truncate(&e.to_string(), 200)
                    ),
                }
            }
            if patterns.is_empty() {
                continue;
            }
            // The same kind twice: one entry with both lists.
            match seen.get(&g.kind) {
                Some(&i) => {
                    let k: &mut Kind = &mut kinds[i];
                    k.patterns.extend(patterns);
                }
                None => {
                    seen.insert(g.kind.clone(), kinds.len());
                    kinds.push(Kind {
                        name: g.kind.clone(),
                        patterns,
                    });
                }
            }
        }
        Gates {
            kinds,
            state: Mutex::default(),
            asking: tokio::sync::Mutex::new(()),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.kinds.is_empty()
    }

    /// The kind names, in assignment order.
    pub fn kinds(&self) -> Vec<&str> {
        self.kinds.iter().map(|k| k.name.as_str()).collect()
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// The first kind whose pattern matches one of the line's commands.
    pub fn matches(&self, line: &str) -> Option<&str> {
        let cmds = commands(line);
        self.kinds
            .iter()
            .find(|k| {
                cmds.iter()
                    .any(|c| k.patterns.iter().any(|p| p.is_match(c)))
            })
            .map(|k| k.name.as_str())
    }

    /// An approval of this module waits for an answer.
    pub fn waiting(&self) -> bool {
        self.lock().pending.is_some()
    }

    /// The approval id is one this module made.
    pub fn owns(&self, approval_id: &str) -> bool {
        approval_id.starts_with(PREFIX)
    }

    /// Answers the hook's `gate.check` (`{tool, command}`): `{ok, result: {decision, kind?}}`.
    /// A match waits for `run.approval`; `emit` sends the `approval.request`.
    pub async fn check(&self, body: &Value, emit: &(dyn Fn(RunEvent) + Send + Sync)) -> Value {
        let Some(command) = body["command"].as_str() else {
            return json!({ "ok": false, "error": { "code": "invalid", "message": "command is required" } });
        };
        let Some(kind) = self.matches(command).map(str::to_string) else {
            return allow(None);
        };
        // One question at a time; a 这件事里都同意 given meanwhile lets this one pass.
        let _turn = self.asking.lock().await;
        let rx = {
            let mut st = self.lock();
            if st.approved.contains(&kind) {
                return allow(Some(&kind));
            }
            st.next += 1;
            let id = format!("{PREFIX}{}", st.next);
            let (tx, rx) = oneshot::channel();
            st.pending = Some((id.clone(), kind.clone(), tx));
            drop(st);
            let cmd = truncate(command, MAX_COMMAND - 100);
            emit(RunEvent::ApprovalRequest {
                approval_id: id,
                item_id: None,
                title: label(&kind),
                detail: Some(cmd.clone()),
                kind: Some(kind.clone()),
                command: Some(cmd),
            });
            rx
        };
        match rx.await {
            Ok(true) => allow(Some(&kind)),
            Ok(false) => deny(&kind, "a person declined it"),
            Err(_) => deny(&kind, "the run ended before a person answered"),
        }
    }

    /// A `run.approval` for this module: wakes the waiting hook and returns `approval.resolved`.
    /// `all` (scope "run") lets later matches of the same kind pass. None when it is not the
    /// approval that waits.
    pub fn answer(&self, approval_id: &str, approve: bool, all: bool) -> Option<RunEvent> {
        if !self.owns(approval_id) {
            return None;
        }
        let mut st = self.lock();
        if st.pending.as_ref().map(|p| p.0.as_str()) != Some(approval_id) {
            return None;
        }
        let (_, kind, tx) = st.pending.take()?;
        if approve && all {
            st.approved.insert(kind);
        }
        drop(st);
        let _ = tx.send(approve);
        Some(RunEvent::ApprovalResolved {
            approval_id: approval_id.to_string(),
            decision: if approve { "approve" } else { "deny" }.into(),
        })
    }

    /// The line the CLI gets in its system prompt when the run has gates.
    pub fn note(&self) -> Option<String> {
        (!self.is_empty()).then(|| {
            let what: Vec<String> = self.kinds.iter().map(|k| action(&k.name)).collect();
            format!(
                "Team rule: commands for {} need a person's approval. When you run one, the \
machine asks the approver and the command waits for the answer; do not ask for it in the topic \
yourself. If a person declines, the command does not run and you are told so: do not run it \
again or work around it, do the rest another way or say in your answer what is left for a person.",
                what.join(", ")
            )
        })
    }
}

fn allow(kind: Option<&str>) -> Value {
    let mut r = json!({ "decision": "allow" });
    if let Some(k) = kind {
        r["kind"] = json!(k);
    }
    json!({ "ok": true, "result": r })
}

fn deny(kind: &str, why: &str) -> Value {
    json!({ "ok": true, "result": { "decision": "deny", "kind": kind, "reason": format!(
        "This command needs a person's approval ({}) and {why}. Do not run it again or work \
around it; do the rest another way, or say in your answer what is left for a person.",
        action(kind)
    ) } })
}

/// The Claude Code tools that run a shell line (their input has it as `command`). `PowerShell`
/// is the shell tool on Windows without Git Bash (and anywhere it is turned on); `Monitor`
/// runs a command and watches its output.
pub const SHELL_TOOLS: &[&str] = &["Bash", "PowerShell", "Monitor"];

/// What the hook checks: the command of a shell tool, or an MCP tool call as
/// `<tool> <input JSON>`. None for other tools (they pass).
pub fn hook_command(input: &Value) -> Option<String> {
    let tool = input["tool_name"].as_str()?;
    if SHELL_TOOLS.contains(&tool) {
        return input["tool_input"]["command"].as_str().map(str::to_string);
    }
    tool.starts_with("mcp__")
        .then(|| format!("{tool} {}", input["tool_input"]))
}

/// The hook's output for a `gate.check` answer: the PreToolUse deny decision, or None (no
/// output: the tool runs as it would without the hook).
pub fn hook_output(answer: &Value) -> Option<Value> {
    let r = &answer["result"];
    (answer["ok"] == json!(true) && r["decision"] == "deny").then(|| {
        json!({ "hookSpecificOutput": {
            "hookEventName": "PreToolUse",
            "permissionDecision": "deny",
            "permissionDecisionReason": r["reason"].as_str().unwrap_or("a person declined this command"),
        } })
    })
}

/// What the hook prints when it could not ask the daemon (`why`). Open (Unix): nothing, so
/// the tool runs; the run would have no way to go on otherwise. Closed (Windows): the deny
/// decision, so a command nobody could be asked about does not run.
pub fn hook_failure(why: &str, closed: bool) -> Option<Value> {
    closed.then(|| {
        json!({ "hookSpecificOutput": {
            "hookEventName": "PreToolUse",
            "permissionDecision": "deny",
            "permissionDecisionReason":
                format!("this command needs an approval and the daemon could not be asked: {why}"),
        } })
    })
}

/// Whether the hook refuses a command when it cannot ask the daemon. Windows: yes. The pipe
/// and the way the hook is started are new there; a failure must not let a gated command
/// through. Unix keeps what it has always done.
fn fails_closed() -> bool {
    cfg!(windows)
}

/// The hook's answer to one hook input, as (standard output, standard error): asks the daemon
/// through `ask` (None: not inside a run).
fn hook_answer(
    input: &[u8],
    closed: bool,
    ask: Option<&dyn Fn(Value) -> Result<Value, String>>,
) -> (Option<Value>, Option<String>) {
    let failed = |why: String| (hook_failure(&why, closed), Some(why));
    let input: Value = match serde_json::from_slice(input) {
        Ok(v) => v,
        Err(e) => return failed(format!("the hook input is not JSON: {e}")),
    };
    let Some(command) = hook_command(&input) else {
        return (None, None);
    };
    let Some(ask) = ask else {
        return failed("not inside a run".into());
    };
    let tool = input["tool_name"].as_str().unwrap_or_default();
    match ask(json!({ "tool": tool, "command": command })) {
        Ok(answer) => match hook_output(&answer) {
            Some(out) => (Some(out), None),
            None if answer["ok"] != json!(true) => failed(answer["error"].to_string()),
            None => (None, None),
        },
        Err(e) => failed(e),
    }
}

/// `chat-run gate`: Claude Code's PreToolUse hook. Reads the hook input on standard input, asks
/// the daemon, prints the deny decision when a person declined, and exits 0. When the daemon
/// cannot be asked, the reason goes to standard error and, on Unix, the tool runs (the run
/// would have no way to go on otherwise); on Windows the command is refused
/// ([`hook_failure`]).
pub fn hook_main() -> i32 {
    let closed = fails_closed();
    let mut buf = Vec::new();
    if let Err(e) = std::io::stdin().take(MAX_HOOK_INPUT).read_to_end(&mut buf) {
        let why = format!("cannot read the hook input: {e}");
        if let Some(out) = hook_failure(&why, closed) {
            println!("{out}");
        }
        eprintln!("chat-run gate: {why}");
        return 0;
    }
    let socket = std::env::var_os(crate::chat_run::ENV_SOCKET).filter(|s| !s.is_empty());
    let call = |body: Value| {
        let socket = socket.as_deref().unwrap_or_default();
        crate::chat_run::call(std::path::Path::new(socket), OP, body, HOOK_WAIT)
    };
    let ask: Option<&dyn Fn(Value) -> Result<Value, String>> =
        if socket.is_some() { Some(&call) } else { None };
    let (out, err) = hook_answer(&buf, closed, ask);
    if let Some(out) = out {
        println!("{out}");
    }
    if let Some(err) = err {
        eprintln!("chat-run gate: {err}");
    }
    0
}

/// The run's Claude Code settings that install the hook (`--settings`): `chat_run` is the
/// run's `chat-run` link.
///
/// Unix: the hook is a shell line, `'<link>' gate`.
///
/// Windows: Claude Code runs a shell line in Git Bash when it finds one and in PowerShell
/// otherwise, and no spelling of a quoted path with an argument is a command in both. So the
/// hook is in the exec form: the program and its arguments, started without a shell
/// (`args`). And `onFailure: "block"`: a hook that cannot be started, times out or ends with
/// an unexpected code blocks the command instead of letting it run. With the hook's own
/// refusal when it cannot ask the daemon ([`hook_failure`]), no failure lets a gated command
/// through. Both need a Claude Code of [`WINDOWS_MIN_CLAUDE`] ([`reported_for`]).
pub fn claude_settings(chat_run: &std::path::Path) -> Value {
    claude_settings_as(chat_run, cfg!(windows))
}

/// [`claude_settings`] in the form of one system: `exec_form` is the Windows one (the tests
/// put it before the real CLI on every system).
pub fn claude_settings_as(chat_run: &std::path::Path, exec_form: bool) -> Value {
    let path = chat_run.display().to_string();
    let hook = if exec_form {
        json!({
            "type": "command",
            "command": path.replace('\\', "/"),
            "args": ["gate"],
            "onFailure": "block",
            "timeout": HOOK_WAIT.as_secs(),
        })
    } else {
        json!({
            "type": "command",
            "command": format!("{} gate", shell_quote(&path)),
            "timeout": HOOK_WAIT.as_secs(),
        })
    };
    json!({ "hooks": { "PreToolUse": [{ "matcher": matcher(), "hooks": [hook] }] } })
}

/// The tools the hook runs before: the shell tools and every MCP tool.
fn matcher() -> String {
    format!("^({}|mcp__.*)$", SHELL_TOOLS.join("|"))
}

/// One shell word.
fn shell_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', r"'\''"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    /// The server's default patterns (packages/protocol `DEFAULT_GATE_PATTERNS`), read from the
    /// protocol package so the daemon's regex engine is checked against the real list.
    fn defaults() -> Vec<RunGate> {
        let src = include_str!("../../../packages/protocol/src/settings.ts");
        let start = src
            .find("export const DEFAULT_GATE_PATTERNS")
            .expect("defaults");
        let body = &src[start..];
        let body = &body[..body.find("\n};").expect("end")];
        let mut out: Vec<RunGate> = vec![];
        for line in body.lines().skip(1) {
            let t = line.trim();
            if let Some(kind) = t.strip_suffix(": [") {
                out.push(RunGate {
                    kind: kind.into(),
                    patterns: vec![],
                });
            } else if t.starts_with('"') {
                let lit = t.trim_end_matches(',');
                let p: String = serde_json::from_str(lit).expect("a JS string literal");
                out.last_mut().expect("kind").patterns.push(p);
            }
        }
        assert_eq!(out.len(), 5, "{out:?}");
        out
    }

    #[test]
    fn default_patterns_compile_and_match_only_what_they_name() {
        let gates = Gates::new(&defaults());
        assert_eq!(
            gates.kinds(),
            [
                "mergeMain",
                "deployProd",
                "prodConfig",
                "deleteData",
                "externalPublish"
            ]
        );
        let hits = [
            ("gh pr merge 12 --squash", "mergeMain"),
            ("cd repo && gh pr merge 12", "mergeMain"),
            ("git push origin main", "mergeMain"),
            ("git push origin HEAD:main", "mergeMain"),
            ("git push -u origin feature:refs/heads/master", "mergeMain"),
            (
                "bunx wrangler deploy -c wrangler.deploy.jsonc --env prod",
                "deployProd",
            ),
            ("wrangler deploy --env=production", "deployProd"),
            ("vercel --prod", "deployProd"),
            ("flyctl deploy", "deployProd"),
            ("bun run deploy:prod", "deployProd"),
            (
                "echo $T | wrangler secret put API_KEY --env prod",
                "prodConfig",
            ),
            ("fly secrets set A=1", "prodConfig"),
            ("rm -rf /", "deleteData"),
            ("rm -rf ~/", "deleteData"),
            ("rm -fr $HOME", "deleteData"),
            (
                "wrangler d1 execute db --remote --command 'DROP TABLE users'",
                "deleteData",
            ),
            (
                "wrangler d1 execute db --command \"delete from t\" --remote",
                "deleteData",
            ),
            ("psql $URL -c 'truncate table events'", "deleteData"),
            ("wrangler r2 bucket delete assets", "deleteData"),
            ("gh repo delete org/x --yes", "deleteData"),
            ("npm publish --access public", "externalPublish"),
            ("cargo publish -p chat-protocol", "externalPublish"),
            ("gh release create v1.2.3", "externalPublish"),
            ("GH  PR   MERGE 3", "mergeMain"),
        ];
        for (cmd, want) in hits {
            assert_eq!(gates.matches(cmd), Some(want), "{cmd}");
        }
        let misses = [
            "echo hi",
            "gh pr view 12",
            "gh pr create --base main",
            "git push origin main-fix",
            "git push -u origin claude/m4",
            "git push --force-with-lease origin claude/m4",
            "git checkout main",
            "git merge main",
            "wrangler deploy --env staging",
            "wrangler dev",
            "rm -rf build",
            "rm -rf ./target /tmp/x",
            "grep -rn 'DROP TABLE' server/migrations",
            "wrangler d1 execute db --local --command 'delete from t'",
            "npm install",
            "cargo build --release",
            "git log --oneline main",
            "git push origin main:backup-main",
        ];
        for cmd in misses {
            assert_eq!(gates.matches(cmd), None, "{cmd}");
        }
    }

    #[test]
    fn commands_split_at_separators() {
        assert_eq!(
            commands("cd a &&  git status\n echo  x | wc -l; ls || true"),
            ["cd a", "git status", "echo x", "wc -l", "ls", "true"]
        );
    }

    #[test]
    fn bad_patterns_are_left_out_and_kinds_merge() {
        let gates = Gates::new(&[
            RunGate {
                kind: "deployProd".into(),
                patterns: vec!["(unclosed".into(), r"\bdeploy\.sh\b".into()],
            },
            RunGate {
                kind: "x".into(),
                patterns: vec!["(".into()],
            },
            RunGate {
                kind: "deployProd".into(),
                patterns: vec![r"\bship-it\b".into()],
            },
        ]);
        assert_eq!(gates.kinds(), ["deployProd"]);
        assert_eq!(gates.matches("./deploy.sh"), Some("deployProd"));
        assert_eq!(gates.matches("ship-it now"), Some("deployProd"));
        assert!(Gates::new(&[]).is_empty());
        assert!(Gates::new(&[]).note().is_none());
    }

    #[test]
    fn hook_input_and_output() {
        let bash = json!({ "hook_event_name": "PreToolUse", "tool_name": "Bash", "tool_input": { "command": "gh pr merge 1" } });
        assert_eq!(hook_command(&bash).as_deref(), Some("gh pr merge 1"));
        let mcp = json!({ "tool_name": "mcp__github__merge_pull_request", "tool_input": { "pull_number": 1 } });
        assert_eq!(
            hook_command(&mcp).as_deref(),
            Some(r#"mcp__github__merge_pull_request {"pull_number":1}"#)
        );
        assert!(hook_command(&json!({ "tool_name": "Edit", "tool_input": {} })).is_none());
        // The other tools that run a shell line are checked like Bash.
        for tool in ["PowerShell", "Monitor"] {
            let call = json!({ "tool_name": tool, "tool_input": { "command": "gh pr merge 1", "description": "x" } });
            assert_eq!(
                hook_command(&call).as_deref(),
                Some("gh pr merge 1"),
                "{tool}"
            );
        }

        assert!(hook_output(&allow(Some("mergeMain"))).is_none());
        let out = hook_output(&deny("mergeMain", "a person declined it")).unwrap();
        assert_eq!(out["hookSpecificOutput"]["permissionDecision"], "deny");
        assert_eq!(out["hookSpecificOutput"]["hookEventName"], "PreToolUse");
        let why = out["hookSpecificOutput"]["permissionDecisionReason"]
            .as_str()
            .unwrap();
        assert!(why.contains("merging into the main branch") && why.contains("declined"));

        let s = claude_settings(std::path::Path::new("/tmp/run dir/bin/chat-run"));
        let h = &s["hooks"]["PreToolUse"][0];
        assert_eq!(h["matcher"], "^(Bash|PowerShell|Monitor|mcp__.*)$");
        let m = regex::Regex::new(h["matcher"].as_str().unwrap()).unwrap();
        for tool in ["Bash", "PowerShell", "Monitor", "mcp__github__merge"] {
            assert!(m.is_match(tool), "{tool}");
        }
        for tool in ["Read", "Edit", "BashOutput", "Agent"] {
            assert!(!m.is_match(tool), "{tool}");
        }
        assert_eq!(h["hooks"][0]["timeout"], HOOK_WAIT.as_secs());
        let path = std::path::Path::new(r"C:\Users\me\run dir\bin\chat-run.exe");
        // Windows. No shell: the program and its argument, and a failure blocks.
        let hook = claude_settings_as(path, true)["hooks"]["PreToolUse"][0]["hooks"][0].clone();
        assert_eq!(hook["command"], "C:/Users/me/run dir/bin/chat-run.exe");
        assert_eq!(hook["args"], json!(["gate"]));
        assert_eq!(hook["onFailure"], "block");
        // Unix: one shell line, as it has always been.
        let path = std::path::Path::new("/tmp/run dir/bin/chat-run");
        let hook = claude_settings_as(path, false)["hooks"]["PreToolUse"][0]["hooks"][0].clone();
        assert_eq!(hook["command"], "'/tmp/run dir/bin/chat-run' gate");
        assert!(hook.get("args").is_none() && hook.get("onFailure").is_none());
        assert_eq!(
            claude_settings(path),
            claude_settings_as(path, cfg!(windows))
        );
    }

    /// Windows reports a Claude Code as gated only from the version whose hooks can block on
    /// failure; Unix reports every Claude Code, and no other runtime anywhere.
    #[test]
    fn gates_are_reported_for_a_claude_code_that_can_block() {
        assert!(windows_can_gate("2.1.295 (Claude Code)"));
        assert!(windows_can_gate("2.1.322 (Claude Code)"));
        assert!(windows_can_gate("2.2.0"));
        assert!(windows_can_gate("3.0.1 (Claude Code)"));
        for old in [
            "2.1.294 (Claude Code)",
            "2.0.999",
            "1.9.400",
            "",
            "found",
            "2.1",
            "v2.1.300",
        ] {
            assert!(!windows_can_gate(old), "{old}");
        }
        assert!(reported_for("claude", Some("2.1.322 (Claude Code)")));
        assert_eq!(
            reported_for("claude", Some("2.1.100 (Claude Code)")),
            cfg!(unix)
        );
        assert_eq!(reported_for("claude", None), cfg!(unix));
        assert!(!reported_for("codex", Some("2.1.322")));
        assert!(enforced_by("claude") && !enforced_by("codex") && !enforced_by("pty"));
    }

    /// The hook when the daemon cannot be asked: open (Unix) prints nothing and the tool runs;
    /// closed (Windows) prints the deny decision. Either way the reason goes to standard error.
    #[test]
    fn the_hook_fails_open_on_unix_and_closed_on_windows() {
        let bash = br#"{"tool_name":"Bash","tool_input":{"command":"gh pr merge 12"}}"#;
        let down = |_: Value| Err::<Value, String>("cannot reach the daemon".into());
        let refused = |_: Value| {
            Ok::<Value, String>(json!({ "ok": false, "error": { "code": "unavailable" } }))
        };
        let allow = |_: Value| {
            Ok::<Value, String>(json!({ "ok": true, "result": { "decision": "allow" } }))
        };
        let deny = |_: Value| {
            Ok::<Value, String>(
                json!({ "ok": true, "result": { "decision": "deny", "reason": "no" } }),
            )
        };
        let denied = |out: &Option<Value>| {
            out.as_ref()
                .is_some_and(|o| o["hookSpecificOutput"]["permissionDecision"] == "deny")
        };
        for closed in [false, true] {
            // The daemon answered: the same on both.
            assert_eq!(hook_answer(bash, closed, Some(&allow)), (None, None));
            let (out, err) = hook_answer(bash, closed, Some(&deny));
            assert!(denied(&out) && err.is_none());
            // Not a command tool: nothing to ask.
            let read = br#"{"tool_name":"Read","tool_input":{"file_path":"a"}}"#;
            assert_eq!(hook_answer(read, closed, None), (None, None));
            // Every way of not getting an answer.
            let failures = [
                hook_answer(bash, closed, None),
                hook_answer(bash, closed, Some(&down)),
                hook_answer(bash, closed, Some(&refused)),
                hook_answer(b"not json", closed, Some(&allow)),
            ];
            for (out, err) in failures {
                assert!(err.is_some(), "the reason is reported");
                assert_eq!(denied(&out), closed, "{out:?}");
                if !closed {
                    assert_eq!(out, None, "Unix prints nothing: the tool runs");
                }
            }
        }
        assert_eq!(fails_closed(), cfg!(windows));
    }

    fn emitted() -> (Arc<Mutex<Vec<RunEvent>>>, impl Fn(RunEvent) + Send + Sync) {
        let seen = Arc::new(Mutex::new(vec![]));
        let s = seen.clone();
        (seen, move |e| s.lock().unwrap().push(e))
    }

    async fn requested(seen: &Arc<Mutex<Vec<RunEvent>>>, n: usize) -> String {
        for _ in 0..200 {
            if let Some(RunEvent::ApprovalRequest { approval_id, .. }) = seen.lock().unwrap().get(n)
            {
                return approval_id.clone();
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
        panic!("no approval.request #{n}");
    }

    #[tokio::test]
    async fn a_match_waits_for_the_answer_and_scope_run_passes_later_ones() {
        let gates = Arc::new(Gates::new(&defaults()));
        let (seen, emit) = emitted();
        let emit = Arc::new(emit);

        // Other commands pass at once and ask nobody.
        let a = gates
            .check(&json!({ "command": "ls -la" }), emit.as_ref())
            .await;
        assert_eq!(a["result"]["decision"], "allow");
        assert!(seen.lock().unwrap().is_empty());

        // A match waits; deny blocks with a reason.
        let (g, e) = (gates.clone(), emit.clone());
        let t = tokio::spawn(async move {
            g.check(&json!({ "command": "gh pr merge 12" }), e.as_ref())
                .await
        });
        let id = requested(&seen, 0).await;
        assert!(gates.waiting());
        match &seen.lock().unwrap()[0] {
            RunEvent::ApprovalRequest {
                kind,
                command,
                title,
                ..
            } => {
                assert_eq!(kind.as_deref(), Some("mergeMain"));
                assert_eq!(command.as_deref(), Some("gh pr merge 12"));
                assert_eq!(title, "合并到 main");
            }
            e => panic!("{e:?}"),
        }
        assert!(gates.answer("other", true, false).is_none());
        let resolved = gates.answer(&id, false, false).unwrap();
        assert!(
            matches!(resolved, RunEvent::ApprovalResolved { ref decision, .. } if decision == "deny")
        );
        let a = t.await.unwrap();
        assert_eq!(a["result"]["decision"], "deny");
        assert!(!gates.waiting());

        // Approve once: the next match asks again.
        let (g, e) = (gates.clone(), emit.clone());
        let t = tokio::spawn(async move {
            g.check(&json!({ "command": "gh pr merge 13" }), e.as_ref())
                .await
        });
        let id = requested(&seen, 1).await;
        gates.answer(&id, true, false).unwrap();
        assert_eq!(t.await.unwrap()["result"]["decision"], "allow");

        // 这件事里都同意: later merges pass without asking; other kinds still ask.
        let (g, e) = (gates.clone(), emit.clone());
        let t = tokio::spawn(async move {
            g.check(&json!({ "command": "gh pr merge 14" }), e.as_ref())
                .await
        });
        let id = requested(&seen, 2).await;
        gates.answer(&id, true, true).unwrap();
        assert_eq!(t.await.unwrap()["result"]["decision"], "allow");
        let a = gates
            .check(&json!({ "command": "git push origin main" }), emit.as_ref())
            .await;
        assert_eq!(a["result"]["decision"], "allow");
        assert_eq!(seen.lock().unwrap().len(), 3);
        let (g, e) = (gates.clone(), emit.clone());
        let t = tokio::spawn(async move {
            g.check(&json!({ "command": "npm publish" }), e.as_ref())
                .await
        });
        let id = requested(&seen, 3).await;
        assert!(id.starts_with(PREFIX));
        drop(gates.answer(&id, true, false));
        t.await.unwrap();
    }

    #[tokio::test]
    async fn a_second_match_waits_for_the_first_answer() {
        let gates = Arc::new(Gates::new(&defaults()));
        let (seen, emit) = emitted();
        let emit = Arc::new(emit);
        let (g, e) = (gates.clone(), emit.clone());
        let first = tokio::spawn(async move {
            g.check(&json!({ "command": "gh pr merge 1" }), e.as_ref())
                .await
        });
        let id = requested(&seen, 0).await;
        let (g, e) = (gates.clone(), emit.clone());
        let second = tokio::spawn(async move {
            g.check(&json!({ "command": "gh pr merge 2" }), e.as_ref())
                .await
        });
        tokio::time::sleep(Duration::from_millis(50)).await;
        // Only one question is open.
        assert_eq!(seen.lock().unwrap().len(), 1);
        gates.answer(&id, true, true).unwrap();
        assert_eq!(first.await.unwrap()["result"]["decision"], "allow");
        // The 这件事里都同意 covers the one that waited.
        assert_eq!(second.await.unwrap()["result"]["decision"], "allow");
        assert_eq!(seen.lock().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn the_run_ending_denies_the_waiting_command() {
        let gates = Arc::new(Gates::new(&defaults()));
        let (seen, emit) = emitted();
        let emit = Arc::new(emit);
        let (g, e) = (gates.clone(), emit.clone());
        let t = tokio::spawn(async move {
            g.check(&json!({ "command": "npm publish" }), e.as_ref())
                .await
        });
        requested(&seen, 0).await;
        // The run's state goes away (the sender is dropped).
        gates.lock().pending.take();
        let a = t.await.unwrap();
        assert_eq!(a["result"]["decision"], "deny");
        assert!(
            a["result"]["reason"]
                .as_str()
                .unwrap()
                .contains("run ended")
        );
    }
}
