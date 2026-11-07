//! One element per process step, in product words.

use gpui::{
    AnyElement, App, ElementId, Hsla, InteractiveElement as _, IntoElement, ParentElement,
    StatefulInteractiveElement as _, Styled, Window, div, prelude::FluentBuilder as _, px,
};

use crate::{
    copy::{dev, fill, process as p},
    data::{FileChangeKind, ProcessStep, RunProcess, RunStatus},
    theme::{self, Type as _, space},
    ui::{self, Sym},
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
    window: &mut Window,
    cx: &mut App,
) -> Vec<AnyElement> {
    let start = process.steps.len().saturating_sub(MAX_STEPS);
    // One failure row (A5): when the run ended failed, a failed turn before
    // it would state the cause a second time.
    let ended_with_error = process.steps.iter().any(|s| {
        matches!(
            s,
            ProcessStep::Done { error: Some(_), .. }
                | ProcessStep::Done {
                    status: RunStatus::Failed | RunStatus::Blocked,
                    ..
                }
        )
    });
    process.steps[start..]
        .iter()
        .enumerate()
        .filter(|(_, step)| {
            !(ended_with_error && matches!(step, ProcessStep::TurnCompleted { ok: false, .. }))
        })
        // R2-07, §2.15: a failed end is the pane's failure Alert (with 重试
        // and 查看日志), not a step row.
        .filter(|(_, step)| !failed_end(step))
        .filter_map(|(i, step)| render_step(start + i, step, bot_name, developer, window, cx))
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

/// Command lines and output, in the thread's code recipe (§2.17, R2-07):
/// `code` 13/20 Menlo, a 1 px border, radius 8, px 12, and lines that never
/// wrap mid-token: a long line scrolls sideways inside the box. Explicit
/// line breaks stay. `id` is unique within the step list.
fn mono_box(id: ElementId, text: String, _window: &mut Window, cx: &mut App) -> impl IntoElement {
    let k = theme::colors(cx);
    let n = crate::ui::native::native_colors(cx);
    // The thread's scroll cue (R1-04): a 16 fade into the box fill at the
    // right edge, over the 16 right padding, so a line that fits ends
    // before it and a longer one reads as cut by a scroll edge, not a clip.
    let fade = div()
        .absolute()
        .top_0()
        .bottom_0()
        .right_0()
        .w(space::S16)
        .bg(gpui::linear_gradient(
            90.,
            gpui::linear_color_stop(k.code_bg.opacity(0.), 0.),
            gpui::linear_color_stop(k.code_bg, 1.),
        ));
    div()
        .mt(space::S2)
        .relative()
        .w_full()
        .min_w(px(0.))
        .rounded(theme::radius::MD)
        .border_1()
        .border_color(n.separator)
        .bg(k.code_bg)
        .overflow_hidden()
        .child(
            div().id(id).w_full().overflow_x_scroll().child(
                div()
                    .pl(space::S12)
                    .pr(space::S16)
                    .py(space::S8)
                    .t_code()
                    .whitespace_nowrap()
                    .text_color(k.text_1)
                    .children(
                        text.lines()
                            .map(|l| div().child(l.to_string()))
                            .collect::<Vec<_>>(),
                    ),
            ),
        )
        .child(fade)
}

/// Where a path is cut: the folders before the last two parts, and those
/// parts (`apps/desktop/src/` and `views/login.rs`). A path of one or two
/// parts has no head.
fn split_path(path: &str) -> (&str, &str) {
    let trimmed = path.trim_end_matches('/');
    let mut cuts = trimmed.rmatch_indices('/').map(|(i, _)| i);
    match (cuts.next(), cuts.next()) {
        (Some(_), Some(i)) => path.split_at(i + 1),
        _ => ("", path),
    }
}

/// A file path on one line, cut in the middle so the file name and its
/// folder stay (`apps/desktop…/views/login.rs`); the tooltip has the whole
/// path only when it is cut (§15). Style the returned box (type, color).
fn path_line(id: ElementId, path: &str) -> gpui::Stateful<gpui::Div> {
    let (head, tail) = split_path(path);
    let line = ui::truncating(id, path.to_string());
    if head.is_empty() {
        return line.text_ellipsis_middle();
    }
    // An end cut whose ellipsis carries the last two parts: the kept start
    // is always inside the folders, and the line stays one text (no gap
    // between two cut boxes).
    line.text_overflow(gpui::TextOverflow::Truncate(
        format!("…/{}", tail.trim_start_matches('/')).into(),
    ))
}

/// The id of the `slot`th box of step `ix`.
fn mono_id(ix: usize, slot: usize) -> ElementId {
    ElementId::NamedInteger("process-mono".into(), (ix * 4 + slot) as u64)
}

/// The 16 pt glyph of a step (§17 U31: steps with icons).
pub fn glyph(step: &ProcessStep, cx: &App) -> (Sym, Hsla) {
    let k = theme::colors(cx);
    match step {
        ProcessStep::Message { from_bot: true, .. } => (Sym::AiMember, k.text_3),
        ProcessStep::Message { .. } => (Sym::Person, k.text_3),
        ProcessStep::Reasoning { .. } => (Sym::Activity, k.text_3),
        ProcessStep::Tool { ok: None, .. } => (Sym::Terminal, k.accent_text),
        ProcessStep::Tool { ok: Some(true), .. } => (Sym::Terminal, k.text_3),
        ProcessStep::Tool {
            ok: Some(false), ..
        } => (Sym::Terminal, k.danger),
        ProcessStep::Files { .. } => (Sym::CodeFile, k.text_3),
        ProcessStep::Approval { approved: None, .. } => (Sym::Help, k.you_glyph),
        ProcessStep::Approval { .. } => (Sym::Done, k.text_3),
        ProcessStep::TurnStarted => (Sym::Live, k.text_3),
        ProcessStep::TurnCompleted { ok: true, .. } => (Sym::Done, k.success),
        ProcessStep::TurnCompleted { .. } => (Sym::Failed, k.danger),
        ProcessStep::Status { .. } => (Sym::Info, k.text_3),
        ProcessStep::Done { status, error, .. }
            if error.is_some() || matches!(status, RunStatus::Failed | RunStatus::Blocked) =>
        {
            (Sym::Alert, k.danger)
        }
        ProcessStep::Done {
            status: RunStatus::Cancelled,
            ..
        } => (Sym::Failed, k.text_3),
        ProcessStep::Done { .. } => (Sym::Done, k.success),
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
/// The run ended failed or stuck.
pub fn failed_end(step: &ProcessStep) -> bool {
    matches!(
        step,
        ProcessStep::Done {
            status: RunStatus::Failed | RunStatus::Blocked,
            ..
        }
    )
}

/// What the member is doing now, in product words, from its newest step: the live line of a
/// working card that has no plan step (`Jojo 正在做：读网页 claude.dev`). Tool names stay out
/// of it (developer words).
pub fn live_action(steps: &[ProcessStep]) -> Option<String> {
    steps.iter().rev().find_map(|s| match s {
        ProcessStep::Tool { title, .. } => Some(tool_action(title)),
        ProcessStep::Files { changes } if !changes.is_empty() => {
            Some(fill(p::STEP_FILES, &[("n", &changes.len().to_string())]))
        }
        ProcessStep::Reasoning { .. } => Some(p::ACT_THINKING.to_string()),
        ProcessStep::Message {
            from_bot: true,
            text,
            ..
        } if !text.trim().is_empty() => Some(p::ACT_REPLY.to_string()),
        ProcessStep::Approval { approved: None, .. } => Some(p::ACT_WAITING.to_string()),
        ProcessStep::TurnStarted => Some(p::ACT_START.to_string()),
        _ => None,
    })
}

/// A tool call's title (`WebFetch: https://…`, `Write: /work/index.html`, a bare command) in
/// product words.
fn tool_action(title: &str) -> String {
    let (name, arg) = match title.split_once(": ") {
        Some((n, a)) if !n.trim().is_empty() && !n.contains(' ') => (n.trim(), Some(a.trim())),
        _ => (title.trim(), None),
    };
    let file = |path: &str| {
        path.trim_end_matches('/')
            .rsplit('/')
            .next()
            .map(|n| clip(n, 40))
            .unwrap_or_default()
    };
    let host = |url: &str| {
        let rest = url.split_once("://").map_or(url, |(_, r)| r);
        clip(rest.split(['/', '?', '#']).next().unwrap_or(rest), 40)
    };
    match (name, arg) {
        ("WebFetch", Some(a)) => fill(p::ACT_READ_PAGE, &[("host", &host(a))]),
        ("WebFetch", None) => p::ACT_READ_PAGE_BARE.to_string(),
        ("WebSearch", Some(a)) => fill(p::ACT_SEARCH, &[("query", &clip(a, 30))]),
        ("Read" | "NotebookRead", Some(a)) => fill(p::ACT_READ_FILE, &[("name", &file(a))]),
        ("Write" | "Edit" | "MultiEdit" | "NotebookEdit", Some(a)) => {
            fill(p::ACT_WRITE_FILE, &[("name", &file(a))])
        }
        ("Grep" | "Glob" | "LS", _) => p::ACT_LOOK.to_string(),
        ("Task" | "Agent", _) => p::ACT_DELEGATE.to_string(),
        ("TodoWrite", _) => p::ACT_PLAN.to_string(),
        ("Bash", _) => p::ACT_RUN.to_string(),
        _ => p::ACT_TOOL.to_string(),
    }
}

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
    ix: usize,
    step: &ProcessStep,
    bot_name: &str,
    developer: bool,
    window: &mut Window,
    cx: &mut App,
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
            // The result as a status badge: the state color stays on its
            // symbol, never on the words (D3).
            let kind = match ok {
                None => crate::ui::native::StatusKind::Working,
                Some(true) => crate::ui::native::StatusKind::Done,
                Some(false) => crate::ui::native::StatusKind::Failed,
            };
            let mut header = div()
                .flex()
                .items_center()
                .gap(space::S6)
                .t_small()
                .child(div().text_color(colors.text_2).child(p::STEP_TOOL))
                .child(crate::ui::native::status_badge(kind, result, cx));
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
                .child(mono_box(mono_id(ix, 0), clip(title, 400), window, cx))
                .when_some(input.clone().filter(|_| developer), |el, json| {
                    el.child(label(
                        fill(dev::RUN_TOOL_INPUT, &[("json", &clip(&json, 600))]),
                        cx,
                    ))
                })
                .when_some(out, |el, (head, more)| {
                    el.child(mono_box(mono_id(ix, 1), head, window, cx))
                        .when(more > 0, |el| {
                            el.child(label(fill(p::MORE_LINES, &[("n", &more.to_string())]), cx))
                        })
                })
        }
        ProcessStep::Files { changes } => {
            let rows: Vec<_> = changes
                .iter()
                .take(50)
                .enumerate()
                .map(|(row, c)| {
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
                            path_line(
                                ElementId::NamedInteger(
                                    "process-file".into(),
                                    (ix * 64 + row) as u64,
                                ),
                                &c.path,
                            )
                            .t_code()
                            .text_color(colors.text_1),
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
                    el.child(mono_box(mono_id(ix, 2), clip(&d, 1200), window, cx))
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
                            .text_color(colors.text_1)
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
            duration_ms: _,
        } => block(cx)
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(space::S8)
                    .t_ui()
                    .medium()
                    .text_color(colors.text_1)
                    // The time is the 用时 fact above (A5): said once.
                    .child(fill(p::STEP_DONE, &[("status", status_text(*status))])),
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
                        .text_color(colors.text_1)
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
    fn the_live_action_names_the_newest_step_in_product_words() {
        let tool = |title: &str| ProcessStep::Tool {
            title: title.into(),
            input: None,
            ok: None,
            output: None,
            exit_code: None,
        };
        let say = |t: &[ProcessStep]| live_action(t);
        assert_eq!(
            say(&[tool(
                "WebFetch: https://claude.dev/blog/solving-code-review/"
            )])
            .as_deref(),
            Some("读网页 claude.dev")
        );
        assert_eq!(
            say(&[tool("WebFetch: x"), tool("Write: /work/out/index.html")]).as_deref(),
            Some("写 index.html")
        );
        assert_eq!(
            say(&[tool("Bash: curl -s https://x")]).as_deref(),
            Some("运行命令")
        );
        assert_eq!(
            say(&[tool("npm run build")]).as_deref(),
            Some("执行一步操作")
        );
        assert_eq!(
            say(&[
                tool("Read: /a/b.md"),
                ProcessStep::Status {
                    message: "x".into()
                }
            ])
            .as_deref(),
            Some("读 b.md")
        );
        assert_eq!(say(&[]), None);
    }

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

    #[test]
    fn a_path_is_cut_before_its_last_two_parts() {
        assert_eq!(
            split_path("apps/desktop/src/views/login.rs"),
            ("apps/desktop/src/", "views/login.rs")
        );
        assert_eq!(split_path("src/login.rs"), ("", "src/login.rs"));
        assert_eq!(split_path("login.rs"), ("", "login.rs"));
        assert_eq!(split_path("a/b/c/"), ("a/", "b/c/"));
    }
}
