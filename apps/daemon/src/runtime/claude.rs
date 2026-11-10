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

/// Claude Code's `modelUsage` of a result: per model its tokens (input apart from cache reads and
/// writes) and its own cost. None when the result has none.
pub fn model_usage(v: &Value) -> Option<Vec<crate::proto::ModelUsage>> {
    let map = v.as_object()?;
    let n = |m: &Value, k: &str| m[k].as_u64().unwrap_or(0);
    let mut out: Vec<crate::proto::ModelUsage> = map
        .iter()
        .filter(|(k, _)| !k.is_empty())
        .map(|(model, m)| crate::proto::ModelUsage {
            model: model.chars().take(200).collect(),
            input_tokens: n(m, "inputTokens"),
            output_tokens: n(m, "outputTokens"),
            cache_read_tokens: Some(n(m, "cacheReadInputTokens")).filter(|x| *x > 0),
            cache_write_tokens: Some(n(m, "cacheCreationInputTokens")).filter(|x| *x > 0),
            cost_usd: m["costUSD"].as_f64().filter(|c| c.is_finite() && *c >= 0.),
        })
        .collect();
    out.sort_by(|a, b| a.model.cmp(&b.model));
    out.truncate(32);
    (!out.is_empty()).then_some(out)
}

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
    plan: PlanTracker,
    /// Claude Code's task tools: (task id once known, subject, status), in creation order.
    tasks: Vec<(Option<String>, String, String)>,
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
                            let plan = self.plan_from_tool(&name, &input);
                            out.push(RunEvent::ToolCall {
                                item_id: id,
                                turn_id: self.turn(),
                                title: tool_title(&name, &input),
                                tool: name,
                                input: Some(input),
                            });
                            out.extend(plan);
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
                    if ok {
                        self.task_created(&id, &value_text(&b["content"]));
                    }
                    out.push(RunEvent::ToolResult {
                        item_id: id.clone(),
                        turn_id: self.turn(),
                        ok,
                        output: Some(truncate(&value_text(&b["content"]), OUTPUT_LIMIT)),
                        exit_code: None,
                    });
                    if let (true, Some((name, input))) = (ok, self.tools.get(&id))
                        && matches!(
                            name.as_str(),
                            "Edit" | "MultiEdit" | "Write" | "NotebookEdit"
                        )
                    {
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
                    // `modelUsage` covers the session so far, like `total_cost_usd`.
                    models: model_usage(&v["modelUsage"]).unwrap_or(prev.models),
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

impl ClaudeNormalizer {
    /// The run plan from Claude Code's to-do tools: `TodoWrite` (the whole list, `todos[]`
    /// with `content` and `status`) and the task tools (`TaskCreate` adds one task,
    /// `TaskUpdate` changes one by `taskId`).
    fn plan_from_tool(&mut self, name: &str, input: &Value) -> Option<RunEvent> {
        match name {
            "TodoWrite" => {
                let todos = input["todos"].as_array()?;
                self.plan.update(todos.iter().map(|t| {
                    (
                        t["content"].as_str().unwrap_or_default(),
                        t["status"].as_str().unwrap_or_default(),
                    )
                }))
            }
            "TaskCreate" => {
                let subject = input["subject"].as_str()?.to_string();
                self.tasks.push((None, subject, "pending".into()));
                self.task_plan()
            }
            "TaskUpdate" => {
                let id = input["taskId"].as_str()?;
                let task = self.tasks.iter_mut().find(|t| t.0.as_deref() == Some(id))?;
                if let Some(s) = input["status"].as_str() {
                    task.2 = s.to_string();
                }
                if let Some(s) = input["subject"].as_str() {
                    task.1 = s.to_string();
                }
                self.tasks.retain(|t| t.2 != "deleted");
                self.task_plan()
            }
            _ => None,
        }
    }

    /// A `TaskCreate` result names the new task's id: `Task #3 created successfully`.
    fn task_created(&mut self, tool_use_id: &str, result: &str) {
        let Some((name, input)) = self.tools.get(tool_use_id) else {
            return;
        };
        if name != "TaskCreate" {
            return;
        }
        let Some(id) = result
            .split("Task #")
            .nth(1)
            .and_then(|r| r.split_whitespace().next())
        else {
            return;
        };
        let subject = input["subject"].as_str().unwrap_or_default();
        if let Some(t) = self
            .tasks
            .iter_mut()
            .find(|t| t.0.is_none() && t.1 == subject)
        {
            t.0 = Some(id.to_string());
        }
    }

    fn task_plan(&mut self) -> Option<RunEvent> {
        let tasks = std::mem::take(&mut self.tasks);
        let ev = self
            .plan
            .update(tasks.iter().map(|t| (t.1.as_str(), t.2.as_str())));
        self.tasks = tasks;
        ev
    }
}

fn tool_title(name: &str, input: &Value) -> String {
    let pick = [
        "command",
        "file_path",
        "pattern",
        "url",
        "description",
        "path",
    ]
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
    project_dir_name_of(&real.to_string_lossy())
}

/// [`project_dir_name`] of a path as the CLI sees it. Windows: `canonicalize` gives the
/// `\\?\` form (`\\?\C:\Users\me\work`), which the CLI never sees; without that prefix
/// `C:\Users\me\work` is `C--Users-me-work`.
pub fn project_dir_name_of(path: &str) -> String {
    let path = match path.strip_prefix(r"\\?\UNC\") {
        Some(share) => format!(r"\\{share}"),
        None => path.strip_prefix(r"\\?\").unwrap_or(path).to_string(),
    };
    path.chars()
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

/// Readies `--resume <session_id>`: Claude Code looks for the session in the project folder of
/// its working directory, and a follow-up run has a new one, so the earlier run's session file
/// (found under any project of `config_dir`) is copied there. False when the file is gone: the
/// run then starts a new session.
pub fn prepare_resume(config_dir: &Path, cwd: &Path, session_id: &str) -> bool {
    if !super::valid_session_id(session_id) {
        return false;
    }
    let Some(found) = find_session_file(config_dir, cwd, session_id) else {
        return false;
    };
    let dir = config_dir.join("projects").join(project_dir_name(cwd));
    let target = dir.join(format!("{session_id}.jsonl"));
    if found == target {
        return true;
    }
    std::fs::create_dir_all(&dir).is_ok() && std::fs::copy(&found, &target).is_ok()
}

/// The only tools a side chat run has: built-in tools that read files.
pub const ASIDE_TOOLS: &str = "Read,Grep,Glob";

/// Tools a side chat run may never use, also if a settings file or a later CLI adds them: the
/// command, edit and web tools and every MCP tool.
pub const ASIDE_DISALLOWED_TOOLS: &str =
    "Bash Edit Write MultiEdit NotebookEdit WebFetch WebSearch mcp__*";

/// The file name (in the run folder) of a side chat run's settings: see [`aside_settings`].
pub const ASIDE_SETTINGS: &str = ".aside-settings.json";

/// The CLI's config folder: `CLAUDE_CONFIG_DIR`, else `$HOME/.claude`.
pub fn config_dir(spec: &RunSpec) -> PathBuf {
    spec.env_var("CLAUDE_CONFIG_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| spec.home().join(".claude"))
}

/// The settings a side chat run keeps from the user's `settings.json`: only `env`,
/// `apiKeyHelper` and `model` (its auth and model). Hooks, permissions, plugins, status line and
/// the rest stay out, as do the project and local settings files. None when the file has none
/// of them.
pub fn aside_settings(config_dir: &Path) -> Option<Value> {
    let text = std::fs::read_to_string(config_dir.join("settings.json")).ok()?;
    let user: Value = serde_json::from_str(&text).ok()?;
    let mut out = serde_json::Map::new();
    if let Some(env) = user["env"].as_object() {
        let env: serde_json::Map<String, Value> = env
            .iter()
            .filter(|(_, v)| v.is_string())
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        if !env.is_empty() {
            out.insert("env".into(), Value::Object(env));
        }
    }
    for k in ["apiKeyHelper", "model"] {
        if let Some(v) = user[k].as_str().filter(|v| !v.is_empty()) {
            out.insert(k.into(), json!(v));
        }
    }
    (!out.is_empty()).then_some(Value::Object(out))
}

/// Writes [`aside_settings`] to `<run folder>/.aside-settings.json` (mode 0600: the env can
/// hold auth values, which must not go on the command line) and returns its path.
fn write_aside_settings(spec: &RunSpec) -> Option<PathBuf> {
    use std::io::Write;
    let settings = aside_settings(&config_dir(spec))?;
    let path = spec.run_dir.join(ASIDE_SETTINGS);
    let _ = std::fs::remove_file(&path);
    let written = crate::sys::mode(
        std::fs::OpenOptions::new().write(true).create_new(true),
        0o600,
    )
    .open(&path)
    .and_then(|mut f| f.write_all(settings.to_string().as_bytes()));
    match written {
        Ok(()) => Some(path),
        Err(e) => {
            eprintln!("aside settings not written: {e}");
            let _ = std::fs::remove_file(&path);
            None
        }
    }
}

/// The file name (in the run folder) of the settings that install the gate hook.
pub const GATE_SETTINGS: &str = ".gate-settings.json";

/// Writes [`crate::gates::claude_settings`] to `<run folder>/.gate-settings.json` and returns
/// its path. Claude Code merges the hooks of every settings source, so the user's own settings
/// stay as they are.
fn write_gate_settings(spec: &RunSpec, link: &Path) -> std::io::Result<PathBuf> {
    let path = spec.run_dir.join(GATE_SETTINGS);
    std::fs::write(&path, crate::gates::claude_settings(link).to_string())?;
    Ok(path)
}

/// Permission flags and the configured extra args. A side chat run (`spec.aside`) is
/// read-only whatever `daemon.toml` or the CLI's own settings say: permission mode `plan`, only
/// [`ASIDE_TOOLS`], [`ASIDE_DISALLOWED_TOOLS`] denied, no MCP servers, and neither
/// `allowed_tools` nor the configured args (which could widen permissions). No settings file
/// loads (`--setting-sources ""`): the user's hooks could run commands and the project and local
/// settings come from the repo. The run keeps its auth and model through `--settings` with only
/// the user's `env`, `apiKeyHelper` and `model` ([`aside_settings`]), added in `run`.
pub fn permission_args(spec: &RunSpec) -> Vec<String> {
    let mut args = vec!["--permission-mode".to_string()];
    if spec.aside {
        args.extend(
            [
                "plan",
                "--tools",
                ASIDE_TOOLS,
                "--disallowedTools",
                ASIDE_DISALLOWED_TOOLS,
                "--strict-mcp-config",
                "--mcp-config",
                r#"{"mcpServers":{}}"#,
                "--setting-sources",
                "",
            ]
            .map(String::from),
        );
        return args;
    }
    // An AI member has every permission: `-p` has no one to answer a permission prompt, so a
    // run asks for nothing. `permission_mode` and `allowed_tools` in `daemon.toml` are ignored
    // (old configs still parse); `chat-run` is listed too, it is how the run posts cards and 产出.
    args.push(PERMISSION_MODE.into());
    args.push("--allowedTools".into());
    args.push(DEFAULT_ALLOWED_TOOLS.to_string());
    args.extend(spec.cfg.args.iter().cloned());
    args
}

/// `--model` (the assignment's, else `daemon.toml`'s) and `--effort`, each value one argument.
pub fn model_args(spec: &RunSpec) -> Vec<String> {
    let mut args = vec![];
    if let Some(m) = spec.model.clone().or(spec.cfg.model.clone()) {
        args.push("--model".into());
        args.push(m);
    }
    if let Some(e) = &spec.effort {
        args.push("--effort".into());
        args.push(e.clone());
    }
    args
}

/// The permission mode of every run that is not an aside: everything allowed.
pub const PERMISSION_MODE: &str = "bypassPermissions";

/// The tools every run that is not an aside may use without asking.
pub const DEFAULT_ALLOWED_TOOLS: &str =
    "Bash Edit Write MultiEdit NotebookEdit WebFetch WebSearch TodoWrite Task Bash(chat-run:*)";

/// The `chat-run` command, allowed in every non-aside run.
pub const CHAT_RUN_TOOL: &str = "Bash(chat-run:*)";

/// Removes a file when dropped.
struct RemoveOnDrop(PathBuf);

impl Drop for RemoveOnDrop {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
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

async fn run(spec: RunSpec, emit: Emit, mut control: mpsc::UnboundedReceiver<Control>) -> Outcome {
    let config_dir = config_dir(&spec);
    let resume = spec
        .resume_session_id
        .clone()
        .filter(|s| prepare_resume(&config_dir, &spec.cwd, s));
    let mut args: Vec<String> = [
        "-p",
        "--input-format",
        "stream-json",
        "--output-format",
        "stream-json",
        "--verbose",
        "--include-partial-messages",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect();
    let session_id = match resume {
        Some(sid) => {
            args.push("--resume".into());
            args.push(sid.clone());
            sid
        }
        None => {
            let sid = uuid_v4();
            args.push("--session-id".into());
            args.push(sid.clone());
            sid
        }
    };
    args.extend(permission_args(&spec));
    let aside_settings = if spec.aside {
        write_aside_settings(&spec)
    } else {
        None
    };
    // Gated actions: a PreToolUse hook asks the daemon before each command (never for a side
    // chat, which has no command tool).
    // A run that should stop at gated commands never runs without the hook.
    let gate_settings = match (&spec.gate_hook, spec.aside) {
        (Some(link), false) => match write_gate_settings(&spec, link) {
            Ok(p) => Some(p),
            Err(e) => return Outcome::failed(format!("cannot install the approval hook: {e}")),
        },
        _ => None,
    };
    if let Some(p) = aside_settings.as_ref().or(gate_settings.as_ref()) {
        args.push("--settings".into());
        args.push(p.display().to_string());
    }
    if let Some(n) = &spec.note {
        args.push("--append-system-prompt".into());
        args.push(n.clone());
    }
    args.extend(model_args(&spec));
    let program = spec.cfg.command();
    // The settings file holds auth values: it goes when the run ends, however it ends.
    let _settings = aside_settings.map(RemoveOnDrop);
    let _gate_settings = gate_settings.map(RemoveOnDrop);

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
            Some(format!(
                "claude exited before the turn ended: {}",
                p.stderr_tail()
            ))
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
