//! The plan cards of the members at work, at the foot of a v2 timeline,
//! one per member (Live.dc.html): the member's tile with a live ring, then a card with
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
    ui::{self, Sym},
    views::topic_page::working_runs,
};

/// Rows of the plan card: 34 pt; its header 40 pt and footer 36 pt.
const ROW: gpui::Pixels = px(34.);
const HEADER: gpui::Pixels = px(40.);
const FOOTER: gpui::Pixels = px(36.);

/// A span of time: whole minutes (rounded, at least 1), and hours and
/// minutes from 60 minutes on (`已 9 小时 49 分钟`, never `已 589 分钟`).
/// The one formatter of the plan card, its footer and the 电脑 pane.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Span {
    Minutes(i64),
    Hours(i64, i64),
}

impl Span {
    pub fn of(ms: i64) -> Self {
        let minutes = ((ms.max(0) + 30_000) / 60_000).max(1);
        if minutes < 60 {
            Span::Minutes(minutes)
        } else {
            Span::Hours(minutes / 60, minutes % 60)
        }
    }

    /// The template of `copy` for this span and its values (`n`, or `h`
    /// and `m`).
    pub fn parts(self, copy: SpanCopy) -> (&'static str, Vec<(&'static str, String)>) {
        match self {
            Span::Minutes(n) => (copy.minutes, vec![("n", n.to_string())]),
            Span::Hours(h, m) => (copy.hours, vec![("h", h.to_string()), ("m", m.to_string())]),
        }
    }
}

/// The copy of one kind of span: in minutes (`{n}`), and in hours and
/// minutes (`{h}`, `{m}`).
#[derive(Clone, Copy, Debug)]
pub struct SpanCopy {
    pub minutes: &'static str,
    pub hours: &'static str,
}

/// `已用 19 分钟`: the time a run has used (the card's footer).
pub const USED: SpanCopy = SpanCopy {
    minutes: c::PLAN_USED,
    hours: c::PLAN_USED_HOURS,
};
/// `已 3 分钟`: how long the current step has run.
pub const FOR: SpanCopy = SpanCopy {
    minutes: l::LIVE_PLAN_FOR,
    hours: l::LIVE_PLAN_FOR_HOURS,
};
/// `4 分钟`: how long a done step took.
pub const TOOK: SpanCopy = SpanCopy {
    minutes: l::LIVE_PLAN_TOOK,
    hours: l::LIVE_PLAN_TOOK_HOURS,
};

/// `ms` as text in `copy`.
pub fn span_text(copy: SpanCopy, ms: i64) -> String {
    let (template, values) = Span::of(ms).parts(copy);
    let values: Vec<(&str, &str)> = values.iter().map(|(k, v)| (*k, v.as_str())).collect();
    fill(template, &values)
}

impl TimelineView {
    /// One plan card per member that works in the open 话题, in the order
    /// they started.
    pub(super) fn render_plans(&mut self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let runs: Vec<BotRunSummary> = working_runs(&self.runs).into_iter().cloned().collect();
        let p = self.services.provider.clone();
        runs.iter()
            .map(|run| {
                let paused_by = p.machine_screen(&run.id).and_then(|s| s.paused_by);
                let times = p.run_step_times(&run.id);
                self.plan_card(run, paused_by.is_some(), &times, cx)
            })
            .collect()
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
        let now = crate::selftest::clock::now_ms();
        let current = run
            .plan
            .iter()
            .position(|s| matches!(s.state, PlanState::Current | PlanState::Waiting));

        // The tile, with its live status dot while the member works (§7.9).
        let tile = div().flex_none().child(
            ui::MemberAvatar::new(bot.as_ref(), GUTTER).ring((!held).then_some(ui::Ring::Live)),
        );

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
                        Sym::ChevronUp
                    } else {
                        Sym::ChevronDown
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
                            ui::icon(Sym::Pause, theme::icon::TAG, k.ink_3).into_any_element()
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
                                        div()
                                            .flex_none()
                                            .t_meta()
                                            .text_color(k.ink_2)
                                            .child(span_text(FOR, now - at))
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
                            ui::icon(Sym::Check, theme::icon::CHIP, k.ink_3).into_any_element()
                        } else {
                            div()
                                .size(px(10.))
                                .rounded_full()
                                .border(px(1.5))
                                .border_color(k.line_2)
                                .into_any_element()
                        };
                        let took = done
                            .then(|| started.zip(next).map(|(a, b)| span_text(TOOK, b - a)))
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
                            .children(
                                took.map(|n| {
                                    div().flex_none().t_meta().text_color(k.ink_3).child(n)
                                }),
                            )
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
                .t_meta()
                .text_color(k.ink_3)
                .child(div().flex_none().child(span_text(USED, now - at)))
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
    fn spans_read_in_minutes_then_hours() {
        const MIN: i64 = 60_000;
        assert_eq!(span_text(USED, 19 * MIN), "已用 19 分钟");
        assert_eq!(span_text(USED, 10_000), "已用 1 分钟");
        assert_eq!(span_text(USED, 65 * MIN), "已用 1 小时 5 分钟");
        assert_eq!(span_text(FOR, 589 * MIN), "已 9 小时 49 分钟");
        assert_eq!(span_text(FOR, 59 * MIN + 20_000), "已 59 分钟");
        assert_eq!(span_text(FOR, 59 * MIN + 40_000), "已 1 小时 0 分钟");
        assert_eq!(span_text(TOOK, 4 * MIN), "4 分钟");
        assert_eq!(span_text(TOOK, 125 * MIN), "2 小时 5 分钟");
        assert_eq!(span_text(TOOK, -5 * MIN), "1 分钟");
    }
}
