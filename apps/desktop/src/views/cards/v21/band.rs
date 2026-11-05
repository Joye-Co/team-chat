//! The status band of a 2.1 card (CARD21 §2): one band per card per viewer, derived from
//! the card, the server's card state, the answers and my events. Members cannot set it.
//!
//! | Band | Word | When |
//! | --- | --- | --- |
//! | amber | 等你 | open ask, the viewer is asked; the detail says what kind of question |
//! | grey | 等{名字} | open ask, someone else is asked (up to 2 names, else 等 n 位成员) |
//! | blue | 处理中 | working, or my live event waits for the author |
//! | green | 完成 | done |
//! | red | 失败 | my answer did not get there, or a check failed |
//! | dimmed | 已作废 / 已过期 | superseded, expired |
//!
//! The 2.0 strip draws it (`v2::strip`); this file gives it the 2.1 words, the right-hand
//! deadline and the pending override.

use crate::{
    copy::{cards21 as c, fill},
    data::{
        Millis,
        cards_v2::{CardIntent, CardV2, CardV2Answer, CardV2Block, PollVoters},
    },
    views::cards::v2::{
        Ctx,
        state::{CardState, Viewer},
    },
};

use super::{interact, is_21};

/// `今天 12:00`, `明天 09:00`, `11月3日 18:00`.
pub fn day_time(at: Millis) -> String {
    use chrono::{Local, TimeZone as _};
    // A missing time is 未知, never 1970 (§2.15): the shell's zero guard.
    let Some(then) =
        crate::views::shell::known(at).and_then(|at| Local.timestamp_millis_opt(at).single())
    else {
        return crate::views::shell::clock(at);
    };
    let today = crate::selftest::clock::local_now().date_naive();
    let time = then.format("%H:%M").to_string();
    if then.date_naive() == today {
        fill(c::BAND_TODAY, &[("time", &time)])
    } else if today.succ_opt() == Some(then.date_naive()) {
        fill(c::BAND_TOMORROW, &[("time", &time)])
    } else {
        fill(
            c::BAND_DAY,
            &[
                ("m", &then.format("%-m").to_string()),
                ("d", &then.format("%-d").to_string()),
                ("time", &time),
            ],
        )
    }
}

/// The card's poll, if it has one.
fn poll(card: &CardV2) -> Option<&CardV2Block> {
    card.blocks
        .iter()
        .find(|b| matches!(b, CardV2Block::Poll { .. }))
}

/// True when `me` may vote in `poll`.
fn voter(b: &CardV2Block, me: Option<&str>) -> bool {
    match (b, me) {
        (CardV2Block::Poll { voters, .. }, Some(me)) => match voters {
            PollVoters::Members(ids) => ids.iter().any(|x| x == me),
            PollVoters::Keyword(_) => true,
        },
        _ => false,
    }
}

/// The 2.1 rules on top of the 2.0 state: a poll stays open for every voter until
/// `closesAt` (amber for a viewer who has not voted, grey after the vote).
pub fn adjust(
    card: &CardV2,
    state: CardState,
    answers: &[CardV2Answer],
    viewer: &Viewer,
    now: Millis,
) -> CardState {
    if !is_21(card) {
        return state;
    }
    if let Some(p @ CardV2Block::Poll { closes_at, .. }) = poll(card)
        && (*closes_at as Millis) > now
        && matches!(
            state,
            CardState::Answered { .. } | CardState::Pending | CardState::WaitingOn
        )
    {
        let voted = answers
            .iter()
            .any(|a| a.rev == card.rev && Some(a.actor_id.as_str()) == viewer.id);
        return if voter(p, viewer.id) && !voted {
            CardState::Pending
        } else {
            CardState::WaitingOn
        };
    }
    state
}

/// True when the viewer can answer a non-blocking ask on a card whose band follows its
/// intent (a question inside a done or info report, INTERACT §3.6): the decision surface
/// is live and marked 问你, the band stays the intent's.
pub fn surface_live(
    ctx_card: &CardV2,
    state: &CardState,
    answers: &[CardV2Answer],
    viewer: &Viewer,
    open: Option<bool>,
) -> bool {
    if !is_21(ctx_card) || state.live() || ctx_card.intent == CardIntent::Ask {
        return false;
    }
    let Some(ask) = &ctx_card.ask else {
        return false;
    };
    let Some(me) = viewer.id else { return false };
    let open = open.unwrap_or_else(|| !answers.iter().any(|a| a.rev == ctx_card.rev));
    open && ask.allows(me, viewer.is_owner, viewer.requester)
        && !matches!(
            state,
            CardState::Sending { .. } | CardState::Expired | CardState::Superseded
        )
}

/// True when block `id` is on the live decision surface of a non-blocking ask.
pub fn asks_here(ctx: &Ctx, id: &str) -> bool {
    ctx.surface_live && !ctx.state.live() && ctx.card.surface_ids().contains(id)
}

/// What kind of question the card asks, for the amber band.
fn ask_kind(card: &CardV2) -> Option<String> {
    let surface = card.surface_ids();
    let on = |b: &&CardV2Block| b.id().is_some_and(|id| surface.contains(id));
    let mut kinds = card.blocks.iter().filter(on);
    if let Some(CardV2Block::Questionnaire { children, .. }) = card
        .blocks
        .iter()
        .find(|b| matches!(b, CardV2Block::Questionnaire { .. }))
    {
        let n = children
            .iter()
            .filter(|id| card.block(id).is_some_and(CardV2Block::takes_input))
            .count();
        return Some(fill(c::BAND_QUESTIONS, &[("n", &n.to_string())]));
    }
    let first = kinds.next()?;
    Some(match first {
        CardV2Block::Compare { options, .. } => {
            fill(c::BAND_COMPARE, &[("n", &options.len().to_string())])
        }
        CardV2Block::Decision {
            multiple: Some(true),
            min_select,
            max_select,
            options,
            ..
        } => fill(
            c::BAND_MULTIPLE,
            &[
                ("min", &min_select.unwrap_or(1).to_string()),
                (
                    "max",
                    &max_select.unwrap_or(options.len() as u32).to_string(),
                ),
            ],
        ),
        CardV2Block::Decision {
            allow_other: Some(_),
            ..
        } => c::BAND_SINGLE_OTHER.into(),
        CardV2Block::Decision { .. } => c::BAND_SINGLE.into(),
        CardV2Block::Rank { .. } => c::BAND_RANK.into(),
        CardV2Block::Rating { .. } => c::BAND_RATING.into(),
        CardV2Block::Scale { .. } => c::BAND_SCALE.into(),
        CardV2Block::Confirm { .. } => c::BAND_CONFIRM.into(),
        CardV2Block::ImageChoice { .. } => c::BAND_IMAGE.into(),
        CardV2Block::Slots { .. } => c::BAND_SLOTS.into(),
        CardV2Block::Matrix { .. } => c::BAND_MATRIX.into(),
        CardV2Block::Table { .. } => c::BAND_TABLE.into(),
        CardV2Block::Input { .. } => c::BAND_FILL.into(),
        _ => return None,
    })
}

/// The card's short summary when the amber band says it (Interact.dc.html 01: 等你 · Ledger
/// 想在周会前确认数据); a questionnaire's band counts its questions instead.
fn band_summary(card: &CardV2) -> Option<String> {
    if card
        .blocks
        .iter()
        .any(|b| matches!(b, CardV2Block::Questionnaire { .. }))
    {
        return None;
    }
    card.summary.clone().filter(|s| s.chars().count() <= 24)
}

/// True when the band says the card's summary, so the body does not say it again
/// (CARD21 §2).
pub fn summary_in_band(ctx: &Ctx) -> bool {
    is_21(ctx.card)
        && matches!(ctx.state, CardState::Pending)
        && poll(ctx.card).is_none()
        && pending(ctx).is_none()
        && band_summary(ctx.card).is_some()
}

pub(crate) fn names(ctx: &Ctx) -> (usize, String) {
    let Some(ask) = &ctx.card.ask else {
        return (0, String::new());
    };
    let resolved: Vec<String> = ctx
        .msg
        .card_state
        .as_ref()
        .and_then(|s| s.ask.as_ref())
        .map(|a| a.from.clone())
        .unwrap_or_else(|| {
            ask.from
                .iter()
                .flat_map(|f| match f.as_str() {
                    crate::data::cards_v2::ASK_FROM_REQUESTER => ctx
                        .env
                        .provider
                        .topic(&ctx.msg.group_id, &ctx.msg.topic_id)
                        .map(|t| vec![t.created_by])
                        .unwrap_or_default(),
                    crate::data::cards_v2::ASK_FROM_OWNERS => ctx
                        .env
                        .provider
                        .members()
                        .into_iter()
                        .filter(|m| m.level == crate::data::MemberLevel::Owner && !m.deactivated)
                        .map(|m| m.id)
                        .collect(),
                    id => vec![id.to_string()],
                })
                .collect()
        });
    let names: Vec<String> = resolved.iter().map(|id| ctx.name(id)).collect();
    (
        names.len(),
        names.into_iter().take(2).collect::<Vec<_>>().join("、"),
    )
}

/// The 2.1 label and detail of the band, from the 2.0 words for the same state.
pub fn words(ctx: &Ctx, base: (String, String)) -> (String, String) {
    if !is_21(ctx.card) {
        return base;
    }
    let (_, detail) = base.clone();
    match ctx.state {
        CardState::Pending => {
            if let Some(p) = poll(ctx.card) {
                let _ = p;
                let voted = ctx.answers.iter().filter(|a| a.rev == ctx.card.rev).count();
                return (
                    c::BAND_YOU.into(),
                    fill(
                        c::BAND_VOTED,
                        &[
                            ("n", &voted.to_string()),
                            ("total", &poll_total(ctx).to_string()),
                        ],
                    ),
                );
            }
            let kind = band_summary(ctx.card)
                .or_else(|| ask_kind(ctx.card))
                .unwrap_or(detail);
            (c::BAND_YOU.into(), kind)
        }
        CardState::WaitingOn => {
            if poll(ctx.card).is_some() {
                let voted = ctx.answers.iter().filter(|a| a.rev == ctx.card.rev).count();
                let total = poll_total(ctx);
                return (
                    fill(
                        c::BAND_WAITING_MANY,
                        &[("n", &total.saturating_sub(voted).to_string())],
                    ),
                    fill(
                        c::BAND_VOTED,
                        &[("n", &voted.to_string()), ("total", &total.to_string())],
                    ),
                );
            }
            let (n, list) = names(ctx);
            let label = if n > 2 {
                fill(c::BAND_WAITING_MANY, &[("n", &n.to_string())])
            } else {
                fill(
                    c::BAND_WAITING_ON,
                    &[("name", &crate::views::spaced_name(&list))],
                )
            };
            let questions = ctx.card.blocks.iter().find_map(|b| match b {
                CardV2Block::Questionnaire { children, .. } => Some(children.len()),
                _ => None,
            });
            let detail = match questions {
                Some(q) => fill(
                    c::BAND_ASKED,
                    &[("name", &ctx.author), ("n", &q.to_string())],
                ),
                None => fill(c::BAND_ASKED_ONE, &[("name", &ctx.author)]),
            };
            (label, detail)
        }
        CardState::Superseded => (c::BAND_VOID.into(), detail),
        CardState::Expired => (c::BAND_EXPIRED.into(), detail),
        CardState::SendFailed {
            error: crate::data::CardPressError::Other,
            ..
        } => (c::BAND_FAILED.into(), c::SEND_FAILED_NET.into()),
        CardState::SendFailed { .. } | CardState::Failed { .. } => (c::BAND_FAILED.into(), detail),
        CardState::Done { .. } => (c::BAND_DONE.into(), detail),
        // One count rule (§2.12): the badge says 在做 done/total, like the bar
        // under it; the detail says what the current step does.
        CardState::Executing { progress, .. } if !progress.waiting => {
            let label = if progress.total > 0 {
                fill(
                    crate::copy::cards_v2::STRIP_WORKING_COUNT,
                    &[
                        ("n", &progress.done.to_string()),
                        ("total", &progress.total.to_string()),
                    ],
                )
            } else {
                c::BAND_WORKING.into()
            };
            (label, working_detail(progress))
        }
        _ => base,
    }
}

/// The detail of a working band: what the current step does (the count is
/// in the badge).
pub(crate) fn working_detail(p: &crate::views::cards::v2::state::Progress) -> String {
    p.current.clone().unwrap_or_default()
}

fn poll_total(ctx: &Ctx) -> usize {
    match poll(ctx.card) {
        Some(CardV2Block::Poll {
            voters: PollVoters::Members(ids),
            ..
        }) => ids.len(),
        Some(_) => ctx
            .env
            .provider
            .members()
            .into_iter()
            .filter(|m| !m.is_bot() && !m.deactivated)
            .count(),
        None => 0,
    }
}

/// The right-hand text of the band: the ask's deadline, the poll's close, or the time of
/// my answer that did not get there.
pub fn right(ctx: &Ctx) -> Option<String> {
    if !is_21(ctx.card) {
        return None;
    }
    match ctx.state {
        CardState::Pending | CardState::WaitingOn => {
            if let Some(CardV2Block::Poll { closes_at, .. }) = poll(ctx.card) {
                return Some(fill(
                    c::BAND_CLOSES,
                    &[("time", &day_time(*closes_at as Millis))],
                ));
            }
            let at = ctx.card.ask.as_ref()?.expires_at?;
            Some(fill(c::BAND_DEADLINE, &[("time", &day_time(at as Millis))]))
        }
        CardState::SendFailed { .. } => ctx
            .env
            .provider
            .card_v2_pending(&ctx.msg.group_id, &ctx.msg.message_id)
            .map(|p| crate::views::shell::clock(p.at)),
        _ => None,
    }
}

/// The band of a rev that a member's event caused (Interact.dc.html 02 step 3, INTERACT
/// §4.4 attribution): `Ledger 更新了这张卡` with `刚刚 · 按你点的「B 站」` for the viewer whose
/// event caused it, `10:14 · 按 Joye 的选择` for everyone else. It stands in for the band of
/// a done or info card until a rev without a cause; an open question or work keeps its own.
pub fn updated(ctx: &Ctx) -> Option<(String, String)> {
    if !is_21(ctx.card) || ctx.card.rev < 2 {
        return None;
    }
    if !matches!(
        ctx.state,
        CardState::Info | CardState::Done { answer: None }
    ) {
        return None;
    }
    let cause = ctx.msg.cause.as_ref()?;
    let at = cause.at as Millis;
    let time = if ctx.env.now().saturating_sub(at) < 60_000 {
        crate::copy::common::JUST_NOW.to_string()
    } else {
        crate::views::shell::clock(at)
    };
    let why = if Some(cause.actor_id.as_str()) == ctx.me.as_deref() {
        match picked(ctx, cause) {
            Some(what) => fill(c::ATTRIBUTION_PICK, &[("what", &what)]),
            None => c::ATTRIBUTION_SELF.to_string(),
        }
    } else {
        fill(
            c::ATTRIBUTION_OTHER,
            &[("name", &ctx.name(&cause.actor_id))],
        )
    };
    Some((
        fill(c::BAND_UPDATED, &[("name", &ctx.author)]),
        fill(c::BAND_UPDATED_DETAIL, &[("time", &time), ("why", &why)]),
    ))
}

/// What my event that caused the rev picked, in the card's words: the bar's x, the
/// option's label, the selected row's first cell. `None` when my outbox no longer has it.
fn picked(ctx: &Ctx, cause: &crate::data::cards_v2::Cause) -> Option<String> {
    let block = cause.block_id.as_deref()?;
    let events = ctx
        .env
        .provider
        .card_v2_events(&ctx.msg.group_id, &ctx.msg.message_id);
    let e = events.iter().rev().find(|e| {
        e.event_id() == Some(cause.event_id.as_str())
            || (e.request.block_id.as_deref() == Some(block) && e.request.event == cause.event)
    })?;
    let v = e.request.value.as_ref()?;
    match (cause.event.as_str(), ctx.card.block(block)) {
        ("chart.select", _) => v.get("x")?.as_str().map(str::to_string),
        ("choice.change", Some(CardV2Block::Decision { options, .. })) => {
            let id = v.get("value")?.as_str()?;
            options.iter().find(|o| o.id == id).map(|o| o.label.clone())
        }
        ("table.rowSelect", Some(CardV2Block::Table { rows, row_keys, .. })) => {
            let key = v.get("values")?.get(0)?.as_str()?;
            let keys = super::data::row_keys(rows, row_keys.as_deref());
            let i = keys.iter().position(|k| k == key)?;
            rows.get(i)?.first().cloned()
        }
        _ => None,
    }
}

/// The look of a band that follows my event, not the card.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PendTone {
    /// Blue, the spinner: the author has it and may still answer.
    Live,
    /// Grey, a still mark: no answer came, or the author will see it later.
    Quiet,
    /// Red 失败: the step was not sent, or the author cannot take it now.
    Bad,
    /// 已停用, the muted ban: the author said the block takes no action (`expect.mode:
    /// none`); nothing failed and nothing is left to retry (D1-70).
    Off,
}

/// The band while my newest event on the card waits (INTERACT §4.4), for the emitter only.
#[derive(Clone, Debug)]
pub struct PendBand {
    pub event_id: String,
    pub stage: interact::Stage,
    pub label: String,
    pub detail: String,
    pub tone: PendTone,
}

/// The band of a pending stage: the stage itself is the band's word (等 Ledger 回应, Ledger
/// 还在处理, Ledger 没有回应), never a 处理中 that stays after the author stopped; a step that
/// was not sent is the red 失败 of CARD21 §2 with the reason.
pub fn pend_band(stage: interact::Stage, author: &str) -> Option<(String, String, PendTone)> {
    use interact::Stage as S;
    let words = interact::stage_words(stage, author);
    Some(match stage {
        S::Quiet => return None,
        // The author has it: waiting on someone else is the muted clock (§2.12).
        S::Wait | S::Slow => (words, String::new(), PendTone::Quiet),
        S::Silent | S::OfflineAnswer => (words, String::new(), PendTone::Quiet),
        S::Refused => (c::BAND_EXPIRED.to_string(), words, PendTone::Off),
        S::OfflineLive | S::Busy => (c::BAND_FAILED.to_string(), words, PendTone::Bad),
    })
}

/// My live event that still waits for the author: the band says its stage, for the
/// emitter only.
pub fn pending(ctx: &Ctx) -> Option<PendBand> {
    if !is_21(ctx.card) {
        return None;
    }
    let events = ctx
        .env
        .provider
        .card_v2_events(&ctx.msg.group_id, &ctx.msg.message_id);
    let p = interact::pending(&events, ctx.env.now())?;
    let (label, detail, tone) = pend_band(p.stage, &ctx.author)?;
    Some(PendBand {
        event_id: p.event.client_event_id,
        stage: p.stage,
        label,
        detail,
        tone,
    })
}

/// A page event from an embed in this card still waits for the author (the page host's
/// band of that version): the card band says the host's words, for the emitter only (the
/// host keeps only my own events).
pub fn page_pending(ctx: &Ctx, cx: &gpui::App) -> Option<(String, PendTone)> {
    use crate::views::artifacts::host::{self, BandView};
    if !is_21(ctx.card) {
        return None;
    }
    ctx.card.blocks.iter().find_map(|b| match b {
        CardV2Block::Embed {
            artifact_id,
            version,
            ..
        } => match host::band(artifact_id, *version, cx)? {
            BandView::Working(t) => Some((t, PendTone::Live)),
            BandView::Waiting(t) => Some((t, PendTone::Quiet)),
            BandView::NoAnswer(t) => Some((t, PendTone::Quiet)),
            _ => None,
        },
        _ => None,
    })
}

/// The quiet mark above the decision surface of a non-blocking ask inside a report:
/// 问你 for the asked viewer, 等{名字} for everyone else.
pub enum Mark {
    You,
    Waiting(String),
}

pub fn surface_mark(ctx: &Ctx, tree: &super::Tree, id: &str) -> Option<Mark> {
    if !is_21(ctx.card) || ctx.card.intent == CardIntent::Ask {
        return None;
    }
    let ask = ctx.card.ask.as_ref()?;
    if ask.blocking == Some(true) {
        return None;
    }
    let open = ctx
        .msg
        .card_state
        .as_ref()
        .map(|s| s.open_ask().is_some())
        .unwrap_or_else(|| !ctx.answers.iter().any(|a| a.rev == ctx.card.rev));
    if !open {
        return None;
    }
    let surface = ctx.card.surface_ids();
    let ids: Vec<&str> = ctx
        .card
        .blocks
        .iter()
        .filter_map(|b| b.id())
        .filter(|b| surface.contains(b))
        .collect();
    if !tree.roots(&ids).contains(&id) {
        return None;
    }
    if ctx.surface_live && !ctx.state.live() {
        return Some(Mark::You);
    }
    let (_, list) = names(ctx);
    (!list.is_empty() && !ctx.surface_live).then_some(Mark::Waiting(list))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::views::cards::v2::state::Progress;

    #[test]
    fn a_working_band_says_what_the_step_does_and_counts_in_the_badge() {
        let p = Progress {
            done: 0,
            total: 3,
            step: Some(1),
            current: Some("Ledger 在补夹夹增长的数".into()),
            waiting: false,
        };
        // The count is in the badge (在做 done/total), never a second count here.
        assert_eq!(working_detail(&p), "Ledger 在补夹夹增长的数");
        let bare = Progress {
            step: None,
            current: None,
            ..p
        };
        assert_eq!(working_detail(&bare), "");
    }
}
