//! The 话题 page (NATIVE.md §9.2): the timeline and composer under the
//! shell's toolbar row (title = the 话题, subtitle = 群 · status words, the
//! 单独问 / 产出 / PR group and the inspector toggle).
//!
//! The timeline draws in its v2 style (`timeline/v2.rs`); under it, with
//! the transcript's 24 pt side insets, the decision bar (`pinned.rs`) sits
//! above the native composer (`composer/native.rs`).

pub mod output_pane;
mod pinned;
pub mod scenes;

use gpui::{
    App, Context, Entity, IntoElement, ParentElement, Render, Styled, Subscription, Window, div, px,
};

use crate::{
    app::{RightPane, Services},
    data::{BotRunSummary, PlanItem, PlanState, RunStatus, TopicStatus},
    theme::{self, StateTone, space},
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
        Self {
            _subscriptions: watch(&services, cx),
            services,
            timeline,
            composer,
        }
    }
}

/// The state line of a 话题 in the title bar.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StateLine {
    pub text: String,
    pub tone: StateTone,
    /// The quieter tail: the step a member is on, or who holds its computer.
    pub detail: Option<String>,
    /// The member's run is paused (someone took over its computer).
    pub paused: bool,
    /// The member's name that leads `text`: the title bar cuts it with `…`
    /// before it cuts `在做 3/5`.
    pub name: Option<String>,
}

impl StateLine {
    fn plain(text: String, tone: StateTone) -> Self {
        Self {
            text,
            tone,
            detail: None,
            paused: false,
            name: None,
        }
    }
}

/// Where a run's plan stands: `(n, total, step)`, with `n` the step it is
/// on (or the steps done when none is on) and `step` the text of that
/// step. `None` when the run has no plan.
pub fn plan_progress(plan: &[PlanItem]) -> Option<(usize, usize, Option<String>)> {
    if plan.is_empty() {
        return None;
    }
    let current = plan
        .iter()
        .position(|p| matches!(p.state, PlanState::Current | PlanState::Waiting));
    let n = current.map_or_else(
        || plan.iter().filter(|p| p.state == PlanState::Done).count(),
        |i| i + 1,
    );
    Some((n, plan.len(), current.map(|i| plan[i].text.clone())))
}

/// The run a member works on in this 话题 now: active and not waiting for
/// a person (an answer or a review is said elsewhere), the latest first.
pub fn working_run(runs: &[BotRunSummary]) -> Option<&BotRunSummary> {
    working_runs(runs).pop()
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
    who: impl FnOnce(&str) -> String,
) -> StateLine {
    use crate::copy::{fill, topic_rail as c};
    if let Some(by) = paused_by {
        let detail = if Some(by) == me {
            c::STATE_PAUSED_YOU.to_string()
        } else {
            fill(c::STATE_PAUSED_OTHER, &[("who", &who(by))])
        };
        return StateLine {
            text: fill(c::STATE_PAUSED, &[("name", name)]),
            tone: StateTone::Quiet,
            detail: Some(detail),
            paused: true,
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
            detail: step,
            paused: false,
            name: Some(name.to_string()),
        },
        None => StateLine {
            text: fill(c::STATE_WORKING_NO_PLAN, &[("name", name)]),
            tone: StateTone::Live,
            detail: None,
            paused: false,
            name: Some(name.to_string()),
        },
    }
}

/// The state line of a 话题: amber only when it waits on me; while a
/// member works on it, what it does (Live.dc.html); quiet otherwise.
pub fn state_sentence(services: &Services, group_id: &str, topic_id: &str) -> Option<StateLine> {
    let p = &services.provider;
    let topic = p.topic(group_id, topic_id)?;
    let members = p.members();
    let me = services.me();
    let owner = topic
        .owner_id
        .as_deref()
        .map(|id| crate::views::shell::member_name(services, id))
        .unwrap_or_default();
    // The change of the 话题 says it best: it merges, or it waits on my review.
    if let Some(ch) = p.topic_changes(group_id, topic_id) {
        if ch.state == crate::data::v2::ChangeState::Merging {
            return Some(StateLine::plain(
                crate::copy::fill(crate::copy::topic_rail::STATE_MERGING, &[("name", &owner)]),
                StateTone::Live,
            ));
        }
        if p.pinned_ask(group_id, topic_id)
            .is_some_and(|a| a.kind == crate::data::v2::AskKind::Review)
        {
            return Some(StateLine::plain(
                crate::copy::fill(
                    crate::copy::topic_rail::STATE_REVIEW,
                    &[("n", &ch.number.to_string())],
                ),
                StateTone::You,
            ));
        }
    }
    let mine = me
        .as_deref()
        .is_some_and(|me| topic.waiting_on.iter().any(|m| m == me));
    if !mine {
        let runs = p.runs(group_id, topic_id);
        if let Some(run) = working_run(&runs) {
            let paused_by = p.machine_screen(&run.id).and_then(|s| s.paused_by);
            let name = crate::views::shell::member_name(services, &run.bot_id);
            return Some(run_state(
                run,
                &name,
                paused_by.as_deref(),
                me.as_deref(),
                |id| crate::views::shell::member_name(services, id),
            ));
        }
    }
    let text = crate::views::status_chip_text(&topic, me.as_deref(), &members)?;
    // Blue is for a member at work; a 话题 in progress with no run is quiet.
    let tone = match topic.status {
        _ if mine => StateTone::You,
        TopicStatus::Blocked => StateTone::Problem,
        TopicStatus::Done => StateTone::Done,
        _ => StateTone::Quiet,
    };
    Some(StateLine::plain(text, tone))
}

/// The pane buttons of a 话题 page: 单独问, 产出 (when it has one), 电脑
/// (while a member's computer is active), 改动 (when it has a PR), rail.
pub fn pane_buttons(services: &Services, group_id: &str, topic_id: &str) -> Vec<TitleButton> {
    let p = &services.provider;
    let mut out = Vec::new();
    if let Some(m) = p.side_chat_default_member() {
        out.push(TitleButton::new(
            TitleButtonKind::SideChat,
            RightPane::SideChat(m),
        ));
    }
    if let Some(a) = p.artifacts(group_id, topic_id).first() {
        out.push(TitleButton::new(
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
        out.push(TitleButton::new(
            TitleButtonKind::Machine,
            RightPane::Live(r.id.clone()),
        ));
    }
    if p.topic_changes(group_id, topic_id).is_some() {
        out.push(TitleButton::new(
            TitleButtonKind::Changes,
            RightPane::Changes,
        ));
    }
    out.push(TitleButton::new(
        TitleButtonKind::Rail,
        RightPane::TopicRail,
    ));
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
        let group = p.groups().into_iter().find(|x| x.id == g).map(|x| x.name);
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
            state: state.as_ref().map(|s| (s.text.clone(), s.tone)),
            state_detail: state.as_ref().and_then(|s| s.detail.clone()),
            state_paused: state.as_ref().is_some_and(|s| s.paused),
            state_name: state.as_ref().and_then(|s| s.name.clone()),
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
                    .child(self.composer.clone()),
            )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(id: &str, status: RunStatus, started: i64, plan: Vec<PlanItem>) -> BotRunSummary {
        BotRunSummary {
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
            Some((3, 5, Some("跑一万条消息的压测".into())))
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
        assert_eq!(line.text, "Forge 在做 3/5");
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
}
