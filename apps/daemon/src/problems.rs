//! 机 5.4: the last warning and error lines of the daemon's log, kept in memory and sent with
//! the heartbeat (`metrics.problems`), so the machine's page in the app shows 最近的问题 without
//! anyone logging in to read the log. Lines are redacted when they leave (the daemon's redactor
//! then knows every key saved since the line was logged).

use crate::proto::{DaemonProblem, MAX_DAEMON_PROBLEMS, MAX_PROBLEM_CHARS};
use crate::redact::Redactor;
use std::collections::VecDeque;
use std::sync::Mutex;

static RECENT: Mutex<VecDeque<DaemonProblem>> = Mutex::new(VecDeque::new());

/// The time of a log line: UTC, `2026-11-18T03:20:00.123Z`, from milliseconds since the epoch.
pub fn stamp(ms: u64) -> String {
    let (secs, millis) = (ms / 1000, ms % 1000);
    let (days, rem) = (secs / 86_400, secs % 86_400);
    // Days since 1970-01-01 to a date of the Gregorian calendar (eras of 400 years that start
    // on a 1 March).
    let z = days + 719_468;
    let (era, doe) = (z / 146_097, z % 146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + u64::from(month <= 2);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}.{millis:03}Z",
        rem / 3600,
        rem % 3600 / 60,
        rem % 60
    )
}

/// One line of the daemon's log (stderr), with its time.
pub fn log(line: std::fmt::Arguments) {
    eprintln!("{} {line}", stamp(crate::proto::now_ms()));
}

/// A log line that is a warning or an error: also kept for 最近的问题. Never pass a secret: the
/// log itself is not redacted.
pub fn warn(line: std::fmt::Arguments) {
    let at = crate::proto::now_ms();
    let line = line.to_string();
    eprintln!("{} {line}", stamp(at));
    record(at, &line);
}

/// Keeps one problem line, logged `at` (machine clock); the oldest goes past the limit. The
/// same line again right after itself (the link down on every retry of an outage) only moves
/// the kept line's time, so it does not push every other line out.
pub fn record(at: u64, line: &str) {
    keep(
        &mut RECENT.lock().unwrap_or_else(|e| e.into_inner()),
        at,
        line,
    );
}

fn keep(recent: &mut VecDeque<DaemonProblem>, at: u64, line: &str) {
    let text = line.trim();
    if text.is_empty() {
        return;
    }
    if let Some(last) = recent.back_mut()
        && last.text == text
    {
        last.at = at;
        return;
    }
    while recent.len() >= MAX_DAEMON_PROBLEMS {
        recent.pop_front();
    }
    recent.push_back(DaemonProblem {
        at,
        text: text.to_string(),
    });
}

/// The kept lines, oldest first, redacted and cut to the protocol's length.
pub fn recent(redactor: &Redactor) -> Vec<DaemonProblem> {
    RECENT
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .iter()
        .filter_map(|p| {
            // The server counts UTF-16 units: a longer line would cost the whole heartbeat its
            // metrics.
            let mut units = 0;
            let text: String = redactor
                .text(&p.text)
                .chars()
                .take_while(|c| {
                    units += c.len_utf16();
                    units <= MAX_PROBLEM_CHARS
                })
                .collect();
            let text = text.trim().to_string();
            (!text.is_empty()).then_some(DaemonProblem { at: p.at, text })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stamps_are_utc_dates() {
        assert_eq!(stamp(0), "1970-01-01T00:00:00.000Z");
        assert_eq!(stamp(951_782_400_000), "2000-02-29T00:00:00.000Z");
        assert_eq!(stamp(1_709_251_199_999), "2024-02-29T23:59:59.999Z");
        assert_eq!(stamp(1_709_251_200_000), "2024-03-01T00:00:00.000Z");
        assert_eq!(stamp(1_779_074_400_123), "2026-05-18T03:20:00.123Z");
        assert_eq!(stamp(4_102_444_799_000), "2099-12-31T23:59:59.000Z");
    }

    #[test]
    fn the_same_line_again_only_moves_its_time() {
        let mut kept = VecDeque::new();
        keep(&mut kept, 1, "link down: connect timed out");
        keep(&mut kept, 2, "key OPENAI: refused");
        for at in 3..200 {
            keep(&mut kept, at, " link down: connect timed out ");
        }
        // Not the last line: kept as a new one.
        keep(&mut kept, 200, "key OPENAI: refused");
        let got: Vec<(u64, &str)> = kept.iter().map(|p| (p.at, p.text.as_str())).collect();
        assert_eq!(
            got,
            [
                (1, "link down: connect timed out"),
                (2, "key OPENAI: refused"),
                (199, "link down: connect timed out"),
                (200, "key OPENAI: refused"),
            ]
        );
    }

    #[test]
    fn keeps_the_last_lines_redacted_and_cut() {
        let secret = "sk-test-0123456789abcdefghijklmnop".to_string();
        let r = Redactor::new(vec![secret.clone()]);
        for i in 0..(MAX_DAEMON_PROBLEMS + 5) {
            record(i as u64, &format!("problem {i}"));
        }
        record(1000, "   ");
        record(1001, &format!("key OPENAI: refused {secret}"));
        record(1002, &"x".repeat(MAX_PROBLEM_CHARS + 50));
        record(1003, &"🙂".repeat(MAX_PROBLEM_CHARS));
        let got = recent(&r);
        // Other tests may log meanwhile: look the lines up rather than count on their place.
        assert_eq!(got.len(), MAX_DAEMON_PROBLEMS);
        let key = got.iter().find(|p| p.at == 1001).expect("the key line");
        assert!(!key.text.contains(&secret), "{}", key.text);
        let long = got.iter().find(|p| p.at == 1002).expect("the long line");
        assert_eq!(long.text.chars().count(), MAX_PROBLEM_CHARS);
        let wide = got.iter().find(|p| p.at == 1003).expect("the wide line");
        assert_eq!(wide.text.encode_utf16().count(), MAX_PROBLEM_CHARS);
        assert!(got.iter().all(|p| p.at != 1000));
        // The oldest went first.
        assert!(got.iter().all(|p| p.text != "problem 0"));
    }
}
