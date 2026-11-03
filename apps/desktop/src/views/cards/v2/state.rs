//! The state of a v2 card for one viewer (DESIGN.md §7.10 "States").
//!
//! Pure functions over the card, its stored answers, my answer on its way,
//! the rev my local edit started from and the clock, so the rules are unit
//! tested without a window. docs/protocol.md "Cards v2": a card is answered
//! when a `card.answered` for its current rev exists, expired after
//! `ask.expiresAt`, and superseded when a later rev exists than the one the
//! viewer worked on.

use crate::data::{
    CardPressError, Millis,
    cards_v2::{
        CardIntent, CardV2, CardV2Answer, CardV2Block, CardV2Pending, CardV2SendPhase,
        EvidenceState, RunPlanState,
    },
};

/// Who looks at the card.
#[derive(Clone, Copy, Debug, Default)]
pub struct Viewer<'a> {
    pub id: Option<&'a str>,
    pub is_owner: bool,
    /// The topic's creator (`requester` in `ask.from`).
    pub requester: Option<&'a str>,
}

/// Plan progress of a card: done steps, all steps, and the current step.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Progress {
    pub done: usize,
    pub total: usize,
    /// 1-based number of the current (or waiting) step.
    pub step: Option<usize>,
    /// The text of the current (or waiting) step.
    pub current: Option<String>,
    /// A step waits for someone.
    pub waiting: bool,
}

impl Progress {
    /// 0..=1 for the strip's bottom bar.
    pub fn fraction(&self) -> Option<f32> {
        (self.total > 0).then(|| self.done as f32 / self.total as f32)
    }
}

/// What the card shows to this viewer.
#[derive(Clone, Debug, PartialEq)]
pub enum CardState {
    /// The viewer is asked and can answer now.
    Pending,
    /// Someone else is asked: controls drawn but disabled.
    WaitingOn,
    /// My answer is on its way.
    Sending { action_id: String },
    /// The server refused my answer.
    SendFailed {
        action_id: String,
        error: CardPressError,
    },
    /// Answered at the current rev (the first answer wins).
    Answered { answer: CardV2Answer },
    /// Work in progress (`working`); `answer` is the answer that started it.
    Executing {
        answer: Option<CardV2Answer>,
        progress: Progress,
    },
    /// A result (`done`).
    Done { answer: Option<CardV2Answer> },
    /// A result with a failed check (an `evidence` block with `fail`).
    Failed { answer: Option<CardV2Answer> },
    /// The ask passed `expiresAt` with no answer.
    Expired,
    /// The card changed after the viewer started on it.
    Superseded,
    /// Plain information: no status strip.
    Info,
}

impl CardState {
    /// Controls take input (the asked viewer, before an answer).
    pub fn live(&self) -> bool {
        matches!(self, Self::Pending | Self::SendFailed { .. })
    }

    /// The input blocks collapse into one answer record.
    pub fn collapsed(&self) -> bool {
        matches!(self, Self::Answered { .. } | Self::Sending { .. })
    }

    /// The body is muted (expired, superseded).
    pub fn muted(&self) -> bool {
        matches!(self, Self::Expired | Self::Superseded)
    }
}

/// The newest answer of any rev.
pub fn latest_answer(answers: &[CardV2Answer]) -> Option<&CardV2Answer> {
    answers.iter().max_by_key(|a| (a.rev, a.at))
}

/// The first answer to `rev` (the server keeps only the first).
pub fn answer_for(answers: &[CardV2Answer], rev: u32) -> Option<&CardV2Answer> {
    answers.iter().filter(|a| a.rev == rev).min_by_key(|a| a.at)
}

/// Plan progress from the card's `plan` blocks.
pub fn progress(card: &CardV2) -> Progress {
    let mut p = Progress::default();
    for block in card.all_blocks() {
        if let CardV2Block::Plan {
            items: Some(items), ..
        } = block
        {
            for item in items {
                p.total += 1;
                match item.state {
                    RunPlanState::Done => p.done += 1,
                    RunPlanState::Current | RunPlanState::Waiting if p.step.is_none() => {
                        p.step = Some(p.total);
                        p.current = Some(item.text.clone());
                        p.waiting = item.state == RunPlanState::Waiting;
                    }
                    _ => {}
                }
            }
        }
    }
    p
}

/// True when an `evidence` block failed.
pub fn has_failure(card: &CardV2) -> bool {
    card.all_blocks().iter().any(|b| {
        matches!(
            b,
            CardV2Block::Evidence {
                state: Some(EvidenceState::Fail),
                ..
            }
        )
    })
}

/// True when the viewer may answer the card's ask.
pub fn may_answer(card: &CardV2, viewer: &Viewer) -> bool {
    match (&card.ask, viewer.id) {
        (Some(ask), Some(me)) => ask.allows(me, viewer.is_owner, viewer.requester),
        // A card with actions and no ask: anyone may press (the server allows it).
        (None, Some(_)) => true,
        _ => false,
    }
}

/// The state of `card` for `viewer` at `now`. `local_rev` is the rev the
/// viewer's open edit (draft edit mode) started from.
pub fn derive(
    card: &CardV2,
    answers: &[CardV2Answer],
    pending: Option<&CardV2Pending>,
    local_rev: Option<u32>,
    viewer: &Viewer,
    now: Millis,
) -> CardState {
    if local_rev.is_some_and(|r| r < card.rev) {
        return CardState::Superseded;
    }
    if let Some(p) = pending {
        let stored = answers
            .iter()
            .any(|a| a.rev == p.rev && Some(a.actor_id.as_str()) == viewer.id);
        if p.rev < card.rev && !stored {
            return CardState::Superseded;
        }
        if p.rev == card.rev && !stored {
            return match p.phase {
                CardV2SendPhase::Sending => CardState::Sending {
                    action_id: p.action_id.clone(),
                },
                CardV2SendPhase::Failed(error) => CardState::SendFailed {
                    action_id: p.action_id.clone(),
                    error,
                },
            };
        }
    }
    let prior = latest_answer(answers).cloned();
    match card.intent {
        CardIntent::Ask => {
            if let Some(a) = answer_for(answers, card.rev) {
                return CardState::Answered { answer: a.clone() };
            }
            if card.is_expired(now.max(0) as u64) {
                return CardState::Expired;
            }
            if may_answer(card, viewer) {
                CardState::Pending
            } else {
                CardState::WaitingOn
            }
        }
        _ if has_failure(card) => CardState::Failed { answer: prior },
        CardIntent::Working => CardState::Executing {
            answer: prior,
            progress: progress(card),
        },
        CardIntent::Done => CardState::Done { answer: prior },
        CardIntent::Info | CardIntent::Unknown => CardState::Info,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::cards_v2::CardV2SendPhase;

    fn card(v: serde_json::Value) -> CardV2 {
        serde_json::from_value(v).expect("card parses")
    }

    fn ask(rev: u32) -> CardV2 {
        card(serde_json::json!({
            "v": 2, "rev": rev, "intent": "ask", "title": "选哪个？", "fallback": "",
            "ask": { "from": ["m-joye"], "expiresAt": 10_000 },
            "blocks": [{ "type": "decision", "id": "pick", "options": [
                { "id": "a", "label": "A" }, { "id": "b", "label": "B" }
            ]}],
            "actions": [{ "id": "ok", "label": "提交" }]
        }))
    }

    fn answer(by: &str, rev: u32, at: Millis) -> CardV2Answer {
        CardV2Answer {
            actor_id: by.into(),
            rev,
            action_id: "ok".into(),
            inputs: None,
            edited_values: None,
            at,
        }
    }

    fn pending(rev: u32, phase: CardV2SendPhase) -> CardV2Pending {
        CardV2Pending {
            rev,
            action_id: "ok".into(),
            inputs: None,
            edited_values: None,
            phase,
            at: 1,
        }
    }

    const JOYE: Viewer = Viewer {
        id: Some("m-joye"),
        is_owner: true,
        requester: None,
    };
    const WANG: Viewer = Viewer {
        id: Some("m-xiaowang"),
        is_owner: false,
        requester: None,
    };

    #[test]
    fn the_asked_viewer_sees_pending_and_others_wait() {
        let c = ask(1);
        assert_eq!(derive(&c, &[], None, None, &JOYE, 0), CardState::Pending);
        assert_eq!(derive(&c, &[], None, None, &WANG, 0), CardState::WaitingOn);
        assert_eq!(
            derive(&c, &[], None, None, &Viewer::default(), 0),
            CardState::WaitingOn
        );
    }

    #[test]
    fn owners_and_requester_entries_resolve_per_viewer() {
        let mut c = ask(1);
        c.ask.as_mut().unwrap().from = vec!["owners".into()];
        assert_eq!(derive(&c, &[], None, None, &JOYE, 0), CardState::Pending);
        assert_eq!(derive(&c, &[], None, None, &WANG, 0), CardState::WaitingOn);
        c.ask.as_mut().unwrap().from = vec!["requester".into()];
        let wang_asked = Viewer {
            requester: Some("m-xiaowang"),
            ..WANG
        };
        assert_eq!(
            derive(&c, &[], None, None, &wang_asked, 0),
            CardState::Pending
        );
    }

    #[test]
    fn the_first_answer_to_the_current_rev_wins() {
        let c = ask(2);
        let answers = [answer("m-joye", 2, 50), answer("m-xiaowang", 2, 40)];
        let CardState::Answered { answer } = derive(&c, &answers, None, None, &WANG, 0) else {
            panic!("answered");
        };
        assert_eq!(answer.actor_id, "m-xiaowang");
        // An answer to an older rev does not answer the new one.
        assert_eq!(
            derive(&c, &[answer_old()], None, None, &JOYE, 0),
            CardState::Pending
        );
    }

    fn answer_old() -> CardV2Answer {
        answer("m-joye", 1, 5)
    }

    #[test]
    fn an_ask_expires_after_its_time_unless_answered() {
        let c = ask(1);
        assert_eq!(
            derive(&c, &[], None, None, &JOYE, 20_000),
            CardState::Expired
        );
        assert!(matches!(
            derive(&c, &[answer("m-joye", 1, 9)], None, None, &JOYE, 20_000),
            CardState::Answered { .. }
        ));
    }

    #[test]
    fn my_answer_on_its_way_and_its_refusal() {
        let c = ask(1);
        assert_eq!(
            derive(
                &c,
                &[],
                Some(&pending(1, CardV2SendPhase::Sending)),
                None,
                &JOYE,
                0
            ),
            CardState::Sending {
                action_id: "ok".into()
            }
        );
        let failed = pending(1, CardV2SendPhase::Failed(CardPressError::Other));
        let s = derive(&c, &[], Some(&failed), None, &JOYE, 0);
        assert!(s.live(), "a refused answer can be sent again");
        // Once stored, the stored answer wins over the outbox row.
        assert!(matches!(
            derive(
                &c,
                &[answer("m-joye", 1, 3)],
                Some(&pending(1, CardV2SendPhase::Sending)),
                None,
                &JOYE,
                0
            ),
            CardState::Answered { .. }
        ));
    }

    #[test]
    fn a_newer_rev_supersedes_my_edit_or_my_unsent_answer() {
        let c = ask(3);
        assert_eq!(
            derive(&c, &[], None, Some(2), &JOYE, 0),
            CardState::Superseded
        );
        assert_eq!(derive(&c, &[], None, Some(3), &JOYE, 0), CardState::Pending);
        assert_eq!(
            derive(
                &c,
                &[],
                Some(&pending(2, CardV2SendPhase::Sending)),
                None,
                &JOYE,
                0
            ),
            CardState::Superseded
        );
        // My answer to rev 2 was stored, then the author edited: not superseded.
        assert_eq!(
            derive(
                &c,
                &[answer("m-joye", 2, 1)],
                Some(&pending(2, CardV2SendPhase::Sending)),
                None,
                &JOYE,
                0
            ),
            CardState::Pending
        );
    }

    #[test]
    fn working_done_and_failed_results() {
        let working = card(serde_json::json!({
            "v": 2, "rev": 2, "intent": "working", "title": "正在发布", "fallback": "",
            "blocks": [{ "type": "plan", "id": "p", "items": [
                { "text": "构建镜像", "state": "done" },
                { "text": "逐个重启各群", "state": "current" },
                { "text": "盯 15 分钟告警", "state": "pending" }
            ]}]
        }));
        let answers = [answer("m-joye", 1, 7)];
        let CardState::Executing { answer, progress } =
            derive(&working, &answers, None, None, &WANG, 0)
        else {
            panic!("executing");
        };
        assert_eq!(answer.unwrap().actor_id, "m-joye");
        assert_eq!(
            (progress.done, progress.total, progress.step),
            (1, 3, Some(2))
        );
        assert_eq!(progress.current.as_deref(), Some("逐个重启各群"));
        assert!((progress.fraction().unwrap() - 1. / 3.).abs() < 1e-6);

        let done = card(serde_json::json!({
            "v": 2, "rev": 3, "intent": "done", "title": "已发布", "fallback": "",
            "blocks": [{ "type": "evidence", "id": "e", "label": "24/24 个群在线", "state": "pass" }]
        }));
        assert!(matches!(
            derive(&done, &answers, None, None, &JOYE, 0),
            CardState::Done { answer: Some(_) }
        ));
        let failed = card(serde_json::json!({
            "v": 2, "rev": 1, "intent": "done", "title": "PR #59", "fallback": "",
            "blocks": [{ "type": "evidence", "id": "e", "label": "1 项检查没过", "state": "fail" }]
        }));
        assert!(matches!(
            derive(&failed, &[], None, None, &JOYE, 0),
            CardState::Failed { answer: None }
        ));
        let info = card(serde_json::json!({
            "v": 2, "rev": 1, "intent": "something_new", "title": "t", "fallback": "", "blocks": []
        }));
        assert_eq!(derive(&info, &[], None, None, &JOYE, 0), CardState::Info);
    }

    #[test]
    fn a_waiting_step_is_the_current_one() {
        let c = card(serde_json::json!({
            "v": 2, "rev": 1, "intent": "working", "title": "t", "fallback": "",
            "blocks": [{ "type": "plan", "id": "p", "items": [
                { "text": "a", "state": "done" },
                { "text": "b", "state": "done" },
                { "text": "定输入框样式", "state": "waiting" },
                { "text": "d", "state": "pending" }
            ]}]
        }));
        let p = progress(&c);
        assert_eq!(p.step, Some(3));
        assert!(p.waiting);
    }
}
