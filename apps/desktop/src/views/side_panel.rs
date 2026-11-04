//! Retired 1.0 view: the 1.1 话题 page has its rail (`views/topic_rail`). The timeline header
//! shares `topic_member_ids`. Section numbers below are DESIGN v2.2's.
//!
//! The right panel of the open topic (DESIGN.md §8.1, GrokBot's right pane):
//! a 92 pt header band (`话题详情` and close, then one context line), then
//! flat sections: the bot's progress, 产出, 文件 and 成员. Empty sections are
//! left out. The process view (过程, `views/process`) is a sub-page with a
//! back button. `Navigation::side_panel_tab` keeps which page shows (0 详情,
//! 1 过程).
//!
//! The panel is closed by default. It opens by itself once per topic when a
//! bot starts work in the open topic or the topic gets its first 产出
//! (`Navigation::auto_open_side_panel`), and never shows with no topic open.

use std::time::Duration;

use gpui::{
    AppContext as _, Context, Entity, InteractiveElement as _, IntoElement, ParentElement, Render,
    StatefulInteractiveElement as _, Styled, Subscription, Window, div,
    prelude::FluentBuilder as _, px, relative,
};
use gpui_component::Disableable as _;
use gpui_component::button::Button;

use crate::ui::NamedButton as _;
use crate::{
    app::{Changes, MentionInComposer, Services},
    copy::{dev, fill, panel as p, process as pc},
    data::{
        BotRunSummary, Change, ChatProvider, GroupId, Member, Message, Millis, PlanState,
        ProcessStep, RunStatus, Topic, TopicId, TransferState,
        card::{CardBlock, CardListItem, CardListStatus},
        create_key,
    },
    theme::{self, Type as _, size, space},
    ui::{self, MemberAvatar, Sym},
    views::{
        artifacts::panel::TopicOutputs,
        cards::{is_ask, waits_for_viewer},
        process::{ProcessView, error_line, step_glyph, step_line},
        time_text,
    },
};

/// Newest messages read for the 成员 list and the bot's state.
const MESSAGE_SCAN: usize = 200;
/// Steps of a run listed under the bot (§8.1 progress).
const PROGRESS_STEPS: usize = 4;
/// Items of a plan list shown at most.
const PLAN_ITEMS: usize = 6;

/// The members of a topic (§8.1 成员, and `n 位成员` in the topic header):
/// the creator, the responsible member, everyone who posted or was
/// @mentioned, and me when I follow it. In that order, without repeats.
pub fn topic_member_ids(
    provider: &dyn ChatProvider,
    me: Option<&str>,
    topic: &Topic,
) -> Vec<String> {
    let messages = provider
        .messages(&topic.group_id, &topic.id, MESSAGE_SCAN)
        .messages;
    member_ids_of(provider, me, topic, &messages)
}

fn member_ids_of(
    provider: &dyn ChatProvider,
    me: Option<&str>,
    topic: &Topic,
    messages: &[Message],
) -> Vec<String> {
    let mut ids: Vec<String> = Vec::new();
    let mut push = |id: &str| {
        if !id.is_empty() && !ids.iter().any(|x| x == id) {
            ids.push(id.to_string());
        }
    };
    push(&topic.created_by);
    if let Some(owner) = &topic.owner_id {
        push(owner);
    }
    for m in messages {
        if m.deleted_at.is_none() {
            push(&m.author_id);
        }
        for id in &m.mentions {
            push(id);
        }
    }
    if let Some(me) = me
        && provider.topic_following(&topic.group_id, &topic.id)
    {
        push(me);
    }
    ids
}

/// What the bot of the topic does now, from the same state as the timeline
/// and the header: a card that waits for me, a run, or its last message.
enum BotState {
    /// A card waits for me: its title, and whether it is an ask card.
    Waits { title: String, ask: bool },
    /// A run is active: its latest step and when it started.
    Working {
        step: Option<String>,
        since: Option<Millis>,
    },
    /// The latest run failed, with its error line.
    Failed { error: Option<String> },
    /// When the bot last posted.
    Last(Millis),
    /// The bot has not posted or worked in the topic.
    Idle,
}

/// One row of the progress list: glyph, color, text, and whether it is the
/// current step (`text-1` 500).
struct PlanRow {
    glyph: Sym,
    color: gpui::Hsla,
    text: String,
    current: bool,
}

pub struct SidePanel {
    services: Services,
    open: Option<(GroupId, TopicId)>,
    topic: Option<Topic>,
    members: Vec<Member>,
    messages: Vec<Message>,
    runs: Vec<BotRunSummary>,
    /// The topic has 产出, files or an upload in progress.
    has_outputs: bool,
    /// When the open topic was opened, and whether it had 产出 then: only
    /// work that starts later opens the panel by itself.
    opened: Option<(Millis, bool)>,
    /// The run whose log the panel asked for, and its status then.
    followed: Option<(String, RunStatus)>,
    ticking: bool,
    /// The 产出 and files of the topic (views/artifacts/panel.rs).
    outputs: Entity<TopicOutputs>,
    process: Entity<ProcessView>,
    _subscriptions: Vec<Subscription>,
}

impl SidePanel {
    pub fn new(services: Services, cx: &mut Context<Self>) -> Self {
        let subscriptions = vec![
            cx.subscribe(&services.hub, |this, _, changes: &Changes, cx| {
                let Some((g, t)) = this.open.clone() else {
                    return;
                };
                let followed = this.followed.as_ref().map(|f| f.0.clone());
                if changes.touches_topic(&g, &t)
                    || changes.any(|c| match c {
                        Change::Topics { group_id } => *group_id == g,
                        Change::Members => true,
                        Change::Runs { group_id, topic_id } => *group_id == g && *topic_id == t,
                        Change::RunLog { run_id } => followed.as_ref() == Some(run_id),
                        _ => false,
                    })
                {
                    this.refresh(cx);
                    cx.notify();
                }
            }),
            cx.observe(&services.nav, |this, nav, cx| {
                let open = nav.read(cx).topic.clone();
                if open != this.open {
                    this.open = open;
                    this.opened = None;
                    this.followed = None;
                }
                this.refresh(cx);
                this.sync_process_visibility(cx);
                cx.notify();
            }),
        ];
        let open = services.nav.read(cx).topic.clone();
        let outputs = cx.new(|cx| TopicOutputs::new(services.clone(), cx));
        let process = cx.new(|cx| ProcessView::new(services.clone(), cx));
        cx.observe(&process, |_, _, cx| cx.notify()).detach();
        let mut this = Self {
            outputs,
            services,
            open,
            topic: None,
            members: Vec::new(),
            messages: Vec::new(),
            runs: Vec::new(),
            has_outputs: false,
            opened: None,
            followed: None,
            ticking: false,
            process,
            _subscriptions: subscriptions,
        };
        this.refresh(cx);
        this.sync_process_visibility(cx);
        this
    }

    /// Switches between 详情 (0) and the 过程 sub-page (1).
    pub fn set_tab(&mut self, tab: usize, cx: &mut Context<Self>) {
        self.services
            .nav
            .update(cx, |nav, cx| nav.set_side_panel_tab(tab.min(1), cx));
    }

    pub fn process(&self) -> &Entity<ProcessView> {
        &self.process
    }

    fn sync_process_visibility(&mut self, cx: &mut Context<Self>) {
        let nav = self.services.nav.read(cx);
        let visible = nav.side_panel_tab == 1 && nav.side_panel_open;
        self.process
            .update(cx, |view, cx| view.set_visible(visible, cx));
    }

    fn refresh(&mut self, cx: &mut Context<Self>) {
        let provider = self.services.provider.clone();
        self.members = provider.members();
        let Some((g, t)) = self.open.clone() else {
            self.topic = None;
            self.messages.clear();
            self.runs.clear();
            self.has_outputs = false;
            return;
        };
        self.topic = provider.topic(&g, &t);
        self.messages = provider.messages(&g, &t, MESSAGE_SCAN).messages;
        self.runs = provider.runs(&g, &t);
        let artifacts = provider.artifacts(&g, &t);
        let uploading = matches!(
            provider.file_transfer(&create_key(&t)),
            Some(TransferState::Running { .. })
        );
        self.has_outputs =
            !artifacts.is_empty() || uploading || !provider.topic_files(&g, &t).is_empty();

        // Opens by itself when a bot starts work or the first 产出 arrives
        // after the topic was opened (DESIGN.md §8.1).
        let now = crate::selftest::clock::utc_now().timestamp_millis();
        match self.opened {
            None => self.opened = Some((now, !artifacts.is_empty())),
            Some((since, had_artifacts)) => {
                let started = self.runs.iter().any(|r| {
                    r.status.is_active()
                        && r.started_at.unwrap_or(r.created_at) >= since
                        && self.member(&r.bot_id).is_some_and(Member::is_bot)
                });
                let first_output =
                    !had_artifacts && artifacts.iter().any(|a| a.updated_at >= since);
                if started || first_output {
                    self.services
                        .nav
                        .update(cx, |nav, cx| nav.auto_open_side_panel(cx));
                }
            }
        }

        // Ask for the log of the bot's newest run, once per status.
        if let Some(run) = self.bot().and_then(|bot| self.newest_run(&bot.id).cloned()) {
            let key = (run.id.clone(), run.status);
            if self.followed.as_ref() != Some(&key) {
                provider.follow_run(&run.id);
                self.followed = Some(key);
            }
        }
        self.keep_ticking(cx);
    }

    /// Repaints once a second while the bot works, for the elapsed time.
    fn keep_ticking(&mut self, cx: &mut Context<Self>) {
        if self.ticking || !self.working() {
            return;
        }
        self.ticking = true;
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(Duration::from_secs(1)).await;
                let go_on = this
                    .update(cx, |this, cx| {
                        let go_on = this.working();
                        if go_on {
                            cx.notify();
                        } else {
                            this.ticking = false;
                        }
                        go_on
                    })
                    .unwrap_or(false);
                if !go_on {
                    break;
                }
            }
        })
        .detach();
    }

    fn working(&self) -> bool {
        self.bot()
            .and_then(|bot| self.newest_run(&bot.id))
            .is_some_and(|r| r.status.is_active())
    }

    fn member(&self, id: &str) -> Option<&Member> {
        self.members.iter().find(|m| m.id == id)
    }

    fn name(&self, id: &str) -> String {
        self.member(id).map(|m| m.name.clone()).unwrap_or_default()
    }

    /// The bot the progress section is about: the responsible member when
    /// it is a bot, otherwise the bot of the newest active run.
    fn bot(&self) -> Option<&Member> {
        let topic = self.topic.as_ref()?;
        if let Some(owner) = topic.owner_id.as_deref().and_then(|id| self.member(id))
            && owner.is_bot()
        {
            return Some(owner);
        }
        if let Some(bot) = self
            .runs
            .iter()
            .filter(|r| r.status.is_active())
            .find_map(|r| self.member(&r.bot_id).filter(|m| m.is_bot()))
        {
            return Some(bot);
        }
        // The bot whose newest run has a plan.
        if let Some(bot) = self.runs.iter().find_map(|r| {
            let bot = self.member(&r.bot_id).filter(|m| m.is_bot())?;
            (self.newest_run(&bot.id)?.id == r.id && !r.plan.is_empty()).then_some(bot)
        }) {
            return Some(bot);
        }
        // The bot that posted the newest plan list.
        self.messages.iter().rev().find_map(|m| {
            let bot = self.member(&m.author_id).filter(|b| b.is_bot())?;
            let card = self.services.provider.card(&topic.group_id, &m.id)?;
            card.blocks
                .iter()
                .any(|b| matches!(b, CardBlock::List { items } if is_plan(items)))
                .then_some(bot)
        })
    }

    /// The newest run of a bot (runs come newest first).
    fn newest_run(&self, bot_id: &str) -> Option<&BotRunSummary> {
        self.runs.iter().find(|r| r.bot_id == bot_id)
    }

    /// The bot's state, by priority: a card that waits for me, an active
    /// run, a failed run newer than its last message, its last message.
    fn bot_state(&self, topic: &Topic, bot: &Member) -> BotState {
        let provider = &self.services.provider;
        let me = self.services.me();
        let waits_on_me = me
            .as_deref()
            .is_some_and(|me| topic.waiting_on.iter().any(|m| m == me));
        let last_message = self
            .messages
            .iter()
            .rev()
            .find(|m| m.author_id == bot.id && m.deleted_at.is_none());
        if waits_on_me {
            // The bot's newest card: it waits for me while it has the
            // `warning` tone and I have not pressed it (the timeline's rule).
            let card = self
                .messages
                .iter()
                .rev()
                .filter(|m| m.author_id == bot.id && m.deleted_at.is_none())
                .find_map(|m| provider.card(&topic.group_id, &m.id).map(|c| (m, c)));
            if let Some((m, card)) = card
                && waits_for_viewer(&card)
                && !provider
                    .card_presses(&topic.group_id, &m.id)
                    .iter()
                    .any(|press| Some(&press.actor_id) == me.as_ref())
            {
                return BotState::Waits {
                    title: card.title.clone(),
                    ask: is_ask(&card),
                };
            }
        }
        if let Some(run) = self.newest_run(&bot.id) {
            if run.status.is_active() {
                let steps = provider.run_process(&run.id).steps;
                return BotState::Working {
                    step: live_step(&steps),
                    since: run.started_at,
                };
            }
            // A failed run stays the state until the bot works again, even
            // when it reported the failure in a message.
            if run.status == RunStatus::Failed {
                let steps = provider.run_process(&run.id).steps;
                return BotState::Failed {
                    error: error_line(&steps).or_else(|| run.summary.clone()),
                };
            }
        }
        match last_message {
            Some(m) => BotState::Last(m.created_at),
            None => BotState::Idle,
        }
    }

    /// The progress list, first match wins: the plan of the bot's newest run
    /// (its own to-do list), a plan list from a card in the topic (statuses
    /// on its items), else the run's latest steps. `waits`: the bot waits for
    /// me, so a card plan's current step shows `eye` (the step is my review,
    /// as in the mockup's `等 Joye 确认`); a run plan says so itself with a
    /// `waiting` item.
    fn plan_rows(
        &self,
        topic: &Topic,
        bot: &Member,
        waits: bool,
        cx: &Context<Self>,
    ) -> Vec<PlanRow> {
        let k = theme::colors(cx);
        let provider = &self.services.provider;
        if let Some(run) = self.newest_run(&bot.id)
            && !run.plan.is_empty()
        {
            let items = &run.plan;
            let start = plan_window(
                items.iter().position(|i| i.state != PlanState::Done),
                items.len(),
            );
            return items[start..]
                .iter()
                .take(PLAN_ITEMS)
                .map(|item| {
                    let (glyph, color, current) = match item.state {
                        PlanState::Done => (Sym::Done, k.success, false),
                        PlanState::Current => (Sym::Live, k.text_1, true),
                        PlanState::Waiting => (Sym::Watch, k.text_1, true),
                        PlanState::Pending => (Sym::Todo, k.text_3, false),
                    };
                    PlanRow {
                        glyph,
                        color,
                        text: item.text.clone(),
                        current,
                    }
                })
                .collect();
        }
        let plan = self.messages.iter().rev().find_map(|m| {
            let card = provider.card(&topic.group_id, &m.id)?;
            card.blocks.iter().find_map(|b| match b {
                CardBlock::List { items } if is_plan(items) => Some(items.clone()),
                _ => None,
            })
        });
        if let Some(items) = plan {
            // A long plan shows a window around its current item.
            let current = items
                .iter()
                .position(|i| {
                    !matches!(
                        i.status,
                        Some(CardListStatus::Done | CardListStatus::Skipped)
                    )
                })
                .unwrap_or(items.len());
            let start = plan_window(Some(current), items.len());
            return items[start..]
                .iter()
                .take(PLAN_ITEMS)
                .map(|item| {
                    let (glyph, color, current) = match item.status {
                        Some(CardListStatus::Done) => (Sym::Done, k.success, false),
                        Some(CardListStatus::InProgress) => {
                            (if waits { Sym::Watch } else { Sym::Live }, k.text_1, true)
                        }
                        Some(CardListStatus::Failed) => (Sym::Failed, k.danger, false),
                        Some(CardListStatus::Skipped) => (Sym::Minus, k.text_3, false),
                        Some(CardListStatus::Todo) | None => (Sym::Todo, k.text_3, false),
                    };
                    PlanRow {
                        glyph,
                        color,
                        text: item.text.clone(),
                        current,
                    }
                })
                .collect();
        }
        let Some(run) = self.newest_run(&bot.id) else {
            return Vec::new();
        };
        let steps = provider.run_process(&run.id).steps;
        let mut rows: Vec<PlanRow> = steps
            .iter()
            .filter_map(|s| {
                let text = step_line(s, &bot.name)?;
                let (glyph, color) = step_glyph(s, cx);
                Some(PlanRow {
                    glyph,
                    color,
                    text,
                    current: false,
                })
            })
            .collect();
        let start = rows.len().saturating_sub(PROGRESS_STEPS);
        let mut rows = rows.split_off(start);
        if run.status.is_active()
            && let Some(last) = rows.last_mut()
        {
            last.current = true;
        }
        rows
    }

    /// The one context line of the header (§8.1): what the timeline does not
    /// show. `Taro 负责 · 等 Alex Chen`, or `Taro 负责 · 14:31 更新`.
    fn context_line(&self, topic: &Topic) -> String {
        let mut parts = Vec::new();
        match topic.owner_id.as_deref() {
            Some(owner) => parts.push(fill(
                p::CONTEXT_OWNER,
                &[("name", &crate::copy::isolate(&self.name(owner)))],
            )),
            None => parts.push(fill(
                p::CONTEXT_STARTED,
                &[("name", &crate::copy::isolate(&self.name(&topic.created_by)))],
            )),
        }
        let me = self.services.me();
        let others: Vec<String> = topic
            .waiting_on
            .iter()
            .filter(|id| Some(*id) != me.as_ref())
            .map(|id| crate::copy::isolate(&self.name(id)))
            .collect();
        if !others.is_empty() {
            parts.push(fill(
                p::CONTEXT_WAITING,
                &[("names", &others.join(p::NAME_JOIN))],
            ));
        } else {
            // The newest message is the topic's last update; the topic's
            // activity time is the fallback before messages load.
            let at = self
                .messages
                .iter()
                .rev()
                .find(|m| m.created_seq > 0)
                .map_or(topic.last_activity_at, |m| m.created_at);
            parts.push(updated_text(at));
        }
        crate::copy::ltr(parts.join(" · "))
    }

    /// `停止` in the 过程 header while the shown run is active (§9.2): a
    /// secondary `md` button; once pressed it is disabled and reads
    /// `正在停止…` until the run ends. Any person in the 群 may stop a run.
    fn stop_button(&self, cx: &mut Context<Self>) -> Option<Button> {
        let run = self.process.read(cx).selected_run()?.clone();
        if !run.status.is_active() || self.services.me().is_none() {
            return None;
        }
        let k = theme::colors(cx);
        let stopping = self.services.provider.run_stopping(&run.id);
        Some(
            ui::native::push_button(
                "panel-stop",
                if stopping { pc::STOPPING } else { pc::STOP },
                ui::native::PushStyle::Outline,
                ui::native::PushSize::Regular,
                cx,
            )
            .mr(space::S4)
            .disabled(stopping)
            .when(stopping, |b| b.text_color(k.text_3))
            .on_click(cx.listener(move |this, _, _, cx| {
                cx.stop_propagation();
                crate::views::process::stop_run(&this.services, &run, cx);
                cx.notify();
            })),
        )
    }

    fn header(&self, topic: Option<&Topic>, sub: bool, cx: &mut Context<Self>) -> gpui::Div {
        let k = theme::colors(cx);
        let title_row = ui::toolbar_band("panel-toolbar")
            .pl(if sub { space::S8 } else { space::S16 })
            .pr(space::S10)
            .gap(space::S4)
            .when(sub, |el| {
                el.child(
                    ui::native::icon_button_bare("panel-back", Sym::ChevronLeft, ui::native::IconSize::Regular)
                        .named(p::BACK)
                        .on_click(cx.listener(|this, _, _, cx| {
                            cx.stop_propagation();
                            this.set_tab(0, cx)
                        })),
                )
            })
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.))
                    .t_title()
                    .text_color(k.text_1)
                    .one_line()
                    .child(if sub { pc::TAB_PROCESS } else { p::HEADER }),
            )
            .when_some(sub.then(|| self.stop_button(cx)).flatten(), |el, b| {
                el.child(b)
            })
            .child(
                ui::native::icon_button_bare("panel-close", Sym::Close, ui::native::IconSize::Regular)
                    .named(p::CLOSE)
                    .on_click(cx.listener(|this, _, _, cx| {
                        cx.stop_propagation();
                        this.services
                            .nav
                            .update(cx, |nav, cx| nav.set_side_panel_open(false, cx));
                    })),
            );
        let context = topic.map(|t| self.context_line(t)).unwrap_or_default();
        ui::header_band(cx).child(title_row).child(
            div()
                .flex_none()
                .h(theme::HEADER_SECOND_ROW - theme::HAIRLINE)
                .px(space::S16)
                .flex()
                .items_center()
                .t_small()
                .tnum()
                .text_color(k.text_2)
                .child(div().min_w(px(0.)).one_line().child(context)),
        )
    }

    /// The bot's live state (§8.1 Progress): flat, a 36 pt tile, the name
    /// 14/600, the state 13/20 `text-2`, the plan or latest steps in 28 pt
    /// rows, and `查看过程`.
    fn progress(&self, topic: &Topic, bot: &Member, cx: &mut Context<Self>) -> gpui::AnyElement {
        let k = theme::colors(cx);
        let now = crate::selftest::clock::utc_now().timestamp_millis();
        let state = self.bot_state(topic, bot);
        let waits = matches!(state, BotState::Waits { .. });
        let mut error = None;
        let mut state_color = k.text_2;
        let mut trailing = None;
        let state_text = match state {
            BotState::Waits { title, ask } => fill(
                if ask {
                    p::STATE_ANSWER
                } else {
                    p::STATE_REVIEW
                },
                &[("title", &title)],
            ),
            BotState::Working { step, since } => {
                trailing = since.map(|at| fill(p::STATE_TIME, &[("time", &elapsed(at, now))]));
                step.unwrap_or_else(|| p::STATE_WORKING.to_string())
            }
            BotState::Failed { error: e } => {
                error = e;
                state_color = k.danger_text;
                p::STATE_FAILED.to_string()
            }
            BotState::Last(at) => fill(p::STATE_LAST, &[("time", &time_text(at))]),
            BotState::Idle => p::PROGRESS_IDLE.to_string(),
        };
        let rows = self.plan_rows(topic, bot, waits, cx);
        let has_run = self.newest_run(&bot.id).is_some();
        div()
            .flex()
            .flex_col()
            .child(ui::panel_label(
                fill(p::PROGRESS, &[("name", &bot.name)]),
                None,
                cx,
            ))
            .child(
                div()
                    .mt(space::S8)
                    .flex()
                    .items_center()
                    .gap(space::S12)
                    .child(MemberAvatar::new(Some(bot), size::AVATAR_MESSAGE).on(k.content))
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(0.))
                            .flex()
                            .flex_col()
                            .child(
                                div()
                                    .t_ui()
                                    .semibold()
                                    .text_color(k.text_1)
                                    .one_line()
                                    .child(bot.name.clone()),
                            )
                            .child(
                                div()
                                    .flex()
                                    .gap(space::S4)
                                    .t_small()
                                    .tnum()
                                    .text_color(state_color)
                                    .child(div().min_w(px(0.)).one_line().child(state_text))
                                    .when_some(trailing, |el, t| {
                                        el.child(div().flex_none().child(t))
                                    }),
                            ),
                    ),
            )
            .when_some(error, |el, e| {
                el.child(
                    div()
                        .mt(space::S8)
                        .t_small()
                        .text_color(k.text_2)
                        .line_clamp(2)
                        .child(e),
                )
            })
            .when(!rows.is_empty(), |el| {
                el.child(
                    div()
                        .mt(space::S8)
                        .flex()
                        .flex_col()
                        .children(rows.into_iter().map(|row| {
                            div()
                                .h(size::PLAN_ROW)
                                .flex()
                                .items_center()
                                .gap(space::S10)
                                .child(div().flex_none().child(ui::icon(
                                    row.glyph,
                                    theme::icon::ROW,
                                    row.color,
                                )))
                                .child(
                                    div()
                                        .flex_1()
                                        .min_w(px(0.))
                                        .t_small()
                                        .text_color(if row.current { k.text_1 } else { k.text_2 })
                                        .when(row.current, |el| el.medium())
                                        .one_line()
                                        .child(row.text),
                                )
                        })),
                )
            })
            .when(has_run, |el| {
                el.child(
                    div().mt(space::S4).ml(-space::S6).flex().child(
                        ui::native::push_button("panel-view-process", p::VIEW_PROCESS, ui::native::PushStyle::Ghost, ui::native::PushSize::Small, cx)
                            .on_click(cx.listener(|this, _, _, cx| this.set_tab(1, cx))),
                    ),
                )
            })
            .into_any_element()
    }

    /// 成员 · n (§8.1): 40 pt rows, a 24 pt avatar, the name `ui` 14 and the
    /// 岗位 trailing in 13/20 `text-3` (a 12 pt `bot` glyph before it for
    /// bots); then `添加成员`, which starts an @ in the composer.
    fn members_section(&self, topic: &Topic, cx: &mut Context<Self>) -> gpui::AnyElement {
        let k = theme::colors(cx);
        let me = self.services.me();
        let ids = member_ids_of(
            self.services.provider.as_ref(),
            me.as_deref(),
            topic,
            &self.messages,
        );
        let list: Vec<&Member> = ids.iter().filter_map(|id| self.member(id)).collect();
        let count = list.len() as u32;
        let rows = list.into_iter().map(|m| {
            div()
                .h(size::PANEL_ROW)
                .flex()
                .items_center()
                .gap(space::S10)
                .child(
                    MemberAvatar::new(Some(m), size::AVATAR_ROW)
                        .presence(Some(m))
                        .on(k.content),
                )
                // The name identifies the member, so it keeps its width (up
                // to 65 % of the row) and the 岗位 gives way first.
                .child(
                    ui::truncating(
                        gpui::ElementId::Name(format!("panel-member-{}", m.id).into()),
                        m.name.clone(),
                    )
                    .flex_shrink_0()
                    .max_w(relative(0.65))
                    .t_ui()
                    .text_color(k.text_1),
                )
                .when_some(m.role.clone().filter(|r| !r.is_empty()), |el, role| {
                    el.child(
                        div()
                            .ml_auto()
                            .flex_shrink_1()
                            .min_w(px(0.))
                            .max_w(px(96.))
                            .flex()
                            .items_center()
                            .gap(space::S4)
                            .t_small()
                            .text_color(k.text_3)
                            .when(m.is_bot(), |el| {
                                el.child(ui::icon(Sym::AiMember, theme::icon::TAG, k.text_3))
                            })
                            .child(ui::truncating(
                                gpui::ElementId::Name(format!("panel-role-{}", m.id).into()),
                                role,
                            )),
                    )
                })
        });
        let add = crate::ui::native::row_button("panel-add-member", cx)
            .h(size::PANEL_ROW)
            .mx(-space::S8)
            .px(space::S8)
            .rounded(theme::radius::MD)
            .child(
                div()
                    .w_full()
                    .flex()
                    .items_center()
                    .gap(space::S10)
                    .child(
                        div()
                            .flex_none()
                            .size(size::AVATAR_ROW)
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(ui::icon(Sym::Invite, theme::icon::ROW, k.accent_text)),
                    )
                    .child(
                        div()
                            .t_ui()
                            .medium()
                            .text_color(k.accent_text)
                            .child(p::ADD_MEMBER),
                    ),
            )
            .on_click(|_, window, cx| window.dispatch_action(Box::new(MentionInComposer), cx));
        div()
            .flex()
            .flex_col()
            .child(
                div()
                    .mb(space::S4)
                    .child(ui::panel_label(p::MEMBERS, Some(count), cx)),
            )
            .children(rows)
            .child(add)
            .into_any_element()
    }
}

/// Where a long plan's window of [`PLAN_ITEMS`] starts: two items before the
/// first one that is not done, so the current item and what follows show.
fn plan_window(current: Option<usize>, len: usize) -> usize {
    current
        .unwrap_or(len)
        .saturating_sub(2)
        .min(len.saturating_sub(PLAN_ITEMS))
}

/// A plan list (§8.1): a card list whose items carry statuses.
fn is_plan(items: &[CardListItem]) -> bool {
    items.len() >= 2 && items.iter().all(|i| i.status.is_some())
}

/// The bot's latest step as one phrase: its newest status line (what the
/// timeline's live line shows).
fn live_step(steps: &[ProcessStep]) -> Option<String> {
    steps.iter().rev().find_map(|s| match s {
        ProcessStep::Status { message } if !message.trim().is_empty() => {
            Some(message.trim().to_string())
        }
        _ => None,
    })
}

/// `m:ss` since `since` (the live line's format), `h:mm:ss` from an hour.
fn elapsed(since: Millis, now: Millis) -> String {
    let s = ((now - since).max(0) / 1000) as u64;
    if s >= 3600 {
        format!("{}:{:02}:{:02}", s / 3600, s / 60 % 60, s % 60)
    } else {
        format!("{}:{:02}", s / 60, s % 60)
    }
}

/// `14:31 更新`, or `刚刚更新`: a space only after a number.
fn updated_text(at: Millis) -> String {
    let now = crate::selftest::clock::utc_now().timestamp_millis();
    let time = time_text(at.min(now));
    let spaced = time.chars().last().is_some_and(|c| c.is_ascii_digit());
    fill(
        if spaced {
            p::CONTEXT_UPDATED
        } else {
            p::CONTEXT_UPDATED_WORD
        },
        &[("time", &time)],
    )
}

impl Render for SidePanel {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let k = theme::colors(cx);
        let developer = self.services.nav.read(cx).developer_view;
        let tab = self.services.nav.read(cx).side_panel_tab;
        let topic = self.topic.clone();
        let page = div().size_full().flex().flex_col().bg(k.content).t_ui();
        let Some(topic) = topic else {
            return page;
        };
        if tab == 1 {
            return page
                .child(self.header(Some(&topic), true, cx))
                .child(div().flex_1().min_h(px(0.)).child(self.process.clone()));
        }
        let bot = self.bot().cloned();
        let progress = bot.map(|m| self.progress(&topic, &m, cx));
        let members = self.members_section(&topic, cx);
        page.child(self.header(Some(&topic), false, cx)).child(
            div()
                .id("panel-body")
                .flex_1()
                .min_h(px(0.))
                .overflow_y_scroll()
                .px(space::S16)
                .pt(space::S16)
                .pb(space::S24)
                .flex()
                .flex_col()
                .gap(space::S16)
                .children(progress)
                .when(self.has_outputs, |el| el.child(self.outputs.clone()))
                .child(members)
                .when(developer, |el| {
                    el.child(
                        div()
                            .flex()
                            .flex_col()
                            .gap(space::S8)
                            .child(ui::panel_label(dev::TITLE, None, cx))
                            .child(
                                div()
                                    .flex()
                                    .flex_col()
                                    .t_code()
                                    .text_color(k.text_2)
                                    .child(fill(dev::GROUP_ID, &[("id", &topic.group_id)]))
                                    .child(fill(dev::TOPIC_ID, &[("id", &topic.id)]))
                                    .child(fill(
                                        dev::LAST_SEQ,
                                        &[("seq", &topic.last_seq.to_string())],
                                    )),
                            ),
                    )
                }),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_long_plan_shows_a_window_around_its_first_open_item() {
        // Short plans show whole.
        assert_eq!(plan_window(Some(3), 4), 0);
        assert_eq!(plan_window(None, 4), 0);
        // Two done items stay above the current one, when the rest fits.
        assert_eq!(plan_window(Some(5), 12), 3);
        // Near the end the window keeps PLAN_ITEMS rows.
        assert_eq!(plan_window(Some(11), 12), 12 - PLAN_ITEMS);
        assert_eq!(plan_window(None, 12), 12 - PLAN_ITEMS);
    }
}
