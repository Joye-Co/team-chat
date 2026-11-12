//! Runtimes: one trait, one implementation per CLI protocol.
//!
//! A runtime starts the CLI for one run, turns the CLI's own events into [`RunEvent`]s, takes
//! follow-up input, approval answers and cancel through [`Control`], and returns an
//! [`Outcome`] when the run is over.

pub mod acp;
pub mod claude;
pub mod codex;
pub mod pty;

use crate::config::RuntimeConfig;
use crate::executor::{Executor, Process};
use crate::proto::{RunEvent, Usage};
use serde_json::Value;
use std::future::Future;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::sync::mpsc;

pub type BoxFuture<T> = Pin<Box<dyn Future<Output = T> + Send + 'static>>;
pub type Emit = mpsc::UnboundedSender<RunEvent>;

#[derive(Debug, Clone)]
pub struct RunSpec {
    pub run_id: String,
    pub runtime_name: String,
    pub cfg: RuntimeConfig,
    pub prompt: String,
    pub model: Option<String>,
    /// `run.assign.effort`, only when the CLI reported it (checked when the run is accepted):
    /// Claude Code `--effort`, Codex `turn/start.effort`. Other runtimes ignore it.
    pub effort: Option<String>,
    /// The CLI's working directory (worktree or scratch folder).
    pub cwd: PathBuf,
    /// The run folder: raw stream and stderr are written here.
    pub run_dir: PathBuf,
    /// The complete environment of the CLI process.
    pub env: Vec<(String, String)>,
    /// A read-only side chat run (`run.assign.kind = "aside"`): the runtime forces its
    /// read-only mode and ignores the configured args and tool grants.
    pub aside: bool,
    /// Run instructions for the CLI's system prompt (how to use `chat-run`), when the run has it.
    pub note: Option<String>,
    /// `run.assign.resumeSessionId`: the CLI session to continue (Claude Code `--resume`, Codex
    /// `thread/resume`). A session the CLI no longer has starts a new one.
    pub resume_session_id: Option<String>,
    /// The run's `chat-run` link when the run has gated actions (`run.assign.gates`): Claude
    /// Code gets a PreToolUse hook that runs `<link> gate` (see [`crate::gates`]).
    pub gate_hook: Option<PathBuf>,
    /// Becomes true when the CLI ran the start-up hook of the gate settings. With it, the
    /// runtime gives the CLI its prompt only after that: a CLI whose hooks are turned off
    /// never starts a gated run ([`crate::gates::claude_settings`]).
    pub gate_alive: Option<tokio::sync::watch::Receiver<bool>>,
    /// The run's gated actions. Codex and ACP agents answer their CLI's own approval requests
    /// with them ([`crate::gates::Gates::allows`]); None or empty: everything is allowed.
    pub gates: Option<Arc<crate::gates::Gates>>,
    /// More host folders a VM run needs at their host paths (the repository's cache). Only
    /// the microVM executor reads it.
    pub vm_shares: Vec<PathBuf>,
    /// Where the CLI runs.
    pub exec: Arc<dyn Executor>,
}

/// A session id the daemon passes on: 1-128 of `[A-Za-z0-9_-]` (it becomes a file name).
pub fn valid_session_id(s: &str) -> bool {
    (1..=128).contains(&s.len())
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

impl RunSpec {
    pub fn env_var(&self, k: &str) -> Option<&str> {
        self.env
            .iter()
            .find(|(n, _)| n == k)
            .map(|(_, v)| v.as_str())
    }

    /// The home folder of the run's CLI: `HOME`, or on Windows the profile folder
    /// (`USERPROFILE`) when `HOME` is not set. `/` when the run has neither.
    pub fn home(&self) -> PathBuf {
        let home = self
            .env_var("HOME")
            .or_else(|| self.env_var("USERPROFILE").filter(|_| cfg!(windows)));
        PathBuf::from(home.unwrap_or("/"))
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Control {
    Input { input_id: String, text: String },
    Approval { approval_id: String, approve: bool },
    Cancel,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    Succeeded,
    Failed,
    Cancelled,
}

impl Status {
    pub fn as_str(self) -> &'static str {
        match self {
            Status::Succeeded => "succeeded",
            Status::Failed => "failed",
            Status::Cancelled => "cancelled",
        }
    }
}

#[derive(Debug, Clone)]
pub struct Outcome {
    pub status: Status,
    pub exit_code: Option<i64>,
    pub result_text: Option<String>,
    pub error: Option<String>,
    pub session_id: Option<String>,
    /// The CLI's own raw session file, when it keeps one.
    pub session_file: Option<PathBuf>,
    pub usage: Option<Usage>,
    /// Why a failed run failed, from what the CLI reported (its typed errors); None leaves it to
    /// the error text ([`crate::failure::of_run`]).
    pub failure: Option<crate::proto::RunFailure>,
}

impl Outcome {
    pub fn failed(error: impl Into<String>) -> Outcome {
        Outcome {
            status: Status::Failed,
            exit_code: None,
            result_text: None,
            error: Some(error.into()),
            session_id: None,
            session_file: None,
            usage: None,
            failure: None,
        }
    }
}

pub trait Runtime: Send + Sync {
    /// Run the CLI until the work is done, failed or cancelled.
    fn start(
        &self,
        spec: RunSpec,
        emit: Emit,
        control: mpsc::UnboundedReceiver<Control>,
    ) -> BoxFuture<Outcome>;
}

pub fn for_kind(kind: &str) -> Option<Box<dyn Runtime>> {
    match kind {
        "codex" => Some(Box::new(codex::CodexRuntime)),
        "claude" => Some(Box::new(claude::ClaudeRuntime)),
        "acp" => Some(Box::new(acp::AcpRuntime)),
        "pty" => Some(Box::new(pty::PtyRuntime)),
        _ => None,
    }
}

/// A CLI that speaks JSON lines on stdio, started by the run's executor.
pub struct Proc {
    process: Box<dyn Process>,
    stdin: Option<crate::executor::Stdin>,
    lines: tokio::io::Lines<BufReader<crate::executor::Stdout>>,
    raw: Option<std::fs::File>,
    run_dir: PathBuf,
}

impl Proc {
    pub fn spawn(spec: &RunSpec, program: &str, args: &[String]) -> std::io::Result<Proc> {
        let raw = std::fs::File::create(spec.run_dir.join("stream.jsonl"))?;
        let s = spec.exec.spawn(spec, program, args)?;
        Ok(Proc {
            process: s.process,
            stdin: s.stdin,
            lines: BufReader::with_capacity(1 << 16, s.stdout).lines(),
            raw: Some(raw),
            run_dir: spec.run_dir.clone(),
        })
    }

    /// Next stdout line, copied to the run's stream file. None at end of output.
    pub async fn next_line(&mut self) -> Option<String> {
        match self.lines.next_line().await {
            Ok(Some(l)) => {
                if let Some(f) = self.raw.as_mut() {
                    use std::io::Write;
                    let _ = f.write_all(l.as_bytes());
                    let _ = f.write_all(b"\n");
                }
                Some(l)
            }
            _ => None,
        }
    }

    pub async fn write_json(&mut self, v: &Value) -> std::io::Result<()> {
        let Some(stdin) = self.stdin.as_mut() else {
            return Err(std::io::Error::new(
                std::io::ErrorKind::BrokenPipe,
                "stdin closed",
            ));
        };
        let mut s = serde_json::to_string(v).map_err(std::io::Error::other)?;
        s.push('\n');
        stdin.write_all(s.as_bytes()).await?;
        stdin.flush().await
    }

    pub fn close_stdin(&mut self) {
        self.stdin = None;
    }

    pub fn signal_group(&self, sig: i32) {
        self.process.signal(sig);
    }

    /// SIGTERM the CLI and the tools below it, then SIGKILL after `grace`. Returns the exit
    /// code.
    pub async fn kill_group(&mut self, grace: Duration) -> Option<i64> {
        self.process.kill(grace).await
    }

    /// Wait for the CLI to exit by itself for up to `limit`, then kill the group.
    pub async fn finish(&mut self, limit: Duration) -> Option<i64> {
        self.close_stdin();
        self.process.finish(limit).await
    }

    pub fn stderr_tail(&self) -> String {
        stderr_tail(&self.run_dir)
    }
}

impl Drop for Proc {
    fn drop(&mut self) {
        self.process.kill_now();
    }
}

pub fn stderr_tail(run_dir: &Path) -> String {
    let s = std::fs::read(run_dir.join("stderr.log")).unwrap_or_default();
    let start = s.len().saturating_sub(2000);
    String::from_utf8_lossy(&s[start..]).trim().to_string()
}

/// Run-scoped helper for JSON-RPC 2.0 style peers (codex app-server, ACP).
#[derive(Default)]
pub struct RpcIds {
    next: u64,
}

impl RpcIds {
    pub fn next_id(&mut self) -> u64 {
        self.next += 1;
        self.next
    }
}

pub enum Rpc<'a> {
    Response {
        id: &'a Value,
        result: Option<&'a Value>,
        error: Option<&'a Value>,
    },
    Request {
        id: &'a Value,
        method: &'a str,
        params: &'a Value,
    },
    Notification {
        method: &'a str,
        params: &'a Value,
    },
    Other,
}

pub fn classify(v: &Value) -> Rpc<'_> {
    static NULL: Value = Value::Null;
    let method = v.get("method").and_then(Value::as_str);
    let id = v.get("id");
    match (method, id) {
        (Some(m), Some(id)) => Rpc::Request {
            id,
            method: m,
            params: v.get("params").unwrap_or(&NULL),
        },
        (Some(m), None) => Rpc::Notification {
            method: m,
            params: v.get("params").unwrap_or(&NULL),
        },
        (None, Some(id)) => Rpc::Response {
            id,
            result: v.get("result"),
            error: v.get("error"),
        },
        _ => Rpc::Other,
    }
}

pub fn rpc_error_text(e: &Value) -> String {
    e.get("message")
        .and_then(Value::as_str)
        .map(str::to_string)
        .unwrap_or_else(|| e.to_string())
}

/// Text form of a JSON value for tool output.
pub fn value_text(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Array(items) => items
            .iter()
            .map(|i| match i.get("text").and_then(Value::as_str) {
                Some(t) => t.to_string(),
                None => value_text(i),
            })
            .collect::<Vec<_>>()
            .join("\n"),
        Value::Null => String::new(),
        other => other.to_string(),
    }
}

pub const OUTPUT_LIMIT: usize = 16 * 1024;
/// The end of a long tool output that [`truncate_output`] keeps.
const OUTPUT_TAIL: usize = 4 * 1024;

/// A tool result's output within [`OUTPUT_LIMIT`] bytes: its start and its last 4 KiB, with the
/// cut marked between them. A test runner prints its summary at the end, and the server reads
/// the test counts of a run from it (接 5.1).
pub fn truncate_output(s: &str) -> String {
    if s.len() <= OUTPUT_LIMIT {
        return s.to_string();
    }
    let mut head = OUTPUT_LIMIT - OUTPUT_TAIL;
    while !s.is_char_boundary(head) {
        head -= 1;
    }
    let mut tail = s.len() - OUTPUT_TAIL;
    while !s.is_char_boundary(tail) {
        tail += 1;
    }
    format!(
        "{}\n[... {} bytes cut]\n{}",
        &s[..head],
        tail - head,
        &s[tail..]
    )
}

/// A random UUID v4 string.
pub fn uuid_v4() -> String {
    use crypto_box::aead::rand_core::RngCore as _;
    let mut b = [0u8; 16];
    // The system's random source on every system (not `/dev/urandom`, which Windows has not:
    // every session there had the same id).
    crypto_box::aead::OsRng.fill_bytes(&mut b);
    b[6] = (b[6] & 0x0f) | 0x40;
    b[8] = (b[8] & 0x3f) | 0x80;
    let h: String = b.iter().map(|x| format!("{x:02x}")).collect();
    format!(
        "{}-{}-{}-{}-{}",
        &h[0..8],
        &h[8..12],
        &h[12..16],
        &h[16..20],
        &h[20..32]
    )
}

/// Maps a CLI's own to-do list to the run plan (docs/machine-protocol.md, `plan`): each text
/// trimmed and cut to 200 characters, empty texts and unknown states dropped, at most 30 items.
/// Remembers the last plan sent, so an unchanged list sends nothing.
#[derive(Debug, Default)]
pub struct PlanTracker {
    last: Option<Vec<crate::proto::RunPlanItem>>,
}

impl PlanTracker {
    /// `items`: (text, provider status). Returns the `plan` event when the plan changed.
    pub fn update<'a>(
        &mut self,
        items: impl IntoIterator<Item = (&'a str, &'a str)>,
    ) -> Option<RunEvent> {
        self.update_at(items, crate::proto::now_ms())
    }

    /// [`PlanTracker::update`] at `now` (ms): an item that becomes `current` gets `startedAt`,
    /// one that becomes `done` gets `endedAt`; both stay while the item's text and state stay.
    pub fn update_at<'a>(
        &mut self,
        items: impl IntoIterator<Item = (&'a str, &'a str)>,
        now: u64,
    ) -> Option<RunEvent> {
        use crate::proto::{MAX_PLAN_ITEMS, MAX_PLAN_TEXT_CHARS, RunPlanItem, RunPlanState};
        let prev = self.last.take();
        let last = prev.as_deref().unwrap_or_default();
        let mut plan: Vec<RunPlanItem> = items
            .into_iter()
            .filter_map(|(text, status)| {
                let state = match status {
                    "completed" | "done" => RunPlanState::Done,
                    "in_progress" | "inProgress" | "in-progress" => RunPlanState::Current,
                    "pending" => RunPlanState::Pending,
                    _ => return None,
                };
                let text: String = text.trim().chars().take(MAX_PLAN_TEXT_CHARS).collect();
                (!text.is_empty()).then(|| RunPlanItem::new(text, state))
            })
            .take(MAX_PLAN_ITEMS)
            .collect();
        for item in &mut plan {
            let before = last.iter().find(|p| p.text == item.text);
            let started = before.and_then(|p| p.started_at);
            match item.state {
                RunPlanState::Current => item.started_at = started.or(Some(now)),
                RunPlanState::Done => {
                    item.started_at = started;
                    item.ended_at = match before {
                        Some(p) if p.state == RunPlanState::Done => p.ended_at,
                        _ => Some(now),
                    };
                }
                _ => {}
            }
        }
        let same = prev.is_some()
            && last.len() == plan.len()
            && last
                .iter()
                .zip(&plan)
                .all(|(a, b)| a.text == b.text && a.state == b.state);
        self.last = Some(plan.clone());
        if same {
            return None;
        }
        Some(RunEvent::Plan { items: plan })
    }
}

#[cfg(test)]
mod output_tests {
    use super::*;

    #[test]
    fn a_long_tool_output_keeps_its_start_and_its_end() {
        let short = "ok\n12 passed".to_string();
        assert_eq!(truncate_output(&short), short);
        let long = format!(
            "start\n{}\ntest result: ok. 12 passed; 0 failed",
            "日志".repeat(20_000)
        );
        let cut = truncate_output(&long);
        assert!(cut.len() <= OUTPUT_LIMIT + 64, "{}", cut.len());
        assert!(cut.starts_with("start\n"));
        assert!(cut.ends_with("test result: ok. 12 passed; 0 failed"));
        assert!(cut.contains(" bytes cut]"));
    }
}
