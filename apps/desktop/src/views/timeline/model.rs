//! Pure timeline logic, kept apart from GPUI so it can be unit tested:
//! message identity, author grouping, the new-messages divider and the list
//! updates that turn one message window into the next.

use std::ops::Range;

use crate::data::{Message, Millis};

/// Consecutive messages of one author within this gap share one header.
pub const GROUP_GAP_MS: Millis = 5 * 60 * 1000;

/// The stable identity of a message row. `client_msg_id` survives the ack
/// (the server id is empty while pending); messages without one fall back to
/// the server id.
pub fn key(message: &Message) -> &str {
    if message.client_msg_id.is_empty() {
        &message.id
    } else {
        &message.client_msg_id
    }
}

/// True when `message` continues the group of `prev` (same author, within
/// [`GROUP_GAP_MS`], and neither is deleted), so it shows no avatar or name.
pub fn continues_group(prev: &Message, message: &Message) -> bool {
    prev.author_id == message.author_id
        && prev.deleted_at.is_none()
        && message.deleted_at.is_none()
        && message.created_at >= prev.created_at
        && message.created_at - prev.created_at <= GROUP_GAP_MS
}

/// The key of the first unread message, given the topic's unread count: the
/// `unread`-th newest message not written by me (pending rows do not count).
/// `None` when nothing is unread or the first unread message is older than
/// the window.
pub fn first_unread_key(messages: &[Message], unread: u32, me: Option<&str>) -> Option<String> {
    if unread == 0 {
        return None;
    }
    let mut left = unread;
    for message in messages.iter().rev() {
        if Some(message.author_id.as_str()) == me || message.created_seq == 0 {
            continue;
        }
        left -= 1;
        if left == 0 {
            return Some(key(message).to_string());
        }
    }
    None
}

/// Key of the first unread message: the oldest message after my read
/// position that someone else wrote and that is not deleted. When every
/// message of the window is unread and older ones exist, the first unread
/// message is older than the window: the divider goes on the first row.
pub fn first_unread_after(
    messages: &[Message],
    read_seq: u64,
    has_older: bool,
    me: Option<&str>,
) -> Option<String> {
    let first = messages.iter().find(|m| {
        m.created_seq > read_seq && m.deleted_at.is_none() && Some(m.author_id.as_str()) != me
    })?;
    let all_unread = messages
        .iter()
        .filter(|m| m.created_seq > 0)
        .all(|m| m.created_seq > read_seq);
    if all_unread && has_older {
        return messages.first().map(|m| key(m).to_string());
    }
    Some(key(first).to_string())
}

/// One change to the GPUI list state.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ListOp {
    /// Forget everything and show `count` rows (scroll position is lost).
    Reset(usize),
    /// Rows in `range` (old indices) became `count` new rows.
    Splice { range: Range<usize>, count: usize },
    /// The row at this (new) index changed content and must be measured
    /// again; its old height stays as the estimate.
    Remeasure(usize),
}

/// The list updates that turn the rows of `old` into the rows of `new`.
///
/// Rows are matched by [`key`]. The common prefix and suffix keep their
/// measured heights (changed ones are remeasured), and the differing middle
/// becomes one splice, so prepends (older page), appends (new messages),
/// trims and an ack that moves a row keep the scroll position.
pub fn diff(old: &[Message], new: &[Message]) -> Vec<ListOp> {
    if old.is_empty() || new.is_empty() {
        return if old.len() == new.len() {
            Vec::new()
        } else {
            vec![ListOp::Reset(new.len())]
        };
    }
    let max = old.len().min(new.len());
    let prefix = old
        .iter()
        .zip(new)
        .take_while(|(a, b)| key(a) == key(b))
        .count();
    let suffix = old
        .iter()
        .rev()
        .zip(new.iter().rev())
        .take(max - prefix)
        .take_while(|(a, b)| key(a) == key(b))
        .count();
    if prefix == 0 && suffix == 0 {
        // Nothing in common: a different window (for example a jump).
        return vec![ListOp::Reset(new.len())];
    }

    let mut ops = Vec::new();
    for ix in 0..prefix {
        if old[ix] != new[ix] {
            ops.push(ListOp::Remeasure(ix));
        }
    }
    let old_mid = prefix..old.len() - suffix;
    let new_mid_len = new.len() - suffix - prefix;
    let spliced = !old_mid.is_empty() || new_mid_len > 0;
    if spliced {
        ops.push(ListOp::Splice {
            range: old_mid,
            count: new_mid_len,
        });
    }
    let new_suffix_start = new.len() - suffix;
    for j in 0..suffix {
        let (o, n) = (old.len() - suffix + j, new_suffix_start + j);
        let header_changed = j == 0 && spliced;
        if old[o] != new[n] || header_changed {
            // The first row after a splice has a new neighbor above it, so its
            // grouping (header or not) may have changed.
            ops.push(ListOp::Remeasure(n));
        }
    }
    ops
}

/// The local day of a time.
pub fn local_day(at: Millis) -> Option<chrono::NaiveDate> {
    use chrono::TimeZone as _;
    chrono::Local
        .timestamp_millis_opt(at)
        .single()
        .map(|t| t.date_naive())
}

/// A date divider goes above `message`: its day differs from the day of the
/// message above it (or it is the first row).
pub fn starts_day(prev: Option<&Message>, message: &Message) -> bool {
    match prev {
        None => true,
        Some(prev) => local_day(prev.created_at) != local_day(message.created_at),
    }
}

/// The date divider label (§8.2): `今天`, `昨天`, `周一` within a week,
/// `10月28日 周二` this year, `2024年12月31日` before.
pub fn day_label(day: chrono::NaiveDate, today: chrono::NaiveDate) -> String {
    use crate::copy::{fill, timeline as t};
    use chrono::Datelike as _;
    const WEEKDAYS: [&str; 7] = ["周一", "周二", "周三", "周四", "周五", "周六", "周日"];
    let weekday = WEEKDAYS[day.weekday().num_days_from_monday() as usize];
    let ago = (today - day).num_days();
    if ago == 0 {
        t::TODAY.to_string()
    } else if ago == 1 {
        t::YESTERDAY.to_string()
    } else if (2..7).contains(&ago) {
        weekday.to_string()
    } else if day.year() == today.year() {
        fill(
            t::DATE_THIS_YEAR,
            &[
                ("m", &day.month().to_string()),
                ("d", &day.day().to_string()),
                ("w", weekday),
            ],
        )
    } else {
        fill(
            t::DATE_OLDER,
            &[
                ("y", &day.year().to_string()),
                ("m", &day.month().to_string()),
                ("d", &day.day().to_string()),
            ],
        )
    }
}

/// The time in a message's meta line: always `HH:MM` (§8.2; the day is in
/// the date divider above it).
pub fn clock(at: Millis) -> String {
    use chrono::TimeZone as _;
    chrono::Local
        .timestamp_millis_opt(at)
        .single()
        .map(|t| t.format("%H:%M").to_string())
        .unwrap_or_default()
}

/// A body of one to three emoji and nothing else: drawn at 32/40 (Slack).
pub fn is_jumbo(body: &str) -> bool {
    let body = body.trim();
    if body.is_empty() || body.chars().count() > 24 {
        return false;
    }
    let mut emoji = 0;
    let mut chars = body.chars().peekable();
    while let Some(c) = chars.next() {
        if is_pictograph(c) {
            emoji += 1;
            // Skin tones, variation selectors and ZWJ sequences stay one emoji.
            while let Some(&n) = chars.peek() {
                if n == '\u{200d}' {
                    chars.next();
                    chars.next();
                } else if matches!(n, '\u{fe0f}' | '\u{1f3fb}'..='\u{1f3ff}' | '\u{20e3}') {
                    chars.next();
                } else {
                    break;
                }
            }
        } else {
            return false;
        }
    }
    (1..=3).contains(&emoji)
}

fn is_pictograph(c: char) -> bool {
    matches!(c,
        '\u{1f300}'..='\u{1faff}'
        | '\u{2600}'..='\u{27bf}'
        | '\u{1f000}'..='\u{1f2ff}'
        | '\u{2b00}'..='\u{2bff}'
        | '\u{2300}'..='\u{23ff}')
}

/// Rows appended at the end by `ops` (for the jump-to-latest count).
pub fn appended(old: &[Message], new: &[Message]) -> usize {
    match old.last() {
        None => 0,
        Some(last) => match new.iter().rposition(|m| key(m) == key(last)) {
            Some(ix) => new.len() - ix - 1,
            None => 0,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::SendState;

    fn msg(id: &str, author: &str, at: Millis) -> Message {
        Message {
            id: id.into(),
            client_msg_id: format!("c-{id}"),
            group_id: "g".into(),
            topic_id: "t".into(),
            author_id: author.into(),
            body: format!("body {id}"),
            mentions: Vec::new(),
            created_seq: at as u64,
            created_at: at,
            edited_at: None,
            deleted_at: None,
            streaming: false,
            send_state: SendState::Sent,
            quote_of: None,
            attachments: Vec::new(),
            artifact_refs: Vec::new(),
        }
    }

    fn many(ids: &[&str]) -> Vec<Message> {
        ids.iter()
            .map(|id| msg(id, "a", id.parse::<Millis>().unwrap() + 1))
            .collect()
    }

    #[test]
    fn append_and_prepend_are_splices() {
        let old = many(&["1", "2", "3"]);
        let appended_new = many(&["1", "2", "3", "4", "5"]);
        assert_eq!(
            diff(&old, &appended_new),
            vec![ListOp::Splice {
                range: 3..3,
                count: 2
            }]
        );
        assert_eq!(appended(&old, &appended_new), 2);

        let prepended = many(&["0", "1", "2", "3"]);
        assert_eq!(
            diff(&old, &prepended),
            vec![
                ListOp::Splice {
                    range: 0..0,
                    count: 1
                },
                ListOp::Remeasure(1)
            ]
        );
        assert_eq!(appended(&old, &prepended), 0);
    }

    #[test]
    fn trim_and_edit() {
        let old = many(&["1", "2", "3", "4"]);
        let mut new = many(&["1", "2", "3", "4"]);
        new.drain(..2);
        assert_eq!(
            diff(&old, &new),
            vec![
                ListOp::Splice {
                    range: 0..2,
                    count: 0
                },
                ListOp::Remeasure(0)
            ]
        );

        let mut edited = old.clone();
        edited[1].body = "changed".into();
        assert_eq!(diff(&old, &edited), vec![ListOp::Remeasure(1)]);
        assert_eq!(diff(&old, &old), Vec::<ListOp>::new());
    }

    #[test]
    fn unrelated_window_resets() {
        assert_eq!(
            diff(&many(&["1", "2"]), &many(&["8", "9"])),
            vec![ListOp::Reset(2)]
        );
        assert_eq!(diff(&[], &many(&["1"])), vec![ListOp::Reset(1)]);
    }

    #[test]
    fn ack_keeps_the_row() {
        let mut pending = msg("", "me", 5);
        pending.client_msg_id = "c-x".into();
        pending.send_state = SendState::Pending;
        let mut acked = pending.clone();
        acked.id = "m-x".into();
        acked.send_state = SendState::Sent;
        let old = vec![msg("1", "a", 1), pending];
        let new = vec![msg("1", "a", 1), acked];
        assert_eq!(diff(&old, &new), vec![ListOp::Remeasure(1)]);
    }

    #[test]
    fn grouping_needs_same_author_within_five_minutes() {
        let a = msg("1", "a", 0);
        assert!(continues_group(&a, &msg("2", "a", GROUP_GAP_MS)));
        assert!(!continues_group(&a, &msg("2", "a", GROUP_GAP_MS + 1)));
        assert!(!continues_group(&a, &msg("2", "b", 10)));
        let mut deleted = msg("2", "a", 10);
        deleted.deleted_at = Some(11);
        assert!(!continues_group(&a, &deleted));
    }

    #[test]
    fn divider_skips_my_messages() {
        let list = vec![
            msg("1", "a", 1),
            msg("2", "b", 2),
            msg("3", "me", 3),
            msg("4", "b", 4),
        ];
        assert_eq!(first_unread_key(&list, 0, Some("me")), None);
        assert_eq!(
            first_unread_key(&list, 1, Some("me")).as_deref(),
            Some("c-4")
        );
        assert_eq!(
            first_unread_key(&list, 2, Some("me")).as_deref(),
            Some("c-2")
        );
        assert_eq!(first_unread_key(&list, 9, Some("me")), None);
    }

    #[test]
    fn day_labels_follow_the_spec() {
        let d = |y, m, dd| chrono::NaiveDate::from_ymd_opt(y, m, dd).unwrap();
        let today = d(2025, 11, 4);
        assert_eq!(day_label(today, today), "今天");
        assert_eq!(day_label(d(2025, 11, 3), today), "昨天");
        assert_eq!(day_label(d(2025, 11, 1), today), "周六");
        assert_eq!(day_label(d(2025, 10, 28), today), "10月28日 周二");
        assert_eq!(day_label(d(2024, 12, 31), today), "2024年12月31日");
    }

    #[test]
    fn jumbo_is_one_to_three_emoji_only() {
        assert!(is_jumbo("👍👍👍"));
        assert!(is_jumbo("🧑‍💻"));
        assert!(is_jumbo(" ✅ "));
        assert!(!is_jumbo("👍👍👍👍"));
        assert!(!is_jumbo("好的👍"));
        assert!(!is_jumbo(""));
    }
}
