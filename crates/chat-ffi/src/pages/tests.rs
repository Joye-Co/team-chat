//! The pages bridge against a cache filled by hand; network calls go to a closed port.

use super::*;
use chat_core::Store;
use chat_core::proto::*;

const ME: &str = "01ME000000000000000000000A";
const ALICE: &str = "01ALICE0000000000000000000";
const BOT: &str = "01BOT00000000000000000000A";
const G1: &str = "01G1000000000000000000000A";
const DM: &str = "01DM000000000000000000000A";

fn member(id: &str, name: &str, kind: MemberKind) -> Member {
    Member {
        id: id.into(),
        kind,
        name: name.into(),
        level: MemberLevel::Member,
        ..Default::default()
    }
}

fn group(id: &str, kind: GroupKind, members: &[&str]) -> GroupSummary {
    GroupSummary {
        group: Group {
            purpose: Some("发布和验收".into()),
            id: id.into(),
            kind,
            name: if kind == GroupKind::Dm {
                String::new()
            } else {
                "研发".into()
            },
            member_ids: members.iter().map(|s| s.to_string()).collect(),
            created_by: ME.into(),
            created_at: 1_000,
        },
        ..Default::default()
    }
}

fn topic(id: &str, title: &str, seq: u64) -> Topic {
    Topic {
        id: id.into(),
        group_id: G1.into(),
        title: title.into(),
        status: TopicStatus::InProgress,
        created_by: ME.into(),
        created_at: 1_000 + seq,
        root_message_id: format!("M{id}"),
        last_seq: seq,
        message_count: 1,
        ..Default::default()
    }
}

fn message(id: &str, topic: &str, author: &str, seq: u64, body: &str) -> Message {
    Message {
        id: id.into(),
        client_msg_id: format!("c-{id}"),
        group_id: G1.into(),
        topic_id: topic.into(),
        author_id: author.into(),
        body: body.into(),
        created_seq: seq,
        created_at: 1_000 + seq,
        ..Default::default()
    }
}

fn event(seq: u64, actor: &str, data: EventData) -> Event {
    Event {
        seq,
        group_id: G1.into(),
        at: 1_000 + seq,
        actor_id: actor.into(),
        data,
    }
}

fn artifact(id: &str, topic: &str, title: &str, by: &str, updated: u64) -> Artifact {
    Artifact {
        id: id.into(),
        group_id: G1.into(),
        topic_id: topic.into(),
        title: title.into(),
        kind: ArtifactKind::Html,
        current_version: 2,
        created_by: by.into(),
        created_at: 1,
        updated_at: updated,
    }
}

fn version(id: &str, v: u32) -> ArtifactVersion {
    ArtifactVersion {
        preview: None,
        events: None,
        run_id: Some("R1".into()),
        media: None,
        data_rev: None,
        parent_version: None,
        artifact_id: id.into(),
        version: v,
        entry: "index.html".into(),
        files: vec![
            ArtifactFile {
                path: "index.html".into(),
                file_id: format!("F{v}"),
                size: 30,
                mime: "text/html".into(),
            },
            ArtifactFile {
                path: "a.png".into(),
                file_id: format!("P{v}"),
                size: 70,
                mime: "image/png".into(),
            },
        ],
        note: Some(format!("第 {v} 版")),
        created_by: BOT.into(),
        created_at: 10 * v as u64,
    }
}

fn run(id: &str, topic: &str, status: RunStatus, created: u64) -> BotRun {
    BotRun {
        queue_position: None,
        queued_at: None,
        usage: None,
        duration_ms: None,
        result_message_id: None,
        acceptance: None,
        paused_by: None,
        kind: None,
        id: id.into(),
        bot_id: BOT.into(),
        group_id: G1.into(),
        topic_id: topic.into(),
        machine_id: Some("MA".into()),
        status,
        requested_by: Some(ME.into()),
        created_at: created,
        started_at: Some(created),
        ended_at: None,
        updated_at: created + 1,
        summary: Some("在写测试".into()),
        plan: Some(vec![
            RunPlanItem::new("复现", RunPlanState::Done),
            RunPlanItem::new("修复", RunPlanState::Current),
            RunPlanItem::new("开 PR", RunPlanState::Pending),
        ]),
        retry_of: None,
        requested_for: None,
        chain_id: None,
        code: None,
        model: None,
        effort: None,
        failure: None,
        phase: None,
        waiting_for: None,
        hold_kind: None,
        held_at: None,
        takeover: None,
        last_handback: None,
        supervision: None,
    }
}

/// A client on a closed port, with 研发 (me, Alice, the AI member), a 私聊 with the AI member,
/// two 话题, 产出 and runs.
fn client(dir: &std::path::Path) -> std::sync::Arc<ChatClient> {
    let c = ChatClient::new(
        "http://127.0.0.1:9".into(),
        "t".into(),
        dir.to_string_lossy().into_owned(),
    )
    .unwrap();
    let s: &Store = c.inner.store();
    s.apply_welcome(
        &member(ME, "我", MemberKind::Human),
        &[
            member(ME, "我", MemberKind::Human),
            member(ALICE, "Alice", MemberKind::Human),
            member(BOT, "Forge", MemberKind::Bot),
        ],
        &[
            group(G1, GroupKind::Group, &[ME, ALICE, BOT]),
            group(DM, GroupKind::Dm, &[ME, BOT]),
        ],
    )
    .unwrap();
    s.apply_events(
        G1,
        &[
            event(
                1,
                ME,
                EventData::TopicCreated(TopicCreatedData {
                    topic: topic("T1", "修复重连", 1),
                }),
            ),
            event(
                2,
                BOT,
                EventData::MessageCreated(Box::new(MessageCreatedData {
                    message: message("MT1", "T1", BOT, 2, "复现了\n重复的问题"),
                })),
            ),
            event(
                3,
                ME,
                EventData::TopicCreated(TopicCreatedData {
                    topic: topic("T2", "发布说明", 3),
                }),
            ),
        ],
    )
    .unwrap();
    let now = now_ms();
    s.store_artifacts(&[
        artifact("A1", "T1", "重连报告", BOT, now),
        artifact("A2", "T2", "发布说明草稿", ALICE, 5),
    ])
    .unwrap();
    s.store_artifact_response(&ArtifactResponse {
        artifact: artifact("A1", "T1", "重连报告", BOT, now),
        versions: vec![version("A1", 1), version("A1", 2)],
    })
    .unwrap();
    s.store_run(&run("R1", "T1", RunStatus::Running, now))
        .unwrap();
    s.store_run(&run("R2", "T1", RunStatus::Done, now - 10))
        .unwrap();
    let mut aside = run("R3", "T1", RunStatus::Running, now);
    aside.kind = Some(RUN_KIND_ASIDE.into());
    s.store_run(&aside).unwrap();
    c
}

#[test]
fn artifacts_list_by_topic_group_and_all_with_names() {
    let dir = tempfile::tempdir().unwrap();
    let c = client(dir.path());
    let all = c.artifacts(None, None).unwrap();
    assert_eq!(
        all.iter().map(|a| a.id.as_str()).collect::<Vec<_>>(),
        ["A1", "A2"]
    );
    let a1 = &all[0];
    assert_eq!(a1.group_name, "研发");
    assert_eq!(a1.topic_title, "修复重连");
    assert_eq!(a1.created_by_name, "Forge");
    assert!(a1.created_by_is_bot && a1.is_page);
    assert_eq!(c.artifacts(Some(G1.into()), None).unwrap().len(), 2);
    let t2 = c.artifacts(Some(G1.into()), Some("T2".into())).unwrap();
    assert_eq!(t2.len(), 1);
    assert_eq!(t2[0].created_by_name, "Alice");
    // Fetching in the background does not fail offline.
    c.refresh_artifacts(None);
    c.refresh_artifacts(Some(G1.into()));
}

#[test]
fn artifact_detail_has_versions_newest_first() {
    let dir = tempfile::tempdir().unwrap();
    let c = client(dir.path());
    let d = c.artifact("A1".into()).unwrap().unwrap();
    assert_eq!(d.artifact.title, "重连报告");
    assert_eq!(
        d.versions.iter().map(|v| v.version).collect::<Vec<_>>(),
        [2, 1]
    );
    let v = &d.versions[0];
    assert_eq!(v.entry, "index.html");
    assert_eq!(v.entry_mime, "text/html");
    assert_eq!(v.note, "第 2 版");
    assert_eq!(v.created_by_name, "Forge");
    assert_eq!((v.file_count, v.size), (2, 100));
    assert_eq!(v.run_id.as_deref(), Some("R1"));
    assert!(c.artifact("nope".into()).unwrap().is_none());
}

#[test]
fn network_calls_fail_as_network_offline() {
    let dir = tempfile::tempdir().unwrap();
    let c = client(dir.path());
    assert!(c.load_artifact("A1".into()).is_err());
    assert!(c.open_artifact_version("A1".into(), Some(2)).is_err());
    assert!(c.run_terminal("R1".into(), 0).is_err());
    assert!(c.cancel_run("R1".into()).is_err());
    assert!(c.open_dm(ALICE.into()).is_err());
    assert!(matches!(
        c.search_messages("重连".into(), None),
        Err(CoreError::Network)
    ));
    assert!(c.search_messages("  ".into(), None).unwrap().is_empty());
}

#[test]
fn member_detail_counts_this_week_and_lists_recent_work() {
    let dir = tempfile::tempdir().unwrap();
    let c = client(dir.path());
    let bot = c.member_detail(BOT.into()).unwrap().unwrap();
    assert!(bot.member.is_bot);
    // R3 is a 单独问 run: not work.
    assert_eq!((bot.week_runs, bot.week_done, bot.active_runs), (2, 1, 1));
    assert_eq!(bot.week_artifacts, 1);
    assert_eq!(
        bot.recent.iter().map(|r| r.id.as_str()).collect::<Vec<_>>(),
        ["R1", "R2"]
    );
    assert_eq!(bot.dm_group_id.as_deref(), Some(DM));
    assert_eq!(bot.shared_groups, ["研发"]);

    // A person: the work they handed out.
    let me = c.member_detail(ME.into()).unwrap().unwrap();
    assert!(me.member.is_me);
    assert_eq!(me.week_runs, 2);
    assert_eq!(me.dm_group_id, None);
    let alice = c.member_detail(ALICE.into()).unwrap().unwrap();
    assert_eq!((alice.week_runs, alice.week_artifacts), (0, 0));
    assert_eq!(alice.dm_group_id, None);
    assert!(c.member_detail("nobody".into()).unwrap().is_none());
    // The 私聊 in the cache needs no network.
    assert_eq!(c.open_dm(BOT.into()).unwrap(), DM);
}

#[test]
fn group_info_has_purpose_members_and_counts() {
    let dir = tempfile::tempdir().unwrap();
    let c = client(dir.path());
    let g = c.group_info(G1.into()).unwrap().unwrap();
    assert_eq!(g.name, "研发");
    assert_eq!(g.purpose, "发布和验收");
    assert!(!g.is_dm);
    assert_eq!(g.main_topic_id, main_topic_id(G1));
    assert_eq!(c.main_topic_of(G1.into()), g.main_topic_id);
    assert_eq!(
        g.members
            .iter()
            .map(|m| m.name.as_str())
            .collect::<Vec<_>>(),
        ["Alice", "我", "Forge"]
    );
    assert_eq!((g.topic_count, g.artifact_count), (2, 2));
    assert_eq!(
        g.active_runs
            .iter()
            .map(|r| r.id.as_str())
            .collect::<Vec<_>>(),
        ["R1"]
    );
    let dm = c.group_info(DM.into()).unwrap().unwrap();
    assert!(dm.is_dm);
    assert_eq!(dm.name, "Forge");
    assert!(c.group_info("nope".into()).unwrap().is_none());
}

#[test]
fn runs_have_plan_progress_and_skip_asides() {
    let dir = tempfile::tempdir().unwrap();
    let c = client(dir.path());
    let runs = c.runs("T1".into()).unwrap();
    assert_eq!(
        runs.iter().map(|r| r.id.as_str()).collect::<Vec<_>>(),
        ["R1", "R2"]
    );
    let r = &runs[0];
    assert_eq!(r.status, "running");
    assert!(r.active && !r.queued);
    assert_eq!(r.member_name, "Forge");
    assert_eq!(r.topic_title, "修复重连");
    assert_eq!(r.summary, "在写测试");
    assert_eq!(r.steps_done, 1);
    assert_eq!(r.current_step.as_deref(), Some("修复"));
    assert_eq!(r.plan[1].state, "current");
    assert_eq!(c.run_progress("R1".into()), Some(1.0 / 3.0));
    assert_eq!(c.run("R2".into()).unwrap().unwrap().status, "done");
    assert!(c.run("nope".into()).unwrap().is_none());
    assert_eq!(
        c.active_runs()
            .unwrap()
            .iter()
            .map(|r| r.id.as_str())
            .collect::<Vec<_>>(),
        ["R1"]
    );
}

#[test]
fn search_finds_topics_and_artifacts_in_the_cache() {
    let dir = tempfile::tempdir().unwrap();
    let c = client(dir.path());
    let topics = c.search_topics("重连".into()).unwrap();
    assert_eq!(
        topics.iter().map(|t| t.id.as_str()).collect::<Vec<_>>(),
        ["T1"]
    );
    let arts = c.search_artifacts("发布".into()).unwrap();
    assert_eq!(
        arts.iter().map(|a| a.id.as_str()).collect::<Vec<_>>(),
        ["A2"]
    );
    assert!(c.search_topics(" ".into()).unwrap().is_empty());
    assert!(c.search_artifacts("".into()).unwrap().is_empty());
}

#[test]
fn message_context_and_asides() {
    let dir = tempfile::tempdir().unwrap();
    let c = client(dir.path());
    let m = c.message_context("MT1".into()).unwrap().unwrap();
    assert_eq!((m.group_id.as_str(), m.topic_id.as_str()), (G1, "T1"));
    assert_eq!(m.seq, 2);
    assert!(m.author_is_bot);
    assert_eq!(m.excerpt, "复现了 重复的问题");
    assert!(c.message_context("nope".into()).unwrap().is_none());
    assert!(c.asides(G1.into(), "T1".into()).unwrap().is_empty());
    // Only an AI member answers a 单独问.
    assert!(matches!(
        c.ask_aside(ALICE.into(), G1.into(), "T1".into(), 2, "为什么？".into()),
        Err(CoreError::Invalid)
    ));
}

#[test]
fn ask_aside_queues_the_question_in_the_cached_dm() {
    let dir = tempfile::tempdir().unwrap();
    let c = client(dir.path());
    let dm = c
        .ask_aside(
            BOT.into(),
            G1.into(),
            "T1".into(),
            2,
            "为什么会重复？".into(),
        )
        .unwrap();
    assert_eq!(dm, DM);
}

#[test]
fn shorten_keeps_the_tail() {
    assert_eq!(shorten("abc", 5), "abc");
    assert_eq!(shorten("abcdef", 3), "…def");
}
