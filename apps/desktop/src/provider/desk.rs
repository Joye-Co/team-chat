//! The 首页 (desk track, 1.1.0) for [`super::CoreProvider`]: 需要你拍板 with the
//! card of each ask, and v2 card answers. Everything is read from the local
//! cache; the desk view keeps a snapshot and rebuilds it on a change, so
//! these reads are not on the frame path.

use std::{collections::BTreeMap, sync::Arc};

use chat_core::proto::{self, CardV2, CardV2Block, MemberLevel};

use super::CoreProvider;
use crate::data::{
    Change, TodoReason, TopicStatus,
    card::{ActionStyle, CardTone},
    v2::{AskItem, AskKind, CardAnswer},
};

/// Messages of a waiting 话题 searched for its newest card.
const ASK_SCAN: usize = 30;

/// The kind of decision a v2 ask wants, from its blocks.
pub fn kind_of_v2(card: &CardV2) -> AskKind {
    let mut kind = None;
    for b in &card.blocks {
        let k = match b {
            CardV2Block::Decision { .. } => Some(AskKind::Choice),
            CardV2Block::Draft { .. } => Some(AskKind::Draft),
            CardV2Block::Diff { .. } => Some(AskKind::Review),
            CardV2Block::Ref {
                target: proto::RefTarget::Pr { .. },
                ..
            } => Some(AskKind::Review),
            _ => None,
        };
        kind = kind.or(k);
    }
    kind.unwrap_or(AskKind::Approval)
}

/// The kind of decision a v1 card wants: plain answers are a choice.
pub fn kind_of_v1(card: &proto::Card) -> AskKind {
    let actions = card.actions.as_deref().unwrap_or(&[]);
    let plain = actions
        .iter()
        .all(|a| a.style.unwrap_or_default() == ActionStyle::Default && a.confirm.is_none());
    if plain && actions.len() >= 2 && card.tone != Some(CardTone::Warning) {
        AskKind::Choice
    } else {
        AskKind::Approval
    }
}

impl CoreProvider {
    /// 需要你拍板: each 话题 that waits on me, with its newest card that I
    /// have not answered yet (a v2 ask I may answer, else a v1 card with
    /// buttons from someone else). Without one it is a plain 话题 ask.
    pub(super) fn desk_asks(&self) -> Vec<AskItem> {
        let Some(a) = self.account() else {
            return Vec::new();
        };
        let me = a.me.id.as_str();
        let owner = a.me.level == MemberLevel::Owner;
        let mut out = Vec::new();
        for t in self.todo_items() {
            if t.reason != TodoReason::WaitingOnMe {
                continue;
            }
            let real = self.resolve_topic(&t.topic.id);
            let messages = a.client.messages(&real, None, ASK_SCAN).unwrap_or_default();
            let card = messages.iter().rev().find_map(|m| {
                if m.is_deleted() || m.author_id == me {
                    return None;
                }
                if let Some(c) = &m.card_v2 {
                    let open = c.ask.as_ref().is_some_and(|ask| {
                        ask.allows(me, owner, Some(t.topic.created_by.as_str()))
                    });
                    let answered = a
                        .client
                        .card_answers(&m.id)
                        .unwrap_or_default()
                        .iter()
                        .any(|r| r.actor_id == me && r.rev == c.rev);
                    return (open && !answered).then(|| (m, c.title.clone(), kind_of_v2(c)));
                }
                let c = m.card.as_ref()?;
                let has_buttons = c.actions.as_ref().is_some_and(|x| !x.is_empty());
                let pressed = self
                    .shared
                    .cards
                    .presses(&a.client, &m.id)
                    .iter()
                    .any(|p| p.actor_id == me);
                (has_buttons && !pressed).then(|| (m, c.title.clone(), kind_of_v1(c)))
            });
            out.push(match card {
                Some((m, title, kind)) => AskItem {
                    group_id: t.group_id.clone(),
                    topic_id: t.topic.id.clone(),
                    message_id: Some(m.id.clone()),
                    from: m.author_id.clone(),
                    kind,
                    title,
                    at: m.created_at.min(i64::MAX as u64) as i64,
                },
                None => AskItem {
                    group_id: t.group_id.clone(),
                    topic_id: t.topic.id.clone(),
                    message_id: None,
                    from: t
                        .topic
                        .owner_id
                        .clone()
                        .unwrap_or_else(|| t.topic.created_by.clone()),
                    kind: match t.topic.status {
                        TopicStatus::WaitingReview => AskKind::Review,
                        _ => AskKind::Topic,
                    },
                    title: t.topic.title.clone(),
                    at: t.topic.last_activity_at,
                },
            });
        }
        out.sort_by_key(|x| std::cmp::Reverse(x.at));
        out
    }

    pub(super) fn desk_card_v2(&self, message_id: &str) -> Option<Arc<CardV2>> {
        let a = self.account()?;
        let m = a.client.store().message(message_id).ok().flatten()?;
        if m.is_deleted() {
            return None;
        }
        m.card_v2.map(Arc::new)
    }

    pub(super) fn desk_card_v2_answers(&self, topic_id: &str, message_id: &str) -> Vec<CardAnswer> {
        let Some(a) = self.account() else {
            return Vec::new();
        };
        let mut out: Vec<CardAnswer> = a
            .client
            .card_answers(message_id)
            .unwrap_or_default()
            .into_iter()
            .map(|r| CardAnswer {
                actor_id: r.actor_id,
                action_id: r.action_id,
                inputs: r.inputs.unwrap_or_default(),
                at: r.at.min(i64::MAX as u64) as i64,
                pending: false,
            })
            .collect();
        let real = self.resolve_topic(topic_id);
        for (id, _rev, action_id, failed) in
            a.client.pending_card_answers(&real).unwrap_or_default()
        {
            if id == message_id && !failed {
                out.push(CardAnswer {
                    actor_id: a.me.id.clone(),
                    action_id,
                    inputs: BTreeMap::new(),
                    at: chrono::Local::now().timestamp_millis(),
                    pending: true,
                });
            }
        }
        out
    }

    pub(super) fn desk_answer_card_v2(
        &self,
        group_id: &str,
        topic_id: &str,
        message_id: &str,
        rev: u32,
        action_id: &str,
        inputs: BTreeMap<String, serde_json::Value>,
    ) {
        let Some(a) = self.account() else {
            return;
        };
        let inputs = (!inputs.is_empty()).then_some(inputs);
        if let Err(error) = a
            .client
            .answer_card(message_id, rev, action_id, inputs, None)
        {
            tracing::warn!("desk: could not answer a card: {error}");
        }
        self.emit(vec![Change::Messages {
            group_id: group_id.to_string(),
            topic_id: topic_id.to_string(),
        }]);
    }
}
