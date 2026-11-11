//! Usable protocol additions: defaults that readers apply and the open values older readers keep.

use chat_protocol::machine::{Down, RunEvent};
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
fn a_pause_without_hold_reads_as_before() {
    let d: Down =
        serde_json::from_value(json!({ "t": "run.pause", "runId": "r1", "by": "u1" })).unwrap();
    assert!(matches!(d, Down::RunPause { hold: None, .. }));
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
