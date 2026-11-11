//! Agent Client Protocol agents over stdio (JSON-RPC 2.0), e.g. `gemini --acp`, `opencode acp`.
//!
//! Flow: `initialize` -> `session/new` -> `session/prompt`; `session/update` notifications
//! stream the work; the `session/prompt` response (`stopReason`) ends a turn. Follow-ups are
//! queued and sent as the next `session/prompt`. `session/request_permission` is allowed at
//! once unless it matches one of the run's gates, which asks a person (see [`crate::gates`]).
//! Cancel sends `session/cancel`, then kills the process group.
//!
//! Resume (`resumeSessionId`): when the agent's `initialize` answer has
//! `agentCapabilities.loadSession`, the run sends `session/load` instead of `session/new`. The
//! history the agent replays while it loads is not reported again. An agent that cannot load the
//! session (no capability, or an error answer) gets a new one and is told so in its first prompt.

use super::*;
use crate::proto::{FileChange, RunEvent, truncate};
use serde_json::{Value, json};
use std::collections::{HashMap, VecDeque};

pub struct AcpRuntime;

#[derive(Default)]
pub struct AcpNormalizer {
    pub last_message: Option<String>,
    turn_n: u32,
    msg_n: u32,
    turn: Option<String>,
    msg: Option<(String, String)>,
    thought: String,
    tools: HashMap<String, String>,
}

impl AcpNormalizer {
    pub fn start_turn(&mut self) -> Vec<RunEvent> {
        self.turn_n += 1;
        let id = format!("t{}", self.turn_n);
        self.turn = Some(id.clone());
        vec![RunEvent::TurnStarted { turn_id: id }]
    }

    fn flush(&mut self) -> Vec<RunEvent> {
        let mut out = vec![];
        if !self.thought.is_empty() {
            self.msg_n += 1;
            out.push(RunEvent::Reasoning {
                item_id: format!("r{}", self.msg_n),
                turn_id: self.turn.clone(),
                text: std::mem::take(&mut self.thought),
            });
        }
        if let Some((id, text)) = self.msg.take() {
            self.last_message = Some(text.clone());
            out.push(RunEvent::Message {
                item_id: id,
                turn_id: self.turn.clone(),
                role: "assistant".into(),
                text,
            });
        }
        out
    }

    pub fn end_turn(&mut self, stop_reason: &str) -> Vec<RunEvent> {
        let mut out = self.flush();
        let (status, error) = match stop_reason {
            "end_turn" | "max_tokens" | "max_turn_requests" => ("completed", None),
            "cancelled" => ("interrupted", None),
            other => ("failed", Some(other.to_string())),
        };
        out.push(RunEvent::TurnCompleted {
            turn_id: self.turn.take().unwrap_or_else(|| "t0".into()),
            status: status.into(),
            error,
        });
        out
    }

    pub fn on_update(&mut self, u: &Value) -> Vec<RunEvent> {
        match u["sessionUpdate"].as_str().unwrap_or_default() {
            "agent_message_chunk" => {
                let text = u["content"]["text"]
                    .as_str()
                    .unwrap_or_default()
                    .to_string();
                if text.is_empty() {
                    return vec![];
                }
                let mut out = vec![];
                if !self.thought.is_empty() {
                    self.msg_n += 1;
                    out.push(RunEvent::Reasoning {
                        item_id: format!("r{}", self.msg_n),
                        turn_id: self.turn.clone(),
                        text: std::mem::take(&mut self.thought),
                    });
                }
                if self.msg.is_none() {
                    self.msg_n += 1;
                    self.msg = Some((format!("m{}", self.msg_n), String::new()));
                }
                let (id, acc) = self.msg.as_mut().expect("message");
                acc.push_str(&text);
                out.push(RunEvent::MessageDelta {
                    item_id: id.clone(),
                    turn_id: self.turn.clone(),
                    text,
                });
                out
            }
            "agent_thought_chunk" => {
                self.thought
                    .push_str(u["content"]["text"].as_str().unwrap_or_default());
                vec![]
            }
            "tool_call" => {
                let mut out = self.flush();
                let id = u["toolCallId"].as_str().unwrap_or_default().to_string();
                let title = u["title"].as_str().unwrap_or("tool").to_string();
                self.tools.insert(id.clone(), title.clone());
                out.push(RunEvent::ToolCall {
                    item_id: id.clone(),
                    turn_id: self.turn.clone(),
                    tool: u["kind"].as_str().unwrap_or("other").to_string(),
                    title,
                    input: u.get("rawInput").cloned(),
                });
                out.extend(self.tool_done(&id, u));
                out
            }
            "tool_call_update" => {
                let id = u["toolCallId"].as_str().unwrap_or_default().to_string();
                self.tool_done(&id, u)
            }
            _ => vec![],
        }
    }

    fn tool_done(&mut self, id: &str, u: &Value) -> Vec<RunEvent> {
        let status = u["status"].as_str();
        if !matches!(status, Some("completed") | Some("failed")) {
            return vec![];
        }
        let mut out = vec![];
        let mut text = vec![];
        let mut changes = vec![];
        for c in u["content"].as_array().into_iter().flatten() {
            match c["type"].as_str() {
                Some("diff") => changes.push(FileChange {
                    path: c["path"].as_str().unwrap_or_default().to_string(),
                    kind: if c["oldText"].is_null() {
                        "add"
                    } else {
                        "modify"
                    }
                    .into(),
                    diff: None,
                }),
                Some("content") => text.push(value_text(&json!([c["content"]]))),
                _ => {}
            }
        }
        if text.is_empty()
            && let Some(raw) = u.get("rawOutput")
        {
            text.push(value_text(raw));
        }
        out.push(RunEvent::ToolResult {
            item_id: id.to_string(),
            turn_id: self.turn.clone(),
            ok: status == Some("completed"),
            output: Some(truncate(&text.join("\n"), OUTPUT_LIMIT)),
            exit_code: None,
        });
        if !changes.is_empty() {
            out.push(RunEvent::FileChange {
                item_id: format!("{id}:file"),
                turn_id: self.turn.clone(),
                changes,
            });
        }
        out
    }
}

impl Runtime for AcpRuntime {
    fn start(
        &self,
        spec: RunSpec,
        emit: Emit,
        control: mpsc::UnboundedReceiver<Control>,
    ) -> BoxFuture<Outcome> {
        Box::pin(run(spec, emit, control))
    }
}

#[derive(Debug)]
enum Pending {
    Initialize,
    NewSession,
    LoadSession(String),
    Prompt,
}

async fn run(spec: RunSpec, emit: Emit, mut control: mpsc::UnboundedReceiver<Control>) -> Outcome {
    let program = spec.cfg.command();
    let mut p = match Proc::spawn(&spec, &program, &spec.cfg.args) {
        Ok(p) => p,
        Err(e) => return Outcome::failed(format!("cannot start {program}: {e}")),
    };
    let mut ids = RpcIds::default();
    let mut pending: HashMap<u64, Pending> = HashMap::new();
    let mut norm = AcpNormalizer::default();
    let mut session: Option<String> = None;
    // A resumed session could not be loaded: the first prompt says the session is new.
    let mut lost = false;
    // `session/load` is not answered yet: the agent's replay of the old session is not reported.
    let mut loading = false;
    let mut prompting = false;
    let mut queued: VecDeque<String> = VecDeque::new();
    let mut approvals: HashMap<String, (Value, Vec<Value>)> = HashMap::new();
    let mut approval_n = 0u32;
    let mut last_stop: String;
    let mut control_open = true;
    // Permission requests that match a gate wait here for a person: (json-rpc id, outcome).
    let mut asks: tokio::task::JoinSet<(Value, Value)> = tokio::task::JoinSet::new();

    let id = ids.next_id();
    pending.insert(id, Pending::Initialize);
    let _ = p.write_json(&initialize_request(id)).await;

    loop {
        tokio::select! {
            line = p.next_line() => {
                let Some(line) = line else {
                    let code = p.finish(Duration::from_secs(3)).await;
                    let mut o = Outcome::failed(format!("ACP agent exited: {}", p.stderr_tail()));
                    o.exit_code = code;
                    o.session_id = session;
                    o.result_text = norm.last_message;
                    return o;
                };
                let Ok(v) = serde_json::from_str::<Value>(&line) else { continue };
                match classify(&v) {
                    Rpc::Response { id, result, error } => {
                        let Some(kind) = id.as_u64().and_then(|i| pending.remove(&i)) else { continue };
                        if let (Pending::LoadSession(_), Some(e)) = (&kind, error) {
                            // The agent no longer has the session: a new one, told so.
                            eprintln!("run {}: ACP session/load failed ({}); a new session starts", spec.run_id, truncate(&rpc_error_text(e), 200));
                            loading = false;
                            lost = true;
                            new_session(&mut p, &mut ids, &mut pending, &spec).await;
                            continue;
                        }
                        if let Some(e) = error {
                            let code = p.kill_group(Duration::from_secs(3)).await;
                            let mut o = Outcome::failed(format!("ACP {kind:?}: {}", rpc_error_text(e)));
                            o.exit_code = code;
                            o.session_id = session;
                            return o;
                        }
                        let r = result.cloned().unwrap_or(Value::Null);
                        match kind {
                            Pending::Initialize => {
                                let resume = spec.resume_session_id.clone().filter(|s| valid_session_id(s));
                                match resume {
                                    Some(sid) if can_load_session(&r) => {
                                        let id = ids.next_id();
                                        pending.insert(id, Pending::LoadSession(sid.clone()));
                                        loading = true;
                                        let _ = p.write_json(&json!({ "jsonrpc": "2.0", "id": id, "method": "session/load",
                                            "params": { "sessionId": sid, "cwd": spec.cwd.to_string_lossy(), "mcpServers": [] } })).await;
                                    }
                                    other => {
                                        lost = other.is_some();
                                        new_session(&mut p, &mut ids, &mut pending, &spec).await;
                                    }
                                }
                            }
                            Pending::NewSession | Pending::LoadSession(_) => {
                                loading = false;
                                let sid = match kind {
                                    Pending::LoadSession(sid) => sid,
                                    _ => r["sessionId"].as_str().unwrap_or_default().to_string(),
                                };
                                let _ = emit.send(RunEvent::Session { session_id: sid.clone(), runtime: spec.runtime_name.clone(), model: None });
                                session = Some(sid);
                                // ACP has no system prompt: the run note goes before the first prompt.
                                let first = first_prompt(&spec, lost);
                                send_prompt(&mut p, &mut ids, &mut pending, &mut norm, &emit, session.as_deref().unwrap_or_default(), &first).await;
                                prompting = true;
                            }
                            Pending::Prompt => {
                                last_stop = r["stopReason"].as_str().unwrap_or("end_turn").to_string();
                                for ev in norm.end_turn(&last_stop) { let _ = emit.send(ev); }
                                super::codex::take_late_input(&mut control, &mut queued, &emit);
                                if last_stop != "cancelled" && !queued.is_empty() {
                                    let text = queued.drain(..).collect::<Vec<_>>().join("\n\n");
                                    send_prompt(&mut p, &mut ids, &mut pending, &mut norm, &emit, session.as_deref().unwrap_or_default(), &text).await;
                                } else {
                                    break;
                                }
                            }
                        }
                    }
                    Rpc::Request { id, method, params } => {
                        if method == "session/request_permission" && !spec.aside {
                            // Permission is open: only a request that matches one of the run's
                            // gates asks a person; any other is allowed at once.
                            let options = params["options"].as_array().cloned().unwrap_or_default();
                            let line = permission_line(&params["toolCall"]);
                            match spec.gates.clone().filter(|g| g.matches(&line).is_some()) {
                                None => {
                                    let _ = p.write_json(&json!({ "jsonrpc": "2.0", "id": id, "result": { "outcome": permission_outcome(&options, true) } })).await;
                                }
                                Some(g) => {
                                    let (rid, e) = (id.clone(), emit.clone());
                                    asks.spawn(async move {
                                        let emit = move |ev: RunEvent| { let _ = e.send(ev); };
                                        let ok = g.allows(&line, &emit).await;
                                        (rid, permission_outcome(&options, ok))
                                    });
                                }
                            }
                        } else if method == "session/request_permission" {
                            approval_n += 1;
                            let aid = format!("a{approval_n}");
                            let tc = &params["toolCall"];
                            let options = params["options"].as_array().cloned().unwrap_or_default();
                            approvals.insert(aid.clone(), (id.clone(), options));
                            let _ = emit.send(RunEvent::ApprovalRequest {
                                approval_id: aid,
                                item_id: tc["toolCallId"].as_str().map(str::to_string),
                                title: tc["title"].as_str().unwrap_or("permission").to_string(),
                                detail: tc.get("rawInput").map(|r| truncate(&r.to_string(), 2000)),
                                kind: None,
                                command: None,
                            });
                        } else {
                            let _ = p.write_json(&json!({ "jsonrpc": "2.0", "id": id, "error": {
                                "code": -32601, "message": format!("{method} is not supported") } })).await;
                        }
                    }
                    Rpc::Notification { method, params } => {
                        if method == "session/update" && !loading {
                            for ev in norm.on_update(&params["update"]) { let _ = emit.send(ev); }
                        }
                    }
                    Rpc::Other => {}
                }
            }
            Some(done) = asks.join_next(), if !asks.is_empty() => {
                if let Ok((rid, outcome)) = done {
                    let _ = p.write_json(&json!({ "jsonrpc": "2.0", "id": rid, "result": { "outcome": outcome } })).await;
                }
            }
            c = control.recv(), if control_open => {
                let Some(c) = c else { control_open = false; continue };
                match c {
                    Control::Input { input_id, text } => {
                        let _ = emit.send(RunEvent::InputAccepted { input_id });
                        if prompting || session.is_none() { queued.push_back(text); }
                        else {
                            send_prompt(&mut p, &mut ids, &mut pending, &mut norm, &emit, session.as_deref().unwrap_or_default(), &text).await;
                            prompting = true;
                        }
                    }
                    Control::Approval { approval_id, approve } => {
                        if let Some((rid, options)) = approvals.remove(&approval_id) {
                            let outcome = permission_outcome(&options, approve);
                            let _ = p.write_json(&json!({ "jsonrpc": "2.0", "id": rid, "result": { "outcome": outcome } })).await;
                            let _ = emit.send(RunEvent::ApprovalResolved { approval_id, decision: if approve { "approve".into() } else { "deny".into() } });
                        }
                    }
                    Control::Cancel => {
                        if let Some(sid) = &session {
                            let _ = p.write_json(&json!({ "jsonrpc": "2.0", "method": "session/cancel", "params": { "sessionId": sid } })).await;
                        }
                        let code = p.kill_group(Duration::from_secs(3)).await;
                        return Outcome { status: Status::Cancelled, exit_code: code, result_text: norm.last_message,
                            error: None, session_id: session, session_file: None, usage: None };
                    }
                }
            }
        }
    }
    let code = p.finish(Duration::from_secs(3)).await;
    let ok = matches!(
        last_stop.as_str(),
        "end_turn" | "max_tokens" | "max_turn_requests"
    );
    Outcome {
        status: if ok {
            Status::Succeeded
        } else {
            Status::Failed
        },
        exit_code: code,
        error: (!ok).then(|| format!("stop reason: {last_stop}")),
        result_text: norm.last_message,
        session_id: session,
        session_file: None,
        usage: None,
    }
}

/// What the gates match for a `session/request_permission` tool call: the command of a shell
/// tool (`rawInput.command`, a string or a list of words), else the title and the raw input.
pub fn permission_line(tool_call: &Value) -> String {
    let raw = &tool_call["rawInput"];
    let command = match &raw["command"] {
        Value::String(c) => Some(c.clone()),
        Value::Array(words) => Some(
            words
                .iter()
                .map(|w| w.as_str().map_or_else(|| w.to_string(), str::to_string))
                .collect::<Vec<_>>()
                .join(" "),
        ),
        _ => None,
    };
    let title = tool_call["title"].as_str().unwrap_or_default();
    match command {
        Some(c) => c,
        None if raw.is_null() => title.to_string(),
        None => format!("{title} {raw}"),
    }
}

/// The `session/request_permission` outcome: the first option of the allow or the reject kind
/// (`allow_once`, `allow_always`, `reject_once`, …), cancelled when the agent offered none.
pub fn permission_outcome(options: &[Value], approve: bool) -> Value {
    let want = if approve { "allow" } else { "reject" };
    let pick = options
        .iter()
        .find(|o| o["kind"].as_str().is_some_and(|k| k.starts_with(want)));
    match pick.and_then(|o| o["optionId"].as_str()) {
        Some(oid) => json!({ "outcome": "selected", "optionId": oid }),
        None => json!({ "outcome": "cancelled" }),
    }
}

/// What a run tells an agent whose earlier session could not be continued: its prompt carries
/// only what is new since that session.
pub const LOST_SESSION_NOTE: &str = "Your earlier conversation in this topic could not be \
continued: this is a new session and you do not remember it. The messages below are only what \
is new since then. Before you act, read the topic and what was decided with `chat-run topic \
history`.";

/// The first prompt: the run note (how to use `chat-run`), when set, the lost-session note when
/// `lost`, then the prompt.
pub fn first_prompt(spec: &RunSpec, lost: bool) -> String {
    let mut parts: Vec<&str> = vec![];
    if let Some(n) = &spec.note {
        parts.push(n);
    }
    if lost {
        parts.push(LOST_SESSION_NOTE);
    }
    parts.push(&spec.prompt);
    parts.join("\n\n")
}

/// Whether an `initialize` answer says the agent can load an earlier session
/// (`agentCapabilities.loadSession`).
pub fn can_load_session(initialize: &Value) -> bool {
    initialize["agentCapabilities"]["loadSession"].as_bool() == Some(true)
}

async fn new_session(
    p: &mut Proc,
    ids: &mut RpcIds,
    pending: &mut HashMap<u64, Pending>,
    spec: &RunSpec,
) {
    let id = ids.next_id();
    pending.insert(id, Pending::NewSession);
    let _ = p
        .write_json(
            &json!({ "jsonrpc": "2.0", "id": id, "method": "session/new",
            "params": { "cwd": spec.cwd.to_string_lossy(), "mcpServers": [] } }),
        )
        .await;
}

/// Asks an ACP agent whether it can load an earlier session (`CliInfo.resume`): starts it with
/// its configured args, sends `initialize` and reads the answer, at most 15 seconds. None when
/// it did not answer.
pub async fn probe_load_session(
    program: &std::path::Path,
    rt: &crate::config::RuntimeConfig,
) -> Option<bool> {
    use std::process::Stdio;
    let mut cmd = crate::health::command(program);
    cmd.args(&rt.args)
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
    let ask = async {
        let mut msg = initialize_request(1).to_string();
        msg.push('\n');
        stdin.write_all(msg.as_bytes()).await.ok()?;
        stdin.flush().await.ok()?;
        let mut lines = BufReader::new(stdout).lines();
        loop {
            let line = lines.next_line().await.ok()??;
            let Ok(v) = serde_json::from_str::<Value>(&line) else {
                continue;
            };
            if v["id"].as_u64() == Some(1) && v.get("method").is_none() {
                return Some(v.get("result").is_some_and(can_load_session));
            }
        }
    };
    let out = tokio::time::timeout(Duration::from_secs(15), ask)
        .await
        .ok()
        .flatten();
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

/// The `initialize` request a run and the capability probe send.
fn initialize_request(id: u64) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "method": "initialize", "params": {
        "protocolVersion": 1,
        "clientCapabilities": { "fs": { "readTextFile": false, "writeTextFile": false }, "terminal": false }
    }})
}

async fn send_prompt(
    p: &mut Proc,
    ids: &mut RpcIds,
    pending: &mut HashMap<u64, Pending>,
    norm: &mut AcpNormalizer,
    emit: &Emit,
    session: &str,
    text: &str,
) {
    for ev in norm.start_turn() {
        let _ = emit.send(ev);
    }
    let id = ids.next_id();
    pending.insert(id, Pending::Prompt);
    let _ = p
        .write_json(
            &json!({ "jsonrpc": "2.0", "id": id, "method": "session/prompt",
            "params": { "sessionId": session, "prompt": [{ "type": "text", "text": text }] } }),
        )
        .await;
}
