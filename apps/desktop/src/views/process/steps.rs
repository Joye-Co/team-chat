//! One element per process step, in product words.

use gpui::{
    AnyElement, App, IntoElement, ParentElement, Styled, div, prelude::FluentBuilder as _, px,
};
use gpui_component::ActiveTheme as _;

use crate::{
    copy::{dev, fill, process as p},
    data::{FileChangeKind, ProcessStep, RunProcess},
    theme,
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
    if text.chars().count() <= max {
        text.to_string()
    } else {
        let mut out: String = text.chars().take(max).collect();
        out.push('…');
        out
    }
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

fn label(text: String, cx: &App) -> impl IntoElement {
    div()
        .text_xs()
        .text_color(theme::colors(cx).muted_foreground)
        .child(text)
}

fn block(cx: &App) -> gpui::Div {
    div()
        .py(px(6.))
        .flex()
        .flex_col()
        .gap(px(2.))
        .border_b_1()
        .border_color(theme::colors(cx).border)
}

fn mono_box(text: String, cx: &App) -> impl IntoElement {
    let colors = theme::colors(cx);
    div()
        .mt(px(2.))
        .px(px(6.))
        .py(px(4.))
        .rounded(px(4.))
        .bg(colors.code_background)
        .font_family(cx.theme().mono_font_family.clone())
        .text_xs()
        .child(text)
}

fn render_step(
    step: &ProcessStep,
    bot_name: &str,
    developer: bool,
    cx: &App,
) -> Option<AnyElement> {
    let colors = theme::colors(cx);
    let el =
        match step {
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
                    .child(div().text_sm().child(clip(text, MAX_TEXT)))
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
                            .text_xs()
                            .text_color(colors.muted_foreground)
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
                    None => colors.muted_foreground,
                    Some(true) => cx.theme().green,
                    Some(false) => colors.danger,
                };
                let mut header = div()
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .text_xs()
                    .child(
                        div()
                            .text_color(colors.muted_foreground)
                            .child(p::STEP_TOOL),
                    )
                    .child(div().text_color(result_color).child(result));
                if let Some(code) = exit_code {
                    header = header.child(
                        div()
                            .text_color(colors.muted_foreground)
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
                            .gap(px(6.))
                            .text_xs()
                            .child(
                                div()
                                    .flex_none()
                                    .text_color(colors.muted_foreground)
                                    .child(kind),
                            )
                            .child(
                                div()
                                    .min_w(px(0.))
                                    .overflow_hidden()
                                    .text_ellipsis()
                                    .font_family(cx.theme().mono_font_family.clone())
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
                    .child(div().text_sm().child(clip(title, 400)))
                    .when_some(detail.clone(), |el, d| {
                        el.child(mono_box(clip(&d, 1200), cx))
                    })
                    .child(
                        div()
                            .text_xs()
                            .text_color(if approved.is_none() {
                                cx.theme().yellow
                            } else {
                                colors.muted_foreground
                            })
                            .child(state),
                    )
            }
            ProcessStep::TurnStarted => {
                block(cx).child(label(p::STEP_TURN_STARTED.to_string(), cx))
            }
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
                                .text_xs()
                                .text_color(colors.danger)
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
                    .child(div().text_xs().child(clip(message, 400)))
            }
            ProcessStep::Done {
                status,
                summary,
                error,
                duration_ms,
            } => {
                block(cx)
                    .child(
                        div()
                            .flex()
                            .gap(px(6.))
                            .text_sm()
                            .child(fill(p::STEP_DONE, &[("status", status_text(*status))]))
                            .child(div().text_xs().text_color(colors.muted_foreground).child(
                                fill(p::DURATION, &[("d", &super::duration_text(*duration_ms))]),
                            )),
                    )
                    .when_some(summary.clone(), |el, s| {
                        el.child(div().text_sm().child(clip(&s, MAX_TEXT)))
                    })
                    .when_some(error.clone(), |el, e| {
                        el.child(
                            div()
                                .text_xs()
                                .text_color(colors.danger)
                                .child(clip(&e, 600)),
                        )
                    })
            }
        };
    Some(el.into_any_element())
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
