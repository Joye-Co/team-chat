//! One element per process step, in product words.

use gpui::{
    AnyElement, App, Hsla, IntoElement, ParentElement, Styled, div, prelude::FluentBuilder as _, px,
};

use crate::{
    copy::{dev, fill, process as p},
    data::{FileChangeKind, ProcessStep, RunProcess, RunStatus},
    theme::{self, Type as _, space},
    ui::{self, Lucide},
};

use super::status_text;

/// Steps shown at most; older ones are left out (the developer view and the
/// raw session keep everything).
const MAX_STEPS: usize = 300;
/// Characters of one text shown at most.
const MAX_TEXT: usize = 4000;
/// Lines of a command's output shown.
const OUTPUT_LINES: usize = 12;

/// Cuts `text` to `max` characters.
pub fn clip(text: &str, max: usize) -> String {
    crate::markdown::clip(text, max)
}

/// The first `n` lines and how many lines were left out.
pub fn head_lines(text: &str, n: usize) -> (String, usize) {
    let lines: Vec<&str> = text.trim_end().lines().collect();
    if lines.len() <= n {
        (lines.join("\n"), 0)
    } else {
        (lines[..n].join("\n"), lines.len() - n)
    }
}

pub fn render_steps(
    process: &RunProcess,
    bot_name: &str,
    developer: bool,
    cx: &App,
) -> Vec<AnyElement> {
    let start = process.steps.len().saturating_sub(MAX_STEPS);
    process.steps[start..]
        .iter()
        .filter_map(|step| render_step(step, bot_name, developer, cx))
        .collect()
}

/// The step's kind and state: `small` 13/20 `text-2`.
fn label(text: String, cx: &App) -> impl IntoElement {
    div()
        .t_small()
        .text_color(theme::colors(cx).text_2)
        .child(text)
}

/// The content column of one step.
fn block(_cx: &App) -> gpui::Div {
    div()
        .flex_1()
        .min_w(px(0.))
        .flex()
        .flex_col()
        .gap(space::S4)
}

/// Command lines and output: `code` 13/20 Menlo on `code-bg`, radius 8.
fn mono_box(text: String, cx: &App) -> impl IntoElement {
    let k = theme::colors(cx);
    div()
        .mt(space::S2)
        .px(space::S10)
        .py(space::S8)
        .rounded(theme::radius::MD)
        .bg(k.code_bg)
        .t_code()
        .text_color(k.text_1)
        .child(text)
}

/// The 16 pt glyph of a step (§17 U31: steps with icons).
pub fn glyph(step: &ProcessStep, cx: &App) -> (Lucide, Hsla) {
    let k = theme::colors(cx);
    match step {
        ProcessStep::Message { from_bot: true, .. } => (Lucide::Bot, k.text_3),
        ProcessStep::Message { .. } => (Lucide::User, k.text_3),
        ProcessStep::Reasoning { .. } => (Lucide::Activity, k.text_3),
        ProcessStep::Tool { ok: None, .. } => (Lucide::Terminal, k.accent_text),
        ProcessStep::Tool { ok: Some(true), .. } => (Lucide::Terminal, k.text_3),
        ProcessStep::Tool {
            ok: Some(false), ..
        } => (Lucide::Terminal, k.danger),
        ProcessStep::Files { .. } => (Lucide::FileCode, k.text_3),
        ProcessStep::Approval { approved: None, .. } => (Lucide::CircleQuestionMark, k.you_glyph),
        ProcessStep::Approval { .. } => (Lucide::CircleCheck, k.text_3),
        ProcessStep::TurnStarted => (Lucide::CircleDot, k.text_3),
        ProcessStep::TurnCompleted { ok: true, .. } => (Lucide::CircleCheck, k.success),
        ProcessStep::TurnCompleted { .. } => (Lucide::CircleX, k.danger),
        ProcessStep::Status { .. } => (Lucide::Info, k.text_3),
        ProcessStep::Done { status, error, .. }
            if error.is_some() || matches!(status, RunStatus::Failed | RunStatus::Blocked) =>
        {
            (Lucide::CircleAlert, k.danger)
        }
        ProcessStep::Done {
            status: RunStatus::Cancelled,
            ..
        } => (Lucide::CircleX, k.text_3),
        ProcessStep::Done { .. } => (Lucide::CircleCheck, k.success),
    }
}

/// One line for a step in the side panel's progress list (§8.1): what the
/// bot did, in product words. `None` for steps with nothing to show.
pub fn step_line(step: &ProcessStep, bot_name: &str) -> Option<String> {
    let first_line = |text: &str| {
        text.lines()
            .map(str::trim)
            .find(|l| !l.is_empty())
            .map(|l| clip(l, 80))
    };
    Some(match step {
        ProcessStep::Message { text, from_bot, .. } => {
            let line = first_line(text)?;
            if *from_bot {
                fill(p::STEP_SAID_LINE, &[("name", bot_name), ("text", &line)])
            } else {
                fill(p::STEP_INPUT_LINE, &[("text", &line)])
            }
        }
        ProcessStep::Reasoning { text } => {
            first_line(text)?;
            p::STEP_THINKING.to_string()
        }
        ProcessStep::Tool { title, .. } => first_line(title)?,
        ProcessStep::Files { changes } => fill(p::STEP_FILES, &[("n", &changes.len().to_string())]),
        ProcessStep::Approval { title, .. } => {
            first_line(title).unwrap_or_else(|| p::STEP_APPROVAL.to_string())
        }
        ProcessStep::TurnStarted => p::STEP_TURN_STARTED.to_string(),
        ProcessStep::TurnCompleted { ok: true, .. } => p::STEP_TURN_DONE.to_string(),
        ProcessStep::TurnCompleted { .. } => p::STEP_TURN_FAILED.to_string(),
        ProcessStep::Status { message } => first_line(message)?,
        ProcessStep::Done { status, .. } => fill(p::STEP_DONE, &[("status", status_text(*status))]),
    })
}

/// The error line of a failed run: the last error its steps carry.
pub fn error_line(steps: &[ProcessStep]) -> Option<String> {
    steps.iter().rev().find_map(|s| match s {
        ProcessStep::Done { error: Some(e), .. }
        | ProcessStep::TurnCompleted { error: Some(e), .. } => e
            .lines()
            .map(str::trim)
            .find(|l| !l.is_empty())
            .map(|l| clip(l, 200)),
        _ => None,
    })
}

fn render_step(
    step: &ProcessStep,
    bot_name: &str,
    developer: bool,
    cx: &App,
) -> Option<AnyElement> {
    let colors = theme::colors(cx);
    let el = match step {
        ProcessStep::Message {
            text,
            from_bot,
            streaming,
        } => {
            if text.trim().is_empty() && !streaming {
                return None;
            }
            let who = if *from_bot {
                fill(p::STEP_SAID, &[("name", bot_name)])
            } else {
                p::STEP_INPUT.to_string()
            };
            block(cx)
                .child(label(who, cx))
                .child(
                    div()
                        .t_body()
                        .text_color(colors.text_1)
                        .child(clip(text, MAX_TEXT)),
                )
                .when(*streaming, |el| {
                    el.child(label(p::STEP_WRITING.to_string(), cx))
                })
        }
        ProcessStep::Reasoning { text } => {
            if text.trim().is_empty() {
                return None;
            }
            block(cx)
                .child(label(p::STEP_THINKING.to_string(), cx))
                .child(
                    div()
                        .t_small()
                        .text_color(colors.text_2)
                        .child(clip(text, 600)),
                )
        }
        ProcessStep::Tool {
            title,
            input,
            ok,
            output,
            exit_code,
        } => {
            let result = match ok {
                None => p::STEP_TOOL_RUNNING,
                Some(true) => p::STEP_TOOL_OK,
                Some(false) => p::STEP_TOOL_FAILED,
            };
            let result_color = match ok {
                None => colors.text_3,
                Some(true) => colors.success_text,
                Some(false) => colors.danger_text,
            };
            let mut header = div()
                .flex()
                .items_center()
                .gap(space::S6)
                .t_small()
                .child(div().text_color(colors.text_2).child(p::STEP_TOOL))
                .child(div().text_color(result_color).child(result));
            if let Some(code) = exit_code {
                header = header.child(
                    div()
                        .t_meta()
                        .tnum()
                        .text_color(colors.text_3)
                        .child(fill(p::STEP_EXIT_CODE, &[("n", &code.to_string())])),
                );
            }
            let out = output.as_deref().filter(|o| !o.trim().is_empty()).map(|o| {
                let (head, more) = head_lines(o, OUTPUT_LINES);
                (clip(&head, MAX_TEXT), more)
            });
            block(cx)
                .child(header)
                .child(mono_box(clip(title, 400), cx))
                .when_some(input.clone().filter(|_| developer), |el, json| {
                    el.child(label(
                        fill(dev::RUN_TOOL_INPUT, &[("json", &clip(&json, 600))]),
                        cx,
                    ))
                })
                .when_some(out, |el, (head, more)| {
                    el.child(mono_box(head, cx)).when(more > 0, |el| {
                        el.child(label(fill(p::MORE_LINES, &[("n", &more.to_string())]), cx))
                    })
                })
        }
        ProcessStep::Files { changes } => {
            let rows: Vec<_> = changes
                .iter()
                .take(50)
                .map(|c| {
                    let kind = match c.kind {
                        FileChangeKind::Added => p::FILE_ADDED,
                        FileChangeKind::Modified => p::FILE_MODIFIED,
                        FileChangeKind::Deleted => p::FILE_DELETED,
                    };
                    div()
                        .flex()
                        .gap(space::S8)
                        .child(
                            div()
                                .flex_none()
                                .t_small()
                                .text_color(colors.text_3)
                                .child(kind),
                        )
                        .child(
                            div()
                                .min_w(px(0.))
                                .t_code()
                                .text_color(colors.text_1)
                                .one_line()
                                .child(c.path.clone()),
                        )
                })
                .collect();
            block(cx)
                .child(label(
                    fill(p::STEP_FILES, &[("n", &changes.len().to_string())]),
                    cx,
                ))
                .children(rows)
        }
        ProcessStep::Approval {
            title,
            detail,
            approved,
        } => {
            let state = match approved {
                None => p::APPROVAL_WAITING,
                Some(true) => p::APPROVAL_APPROVED,
                Some(false) => p::APPROVAL_DENIED,
            };
            block(cx)
                .child(label(p::STEP_APPROVAL.to_string(), cx))
                .child(
                    div()
                        .t_body()
                        .text_color(colors.text_1)
                        .child(clip(title, 400)),
                )
                .when_some(detail.clone(), |el, d| {
                    el.child(mono_box(clip(&d, 1200), cx))
                })
                .child(
                    div()
                        .t_meta_medium()
                        .text_color(if approved.is_none() {
                            colors.you_text
                        } else {
                            colors.text_3
                        })
                        .child(state),
                )
        }
        ProcessStep::TurnStarted => block(cx).child(label(p::STEP_TURN_STARTED.to_string(), cx)),
        ProcessStep::TurnCompleted { ok, error } => {
            let text = if *ok {
                p::STEP_TURN_DONE
            } else {
                p::STEP_TURN_FAILED
            };
            block(cx)
                .child(label(text.to_string(), cx))
                .when_some(error.clone(), |el, e| {
                    el.child(
                        div()
                            .t_small()
                            .text_color(colors.danger_text)
                            .child(clip(&e, 600)),
                    )
                })
        }
        ProcessStep::Status { message } => {
            if message.trim().is_empty() {
                return None;
            }
            block(cx)
                .child(label(p::STEP_STATUS.to_string(), cx))
                .child(
                    div()
                        .t_small()
                        .text_color(colors.text_1)
                        .child(clip(message, 400)),
                )
        }
        ProcessStep::Done {
            status,
            summary,
            error,
            duration_ms,
        } => block(cx)
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(space::S8)
                    .t_ui()
                    .medium()
                    .text_color(colors.text_1)
                    .child(fill(p::STEP_DONE, &[("status", status_text(*status))]))
                    .child(div().t_meta().tnum().text_color(colors.text_3).child(fill(
                        p::DURATION,
                        &[("d", &super::duration_text(*duration_ms))],
                    ))),
            )
            .when_some(summary.clone(), |el, s| {
                el.child(
                    div()
                        .t_body()
                        .text_color(colors.text_1)
                        .child(clip(&s, MAX_TEXT)),
                )
            })
            .when_some(error.clone(), |el, e| {
                el.child(
                    div()
                        .t_small()
                        .text_color(colors.danger_text)
                        .child(clip(&e, 600)),
                )
            }),
    };
    let (g, color) = glyph(step, cx);
    Some(
        div()
            .py(space::S6)
            .flex()
            .items_start()
            .gap(space::S10)
            .child(
                div()
                    .flex_none()
                    .h(px(20.))
                    .flex()
                    .items_center()
                    .child(ui::icon(g, theme::icon::ROW, color)),
            )
            .child(el)
            .into_any_element(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn long_text_is_clipped_by_chars() {
        assert_eq!(clip("你好", 5), "你好");
        assert_eq!(clip("你好世界", 2), "你好…");
    }

    #[test]
    fn output_keeps_the_first_lines() {
        assert_eq!(head_lines("a\nb\n", 5), ("a\nb".to_string(), 0));
        let text = (1..=20)
            .map(|i| i.to_string())
            .collect::<Vec<_>>()
            .join("\n");
        let (head, more) = head_lines(&text, 12);
        assert_eq!(head.lines().count(), 12);
        assert_eq!(more, 8);
    }
}
