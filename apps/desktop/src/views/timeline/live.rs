//! Bots at work and people typing (DESIGN.md §9.2, §7.2):
//!
//! - the pending row: one 28 pt row per bot that works and has not posted
//!   yet, 12 pt under the last message (the working character, the bot's
//!   latest step, three pulsing dots, the elapsed time, `查看过程` and, for
//!   the person who asked, `停止`);
//! - the steps row: one collapsed 24 pt row above a reply that came out of a
//!   run (`改了 6 个文件，运行 3 条命令 · 4 分 12 秒`), which opens the
//!   steps inline;
//! - the typing line under the timeline, for people only.

use std::time::{Duration, Instant};

use gpui::{
    AnyElement, Context, ElementId, InteractiveElement as _, IntoElement, ParentElement,
    SharedString, StatefulInteractiveElement as _, Styled, div, prelude::FluentBuilder as _, px,
};

use gpui_component::Disableable as _;

use super::TimelineView;
use crate::{
    characters::CharacterStatus,
    copy::{fill, process as pr, timeline as t},
    data::{BotRunSummary, Message, Millis, ProcessStep, RunStatus},
    theme::{self, Type as _, space},
    ui::{self, Lucide},
};

/// Most pending rows; more working bots fold into one line.
const MAX_LIVE: usize = 3;
/// Most steps an opened steps row lists.
const MAX_STEPS: usize = 40;
/// The pending row enters over this (§12).
const LIVE_ENTER: Duration = theme::motion::SLOW;
/// A pending row whose run ended stays this long before it fades (§12).
const LIVE_HOLD: Duration = Duration::from_millis(400);

/// How long a leaving pending row stays: the hold, then the exit fade. The
/// fade is opacity, so it stays with reduce motion (§12).
fn leave_time() -> Duration {
    LIVE_HOLD + theme::motion::NORMAL
}

/// Opacity of a leaving pending row at `t` (0..1 of [`leave_time`]): full
/// through the hold, then down to 0 on `ease`.
fn leave_opacity(t: f32, ease: &impl Fn(f32) -> f32) -> f32 {
    let total = leave_time().as_secs_f32();
    let hold = LIVE_HOLD.as_secs_f32() / total;
    1. - ease(((t - hold) / (1. - hold)).clamp(0., 1.))
}

/// A run whose end is this close to a bot message is that message's run.
const RUN_MATCH_MS: Millis = 2 * 60_000;

/// Three 4 pt dots, opacity 0.3 to 0.9, 1.2 s loop, 150 ms apart; static
/// 0.9 / 0.6 / 0.3 with reduce motion (§12).
fn pulse_dots(color: gpui::Hsla, reduce_motion: bool) -> impl IntoElement {
    use gpui::{Animation, AnimationExt as _};
    div()
        .flex_none()
        .flex()
        .items_center()
        .gap(px(3.))
        .children((0..3).map(move |i| {
            let dot = div().size(px(4.)).rounded_full().bg(color);
            if reduce_motion {
                return dot.opacity([0.9, 0.6, 0.3][i]).into_any_element();
            }
            dot.with_animation(
                gpui::ElementId::NamedInteger("pending-dot".into(), i as u64),
                // A capped frame rate: every frame of this loop re-renders the
                // whole timeline, and 24 fps steps are not visible on a 1.2 s wave.
                Animation::new(theme::motion::TYPING_PULSE)
                    .repeat()
                    .with_max_fps(theme::motion::DECORATIVE_FPS),
                move |el, t| {
                    let phase = (t - i as f32 * 0.125).rem_euclid(1.0);
                    let wave = (phase * std::f32::consts::TAU).cos() * 0.5 + 0.5;
                    el.opacity(0.3 + 0.6 * wave)
                },
            )
            .into_any_element()
        }))
}

/// `m:ss` since `since`.
fn elapsed(since: Millis, now: Millis) -> String {
    let s = ((now - since).max(0) / 1000) as u64;
    if s < 3600 {
        format!("{}:{:02}", s / 60, s % 60)
    } else {
        format!("{}:{:02}:{:02}", s / 3600, s / 60 % 60, s % 60)
    }
}

/// `4 分 12 秒`, or `12 秒` under a minute.
pub fn duration_text(ms: i64) -> String {
    let s = (ms.max(0) / 1000) as u64;
    if s < 60 {
        fill(t::STEPS_DURATION_SEC, &[("s", &s.to_string())])
    } else {
        fill(
            t::STEPS_DURATION_MIN,
            &[("m", &(s / 60).to_string()), ("s", &(s % 60).to_string())],
        )
    }
}

/// The summary of a run's steps: what it changed and ran, and its failures.
pub fn steps_summary(steps: &[ProcessStep]) -> (String, usize) {
    let mut files = 0;
    let mut commands = 0;
    let mut failed = 0;
    let mut other = 0;
    for step in steps {
        match step {
            ProcessStep::Files { changes } => files += changes.len(),
            ProcessStep::Tool { ok, .. } => {
                commands += 1;
                if *ok == Some(false) {
                    failed += 1;
                }
            }
            ProcessStep::Approval { .. } => other += 1,
            _ => {}
        }
    }
    let mut parts = Vec::new();
    if files > 0 {
        parts.push(fill(t::STEPS_FILES, &[("n", &files.to_string())]));
    }
    if commands > 0 {
        parts.push(fill(t::STEPS_COMMANDS, &[("n", &commands.to_string())]));
    }
    if parts.is_empty() {
        parts.push(fill(t::STEPS_OTHER, &[("n", &other.max(1).to_string())]));
    }
    (parts.join("，"), failed)
}

/// One line per step for the opened steps row.
fn step_line(step: &ProcessStep) -> Option<(Lucide, String, bool)> {
    Some(match step {
        ProcessStep::Tool { title, ok, .. } => {
            (Lucide::SquareTerminal, title.clone(), *ok == Some(false))
        }
        ProcessStep::Files { changes } => (
            Lucide::FileText,
            changes
                .iter()
                .map(|c| c.path.rsplit('/').next().unwrap_or(&c.path))
                .collect::<Vec<_>>()
                .join("、"),
            false,
        ),
        ProcessStep::Approval { title, .. } => (Lucide::CircleCheck, title.clone(), false),
        ProcessStep::Status { message } => (Lucide::CircleDot, message.clone(), false),
        _ => return None,
    })
}

/// The latest step of a running bot, as one phrase for the pending row.
pub(super) fn live_label(steps: &[ProcessStep]) -> Option<String> {
    steps.iter().rev().find_map(|s| match s {
        ProcessStep::Status { message } if !message.trim().is_empty() => {
            Some(message.trim().to_string())
        }
        _ => None,
    })
}

impl TimelineView {
    /// The run a bot message came out of: a finished run of its author that
    /// ended within two minutes of it.
    fn run_of(&self, message: &Message) -> Option<&BotRunSummary> {
        self.runs.iter().find(|r| {
            r.bot_id == message.author_id
                && !r.status.is_active()
                && r.ended_at
                    .is_some_and(|end| (end - message.created_at).abs() <= RUN_MATCH_MS)
        })
    }

    /// The collapsed steps row above a bot reply that used tools (§9.2).
    pub(super) fn render_steps(
        &self,
        message: &Message,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let run = self.run_of(message)?;
        let colors = theme::colors(cx);
        let process = self.services.provider.run_process(&run.id);
        let (summary, failed) = if process.steps.is_empty() {
            (t::VIEW_PROCESS.to_string(), 0)
        } else {
            steps_summary(&process.steps)
        };
        let duration = run
            .started_at
            .zip(run.ended_at)
            .map(|(a, b)| duration_text(b - a));
        let open = self.open_steps.contains(&run.id);
        let run_id = run.id.clone();
        let key = super::model::key(message).to_string();
        let toggle = div()
            .id(ElementId::Name(format!("steps-{}", run.id).into()))
            .h(px(24.))
            .ml(-space::S4)
            .pl(space::S4)
            .pr(space::S8)
            .flex()
            .items_center()
            .gap(space::S6)
            .self_start()
            .rounded(theme::radius::SM)
            .hover(|el| el.bg(colors.hover))
            .active(|el| el.bg(colors.pressed))
            .t_small()
            .text_color(colors.text_2)
            .child(ui::icon(
                if open {
                    Lucide::ChevronDown
                } else {
                    Lucide::ChevronRight
                },
                theme::icon::TAG,
                colors.text_3,
            ))
            .child(div().one_line().child(summary))
            .when(failed > 0, |el| {
                el.child(
                    div()
                        .flex_none()
                        .text_color(colors.danger_text)
                        .child(fill(t::STEPS_FAILED, &[("n", &failed.to_string())])),
                )
            })
            .when_some(duration, |el, d| {
                el.child(
                    div()
                        .flex_none()
                        .tnum()
                        .text_color(colors.text_3)
                        .child(format!("· {d}")),
                )
            })
            .on_click(cx.listener(move |this, _, _, cx| {
                if !this.open_steps.remove(&run_id) {
                    this.open_steps.insert(run_id.clone());
                }
                this.remeasure_key(&key);
                cx.notify();
            }));
        let lines = open.then(|| {
            div()
                .id(ElementId::Name(format!("steps-list-{}", run.id).into()))
                .mt(space::S4)
                .mb(space::S4)
                .ml(space::S4)
                .pl(space::S10)
                .border_l_1()
                .border_color(colors.border)
                .max_h(px(220.))
                .overflow_y_scroll()
                .flex()
                .flex_col()
                .gap(space::S4)
                .children(
                    process
                        .steps
                        .iter()
                        .filter_map(step_line)
                        .take(MAX_STEPS)
                        .map(|(glyph, text, failed)| {
                            let color = if failed {
                                colors.danger_text
                            } else {
                                colors.text_2
                            };
                            div()
                                .flex()
                                .items_center()
                                .gap(space::S6)
                                .t_meta()
                                .text_color(color)
                                .child(ui::icon(glyph, theme::icon::TAG, colors.text_3))
                                .child(div().min_w(px(0.)).one_line().child(text))
                        }),
                )
        });
        Some(
            div()
                .flex()
                .flex_col()
                .mb(space::S2)
                .child(toggle)
                .children(lines)
                .into_any_element(),
        )
    }

    /// Bots that work in this topic and have not started their reply. A run
    /// that waits for a member (an answer, a review) is not at work.
    fn working_runs(&self) -> Vec<&BotRunSummary> {
        let mut seen = Vec::new();
        self.runs
            .iter()
            .filter(|r| r.status.is_active() && r.status != RunStatus::Waiting)
            .filter(|r| {
                !self
                    .messages
                    .iter()
                    .rev()
                    .take(20)
                    .any(|m| m.streaming && m.author_id == r.bot_id)
            })
            .filter(|r| {
                if seen.contains(&r.bot_id) {
                    false
                } else {
                    seen.push(r.bot_id.clone());
                    true
                }
            })
            .collect()
    }

    /// Follows the working runs after each refresh: a run seen for the first
    /// time gets its enter animation (once, never again on a return to the
    /// topic), and a run that stopped working stays as a leaving row for the
    /// hold and the exit (§12). `quiet` (opening a topic) shows the runs that
    /// are already there without an animation.
    pub(super) fn track_live(&mut self, quiet: bool, cx: &mut Context<Self>) {
        let now = Instant::now();
        let working: Vec<BotRunSummary> = self.working_runs().into_iter().cloned().collect();
        let gone: Vec<BotRunSummary> = self
            .live_shown
            .iter()
            .filter(|r| !working.iter().any(|w| w.id == r.id))
            .cloned()
            .collect();
        self.live_leaving
            .retain(|(r, _)| !working.iter().any(|w| w.id == r.id));
        for run in gone {
            if !quiet && !self.live_leaving.iter().any(|(r, _)| r.id == run.id) {
                self.live_leaving.push((run, now));
            }
        }
        for run in &working {
            self.live_entered.entry(run.id.clone()).or_insert(if quiet {
                now - LIVE_ENTER
            } else {
                now
            });
        }
        if self.live_entered.len() > 512 {
            let keep: Vec<String> = working.iter().map(|r| r.id.clone()).collect();
            self.live_entered.retain(|id, _| keep.contains(id));
        }
        self.live_shown = working;
        if !self.live_leaving.is_empty() {
            let wait = LIVE_HOLD + theme::motion::NORMAL;
            cx.spawn(async move |this, cx| {
                cx.background_executor().timer(wait).await;
                let _ = this.update(cx, |this, cx| {
                    this.live_leaving
                        .retain(|(_, at)| at.elapsed() < leave_time());
                    cx.notify();
                });
            })
            .detach();
        }
    }

    /// Forgets the rows of the topic that closed (switching is never
    /// animated, §12).
    pub(super) fn reset_live(&mut self) {
        self.live_shown.clear();
        self.live_leaving.clear();
    }

    /// `· 停止` for the person who asked (§9.2), or `· 正在停止…` (disabled)
    /// once pressed, until the run ends. A leaving row keeps the control,
    /// disabled, so its text does not jump while it fades.
    fn stop_control(
        &self,
        run: &BotRunSummary,
        leaving: bool,
        cx: &mut Context<Self>,
    ) -> Option<[AnyElement; 2]> {
        if !crate::views::process::asked_by(run, self.me.as_deref()) {
            return None;
        }
        let colors = theme::colors(cx);
        let stopping = if leaving {
            self.runs
                .iter()
                .any(|r| r.id == run.id && r.status == RunStatus::Cancelled)
        } else {
            self.services.provider.run_stopping(&run.id)
        };
        let button = ui::text_button(
            ElementId::Name(format!("live-stop-{}", run.id).into()),
            if stopping { pr::STOPPING } else { pr::STOP },
            cx,
        )
        .ml(-space::S6)
        .disabled(stopping || leaving)
        .when(stopping || leaving, |b| b.text_color(colors.text_3))
        .on_click({
            let run = run.clone();
            cx.listener(move |this, _, _, cx| {
                crate::views::process::stop_run(&this.services, &run, cx);
                cx.notify();
            })
        });
        Some([
            div()
                .flex_none()
                .text_color(colors.text_3)
                .child("·")
                .into_any_element(),
            button.into_any_element(),
        ])
    }

    /// One 28 pt pending row.
    fn live_row(
        &self,
        run: &BotRunSummary,
        now: Millis,
        reduce: bool,
        leaving: bool,
        cx: &mut Context<Self>,
    ) -> gpui::Div {
        let stop = self.stop_control(run, leaving, cx);
        let colors = theme::colors(cx);
        let bot = self.members.iter().find(|m| m.id == run.bot_id);
        let name = bot.map(|m| m.name.clone()).unwrap_or_default();
        let process = self.services.provider.run_process(&run.id);
        let label = live_label(&process.steps).unwrap_or_else(|| t::BOT_WORKING.to_string());
        div()
            .h(px(28.))
            .flex()
            .items_center()
            .gap(space::S12)
            .child(
                div()
                    .w(theme::AVATAR_SIZE)
                    .flex_none()
                    .flex()
                    .justify_center()
                    .child(ui::MemberAvatar::new(bot, px(24.)).status(CharacterStatus::Working)),
            )
            .child(
                div()
                    .min_w(px(0.))
                    .flex()
                    .items_center()
                    .gap(space::S8)
                    .t_small()
                    .text_color(colors.text_2)
                    .child(
                        div()
                            .min_w(px(0.))
                            .flex()
                            .gap(space::S4)
                            .child(
                                div()
                                    .flex_shrink(1.)
                                    .min_w(px(0.))
                                    .max_w(px(160.))
                                    .semibold()
                                    .text_color(colors.text_1)
                                    .one_line()
                                    .child(name),
                            )
                            // The dots follow the label at once (no gap when it is short).
                            .child(div().flex_shrink(1.).min_w(px(0.)).one_line().child(label)),
                    )
                    .child(pulse_dots(colors.text_3, reduce))
                    .when_some(run.started_at, |el, at| {
                        el.child(
                            div()
                                .flex_none()
                                .t_meta()
                                .tnum()
                                .text_color(colors.text_3)
                                .child(elapsed(at, now)),
                        )
                    })
                    .child(div().flex_none().text_color(colors.text_3).child("·"))
                    .child(
                        ui::text_button(
                            ElementId::Name(format!("live-process-{}", run.id).into()),
                            t::VIEW_PROCESS,
                            cx,
                        )
                        .ml(-space::S6)
                        .on_click(cx.listener(|this, _, _, cx| this.open_process(cx))),
                    )
                    .children(stop.into_iter().flatten()),
            )
    }

    /// The pending rows (§9.2), at most three, then one line for the rest,
    /// then the rows of runs that just ended, fading out.
    pub(super) fn render_live(
        &self,
        window: &mut gpui::Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let runs = self.working_runs();
        let reduce = cx.reduce_motion();
        let leaving: Vec<&(BotRunSummary, Instant)> = self
            .live_leaving
            .iter()
            .filter(|(r, at)| at.elapsed() < leave_time() && !runs.iter().any(|w| w.id == r.id))
            .collect();
        if runs.is_empty() && leaving.is_empty() {
            return None;
        }
        let colors = theme::colors(cx);
        let now = crate::selftest::clock::utc_now().timestamp_millis();
        let mut rows: Vec<AnyElement> = Vec::new();
        for run in runs.iter().take(MAX_LIVE) {
            let row = self.live_row(run, now, reduce, false, cx);
            // Enter: 200 ms opacity + a 6 pt rise, opacity only with reduce
            // motion (§12), and only the first time the run shows: the
            // animation state lives in the element, so a row is wrapped only
            // while its first 200 ms last, never again on a return.
            let fresh = self
                .live_entered
                .get(&run.id)
                .is_some_and(|at| at.elapsed() < LIVE_ENTER);
            rows.push(if fresh {
                ui::motion::enter(
                    ElementId::Name(format!("live-enter-{}", run.id).into()),
                    LIVE_ENTER,
                    row,
                    |el, t| el.relative().top(theme::motion::RISE * (1. - t)),
                    cx,
                )
            } else {
                row.into_any_element()
            });
        }
        let more = runs.len().saturating_sub(MAX_LIVE);
        let ease = theme::motion::out_fn();
        for (run, at) in leaving {
            // Exit: a 400 ms hold, then 160 ms opacity on the out curve; the
            // fade stays with reduce motion (§12). The opacity comes from the
            // clock, not a GPUI animation, which would snap under reduce motion.
            let t = (at.elapsed().as_secs_f32() / leave_time().as_secs_f32()).min(1.);
            let row = self.live_row(run, now, reduce, true, cx);
            rows.push(row.opacity(leave_opacity(t, &ease)).into_any_element());
            window.request_animation_frame();
        }
        Some(
            div()
                .flex_none()
                .pt(space::S4)
                .px(theme::CONTENT_GUTTER)
                .flex()
                .flex_col()
                .children(rows)
                .when(more > 0, |el| {
                    el.child(
                        div()
                            .h(px(24.))
                            .pl(theme::AVATAR_SIZE + space::S12)
                            .flex()
                            .items_center()
                            .t_small()
                            .text_color(colors.text_2)
                            .child(fill(t::MORE_WORKING, &[("n", &more.to_string())])),
                    )
                })
                .into_any_element(),
        )
    }

    /// Opens the 过程 tab of the side panel.
    pub(super) fn open_process(&mut self, cx: &mut Context<Self>) {
        self.services.nav.update(cx, |nav, cx| {
            nav.set_side_panel_tab(1, cx);
            if !nav.side_panel_open {
                nav.toggle_side_panel(cx);
            }
        });
    }

    /// Repaints once a second while a bot works, for the elapsed time.
    pub(super) fn keep_ticking(&mut self, cx: &mut Context<Self>) {
        if self.ticking || self.working_runs().is_empty() {
            return;
        }
        self.ticking = true;
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(Duration::from_secs(1)).await;
                let go_on = this
                    .update(cx, |this, cx| {
                        let go_on = !this.working_runs().is_empty();
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

    /// Who is typing, people only: `李雪 正在输入…`,
    /// `李雪、陈晨 正在输入…`, `李雪、陈晨 和另外 3 人正在输入…`.
    pub(super) fn typing_text(&self) -> Option<String> {
        let names: Vec<String> = self
            .typing
            .iter()
            .filter(|id| Some(id.as_str()) != self.me.as_deref())
            .filter_map(|id| self.members.iter().find(|m| &m.id == id))
            .filter(|m| !m.is_bot())
            // Short names only: an email or a long name never shows in full.
            .map(|m| crate::ui::emoji::short_name(&m.name))
            .collect();
        match names.as_slice() {
            [] => None,
            [one] => Some(fill(t::TYPING_ONE, &[("name", one)])),
            [a, b] => Some(fill(t::TYPING_TWO, &[("a", a), ("b", b)])),
            [a, b, rest @ ..] => Some(fill(
                t::TYPING_MANY,
                &[("a", a), ("b", b), ("n", &rest.len().to_string())],
            )),
        }
    }

    /// The typing line: 12/20 `text-3`, aligned with the composer's text,
    /// 4 pt above the composer box. No space when nobody types (§7.2).
    pub(super) fn render_typing(&self, cx: &mut Context<Self>) -> Option<impl IntoElement> {
        let text = self.typing_text()?;
        let colors = theme::colors(cx);
        Some(
            div()
                .flex_none()
                .h(px(20.))
                .mt(space::S4)
                .mb(-space::S4)
                .pl(theme::CONTENT_GUTTER + space::S14)
                .pr(theme::CONTENT_GUTTER)
                .flex()
                .items_center()
                .t_meta()
                .line_height(px(20.))
                .text_color(colors.text_3)
                .child(
                    div()
                        .min_w(px(0.))
                        .one_line()
                        .child(SharedString::from(text)),
                ),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_leaving_row_holds_then_fades() {
        let linear = |t: f32| t;
        assert_eq!(leave_time(), Duration::from_millis(560));
        assert_eq!(leave_opacity(0., &linear), 1.);
        assert_eq!(leave_opacity(0.7, &linear), 1.);
        assert!((leave_opacity(0.85, &linear) - 0.525).abs() < 0.01);
        assert_eq!(leave_opacity(1., &linear), 0.);
    }

    #[test]
    fn durations_read_as_minutes_and_seconds() {
        assert_eq!(duration_text(252_000), "4 分 12 秒");
        assert_eq!(duration_text(12_400), "12 秒");
        assert_eq!(elapsed(0, 134_000), "2:14");
        assert_eq!(elapsed(0, 3_599_000), "59:59");
        assert_eq!(elapsed(0, 10_680_000), "2:58:00");
    }

    #[test]
    fn steps_summary_counts_files_commands_and_failures() {
        let tool = |ok| ProcessStep::Tool {
            title: "x".into(),
            input: None,
            ok: Some(ok),
            output: None,
            exit_code: None,
        };
        let files = ProcessStep::Files {
            changes: vec![
                crate::data::FileChangeView {
                    path: "a".into(),
                    kind: crate::data::FileChangeKind::Modified,
                };
                6
            ],
        };
        let (text, failed) = steps_summary(&[files, tool(true), tool(true), tool(false)]);
        assert_eq!(text, "改了 6 个文件，运行 3 条命令");
        assert_eq!(failed, 1);
    }
}
