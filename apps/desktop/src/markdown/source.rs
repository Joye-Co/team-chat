//! Text passes on a message body before it is parsed as Markdown: code fences
//! the way Slack reads them, and `@name` turned into mention links.

/// A member that `@name` in a body can mention.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MentionTarget {
    pub name: String,
    pub id: String,
    /// The viewer: drawn with the `me` tokens.
    pub me: bool,
}

/// Link scheme of a mention of someone else.
pub const MENTION_SCHEME: &str = "mention:";
/// Link scheme of a mention of the viewer.
pub const MENTION_ME_SCHEME: &str = "mention-me:";

/// Link fragment that carries the full-width mark before a mention.
pub const AFTER_MARK: &str = "#after=";

const FENCE: &str = "```";

/// Link scheme of the inline `（已编辑）` mark (see [`append_edited_mark`]).
pub const EDITED_SCHEME: &str = "edited:";

/// `source` (after [`normalize_fences`]) with the edited mark as an inline
/// link at the end of its last line, so `（已编辑）` follows the last line
/// (DESIGN.md §7.5, U12). `None` when the body ends in a block that cannot
/// take inline text (a code block, a table, a rule or a setext underline):
/// the caller then puts the mark on its own line.
pub fn append_edited_mark(source: &str, label: &str) -> Option<String> {
    let mut in_fence = false;
    let mut last = None;
    for line in source.lines() {
        if line.trim_start().starts_with(FENCE) {
            in_fence = !in_fence;
            last = Some(line);
            continue;
        }
        if !line.trim().is_empty() {
            last = Some(line);
        }
    }
    let last = last?;
    let t = last.trim();
    let block_end = in_fence
        || t.starts_with(FENCE)
        || t.starts_with('|')
        || last.starts_with("    ")
        || last.starts_with('\t')
        || t.starts_with('<')
        || t.chars().all(|c| matches!(c, '-' | '=' | '*' | '_' | ' '));
    if block_end {
        return None;
    }
    let body = source.trim_end();
    // A trailing backslash would escape the mark's bracket.
    let sep = if body.ends_with('\\') { "\\ " } else { " " };
    Some(format!("{body}{sep}[{label}](<{EDITED_SCHEME}>)"))
}

/// Full-width punctuation that CoreText compresses at the end of a run.
fn is_full_width_mark(c: char) -> bool {
    gpui::LineWrapper::is_no_start_char(c) && c != '…' && c != '‥' && c != '·'
        || gpui::LineWrapper::is_no_end_char(c) && !c.is_ascii()
}

/// Code fences as Slack reads them (DESIGN.md U10):
///
/// - a fence after text on the same line (`see ```code`) starts a block on
///   the next line; text before a closing fence (`code```) stays in the block;
/// - a block with no code in it is dropped, closed or not, so the body never
///   shows an empty code box;
/// - a block that is not closed runs to the end of the message.
pub fn normalize_fences(text: &str) -> String {
    if !text.contains(FENCE) {
        return text.to_string();
    }
    let mut out: Vec<String> = Vec::new();
    // Index in `out` of the open fence line, and whether code followed it.
    let mut open: Option<(usize, bool)> = None;
    for line in text.split('\n') {
        let trimmed = line.trim_start();
        match open {
            None => {
                if trimmed.starts_with(FENCE) && !trimmed[FENCE.len()..].contains('`') {
                    out.push(line.to_string());
                    open = Some((out.len() - 1, false));
                } else if let Some(at) = lone_fence(line) {
                    let before = line[..at].trim_end();
                    if !before.is_empty() {
                        out.push(before.to_string());
                    }
                    out.push(line[at..].trim_start().to_string());
                    open = Some((out.len() - 1, false));
                } else {
                    out.push(line.to_string());
                }
            }
            Some((start, has_code)) => {
                let closing = trimmed.trim_end() == FENCE
                    || (line.trim_end().ends_with(FENCE) && lone_fence(line).is_some());
                if closing {
                    let code = line.trim_end().strip_suffix(FENCE).unwrap_or_default();
                    let has_code = has_code || !code.trim().is_empty();
                    if !code.trim().is_empty() {
                        out.push(code.trim_end().to_string());
                    }
                    if has_code {
                        out.push(FENCE.to_string());
                    } else {
                        out.truncate(start);
                    }
                    open = None;
                } else {
                    out.push(line.to_string());
                    open = Some((start, has_code || !line.trim().is_empty()));
                }
            }
        }
    }
    if let Some((start, false)) = open {
        out.truncate(start);
    }
    while out.last().is_some_and(|l| l.trim().is_empty()) {
        out.pop();
    }
    out.join("\n")
}

/// The byte index of the only triple-backtick run in `line`, when it has
/// exactly one (two or more make inline code spans).
fn lone_fence(line: &str) -> Option<usize> {
    let at = line.find(FENCE)?;
    let rest = &line[at..];
    let run = rest.len() - rest.trim_start_matches('`').len();
    (run == FENCE.len() && !rest[run..].contains('`')).then_some(at)
}

/// Turns `@name` of a known member into a mention link
/// (`[@name](<mention:id>)`) outside code. The longest name wins, a name must
/// end at a word edge (`@Sc` does not mention Scout), and `a@b.com` is not a
/// mention.
pub fn link_mentions(text: &str, targets: &[MentionTarget]) -> String {
    if targets.is_empty() || !text.contains('@') {
        return text.to_string();
    }
    let mut by_length: Vec<&MentionTarget> =
        targets.iter().filter(|t| !t.name.is_empty()).collect();
    by_length.sort_by_key(|t| std::cmp::Reverse(t.name.len()));

    let mut out = String::with_capacity(text.len() + 32);
    let mut in_fence = false;
    for (i, line) in text.split('\n').enumerate() {
        if i > 0 {
            out.push('\n');
        }
        if line.trim_start().starts_with(FENCE) {
            in_fence = !in_fence;
            out.push_str(line);
            continue;
        }
        if in_fence {
            out.push_str(line);
            continue;
        }
        link_line(line, &by_length, &mut out);
    }
    out
}

fn link_line(line: &str, targets: &[&MentionTarget], out: &mut String) {
    let mut rest = line;
    let mut prev: Option<char> = None;
    while let Some(c) = rest.chars().next() {
        // Inline code: copy through the matching backtick run.
        if c == '`' {
            let run = rest.len() - rest.trim_start_matches('`').len();
            let ticks = &rest[..run];
            let end = rest[run..]
                .find(ticks)
                .map_or(rest.len(), |at| run + at + run);
            out.push_str(&rest[..end]);
            prev = rest[..end].chars().next_back();
            rest = &rest[end..];
            continue;
        }
        // A Markdown link or image: keep it as written.
        if (c == '[' || (c == '!' && rest[1..].starts_with('[')))
            && let Some(end) = link_end(rest)
        {
            out.push_str(&rest[..end]);
            prev = rest[..end].chars().next_back();
            rest = &rest[end..];
            continue;
        }
        if c == '@' && !prev.is_some_and(|p| p.is_ascii_alphanumeric() || p == '_') {
            let after = &rest[1..];
            let hit = targets.iter().find(|t| {
                after.starts_with(t.name.as_str())
                    && !after[t.name.len()..]
                        .chars()
                        .next()
                        .is_some_and(|n| n.is_ascii_alphanumeric() || n == '_')
            });
            if let Some(t) = hit {
                let scheme = if t.me {
                    MENTION_ME_SCHEME
                } else {
                    MENTION_SCHEME
                };
                out.push_str("[@");
                for ch in t.name.chars() {
                    if matches!(ch, '[' | ']' | '\\' | '*' | '_' | '`') {
                        out.push('\\');
                    }
                    out.push(ch);
                }
                out.push_str("](<");
                out.push_str(scheme);
                out.push_str(&t.id.replace(['<', '>', ' ', '#'], ""));
                // The full-width mark before the pill ends a text run, and
                // CoreText sets a run-final mark half wide; the pill takes
                // back the difference (see `MentionPlugin`).
                if let Some(p) = prev.filter(|p| is_full_width_mark(*p)) {
                    out.push_str(AFTER_MARK);
                    out.push(p);
                }
                out.push_str(">)");
                prev = t.name.chars().next_back();
                rest = &after[t.name.len()..];
                continue;
            }
        }
        out.push(c);
        prev = Some(c);
        rest = &rest[c.len_utf8()..];
    }
}

/// The end of a `[text](dest)` link that starts at the beginning of `s`.
fn link_end(s: &str) -> Option<usize> {
    let open = s.find('[')?;
    let close = open + s[open..].find("](")?;
    let end = close + s[close..].find(')')?;
    Some(end + 1)
}

// --- inc-refs: the Markdown block counter of 引用提问 (REFS §A2.1) ---

/// The kind of a top-level Markdown block.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum MdBlock {
    Paragraph,
    Heading,
    List,
    Quote,
    Code,
    Table,
    Rule,
    Html,
    Math,
    Other,
}

/// The top-level blocks of a normalized body ([`normalize_fences`]) in source order, with
/// their byte ranges: paragraph, heading, list (one block for the whole list), block quote,
/// fenced or indented code, table, thematic break, HTML block. Blank lines and link
/// definitions are not blocks. Parsed with the `markdown` crate at the options of
/// gpui-component's renderer (GFM and math), so the index `md:<n>` names the block the
/// person sees.
pub fn top_level_blocks(source: &str) -> Vec<(MdBlock, std::ops::Range<usize>)> {
    use markdown::mdast::Node;
    let mut options = markdown::ParseOptions::gfm();
    options.constructs.math_text = true;
    options.constructs.math_flow = true;
    let Ok(Node::Root(root)) = markdown::to_mdast(source, &options) else {
        return Vec::new();
    };
    root.children
        .iter()
        .filter_map(|node| {
            let kind = match node {
                Node::Definition(_) | Node::FootnoteDefinition(_) => return None,
                Node::Paragraph(_) => MdBlock::Paragraph,
                Node::Heading(_) => MdBlock::Heading,
                Node::List(_) => MdBlock::List,
                Node::Blockquote(_) => MdBlock::Quote,
                Node::Code(_) => MdBlock::Code,
                Node::Table(_) => MdBlock::Table,
                Node::ThematicBreak(_) => MdBlock::Rule,
                Node::Html(_) => MdBlock::Html,
                Node::Math(_) => MdBlock::Math,
                _ => MdBlock::Other,
            };
            let p = node.position()?;
            Some((kind, p.start.offset..p.end.offset))
        })
        .collect()
}

// --- end inc-refs ---

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_fence_after_text_starts_a_block() {
        assert_eq!(
            normalize_fences("#9996 ```\ncargo bench -p chat-core\n```"),
            "#9996\n```\ncargo bench -p chat-core\n```"
        );
        assert_eq!(
            normalize_fences("看这个 ```rust\nlet a = 1;\n```"),
            "看这个\n```rust\nlet a = 1;\n```"
        );
        // Code before the closing fence stays in the block.
        assert_eq!(
            normalize_fences("```\nlet a = 1;```"),
            "```\nlet a = 1;\n```"
        );
    }

    #[test]
    fn edited_mark_follows_the_last_line() {
        let m = |s: &str| append_edited_mark(s, "（已编辑）");
        assert_eq!(
            m("周五下午 4 点 demo").as_deref(),
            Some("周五下午 4 点 demo [（已编辑）](<edited:>)")
        );
        assert_eq!(
            m("第一段\n\n- 一\n- 二\n").as_deref(),
            Some("第一段\n\n- 一\n- 二 [（已编辑）](<edited:>)")
        );
        assert_eq!(m("看\n```\nlet a = 1;\n```"), None);
        assert_eq!(m("```\nlet a = 1;"), None);
        assert_eq!(m("| a | b |\n| - | - |\n| 1 | 2 |"), None);
        assert_eq!(m("标题\n==="), None);
        assert_eq!(m("   "), None);
    }

    #[test]
    fn never_an_empty_code_block() {
        assert_eq!(normalize_fences("结果如下 ```"), "结果如下");
        assert_eq!(normalize_fences("结果如下\n```"), "结果如下");
        assert_eq!(normalize_fences("结果如下\n```\n\n```\n完"), "结果如下\n完");
        // Not closed but with code: the block runs to the end.
        assert_eq!(normalize_fences("```\nlet a = 1;"), "```\nlet a = 1;");
    }

    #[test]
    fn inline_code_and_plain_text_are_left_alone() {
        for text in [
            "use ```x``` here",
            "a `b` c",
            "无代码",
            "```rust\nfn a() {}\n```",
        ] {
            assert_eq!(normalize_fences(text), text);
        }
    }

    fn targets() -> Vec<MentionTarget> {
        let t = |name: &str, id: &str, me: bool| MentionTarget {
            name: name.into(),
            id: id.into(),
            me,
        };
        vec![
            t("Scout", "m-scout", false),
            t("Alex Chen", "m-alex", false),
            t("Alex", "m-alex2", false),
            t("Joye", "m-joye", true),
        ]
    }

    #[test]
    fn mentions_become_links() {
        let t = targets();
        assert_eq!(
            link_mentions("等 @Scout 在 staging 验收。", &t),
            "等 [@Scout](<mention:m-scout>) 在 staging 验收。"
        );
        assert_eq!(
            link_mentions("@Joye，@Alex Chen 看下", &t),
            "[@Joye](<mention-me:m-joye>)，[@Alex Chen](<mention:m-alex#after=，>) 看下"
        );
        assert_eq!(
            link_mentions("@Scout在吗", &t),
            "[@Scout](<mention:m-scout>)在吗"
        );
    }

    #[test]
    fn not_a_mention() {
        let t = targets();
        for text in [
            "@Sc",
            "@Scouts",
            "mail a@Scout.com",
            "`@Scout`",
            "```\n@Scout\n```",
            "[@Scout](https://example.com)",
            "@某人",
        ] {
            assert_eq!(link_mentions(text, &t), text);
        }
    }
}
