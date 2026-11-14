//! The composer's rules without GPUI: @ queries, mention ids, the new-topic
//! title, the typing limit and which message Up edits.

use std::{
    collections::HashMap,
    time::{Duration, Instant},
};

use crate::data::{GroupId, Member, MemberId, Message, SendState, TopicId};

/// At most one typing event per topic in this time.
pub const TYPING_INTERVAL: Duration = Duration::from_secs(3);
/// Longest topic title, in characters (`StartTopicRequest::title`).
pub const MAX_TITLE_CHARS: usize = 80;
/// Rows in the @ list.
pub const MAX_MENTION_RESULTS: usize = 8;
/// An @ query longer than this is not a name.
const MAX_QUERY_CHARS: usize = 32;

/// The `@word` the cursor is in.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MentionQuery {
    /// Byte offset of the `@`.
    pub start: usize,
    /// Byte offset of the cursor (end of the query).
    pub end: usize,
    /// The text after `@`, up to the cursor.
    pub text: String,
}

fn is_at(c: char) -> bool {
    c == '@' || c == '＠'
}

/// Finds the `@query` that ends at `cursor` (a byte offset). The `@` must
/// start the text or follow a character that is not an ASCII letter or digit
/// (so `a@b.com` is not a mention, but `请@小王` is), and the query has no
/// space.
pub fn mention_query(text: &str, cursor: usize) -> Option<MentionQuery> {
    if cursor > text.len() || !text.is_char_boundary(cursor) {
        return None;
    }
    let before = &text[..cursor];
    let (start, at) = before.char_indices().rev().find(|(_, c)| is_at(*c))?;
    let query = &before[start + at.len_utf8()..];
    if query.chars().any(char::is_whitespace) || query.chars().count() > MAX_QUERY_CHARS {
        return None;
    }
    if let Some(prev) = before[..start].chars().next_back()
        && prev.is_ascii_alphanumeric()
    {
        return None;
    }
    Some(MentionQuery {
        start,
        end: cursor,
        text: query.to_string(),
    })
}

/// The `@name` that ends at `cursor`, with the one space after it when the
/// cursor follows that space: Backspace there deletes the mention whole.
/// `names` are the members a body can mention; the longest match wins, and
/// the `@` follows the same rule as [`mention_query`].
pub fn mention_before(
    text: &str,
    cursor: usize,
    names: &[String],
) -> Option<std::ops::Range<usize>> {
    if cursor > text.len() || !text.is_char_boundary(cursor) {
        return None;
    }
    let before = &text[..cursor];
    let core = before.strip_suffix(' ').unwrap_or(before);
    names
        .iter()
        .filter(|name| !name.is_empty() && core.ends_with(name.as_str()))
        .filter_map(|name| {
            let name_start = core.len() - name.len();
            let (at_start, at) = core[..name_start].char_indices().next_back()?;
            if !is_at(at) {
                return None;
            }
            if let Some(prev) = core[..at_start].chars().next_back()
                && prev.is_ascii_alphanumeric()
            {
                return None;
            }
            Some(at_start..cursor)
        })
        .min_by_key(|range| range.start)
}

/// Members for the @ list: active, not me, in the 群 when its members are
/// known, whose name or 岗位 contains the query. Names that start with the
/// query come first.
pub fn match_members(
    members: &[Member],
    group_members: &[MemberId],
    me: Option<&str>,
    query: &str,
) -> Vec<Member> {
    let query = query.to_lowercase();
    let mut ranked: Vec<(u8, &Member)> = members
        .iter()
        .filter(|m| !m.deactivated && Some(m.id.as_str()) != me)
        .filter(|m| group_members.is_empty() || group_members.contains(&m.id))
        .filter_map(|m| {
            let name = m.name.to_lowercase();
            if query.is_empty() || name.starts_with(&query) {
                Some((0, m))
            } else if name.contains(&query) {
                Some((1, m))
            } else if m
                .role
                .as_ref()
                .is_some_and(|r| r.to_lowercase().contains(&query))
            {
                Some((2, m))
            } else {
                None
            }
        })
        .collect();
    ranked.sort_by(|a, b| a.0.cmp(&b.0).then_with(|| a.1.name.cmp(&b.1.name)));
    ranked
        .into_iter()
        .take(MAX_MENTION_RESULTS)
        .map(|(_, m)| m.clone())
        .collect()
}

/// The member ids a body mentions: at each `@`, the member of the 群 (`group_members`)
/// with the longest name that follows it. When two members share that name, a picked one
/// wins. Each id appears once, in order of first mention. A typed `@name` of a member who is
/// not in the 群 stays plain text: the server refuses a mention outside the 群.
pub fn resolve_mentions(
    body: &str,
    picked: &[MemberId],
    members: &[Member],
    group_members: &[MemberId],
) -> Vec<MemberId> {
    let mut out: Vec<MemberId> = Vec::new();
    for (i, c) in body.char_indices() {
        if !is_at(c) {
            continue;
        }
        let rest = &body[i + c.len_utf8()..];
        let best = members
            .iter()
            .filter(|m| {
                !m.deactivated
                    && !m.name.is_empty()
                    && group_members.contains(&m.id)
                    && rest.starts_with(&m.name)
            })
            .max_by_key(|m| (m.name.len(), picked.contains(&m.id)));
        if let Some(m) = best
            && !out.contains(&m.id)
        {
            out.push(m.id.clone());
        }
    }
    out
}

/// The title of a new topic: the first line with text, cut to 80 characters.
pub fn topic_title(body: &str) -> Option<String> {
    let line = body.lines().map(str::trim).find(|l| !l.is_empty())?;
    Some(line.chars().take(MAX_TITLE_CHARS).collect())
}

/// Limits typing events to one per topic per [`TYPING_INTERVAL`].
#[derive(Default)]
pub struct TypingThrottle {
    last: HashMap<(GroupId, TopicId), Instant>,
}

impl TypingThrottle {
    /// True when a typing event may be sent now (and records it).
    pub fn hit(&mut self, group_id: &str, topic_id: &str, now: Instant) -> bool {
        let key = (group_id.to_string(), topic_id.to_string());
        match self.last.get(&key) {
            Some(at) if now.saturating_duration_since(*at) < TYPING_INTERVAL => false,
            _ => {
                // Keep the map small: forget topics that are past the interval.
                self.last
                    .retain(|_, at| now.saturating_duration_since(*at) < TYPING_INTERVAL);
                self.last.insert(key, now);
                true
            }
        }
    }

    /// After a send the next keystroke is new typing.
    pub fn reset(&mut self, group_id: &str, topic_id: &str) {
        self.last
            .remove(&(group_id.to_string(), topic_id.to_string()));
    }
}

/// My newest message that Up can edit: sent, not deleted, not streaming.
pub fn last_editable<'a>(messages: &'a [Message], me: &str) -> Option<&'a Message> {
    messages.iter().rev().find(|m| {
        m.author_id == me
            && !m.id.is_empty()
            && m.deleted_at.is_none()
            && !m.streaming
            && m.send_state == SendState::Sent
    })
}

/// A short one-line excerpt of a quoted body.
pub fn excerpt(body: &str, max_chars: usize) -> String {
    let line: String = body.split_whitespace().collect::<Vec<_>>().join(" ");
    crate::markdown::clip(&line, max_chars)
}

/// A Markdown format of the format bar.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Format {
    /// Inline marks around the selection (`**`, `_`, `~~`, `` ` ``).
    Wrap(&'static str),
    /// `[text](url)` with `url` selected.
    Link,
    /// A prefix at the start of the selection's first line (`- `, `> `).
    Line(&'static str),
}

/// One edit to the text: replace `range` with `insert`, then select
/// `select` (byte offsets in the new text) or leave the caret after it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FormatEdit {
    pub range: std::ops::Range<usize>,
    pub insert: String,
    pub select: Option<std::ops::Range<usize>>,
}

/// The edit that applies `format` to the selection `range` (`selected` is
/// its text). With nothing selected, inline marks go around an empty
/// selection and the caret lands between them.
pub fn format_edit(
    text: &str,
    range: std::ops::Range<usize>,
    selected: &str,
    format: Format,
) -> FormatEdit {
    match format {
        Format::Wrap(mark) => {
            let start = range.start + mark.len();
            FormatEdit {
                insert: format!("{mark}{selected}{mark}"),
                select: Some(start..start + selected.len()),
                range,
            }
        }
        Format::Link => {
            let url_start = range.start + selected.len() + 3;
            FormatEdit {
                insert: format!("[{selected}](url)"),
                select: Some(url_start..url_start + 3),
                range,
            }
        }
        Format::Line(prefix) => {
            let line_start = text[..range.start].rfind('\n').map_or(0, |i| i + 1);
            let caret = range.end + prefix.len();
            FormatEdit {
                range: line_start..line_start,
                insert: prefix.to_string(),
                select: Some(caret..caret),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::{MemberKind, MemberLevel};

    fn member(id: &str, name: &str, role: &str, kind: MemberKind) -> Member {
        Member {
            id: id.into(),
            kind,
            name: name.into(),
            role: Some(role.into()),
            level: MemberLevel::Member,
            avatar_url: None,
            deactivated: false,
            online: true,
            character: None,
            team_owner: false,
            owner_id: None,
            deleted: false,
        }
    }

    fn sample() -> Vec<Member> {
        let mut gone = member("m-gone", "小李", "实习", MemberKind::Human);
        gone.deactivated = true;
        vec![
            member("m-joye", "Joye", "负责人", MemberKind::Human),
            member("m-wang", "小王", "前端", MemberKind::Human),
            member("m-wangwu", "小王五", "后端", MemberKind::Human),
            member("m-li", "Li", "设计", MemberKind::Human),
            member("m-lisa", "Lisa", "测试", MemberKind::Human),
            member("b-coder", "Coder", "写代码", MemberKind::Bot),
            gone,
        ]
    }

    #[test]
    fn backspace_takes_a_mention_whole() {
        let names: Vec<String> = ["小王", "小王五", "Li", "Lisa"]
            .iter()
            .map(|n| n.to_string())
            .collect();
        let at = |text: &str| mention_before(text, text.len(), &names);
        // With the space the mention ends in, and without it.
        assert_eq!(at("@小王 "), Some(0.."@小王 ".len()));
        assert_eq!(at("请@小王"), Some("请".len().."请@小王".len()));
        // The longest name wins.
        assert_eq!(at("@小王五 "), Some(0.."@小王五 ".len()));
        assert_eq!(at("＠Lisa "), Some(0.."＠Lisa ".len()));
        // Not a mention: a word, an e-mail, two spaces, an unknown name.
        assert_eq!(at("@小王 好"), None);
        assert_eq!(at("a@Li"), None);
        assert_eq!(at("@小王  "), None);
        assert_eq!(at("@小张 "), None);
        assert_eq!(at(""), None);
        // In the middle of the text, at the caret.
        let text = "@Li 你好";
        assert_eq!(mention_before(text, "@Li ".len(), &names), Some(0..4));
        assert_eq!(mention_before(text, 2, &names), None);
    }

    #[test]
    fn finds_the_query_at_the_cursor() {
        let text = "请@小王 看一下";
        let cursor = "请@小王".len();
        let q = mention_query(text, cursor).unwrap();
        assert_eq!(q.text, "小王");
        assert_eq!(q.start, "请".len());
        assert_eq!(q.end, cursor);
        assert_eq!(mention_query("@", 1).unwrap().text, "");
        assert_eq!(
            mention_query("hi ＠co", "hi ＠co".len()).unwrap().text,
            "co"
        );
        // A space ends the query; an e-mail is not a mention.
        assert!(mention_query(text, text.len()).is_none());
        assert!(mention_query("a@b.com", 7).is_none());
        assert!(mention_query("no at", 5).is_none());
        // Not a char boundary or past the end.
        assert!(mention_query("@小王", 2).is_none());
        assert!(mention_query("@", 9).is_none());
    }

    #[test]
    fn matches_members() {
        let members = sample();
        let names = |q: &str, group: &[MemberId]| -> Vec<String> {
            match_members(&members, group, Some("m-joye"), q)
                .into_iter()
                .map(|m| m.name)
                .collect()
        };
        // Not me, not deactivated; prefix first.
        assert_eq!(names("", &[]).len(), 5);
        assert_eq!(names("小", &[]), vec!["小王", "小王五"]);
        assert_eq!(names("li", &[]), vec!["Li", "Lisa"]);
        assert_eq!(names("is", &[]), vec!["Lisa"]);
        // By 岗位, and bots too.
        assert_eq!(names("代码", &[]), vec!["Coder"]);
        // Only members of the 群.
        assert_eq!(
            names("", &["m-li".into(), "b-coder".into()]),
            vec!["Coder", "Li"]
        );
        assert!(names("nobody", &[]).is_empty());
    }

    #[test]
    fn resolves_mention_ids() {
        let members = sample();
        let all: Vec<MemberId> = members.iter().map(|m| m.id.clone()).collect();
        assert_eq!(
            resolve_mentions(
                "@小王五 和 @小王，还有 @Lisa 和 @Li。@小王",
                &[],
                &members,
                &all
            ),
            vec!["m-wangwu", "m-wang", "m-lisa", "m-li"]
        );
        // A removed name is no longer mentioned, even if it was picked.
        assert!(resolve_mentions("没有提及", &["m-wang".into()], &members, &all).is_empty());
        // Deactivated members are not mentioned.
        assert!(resolve_mentions("@小李", &[], &members, &all).is_empty());
        // Same name twice: the picked one wins.
        let mut twins = sample();
        twins.push(member("m-wang2", "小王", "运营", MemberKind::Human));
        let mut both = all.clone();
        both.push("m-wang2".into());
        assert_eq!(
            resolve_mentions("@小王 你好", &["m-wang2".into()], &twins, &both),
            vec!["m-wang2"]
        );
        // A typed name of a member outside the 群 is not a mention (the server refuses those).
        let only_li = vec!["m-li".to_string()];
        assert_eq!(
            resolve_mentions("@Lisa 和 @Li 看看", &[], &members, &only_li),
            vec!["m-li"]
        );
    }

    #[test]
    fn title_is_the_first_line() {
        assert_eq!(topic_title("\n  接口设计  \n正文").unwrap(), "接口设计");
        assert!(topic_title(" \n ").is_none());
        let long = "长".repeat(100);
        assert_eq!(topic_title(&long).unwrap().chars().count(), MAX_TITLE_CHARS);
    }

    #[test]
    fn typing_is_limited_per_topic() {
        let mut t = TypingThrottle::default();
        let t0 = Instant::now();
        assert!(t.hit("g", "t1", t0));
        assert!(!t.hit("g", "t1", t0 + Duration::from_millis(2900)));
        assert!(t.hit("g", "t2", t0 + Duration::from_millis(100)));
        assert!(t.hit("g", "t1", t0 + Duration::from_millis(3000)));
        t.reset("g", "t1");
        assert!(t.hit("g", "t1", t0 + Duration::from_millis(3001)));
    }

    #[test]
    fn excerpt_is_one_short_line() {
        assert_eq!(excerpt("第一行\n第二行", 20), "第一行 第二行");
        assert_eq!(excerpt("abcdef", 3), "abc…");
    }

    #[test]
    fn format_edits_wrap_link_and_prefix() {
        let e = format_edit("ab cd", 3..5, "cd", Format::Wrap("**"));
        assert_eq!(
            (e.range, e.insert.as_str(), e.select),
            (3..5, "**cd**", Some(5..7))
        );
        let e = format_edit("x", 1..1, "", Format::Link);
        assert_eq!((e.insert.as_str(), e.select), ("[](url)", Some(4..7)));
        let e = format_edit("a\nbc", 3..3, "", Format::Line("- "));
        assert_eq!(
            (e.range, e.insert.as_str(), e.select),
            (2..2, "- ", Some(5..5))
        );
    }
}
