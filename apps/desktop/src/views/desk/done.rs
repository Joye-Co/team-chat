//! 做完了 rows (NATIVE.md §9.4): the result as the title and its proof as
//! one `secondary` line of words, not chips.

use crate::{
    copy::{desk as c, fill, members_page as m},
    data::{ArtifactKind, v2::Proof},
};

use super::model::Snapshot;

/// `0:42`.
pub fn duration(ms: u64) -> String {
    let s = ms / 1000;
    format!("{}:{:02}", s / 60, s % 60)
}

fn kind_name(kind: Option<ArtifactKind>) -> &'static str {
    match kind {
        Some(ArtifactKind::File) => c::DESK_KIND_FILE,
        Some(ArtifactKind::Image) => c::DESK_KIND_IMAGE,
        Some(ArtifactKind::Decision) => c::DESK_KIND_DECISION,
        _ => c::DESK_KIND_PAGE,
    }
}

/// The proof of a result in words: `PR #61 · 测试 148 项通过 · 录屏 0:42`.
pub fn proof_line(proof: &[Proof], snap: &Snapshot) -> Option<String> {
    let parts: Vec<String> = proof
        .iter()
        .map(|p| match p {
            Proof::Pr { number, .. } => fill(c::DESK_PROOF_PR, &[("n", &number.to_string())]),
            Proof::Tests { failed, .. } if *failed > 0 => {
                fill(c::DESK_TESTS_FAILED, &[("n", &failed.to_string())])
            }
            Proof::Tests { passed, .. } => {
                fill(c::DESK_TESTS_PASSED, &[("n", &passed.to_string())])
            }
            Proof::Recording { duration_ms } => {
                fill(c::DESK_RECORDING, &[("t", &duration(*duration_ms))])
            }
            Proof::Artifact {
                artifact_id,
                title,
                version,
            } => {
                fill(
                    m::MEMBERS_ARTIFACT,
                    &[("title", title), ("v", &version.to_string())],
                ) + " · "
                    + kind_name(snap.artifact_kinds.get(artifact_id).copied())
            }
            Proof::Checks { passed, total } if passed == total => {
                fill(c::DESK_CHECKS_ALL, &[("n", &total.to_string())])
            }
            Proof::Checks { passed, total } => fill(
                c::DESK_CHECKS_SOME,
                &[
                    ("passed", &passed.to_string()),
                    ("total", &total.to_string()),
                ],
            ),
        })
        .collect();
    (!parts.is_empty()).then(|| parts.join(" · "))
}
