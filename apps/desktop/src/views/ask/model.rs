//! What ⌘K 交代或搜索 offers for a query (no view code): who it can be
//! given to, and which 话题, 产出, 成员 and 群 match, with the matched parts
//! that the palette sets in weight 600.

use std::ops::Range;

use crate::data::Member;

/// Words before a name that only say "ask" (让 Ledger …, 请 Forge …).
const LEAD_WORDS: &[&str] = &["让", "请", "叫", "找", "交给", "给", "@"];

/// A match of `text` against `query`: its score (matched characters) and
/// the byte ranges of `text` to set heavier.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Weighed {
    pub score: usize,
    pub ranges: Vec<Range<usize>>,
}

fn solid(c: &char) -> bool {
    !c.is_whitespace()
}

/// Matches `text` against a free-form `query` (a sentence like
/// `让 Ledger 拉一下 10 月各渠道的新增`): every longest run of `text` that
/// also appears in the query, of at least two visible characters (one when
/// the query has one), counts. `None` when nothing counts.
pub fn weigh(text: &str, query: &str) -> Option<Weighed> {
    let q: Vec<char> = query.trim().chars().flat_map(char::to_lowercase).collect();
    let min = q.iter().filter(|c| solid(c)).count().clamp(1, 2);
    if q.is_empty() {
        return None;
    }
    let chars: Vec<(usize, char)> = text.char_indices().collect();
    let lower: Vec<char> = chars
        .iter()
        .map(|(_, c)| c.to_lowercase().next().unwrap_or(*c))
        .collect();
    let contains = |run: &[char]| q.windows(run.len()).any(|w| w == run);
    let mut out = Weighed::default();
    let mut i = 0;
    while i < lower.len() {
        let mut len = 0;
        while i + len < lower.len() && contains(&lower[i..i + len + 1]) {
            len += 1;
        }
        if len == 0 {
            i += 1;
            continue;
        }
        // Trim spaces at both ends of the run.
        let (mut a, mut b) = (i, i + len);
        while a < b && !solid(&lower[a]) {
            a += 1;
        }
        while b > a && !solid(&lower[b - 1]) {
            b -= 1;
        }
        let visible = lower[a..b].iter().filter(|c| solid(c)).count();
        // Latin letters and digits alone need three (`ge` of Forge is not
        // a match for Ledger); any run with Han or other script needs two;
        // the whole query always counts.
        let ascii = lower[a..b].iter().all(|c| c.is_ascii());
        let whole = b - a == q.len() && lower[a..b] == q[..];
        if visible >= min && (!ascii || visible >= 3 || whole) {
            out.score += visible;
            let start = chars[a].0;
            let end = chars.get(b).map(|(at, _)| *at).unwrap_or(text.len());
            out.ranges.push(start..end);
            i += len;
        } else {
            i += 1;
        }
    }
    (out.score > 0).then_some(out)
}

/// The members a query names (`让 Ledger …`), in the order they appear.
pub fn named<'a>(query: &str, members: &'a [Member], me: Option<&str>) -> Vec<&'a Member> {
    let q = query.to_lowercase();
    let mut found: Vec<(usize, &Member)> = members
        .iter()
        .filter(|m| !m.deactivated && Some(m.id.as_str()) != me && !m.name.trim().is_empty())
        .filter_map(|m| q.find(&m.name.to_lowercase()).map(|at| (at, m)))
        .collect();
    found.sort_by_key(|(at, _)| *at);
    found.into_iter().map(|(_, m)| m).collect()
}

/// The work in a query once the member is taken out: `让 Ledger 拉一下 10
/// 月各渠道的新增` → `拉一下 10 月各渠道的新增`. The whole query when no
/// name is in it.
pub fn task(query: &str, name: Option<&str>) -> String {
    let query = query.trim();
    let Some(name) = name else {
        return query.to_string();
    };
    let lower = query.to_lowercase();
    let Some(at) = lower.find(&name.to_lowercase()) else {
        return query.to_string();
    };
    let before = query[..at].trim();
    let after = query[at + name.len()..]
        .trim_start_matches([',', '，', ':', '：', ' '])
        .trim();
    // `帮我让 Ledger …` keeps `帮我`; a lone `让` goes.
    let before = LEAD_WORDS
        .iter()
        .find_map(|w| before.strip_suffix(w))
        .unwrap_or(before)
        .trim();
    let text = format!("{before} {after}");
    let text = text.trim();
    if text.is_empty() {
        query.to_string()
    } else {
        text.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fake::v2::{self as d};

    const ASK: &str = "让 Ledger 拉一下 10 月各渠道的新增";

    #[test]
    fn a_sentence_marks_the_shared_words() {
        let w = weigh("10 月数据月报", ASK).unwrap();
        assert_eq!(w.ranges, vec![0..6]);
        assert_eq!(&"10 月数据月报"[w.ranges[0].clone()], "10 月");
        // One shared character is not a match.
        assert!(weigh("夹夹 4.0 需求", ASK).is_none());
        // Two Latin letters of a sentence are not a match.
        assert!(weigh("Forge", ASK).is_none());
        assert!(weigh("Alex Chen", ASK).is_none());
        // A short query matches as typed, ignoring case.
        let w = weigh("Ledger", "led").unwrap();
        assert_eq!(w.ranges, vec![0..3]);
        assert_eq!(weigh("话题列表虚拟滚动", "话题").unwrap().score, 2);
    }

    #[test]
    fn names_and_the_task_come_out_of_the_query() {
        let members = d::members();
        let found = named(ASK, &members, Some(d::ME));
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].id, d::LEDGER);
        assert_eq!(task(ASK, Some("Ledger")), "拉一下 10 月各渠道的新增");
        assert_eq!(task("Forge，修一下登录", Some("Forge")), "修一下登录");
        assert_eq!(task("今天的周报", None), "今天的周报");
        // I am never offered.
        assert!(named("Joye 看一下", &members, Some(d::ME)).is_empty());
    }
}
