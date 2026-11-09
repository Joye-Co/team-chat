//! Fixture parity with packages/protocol: every valid fixture in protocol/fixtures parses into
//! the Rust type named by its `kind` and serializes back to `normalized` (or `value`) as an
//! equal JSON value; every invalid fixture fails. A `kind` with no Rust type fails the test.
//!
//! Normalization is the same as Zod: unknown fields are dropped on write. Fixtures that need it
//! carry a `normalized` value. For `ClientFrame` and HTTP request kinds, "parse" means
//! deserialize and then `validate()`, because Zod checks lengths and ranges while parsing.

use std::path::{Path, PathBuf};

use chat_protocol::machine::{Down, RunEvent, RunLogEntry, Up};
use chat_protocol::*;
use serde::{Serialize, de::DeserializeOwned};
use serde_json::Value;

type RoundTrip = fn(&Value) -> Result<Value, String>;

fn plain<T: DeserializeOwned + Serialize>(v: &Value) -> Result<Value, String> {
    let t: T = serde_json::from_value(v.clone()).map_err(|e| e.to_string())?;
    serde_json::to_value(&t).map_err(|e| e.to_string())
}

trait Check {
    fn check(&self) -> Validation;
}
macro_rules! check {
    ($($t:ty),*) => { $(impl Check for $t { fn check(&self) -> Validation { self.validate() } })* };
}
check!(
    ClientFrame,
    BootstrapRequest,
    RedeemRequest,
    CreateInviteRequest,
    CreateBotRequest,
    RevokeTokensRequest,
    CreateGroupRequest,
    AddGroupMemberRequest,
    CreateDmRequest,
    Card,
    CreateFileRequest,
    CreateArtifactRequest,
    MachineCreateArtifactRequest,
    AddArtifactVersionRequest,
    UpdateMemberRequest,
    UpdateGroupPrefsRequest,
    UpdateGroupRequest,
    UpdateInfo,
    CreateHookRequest,
    HookInRequest,
    PostMessageRequest,
    SetBotRunnerRequest,
    CreateMachineRequest,
    StartRunRequest,
    SetRunPlanRequest,
    EndRunRequest,
    CardV2,
    TopicLink,
    TopicContext,
    SetRunAcceptanceRequest,
    ResumeRunRequest,
    PartRef,
    BlockRefInput,
    BlockRef,
    PutArtifactDataRequest,
    NotifySettings,
    UpdateNotifySettingsRequest,
    ApprovalPolicy,
    UpdateApprovalPolicyRequest,
    AiServiceConfig,
    SetWorkerModelRequest,
    WorkerModel,
    UpdateAiServiceRequest,
    RecordDecisionRequest,
    PutArtifactMediaRequest,
    FileRef,
    PutDeskNoteRequest,
    EmailCodeRequest,
    EmailSignInRequest,
    VerifyEmailRequest,
    MemoryItem,
    CreateMemoryRequest,
    UpdateMemoryRequest,
    MemoryUpdatesResponse,
    MachineMemoryNoteRequest
);

fn validated<T: DeserializeOwned + Serialize + Check>(v: &Value) -> Result<Value, String> {
    let t: T = serde_json::from_value(v.clone()).map_err(|e| e.to_string())?;
    t.check().map_err(|e| e.to_string())?;
    serde_json::to_value(&t).map_err(|e| e.to_string())
}

/// Fixture kind -> Rust round trip. Add one line per new schema in packages/protocol.
const KINDS: &[(&str, RoundTrip)] = &[
    ("Member", plain::<Member>),
    ("Group", plain::<Group>),
    ("GroupSummary", plain::<GroupSummary>),
    ("Topic", plain::<Topic>),
    ("Message", plain::<Message>),
    ("ErrorBody", plain::<ErrorBody>),
    ("Event", plain::<Event>),
    ("TransientEvent", plain::<TransientEvent>),
    ("ClientFrame", validated::<ClientFrame>),
    ("ServerFrame", plain::<ServerFrame>),
    ("SendResult", plain::<SendResult>),
    ("ErrorResponse", plain::<ErrorResponse>),
    ("BootstrapRequest", validated::<BootstrapRequest>),
    ("BootstrapResponse", plain::<BootstrapResponse>),
    ("RedeemRequest", validated::<RedeemRequest>),
    ("RedeemResponse", plain::<RedeemResponse>),
    ("MeResponse", plain::<MeResponse>),
    ("MembersResponse", plain::<MembersResponse>),
    ("CreateInviteRequest", validated::<CreateInviteRequest>),
    ("CreateInviteResponse", plain::<CreateInviteResponse>),
    ("CreateBotRequest", validated::<CreateBotRequest>),
    ("CreateBotResponse", plain::<CreateBotResponse>),
    ("RevokeTokensRequest", validated::<RevokeTokensRequest>),
    ("RevokeTokensResponse", plain::<RevokeTokensResponse>),
    ("GroupsResponse", plain::<GroupsResponse>),
    ("CreateGroupRequest", validated::<CreateGroupRequest>),
    ("AddGroupMemberRequest", validated::<AddGroupMemberRequest>),
    ("CreateDmRequest", validated::<CreateDmRequest>),
    ("GroupSummaryResponse", plain::<GroupSummaryResponse>),
    ("OkResponse", plain::<OkResponse>),
    ("HealthResponse", plain::<HealthResponse>),
    // V1 extensions
    ("TopicState", plain::<TopicState>),
    ("Card", validated::<Card>),
    ("FileRef", validated::<FileRef>),
    ("FileMeta", plain::<FileMeta>),
    ("CreateFileRequest", validated::<CreateFileRequest>),
    ("CreateFileResponse", plain::<CreateFileResponse>),
    ("FileResponse", plain::<FileResponse>),
    ("UploadPartResponse", plain::<UploadPartResponse>),
    ("ArtifactEventsResponse", plain::<ArtifactEventsResponse>),
    ("PutDeskNoteRequest", validated::<PutDeskNoteRequest>),
    ("MemoryItem", validated::<MemoryItem>),
    ("CreateMemoryRequest", validated::<CreateMemoryRequest>),
    ("UpdateMemoryRequest", validated::<UpdateMemoryRequest>),
    ("MemoriesResponse", plain::<MemoriesResponse>),
    ("MemoryVersionsResponse", plain::<MemoryVersionsResponse>),
    ("MemoryUpdatesResponse", validated::<MemoryUpdatesResponse>),
    (
        "MachineMemoryNoteRequest",
        validated::<MachineMemoryNoteRequest>,
    ),
    ("PutDeskNoteResponse", plain::<PutDeskNoteResponse>),
    ("DeskNotesResponse", plain::<DeskNotesResponse>),
    ("Artifact", plain::<Artifact>),
    ("ArtifactVersion", plain::<ArtifactVersion>),
    ("CreateArtifactRequest", validated::<CreateArtifactRequest>),
    (
        "MachineCreateArtifactRequest",
        validated::<MachineCreateArtifactRequest>,
    ),
    (
        "AddArtifactVersionRequest",
        validated::<AddArtifactVersionRequest>,
    ),
    ("ArtifactVersionResponse", plain::<ArtifactVersionResponse>),
    ("ArtifactResponse", plain::<ArtifactResponse>),
    ("ArtifactsResponse", plain::<ArtifactsResponse>),
    ("UpdateMemberRequest", validated::<UpdateMemberRequest>),
    ("MemberResponse", plain::<MemberResponse>),
    (
        "UpdateGroupPrefsRequest",
        validated::<UpdateGroupPrefsRequest>,
    ),
    ("UpdateGroupRequest", validated::<UpdateGroupRequest>),
    ("UpdateInfo", validated::<UpdateInfo>),
    ("DiagnosticsResponse", plain::<DiagnosticsResponse>),
    ("Hook", plain::<Hook>),
    ("CreateHookRequest", validated::<CreateHookRequest>),
    ("CreateHookResponse", plain::<CreateHookResponse>),
    ("HooksResponse", plain::<HooksResponse>),
    ("HookInRequest", validated::<HookInRequest>),
    ("PostMessageRequest", validated::<PostMessageRequest>),
    ("AuditEntry", plain::<AuditEntry>),
    ("AuditResponse", plain::<AuditResponse>),
    // Bot runs and machines
    ("BotRun", plain::<BotRun>),
    ("RunEvent", plain::<RunEvent>),
    ("RunLogEntry", plain::<RunLogEntry>),
    ("RunEventsResponse", plain::<RunEventsResponse>),
    ("RunContext", plain::<RunContext>),
    ("TopicHistoryResponse", plain::<TopicHistoryResponse>),
    ("TopicMessageResponse", plain::<TopicMessageResponse>),
    ("ContextSearchResponse", plain::<ContextSearchResponse>),
    ("FrontDeskDecision", plain::<FrontDeskDecision>),
    ("BotMetrics", plain::<BotMetrics>),
    (
        "FrontDeskDecisionsResponse",
        plain::<FrontDeskDecisionsResponse>,
    ),
    ("SetBotRunnerRequest", validated::<SetBotRunnerRequest>),
    ("BotRunner", plain::<BotRunner>),
    ("RunPlan", plain::<Vec<RunPlanItem>>),
    ("StartRunRequest", validated::<StartRunRequest>),
    ("SetRunPlanRequest", validated::<SetRunPlanRequest>),
    ("EndRunRequest", validated::<EndRunRequest>),
    ("Machine", plain::<Machine>),
    ("CreateMachineRequest", validated::<CreateMachineRequest>),
    ("CreateMachineResponse", plain::<CreateMachineResponse>),
    ("MachinesResponse", plain::<MachinesResponse>),
    ("MachineMonitor", plain::<MachineMonitor>),
    ("MachineUpFrame", plain::<Up>),
    ("MachineDownFrame", plain::<Down>),
    // 1.1
    ("CardV2", validated::<CardV2>),
    ("TopicLink", validated::<TopicLink>),
    ("TopicContext", validated::<TopicContext>),
    ("GroupDecision", plain::<GroupDecision>),
    ("DecisionsResponse", plain::<DecisionsResponse>),
    (
        "SetRunAcceptanceRequest",
        validated::<SetRunAcceptanceRequest>,
    ),
    ("ResumeRunRequest", validated::<ResumeRunRequest>),
    // 1.1 interaction loop, refs, media
    ("CardState", plain::<CardState>),
    ("PartRef", validated::<PartRef>),
    ("BlockRefInput", validated::<BlockRefInput>),
    ("BlockRef", validated::<BlockRef>),
    ("MediaInfo", plain::<MediaInfo>),
    ("CardEventAck", plain::<CardEventAck>),
    ("CardPatchAck", plain::<CardPatchAck>),
    ("ArtifactEventAck", plain::<ArtifactEventAck>),
    ("ArtifactDataAck", plain::<ArtifactDataAck>),
    ("ArtifactDataResponse", plain::<ArtifactDataResponse>),
    (
        "PutArtifactDataRequest",
        validated::<PutArtifactDataRequest>,
    ),
    // Settings
    ("NotifySettings", validated::<NotifySettings>),
    (
        "UpdateNotifySettingsRequest",
        validated::<UpdateNotifySettingsRequest>,
    ),
    ("ApprovalPolicy", validated::<ApprovalPolicy>),
    (
        "UpdateApprovalPolicyRequest",
        validated::<UpdateApprovalPolicyRequest>,
    ),
    ("AiServiceConfig", validated::<AiServiceConfig>),
    ("WorkerModel", validated::<WorkerModel>),
    ("WorkerModelsResponse", plain::<WorkerModelsResponse>),
    ("SetWorkerModelRequest", validated::<SetWorkerModelRequest>),
    (
        "UpdateAiServiceRequest",
        validated::<UpdateAiServiceRequest>,
    ),
    ("TestAiServiceRequest", validated::<TestAiServiceRequest>),
    ("AiServiceTestResult", plain::<AiServiceTestResult>),
    ("Device", plain::<Device>),
    ("DevicesResponse", plain::<DevicesResponse>),
    ("EmailCodeRequest", validated::<EmailCodeRequest>),
    ("EmailCodeResponse", plain::<EmailCodeResponse>),
    ("EmailSignInRequest", validated::<EmailSignInRequest>),
    ("VerifyEmailRequest", validated::<VerifyEmailRequest>),
    ("MyEmailResponse", plain::<MyEmailResponse>),
    // 1.1 gap fixes
    ("RecordedDecision", plain::<RecordedDecision>),
    ("RecordDecisionRequest", validated::<RecordDecisionRequest>),
    ("RecordDecisionResponse", plain::<RecordDecisionResponse>),
    (
        "PutArtifactMediaRequest",
        validated::<PutArtifactMediaRequest>,
    ),
    (
        "PutArtifactMediaResponse",
        plain::<PutArtifactMediaResponse>,
    ),
];

fn fixtures_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../protocol/fixtures")
}

fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(dir).expect("read fixtures dir") {
        let path = entry.expect("dir entry").path();
        if path.is_dir() {
            walk(&path, out);
        } else if path.extension().is_some_and(|e| e == "json") {
            out.push(path);
        }
    }
}

fn load() -> Vec<(String, Value)> {
    let root = fixtures_root();
    let mut files = Vec::new();
    walk(&root, &mut files);
    files.sort();
    files
        .into_iter()
        .map(|p| {
            let name = p.strip_prefix(&root).unwrap().display().to_string();
            let text = std::fs::read_to_string(&p).expect("read fixture");
            (name, serde_json::from_str(&text).expect("fixture is JSON"))
        })
        .collect()
}

#[test]
fn fixture_parity() {
    let fixtures = load();
    assert!(
        fixtures.len() > 100,
        "expected the fixture set, found {}",
        fixtures.len()
    );
    let mut failures = Vec::new();
    for (name, fixture) in &fixtures {
        let kind = fixture["kind"].as_str().expect("fixture kind");
        let valid = fixture["valid"].as_bool().expect("fixture valid");
        let Some((_, round_trip)) = KINDS.iter().find(|(k, _)| *k == kind) else {
            failures.push(format!("{name}: no Rust type for kind {kind}"));
            continue;
        };
        let result = round_trip(&fixture["value"]);
        match (valid, result) {
            (true, Ok(out)) => {
                let expected = fixture.get("normalized").unwrap_or(&fixture["value"]);
                if &out != expected {
                    failures.push(format!(
                        "{name}: round trip differs\n  got:  {out}\n  want: {expected}"
                    ));
                }
            }
            (true, Err(e)) => failures.push(format!("{name}: valid fixture failed: {e}")),
            (false, Ok(out)) => failures.push(format!("{name}: invalid fixture parsed as {out}")),
            (false, Err(_)) => {}
        }
        // Invalid cards name the same `ack.error.reason` as the server.
        if let (false, Some(want)) = (valid, fixture.get("reason").and_then(Value::as_str)) {
            let got = match kind {
                "CardV2" => serde_json::from_value::<CardV2>(fixture["value"].clone())
                    .map(|c| c.problem().map(|p| p.reason.to_string()))
                    .unwrap_or_else(|e| Some(format!("serde: {e}"))),
                _ => Some(format!("no reason check for kind {kind}")),
            };
            if got.as_deref() != Some(want) {
                failures.push(format!("{name}: reason {got:?}, want {want}"));
            }
        }
    }
    assert!(
        failures.is_empty(),
        "{} fixture failures:\n{}",
        failures.len(),
        failures.join("\n")
    );
}

#[test]
fn every_kind_has_a_valid_fixture() {
    let fixtures = load();
    for (kind, _) in KINDS {
        assert!(
            fixtures
                .iter()
                .any(|(_, f)| f["kind"] == *kind && f["valid"] == true),
            "no valid fixture for {kind}"
        );
    }
}

#[test]
fn every_frame_and_event_type_has_a_valid_fixture() {
    let fixtures = load();
    let valid = |kind: &str, key: &str| -> std::collections::BTreeSet<String> {
        fixtures
            .iter()
            .filter(|(_, f)| f["kind"] == kind && f["valid"] == true)
            .filter_map(|(_, f)| f["value"][key].as_str().map(String::from))
            .collect()
    };
    let client: Vec<_> = valid("ClientFrame", "t").into_iter().collect();
    assert_eq!(
        client,
        [
            "artifact.data",
            "artifact.event",
            "artifact.reply",
            "card.action",
            "card.answer",
            "card.answer.retract",
            "card.event",
            "card.patch",
            "delete",
            "delta",
            "edit",
            "fetch",
            "hello",
            "history",
            "interaction.resolve",
            "ping",
            "react",
            "read",
            "search",
            "send",
            "topic.follow",
            "topic.mute",
            "topic.update",
            "typing"
        ]
    );
    let server: Vec<_> = valid("ServerFrame", "t").into_iter().collect();
    assert_eq!(
        server,
        [
            "ack",
            "directory",
            "error",
            "event",
            "history",
            "page",
            "pong",
            "reads",
            "searchResults",
            "transient",
            "welcome"
        ]
    );
    assert_eq!(valid("Event", "type").len(), 19);
    assert_eq!(valid("TransientEvent", "type").len(), 9);
}

#[test]
fn parse_and_encode_frames() {
    let frame = parse_server_frame(r#"{"t":"ack","rid":"r1","ok":true,"result":{"message":{"id":"m","clientMsgId":"c","groupId":"g","topicId":"t","authorId":"a","body":"hi","mentions":[],"createdSeq":1,"createdAt":0}}}"#).unwrap();
    let ServerFrame::Ack(ack) = frame else {
        panic!("not an ack")
    };
    assert_eq!(ack.send_result().unwrap().unwrap().message.body, "hi");

    let big = format!(r#"{{"t":"pong","x":"{}"}}"#, "a".repeat(MAX_FRAME_BYTES));
    assert_eq!(
        parse_server_frame(&big).unwrap_err().code,
        ErrorCode::TooLarge
    );
    assert_eq!(
        parse_server_frame("{").unwrap_err().code,
        ErrorCode::Invalid
    );

    let send = ClientFrame::Send(SendFrame {
        rid: "r1".into(),
        group_id: "g".into(),
        topic_id: None,
        title: Some("x".repeat(MAX_TITLE_CHARS + 1)),
        client_msg_id: "c".into(),
        body: "hi".into(),
        mentions: None,
        streaming: None,
        ..Default::default()
    });
    assert_eq!(send.validate().unwrap_err().field, "title");
    assert!(encode_client_frame(&send).is_err());

    let ping = encode_client_frame(&ClientFrame::Ping).unwrap();
    assert_eq!(ping, r#"{"t":"ping"}"#);
    assert_eq!(default_title("第一行 line\nsecond"), "第一行 line");
}

#[test]
fn unknown_card_block_is_read_but_rejected() {
    let card: Card = serde_json::from_str(
        r#"{"v":1,"title":"t","blocks":[{"type":"chart","data":[1]},{"type":"divider"}]}"#,
    )
    .unwrap();
    assert_eq!(card.blocks[0], CardBlock::Unknown);
    assert!(card.validate().is_err());
    let progress: CardBlock = serde_json::from_str(r#"{"type":"progress","value":0.25}"#).unwrap();
    assert_eq!(progress.progress(), Some(0.25));
}

#[test]
fn v1_helpers() {
    assert!(is_artifact_path("assets/图.png"));
    for p in ["", "/a", "a/", "a//b", "../a", "a/./b", "a\\b"] {
        assert!(!is_artifact_path(p), "{p}");
    }
    assert_eq!(parse_release_version("1.20.3"), Some((1, 20, 3)));
    assert_eq!(parse_release_version("01.2.3"), None);
    let summary: GroupSummary = serde_json::from_str(
        r#"{"group":{"id":"g","kind":"dm","name":"A","memberIds":[],"createdBy":"a","createdAt":0},"lastSeq":0,"readSeq":0,"unread":0,"mentions":0}"#,
    )
    .unwrap();
    assert_eq!(summary.notify_level(), NotifyLevel::All);
    assert!(!summary.is_muted());
    let post = PostMessageRequest {
        client_msg_id: "c".into(),
        body: "hi".into(),
        ..Default::default()
    };
    assert!(post.validate().is_ok());
}

#[test]
fn card_v2_readers_are_lenient() {
    // Values a later version may add read as Unknown, and validate() rejects them.
    let card: CardV2 = serde_json::from_str(
        r#"{"v":2,"rev":1,"intent":"vote","title":"t","fallback":"","blocks":[
            {"type":"map","id":"m"},
            {"type":"input","id":"i","kind":"color","label":"l"},
            {"type":"chart","id":"c","kind":"pie","series":[{"name":"a","values":[1]}]},
            {"type":"ref","id":"r","label":"x","target":{"kind":"issue","url":"https://e.com"}}],
          "actions":[{"id":"a","label":"A","kind":"later","style":"ghost"}]}"#,
    )
    .unwrap();
    assert_eq!(card.intent, CardIntent::Unknown);
    assert_eq!(card.blocks[0], CardV2Block::Unknown);
    assert!(matches!(
        card.blocks[1],
        CardV2Block::Input {
            kind: InputKind::Unknown,
            ..
        }
    ));
    assert!(matches!(
        card.blocks[2],
        CardV2Block::Chart {
            kind: ChartKind::Unknown,
            ..
        }
    ));
    assert!(matches!(
        card.blocks[3],
        CardV2Block::Ref {
            target: RefTarget::Unknown,
            ..
        }
    ));
    assert_eq!(card.actions()[0].kind(), CardActionKind::Unknown);
    assert!(card.validate().is_err());

    // A future block type keeps its type and fallbackText (CARD21 §6), also through a round trip.
    let future: CardV2 = serde_json::from_str(
        r#"{"v":2,"catalog":"tc.card/2.9","rev":1,"intent":"info","title":"t","fallback":"",
          "blocks":[{"type":"text","id":"a","text":"x"},{"type":"timeline","id":"tl","fallbackText":"三月上线，四月推广","events":[1]}]}"#,
    )
    .unwrap();
    assert_eq!(future.blocks[1], CardV2Block::Unknown);
    assert_eq!(future.unknown_fallback(1), Some("三月上线，四月推广"));
    assert_eq!(future.unknown_fallback(0), None);
    let back: CardV2 = serde_json::from_value(serde_json::to_value(&future).unwrap()).unwrap();
    assert_eq!(back.unknown_fallback(1), Some("三月上线，四月推广"));
    assert_eq!(back.unknown[&1].type_name, "timeline");

    // A cardV2 that does not parse at all is dropped; the message and its v1 card stay.
    let m: Message = serde_json::from_str(
        r#"{"id":"m","clientMsgId":"c","groupId":"g","topicId":"t","authorId":"a","body":"x","mentions":[],
            "createdSeq":1,"createdAt":0,"kind":"card","card":{"v":1,"title":"t","blocks":[]},
            "cardV2":{"v":3,"anything":true}}"#,
    )
    .unwrap();
    assert!(m.card.is_some());
    assert!(m.card_v2.is_none());

    let ask = CardAsk {
        from: vec!["requester".into(), "owners".into(), "m1".into()],
        ..Default::default()
    };
    assert!(ask.allows("m1", false, None));
    assert!(ask.allows("m2", true, None));
    assert!(ask.allows("m3", false, Some("m3")));
    assert!(!ask.allows("m4", false, Some("m3")));
}

#[test]
fn unknown_topic_link_kind_is_read_but_rejected() {
    let ev: Event = serde_json::from_str(
        r#"{"seq":1,"groupId":"g","at":0,"actorId":"a","type":"topic.linked","data":{"topicId":"t","link":{"kind":"issue","url":"https://e.com"}}}"#,
    )
    .unwrap();
    let EventData::TopicLinked(d) = ev.data else {
        panic!("not topic.linked")
    };
    assert_eq!(d.link, TopicLink::Unknown);
    assert!(d.validate().is_err());
}

#[test]
fn diagram_checks_match_the_shared_cases() {
    let path =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../protocol/checks/diagram-checks.json");
    let file: Value = serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    let cases = file["cases"].as_array().unwrap();
    assert!(cases.len() > 10);
    for c in cases {
        let source = c["source"].as_str().unwrap();
        let got = check_diagram_source(source);
        let want = if c["ok"] == true {
            DiagramCheck::Ok {
                kind: DIAGRAM_KEYWORDS
                    .iter()
                    .find(|(_, k)| *k == c["type"].as_str().unwrap())
                    .unwrap()
                    .1,
            }
        } else {
            DiagramCheck::Refused {
                reason: c["reason"].as_str().unwrap().to_string(),
            }
        };
        assert_eq!(got, want, "{}", source.chars().take(40).collect::<String>());
    }
}

#[test]
fn card_v2_write_rules_name_the_reason() {
    let card: CardV2 = serde_json::from_str(
        r#"{"v":2,"catalog":"tc.card/2.1","rev":1,"intent":"info","title":"t","fallback":"f",
            "blocks":[{"id":"a","type":"text","text":"x","color":"red"}]}"#,
    )
    .unwrap();
    assert_eq!(card.style_key.as_deref(), Some("color"));
    assert_eq!(card.problem().unwrap().reason, "card.style_property");
    // Readers keep the card (the style key is dropped on write back).
    let out = serde_json::to_value(&card).unwrap();
    assert!(out["blocks"][0].get("color").is_none());
    let two: CardV2 = serde_json::from_str(
        r#"{"v":2,"rev":1,"intent":"info","title":"t","fallback":"f",
            "blocks":[{"id":"s","type":"stack","children":["a"]},{"id":"a","type":"text","text":"x"}]}"#,
    )
    .unwrap();
    assert_eq!(two.problem().unwrap().reason, "card.unknown_block");
    let order: CardV2 = serde_json::from_str(
        r#"{"v":2,"catalog":"tc.card/2.1","rev":1,"intent":"info","title":"t","fallback":"f",
            "blocks":[{"id":"b","type":"text","text":"b"},{"id":"r","type":"row","children":["a","b"]},{"id":"a","type":"text","text":"a"}]}"#,
    )
    .unwrap();
    assert!(order.problem().is_none());
    let ids: Vec<_> = order
        .reading_order()
        .iter()
        .filter_map(|b| b.id())
        .collect();
    assert_eq!(ids, ["r", "a", "b"]);
}
