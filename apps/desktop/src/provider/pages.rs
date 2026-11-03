//! The 群, 成员 and 产出 pages on the chat-core cache (pages track):
//! 群里的决定 derived from answered asks marked `record`, 页面里的回应 from
//! `artifact.evented`, page events declared by bot-made versions and
//! forwarded as `artifact.event`, and 重试 for a stuck run.

use chat_core::{
    DecisionRow,
    proto::{CardV2, CardV2Block},
};

use super::CoreProvider;
use crate::{
    copy::{artifact_page as ca, fill, group_page as cg, members_page as cm},
    data::{
        AdminError, AdminReply, ChatProvider as _, SendRequest, pages::ArtifactResponse, reply_now,
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

impl CoreProvider {
    pub(super) fn group_decisions_inner(&self, group_id: &str) -> Vec<Decision> {
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
        if let Err(e) = a
            .client
            .send_artifact_event(artifact_id, version, name, payload)
        {
            tracing::warn!(error = %e, "产出 page event dropped");
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
        a.client
            .artifact_events(artifact_id)
            .unwrap_or_default()
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
                    at: Some(e.at.min(i64::MAX as u64) as i64),
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
        let week_ago =
            (chrono::Local::now().timestamp_millis() - 14 * 24 * 3_600_000).max(0) as u64;
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
