//! The 群, 成员 and 产出 pages on the chat-core cache (pages track):
//! 群里的决定 derived from answered asks marked `record`, 页面里的回应 from the
//! local page event log (page events are transient, never stored in the group log),
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
        AdminError, AdminReply, Change, ChatProvider as _, SendRequest, pages::ArtifactResponse,
        reply_now, v2::Decision,
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
/// newest card). Fetched again when the cache gets a newer local decision.
#[derive(Default)]
pub(super) struct DecisionsCache {
    inner: Mutex<HashMap<String, DecisionsEntry>>,
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

/// The local page event log, per 产出 id, oldest first (R5: page events are transient).
#[derive(Default)]
pub(super) struct PageEventLog {
    inner: Mutex<HashMap<String, Vec<PageEvent>>>,
}

impl PageEventLog {
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
    pub(super) fn group_decisions_inner(&self, group_id: &str) -> Vec<Decision> {
        let local = self.local_decisions(group_id);
        let Some(a) = self.account() else {
            return local;
        };
        let key = local.first().map(|d| d.id.clone());
        let mut map = self
            .shared
            .decisions
            .inner
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let entry = map.entry(group_id.to_string()).or_default();
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
                    let entry = map.entry(group.clone()).or_default();
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
            {
                let mut map = this
                    .shared
                    .decisions
                    .inner
                    .lock()
                    .unwrap_or_else(|e| e.into_inner());
                if let Some(entry) = map.get_mut(&group) {
                    entry.list = None;
                }
            }
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

    pub(super) fn artifact_responses_inner(&self, artifact_id: &str) -> Vec<ArtifactResponse> {
        let Some(a) = self.account() else {
            return Vec::new();
        };
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

    /// 重试: asks the member in the run's 话题 to run again (a normal
    /// message that mentions it).
    pub(super) fn retry_member_problem_inner(
        &self,
        member_id: &str,
        run_id: &str,
    ) -> AdminReply<()> {
        let Some(a) = self.account() else {
            return reply_now(Err(AdminError::Unsupported));
        };
        let week_ago = (crate::selftest::clock::now_ms() - 14 * 24 * 3_600_000).max(0) as u64;
        let run = a
            .client
            .runs_since(week_ago)
            .unwrap_or_default()
            .into_iter()
            .find(|r| r.id == run_id && r.bot_id == member_id);
        let Some(run) = run else {
            return reply_now(Err(AdminError::Unsupported));
        };
        let name = self.member(member_id).map(|m| m.name).unwrap_or_default();
        self.send(SendRequest {
            group_id: run.group_id,
            topic_id: run.topic_id,
            body: fill(cm::MEMBERS_RETRY_MESSAGE, &[("name", &name)]),
            mentions: vec![member_id.to_string()],
            quote_of: None,
        });
        reply_now(Ok(()))
    }
}
