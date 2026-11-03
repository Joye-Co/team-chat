//! Codex app-server: JSON-RPC over stdio (`codex app-server`, protocol v2).
//!
//! Flow: `initialize` -> `initialized` -> `thread/start` -> `turn/start`; notifications
//! `turn/*` and `item/*` stream the work; follow-ups use `turn/steer` while a turn runs and a
//! new `turn/start` after it; `turn/completed` with no queued input ends the run.

use super::*;
use crate::proto::{FileChange, RunEvent, Usage, truncate};
use serde_json::{Value, json};
use std::collections::{HashMap, VecDeque};

pub struct CodexRuntime;

/// Turns app-server notifications into run events.
#[derive(Default)]
pub struct CodexNormalizer {
    pub last_message: Option<String>,
    pub last_turn_status: Option<String>,
    pub last_error: Option<String>,
    pub usage: Option<Usage>,
    plan: PlanTracker,
}

fn s(v: &Value, k: &str) -> Option<String> {
    v.get(k).and_then(Value::as_str).map(str::to_string)
}

impl CodexNormalizer {
    pub fn on_notification(&mut self, method: &str, p: &Value) -> Vec<RunEvent> {
        let turn_id = s(p, "turnId");
        match method {
            "turn/started" => {
                let id = p["turn"]["id"].as_str().unwrap_or_default().to_string();
                vec![RunEvent::TurnStarted { turn_id: id }]
            }
            "turn/completed" => {
                let t = &p["turn"];
                let status = match t["status"].as_str() {
                    Some("completed") => "completed",
                    Some("interrupted") => "interrupted",
                    _ => "failed",
                };
                let error = t["error"]["message"].as_str().map(str::to_string);
                self.last_turn_status = Some(status.to_string());
                if error.is_some() {
                    self.last_error = error.clone();
                }
                vec![RunEvent::TurnCompleted {
                    turn_id: t["id"].as_str().unwrap_or_default().to_string(),
                    status: status.into(),
                    error,
                }]
            }
            // The plan tool (`update_plan`): the whole list, `plan[]` with `step` and `status`.
            "turn/plan/updated" => {
                let Some(steps) = p["plan"].as_array() else {
                    return vec![];
                };
                self.plan
                    .update(steps.iter().map(|x| {
                        (
                            x["step"].as_str().unwrap_or_default(),
                            x["status"].as_str().unwrap_or_default(),
                        )
                    }))
                    .into_iter()
                    .collect()
            }
            "item/agentMessage/delta" => vec![RunEvent::MessageDelta {
                item_id: s(p, "itemId").unwrap_or_default(),
                turn_id,
                text: s(p, "delta").unwrap_or_default(),
            }],
            "item/started" => self.item_started(&p["item"], turn_id),
            "item/completed" => self.item_completed(&p["item"], turn_id),
            "thread/tokenUsage/updated" => {
                let t = &p["tokenUsage"]["total"];
                self.usage = Some(Usage {
                    input_tokens: t["inputTokens"].as_u64().unwrap_or(0),
                    output_tokens: t["outputTokens"].as_u64().unwrap_or(0),
                    cost_usd: None,
                });
                vec![]
            }
            "error" => {
                let msg = p["error"]["message"]
                    .as_str()
                    .unwrap_or("error")
                    .to_string();
                if p["willRetry"].as_bool() == Some(true) {
                    vec![RunEvent::Status {
                        state: "retrying".into(),
                        message: Some(msg),
                    }]
                } else {
                    self.last_error = Some(msg.clone());
                    vec![RunEvent::Status {
                        state: "error".into(),
                        message: Some(msg),
                    }]
                }
            }
            _ => vec![],
        }
    }

    fn item_started(&mut self, item: &Value, turn_id: Option<String>) -> Vec<RunEvent> {
        let id = s(item, "id").unwrap_or_default();
        match item["type"].as_str().unwrap_or_default() {
            "commandExecution" => vec![RunEvent::ToolCall {
                item_id: id,
                turn_id,
                tool: "shell".into(),
                title: s(item, "command").unwrap_or_default(),
                input: Some(json!({ "command": item["command"], "cwd": item["cwd"] })),
            }],
            "mcpToolCall" => vec![RunEvent::ToolCall {
                item_id: id,
                turn_id,
                tool: format!(
                    "{}/{}",
                    item["server"].as_str().unwrap_or("mcp"),
                    item["tool"].as_str().unwrap_or("tool")
                ),
                title: item["tool"].as_str().unwrap_or("tool").to_string(),
                input: Some(item["arguments"].clone()),
            }],
            "dynamicToolCall" => vec![RunEvent::ToolCall {
                item_id: id,
                turn_id,
                tool: item["tool"].as_str().unwrap_or("tool").to_string(),
                title: item["tool"].as_str().unwrap_or("tool").to_string(),
                input: Some(item["arguments"].clone()),
            }],
            "webSearch" => vec![RunEvent::ToolCall {
                item_id: id,
                turn_id,
                tool: "webSearch".into(),
                title: s(item, "query").unwrap_or_else(|| "web search".into()),
                input: None,
            }],
            _ => vec![],
        }
    }

    fn item_completed(&mut self, item: &Value, turn_id: Option<String>) -> Vec<RunEvent> {
        let id = s(item, "id").unwrap_or_default();
        match item["type"].as_str().unwrap_or_default() {
            "agentMessage" => {
                let text = s(item, "text").unwrap_or_default();
                self.last_message = Some(text.clone());
                vec![RunEvent::Message {
                    item_id: id,
                    turn_id,
                    role: "assistant".into(),
                    text,
                }]
            }
            "userMessage" => {
                let text = item["content"]
                    .as_array()
                    .map(|c| {
                        c.iter()
                            .filter_map(|i| i["text"].as_str())
                            .collect::<Vec<_>>()
                            .join("\n")
                    })
                    .unwrap_or_default();
                vec![RunEvent::Message {
                    item_id: id,
                    turn_id,
                    role: "user".into(),
                    text,
                }]
            }
            "reasoning" => {
                let parts = item["summary"]
                    .as_array()
                    .filter(|a| !a.is_empty())
                    .or(item["content"].as_array());
                let text = parts
                    .map(|a| {
                        a.iter()
                            .filter_map(Value::as_str)
                            .collect::<Vec<_>>()
                            .join("\n")
                    })
                    .unwrap_or_default();
                if text.is_empty() {
                    return vec![];
                }
                vec![RunEvent::Reasoning {
                    item_id: id,
                    turn_id,
                    text,
                }]
            }
            "commandExecution" => {
                let exit = item["exitCode"].as_i64();
                vec![RunEvent::ToolResult {
                    item_id: id,
                    turn_id,
                    ok: exit == Some(0) && item["status"].as_str() != Some("failed"),
                    output: item["aggregatedOutput"]
                        .as_str()
                        .map(|o| truncate(o, OUTPUT_LIMIT)),
                    exit_code: exit,
                }]
            }
            "mcpToolCall" | "dynamicToolCall" => {
                let err = item["error"]["message"].as_str();
                let ok = err.is_none() && item["success"].as_bool() != Some(false);
                let out = match err {
                    Some(e) => e.to_string(),
                    None if !item["result"].is_null() => value_text(&item["result"]["content"]),
                    None => value_text(&item["contentItems"]),
                };
                vec![RunEvent::ToolResult {
                    item_id: id,
                    turn_id,
                    ok,
                    output: Some(truncate(&out, OUTPUT_LIMIT)),
                    exit_code: None,
                }]
            }
            "webSearch" => vec![RunEvent::ToolResult {
                item_id: id,
                turn_id,
                ok: true,
                output: None,
                exit_code: None,
            }],
            "fileChange" => {
                let changes = item["changes"]
                    .as_array()
                    .map(|a| {
                        a.iter()
                            .map(|c| FileChange {
                                path: s(c, "path").unwrap_or_default(),
                                kind: match c["kind"]["type"].as_str() {
                                    Some("add") => "add",
                                    Some("delete") => "delete",
                                    _ => "modify",
                                }
                                .into(),
                                diff: c["diff"].as_str().map(|d| truncate(d, OUTPUT_LIMIT)),
                            })
                            .collect()
                    })
                    .unwrap_or_default();
                vec![RunEvent::FileChange {
                    item_id: id,
                    turn_id,
                    changes,
                }]
            }
            _ => vec![],
        }
    }
}

/// How long to wait, after a turn completed, for the answer to a steer sent as it ended.
const STEER_WAIT: Duration = Duration::from_secs(10);

#[derive(Debug)]
enum Pending {
    Initialize,
    ThreadStart,
    TurnStart,
    Steer { text: String },
    Interrupt,
}

impl Runtime for CodexRuntime {
    fn start(
        &self,
        spec: RunSpec,
        emit: Emit,
        control: mpsc::UnboundedReceiver<Control>,
    ) -> BoxFuture<Outcome> {
        Box::pin(run(spec, emit, control))
    }
}

fn text_input(text: &str) -> Value {
    json!([{ "type": "text", "text": text, "text_elements": [] }])
}

async fn run(spec: RunSpec, emit: Emit, mut control: mpsc::UnboundedReceiver<Control>) -> Outcome {
    let program = spec.cfg.command();
    let args = match app_server_args(&spec) {
        Ok(a) => a,
        Err(e) => return Outcome::failed(e),
    };
    let mut p = match Proc::spawn(&spec, &program, &args) {
        Ok(p) => p,
        Err(e) => return Outcome::failed(format!("cannot start {program}: {e}")),
    };
    let mut ids = RpcIds::default();
    let mut pending: HashMap<u64, Pending> = HashMap::new();
    let mut norm = CodexNormalizer::default();
    let mut thread_id: Option<String> = None;
    let mut session_file: Option<PathBuf> = None;
    let mut turn_running = false;
    let mut active_turn: Option<String> = None;
    let mut queued: VecDeque<String> = VecDeque::new();
    // approval id -> (json-rpc id of the server request)
    let mut approvals: HashMap<String, Value> = HashMap::new();
    let mut approval_n = 0u32;
    let mut control_open = true;
    // Set when a turn completed: the run ends (or the next turn starts) once no steer is
    // unanswered, or at this deadline.
    let mut ending: Option<tokio::time::Instant> = None;

    let id = ids.next_id();
    pending.insert(id, Pending::Initialize);
    let init = json!({ "id": id, "method": "initialize", "params": {
        "clientInfo": { "name": "machine-daemon", "title": null, "version": env!("CARGO_PKG_VERSION") },
        "capabilities": null
    }});
    if let Err(e) = p.write_json(&init).await {
        return Outcome::failed(format!("codex app-server: {e}"));
    }

    let outcome_status;
    loop {
        // The last turn ended and no steer is unanswered: start the next turn or end the run.
        if ending.is_some() && !pending.values().any(|k| matches!(k, Pending::Steer { .. })) {
            ending = None;
            take_late_input(&mut control, &mut queued, &emit);
            if !queued.is_empty() {
                let text = queued.drain(..).collect::<Vec<_>>().join("\n\n");
                let id = ids.next_id();
                pending.insert(id, Pending::TurnStart);
                turn_running = true;
                let _ = p.write_json(&json!({ "id": id, "method": "turn/start", "params": {
                    "threadId": thread_id.clone().unwrap_or_default(), "input": text_input(&text) } })).await;
            } else {
                outcome_status = if norm.last_turn_status.as_deref() == Some("completed") {
                    Status::Succeeded
                } else {
                    Status::Failed
                };
                break;
            }
        }
        tokio::select! {
            line = p.next_line() => {
                let Some(line) = line else {
                    let code = p.finish(Duration::from_secs(5)).await;
                    let mut o = Outcome::failed(format!("codex app-server exited: {}", p.stderr_tail()));
                    o.exit_code = code;
                    o.session_id = thread_id;
                    o.session_file = session_file;
                    o.result_text = norm.last_message;
                    o.usage = norm.usage;
                    return o;
                };
                let Ok(v) = serde_json::from_str::<Value>(&line) else { continue };
                match classify(&v) {
                    Rpc::Response { id, result, error } => {
                        let Some(kind) = id.as_u64().and_then(|i| pending.remove(&i)) else { continue };
                        if let Some(e) = error {
                            let msg = rpc_error_text(e);
                            match kind {
                                Pending::Steer { text } => queued.push_back(text),
                                Pending::Interrupt => {}
                                _ => {
                                    p.kill_group(Duration::from_secs(3)).await;
                                    let mut o = Outcome::failed(format!("codex {kind:?}: {msg}"));
                                    o.session_id = thread_id;
                                    o.session_file = session_file;
                                    return o;
                                }
                            }
                            continue;
                        }
                        let r = result.cloned().unwrap_or(Value::Null);
                        match kind {
                            Pending::Initialize => {
                                let _ = p.write_json(&json!({ "method": "initialized" })).await;
                                let id = ids.next_id();
                                pending.insert(id, Pending::ThreadStart);
                                let (approval, sandbox) = thread_policy(&spec);
                                let mut params = json!({
                                    "cwd": spec.cwd.to_string_lossy(),
                                    "approvalPolicy": approval,
                                    "sandbox": sandbox,
                                });
                                if let Some(m) = spec.model.clone().or(spec.cfg.model.clone()) {
                                    params["model"] = json!(m);
                                }
                                let _ = p.write_json(&json!({ "id": id, "method": "thread/start", "params": params })).await;
                            }
                            Pending::ThreadStart => {
                                let tid = r["thread"]["id"].as_str().unwrap_or_default().to_string();
                                session_file = r["thread"]["path"].as_str().map(PathBuf::from);
                                let _ = emit.send(RunEvent::Session {
                                    session_id: tid.clone(),
                                    runtime: spec.runtime_name.clone(),
                                    model: r["model"].as_str().map(str::to_string),
                                });
                                thread_id = Some(tid.clone());
                                let id = ids.next_id();
                                pending.insert(id, Pending::TurnStart);
                                turn_running = true;
                                let _ = p.write_json(&json!({ "id": id, "method": "turn/start", "params": {
                                    "threadId": tid, "input": text_input(&spec.prompt) } })).await;
                            }
                            Pending::TurnStart => {
                                if active_turn.is_none() {
                                    active_turn = r["turn"]["id"].as_str().map(str::to_string);
                                }
                            }
                            Pending::Steer { .. } | Pending::Interrupt => {}
                        }
                    }
                    Rpc::Request { id, method, params } => {
                        let approval = matches!(method,
                            "item/commandExecution/requestApproval" | "item/fileChange/requestApproval");
                        if approval {
                            approval_n += 1;
                            let aid = format!("a{approval_n}");
                            let (title, detail) = if method.contains("commandExecution") {
                                (format!("run command: {}", params["command"].as_str().unwrap_or("?")),
                                 params["reason"].as_str().map(str::to_string))
                            } else {
                                ("apply file changes".to_string(), params["reason"].as_str().map(str::to_string))
                            };
                            approvals.insert(aid.clone(), id.clone());
                            let _ = emit.send(RunEvent::ApprovalRequest {
                                approval_id: aid, item_id: params["itemId"].as_str().map(str::to_string),
                                title, detail,
                            });
                        } else {
                            let _ = p.write_json(&json!({ "id": id, "error": {
                                "code": -32601, "message": format!("{method} is not supported by this client") } })).await;
                        }
                    }
                    Rpc::Notification { method, params } => {
                        if method == "turn/started" {
                            active_turn = params["turn"]["id"].as_str().map(str::to_string);
                        }
                        for ev in norm.on_notification(method, params) {
                            let _ = emit.send(ev);
                        }
                        if method == "turn/completed" {
                            active_turn = None;
                            turn_running = false;
                            // A steer sent as the turn ended gets its answer after this; wait
                            // for it, so input that missed the turn starts the next one.
                            ending = Some(tokio::time::Instant::now() + STEER_WAIT);
                        }
                    }
                    Rpc::Other => {}
                }
            }
            c = control.recv(), if control_open => {
                let Some(c) = c else { control_open = false; continue };
                match c {
                    Control::Input { input_id, text } => {
                        let _ = emit.send(RunEvent::InputAccepted { input_id });
                        match (&thread_id, &active_turn, turn_running) {
                            (Some(tid), Some(turn), true) => {
                                let id = ids.next_id();
                                pending.insert(id, Pending::Steer { text: text.clone() });
                                let _ = p.write_json(&json!({ "id": id, "method": "turn/steer", "params": {
                                    "threadId": tid, "input": text_input(&text), "expectedTurnId": turn } })).await;
                            }
                            _ => queued.push_back(text),
                        }
                    }
                    Control::Approval { approval_id, approve } => {
                        if let Some(rid) = approvals.remove(&approval_id) {
                            let decision = if approve { "accept" } else { "decline" };
                            let _ = p.write_json(&json!({ "id": rid, "result": { "decision": decision } })).await;
                            let _ = emit.send(RunEvent::ApprovalResolved { approval_id, decision: if approve { "approve".into() } else { "deny".into() } });
                        }
                    }
                    Control::Cancel => {
                        if let (Some(tid), Some(turn)) = (&thread_id, &active_turn) {
                            let id = ids.next_id();
                            pending.insert(id, Pending::Interrupt);
                            let _ = p.write_json(&json!({ "id": id, "method": "turn/interrupt", "params": {
                                "threadId": tid, "turnId": turn } })).await;
                        }
                        let code = p.kill_group(Duration::from_secs(3)).await;
                        return Outcome {
                            status: Status::Cancelled, exit_code: code, result_text: norm.last_message,
                            error: None, session_id: thread_id, session_file, usage: norm.usage,
                        };
                    }
                }
            }
            _ = tokio::time::sleep_until(ending.unwrap_or_else(tokio::time::Instant::now)), if ending.is_some() => {
                // No answer to a steer in time: its text gets the next turn.
                let ids_left: Vec<u64> = pending
                    .iter()
                    .filter(|(_, k)| matches!(k, Pending::Steer { .. }))
                    .map(|(i, _)| *i)
                    .collect();
                for i in ids_left {
                    if let Some(Pending::Steer { text }) = pending.remove(&i) {
                        queued.push_back(text);
                    }
                }
            }
        }
    }
    let code = p.finish(Duration::from_secs(3)).await;
    Outcome {
        status: outcome_status,
        exit_code: code,
        result_text: norm.last_message,
        error: if outcome_status == Status::Failed {
            norm.last_error
        } else {
            None
        },
        session_id: thread_id,
        session_file,
        usage: norm.usage,
    }
}

/// `thread/start` approval policy and sandbox. A side chat run (`spec.aside`) is read-only
/// whatever `daemon.toml` says: sandbox `read-only`, approval policy `never` (nothing to ask).
pub fn thread_policy(spec: &RunSpec) -> (String, String) {
    if spec.aside {
        return ("never".into(), "read-only".into());
    }
    (
        spec.cfg
            .approval_policy
            .clone()
            .unwrap_or_else(|| "never".into()),
        spec.cfg
            .sandbox
            .clone()
            .unwrap_or_else(|| "workspace-write".into()),
    )
}

/// Config overrides of a side chat run: tools outside the sandbox (web search, apps, plugins,
/// browser and computer use) are off.
pub const ASIDE_CONFIG: &[&str] = &[
    "web_search=\"disabled\"",
    "features.apps=false",
    "features.plugins=false",
    "features.browser_use=false",
    "features.browser_use_external=false",
    "features.computer_use=false",
];

/// Names of the MCP servers in the Codex config files a run can load, each with whether its
/// transport is a URL: `$CODEX_HOME/config.toml` and `.codex/config.toml` in the working
/// directory and each folder above it (Codex loads these only for a trusted project). The first
/// file that defines a name gives its transport: the user config, then the nearest folder.
fn configured_mcp_servers(spec: &RunSpec) -> Result<Vec<(String, bool)>, String> {
    let home = spec
        .env_var("CODEX_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(spec.env_var("HOME").unwrap_or("/")).join(".codex"));
    let files = std::iter::once(home.join("config.toml"))
        .chain(spec.cwd.ancestors().map(|d| d.join(".codex/config.toml")));
    let mut names: Vec<(String, bool)> = vec![];
    for f in files {
        let Ok(text) = std::fs::read_to_string(&f) else {
            continue;
        };
        let table: toml::Table =
            toml::from_str(&text).map_err(|_| format!("cannot read {}", f.display()))?;
        if let Some(servers) = table.get("mcp_servers").and_then(toml::Value::as_table) {
            for (n, def) in servers {
                if !names.iter().any(|(m, _)| m == n) {
                    names.push((n.clone(), def.get("url").is_some()));
                }
            }
        }
    }
    Ok(names)
}

/// `codex app-server` arguments. A side chat run ignores the configured args (they could widen
/// the sandbox) and turns off [`ASIDE_CONFIG`] and every configured MCP server: MCP tools run
/// outside the sandbox. The sandbox (`thread_policy`) blocks writes and network for commands.
///
/// Codex has no override that turns off all MCP servers, and `enabled=false` alone for a server
/// Codex did not load (an untrusted project's) stops `codex app-server` ("invalid transport").
/// So each server gets a disabled definition with a placeholder of its own transport: Codex
/// merges it into the loaded server, or adds it as a new disabled one.
pub fn app_server_args(spec: &RunSpec) -> Result<Vec<String>, String> {
    let mut args = vec!["app-server".to_string()];
    if !spec.aside {
        args.extend(spec.cfg.args.iter().cloned());
        return Ok(args);
    }
    let plain = |n: &str| {
        !n.is_empty()
            && n.chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
    };
    let mut overrides: Vec<String> = ASIDE_CONFIG.iter().map(|c| c.to_string()).collect();
    for (name, url) in configured_mcp_servers(spec)? {
        if !plain(&name) {
            return Err(format!(
                "cannot turn off MCP server {name:?} for a read-only run"
            ));
        }
        let transport = if url {
            "url=\"http://127.0.0.1/\""
        } else {
            "command=\"true\""
        };
        overrides.push(format!("mcp_servers.{name}={{{transport},enabled=false}}"));
    }
    for o in overrides {
        args.push("-c".into());
        args.push(o);
    }
    Ok(args)
}

/// Input that arrived while the last turn was ending still gets a turn.
pub(crate) fn take_late_input(
    control: &mut mpsc::UnboundedReceiver<Control>,
    queued: &mut VecDeque<String>,
    emit: &Emit,
) {
    while let Ok(c) = control.try_recv() {
        if let Control::Input { input_id, text } = c {
            let _ = emit.send(RunEvent::InputAccepted { input_id });
            queued.push_back(text);
        }
    }
}
