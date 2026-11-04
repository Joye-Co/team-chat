//! Proof chips of a result (做完了 on the 首页, the done row of a 话题) for
//! [`super::CoreProvider`], read from the done card the server posts when a
//! run ends (Card v2 intent `done`, docs/protocol.md "Runs and the output
//! contract") and from the 话题's PR links.

use chat_core::proto::{CardIntent, CardV2, CardV2Block, RefTarget, TopicLink};

use super::CoreProvider;
use crate::data::v2::Proof;

/// The first two numbers of a label: `测试：12 个通过，3 个没通过` gives 12 and 3.
fn counts(label: &str) -> (Option<u32>, Option<u32>) {
    let mut nums = label
        .split(|c: char| !c.is_ascii_digit())
        .filter(|s| !s.is_empty())
        .filter_map(|s| s.parse::<u32>().ok());
    (nums.next(), nums.next())
}

/// The proof of a done card: its PRs (with their size from the 话题's links),
/// its test counts and its 产出.
pub fn of_card(card: &CardV2, links: &[TopicLink]) -> Vec<Proof> {
    if card.intent != CardIntent::Done {
        return Vec::new();
    }
    let mut out = Vec::new();
    for block in card.all_blocks() {
        match block {
            CardV2Block::Ref {
                target: RefTarget::Pr {
                    number: Some(n), ..
                },
                ..
            } => {
                let size = links.iter().find_map(|l| match l {
                    TopicLink::Pr(p) if p.number == *n => Some((p.additions, p.deletions)),
                    _ => None,
                });
                let (additions, deletions) = size.unwrap_or_default();
                out.push(Proof::Pr {
                    number: (*n).min(u32::MAX as u64) as u32,
                    additions: additions.unwrap_or(0).min(u32::MAX as u64) as u32,
                    deletions: deletions.unwrap_or(0).min(u32::MAX as u64) as u32,
                });
            }
            CardV2Block::Ref {
                label,
                target:
                    RefTarget::Artifact {
                        artifact_id,
                        version,
                        ..
                    },
                ..
            } => out.push(Proof::Artifact {
                artifact_id: artifact_id.clone(),
                title: label.clone(),
                version: version.unwrap_or(1),
            }),
            // 1.1: the counts the server parsed from the run's output; older cards only
            // have the label.
            CardV2Block::Evidence {
                tests: Some(t), ..
            } => out.push(Proof::Tests {
                passed: t.passed.min(u32::MAX as u64) as u32,
                failed: t.failed.min(u32::MAX as u64) as u32,
            }),
            CardV2Block::Evidence {
                recording: Some(r),
                ..
            } => out.push(Proof::Recording {
                duration_ms: r.duration_ms,
            }),
            CardV2Block::Evidence { id, label, .. } if id == "tests" => {
                if let (Some(passed), failed) = counts(label) {
                    out.push(Proof::Tests {
                        passed,
                        failed: failed.unwrap_or(0),
                    });
                }
            }
            _ => {}
        }
    }
    out
}

impl CoreProvider {
    /// Proof of one result message, from its done card.
    pub(super) fn message_proof_inner(&self, message_id: &str) -> Vec<Proof> {
        let Some(a) = self.account() else {
            return Vec::new();
        };
        let Some(m) = a.client.store().message(message_id).ok().flatten() else {
            return Vec::new();
        };
        if m.is_deleted() {
            return Vec::new();
        }
        let Some(card) = m.card_v2 else {
            return Vec::new();
        };
        let links = a.client.topic_links(&m.topic_id).unwrap_or_default();
        of_card(&card, &links)
    }

    /// Proof of the newest result of a 话题: the done card of its newest run
    /// that posted one.
    pub(super) fn topic_result_proof_inner(&self, topic_id: &str) -> Vec<Proof> {
        let Some(a) = self.account() else {
            return Vec::new();
        };
        let mut runs = a.client.runs(topic_id).unwrap_or_default();
        runs.sort_by_key(|r| std::cmp::Reverse(r.updated_at));
        runs.iter()
            .find_map(|r| r.result_message_id.as_deref())
            .map(|m| self.message_proof_inner(m))
            .unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_done_card_gives_its_pr_tests_and_artifact() {
        let card: CardV2 = serde_json::from_value(serde_json::json!({
            "v": 2, "rev": 1, "intent": "done", "title": "修好了", "fallback": "修好了",
            "blocks": [
                { "type": "text", "id": "result", "text": "修好了" },
                { "type": "evidence", "id": "tests", "label": "测试：12 个通过，3 个没通过", "state": "fail" },
                { "type": "ref", "id": "pr-1", "label": "acme/app#42",
                  "target": { "kind": "pr", "url": "https://github.com/acme/app/pull/42", "repo": "acme/app", "number": 42 } },
                { "type": "ref", "id": "artifact-1", "label": "报告（第 2 版）",
                  "target": { "kind": "artifact", "artifactId": "a1", "version": 2 } }
            ]
        }))
        .expect("a done card");
        let proof = of_card(&card, &[]);
        assert_eq!(
            proof,
            vec![
                Proof::Tests {
                    passed: 12,
                    failed: 3
                },
                Proof::Pr {
                    number: 42,
                    additions: 0,
                    deletions: 0
                },
                Proof::Artifact {
                    artifact_id: "a1".into(),
                    title: "报告（第 2 版）".into(),
                    version: 2
                },
            ]
        );
    }

    #[test]
    fn counts_come_from_the_evidence_tests_and_a_recording_is_proof() {
        let card: CardV2 = serde_json::from_value(serde_json::json!({
            "v": 2, "rev": 1, "intent": "done", "title": "修好了", "fallback": "修好了",
            "blocks": [
                { "type": "evidence", "id": "tests", "label": "测试：全部通过（3 个跳过）",
                  "tests": { "passed": 12, "failed": 1, "skipped": 3 } },
                { "type": "evidence", "id": "rec", "label": "录屏",
                  "recording": { "fileId": "f1", "durationMs": 42000 } }
            ]
        }))
        .expect("a done card");
        assert_eq!(
            of_card(&card, &[]),
            vec![
                Proof::Tests {
                    passed: 12,
                    failed: 1
                },
                Proof::Recording { duration_ms: 42_000 },
            ]
        );
    }
}
