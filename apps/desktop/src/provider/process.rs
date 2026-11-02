//! Search and the bot-run process view on chat-core: `search` goes over the
//! connection (`Client::search_hits`), runs come from the cache
//! (`run.updated` events), and run logs are fetched over HTTP and folded into
//! steps by `chat_core::ProcessView`, one cache entry per run.

use std::{
    collections::{HashMap, VecDeque},
    path::PathBuf,
    sync::Mutex,
    time::{Duration, Instant},
};

use chat_core::{ApiError, ClientError, ProcessItem, ProcessView, SearchRequest};
use futures::channel::oneshot;

use super::{CoreProvider, lock};
use crate::data::{
    BotRunSummary, Change, FileChangeKind, FileChangeView, ProcessStep, RawRunEntry, RunLoadError,
    RunProcess, RunStatus, SearchError, SearchHit, SearchQuery, SearchReply, SessionDownload,
};

/// Raw entries kept per run for the developer view.
const RAW_KEEP: usize = 300;
/// The machine list is fetched again at most this often.
const MACHINES_REFRESH: Duration = Duration::from_secs(60);

/// Per-account state of runs; cleared when the account changes.
#[derive(Default)]
pub(super) struct RunCache {
    inner: Mutex<Inner>,
}

#[derive(Default)]
struct Inner {
    generation: u64,
    runs: HashMap<String, RunEntry>,
    machines: HashMap<String, String>,
    machines_at: Option<Instant>,
    machines_loading: bool,
}

#[derive(Default)]
struct RunEntry {
    view: ProcessView,
    /// Next `afterSeq`.
    after: u64,
    raw: VecDeque<RawRunEntry>,
    loading: bool,
    loaded: bool,
    error: Option<RunLoadError>,
}

impl Inner {
    /// Forgets everything of an older account.
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

fn run_status(s: chat_core::proto::RunStatus) -> RunStatus {
    use chat_core::proto::RunStatus as R;
    match s {
        R::Created => RunStatus::Created,
        R::Running => RunStatus::Running,
        R::Waiting => RunStatus::Waiting,
        R::Blocked => RunStatus::Blocked,
        R::Done => RunStatus::Done,
        R::Failed => RunStatus::Failed,
        R::Cancelled => RunStatus::Cancelled,
    }
}

/// `run.done` status: "succeeded" | "failed" | "cancelled" | "timeout".
fn done_status(s: &str) -> RunStatus {
    match s {
        "succeeded" | "done" => RunStatus::Done,
        "cancelled" => RunStatus::Cancelled,
        _ => RunStatus::Failed,
    }
}

fn file_kind(kind: &str) -> FileChangeKind {
    match kind {
        "add" | "added" | "create" => FileChangeKind::Added,
        "delete" | "deleted" | "remove" => FileChangeKind::Deleted,
        _ => FileChangeKind::Modified,
    }
}

/// One process view row in the app's words.
pub fn step(item: &ProcessItem) -> ProcessStep {
    match item {
        ProcessItem::Message {
            role,
            text,
            streaming,
            ..
        } => ProcessStep::Message {
            text: text.clone(),
            from_bot: role != "user",
            streaming: *streaming,
        },
        ProcessItem::Reasoning { text, .. } => ProcessStep::Reasoning { text: text.clone() },
        ProcessItem::Tool {
            tool,
            title,
            input,
            ok,
            output,
            exit_code,
            ..
        } => ProcessStep::Tool {
            title: if title.trim().is_empty() {
                tool.clone()
            } else {
                title.clone()
            },
            input: input.as_ref().map(|v| v.to_string()),
            ok: *ok,
            output: output.clone(),
            exit_code: *exit_code,
        },
        ProcessItem::Files { changes, .. } => ProcessStep::Files {
            changes: changes
                .iter()
                .map(|c| FileChangeView {
                    path: c.path.clone(),
                    kind: file_kind(&c.kind),
                })
                .collect(),
        },
        ProcessItem::Approval {
            title,
            detail,
            decision,
            ..
        } => ProcessStep::Approval {
            title: title.clone(),
            detail: detail.clone(),
            approved: decision.as_deref().map(|d| d == "approve"),
        },
        ProcessItem::TurnStarted { .. } => ProcessStep::TurnStarted,
        ProcessItem::TurnCompleted { status, error, .. } => ProcessStep::TurnCompleted {
            ok: error.is_none()
                && !matches!(
                    status.as_str(),
                    "failed" | "error" | "interrupted" | "cancelled" | "timeout"
                ),
            error: error.clone(),
        },
        ProcessItem::Status { message, .. } => ProcessStep::Status {
            message: message.clone().unwrap_or_default(),
        },
        ProcessItem::Done {
            status,
            summary,
            error,
            duration_ms,
        } => ProcessStep::Done {
            status: done_status(status),
            summary: summary.clone(),
            error: error.clone(),
            duration_ms: *duration_ms,
        },
    }
}

fn load_error(e: &ClientError) -> RunLoadError {
    match e {
        ClientError::Api(ApiError::Network(_) | ApiError::NoToken) => RunLoadError::Offline,
        ClientError::Api(
            ApiError::Server { status: 404, .. } | ApiError::Unexpected { status: 404 },
        )
        | ClientError::NotFound => RunLoadError::NotFound,
        ClientError::Api(
            ApiError::Server {
                status: 401 | 403, ..
            }
            | ApiError::Unexpected { status: 401 | 403 },
        ) => RunLoadError::Forbidden,
        _ => RunLoadError::Failed,
    }
}

fn hit(h: chat_core::proto::SearchHit) -> SearchHit {
    SearchHit {
        highlights: h
            .highlights
            .unwrap_or_default()
            .into_iter()
            .map(|r| r.start as usize..(r.start as usize + r.length as usize))
            .collect(),
        group_id: h.group_id,
        topic_id: h.topic_id,
        message_id: h.message_id,
        author_id: h.author_id,
        snippet: h.snippet,
        created_at: h.created_at as i64,
        seq: h.seq,
    }
}

/// A file name for a run's session: only safe characters of the run id.
pub fn session_file_name(run_id: &str) -> String {
    let safe: String = run_id
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_')
        .take(80)
        .collect();
    format!("run-{safe}.jsonl.gz")
}

impl CoreProvider {
    fn run_generation(&self) -> u64 {
        self.state().generation
    }

    pub(super) fn search_inner(&self, query: SearchQuery) -> SearchReply {
        let (tx, rx) = oneshot::channel();
        let Some(account) = self.account() else {
            let _ = tx.send(Err(SearchError::Offline));
            return rx;
        };
        let request = SearchRequest {
            query: query.text,
            group_id: query.group_id,
            topic_id: None,
            limit: Some(query.limit.clamp(1, 50)),
        };
        let future = account.client.search_hits(request);
        account.client.spawn(async move {
            let result = future
                .await
                .map(|hits| hits.into_iter().map(hit).collect())
                .map_err(|e| match e {
                    chat_core::SearchError::NotConnected => SearchError::Offline,
                    chat_core::SearchError::Refused { .. } => SearchError::Refused,
                    chat_core::SearchError::Timeout => SearchError::TimedOut,
                    chat_core::SearchError::Invalid(_) => SearchError::Unavailable,
                });
            let _ = tx.send(result);
        });
        rx
    }

    pub(super) fn runs_inner(&self, group_id: &str, topic_id: &str) -> Vec<BotRunSummary> {
        let Some(account) = self.account() else {
            return Vec::new();
        };
        let topic = self.resolve_topic(topic_id);
        let runs = match account.client.runs(&topic) {
            Ok(runs) => runs,
            Err(e) => {
                tracing::warn!(error = %e, "cannot read runs");
                return Vec::new();
            }
        };
        let generation = self.run_generation();
        let (names, fetch) = {
            let mut cache = lock(&self.shared.runs.inner);
            let inner = cache.for_generation(generation);
            let unknown = runs
                .iter()
                .filter_map(|r| r.machine_id.as_ref())
                .any(|m| !inner.machines.contains_key(m));
            let stale = inner
                .machines_at
                .is_none_or(|at| at.elapsed() > MACHINES_REFRESH);
            let fetch = unknown && stale && !inner.machines_loading;
            if fetch {
                inner.machines_loading = true;
            }
            (inner.machines.clone(), fetch)
        };
        if fetch {
            let this = self.clone();
            let http = account.client.http();
            let (g, t) = (group_id.to_string(), topic_id.to_string());
            account.client.spawn(async move {
                let result = http.machines().await;
                {
                    let mut cache = lock(&this.shared.runs.inner);
                    let inner = cache.for_generation(generation);
                    inner.machines_loading = false;
                    inner.machines_at = Some(Instant::now());
                    match &result {
                        Ok(list) => {
                            for m in list {
                                inner.machines.insert(m.id.clone(), m.name.clone());
                            }
                        }
                        // Not every member may list machines; the id is shown instead.
                        Err(e) => tracing::info!(error = %e, "cannot list machines"),
                    }
                }
                if result.is_ok() {
                    this.emit(vec![Change::Runs {
                        group_id: g,
                        topic_id: t,
                    }]);
                }
            });
        }
        runs.into_iter()
            .map(|r| BotRunSummary {
                machine_name: r.machine_id.as_ref().and_then(|m| names.get(m).cloned()),
                id: r.id,
                bot_id: r.bot_id,
                group_id: group_id.to_string(),
                topic_id: topic_id.to_string(),
                machine_id: r.machine_id,
                status: run_status(r.status),
                requested_by: r.requested_by,
                created_at: r.created_at as i64,
                started_at: r.started_at.map(|t| t as i64),
                ended_at: r.ended_at.map(|t| t as i64),
                updated_at: r.updated_at as i64,
                summary: r.summary,
                plan: r
                    .plan
                    .unwrap_or_default()
                    .into_iter()
                    .map(plan_item)
                    .collect(),
            })
            .collect()
    }

    pub(super) fn run_process_inner(&self, run_id: &str) -> RunProcess {
        let generation = self.run_generation();
        let mut cache = lock(&self.shared.runs.inner);
        let inner = cache.for_generation(generation);
        let Some(entry) = inner.runs.get(run_id) else {
            return RunProcess::default();
        };
        RunProcess {
            steps: entry.view.items.iter().map(step).collect(),
            raw: entry.raw.iter().cloned().collect(),
            last_seq: entry.after,
            session: entry.view.session.clone(),
            loading: entry.loading,
            loaded: entry.loaded,
            error: entry.error,
        }
    }

    pub(super) fn follow_run_inner(&self, run_id: &str) {
        let generation = self.run_generation();
        let changed = vec![Change::RunLog {
            run_id: run_id.to_string(),
        }];
        let Some(account) = self.account() else {
            let mut cache = lock(&self.shared.runs.inner);
            let entry = cache
                .for_generation(generation)
                .runs
                .entry(run_id.to_string())
                .or_default();
            entry.error = Some(RunLoadError::Offline);
            entry.loaded = true;
            drop(cache);
            self.emit(changed);
            return;
        };
        let after = {
            let mut cache = lock(&self.shared.runs.inner);
            let entry = cache
                .for_generation(generation)
                .runs
                .entry(run_id.to_string())
                .or_default();
            if entry.loading {
                return;
            }
            entry.loading = true;
            entry.after
        };
        let handle = account.client.run_log(run_id, after);
        let this = self.clone();
        let id = run_id.to_string();
        account.client.spawn(async move {
            let result = match handle.await {
                Ok(r) => r,
                Err(e) => Err(ClientError::Runtime(e.to_string())),
            };
            {
                let mut cache = lock(&this.shared.runs.inner);
                let inner = cache.for_generation(generation);
                let Some(entry) = inner.runs.get_mut(&id) else {
                    return;
                };
                entry.loading = false;
                entry.loaded = true;
                match result {
                    Ok((entries, last)) => {
                        entry.error = None;
                        for e in &entries {
                            let seq = chat_core::runs::entry_seq(e);
                            if seq <= entry.after {
                                continue;
                            }
                            let json = serde_json::to_string(e).unwrap_or_default();
                            entry.raw.push_back(RawRunEntry { seq, json });
                        }
                        while entry.raw.len() > RAW_KEEP {
                            entry.raw.pop_front();
                        }
                        entry.view.apply(&entries);
                        entry.after = entry.after.max(last).max(entry.view.last_seq);
                    }
                    Err(e) => {
                        tracing::warn!(error = %e, "cannot load a run log");
                        entry.error = Some(load_error(&e));
                    }
                }
            }
            this.emit(vec![Change::RunLog { run_id: id }]);
        });
        self.emit(changed);
    }

    pub(super) fn download_session_inner(&self, run_id: &str, dir: PathBuf) -> SessionDownload {
        let (tx, rx) = oneshot::channel();
        let Some(account) = self.account() else {
            let _ = tx.send(Err(RunLoadError::Offline));
            return rx;
        };
        let http = account.client.http();
        let id = run_id.to_string();
        account.client.spawn(async move {
            let result = match http.run_session(&id).await {
                Ok(bytes) => {
                    let path = dir.join(session_file_name(&id));
                    std::fs::create_dir_all(&dir)
                        .and_then(|_| std::fs::write(&path, &bytes))
                        .map(|_| path)
                        .map_err(|e| {
                            tracing::warn!(error = %e, "cannot save a session");
                            RunLoadError::Failed
                        })
                }
                Err(e) => Err(load_error(&ClientError::Api(e))),
            };
            let _ = tx.send(result);
        });
        rx
    }
}

/// A run plan item of the protocol, as the views read it.
fn plan_item(i: chat_core::proto::RunPlanItem) -> crate::data::PlanItem {
    use crate::data::PlanState;
    use chat_core::proto::RunPlanState as S;
    let state = match i.state {
        S::Done => PlanState::Done,
        S::Current => PlanState::Current,
        S::Waiting => PlanState::Waiting,
        S::Pending => PlanState::Pending,
    };
    crate::data::PlanItem::new(i.text, state)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn session_file_name_is_safe() {
        assert_eq!(session_file_name("r_1-a"), "run-r_1-a.jsonl.gz");
        assert_eq!(session_file_name("../../etc/x"), "run-etcx.jsonl.gz");
    }

    #[test]
    fn items_map_to_steps() {
        let item = ProcessItem::Approval {
            approval_id: "a".into(),
            title: "t".into(),
            detail: None,
            decision: Some("deny".into()),
        };
        assert_eq!(
            step(&item),
            ProcessStep::Approval {
                title: "t".into(),
                detail: None,
                approved: Some(false)
            }
        );
        let done = ProcessItem::Done {
            status: "succeeded".into(),
            summary: None,
            error: None,
            duration_ms: 5,
        };
        assert!(matches!(
            step(&done),
            ProcessStep::Done {
                status: RunStatus::Done,
                ..
            }
        ));
        let turn = ProcessItem::TurnCompleted {
            turn_id: "1".into(),
            status: "failed".into(),
            error: None,
        };
        assert!(matches!(
            step(&turn),
            ProcessStep::TurnCompleted { ok: false, .. }
        ));
        assert_eq!(file_kind("add"), FileChangeKind::Added);
        assert_eq!(file_kind("modify"), FileChangeKind::Modified);
        assert_eq!(file_kind("delete"), FileChangeKind::Deleted);
    }

    #[test]
    fn errors_map_to_load_errors() {
        let e = ClientError::Api(ApiError::Network("x".into()));
        assert_eq!(load_error(&e), RunLoadError::Offline);
        let e = ClientError::Api(ApiError::Unexpected { status: 404 });
        assert_eq!(load_error(&e), RunLoadError::NotFound);
        let e = ClientError::Api(ApiError::Unexpected { status: 403 });
        assert_eq!(load_error(&e), RunLoadError::Forbidden);
    }
}

