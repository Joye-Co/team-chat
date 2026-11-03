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
    CardActionKind, CardIntent, CardV2, CardV2Action, CardV2ActionStyle, CardV2Block, ChartKind,
    ChartSeries, DecisionOption, EvidenceState, InputKind, InputOption, MetricBetter, RefTarget,
    RunPlanItem, RunPlanState,
};

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
