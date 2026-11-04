//! My Card v2.1 events for [`super::CoreProvider`] (the inc-cards21 track owns this file):
//! what I sent with `card.event`, its ack, and the author's response (INTERACT §3, §4.4).
//!
//! The log lives in memory for the session, at most [`PER_CARD`] events per card. Views read
//! it on every frame through `card_v2_events`, so reads clone a short list. A live event that
//! never left (not connected) fails at once with [`CardEventFailure::Offline`]; an answer is
//! an outbox row that chat-core resends with the same `clientEventId`.

use std::{
    collections::HashMap,
    sync::{Mutex, MutexGuard},
};

use chat_core::proto::{CardEventAck, ErrorBody, InteractionResolvedTransient, InteractionResult};

use crate::data::{
    CardPressError, Millis,
    cards_v2::{
        CardEventFailure, CardEventPhase, CardEventRequest, CardV2Pending, CardV2SendPhase, Cause,
        MyCardEvent,
    },
};

/// Events kept per card.
pub const PER_CARD: usize = 20;
/// Cards kept.
const MAX_CARDS: usize = 500;

#[derive(Default)]
pub struct CardEventLog {
    inner: Mutex<Inner>,
}

#[derive(Default)]
struct Inner {
    /// Message id → my events, oldest first.
    cards: HashMap<String, Vec<MyCardEvent>>,
    /// Server event id → message id (for `interaction.resolved`).
    by_event: HashMap<String, String>,
}

/// The failure of an `ack.error`.
pub fn failure_of(e: &ErrorBody) -> CardEventFailure {
    let code = serde_json::to_value(e.code)
        .ok()
        .and_then(|v| v.as_str().map(str::to_string))
        .unwrap_or_default();
    CardEventFailure::from_error(&code, e.reason.as_deref())
}

impl CardEventLog {
    fn lock(&self) -> MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn clear(&self) {
        *self.lock() = Inner::default();
    }

    /// Records an event that left (or was queued). A newer live event of the same block
    /// ends the pending state of the older ones (latest wins).
    pub fn sent(
        &self,
        message_id: &str,
        client_event_id: String,
        request: CardEventRequest,
        durable: bool,
        at: Millis,
    ) {
        let mut inner = self.lock();
        if inner.cards.len() >= MAX_CARDS && !inner.cards.contains_key(message_id) {
            inner.cards.clear();
            inner.by_event.clear();
        }
        let list = inner.cards.entry(message_id.to_string()).or_default();
        supersede(list, &request);
        list.push(MyCardEvent {
            client_event_id,
            request,
            durable,
            sent_at: at,
            phase: CardEventPhase::Sending,
            dropped: false,
            reply: None,
        });
        if list.len() > PER_CARD {
            let extra = list.len() - PER_CARD;
            list.drain(..extra);
        }
    }

    /// An event could not be made or sent.
    pub fn failed_now(
        &self,
        message_id: &str,
        request: CardEventRequest,
        failure: CardEventFailure,
        at: Millis,
    ) {
        let id = format!("local-{at}-{}", request.event);
        self.sent(message_id, id.clone(), request, false, at);
        self.set_phase(message_id, &id, CardEventPhase::Failed { failure, at });
    }

    fn set_phase(&self, message_id: &str, client_event_id: &str, phase: CardEventPhase) -> bool {
        let mut inner = self.lock();
        let Some(e) = inner
            .cards
            .get_mut(message_id)
            .and_then(|l| l.iter_mut().find(|e| e.client_event_id == client_event_id))
        else {
            return false;
        };
        e.phase = phase;
        true
    }

    /// `card.event` ack. True when it was one of mine.
    pub fn acked(
        &self,
        message_id: &str,
        client_event_id: &str,
        result: &Result<CardEventAck, ErrorBody>,
        at: Millis,
    ) -> bool {
        let phase = match result {
            Ok(ack) => {
                self.lock()
                    .by_event
                    .insert(ack.event_id.clone(), message_id.to_string());
                CardEventPhase::Acked {
                    ack: ack.clone(),
                    at,
                }
            }
            Err(e) => CardEventPhase::Failed {
                failure: failure_of(e),
                at,
            },
        };
        self.set_phase(message_id, client_event_id, phase)
    }

    /// The author answered one of my events. Returns the message id.
    pub fn resolved(&self, t: &InteractionResolvedTransient, at: Millis) -> Option<String> {
        let mut inner = self.lock();
        let message_id = inner
            .by_event
            .get(&t.event_id)
            .cloned()
            .or_else(|| t.message_id.clone())?;
        let e = inner.cards.get_mut(&message_id)?.iter_mut().find(
            |e| matches!(&e.phase, CardEventPhase::Acked { ack, .. } if ack.event_id == t.event_id),
        )?;
        e.phase = CardEventPhase::Resolved {
            result: t.result,
            note: t.note.clone(),
            at,
        };
        Some(message_id)
    }

    /// A new rev or message named one of my events as its cause.
    pub fn caused(&self, message_id: &str, cause: &Cause, at: Millis) -> bool {
        self.caused_by(message_id, cause, at, None)
    }

    /// Like [`caused`](Self::caused), and remembers the new message `reply` that named it
    /// (the block offers a jump to it, INTERACT §4.4).
    fn caused_by(&self, message_id: &str, cause: &Cause, at: Millis, reply: Option<&str>) -> bool {
        let mut inner = self.lock();
        let Some(list) = inner.cards.get_mut(message_id) else {
            return false;
        };
        let Some(e) = list.iter_mut().find(
            |e| matches!(&e.phase, CardEventPhase::Acked { ack, .. } if ack.event_id == cause.event_id),
        ) else {
            return false;
        };
        e.phase = CardEventPhase::Resolved {
            result: InteractionResult::Patched,
            note: None,
            at,
        };
        e.reply = reply.map(str::to_string);
        true
    }

    /// A new message named an event as its cause: find the card by the event id.
    pub fn caused_anywhere(&self, cause: &Cause, at: Millis, reply: &str) -> bool {
        let Some(message_id) = self.lock().by_event.get(&cause.event_id).cloned() else {
            return false;
        };
        self.caused_by(&message_id, cause, at, Some(reply))
    }

    pub fn events(&self, message_id: &str) -> Vec<MyCardEvent> {
        self.lock()
            .cards
            .get(message_id)
            .cloned()
            .unwrap_or_default()
    }

    /// The event, by id.
    pub fn get(&self, message_id: &str, client_event_id: &str) -> Option<MyCardEvent> {
        self.lock()
            .cards
            .get(message_id)?
            .iter()
            .find(|e| e.client_event_id == client_event_id)
            .cloned()
    }

    /// 不等了.
    pub fn drop_pending(&self, message_id: &str, client_event_id: &str) {
        let mut inner = self.lock();
        if let Some(e) = inner
            .cards
            .get_mut(message_id)
            .and_then(|l| l.iter_mut().find(|e| e.client_event_id == client_event_id))
        {
            e.dropped = true;
        }
    }

    /// 重试 with the same id: back to sending.
    pub fn resending(&self, message_id: &str, client_event_id: &str, at: Millis) {
        let mut inner = self.lock();
        if let Some(e) = inner
            .cards
            .get_mut(message_id)
            .and_then(|l| l.iter_mut().find(|e| e.client_event_id == client_event_id))
        {
            e.phase = CardEventPhase::Sending;
            e.sent_at = at;
            e.dropped = false;
        }
    }
}

/// My 2.1 answer while it is queued or after it was refused, in the shape the card state
/// reads (`CardV2Pending`). An acked answer shows from the stored answers.
pub fn pending_answer(events: &[MyCardEvent]) -> Option<CardV2Pending> {
    let e = events.iter().rev().find(|e| e.durable && !e.dropped)?;
    let phase = match &e.phase {
        CardEventPhase::Sending => CardV2SendPhase::Sending,
        CardEventPhase::Failed { failure, .. } => CardV2SendPhase::Failed(match failure {
            CardEventFailure::Conflict => CardPressError::Gone,
            CardEventFailure::Forbidden => CardPressError::Forbidden,
            _ => CardPressError::Other,
        }),
        _ => return None,
    };
    Some(CardV2Pending {
        rev: e.request.rev,
        action_id: e
            .request
            .action_id
            .clone()
            .or_else(|| e.request.block_id.clone())
            .unwrap_or_default(),
        inputs: e.request.inputs.clone(),
        edited_values: None,
        phase,
        at: e.sent_at,
    })
}

/// Latest wins: an older live event of the same block stops waiting.
fn supersede(list: &mut [MyCardEvent], request: &CardEventRequest) {
    if request.block_id.is_none() {
        return;
    }
    for e in list.iter_mut().filter(|e| {
        !e.durable && e.request.block_id == request.block_id && e.request.event == request.event
    }) {
        e.dropped = true;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chat_core::proto::{ErrorCode, Expect, ExpectMode};

    fn req(block: &str) -> CardEventRequest {
        CardEventRequest {
            rev: 1,
            block_id: Some(block.into()),
            event: "chart.select".into(),
            ..Default::default()
        }
    }

    fn ack(id: &str) -> CardEventAck {
        CardEventAck {
            event_id: id.into(),
            stored: false,
            answer_id: None,
            duplicate: None,
            expect: Expect {
                by: "bot".into(),
                mode: ExpectMode::Live,
                eta_ms: Some(2000),
            },
        }
    }

    #[test]
    fn a_newer_event_of_the_same_block_wins() {
        let log = CardEventLog::default();
        log.sent("m", "c1".into(), req("bars"), false, 1);
        log.sent("m", "c2".into(), req("bars"), false, 2);
        log.sent("m", "c3".into(), req("other"), false, 3);
        let ev = log.events("m");
        assert!(ev[0].dropped);
        assert!(!ev[1].dropped && !ev[2].dropped);
    }

    #[test]
    fn ack_then_resolve_or_cause_ends_pending() {
        let log = CardEventLog::default();
        log.sent("m", "c1".into(), req("bars"), false, 1);
        assert!(log.acked("m", "c1", &Ok(ack("EV1")), 2));
        let t = InteractionResolvedTransient {
            group_id: "g".into(),
            event_id: "EV1".into(),
            result: InteractionResult::Ignored,
            message_id: None,
            note: Some("没有 10-14 之前的数".into()),
        };
        assert_eq!(log.resolved(&t, 3).as_deref(), Some("m"));
        assert!(matches!(
            log.events("m")[0].phase,
            CardEventPhase::Resolved {
                result: InteractionResult::Ignored,
                ..
            }
        ));
        log.sent("m", "c2".into(), req("x"), false, 4);
        log.acked("m", "c2", &Ok(ack("EV2")), 5);
        let cause = Cause {
            event_id: "EV2".into(),
            ..Default::default()
        };
        assert!(log.caused("m", &cause, 6));
        assert_eq!(log.events("m")[1].reply, None, "a rev of the card itself");
        // A new message that names my event: the block can jump to it.
        log.sent("m", "c3".into(), req("y"), false, 7);
        log.acked("m", "c3", &Ok(ack("EV3")), 8);
        let cause = Cause {
            event_id: "EV3".into(),
            ..Default::default()
        };
        assert!(log.caused_anywhere(&cause, 9, "msg-reply"));
        assert_eq!(log.events("m")[2].reply.as_deref(), Some("msg-reply"));
    }

    #[test]
    fn refusals_are_classified() {
        let log = CardEventLog::default();
        log.sent("m", "c1".into(), req("bars"), false, 1);
        let err = ErrorBody {
            code: ErrorCode::Conflict,
            message: String::new(),
            reason: Some("card.stale_rev".into()),
        };
        log.acked("m", "c1", &Err(err), 2);
        assert!(matches!(
            log.events("m")[0].phase,
            CardEventPhase::Failed {
                failure: CardEventFailure::Conflict,
                ..
            }
        ));
        let off = ErrorBody {
            code: ErrorCode::Internal,
            message: String::new(),
            reason: Some("client.offline".into()),
        };
        assert_eq!(failure_of(&off), CardEventFailure::Offline);
    }

    #[test]
    fn the_log_keeps_the_newest_events() {
        let log = CardEventLog::default();
        for i in 0..(PER_CARD + 5) {
            log.sent("m", format!("c{i}"), req("b"), false, i as Millis);
        }
        let ev = log.events("m");
        assert_eq!(ev.len(), PER_CARD);
        assert_eq!(
            ev.last().unwrap().client_event_id,
            format!("c{}", PER_CARD + 4)
        );
    }
}
