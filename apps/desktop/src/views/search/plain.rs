//! Markdown to one line of plain text, for switcher snippets and topic row
//! previews (DESIGN.md §7.6, §7.12). Headings, quote and list markers, task
//! boxes, emphasis, strike, code marks and fences go; a link or an image
//! becomes its text; line breaks become one space. Highlight ranges (in
//! characters) move with the text they cover.

use std::ops::Range;

/// `text` as plain text, with `ranges` (character offsets into `text`) moved
/// to the same characters in the result. Ranges that only covered marks are
/// dropped.
pub fn plain_with_ranges(text: &str, ranges: &[Range<usize>]) -> (String, Vec<Range<usize>>) {
    let chars: Vec<char> = text.chars().collect();
    let n = chars.len();
    let drop = marks(&chars);
    let mut out: Vec<char> = Vec::with_capacity(n);
    // `at[i]`: where input character `i` lands (or would land) in `out`.
    let mut at: Vec<usize> = Vec::with_capacity(n + 1);
    for (i, &c) in chars.iter().enumerate() {
        at.push(out.len());
        if drop[i] {
            continue;
        }
        if c.is_whitespace() {
            // One space between words, none after full-width punctuation
            // (`方式：邀请码`, U47).
            if out
                .last()
                .is_some_and(|l| *l != ' ' && !full_width_punct(*l))
            {
                out.push(' ');
            }
        } else {
            out.push(c);
        }
    }
    at.push(out.len());
    while out.last() == Some(&' ') {
        out.pop();
    }
    let len = out.len();
    let moved = ranges
        .iter()
        .map(|r| at[r.start.min(n)].min(len)..at[r.end.min(n)].min(len))
        .map(|r| {
            // A mark never starts or ends on a space.
            let (mut s, mut e) = (r.start, r.end);
            while s < e && out[s] == ' ' {
                s += 1;
            }
            while e > s && out[e - 1] == ' ' {
                e -= 1;
            }
            s..e
        })
        .filter(|r| r.start < r.end)
        .collect();
    (out.into_iter().collect(), moved)
}

/// `text` as one line of plain text.
pub fn plain(text: &str) -> String {
    plain_with_ranges(text, &[]).0
}

/// Which characters are Markdown marks.
fn marks(c: &[char]) -> Vec<bool> {
    let n = c.len();
    let mut drop = vec![false; n];
    // A block mark may follow the start, a line break, a space (snippets
    // arrive with line breaks already folded) or a leading `…`.
    let block_start = |i: usize| i == 0 || c[i - 1].is_whitespace() || (c[i - 1] == '…' && i == 1);
    let mut i = 0;
    while i < n {
        let ch = c[i];
        if block_start(i) {
            // Code fence with its language word.
            if run(c, i, '`') >= 3 {
                let mut j = i;
                while j < n && !c[j].is_whitespace() {
                    drop[j] = true;
                    j += 1;
                }
                i = j;
                continue;
            }
            // `# ` to `###### `.
            let hashes = run(c, i, '#');
            if (1..=6).contains(&hashes) && space_at(c, i + hashes) {
                drop[i..=i + hashes].fill(true);
                i += hashes + 1;
                continue;
            }
            // `> `, `- `, `* `, `+ `.
            // `*` and `+` only at a real line start (`2 * 3` stays); `-`
            // and `>` also in folded snippets.
            let line_start = i == 0 || c[i - 1] == '\n';
            let marker = match ch {
                '>' | '-' => true,
                '*' | '+' => line_start,
                _ => false,
            };
            if marker && space_at(c, i + 1) {
                drop[i] = true;
                drop[i + 1] = true;
                i += 2;
                // A task box after a list marker.
                if i + 3 < n
                    && c[i] == '['
                    && matches!(c[i + 1], ' ' | 'x' | 'X')
                    && c[i + 2] == ']'
                    && c[i + 3] == ' '
                {
                    drop[i..i + 4].fill(true);
                    i += 4;
                }
                continue;
            }
            // `1. `
            let digits = c[i..].iter().take_while(|d| d.is_ascii_digit()).count();
            if digits > 0
                && digits <= 3
                && c.get(i + digits) == Some(&'.')
                && space_at(c, i + digits + 1)
            {
                drop[i..=i + digits + 1].fill(true);
                i += digits + 2;
                continue;
            }
        }
        match ch {
            '`' => drop[i] = true,
            '*' => {
                // Emphasis, not a lone `*` between spaces.
                let r = run(c, i, '*');
                let lone = (i == 0 || c[i - 1].is_whitespace())
                    && c.get(i + r).is_none_or(|x| x.is_whitespace());
                if !lone {
                    drop[i..i + r].fill(true);
                }
                i += r;
                continue;
            }
            '~' if run(c, i, '~') >= 2 => {
                let r = run(c, i, '~');
                drop[i..i + r].fill(true);
                i += r;
                continue;
            }
            '_' => {
                // `__bold__` and `_em_`, but not `snake_case`.
                let r = run(c, i, '_');
                let word_before = i > 0 && c[i - 1].is_alphanumeric();
                let word_after = c.get(i + r).is_some_and(|x| x.is_alphanumeric());
                if !(word_before && word_after) && (word_before || word_after) {
                    drop[i..i + r].fill(true);
                }
                i += r;
                continue;
            }
            '!' if c.get(i + 1) == Some(&'[') && link_end(c, i + 1).is_some() => {
                drop[i] = true;
            }
            '[' => {
                if let Some((close, end)) = link_end(c, i) {
                    drop[i] = true;
                    drop[close..=end].fill(true);
                }
            }
            _ => {}
        }
        i += 1;
    }
    drop
}

fn full_width_punct(c: char) -> bool {
    matches!(
        c,
        '：' | '，'
            | '。'
            | '；'
            | '！'
            | '？'
            | '、'
            | '（'
            | '）'
            | '「'
            | '」'
            | '『'
            | '』'
            | '【'
            | '】'
    )
}

/// How many `ch` start at `i`.
fn run(c: &[char], i: usize, ch: char) -> usize {
    c[i..].iter().take_while(|x| **x == ch).count()
}

fn space_at(c: &[char], i: usize) -> bool {
    c.get(i).is_some_and(|x| *x == ' ' || *x == '\t')
}

/// For `[text](url)` starting at `open`: the index of `]` and of `)`.
fn link_end(c: &[char], open: usize) -> Option<(usize, usize)> {
    let close = open + 1 + c[open + 1..].iter().position(|x| *x == ']' || *x == '\n')?;
    if c[close] != ']' || c.get(close + 1) != Some(&'(') {
        return None;
    }
    let end = close
        + 2
        + c[close + 2..]
            .iter()
            .position(|x| *x == ')' || x.is_whitespace())?;
    (c[end] == ')').then_some((close, end))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn marked(text: &str, ranges: &[Range<usize>]) -> Vec<String> {
        let (out, moved) = plain_with_ranges(text, ranges);
        let chars: Vec<char> = out.chars().collect();
        moved
            .into_iter()
            .map(|r| chars[r].iter().collect())
            .collect()
    }

    #[test]
    fn strips_blocks_and_inline_marks() {
        assert_eq!(
            plain("### 新功能\n- 群和私聊\n- **话题**状态与待办\n\n```rust\nlet x = 1;\n```"),
            "新功能 群和私聊 话题状态与待办 let x = 1;"
        );
        assert_eq!(plain("1. 邀请码 `XXXX`\n2. 粘贴"), "邀请码 XXXX 粘贴");
        assert_eq!(plain("> 引用 ~~删掉~~ _斜体_"), "引用 删掉 斜体");
        assert_eq!(
            plain("看 [文档](https://example.com/a) 和 ![图](x.png)"),
            "看 文档 和 图"
        );
        assert_eq!(plain("- [x] 完成\n- [ ] 未完成"), "完成 未完成");
        assert_eq!(plain("snake_case 和 2 * 3"), "snake_case 和 2 * 3");
        assert_eq!(plain("两种方式：\n\n1. 邀请码"), "两种方式：邀请码");
    }

    #[test]
    fn folded_snippets_lose_their_marks() {
        assert_eq!(
            plain("…明（草稿） ### 新功能 - 群和私聊 - 话题状态"),
            "…明（草稿）新功能 群和私聊 话题状态"
        );
    }

    #[test]
    fn ranges_move_with_the_text() {
        let text = "…明 ### 新功能 - 群和私聊 - 话题状态";
        let at = text.chars().position(|c| c == '话').unwrap();
        assert_eq!(marked(text, &[at..at + 2]), vec!["话题"]);
        let text = "**加粗的话题**";
        assert_eq!(marked(text, &[0..text.chars().count()]), vec!["加粗的话题"]);
        // A range over marks only is dropped.
        assert!(marked("### 标题", &[0..3]).is_empty());
    }
}
