//! 机 5.4: the last warning and error lines of the daemon's log, kept in memory and sent with
//! the heartbeat (`metrics.problems`), so the machine's page in the app shows 最近的问题 without
//! anyone logging in to read the log. Lines are redacted when they leave (the daemon's redactor
//! then knows every key saved since the line was logged).

use crate::proto::{DaemonProblem, MAX_DAEMON_PROBLEMS, MAX_PROBLEM_CHARS};
use crate::redact::Redactor;
use std::collections::VecDeque;
use std::sync::Mutex;

static RECENT: Mutex<VecDeque<DaemonProblem>> = Mutex::new(VecDeque::new());

/// Keeps one problem line, logged `at` (machine clock); the oldest goes past the limit.
pub fn record(at: u64, line: &str) {
    let text = line.trim();
    if text.is_empty() {
        return;
    }
    let mut recent = RECENT.lock().unwrap_or_else(|e| e.into_inner());
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
