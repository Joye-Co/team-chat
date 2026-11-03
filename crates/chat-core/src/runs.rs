//! Bot runs: the run log for the process view, and owner admin of bot runners and
//! machines (docs/protocol.md, "Bot runs").

use crate::http::{ApiError, ApiResult, HttpClient, enc};
use crate::proto::machine::{FileChange, RunEvent, RunLogEntry};
use crate::proto::{
    BotRun, BotRunner, CreateMachineRequest, CreateMachineResponse, EndRunRequest, Machine,
    RunEventsResponse, SetBotRunnerRequest, SetRunPlanRequest, StartRunRequest,
};
use reqwest::Method;
use std::collections::HashMap;

impl HttpClient {
    pub async fn run(&self, run_id: &str) -> ApiResult<BotRun> {
        self.call::<(), _>(
            Method::GET,
            &format!("/v1/runs/{}", enc(run_id)),
            None,
            true,
        )
        .await
    }

    /// One page of a run log: entries with `seq > after_seq`, at most `limit` (1..=500).
    pub async fn run_events(
        &self,
        run_id: &str,
        after_seq: u64,
        limit: u32,
    ) -> ApiResult<RunEventsResponse> {
        self.call::<(), _>(
            Method::GET,
            &format!(
                "/v1/runs/{}/events?afterSeq={after_seq}&limit={}",
                enc(run_id),
                limit.clamp(1, 500)
            ),
            None,
            true,
        )
        .await
    }

    /// Bot (not runner-backed): starts a run of this bot in a topic, or returns its active
    /// run there (docs/protocol.md, "Run plans").
    pub async fn start_run(&self, req: &StartRunRequest) -> ApiResult<BotRun> {
        req.validate()
            .map_err(|e| ApiError::Invalid(e.to_string()))?;
        self.call(Method::POST, "/v1/runs", Some(req), true).await
    }

    /// The run's bot: replaces the whole plan of an active run; an empty list clears it.
    pub async fn set_run_plan(&self, run_id: &str, req: &SetRunPlanRequest) -> ApiResult<BotRun> {
        req.validate()
            .map_err(|e| ApiError::Invalid(e.to_string()))?;
        self.call(
            Method::PUT,
            &format!("/v1/runs/{}/plan", enc(run_id)),
            Some(req),
            true,
        )
        .await
    }

    /// The run's bot: ends a run it started with [`HttpClient::start_run`].
    pub async fn end_run(&self, run_id: &str, req: &EndRunRequest) -> ApiResult<BotRun> {
        req.validate()
            .map_err(|e| ApiError::Invalid(e.to_string()))?;
        self.call(
            Method::POST,
            &format!("/v1/runs/{}/end", enc(run_id)),
            Some(req),
            true,
        )
        .await
    }

    /// A person in the run's group: stops an active run (docs/protocol.md, "Bot runs"). The
    /// machine gets `run.cancel` and the run ends as `cancelled` at once. Answers with the
    /// ended run; `conflict` (409) when the run had already ended.
    pub async fn cancel_run(&self, run_id: &str) -> ApiResult<BotRun> {
        self.call::<(), _>(
            Method::POST,
            &format!("/v1/runs/{}/cancel", enc(run_id)),
            None,
            true,
        )
        .await
    }

    /// 1.1, the run's requester or an owner: pauses an active run for 接管 (docs/protocol.md,
    /// "Run fields, pause and resume"). Answers with the run (`pausedBy` set); `conflict` when
    /// it ended, is paused already, or its machine cannot pause.
    pub async fn pause_run(&self, run_id: &str) -> ApiResult<BotRun> {
        self.call::<(), _>(
            Method::POST,
            &format!("/v1/runs/{}/pause", enc(run_id)),
            None,
            true,
        )
        .await
    }

    /// 1.1: resumes a paused run (交还). `note` becomes the CLI's next input when set.
    /// Answers with the run (`pausedBy` cleared); `conflict` when it is not paused.
    pub async fn resume_run(&self, run_id: &str, note: Option<&str>) -> ApiResult<BotRun> {
        let body = serde_json::json!({ "note": note });
        let body = if note.is_some() {
            body
        } else {
            serde_json::json!({})
        };
        self.call(
            Method::POST,
            &format!("/v1/runs/{}/resume", enc(run_id)),
            Some(&body),
            true,
        )
        .await
    }

    /// Owner: the raw CLI session of a run (gzip). 404 until the daemon uploaded it.
    pub async fn run_session(&self, run_id: &str) -> ApiResult<Vec<u8>> {
        let resp = self
            .send_checked(
                self.authed(
                    Method::GET,
                    &self.url(&format!("/v1/runs/{}/session", enc(run_id))),
                )?
                .timeout(crate::files::TRANSFER_TIMEOUT),
            )
            .await?;
        Ok(resp.bytes().await.map_err(crate::http::redact)?.to_vec())
    }

    /// Owner.
    pub async fn set_bot_runner(
        &self,
        bot_id: &str,
        req: &SetBotRunnerRequest,
    ) -> ApiResult<BotRunner> {
        req.validate()
            .map_err(|e| ApiError::Invalid(e.to_string()))?;
        self.call(
            Method::PUT,
            &format!("/v1/bots/{}/runner", enc(bot_id)),
            Some(req),
            true,
        )
        .await
    }

    /// Owner. `None` when the bot is not runner-backed (404).
    pub async fn bot_runner(&self, bot_id: &str) -> ApiResult<Option<BotRunner>> {
        match self
            .call::<(), BotRunner>(
                Method::GET,
                &format!("/v1/bots/{}/runner", enc(bot_id)),
                None,
                true,
            )
            .await
        {
            Ok(r) => Ok(Some(r)),
            Err(ApiError::Server { status: 404, .. } | ApiError::Unexpected { status: 404 }) => {
                Ok(None)
            }
            Err(e) => Err(e),
        }
    }

    /// Owner.
    pub async fn delete_bot_runner(&self, bot_id: &str) -> ApiResult<()> {
        let _: serde_json::Value = self
            .call::<(), _>(
                Method::DELETE,
                &format!("/v1/bots/{}/runner", enc(bot_id)),
                None,
                true,
            )
            .await?;
        Ok(())
    }

    /// Owner. The response `token` is the machine's secret: never log it.
    pub async fn create_machine(
        &self,
        req: &CreateMachineRequest,
    ) -> ApiResult<CreateMachineResponse> {
        req.validate()
            .map_err(|e| ApiError::Invalid(e.to_string()))?;
        self.call(Method::POST, "/v1/machines", Some(req), true)
            .await
    }

    /// Owner.
    pub async fn machines(&self) -> ApiResult<Vec<Machine>> {
        self.call::<(), _>(Method::GET, "/v1/machines", None, true)
            .await
    }

    /// Owner. Revokes the machine token and closes its link.
    pub async fn delete_machine(&self, machine_id: &str) -> ApiResult<()> {
        let _: serde_json::Value = self
            .call::<(), _>(
                Method::DELETE,
                &format!("/v1/machines/{}", enc(machine_id)),
                None,
                true,
            )
            .await?;
        Ok(())
    }

    /// Every run log entry after `after_seq`, page by page (at most `max_pages` pages of
    /// 500). Returns the newest run record, the entries and the last seq seen.
    pub async fn run_events_all(
        &self,
        run_id: &str,
        after_seq: u64,
        max_pages: usize,
    ) -> ApiResult<(BotRun, Vec<RunLogEntry>, u64)> {
        let mut after = after_seq;
        let mut all = Vec::new();
        let mut run = None;
        for _ in 0..max_pages.max(1) {
            let page = self.run_events(run_id, after, 500).await?;
            let last = page.entries.iter().map(entry_seq).max().unwrap_or(after);
            run = Some(page.run);
            all.extend(page.entries);
            if !page.has_more || last <= after {
                after = after.max(last);
                break;
            }
            after = last;
        }
        Ok((run.ok_or(ApiError::Decode("no page".into()))?, all, after))
    }
}

/// The `seq` of a run log entry.
pub fn entry_seq(e: &RunLogEntry) -> u64 {
    match e {
        RunLogEntry::RunEvent { seq, .. } => *seq,
        RunLogEntry::RunDone(d) => d.seq,
        RunLogEntry::RunSession(s) => s.seq,
    }
}

/// One line of the live terminal (实时查看): a command a run's CLI ran, or its output. Built
/// from the run log's `tool.call` and `tool.result` events with [`terminal_lines`].
#[derive(Debug, Clone, PartialEq)]
pub enum TerminalLine {
    Command {
        seq: u64,
        at: u64,
        item_id: String,
        tool: String,
        title: String,
        input: Option<serde_json::Value>,
    },
    Output {
        seq: u64,
        at: u64,
        item_id: String,
        ok: bool,
        output: Option<String>,
        exit_code: Option<i64>,
    },
}

impl TerminalLine {
    pub fn seq(&self) -> u64 {
        match self {
            Self::Command { seq, .. } | Self::Output { seq, .. } => *seq,
        }
    }
    pub fn at(&self) -> u64 {
        match self {
            Self::Command { at, .. } | Self::Output { at, .. } => *at,
        }
    }
}

/// The terminal lines of run log entries, in seq order.
pub fn terminal_lines(entries: &[RunLogEntry]) -> Vec<TerminalLine> {
    let mut out: Vec<TerminalLine> = entries
        .iter()
        .filter_map(|e| match e {
            RunLogEntry::RunEvent { seq, at, event, .. } => match event {
                RunEvent::ToolCall {
                    item_id,
                    tool,
                    title,
                    input,
                    ..
                } => Some(TerminalLine::Command {
                    seq: *seq,
                    at: *at,
                    item_id: item_id.clone(),
                    tool: tool.clone(),
                    title: title.clone(),
                    input: input.clone(),
                }),
                RunEvent::ToolResult {
                    item_id,
                    ok,
                    output,
                    exit_code,
                    ..
                } => Some(TerminalLine::Output {
                    seq: *seq,
                    at: *at,
                    item_id: item_id.clone(),
                    ok: *ok,
                    output: output.clone(),
                    exit_code: *exit_code,
                }),
                _ => None,
            },
            _ => None,
        })
        .collect();
    out.sort_by_key(TerminalLine::seq);
    out.dedup_by_key(|l| l.seq());
    out
}

/// One row of the process view.
#[derive(Debug, Clone, PartialEq)]
pub enum ProcessItem {
    /// Assistant or user text; deltas are merged, the final message replaces them.
    Message {
        item_id: String,
        role: String,
        text: String,
        streaming: bool,
    },
    Reasoning {
        item_id: String,
        text: String,
    },
    /// A tool call with its result when it came.
    Tool {
        item_id: String,
        tool: String,
        title: String,
        input: Option<serde_json::Value>,
        ok: Option<bool>,
        output: Option<String>,
        exit_code: Option<i64>,
    },
    Files {
        item_id: String,
        changes: Vec<FileChange>,
    },
    Approval {
        approval_id: String,
        title: String,
        detail: Option<String>,
        /// "approve" | "deny" once resolved.
        decision: Option<String>,
    },
    TurnStarted {
        turn_id: String,
    },
    TurnCompleted {
        turn_id: String,
        status: String,
        error: Option<String>,
    },
    Status {
        state: String,
        message: Option<String>,
    },
    Done {
        status: String,
        summary: Option<String>,
        error: Option<String>,
        duration_ms: u64,
    },
}

/// Folds a run log into process view rows, in seq order. Feed pages with [`Self::apply`]
/// as they arrive; entries at or below `last_seq` are skipped, so a page may overlap.
/// Gaps in seq are normal (the daemon may drop deltas).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ProcessView {
    pub items: Vec<ProcessItem>,
    pub last_seq: u64,
    /// The CLI session id and runtime, from the `session` event.
    pub session: Option<(String, String)>,
    /// 1.1: the latest acceptance criteria the run reported.
    pub acceptance: Option<Vec<crate::proto::AcceptanceItem>>,
    /// 1.1: whether the CLI is paused now, from the last `paused` event, and by whom.
    pub paused: Option<(bool, Option<String>)>,
    by_item: HashMap<String, usize>,
    by_approval: HashMap<String, usize>,
}

impl ProcessView {
    pub fn apply(&mut self, entries: &[RunLogEntry]) {
        let mut sorted: Vec<&RunLogEntry> = entries.iter().collect();
        sorted.sort_by_key(|e| entry_seq(e));
        for e in sorted {
            let seq = entry_seq(e);
            if seq <= self.last_seq {
                continue;
            }
            self.last_seq = seq;
            match e {
                RunLogEntry::RunEvent { event, .. } => self.on_event(event),
                RunLogEntry::RunDone(d) => {
                    self.close_streams();
                    self.items.push(ProcessItem::Done {
                        status: d.status.clone(),
                        summary: d.summary.clone(),
                        error: d.error.clone(),
                        duration_ms: d.duration_ms,
                    });
                }
                RunLogEntry::RunSession(_) => {}
            }
        }
    }

    fn push_item(&mut self, item_id: &str, item: ProcessItem) -> usize {
        self.items.push(item);
        let i = self.items.len() - 1;
        self.by_item.insert(item_id.to_string(), i);
        i
    }

    fn close_streams(&mut self) {
        for it in &mut self.items {
            if let ProcessItem::Message { streaming, .. } = it {
                *streaming = false;
            }
        }
    }

    fn on_event(&mut self, ev: &RunEvent) {
        match ev {
            RunEvent::Session {
                session_id,
                runtime,
                ..
            } => self.session = Some((session_id.clone(), runtime.clone())),
            RunEvent::TurnStarted { turn_id } => self.items.push(ProcessItem::TurnStarted {
                turn_id: turn_id.clone(),
            }),
            RunEvent::TurnCompleted {
                turn_id,
                status,
                error,
            } => {
                self.close_streams();
                self.items.push(ProcessItem::TurnCompleted {
                    turn_id: turn_id.clone(),
                    status: status.clone(),
                    error: error.clone(),
                });
            }
            RunEvent::MessageDelta { item_id, text, .. } => match self.by_item.get(item_id) {
                Some(&i) => {
                    if let ProcessItem::Message {
                        text: t, streaming, ..
                    } = &mut self.items[i]
                        && *streaming
                    {
                        t.push_str(text);
                    }
                }
                None => {
                    self.push_item(
                        item_id,
                        ProcessItem::Message {
                            item_id: item_id.clone(),
                            role: "assistant".into(),
                            text: text.clone(),
                            streaming: true,
                        },
                    );
                }
            },
            RunEvent::Message {
                item_id,
                role,
                text,
                ..
            } => {
                let item = ProcessItem::Message {
                    item_id: item_id.clone(),
                    role: role.clone(),
                    text: text.clone(),
                    streaming: false,
                };
                match self.by_item.get(item_id) {
                    Some(&i) => self.items[i] = item,
                    None => {
                        self.push_item(item_id, item);
                    }
                }
            }
            RunEvent::Reasoning { item_id, text, .. } => match self.by_item.get(item_id) {
                Some(&i) => {
                    if let ProcessItem::Reasoning { text: t, .. } = &mut self.items[i] {
                        t.push_str(text);
                    }
                }
                None => {
                    self.push_item(
                        item_id,
                        ProcessItem::Reasoning {
                            item_id: item_id.clone(),
                            text: text.clone(),
                        },
                    );
                }
            },
            RunEvent::ToolCall {
                item_id,
                tool,
                title,
                input,
                ..
            } => {
                self.push_item(
                    item_id,
                    ProcessItem::Tool {
                        item_id: item_id.clone(),
                        tool: tool.clone(),
                        title: title.clone(),
                        input: input.clone(),
                        ok: None,
                        output: None,
                        exit_code: None,
                    },
                );
            }
            RunEvent::ToolResult {
                item_id,
                ok,
                output,
                exit_code,
                ..
            } => {
                let idx = self.by_item.get(item_id).copied();
                match idx.map(|i| &mut self.items[i]) {
                    Some(ProcessItem::Tool {
                        ok: o,
                        output: out,
                        exit_code: ec,
                        ..
                    }) => {
                        *o = Some(*ok);
                        *out = output.clone();
                        *ec = *exit_code;
                    }
                    _ => {
                        self.push_item(
                            item_id,
                            ProcessItem::Tool {
                                item_id: item_id.clone(),
                                tool: String::new(),
                                title: String::new(),
                                input: None,
                                ok: Some(*ok),
                                output: output.clone(),
                                exit_code: *exit_code,
                            },
                        );
                    }
                }
            }
            RunEvent::FileChange {
                item_id, changes, ..
            } => {
                self.push_item(
                    item_id,
                    ProcessItem::Files {
                        item_id: item_id.clone(),
                        changes: changes.clone(),
                    },
                );
            }
            RunEvent::ApprovalRequest {
                approval_id,
                title,
                detail,
                ..
            } => {
                self.items.push(ProcessItem::Approval {
                    approval_id: approval_id.clone(),
                    title: title.clone(),
                    detail: detail.clone(),
                    decision: None,
                });
                self.by_approval
                    .insert(approval_id.clone(), self.items.len() - 1);
            }
            RunEvent::ApprovalResolved {
                approval_id,
                decision,
            } => {
                if let Some(&i) = self.by_approval.get(approval_id)
                    && let ProcessItem::Approval { decision: d, .. } = &mut self.items[i]
                {
                    *d = Some(decision.clone());
                }
            }
            RunEvent::Status { state, message } => self.items.push(ProcessItem::Status {
                state: state.clone(),
                message: message.clone(),
            }),
            RunEvent::Acceptance { items } => self.acceptance = Some(items.clone()),
            RunEvent::Paused { paused, by } => self.paused = Some((*paused, by.clone())),
            RunEvent::InputAccepted { .. } | RunEvent::Plan { .. } | RunEvent::Unknown => {}
        }
    }
}
