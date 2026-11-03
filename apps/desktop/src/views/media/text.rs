//! Text work behind the previews: CSV tables, monochrome code highlighting and the
//! Markdown outline. Pure functions, unit-tested below.

/// A parsed table: header, the first rows, and how many data rows the file has.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Table {
    pub header: Vec<String>,
    pub rows: Vec<Vec<String>>,
    pub total_rows: usize,
    /// True for a column whose every non-empty cell reads as a number.
    pub numeric: Vec<bool>,
}

/// Parses CSV (or TSV with `delim = '\t'`): quoted fields, `""` escapes, CRLF. Keeps at
/// most `max_rows` data rows and counts the rest.
pub fn parse_table(text: &str, delim: char, max_rows: usize) -> Table {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let mut records: Vec<Vec<String>> = Vec::new();
    let mut total = 0usize;
    let mut field = String::new();
    let mut record: Vec<String> = Vec::new();
    let mut quoted = false;
    let mut chars = text.chars().peekable();
    let mut push_record = |record: &mut Vec<String>, records: &mut Vec<Vec<String>>| {
        if record.len() == 1 && record[0].is_empty() {
            record.clear();
            return;
        }
        if records.len() <= max_rows {
            records.push(std::mem::take(record));
        } else {
            record.clear();
        }
        total += 1;
    };
    while let Some(c) = chars.next() {
        if quoted {
            if c == '"' {
                if chars.peek() == Some(&'"') {
                    chars.next();
                    field.push('"');
                } else {
                    quoted = false;
                }
            } else {
                field.push(c);
            }
            continue;
        }
        match c {
            '"' if field.is_empty() => quoted = true,
            '\r' => {}
            '\n' => {
                record.push(std::mem::take(&mut field));
                push_record(&mut record, &mut records);
            }
            c if c == delim => record.push(std::mem::take(&mut field)),
            c => field.push(c),
        }
    }
    if !field.is_empty() || !record.is_empty() {
        record.push(field);
        push_record(&mut record, &mut records);
    }
    let mut it = records.into_iter();
    let header = it.next().unwrap_or_default();
    let rows: Vec<Vec<String>> = it.collect();
    let cols = header.len();
    let numeric = (0..cols)
        .map(|c| {
            let mut any = false;
            let all = rows.iter().all(|r| {
                let v = r.get(c).map(|s| s.trim()).unwrap_or("");
                if v.is_empty() || v == "-" || v == "–" {
                    return true;
                }
                any = true;
                number(v).is_some()
            });
            all && any
        })
        .collect();
    Table {
        header,
        rows,
        total_rows: total.saturating_sub(1),
        numeric,
    }
}

/// A cell read as a number: `2,113`, `¥105`, `-0.3`, `12%`, `1.2 秒` (the unit is ignored).
pub fn number(cell: &str) -> Option<f64> {
    let s: String = cell
        .trim()
        .trim_start_matches(['¥', '$', '€', '+'])
        .chars()
        .filter(|c| *c != ',')
        .collect();
    let s = s.trim_end_matches(['%', ' ']).trim();
    let end = s
        .char_indices()
        .find(|(i, c)| !(c.is_ascii_digit() || *c == '.' || (*i == 0 && *c == '-')))
        .map_or(s.len(), |(i, _)| i);
    let (num, rest) = s.split_at(end);
    if num.is_empty() || num == "-" {
        return None;
    }
    // Allow a trailing unit word, not a second number.
    if rest.trim().chars().any(|c| c.is_ascii_digit()) {
        return None;
    }
    num.parse().ok()
}

// --- code ---------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tok {
    Plain,
    Keyword,
    Str,
    Num,
    Comment,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Lang {
    /// `//` and `/* */` comments: TypeScript, JavaScript, Rust, Go, Swift, Kotlin, Java, C.
    CLike,
    /// `#` comments: Python, shell, Ruby, YAML, TOML.
    Hash,
    /// `--` comments.
    Sql,
    /// No comments (JSON, CSS treated as C-like for `/* */`).
    Plain,
}

/// The display name and comment style of a code file, from its extension.
pub fn language(path: &str) -> Option<(&'static str, Lang)> {
    let ext = path.rsplit_once('.')?.1.to_ascii_lowercase();
    Some(match ext.as_str() {
        "ts" | "mts" | "cts" => ("TypeScript", Lang::CLike),
        "tsx" => ("TSX", Lang::CLike),
        "js" | "mjs" | "cjs" => ("JavaScript", Lang::CLike),
        "jsx" => ("JSX", Lang::CLike),
        "rs" => ("Rust", Lang::CLike),
        "go" => ("Go", Lang::CLike),
        "swift" => ("Swift", Lang::CLike),
        "kt" | "kts" => ("Kotlin", Lang::CLike),
        "java" => ("Java", Lang::CLike),
        "c" | "h" => ("C", Lang::CLike),
        "cc" | "cpp" | "hpp" | "cxx" => ("C++", Lang::CLike),
        "m" | "mm" => ("Objective-C", Lang::CLike),
        "cs" => ("C#", Lang::CLike),
        "css" | "scss" => ("CSS", Lang::CLike),
        "py" => ("Python", Lang::Hash),
        "rb" => ("Ruby", Lang::Hash),
        "sh" | "bash" | "zsh" => ("Shell", Lang::Hash),
        "yaml" | "yml" => ("YAML", Lang::Hash),
        "toml" => ("TOML", Lang::Hash),
        "sql" => ("SQL", Lang::Sql),
        "json" | "jsonc" => ("JSON", Lang::CLike),
        "txt" | "log" => ("Text", Lang::Plain),
        _ => return None,
    })
}

const KEYWORDS: &[&str] = &[
    "import",
    "export",
    "from",
    "function",
    "return",
    "const",
    "let",
    "var",
    "if",
    "else",
    "for",
    "while",
    "of",
    "in",
    "continue",
    "break",
    "class",
    "interface",
    "type",
    "new",
    "async",
    "await",
    "true",
    "false",
    "null",
    "undefined",
    "number",
    "string",
    "boolean",
    "void",
    "fn",
    "pub",
    "use",
    "mod",
    "struct",
    "enum",
    "impl",
    "trait",
    "match",
    "mut",
    "self",
    "Self",
    "where",
    "loop",
    "def",
    "None",
    "True",
    "False",
    "elif",
    "lambda",
    "yield",
    "func",
    "package",
    "go",
    "defer",
    "switch",
    "case",
    "default",
    "static",
    "extends",
    "implements",
    "try",
    "catch",
    "finally",
    "throw",
    "public",
    "private",
    "protected",
    "struct",
    "nil",
    "select",
    "SELECT",
    "FROM",
    "WHERE",
    "and",
    "or",
    "not",
    "with",
    "as",
    "is",
];

/// Splits one line into tokens. `block` carries an open `/* */` comment across lines.
pub fn highlight(line: &str, lang: Lang, block: &mut bool) -> Vec<(Tok, String)> {
    let mut out: Vec<(Tok, String)> = Vec::new();
    let push = |out: &mut Vec<(Tok, String)>, tok: Tok, s: &str| {
        if s.is_empty() {
            return;
        }
        match out.last_mut() {
            Some((t, prev)) if *t == tok => prev.push_str(s),
            _ => out.push((tok, s.to_string())),
        }
    };
    let b = line.as_bytes();
    let mut i = 0;
    while i < b.len() {
        if *block {
            match line[i..].find("*/") {
                Some(end) => {
                    push(&mut out, Tok::Comment, &line[i..i + end + 2]);
                    i += end + 2;
                    *block = false;
                }
                None => {
                    push(&mut out, Tok::Comment, &line[i..]);
                    return out;
                }
            }
            continue;
        }
        let rest = &line[i..];
        let comment_start = match lang {
            Lang::CLike => rest.starts_with("//"),
            Lang::Hash => rest.starts_with('#'),
            Lang::Sql => rest.starts_with("--"),
            Lang::Plain => false,
        };
        if comment_start {
            push(&mut out, Tok::Comment, rest);
            return out;
        }
        if lang == Lang::CLike && rest.starts_with("/*") {
            *block = true;
            push(&mut out, Tok::Comment, "/*");
            i += 2;
            continue;
        }
        let c = b[i];
        if lang != Lang::Plain && (c == b'"' || c == b'\'' || c == b'`') {
            let mut j = i + 1;
            while j < b.len() && b[j] != c {
                if b[j] == b'\\' {
                    j += 1;
                }
                j += 1;
            }
            let end = (j + 1).min(b.len());
            push(&mut out, Tok::Str, &line[i..end]);
            i = end;
            continue;
        }
        if c.is_ascii_digit() && (i == 0 || !is_ident(b[i - 1])) {
            let mut j = i;
            while j < b.len() && (b[j].is_ascii_alphanumeric() || b[j] == b'_' || b[j] == b'.') {
                j += 1;
            }
            push(&mut out, Tok::Num, &line[i..j]);
            i = j;
            continue;
        }
        if is_ident(c) {
            let mut j = i;
            while j < b.len() && is_ident(b[j]) {
                j += 1;
            }
            let word = &line[i..j];
            let tok = if lang != Lang::Plain && KEYWORDS.contains(&word) {
                Tok::Keyword
            } else {
                Tok::Plain
            };
            push(&mut out, tok, word);
            i = j;
            continue;
        }
        // One character (keeps multi-byte characters whole).
        let len = rest.chars().next().map_or(1, char::len_utf8);
        push(&mut out, Tok::Plain, &line[i..i + len]);
        i += len;
    }
    out
}

fn is_ident(c: u8) -> bool {
    c.is_ascii_alphanumeric() || c == b'_' || c == b'$'
}

// --- Markdown -------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Heading {
    pub level: u8,
    pub text: String,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct MdStats {
    /// Characters of text, without Markdown marks and spaces.
    pub chars: usize,
    pub tables: usize,
    pub images: usize,
    pub todo_done: usize,
    pub todo_all: usize,
}

/// Headings outside code fences, in order.
pub fn outline(md: &str) -> Vec<Heading> {
    let mut fence = false;
    let mut out = Vec::new();
    for line in md.lines() {
        let t = line.trim_start();
        if t.starts_with("```") || t.starts_with("~~~") {
            fence = !fence;
            continue;
        }
        if fence {
            continue;
        }
        let level = t.chars().take_while(|c| *c == '#').count();
        if (1..=4).contains(&level) && t[level..].starts_with(' ') {
            out.push(Heading {
                level: level as u8,
                text: t[level..].trim().trim_end_matches('#').trim().to_string(),
            });
        }
    }
    out
}

pub fn md_stats(md: &str) -> MdStats {
    let mut s = MdStats::default();
    let mut fence = false;
    let mut in_table = false;
    for line in md.lines() {
        let t = line.trim();
        if t.starts_with("```") || t.starts_with("~~~") {
            fence = !fence;
            continue;
        }
        if !fence {
            let is_row = t.starts_with('|') && t.ends_with('|') && t.len() > 1;
            if is_row && !in_table {
                s.tables += 1;
            }
            in_table = is_row;
            s.images += t.matches("![").count();
            let item = t.trim_start_matches(['-', '*', '+']).trim_start();
            if t.len() != item.len() {
                if item.starts_with("[x]") || item.starts_with("[X]") {
                    s.todo_done += 1;
                    s.todo_all += 1;
                } else if item.starts_with("[ ]") {
                    s.todo_all += 1;
                }
            }
        }
        s.chars += t
            .chars()
            .filter(|c| !c.is_whitespace() && !"#*-_`|>[]()!".contains(*c))
            .count();
    }
    s
}

/// The first `lines` lines of content (blank lines kept between blocks, but not at the
/// start), with an open code fence closed so the excerpt still parses.
pub fn md_excerpt(md: &str, lines: usize) -> String {
    let mut out = Vec::new();
    let mut fence = false;
    let mut count = 0;
    for line in md.lines() {
        let blank = line.trim().is_empty();
        if out.is_empty() && blank {
            continue;
        }
        // The first-level title repeats the 产出 name: skip it.
        if out.is_empty() && line.starts_with("# ") {
            continue;
        }
        if line.trim_start().starts_with("```") {
            fence = !fence;
        }
        out.push(line);
        if !blank {
            count += 1;
        }
        if count >= lines {
            break;
        }
    }
    let mut s = out.join("\n");
    if fence {
        s.push_str("\n```");
    }
    s
}

/// The Markdown from its first heading of `level` ≤ 2 named `heading` on: used to scroll the
/// reader to a heading by showing the document from there.
pub fn heading_offset(md: &str, heading: &str) -> Option<usize> {
    let mut fence = false;
    let mut at = 0;
    for line in md.split_inclusive('\n') {
        let t = line.trim_start();
        if t.starts_with("```") {
            fence = !fence;
        }
        if !fence && t.starts_with('#') && t.trim_start_matches('#').trim() == heading {
            return Some(at);
        }
        at += line.len();
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_csv_with_quotes_and_counts_rows() {
        let t = parse_table(
            "日期,渠道,花费\r\n10-01,\"B 站, 测试\",¥105\n10-02,抖音,¥61\n10-03,\"说\"\"好\"\"\",¥0\n",
            ',',
            2,
        );
        assert_eq!(t.header, ["日期", "渠道", "花费"]);
        assert_eq!(t.rows.len(), 2);
        assert_eq!(t.rows[0][1], "B 站, 测试");
        assert_eq!(t.total_rows, 3);
        assert_eq!(t.numeric, [false, false, true]);
        let t = parse_table("a\tb\n1\t2", '\t', 10);
        assert_eq!(t.rows, vec![vec!["1".to_string(), "2".to_string()]]);
        assert_eq!(t.numeric, [true, true]);
    }

    #[test]
    fn reads_numbers_with_units() {
        assert_eq!(number("2,113"), Some(2113.));
        assert_eq!(number("¥105"), Some(105.));
        assert_eq!(number("-0.3"), Some(-0.3));
        assert_eq!(number("12%"), Some(12.));
        assert_eq!(number("1.2 秒"), Some(1.2));
        assert_eq!(number("10-01"), None);
        assert_eq!(number("B 站"), None);
    }

    #[test]
    fn highlights_keywords_strings_and_comments() {
        let mut block = false;
        let toks = highlight(
            "export function f(s: number) { return \"a\" + 1; } // done",
            Lang::CLike,
            &mut block,
        );
        assert_eq!(toks[0], (Tok::Keyword, "export".into()));
        assert!(toks.contains(&(Tok::Str, "\"a\"".into())));
        assert!(toks.contains(&(Tok::Num, "1".into())));
        assert_eq!(toks.last().unwrap(), &(Tok::Comment, "// done".into()));
        let toks = highlight("a /* open", Lang::CLike, &mut block);
        assert!(block);
        assert_eq!(toks.last().unwrap().0, Tok::Comment);
        let toks = highlight("still */ b", Lang::CLike, &mut block);
        assert!(!block);
        assert_eq!(toks[0], (Tok::Comment, "still */".into()));
        assert_eq!(language("src/net/reconnect.ts").unwrap().0, "TypeScript");
        assert!(language("a.key").is_none());
    }

    #[test]
    fn markdown_outline_stats_and_excerpt() {
        let md = "# 复盘\n\n## 原因\n原因是旧的 seq。\n\n```\n# 不是标题\n```\n\n| a | b |\n|---|---|\n| 1 | 2 |\n\n![图](a.png)\n\n## 后续\n- [x] 去重\n- [ ] 测试\n";
        let o = outline(md);
        assert_eq!(
            o.iter().map(|h| h.text.as_str()).collect::<Vec<_>>(),
            ["复盘", "原因", "后续"]
        );
        let s = md_stats(md);
        assert_eq!((s.tables, s.images, s.todo_done, s.todo_all), (1, 1, 1, 2));
        let e = md_excerpt(md, 3);
        assert!(e.starts_with("## 原因"));
        assert!(e.ends_with("```"), "closes the open fence: {e}");
        assert_eq!(
            heading_offset(md, "原因"),
            Some(md.find("## 原因").unwrap())
        );
    }
}
