//! The 首页 (desk track, 1.1.0) for [`super::CoreProvider`]: 需要你拍板 with the
//! card of each ask. v2 card reads and answers live in `cards_v2.rs`. Everything is read from the local
//! cache; the desk view keeps a snapshot and rebuilds it on a change, so
//! these reads are not on the frame path.

use chat_core::proto::{self, CardV2, CardV2Block, MemberLevel};

use super::CoreProvider;
use crate::data::{
    ChatProvider as _, TodoReason, TopicStatus,
    card::{ActionStyle, CardTone},
    v2::{AskItem, AskKind, DeskNote, QueuedItem},
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
    /// Jojo's one-line priority (首页): the newest `desk.noted` of the default 单独问 member
    /// (Jojo, else the first bot) in any 群. None when it has no note.
    pub(super) fn desk_note_inner(&self) -> Option<DeskNote> {
        let a = self.account()?;
        let who = self.side_chat_default_member()?;
        let notes = a.client.desk_notes().unwrap_or_default();
        let n = notes.into_iter().find(|n| n.member_id == who)?;
        Some(DeskNote {
            member_id: n.member_id,
            text: n.text,
            at: n.at.min(i64::MAX as u64) as i64,
        })
    }

    /// 排队: runs that wait for a free machine (created, no machine, `queuedAt`), in queue
    /// order per member.
    pub(super) fn queue_inner(&self) -> Vec<QueuedItem> {
        let Some(a) = self.account() else {
            return Vec::new();
        };
        let mut runs: Vec<_> = a
            .client
            .active_runs()
            .unwrap_or_default()
            .into_iter()
            .filter(|r| r.is_queued())
            .collect();
        runs.sort_by_key(|r| {
            (
                r.queue_position.unwrap_or(u32::MAX),
                r.queued_at.unwrap_or(r.created_at),
            )
        });
        let mut titles = std::collections::HashMap::new();
        runs.into_iter()
            .map(|r| {
                let title = titles
                    .entry(r.group_id.clone())
                    .or_insert_with(|| self.topics(&r.group_id))
                    .iter()
                    .find(|t| t.id == r.topic_id)
                    .map(|t| t.title.clone())
                    .unwrap_or_default();
                QueuedItem {
                    member_id: r.bot_id,
                    title,
                    at: None,
                }
            })
            .collect()
    }

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
}
