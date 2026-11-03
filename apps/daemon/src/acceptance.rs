//! Acceptance criteria (验收标准) a CLI reports in its own text.
//!
//! The daemon has no tool server yet, so a CLI reports its criteria as a fenced block with the
//! info string `acceptance` in an assistant message:
//!
//! ````text
//! ```acceptance
//! [{ "text": "Tests pass", "state": "pass", "evidence": { "text": "cargo test: 42 passed" } },
//!  { "text": "Deployed to staging", "state": "pending" }]
//! ```
//! ````
//!
//! (`{ "items": [...] }` is read too.) The last block of a message wins. Items are cut to the
//! protocol limits (text 1..500 characters, evidence text <= 1000, at most 20 items); unknown
//! states and malformed evidence are dropped. [`AcceptanceTracker`] sends the `acceptance` run
//! event only when the list changed.

use crate::proto::RunEvent;
use chat_protocol::bots::{
    AcceptanceEvidence, AcceptanceItem, AcceptanceState, MAX_ACCEPTANCE_ITEMS,
};
use chat_protocol::message::ArtifactRef;
use serde_json::Value;

const MAX_TEXT: usize = 500;
const MAX_EVIDENCE_TEXT: usize = 1000;
const MAX_URL: usize = 2048;

fn cut(s: &str, max: usize) -> String {
    s.trim().chars().take(max).collect()
}

fn id_ok(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 64
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

fn state(s: &str) -> Option<AcceptanceState> {
    match s.trim().to_ascii_lowercase().as_str() {
        "pending" | "todo" | "open" => Some(AcceptanceState::Pending),
        "pass" | "passed" | "met" | "done" | "ok" => Some(AcceptanceState::Pass),
        "fail" | "failed" | "unmet" | "not_met" => Some(AcceptanceState::Fail),
        _ => None,
    }
}

fn evidence(v: &Value) -> Option<AcceptanceEvidence> {
    let mut e = AcceptanceEvidence::default();
    if let Some(t) = v.as_str() {
        e.text = Some(cut(t, MAX_EVIDENCE_TEXT)).filter(|t| !t.is_empty());
        return e.text.is_some().then_some(e);
    }
    e.text = v["text"]
        .as_str()
        .map(|t| cut(t, MAX_EVIDENCE_TEXT))
        .filter(|t| !t.is_empty());
    e.file_id = v["fileId"]
        .as_str()
        .filter(|s| id_ok(s))
        .map(str::to_string);
    e.url = v["url"]
        .as_str()
        .filter(|u| {
            u.len() <= MAX_URL
                && (u.starts_with("https://") || u.starts_with("http://"))
                && u.len() > "https://".len()
                && !u.chars().any(|c| c.is_whitespace() || c.is_control())
        })
        .map(str::to_string);
    e.artifact = match (
        v["artifact"]["artifactId"].as_str(),
        v["artifact"]["version"].as_u64(),
    ) {
        (Some(id), Some(ver)) if id_ok(id) && ver >= 1 && ver <= u32::MAX as u64 => {
            Some(ArtifactRef {
                artifact_id: id.to_string(),
                version: ver as u32,
            })
        }
        _ => None,
    };
    (e != AcceptanceEvidence::default()).then_some(e)
}

fn items(v: &Value) -> Option<Vec<AcceptanceItem>> {
    let list = v.as_array().or_else(|| v["items"].as_array())?;
    let out: Vec<AcceptanceItem> = list
        .iter()
        .filter_map(|i| {
            let text = cut(i["text"].as_str()?, MAX_TEXT);
            if text.is_empty() {
                return None;
            }
            Some(AcceptanceItem {
                text,
                state: state(i["state"].as_str().unwrap_or("pending"))?,
                evidence: evidence(&i["evidence"]),
            })
        })
        .take(MAX_ACCEPTANCE_ITEMS)
        .collect();
    Some(out)
}

/// The criteria of the last well-formed `acceptance` block in `text`.
pub fn parse(text: &str) -> Option<Vec<AcceptanceItem>> {
    let mut found = None;
    let mut lines = text.lines();
    while let Some(line) = lines.next() {
        let t = line.trim_start();
        let ticks = t.bytes().take_while(|b| *b == b'`').count();
        if ticks < 3 || t[ticks..].trim() != "acceptance" {
            continue;
        }
        let fence = &t[..ticks];
        let mut body = String::new();
        let mut closed = false;
        for l in lines.by_ref() {
            if l.trim() == fence {
                closed = true;
                break;
            }
            body.push_str(l);
            body.push('\n');
        }
        if !closed {
            break;
        }
        if let Some(list) = serde_json::from_str::<Value>(&body)
            .ok()
            .as_ref()
            .and_then(items)
        {
            found = Some(list);
        }
    }
    found
}

/// Remembers the last list sent, so an unchanged list sends nothing.
#[derive(Debug, Default)]
pub struct AcceptanceTracker {
    last: Option<Vec<AcceptanceItem>>,
}

impl AcceptanceTracker {
    /// The `acceptance` event for an assistant message that reports a changed list.
    pub fn on_event(&mut self, ev: &RunEvent) -> Option<RunEvent> {
        let RunEvent::Message { role, text, .. } = ev else {
            return None;
        };
        if role != "assistant" || !text.contains("acceptance") {
            return None;
        }
        let list = parse(text)?;
        if self.last.as_ref() == Some(&list) {
            return None;
        }
        self.last = Some(list.clone());
        Some(RunEvent::Acceptance { items: list })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_last_block_and_cuts_to_limits() {
        let long = "x".repeat(600);
        let text = format!(
            "Done.\n```acceptance\n[{{\"text\":\"old\",\"state\":\"pending\"}}]\n```\nThen:\n````acceptance\n{{\"items\":[{{\"text\":\"Tests pass\",\"state\":\"passed\",\"evidence\":{{\"text\":\"42 ok\",\"url\":\"https://ci.example/1\",\"fileId\":\"../bad\"}}}},{{\"text\":\"{long}\",\"state\":\"fail\"}},{{\"text\":\"bad state\",\"state\":\"maybe\"}},{{\"text\":\"  \"}}]}}\n````\n"
        );
        let items = parse(&text).unwrap();
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].text, "Tests pass");
        assert_eq!(items[0].state, AcceptanceState::Pass);
        let e = items[0].evidence.as_ref().unwrap();
        assert_eq!(e.text.as_deref(), Some("42 ok"));
        assert_eq!(e.url.as_deref(), Some("https://ci.example/1"));
        assert_eq!(e.file_id, None);
        assert_eq!(items[1].text.chars().count(), 500);
        assert_eq!(items[1].state, AcceptanceState::Fail);
    }

    #[test]
    fn ignores_other_blocks_and_unclosed_fences() {
        assert_eq!(
            parse("```json\n[{\"text\":\"a\",\"state\":\"pass\"}]\n```"),
            None
        );
        assert_eq!(
            parse("```acceptance\n[{\"text\":\"a\",\"state\":\"pass\"}]\n"),
            None
        );
        assert_eq!(parse("```acceptance\nnot json\n```"), None);
        let many: Vec<String> = (0..30)
            .map(|i| format!("{{\"text\":\"c{i}\",\"state\":\"pending\"}}"))
            .collect();
        let t = format!("```acceptance\n[{}]\n```", many.join(","));
        assert_eq!(parse(&t).unwrap().len(), MAX_ACCEPTANCE_ITEMS);
    }

    #[test]
    fn tracker_sends_only_changes() {
        let msg = |text: &str| RunEvent::Message {
            item_id: "m".into(),
            turn_id: None,
            role: "assistant".into(),
            text: text.into(),
        };
        let mut t = AcceptanceTracker::default();
        let a = "```acceptance\n[{\"text\":\"a\",\"state\":\"pending\"}]\n```";
        assert!(
            matches!(t.on_event(&msg(a)), Some(RunEvent::Acceptance { items }) if items.len() == 1)
        );
        assert_eq!(t.on_event(&msg(a)), None);
        let b = "```acceptance\n[{\"text\":\"a\",\"state\":\"pass\"}]\n```";
        assert!(t.on_event(&msg(b)).is_some());
        assert_eq!(t.on_event(&msg("no block")), None);
    }
}
