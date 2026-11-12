//! The 话题 page (NATIVE.md §9.2): the timeline and composer under the
//! shell's toolbar row (title = the 话题, subtitle = 群 · status words, the
//! 单独问 / 产出 / PR group and the inspector toggle).
//!
//! The timeline draws in its v2 style (`timeline/v2.rs`); under it, with
//! the transcript's 24 pt side insets, the decision bar (`pinned.rs`) sits
//! above the native composer (`composer/native.rs`).

pub mod output_pane;
mod pinned;
pub mod relay_strip;
pub mod scenes;

use gpui::{
    App, AppContext as _, Context, Entity, IntoElement, ParentElement, Render, Styled,
    Subscription, Window, div, px,
};

use crate::{
    app::{RightPane, Services},
    data::{BotRunSummary, ChatProvider, PlanItem, PlanState, RunStatus, Topic, TopicStatus},
    theme::{self, StateTone, space},
    ui::native::BadgeState,
    views::{
        composer::ComposerView,
        shell::{PageView, TitleBarSpec, TitleButton, TitleButtonKind, TitleStyle, panes::watch},
        timeline::{TimelineView, v2 as timeline_v2},
    },
};

pub struct TopicPage {
    services: Services,
    timeline: Entity<TimelineView>,
    composer: Entity<ComposerView>,
    /// Usable: the 按顺序做 bar at the top of the column.
    relay_strip: Entity<relay_strip::RelayStrip>,
    _subscriptions: Vec<Subscription>,
}

impl TopicPage {
    pub fn new(
        services: Services,
        timeline: Entity<TimelineView>,
        composer: Entity<ComposerView>,
        cx: &mut Context<Self>,
    ) -> Self {
        // The shell draws the title bar; the v1 header band would repeat it.
        timeline.update(cx, |t, cx| {
            t.set_show_header(false, cx);
            t.set_v2(true, cx);
        });
        composer.update(cx, |c, cx| c.set_v2(true, cx));
        let mut subscriptions = watch(&services, cx);
        // R2-09: the pinned banner gives way while its card is on screen.
        subscriptions.push(cx.observe(&timeline, |_, _, cx| cx.notify()));
        let relay_strip = cx.new(|cx| relay_strip::RelayStrip::new(services.clone(), cx));
        Self {
            _subscriptions: subscriptions,
            services,
            timeline,
            composer,
            relay_strip,
        }
    }
}

/// The one state of a 话题 (§2.12, A5): the title bar, the inspector's
/// 状态, the sidebar row and the pinned banner all draw it from
/// [`topic_state`], so one 话题 never shows two state words.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StateLine {
    pub text: String,
    pub tone: StateTone,
    /// The StatusBadge state of `text`.
    pub badge: BadgeState,
    /// The short words for the sidebar's right column (`等你`, `3/5`,
    /// `失败`): the same state, fewer words. Empty when the row shows none.
    pub short: String,
    /// The quieter tail: the step a member is on, or who holds its computer.
    pub detail: Option<String>,
    /// The member's run is paused (someone took over its computer).
    pub paused: bool,
    /// The member's name that leads `text`: the title bar cuts it with `…`
    /// before it cuts `在做 3/5`.
    pub name: Option<String>,
}

impl StateLine {
    fn plain(text: String, badge: BadgeState) -> Self {
        Self {
            text,
            tone: tone_of(badge),
            badge,
            short: String::new(),
            detail: None,
            paused: false,
            name: None,
        }
    }

    fn short(mut self, short: impl Into<String>) -> Self {
        self.short = short.into();
        self
    }
}

/// The subtitle tone of a badge state (older callers read the tone).
fn tone_of(badge: BadgeState) -> StateTone {
    match badge {
        BadgeState::NeedsYou => StateTone::You,
        BadgeState::Working => StateTone::Live,
        BadgeState::Failed | BadgeState::Abnormal => StateTone::Problem,
        BadgeState::Done => StateTone::Done,
        _ => StateTone::Quiet,
    }
}

/// Where a run's plan stands: `(done, total, step)`, the one count rule of
/// §2.12 (badge, meta and bar all say done/total), and `step` the text of
/// the step it is on. `None` when the run has no plan.
pub fn plan_progress(plan: &[PlanItem]) -> Option<(usize, usize, Option<String>)> {
    if plan.is_empty() {
        return None;
    }
    let current = plan
        .iter()
        .position(|p| matches!(p.state, PlanState::Current | PlanState::Waiting));
    let n = plan.iter().filter(|p| p.state == PlanState::Done).count();
    Some((n, plan.len(), current.map(|i| plan[i].text.clone())))
}

/// How long a run has worked (P-01): from its start to its end, or to the
/// provider's now. Every place that says a run's time (首页, 群, 成员, the
/// live pane, the working card) reads this one clock, so one run never
/// shows two times. `None` before the run starts.
pub fn run_elapsed(p: &dyn ChatProvider, run: &BotRunSummary) -> Option<i64> {
    let start = run.started_at?;
    let end = run.ended_at.unwrap_or_else(|| p.clock_now());
    Some((end - start).max(0))
}

/// [`run_elapsed`] in words: `已用 35 分钟`, `已用 2 小时 5 分钟`.
pub fn run_elapsed_text(p: &dyn ChatProvider, run: &BotRunSummary) -> Option<String> {
    use crate::views::timeline::plan;
    run_elapsed(p, run).map(|ms| plan::span_text(plan::USED, ms))
}

/// How long the run's current step has run, in words (`已 12 分钟`): the
/// step's own time, from its start on the provider's clock. `None` when no
/// step is current or its start is not known. A step row shows this, never
/// the run's [`run_elapsed`] (which reads `已用 …`).
pub fn step_elapsed_text(p: &dyn ChatProvider, run: &BotRunSummary) -> Option<String> {
    let i = run
        .plan
        .iter()
        .position(|s| s.state == PlanState::Current)?;
    let start = *p.run_step_times(&run.id).get(i)?;
    crate::views::shell::known(start)?;
    let minutes = ((p.clock_now() - start).max(0) / 60_000).to_string();
    Some(crate::copy::fill(
        crate::copy::topic_rail::STEP_SO_FAR,
        &[("n", &minutes)],
    ))
}

/// The run a member works on in this 话题 now: active and not waiting for
/// a person (an answer or a review is said elsewhere), the latest first.
pub fn working_run(runs: &[BotRunSummary]) -> Option<&BotRunSummary> {
    working_runs(runs).pop()
}

/// The run a 话题's state names (T2-01): the owner's run at work, so the
/// title bar, the inspector's 负责 and 计划 and the composer hint name one
/// member; else the newest member at work.
pub fn lead_run<'a>(runs: &'a [BotRunSummary], owner: Option<&str>) -> Option<&'a BotRunSummary> {
    let at_work = working_runs(runs);
    at_work
        .iter()
        .find(|r| owner == Some(r.bot_id.as_str()))
        .or(at_work.last())
        .copied()
}

/// The runs members work on in this 话题 now, one per member (its latest),
/// in the order they started: one plan card each on the timeline.
pub fn working_runs(runs: &[BotRunSummary]) -> Vec<&BotRunSummary> {
    let start = |r: &BotRunSummary| r.started_at.unwrap_or(r.created_at);
    let mut out: Vec<&BotRunSummary> = Vec::new();
    for r in runs
        .iter()
        .filter(|r| r.status.is_active() && r.status != RunStatus::Waiting)
    {
        match out.iter_mut().find(|x| x.bot_id == r.bot_id) {
            Some(x) if start(r) > start(x) => *x = r,
            Some(_) => {}
            None => out.push(r),
        }
    }
    out.sort_by_key(|r| start(r));
    out
}

/// The state line of a member's run: `Forge 在做 3/5 · 跑一万条消息的压测`,
/// or, while someone holds its computer, `Forge 已暂停 · 你在操作它的电脑`.
pub fn run_state(
    run: &BotRunSummary,
    name: &str,
    paused_by: Option<&str>,
    me: Option<&str>,
    who: impl Fn(&str) -> String,
) -> StateLine {
    use crate::copy::{fill, topic_rail as c};
    use crate::data::run_phase::{RunPhase, phase_of};
    // Usable (design-takeover 3.9): the run's phase. A pause and a take-over both stop the
    // member; a server phase other than 在做 has its own words.
    let paused_by = paused_by.or(run.hold.paused_by.as_deref());
    let mut held_run;
    let run = if paused_by.is_some() && run.hold.paused_by.is_none() {
        held_run = run.clone();
        held_run.hold.paused_by = paused_by.map(Into::into);
        &held_run
    } else {
        run
    };
    let (phase, _) = phase_of(run);
    let server_phase = run
        .hold
        .phase
        .as_deref()
        .and_then(RunPhase::parse)
        .is_some();
    if let Some(by) = paused_by.filter(|_| phase.held()) {
        // An older server's hold (no kind) keeps the words of before.
        let detail = if phase == RunPhase::Paused && run.hold.hold_kind.as_deref() == Some("pause")
        {
            if Some(by) == me {
                fill(
                    crate::copy::live::LIVE_PHASE_PAUSED_YOU,
                    &[("member", name)],
                )
            } else {
                fill(
                    crate::copy::live::LIVE_PHASE_PAUSED,
                    &[("who", &who(by)), ("member", name)],
                )
            }
        } else if Some(by) == me {
            c::STATE_PAUSED_YOU.to_string()
        } else {
            fill(c::STATE_PAUSED_OTHER, &[("who", &who(by))])
        };
        return StateLine {
            text: fill(c::STATE_PAUSED, &[("name", name)]),
            tone: StateTone::Quiet,
            badge: BadgeState::Paused,
            short: c::STATE_PAUSED_SHORT.to_string(),
            detail: Some(detail),
            paused: true,
            name: Some(name.to_string()),
        };
    }
    // A server that says the run waits (for a person, an approval, a computer) or ended is
    // believed; an older one keeps the words of before.
    if server_phase && phase != RunPhase::Working {
        let words = crate::views::live::phase::phase_words(
            run,
            &crate::views::live::phase::Names {
                member: name,
                computer: run.machine_name.as_deref().unwrap_or(name),
                me,
            },
            &who,
        );
        return StateLine {
            text: format!("{name} {}", words.text),
            tone: tone_of(words.badge),
            badge: words.badge,
            short: words.short,
            detail: None,
            paused: false,
            name: Some(name.to_string()),
        };
    }
    match plan_progress(&run.plan) {
        Some((n, total, step)) => StateLine {
            text: fill(
                c::STATE_WORKING,
                &[
                    ("name", name),
                    ("n", &n.to_string()),
                    ("total", &total.to_string()),
                ],
            ),
            tone: StateTone::Live,
            badge: BadgeState::Working,
            short: fill(
                c::OWNER_WORKING,
                &[("n", &n.to_string()), ("total", &total.to_string())],
            ),
            detail: step,
            paused: false,
            name: Some(name.to_string()),
        },
        None => StateLine {
            text: fill(c::STATE_WORKING_NO_PLAN, &[("name", name)]),
            tone: StateTone::Live,
            badge: BadgeState::Working,
            short: c::STATE_WORKING_SHORT.to_string(),
            detail: None,
            paused: false,
            name: Some(name.to_string()),
        },
    }
}

/// The state of a 话题 for the title bar, the inspector, the sidebar and
/// the pinned banner (§2.12, A5): one kind and one set of words.
pub fn state_sentence(services: &Services, group_id: &str, topic_id: &str) -> Option<StateLine> {
    let p = &services.provider;
    let topic = p.topic(group_id, topic_id)?;
    Some(topic_state(p.as_ref(), services.me().as_deref(), &topic))
}

/// Whether the provider has my answer to an ask: one on its way or stored
/// (a Card v2 answer to its current rev, a v1 press). An ask with no card
/// (the 话题 itself waits on me) has none.
pub fn ask_answered(p: &dyn ChatProvider, me: &str, ask: &crate::data::v2::AskItem) -> bool {
    let Some(m) = ask.message_id.as_deref() else {
        return false;
    };
    let g = ask.group_id.as_str();
    if let Some(card) = p.card_v2(g, m) {
        let sending = p
            .card_v2_pending(g, m)
            .is_some_and(|x| matches!(x.phase, crate::data::cards_v2::CardV2SendPhase::Sending));
        return sending
            || p.card_v2_answers(g, m)
                .iter()
                .any(|a| a.actor_id == me && a.rev == card.card.rev);
    }
    p.card_presses(g, m).iter().any(|x| x.actor_id == me)
}

/// The asks for me in one 话题 (newest first), and whether each one has my
/// answer at the provider ([`ask_answered`]).
pub fn asks_in_topic(
    p: &dyn ChatProvider,
    me: Option<&str>,
    group_id: &str,
    topic_id: &str,
) -> Vec<(crate::data::v2::AskItem, bool)> {
    let Some(me) = me else {
        return Vec::new();
    };
    let mut asks: Vec<_> = p
        .asks_for_me()
        .into_iter()
        .filter(|a| a.group_id == group_id && a.topic_id == topic_id)
        .map(|a| {
            let done = ask_answered(p, me, &a);
            (a, done)
        })
        .collect();
    asks.sort_by_key(|(a, _)| std::cmp::Reverse(a.at));
    asks
}

/// The words of an open ask for me (§2.12): a review is 等你审, any other
/// ask 等你回答.
pub fn ask_words(kind: crate::data::v2::AskKind) -> &'static str {
    use crate::copy::common as cm;
    match kind {
        crate::data::v2::AskKind::Review => cm::STATUS_WAITING_REVIEW_ME,
        _ => cm::STATUS_WAITING_ANSWER_ME,
    }
}

/// [`state_sentence`] from the provider and the signed-in member (the
/// sidebar and the palette read it without `Services`). In order (§2.12,
/// T-03): a merge in progress, then what waits on me (my review, an open
/// ask, the 话题 itself), then a member stopping, at work or paused, a
/// failed run, then the 话题's own status. 在做 only while a run works.
pub fn topic_state(p: &dyn ChatProvider, me: Option<&str>, topic: &Topic) -> StateLine {
    use crate::copy::{common as cm, fill, rail as cr, topic_rail as c};
    let (g, t) = (topic.group_id.as_str(), topic.id.as_str());
    let name = |id: &str| p.member(id).map(|m| m.name).unwrap_or_default();
    let owner = topic.owner_id.as_deref().map(name).unwrap_or_default();
    let changes = p.topic_changes(g, t);
    if let Some(ch) = &changes
        && ch.state == crate::data::v2::ChangeState::Merging
    {
        return StateLine::plain(
            fill(c::STATE_MERGING, &[("name", &owner)]),
            BadgeState::Merging,
        )
        .short(c::STATE_MERGING_SHORT);
    }
    let asks = asks_in_topic(p, me, g, t);
    let open: Vec<_> = asks
        .iter()
        .filter(|(_, done)| !done)
        .map(|(a, _)| a)
        .collect();
    if let Some(ask) = open.first() {
        // The provider's pinned ask knows a v1 approval that is the PR's
        // review.
        let kind = p
            .pinned_ask(g, t)
            .filter(|x| x.message_id == ask.message_id)
            .map_or(ask.kind, |x| x.kind);
        let review = kind == crate::data::v2::AskKind::Review;
        let text = match &changes {
            Some(ch) if review => fill(c::STATE_REVIEW, &[("n", &ch.number.to_string())]),
            _ => ask_words(kind).to_string(),
        };
        return StateLine::plain(text, BadgeState::NeedsYou).short(c::STATE_YOU_SHORT);
    }
    let mine = me.is_some_and(|me| topic.waiting_on.iter().any(|m| m == me));
    // B1-06: every ask for me here has my answer at the provider, though
    // the 话题 still lists me: answered, until the server's state follows.
    if mine && !asks.is_empty() {
        return StateLine::plain(cr::RAIL_ANSWERED.to_string(), BadgeState::Done)
            .short(cr::RAIL_ANSWERED);
    }
    if mine {
        let text = match topic.status {
            TopicStatus::WaitingReview => cm::STATUS_WAITING_REVIEW_ME,
            _ => cm::STATUS_WAITING_ANSWER_ME,
        };
        return StateLine::plain(text.to_string(), BadgeState::NeedsYou).short(c::STATE_YOU_SHORT);
    }
    let runs = p.runs(g, t);
    if let Some(run) = lead_run(&runs, topic.owner_id.as_deref()) {
        let who = name(&run.bot_id);
        if p.run_stopping(&run.id) {
            let mut line = StateLine::plain(
                fill(c::STATE_STOPPING, &[("name", &who)]),
                BadgeState::Stopping,
            )
            .short(crate::copy::process::STATUS_STOPPING);
            line.name = Some(who);
            return line;
        }
        let paused_by = p.machine_screen(&run.id).and_then(|s| s.paused_by);
        let mut line = run_state(run, &who, paused_by.as_deref(), me, name);
        // A2-11: two members at work are one state, `2 个成员在做`. Usable: the
        // server's `participants` count members whose runs this window has not read.
        let at_work = working_runs(&runs).len().max(topic.working_members().len());
        if at_work > 1 && !line.paused {
            line.text = fill(c::STATE_MANY_WORKING, &[("n", &at_work.to_string())]);
            // A2-05: the sidebar says the same words, not the lead's `2/5`.
            line.short = line.text.clone();
            line.name = None;
            line.detail = None;
        }
        return line;
    }
    // R2-11: a run that waits on other members (not on a person's answer,
    // which `waiting_on` says) is still at work: `Jojo 在做 2/3`, as the
    // 首页 and the sidebar's 在做 section say.
    if topic.waiting_on.is_empty()
        && let Some(run) = runs
            .iter()
            .filter(|r| r.status == RunStatus::Waiting)
            .max_by_key(|r| r.started_at.unwrap_or(r.created_at))
    {
        let who = name(&run.bot_id);
        return run_state(run, &who, None, me, name);
    }
    // The newest run failed and the 话题 is not done: say it, the same word
    // as the run log and the inspector.
    let latest_failed = runs
        .iter()
        .max_by_key(|r| r.updated_at)
        .is_some_and(|r| r.status == RunStatus::Failed);
    if latest_failed && topic.status != TopicStatus::Done {
        return StateLine::plain(
            crate::copy::process::STATUS_FAILED.to_string(),
            BadgeState::Failed,
        )
        .short(crate::copy::process::STATUS_FAILED);
    }
    // Usable: the server says members work here (`participants`) though this
    // window has not read their runs (the sidebar): the same words as above.
    if let Some(line) = participants_state(topic, name) {
        return line;
    }
    // §2.12: 在做 and its spinner only while a member works on a run (above).
    // A 话题 marked in progress with no run is queued: 排队, the clock, muted.
    if topic.status == TopicStatus::InProgress {
        return StateLine::plain(cm::STATUS_QUEUED.to_string(), BadgeState::Queued);
    }
    let members = p.members();
    let text = crate::views::status_chip_text(topic, me, &members)
        .unwrap_or_else(|| cm::STATUS_QUEUED.to_string());
    let badge = match topic.status {
        TopicStatus::Blocked => BadgeState::Failed,
        TopicStatus::Done => BadgeState::Done,
        // Waiting on someone else, or not started: the clock, muted.
        _ => BadgeState::Queued,
    };
    // §2.12, B1-25: waiting on someone else is a state too (等李雪回答,
    // the clock, muted), so the sidebar shows it.
    let short = match topic.status {
        TopicStatus::Blocked | TopicStatus::WaitingAnswer | TopicStatus::WaitingReview => {
            text.clone()
        }
        _ => String::new(),
    };
    StateLine::plain(text, badge).short(short)
}

/// `{name} 在做` or `{n} 个成员在做` from `Topic.participants` alone, when a member
/// works there; `None` otherwise.
pub fn participants_state(topic: &Topic, name: impl Fn(&str) -> String) -> Option<StateLine> {
    use crate::copy::{fill, topic_rail as c};
    let at_work = topic.working_members();
    let text = match at_work.as_slice() {
        [] => return None,
        [one] => fill(c::STATE_WORKING_NO_PLAN, &[("name", &name(one))]),
        many => fill(c::STATE_MANY_WORKING, &[("n", &many.len().to_string())]),
    };
    let short = if at_work.len() > 1 {
        text.clone()
    } else {
        c::STATE_WORKING_SHORT.to_string()
    };
    Some(StateLine::plain(text, BadgeState::Working).short(short))
}

/// The pane buttons of a 话题 page: 单独问, 产出 (when it has one), 电脑
/// (while a member's computer is active), 改动 (when it has a PR), rail.
pub fn pane_buttons(services: &Services, group_id: &str, topic_id: &str) -> Vec<TitleButton> {
    let p = &services.provider;
    let mut out = Vec::new();
    // The 群聊: 详情 (群资料: 成员, 通知, 决定 …) and the 群's 话题; a member's
    // computer while one works here. No 单独问 and no inspector: it is a
    // chat, not work. 新话题 is on the 话题 page, ⌘N and a message's menu.
    if crate::data::is_main_topic(topic_id) {
        out.push(TitleButton::of(TitleButtonKind::Rail, RightPane::GroupInfo));
        out.push(TitleButton::of(
            TitleButtonKind::GroupTopics,
            RightPane::None,
        ));
        if let Some(r) = p
            .runs(group_id, topic_id)
            .iter()
            .find(|r| r.status.is_active())
        {
            out.push(TitleButton::of(
                TitleButtonKind::Machine,
                RightPane::Live(r.id.clone()),
            ));
        }
        return out;
    }
    if let Some(m) = p.side_chat_default_member() {
        out.push(TitleButton::of(
            TitleButtonKind::SideChat,
            RightPane::SideChat(m),
        ));
    }
    if let Some(a) = p.artifacts(group_id, topic_id).first() {
        out.push(TitleButton::of(
            TitleButtonKind::Outputs,
            RightPane::Artifact {
                id: a.id.clone(),
                version: None,
            },
        ));
    }
    if let Some(r) = p
        .runs(group_id, topic_id)
        .iter()
        .find(|r| r.status.is_active())
    {
        out.push(TitleButton::of(
            TitleButtonKind::Machine,
            RightPane::Live(r.id.clone()),
        ));
    }
    if p.topic_changes(group_id, topic_id).is_some() {
        out.push(TitleButton::of(
            TitleButtonKind::Changes,
            RightPane::Changes,
        ));
    }
    out.push(TitleButton::of(TitleButtonKind::Rail, RightPane::TopicRail));
    out
}

/// The toolbar items of a 话题 (NATIVE.md §9.2): 单独问, 产出 and PR as
/// one group, then the inspector toggle. The member's computer opens from
/// the inspector's 在哪干活 and the View menu, not from the toolbar.
pub fn toolbar_buttons(mut buttons: Vec<TitleButton>) -> Vec<TitleButton> {
    buttons.retain(|b| b.kind != TitleButtonKind::Machine);
    buttons
}

impl PageView for TopicPage {
    fn title_bar(&self, cx: &App) -> TitleBarSpec {
        let Some((g, t)) = self.services.nav.read(cx).topic.clone() else {
            return TitleBarSpec::default();
        };
        let p = &self.services.provider;
        let group = p.groups().into_iter().find(|x| x.id == g);
        let peer = group
            .as_ref()
            .and_then(|x| x.dm_peer.as_deref())
            .and_then(|id| p.member(id))
            .map(crate::views::shell::title_bar::Peer);
        let group = group.map(|x| x.name);
        // 1.1.1: the 群聊 is called by its 群 (a 私聊 by the peer) and has
        // no state of its own; its purpose is the subtitle.
        if crate::data::is_main_topic(&t) {
            return TitleBarSpec {
                title: group.unwrap_or_default(),
                style: TitleStyle::Topic,
                subtitle: p.group_purpose(&g),
                peer,
                buttons: toolbar_buttons(pane_buttons(&self.services, &g, &t)),
                ..TitleBarSpec::default()
            };
        }
        let title = p
            .topic(&g, &t)
            .map(|x| crate::views::sidebar::topic_display_title(&x.title).0)
            .unwrap_or_default();
        let state = state_sentence(&self.services, &g, &t);
        // NATIVE.md §9.2: no breadcrumb; the 群 leads the subtitle, the
        // status words follow it (`开发 · 需要你审 PR #61`), and the shell
        // draws the needs-you symbol from the state's tone.
        TitleBarSpec {
            // The shell draws `breadcrumb` as the subtitle's lead, not as a
            // breadcrumb.
            breadcrumb: group,
            title,
            style: TitleStyle::Topic,
            subtitle: None,
            state: state.as_ref().map(|s| (s.text.clone(), s.badge)),
            state_detail: state.as_ref().and_then(|s| s.detail.clone()),
            state_name: state.as_ref().and_then(|s| s.name.clone()),
            peer,
            buttons: toolbar_buttons(pane_buttons(&self.services, &g, &t)),
        }
    }
}

impl Render for TopicPage {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let k = theme::colors(cx);
        let open = self.services.nav.read(cx).topic.clone();
        let pinned = open
            .as_ref()
            .and_then(|(g, t)| self.render_pinned(g, t, window, cx));
        // NATIVE.md §9.2: the decision bar and the composer take the full
        // content width, from the shared leading inset to the transcript's
        // 24 pt trailing inset.
        div()
            .size_full()
            .flex()
            .flex_col()
            .bg(k.bg)
            // Usable: the 按顺序做 bar, under the title, above the plan cards.
            .child(self.relay_strip.clone())
            .child(div().flex_1().min_h(px(0.)).child(self.timeline.clone()))
            .child(
                div()
                    .flex_none()
                    .w_full()
                    .pl(timeline_v2::LEADING)
                    .pr(timeline_v2::SIDE)
                    .pt(space::S8)
                    .pb(space::S16)
                    .flex()
                    .flex_col()
                    .gap(space::S10)
                    .children(pinned)
                    // §2.15: the offline Alert docked above the composer.
                    .children(
                        cx.try_global::<crate::views::banners::AppBanners>()
                            .map(|b| b.connection.clone()),
                    )
                    .child(self.composer.clone()),
            )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(id: &str, status: RunStatus, started: i64, plan: Vec<PlanItem>) -> BotRunSummary {
        BotRunSummary {
            hold: Default::default(),
            id: id.into(),
            bot_id: "forge".into(),
            group_id: "g".into(),
            topic_id: "t".into(),
            machine_id: None,
            machine_name: None,
            status,
            requested_by: None,
            created_at: started,
            started_at: Some(started),
            ended_at: None,
            updated_at: started,
            model: None,
            effort: None,
            summary: None,
            plan,
        }
    }

    fn plan() -> Vec<PlanItem> {
        vec![
            PlanItem::new("读现有列表实现", PlanState::Done),
            PlanItem::new("换成虚拟列表", PlanState::Done),
            PlanItem::new("跑一万条消息的压测", PlanState::Current),
            PlanItem::new("修滚动锚点", PlanState::Pending),
            PlanItem::new("提 PR", PlanState::Pending),
        ]
    }

    #[test]
    fn plan_progress_names_the_current_step() {
        assert_eq!(
            plan_progress(&plan()),
            Some((2, 5, Some("跑一万条消息的压测".into())))
        );
        let done = vec![
            PlanItem::new("a", PlanState::Done),
            PlanItem::new("b", PlanState::Pending),
        ];
        assert_eq!(plan_progress(&done), Some((1, 2, None)));
        assert_eq!(plan_progress(&[]), None);
    }

    #[test]
    fn the_working_run_is_active_and_not_waiting() {
        let runs = vec![
            run("old", RunStatus::Done, 1, plan()),
            run("ask", RunStatus::Waiting, 5, plan()),
            run("now", RunStatus::Running, 3, plan()),
        ];
        assert_eq!(working_run(&runs).map(|r| r.id.as_str()), Some("now"));
        assert!(working_run(&runs[..2]).is_none());
    }

    #[test]
    fn each_member_at_work_has_its_run() {
        let mut other = run("coder", RunStatus::Running, 2, plan());
        other.bot_id = "coder".into();
        let runs = vec![
            run("forge-old", RunStatus::Running, 1, plan()),
            run("forge-ask", RunStatus::Waiting, 6, plan()),
            run("forge-new", RunStatus::Running, 4, plan()),
            other,
            run("forge-done", RunStatus::Done, 9, plan()),
        ];
        let ids: Vec<&str> = working_runs(&runs).iter().map(|r| r.id.as_str()).collect();
        assert_eq!(ids, ["coder", "forge-new"]);
        assert_eq!(working_run(&runs).map(|r| r.id.as_str()), Some("forge-new"));
    }

    #[test]
    fn a_run_reads_as_member_step_and_progress() {
        let r = run("now", RunStatus::Running, 3, plan());
        let line = run_state(&r, "Forge", None, Some("me"), |_| String::new());
        // §2.12: done/total (2 of 5 steps done, on the third).
        assert_eq!(line.text, "Forge 在做 2/5");
        assert_eq!(line.detail.as_deref(), Some("跑一万条消息的压测"));
        assert_eq!(line.tone, StateTone::Live);
        assert!(!line.paused);
    }

    #[test]
    fn a_taken_over_run_reads_as_paused_and_never_live() {
        let r = run("now", RunStatus::Running, 3, plan());
        let mine = run_state(&r, "Forge", Some("me"), Some("me"), |_| String::new());
        assert_eq!(mine.text, "Forge 已暂停");
        assert_eq!(mine.detail.as_deref(), Some("你在操作它的电脑"));
        assert!(mine.paused);
        assert_ne!(mine.tone, StateTone::Live);
        let other = run_state(&r, "Forge", Some("w"), Some("me"), |_| "小王".into());
        assert_eq!(other.detail.as_deref(), Some("小王 在操作它的电脑"));
    }

    #[test]
    fn a_pause_a_wait_and_a_take_over_read_apart() {
        let mut r = run("now", RunStatus::Running, 3, plan());
        r.hold.paused_by = Some("w".into());
        r.hold.hold_kind = Some("pause".into());
        r.hold.phase = Some("paused".into());
        let paused = run_state(&r, "Forge", None, Some("me"), |_| "小王".into());
        assert!(paused.paused);
        assert_eq!(paused.detail.as_deref(), Some("小王 暂停了 Forge"));
        r.hold = Default::default();
        r.hold.phase = Some("waiting_person".into());
        r.hold.waiting_for = Some("question".into());
        r.requested_by = Some("me".into());
        let waiting = run_state(&r, "Forge", None, Some("me"), |_| String::new());
        assert_eq!(waiting.text, "Forge 等你回复");
        assert!(!waiting.paused);
        assert_ne!(waiting.badge, paused.badge);
        // An older server: a waiting run keeps the words of before.
        r.hold.phase = None;
        let old = run_state(&r, "Forge", None, Some("me"), |_| String::new());
        assert_eq!(old.text, "Forge 在做 2/5");
    }
}

#[cfg(test)]
mod participants_tests {
    use super::participants_state;
    use crate::{
        data::{Topic, TopicParticipant, TopicStatus},
        ui::native::BadgeState,
    };

    fn topic(working: &[(&str, bool)]) -> Topic {
        Topic {
            id: "t".into(),
            group_id: "g".into(),
            title: "t".into(),
            status: TopicStatus::InProgress,
            waiting_on: vec![],
            owner_id: None,
            created_by: "me".into(),
            created_at: 0,
            root_message_id: String::new(),
            last_seq: 0,
            last_activity_at: 0,
            message_count: 0,
            unread: 0,
            mentions: 0,
            participants: working
                .iter()
                .enumerate()
                .map(|(i, (id, w))| TopicParticipant {
                    member_id: id.to_string(),
                    working: *w,
                    last_run_at: i as u64,
                })
                .collect(),
            relay: None,
        }
    }

    fn name(id: &str) -> String {
        id.to_uppercase()
    }

    #[test]
    fn the_sidebar_says_who_works_from_participants() {
        assert!(participants_state(&topic(&[]), name).is_none());
        assert!(participants_state(&topic(&[("a", false)]), name).is_none());
        let one = participants_state(&topic(&[("a", false), ("b", true)]), name).unwrap();
        assert_eq!(one.text, "B 在做");
        assert_eq!(one.short, "在做");
        assert_eq!(one.badge, BadgeState::Working);
        let two = participants_state(&topic(&[("a", true), ("b", true)]), name).unwrap();
        assert_eq!(two.text, "2 个成员在做");
        assert_eq!(two.short, "2 个成员在做");
    }
}
