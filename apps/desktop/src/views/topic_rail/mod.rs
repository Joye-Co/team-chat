//! The topic rail beside a 话题 page (Topic.dc.html, CONCEPT2 §3): cards 负责,
//! 计划, 改动, 产出, 用到的决定, 电脑 and 用时和成本 on `rail`, each hidden
//! when it has no data. Also the 改动 pane.
//!
//! Foundation stub: every card reads its data and shows it plainly; the
//! topic rail track builds the artboard cards (checklist glyphs, the 产出
//! thumbnail, checks row, collapse of a finished plan).

use gpui::{
    AnyElement, App, Context, InteractiveElement as _, IntoElement, ParentElement, Pixels, Render,
    SharedString, StatefulInteractiveElement as _, Styled, Subscription, Window, div, px,
};

use crate::{
    app::Services,
    copy::{fill, topic_rail as c},
    data::{PlanState, v2::ChangeState},
    theme::{self, Type as _, space},
    ui::{self, Lucide, Ring},
    views::shell::{
        PaneView, clock, identity, member_name, panes::watch, section_label, stub_body,
    },
};

pub struct TopicRail {
    services: Services,
    _subscriptions: Vec<Subscription>,
}

impl TopicRail {
    pub fn new(services: Services, cx: &mut Context<Self>) -> Self {
        Self {
            _subscriptions: watch(&services, cx),
            services,
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
    let min = ms / 60_000;
    if min >= 90 {
        fill(c::HOURS, &[("n", &format!("{:.1}", min as f32 / 60.))])
    } else {
        fill(c::MINUTES, &[("n", &min.to_string())])
    }
}

/// Cents as `¥6.20`.
pub fn money_text(cents: u64) -> String {
    format!("¥{}.{:02}", cents / 100, cents % 100)
}

fn card(id: &str, ix: usize, label: &'static str, cx: &App) -> gpui::Div {
    let _ = (id, ix);
    ui::rail_card(cx).child(section_label(label, None, cx))
}

fn line(text: String, cx: &App) -> gpui::Div {
    let k = theme::colors(cx);
    div().t_small().text_color(k.ink).child(text)
}

fn mono(text: String, cx: &App) -> gpui::Div {
    let k = theme::colors(cx);
    div().t_mono().text_color(k.ink_3).child(text)
}

impl Render for TopicRail {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let k = theme::colors(cx);
        let Some((g, t)) = self.services.nav.read(cx).topic.clone() else {
            return stub_body(Lucide::PanelRight, c::TOPIC_RAIL_EMPTY, None, cx).into_any_element();
        };
        let p = self.services.provider.clone();
        let topic = p.topic(&g, &t);
        let runs = p.runs(&g, &t);
        let run = runs.first();
        let mut cards: Vec<AnyElement> = Vec::new();
        // 负责
        if let Some(owner) = topic.as_ref().and_then(|t| t.owner_id.clone()) {
            let live = run.is_some_and(|r| r.status.is_active());
            let state = match run {
                Some(r) if r.status.is_active() => {
                    let done = r.plan.iter().filter(|i| i.state == PlanState::Done).count();
                    fill(
                        c::OWNER_WORKING,
                        &[
                            ("n", &(done + 1).min(r.plan.len()).to_string()),
                            ("total", &r.plan.len().to_string()),
                        ],
                    )
                }
                Some(r) => fill(
                    c::OWNER_DONE,
                    &[("time", &r.ended_at.map(clock).unwrap_or_default())],
                ),
                None => String::new(),
            };
            let by = topic.as_ref().map(|t| {
                fill(
                    c::ASKED_BY,
                    &[
                        ("name", &member_name(&self.services, &t.created_by)),
                        ("time", &clock(t.created_at)),
                    ],
                )
            });
            cards.push(
                card("owner", 0, c::TOPIC_RAIL_OWNER, cx)
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(space::S8)
                            .child(identity(
                                &self.services,
                                &owner,
                                live.then_some(Ring::Live),
                                cx,
                            ))
                            .child(div().flex_1())
                            .child(mono(state, cx)),
                    )
                    .children(by.map(|b| div().t_meta().text_color(k.ink_3).child(b)))
                    .into_any_element(),
            );
        }
        // 计划
        if let Some(r) = run.filter(|r| !r.plan.is_empty()) {
            cards.push(
                card("plan", 1, c::TOPIC_RAIL_PLAN, cx)
                    .children(r.plan.iter().map(|i| {
                        let (glyph, color) = match i.state {
                            PlanState::Done => ("✓", k.ink_3),
                            PlanState::Current => ("●", k.live),
                            PlanState::Waiting => ("●", k.you),
                            PlanState::Pending => ("○", k.ink_3),
                        };
                        div()
                            .flex()
                            .gap(space::S8)
                            .t_small()
                            .child(div().w(px(12.)).text_color(color).child(glyph))
                            .child(div().text_color(k.ink).child(i.text.clone()))
                    }))
                    .into_any_element(),
            );
        }
        // 改动
        if let Some(ch) = p.topic_changes(&g, &t) {
            let state = match ch.state {
                ChangeState::Open => c::CHANGES_OPEN,
                ChangeState::Merging => c::CHANGES_MERGING,
                ChangeState::Merged => c::CHANGES_MERGED,
                ChangeState::Closed => c::CHANGES_CLOSED,
            };
            cards.push(
                card("changes", 2, c::TOPIC_RAIL_CHANGES, cx)
                    .child(line(ch.title.clone(), cx))
                    .child(mono(
                        fill(
                            c::CHANGES_LINE,
                            &[
                                ("n", &ch.number.to_string()),
                                ("add", &ch.additions.to_string()),
                                ("del", &ch.deletions.to_string()),
                                ("files", &ch.files.to_string()),
                            ],
                        ),
                        cx,
                    ))
                    .child(mono(
                        fill(
                            c::CHECKS_LINE,
                            &[
                                ("passed", &ch.checks_passed.to_string()),
                                ("total", &ch.checks_total.to_string()),
                                ("state", state),
                            ],
                        ),
                        cx,
                    ))
                    .into_any_element(),
            );
        }
        // 产出
        let outputs = p.artifacts(&g, &t);
        if !outputs.is_empty() {
            cards.push(
                card("outputs", 3, c::TOPIC_RAIL_OUTPUTS, cx)
                    .children(outputs.iter().map(|a| {
                        div()
                            .flex()
                            .justify_between()
                            .child(line(a.title.clone(), cx))
                            .child(mono(
                                fill(
                                    c::OUTPUT_VERSION,
                                    &[
                                        ("v", &a.version.to_string()),
                                        ("time", &clock(a.updated_at)),
                                    ],
                                ),
                                cx,
                            ))
                    }))
                    .into_any_element(),
            );
        }
        // 用到的决定
        let decisions = p.topic_decisions(&g, &t);
        if !decisions.is_empty() {
            cards.push(
                card("decisions", 4, c::TOPIC_RAIL_DECISIONS, cx)
                    .children(decisions.iter().map(|d| {
                        line(
                            fill(
                                c::DECISION_LINE,
                                &[
                                    ("text", &d.text),
                                    ("name", &member_name(&self.services, &d.by)),
                                    ("time", &clock(d.at)),
                                ],
                            ),
                            cx,
                        )
                    }))
                    .into_any_element(),
            );
        }
        // 电脑
        if let Some(name) = run.and_then(|r| r.machine_name.clone()) {
            let busy = run.is_some_and(|r| r.status.is_active());
            cards.push(
                card("machine", 5, c::TOPIC_RAIL_MACHINE, cx)
                    .child(
                        div()
                            .flex()
                            .justify_between()
                            .child(line(name, cx))
                            .child(mono(
                                if busy {
                                    c::MACHINE_BUSY
                                } else {
                                    c::MACHINE_IDLE
                                }
                                .into(),
                                cx,
                            )),
                    )
                    .into_any_element(),
            );
        }
        // 用时和成本
        if let Some(cost) = p.topic_cost(&g, &t) {
            let mut text = fill(c::COST_TIME, &[("time", &duration_text(cost.duration_ms))]);
            if let Some(cents) = cost.cost_cents {
                text.push_str(&format!(" · {}", money_text(cents)));
            }
            cards.push(
                card("cost", 6, c::TOPIC_RAIL_COST, cx)
                    .child(div().t_mono().text_color(k.ink).child(text))
                    .into_any_element(),
            );
        }
        let cards = cards.into_iter().enumerate().map(|(i, el)| {
            ui::motion::rise(
                SharedString::from(format!("topic-rail-{t}-{i}")),
                i,
                div().child(el),
                cx,
            )
        });
        div()
            .id("topic-rail")
            .size_full()
            .overflow_y_scroll()
            .bg(k.rail)
            .p(space::S16)
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
