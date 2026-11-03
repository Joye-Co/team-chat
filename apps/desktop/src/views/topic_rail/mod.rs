//! The topic rail beside a 话题 page (Topic.dc.html, CONCEPT2 §3): cards 负责,
//! 计划, 改动, 产出, 用到的决定, 电脑 and 用时和成本 on `rail`, each hidden
//! when it has no data. Also the 改动 pane.
//!
//! The rail shows status only: the merge decision is the pinned card above
//! the composer, never a second button here.

use std::{collections::HashMap, rc::Rc};

use gpui::{
    AnyElement, App, ClickEvent, Context, ElementId, Hsla, InteractiveElement as _, IntoElement,
    ParentElement, Pixels, Render, SharedString, StatefulInteractiveElement as _, Styled,
    Subscription, Window, div, prelude::FluentBuilder as _, px,
};

use crate::{
    app::{Changes, RightPane, Services},
    copy::{fill, topic_rail as c},
    data::{
        ArtifactKind, ArtifactSummary, BotRunSummary, Member, MemberId, Millis, PlanState, Topic,
        v2::{ArtifactPreview, ChangeState, Decision, Proof, TopicChanges, TopicCost},
    },
    theme::{self, Type as _, space},
    ui::{self, Lucide},
    views::shell::{PaneView, clock, panes::watch, stub_body},
};

/// What the rail of one 话题 shows, read from the provider once per change
/// or 话题 switch. A breathing ring renders the rail on every frame it asks
/// for, so render reads only this.
struct Data {
    group_id: String,
    topic_id: String,
    topic: Option<Topic>,
    /// The newest run.
    run: Option<BotRunSummary>,
    step_times: Vec<Millis>,
    changes: Option<TopicChanges>,
    artifacts: Vec<ArtifactSummary>,
    /// The preview of the newest 产出.
    preview: Option<ArtifactPreview>,
    decisions: Vec<Decision>,
    cost: Option<TopicCost>,
    recording: Option<u64>,
    /// The members the cards name.
    members: HashMap<MemberId, Member>,
}

impl Data {
    fn read(services: &Services, g: &str, t: &str) -> Self {
        let p = &services.provider;
        let topic = p.topic(g, t);
        let run = p.runs(g, t).into_iter().next();
        let artifacts = p.artifacts(g, t);
        let preview = artifacts
            .first()
            .and_then(|a| p.artifact_preview(&a.id, a.version));
        let decisions = p.topic_decisions(g, t);
        let mut ids: Vec<&str> = Vec::new();
        if let Some(t) = &topic {
            ids.extend(t.owner_id.as_deref());
            ids.push(&t.created_by);
        }
        if let Some(r) = &run {
            ids.push(&r.bot_id);
        }
        ids.extend(decisions.first().map(|d| d.by.as_str()));
        let members = ids
            .into_iter()
            .filter_map(|id| p.member(id).map(|m| (id.to_string(), m)))
            .collect();
        Self {
            group_id: g.to_string(),
            topic_id: t.to_string(),
            step_times: run.as_ref().map(|r| p.run_step_times(&r.id)).unwrap_or_default(),
            changes: p.topic_changes(g, t),
            cost: p.topic_cost(g, t),
            recording: topic_recording(services, g, t),
            topic,
            run,
            artifacts,
            preview,
            decisions,
            members,
        }
    }

    fn member(&self, id: &str) -> Option<Member> {
        self.members.get(id).cloned()
    }

    fn name(&self, id: &str) -> String {
        self.members.get(id).map(|m| m.name.clone()).unwrap_or_default()
    }
}

pub struct TopicRail {
    services: Services,
    /// The finished plan is opened to its steps.
    plan_open: bool,
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
            plan_open: false,
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

impl PaneView for TopicRail {
    fn width(&self, _window_w: Pixels) -> Pixels {
        theme::TOPIC_RAIL_WIDTH
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

/// Cents as `¥6.20`.
pub fn money_text(cents: u64) -> String {
    format!("¥{}.{:02}", cents / 100, cents % 100)
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
        .unwrap_or_else(|| chrono::Utc::now().timestamp_millis());
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

/// The label row of a rail card: 12/500 `ink-3`, 20 pt tall, with an
/// optional action at the right.
fn label_row(label: &'static str, action: Option<AnyElement>, cx: &App) -> gpui::Div {
    let k = theme::colors(cx);
    div()
        .h(px(20.))
        .flex()
        .items_center()
        .child(
            div()
                .flex_1()
                .t_meta_medium()
                .line_height(px(20.))
                .text_color(k.ink_3)
                .child(label),
        )
        .children(action)
}

/// A quiet text action at the right of a label row (`看改动`, `打开`).
fn label_action(
    id: &'static str,
    label: impl Into<SharedString>,
    cx: &App,
) -> gpui::Stateful<gpui::Div> {
    let k = theme::colors(cx);
    div()
        .id(id)
        .flex_none()
        .mr(px(-6.))
        .h(px(22.))
        .px(space::S6)
        .rounded(theme::radius::SM)
        .flex()
        .items_center()
        .gap(px(2.))
        .t_meta()
        .text_color(k.ink_2)
        .cursor_pointer()
        .hover(|s| s.bg(k.hover))
        .child(label.into())
}

fn check(size: Pixels, color: Hsla) -> AnyElement {
    ui::icon(Lucide::Check, size, color).into_any_element()
}

impl TopicRail {
    fn open_pane(&self, pane: RightPane, cx: &mut Context<Self>) {
        self.services
            .nav
            .update(cx, |nav, cx| nav.set_right_pane(pane, cx));
    }

    fn owner_card(
        &self,
        d: &Data,
        owner: &str,
        run: Option<&BotRunSummary>,
        by: Option<(String, Millis)>,
        cx: &App,
    ) -> AnyElement {
        let k = theme::colors(cx);
        let member = d.member(owner);
        let live = run.is_some_and(|r| r.status.is_active());
        let state: Option<AnyElement> = match run {
            Some(r) if r.status.is_active() => {
                let done = r.plan.iter().filter(|i| i.state == PlanState::Done).count();
                Some(
                    div()
                        .flex()
                        .items_center()
                        .gap(space::S6)
                        .t_meta()
                        .text_color(k.ink_2)
                        .child(live_dot("rail-owner-live", k.live, cx))
                        .child(fill(
                            c::OWNER_WORKING,
                            &[
                                ("n", &(done + 1).min(r.plan.len()).to_string()),
                                ("total", &r.plan.len().to_string()),
                            ],
                        ))
                        .into_any_element(),
                )
            }
            Some(r) if r.ended_at.is_some() => Some(
                div()
                    .flex()
                    .items_center()
                    .gap(space::S4)
                    .t_meta()
                    .text_color(k.ink_2)
                    .child(check(px(12.), k.ok))
                    .child(c::DONE_MARK)
                    .child("·")
                    .child(
                        div()
                            .mono()
                            .child(r.ended_at.map(clock).unwrap_or_default()),
                    )
                    .into_any_element(),
            ),
            _ => None,
        };
        let asker = by.map(|(id, at)| {
            let m = d.member(&id);
            div()
                .flex()
                .items_center()
                .gap(space::S8)
                .t_meta()
                .text_color(k.ink_3)
                .child(
                    div()
                        .flex_none()
                        .w(px(24.))
                        .flex()
                        .justify_center()
                        .child(ui::MemberAvatar::new(m.as_ref(), px(16.))),
                )
                .child(
                    div()
                        .flex()
                        .gap(space::S4)
                        .child(fill(
                            c::ASKED_BY_NAME,
                            &[("name", &d.name(&id))],
                        ))
                        .child(div().mono().child(clock(at))),
                )
        });
        div()
            .child(label_row(c::TOPIC_RAIL_OWNER, None, cx))
            .child(
                div()
                    .mt(space::S8)
                    .flex()
                    .items_center()
                    .gap(space::S8)
                    .child(
                        ui::MemberAvatar::new(member.as_ref(), px(24.))
                            .ring(live.then_some(ui::Ring::Live)),
                    )
                    .child(
                        div()
                            .min_w_0()
                            .one_line()
                            .t_small()
                            .medium()
                            .text_color(k.ink)
                            .child(member.as_ref().map(|m| m.name.clone()).unwrap_or_default()),
                    )
                    .children(
                        member
                            .as_ref()
                            .and_then(|m| m.role.clone())
                            .filter(|r| !r.trim().is_empty())
                            .map(ui::JobTag::new),
                    )
                    .child(div().flex_1())
                    .children(state),
            )
            .children(asker.map(|a| a.mt(space::S8)))
            .into_any_element()
    }

    fn plan_card(&self, d: &Data, run: &BotRunSummary, cx: &mut Context<Self>) -> AnyElement {
        let k = theme::colors(cx);
        let starts = &d.step_times;
        let minutes = step_minutes(run, &starts);
        let all_done = run.plan.iter().all(|i| i.state == PlanState::Done);
        let open = !all_done || self.plan_open;
        let total = match (run.started_at, run.ended_at) {
            (Some(a), Some(b)) => Some(((b - a).max(0)) as u64),
            _ => None,
        };
        let toggle = all_done.then(|| {
            label_action(
                "rail-plan-toggle",
                if self.plan_open {
                    c::PLAN_COLLAPSE
                } else {
                    c::PLAN_EXPAND
                },
                cx,
            )
            .child(ui::icon(
                if self.plan_open {
                    Lucide::ChevronUp
                } else {
                    Lucide::ChevronDown
                },
                px(12.),
                k.ink_2,
            ))
            .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                this.plan_open = !this.plan_open;
                cx.notify();
            }))
            .into_any_element()
        });
        // The segments: one per step, as wide as the step took.
        let weights: Vec<f32> = minutes
            .iter()
            .map(|m| m.map(|m| m.max(1) as f32).unwrap_or(4.))
            .collect();
        let segments = div().mt(space::S8).flex().gap(px(3.)).children(
            run.plan
                .iter()
                .zip(weights)
                .enumerate()
                .map(|(i, (item, w))| {
                    let color = match item.state {
                        PlanState::Done => k.ink_3,
                        PlanState::Current => k.live,
                        PlanState::Waiting => k.you,
                        PlanState::Pending => k.line_2,
                    };
                    let mut bar = div().flex_basis(px(0.)).h(px(3.)).rounded(px(2.)).bg(color);
                    bar.style().flex_grow = Some(w);
                    if item.state == PlanState::Current {
                        ui::motion::breathe(
                            ElementId::Name(format!("rail-seg-{i}").into()),
                            bar,
                            cx,
                        )
                    } else {
                        bar.into_any_element()
                    }
                }),
        );
        let summary = all_done.then(|| {
            let (n, unit) = total.map(duration_parts).unwrap_or_default();
            div()
                .mt(space::S8)
                .flex()
                .items_center()
                .gap(space::S8)
                .t_small()
                .text_color(k.ink)
                .child(check(px(14.), k.ok))
                .child(fill(
                    c::PLAN_ALL_DONE,
                    &[("n", &run.plan.len().to_string())],
                ))
                .child(div().flex_1())
                .when(total.is_some(), |el| {
                    el.child(
                        div()
                            .t_mono()
                            .text_color(k.ink_3)
                            .child(format!("{n} {unit}")),
                    )
                })
        });
        let steps = open.then(|| {
            div()
                .mt(space::S10)
                .when(all_done, |el| {
                    el.pt(space::S8).border_t_1().border_color(k.line)
                })
                .when(!all_done, |el| el.mt(space::S8))
                .flex()
                .flex_col()
                .children(run.plan.iter().enumerate().map(|(i, item)| {
                    let glyph: AnyElement = match item.state {
                        PlanState::Done => check(px(13.), k.ink_3),
                        PlanState::Current => {
                            live_dot(ElementId::Name(format!("rail-step-{i}").into()), k.live, cx)
                        }
                        PlanState::Waiting => div()
                            .size(px(6.))
                            .rounded(px(3.))
                            .bg(k.you)
                            .into_any_element(),
                        PlanState::Pending => div()
                            .size(px(7.))
                            .rounded(px(4.))
                            .border(px(1.5))
                            .border_color(k.ink_3)
                            .into_any_element(),
                    };
                    let color = match item.state {
                        PlanState::Done | PlanState::Pending => k.ink_3,
                        _ => k.ink,
                    };
                    let row = div()
                        .h(theme::size::PLAN_ROW)
                        .flex()
                        .items_center()
                        .gap(space::S8)
                        .t_small()
                        .child(
                            div()
                                .flex_none()
                                .w(px(14.))
                                .flex()
                                .justify_center()
                                .child(glyph),
                        )
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .one_line()
                                .text_color(color)
                                .child(item.text.clone()),
                        )
                        .children(minutes.get(i).copied().flatten().map(|m| {
                            div()
                                .flex_none()
                                .t_mono()
                                .text_color(k.ink_3)
                                .child(fill(c::STEP_MINUTES, &[("n", &m.to_string())]))
                        }));
                    ui::motion::rise(
                        ElementId::Name(format!("rail-step-rise-{}-{i}", run.id).into()),
                        i,
                        row,
                        cx,
                    )
                }))
        });
        div()
            .child(label_row(c::TOPIC_RAIL_PLAN, toggle, cx))
            .children(summary)
            .child(segments)
            .children(steps)
            .into_any_element()
    }

    fn changes_card(&self, d: &Data, cx: &mut Context<Self>) -> Option<AnyElement> {
        let k = theme::colors(cx);
        let ch = d.changes.clone()?;
        let action = label_action("rail-see-changes", c::SEE_CHANGES, cx)
            .on_click(
                cx.listener(|this, _: &ClickEvent, _, cx| this.open_pane(RightPane::Changes, cx)),
            )
            .into_any_element();
        let status: AnyElement = match ch.state {
            ChangeState::Merging => div()
                .flex()
                .items_center()
                .gap(space::S6)
                .h(px(20.))
                .px(px(7.))
                .rounded(theme::TAG_RADIUS)
                .bg(k.sunken)
                .t_meta()
                .text_color(k.live_ink)
                .child(live_dot("rail-merging", k.live, cx))
                .child(c::CHANGES_MERGING)
                .into_any_element(),
            state => div()
                .h(px(20.))
                .px(px(7.))
                .rounded(theme::TAG_RADIUS)
                .bg(k.sunken)
                .flex()
                .items_center()
                .t_meta()
                .text_color(k.ink_2)
                .child(match state {
                    ChangeState::Merged => c::CHANGES_MERGED,
                    ChangeState::Closed => c::CHANGES_CLOSED,
                    _ => c::CHANGES_OPEN,
                })
                .into_any_element(),
        };
        let checks_ok = ch.checks_total > 0 && ch.checks_passed == ch.checks_total;
        Some(
            div()
                .child(label_row(c::TOPIC_RAIL_CHANGES, Some(action), cx))
                .child(
                    div()
                        .mt(space::S8)
                        .one_line()
                        .t_small()
                        .medium()
                        .text_color(k.ink)
                        .child(ch.title.clone()),
                )
                .child(
                    div()
                        .mt(space::S2)
                        .flex()
                        .items_center()
                        .gap(space::S8)
                        .t_meta()
                        .line_height(px(18.))
                        .text_color(k.ink_3)
                        .child(div().mono().child(format!("#{}", ch.number)))
                        .child(
                            div()
                                .mono()
                                .flex()
                                .gap(space::S4)
                                .child(
                                    div()
                                        .text_color(k.ok_ink)
                                        .child(format!("+{}", ch.additions)),
                                )
                                .child(
                                    div()
                                        .text_color(k.bad_ink)
                                        .child(format!("−{}", ch.deletions)),
                                ),
                        )
                        .child(diff_squares(ch.additions, ch.deletions, cx))
                        .child(div().flex_1())
                        .child(
                            div()
                                .flex()
                                .gap(space::S4)
                                .child(div().mono().child(ch.files.to_string()))
                                .child(c::FILES_UNIT),
                        ),
                )
                .when(ch.checks_total > 0, |el| {
                    el.child(
                        div()
                            .mt(space::S10)
                            .pt(space::S10)
                            .border_t_1()
                            .border_color(k.line)
                            .flex()
                            .items_center()
                            .gap(space::S6)
                            .t_meta()
                            .line_height(px(18.))
                            .text_color(k.ink_2)
                            .child(if checks_ok {
                                check(px(12.), k.ok)
                            } else {
                                ui::icon(Lucide::CircleAlert, px(12.), k.bad).into_any_element()
                            })
                            .child(
                                div()
                                    .flex()
                                    .gap(space::S4)
                                    .child(
                                        div().mono().child(format!(
                                            "{}/{}",
                                            ch.checks_passed, ch.checks_total
                                        )),
                                    )
                                    .child(c::CHECKS_UNIT),
                            )
                            .child(div().flex_1())
                            .child(status),
                    )
                })
                .into_any_element(),
        )
    }

    fn outputs_card(&self, d: &Data, cx: &mut Context<Self>) -> Option<AnyElement> {
        let k = theme::colors(cx);
        let recording = d.recording;
        let outputs = &d.artifacts;
        let first = outputs.first()?.clone();
        let pane = RightPane::Artifact {
            id: first.id.clone(),
            version: None,
        };
        let action = label_action("rail-open-output", c::OPEN, cx)
            .on_click(cx.listener({
                let pane = pane.clone();
                move |this, _: &ClickEvent, _, cx| this.open_pane(pane.clone(), cx)
            }))
            .into_any_element();
        let thumb = d.preview.clone().map(|preview| {
            div()
                .id("rail-output-thumb")
                .mt(space::S8)
                .w_full()
                .h(px(80.))
                .pt(space::S8)
                .px(space::S12)
                .rounded(theme::radius::MD)
                .bg(k.sunken)
                .overflow_hidden()
                .cursor_pointer()
                .child(preview_thumbnail(&preview, cx))
                .on_click(cx.listener({
                    let pane = pane.clone();
                    move |this, _: &ClickEvent, _, cx| this.open_pane(pane.clone(), cx)
                }))
        });
        let rows = outputs.iter().map(|a| {
            div()
                .mt(space::S8)
                .flex()
                .items_center()
                .gap(space::S8)
                .t_small()
                .child(
                    div()
                        .min_w_0()
                        .one_line()
                        .medium()
                        .text_color(k.ink)
                        .child(a.title.clone()),
                )
                .child(div().flex_1())
                .child(div().flex_none().t_mono().text_color(k.ink_3).child(fill(
                    c::OUTPUT_VERSION,
                    &[
                        ("v", &a.version.to_string()),
                        ("time", &clock(a.updated_at)),
                    ],
                )))
        });
        let rec = recording.map(|ms| {
            div()
                .mt(space::S4)
                .mx(px(-6.))
                .mb(px(-4.))
                .h(px(32.))
                .px(space::S6)
                .rounded(theme::radius::MD)
                .flex()
                .items_center()
                .gap(space::S10)
                .t_small()
                .text_color(k.ink)
                .child(
                    div()
                        .flex_none()
                        .w(px(36.))
                        .h(px(24.))
                        .rounded(px(5.))
                        .bg(k.screen)
                        .flex()
                        .items_center()
                        .justify_center()
                        .when(theme::is_dark(cx), |el| {
                            el.border_1().border_color(k.line_2)
                        })
                        .child(ui::icon(Lucide::Play, px(10.), k.screen_ink)),
                )
                .child(c::RECORDING_ROW)
                .child(div().flex_1())
                .child(div().t_mono().text_color(k.ink_3).child(media_time(ms)))
        });
        Some(
            div()
                .child(label_row(c::TOPIC_RAIL_OUTPUTS, Some(action), cx))
                .children(thumb)
                .children(rows)
                .children(rec)
                .into_any_element(),
        )
    }

    fn decisions_card(&self, d: &Data, cx: &App) -> Option<AnyElement> {
        let k = theme::colors(cx);
        let decisions = &d.decisions;
        let first = decisions.first()?;
        let by = d.member(&first.by);
        let who = div()
            .flex()
            .items_center()
            .gap(space::S6)
            .t_meta()
            .text_color(k.ink_3)
            .child(ui::MemberAvatar::new(by.as_ref(), px(16.)))
            .child(by.as_ref().map(|m| m.name.clone()).unwrap_or_default())
            .child(div().mono().child(clock(first.at)))
            .into_any_element();
        Some(
            div()
                .child(label_row(c::TOPIC_RAIL_DECISIONS, Some(who), cx))
                .children(decisions.iter().map(|d| {
                    div()
                        .mt(space::S8)
                        .flex()
                        .items_start()
                        .gap(space::S8)
                        .t_small()
                        .text_color(k.ink)
                        .child(div().flex_none().pt(px(3.)).child(check(px(14.), k.ink_3)))
                        .child(div().flex_1().min_w_0().child(d.text.clone()))
                }))
                .into_any_element(),
        )
    }

    fn machine_card(
        &self,
        d: &Data,
        run: &BotRunSummary,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let k = theme::colors(cx);
        let name = run.machine_name.clone()?;
        let busy = run.status.is_active();
        let who = d.name(&run.bot_id);
        let line = if busy {
            fill(c::MACHINE_USING, &[("name", &who)])
        } else {
            fill(
                c::MACHINE_DONE,
                &[
                    ("name", &who),
                    ("time", &run.ended_at.map(clock).unwrap_or_default()),
                ],
            )
        };
        let state: AnyElement = if busy {
            div()
                .flex()
                .items_center()
                .gap(space::S6)
                .t_meta()
                .text_color(k.live_ink)
                .child(live_dot("rail-machine-live", k.live, cx))
                .child(c::MACHINE_BUSY)
                .into_any_element()
        } else {
            div()
                .flex()
                .items_center()
                .gap(space::S6)
                .t_meta()
                .text_color(k.ink_2)
                .child(
                    div()
                        .size(px(7.))
                        .rounded(px(4.))
                        .border(px(1.5))
                        .border_color(k.ink_3),
                )
                .child(c::MACHINE_IDLE)
                .into_any_element()
        };
        let see = busy.then(|| {
            let run_id = run.id.clone();
            div()
                .id("rail-see-machine")
                .mt(space::S8)
                .h(px(28.))
                .rounded(theme::radius::MD)
                .border_1()
                .border_color(k.line_2)
                .shadow(k.sec_hi())
                .flex()
                .items_center()
                .justify_center()
                .gap(space::S6)
                .t_small()
                .text_color(k.ink)
                .cursor_pointer()
                .hover(|s| s.bg(k.hover))
                .child(ui::icon(Lucide::Monitor, px(14.), k.ink_2))
                .child(fill(c::SEE_MACHINE, &[("name", &who)]))
                .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                    this.open_pane(RightPane::Live(run_id.clone()), cx)
                }))
        });
        Some(
            div()
                .child(label_row(c::TOPIC_RAIL_MACHINE, None, cx))
                .child(
                    div()
                        .mt(space::S8)
                        .flex()
                        .items_center()
                        .gap(space::S10)
                        .child(
                            div()
                                .flex_none()
                                .size(px(28.))
                                .rounded(theme::radius::MD)
                                .bg(k.sunken)
                                .flex()
                                .items_center()
                                .justify_center()
                                .child(ui::icon(Lucide::Monitor, px(16.), k.ink_2)),
                        )
                        .child(
                            div()
                                .min_w_0()
                                .flex()
                                .flex_col()
                                .child(
                                    div()
                                        .t_small()
                                        .medium()
                                        .text_color(k.ink)
                                        .one_line()
                                        .child(name),
                                )
                                .child(
                                    div()
                                        .t_meta()
                                        .line_height(px(18.))
                                        .text_color(k.ink_3)
                                        .one_line()
                                        .child(line),
                                ),
                        )
                        .child(div().flex_1())
                        .child(state),
                )
                .children(see)
                .into_any_element(),
        )
    }

    fn cost_card(&self, d: &Data, cx: &App) -> Option<AnyElement> {
        let k = theme::colors(cx);
        let cost = d.cost.clone()?;
        if cost.duration_ms == 0 && cost.cost_cents.is_none() {
            return None;
        }
        let (n, unit) = duration_parts(cost.duration_ms);
        let value = |s: String| div().mono().medium().text_color(k.ink).child(s);
        let range = match (cost.started_at, cost.ended_at) {
            (Some(a), Some(b)) => Some(format!("{}–{}", clock(a), clock(b))),
            _ => None,
        };
        Some(
            div()
                .child(label_row(c::TOPIC_RAIL_COST, None, cx))
                .child(
                    div()
                        .mt(space::S8)
                        .flex()
                        .items_baseline()
                        .gap(space::S6)
                        .t_small()
                        .text_color(k.ink_2)
                        .child(c::COST_LABEL)
                        .child(value(n))
                        .child(unit)
                        .when_some(cost.cost_cents, |el, cents| {
                            el.child(div().text_color(k.ink_3).child("·"))
                                .child(value(money_text(cents)))
                        })
                        .child(div().flex_1())
                        .when_some(range, |el, r| {
                            el.child(
                                div()
                                    .flex()
                                    .gap(space::S4)
                                    .t_meta()
                                    .text_color(k.ink_3)
                                    .child(c::TODAY)
                                    .child(div().mono().child(r)),
                            )
                        }),
                )
                .into_any_element(),
        )
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
                .font_family(theme::MONO_FONT_FAMILY)
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
                            .font_family(theme::MONO_FONT_FAMILY)
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
                                .font_family(theme::MONO_FONT_FAMILY)
                                .child(r.value.clone()),
                        )
                })),
        )
        .into_any_element()
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
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let k = theme::colors(cx);
        let Some((g, t)) = self.services.nav.read(cx).topic.clone() else {
            return stub_body(Lucide::PanelRight, c::TOPIC_RAIL_EMPTY, None, cx).into_any_element();
        };
        let d = self.data(&g, &t);
        let topic = d.topic.as_ref();
        let run = d.run.as_ref();
        let mut cards: Vec<AnyElement> = Vec::new();
        // 负责
        if let Some(owner) = topic.and_then(|t| t.owner_id.clone()) {
            let by = topic
                .filter(|x| x.created_by != owner)
                .map(|x| (x.created_by.clone(), x.created_at));
            cards.push(self.owner_card(&d, &owner, run, by, cx));
        }
        // 计划
        if let Some(r) = run.filter(|r| !r.plan.is_empty()) {
            cards.push(self.plan_card(&d, r, cx));
        }
        // 改动
        cards.extend(self.changes_card(&d, cx));
        // 产出
        cards.extend(self.outputs_card(&d, cx));
        // 用到的决定
        cards.extend(self.decisions_card(&d, cx));
        // 电脑
        if let Some(r) = run {
            cards.extend(self.machine_card(&d, r, cx));
        }
        // 用时和成本
        cards.extend(self.cost_card(&d, cx));
        if cards.is_empty() {
            return stub_body(Lucide::PanelRight, c::TOPIC_RAIL_EMPTY, None, cx).into_any_element();
        }
        // One id per place, not per 话题: the rail rises on its first paint
        // only, never again on a 话题 switch (§12).
        let cards = cards.into_iter().enumerate().map(|(i, el)| {
            ui::motion::rise(
                SharedString::from(format!("topic-rail-{i}")),
                i + 1,
                ui::card(cx).flex_none().p(space::S12).child(el),
                cx,
            )
        });
        div()
            .id("topic-rail")
            .size_full()
            .overflow_y_scroll()
            .bg(k.rail)
            .pt(space::S8)
            .px(space::S16)
            .pb(space::S16)
            .flex()
            .flex_col()
            .gap(space::S10)
            .children(cards)
            .into_any_element()
    }
}

/// The 改动 pane (the PR of the 话题). Foundation stub.
pub struct ChangesPane {
    services: Services,
    _subscriptions: Vec<Subscription>,
}

impl ChangesPane {
    pub fn new(services: Services, cx: &mut Context<Self>) -> Self {
        Self {
            _subscriptions: watch(&services, cx),
            services,
        }
    }
}

impl PaneView for ChangesPane {
    fn width(&self, _window_w: Pixels) -> Pixels {
        theme::ARTIFACT_PANE_WIDTH
    }
}

impl Render for ChangesPane {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let changes = self
            .services
            .nav
            .read(cx)
            .topic
            .clone()
            .and_then(|(g, t)| self.services.provider.topic_changes(&g, &t));
        let line = changes.map(|ch| {
            format!(
                "#{} {} · +{} −{}",
                ch.number, ch.title, ch.additions, ch.deletions
            )
        });
        stub_body(Lucide::GitPullRequest, c::TITLE_CHANGES, line, cx)
    }
}
