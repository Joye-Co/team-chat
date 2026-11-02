//! Claude Code headless: `claude -p --input-format stream-json --output-format stream-json
//! --verbose --include-partial-messages`.
//!
//! Each user line written to stdin is one turn; each `result` line ends one turn. Follow-ups
//! are written while a turn runs and the CLI queues them. When every turn has its result, stdin
//! is closed and the CLI exits. The raw session is
//! `$CLAUDE_CONFIG_DIR/projects/<cwd with non-alphanumerics as '-'>/<session id>.jsonl`
//! (default config dir `~/.claude`).

use super::*;
use crate::proto::{FileChange, RunEvent, Usage, truncate};
use serde_json::{Value, json};
use std::collections::{HashMap, VecDeque};

pub struct ClaudeRuntime;

#[derive(Default)]
pub struct ClaudeNormalizer {
    pub session_id: Option<String>,
    pub last_result: Option<String>,
    pub last_is_error: bool,
    pub usage: Option<Usage>,
    turns: VecDeque<String>,
    turn_n: u32,
    current_msg: Option<String>,
    consumed: HashMap<String, usize>,
    tools: HashMap<String, (String, Value)>,
}

impl ClaudeNormalizer {
    /// Called when the daemon writes a user line. Returns turn.started when it is the only
    /// open turn (otherwise the turn starts when the previous one ends).
    pub fn queue_turn(&mut self) -> Vec<RunEvent> {
        self.turn_n += 1;
        let id = format!("t{}", self.turn_n);
        self.turns.push_back(id.clone());
        if self.turns.len() == 1 {
            vec![RunEvent::TurnStarted { turn_id: id }]
        } else {
            vec![]
        }
    }

    pub fn open_turns(&self) -> usize {
        self.turns.len()
    }

    fn turn(&self) -> Option<String> {
        self.turns.front().cloned()
    }

    pub fn on_line(&mut self, v: &Value) -> Vec<RunEvent> {
        match v["type"].as_str().unwrap_or_default() {
            "system" => match v["subtype"].as_str() {
                Some("init") => {
                    let sid = v["session_id"].as_str().unwrap_or_default().to_string();
                    self.session_id = Some(sid.clone());
                    vec![RunEvent::Session {
                        session_id: sid,
                        runtime: "claude".into(),
                        model: v["model"].as_str().map(str::to_string),
                    }]
                }
                Some("api_retry") => vec![RunEvent::Status {
                    state: "retrying".into(),
                    message: v["error"].as_str().map(str::to_string),
                }],
                _ => vec![],
            },
            "stream_event" => {
                let e = &v["event"];
                match e["type"].as_str() {
                    Some("message_start") => {
                        self.current_msg = e["message"]["id"].as_str().map(str::to_string);
                        vec![]
                    }
                    Some("content_block_delta") if e["delta"]["type"] == "text_delta" => {
                        let msg = self.current_msg.clone().unwrap_or_else(|| "m".into());
                        vec![RunEvent::MessageDelta {
                            item_id: format!("{msg}:{}", e["index"].as_u64().unwrap_or(0)),
                            turn_id: self.turn(),
                            text: e["delta"]["text"].as_str().unwrap_or_default().to_string(),
                        }]
                    }
                    _ => vec![],
                }
            }
            "assistant" => {
                let m = &v["message"];
                let msg = m["id"].as_str().unwrap_or("m").to_string();
                let blocks = m["content"].as_array().cloned().unwrap_or_default();
                let base = *self.consumed.get(&msg).unwrap_or(&0);
                self.consumed.insert(msg.clone(), base + blocks.len());
                let mut out = vec![];
                for (i, b) in blocks.iter().enumerate() {
                    let item_id = format!("{msg}:{}", base + i);
                    match b["type"].as_str() {
                        Some("text") => out.push(RunEvent::Message {
                            item_id,
                            turn_id: self.turn(),
                            role: "assistant".into(),
                            text: b["text"].as_str().unwrap_or_default().to_string(),
                        }),
                        Some("thinking") => {
                            let t = b["thinking"].as_str().unwrap_or_default();
                            if !t.is_empty() {
                                out.push(RunEvent::Reasoning {
                                    item_id,
                                    turn_id: self.turn(),
                                    text: t.to_string(),
                                });
                            }
                        }
                        Some("tool_use") => {
                            let id = b["id"].as_str().unwrap_or_default().to_string();
                            let name = b["name"].as_str().unwrap_or("tool").to_string();
                            let input = b["input"].clone();
                            self.tools.insert(id.clone(), (name.clone(), input.clone()));
                            out.push(RunEvent::ToolCall {
                                item_id: id,
                                turn_id: self.turn(),
                                title: tool_title(&name, &input),
                                tool: name,
                                input: Some(input),
                            });
                        }
                        _ => {}
                    }
                }
                out
            }
            "user" => {
                let mut out = vec![];
                let Some(blocks) = v["message"]["content"].as_array() else {
                    return out;
                };
                for b in blocks {
                    if b["type"] != "tool_result" {
                        continue;
                    }
                    let id = b["tool_use_id"].as_str().unwrap_or_default().to_string();
                    let ok = b["is_error"].as_bool() != Some(true);
                    out.push(RunEvent::ToolResult {
                        item_id: id.clone(),
                        turn_id: self.turn(),
                        ok,
                        output: Some(truncate(&value_text(&b["content"]), OUTPUT_LIMIT)),
                        exit_code: None,
                    });
                    if let (true, Some((name, input))) = (ok, self.tools.get(&id)) {
                        if matches!(name.as_str(), "Edit" | "MultiEdit" | "Write" | "NotebookEdit") {
                            let path = input["file_path"]
                                .as_str()
                                .or(input["notebook_path"].as_str())
                                .unwrap_or_default()
                                .to_string();
                            out.push(RunEvent::FileChange {
                                item_id: format!("{id}:file"),
                                turn_id: self.turn(),
                                changes: vec![FileChange {
                                    path,
                                    kind: "modify".into(),
                                    diff: None,
                                }],
                            });
                        }
                    }
                }
                out
            }
            "result" => {
                let is_error = v["is_error"].as_bool() == Some(true)
                    || v["subtype"].as_str().is_some_and(|s| s != "success");
                let text = v["result"].as_str().map(str::to_string);
                self.last_is_error = is_error;
                if text.is_some() {
                    self.last_result = text.clone();
                }
                if let Some(sid) = v["session_id"].as_str() {
                    self.session_id.get_or_insert_with(|| sid.to_string());
                }
                let u = &v["usage"];
                let prev = self.usage.clone().unwrap_or_default();
                self.usage = Some(Usage {
                    input_tokens: prev.input_tokens
                        + u["input_tokens"].as_u64().unwrap_or(0)
                        + u["cache_read_input_tokens"].as_u64().unwrap_or(0)
                        + u["cache_creation_input_tokens"].as_u64().unwrap_or(0),
                    output_tokens: prev.output_tokens + u["output_tokens"].as_u64().unwrap_or(0),
                    cost_usd: v["total_cost_usd"].as_f64().or(prev.cost_usd),
                });
                let mut out = vec![];
                let done = self.turns.pop_front().unwrap_or_else(|| "t0".into());
                out.push(RunEvent::TurnCompleted {
                    turn_id: done,
                    status: if is_error { "failed" } else { "completed" }.into(),
                    error: if is_error { text } else { None },
                });
                if let Some(next) = self.turns.front() {
                    out.push(RunEvent::TurnStarted {
                        turn_id: next.clone(),
                    });
                }
                out
            }
            _ => vec![],
        }
    }
}

fn tool_title(name: &str, input: &Value) -> String {
    let pick = ["command", "file_path", "pattern", "url", "description", "path"]
        .iter()
        .find_map(|k| input[*k].as_str());
    match pick {
        Some(p) => format!("{name}: {}", truncate(p, 200)),
        None => name.to_string(),
    }
}

/// `~/.claude/projects` folder name for a working directory.
pub fn project_dir_name(cwd: &Path) -> String {
    let real = std::fs::canonicalize(cwd).unwrap_or_else(|_| cwd.to_path_buf());
    real.to_string_lossy()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect()
}

pub fn find_session_file(config_dir: &Path, cwd: &Path, session_id: &str) -> Option<PathBuf> {
    let projects = config_dir.join("projects");
    let direct = projects
        .join(project_dir_name(cwd))
        .join(format!("{session_id}.jsonl"));
    if direct.is_file() {
        return Some(direct);
    }
    std::fs::read_dir(&projects).ok()?.flatten().find_map(|d| {
        let f = d.path().join(format!("{session_id}.jsonl"));
        f.is_file().then_some(f)
    })
}

fn user_line(text: &str) -> Value {
    json!({ "type": "user", "message": { "role": "user", "content": [{ "type": "text", "text": text }] } })
}

impl Runtime for ClaudeRuntime {
    fn start(
        &self,
        spec: RunSpec,
        emit: Emit,
        control: mpsc::UnboundedReceiver<Control>,
    ) -> BoxFuture<Outcome> {
        Box::pin(run(spec, emit, control))
    }
}

async fn run(
    spec: RunSpec,
    emit: Emit,
    mut control: mpsc::UnboundedReceiver<Control>,
) -> Outcome {
    let session_id = uuid_v4();
    let mut args: Vec<String> = [
        "-p",
        "--input-format",
        "stream-json",
        "--output-format",
        "stream-json",
        "--verbose",
        "--include-partial-messages",
        "--session-id",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect();
    args.push(session_id.clone());
    args.push("--permission-mode".into());
    args.push(
        spec.cfg
            .permission_mode
            .clone()
            .unwrap_or_else(|| "acceptEdits".into()),
    );
    if let Some(m) = spec.model.clone().or(spec.cfg.model.clone()) {
        args.push("--model".into());
        args.push(m);
    }
    if let Some(t) = &spec.cfg.allowed_tools {
        args.push("--allowedTools".into());
        args.push(t.clone());
    }
    args.extend(spec.cfg.args.iter().cloned());
    let program = spec.cfg.command();
    let config_dir = spec
        .env_var("CLAUDE_CONFIG_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(spec.env_var("HOME").unwrap_or("/")).join(".claude"));

    let mut p = match Proc::spawn(&spec, &program, &args) {
        Ok(p) => p,
        Err(e) => return Outcome::failed(format!("cannot start {program}: {e}")),
    };
    let mut norm = ClaudeNormalizer::default();
    for ev in norm.queue_turn() {
        let _ = emit.send(ev);
    }
    if let Err(e) = p.write_json(&user_line(&spec.prompt)).await {
        return Outcome::failed(format!("claude: {e}"));
    }
    let mut control_open = true;
    let mut exited_early = false;
    loop {
        tokio::select! {
            line = p.next_line() => {
                let Some(line) = line else { exited_early = norm.open_turns() > 0; break };
                let Ok(v) = serde_json::from_str::<Value>(&line) else { continue };
                for ev in norm.on_line(&v) {
                    let _ = emit.send(ev);
                }
                if v["type"] == "result" && norm.open_turns() == 0 {
                    let mut late = VecDeque::new();
                    super::codex::take_late_input(&mut control, &mut late, &emit);
                    if late.is_empty() { break; }
                    for text in late {
                        for ev in norm.queue_turn() { let _ = emit.send(ev); }
                        if p.write_json(&user_line(&text)).await.is_err() { exited_early = true; break; }
                    }
                    if exited_early { break; }
                }
            }
            c = control.recv(), if control_open => {
                let Some(c) = c else { control_open = false; continue };
                match c {
                    Control::Input { input_id, text } => {
                        let _ = emit.send(RunEvent::InputAccepted { input_id });
                        for ev in norm.queue_turn() { let _ = emit.send(ev); }
                        if p.write_json(&user_line(&text)).await.is_err() {
                            exited_early = true;
                            break;
                        }
                    }
                    Control::Approval { .. } => {}
                    Control::Cancel => {
                        let code = p.kill_group(Duration::from_secs(3)).await;
                        let sid = norm.session_id.clone().unwrap_or(session_id.clone());
                        return Outcome {
                            status: Status::Cancelled, exit_code: code, result_text: norm.last_result,
                            error: None, session_file: find_session_file(&config_dir, &spec.cwd, &sid),
                            session_id: Some(sid), usage: norm.usage,
                        };
                    }
                }
            }
        }
    }
    let code = p.finish(Duration::from_secs(30)).await;
    let sid = norm.session_id.clone().unwrap_or(session_id);
    let failed = exited_early || norm.last_is_error || norm.last_result.is_none();
    Outcome {
        status: if failed {
            Status::Failed
        } else {
            Status::Succeeded
        },
        exit_code: code,
        error: if exited_early {
            Some(format!("claude exited before the turn ended: {}", p.stderr_tail()))
        } else if failed {
            norm.last_result.clone()
        } else {
            None
        },
        result_text: norm.last_result,
        session_file: find_session_file(&config_dir, &spec.cwd, &sid),
        session_id: Some(sid),
        usage: norm.usage,
    }
}
