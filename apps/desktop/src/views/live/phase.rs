//! The words of a run's phase (design-takeover 3.9): one word per state, with the time it has
//! lasted (`· 已 N 分钟`), the badge that goes with it, and the two lines the 话题 shows for a
//! take-over and a hand-back. 暂停, 等你 and 完成 never share words or a badge.

use crate::{
    copy::{fill, live as c},
    data::{
        BotRunSummary, Millis,
        run_phase::{Handback, RunPhase, WaitingFor, phase_of},
    },
    ui::native::BadgeState,
};

/// What a run's state reads as.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PhaseWords {
    pub phase: RunPhase,
    /// The full words: `等 小王 回复`, `你在操作 Mac mini`.
    pub text: String,
    /// The badge's short words: `等你`, `已暂停`.
    pub short: String,
    pub badge: BadgeState,
}

/// The names a phase needs.
pub struct Names<'a> {
    /// The AI member.
    pub member: &'a str,
    /// The computer: its name, else `{member} 的电脑`.
    pub computer: &'a str,
    pub me: Option<&'a str>,
}

/// The words of `run`'s phase; `name` gives a member's name.
pub fn phase_words(run: &BotRunSummary, n: &Names, name: impl Fn(&str) -> String) -> PhaseWords {
    let (phase, waiting) = phase_of(run);
    let is_me = |id: Option<&str>| id.is_some() && id == n.me;
    let requester = run.requester();
    let holder = run.hold.paused_by.as_deref();
    let who = |id: Option<&str>| id.map(&name).unwrap_or_default();
    let (text, short, badge) = match phase {
        RunPhase::Working => (
            c::LIVE_PHASE_WORKING.to_string(),
            c::LIVE_PHASE_WORKING.to_string(),
            BadgeState::Working,
        ),
        RunPhase::WaitingPerson | RunPhase::NeedsReply
            if matches!(waiting, Some(WaitingFor::Stuck)) =>
        {
            (
                c::LIVE_PHASE_STUCK.to_string(),
                c::LIVE_SHORT_STUCK.to_string(),
                BadgeState::Abnormal,
            )
        }
        RunPhase::WaitingPerson | RunPhase::NeedsReply | RunPhase::WaitingApproval => {
            let approval = phase == RunPhase::WaitingApproval;
            if is_me(requester) || requester.is_none() {
                (
                    if approval {
                        c::LIVE_PHASE_WAITING_APPROVAL_YOU
                    } else {
                        c::LIVE_PHASE_WAITING_REPLY_YOU
                    }
                    .to_string(),
                    c::LIVE_SHORT_WAITING_YOU.to_string(),
                    BadgeState::NeedsYou,
                )
            } else {
                let w = who(requester);
                (
                    fill(
                        if approval {
                            c::LIVE_PHASE_WAITING_APPROVAL
                        } else {
                            c::LIVE_PHASE_WAITING_REPLY
                        },
                        &[("who", &w)],
                    ),
                    fill(c::LIVE_SHORT_WAITING, &[("who", &w)]),
                    BadgeState::Quiet,
                )
            }
        }
        RunPhase::WaitingComputer => (
            match (waiting, run.hold.queue_position) {
                (Some(WaitingFor::Offline), _) => c::LIVE_PHASE_OFFLINE.to_string(),
                (Some(WaitingFor::Booting), _) => c::LIVE_PHASE_BOOTING.to_string(),
                (_, Some(p)) => fill(c::LIVE_PHASE_QUEUE, &[("n", &p.to_string())]),
                _ => c::LIVE_PHASE_QUEUE_PLAIN.to_string(),
            },
            c::LIVE_SHORT_COMPUTER.to_string(),
            BadgeState::Queued,
        ),
        RunPhase::Paused => (
            if is_me(holder) {
                fill(c::LIVE_PHASE_PAUSED_YOU, &[("member", n.member)])
            } else {
                fill(
                    c::LIVE_PHASE_PAUSED,
                    &[("who", &who(holder)), ("member", n.member)],
                )
            },
            c::LIVE_SHORT_PAUSED.to_string(),
            BadgeState::Paused,
        ),
        RunPhase::TakenOver => {
            if is_me(holder) {
                (
                    fill(c::LIVE_PHASE_HELD_YOU, &[("computer", n.computer)]),
                    c::LIVE_SHORT_HELD_YOU.to_string(),
                    BadgeState::Paused,
                )
            } else {
                let w = who(holder);
                (
                    fill(
                        c::LIVE_PHASE_HELD_OTHER,
                        &[("who", &w), ("computer", n.computer)],
                    ),
                    fill(c::LIVE_SHORT_HELD_OTHER, &[("who", &w)]),
                    BadgeState::Paused,
                )
            }
        }
        RunPhase::Done => (
            c::LIVE_PHASE_DONE.to_string(),
            c::LIVE_PHASE_DONE.to_string(),
            BadgeState::Done,
        ),
        RunPhase::Failed => (
            c::LIVE_PHASE_FAILED.to_string(),
            c::LIVE_PHASE_FAILED.to_string(),
            BadgeState::Failed,
        ),
        RunPhase::Stopped => (
            c::LIVE_PHASE_STOPPED.to_string(),
            c::LIVE_PHASE_STOPPED.to_string(),
            BadgeState::Deactivated,
        ),
    };
    PhaseWords {
        phase,
        text,
        short,
        badge,
    }
}

/// When the current phase started, as well as the client knows: the hold's start, the run's
/// start while it works, its end once over.
pub fn phase_since(run: &BotRunSummary, phase: RunPhase) -> Option<Millis> {
    match phase {
        RunPhase::Paused | RunPhase::TakenOver => run.hold.held_at,
        RunPhase::WaitingComputer => Some(run.created_at),
        RunPhase::Working => run.started_at.or(Some(run.created_at)),
        RunPhase::NeedsReply | RunPhase::Done | RunPhase::Failed | RunPhase::Stopped => {
            run.ended_at
        }
        RunPhase::WaitingPerson | RunPhase::WaitingApproval => Some(run.updated_at),
    }
}

/// `· 已 12 分钟`, `· 已 2 小时 5 分钟`: how long the phase has lasted.
pub fn lasted(since: Millis, now: Millis) -> String {
    let minutes = ((now - since).max(0) / 60_000) as u64;
    if minutes < 60 {
        fill(c::LIVE_PHASE_FOR, &[("n", &minutes.to_string())])
    } else {
        fill(
            c::LIVE_PHASE_FOR_HOURS,
            &[
                ("h", &(minutes / 60).to_string()),
                ("m", &(minutes % 60).to_string()),
            ],
        )
    }
}

/// The holder is reminded after 2 hours; the requester and admins after 24 (design 3.3).
pub const REMIND_HOLDER_MS: Millis = 2 * 3_600_000;
pub const REMIND_ALL_MS: Millis = 24 * 3_600_000;

/// The reminder a held run shows to me now, when it is due.
pub fn reminder(
    run: &BotRunSummary,
    member: &str,
    me: Option<&str>,
    now: Millis,
    name: impl Fn(&str) -> String,
) -> Option<String> {
    let (phase, _) = phase_of(run);
    if !phase.held() {
        return None;
    }
    let held = now - run.hold.held_at?;
    let holder = run.hold.paused_by.as_deref()?;
    if Some(holder) == me {
        return (held >= REMIND_HOLDER_MS)
            .then(|| fill(crate::copy::notify::HOLD_REMIND, &[("member", member)]));
    }
    (held >= REMIND_ALL_MS).then(|| {
        fill(
            crate::copy::notify::HOLD_REMIND_LONG,
            &[
                ("who", &name(holder)),
                ("member", member),
                ("h", &(held / 3_600_000).to_string()),
            ],
        )
    })
}

/// The 话题's line while someone holds the run: `小王 接管了 Forge`.
pub fn took_over_line(by: &str, member: &str, me: Option<&str>, who: &str) -> String {
    if Some(by) == me {
        fill(c::LIVE_TOPIC_TOOK_OVER_YOU, &[("member", member)])
    } else {
        fill(c::LIVE_TOPIC_TOOK_OVER, &[("who", who), ("member", member)])
    }
}

/// The 话题's line after a hand-back:
/// `小王 交还了 Forge · 12 分钟 · 跑了 3 条命令 · 改了 .env.example`.
pub fn handed_back_line(h: &Handback, member: &str, me: Option<&str>, who: &str) -> String {
    let mut parts = vec![if Some(h.by.as_str()) == me {
        fill(c::LIVE_TOPIC_HANDED_BACK_YOU, &[("member", member)])
    } else {
        fill(
            c::LIVE_TOPIC_HANDED_BACK,
            &[("who", who), ("member", member)],
        )
    }];
    parts.push(fill(
        c::LIVE_TOPIC_HELD_MINUTES,
        &[("n", &(h.held_ms / 60_000).to_string())],
    ));
    if h.commands > 0 {
        parts.push(fill(
            c::LIVE_TOPIC_COMMANDS,
            &[("k", &h.commands.to_string())],
        ));
    }
    if let Some(first) = h.files.first() {
        let files = if h.files.len() > 1 {
            fill(
                c::LIVE_TOPIC_FILES_MORE,
                &[("files", first), ("n", &h.files.len().to_string())],
            )
        } else {
            first.clone()
        };
        parts.push(fill(c::LIVE_TOPIC_FILES, &[("files", &files)]));
    }
    parts.join(" · ")
}

/// The badge of a run's card in the 话题 (design-takeover 3.9, one word per run): the
/// phase's own short words, and while it works the plan's count (`在做 2/5`), or 等你 when its
/// plan waits on a step.
pub fn card_badge(
    words: &PhaseWords,
    progress: Option<(usize, usize)>,
    plan_waiting: bool,
) -> (BadgeState, String) {
    if words.phase != RunPhase::Working {
        return (words.badge, words.short.clone());
    }
    if plan_waiting {
        return (BadgeState::NeedsYou, c::LIVE_SHORT_WAITING_YOU.to_string());
    }
    match progress {
        Some((step, total)) => (
            BadgeState::Working,
            format!("{} {step}/{total}", c::LIVE_PHASE_WORKING),
        ),
        None => (BadgeState::Working, c::LIVE_PHASE_WORKING.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::{RunStatus, run_phase::RunHold};

    fn run() -> BotRunSummary {
        BotRunSummary {
            id: "r1".into(),
            bot_id: "b1".into(),
            group_id: "g1".into(),
            topic_id: "t1".into(),
            machine_id: Some("m1".into()),
            machine_name: None,
            status: RunStatus::Running,
            requested_by: Some("me".into()),
            created_at: 0,
            started_at: Some(0),
            ended_at: None,
            updated_at: 0,
            summary: None,
            plan: Vec::new(),
            hold: RunHold::default(),
        }
    }

    fn words(r: &BotRunSummary) -> PhaseWords {
        let n = Names {
            member: "Forge",
            computer: "Mac mini",
            me: Some("me"),
        };
        phase_words(r, &n, |id| {
            if id == "w" {
                "小王".into()
            } else {
                id.into()
            }
        })
    }

    #[test]
    fn the_card_in_the_topic_reads_the_same_phase() {
        let mut r = run();
        assert_eq!(
            card_badge(&words(&r), Some((2, 5)), false),
            (BadgeState::Working, "在做 2/5".into())
        );
        // A run waiting in a question card says so without a waiting plan step.
        r.status = RunStatus::Waiting;
        assert_eq!(
            card_badge(&words(&r), Some((2, 5)), false),
            (BadgeState::NeedsYou, "等你".into())
        );
        r.status = RunStatus::Running;
        r.hold.phase = Some("waiting_approval".into());
        r.requested_by = Some("w".into());
        assert_eq!(card_badge(&words(&r), None, false).1, "等小王");
        // A pause and a take-over do not read alike.
        r.hold.phase = None;
        r.hold.paused_by = Some("me".into());
        r.hold.hold_kind = Some("pause".into());
        assert_eq!(
            card_badge(&words(&r), Some((2, 5)), false),
            (BadgeState::Paused, "已暂停".into())
        );
        r.hold.hold_kind = Some("takeover".into());
        assert_eq!(card_badge(&words(&r), None, false).1, "你在操作");
    }

    #[test]
    fn pause_waiting_and_done_never_look_alike() {
        let mut r = run();
        r.hold.phase = Some("paused".into());
        r.hold.paused_by = Some("w".into());
        let paused = words(&r);
        assert_eq!(paused.text, "小王 暂停了 Forge");
        r.hold.paused_by = Some("me".into());
        assert_eq!(words(&r).text, "你暂停了 Forge");
        r.hold.phase = Some("waiting_person".into());
        r.hold.waiting_for = Some("question".into());
        let waiting = words(&r);
        assert_eq!(waiting.text, "等你回复");
        r.hold.phase = Some("done".into());
        let done = words(&r);
        assert_eq!(done.text, "完成");
        for (a, b) in [(&paused, &waiting), (&paused, &done), (&waiting, &done)] {
            assert_ne!(a.badge, b.badge);
            assert_ne!(a.short, b.short);
        }
    }

    #[test]
    fn every_phase_has_its_words() {
        let mut r = run();
        let cases = [
            ("working", None, "在做"),
            ("waiting_person", Some("stuck"), "卡住了，需要你看一下"),
            ("needs_reply", Some("report"), "等你回复"),
            ("waiting_approval", None, "等你批准"),
            ("waiting_computer", Some("offline"), "在等电脑：电脑不在线"),
            ("waiting_computer", Some("booting"), "在等电脑：正在准备"),
            ("waiting_computer", Some("queue"), "在等电脑：排队"),
            ("failed", None, "没做完"),
            ("stopped", None, "已停止"),
        ];
        for (phase, waiting, text) in cases {
            r.hold.phase = Some(phase.into());
            r.hold.waiting_for = waiting.map(Into::into);
            assert_eq!(words(&r).text, text, "{phase}");
        }
        r.hold.phase = Some("waiting_computer".into());
        r.hold.waiting_for = Some("queue".into());
        r.hold.queue_position = Some(2);
        assert_eq!(words(&r).text, "在等电脑：排队第 2 个");
        r.requested_by = Some("w".into());
        r.hold.phase = Some("waiting_person".into());
        r.hold.waiting_for = Some("question".into());
        assert_eq!(words(&r).text, "等 小王 回复");
        assert_eq!(words(&r).short, "等小王");
        r.hold.phase = Some("taken_over".into());
        r.hold.paused_by = Some("me".into());
        assert_eq!(words(&r).text, "你在操作 Mac mini");
        r.hold.paused_by = Some("w".into());
        assert_eq!(words(&r).text, "小王 在操作 Mac mini");
    }

    #[test]
    fn the_time_and_the_reminders() {
        assert_eq!(lasted(0, 12 * 60_000), "· 已 12 分钟");
        assert_eq!(lasted(0, 125 * 60_000), "· 已 2 小时 5 分钟");
        let mut r = run();
        r.hold.phase = Some("taken_over".into());
        r.hold.paused_by = Some("me".into());
        r.hold.held_at = Some(0);
        let name = |id: &str| id.to_string();
        assert_eq!(reminder(&r, "Forge", Some("me"), 3_600_000, name), None);
        assert_eq!(
            reminder(&r, "Forge", Some("me"), REMIND_HOLDER_MS, name).as_deref(),
            Some("Forge 还停着，等你交还")
        );
        assert_eq!(
            reminder(&r, "Forge", Some("ada"), REMIND_HOLDER_MS, name),
            None
        );
        assert_eq!(
            reminder(&r, "Forge", Some("ada"), REMIND_ALL_MS + 1, name).as_deref(),
            Some("me 接管 Forge 已经 24 小时")
        );
    }

    #[test]
    fn the_topic_lines() {
        assert_eq!(
            took_over_line("w", "Forge", Some("me"), "小王"),
            "小王 接管了 Forge"
        );
        assert_eq!(
            took_over_line("me", "Forge", Some("me"), "我"),
            "你接管了 Forge"
        );
        let h = Handback {
            by: "w".into(),
            held_ms: 12 * 60_000,
            commands: 3,
            files: vec![".env.example".into()],
            ..Default::default()
        };
        assert_eq!(
            handed_back_line(&h, "Forge", Some("me"), "小王"),
            "小王 交还了 Forge · 12 分钟 · 跑了 3 条命令 · 改了 .env.example"
        );
        let quiet = Handback {
            by: "me".into(),
            held_ms: 60_000,
            ..Default::default()
        };
        assert_eq!(
            handed_back_line(&quiet, "Forge", Some("me"), "我"),
            "你交还了 Forge · 1 分钟"
        );
    }
}
