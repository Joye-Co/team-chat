//! The two right panes on chat-core (panes track, 1.1.0):
//!
//! - 单独问: a side chat is a topic in my DM with the bot whose `contextOf` names the source
//!   topic (`Client::start_side_chat`, `reply_in_side_chat`, `side_chats`). A suggestion is
//!   the first `draft` block of the bot's Card v2 answer; pressing it sends the text into the
//!   source topic as my own message.
//! - 实时查看: the terminal is rebuilt from the run log (`tool.call` / `tool.result`), and the
//!   run's `plan` events give each step its start time (回放). 验收标准 and `pausedBy` come from
//!   the cached `BotRun`; 接管 / 交还 are `Client::pause_run` / `resume_run`. The machine's
//!   `takeoverUrl` comes from the machine list (owners and run requesters only).

use std::{
    collections::HashMap,
    sync::Mutex,
    time::{Duration, Instant},
};

use chat_core::{
    ClientError, SideChatRequest,
    proto::{
        AcceptanceState, Message, RunPlanState, TopicContext,
        CardV2Block,
        machine::{RunEvent, RunLogEntry},
    },
};
use futures::channel::oneshot;

use super::{CoreProvider, lock};
use crate::data::{
    AdminError, AdminReply, Change, Millis, SendRequest,
    v2::{
        AcceptanceItem, MachineScreen, ScreenLine, ScreenLineKind, SideChatMessage,
        SideChatSuggestion, SideChatThread,
    },
};

/// A running run's log is read again at most this often while the pane asks for it.
const LIVE_REFRESH: Duration = Duration::from_millis(1500);
/// Lines of one tool output kept on the screen.
const OUTPUT_LINES: usize = 12;
/// Screen lines kept per run.
const SCREEN_KEEP: usize = 400;
/// Messages of a side chat read from the cache.
const SIDE_CHAT_LIMIT: usize = 200;
/// The machine list is read again at most this often (for `takeoverUrl`).
const MACHINES_REFRESH: Duration = Duration::from_secs(60);

/// Per-account state of the two panes; cleared when the account changes.
#[derive(Default)]
pub(super) struct PanesCache {
    inner: Mutex<Inner>,
}

#[derive(Default)]
struct Inner {
    generation: u64,
    live: HashMap<String, LiveEntry>,
    /// Machine id -> screen sharing address.
    takeover: HashMap<String, String>,
    machines_at: Option<Instant>,
    machines_loading: bool,
    /// (group, topic, member) -> my first questions not in the cache yet: (clientMsgId, body, at).
    pending: HashMap<(String, String, String), Vec<(String, String, Millis)>>,
    /// Side chat message id -> when I posted its suggestion into the topic.
    posted: HashMap<String, Millis>,
}

impl Inner {
    fn for_generation(&mut self, generation: u64) -> &mut Self {
        if self.generation != generation {
            *self = Inner {
                generation,
                ..Inner::default()
            };
        }
        self
    }
}

#[derive(Default)]
struct LiveEntry {
    after: u64,
    lines: Vec<ScreenLine>,
    /// Start time of each plan step, by index.
    step_times: Vec<Option<Millis>>,
    /// The plan step the run is on now.
    step: Option<usize>,
    loading: bool,
    fetched_at: Option<Instant>,
}

fn now_ms() -> Millis {
    chrono::Utc::now().timestamp_millis()
}

/// The command a `tool.call` ran, as the terminal prints it.
fn command_text(tool: &str, title: &str, input: Option<&serde_json::Value>) -> String {
    let from_input = input.and_then(|v| {
        ["command", "cmd", "script"]
            .iter()
            .find_map(|k| v.get(k))
            .and_then(|c| match c {
                serde_json::Value::String(s) => Some(s.clone()),
                serde_json::Value::Array(a) => Some(
                    a.iter()
                        .filter_map(|x| x.as_str())
                        .collect::<Vec<_>>()
                        .join(" "),
                ),
                _ => None,
            })
    });
    let text = from_input
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| {
            if title.trim().is_empty() {
                tool.to_string()
            } else {
                title.to_string()
            }
        });
    text.lines().next().unwrap_or_default().to_string()
}

impl LiveEntry {
    /// Folds new run log entries into the screen and the step times.
    fn apply(&mut self, entries: &[RunLogEntry]) {
        let mut sorted: Vec<&RunLogEntry> = entries
            .iter()
            .filter(|e| chat_core::runs::entry_seq(e) > self.after)
            .collect();
        sorted.sort_by_key(|e| chat_core::runs::entry_seq(e));
        for e in sorted {
            let RunLogEntry::RunEvent { at, event, .. } = e else {
                continue;
            };
            let at = *at as Millis;
            match event {
                RunEvent::Plan { items } => {
                    if self.step_times.len() < items.len() {
                        self.step_times.resize(items.len(), None);
                    }
                    for (i, item) in items.iter().enumerate() {
                        if item.state != RunPlanState::Pending && self.step_times[i].is_none() {
                            self.step_times[i] = Some(at);
                        }
                    }
                    self.step = items
                        .iter()
                        .position(|i| {
                            matches!(i.state, RunPlanState::Current | RunPlanState::Waiting)
                        })
                        .or_else(|| {
                            items
                                .iter()
                                .rposition(|i| i.state == RunPlanState::Done)
                        });
                }
                RunEvent::ToolCall {
                    tool, title, input, ..
                } => self.lines.push(ScreenLine {
                    kind: ScreenLineKind::Command,
                    text: command_text(tool, title, input.as_ref()),
                    at,
                    step: self.step,
                }),
                RunEvent::ToolResult { ok, output, .. } => {
                    let text = output.clone().unwrap_or_default();
                    let all: Vec<&str> = text.lines().filter(|l| !l.trim().is_empty()).collect();
                    let skip = all.len().saturating_sub(OUTPUT_LINES);
                    for l in &all[skip..] {
                        self.lines.push(ScreenLine {
                            kind: if *ok {
                                ScreenLineKind::Output
                            } else {
                                ScreenLineKind::Error
                            },
                            text: l.to_string(),
                            at,
                            step: self.step,
                        });
                    }
                }
                _ => {}
            }
        }
        if self.lines.len() > SCREEN_KEEP {
            let cut = self.lines.len() - SCREEN_KEEP;
            self.lines.drain(..cut);
        }
    }
}

/// Body, ref chips and suggestion of one side chat message.
fn side_message(m: &Message, posted: &HashMap<String, Millis>) -> SideChatMessage {
    let mut refs: Vec<String> = Vec::new();
    let mut suggestion = None;
    let mut body = m.body.clone();
    if let Some(card) = &m.card_v2 {
        if body.trim().is_empty() {
            body = card.fallback.clone();
        }
        let mut stack: Vec<&CardV2Block> = card.blocks.iter().rev().collect();
        while let Some(b) = stack.pop() {
            match b {
                CardV2Block::Ref { label, .. } => refs.push(label.clone()),
                CardV2Block::Draft { text, .. } if suggestion.is_none() => {
                    suggestion = Some(SideChatSuggestion {
                        text: text.trim().to_string(),
                        posted_at: posted.get(&m.id).copied(),
                    })
                }
                CardV2Block::Group { blocks, .. } => stack.extend(blocks.iter().rev()),
                _ => {}
            }
        }
    }
    SideChatMessage {
        id: m.id.clone(),
        author_id: m.author_id.clone(),
        body,
        at: m.created_at as Millis,
        refs,
        suggestion,
        streaming: m.streaming.unwrap_or(false),
    }
}

impl CoreProvider {
    fn panes_generation(&self) -> u64 {
        self.state().generation
    }

    /// The newest seq of a topic in the cache: what a side chat may read.
    fn latest_seq(&self, topic_id: &str) -> u64 {
        self.account()
            .and_then(|a| a.client.messages(topic_id, None, 1).ok())
            .and_then(|m| m.last().map(|m| m.created_seq))
            .unwrap_or(0)
    }

    /// My side chat topic with `member_id` about a topic, newest first.
    fn side_topic(&self, group_id: &str, topic_id: &str, member_id: &str) -> Option<String> {
        let a = self.account()?;
        let dm = a.client.dm_with(member_id).ok().flatten()?;
        a.client
            .side_chats(group_id, topic_id)
            .ok()?
            .into_iter()
            .find(|r| r.topic.group_id == dm)
            .map(|r| r.topic.id)
    }

    pub(super) fn side_chat_inner(
        &self,
        group_id: &str,
        topic_id: &str,
        member_id: &str,
    ) -> SideChatThread {
        let mut thread = SideChatThread {
            member_id: member_id.to_string(),
            messages: Vec::new(),
        };
        let Some(a) = self.account() else {
            return thread;
        };
        let source = self.resolve_topic(topic_id);
        let generation = self.panes_generation();
        let key = (group_id.to_string(), source.clone(), member_id.to_string());
        let side = self.side_topic(group_id, &source, member_id);
        let msgs: Vec<Message> = side
            .as_ref()
            .and_then(|s| a.client.messages(s, None, SIDE_CHAT_LIMIT).ok())
            .unwrap_or_default();
        let mut cache = lock(&self.shared.panes.inner);
        let inner = cache.for_generation(generation);
        thread.messages = msgs
            .iter()
            .filter(|m| m.deleted_at.flatten().is_none())
            .map(|m| side_message(m, &inner.posted))
            .collect();
        // My questions the server has not stored yet.
        let known: std::collections::HashSet<&str> =
            msgs.iter().map(|m| m.client_msg_id.as_str()).collect();
        let pending = inner.pending.entry(key).or_default();
        pending.retain(|(cid, _, _)| !known.contains(cid.as_str()));
        let me = a.me.id.clone();
        thread
            .messages
            .extend(pending.iter().map(|(cid, body, at)| SideChatMessage {
                id: cid.clone(),
                author_id: me.clone(),
                body: body.clone(),
                at: *at,
                refs: Vec::new(),
                suggestion: None,
                streaming: false,
            }));
        if let Some(side) = side {
            for item in a.client.pending_sends(&side).unwrap_or_default() {
                if known.contains(item.id.as_str()) {
                    continue;
                }
                if let chat_core::store::OutboxOp::Send { body, .. } = &item.op {
                    thread.messages.push(SideChatMessage {
                        id: item.id.clone(),
                        author_id: me.clone(),
                        body: body.clone(),
                        at: item.created_at as Millis,
                        refs: Vec::new(),
                        suggestion: None,
                        streaming: false,
                    });
                }
            }
        }
        thread
    }

    pub(super) fn send_side_chat_inner(
        &self,
        group_id: &str,
        topic_id: &str,
        member_id: &str,
        body: String,
    ) {
        let body = body.trim().to_string();
        let Some(a) = self.account() else {
            return;
        };
        if body.is_empty() {
            return;
        }
        let source = self.resolve_topic(topic_id);
        let upto = self.latest_seq(&source);
        let change = vec![Change::Messages {
            group_id: group_id.to_string(),
            topic_id: topic_id.to_string(),
        }];
        if let Some(side) = self.side_topic(group_id, &source, member_id) {
            if let Err(e) = a.client.reply_in_side_chat(&side, &body, Some(upto)) {
                tracing::warn!(error = %e, "cannot reply in a side chat");
            }
            self.emit(change);
            return;
        }
        let generation = self.panes_generation();
        let key = (group_id.to_string(), source.clone(), member_id.to_string());
        let handle = a.client.start_side_chat(SideChatRequest {
            bot_id: member_id.to_string(),
            context: TopicContext {
                group_id: group_id.to_string(),
                topic_id: source,
                upto_seq: upto,
            },
            body: body.clone(),
            title: None,
        });
        let this = self.clone();
        let at = now_ms();
        a.client.spawn(async move {
            let result = match handle.await {
                Ok(r) => r,
                Err(e) => Err(ClientError::Runtime(e.to_string())),
            };
            match result {
                Ok(started) => {
                    lock(&this.shared.panes.inner)
                        .for_generation(generation)
                        .pending
                        .entry(key)
                        .or_default()
                        .push((started.client_msg_id, body, at));
                }
                Err(e) => tracing::warn!(error = %e, "cannot start a side chat"),
            }
            this.emit(change);
        });
    }

    pub(super) fn post_side_chat_suggestion_inner(
        &self,
        group_id: &str,
        topic_id: &str,
        member_id: &str,
        message_id: &str,
    ) {
        let thread = self.side_chat_inner(group_id, topic_id, member_id);
        let Some(text) = thread
            .messages
            .iter()
            .find(|m| m.id == message_id)
            .and_then(|m| m.suggestion.as_ref())
            .filter(|s| s.posted_at.is_none() && !s.text.is_empty())
            .map(|s| s.text.clone())
        else {
            return;
        };
        let generation = self.panes_generation();
        lock(&self.shared.panes.inner)
            .for_generation(generation)
            .posted
            .insert(message_id.to_string(), now_ms());
        use crate::data::ChatProvider as _;
        self.send(SendRequest {
            group_id: group_id.to_string(),
            topic_id: topic_id.to_string(),
            body: text,
            mentions: Vec::new(),
            quote_of: None,
        });
    }

    /// Reads more of a run's log when the cache is stale (a running run every
    /// [`LIVE_REFRESH`], an ended one once).
    fn refresh_live(&self, run_id: &str, active: bool) {
        let Some(a) = self.account() else {
            return;
        };
        let generation = self.panes_generation();
        let after = {
            let mut cache = lock(&self.shared.panes.inner);
            let entry = cache
                .for_generation(generation)
                .live
                .entry(run_id.to_string())
                .or_default();
            let stale = match entry.fetched_at {
                None => true,
                Some(t) => active && t.elapsed() >= LIVE_REFRESH,
            };
            if entry.loading || !stale {
                return;
            }
            entry.loading = true;
            entry.after
        };
        let handle = a.client.run_log(run_id, after);
        let this = self.clone();
        let id = run_id.to_string();
        a.client.spawn(async move {
            let result = match handle.await {
                Ok(r) => r,
                Err(e) => Err(ClientError::Runtime(e.to_string())),
            };
            let changed = {
                let mut cache = lock(&this.shared.panes.inner);
                let Some(entry) = cache.for_generation(generation).live.get_mut(&id) else {
                    return;
                };
                entry.loading = false;
                entry.fetched_at = Some(Instant::now());
                match result {
                    Ok((entries, last)) => {
                        entry.apply(&entries);
                        let moved = last > entry.after;
                        entry.after = entry.after.max(last);
                        moved
                    }
                    Err(e) => {
                        tracing::info!(error = %e, "cannot read a run log for the live view");
                        false
                    }
                }
            };
            if changed {
                this.emit(vec![Change::RunLog { run_id: id }]);
            }
        });
    }

    /// The machine list, read in the background for `takeoverUrl`.
    fn refresh_takeover(&self) {
        let Some(a) = self.account() else {
            return;
        };
        let generation = self.panes_generation();
        {
            let mut cache = lock(&self.shared.panes.inner);
            let inner = cache.for_generation(generation);
            let stale = inner
                .machines_at
                .is_none_or(|t| t.elapsed() >= MACHINES_REFRESH);
            if inner.machines_loading || !stale {
                return;
            }
            inner.machines_loading = true;
        }
        let http = a.client.http();
        let this = self.clone();
        a.client.spawn(async move {
            let result = http.machines().await;
            let mut cache = lock(&this.shared.panes.inner);
            let inner = cache.for_generation(generation);
            inner.machines_loading = false;
            inner.machines_at = Some(Instant::now());
            if let Ok(list) = result {
                for m in list {
                    if let Some(url) = m.takeover_url {
                        inner.takeover.insert(m.id, url);
                    }
                }
            }
        });
    }

    pub(super) fn machine_screen_inner(&self, run_id: &str) -> Option<MachineScreen> {
        let a = self.account()?;
        let run = a.client.store().run(run_id).ok().flatten()?;
        self.refresh_live(run_id, run.is_active());
        if run.paused_by.is_some() {
            self.refresh_takeover();
        }
        // The run summaries know the machine's name once the machine list is in.
        let machine_name = self
            .runs_inner(&run.group_id, &run.topic_id)
            .into_iter()
            .find(|r| r.id == run_id)
            .and_then(|r| r.machine_name);
        let generation = self.panes_generation();
        let mut cache = lock(&self.shared.panes.inner);
        let inner = cache.for_generation(generation);
        let takeover_url = run
            .machine_id
            .as_ref()
            .and_then(|m| inner.takeover.get(m).cloned());
        let lines = inner
            .live
            .get(run_id)
            .map(|e| e.lines.clone())
            .unwrap_or_default();
        Some(MachineScreen {
            run_id: run_id.to_string(),
            machine_name,
            lines,
            started_at: run.started_at.map(|t| t as Millis),
            takeover_url,
            paused_by: run.paused_by,
        })
    }

    pub(super) fn acceptance_inner(&self, run_id: &str) -> Vec<AcceptanceItem> {
        let Some(a) = self.account() else {
            return Vec::new();
        };
        let Some(run) = a.client.store().run(run_id).ok().flatten() else {
            return Vec::new();
        };
        run.acceptance
            .unwrap_or_default()
            .into_iter()
            .map(|i| AcceptanceItem {
                text: i.text,
                met: match i.state {
                    AcceptanceState::Pass => Some(true),
                    AcceptanceState::Fail => Some(false),
                    _ => None,
                },
                evidence: i.evidence.and_then(|e| e.text),
            })
            .collect()
    }

    pub(super) fn run_step_times_inner(&self, run_id: &str) -> Vec<Millis> {
        let generation = self.panes_generation();
        let mut cache = lock(&self.shared.panes.inner);
        let Some(entry) = cache.for_generation(generation).live.get(run_id) else {
            return Vec::new();
        };
        // Steps in order until the first one that has not started.
        entry.step_times.iter().map_while(|t| *t).collect()
    }

    /// 接管 (`pause`) or 交还 (`resume`); `Change::Runs` when the answer is in.
    pub(super) fn pause_inner(
        &self,
        group_id: &str,
        topic_id: &str,
        run_id: &str,
        pause: bool,
    ) -> AdminReply<()> {
        let (tx, rx) = oneshot::channel();
        let Some(a) = self.account() else {
            let _ = tx.send(Err(AdminError::Offline));
            return rx;
        };
        let handle = if pause {
            a.client.pause_run(run_id)
        } else {
            a.client.resume_run(run_id, None)
        };
        let this = self.clone();
        let change = vec![Change::Runs {
            group_id: group_id.to_string(),
            topic_id: topic_id.to_string(),
        }];
        a.client.spawn(async move {
            let result = match handle.await {
                Ok(r) => r,
                Err(e) => Err(ClientError::Runtime(e.to_string())),
            };
            this.emit(change);
            let _ = tx.send(match result {
                Ok(_) => Ok(()),
                Err(e) => {
                    tracing::warn!(error = %e, pause, "cannot pause or resume a run");
                    Err(super::settings::admin_error(&e))
                }
            });
        });
        rx
    }
}
