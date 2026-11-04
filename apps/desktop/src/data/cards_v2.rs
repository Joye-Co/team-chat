//! Card v2 view types (docs/protocol.md "Cards v2"; the cards v2 track owns
//! this file).
//!
//! A v2 card travels in `Message.cardV2` next to a server-derived v1 card.
//! The provider hands views the card with its message context, the answers
//! the server stored (`card.answered`) and my answer while it is on its way.
//! Views derive the state of the card from these (see
//! `views::cards::v2::state`); nothing here is computed on the server.

use std::sync::Arc;

pub use chat_core::proto::{
    ASK_FROM_OWNERS, ASK_FROM_REQUESTER, AnswerEdits, AnswerInputs, AskIrreversible,
    CardActionKind, CardIntent, CardWorkStatus, CardV2, CardV2Action, CardV2ActionStyle, CardV2Block,
    ChartKind,
    ChartSeries, DecisionOption, EvidenceState, InputKind, InputOption, MetricBetter, RefTarget,
    RunPlanItem, RunPlanState,
};
// --- inc-cards21: Card v2.1 (containers, choice variants, the interaction loop) ---
#[allow(unused_imports)]
pub use chat_core::proto::{
    AskStatus, BlockBase, ButtonStyle, CardAnswer as WireCardAnswer, CardEventAck,
    CardState as WireCardState, Cause, ComparePreview, Deliver, EmitBy, EmitByKeyword, Emphasis,
    Expect, ExpectMode, InteractionResult, MetricTone, PartKey, PartRef, PollVoters, Slot,
    TableSelect,
};
// --- end inc-cards21 ---

use super::{CardPressError, GroupId, MemberId, MessageId, Millis, TopicId};

/// A v2 card with the message it belongs to.
#[derive(Clone, Debug, PartialEq)]
pub struct CardV2Message {
    pub group_id: GroupId,
    pub topic_id: TopicId,
    pub message_id: MessageId,
    /// The card's author (the member that asks or reports).
    pub author_id: MemberId,
    pub created_at: Millis,
    /// The time of the last edit, when the card was edited.
    pub edited_at: Option<Millis>,
    pub card: Arc<CardV2>,
    // --- inc-cards21 ---
    /// Server-owned answer state (`Message.cardState`, 2.1).
    pub card_state: Option<Arc<WireCardState>>,
    /// The event the newest rev answered (`Message.cause`, 2.1).
    pub cause: Option<Cause>,
    // --- end inc-cards21 ---
}

/// One stored answer to a v2 card (`card.answered`).
#[derive(Clone, Debug, PartialEq)]
pub struct CardV2Answer {
    pub actor_id: MemberId,
    /// The rev of the card that was answered.
    pub rev: u32,
    pub action_id: String,
    pub inputs: Option<AnswerInputs>,
    pub edited_values: Option<AnswerEdits>,
    pub at: Millis,
}

/// Where my answer is while the server has not stored it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CardV2SendPhase {
    /// Queued or on the wire.
    Sending,
    /// The server refused it.
    Failed(CardPressError),
}

/// My answer to a card while it is on its way, or after the server refused it.
#[derive(Clone, Debug, PartialEq)]
pub struct CardV2Pending {
    pub rev: u32,
    pub action_id: String,
    pub inputs: Option<AnswerInputs>,
    pub edited_values: Option<AnswerEdits>,
    pub phase: CardV2SendPhase,
    pub at: Millis,
}

/// An answer to send (`card.answer`).
#[derive(Clone, Debug, PartialEq, Default)]
pub struct CardV2AnswerRequest {
    /// The rev of the card the answer was made on.
    pub rev: u32,
    pub action_id: String,
    pub inputs: Option<AnswerInputs>,
    pub edited_values: Option<AnswerEdits>,
}

// --- inc-cards21: my card events (INTERACT §3, §4.4) ---

/// One interaction to send (`card.event`). Exactly one of `block_id` / `action_id`.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct CardEventRequest {
    /// The rev I saw.
    pub rev: u32,
    pub block_id: Option<String>,
    pub action_id: Option<String>,
    pub event: String,
    pub value: Option<serde_json::Value>,
    /// Only with `action.press` of a submit or reject action.
    pub inputs: Option<AnswerInputs>,
    pub part: Option<PartRef>,
}

/// Why the server (or this client) refused one of my events.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CardEventFailure {
    /// The card changed: `card.stale_rev`, `card.superseded`, `card.already_answered`,
    /// `card.expired`, `card.block_gone`. Re-render the latest rev and say so.
    Conflict,
    /// It never left this client (not connected).
    Offline,
    Forbidden,
    RateLimited,
    Other,
}

impl CardEventFailure {
    /// From `ack.error`: the code and the specific reason.
    pub fn from_error(code: &str, reason: Option<&str>) -> Self {
        match reason.unwrap_or("") {
            "client.offline" => return Self::Offline,
            r if r.starts_with("card.") && r != "card.undeclared_event" => {
                if matches!(
                    r,
                    "card.stale_rev"
                        | "card.superseded"
                        | "card.already_answered"
                        | "card.expired"
                        | "card.block_gone"
                ) {
                    return Self::Conflict;
                }
            }
            _ => {}
        }
        match code {
            "conflict" | "not_found" => Self::Conflict,
            "forbidden" => Self::Forbidden,
            "rate_limited" => Self::RateLimited,
            _ => Self::Other,
        }
    }
}

/// Where one of my events is.
#[derive(Clone, Debug, PartialEq)]
pub enum CardEventPhase {
    /// On its way (a live event) or queued (an answer).
    Sending,
    /// The server took it; the author responds per `ack.expect`.
    Acked { ack: CardEventAck, at: Millis },
    /// Refused, or it never left.
    Failed {
        failure: CardEventFailure,
        at: Millis,
    },
    /// The author responded (`interaction.resolved`, or a rev / message that names it).
    Resolved {
        result: InteractionResult,
        note: Option<String>,
        at: Millis,
    },
}

/// One event I sent on a card, newest last.
#[derive(Clone, Debug, PartialEq)]
pub struct MyCardEvent {
    pub client_event_id: String,
    pub request: CardEventRequest,
    /// An answer (kept in the outbox and resent with the same id).
    pub durable: bool,
    pub sent_at: Millis,
    pub phase: CardEventPhase,
    /// 不等了: the pending state was dropped (the local selection stays).
    pub dropped: bool,
    /// v11/pb-cards: the new message that named this event as its cause.
    pub reply: Option<String>,
}

impl MyCardEvent {
    /// The server's event id, once acked.
    pub fn event_id(&self) -> Option<&str> {
        match &self.phase {
            CardEventPhase::Acked { ack, .. } => Some(ack.event_id.as_str()),
            _ => None,
        }
    }
}
// --- end inc-cards21 ---
