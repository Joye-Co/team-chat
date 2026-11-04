//! Search and bot runs for [`crate::fake::FakeProvider`]: a substring search
//! over the sample messages and three sample runs with their process steps.

use crate::data::{
    BotRunSummary, FileChangeKind, FileChangeView, Message, Millis, PlanItem, PlanState,
    ProcessStep, RawRunEntry, RunProcess, RunStatus, SearchHit, Topic,
};

/// Characters of context on each side of a match in a snippet.
const CONTEXT: usize = 24;

/// A case-insensitive substring search over message bodies and topic titles,
/// newest first, like the server's (`docs/protocol.md`, "Search").
#[allow(clippy::single_range_in_vec_init)] // one highlight per fake hit
pub fn search(
    topics: &[Topic],
    messages: &[&Message],
    query: &str,
    group_id: Option<&str>,
    limit: usize,
) -> Vec<SearchHit> {
    let needle: Vec<char> = query.trim().to_lowercase().chars().collect();
    if needle.is_empty() {
        return Vec::new();
    }
    let mut hits: Vec<SearchHit> = Vec::new();
    for m in messages {
        if m.deleted_at.is_some() || m.id.is_empty() || group_id.is_some_and(|g| g != m.group_id) {
            continue;
        }
        let topic = topics.iter().find(|t| t.id == m.topic_id);
        let title_hit = topic
            .filter(|t| t.root_message_id == m.id)
            .and_then(|t| find(&t.title, &needle).map(|at| (t.title.clone(), at)));
        let found = find(&m.body, &needle)
            .map(|at| (m.body.clone(), at))
            .or(title_hit);
        if let Some((text, at)) = found {
            let (snippet, start) = snippet(&text, at, needle.len());
            hits.push(SearchHit {
                group_id: m.group_id.clone(),
                topic_id: m.topic_id.clone(),
                message_id: m.id.clone(),
                author_id: m.author_id.clone(),
                snippet,
                highlights: vec![start..start + needle.len()],
                created_at: m.created_at,
                seq: m.created_seq,
            });
        }
    }
    hits.sort_by(|a, b| b.created_at.cmp(&a.created_at).then(b.seq.cmp(&a.seq)));
    hits.truncate(limit);
    hits
}

/// Code-point index of the first match of `needle` (lowercase) in `text`.
fn find(text: &str, needle: &[char]) -> Option<usize> {
    let hay: Vec<char> = text.to_lowercase().chars().collect();
    if hay.len() != text.chars().count() {
        // Lowercasing changed the length; fall back to an exact match.
        let exact: Vec<char> = text.chars().collect();
        return exact.windows(needle.len()).position(|w| w == needle);
    }
    hay.windows(needle.len()).position(|w| w == needle)
}

/// The text around a match, and where the match starts in it (code points).
fn snippet(text: &str, at: usize, len: usize) -> (String, usize) {
    let chars: Vec<char> = text
        .chars()
        .map(|c| if c == '\n' { ' ' } else { c })
        .collect();
    let start = snippet_start(&chars, at);
    let end = (at + len + CONTEXT).min(chars.len());
    let rest: String = chars[start..].iter().collect();
    let mut out = crate::markdown::clip(&rest, end - start);
    let mut offset = at - start;
    if start > 0 {
        out.insert(0, '…');
        offset += 1;
    }
    (out, offset)
}

/// Characters a snippet start may move to find a word boundary.
const BOUNDARY_REACH: usize = 12;

/// Whether a snippet may start right after `c` without cutting a word
/// (DESIGN.md §15): a space, or ASCII or CJK punctuation.
fn is_boundary(c: char) -> bool {
    c.is_whitespace()
        || c.is_ascii_punctuation()
        || "，。！？；：、（）【】「」『』《》〈〉“”‘’…—·～".contains(c)
}

/// Where a snippet of `chars` around a match at `at` starts: `CONTEXT`
/// characters before it, moved to the nearest word or punctuation boundary
/// within `BOUNDARY_REACH` (forward first, never past the match), else 8
/// characters before the match.
fn snippet_start(chars: &[char], at: usize) -> usize {
    let ideal = at.saturating_sub(CONTEXT);
    if ideal == 0 {
        return 0;
    }
    let ok = |i: usize| i == 0 || is_boundary(chars[i - 1]) && !chars[i].is_whitespace();
    let forward = (ideal..=(ideal + BOUNDARY_REACH).min(at)).find(|&i| ok(i));
    let back = || {
        (ideal.saturating_sub(BOUNDARY_REACH)..ideal)
            .rev()
            .find(|&i| ok(i))
    };
    forward.or_else(back).unwrap_or(at.saturating_sub(8))
}

/// Sample runs: a running run in `t-login` and a failed one in `t-blocked`.
pub fn sample_runs(now: Millis) -> Vec<BotRunSummary> {
    const MIN: i64 = 60_000;
    vec![
        BotRunSummary {
            id: "run-login-1".into(),
            bot_id: "m-coder".into(),
            group_id: "g-dev".into(),
            topic_id: "t-login".into(),
            machine_id: Some("mac-mini-1".into()),
            machine_name: Some("Mac mini（办公室）".into()),
            status: RunStatus::Running,
            requested_by: Some("m-joye".into()),
            created_at: now - 179 * MIN,
            started_at: Some(now - 178 * MIN),
            ended_at: None,
            updated_at: now - 10 * MIN,
            summary: None,
            plan: Vec::new(),
        },
        BotRunSummary {
            id: "run-blocked-1".into(),
            bot_id: "m-deploy".into(),
            group_id: "g-dev".into(),
            topic_id: "t-blocked".into(),
            machine_id: Some("mac-mini-1".into()),
            machine_name: Some("Mac mini（办公室）".into()),
            status: RunStatus::Failed,
            requested_by: Some("m-joye".into()),
            created_at: now - 2 * 24 * 60 * MIN,
            started_at: Some(now - 2 * 24 * 60 * MIN + MIN),
            ended_at: Some(now - 2 * 24 * 60 * MIN + 7 * MIN),
            updated_at: now - 2 * 24 * 60 * MIN + 7 * MIN,
            summary: Some("打包失败：找不到签名证书。".into()),
            plan: Vec::new(),
        },
        // The finished run of the 私聊 话题 `t-dm-2` (完成): its plan and its
        // time, so the inspector agrees with the 完成 badge.
        BotRunSummary {
            id: "run-dm-cache-1".into(),
            bot_id: "m-coder".into(),
            group_id: "d-coder".into(),
            topic_id: "t-dm-2".into(),
            machine_id: Some("mac-mini-1".into()),
            machine_name: Some("Mac mini（办公室）".into()),
            status: RunStatus::Done,
            requested_by: Some("m-joye".into()),
            created_at: now - 8 * 60 * MIN,
            started_at: Some(now - 8 * 60 * MIN + MIN),
            ended_at: Some(now - 7 * 60 * MIN),
            updated_at: now - 7 * 60 * MIN,
            summary: Some("完成，释放了 42 GB。".into()),
            plan: vec![
                PlanItem::new("找出超过 7 天的构建缓存", PlanState::Done),
                PlanItem::new("删除并统计释放的空间", PlanState::Done),
            ],
        },
    ]
}

/// The process of a sample run, as a real provider would build it.
pub fn sample_process(run_id: &str) -> RunProcess {
    let steps = match run_id {
        "run-login-1" => vec![
            ProcessStep::TurnStarted,
            ProcessStep::Message {
                text: "登录页需要支持两种方式：邀请码和粘贴已有的登录凭证。".into(),
                from_bot: false,
                streaming: false,
            },
            ProcessStep::Reasoning {
                text: "先看现有的登录视图和错误类型，再加 Keychain 存储。".into(),
            },
            ProcessStep::Tool {
                title: "rg -n \"LoginError\" apps/desktop/src".into(),
                input: Some(r#"{"command":"rg -n \"LoginError\" apps/desktop/src"}"#.into()),
                ok: Some(true),
                output: Some(
                    "apps/desktop/src/data.rs:262:pub enum LoginError {\napps/desktop/src/views/login.rs:40:    LoginError::BadCredential =>"
                        .into(),
                ),
                exit_code: Some(0),
            },
            ProcessStep::Files {
                changes: vec![
                    FileChangeView {
                        path: "apps/desktop/src/views/login.rs".into(),
                        kind: FileChangeKind::Modified,
                    },
                    FileChangeView {
                        path: "apps/desktop/src/provider/keychain.rs".into(),
                        kind: FileChangeKind::Added,
                    },
                ],
            },
            ProcessStep::Tool {
                title: "cargo test -p desktop login".into(),
                input: None,
                ok: Some(false),
                output: Some("error[E0425]: cannot find value `entry` in this scope\n  --> src/provider/keychain.rs:18:9".into()),
                exit_code: Some(101),
            },
            ProcessStep::Approval {
                title: "要在钥匙串里新建一个条目吗？".into(),
                detail: Some("security add-generic-password -s chat -a staging".into()),
                approved: Some(true),
            },
            ProcessStep::Message {
                text: "进展：表单和校验已完成，正在接 Keychain。".into(),
                from_bot: true,
                streaming: true,
            },
        ],
        "run-blocked-1" => vec![
            ProcessStep::TurnStarted,
            ProcessStep::Tool {
                title: "scripts/build-dmg.sh --sign".into(),
                input: None,
                ok: Some(false),
                output: Some("codesign: no identity found".into()),
                exit_code: Some(1),
            },
            ProcessStep::TurnCompleted {
                ok: false,
                error: Some("签名证书不存在".into()),
            },
            ProcessStep::Done {
                status: RunStatus::Failed,
                summary: Some("打包失败：找不到签名证书。".into()),
                error: None,
                duration_ms: 6 * 60_000,
            },
        ],
        _ => Vec::new(),
    };
    let raw = steps
        .iter()
        .enumerate()
        .map(|(i, s)| RawRunEntry {
            seq: i as u64 + 1,
            json: format!("{s:?}"),
        })
        .collect();
    RunProcess {
        last_seq: steps.len() as u64,
        steps,
        raw,
        session: Some(("sess-0001".into(), "claude".into())),
        loading: false,
        loaded: true,
        error: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snippet_marks_the_match() {
        let (s, at) = snippet("登录页需要支持邀请码", 7, 3);
        assert_eq!(s, "登录页需要支持邀请码");
        assert_eq!(s.chars().skip(at).take(3).collect::<String>(), "邀请码");
        let long = format!("{}关键词{}", "前".repeat(40), "后".repeat(40));
        let (s, at) = snippet(&long, 40, 3);
        assert!(s.starts_with('…') && s.ends_with('…'));
        assert_eq!(s.chars().skip(at).take(3).collect::<String>(), "关键词");
    }

    #[test]
    fn snippet_starts_at_a_boundary() {
        // DESIGN.md §15: no '…明（草稿）', the cut moves past the bracket.
        let text = "这是一段比较长的产品说明（草稿）新功能的话题会在这里讨论，请大家看话题列表";
        let at = text.chars().position(|c| c == '列').unwrap() - 2;
        let (s, i) = snippet(text, at, 2);
        assert!(s.starts_with("…草稿）"), "{s}");
        assert_eq!(s.chars().skip(i).take(2).collect::<String>(), "话题");
        let latin = format!("{} release notes for the parser", "word ".repeat(10));
        let at = latin.find("parser").unwrap();
        let (s, _) = snippet(&latin, at, 6);
        let first = s.trim_start_matches('…').split(' ').next().unwrap();
        assert!(
            ["word", "release", "notes", "for", "the"].contains(&first),
            "{s}"
        );
    }
}
