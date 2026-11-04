//! The 首页 (desk track, 1.1.0): the kind of decision an ask wants. 需要你拍板 itself is
//! built in `snapshot.rs`; v2 card reads and answers live in `cards_v2.rs`.

use chat_core::proto::{self, CardV2, CardV2Block};

use super::CoreProvider;
use crate::data::{
    ChatProvider as _,
    card::{ActionStyle, CardTone},
    v2::{AskKind, DeskNote, QueuedItem},
};

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
                    at: r.queued_at.map(|at| at.min(i64::MAX as u64) as i64),
                }
            })
            .collect()
    }
}
