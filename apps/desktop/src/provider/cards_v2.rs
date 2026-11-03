//! Card v2 for [`super::CoreProvider`] (the cards v2 track owns this file):
//! an in-memory cache of v2 cards, their stored answers and my answer on
//! its way.
//!
//! Views read cards on every frame, so repeated reads hit memory:
//! - Cards are refreshed from the messages the timeline reads
//!   ([`CardV2Cache::remember`]); the `Arc` of a card stays the same while
//!   the card does not change.
//! - Answers come from the store once per message and are dropped from the
//!   cache when a `card.answered` event arrives.
//! - My answer is an outbox row (`card_answer`). While it is queued the card
//!   shows it as sent; a refusal is kept in memory and the row is removed.

use std::{
    collections::HashMap,
    sync::{Arc, Mutex, MutexGuard},
};

use chat_core::{
    Client, OutboxState,
    proto::{self, ErrorCode},
};

use crate::data::{
    CardPressError, Millis,
    cards_v2::{CardV2Answer, CardV2AnswerRequest, CardV2Message, CardV2Pending, CardV2SendPhase},
};

const MAX_ENTRIES: usize = 5000;

#[derive(Default)]
pub struct CardV2Cache {
    inner: Mutex<Inner>,
}

#[derive(Default)]
struct Inner {
    cards: HashMap<String, Option<CardV2Message>>,
    answers: HashMap<String, Arc<Vec<CardV2Answer>>>,
    mine: HashMap<String, Mine>,
    /// Messages whose queued answer was looked up in the outbox (after a restart `mine` is
    /// empty while the outbox still holds my answer).
    seeded: std::collections::HashSet<String>,
}

struct Mine {
    request: CardV2AnswerRequest,
    outbox_id: String,
    failed: Option<CardPressError>,
    at: Millis,
}

fn bound<K, V>(map: &mut HashMap<K, V>) {
    if map.len() >= MAX_ENTRIES {
        map.clear();
    }
}

fn millis(v: u64) -> Millis {
    v.min(i64::MAX as u64) as Millis
}

fn message_of(m: &proto::Message, old: Option<&CardV2Message>) -> Option<CardV2Message> {
    if m.is_deleted() {
        return None;
    }
    let card = m.card_v2.as_ref()?;
    let card = match old {
        Some(o) if *o.card == *card => o.card.clone(),
        _ => Arc::new(card.clone()),
    };
    Some(CardV2Message {
        group_id: m.group_id.clone(),
        topic_id: m.topic_id.clone(),
        message_id: m.id.clone(),
        author_id: m.author_id.clone(),
        created_at: millis(m.created_at),
        edited_at: m.edited_at().map(millis),
        card,
    })
}

pub(crate) fn error_of(code: ErrorCode) -> CardPressError {
    match code {
        ErrorCode::NotFound | ErrorCode::Invalid | ErrorCode::Conflict => CardPressError::Gone,
        ErrorCode::Forbidden => CardPressError::Forbidden,
        _ => CardPressError::Other,
    }
}

fn error_of_text(code: &str) -> CardPressError {
    match code {
        "not_found" | "invalid" | "conflict" => CardPressError::Gone,
        "forbidden" => CardPressError::Forbidden,
        _ => CardPressError::Other,
    }
}

/// True when a `card.answered` to `rev` ends my answer `m`.
fn answered_clears(m: &Mine, mine: bool, rev: u32) -> bool {
    m.request.rev == rev && (mine || m.failed.is_some())
}

impl CardV2Cache {
    fn lock(&self) -> MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn clear(&self) {
        *self.lock() = Inner::default();
    }

    /// Refreshes the cards of messages just read from the store.
    pub fn remember(&self, messages: &[proto::Message]) {
        let mut inner = self.lock();
        for m in messages.iter().filter(|m| !m.id.is_empty()) {
            let old = inner.cards.get(&m.id).cloned().flatten();
            let new = message_of(m, old.as_ref());
            bound(&mut inner.cards);
            inner.cards.insert(m.id.clone(), new);
        }
    }

    pub fn card(&self, client: &Client, message_id: &str) -> Option<CardV2Message> {
        if let Some(c) = self.lock().cards.get(message_id) {
            return c.clone();
        }
        let m = client.store().message(message_id).ok().flatten();
        let c = m.as_ref().and_then(|m| message_of(m, None));
        let mut inner = self.lock();
        bound(&mut inner.cards);
        inner.cards.insert(message_id.to_string(), c.clone());
        c
    }

    pub fn answers(&self, client: &Client, message_id: &str) -> Vec<CardV2Answer> {
        if let Some(a) = self.lock().answers.get(message_id) {
            return a.as_ref().clone();
        }
        let rows = client.card_answers(message_id).unwrap_or_default();
        let list: Vec<CardV2Answer> = rows
            .into_iter()
            .map(|r| CardV2Answer {
                actor_id: r.actor_id,
                rev: r.rev,
                action_id: r.action_id,
                inputs: r.inputs,
                edited_values: r.edited_values,
                at: millis(r.at),
            })
            .collect();
        let mut inner = self.lock();
        bound(&mut inner.answers);
        inner
            .answers
            .insert(message_id.to_string(), Arc::new(list.clone()));
        list
    }

    /// A `card.answered` arrived: answers are read again. My answer to that
    /// rev is done when it is mine, or when mine was refused (another
    /// member's answer won). One still on its way stays until the server
    /// refuses it, so the refusal is known as mine.
    pub fn on_answered(&self, message_id: &str, mine: bool, rev: u32) {
        let mut inner = self.lock();
        inner.answers.remove(message_id);
        if inner
            .mine
            .get(message_id)
            .is_some_and(|m| answered_clears(m, mine, rev))
        {
            inner.mine.remove(message_id);
        }
    }

    /// Messages of a topic changed (a card edit among them): read its cards
    /// again.
    pub fn forget_topic(&self, topic_id: &str) {
        self.lock()
            .cards
            .retain(|_, c| c.as_ref().is_none_or(|c| c.topic_id != topic_id));
    }

    /// My answer queued before the app started: the outbox row, read once per message.
    fn seed_from_outbox(&self, client: &Client, message_id: &str) {
        {
            let mut inner = self.lock();
            if inner.mine.contains_key(message_id) || inner.seeded.contains(message_id) {
                return;
            }
            if inner.seeded.len() >= MAX_ENTRIES {
                inner.seeded.clear();
            }
            inner.seeded.insert(message_id.to_string());
        }
        let found = client
            .store()
            .outbox_all()
            .unwrap_or_default()
            .into_iter()
            .rev()
            .find_map(|item| match item.op {
                chat_core::OutboxOp::CardAnswer {
                    message_id: m,
                    rev,
                    action_id,
                    inputs,
                    edited_values,
                    ..
                } if m == message_id => Some(Mine {
                    request: CardV2AnswerRequest {
                        rev,
                        action_id,
                        inputs,
                        edited_values,
                    },
                    outbox_id: item.id,
                    failed: None,
                    at: millis(item.created_at),
                }),
                _ => None,
            });
        if let Some(mine) = found {
            let mut inner = self.lock();
            bound(&mut inner.mine);
            inner.mine.entry(message_id.to_string()).or_insert(mine);
        }
    }

    pub fn pending(&self, client: &Client, message_id: &str) -> Option<CardV2Pending> {
        self.seed_from_outbox(client, message_id);
        let (request, outbox_id, failed, at) = {
            let inner = self.lock();
            let m = inner.mine.get(message_id)?;
            (m.request.clone(), m.outbox_id.clone(), m.failed, m.at)
        };
        let pending = |phase| CardV2Pending {
            rev: request.rev,
            action_id: request.action_id.clone(),
            inputs: request.inputs.clone(),
            edited_values: request.edited_values.clone(),
            phase,
            at,
        };
        if let Some(e) = failed {
            // Refused after another member's answer to this rev was stored:
            // that answer shows, and mine is done.
            let taken = self
                .lock()
                .answers
                .get(message_id)
                .is_some_and(|a| a.iter().any(|x| x.rev == request.rev));
            if taken {
                self.lock().mine.remove(message_id);
                return None;
            }
            return Some(pending(CardV2SendPhase::Failed(e)));
        }
        match client.store().outbox_get(&outbox_id).ok().flatten() {
            Some(item) if item.state == OutboxState::Failed => Some(pending(
                CardV2SendPhase::Failed(error_of_text(item.last_error.as_deref().unwrap_or(""))),
            )),
            Some(_) => Some(pending(CardV2SendPhase::Sending)),
            None => {
                // Acked: the stored answer arrives as `card.answered`; keep
                // showing it as sent until then, unless it is already there.
                let stored = self
                    .lock()
                    .answers
                    .get(message_id)
                    .is_some_and(|a| a.iter().any(|x| x.rev == request.rev));
                if stored {
                    self.lock().mine.remove(message_id);
                    None
                } else {
                    Some(pending(CardV2SendPhase::Sending))
                }
            }
        }
    }

    /// Queues my answer. An answer of this card that is still queued wins.
    pub fn answer(&self, client: &Client, message_id: &str, request: CardV2AnswerRequest) {
        if let Some(p) = self.pending(client, message_id)
            && p.phase == CardV2SendPhase::Sending
            && p.rev == request.rev
        {
            return;
        }
        let at = crate::selftest::clock::now_ms();
        let result = client.answer_card(
            message_id,
            request.rev,
            &request.action_id,
            request.inputs.clone(),
            request.edited_values.clone(),
        );
        let mut inner = self.lock();
        bound(&mut inner.mine);
        let (outbox_id, failed) = match result {
            Ok(id) => (id, None),
            Err(e) => {
                tracing::warn!(error = %e, "card answer refused");
                (String::new(), Some(CardPressError::Gone))
            }
        };
        inner.mine.insert(
            message_id.to_string(),
            Mine {
                request,
                outbox_id,
                failed,
                at,
            },
        );
    }

    /// Takes back my queued answer, or forgets a refused one.
    pub fn withdraw(&self, client: &Client, message_id: &str) {
        let Some(m) = self.lock().mine.remove(message_id) else {
            return;
        };
        if !m.outbox_id.is_empty()
            && client
                .store()
                .outbox_get(&m.outbox_id)
                .ok()
                .flatten()
                .is_some()
            && let Err(e) = client.discard(&m.outbox_id)
        {
            tracing::warn!(error = %e, "cannot take back a card answer");
        }
    }

    /// The server refused outbox row `outbox_id`. True when it was my answer
    /// (the row is then removed and the error kept here).
    pub fn on_refused(&self, client: &Client, outbox_id: &str, code: ErrorCode) -> bool {
        let found = {
            let mut inner = self.lock();
            match inner.mine.values_mut().find(|m| m.outbox_id == outbox_id) {
                Some(m) => {
                    m.failed = Some(error_of(code));
                    true
                }
                None => false,
            }
        };
        if found && let Err(e) = client.discard(outbox_id) {
            tracing::warn!(error = %e, "cannot drop a refused card answer");
        }
        found
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn refusals_map_to_errors() {
        assert_eq!(error_of(ErrorCode::Conflict), CardPressError::Gone);
        assert_eq!(error_of(ErrorCode::Forbidden), CardPressError::Forbidden);
        assert_eq!(error_of_text("rate_limited"), CardPressError::Other);
    }

    #[test]
    fn an_answer_to_my_rev_ends_my_refused_or_stored_answer() {
        let m = |failed| Mine {
            request: CardV2AnswerRequest {
                rev: 2,
                action_id: "ok".into(),
                inputs: None,
                edited_values: None,
            },
            outbox_id: "o1".into(),
            failed,
            at: 1,
        };
        // Mine was stored.
        assert!(answered_clears(&m(None), true, 2));
        // Another member won and mine was refused with `conflict`.
        assert!(answered_clears(&m(Some(CardPressError::Gone)), false, 2));
        // Another member won while mine is still on its way: keep it until
        // the refusal comes, so the refusal is matched to my answer.
        assert!(!answered_clears(&m(None), false, 2));
        // An answer to another rev does not end mine.
        assert!(!answered_clears(&m(Some(CardPressError::Gone)), false, 3));
    }

    #[test]
    fn an_unchanged_card_keeps_its_arc() {
        let m: proto::Message = serde_json::from_value(serde_json::json!({
            "id": "m1", "clientMsgId": "c1", "groupId": "g", "topicId": "t", "authorId": "b",
            "body": "", "mentions": [], "createdSeq": 1, "createdAt": 5,
            "cardV2": { "v": 2, "rev": 1, "intent": "info", "title": "t", "fallback": "", "blocks": [] }
        }))
        .expect("message parses");
        let a = message_of(&m, None).expect("v2 card");
        let b = message_of(&m, Some(&a)).expect("v2 card");
        assert!(Arc::ptr_eq(&a.card, &b.card));
        assert_eq!(a.created_at, 5);
    }
}
