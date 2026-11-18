//! Folding of long content in the timeline: a long code fence shows its first
//! lines, and a long answer of an AI member shows its first blocks. Both are
//! decided from the text alone, before layout, so a row knows its content
//! when it is measured.

use std::{
    cell::RefCell,
    collections::{HashMap, HashSet},
    hash::{Hash, Hasher},
};

use super::source::{MdBlock, top_level_blocks};
use crate::ui::native::metrics::{
    BODY_FOLD_CHARS, BODY_FOLD_OVER, BODY_FOLD_ROWS, BODY_FOLD_UNITS, CODE_FOLD_LINES,
    CODE_FOLD_OVER,
};

/// The last line of a folded code block: the cue that lines follow.
pub const CUT_MARK: &str = "⋯";

/// One long code fence of a body.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CodeFold {
    /// The code the renderer draws: the whole code when open, else its first
    /// lines and [`CUT_MARK`].
    pub shown: String,
    /// The whole code (what the copy button copies).
    pub full: String,
    /// Lines of the whole code.
    pub lines: usize,
    pub open: bool,
}

pub fn text_hash(text: &str) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    text.trim_end().hash(&mut hasher);
    hasher.finish()
}

/// `(fence char, run length, info)` of an opening fence line at column 0.
fn fence_open(line: &str) -> Option<(char, usize, &str)> {
    let text = line.trim_end_matches(['\n', '\r']);
    let ch = match text.chars().next() {
        Some(c @ ('`' | '~')) => c,
        _ => return None,
    };
    let run = text.chars().take_while(|c| *c == ch).count();
    let info = text[run..].trim();
    (run >= 3 && !(ch == '`' && info.contains('`'))).then_some((ch, run, info))
}

fn fence_close(line: &str, ch: char, run: usize) -> bool {
    let text = line.trim_end_matches(['\n', '\r']);
    let indent = text.len() - text.trim_start_matches(' ').len();
    let t = &text[indent..];
    let n = t.chars().take_while(|c| *c == ch).count();
    indent <= 3 && n >= run && t[n..].trim().is_empty()
}

/// `source` with every closed top-level code fence of more than
/// [`CODE_FOLD_OVER`] lines cut to its first [`CODE_FOLD_LINES`] lines and a
/// [`CUT_MARK`] line, unless `is_open` says the person opened it. Diagram
/// fences are left alone (they are drawn, not listed). The folds come back in
/// source order, the open ones too (they get a 收起 button).
pub fn fold_code(source: &str, is_open: &dyn Fn(&str) -> bool) -> (String, Vec<CodeFold>) {
    let lines: Vec<&str> = source.split_inclusive('\n').collect();
    let mut out = String::with_capacity(source.len());
    let mut folds = Vec::new();
    let mut i = 0;
    while i < lines.len() {
        let Some((ch, run, info)) = fence_open(lines[i]) else {
            out.push_str(lines[i]);
            i += 1;
            continue;
        };
        let Some(close) = (i + 1..lines.len()).find(|j| fence_close(lines[*j], ch, run)) else {
            // Not closed: the block runs to the end of the body, as it is.
            out.extend(lines[i..].iter().copied());
            break;
        };
        let code = &lines[i + 1..close];
        let long = code.len() > CODE_FOLD_OVER && !crate::diagrams::is_diagram_info(info);
        let full = code.concat();
        let full = full.trim_end_matches(['\n', '\r']).to_string();
        let open = long && is_open(&full);
        if long && !open {
            out.push_str(lines[i]);
            let head = code[..CODE_FOLD_LINES].concat();
            out.push_str(&head);
            out.push_str(CUT_MARK);
            out.push('\n');
            out.push_str(lines[close]);
            folds.push(CodeFold {
                shown: format!("{}\n{CUT_MARK}", head.trim_end_matches(['\n', '\r'])),
                full,
                lines: code.len(),
                open: false,
            });
        } else {
            out.extend(lines[i..=close].iter().copied());
            if long {
                folds.push(CodeFold {
                    shown: full.clone(),
                    full,
                    lines: code.len(),
                    open: true,
                });
            }
        }
        i = close + 1;
    }
    (out, folds)
}

thread_local! {
    /// The code blocks the person opened, by the hash of their whole code.
    static OPEN_CODE: RefCell<HashSet<u64>> = RefCell::new(HashSet::new());
    /// The folds on screen, by the hash of the code the renderer draws: the
    /// code block's actions only get that text.
    static SHOWN: RefCell<HashMap<u64, CodeFold>> = RefCell::new(HashMap::new());
}

/// At most this many folds are remembered; older ones are found again when
/// their body is built.
const SHOWN_MAX: usize = 512;

/// [`fold_code`] with the person's open blocks, remembering the folds for
/// [`fold_of`].
pub fn fold_code_remembered(source: &str) -> String {
    let (out, folds) = OPEN_CODE.with(|open| {
        let open = open.borrow();
        fold_code(source, &|full| open.contains(&text_hash(full)))
    });
    if !folds.is_empty() {
        SHOWN.with(|shown| {
            let mut shown = shown.borrow_mut();
            if shown.len() > SHOWN_MAX {
                shown.clear();
            }
            for fold in folds {
                shown.insert(text_hash(&fold.shown), fold);
            }
        });
    }
    out
}

/// The fold whose drawn code is `shown`, if that code block is a long one.
pub fn fold_of(shown: &str) -> Option<CodeFold> {
    SHOWN.with(|s| s.borrow().get(&text_hash(shown)).cloned())
}

/// Opens or closes the code block whose whole code is `full`.
pub fn set_code_open(full: &str, open: bool) {
    let hash = text_hash(full);
    OPEN_CODE.with(|set| {
        let mut set = set.borrow_mut();
        if open {
            set.insert(hash);
        } else {
            set.remove(&hash);
        }
    });
}

/// Rows `text` takes in the in-thread column, estimated from its characters
/// (a full-width character is two units, a row holds [`BODY_FOLD_UNITS`]).
pub fn rendered_rows(text: &str) -> usize {
    text.lines()
        .map(|line| {
            let units: usize = line.chars().map(|c| if c.is_ascii() { 1 } else { 2 }).sum();
            units.div_ceil(BODY_FOLD_UNITS).max(1)
        })
        .sum()
}

/// Whether a finished answer is long enough to fold.
pub fn is_long_body(body: &str) -> bool {
    body.chars().count() > BODY_FOLD_CHARS || rendered_rows(body) > BODY_FOLD_OVER
}

/// Rows a block takes once drawn: a long code fence counts as its fold.
fn block_rows(kind: MdBlock, text: &str) -> usize {
    let rows = rendered_rows(text);
    if kind == MdBlock::Code {
        rows.min(CODE_FOLD_OVER + 2)
    } else {
        rows
    }
}

/// The byte length of the head of a long body to show while it is folded:
/// whole top-level blocks, up to [`BODY_FOLD_ROWS`] rows, at least one block.
/// `None` when the body is not long, or when the head would be the whole body
/// (one giant block, or nothing after the head).
pub fn body_fold_at(body: &str) -> Option<usize> {
    if !is_long_body(body) {
        return None;
    }
    let blocks = top_level_blocks(body);
    let mut rows = 0;
    let mut end = 0;
    for (i, (kind, range)) in blocks.iter().enumerate() {
        let text = body.get(range.clone()).unwrap_or_default();
        // The blank lines between two blocks are rows too.
        let gap = body
            .get(end..range.start)
            .map_or(0, |between| between.matches('\n').count().saturating_sub(1));
        let next = rows + gap + block_rows(*kind, text);
        if i > 0 && next > BODY_FOLD_ROWS {
            break;
        }
        rows = next;
        end = range.end;
    }
    let rest = body.get(end..).unwrap_or_default();
    (end > 0 && !rest.trim().is_empty()).then_some(end)
}

/// Minutes to read `text` (300 characters a minute, at least one).
pub fn reading_minutes(text: &str) -> usize {
    let chars = text.chars().filter(|c| !c.is_whitespace()).count();
    chars.div_ceil(300).max(1)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fence(lang: &str, n: usize) -> String {
        let code: String = (1..=n).map(|i| format!("line {i}\n")).collect();
        format!("```{lang}\n{code}```\n")
    }

    #[test]
    fn a_long_fence_shows_its_first_lines_and_a_cut_mark() {
        let body = format!("before\n\n{}\nafter\n", fence("rust", 80));
        let (out, folds) = fold_code(&body, &|_| false);
        assert_eq!(folds.len(), 1);
        assert_eq!(folds[0].lines, 80);
        assert!(!folds[0].open);
        assert!(folds[0].full.ends_with("line 80"));
        assert!(folds[0].shown.ends_with(&format!("line 24\n{CUT_MARK}")));
        assert!(out.starts_with("before\n\n```rust\nline 1\n"));
        assert!(out.contains(&format!("line 24\n{CUT_MARK}\n```\n\nafter\n")));
        assert!(!out.contains("line 25"));
        // The blocks of the body keep their order and count (`md:<n>` of a reference).
        assert_eq!(top_level_blocks(&out).len(), top_level_blocks(&body).len());
    }

    #[test]
    fn short_open_unclosed_and_diagram_fences_stay_whole() {
        let short = fence("ts", CODE_FOLD_OVER);
        assert_eq!(fold_code(&short, &|_| false), (short.clone(), vec![]));

        let long = fence("ts", 50);
        let (out, folds) = fold_code(&long, &|_| true);
        assert_eq!(out, long);
        assert!(folds[0].open);
        assert_eq!(folds[0].shown, folds[0].full);

        let unclosed = long.trim_end().trim_end_matches("```").to_string();
        assert_eq!(fold_code(&unclosed, &|_| false), (unclosed.clone(), vec![]));

        let diagram = fence("mermaid", 50);
        assert_eq!(fold_code(&diagram, &|_| false), (diagram.clone(), vec![]));

        // A fence in a list is not at the top level: left alone.
        let nested = format!("- item\n\n  {}", fence("ts", 50).replace('\n', "\n  "));
        assert_eq!(fold_code(&nested, &|_| false).1, vec![]);
    }

    #[test]
    fn the_remembered_fold_is_found_by_the_drawn_code_and_opens() {
        let body = fence("py", 40);
        let folded = fold_code_remembered(&body);
        let shown = folded
            .trim_start_matches("```py\n")
            .trim_end()
            .trim_end_matches("```");
        let fold = fold_of(shown).expect("fold");
        assert_eq!(fold.lines, 40);
        set_code_open(&fold.full, true);
        assert_eq!(fold_code_remembered(&body), body);
        assert!(fold_of(&fold.full).expect("open fold").open);
        set_code_open(&fold.full, false);
        assert_ne!(fold_code_remembered(&body), body);
    }

    #[test]
    fn rows_count_wrapped_lines_and_full_width_characters() {
        assert_eq!(rendered_rows("a\n\nb"), 3);
        assert_eq!(rendered_rows(&"x".repeat(BODY_FOLD_UNITS + 1)), 2);
        assert_eq!(rendered_rows(&"中".repeat(BODY_FOLD_UNITS / 2 + 1)), 2);
    }

    #[test]
    fn a_long_answer_folds_at_a_block_boundary() {
        let paragraph = |i: usize| format!("第 {i} 段，说明一件事。\n\n");
        let short: String = (0..20).map(paragraph).collect();
        assert_eq!(body_fold_at(&short), None);

        let long: String = (0..60).map(paragraph).collect();
        let at = body_fold_at(&long).expect("folds");
        let head = &long[..at];
        assert!(rendered_rows(head) <= BODY_FOLD_ROWS);
        assert!(head.ends_with("说明一件事。"));
        assert!(long[at..].trim_start().starts_with("第 "));

        // One giant block has no boundary to fold at.
        let giant = "word ".repeat(2000);
        assert_eq!(body_fold_at(&giant), None);

        // A long code fence first: the fence is never cut here (it folds
        // itself and counts as its fold), and text after it is hidden.
        let code_first = format!("{}\n{}", fence("rust", 120), long);
        let at = body_fold_at(&code_first).expect("folds");
        assert!(code_first[..at].contains("line 120\n```"));
        assert!(code_first[..at].ends_with("说明一件事。"));
        assert!(at < code_first.trim_end().len());
    }

    #[test]
    fn reading_time_rounds_up() {
        assert_eq!(reading_minutes("短"), 1);
        assert_eq!(reading_minutes(&"字".repeat(301)), 2);
    }
}
