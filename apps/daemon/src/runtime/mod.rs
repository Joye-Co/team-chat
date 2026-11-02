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
use crate::proto::{RunEvent, Usage};
use serde_json::Value;
use std::future::Future;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::process::Stdio;
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin, ChildStdout};
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
    /// The CLI's working directory (worktree or scratch folder).
    pub cwd: PathBuf,
    /// The run folder: raw stream and stderr are written here.
    pub run_dir: PathBuf,
    /// The complete environment of the CLI process.
    pub env: Vec<(String, String)>,
}

impl RunSpec {
    pub fn env_var(&self, k: &str) -> Option<&str> {
        self.env
            .iter()
            .find(|(n, _)| n == k)
            .map(|(_, v)| v.as_str())
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

/// A CLI child in its own process group that speaks JSON lines on stdio.
pub struct Proc {
    child: Child,
    pgid: i32,
    stdin: Option<ChildStdin>,
    lines: tokio::io::Lines<BufReader<ChildStdout>>,
    raw: Option<std::fs::File>,
    run_dir: PathBuf,
}

impl Proc {
    pub fn spawn(spec: &RunSpec, program: &str, args: &[String]) -> std::io::Result<Proc> {
        let stderr = std::fs::File::create(spec.run_dir.join("stderr.log"))?;
        let raw = std::fs::File::create(spec.run_dir.join("stream.jsonl"))?;
        let mut cmd = tokio::process::Command::new(program);
        cmd.args(args)
            .current_dir(&spec.cwd)
            .env_clear()
            .envs(spec.env.iter().map(|(k, v)| (k, v)))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::from(stderr))
            .process_group(0)
            .kill_on_drop(true);
        let mut child = cmd.spawn()?;
        let pgid = child.id().map(|p| p as i32).unwrap_or(0);
        let stdin = child.stdin.take();
        let stdout = child.stdout.take().expect("piped stdout");
        Ok(Proc {
            child,
            pgid,
            stdin,
            lines: BufReader::with_capacity(1 << 16, stdout).lines(),
            raw: Some(raw),
            run_dir: spec.run_dir.clone(),
        })
    }

    pub fn pid(&self) -> i32 {
        self.pgid
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
        if self.pgid > 0 {
            // SAFETY: plain syscall; a negative pid addresses the child's process group.
            unsafe {
                libc::kill(-self.pgid, sig);
            }
        }
    }

    /// SIGTERM the process group, then SIGKILL after `grace`. Returns the exit code.
    pub async fn kill_group(&mut self, grace: Duration) -> Option<i64> {
        self.signal_group(libc::SIGTERM);
        match tokio::time::timeout(grace, self.child.wait()).await {
            Ok(Ok(s)) => {
                self.signal_group(libc::SIGKILL);
                s.code().map(i64::from)
            }
            _ => {
                self.signal_group(libc::SIGKILL);
                self.child
                    .wait()
                    .await
                    .ok()
                    .and_then(|s| s.code().map(i64::from))
            }
        }
    }

    /// Wait for the CLI to exit by itself for up to `limit`, then kill the group.
    pub async fn finish(&mut self, limit: Duration) -> Option<i64> {
        self.close_stdin();
        match tokio::time::timeout(limit, self.child.wait()).await {
            Ok(Ok(s)) => {
                // Remove helpers the CLI left in its group.
                self.signal_group(libc::SIGKILL);
                s.code().map(i64::from)
            }
            _ => self.kill_group(Duration::from_secs(3)).await,
        }
    }

    pub fn stderr_tail(&self) -> String {
        stderr_tail(&self.run_dir)
    }
}

impl Drop for Proc {
    fn drop(&mut self) {
        self.signal_group(libc::SIGKILL);
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

/// A random UUID v4 string.
pub fn uuid_v4() -> String {
    let mut b = [0u8; 16];
    if let Ok(mut f) = std::fs::File::open("/dev/urandom") {
        use std::io::Read;
        let _ = f.read_exact(&mut b);
    }
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
        use crate::proto::{MAX_PLAN_ITEMS, MAX_PLAN_TEXT_CHARS, RunPlanItem, RunPlanState};
        let plan: Vec<RunPlanItem> = items
            .into_iter()
            .filter_map(|(text, status)| {
                let state = match status {
                    "completed" | "done" => RunPlanState::Done,
                    "in_progress" | "inProgress" | "in-progress" => RunPlanState::Current,
                    "pending" => RunPlanState::Pending,
                    _ => return None,
                };
                let text: String = text.trim().chars().take(MAX_PLAN_TEXT_CHARS).collect();
                (!text.is_empty()).then_some(RunPlanItem { text, state })
            })
            .take(MAX_PLAN_ITEMS)
            .collect();
        if self.last.as_ref() == Some(&plan) {
            return None;
        }
        self.last = Some(plan.clone());
        Some(RunEvent::Plan { items: plan })
    }
}
