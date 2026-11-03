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
//! - `expect.mode: none` on a live event says {name} 现在接不了，稍后再试 for 4 s at the
//!   block and in the band; the block stays usable (INTERACT §6.2).
//! - A live event that comes back offline or busy puts the block back to its state before
//!   the gesture (INTERACT §4.4) while the band says why for 4 s.
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

use gpui::{
    AnyElement, App, Entity, IntoElement, ParentElement, StatefulInteractiveElement as _, Styled,
    Window, div, prelude::FluentBuilder as _, px,
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
    /// `expect.mode: offline` for a live event: the step was not sent (4 s).
    OfflineLive,
    /// The author is offline; my answer is stored and they will see it.
    OfflineAnswer,
    /// Refused because of limits, or `expect.mode: none`: 现在接不了，稍后再试 (4 s).
    Busy,
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

/// The pending state of one event at `now`, or `None` when it no longer waits.
pub fn stage_of(e: &MyCardEvent, now: Millis) -> Option<Pending> {
    if e.dropped {
        return None;
    }
    match &e.phase {
        CardEventPhase::Acked { ack, at } => {
            if e.durable || ack.stored {
                // An answer follows the run (CARD21 §2); only an offline author is said.
                return (ack.expect.mode == ExpectMode::Offline).then(|| Pending {
                    event: e.clone(),
                    stage: Stage::OfflineAnswer,
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
                // The author cannot take it now (a runner over its limit, INTERACT §6.2):
                // {name} 现在接不了，稍后再试 for 4 s; the block stays usable.
                ExpectMode::None => {
                    if age >= ms(t::OFFLINE_NOTE) {
                        return None;
                    }
                    (Stage::Busy, Some(at + ms(t::OFFLINE_NOTE)))
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
            _ => None,
        },
        _ => None,
    }
}

/// The newest of my events on the card that still waits, if any (the band).
pub fn pending(events: &[MyCardEvent], now: Millis) -> Option<Pending> {
    events.iter().rev().find_map(|e| stage_of(e, now))
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

/// The note of the newest resolved event of `block`, while it shows (5 s).
pub fn resolved_note(events: &[MyCardEvent], block: &str, now: Millis) -> Option<String> {
    events.iter().rev().find_map(|e| match &e.phase {
        CardEventPhase::Resolved {
            note: Some(n),
            at,
            result: InteractionResult::Ignored | InteractionResult::Failed,
        } if e.request.block_id.as_deref() == Some(block) && now - at < ms(t::RESOLVED_NOTE) => {
            Some(n.clone())
        }
        _ => None,
    })
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
    };
    fill(t, &[("name", name)])
}

/// Runs after the body each frame: flushes a waiting event whose block is free, toasts a
/// conflict once, and schedules a redraw at the next pending stage.
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
    // A conflict: toast once.
    let conflicts: Vec<String> = events
        .iter()
        .filter(|e| {
            matches!(
                e.phase,
                CardEventPhase::Failed {
                    failure: CardEventFailure::Conflict,
                    ..
                }
            )
        })
        .map(|e| e.client_event_id.clone())
        .filter(|id| !ui.read(cx).toasted.contains(id))
        .collect();
    if !conflicts.is_empty() {
        let u2 = ui.clone();
        cx.defer(move |cx| {
            u2.update(cx, |u, cx| {
                u.toasted.extend(conflicts.iter().cloned());
                cx.notify();
            });
            if !conflicts.is_empty() {
                crate::views::files::notify(
                    gpui_component::notification::Notification::new()
                        .message(c::CONFLICT_TOAST.to_string()),
                    cx,
                );
            }
        });
    }
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
/// (offline) or the author cannot take it now (busy, `expect.mode: none`).
pub fn undone(e: &MyCardEvent, now: Millis) -> bool {
    !e.durable
        && stage_of(e, now).is_some_and(|p| matches!(p.stage, Stage::OfflineLive | Stage::Busy))
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
        .text_color(k.live_ink)
        .child(div().size(px(5.)).rounded_full().bg(k.live))
        .child(
            b20::quiet_button(
                ctx.id(format!("reply-{block}")),
                &fill(c::NEW_MESSAGE, &[("name", &ctx.author)]),
                cx,
            )
            .text_color(k.live_ink)
            .on_click(move |_, window, cx| a.open_message(&reply, window, cx)),
        )
        .child(ui::icon(ui::Lucide::ArrowDown, px(12.), k.live_ink))
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
    let pend = pending_at(&events, id, now);
    let note = resolved_note(&events, id, now);
    let mark = super::band::surface_mark(ctx, node.tree, id);

    let mut col = div().flex().flex_col().min_w(px(0.));
    if let Some(m) = mark {
        let (ink, dot, text) = match m {
            super::band::Mark::You => (k.you_ink, k.you, c::BAND_SURFACE_YOU.to_string()),
            super::band::Mark::Waiting(names) => (
                k.ink_2,
                k.ink_3,
                fill(c::BAND_WAITING_ON, &[("name", &names)]),
            ),
        };
        col = col.child(
            meta(div())
                .mb(space::S6)
                .flex()
                .items_center()
                .gap(space::S6)
                .text_color(ink)
                .font_weight(theme::MEDIUM_WEIGHT)
                .child(div().size(px(6.)).rounded_full().bg(dot))
                .child(text),
        );
    }
    col = col.child(inner);
    if let Some(p) = pend.as_ref().filter(|p| p.stage != Stage::Quiet) {
        col = col.child(pending_line(ctx, p, cx));
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
    if changed && let Some(d) = diff {
        if let Some(cause) = &d.cause {
            let text = if Some(cause.actor_id.as_str()) == ctx.me.as_deref() {
                c::ATTRIBUTION_SELF.to_string()
            } else {
                fill(
                    c::ATTRIBUTION_OTHER,
                    &[
                        ("name", &ctx.name(&cause.actor_id)),
                        ("time", &crate::views::shell::clock(cause.at as Millis)),
                    ],
                )
            };
            col = col.child(
                meta(div())
                    .mt(space::S6)
                    .flex()
                    .items_center()
                    .gap(space::S6)
                    .text_color(k.live_ink)
                    .child(div().size(px(5.)).rounded_full().bg(k.live))
                    .child(text),
            );
        }
        let tinted = div()
            .mx(px(-8.))
            .px(px(8.))
            .py(px(6.))
            .my(px(-6.))
            .rounded(theme::radius::MD)
            .child(col);
        if cx.reduce_motion() {
            return tinted.into_any_element();
        }
        let tint = mix(k.live, k.bg, t::CHANGE_FILL);
        let bg = k.bg;
        let rev = ctx.card.rev;
        return crate::ui::motion::once(
            ctx.id(format!("chg-{id}-{rev}")),
            t::CHANGE_FLASH,
            tinted,
            move |el, p| {
                let fade_in = (p * (t::CHANGE_FLASH.as_secs_f32() / t::CHANGE_IN.as_secs_f32()))
                    .clamp(0., 1.);
                let tint_left = 1. - p;
                el.opacity(fade_in).bg(mix(tint, bg, tint_left))
            },
        );
    }
    col.into_any_element()
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
        .text_color(k.live_ink)
        .child(div().size(px(5.)).rounded_full().bg(k.live))
        .child(c::DRAFT_CHANGED)
        .child(
            b20::quiet_button(ctx.id(format!("use-new-{id}")), c::DRAFT_USE_NEW, cx)
                .text_color(k.ink)
                .on_click(move |_, _, cx| {
                    form.update(cx, |f, cx| f.use_new(&card, &block, cx));
                    ui.update(cx, |u, cx| {
                        u.drafts_kept.remove(&block);
                        cx.notify();
                    });
                }),
        )
        .into_any_element()
}

/// The line at a block while its event waits: a spinner and the stage, and at the end
/// 重试 and 不等了.
fn pending_line(ctx: &Ctx, p: &Pending, cx: &mut App) -> AnyElement {
    let k = theme::colors(cx);
    let words = stage_words(p.stage, &ctx.author);
    let mut row = meta(div())
        .mt(space::S6)
        .flex()
        .items_center()
        .gap(space::S6)
        .text_color(match p.stage {
            Stage::Silent | Stage::OfflineLive | Stage::Busy => k.ink_2,
            _ => k.live_ink,
        })
        .when(matches!(p.stage, Stage::Wait | Stage::Slow), |el| {
            el.child(crate::ui::motion::spinner().color(k.live))
        })
        .child(div().min_w(px(0.)).one_line().child(words));
    if p.stage == Stage::Silent {
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
            .child(
                b20::quiet_button(ctx.id(format!("stop-{cid}")), c::PENDING_STOP, cx)
                    .on_click(move |_, _, _| provider.drop_card_event(&g, &tp, &m, &cid)),
            );
    }
    row.into_any_element()
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
        assert_eq!(
            stage_of(&none, 100).unwrap().stage,
            Stage::Busy,
            "现在接不了"
        );
        assert!(stage_of(&none, 5_000).is_none(), "the note goes after 4 s");
        assert!(
            stage_of(&ev(ExpectMode::None, 0, true), 100).is_none(),
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
                { "id": "t", "type": "table", "columns": ["渠道"], "rows": [["小红书"]] }
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
