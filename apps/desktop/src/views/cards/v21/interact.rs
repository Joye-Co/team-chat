//! The interaction loop on the desktop (INTERACT §3, §4.4): sending `card.event`, the
//! pending state the emitter sees, conflicts, the attribution line and the change motion.
//!
//! Rules kept here:
//! - A block sends a live event only when the name is in its `events`; commits follow from
//!   `deliver`, the ask and the footer actions (§3.2, §3.3).
//! - One live event in flight per block; a newer one waits and replaces a waiting older one
//!   (latest wins, §3.8). Text sends 600 ms after the last keystroke.
//! - Pending shows only to the emitter: nothing for 400 ms, then 等 {name} 回应, then
//!   {name} 还在处理, then {name} 没有回应 with 重试 and 不等了 (§4.4, soft and hard from
//!   `expect.mode`, §6.4).
//! - `expect.mode: none` on a live event means nobody will respond: nothing waits. The
//!   block goes back to its state and the band says {name} 接不了 with `expect.reason`. A
//!   reason that lasts (or no reason) leaves the block read-only for that rev with no 重试;
//!   after `run.limit` and `run.failed` the block stays usable and the words go after 4 s.
//! - A live event that comes back offline or busy puts the block back to its state before
//!   the gesture (INTERACT §4.4) while the band says why for 4 s.
//! - So does every other live event that did not get through: one the server refused
//!   (forbidden: 这一步你现在不能做; anything else: 没发出去，再试一次 with 重试), one the
//!   author answered with `failed` ({name} 没能处理这一步 with 重试, or its note) or
//!   `ignored`, and one refused because the card changed (no words here: the conflict
//!   Toast says it). The optimistic state never stays as if the step had worked.
//! - An answer the author has had for a while says so at the record: {name} 还没开始, then
//!   {name} 一直没接这一步 with 提醒一下 ([`answer_stage`]).
//! - A rev that changes a block I typed in keeps my text and says 内容变了 · 用新的.
//! - When the member answers my event with a new message whose row is not drawn (off
//!   screen), the block says {name} 回了一条新消息 and jumps to it.
//! - A conflict re-renders the latest rev, keeps local values whose ids still exist and
//!   toasts 卡片刚更新了，请再看一眼 once.
//! - A top-level block a new rev removed stays where it was and fades out (opacity only).
//! - A block a new rev inserted or changed fades in with a live tint that fades out, and
//!   gets 按 {name} 的选择 · {time} (按你的选择 for my own cause). Reduce motion: no
//!   animation.

use std::{collections::HashMap, sync::Arc, time::Duration};

use crate::data::page_host::NoneReason;

use gpui::{
    AnyElement, App, Entity, IntoElement, ParentElement, Styled, Window, div,
    prelude::FluentBuilder as _, px,
};
use serde_json::Value;

use super::{BlockUi, CardUi, Node};
use crate::{
    copy::{cards21 as c, fill},
    data::{
        ChatProvider, MemberLevel, Millis,
        cards_v2::{
            CardEventFailure, CardEventPhase, CardEventRequest, CardV2, CardV2Block, EmitBy,
            EmitByKeyword, ExpectMode, InteractionResult, MyCardEvent, PartRef,
        },
    },
    theme::{self, Type as _, card_v2::mix, card_v21 as t, space},
    ui,
    views::cards::v2::{Ctx, blocks as b20, form::FormSnap, meta},
};

// ---------------------------------------------------------------------------
// Who may send what

/// Events that need no declaration: they follow from `deliver`, the ask and the actions.
fn commit(card: &CardV2, b: &CardV2Block, event: &str) -> bool {
    let immediate = b.base().is_some_and(|x| x.is_immediate());
    match event {
        "choice.submit" => {
            immediate || matches!(b, CardV2Block::Confirm { .. } | CardV2Block::Poll { .. })
        }
        "input.submit" => immediate,
        "compare.pick" => matches!(b, CardV2Block::Compare { .. }) && card.ask.is_some(),
        _ => false,
    }
}

/// True when block `b` sends `event` (declared, or a commit).
pub fn sends(card: &CardV2, b: &CardV2Block, event: &str) -> bool {
    b.base()
        .is_some_and(|x| x.events().iter().any(|e| e == event))
        || commit(card, b, event)
}

/// True when `me` may send the live events of block `b` (INTERACT §3.5). Answers follow
/// `ask.from` (the card state's `live`).
pub fn may_emit(ctx: &Ctx, b: &CardV2Block) -> bool {
    let Some(me) = ctx.me.as_deref() else {
        return false;
    };
    let Some(id) = b.id() else { return false };
    let provider = &ctx.env.provider;
    let member = provider.member(me);
    if member.as_ref().is_some_and(|m| m.is_bot() || m.deactivated) {
        return false;
    }
    let on_surface = ctx.card.surface_ids().contains(id);
    let by = b.base().and_then(|x| x.emit_by.clone());
    let resolved = ctx
        .msg
        .card_state
        .as_ref()
        .and_then(|s| s.emitters.as_ref())
        .and_then(|e| e.get(id));
    // The author said this block is not interactive (`expect.mode: none`, INTERACT §4.4).
    if refused(
        &provider.card_v2_events(&ctx.msg.group_id, &ctx.msg.message_id),
        id,
        ctx.card.rev,
    ) {
        crate::selftest::drawn::mark("card-refused", ctx.msg.message_id.clone());
        return false;
    }
    if let Some(ids) = resolved {
        return ids.iter().any(|x| x == me);
    }
    match by {
        Some(EmitBy::Members { member_ids }) => member_ids.iter().any(|x| x == me),
        Some(EmitBy::Keyword(EmitByKeyword::Members)) => true,
        Some(EmitBy::Keyword(EmitByKeyword::Owners)) => {
            member.is_some_and(|m| m.level == MemberLevel::Owner)
        }
        Some(EmitBy::Keyword(EmitByKeyword::Requester)) => provider
            .topic(&ctx.msg.group_id, &ctx.msg.topic_id)
            .is_some_and(|t| t.created_by == me),
        Some(EmitBy::Keyword(EmitByKeyword::Ask)) => ctx.live(),
        Some(EmitBy::Keyword(EmitByKeyword::Unknown)) => false,
        None if on_surface && ctx.card.ask.is_some() => ctx.live(),
        None => true,
    }
}

// ---------------------------------------------------------------------------
// Sending

/// Sends the events of one card.
#[derive(Clone)]
pub struct Emitter {
    provider: Arc<dyn ChatProvider>,
    group_id: String,
    topic_id: String,
    message_id: String,
    card: Arc<CardV2>,
    ui: Entity<CardUi>,
    may: HashMap<String, bool>,
}

impl Emitter {
    pub fn of(ctx: &Ctx) -> Self {
        let may = ctx
            .card
            .blocks
            .iter()
            .filter_map(|b| b.id().map(|id| (id.to_string(), may_emit(ctx, b))))
            .collect();
        Self {
            provider: ctx.env.provider.clone(),
            group_id: ctx.msg.group_id.clone(),
            topic_id: ctx.msg.topic_id.clone(),
            message_id: ctx.msg.message_id.clone(),
            card: ctx.card.clone(),
            ui: ctx.ui.clone(),
            may,
        }
    }

    /// Sends `event` of block `block` when the block sends it and I may: now, or after the
    /// one in flight on that block (a newer one replaces a waiting one).
    pub fn send(
        &self,
        block: &str,
        event: &str,
        value: Value,
        part: Option<PartRef>,
        cx: &mut App,
    ) {
        let Some(b) = self.card.block(block) else {
            return;
        };
        if !sends(&self.card, b, event) || !self.may.get(block).copied().unwrap_or(false) {
            return;
        }
        let request = CardEventRequest {
            rev: self.card.rev,
            block_id: Some(block.to_string()),
            action_id: None,
            event: event.to_string(),
            value: Some(value),
            inputs: None,
            part,
        };
        let busy = self
            .provider
            .card_v2_events(&self.group_id, &self.message_id)
            .iter()
            .any(|e| {
                !e.durable
                    && e.request.block_id.as_deref() == Some(block)
                    && e.phase == CardEventPhase::Sending
            });
        if busy {
            self.ui.update(cx, |u, cx| {
                u.queued.insert(block.to_string(), request);
                cx.notify();
            });
            return;
        }
        self.provider
            .send_card_event(&self.group_id, &self.topic_id, &self.message_id, request);
    }

    /// Sends `event` 600 ms after the last call for this block (text inputs).
    pub fn send_later(&self, block: &str, event: &str, value: Value, wait: Duration, cx: &mut App) {
        let generation = self.ui.update(cx, |u, _| {
            let g = u.debounce.entry(block.to_string()).or_default();
            *g += 1;
            *g
        });
        let (me, block, event) = (self.clone(), block.to_string(), event.to_string());
        cx.spawn(async move |cx: &mut gpui::AsyncApp| {
            cx.background_executor().timer(wait).await;
            cx.update(|cx| {
                if me.ui.read(cx).debounce.get(&block) == Some(&generation) {
                    me.send(&block, &event, value, None, cx);
                }
            });
        })
        .detach();
    }

    /// A press of a footer action (`action.press`), with `inputs` for submit and reject.
    pub fn press(
        &self,
        action_id: &str,
        inputs: Option<crate::data::cards_v2::AnswerInputs>,
        _cx: &mut App,
    ) -> Option<String> {
        self.provider.send_card_event(
            &self.group_id,
            &self.topic_id,
            &self.message_id,
            CardEventRequest {
                rev: self.card.rev,
                block_id: None,
                action_id: Some(action_id.to_string()),
                event: "action.press".into(),
                value: None,
                inputs,
                part: None,
            },
        )
    }

    /// Sends the commit of an immediate block (an answer when the card asks).
    pub fn commit(&self, block: &str, event: &str, value: Value, cx: &mut App) {
        self.send(block, event, value, None, cx);
    }
}

// ---------------------------------------------------------------------------
// Pending (only the emitter sees it)

/// How long the author has had my event.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stage {
    /// Under 400 ms: the optimistic state only.
    Quiet,
    /// 等 {name} 回应.
    Wait,
    /// {name} 还在处理.
    Slow,
    /// {name} 没有回应: 重试, 不等了.
    Silent,
    /// My stored answer will not be taken (`expect.mode: none` with a reason): {name} 接不了.
    RefusedAnswer,
    /// `expect.mode: offline` for a live event: the step was not sent (4 s).
    OfflineLive,
    /// The author is offline; my answer is stored and they will see it.
    OfflineAnswer,
    /// Refused because of limits: 现在接不了，稍后再试 (4 s).
    Busy,
    /// `expect.mode: none` for a live event: the block was not interactive, or the author is
    /// over its limits (the ack does not say which). The state goes back, the block is
    /// read-only for that rev and says {name} 现在接不了，稍后再试 for 4 s, with no 重试.
    Refused,
    /// The server refused the step: I may not do it (4 s, no 重试).
    Forbidden,
    /// The step did not get to the author (the server refused it for another reason, or
    /// the client could not send it): 没发出去，再试一次 with 重试 (4 s).
    Rejected,
    /// The author answered `failed` with no note: {name} 没能处理这一步 with 重试 (4 s).
    Unhandled,
}

/// My newest event on a card that still waits for the author.
#[derive(Clone, Debug)]
pub struct Pending {
    pub event: MyCardEvent,
    pub stage: Stage,
    pub mode: ExpectMode,
    /// When the next stage starts (ms on the self-test clock), if one does.
    pub next_at: Option<Millis>,
}

fn waits(mode: ExpectMode) -> Option<(Duration, Duration)> {
    match mode {
        ExpectMode::Live => Some((t::SOFT_LIVE, t::HARD_LIVE)),
        ExpectMode::Run => Some((t::SOFT_RUN, t::HARD_RUN)),
        ExpectMode::Starting => Some((t::SOFT_STARTING, t::HARD_STARTING)),
        _ => None,
    }
}

fn ms(d: Duration) -> Millis {
    d.as_millis() as Millis
}

/// Why the author will not take event `e`, when its ack said (`expect.reason`).
pub fn none_reason(e: &MyCardEvent) -> Option<NoneReason> {
    match &e.phase {
        CardEventPhase::Acked { ack, .. } if ack.expect.mode == ExpectMode::None => {
            NoneReason::parse(ack.expect.reason.as_deref())
        }
        _ => None,
    }
}

/// The pending state of one event at `now`, or `None` when it no longer waits.
pub fn stage_of(e: &MyCardEvent, now: Millis) -> Option<Pending> {
    // A 撤回 is not a step the author answers: the record shows it ([`retract_state`]).
    if e.dropped || e.request.event == ANSWER_RETRACT {
        return None;
    }
    match &e.phase {
        CardEventPhase::Acked { ack, at } => {
            if e.durable || ack.stored {
                // An answer follows the run (CARD21 §2); only an author that will not take
                // it is said: offline, or `none` with the reason why.
                let stage = match ack.expect.mode {
                    ExpectMode::Offline => Stage::OfflineAnswer,
                    // A passing reason says nothing about a stored answer: it stays stored,
                    // and there is no step to try again.
                    ExpectMode::None if none_reason(e).is_some_and(|r| !r.retryable()) => {
                        Stage::RefusedAnswer
                    }
                    _ => return None,
                };
                return Some(Pending {
                    event: e.clone(),
                    stage,
                    mode: ack.expect.mode,
                    next_at: None,
                });
            }
            let age = now - at;
            let (stage, next_at) = match ack.expect.mode {
                ExpectMode::Offline => {
                    if age >= ms(t::OFFLINE_NOTE) {
                        return None;
                    }
                    (Stage::OfflineLive, Some(at + ms(t::OFFLINE_NOTE)))
                }
                // The block was not interactive (INTERACT §4.4): its state goes back once
                // and it is read-only from now on (`refused`); no band, no retry.
                // A reason that lasts stays said: the block is read-only because of it.
                ExpectMode::None => {
                    if none_reason(e).is_some_and(|r| !r.retryable()) {
                        (Stage::Refused, None)
                    } else if age >= ms(t::OFFLINE_NOTE) {
                        return None;
                    } else {
                        (Stage::Refused, Some(at + ms(t::OFFLINE_NOTE)))
                    }
                }
                ExpectMode::Unknown => return None,
                mode => {
                    let (soft, hard) = waits(mode)?;
                    if age < ms(t::PENDING_QUIET) {
                        (Stage::Quiet, Some(at + ms(t::PENDING_QUIET)))
                    } else if age < ms(soft) {
                        (Stage::Wait, Some(at + ms(soft)))
                    } else if age < ms(hard) {
                        (Stage::Slow, Some(at + ms(hard)))
                    } else {
                        (Stage::Silent, None)
                    }
                }
            };
            Some(Pending {
                event: e.clone(),
                stage,
                mode: ack.expect.mode,
                next_at,
            })
        }
        CardEventPhase::Failed { failure, at } if !e.durable => match failure {
            CardEventFailure::Offline if now - at < ms(t::OFFLINE_NOTE) => Some(Pending {
                event: e.clone(),
                stage: Stage::OfflineLive,
                mode: ExpectMode::Offline,
                next_at: Some(at + ms(t::OFFLINE_NOTE)),
            }),
            CardEventFailure::RateLimited if now - at < ms(t::OFFLINE_NOTE) => Some(Pending {
                event: e.clone(),
                stage: Stage::Busy,
                mode: ExpectMode::Unknown,
                next_at: Some(at + ms(t::OFFLINE_NOTE)),
            }),
            CardEventFailure::Forbidden | CardEventFailure::Other
                if now - at < ms(t::OFFLINE_NOTE) =>
            {
                Some(Pending {
                    event: e.clone(),
                    stage: if *failure == CardEventFailure::Forbidden {
                        Stage::Forbidden
                    } else {
                        Stage::Rejected
                    },
                    mode: ExpectMode::Unknown,
                    next_at: Some(at + ms(t::OFFLINE_NOTE)),
                })
            }
            // A conflict has its own Toast and header line ([`conflict_toast`]).
            _ => None,
        },
        // The author could not handle the step and said nothing: the block says so.
        CardEventPhase::Resolved {
            result: InteractionResult::Failed,
            note: None,
            at,
        } if !e.durable && now - at < ms(t::OFFLINE_NOTE) => Some(Pending {
            event: e.clone(),
            stage: Stage::Unhandled,
            mode: ExpectMode::Unknown,
            next_at: Some(at + ms(t::OFFLINE_NOTE)),
        }),
        _ => None,
    }
}

/// True while a live event that ended with no stage of its own still has its optimistic
/// state to put back: the author ignored it or failed at it (its note says why), or the
/// card changed under it. Only for [`t::OFFLINE_NOTE`] after it ended, so an old event
/// never undoes what I did later.
fn ended_unsaid(e: &MyCardEvent, now: Millis) -> bool {
    let at = match &e.phase {
        CardEventPhase::Failed {
            failure: CardEventFailure::Conflict,
            at,
        } => at,
        CardEventPhase::Resolved {
            result: InteractionResult::Ignored | InteractionResult::Failed,
            at,
            ..
        } => at,
        _ => return false,
    };
    !e.dropped && now - at < ms(t::OFFLINE_NOTE)
}

/// How long the author has had my answer (a durable, stored event) without starting.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AnswerStage {
    /// {name} 还没开始.
    Slow,
    /// {name} 一直没接这一步: 提醒一下.
    Silent,
}

fn answer_waits(mode: ExpectMode) -> Option<(Duration, Duration)> {
    match mode {
        ExpectMode::Live => Some((t::SOFT_ANSWER_LIVE, t::HARD_ANSWER_LIVE)),
        ExpectMode::Run => Some((t::SOFT_ANSWER_RUN, t::HARD_ANSWER_RUN)),
        ExpectMode::Starting => Some((t::SOFT_ANSWER_STARTING, t::HARD_ANSWER_STARTING)),
        _ => None,
    }
}

/// The stage of my newest answer on the card at `now`, and when the next one starts.
/// `None` while it is fresh, once the author responded to it (resolved, a new rev or a
/// message that names it), when I dropped it, or when the author is offline (the record
/// says that already). The caller stops asking once the card has a status or a newer rev.
pub fn answer_stage(events: &[MyCardEvent], now: Millis) -> (Option<AnswerStage>, Option<Millis>) {
    let Some(e) = events
        .iter()
        .rev()
        .find(|e| e.durable && !e.dropped && e.request.event != ANSWER_RETRACT)
    else {
        return (None, None);
    };
    let CardEventPhase::Acked { ack, at } = &e.phase else {
        return (None, None);
    };
    if e.reply.is_some() {
        return (None, None);
    }
    answer_stage_since(*at, ack.expect.mode, now)
}

/// True when the log holds an answer of mine on the card (a durable event that is not a
/// 撤回): its ack, not a guess, says how the author takes it.
pub fn has_answer(events: &[MyCardEvent]) -> bool {
    events
        .iter()
        .any(|e| e.durable && !e.dropped && e.request.event != ANSWER_RETRACT)
}

/// True while the server still takes back an answer stored at `at` ([`t::RETRACT_WINDOW`],
/// the server's own window; it has the last word).
pub fn retract_open(at: Millis, now: Millis) -> bool {
    now - at < ms(t::RETRACT_WINDOW)
}

/// [`answer_stage`] of an answer the author has had since `at`, expected in `mode` (a
/// 2.0 answer has no ack that says: it waits like a run).
pub fn answer_stage_since(
    at: Millis,
    mode: ExpectMode,
    now: Millis,
) -> (Option<AnswerStage>, Option<Millis>) {
    let Some((soft, hard)) = answer_waits(mode) else {
        return (None, None);
    };
    let age = now - at;
    if age < ms(soft) {
        (None, Some(at + ms(soft)))
    } else if age < ms(hard) {
        (Some(AnswerStage::Slow), Some(at + ms(hard)))
    } else {
        (Some(AnswerStage::Silent), None)
    }
}

/// How long after my answer anything in its 话题 counts as the member picking it up (the
/// answer itself moves the 话题's activity).
const PICKED_UP_AFTER: Millis = 5_000;

/// True when the author picked my answer of `at` up: it started a run in the 话题 since
/// (`runs`: when each of its runs there was made), or the 话题 moved on (`activity`: its
/// newest activity). A card that was acted on never says 一直没接这一步.
pub fn picked_up(at: Millis, runs: &[Millis], activity: Millis) -> bool {
    runs.iter().any(|made| made + PICKED_UP_AFTER >= at) || activity > at + PICKED_UP_AFTER
}

/// The event name of taking an answer back (`card.event answer.retract`).
pub const ANSWER_RETRACT: &str = "answer.retract";

/// Where my 撤回 of an answer is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Retract {
    /// On its way: the button spins.
    Sending,
    /// The server refused it because the answer can no longer be taken back: the member
    /// has started.
    Refused,
    /// It did not go through for another reason (not sent, too many at once): 撤回 stays.
    NotSent,
}

/// The state of my newest 撤回 on the card, if it still shows: on its way, or refused. An
/// accepted one shows nothing here: the answer goes and the card asks again.
pub fn retract_state(events: &[MyCardEvent]) -> Option<Retract> {
    let e = events
        .iter()
        .rev()
        .find(|e| e.request.event == ANSWER_RETRACT && !e.dropped)?;
    match &e.phase {
        CardEventPhase::Sending => Some(Retract::Sending),
        // `card.retract_closed` (and an answer or a card that is gone) is a conflict.
        CardEventPhase::Failed {
            failure: CardEventFailure::Conflict,
            ..
        } => Some(Retract::Refused),
        CardEventPhase::Failed { .. } => Some(Retract::NotSent),
        _ => None,
    }
}

/// The log without the refusals that no longer hold on the card at `rev`: `expect.mode: none`
/// said how things were at that press, so a newer rev of the card, or a newer event of mine the
/// author took, ends it.
pub fn current(events: &[MyCardEvent], rev: u32) -> Vec<MyCardEvent> {
    let took = |e: &MyCardEvent| {
        !e.dropped
            && match &e.phase {
                CardEventPhase::Acked { ack, .. } => matches!(
                    ack.expect.mode,
                    ExpectMode::Live | ExpectMode::Run | ExpectMode::Starting
                ),
                CardEventPhase::Resolved { .. } => true,
                _ => false,
            }
    };
    events
        .iter()
        .enumerate()
        .filter(|(i, e)| {
            let refusal = matches!(&e.phase, CardEventPhase::Acked { ack, .. }
                if ack.expect.mode == ExpectMode::None);
            !refusal || (e.request.rev >= rev && !events[i + 1..].iter().any(took))
        })
        .map(|(_, e)| e.clone())
        .collect()
}

/// The newest of my events on the card that still waits, if any (the band).
pub fn pending(events: &[MyCardEvent], now: Millis) -> Option<Pending> {
    events.iter().rev().find_map(|e| stage_of(e, now))
}

/// True when the author answered a live event of mine on `block` at `rev` with
/// `expect.mode: none`: the block is not interactive and draws read-only (INTERACT §4.4).
pub fn refused(events: &[MyCardEvent], block: &str, rev: u32) -> bool {
    events.iter().any(|e| {
        !e.durable
            && !e.dropped
            && e.request.rev == rev
            && e.request.block_id.as_deref() == Some(block)
            && matches!(&e.phase, CardEventPhase::Acked { ack, .. }
                if !ack.stored && ack.expect.mode == ExpectMode::None)
            // Worth another try: the block stays usable.
            && !none_reason(e).is_some_and(NoneReason::retryable)
    })
}

/// The newest of my events on block `block` that still waits, if any (the line at the
/// block). Another block's newer event does not hide it (INTERACT §4.4: only a newer
/// event from the same block ends it).
pub fn pending_at(events: &[MyCardEvent], block: &str, now: Millis) -> Option<Pending> {
    events
        .iter()
        .rev()
        .filter(|e| e.request.block_id.as_deref() == Some(block))
        .find_map(|e| stage_of(e, now))
}

/// The next time any waiting event of the card changes its stage.
pub fn next_stage(events: &[MyCardEvent], now: Millis) -> Option<Millis> {
    events
        .iter()
        .filter_map(|e| stage_of(e, now).and_then(|p| p.next_at))
        .min()
}

/// The author's note on my newest event of `block`, while it shows: why it failed stays
/// until my next event on that block (it is the only word on what went wrong); why it was
/// ignored shows for 5 s.
pub fn resolved_note(events: &[MyCardEvent], block: &str, now: Millis) -> Option<String> {
    let e = events
        .iter()
        .rev()
        .find(|e| e.request.block_id.as_deref() == Some(block))?;
    match &e.phase {
        CardEventPhase::Resolved {
            note: Some(n),
            result: InteractionResult::Failed,
            ..
        } if !e.dropped => Some(n.clone()),
        CardEventPhase::Resolved {
            note: Some(n),
            at,
            result: InteractionResult::Ignored,
        } if now - at < ms(t::RESOLVED_NOTE) => Some(n.clone()),
        _ => None,
    }
}

/// True while the author may still be working on the event (a spinner); false once it
/// will not come (no answer, offline, busy).
pub fn spins(stage: Stage) -> bool {
    matches!(stage, Stage::Quiet | Stage::Wait | Stage::Slow)
}

/// Words of a pending stage for the band.
pub fn stage_words(stage: Stage, name: &str) -> String {
    let t = match stage {
        Stage::Quiet | Stage::Wait => c::PENDING_WAIT,
        Stage::Slow => c::PENDING_SLOW,
        Stage::Silent => c::PENDING_NONE,
        Stage::OfflineLive => c::OFFLINE_LIVE,
        Stage::OfflineAnswer => c::OFFLINE_ANSWER,
        Stage::Busy => c::BUSY,
        // The ack of a short-run limit is `expect.mode: none` too: say so (INTERACT §6.2).
        Stage::Refused | Stage::RefusedAnswer => c::BUSY,
        Stage::Forbidden => c::REJECTED_FORBIDDEN,
        Stage::Rejected => c::REJECTED_NOT_SENT,
        Stage::Unhandled => c::RESOLVED_FAILED,
    };
    fill(t, &[("name", name)])
}

/// Runs after the body each frame: flushes a waiting event whose block is free and schedules a redraw at the next pending stage.
pub fn after_body(ctx: &Ctx, _window: &mut Window, cx: &mut App) {
    let events = ctx
        .env
        .provider
        .card_v2_events(&ctx.msg.group_id, &ctx.msg.message_id);
    let now = ctx.env.now();
    let ui = ctx.ui.clone();
    // Latest wins: the waiting event of a free block goes now.
    let free: Vec<String> = ui
        .read(cx)
        .queued
        .keys()
        .filter(|b| {
            !events.iter().any(|e| {
                !e.durable
                    && e.request.block_id.as_deref() == Some(b.as_str())
                    && e.phase == CardEventPhase::Sending
            })
        })
        .cloned()
        .collect();
    if !free.is_empty() {
        let emitter = Emitter::of(ctx);
        let (g, t, m) = (
            ctx.msg.group_id.clone(),
            ctx.msg.topic_id.clone(),
            ctx.msg.message_id.clone(),
        );
        let provider = ctx.env.provider.clone();
        cx.defer(move |cx| {
            let reqs: Vec<CardEventRequest> = emitter.ui.update(cx, |u, _| {
                free.iter().filter_map(|b| u.queued.remove(b)).collect()
            });
            for r in reqs {
                provider.send_card_event(&g, &t, &m, r);
            }
        });
    }
    settle_or_revert(ctx, &events, now, cx);
    // A redraw at the next stage.
    let reply_due = events
        .iter()
        .filter_map(|e| match &e.phase {
            CardEventPhase::Resolved { at, .. } if e.reply.is_some() => Some(at + ms(REPLY_QUIET)),
            _ => None,
        })
        .filter(|due| *due > now)
        .min();
    let next = match (next_stage(&events, now), reply_due) {
        (Some(a), Some(b)) => Some(a.min(b)),
        (a, b) => a.or(b),
    };
    if let Some(next) = next {
        let wait = Duration::from_millis((next - now).max(0) as u64 + 20);
        let due = std::time::Instant::now() + wait;
        let scheduled = ui.read(cx).wake_at;
        if scheduled
            .is_none_or(|w| w > due + Duration::from_millis(50) || w < std::time::Instant::now())
        {
            ui.update(cx, |u, _| u.wake_at = Some(due));
            cx.spawn(async move |cx: &mut gpui::AsyncApp| {
                cx.background_executor().timer(wait).await;
                cx.update(|cx| {
                    ui.update(cx, |u, cx| {
                        u.wake_at = None;
                        cx.notify();
                    })
                });
            })
            .detach();
        }
    }
}

/// True when the optimistic state of live event `e` must go back: the step was not sent
/// (offline, refused by the server), the author cannot take it now (busy,
/// `expect.mode: none`), could not handle it or ignored it, or the card changed under it.
pub fn undone(e: &MyCardEvent, now: Millis) -> bool {
    !e.durable
        && (stage_of(e, now).is_some_and(|p| {
            matches!(
                p.stage,
                Stage::OfflineLive
                    | Stage::Busy
                    | Stage::Refused
                    | Stage::Forbidden
                    | Stage::Rejected
                    | Stage::Unhandled
            )
        }) || ended_unsaid(e, now))
}

/// Blocks with a live event of mine on its way, or one whose state is still to go back.
pub fn in_flight(
    events: &[MyCardEvent],
    reverted: &std::collections::HashSet<String>,
    now: Millis,
) -> Vec<String> {
    events
        .iter()
        .filter(|e| {
            !e.durable
                && (e.phase == CardEventPhase::Sending
                    || (undone(e, now) && !reverted.contains(&e.client_event_id)))
        })
        .filter_map(|e| e.request.block_id.clone())
        .collect()
}

/// Keeps each quiet block's state as the one to go back to, and puts it back for a live
/// event that came back offline or busy (INTERACT §4.4: the optimistic state reverts).
fn settle_or_revert(ctx: &Ctx, events: &[MyCardEvent], now: Millis, cx: &mut App) {
    let ui = ctx.ui.read(cx);
    let mut busy = in_flight(events, &ui.reverted, now);
    busy.extend(ui.queued.keys().cloned());
    let form = ctx.form.read(cx);
    let mut settle = Vec::new();
    for id in ctx.card.blocks.iter().filter_map(CardV2Block::id) {
        if busy.iter().any(|b| b == id) {
            continue;
        }
        let mut state = (ui.block(id), form.snap(id));
        state.0.hot = None;
        if ui.settled.get(id) != Some(&state) {
            settle.push((id.to_string(), state));
        }
    }
    let undo: Vec<(String, String)> = events
        .iter()
        .filter(|e| undone(e, now) && !ui.reverted.contains(&e.client_event_id))
        .filter_map(|e| Some((e.client_event_id.clone(), e.request.block_id.clone()?)))
        .collect();
    if settle.is_empty() && undo.is_empty() {
        return;
    }
    let (ui, form) = (ctx.ui.clone(), ctx.form.clone());
    cx.defer(move |cx| {
        let back: Vec<(String, Option<(BlockUi, FormSnap)>)> = ui.update(cx, |u, cx| {
            u.settled.extend(settle);
            let back = undo
                .iter()
                .map(|(cid, block)| {
                    u.reverted.insert(cid.clone());
                    (block.clone(), u.settled.get(block).cloned())
                })
                .collect::<Vec<_>>();
            for (block, s) in &back {
                let hot = u.block(block).hot;
                let mut b = s.as_ref().map(|s| s.0.clone()).unwrap_or_default();
                b.hot = hot;
                u.blocks.insert(block.clone(), b);
            }
            cx.notify();
            back
        });
        for (block, s) in back {
            let snap = s.map(|s| s.1).unwrap_or_default();
            form.update(cx, |f, cx| f.restore(&block, &snap, cx));
        }
    });
}

// ---------------------------------------------------------------------------
// The jump to a reply off screen

/// Message rows the timelines drew in this session (a drawn row was on screen).
#[derive(Default)]
pub struct DrawnRows(std::collections::HashSet<String>);

impl gpui::Global for DrawnRows {}

/// Most rows kept; past it the set starts over (a reply drawn before is not a new one).
const DRAWN_MAX: usize = 4096;

/// A timeline drew the row of message `id`.
pub fn mark_drawn(id: &str, cx: &mut App) {
    let rows = &mut cx.default_global::<DrawnRows>().0;
    if rows.contains(id) {
        return;
    }
    if rows.len() >= DRAWN_MAX {
        rows.clear();
    }
    rows.insert(id.to_string());
}

/// The reply that ended my newest event on `block`, while its row was never drawn (it
/// is off screen), [`REPLY_QUIET`] after it came, so a reply that lands on screen in the
/// same frame never flashes the line.
pub fn reply_off_screen<'e>(
    events: &'e [MyCardEvent],
    block: &str,
    now: Millis,
    drawn: impl Fn(&str) -> bool,
) -> Option<&'e str> {
    let e = events
        .iter()
        .rev()
        .find(|e| e.request.block_id.as_deref() == Some(block))?;
    let CardEventPhase::Resolved { at, .. } = &e.phase else {
        return None;
    };
    let reply = e.reply.as_deref()?;
    (now - at >= ms(REPLY_QUIET) && !drawn(reply)).then_some(reply)
}

/// How long a reply may take to be drawn before the block offers the jump.
const REPLY_QUIET: Duration = Duration::from_millis(300);

/// {name} 回了一条新消息: a link at the block to the reply below, off screen.
fn reply_link(ctx: &Ctx, block: &str, reply: &str, cx: &mut App) -> AnyElement {
    let k = theme::colors(cx);
    let a = ctx.answerer();
    let reply = reply.to_string();
    meta(div())
        .mt(space::S6)
        .flex()
        .items_center()
        .gap(space::S6)
        .text_color(k.ink_2)
        .child(
            b20::quiet_button(
                ctx.id(format!("reply-{block}")),
                &fill(c::NEW_MESSAGE, &[("name", &ctx.author)]),
                cx,
            )
            .text_color(k.ink)
            // The ↓ sits in the button's icon slot, part of the one target (D1-70).
            .icon(crate::ui::icons::button_sym(
                ui::Sym::ArrowDown,
                crate::ui::icons::SymSize::for_slot(crate::ui::native::tokens::icon::MD),
                ui::native::PushSize::Small.height(),
            ))
            .on_click(move |_, window, cx| a.open_message(&reply, window, cx)),
        )
        .into_any_element()
}

// ---------------------------------------------------------------------------
// The block diff

/// Ids of blocks that `new` inserted or changed against `old`. A container whose only
/// change is its children list is not counted (its new children are).
pub fn changed_blocks(old: &CardV2, new: &CardV2) -> Vec<String> {
    let mut out = Vec::new();
    for b in &new.blocks {
        let Some(id) = b.id() else { continue };
        match old.block(id) {
            None => out.push(id.to_string()),
            Some(o) if o == b => {}
            Some(o) => {
                if b.is_container() && o.type_name() == b.type_name() && same_but_children(o, b) {
                    continue;
                }
                out.push(id.to_string());
            }
        }
    }
    out
}

/// Top-level blocks of `old` that `new` removed, each with the id of the nearest earlier
/// top-level block that `new` still has (`None`: at the start), so the body draws the
/// removed block where it was while it fades out.
pub fn removed_blocks(old: &CardV2, new: &CardV2) -> Vec<(Option<String>, CardV2Block)> {
    let tree = super::Tree::new(old);
    let mut after: Option<String> = None;
    let mut out = Vec::new();
    for b in &tree.top {
        let Some(id) = b.id() else { continue };
        if new.block(id).is_some() {
            after = Some(id.to_string());
        } else {
            out.push((after.clone(), (*b).clone()));
        }
    }
    out
}

/// True when two containers differ only in which children they name.
fn same_but_children(a: &CardV2Block, b: &CardV2Block) -> bool {
    let strip = |x: &CardV2Block| -> Value {
        let mut v = serde_json::to_value(x).unwrap_or(Value::Null);
        if let Some(o) = v.as_object_mut() {
            o.remove("children");
            o.remove("tabs");
            o.remove("options");
        }
        v
    };
    strip(a) == strip(b)
}

/// The block and the attribution line, the change tint, the pending line at the block,
/// the amber 问你 of a non-blocking ask's surface and a resolved note.
pub fn decorate(
    node: &Node,
    b: &CardV2Block,
    inner: AnyElement,
    _window: &mut Window,
    cx: &mut App,
) -> AnyElement {
    let ctx = node.ctx;
    let Some(id) = b.id() else { return inner };
    let k = theme::colors(cx);
    let now = ctx.env.now();
    let events = ctx
        .env
        .provider
        .card_v2_events(&ctx.msg.group_id, &ctx.msg.message_id);
    let diff = node.ui.diff.as_ref().filter(|d| d.to == ctx.card.rev);
    let changed = diff.is_some_and(|d| d.changed.contains_key(id))
        && !node
            .tree
            .parent(id)
            .is_some_and(|p| diff.is_some_and(|d| d.changed.contains_key(p)));
    let pend = pending_at(&current(&events, ctx.card.rev), id, now);
    let note = resolved_note(&events, id, now);
    let mark = super::band::surface_mark(ctx, node.tree, id);
    // Interact.dc.html 02 step 3: a block the rev inserted keeps a live tint and
    // `Ledger 刚加的` at its top right until the next rev; a changed block only flashes. Who
    // caused the rev is said once, in the band (`band::updated`).
    let inserted = changed && diff.is_some_and(|d| d.inserted.contains(id));
    let added = inserted.then(|| {
        meta(div())
            .flex_none()
            .ml_auto()
            .one_line()
            .text_color(k.ink_3)
            .child(fill(c::JUST_ADDED, &[("name", &ctx.author)]))
    });

    let mut col = div().flex().flex_col().min_w(px(0.));
    let mut added = added;
    if let Some(m) = mark {
        // The ask on a surface is a StatusBadge (§2.12), never a free dot.
        let (state, text) = match m {
            super::band::Mark::You => (
                crate::ui::native::BadgeState::NeedsYou,
                c::BAND_SURFACE_YOU.to_string(),
            ),
            super::band::Mark::Waiting(names) => (
                crate::ui::native::BadgeState::Queued,
                fill(
                    c::BAND_WAITING_ON,
                    &[("name", &crate::views::spaced_name(&names))],
                ),
            ),
        };
        col = col.child(
            div()
                .mb(space::S8)
                .flex()
                .items_center()
                .gap(space::S8)
                .child(crate::ui::native::StatusBadge::new(state, text))
                // The mark and 刚加的 share the row (the artboard's 问你 … Ledger 刚加的).
                .children(added.take()),
        );
    }
    col = col.child(inner);
    if let Some(p) = pend.as_ref().filter(|p| p.stage != Stage::Quiet) {
        // The band says the stage of my newest event; the block says it only for an older
        // event the band does not show (CARD21 §2: one state is said once).
        let in_band =
            super::band::pending(ctx).is_some_and(|b| b.event_id == p.event.client_event_id);
        if let Some(line) = pending_line(ctx, p, !in_band, cx) {
            col = col.child(line);
        }
    }
    let drawn = cx.try_global::<DrawnRows>();
    let reply = reply_off_screen(&events, id, now, |m| drawn.is_some_and(|d| d.0.contains(m)))
        .map(str::to_string);
    if let Some(r) = reply {
        col = col.child(reply_link(ctx, id, &r, cx));
    }
    if node.ui.drafts_kept.contains(id) {
        col = col.child(draft_changed(ctx, id, cx));
    }
    if let Some(n) = note {
        col = col.child(meta(div()).mt(space::S6).text_color(k.ink_2).child(fill(
            c::RESOLVED_NOTE,
            &[("name", &ctx.author), ("note", &n)],
        )));
    }
    if changed {
        if let Some(a) = added {
            // On the title row of a block that has one at its left, else on a line of its
            // own above the block.
            col = if titled(b) {
                div()
                    .relative()
                    .flex()
                    .flex_col()
                    .min_w(px(0.))
                    .child(col)
                    .child(div().absolute().top(px(5.)).right_0().child(a))
            } else {
                div()
                    .flex()
                    .flex_col()
                    .min_w(px(0.))
                    .child(div().flex().mb(space::S4).child(a))
                    .child(col)
            };
        }
        // Provenance is a 2 px line at the left and the muted label, never a state
        // color fill (§1.3). The line sits inside the card padding, on the content x,
        // and the block's text 10 in from it (C-09: never flush on the card border).
        let col = if inserted {
            div()
                .border_l_2()
                .border_color(k.border_strong)
                .pl(space::S10)
                .child(col)
        } else {
            col
        };
        // A top-level block spans the card's width (its 16 pt padding), as the artboard
        // draws it; a nested one gets a small rounded tint.
        let top = node.depth == 0;
        // A block right after another tinted one joins its run: the body drops the gap
        // between them and a hairline splits them.
        let after_tinted = top
            && diff.is_some_and(|d| {
                let pos = node.tree.top.iter().position(|x| x.id() == Some(id));
                pos.and_then(|i| i.checked_sub(1))
                    .and_then(|i| node.tree.top[i].id())
                    .is_some_and(|prev| d.changed.contains_key(prev))
            });
        let tinted = if top {
            div()
                .mx(-space::S16)
                .px(space::S16)
                .py(space::S12)
                .when(after_tinted, |el| el.border_t_1().border_color(k.line))
                .child(col)
        } else {
            div()
                .mx(px(-8.))
                .px(px(8.))
                .py(px(6.))
                .my(px(-6.))
                .rounded(theme::radius::MD)
                .child(col)
        };
        // A change flashes in the muted fill only and ends on the card itself (no
        // resting fill: the border and the label carry the provenance), the same
        // look as with Reduce Motion on.
        let rest = k.hover.opacity(0.);
        if cx.reduce_motion() {
            return tinted.into_any_element();
        }
        let tint = k.hover;
        let rev = ctx.card.rev;
        return crate::ui::motion::once(
            ctx.id(format!("chg-{id}-{rev}")),
            t::CHANGE_FLASH,
            tinted,
            move |el, p| {
                let fade_in = (p * (t::CHANGE_FLASH.as_secs_f32() / t::CHANGE_IN.as_secs_f32()))
                    .clamp(0., 1.);
                el.opacity(fade_in).bg(mix(tint, rest, 1. - p))
            },
        );
    }
    col.into_any_element()
}

/// True when a block starts with a title at its left and nothing at its right (the
/// attribution line can share that row).
fn titled(b: &CardV2Block) -> bool {
    // A group's head is 28 pt: the title and an open group's summary at the left; a
    // collapsed one puts its summary at the right.
    matches!(b, CardV2Block::Group { collapsed, .. } if *collapsed != Some(true))
}

/// 内容变了 · 用新的: the member changed a block while I had typed in it; my text stays
/// until I take the new one (INTERACT §4.4).
fn draft_changed(ctx: &Ctx, id: &str, cx: &mut App) -> AnyElement {
    let k = theme::colors(cx);
    let (form, ui, card, block) = (
        ctx.form.clone(),
        ctx.ui.clone(),
        ctx.card.clone(),
        id.to_string(),
    );
    meta(div())
        .mt(space::S6)
        .flex()
        .items_center()
        .gap(space::S6)
        .text_color(k.ink_2)
        .child(ui::icon(ui::Sym::Alert, px(12.), k.ink_2))
        .child(c::DRAFT_CHANGED)
        .child(
            b20::quiet_button(ctx.id(format!("use-new-{id}")), c::DRAFT_USE_NEW, cx)
                .text_color(k.ink)
                .on_click(move |_, _, cx| {
                    form.update(cx, |f, cx| f.use_new(&card, &block, cx));
                    ui.update(cx, |u, cx| {
                        u.use_new(&block);
                        cx.notify();
                    });
                }),
        )
        .into_any_element()
}

/// The line at a block while its event waits: a spinner and the stage when `words` (the
/// band does not say it), and 重试 and 不等了 once no answer came. `None`: nothing to add.
fn pending_line(ctx: &Ctx, p: &Pending, words: bool, cx: &mut App) -> Option<AnyElement> {
    // No answer came, or the author was offline: the line offers 重试
    // (§2.15: a failure always has an action).
    let retry = matches!(
        p.stage,
        Stage::Silent | Stage::OfflineLive | Stage::Rejected | Stage::Unhandled
    );
    if !words && !retry {
        return None;
    }
    let k = theme::colors(cx);
    // The line says the same reason the band would: 稍后再试 is not true of all of them.
    let text = match (p.stage, none_reason(&p.event)) {
        (Stage::Refused | Stage::RefusedAnswer, Some(r)) => fill(
            crate::copy::page_host::PAGE_NONE,
            &[
                ("name", &ctx.author),
                ("why", crate::views::artifacts::host::none_why(Some(r))),
            ],
        ),
        _ => stage_words(p.stage, &ctx.author),
    };
    let mut row = meta(div())
        .mt(space::S6)
        .flex()
        .items_center()
        .gap(space::S6)
        .text_color(k.ink_2)
        .when(
            words && matches!(p.stage, Stage::Wait | Stage::Slow),
            |el| el.child(ui::icon(ui::Sym::Waiting, px(12.), k.ink_3)),
        )
        .when(words, |el| {
            el.child(div().min_w(px(0.)).one_line().child(text))
        });
    if retry {
        crate::selftest::drawn::mark("card-retry", ctx.msg.message_id.clone());
        let (provider, g, tp, m, cid) = (
            ctx.env.provider.clone(),
            ctx.msg.group_id.clone(),
            ctx.msg.topic_id.clone(),
            ctx.msg.message_id.clone(),
            p.event.client_event_id.clone(),
        );
        let retry_req = p.event.request.clone();
        let (p2, g2, t2, m2, c2) = (
            provider.clone(),
            g.clone(),
            tp.clone(),
            m.clone(),
            cid.clone(),
        );
        row = row
            .child(
                b20::quiet_button(ctx.id(format!("retry-{cid}")), c::PENDING_RETRY, cx)
                    .text_color(k.ink)
                    .on_click(move |_, _, _| {
                        // A new event with the same value (INTERACT §4.4).
                        p2.drop_card_event(&g2, &t2, &m2, &c2);
                        p2.send_card_event(&g2, &t2, &m2, retry_req.clone());
                    }),
            )
            .when(p.stage == Stage::Silent, |el| {
                el.child(
                    b20::quiet_button(ctx.id(format!("stop-{cid}")), c::PENDING_STOP, cx)
                        .on_click(move |_, _, _| provider.drop_card_event(&g, &tp, &m, &cid)),
                )
            });
    }
    Some(row.into_any_element())
}

#[cfg(test)]
mod none_reason_tests {
    use super::*;
    use crate::views::cards::v21::{band, samples};

    fn none(reason: &str, stored: bool) -> MyCardEvent {
        let mut e = samples::acked_none("n", "bars", reason);
        e.durable = stored;
        if let CardEventPhase::Acked { ack, .. } = &mut e.phase {
            ack.stored = stored;
        }
        e
    }

    #[test]
    fn every_reason_has_its_own_words_and_ends_the_waiting() {
        for (reason, parsed, why) in [
            (
                "member.inactive",
                NoneReason::MemberInactive,
                c::NONE_MEMBER_INACTIVE,
            ),
            ("runner.off", NoneReason::RunnerOff, c::NONE_RUNNER_OFF),
            (
                "run.external",
                NoneReason::RunExternal,
                c::NONE_RUN_EXTERNAL,
            ),
            ("run.limit", NoneReason::RunLimit, c::NONE_RUN_LIMIT),
            ("run.failed", NoneReason::RunFailed, c::NONE_RUN_FAILED),
        ] {
            let e = none(reason, false);
            assert_eq!(none_reason(&e), Some(parsed));
            let p = stage_of(&e, 100).expect("the band says it");
            assert_eq!(p.stage, Stage::Refused);
            assert!(!spins(p.stage), "{reason}: nothing spins");
            let (_, detail, _) = band::pend_band(p.stage, none_reason(&e), "Ledger").unwrap();
            assert_eq!(detail, why);
        }
    }

    #[test]
    fn a_reason_this_version_does_not_know_is_skipped() {
        let e = none("run.someday", false);
        assert_eq!(none_reason(&e), None);
        let (_, detail, _) = band::pend_band(Stage::Refused, None, "Ledger").unwrap();
        assert_eq!(detail, c::BUSY_LATER);
    }

    #[test]
    fn a_lasting_reason_stays_said_and_a_passing_one_goes() {
        let late = ms(t::OFFLINE_NOTE) + 1;
        let lasting = stage_of(&none("runner.off", false), late).expect("still said");
        assert_eq!((lasting.stage, lasting.next_at), (Stage::Refused, None));
        assert!(stage_of(&none("run.limit", false), late).is_none());
    }

    #[test]
    fn only_a_passing_reason_leaves_the_block_usable() {
        assert!(refused(&[none("member.inactive", false)], "bars", 1));
        assert!(refused(&[none("run.someday", false)], "bars", 1));
        assert!(!refused(&[none("run.limit", false)], "bars", 1));
        assert!(!refused(&[none("run.failed", false)], "bars", 1));
    }

    #[test]
    fn a_stored_answer_says_nothing_of_a_reason_that_passes() {
        assert!(stage_of(&none("run.failed", true), 1).is_none());
        assert!(stage_of(&none("run.limit", true), 1).is_none());
    }

    #[test]
    fn a_refusal_ends_with_a_newer_rev_or_a_press_the_author_took() {
        let off = none("runner.off", false);
        assert!(pending(&current(&[off.clone()], 1), 1_000_000).is_some());
        assert!(pending(&current(&[off.clone()], 2), 1_000_000).is_none());
        let answered = none("runner.off", true);
        assert!(pending(&current(&[answered], 2), 1_000_000).is_none());
        let mut took = samples::acked_none("t", "other", "runner.off");
        if let CardEventPhase::Acked { ack, .. } = &mut took.phase {
            ack.expect.mode = ExpectMode::Run;
            ack.expect.reason = None;
        }
        let kept = current(&[off.clone(), took.clone()], 1);
        assert_eq!(kept.len(), 1);
        assert_eq!(kept[0].client_event_id, took.client_event_id);
        // An older press the author took says nothing about the refusal after it.
        assert_eq!(current(&[took, off], 1).len(), 2);
    }

    #[test]
    fn a_stored_answer_nobody_will_take_says_why_and_never_waits() {
        let e = none("runner.off", true);
        assert_eq!(stage_of(&e, 1).unwrap().stage, Stage::RefusedAnswer);
        assert_eq!(answer_stage(&[e], 1_000_000), (None, None));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::cards_v2::{CardEventAck, Expect};

    fn ev(mode: ExpectMode, at: Millis, durable: bool) -> MyCardEvent {
        MyCardEvent {
            client_event_id: "c1".into(),
            request: CardEventRequest {
                rev: 1,
                block_id: Some("bars".into()),
                event: "chart.select".into(),
                ..Default::default()
            },
            durable,
            sent_at: at,
            phase: CardEventPhase::Acked {
                ack: CardEventAck {
                    event_id: "EV1".into(),
                    stored: durable,
                    answer_id: None,
                    duplicate: None,
                    expect: Expect {
                        by: "b".into(),
                        mode,
                        eta_ms: None,
                        reason: None,
                    },
                },
                at,
            },
            dropped: false,
            reply: None,
        }
    }

    #[test]
    fn stages_follow_the_expect_mode() {
        let e = ev(ExpectMode::Live, 1000, false);
        assert_eq!(stage_of(&e, 1100).unwrap().stage, Stage::Quiet);
        assert_eq!(stage_of(&e, 1500).unwrap().stage, Stage::Wait);
        assert_eq!(stage_of(&e, 1000 + 9_000).unwrap().stage, Stage::Slow);
        assert_eq!(stage_of(&e, 1000 + 46_000).unwrap().stage, Stage::Silent);
        let r = ev(ExpectMode::Run, 0, false);
        assert_eq!(stage_of(&r, 10_000).unwrap().stage, Stage::Wait);
        assert_eq!(stage_of(&r, 30_000).unwrap().stage, Stage::Slow);
        let s = ev(ExpectMode::Starting, 0, false);
        assert_eq!(stage_of(&s, 44_000).unwrap().stage, Stage::Wait);
        assert_eq!(stage_of(&s, 179_000).unwrap().stage, Stage::Slow);
    }

    #[test]
    fn offline_and_answers() {
        let off = ev(ExpectMode::Offline, 0, false);
        assert_eq!(stage_of(&off, 100).unwrap().stage, Stage::OfflineLive);
        assert!(stage_of(&off, 5_000).is_none(), "the note goes after 4 s");
        let ans = ev(ExpectMode::Offline, 0, true);
        assert_eq!(stage_of(&ans, 100_000).unwrap().stage, Stage::OfflineAnswer);
        assert!(stage_of(&ev(ExpectMode::Live, 0, true), 10_000).is_none());
        let mut dropped = ev(ExpectMode::Live, 0, false);
        dropped.dropped = true;
        assert!(stage_of(&dropped, 10_000).is_none(), "不等了");
        let none = ev(ExpectMode::None, 0, false);
        assert_eq!(stage_of(&none, 100).unwrap().stage, Stage::Refused);
        assert!(stage_of(&none, 5_000).is_none());
        assert!(
            stage_of(&ev(ExpectMode::None, 0, true), 100).is_none(),
            "an answer"
        );
    }

    #[test]
    fn a_block_the_author_refused_is_read_only_with_no_retry() {
        // INTERACT §4.4: `expect.mode: none` means the block was not interactive.
        let none = ev(ExpectMode::None, 0, false);
        let events = [none.clone()];
        // The band and the block say 现在接不了，稍后再试 for 4 s (a short-run limit sends
        // `expect.mode: none` too), with no 重试 (only Silent has one).
        assert_eq!(pending(&events, 100).unwrap().stage, Stage::Refused);
        assert_eq!(
            stage_words(Stage::Refused, "Ledger"),
            "Ledger 现在接不了，稍后再试"
        );
        assert_eq!(
            pending_at(&events, "bars", 100).unwrap().stage,
            Stage::Refused
        );
        assert!(pending(&events, 5_000).is_none(), "the note goes after 4 s");
        assert!(pending_at(&events, "bars", 60_000).is_none());
        assert!(refused(&events, "bars", 1), "read-only after the note too");
        assert!(refused(&events, "bars", 1), "read-only for that rev");
        assert!(
            !refused(&events, "bars", 2),
            "a new rev may make it interactive"
        );
        assert!(!refused(&events, "other", 1));
        assert!(undone(&none, 100), "the optimistic state goes back");
        assert!(!refused(&[ev(ExpectMode::Live, 0, false)], "bars", 1));
        assert!(
            !refused(&[ev(ExpectMode::None, 0, true)], "bars", 1),
            "an answer"
        );
    }

    #[test]
    fn each_block_keeps_its_own_pending_line() {
        // The chart waits since 0; a table sort goes at 30 s.
        let chart = ev(ExpectMode::Live, 0, false);
        let mut sort = ev(ExpectMode::Live, 30_000, false);
        sort.client_event_id = "c2".into();
        sort.request.block_id = Some("t".into());
        sort.request.event = "table.sort".into();
        let events = [chart, sort];
        let now = 46_000;
        assert_eq!(pending(&events, now).unwrap().event.client_event_id, "c2");
        let at_chart = pending_at(&events, "bars", now).expect("the chart still waits");
        assert_eq!(
            at_chart.stage,
            Stage::Silent,
            "重试 and 不等了 show at the chart"
        );
        assert_eq!(pending_at(&events, "t", now).unwrap().stage, Stage::Slow);
        assert!(pending_at(&events, "other", now).is_none());
        // The next redraw follows the table's soft-to-hard step, not only the newest event.
        assert!(next_stage(&events, now).is_some());
    }

    #[test]
    fn the_diff_names_inserted_and_changed_blocks_only() {
        let card = |v: Value| -> CardV2 { serde_json::from_value(v).unwrap() };
        let old = card(serde_json::json!({
            "v": 2, "catalog": "tc.card/2.1", "rev": 1, "intent": "info", "title": "t", "fallback": "f",
            "blocks": [
                { "id": "g", "type": "group", "title": "明细", "children": ["a"] },
                { "id": "a", "type": "text", "text": "一" },
                { "id": "b", "type": "text", "text": "二" }
            ]
        }));
        let new = card(serde_json::json!({
            "v": 2, "catalog": "tc.card/2.1", "rev": 2, "intent": "info", "title": "t", "fallback": "f",
            "blocks": [
                { "id": "g", "type": "group", "title": "明细", "children": ["a", "c"] },
                { "id": "a", "type": "text", "text": "一" },
                { "id": "b", "type": "text", "text": "二改了" },
                { "id": "c", "type": "text", "text": "三" }
            ]
        }));
        assert_eq!(changed_blocks(&old, &new), ["b", "c"]);
    }

    #[test]
    fn an_offline_or_busy_live_event_goes_back_once() {
        let off = ev(ExpectMode::Offline, 0, false);
        let busy = ev(ExpectMode::None, 0, false);
        let live = ev(ExpectMode::Live, 0, false);
        let answer = ev(ExpectMode::Offline, 0, true);
        assert!(undone(&off, 100) && undone(&busy, 100));
        assert!(!undone(&live, 100), "a waiting event keeps its state");
        assert!(
            !undone(&answer, 100),
            "an offline answer is stored and stays"
        );
        let mut failed = live.clone();
        failed.phase = CardEventPhase::Failed {
            failure: CardEventFailure::Offline,
            at: 0,
        };
        assert!(undone(&failed, 100), "client.offline: never left");
        // The block stays out of the settled snapshots until its state went back.
        let mut sending = live.clone();
        sending.client_event_id = "c9".into();
        sending.request.block_id = Some("t".into());
        sending.phase = CardEventPhase::Sending;
        let mut reverted = std::collections::HashSet::new();
        let events = [off.clone(), sending, live];
        assert_eq!(in_flight(&events, &reverted, 100), ["bars", "t"]);
        reverted.insert(off.client_event_id.clone());
        assert_eq!(in_flight(&events, &reverted, 100), ["t"]);
    }

    fn failed(failure: CardEventFailure, at: Millis) -> MyCardEvent {
        let mut e = ev(ExpectMode::Live, at, false);
        e.phase = CardEventPhase::Failed { failure, at };
        e
    }

    fn resolved(result: InteractionResult, note: Option<&str>, at: Millis) -> MyCardEvent {
        let mut e = ev(ExpectMode::Live, at, false);
        e.phase = CardEventPhase::Resolved {
            result,
            note: note.map(str::to_string),
            at,
        };
        e
    }

    #[test]
    fn a_refused_or_failed_live_event_goes_back_and_says_why() {
        // The server refused it: forbidden has no 重试, anything else has one.
        let forbidden = failed(CardEventFailure::Forbidden, 0);
        let other = failed(CardEventFailure::Other, 0);
        assert_eq!(stage_of(&forbidden, 100).unwrap().stage, Stage::Forbidden);
        assert_eq!(stage_of(&other, 100).unwrap().stage, Stage::Rejected);
        assert!(undone(&forbidden, 100) && undone(&other, 100));
        assert_eq!(
            stage_words(Stage::Forbidden, "Ledger"),
            "这一步你现在不能做"
        );
        assert_eq!(stage_words(Stage::Rejected, "Ledger"), "没发出去，再试一次");
        assert!(stage_of(&other, 5_000).is_none(), "the note goes after 4 s");
        assert!(!undone(&other, 5_000), "an old event undoes nothing later");
        // The author failed at it and said nothing: the block says so, with 重试.
        let unhandled = resolved(InteractionResult::Failed, None, 0);
        assert_eq!(stage_of(&unhandled, 100).unwrap().stage, Stage::Unhandled);
        assert_eq!(
            stage_words(Stage::Unhandled, "Ledger"),
            "Ledger 没能处理这一步"
        );
        assert!(undone(&unhandled, 100));
        // The author ignored it, or the card changed: back, with no words here.
        let ignored = resolved(InteractionResult::Ignored, None, 0);
        let conflict = failed(CardEventFailure::Conflict, 0);
        assert!(stage_of(&ignored, 100).is_none() && stage_of(&conflict, 100).is_none());
        assert!(undone(&ignored, 100) && undone(&conflict, 100));
        // A step the author handled keeps its state, and so does an answer.
        assert!(!undone(&resolved(InteractionResult::Patched, None, 0), 100));
        let mut answer = failed(CardEventFailure::Other, 0);
        answer.durable = true;
        assert!(!undone(&answer, 100) && stage_of(&answer, 100).is_none());
        // The band and the block's line see them like any pending stage.
        assert_eq!(
            pending(&[other.clone()], 100).unwrap().stage,
            Stage::Rejected
        );
        assert_eq!(in_flight(&[other], &Default::default(), 100), ["bars"]);
    }

    #[test]
    fn why_a_step_failed_stays_until_the_next_event_on_the_block() {
        let failed = resolved(InteractionResult::Failed, Some("B 站的数还没同步"), 0);
        let events = [failed.clone()];
        assert_eq!(
            resolved_note(&events, "bars", 60_000).as_deref(),
            Some("B 站的数还没同步"),
            "past 5 s"
        );
        assert!(resolved_note(&events, "t", 100).is_none());
        // Why it was ignored shows for 5 s.
        let ignored = [resolved(
            InteractionResult::Ignored,
            Some("已经是这个了"),
            0,
        )];
        assert!(resolved_note(&ignored, "bars", 1_000).is_some());
        assert!(resolved_note(&ignored, "bars", 6_000).is_none());
        // A newer event on the block ends the note.
        let mut newer = ev(ExpectMode::Live, 1_000, false);
        newer.client_event_id = "c2".into();
        assert!(resolved_note(&[failed, newer], "bars", 1_100).is_none());
    }

    #[test]
    fn an_answer_nobody_picks_up_says_so_at_the_record() {
        // A run that is queued is normal: an answer waits longer than a live event.
        let answer = ev(ExpectMode::Run, 0, true);
        let soft = ms(t::SOFT_ANSWER_RUN);
        let hard = ms(t::HARD_ANSWER_RUN);
        assert!(soft > ms(t::HARD_LIVE));
        assert_eq!(answer_stage(&[answer.clone()], 1_000), (None, Some(soft)));
        assert_eq!(
            answer_stage(&[answer.clone()], soft + 1),
            (Some(AnswerStage::Slow), Some(hard))
        );
        assert_eq!(
            answer_stage(&[answer.clone()], hard + 1),
            (Some(AnswerStage::Silent), None)
        );
        // The author responded, is offline, or I dropped it: nothing more to say.
        let mut replied = answer.clone();
        replied.reply = Some("m".into());
        assert_eq!(answer_stage(&[replied], hard + 1).0, None);
        let offline = ev(ExpectMode::Offline, 0, true);
        assert_eq!(answer_stage(&[offline], hard + 1), (None, None));
        let mut done = answer.clone();
        done.phase = CardEventPhase::Resolved {
            result: InteractionResult::Patched,
            note: None,
            at: 10,
        };
        assert_eq!(answer_stage(&[done], hard + 1).0, None);
        // A live event is not an answer.
        assert_eq!(
            answer_stage(&[ev(ExpectMode::Run, 0, false)], hard + 1),
            (None, None)
        );
    }

    #[test]
    fn an_answer_the_member_acted_on_never_reads_as_ignored() {
        let at = 1_000_000;
        assert!(!picked_up(at, &[], at), "the answer itself moved the 话题");
        assert!(!picked_up(at, &[at - 600_000], at + 1_000), "an older run");
        assert!(picked_up(at, &[at + 2_000], at), "a run since");
        assert!(picked_up(at, &[at - 1_000], at), "a run made as I answered");
        assert!(picked_up(at, &[], at + 60_000), "the 话题 moved on");
    }

    #[test]
    fn a_retract_spins_then_goes_or_says_it_was_refused() {
        let mut r = ev(ExpectMode::Run, 0, true);
        r.request.event = ANSWER_RETRACT.into();
        r.request.block_id = None;
        r.phase = CardEventPhase::Sending;
        assert_eq!(retract_state(&[r.clone()]), Some(Retract::Sending));
        let mut refused = r.clone();
        refused.phase = CardEventPhase::Failed {
            failure: CardEventFailure::Conflict,
            at: 5,
        };
        assert_eq!(retract_state(&[refused.clone()]), Some(Retract::Refused));
        // Only a refusal that says the answer is closed reads as "the member has started".
        for failure in [
            CardEventFailure::Other,
            CardEventFailure::RateLimited,
            CardEventFailure::Offline,
        ] {
            let mut lost = r.clone();
            lost.phase = CardEventPhase::Failed { failure, at: 5 };
            assert_eq!(retract_state(&[lost]), Some(Retract::NotSent));
        }
        // 撤回 shows only while the server would take the answer back.
        assert!(retract_open(0, ms(t::RETRACT_WINDOW) - 1));
        assert!(!retract_open(0, ms(t::RETRACT_WINDOW)));
        assert!(has_answer(&[ev(ExpectMode::None, 0, true)]));
        assert!(!has_answer(&[r.clone()]) && !has_answer(&[ev(ExpectMode::Live, 0, false)]));
        // Accepted: the answer goes with `card.retracted`; nothing stays here.
        let acked = ev(ExpectMode::Run, 0, true);
        let mut ok = r.clone();
        ok.phase = acked.phase.clone();
        assert_eq!(retract_state(&[ok.clone()]), None);
        // A retract is never a step that waits for the author, nor an answer in flight.
        assert!(stage_of(&ok, 100).is_none() && stage_of(&refused, 100).is_none());
        assert_eq!(answer_stage(&[ok], 10_000_000), (None, None));
        assert_eq!(retract_state(&[ev(ExpectMode::Live, 0, false)]), None);
    }

    #[test]
    fn a_reply_off_screen_gets_a_jump_link() {
        let mut e = ev(ExpectMode::Live, 0, false);
        e.phase = CardEventPhase::Resolved {
            result: InteractionResult::Patched,
            note: None,
            at: 1_000,
        };
        e.reply = Some("m-reply".into());
        let events = [e.clone()];
        let never = |_: &str| false;
        assert_eq!(
            reply_off_screen(&events, "bars", 1_100, never),
            None,
            "quiet first"
        );
        assert_eq!(
            reply_off_screen(&events, "bars", 1_400, never),
            Some("m-reply")
        );
        assert_eq!(
            reply_off_screen(&events, "bars", 1_400, |m| m == "m-reply"),
            None,
            "its row was drawn: on screen"
        );
        assert_eq!(reply_off_screen(&events, "t", 1_400, never), None);
        let mut newer = ev(ExpectMode::Live, 2_000, false);
        newer.client_event_id = "c2".into();
        assert_eq!(
            reply_off_screen(&[e, newer], "bars", 2_500, never),
            None,
            "a newer event on the block ends it"
        );
    }

    #[test]
    fn removed_blocks_keep_their_place() {
        let card = |v: Value| -> CardV2 { serde_json::from_value(v).unwrap() };
        let old = card(serde_json::json!({
            "v": 2, "catalog": "tc.card/2.1", "rev": 1, "intent": "info", "title": "t", "fallback": "f",
            "blocks": [
                { "id": "a", "type": "text", "text": "一" },
                { "id": "g", "type": "group", "title": "明细", "children": ["x"] },
                { "id": "x", "type": "text", "text": "组里" },
                { "id": "b", "type": "text", "text": "二" },
                { "id": "c", "type": "text", "text": "三" }
            ]
        }));
        let new = card(serde_json::json!({
            "v": 2, "catalog": "tc.card/2.1", "rev": 2, "intent": "info", "title": "t", "fallback": "f",
            "blocks": [
                { "id": "b", "type": "text", "text": "二" }
            ]
        }));
        let r: Vec<(Option<String>, String)> = removed_blocks(&old, &new)
            .into_iter()
            .map(|(after, b)| (after, b.id().unwrap().to_string()))
            .collect();
        assert_eq!(
            r,
            [
                (None, "a".to_string()),
                (None, "g".to_string()),
                (Some("b".to_string()), "c".to_string())
            ],
            "the child x goes with its group"
        );
    }

    #[test]
    fn commits_need_no_declaration_but_live_events_do() {
        let card: CardV2 = serde_json::from_value(serde_json::json!({
            "v": 2, "catalog": "tc.card/2.1", "rev": 1, "intent": "ask", "title": "t", "fallback": "f",
            "ask": { "from": ["me"] },
            "blocks": [
                { "id": "ok", "type": "confirm", "label": "可以吗", "yes": "可以", "no": "不行", "fallbackText": "f" },
                { "id": "bars", "type": "chart", "kind": "bar", "x": ["a"], "series": [{ "name": "s", "values": [1] }],
                  "events": ["chart.select"] },
                { "id": "t", "type": "table", "columns": ["渠道"], "rows": [["微博"]] }
            ]
        }))
        .unwrap();
        let b = |id| card.block(id).unwrap();
        assert!(sends(&card, b("ok"), "choice.submit"));
        assert!(sends(&card, b("bars"), "chart.select"));
        assert!(!sends(&card, b("bars"), "chart.clear"));
        assert!(!sends(&card, b("t"), "table.sort"), "local sort only");
    }
}

// --- v12/fx-cards ---
/// The conflicts that already showed their Toast, by window and client event id: each
/// window that opens the card says it once (a scene reopened in another theme too).
#[derive(Default)]
struct ShownConflicts(std::collections::HashSet<(gpui::WindowId, String)>);

impl gpui::Global for ShownConflicts {}

/// Shows the conflict Toast once per conflicting event (§2.16, owner decision on F53):
/// 卡片刚更新了, what happened, and 看新版, which settles the event.
pub fn conflict_toast(ctx: &Ctx, window: &Window, cx: &mut App) {
    let events = ctx
        .env
        .provider
        .card_v2_events(&ctx.msg.group_id, &ctx.msg.message_id);
    // A refused 撤回 is said at the answer record, not as a changed card.
    let Some(e) = events.last().filter(|e| {
        e.request.event != ANSWER_RETRACT
            && matches!(
                e.phase,
                CardEventPhase::Failed {
                    failure: CardEventFailure::Conflict,
                    ..
                }
            )
    }) else {
        return;
    };
    let cid = e.client_event_id.clone();
    let handle = window.window_handle();
    let shown = cx.default_global::<ShownConflicts>();
    if !shown.0.insert((handle.window_id(), cid.clone())) {
        return;
    }
    let (provider, g, t, m) = (
        ctx.env.provider.clone(),
        ctx.msg.group_id.clone(),
        ctx.msg.topic_id.clone(),
        ctx.msg.message_id.clone(),
    );
    let toast_id = format!("card-conflict-{m}");
    cx.defer(move |cx| {
        let note = crate::ui::overlay::Toast::new(ui::Sym::Refresh, c::CONFLICT_TITLE)
            .description(c::CONFLICT_DETAIL)
            .action_button(crate::copy::cards_v2::STRIP_SEE_NEW, move |_, _| {
                provider.drop_card_event(&g, &t, &m, &cid)
            })
            .id(toast_id)
            .notification();
        // On the window that drew the card, not whichever window is active.
        use gpui_component::WindowExt as _;
        let _ = handle.update(cx, |_, window, cx| window.push_notification(note, cx));
    });
}

/// True while my newest event on the card came back as a conflict: the card changed under
/// me. The header keeps a muted line; the Toast says it once ([`conflict_toast`]).
pub fn conflicted(ctx: &Ctx) -> bool {
    let events = ctx
        .env
        .provider
        .card_v2_events(&ctx.msg.group_id, &ctx.msg.message_id);
    events.last().is_some_and(|e| {
        e.request.event != ANSWER_RETRACT
            && matches!(
                e.phase,
                CardEventPhase::Failed {
                    failure: CardEventFailure::Conflict,
                    ..
                }
            )
    })
}
// --- end v12/fx-cards ---
