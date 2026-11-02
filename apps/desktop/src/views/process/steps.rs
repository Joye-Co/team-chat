//! One element per process step, in product words.

use gpui::{
    AnyElement, App, Hsla, IntoElement, ParentElement, Styled, div, prelude::FluentBuilder as _,
    px,
};
use gpui_component::ActiveTheme as _;

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
    let parts: Vec<Step> = process.steps[start..]
        .iter()
        .filter_map(|step| describe(step, bot_name, developer, cx))
        .collect();
    let last = parts.len().saturating_sub(1);
    parts
        .into_iter()
        .enumerate()
        .map(|(i, part)| part.render(i == last, cx))
        .collect()
}

/// One row of the 步骤 list (§13.7): a 16 pt glyph on a 1 pt `sep-strong`
/// connector, a 13 / 500 title, an 11 pt meta line and an optional body.
struct Step {
    glyph: Lucide,
    glyph_color: Hsla,
    title: String,
    /// Draw the title in Menlo (commands).
    mono_title: bool,
    meta: Vec<(String, Hsla)>,
    body: Vec<AnyElement>,
}

impl Step {
    fn new(glyph: Lucide, glyph_color: Hsla, title: impl Into<String>) -> Self {
        Self {
            glyph,
            glyph_color,
            title: title.into(),
            mono_title: false,
            meta: Vec::new(),
            body: Vec::new(),
        }
    }

    fn meta(mut self, text: impl Into<String>, color: Hsla) -> Self {
        self.meta.push((text.into(), color));
        self
    }

    fn body(mut self, el: impl IntoElement) -> Self {
        self.body.push(el.into_any_element());
        self
    }

    fn render(self, last: bool, cx: &App) -> AnyElement {
        let colors = theme::colors(cx);
        let rail = div()
            .w(px(16.))
            .flex_none()
            .flex()
            .flex_col()
            .items_center()
            .child(
                div()
                    .h(theme::type_scale::UI.1)
                    .flex()
                    .items_center()
                    .child(ui::icon(self.glyph, theme::icon::TOOLBAR, self.glyph_color)),
            )
            .when(!last, |el| {
                el.child(
                    div()
                        .flex_1()
                        .w(px(1.))
                        .mt(space::S2)
                        .bg(colors.sep_strong),
                )
            });
        let title = div()
            .t_ui()
            .medium()
            .text_color(colors.text_1)
            .one_line()
            .when(self.mono_title, |el| {
                el.font_family(theme::MONO_FONT_FAMILY)
                    .text_size(px(12.))
            })
            .child(self.title);
        let meta = (!self.meta.is_empty()).then(|| {
            let n = self.meta.len();
            div()
                .flex()
                .flex_wrap()
                .items_center()
                .gap_x(space::S4)
                .t_caption()
                .font_weight(gpui::FontWeight::NORMAL)
                .tnum()
                .children(self.meta.into_iter().enumerate().flat_map(|(i, (t, c))| {
                    let dot = (i + 1 < n).then(|| div().text_color(colors.text_3).child("·"));
                    std::iter::once(div().text_color(c).child(t)).chain(dot)
                }))
        });
        div()
            .flex()
            .gap(space::S10)
            .child(rail)
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.))
                    .pb(if last { space::S4 } else { space::S12 })
                    .flex()
                    .flex_col()
                    .gap(space::S2)
                    .child(title)
                    .children(meta)
                    .children(self.body),
            )
            .into_any_element()
    }
}

/// Text of a step body: 12 / 17.
fn text_body(text: String, color: Hsla) -> impl IntoElement {
    div()
        .pt(space::S2)
        .text_size(theme::type_scale::CALLOUT.0)
        .line_height(px(17.))
        .text_color(color)
        .child(text)
}

/// Command output and details: Menlo 11.5 on `code-bg`, radius 6.
fn mono_box(text: String, cx: &App) -> impl IntoElement {
    let colors = theme::colors(cx);
    div()
        .mt(space::S4)
        .px(space::S8)
        .py(space::S6)
        .rounded(theme::radius::SM)
        .bg(colors.code_bg)
        .text_color(colors.code_text)
        .font_family(cx.theme().mono_font_family.clone())
        .text_size(px(11.5))
        .line_height(px(17.))
        .child(text)
}

fn describe(step: &ProcessStep, bot_name: &str, developer: bool, cx: &App) -> Option<Step> {
    let k = theme::colors(cx);
    let done = |ok: Option<bool>| match ok {
        None => (Lucide::Loader, k.accent),
        Some(true) => (Lucide::CircleCheck, k.success),
        Some(false) => (Lucide::CircleAlert, k.danger),
    };
    let step = match step {
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
            let (glyph, color) = if *streaming {
                (Lucide::Loader, k.accent)
            } else {
                (Lucide::CircleCheck, k.success)
            };
            let step = Step::new(glyph, color, who);
            let step = if *streaming {
                step.meta(p::STEP_WRITING, k.text_2)
            } else {
                step
            };
            step.body(text_body(clip(text, MAX_TEXT), k.text_1))
        }
        ProcessStep::Reasoning { text } => {
            if text.trim().is_empty() {
                return None;
            }
            Step::new(Lucide::CircleDashed, k.text_3, p::STEP_THINKING)
                .body(text_body(clip(text, 600), k.text_2))
        }
        ProcessStep::Tool {
            title,
            input,
            ok,
            output,
            exit_code,
        } => {
            let (glyph, color) = done(*ok);
            let result = match ok {
                None => p::STEP_TOOL_RUNNING,
                Some(true) => p::STEP_TOOL_OK,
                Some(false) => p::STEP_TOOL_FAILED,
            };
            let mut step = Step::new(glyph, color, clip(title, 400))
                .meta(p::STEP_TOOL, k.text_2)
                .meta(
                    result,
                    if *ok == Some(false) {
                        k.danger_text
                    } else {
                        k.text_2
                    },
                );
            step.mono_title = true;
            if let Some(code) = exit_code {
                step = step.meta(fill(p::STEP_EXIT_CODE, &[("n", &code.to_string())]), k.text_2);
            }
            if developer && let Some(json) = input.clone() {
                step = step.body(text_body(
                    fill(dev::RUN_TOOL_INPUT, &[("json", &clip(&json, 600))]),
                    k.text_2,
                ));
            }
            if let Some(o) = output.as_deref().filter(|o| !o.trim().is_empty()) {
                let (head, more) = head_lines(o, OUTPUT_LINES);
                step = step.body(mono_box(clip(&head, MAX_TEXT), cx));
                if more > 0 {
                    step = step.body(text_body(
                        fill(p::MORE_LINES, &[("n", &more.to_string())]),
                        k.text_3,
                    ));
                }
            }
            step
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
                        .gap(space::S6)
                        .t_caption()
                        .font_weight(gpui::FontWeight::NORMAL)
                        .child(div().flex_none().text_color(k.text_2).child(kind))
                        .child(
                            div()
                                .min_w(px(0.))
                                .overflow_hidden()
                                .whitespace_nowrap()
                                .text_ellipsis_start()
                                .text_color(k.text_1)
                                .font_family(cx.theme().mono_font_family.clone())
                                .child(c.path.clone()),
                        )
                })
                .collect();
            let mut step = Step::new(
                Lucide::FileCode,
                k.text_2,
                fill(p::STEP_FILES, &[("n", &changes.len().to_string())]),
            );
            for r in rows {
                step = step.body(r);
            }
            step
        }
        ProcessStep::Approval {
            title,
            detail,
            approved,
        } => {
            let (glyph, color, state, state_color) = match approved {
                None => (Lucide::Eye, k.glyph_wait, p::APPROVAL_WAITING, k.text_2),
                Some(true) => (Lucide::CircleCheck, k.success, p::APPROVAL_APPROVED, k.text_2),
                Some(false) => (Lucide::CircleAlert, k.danger, p::APPROVAL_DENIED, k.danger_text),
            };
            let mut step = Step::new(glyph, color, clip(title, 400))
                .meta(p::STEP_APPROVAL, k.text_2)
                .meta(state, state_color);
            if let Some(d) = detail.clone() {
                step = step.body(mono_box(clip(&d, 1200), cx));
            }
            step
        }
        ProcessStep::TurnStarted => Step::new(Lucide::Circle, k.text_3, p::STEP_TURN_STARTED),
        ProcessStep::TurnCompleted { ok, error } => {
            let (glyph, color) = done(Some(*ok));
            let text = if *ok {
                p::STEP_TURN_DONE
            } else {
                p::STEP_TURN_FAILED
            };
            let step = Step::new(glyph, color, text);
            match error.clone() {
                Some(e) => step.body(text_body(clip(&e, 600), k.danger_text)),
                None => step,
            }
        }
        ProcessStep::Status { message } => {
            if message.trim().is_empty() {
                return None;
            }
            Step::new(Lucide::CircleDot, k.text_2, p::STEP_STATUS)
                .body(text_body(clip(message, 400), k.text_1))
        }
        ProcessStep::Done {
            status,
            summary,
            error,
            duration_ms,
        } => {
            let failed = matches!(status, RunStatus::Failed | RunStatus::Blocked);
            let (glyph, color) = if failed {
                (Lucide::CircleAlert, k.danger)
            } else {
                (Lucide::CircleCheck, k.success)
            };
            let mut step = Step::new(
                glyph,
                color,
                fill(p::STEP_DONE, &[("status", status_text(*status))]),
            )
            .meta(
                fill(p::DURATION, &[("d", &super::duration_text(*duration_ms))]),
                k.text_2,
            );
            if let Some(s) = summary.clone() {
                step = step.body(text_body(clip(&s, MAX_TEXT), k.text_1));
            }
            if let Some(e) = error.clone() {
                step = step.body(text_body(clip(&e, 600), k.danger_text));
            }
            step
        }
    };
    Some(step)
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
