//! The plan cards of the members at work, at the foot of a v2 timeline,
//! one per member (Live.dc.html, NATIVE.md §9.2): the member's tile with a
//! live ring, then a card on `fill` (radius 12, no border, no shadow) with
//! `计划 3/5` and a disclosure chevron, the 4 pt determinate bar, the steps
//! (the current one with what it does now) and a footer with the time used
//! and when it started. While someone holds the member's computer the run
//! is paused: a pause mark, no breathing, and the held line. 停止 and 接管
//! are in the 电脑 pane (the inspector's 在哪干活).
//!
//! It replaces the v1 pending row (`live.rs`) on the 话题 page.

use gpui::{
    AnyElement, ClickEvent, Context, ElementId, IntoElement, ParentElement, SharedString, Styled,
    div,
    prelude::FluentBuilder as _, px,
};

use super::{TimelineView, model, v2::GUTTER};
use crate::ui::controls::NamedButton as _;
use crate::{
    copy::{fill, live as l, process as pr, timeline as t, topic_rail as c},
    data::{BotRunSummary, Millis, PlanState},
    theme::{Type as _, space},
    ui::{
        self, Sym,
        native::{
            NativeType as _, PushSize, PushStyle, disabled_push, native_colors, push_button,
        },
    },
    views::topic_page::working_runs,
};

/// Rows of the plan card: 28 pt; its header 40 pt and footer 32 pt.
const ROW: gpui::Pixels = crate::ui::native::tokens::height::SM;
const HEADER: gpui::Pixels = px(40.);

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
        // The run 实时查看 shows on the right: the plan is drawn there, so
        // the 话题 keeps one line for it (never the plan twice).
        let in_pane = match self.services.nav.read(cx).visible_pane() {
            crate::app::RightPane::Live(id) => Some(id),
            _ => None,
        };
        let mut out: Vec<AnyElement> = runs
            .iter()
            .map(|run| {
                if in_pane.as_deref() == Some(run.id.as_str()) {
                    return self.plan_line(run, cx);
                }
                let paused_by = p.machine_screen(&run.id).and_then(|s| s.paused_by);
                let times = p.run_step_times(&run.id);
                self.plan_card(run, paused_by.is_some(), &times, cx)
            })
            .collect();
        out.extend(self.failed_run_alert(cx));
        out
    }

    /// The one-line card of a run whose plan shows in 实时查看: `计划 3/5`,
    /// the step it is on and `在右侧查看`.
    fn plan_line(&self, run: &BotRunSummary, cx: &mut Context<Self>) -> AnyElement {
        let n = native_colors(cx);
        let bot = self.members.iter().find(|m| m.id == run.bot_id).cloned();
        let name = bot.as_ref().map(|m| m.name.clone()).unwrap_or_default();
        let progress = crate::views::topic_page::plan_progress(&run.plan);
        let stopping = self.services.provider.run_stopping(&run.id);
        let tile = div()
            .flex_none()
            .child(ui::MemberAvatar::new(bot.as_ref(), GUTTER));
        let card = div()
            .flex_1()
            .min_w_0()
            .max_w(super::v2::TEXT_MAX)
            .h(HEADER)
            .px(space::S16)
            // §1.7, D5: a bordered surface, never a fill step alone.
            .rounded(crate::ui::native::tokens::radius::CONTAINER)
            .border_1()
            .border_color(n.separator)
            .bg(n.content)
            .flex()
            .items_center()
            .gap(space::S8)
            .child(div().flex_none().n_headline().text_color(n.label).child(
                if run.plan.is_empty() {
                    fill(c::STATE_WORKING_NO_PLAN, &[("name", &name)])
                } else {
                    l::LIVE_PLAN.to_string()
                },
            ))
            .children(progress.as_ref().map(|(step, total, _)| {
                div()
                    .flex_none()
                    .n_subheadline()
                    .n_figures()
                    .text_color(n.secondary)
                    .child(format!("{step}/{total}"))
            }))
            .children(progress.and_then(|(_, _, now)| now).map(|now| {
                div()
                    .flex_1()
                    .min_w_0()
                    .one_line()
                    .n_body()
                    .text_color(n.label)
                    .child(now)
            }))
            .when(run.plan.is_empty(), |el| el.child(div().flex_1()))
            .when(stopping, |el| {
                el.child(crate::views::process::run_badge(run, true, cx))
            })
            .child(
                div()
                    .flex_none()
                    .n_subheadline()
                    .text_color(n.secondary)
                    .child(t::PLAN_IN_PANE),
            );
        div()
            .mt(space::S16)
            .px(space::S8)
            .flex()
            .gap(space::S12)
            .child(tile)
            .child(card)
            .into_any_element()
    }

    /// A run that failed, in place of its card at the foot of the 话题: an
    /// alert that says why (COMPLETENESS §2.15), with 重试 and 查看日志. The
    /// red is on its symbol only (D3); title and frame stay neutral. It
    /// shows while the newest run of the 话题 is the failed one.
    fn failed_run_alert(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        use crate::data::RunStatus;
        let start = |r: &BotRunSummary| r.started_at.unwrap_or(r.created_at);
        let run = self.runs.iter().max_by_key(|r| start(r))?;
        if run.status != RunStatus::Failed {
            return None;
        }
        let n = native_colors(cx);
        let bot = self.members.iter().find(|m| m.id == run.bot_id).cloned();
        let name = crate::copy::isolate(&bot.as_ref().map(|m| m.name.clone()).unwrap_or_default());
        let cause = run
            .summary
            .clone()
            .or_else(|| {
                let process = self.services.provider.run_process(&run.id);
                crate::views::process::error_line(&process.steps)
            })
            .map(|c| c.trim().trim_end_matches(['。', '.']).to_string())
            .filter(|c| !c.is_empty());
        // The title states the cause (`打包失败：找不到签名证书`).
        let title = cause.unwrap_or_else(|| fill(t::RUN_FAILED_TITLE_NO_CAUSE, &[("name", &name)]));
        let ended = run.ended_at.unwrap_or(run.updated_at);
        let at = match model::local_day(ended) {
            Some(day) => format!(
                "{} {}",
                model::day_label(day, crate::selftest::clock::local_now().date_naive()),
                model::clock(ended)
            ),
            None => model::clock(ended),
        };
        let step = run
            .plan
            .iter()
            .find(|p| !matches!(p.state, PlanState::Done))
            .map(|p| p.text.clone());
        let detail = match &step {
            Some(step) => fill(
                t::RUN_FAILED_DETAIL,
                &[("name", &name), ("time", &at), ("step", step)],
            ),
            None => fill(
                t::RUN_FAILED_DETAIL_NO_STEP,
                &[("name", &name), ("time", &at)],
            ),
        };
        let retry = {
            let run = run.clone();
            push_button(
                ElementId::Name(format!("run-retry-{}", run.id).into()),
                t::RUN_RETRY,
                PushStyle::Default,
                PushSize::Small,
                cx,
            )
            .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                // A new run with the same request (`POST /v1/runs/:id/retry`).
                // A refusal blocks: an alert, no toast.
                let rx = this
                    .services
                    .provider
                    .retry_member_problem(&run.bot_id, &run.id);
                cx.spawn(async move |_, cx| {
                    let result = rx.await.unwrap_or(Err(crate::data::AdminError::Other));
                    if let Err(e) = result {
                        cx.update(|cx| {
                            if let Some(window) = cx.active_window() {
                                let _ = window.update(cx, |_, window, cx| {
                                    crate::views::nav::dialogs::error_alert(
                                        t::RUN_RETRY_REFUSED,
                                        crate::views::settings::widgets::error_text(e),
                                        |_, _| {},
                                        window,
                                        cx,
                                    )
                                });
                            }
                        });
                    }
                })
                .detach();
            }))
        };
        let log = {
            let id = run.id.clone();
            push_button(
                ElementId::Name(format!("run-log-{}", run.id).into()),
                t::RUN_VIEW_LOG,
                PushStyle::Outline,
                PushSize::Small,
                cx,
            )
            .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                this.services.nav.update(cx, |nav, cx| {
                    nav.set_right_pane(crate::app::RightPane::Live(id.clone()), cx)
                });
            }))
        };
        Some(
            div()
                .mt(space::S16)
                .px(space::S8)
                .flex()
                .gap(space::S12)
                .child(
                    div()
                        .flex_none()
                        .child(ui::MemberAvatar::new(bot.as_ref(), GUTTER)),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .max_w(super::v2::TEXT_MAX)
                        .px(space::S16)
                        .py(space::S12)
                        .rounded(crate::theme::radius::FIELD)
                        .border_1()
                        .border_color(n.separator)
                        .flex()
                        .items_start()
                        .gap(space::S12)
                        .child(
                            div()
                                .flex_none()
                                .h(crate::ui::native::tokens::text::UI.1)
                                .flex()
                                .items_center()
                                .child(ui::sym(Sym::Failed, ui::SymSize::INLINE, n.red)),
                        )
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .flex()
                                .flex_col()
                                .gap(space::S4)
                                .child(
                                    div()
                                        .n_headline()
                                        .text_color(n.label)
                                        .line_clamp(2)
                                        .child(title),
                                )
                                .child(div().n_body().text_color(n.secondary).child(detail))
                                // The actions sit under the description, so
                                // the text keeps the card's width and never
                                // wraps a lone `次。` (break-ui).
                                .child(
                                    div()
                                        .pt(space::S8)
                                        .flex()
                                        .items_center()
                                        .gap(space::S8)
                                        .child(retry)
                                        .child(log),
                                ),
                        ),
                )
                .into_any_element(),
        )
    }

    /// The working card (§2.4): the member's author line over a compact
    /// bordered card. Header: `正在做：<step>` 14 medium and a StatusBadge
    /// (`在做 3/5` spinning, 已暂停, 停止中) with the disclosure chevron;
    /// content: Progress h4 and `3/5` 12 muted, then the steps; footer:
    /// `已用 50 分钟 · 09:31 开始` 12 muted and ghost sm 查看, outline sm 停止….
    fn plan_card(
        &mut self,
        run: &BotRunSummary,
        held: bool,
        times: &[Millis],
        cx: &mut Context<Self>,
    ) -> AnyElement {
        use crate::ui::native::{BadgeState, StatusBadge, badge::state_mark, progress};
        crate::selftest::drawn::mark("plan-card", run.id.clone());
        let n = native_colors(cx);
        let bot = self.members.iter().find(|m| m.id == run.bot_id).cloned();
        let open = !self.plan_collapsed.contains(&run.id);
        let progress_of = crate::views::topic_page::plan_progress(&run.plan);
        let now = crate::selftest::clock::now_ms();
        let stopping = self.services.provider.run_stopping(&run.id);
        let current = run
            .plan
            .iter()
            .position(|s| matches!(s.state, PlanState::Current | PlanState::Waiting));
        let waiting = current.is_some_and(|i| run.plan[i].state == PlanState::Waiting);

        // The disclosure chevron (28 ghost), only with steps to show.
        let toggle = (!run.plan.is_empty()).then(|| {
            let id = run.id.clone();
            crate::ui::native::icon_button_bare(
                ElementId::Name(format!("plan-toggle-{}", run.id).into()),
                if open { Sym::ChevronUp } else { Sym::ChevronDown },
                crate::ui::native::IconSize::Small,
            )
            .named(if open { c::PLAN_COLLAPSE } else { c::PLAN_EXPAND })
            .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                if !this.plan_collapsed.remove(&id) {
                    this.plan_collapsed.insert(id.clone());
                }
                cx.notify();
            }))
        });
        let (badge_state, badge_text) = if stopping {
            (BadgeState::Stopping, pr::STATUS_STOPPING.to_string())
        } else if held {
            (BadgeState::Paused, c::STATE_PAUSED_SHORT.to_string())
        } else if waiting {
            (BadgeState::NeedsYou, c::STATE_YOU_SHORT.to_string())
        } else {
            match &progress_of {
                Some((step, total, _)) => (BadgeState::Working, format!("{} {step}/{total}", c::STATE_WORKING_SHORT)),
                None => (BadgeState::Working, c::STATE_WORKING_SHORT.to_string()),
            }
        };
        let doing = progress_of
            .as_ref()
            .and_then(|(_, _, now)| now.clone())
            .map(|step| fill(t::PLAN_DOING, &[("step", &step)]))
            .unwrap_or_else(|| l::LIVE_PLAN.to_string());
        let header = div()
            .flex()
            .items_center()
            .gap(space::S8)
            .child(
                crate::ui::truncate::TruncText::new(doing)
                    .id(SharedString::from(format!("plan-doing-{}", run.id)))
                    .flex_1()
                    .n_headline()
                    .text_color(n.label),
            )
            .child(div().flex_none().child(StatusBadge::new(badge_state, badge_text)))
            .children(toggle);

        // D12: the one Progress, and the count beside it.
        let bar = progress_of.as_ref().map(|(step, total, _)| {
            let done = run
                .plan
                .iter()
                .filter(|s| s.state == PlanState::Done)
                .count();
            div()
                .flex()
                .items_center()
                .gap(space::S12)
                .child(div().flex_1().child(progress(done as f32 / (*total).max(1) as f32, cx)))
                .child(
                    div()
                        .flex_none()
                        .n_footnote()
                        .n_figures()
                        .text_color(n.secondary)
                        .child(format!("{step}/{total}")),
                )
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
                let at_work = matches!(s.state, PlanState::Current | PlanState::Waiting);
                // §2.12: the step at work spins; never a free colored dot.
                let mark: AnyElement = match s.state {
                    _ if at_work && held => {
                        ui::sym(Sym::Pause, ui::SymSize::INLINE_SMALL, n.secondary).into_any_element()
                    }
                    PlanState::Waiting => state_mark(BadgeState::NeedsYou, &n)
                        .unwrap_or_else(|| div().into_any_element()),
                    PlanState::Current => state_mark(BadgeState::Working, &n)
                        .unwrap_or_else(|| div().into_any_element()),
                    PlanState::Done => {
                        ui::sym(Sym::Check, ui::SymSize::INLINE_SMALL, n.secondary).into_any_element()
                    }
                    PlanState::Pending => {
                        ui::sym(Sym::Todo, ui::SymSize::INLINE_SMALL, n.tertiary).into_any_element()
                    }
                };
                let time = if at_work {
                    started.map(|at| span_text(FOR, now - at))
                } else if s.state == PlanState::Done {
                    started.zip(next).map(|(a, b)| span_text(TOOK, b - a))
                } else {
                    None
                };
                div()
                    .h(ROW)
                    .px(space::S8)
                    .rounded(crate::ui::native::tokens::radius::ITEM)
                    .when(at_work, |el| el.bg(n.fill))
                    .flex()
                    .items_center()
                    .gap(space::S8)
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
                        crate::ui::truncate::TruncText::new(s.text.clone())
                            .id(SharedString::from(format!("plan-step-{}-{i}", run.id)))
                            .flex_1()
                            .n_body()
                            .text_color(if at_work { n.label } else { n.secondary }),
                    )
                    .children(time.map(|t| {
                        div()
                            .flex_none()
                            .n_footnote()
                            .n_figures()
                            .text_color(n.secondary)
                            .child(t)
                    }))
                    .into_any_element()
            })
            .collect();
        let doing_now = (!held)
            .then(|| self.doing_now(&run.id))
            .flatten()
            .or_else(|| held.then(|| fill(l::LIVE_PLAN_HELD, &[("name", &bot.as_ref().map(|m| m.name.clone()).unwrap_or_default())])));

        // Footer: the time used and the start, then 查看 and 停止….
        let view = {
            let id = run.id.clone();
            push_button(
                ElementId::Name(format!("plan-view-{}", run.id).into()),
                t::PLAN_VIEW,
                PushStyle::Ghost,
                PushSize::Small,
                cx,
            )
            .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                let id = id.clone();
                this.services.nav.update(cx, |nav, cx| {
                    nav.set_right_pane(crate::app::RightPane::Live(id), cx)
                });
            }))
        };
        let stop = crate::views::process::asked_by(run, self.me.as_deref()).then(|| {
            let run = run.clone();
            disabled_push(
                push_button(
                    ElementId::Name(format!("plan-stop-{}", run.id).into()),
                    t::PLAN_STOP,
                    PushStyle::Outline,
                    PushSize::Small,
                    cx,
                )
                .loading(stopping),
                stopping,
                cx,
            )
            .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                crate::views::process::stop_run(&this.services, &run, cx);
                cx.notify();
            }))
        });
        let footer = div()
            .flex()
            .items_center()
            .gap(space::S8)
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .one_line()
                    .n_footnote()
                    .n_figures()
                    .text_color(n.secondary)
                    .children(run.started_at.map(|at| {
                        fill(
                            t::PLAN_FOOTER,
                            &[("used", &span_text(USED, now - at)), ("time", &model::clock(at))],
                        )
                    })),
            )
            .child(view)
            .children(stop);

        // §2.4 compact card: radius 14, a 1 px border, the card fill,
        // shadow-sm, p 16, gap 16, the one in-thread width.
        let card = div()
            .w_full()
            .max_w(super::v2::TEXT_MAX)
            .rounded(crate::ui::native::tokens::radius::CARD)
            .border_1()
            .border_color(n.separator)
            .bg(n.sheet)
            .shadow(crate::ui::native::tokens::shadow::sm())
            .p(space::S16)
            .flex()
            .flex_col()
            .gap(space::S16)
            .child(header)
            .children(bar)
            .when(!rows.is_empty(), |el| {
                el.child(
                    div()
                        .flex()
                        .flex_col()
                        .gap(space::S2)
                        .children(rows)
                        .children(doing_now.map(|d| {
                            div()
                                .pl(space::S32)
                                .min_w_0()
                                .one_line()
                                .n_footnote()
                                .text_color(n.secondary)
                                .child(d)
                        })),
                )
            })
            .child(footer);

        div()
            .mt(space::S16)
            .px(space::S8)
            .flex()
            .gap(space::S12)
            .child(
                div()
                    .flex_none()
                    .child(ui::MemberAvatar::new(bot.as_ref(), GUTTER)),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .gap(space::S6)
                    .child(self.v2_meta(bot.as_ref(), run.started_at.unwrap_or(run.created_at), cx))
                    .child(card),
            )
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
