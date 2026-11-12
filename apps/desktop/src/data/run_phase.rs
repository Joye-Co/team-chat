//! The one state a person sees of a run, and who may pause, take over or hand it back
//! (docs/plans/usable/design-takeover.zh.md 3.1, 3.2, 3.9; docs/protocol.md, "Run phase and
//! take-over"). The server computes `BotRun.phase`; a run from an older server, or with a
//! phase this build does not know, is worked out here from `status` and `pausedBy`.

use super::{BotRunSummary, MemberId, Millis, RunStatus, TeamRole};

/// The one state of a run (`BotRun.phase`). Pause, waiting for a person and finished are three
/// different phases and never look alike.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum RunPhase {
    Working,
    /// The CLI waits in a question card, or the watcher found it stuck.
    WaitingPerson,
    /// The run ended and waits for a reply in the 话题 (its report said waiting or blocked).
    NeedsReply,
    WaitingApproval,
    WaitingComputer,
    /// A person paused it (暂停).
    Paused,
    /// A person took it over (接管) and may run commands in its work folder.
    TakenOver,
    Done,
    Failed,
    Stopped,
}

impl RunPhase {
    /// A wire value (`RUN_PHASES`); `None` for one this build does not know.
    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "working" => Self::Working,
            "waiting_person" => Self::WaitingPerson,
            "needs_reply" => Self::NeedsReply,
            "waiting_approval" => Self::WaitingApproval,
            "waiting_computer" => Self::WaitingComputer,
            "paused" => Self::Paused,
            "taken_over" => Self::TakenOver,
            "done" => Self::Done,
            "failed" => Self::Failed,
            "stopped" => Self::Stopped,
            _ => return None,
        })
    }

    /// Paused or taken over: a person holds it.
    pub fn held(self) -> bool {
        matches!(self, Self::Paused | Self::TakenOver)
    }

    /// The run is over (its CLI exited); `NeedsReply` too.
    pub fn ended(self) -> bool {
        matches!(
            self,
            Self::NeedsReply | Self::Done | Self::Failed | Self::Stopped
        )
    }
}

/// What a waiting phase waits for (`BotRun.waitingFor`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum WaitingFor {
    Question,
    Stuck,
    Report,
    Queue,
    Offline,
    Booting,
}

impl WaitingFor {
    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "question" => Self::Question,
            "stuck" => Self::Stuck,
            "report" => Self::Report,
            "queue" => Self::Queue,
            "offline" => Self::Offline,
            "booting" => Self::Booting,
            _ => return None,
        })
    }
}

/// How the run's computer can be taken over (`BotRun.takeover`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum TakeoverKind {
    /// Pause only: an older daemon, an older server (no field) or a value this build does not
    /// know.
    #[default]
    None,
    /// Commands run as the daemon's user (a personal computer).
    Commands,
    /// Commands run as another user than the run (a VM on a Linux host).
    Isolated,
}

impl TakeoverKind {
    pub fn parse(s: Option<&str>) -> Self {
        match s {
            Some("commands") => Self::Commands,
            Some("isolated") => Self::Isolated,
            _ => Self::None,
        }
    }
}

/// What a person did while holding a run (`BotRun.lastHandback`).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Handback {
    pub by: MemberId,
    pub held_ms: u64,
    pub commands: u64,
    /// Changed paths in the work folder, at most 20.
    pub files: Vec<String>,
    pub head_from: Option<String>,
    pub head_to: Option<String>,
}

/// The phase fields of a run, as the server sent them (all optional), and who it is for.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RunHold {
    /// `BotRun.phase` as sent; read it through [`phase_of`].
    pub phase: Option<String>,
    pub waiting_for: Option<String>,
    /// Who holds the run (paused or took it over).
    pub paused_by: Option<MemberId>,
    /// "pause" | "takeover", with `paused_by` only.
    pub hold_kind: Option<String>,
    pub held_at: Option<Millis>,
    /// `BotRun.takeover` as sent; read it through [`TakeoverKind::parse`].
    pub takeover: Option<String>,
    pub last_handback: Option<Handback>,
    /// The person the run works for (`requestedFor`), when the server says.
    pub requested_for: Option<MemberId>,
    /// The person whose own computer runs it; `None`: a team computer, or not known yet.
    pub machine_owner: Option<MemberId>,
    /// 排队: its 1-based place among the member's queued runs.
    pub queue_position: Option<u32>,
}

impl BotRunSummary {
    /// The person the run works for: `requestedFor`, else `requestedBy`.
    pub fn requester(&self) -> Option<&str> {
        self.hold
            .requested_for
            .as_deref()
            .or(self.requested_by.as_deref())
    }
}

/// The run's phase and what it waits for: the server's when it sent one this build knows,
/// else worked out from `status` and `pausedBy` (design-takeover 3.1, old servers).
pub fn phase_of(run: &BotRunSummary) -> (RunPhase, Option<WaitingFor>) {
    let waiting = run.hold.waiting_for.as_deref().and_then(WaitingFor::parse);
    if let Some(p) = run.hold.phase.as_deref().and_then(RunPhase::parse) {
        return (p, waiting);
    }
    match run.status {
        RunStatus::Cancelled => return (RunPhase::Stopped, None),
        RunStatus::Done => return (RunPhase::Done, None),
        RunStatus::Failed => return (RunPhase::Failed, None),
        RunStatus::Blocked if run.ended_at.is_some() => return (RunPhase::Failed, None),
        _ => {}
    }
    if run.hold.paused_by.is_some() {
        let phase = if run.hold.hold_kind.as_deref() == Some("takeover") {
            RunPhase::TakenOver
        } else {
            // Older data has no kind: a pause.
            RunPhase::Paused
        };
        return (phase, waiting);
    }
    match run.status {
        RunStatus::Waiting => (RunPhase::WaitingPerson, Some(WaitingFor::Question)),
        RunStatus::Blocked => (RunPhase::WaitingPerson, Some(WaitingFor::Stuck)),
        _ => (RunPhase::Working, waiting),
    }
}

/// The person looking at a run.
#[derive(Clone, Copy, Debug)]
pub struct Viewer<'a> {
    pub id: &'a str,
    pub role: TeamRole,
    /// A person who is not deactivated (a bot never holds a run).
    pub active_person: bool,
}

/// What the viewer may do with a run now (design-takeover 3.2). The server stays the judge.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RunRights {
    pub pause: bool,
    /// 继续 a paused run.
    pub resume: bool,
    pub take_over: bool,
    /// 交还 a run I hold.
    pub hand_back: bool,
    /// 替 {who} 交还: an admin, for someone holding a run on a team computer.
    pub hand_back_for: bool,
    /// The command box: I took the run over.
    pub run_commands: bool,
}

/// Who may hold the run at all: the requester; on a team computer also an admin; on a personal
/// computer also its owner and no admin (design-roles 3.6).
fn may_hold(run: &BotRunSummary, v: Viewer) -> bool {
    if run.requester() == Some(v.id) {
        return true;
    }
    match run.hold.machine_owner.as_deref() {
        Some(owner) => owner == v.id,
        None => v.role != TeamRole::Member,
    }
}

/// Who may take it over: on a personal computer only its owner; on a team computer only the
/// requester, and only when it runs commands as another user; never without the feature.
pub fn may_take_over(run: &BotRunSummary, v: Viewer) -> bool {
    if !v.active_person {
        return false;
    }
    match (
        TakeoverKind::parse(run.hold.takeover.as_deref()),
        run.hold.machine_owner.as_deref(),
    ) {
        (TakeoverKind::None, _) => false,
        (_, Some(owner)) => owner == v.id,
        (TakeoverKind::Isolated, None) => run.requester() == Some(v.id),
        (TakeoverKind::Commands, None) => false,
    }
}

pub fn rights(run: &BotRunSummary, v: Viewer) -> RunRights {
    if !v.active_person {
        return RunRights::default();
    }
    let (phase, _) = phase_of(run);
    let hold = may_hold(run, v);
    let mine = run.hold.paused_by.as_deref() == Some(v.id);
    match phase {
        RunPhase::Working | RunPhase::WaitingPerson | RunPhase::WaitingApproval => RunRights {
            pause: hold,
            take_over: may_take_over(run, v),
            ..RunRights::default()
        },
        RunPhase::Paused => RunRights {
            resume: mine || hold,
            take_over: may_take_over(run, v),
            ..RunRights::default()
        },
        RunPhase::TakenOver => RunRights {
            hand_back: mine,
            hand_back_for: !mine && v.role != TeamRole::Member && run.hold.machine_owner.is_none(),
            run_commands: mine,
            ..RunRights::default()
        },
        _ => RunRights::default(),
    }
}

/// A button at the foot of 实时查看.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RunAction {
    Pause,
    Continue,
    TakeOver,
    HandBack,
    HandBackFor,
    Stop,
    /// 去回复: back to the 话题's composer.
    GoReply,
}

/// The buttons of a run, the primary first (design-takeover 3.9). A button the viewer may not
/// use is left out, never drawn disabled.
pub fn actions(run: &BotRunSummary, v: Viewer) -> Vec<RunAction> {
    let (phase, _) = phase_of(run);
    let r = rights(run, v);
    let mut out = Vec::new();
    match phase {
        RunPhase::Working | RunPhase::WaitingPerson | RunPhase::WaitingApproval => {
            if r.pause {
                out.push(RunAction::Pause);
            }
            if r.take_over {
                out.push(RunAction::TakeOver);
            }
            out.push(RunAction::Stop);
        }
        RunPhase::NeedsReply => out.push(RunAction::GoReply),
        RunPhase::WaitingComputer => out.push(RunAction::Stop),
        RunPhase::Paused => {
            if r.resume {
                out.push(RunAction::Continue);
            }
            if r.take_over {
                out.push(RunAction::TakeOver);
            }
            out.push(RunAction::Stop);
        }
        RunPhase::TakenOver => {
            if r.hand_back {
                out.push(RunAction::HandBack);
            } else if r.hand_back_for {
                out.push(RunAction::HandBackFor);
            }
            out.push(RunAction::Stop);
        }
        RunPhase::Done | RunPhase::Failed | RunPhase::Stopped => {}
    }
    out
}

/// One command of the person holding a run (`hand.command`, `hand.output`, `hand.result`).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct HandCommand {
    pub id: String,
    pub command: String,
    pub by: MemberId,
    /// Its output so far (the newest 16 KiB at most).
    pub output: String,
    pub result: Option<HandResult>,
    pub at: Millis,
}

/// How a person's command ended.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct HandResult {
    pub ok: bool,
    pub exit_code: Option<i64>,
    pub ms: u64,
    pub timed_out: bool,
    /// "not_held" | "busy" | "unsupported", when the computer refused it.
    pub error: Option<String>,
}

/// The output kept per command in the terminal (design 3.3: the last 16 KB).
pub const HAND_OUTPUT_KEEP: usize = 16 * 1024;

/// Appends `text` to a command's output, keeping its last [`HAND_OUTPUT_KEEP`] bytes on a
/// character boundary.
pub fn push_output(output: &mut String, text: &str) {
    output.push_str(text);
    if output.len() > HAND_OUTPUT_KEEP {
        let mut cut = output.len() - HAND_OUTPUT_KEEP;
        while !output.is_char_boundary(cut) {
            cut += 1;
        }
        output.drain(..cut);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(status: RunStatus) -> BotRunSummary {
        BotRunSummary {
            id: "r1".into(),
            bot_id: "b1".into(),
            group_id: "g1".into(),
            topic_id: "t1".into(),
            machine_id: Some("m1".into()),
            machine_name: None,
            status,
            requested_by: Some("alice".into()),
            created_at: 0,
            started_at: None,
            ended_at: None,
            updated_at: 0,
            summary: None,
            plan: Vec::new(),
            hold: RunHold::default(),
        }
    }

    fn viewer(id: &str, role: TeamRole) -> Viewer<'_> {
        Viewer {
            id,
            role,
            active_person: true,
        }
    }

    #[test]
    fn the_server_phase_wins_and_an_unknown_one_falls_back() {
        let mut r = run(RunStatus::Done);
        r.hold.phase = Some("needs_reply".into());
        r.hold.waiting_for = Some("report".into());
        assert_eq!(
            phase_of(&r),
            (RunPhase::NeedsReply, Some(WaitingFor::Report))
        );
        r.hold.phase = Some("some_new_phase".into());
        assert_eq!(phase_of(&r).0, RunPhase::Done);
        r.hold.phase = None;
        assert_eq!(phase_of(&r).0, RunPhase::Done);
    }

    #[test]
    fn the_fallback_keeps_pause_waiting_and_done_apart() {
        let mut r = run(RunStatus::Running);
        assert_eq!(phase_of(&r).0, RunPhase::Working);
        r.hold.paused_by = Some("alice".into());
        assert_eq!(phase_of(&r).0, RunPhase::Paused);
        r.hold.hold_kind = Some("takeover".into());
        assert_eq!(phase_of(&r).0, RunPhase::TakenOver);
        let w = run(RunStatus::Waiting);
        assert_eq!(
            phase_of(&w),
            (RunPhase::WaitingPerson, Some(WaitingFor::Question))
        );
        let b = run(RunStatus::Blocked);
        assert_eq!(
            phase_of(&b),
            (RunPhase::WaitingPerson, Some(WaitingFor::Stuck))
        );
        let mut ended = run(RunStatus::Blocked);
        ended.ended_at = Some(5);
        assert_eq!(phase_of(&ended).0, RunPhase::Failed);
        assert_eq!(phase_of(&run(RunStatus::Cancelled)).0, RunPhase::Stopped);
        assert_eq!(phase_of(&run(RunStatus::Failed)).0, RunPhase::Failed);
        // A paused run that ended is over, whatever pausedBy says.
        let mut p = run(RunStatus::Done);
        p.hold.paused_by = Some("alice".into());
        assert_eq!(phase_of(&p).0, RunPhase::Done);
    }

    #[test]
    fn take_over_follows_the_computer() {
        let mut r = run(RunStatus::Running);
        let alice = viewer("alice", TeamRole::Member);
        let admin = viewer("ada", TeamRole::Admin);
        // No feature: nobody takes over, the requester and an admin pause.
        assert!(!may_take_over(&r, alice));
        assert_eq!(actions(&r, alice), [RunAction::Pause, RunAction::Stop]);
        assert_eq!(actions(&r, admin), [RunAction::Pause, RunAction::Stop]);
        // A team Mac: still pause only.
        r.hold.takeover = Some("commands".into());
        assert!(!may_take_over(&r, alice));
        // A team VM: the requester, not an admin.
        r.hold.takeover = Some("isolated".into());
        assert_eq!(
            actions(&r, alice),
            [RunAction::Pause, RunAction::TakeOver, RunAction::Stop]
        );
        assert!(!may_take_over(&r, admin));
        // B's run on A's computer: A takes over, B pauses, an admin does neither.
        r.hold.takeover = Some("commands".into());
        r.hold.machine_owner = Some("owner".into());
        let owner = viewer("owner", TeamRole::Member);
        assert_eq!(
            actions(&r, owner),
            [RunAction::Pause, RunAction::TakeOver, RunAction::Stop]
        );
        assert_eq!(actions(&r, alice), [RunAction::Pause, RunAction::Stop]);
        assert_eq!(actions(&r, admin), [RunAction::Stop]);
        // requestedFor wins over requestedBy.
        r.hold.requested_for = Some("bob".into());
        assert_eq!(actions(&r, alice), [RunAction::Stop]);
        assert_eq!(
            actions(&r, viewer("bob", TeamRole::Member)),
            [RunAction::Pause, RunAction::Stop]
        );
    }

    #[test]
    fn the_buttons_follow_the_phase() {
        let alice = viewer("alice", TeamRole::Member);
        let admin = viewer("ada", TeamRole::Owner);
        let mut r = run(RunStatus::Running);
        r.hold.takeover = Some("isolated".into());
        r.hold.paused_by = Some("alice".into());
        r.hold.hold_kind = Some("pause".into());
        assert_eq!(
            actions(&r, alice),
            [RunAction::Continue, RunAction::TakeOver, RunAction::Stop]
        );
        assert_eq!(actions(&r, admin), [RunAction::Continue, RunAction::Stop]);
        r.hold.hold_kind = Some("takeover".into());
        assert_eq!(actions(&r, alice), [RunAction::HandBack, RunAction::Stop]);
        assert!(rights(&r, alice).run_commands);
        assert_eq!(
            actions(&r, admin),
            [RunAction::HandBackFor, RunAction::Stop]
        );
        assert!(!rights(&r, admin).run_commands);
        // On a personal computer an admin does not hand back for someone.
        r.hold.machine_owner = Some("alice".into());
        assert_eq!(actions(&r, admin), [RunAction::Stop]);
        let mut w = run(RunStatus::Running);
        w.hold.phase = Some("waiting_computer".into());
        assert_eq!(actions(&w, alice), [RunAction::Stop]);
        w.hold.phase = Some("needs_reply".into());
        assert_eq!(actions(&w, alice), [RunAction::GoReply]);
        for p in ["done", "failed", "stopped"] {
            w.hold.phase = Some(p.into());
            assert!(actions(&w, alice).is_empty(), "{p}");
        }
        // A bot or a deactivated person gets no hold buttons.
        let gone = Viewer {
            active_person: false,
            ..alice
        };
        assert_eq!(actions(&run(RunStatus::Running), gone), [RunAction::Stop]);
    }

    #[test]
    fn output_keeps_its_tail_on_a_char_boundary() {
        let mut out = String::new();
        push_output(&mut out, &"中".repeat(HAND_OUTPUT_KEEP));
        assert!(out.len() <= HAND_OUTPUT_KEEP);
        push_output(&mut out, "end");
        assert!(out.ends_with("end"));
        assert!(out.len() <= HAND_OUTPUT_KEEP);
    }
}
