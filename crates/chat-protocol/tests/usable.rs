//! Usable protocol additions: defaults that readers apply and the open values older readers keep.

use chat_protocol::machine::RunEvent;
use chat_protocol::*;
use serde_json::json;

#[test]
fn parent_version_defaults_to_the_previous_version() {
    let mut v: ArtifactVersion = serde_json::from_value(json!({
        "artifactId": "A", "version": 3, "entry": "index.html",
        "files": [{ "path": "index.html", "fileId": "F", "size": 1, "mime": "text/html" }],
        "createdBy": "B", "createdAt": 1
    }))
    .unwrap();
    assert_eq!(v.parent(), Some(2));
    v.parent_version = Some(1);
    assert_eq!(v.parent(), Some(1));
    v.parent_version = None;
    v.version = 1;
    assert_eq!(v.parent(), None);
}

#[test]
fn team_owner_is_absent_for_admins() {
    let m: Member = serde_json::from_value(
        json!({ "id": "M", "kind": "human", "name": "A", "level": "owner" }),
    )
    .unwrap();
    assert!(!m.is_team_owner());
    assert_eq!(m.level, MemberLevel::Owner);
    let o: Member = serde_json::from_value(
        json!({ "id": "M", "kind": "human", "name": "A", "level": "owner", "teamOwner": true }),
    )
    .unwrap();
    assert!(o.is_team_owner());
}

#[test]
fn a_run_event_from_a_newer_daemon_is_skipped() {
    // A reader before `hand.*` skips them as `Unknown`; this one reads them and still skips
    // types it does not know.
    let e: RunEvent = serde_json::from_value(json!({ "type": "hand.something", "x": 1 })).unwrap();
    assert_eq!(e, RunEvent::Unknown);
    let e: RunEvent = serde_json::from_value(
        json!({ "type": "hand.result", "commandId": "c1", "ok": true, "ms": 5 }),
    )
    .unwrap();
    assert!(matches!(e, RunEvent::HandResult { ok: true, .. }));
}

#[test]
fn hand_command_ids() {
    assert!(is_hand_command_id("c1"));
    assert!(is_hand_command_id(&"a".repeat(64)));
    assert!(!is_hand_command_id(""));
    assert!(!is_hand_command_id(&"a".repeat(65)));
    assert!(!is_hand_command_id("c 1"));
}

#[test]
fn the_token_response_does_not_print_the_token() {
    let r = MachineGithubTokenResponse {
        token: "placeholder-not-a-real-token".into(),
        login: "alice-dev".into(),
    };
    let shown = format!("{r:?}");
    assert!(!shown.contains("placeholder"));
    assert!(shown.contains("alice-dev"));
}

#[test]
fn github_by_computer_fields_are_optional_and_open() {
    use chat_protocol::github::RunGithub;
    use chat_protocol::machine::{MachineMetrics, feature};
    // An old daemon's github event has no reason; a newer reason is kept as a string.
    let e: RunEvent =
        serde_json::from_value(json!({ "type": "github", "state": "missing" })).unwrap();
    assert!(matches!(e, RunEvent::Github { reason: None, .. }));
    let e: RunEvent = serde_json::from_value(
        json!({ "type": "github", "state": "missing", "reason": "a_future_reason" }),
    )
    .unwrap();
    assert!(matches!(e, RunEvent::Github { reason: Some(ref r), .. } if r == "a_future_reason"));
    // An assign from a server before askedBy, and one with it.
    let committer = json!({ "name": "dev", "email": "ai-B1@users.noreply.invalid" });
    let g: RunGithub = serde_json::from_value(
        json!({ "source": "machine", "memberId": "U1", "committer": committer }),
    )
    .unwrap();
    assert_eq!(g.asked_by, None);
    let g: RunGithub = serde_json::from_value(
        json!({ "source": "machine", "memberId": "U1", "committer": committer, "askedBy": "实习生" }),
    )
    .unwrap();
    assert_eq!(g.asked_by.as_deref(), Some("实习生"));
    // Metrics without github (older daemons), and an unknown state kept as is.
    let m: MachineMetrics = serde_json::from_value(json!({ "at": 1, "runs": [] })).unwrap();
    assert_eq!(m.github, None);
    let m: MachineMetrics =
        serde_json::from_value(json!({ "at": 1, "runs": [], "github": { "state": "later" } }))
            .unwrap();
    assert_eq!(m.github.map(|g| g.state).as_deref(), Some("later"));
    assert_eq!(feature::GITHUB_HOST, "github-host");
    assert!(GITHUB_UNAVAILABLE_REASONS.contains(&"off"));
}
