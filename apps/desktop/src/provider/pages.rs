//! The 群, 成员 and 产出 pages on the chat-core cache (pages track):
//! 群里的决定 derived from answered asks marked `record`, 页面里的回应 from the
//! server's page log of the 产出 (`GET /v1/artifacts/:id/events`, GAPS SF-2) plus the page
//! events this window saw since (page events are never stored in the group log),
//! page events declared by bot-made versions and forwarded as `artifact.event`, and
//! 重试 for a stuck run.

use std::{collections::HashMap, sync::Mutex};

use chat_core::{
    DecisionRow,
    proto::{ArtifactEventKind, ArtifactEventTransient, CardV2, CardV2Block, GroupDecision},
};

use super::CoreProvider;
use crate::{
    copy::{artifact_page as ca, fill, group_page as cg, members_page as cm},
    data::{
        AdminError, AdminReply, Change, ChatProvider as _, SendRequest,
        pages::{ArtifactResponse, ResponsesLoad},
        v2::Decision,
    },
};

/// The words of an answer: the picked options of the card's decision blocks,
/// else the pressed action's label.
fn choice(card: &CardV2, row: &DecisionRow) -> String {
    let inputs = row.answer.inputs.clone().unwrap_or_default();
    let mut picked = Vec::new();
    for block in card.all_blocks() {
        if let CardV2Block::Decision { id, options, .. } = block
            && let Some(value) = inputs.get(id)
        {
            let ids: Vec<String> = match value {
                serde_json::Value::String(s) => vec![s.clone()],
                serde_json::Value::Array(a) => a
                    .iter()
                    .filter_map(|v| v.as_str().map(str::to_string))
                    .collect(),
                _ => Vec::new(),
            };
            picked.extend(
                options
                    .iter()
                    .filter(|o| ids.contains(&o.id))
                    .map(|o| o.label.clone()),
            );
        }
    }
    if picked.is_empty() {
        card.actions()
            .iter()
            .find(|a| a.id == row.answer.action_id)
            .map(|a| a.label.clone())
            .unwrap_or_default()
    } else {
        picked.join(cm::MEMBERS_LIST_SEP)
    }
}

/// Decisions per 群 from `GET /v1/groups/:id/decisions`. The server keeps the card as it was
/// answered, so a decision stays after its author edits the card (the cache only has the
/// newest card). Fetched again when the cache gets a newer local decision. Kept per account:
/// a sign-in as someone else starts empty.
#[derive(Default)]
pub(super) struct DecisionsCache {
    inner: Mutex<DecisionsMap>,
}

#[derive(Default)]
struct DecisionsMap {
    generation: u64,
    groups: HashMap<String, DecisionsEntry>,
}

impl DecisionsMap {
    /// The entries of this account; an older account's are dropped.
    fn of(&mut self, generation: u64) -> &mut HashMap<String, DecisionsEntry> {
        if self.generation != generation {
            self.generation = generation;
            self.groups.clear();
        }
        &mut self.groups
    }
}

/// Page events most kept per 产出 in the local log.
const PAGE_EVENTS_KEPT: usize = 200;

/// One page event seen by this window: one I sent, or one of my 产出 versions got
/// (transient `artifact.event`). Lives only while the app runs.
#[derive(Debug, Clone, PartialEq)]
pub(super) struct PageEvent {
    pub actor_id: String,
    pub version: u32,
    pub name: String,
    pub payload: Option<serde_json::Value>,
    pub at: i64,
}

/// The page event log, per 产出 id, oldest first: the server's page log once fetched, and
/// the events this window saw after it.
#[derive(Default)]
pub(super) struct PageEventLog {
    inner: Mutex<HashMap<String, Vec<PageEvent>>>,
    /// When the server's page log of a 产出 was last asked for.
    fetched: Mutex<HashMap<String, std::time::Instant>>,
    /// Where the last read of each 产出's page log stands.
    loads: Mutex<HashMap<String, ResponsesLoad>>,
}

/// How often the server's page log of an open 产出 is read again.
const PAGE_LOG_REFRESH: std::time::Duration = std::time::Duration::from_secs(60);

impl PageEventLog {
    /// True when the server's page log of `artifact_id` should be read now (marks it).
    pub(super) fn due(&self, artifact_id: &str) -> bool {
        let mut map = self.fetched.lock().unwrap_or_else(|e| e.into_inner());
        let now = std::time::Instant::now();
        match map.get(artifact_id) {
            Some(at) if now.duration_since(*at) < PAGE_LOG_REFRESH => false,
            _ => {
                map.insert(artifact_id.to_string(), now);
                true
            }
        }
    }

    /// Where the read of the page log of `artifact_id` stands. Before the first answer it is
    /// [`ResponsesLoad::Loading`]; a failed later read keeps the events it had.
    pub(super) fn load(&self, artifact_id: &str) -> ResponsesLoad {
        self.loads
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(artifact_id)
            .copied()
            .unwrap_or(ResponsesLoad::Loading)
    }

    fn set_load(&self, artifact_id: &str, load: ResponsesLoad) {
        self.loads
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(artifact_id.to_string(), load);
    }

    /// Makes the next [`PageEventLog::due`] true (重试).
    pub(super) fn forget_fetch(&self, artifact_id: &str) {
        self.fetched
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(artifact_id);
        let mut loads = self.loads.lock().unwrap_or_else(|e| e.into_inner());
        if loads.get(artifact_id) == Some(&ResponsesLoad::Failed) {
            loads.insert(artifact_id.to_string(), ResponsesLoad::Loading);
        }
    }

    /// Takes the server's page log (newest first, as the server sends it): the stored events,
    /// then the events this window saw after the newest stored one.
    pub(super) fn merge_server(&self, artifact_id: &str, newest_first: Vec<PageEvent>) {
        let mut stored = newest_first;
        stored.reverse();
        let newest = stored.last().map(|e| e.at).unwrap_or(i64::MIN);
        let mut map = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        let local = map.remove(artifact_id).unwrap_or_default();
        stored.extend(local.into_iter().filter(|e| e.at > newest));
        if stored.len() > PAGE_EVENTS_KEPT {
            let extra = stored.len() - PAGE_EVENTS_KEPT;
            stored.drain(..extra);
        }
        map.insert(artifact_id.to_string(), stored);
    }

    pub(super) fn record(&self, artifact_id: &str, event: PageEvent) {
        let mut map = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        let list = map.entry(artifact_id.to_string()).or_default();
        list.push(event);
        if list.len() > PAGE_EVENTS_KEPT {
            let extra = list.len() - PAGE_EVENTS_KEPT;
            list.drain(..extra);
        }
    }

    /// A transient page event of one of my 产出 versions.
    pub(super) fn record_transient(&self, t: &ArtifactEventTransient) {
        self.record(
            &t.artifact_id,
            PageEvent {
                actor_id: t.actor_id.clone(),
                version: t.version,
                name: t.name.clone(),
                payload: t.payload.clone(),
                at: t.at.min(i64::MAX as u64) as i64,
            },
        );
    }

    pub(super) fn events(&self, artifact_id: &str) -> Vec<PageEvent> {
        let map = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        map.get(artifact_id).cloned().unwrap_or_default()
    }
}

#[derive(Default)]
struct DecisionsEntry {
    /// The server's list, once fetched.
    list: Option<Vec<Decision>>,
    /// The newest local decision when the fetch started.
    local_key: Option<String>,
    in_flight: bool,
}

/// The text of a server decision: the question with the chosen options, else the action.
fn server_text(d: &GroupDecision) -> String {
    let choice = match d.choices.as_deref() {
        Some(c) if !c.is_empty() => c.join(cm::MEMBERS_LIST_SEP),
        _ => d.action_label.clone(),
    };
    if choice.is_empty() {
        d.title.clone()
    } else {
        fill(
            cg::GROUP_PAGE_DECISION_FROM,
            &[("question", &d.title), ("choice", &choice)],
        )
    }
}

impl CoreProvider {
    /// Drops the fetched server list of a 群's decisions: the next read fetches it again
    /// (after 记一条 here, or a `decision.recorded` from another device).
    pub(super) fn forget_decisions(&self, group_id: &str) {
        let mut map = self
            .shared
            .decisions
            .inner
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        if let Some(entry) = map.groups.get_mut(group_id) {
            entry.list = None;
        }
    }

    pub(super) fn group_decisions_inner(&self, group_id: &str) -> Vec<Decision> {
        let local = self.local_decisions(group_id);
        let Some(a) = self.account() else {
            return local;
        };
        let key = local.first().map(|d| d.id.clone());
        let generation = self.state().generation;
        let mut map = self
            .shared
            .decisions
            .inner
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let entry = map.of(generation).entry(group_id.to_string()).or_default();
        if !entry.in_flight && (entry.list.is_none() || entry.local_key != key) {
            entry.in_flight = true;
            entry.local_key = key;
            let handle = a.client.fetch_decisions_page(group_id, 50);
            let this = self.clone();
            let group = group_id.to_string();
            a.client.spawn(async move {
                let fetched = match handle.await {
                    Ok(Ok(list)) => Some(list),
                    Ok(Err(e)) => {
                        tracing::warn!(error = %e, "群里的决定: fetch failed, using the cache");
                        None
                    }
                    Err(e) => {
                        tracing::warn!(error = %e, "群里的决定: fetch task failed");
                        None
                    }
                };
                {
                    let mut map = this
                        .shared
                        .decisions
                        .inner
                        .lock()
                        .unwrap_or_else(|e| e.into_inner());
                    if map.generation != generation {
                        // Another account signed in meanwhile: this list is not theirs.
                        return;
                    }
                    let entry = map.of(generation).entry(group.clone()).or_default();
                    entry.in_flight = false;
                    if let Some(page) = &fetched {
                        let mut list: Vec<Decision> = page
                            .decisions
                            .iter()
                            .map(|d| Decision {
                                id: format!("{}-{}", d.message_id, d.seq),
                                group_id: group.clone(),
                                topic_id: Some(d.topic_id.clone()),
                                text: server_text(d),
                                by: d.answered_by.clone(),
                                at: d.answered_at.min(i64::MAX as u64) as i64,
                            })
                            .collect();
                        // 记一条: the decisions recorded by hand.
                        list.extend(page.recorded.iter().flatten().map(|d| Decision {
                            id: format!("recorded-{}", d.id),
                            group_id: group.clone(),
                            topic_id: d.topic_id.clone(),
                            text: d.text.clone(),
                            by: d.recorded_by.clone(),
                            at: d.recorded_at.min(i64::MAX as u64) as i64,
                        }));
                        list.sort_by_key(|d| std::cmp::Reverse(d.at));
                        entry.list = Some(list);
                    }
                }
                if fetched.is_some() {
                    this.emit(vec![Change::Groups]);
                }
            });
        }
        match &entry.list {
            // The server's list, plus local ones it does not have yet (answered after the
            // fetch started), newest first.
            Some(list) => {
                let mut out: Vec<Decision> = local
                    .into_iter()
                    .filter(|l| !list.iter().any(|d| d.id == l.id))
                    .filter(|l| list.first().is_none_or(|d| l.at >= d.at))
                    .collect();
                out.extend(list.iter().cloned());
                out
            }
            None => local,
        }
    }

    /// 记一条: records a decision by hand; the next read fetches the list again.
    pub(super) fn record_decision_inner(&self, group_id: &str, text: String) -> AdminReply<()> {
        let this = self.clone();
        let group = group_id.to_string();
        self.request(move |client, _| async move {
            let handle = client
                .record_decision(&group, &text, None)
                .map_err(|e| super::settings::admin_error(&e))?;
            handle
                .await
                .map_err(|_| AdminError::Other)?
                .map_err(|e| super::settings::admin_error(&e))?;
            this.forget_decisions(&group);
            this.emit(vec![Change::Groups]);
            Ok(())
        })
    }

    fn local_decisions(&self, group_id: &str) -> Vec<Decision> {
        let Some(a) = self.account() else {
            return Vec::new();
        };
        let rows = a.client.decisions(Some(group_id), 50).unwrap_or_default();
        rows.into_iter()
            .map(|row| {
                let answer = choice(&row.card, &row);
                let text = if answer.is_empty() {
                    row.card.title.clone()
                } else {
                    fill(
                        cg::GROUP_PAGE_DECISION_FROM,
                        &[("question", &row.card.title), ("choice", &answer)],
                    )
                };
                Decision {
                    id: format!("{}-{}", row.message_id, row.answer.seq),
                    group_id: row.group_id.clone(),
                    topic_id: Some(row.topic_id.clone()),
                    text,
                    by: row.answer.actor_id.clone(),
                    at: row.answer.at.min(i64::MAX as u64) as i64,
                }
            })
            .collect()
    }

    pub(super) fn artifact_page_events_inner(
        &self,
        artifact_id: &str,
        version: u32,
    ) -> Vec<String> {
        let Some(a) = self.account() else {
            return Vec::new();
        };
        let Some(row) = a.client.artifact(artifact_id).ok().flatten() else {
            return Vec::new();
        };
        let Some(v) = row.versions.iter().find(|v| v.version == version) else {
            return Vec::new();
        };
        if !self.member(&v.created_by).is_some_and(|m| m.is_bot()) {
            return Vec::new();
        }
        v.events.iter().flatten().map(|e| e.name.clone()).collect()
    }

    pub(super) fn send_artifact_event_inner(
        &self,
        artifact_id: &str,
        version: u32,
        name: &str,
        payload: Option<serde_json::Value>,
    ) {
        let Some(a) = self.account() else { return };
        match a.client.send_artifact_event(
            artifact_id,
            version,
            name,
            ArtifactEventKind::Emit,
            payload.clone(),
        ) {
            Ok(_) => self.shared.page_events.record(
                artifact_id,
                PageEvent {
                    actor_id: a.me.id.clone(),
                    version,
                    name: name.to_string(),
                    payload,
                    at: crate::selftest::clock::now_ms(),
                },
            ),
            Err(e) => tracing::warn!(error = %e, "产出 page event dropped"),
        }
    }

    /// Redraws the 产出 page of `artifact_id`.
    pub(super) fn emit_artifact_change(&self, artifact_id: &str) {
        let Some(a) = self.account() else { return };
        if let Some(r) = a.client.artifact(artifact_id).ok().flatten() {
            self.emit(vec![Change::Artifacts {
                group_id: r.artifact.group_id,
                topic_id: r.artifact.topic_id,
            }]);
        }
    }

    /// Reads the server's page log of a 产出 in the background (at most every minute) and
    /// redraws the 产出 page when it arrives.
    pub(super) fn refresh_page_log(&self, artifact_id: &str) {
        let Some(a) = self.account() else { return };
        if !self.shared.page_events.due(artifact_id) {
            return;
        }
        let http = a.client.http();
        let this = self.clone();
        let id = artifact_id.to_string();
        let change = a
            .client
            .artifact(artifact_id)
            .ok()
            .flatten()
            .map(|r| Change::Artifacts {
                group_id: r.artifact.group_id,
                topic_id: r.artifact.topic_id,
            });
        chat_core::background().spawn(async move {
            match http
                .artifact_events(&id, None, PAGE_EVENTS_KEPT as u32)
                .await
            {
                Ok(r) => {
                    let events = r
                        .events
                        .into_iter()
                        .map(|e| PageEvent {
                            actor_id: e.actor_id,
                            version: e.version,
                            name: e.name,
                            payload: e.payload,
                            at: e.at.min(i64::MAX as u64) as i64,
                        })
                        .collect();
                    this.shared.page_events.merge_server(&id, events);
                    this.shared.page_events.set_load(&id, ResponsesLoad::Ready);
                }
                Err(e) => {
                    tracing::info!(error = %e, "产出 page log not read");
                    this.shared.page_events.set_load(&id, ResponsesLoad::Failed);
                }
            }
            if let Some(c) = change {
                this.emit(vec![c]);
            }
        });
    }

    pub(super) fn artifact_responses_inner(&self, artifact_id: &str) -> Vec<ArtifactResponse> {
        let Some(a) = self.account() else {
            return Vec::new();
        };
        self.refresh_page_log(artifact_id);
        let row = a.client.artifact(artifact_id).ok().flatten();
        let label = |version: u32, name: &str| -> String {
            row.as_ref()
                .and_then(|r| r.versions.iter().find(|v| v.version == version))
                .and_then(|v| v.events.as_ref())
                .and_then(|e| e.iter().find(|d| d.name == name))
                .and_then(|d| d.label.clone())
                .unwrap_or_else(|| name.to_string())
        };
        self.shared
            .page_events
            .events(artifact_id)
            .into_iter()
            .map(|e| {
                let who = self.member(&e.actor_id).map(|m| m.name).unwrap_or_default();
                let action = label(e.version, &e.name);
                let value = e.payload.as_ref().and_then(|p| match p {
                    serde_json::Value::String(s) => Some(s.clone()),
                    serde_json::Value::Object(o) => ["label", "value", "choice"]
                        .iter()
                        .find_map(|k| o.get(*k).and_then(|v| v.as_str()).map(str::to_string)),
                    _ => None,
                });
                let text = match value {
                    Some(v) => fill(
                        ca::ARTIFACT_RESPONSE_VALUE,
                        &[("name", &who), ("action", &action), ("value", &v)],
                    ),
                    None => fill(
                        ca::ARTIFACT_RESPONSE,
                        &[("name", &who), ("action", &action)],
                    ),
                };
                ArtifactResponse {
                    member_id: e.actor_id,
                    text,
                    at: Some(e.at),
                    topic: None,
                }
            })
            .collect()
    }

    /// 重试: a new run with the same request (`POST /v1/runs/:id/retry`). A server
    /// without that route gets the old way: a message in the run's 话题 that
    /// mentions the member.
    pub(super) fn retry_member_problem_inner(
        &self,
        member_id: &str,
        run_id: &str,
    ) -> AdminReply<()> {
        let (tx, rx) = futures::channel::oneshot::channel();
        let Some(a) = self.account() else {
            let _ = tx.send(Err(AdminError::Unsupported));
            return rx;
        };
        // Any run the cache has, however old: the member page lists the stuck ones.
        let run = a
            .client
            .runs_since(0)
            .unwrap_or_default()
            .into_iter()
            .find(|r| r.id == run_id && r.bot_id == member_id);
        let Some(run) = run else {
            let _ = tx.send(Err(AdminError::Unsupported));
            return rx;
        };
        let handle = a.client.retry_run(run_id);
        let this = self.clone();
        let member = member_id.to_string();
        let change = vec![Change::Runs {
            group_id: run.group_id.clone(),
            topic_id: run.topic_id.clone(),
        }];
        a.client.spawn(async move {
            let result = match handle.await {
                Ok(r) => r,
                Err(e) => Err(chat_core::ClientError::Runtime(e.to_string())),
            };
            match result {
                Ok(_) => {
                    this.emit(change);
                    let _ = tx.send(Ok(()));
                }
                Err(chat_core::ClientError::Api(chat_core::ApiError::Server {
                    status: 404,
                    ..
                })) => {
                    // An older server: ask in the 话题 instead.
                    let name = this.member(&member).map(|m| m.name).unwrap_or_default();
                    this.send(SendRequest {
                        group_id: run.group_id,
                        topic_id: run.topic_id,
                        body: fill(cm::MEMBERS_RETRY_MESSAGE, &[("name", &name)]),
                        mentions: vec![member],
                        quote_of: None,
                    });
                    let _ = tx.send(Ok(()));
                }
                Err(e) => {
                    tracing::warn!(error = %e, "cannot retry a run");
                    let _ = tx.send(Err(super::settings::admin_error(&e)));
                }
            }
        });
        rx
    }
}

#[cfg(test)]
impl CoreProvider {
    /// Tests: a server list of 群里的决定 for a 群, as if it was fetched now.
    pub(super) fn seed_group_decisions_for_test(&self, group_id: &str, list: Vec<Decision>) {
        let generation = self.state().generation;
        let mut map = self
            .shared
            .decisions
            .inner
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let key = self.local_decisions(group_id).first().map(|d| d.id.clone());
        let entry = map.of(generation).entry(group_id.to_string()).or_default();
        entry.list = Some(list);
        entry.local_key = key;
        entry.in_flight = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_page_log_takes_the_servers_events_and_keeps_newer_local_ones() {
        let ev = |name: &str, at: i64| PageEvent {
            actor_id: "m".into(),
            version: 1,
            name: name.into(),
            payload: None,
            at,
        };
        let log = PageEventLog::default();
        log.record("A", ev("seen", 5));
        log.record("A", ev("mine-later", 30));
        // The server sends newest first; an event this window saw before is in it.
        log.merge_server("A", vec![ev("pick", 20), ev("seen", 5)]);
        let names: Vec<String> = log.events("A").into_iter().map(|e| e.name).collect();
        assert_eq!(names, ["seen", "pick", "mine-later"]);
        assert!(log.due("A"));
        assert!(!log.due("A"));
    }
}
