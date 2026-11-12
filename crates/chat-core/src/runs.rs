//! Bot runs: the run log for the process view, and owner admin of bot runners and
//! machines (docs/protocol.md, "Bot runs").

use crate::http::{ApiError, ApiResult, HttpClient, enc};
use crate::proto::machine::{FileChange, RunEvent, RunLogEntry};
use crate::proto::{
    BotRun, BotRunner, ChainStopResponse, CreateMachineRequest, CreateMachineResponse,
    DecisionsResponse, EndRunRequest, MAX_DECISIONS_PAGE, Machine, RunEventsResponse,
    SetBotRunnerRequest, SetMachineBotRequest, SetRunPlanRequest, StartRunRequest, TopicRelay,
    UpdateMachineRequest,
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

    /// Usable, a person in the group: stops a relay (按顺序做) and the run of its current step
    /// (docs/protocol.md, "Participants, chains and relays"). Answers with the relay;
    /// `conflict` when it had already ended.
    pub async fn stop_relay(&self, group_id: &str, relay_id: &str) -> ApiResult<TopicRelay> {
        self.call::<(), _>(
            Method::POST,
            &format!("/v1/groups/{}/relays/{}/stop", enc(group_id), enc(relay_id)),
            None,
            true,
        )
        .await
    }

    /// Usable, a person in the group: starts a paused relay again from the step it stopped at.
    /// Answers with the relay; `conflict` when it is not paused.
    pub async fn retry_relay(&self, group_id: &str, relay_id: &str) -> ApiResult<TopicRelay> {
        self.call::<(), _>(
            Method::POST,
            &format!(
                "/v1/groups/{}/relays/{}/retry",
                enc(group_id),
                enc(relay_id)
            ),
            None,
            true,
        )
        .await
    }

    /// Usable, a person in the group: stops a chain of hand-overs and every run in it that has
    /// not ended (全部停止).
    pub async fn stop_chain(&self, group_id: &str, chain_id: &str) -> ApiResult<ChainStopResponse> {
        self.call::<(), _>(
            Method::POST,
            &format!("/v1/groups/{}/chains/{}/stop", enc(group_id), enc(chain_id)),
            None,
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
    /// 1.1 重试: a new run with the same request (`POST /v1/runs/:id/retry`); the answer has
    /// `retryOf`.
    pub async fn retry_run(&self, run_id: &str) -> ApiResult<BotRun> {
        self.call::<(), _>(
            Method::POST,
            &format!("/v1/runs/{}/retry", enc(run_id)),
            None,
            true,
        )
        .await
    }

    pub async fn pause_run(&self, run_id: &str) -> ApiResult<BotRun> {
        self.call::<(), _>(
            Method::POST,
            &format!("/v1/runs/{}/pause", enc(run_id)),
            None,
            true,
        )
        .await
    }

    /// 1.1: one page of a group's decisions (群里的决定, `GET /v1/groups/:id/decisions`),
    /// newest first: answers to v2 asks with `record`, as the card was when it was answered.
    /// `before_seq` pages back; `limit` is 1..=200.
    pub async fn group_decisions(
        &self,
        group_id: &str,
        before_seq: Option<u64>,
        limit: u32,
    ) -> ApiResult<DecisionsResponse> {
        let mut path = format!(
            "/v1/groups/{}/decisions?limit={}",
            enc(group_id),
            limit.clamp(1, MAX_DECISIONS_PAGE as u32)
        );
        if let Some(seq) = before_seq {
            path.push_str(&format!("&beforeSeq={seq}"));
        }
        self.call::<(), _>(Method::GET, &path, None, true).await
    }

    /// 1.1: 记一条, records a decision by hand (`POST /v1/groups/:id/decisions`).
    pub async fn record_decision(
        &self,
        group_id: &str,
        req: &crate::proto::RecordDecisionRequest,
    ) -> ApiResult<crate::proto::RecordDecisionResponse> {
        self.call(
            Method::POST,
            &format!("/v1/groups/{}/decisions", enc(group_id)),
            Some(req),
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

    /// Owner: what the run's prompt carried (docs/protocol.md, "Run context"). 404 for older runs.
    pub async fn run_context(&self, run_id: &str) -> ApiResult<crate::proto::RunContext> {
        self.call::<(), _>(
            Method::GET,
            &format!("/v1/runs/{}/context", enc(run_id)),
            None,
            true,
        )
        .await
    }

    /// Owner: the front desk's decisions in a topic, newest first.
    pub async fn bot_decisions(
        &self,
        bot_id: &str,
        topic_id: &str,
    ) -> ApiResult<crate::proto::FrontDeskDecisionsResponse> {
        self.call::<(), _>(
            Method::GET,
            &format!(
                "/v1/bots/{}/decisions?topicId={}",
                enc(bot_id),
                enc(topic_id)
            ),
            None,
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

    /// Owner. The monitor of one machine (docs/protocol.md, "Machine monitor"): `range` is
    /// "15m", "1h" or "24h", `day_start` the viewer's local midnight in ms.
    pub async fn machine_monitor(
        &self,
        machine_id: &str,
        range: &str,
        day_start: u64,
    ) -> ApiResult<crate::proto::MachineMonitor> {
        self.call::<(), _>(
            Method::GET,
            &format!(
                "/v1/machines/{}/monitor?range={}&dayStart={day_start}",
                enc(machine_id),
                enc(range)
            ),
            None,
            true,
        )
        .await
    }

    /// Owner (feature "accounts"): starts a CLI sign-in on the machine; answers its id.
    pub async fn start_machine_login(&self, machine_id: &str, runtime: &str) -> ApiResult<String> {
        let v: serde_json::Value = self
            .call(
                Method::POST,
                &format!("/v1/machines/{}/logins", enc(machine_id)),
                Some(&crate::proto::monitor::StartLoginRequest {
                    runtime: runtime.to_string(),
                }),
                true,
            )
            .await?;
        v.get("loginId")
            .and_then(|x| x.as_str())
            .map(str::to_string)
            .ok_or_else(|| ApiError::Invalid("no loginId".into()))
    }

    /// Owner: where a sign-in stands.
    pub async fn machine_login(
        &self,
        machine_id: &str,
        login_id: &str,
    ) -> ApiResult<crate::proto::monitor::MachineLogin> {
        self.call::<(), _>(
            Method::GET,
            &format!("/v1/machines/{}/logins/{}", enc(machine_id), enc(login_id)),
            None,
            true,
        )
        .await
    }

    /// Owner: the code the sign-in page showed, for the CLI on the machine.
    pub async fn machine_login_code(
        &self,
        machine_id: &str,
        login_id: &str,
        code: &str,
    ) -> ApiResult<()> {
        let _: serde_json::Value = self
            .call(
                Method::POST,
                &format!(
                    "/v1/machines/{}/logins/{}/code",
                    enc(machine_id),
                    enc(login_id)
                ),
                Some(&crate::proto::monitor::LoginCodeRequest {
                    code: code.to_string(),
                }),
                true,
            )
            .await?;
        Ok(())
    }

    /// Owner: stops a sign-in.
    pub async fn cancel_machine_login(&self, machine_id: &str, login_id: &str) -> ApiResult<()> {
        let _: serde_json::Value = self
            .call::<(), _>(
                Method::DELETE,
                &format!("/v1/machines/{}/logins/{}", enc(machine_id), enc(login_id)),
                None,
                true,
            )
            .await?;
        Ok(())
    }

    /// Owner: the machine's public key and the keys it holds (names and last 4).
    pub async fn machine_secrets(
        &self,
        machine_id: &str,
    ) -> ApiResult<crate::proto::monitor::MachineSecretsResponse> {
        self.call::<(), _>(
            Method::GET,
            &format!("/v1/machines/{}/secrets", enc(machine_id)),
            None,
            true,
        )
        .await
    }

    /// Owner: gives the machine an API key. The value is sealed to `public_key` here, so only
    /// the machine can open it.
    pub async fn set_machine_secret(
        &self,
        machine_id: &str,
        public_key: &str,
        name: &str,
        value: &str,
        members: Vec<String>,
    ) -> ApiResult<()> {
        let sealed = seal_secret(public_key, value).map_err(ApiError::Invalid)?;
        let _: serde_json::Value = self
            .call(
                Method::PUT,
                &format!("/v1/machines/{}/secrets/{}", enc(machine_id), enc(name)),
                Some(&crate::proto::monitor::SetSecretRequest { sealed, members }),
                true,
            )
            .await?;
        Ok(())
    }

    /// Owner: removes an API key from the machine.
    pub async fn delete_machine_secret(&self, machine_id: &str, name: &str) -> ApiResult<()> {
        let _: serde_json::Value = self
            .call::<(), _>(
                Method::DELETE,
                &format!("/v1/machines/{}/secrets/{}", enc(machine_id), enc(name)),
                None,
                true,
            )
            .await?;
        Ok(())
    }

    /// Owner: when members joined, who added them, devices and open invites.
    pub async fn team_roster(&self) -> ApiResult<crate::proto::TeamRoster> {
        self.call::<(), _>(Method::GET, "/v1/team/roster", None, true)
            .await
    }

    /// Owner: 用量 of AI members' runs that ended in [from, to) (docs/protocol.md "Team usage").
    pub async fn team_usage(&self, from: u64, to: u64) -> ApiResult<crate::proto::TeamUsage> {
        self.call::<(), _>(
            Method::GET,
            &format!("/v1/usage?from={from}&to={to}"),
            None,
            true,
        )
        .await
    }

    /// Owner: 活动记录, newest first (docs/protocol.md "Activity"). `kinds` empty = every kind.
    pub async fn activity(
        &self,
        before: Option<u64>,
        actor: Option<&str>,
        kinds: &[String],
        machine: Option<&str>,
    ) -> ApiResult<crate::proto::ActivityResponse> {
        let mut q = vec!["limit=100".to_string()];
        if let Some(b) = before {
            q.push(format!("before={b}"));
        }
        if let Some(a) = actor {
            q.push(format!("actor={}", enc(a)));
        }
        if !kinds.is_empty() {
            q.push(format!("kind={}", enc(&kinds.join(","))));
        }
        if let Some(m) = machine {
            q.push(format!("machine={}", enc(m)));
        }
        self.call::<(), _>(
            Method::GET,
            &format!("/v1/activity?{}", q.join("&")),
            None,
            true,
        )
        .await
    }

    /// Usable: renames a machine, or moves it between the team and me (`PATCH /v1/machines/:id`;
    /// a team machine needs an admin, a personal one its owner). Answers the machine as I see it.
    pub async fn update_machine(
        &self,
        machine_id: &str,
        req: &UpdateMachineRequest,
    ) -> ApiResult<Machine> {
        req.validate()
            .map_err(|e| ApiError::Invalid(e.to_string()))?;
        self.call(
            Method::PATCH,
            &format!("/v1/machines/{}", enc(machine_id)),
            Some(req),
            true,
        )
        .await
    }

    /// Usable, the owner of a personal machine: my work for AI member `bot_id` runs on it
    /// (`PUT /v1/machines/:id/bots/:botId`), with my code folder there.
    pub async fn set_machine_bot(
        &self,
        machine_id: &str,
        bot_id: &str,
        req: &SetMachineBotRequest,
    ) -> ApiResult<Machine> {
        req.validate()
            .map_err(|e| ApiError::Invalid(e.to_string()))?;
        self.call(
            Method::PUT,
            &format!("/v1/machines/{}/bots/{}", enc(machine_id), enc(bot_id)),
            Some(req),
            true,
        )
        .await
    }

    /// Usable, the owner of a personal machine: my work for `bot_id` no longer runs on it.
    pub async fn delete_machine_bot(&self, machine_id: &str, bot_id: &str) -> ApiResult<Machine> {
        self.call::<(), _>(
            Method::DELETE,
            &format!("/v1/machines/{}/bots/{}", enc(machine_id), enc(bot_id)),
            None,
            true,
        )
        .await
    }

    /// Revokes the machine token and closes its link (a team machine: an admin; a personal one:
    /// its owner).
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
            RunEvent::InputAccepted { .. }
            | RunEvent::Plan { .. }
            | RunEvent::Output { .. }
            // usable: GitHub use and a person's take-over commands (shown by later work)
            | RunEvent::Github { .. }
            | RunEvent::HandCommand { .. }
            | RunEvent::HandOutput { .. }
            | RunEvent::HandResult { .. }
            | RunEvent::HandSummary(_)
            | RunEvent::Unknown => {}
        }
    }
}

/// Seals `value` to a machine's X25519 public key (base64 of 32 bytes) as a libsodium sealed
/// box, base64-encoded: what `PUT /v1/machines/:id/secrets/:name` takes.
pub fn seal_secret(public_key_b64: &str, value: &str) -> Result<String, String> {
    use base64::Engine as _;
    let b64 = base64::engine::general_purpose::STANDARD;
    let raw: [u8; 32] = b64
        .decode(public_key_b64.trim())
        .ok()
        .and_then(|v| v.try_into().ok())
        .ok_or("the machine key is not 32 bytes of base64")?;
    let sealed = crypto_box::PublicKey::from(raw)
        .seal(&mut crypto_box::aead::OsRng, value.as_bytes())
        .map_err(|_| "cannot seal the key".to_string())?;
    Ok(b64.encode(sealed))
}

#[cfg(test)]
mod seal_tests {
    #[test]
    fn a_sealed_value_opens_with_the_machine_secret_key() {
        use base64::Engine as _;
        let secret = crypto_box::SecretKey::from([7u8; 32]);
        let public =
            base64::engine::general_purpose::STANDARD.encode(secret.public_key().as_bytes());
        let sealed = super::seal_secret(&public, "sk-test-0123456789").unwrap();
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(sealed)
            .unwrap();
        assert_eq!(secret.unseal(&bytes).unwrap(), b"sk-test-0123456789");
        assert!(super::seal_secret("short", "x").is_err());
    }
}
