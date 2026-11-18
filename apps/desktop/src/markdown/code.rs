//! Code fences of a body: the language name in the block's strip and the
//! highlighting, with the app's own tokenizer (`views::media::text`), the one
//! a code 产出 uses. Monochrome like there: keywords 600, strings and numbers
//! `text-2`, comments `text-3` italic, so status colors keep their meaning.
//! The `+` and `-` lines of a `diff` fence are tinted across the block
//! (`markdown::diff_lines`).

use std::{cell::RefCell, ops::Range, sync::Arc};

use gpui::{App, HighlightStyle, Hsla};
use gpui_base::text::CodeBlock;

use crate::{
    theme,
    views::media::text::{self, Tok},
};

/// What gpui-component's text view takes as a code block highlighter.
pub type Highlighter = Arc<dyn Fn(&CodeBlock) -> Vec<(Range<usize>, HighlightStyle)> + Send + Sync>;

/// How a run of code is drawn.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Span {
    Keyword,
    /// Strings and numbers.
    Value,
    Comment,
    /// A `+` line of a diff.
    Added,
    /// A `-` line of a diff.
    Removed,
    /// A file or hunk header of a diff.
    Meta,
}

fn is_diff(info: &str) -> bool {
    matches!(
        info.trim().to_ascii_lowercase().as_str(),
        "diff" | "patch" | "udiff"
    )
}

/// The name of the language of a fence, for the block's strip. `None` for a
/// fence without a known language: it shows no name.
pub fn label(info: &str) -> Option<&'static str> {
    if is_diff(info) {
        return Some("Diff");
    }
    if crate::diagrams::is_diagram_info(info) {
        // A mermaid fence shown as code (a fifth diagram, or a streaming body).
        return Some("Mermaid");
    }
    text::language_of_info(info)
        .map(|(name, _)| name)
        .filter(|name| *name != "Text")
}

/// The runs of `code` to draw differently from plain code, as byte ranges in
/// source order. A fence without a known language has none.
pub fn spans(code: &str, info: &str) -> Vec<(Range<usize>, Span)> {
    let mut out = Vec::new();
    if is_diff(info) {
        let mut at = 0;
        for line in code.split_inclusive('\n') {
            let text = line.trim_end_matches(['\n', '\r']);
            let header = ["+++", "---", "@@", "diff "]
                .iter()
                .any(|mark| text.starts_with(mark));
            let span = if header {
                Some(Span::Meta)
            } else if text.starts_with('+') {
                Some(Span::Added)
            } else if text.starts_with('-') {
                Some(Span::Removed)
            } else {
                None
            };
            if let Some(span) = span.filter(|_| !text.is_empty()) {
                out.push((at..at + text.len(), span));
            }
            at += line.len();
        }
        return out;
    }
    let Some((_, lang)) = text::language_of_info(info) else {
        return out;
    };
    let mut block = false;
    let mut at = 0;
    for line in code.split_inclusive('\n') {
        let text = line.trim_end_matches(['\n', '\r']);
        let mut pos = at;
        for (tok, s) in text::highlight(text, lang, &mut block) {
            let range = pos..pos + s.len();
            pos = range.end;
            let span = match tok {
                Tok::Keyword => Span::Keyword,
                Tok::Str | Tok::Num => Span::Value,
                Tok::Comment => Span::Comment,
                Tok::Plain => continue,
            };
            out.push((range, span));
        }
        at += line.len();
    }
    out
}

/// The colors of the spans in one mode.
#[derive(Clone, Copy, PartialEq)]
struct Palette {
    value: Hsla,
    comment: Hsla,
}

impl Palette {
    fn style(&self, span: Span) -> HighlightStyle {
        match span {
            Span::Keyword => HighlightStyle {
                font_weight: Some(gpui::FontWeight::SEMIBOLD),
                ..Default::default()
            },
            Span::Value => HighlightStyle {
                color: Some(self.value),
                ..Default::default()
            },
            Span::Comment => HighlightStyle {
                color: Some(self.comment),
                font_style: Some(gpui::FontStyle::Italic),
                ..Default::default()
            },
            // Their tint is the whole line's, drawn behind the block's width.
            Span::Added | Span::Removed => HighlightStyle::default(),
            Span::Meta => HighlightStyle {
                color: Some(self.comment),
                ..Default::default()
            },
        }
    }
}

thread_local! {
    /// The highlighter of the current colors. A code block keeps its
    /// highlights only while it is handed the same `Arc`, so one is kept per
    /// palette instead of one per frame.
    static CURRENT: RefCell<Option<(Palette, Highlighter)>> = const { RefCell::new(None) };
}

impl Palette {
    fn of(k: &theme::Colors) -> Self {
        Self {
            value: k.text_2,
            comment: k.text_3,
        }
    }

    fn highlighter(self) -> Highlighter {
        Arc::new(move |block: &CodeBlock| {
            let info = block.lang().unwrap_or_default();
            spans(&block.code(), &info)
                .into_iter()
                .map(|(range, span)| (range, self.style(span)))
                .collect()
        })
    }
}

/// The highlighter of the colors last seen by [`sync_colors`] (the light ones
/// before any code block was drawn). A body is built without the app, so the
/// colors come from the code blocks on screen.
pub fn highlighter() -> Highlighter {
    CURRENT.with(|current| {
        current
            .borrow_mut()
            .get_or_insert_with(|| {
                let palette = Palette::of(&theme::colors_for(false, false));
                (palette, palette.highlighter())
            })
            .1
            .clone()
    })
}

/// The lines of a diff fence that are tinted: their index, and whether it is added.
pub fn diff_lines(code: &str, info: &str) -> Vec<(usize, bool)> {
    if !is_diff(info) {
        return Vec::new();
    }
    let spans = spans(code, info);
    let mut at = 0;
    let mut out = Vec::new();
    for (i, line) in code.split_inclusive('\n').enumerate() {
        match spans.iter().find(|(r, _)| r.start == at) {
            Some((_, Span::Added)) => out.push((i, true)),
            Some((_, Span::Removed)) => out.push((i, false)),
            _ => {}
        }
        at += line.len();
    }
    out
}

/// The app's highlighter with the find bar's matches on top: `marks` are the fences that
/// hold one, each match in `ink` on its own fill.
pub fn marked_highlighter(
    marks: Vec<crate::views::timeline::find::FenceMarks>,
    ink: Hsla,
) -> Highlighter {
    let base = highlighter();
    Arc::new(move |block: &CodeBlock| {
        let styles = base(block);
        let code = block.code();
        let code = code.trim_end_matches(['\n', '\r']);
        match marks.iter().find(|(fence, _)| fence == code) {
            Some((_, on)) => overlay(styles, on, ink),
            None => styles,
        }
    })
}

/// `styles` cut around `marks`, then the marks: the runs must not overlap.
fn overlay(
    styles: Vec<(Range<usize>, HighlightStyle)>,
    marks: &[(Range<usize>, Hsla)],
    ink: Hsla,
) -> Vec<(Range<usize>, HighlightStyle)> {
    let mut out = Vec::new();
    for (range, style) in styles {
        let mut at = range.start;
        for (mark, _) in marks {
            if mark.end <= at || mark.start >= range.end {
                continue;
            }
            if mark.start > at {
                out.push((at..mark.start, style));
            }
            at = mark.end.min(range.end);
        }
        if at < range.end {
            out.push((at..range.end, style));
        }
    }
    out.extend(marks.iter().map(|(mark, fill)| {
        (
            mark.clone(),
            HighlightStyle {
                color: Some(ink),
                background_color: Some(*fill),
                ..Default::default()
            },
        )
    }));
    out.sort_by_key(|(range, _)| range.start);
    out
}

/// Called while a code block draws: when the app's colors are not the ones
/// the highlighter was built with (the first dark frame, a change of mode or
/// of contrast), the next frame gets a new highlighter.
pub fn sync_colors(cx: &mut App) {
    let palette = Palette::of(&theme::colors(cx));
    let changed = CURRENT.with(|current| {
        let mut current = current.borrow_mut();
        if current.as_ref().is_some_and(|(p, _)| *p == palette) {
            return false;
        }
        *current = Some((palette, palette.highlighter()));
        true
    });
    if changed {
        cx.refresh_windows();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn of<'a>(code: &'a str, info: &str, want: Span) -> Vec<&'a str> {
        spans(code, info)
            .into_iter()
            .filter(|(_, s)| *s == want)
            .map(|(r, _)| &code[r])
            .collect()
    }

    #[test]
    fn a_rust_fence_marks_keywords_values_and_comments_across_lines() {
        let code =
            "pub fn f() -> u32 {\n    // 说明\n    let s = \"a\"; /* open\n still */ 42\n}\n";
        assert_eq!(of(code, "rust", Span::Keyword), ["pub", "fn", "let"]);
        assert_eq!(of(code, "rs", Span::Value), ["\"a\"", "42"]);
        assert_eq!(
            of(code, "rust", Span::Comment),
            ["// 说明", "/* open", " still */"]
        );
        // Ranges are in source order and inside the code.
        let all = spans(code, "rust");
        assert!(all.windows(2).all(|w| w[0].0.end <= w[1].0.start));
        assert!(all.iter().all(|(r, _)| code.get(r.clone()).is_some()));
    }

    #[test]
    fn a_diff_fence_marks_whole_lines() {
        let code = "--- a/x.rs\n+++ b/x.rs\n@@ -1,2 +1,2 @@\n context\n-old\n+new\n\n";
        assert_eq!(of(code, "diff", Span::Added), ["+new"]);
        assert_eq!(of(code, "diff", Span::Removed), ["-old"]);
        assert_eq!(
            of(code, "DIFF", Span::Meta),
            ["--- a/x.rs", "+++ b/x.rs", "@@ -1,2 +1,2 @@"]
        );
    }

    #[test]
    fn an_unknown_or_missing_language_is_plain() {
        assert!(spans("let x = 1;", "").is_empty());
        assert!(spans("let x = 1;", "whitespace").is_empty());
        assert_eq!(label(""), None);
        assert_eq!(label("whitespace"), None);
        assert_eq!(label("text"), None);
        assert_eq!(label("ts"), Some("TypeScript"));
        assert_eq!(label("diff"), Some("Diff"));
        assert_eq!(label("mermaid"), Some("Mermaid"));
    }

    #[test]
    fn the_highlighter_styles_a_block_by_its_language() {
        let palette = Palette {
            value: gpui::red(),
            comment: gpui::blue(),
        };
        assert_eq!(
            palette.style(Span::Keyword).font_weight,
            Some(gpui::FontWeight::SEMIBOLD)
        );
        assert_eq!(palette.style(Span::Value).color, Some(gpui::red()));
        assert_eq!(palette.style(Span::Added).background_color, None);
        assert_eq!(
            diff_lines("--- a\n+new\n same\n-old\n", "diff"),
            [(1, true), (3, false)]
        );
        let block = CodeBlock::from_code("fn x() {}", Some("rust"));
        assert_eq!(block.lang().as_deref(), Some("rust"));
        assert_eq!(
            of(&block.code(), &block.lang().unwrap(), Span::Keyword),
            ["fn"]
        );
    }
}
