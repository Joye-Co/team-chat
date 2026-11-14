//! The topic inspector beside a 话题 page (NATIVE.md §9.3, `native.rs`):
//! sections 通知, 负责, 计划, 改动, 产出, 用到的决定, 在哪干活 and 用时和成本 on
//! `sidebar`, each hidden when it has no data. Also the 改动 pane.
//!
//! The rail shows status only: the merge decision is the pinned card above
//! the composer, never a second button here.

use std::{collections::HashMap, rc::Rc};

use gpui::{
    AnyElement, App, Context, ElementId, Hsla, InteractiveElement as _, IntoElement, ParentElement,
    Pixels, Render, StatefulInteractiveElement as _, Styled, Subscription, Window, div,
    prelude::FluentBuilder as _, px,
};

use crate::{
    app::{Changes, RightPane, Services},
    copy::{fill, topic_rail as c},
    data::{
        ArtifactKind, ArtifactSummary, BotRunSummary, Member, MemberId, Millis, PlanState, Topic,
        v2::{ArtifactPreview, Currency, Decision, Proof, TopicChanges, TopicCost},
    },
    theme::{self, Type as _, space},
    ui::{self, native::NativeType as _},
    views::shell::{PaneView, panes::watch},
};

/// What the rail of one 话题 shows, read from the provider once per change
/// or 话题 switch. A breathing ring renders the rail on every frame it asks
/// for, so render reads only this.
struct Data {
    group_id: String,
    topic_id: String,
    topic: Option<Topic>,
    /// The one state of the 话题 (`topic_page::topic_state`).
    state: Option<crate::views::topic_page::StateLine>,
    /// The run the rail shows (`rail_run`): the owner's newest, else the newest.
    run: Option<BotRunSummary>,
    step_times: Vec<Millis>,
    /// The other members' runs at work with a plan, and their step times:
    /// 计划 shows one block per run (A2-11).
    others: Vec<(BotRunSummary, Vec<Millis>)>,
    changes: Option<TopicChanges>,
    artifacts: Vec<ArtifactSummary>,
    /// The preview of the newest 产出.
    preview: Option<ArtifactPreview>,
    decisions: Vec<Decision>,
    cost: Option<TopicCost>,
    recording: Option<u64>,
    /// The members the cards name.
    members: HashMap<MemberId, Member>,
    /// Everyone in the 话题's 群 (成员): every member of a 群 sees its 话题.
    group_members: Vec<Member>,
}

impl Data {
    fn read(services: &Services, g: &str, t: &str) -> Self {
        let p = &services.provider;
        let topic = p.topic(g, t);
        let all_runs = p.runs(g, t);
        let run = rail_run(
            all_runs.clone(),
            topic.as_ref().and_then(|t| t.owner_id.as_deref()),
        );
        let others: Vec<(BotRunSummary, Vec<Millis>)> =
            crate::views::topic_page::working_runs(&all_runs)
                .into_iter()
                .filter(|r| !r.plan.is_empty() && run.as_ref().is_none_or(|x| x.id != r.id))
                .map(|r| (r.clone(), p.run_step_times(&r.id)))
                .collect();
        let artifacts = p.artifacts(g, t);
        let preview = artifacts
            .first()
            .and_then(|a| p.artifact_preview(&a.id, a.version));
        let decisions = p.topic_decisions(g, t);
        let mut ids: Vec<&str> = others.iter().map(|(r, _)| r.bot_id.as_str()).collect();
        if let Some(t) = &topic {
            ids.extend(t.owner_id.as_deref());
            ids.push(&t.created_by);
        }
        if let Some(r) = &run {
            ids.push(&r.bot_id);
        }
        ids.extend(decisions.iter().map(|d| d.by.as_str()));
        let members = ids
            .into_iter()
            .filter_map(|id| p.member(id).map(|m| (id.to_string(), m)))
            .collect();
        let state = topic.as_ref().map(|x| {
            crate::views::topic_page::topic_state(p.as_ref(), services.me().as_deref(), x)
        });
        Self {
            state,
            group_id: g.to_string(),
            topic_id: t.to_string(),
            step_times: run
                .as_ref()
                .map(|r| p.run_step_times(&r.id))
                .unwrap_or_default(),
            changes: p.topic_changes(g, t),
            others,
            cost: p.topic_cost(g, t),
            recording: topic_recording(services, g, t),
            topic,
            run,
            artifacts,
            preview,
            decisions,
            members,
            group_members: crate::views::topic_menu::group_members(services, g),
        }
    }

    fn member(&self, id: &str) -> Option<Member> {
        self.members.get(id).cloned()
    }

    fn name(&self, id: &str) -> String {
        self.members
            .get(id)
            .map(|m| m.name.clone())
            .unwrap_or_default()
    }
}

mod native;
pub use native::run_since;

/// The members 成员 marks, in order (design-relay §3.6): the AI members at work here (在做,
/// the newest start first), then those that took part (参与过), then the one who 提出 the
/// 话题 (提出) unless already marked.
pub fn people_marks(topic: &Topic) -> Vec<(MemberId, &'static str)> {
    use crate::copy::relay as r;
    let mut ps: Vec<&crate::data::TopicParticipant> = topic.participants.iter().collect();
    ps.sort_by_key(|p| (!p.working, std::cmp::Reverse(p.last_run_at)));
    let mut out: Vec<(MemberId, &'static str)> = ps
        .into_iter()
        .map(|p| {
            let tag = if p.working {
                r::PEOPLE_WORKING
            } else {
                r::PEOPLE_TOOK_PART
            };
            (p.member_id.clone(), tag)
        })
        .collect();
    let by = &topic.created_by;
    if !by.is_empty() && !out.iter().any(|(id, _)| id == by) {
        out.push((by.clone(), c::INSPECTOR_ASKED));
    }
    out
}

// --- v12/fx-topic: one trailing pane width (coordinator decision): the
// inspector is 280 pt, the wider panes (单独问, 产出, 改动) are 400 pt. Use the
// shell's token for the wide pane once it lands.
/// The width of the wider trailing panes: 单独问, the topic's 产出 and 改动.
pub const PANE_WIDE: Pixels = crate::ui::native::metrics::WIDE_PANE_WIDTH;
// --- end v12/fx-topic ---

/// The empty inspector (NATIVE.md §9.4 empty states): centered `secondary`
/// Body text on the inspector fill, no illustration.
fn rail_empty(cx: &App) -> AnyElement {
    use crate::ui::native::{NativeType as _, native_colors};
    let n = native_colors(cx);
    div()
        .size_full()
        .bg(n.sidebar)
        .flex()
        .items_center()
        .justify_center()
        .px(space::S16)
        .n_body()
        .text_color(n.secondary)
        .child(c::TOPIC_RAIL_EMPTY)
        .into_any_element()
}

pub struct TopicRail {
    services: Services,
    /// The inspector sections closed with their disclosure chevron.
    collapsed: std::collections::HashSet<&'static str>,
    data: Option<Rc<Data>>,
    _subscriptions: Vec<Subscription>,
}

impl TopicRail {
    pub fn new(services: Services, cx: &mut Context<Self>) -> Self {
        let subscriptions = vec![
            cx.subscribe(&services.hub, |this, _, _: &Changes, cx| {
                this.data = None;
                cx.notify();
            }),
            cx.observe(&services.nav, |_, _, cx| cx.notify()),
        ];
        Self {
            _subscriptions: subscriptions,
            services,
            collapsed: Default::default(),
            data: None,
        }
    }

    /// The rail of 话题 `t`, read again only after a change.
    fn data(&mut self, g: &str, t: &str) -> Rc<Data> {
        match &self.data {
            Some(d) if d.group_id == g && d.topic_id == t => d.clone(),
            _ => {
                let d = Rc::new(Data::read(&self.services, g, t));
                self.data = Some(d.clone());
                d
            }
        }
    }
}

impl TopicRail {
    /// The members of the 群 to hand the 话题 to, as a dropdown on `button`.
    fn assign_menu(&self, topic: &Topic, button: gpui_component::button::Button) -> AnyElement {
        use gpui_component::menu::{DropdownMenu as _, PopupMenuItem};
        let (g, t) = (topic.group_id.clone(), topic.id.clone());
        let members = crate::views::topic_menu::group_members(&self.services, &g);
        let owner = topic.owner_id.clone();
        let services = self.services.clone();
        button
            .dropdown_menu(move |mut menu, _, _| {
                for m in &members {
                    let (s, g, t, id) = (services.clone(), g.clone(), t.clone(), m.id.clone());
                    menu = menu.item(
                        PopupMenuItem::new(m.name.clone())
                            .checked(owner.as_deref() == Some(m.id.as_str()))
                            .on_click(move |_, _, _| {
                                s.provider.set_topic_owner(&g, &t, Some(id.clone()))
                            }),
                    );
                }
                menu
            })
            .into_any_element()
    }

    /// 指派: the ghost sm action of an empty 负责 (§2.13 compact).
    fn assign_button(&self, topic: &Topic, cx: &App) -> AnyElement {
        use crate::ui::native::{PushSize, PushStyle, push_button};
        self.assign_menu(
            topic,
            push_button(
                "rail-assign",
                c::ASSIGN,
                PushStyle::Ghost,
                PushSize::Small,
                cx,
            ),
        )
    }

    /// The actions of the 话题 at the top of the pane (owner decision,
    /// 2026-11-08: no pinned full-width footer): 标记完成 (outline, with a
    /// check) and 交给… (ghost, a menu of the 群's members), small, side by
    /// side. While the 话题 waits on a review, an answer or a merge,
    /// 标记完成 is not the next step (R2-17); 交给… stays.
    fn actions(&self, topic: &Topic, cx: &mut Context<Self>) -> AnyElement {
        use crate::ui::native::{BadgeState, PushSize, PushStyle, push_button};
        let (g, t) = (topic.group_id.clone(), topic.id.clone());
        let waits = matches!(
            topic.status,
            crate::data::TopicStatus::WaitingReview | crate::data::TopicStatus::WaitingAnswer
        ) || self
            .data
            .as_ref()
            .and_then(|d| d.state.as_ref())
            .is_some_and(|s| matches!(s.badge, BadgeState::NeedsYou | BadgeState::Merging));
        let done = (!waits).then(|| {
            let (s, g, t) = (self.services.clone(), g.clone(), t.clone());
            push_button(
                "rail-mark-done",
                c::MARK_DONE,
                PushStyle::Outline,
                PushSize::Small,
                cx,
            )
            .icon(gpui_component::Icon::new(crate::ui::Sym::Check))
            .on_click(move |_, _, _| {
                s.provider
                    .set_topic_status(&g, &t, crate::data::TopicStatus::Done, Vec::new())
            })
        });
        let hand = self.assign_menu(
            topic,
            push_button(
                "rail-hand-to",
                c::HAND_TO,
                PushStyle::Ghost,
                PushSize::Small,
                cx,
            ),
        );
        div()
            .w_full()
            .flex()
            .flex_wrap()
            .items_center()
            .gap(space::S8)
            .children(done)
            .child(hand)
            .into_any_element()
    }
}

impl PaneView for TopicRail {
    fn width(&self, _window_w: Pixels) -> Pixels {
        crate::ui::native::metrics::INSPECTOR_WIDTH
    }
}

/// Minutes as `41 分钟`, hours past 90 minutes as `1.5 小时`.
pub fn duration_text(ms: u64) -> String {
    let (n, unit) = duration_parts(ms);
    format!("{n} {unit}")
}

/// A duration as its mono number and its unit (`41`, `分钟`).
pub fn duration_parts(ms: u64) -> (String, &'static str) {
    let min = ms / 60_000;
    if min >= 90 {
        (format!("{:.1}", min as f32 / 60.), c::HOURS_UNIT)
    } else {
        (min.to_string(), c::MINUTES_UNIT)
    }
}

/// Cents in the currency the data has: `$0.85` for US dollars, `¥6.20`
/// for yuan. Never converted.
pub fn money_text(cents: u64, currency: Currency) -> String {
    let v = format!("{}.{:02}", cents / 100, cents % 100);
    let template = match currency {
        Currency::Usd => c::MONEY_USD,
        Currency::Cny => c::MONEY_CNY,
    };
    fill(template, &[("v", &v)])
}

/// A recording length as `0:42`.
pub fn media_time(ms: u64) -> String {
    let s = ms / 1000;
    format!("{}:{:02}", s / 60, s % 60)
}

/// How long each plan step took, from the step start times and the run's
/// end (or now while it runs). Steps with no start time get `None`.
pub fn step_minutes(run: &BotRunSummary, starts: &[Millis]) -> Vec<Option<u64>> {
    let end = run
        .ended_at
        .unwrap_or_else(|| crate::selftest::clock::utc_now().timestamp_millis());
    (0..run.plan.len())
        .map(|i| {
            let from = *starts.get(i)?;
            let to = match run.plan[i].state {
                PlanState::Done => starts.get(i + 1).copied().unwrap_or(end),
                _ => return None,
            };
            Some(((to - from).max(0) / 60_000) as u64)
        })
        .collect()
}

/// The kind of a 产出 in a meta line (`网页`).
pub fn kind_text(kind: ArtifactKind) -> &'static str {
    match kind {
        ArtifactKind::Page => c::KIND_PAGE,
        ArtifactKind::File => c::KIND_FILE,
        ArtifactKind::Image => c::KIND_IMAGE,
        ArtifactKind::Decision => c::KIND_DECISION,
    }
}

/// The diffstat squares of a change: five, green for added and red for
/// removed lines in proportion, the last one neutral.
pub fn diff_squares(additions: u32, deletions: u32, cx: &App) -> gpui::Div {
    let k = theme::colors(cx);
    let total = (additions + deletions).max(1) as f32;
    let green = if additions + deletions == 0 {
        0
    } else {
        ((additions as f32 / total) * 4.).round() as usize
    };
    let red = if additions + deletions == 0 {
        0
    } else {
        4 - green
    };
    div().flex().gap(px(2.)).children((0..5).map(move |i| {
        let color = if i < green {
            k.ok
        } else if i < green + red {
            k.bad
        } else {
            k.line_2
        };
        div().size(px(7.)).rounded(px(2.)).bg(color)
    }))
}

/// A breathing 6 pt dot in `color` (live work).
pub fn live_dot(id: impl Into<ElementId>, color: Hsla, cx: &App) -> AnyElement {
    ui::motion::breathe(
        id,
        div().flex_none().size(px(6.)).rounded(px(3.)).bg(color),
        cx,
    )
}

impl TopicRail {
    fn open_pane(&self, pane: RightPane, cx: &mut Context<Self>) {
        self.services
            .nav
            .update(cx, |nav, cx| nav.set_right_pane(pane, cx));
    }
}

/// The small real-content thumbnail of a 产出 page: its eyebrow, title, the
/// first metrics and the first rows of its table, on a lifted sheet.
pub fn preview_thumbnail(preview: &ArtifactPreview, cx: &App) -> AnyElement {
    let k = theme::colors(cx);
    let tiny = |px_size: f32, line: f32| div().text_size(px(px_size)).line_height(px(line));
    div()
        .size_full()
        .pt(space::S8)
        .px(space::S10)
        .rounded_t(px(6.))
        .border_1()
        .border_b_0()
        .border_color(k.line)
        .bg(k.bg)
        .shadow(k.lift())
        .overflow_hidden()
        .children(preview.eyebrow.clone().map(|e| {
            tiny(8., 10.)
                .figures()
                .text_color(k.ink_3)
                .one_line()
                .child(e)
        }))
        .child(
            tiny(11., 14.)
                .mt(px(2.))
                .font_weight(gpui::FontWeight::SEMIBOLD)
                .text_color(k.ink)
                .one_line()
                .child(preview.title.clone()),
        )
        .when(!preview.metrics.is_empty(), |el| {
            el.child(
                tiny(8., 10.)
                    .mt(px(4.))
                    .flex()
                    .gap(space::S10)
                    .text_color(k.ink_2)
                    .children(preview.metrics.iter().take(2).map(|m| {
                        // `6/6 场景` for a count, `最长重连 1.8 秒` for a value.
                        let count = m.unit.as_deref().is_some_and(|u| u.starts_with('/'));
                        let value = div()
                            .figures()
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .text_color(k.ink);
                        if count {
                            div()
                                .flex()
                                .gap(px(2.))
                                .child(value.child(format!(
                                    "{}{}",
                                    m.value,
                                    m.unit.as_deref().unwrap_or_default().replace(' ', "")
                                )))
                                .child(m.label.clone())
                        } else {
                            div()
                                .flex()
                                .gap(px(2.))
                                .child(m.label.clone())
                                .child(value.child(m.value.clone()))
                                .children(m.unit.clone())
                        }
                    })),
            )
        })
        .child(
            tiny(8., 12.)
                .mt(px(5.))
                .flex()
                .flex_col()
                .text_color(k.ink_2)
                .children(preview.rows.iter().take(3).map(|r| {
                    div()
                        .flex()
                        .items_center()
                        .gap(space::S4)
                        .border_t_1()
                        .border_color(k.line)
                        .child(div().flex_1().one_line().child(r.label.clone()))
                        .children(r.ok.map(|ok| {
                            div()
                                .text_color(if ok { k.ok } else { k.bad })
                                .child(if ok { "✓" } else { "✕" })
                        }))
                        .child(
                            div()
                                .w(px(16.))
                                .flex()
                                .justify_end()
                                .figures()
                                .child(r.value.clone()),
                        )
                })),
        )
        .into_any_element()
}

/// The run of a 话题 the rail shows (runs come newest first): the owner's newest run, so
/// the 负责 card and the 计划 card belong to the same member; else the newest run.
fn rail_run(runs: Vec<BotRunSummary>, owner: Option<&str>) -> Option<BotRunSummary> {
    // R2-04: while a member works, the rail shows the run the title bar and
    // the working card name (`topic_page::lead_run`), never an older one.
    if let Some(lead) = crate::views::topic_page::lead_run(&runs, owner)
        .filter(|r| owner == Some(r.bot_id.as_str()))
    {
        return Some(lead.clone());
    }
    let own = owner.and_then(|o| runs.iter().position(|r| r.bot_id == o));
    let mut runs = runs;
    match own {
        Some(i) => Some(runs.swap_remove(i)),
        None => runs.into_iter().next(),
    }
}

/// The recording proof of the 话题's result, when one reported it.
fn topic_recording(services: &Services, g: &str, t: &str) -> Option<u64> {
    let p = &services.provider;
    p.messages(g, t, 50)
        .messages
        .iter()
        .rev()
        .filter(|m| !m.id.is_empty())
        .find_map(|m| {
            p.message_proof(g, &m.id)
                .into_iter()
                .find_map(|proof| match proof {
                    Proof::Recording { duration_ms } => Some(duration_ms),
                    _ => None,
                })
        })
}

impl Render for TopicRail {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let k = ui::native::native_colors(cx);
        let Some((g, t)) = self.services.nav.read(cx).topic.clone() else {
            return rail_empty(cx);
        };
        let d = self.data(&g, &t);
        let topic = d.topic.as_ref();
        let run = d.run.as_ref();
        // Owner decision (2026-11-08), replacing COMPLETENESS §2.5's fixed
        // list: the actions, what the 话题 is doing now, 提醒, then only the
        // sections that have something (成员, 计划, 改动, 产出, 用到的决定,
        // 在哪干活, 用时和成本, in this order). The empty ones are named in
        // one quiet line at the end. Nothing in the pane scrolls sideways.
        let mut sections: Vec<AnyElement> = Vec::new();
        if let Some(t) = topic.filter(|t| t.status != crate::data::TopicStatus::Done) {
            sections.push(self.actions(t, cx));
        }
        sections.push(self.native_status(&d, cx));
        sections.push(
            crate::views::nav::notify::topic_select(&self.services, &g, &t, cx).into_any_element(),
        );
        // Usable (design-relay §3.6): who works here now and who took part, never one
        // responsible member.
        let marks = topic.map(people_marks).unwrap_or_default();
        sections.push(self.native_owner(&d, &marks, cx));
        let mut empty: Vec<&'static str> = Vec::new();
        let mut add =
            |el: Option<AnyElement>, name: &'static str, sections: &mut Vec<AnyElement>| match el {
                Some(el) => sections.push(el),
                None => empty.push(name),
            };
        let plan = run
            .filter(|r| !r.plan.is_empty())
            .map(|r| self.native_plan(&d, r, cx));
        add(plan, c::RAIL_EMPTY_NAME_PLAN, &mut sections);
        let changes = self.native_changes(&d, window, cx);
        add(changes, c::RAIL_EMPTY_NAME_CHANGES, &mut sections);
        let outputs = self.native_outputs(&d, window, cx);
        add(outputs, c::RAIL_EMPTY_NAME_OUTPUTS, &mut sections);
        let decisions = self.native_decisions(&d, cx);
        add(decisions, c::RAIL_EMPTY_NAME_DECISIONS, &mut sections);
        let place = run.and_then(|r| self.native_where(&d, r, window, cx));
        add(place, c::RAIL_EMPTY_NAME_WHERE, &mut sections);
        let cost = self.native_cost(&d, cx);
        add(cost, c::RAIL_EMPTY_NAME_COST, &mut sections);
        if !empty.is_empty() {
            sections.push(
                div()
                    .w_full()
                    .px(px(2.))
                    .n_subheadline()
                    .text_color(k.secondary)
                    .child(fill(
                        c::RAIL_EMPTY_REST,
                        &[("names", &empty.join(crate::copy::rail::RAIL_NAMES_SEP))],
                    ))
                    .into_any_element(),
            );
        }
        div()
            .size_full()
            .bg(k.content)
            .flex()
            .flex_col()
            .child(
                div()
                    .id("topic-rail")
                    .flex_1()
                    .min_h_0()
                    .min_w_0()
                    .overflow_y_scroll()
                    .overflow_x_hidden()
                    .px(space::S16)
                    .pt(space::S16)
                    .pb(space::S24)
                    .flex()
                    .flex_col()
                    .gap(space::S20)
                    .children(sections),
            )
            .into_any_element()
    }
}

/// The 改动 pane (the PR of the 话题): its summary and its files with their patches.
pub struct ChangesPane {
    services: Services,
    /// The files whose patch shows, for the 话题 in `open_for`.
    open: std::collections::HashSet<String>,
    open_for: Option<(String, String)>,
    /// The first patch was opened for this 话题 (once its diff was read).
    opened_first: bool,
    _subscriptions: Vec<Subscription>,
}

impl ChangesPane {
    pub fn new(services: Services, cx: &mut Context<Self>) -> Self {
        Self {
            _subscriptions: watch(&services, cx),
            services,
            open: Default::default(),
            open_for: None,
            opened_first: false,
        }
    }
}

impl PaneView for ChangesPane {
    fn width(&self, _window_w: Pixels) -> Pixels {
        PANE_WIDE
    }
}

impl Render for ChangesPane {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let topic = self.services.nav.read(cx).topic.clone();
        let p = &self.services.provider;
        let changes = topic.as_ref().and_then(|(g, t)| p.topic_changes(g, t));
        let diff = topic.as_ref().and_then(|(g, t)| p.topic_diff(g, t));
        if self.open_for != topic {
            self.open_for = topic;
            self.open.clear();
            self.opened_first = false;
        }
        // The first file with a patch shows open, once the diff is read.
        if !self.opened_first
            && let Some(crate::data::v2::TopicDiff::Ready(f)) = &diff
        {
            self.opened_first = true;
            if let Some(first) = f.files.iter().find(|f| f.patch.is_some()) {
                self.open.insert(first.path.clone());
            }
        }
        let this = cx.entity().downgrade();
        let toggle: native::ToggleFile = Rc::new(move |path: String, cx: &mut App| {
            let _ = this.update(cx, |pane, cx| {
                if !pane.open.remove(&path) {
                    pane.open.insert(path);
                }
                cx.notify();
            });
        });
        native::changes_pane_body(changes, diff, &self.open, toggle, cx)
    }
}

#[cfg(test)]
mod money_tests {
    use super::*;

    /// A cost shows in the currency the data has, never converted.
    #[test]
    fn money_shows_the_reported_currency() {
        assert_eq!(money_text(85, Currency::Usd), "$0.85");
        assert_eq!(money_text(1_234, Currency::Usd), "$12.34");
        assert_eq!(money_text(620, Currency::Cny), "¥6.20");
        assert_eq!(money_text(0, Currency::Usd), "$0.00");
    }
}

#[cfg(test)]
mod run_tests {
    use super::*;
    use crate::data::RunStatus;

    fn run(id: &str, bot: &str) -> BotRunSummary {
        BotRunSummary {
            hold: Default::default(),
            id: id.into(),
            bot_id: bot.into(),
            group_id: "g".into(),
            topic_id: "t".into(),
            machine_id: None,
            machine_name: None,
            status: RunStatus::Running,
            requested_by: None,
            created_at: 0,
            started_at: Some(0),
            ended_at: None,
            updated_at: 0,
            model: None,
            effort: None,
            summary: None,
            plan: Vec::new(),
        }
    }

    /// DU-1: two members work in one 话题; the rail shows the owner's run, not the newest.
    #[test]
    fn the_rail_shows_the_owners_run() {
        let runs = || {
            vec![
                run("scout", "Scout"),
                run("forge", "Forge"),
                run("old", "Forge"),
            ]
        };
        assert_eq!(rail_run(runs(), Some("Forge")).unwrap().id, "forge");
        assert_eq!(rail_run(runs(), Some("Taro")).unwrap().id, "scout");
        assert_eq!(rail_run(runs(), None).unwrap().id, "scout");
        assert!(rail_run(vec![], Some("Forge")).is_none());
    }
}

#[cfg(test)]
mod people_tests {
    use super::people_marks;
    use crate::data::{Topic, TopicParticipant, TopicStatus};

    fn who(id: &str, working: bool, at: u64) -> TopicParticipant {
        TopicParticipant {
            member_id: id.into(),
            working,
            last_run_at: at,
        }
    }

    fn topic(participants: Vec<TopicParticipant>, by: &str, owner: Option<&str>) -> Topic {
        Topic {
            id: "t".into(),
            group_id: "g".into(),
            title: "t".into(),
            status: TopicStatus::InProgress,
            waiting_on: vec![],
            owner_id: owner.map(Into::into),
            created_by: by.into(),
            created_at: 0,
            root_message_id: String::new(),
            last_seq: 0,
            last_activity_at: 0,
            message_count: 0,
            unread: 0,
            mentions: 0,
            participants,
            relay: None,
        }
    }

    #[test]
    fn who_works_comes_first_then_who_took_part_then_the_asker() {
        let t = topic(
            vec![
                who("a", false, 5),
                who("b", true, 3),
                who("c", true, 9),
                who("d", false, 7),
            ],
            "me",
            Some("a"),
        );
        let marks = people_marks(&t);
        assert_eq!(
            marks,
            vec![
                ("c".to_string(), "在做"),
                ("b".to_string(), "在做"),
                ("d".to_string(), "参与过"),
                ("a".to_string(), "参与过"),
                ("me".to_string(), "提出"),
            ]
        );
        // The routing owner is never marked 负责.
        assert!(marks.iter().all(|(_, tag)| *tag != "负责"));
    }

    #[test]
    fn a_topic_nobody_worked_on_marks_only_the_asker() {
        let t = topic(vec![], "me", Some("a"));
        assert_eq!(people_marks(&t), vec![("me".to_string(), "提出")]);
        assert!(people_marks(&topic(vec![], "", None)).is_empty());
    }
}
