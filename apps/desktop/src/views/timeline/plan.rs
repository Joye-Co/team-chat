//! The plan card of a member at work, at the foot of a v2 timeline
//! (Live.dc.html): the member's tile with a live ring, then a card with
//! `计划 3/5` and 收起 / 展开, the step bar, the steps (the current one on
//! a `sunken` fill with what it does now) and a footer with the time used
//! and when it started. While someone holds the member's computer the run
//! is paused: a pause mark, no breathing, and the held line. 停止 and 接管
//! are in the 电脑 pane (the title bar's 电脑 button).
//!
//! It replaces the v1 pending row (`live.rs`) on the 话题 page.

use gpui::{
    AnyElement, ClickEvent, Context, ElementId, InteractiveElement as _, IntoElement,
    ParentElement, SharedString, StatefulInteractiveElement as _, Styled, div,
    prelude::FluentBuilder as _, px,
};

use super::{TimelineView, model, v2::GUTTER};
use crate::{
    copy::{fill, live as l, topic_rail as c},
    data::{BotRunSummary, Millis, PlanState},
    theme::{self, Type as _, space},
    ui::{self, Lucide},
    views::topic_page::working_run,
};

/// Rows of the plan card: 34 pt; its header 40 pt and footer 36 pt.
const ROW: gpui::Pixels = px(34.);
const HEADER: gpui::Pixels = px(40.);
const FOOTER: gpui::Pixels = px(36.);

/// `已用 19 分钟` or `已用 1 小时 5 分钟`.
pub fn used_text(ms: i64) -> String {
    let minutes = (ms.max(0) / 60_000).max(1);
    if minutes < 60 {
        fill(c::PLAN_USED, &[("n", &minutes.to_string())])
    } else {
        fill(
            c::PLAN_USED_HOURS,
            &[
                ("h", &(minutes / 60).to_string()),
                ("m", &(minutes % 60).to_string()),
            ],
        )
    }
}

/// Whole minutes, rounded, at least 1.
fn minutes(ms: i64) -> String {
    ((ms.max(0) + 30_000) / 60_000).max(1).to_string()
}

fn now_ms() -> Millis {
    chrono::Utc::now().timestamp_millis()
}

impl TimelineView {
    /// The plan card of the member that works in the open 话题, if any.
    pub(super) fn render_plan(&mut self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let run = working_run(&self.runs)?.clone();
        let p = self.services.provider.clone();
        let paused_by = p.machine_screen(&run.id).and_then(|s| s.paused_by);
        let times = p.run_step_times(&run.id);
        Some(self.plan_card(&run, paused_by.is_some(), &times, cx))
    }

    fn plan_card(
        &mut self,
        run: &BotRunSummary,
        held: bool,
        times: &[Millis],
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let k = theme::colors(cx);
        let bot = self.members.iter().find(|m| m.id == run.bot_id).cloned();
        let name = bot.as_ref().map(|m| m.name.clone()).unwrap_or_default();
        let open = !self.plan_collapsed.contains(&run.id);
        let progress = crate::views::topic_page::plan_progress(&run.plan);
        let now = now_ms();
        let current = run
            .plan
            .iter()
            .position(|s| matches!(s.state, PlanState::Current | PlanState::Waiting));

        // The tile, with a live ring while the member works.
        let ring = div()
            .absolute()
            .top(px(-3.))
            .left(px(-3.))
            .size(GUTTER + px(6.))
            .rounded(px(11.))
            .border(px(1.5))
            .border_color(k.live);
        let tile = div()
            .flex_none()
            .relative()
            .size(GUTTER)
            .child(ui::MemberAvatar::new(bot.as_ref(), GUTTER))
            .when(!held, |el| {
                el.child(ui::motion::breathe(
                    SharedString::from(format!("plan-ring-{}", run.id)),
                    ring,
                    cx,
                ))
            });

        let toggle = (!run.plan.is_empty()).then(|| {
            let id = run.id.clone();
            div()
                .id(ElementId::Name(format!("plan-toggle-{}", run.id).into()))
                .flex_none()
                .ml_auto()
                .h(px(28.))
                .pl(space::S8)
                .pr(space::S6)
                .rounded(px(7.))
                .flex()
                .items_center()
                .gap(space::S4)
                .t_meta()
                .text_color(k.ink_2)
                .cursor_pointer()
                .hover(|s| s.bg(k.hover))
                .child(if open {
                    c::PLAN_COLLAPSE
                } else {
                    c::PLAN_EXPAND
                })
                .child(ui::icon(
                    if open {
                        Lucide::ChevronUp
                    } else {
                        Lucide::ChevronDown
                    },
                    px(12.),
                    k.ink_2,
                ))
                .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                    if !this.plan_collapsed.remove(&id) {
                        this.plan_collapsed.insert(id.clone());
                    }
                    cx.notify();
                }))
        });
        let header = div()
            .h(HEADER)
            .pl(space::S14)
            .pr(space::S6)
            .flex()
            .items_center()
            .gap(space::S8)
            .child(
                div()
                    .flex_none()
                    .t_small()
                    .medium()
                    .text_color(k.ink)
                    .child(if run.plan.is_empty() {
                        fill(c::STATE_WORKING_NO_PLAN, &[("name", &name)])
                    } else {
                        l::LIVE_PLAN.to_string()
                    }),
            )
            .children(progress.as_ref().map(|(n, total, _)| {
                div()
                    .flex_none()
                    .t_mono()
                    .text_color(k.ink_3)
                    .child(format!("{n}/{total}"))
            }))
            .children(toggle);

        let bar = (!run.plan.is_empty()).then(|| {
            div()
                .px(space::S14)
                .flex()
                .gap(px(3.))
                .children(run.plan.iter().enumerate().map(|(i, s)| {
                    let seg = div().flex_1().h(px(3.)).rounded(px(2.));
                    match s.state {
                        PlanState::Done => seg.bg(k.ink_3).into_any_element(),
                        PlanState::Pending => seg.bg(k.line_2).into_any_element(),
                        PlanState::Waiting => seg.bg(k.you).into_any_element(),
                        PlanState::Current if held => seg.bg(k.ink_3).into_any_element(),
                        PlanState::Current => ui::motion::breathe(
                            SharedString::from(format!("plan-bar-{}-{i}", run.id)),
                            seg.bg(k.live),
                            cx,
                        ),
                    }
                }))
        });

        // Expanded: every step. Collapsed: only the step it is on.
        let rows: Vec<AnyElement> = run
            .plan
            .iter()
            .enumerate()
            .filter(|(i, _)| open || Some(*i) == current)
            .map(|(i, s)| {
                let started = times.get(i).copied();
                let next = times.get(i + 1).copied();
                match s.state {
                    PlanState::Current | PlanState::Waiting => {
                        let mark: AnyElement = if held {
                            ui::icon(Lucide::Pause, theme::icon::TAG, k.ink_3).into_any_element()
                        } else if s.state == PlanState::Waiting {
                            div()
                                .size(px(8.))
                                .rounded_full()
                                .bg(k.you)
                                .into_any_element()
                        } else {
                            ui::motion::breathe(
                                SharedString::from(format!("plan-dot-{}-{i}", run.id)),
                                div().size(px(8.)).rounded_full().bg(k.live),
                                cx,
                            )
                        };
                        let doing = if held {
                            Some(fill(l::LIVE_PLAN_HELD, &[("name", &name)]))
                        } else {
                            self.doing_now(&run.id)
                        };
                        div()
                            .flex()
                            .flex_col()
                            .gap(space::S2)
                            .pt(px(7.))
                            .pb(space::S8)
                            .px(space::S8)
                            .rounded(theme::radius::MD)
                            .bg(k.sunken)
                            .child(
                                div()
                                    .h(px(20.))
                                    .flex()
                                    .items_center()
                                    .gap(space::S10)
                                    .child(
                                        div()
                                            .flex_none()
                                            .size(px(16.))
                                            .flex()
                                            .items_center()
                                            .justify_center()
                                            .child(mark),
                                    )
                                    .child(
                                        div()
                                            .flex_1()
                                            .min_w_0()
                                            .one_line()
                                            .t_small()
                                            .medium()
                                            .text_color(k.ink)
                                            .child(s.text.clone()),
                                    )
                                    .children(started.map(|at| {
                                        div().flex_none().t_meta().text_color(k.ink_2).child(
                                            fill(l::LIVE_PLAN_FOR, &[("n", &minutes(now - at))]),
                                        )
                                    })),
                            )
                            .children(doing.map(|d| {
                                div()
                                    .pl(px(26.))
                                    .min_w_0()
                                    .one_line()
                                    .t_meta()
                                    .text_color(k.ink_2)
                                    .child(d)
                            }))
                            .into_any_element()
                    }
                    state => {
                        let done = state == PlanState::Done;
                        let mark: AnyElement = if done {
                            ui::icon(Lucide::Check, theme::icon::CHIP, k.ink_3).into_any_element()
                        } else {
                            div()
                                .size(px(10.))
                                .rounded_full()
                                .border(px(1.5))
                                .border_color(k.line_2)
                                .into_any_element()
                        };
                        let took = done
                            .then(|| started.zip(next).map(|(a, b)| minutes(b - a)))
                            .flatten();
                        div()
                            .h(ROW)
                            .px(space::S8)
                            .flex()
                            .items_center()
                            .gap(space::S10)
                            .child(
                                div()
                                    .flex_none()
                                    .size(px(16.))
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .child(mark),
                            )
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .one_line()
                                    .t_small()
                                    .text_color(if done { k.ink_3 } else { k.ink_2 })
                                    .child(s.text.clone()),
                            )
                            .children(took.map(|n| {
                                div()
                                    .flex_none()
                                    .t_meta()
                                    .text_color(k.ink_3)
                                    .child(fill(l::LIVE_PLAN_TOOK, &[("n", &n)]))
                            }))
                            .into_any_element()
                    }
                }
            })
            .collect();

        let footer = run.started_at.map(|at| {
            div()
                .h(FOOTER)
                .px(space::S14)
                .flex()
                .items_center()
                .gap(space::S6)
                .border_t_1()
                .border_color(k.line)
                .bg(k.raised)
                .t_meta()
                .text_color(k.ink_3)
                .child(div().flex_none().child(used_text(now - at)))
                .child(div().flex_1())
                .child(
                    div()
                        .flex_none()
                        .t_mono()
                        .child(fill(c::PLAN_STARTED_AT, &[("time", &model::clock(at))])),
                )
        });

        let card = div()
            .flex_1()
            .min_w_0()
            .rounded(theme::radius::LG)
            .border_1()
            .border_color(k.line)
            .bg(k.bg)
            .shadow(k.lift())
            .overflow_hidden()
            .child(header)
            .children(bar)
            .when(!rows.is_empty(), |el| {
                el.child(
                    div()
                        .pt(space::S8)
                        .px(space::S6)
                        .pb(space::S6)
                        .flex()
                        .flex_col()
                        .gap(space::S2)
                        .children(rows),
                )
            })
            .when(run.plan.is_empty(), |el| el.pb(space::S2))
            .children(footer);

        div()
            .mt(space::S16)
            .px(space::S8)
            .flex()
            .gap(space::S12)
            .child(tile)
            .child(card)
            .into_any_element()
    }

    /// What the member does now, from its latest status line.
    fn doing_now(&self, run_id: &str) -> Option<String> {
        let process = self.services.provider.run_process(run_id);
        super::live::live_label(&process.steps)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn time_used_reads_in_minutes_then_hours() {
        assert_eq!(used_text(19 * 60_000), "已用 19 分钟");
        assert_eq!(used_text(10_000), "已用 1 分钟");
        assert_eq!(used_text(65 * 60_000), "已用 1 小时 5 分钟");
    }
}
