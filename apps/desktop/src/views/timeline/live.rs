//! Bots at work and people typing (DESIGN.md §9.2, §7.2):
//!
//! - the pending row: one 28 pt row per bot that works and has not posted
//!   yet, 12 pt under the last message (the working character, the bot's
//!   latest step, three pulsing dots, the elapsed time and `查看过程`);
//! - the steps row: one collapsed 24 pt row above a reply that came out of a
//!   run (`改了 6 个文件，运行 3 条命令 · 4 分 12 秒`), which opens the
//!   steps inline;
//! - the typing line under the timeline, for people only.

use std::time::Duration;

use chrono::Utc;
use gpui::{
    AnimationExt as _, AnyElement, Context, ElementId, InteractiveElement as _, IntoElement,
    ParentElement, SharedString, StatefulInteractiveElement as _, Styled, div,
    prelude::FluentBuilder as _, px,
};

use super::TimelineView;
use crate::{
    characters::CharacterStatus,
    copy::{fill, timeline as t},
    data::{BotRunSummary, Message, Millis, ProcessStep, RunStatus},
    theme::{self, Type as _, space},
    ui::{self, Lucide},
};

/// Most pending rows; more working bots fold into one line.
const MAX_LIVE: usize = 3;
/// Most steps an opened steps row lists.
const MAX_STEPS: usize = 40;
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
                Animation::new(theme::motion::TYPING_PULSE).repeat(),
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
fn live_label(steps: &[ProcessStep]) -> Option<String> {
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
            .cursor_pointer()
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

    /// The pending rows (§9.2), at most three, then one line for the rest.
    pub(super) fn render_live(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let runs = self.working_runs();
        if runs.is_empty() {
            return None;
        }
        let colors = theme::colors(cx);
        let now = Utc::now().timestamp_millis();
        let reduce = cx.reduce_motion();
        let rows = runs.iter().take(MAX_LIVE).map(|run| {
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
                        .child(
                            ui::MemberAvatar::new(bot, px(24.)).status(CharacterStatus::Working),
                        ),
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
                        ),
                )
                .with_animation(
                    ElementId::Name(format!("live-enter-{}", run.id).into()),
                    // Enter: 200 ms opacity + a 6 pt rise; opacity only
                    // with reduce motion (§12).
                    gpui::Animation::new(theme::motion::SLOW).with_easing(theme::motion::out_fn()),
                    move |el, t| {
                        let el = el.opacity(t);
                        if reduce {
                            el
                        } else {
                            el.relative().top(theme::motion::RISE * (1. - t))
                        }
                    },
                )
                .into_any_element()
        });
        let more = runs.len().saturating_sub(MAX_LIVE);
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
