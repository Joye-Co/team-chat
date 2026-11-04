//! The page aggregates of the real provider on the seeded cache: 成员 (这周 cost, 电脑,
//! 最近的问题, 能用的工具), 做完了, 需要你拍板, and the frame time with 500 话题.

use std::{path::PathBuf, sync::Arc, time::Instant};

use chat_core::proto::{
    self, Event, EventData, MessageCreatedData, RunUpdatedData, TopicCreatedData,
};
use serde_json::json;

use super::*;
use crate::data::{Change, ChatProvider as _, v2::Currency};

const MIN: u64 = 60_000;

struct Seeded {
    p: Arc<CoreProvider>,
    root: PathBuf,
    now: u64,
    seq: u64,
}

impl Drop for Seeded {
    fn drop(&mut self) {
        self.p.sign_out_local_for_selftest();
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

impl Seeded {
    fn new(name: &str) -> Self {
        let root = std::env::temp_dir().join(format!("seeded-{name}-{}", std::process::id()));
        let now = chrono::Utc::now().timestamp_millis() as u64;
        let p = seeded(&root, now);
        Seeded {
            p,
            root,
            now,
            seq: 14,
        }
    }

    /// Applies events to the cache and tells the provider, as the pump does.
    fn apply(&mut self, data: Vec<(String, u64, EventData)>) {
        let events: Vec<Event> = data
            .into_iter()
            .map(|(actor, at, mut data)| {
                self.seq += 1;
                if let EventData::MessageCreated(m) = &mut data {
                    m.message.created_seq = self.seq;
                }
                Event {
                    seq: self.seq,
                    group_id: GROUP.into(),
                    at,
                    actor_id: actor,
                    data,
                }
            })
            .collect();
        let a = self.p.account().expect("signed in");
        a.client
            .store()
            .apply_events(GROUP, &events)
            .expect("events");
        self.p.emit(vec![Change::Members]);
    }
}

fn run_event(run: serde_json::Value) -> (String, u64, EventData) {
    let run: proto::BotRun = from(run);
    (
        run.bot_id.clone(),
        run.updated_at,
        EventData::RunUpdated(RunUpdatedData { run }),
    )
}

fn message_event(m: proto::Message) -> (String, u64, EventData) {
    (
        m.author_id.clone(),
        m.created_at,
        EventData::MessageCreated(Box::new(MessageCreatedData { message: m })),
    )
}

fn topic_event(t: serde_json::Value) -> (String, u64, EventData) {
    let topic: proto::Topic = from(t);
    (
        topic.created_by.clone(),
        topic.created_at,
        EventData::TopicCreated(TopicCreatedData { topic }),
    )
}

#[test]
fn this_week_cost_adds_up_the_runs_in_their_currency_and_is_absent_without_one() {
    let mut s = Seeded::new("cost");
    let now = s.now;
    s.apply(vec![run_event(json!({
        "id": "01SEEDRUN2", "botId": FORGE, "groupId": GROUP, "topicId": TOPIC_WEEKLY,
        "status": "done", "createdAt": now - 20 * MIN, "startedAt": now - 20 * MIN,
        "endedAt": now - 10 * MIN, "updatedAt": now - 10 * MIN,
        "usage": { "inputTokens": 1, "outputTokens": 1, "costUsd": 1.24 }
    }))]);
    let forge = s.p.member_stats(FORGE).expect("stats");
    assert_eq!(forge.cost_cents_this_week, Some(86 + 124));
    assert_eq!(forge.cost_currency, Currency::Usd);
    assert_eq!(
        crate::views::topic_rail::money_text(210, forge.cost_currency),
        "$2.10"
    );
    assert_eq!(forge.done_this_week, 2);
    assert_eq!(forge.topics_this_week, 2);
    // A bot without a reported cost has no cost tile.
    let scout = s.p.member_stats(SCOUT).expect("stats");
    assert_eq!(scout.cost_cents_this_week, None);
}

#[test]
fn the_machine_card_problems_and_tools_come_from_runs_machines_and_the_runner() {
    let mut s = Seeded::new("machine");
    let now = s.now;
    let machine: proto::Machine = from(json!({
        "id": "01SEEDMACHINE", "name": "mac-mini-2", "labels": [], "createdBy": ME,
        "createdAt": now - 86_400_000, "connected": true, "healthy": true,
        "health": { "diskFreeBytes": 40, "diskTotalBytes": 100, "load1": 0.1, "load5": 0.1,
                    "load15": 0.1, "cpuCount": 8, "activeRuns": 1 },
        "runningRuns": ["01SEEDLIVE"]
    }));
    let runner: proto::BotRunner = from(json!({
        "botId": FORGE, "runtime": "claude-code", "labels": [],
        "repo": { "url": "https://github.com/acme/chat.git" }, "baseBranch": "main",
        "model": "opus", "envAllowlist": [], "enabled": true, "updatedAt": now
    }));
    s.p.seed_remote_for_test(vec![machine], vec![runner]);
    s.apply(vec![
        run_event(json!({
            "id": "01SEEDFAIL", "botId": FORGE, "groupId": GROUP, "topicId": TOPIC_WEEKLY,
            "status": "failed", "createdAt": now - 30 * MIN, "updatedAt": now - 25 * MIN,
            "summary": "CI 超时，检查没跑完", "machineId": "01SEEDMACHINE"
        })),
        run_event(json!({
            "id": "01SEEDLIVE", "botId": FORGE, "groupId": GROUP, "topicId": TOPIC,
            "status": "running", "createdAt": now - 5 * MIN, "updatedAt": now - MIN,
            "machineId": "01SEEDMACHINE"
        })),
    ]);
    let forge = s.p.member_stats(FORGE).expect("stats");
    let mc = forge.machine.expect("the 电脑 card");
    assert_eq!(mc.name, "mac-mini-2");
    assert!(mc.online && mc.busy);
    assert_eq!(mc.disk_used_pct, Some(60));
    assert_eq!(forge.active, [(GROUP.to_string(), TOPIC.to_string())]);
    assert_eq!(forge.problems.len(), 1);
    assert_eq!(forge.problems[0].text, "CI 超时，检查没跑完");
    assert_eq!(forge.problems[0].run_id.as_deref(), Some("01SEEDFAIL"));
    assert_eq!(forge.tools, ["claude-code"]);
    let extras = s.p.member_extras(FORGE);
    let tools: Vec<(String, Option<String>, Option<String>)> = extras
        .tools
        .iter()
        .map(|t| (t.name.clone(), t.access.clone(), t.detail.clone()))
        .collect();
    assert_eq!(
        tools,
        [
            ("claude-code".into(), Some("opus".into()), None),
            ("acme/chat".into(), None, Some("main".into())),
        ]
    );
    assert_eq!(extras.budget_cents, None);
    assert_eq!(extras.prs_this_week, 1);
    assert_eq!(extras.pages_this_week, 1);

    // A later good run in the same 话题 clears the problem; the machine is idle then.
    s.apply(vec![
        run_event(json!({
            "id": "01SEEDFIX", "botId": FORGE, "groupId": GROUP, "topicId": TOPIC_WEEKLY,
            "status": "done", "createdAt": now - 20 * MIN, "updatedAt": now - 15 * MIN,
            "machineId": "01SEEDMACHINE"
        })),
        run_event(json!({
            "id": "01SEEDLIVE", "botId": FORGE, "groupId": GROUP, "topicId": TOPIC,
            "status": "done", "createdAt": now - 5 * MIN, "updatedAt": now,
            "machineId": "01SEEDMACHINE"
        })),
    ]);
    let forge = s.p.member_stats(FORGE).expect("stats");
    assert!(forge.problems.is_empty());
    assert!(!forge.machine.expect("the 电脑 card").busy);
    // Without a run on a machine there is no 电脑 card.
    assert_eq!(s.p.member_stats(SCOUT).and_then(|x| x.machine), None);
}

#[test]
fn done_today_lists_results_not_topics_with_a_new_message() {
    let mut s = Seeded::new("done");
    let now = s.now;
    let done_card = json!({
        "v": 2, "rev": 1, "intent": "done", "title": "周报写好了", "fallback": "周报写好了",
        "blocks": [{ "type": "text", "id": "result", "text": "周报写好了" }]
    });
    s.apply(vec![
        topic_event(json!({
            "id": "01SEEDOLD", "groupId": GROUP, "title": "上个月的旧话题", "status": "done",
            "createdBy": ME, "createdAt": now - 40 * 86_400_000, "rootMessageId": "01SEEDO1",
            "lastSeq": 1, "messageCount": 2, "lastMessageAt": now - 1000
        })),
        message_event(message(
            "01SEEDO2",
            "01SEEDOLD",
            ME,
            0,
            now - 1000,
            "补一句",
        )),
        message_event(card(
            "01SEEDWDONE",
            TOPIC_WEEKLY,
            0,
            now - 2000,
            "周报写好了",
            done_card,
        )),
        run_event(json!({
            "id": "01SEEDWRUN", "botId": FORGE, "groupId": GROUP, "topicId": TOPIC_WEEKLY,
            "status": "done", "createdAt": now - 3000, "startedAt": now - 3000,
            "endedAt": now - 2000, "updatedAt": now - 2000, "resultMessageId": "01SEEDWDONE"
        })),
    ]);
    let done = s.p.done_today();
    let weekly = done
        .iter()
        .find(|d| d.topic_id == TOPIC_WEEKLY)
        .expect("the result of an open 话题");
    assert_eq!(weekly.title, "周报写好了");
    assert_eq!(weekly.member_id, FORGE);
    assert!(
        done.iter().all(|d| d.topic_id != "01SEEDOLD"),
        "an old done 话题 with a new message is no result"
    );
    let recent = s.p.group_recent_done(GROUP);
    let r = recent
        .iter()
        .find(|d| d.topic_id == TOPIC_WEEKLY)
        .expect("最近完成");
    assert_eq!(
        (r.title.as_str(), r.note.as_deref()),
        ("本周周报", Some("周报写好了"))
    );
    assert!(recent.iter().all(|d| d.topic_id != "01SEEDOLD"));
}

#[test]
fn an_ask_for_me_deep_in_a_topic_that_does_not_wait_on_me_is_on_the_desk() {
    let mut s = Seeded::new("asks");
    let now = s.now;
    let ask = json!({
        "v": 2, "rev": 1, "intent": "ask", "title": "周报发给谁？", "fallback": "周报发给谁？",
        "ask": { "from": [ME] },
        "blocks": [{ "type": "decision", "id": "to", "options": [
            { "id": "all", "label": "全员" }, { "id": "leads", "label": "负责人" }
        ] }],
        "actions": [{ "id": "ok", "label": "确定", "style": "primary" }]
    });
    let mut events = vec![message_event(card(
        "01SEEDDEEPASK",
        TOPIC_WEEKLY,
        0,
        now - 40 * MIN,
        "周报发给谁？",
        ask,
    ))];
    for i in 0..35u64 {
        events.push(message_event(message(
            &format!("01SEEDCHAT{i:02}"),
            TOPIC_WEEKLY,
            FORGE,
            0,
            now - 39 * MIN + i * 1000,
            "进度更新",
        )));
    }
    s.apply(events);
    let asks = s.p.asks_for_me();
    let deep = asks
        .iter()
        .find(|a| a.message_id.as_deref() == Some("01SEEDDEEPASK"))
        .expect("the older ask");
    assert_eq!(
        (deep.topic_id.as_str(), deep.from.as_str()),
        (TOPIC_WEEKLY, FORGE)
    );
    // The review ask of the seeded 话题 is still there.
    assert!(
        asks.iter()
            .any(|a| a.message_id.as_deref() == Some(REVIEW_ASK))
    );
    // The 群 page glance of that 话题 shows the two v2 options.
    let glance = s.p.topic_glance(GROUP, TOPIC_WEEKLY);
    match glance.preview {
        Some(crate::data::pages::TopicPreview::Choice { options }) => {
            let labels: Vec<&str> = options.iter().map(|o| o.label.as_str()).collect();
            assert_eq!(labels, ["全员", "负责人"]);
        }
        other => panic!("no choice glance: {other:?}"),
    }
}

/// The 成员, 首页, 群 and 产出 pages read their aggregates on every frame. With 500
/// 话题 a frame of those reads stays within budget; the report goes to
/// `.tmp/selftest-page-aggregates.json`.
#[test]
fn page_aggregates_with_500_topics_stay_within_the_frame_budget() {
    const TOPICS: u64 = 500;
    const FRAMES: u32 = 20;
    const BUDGET_MS: f64 = 4.0;
    let mut s = Seeded::new("perf");
    let now = s.now;
    let mut events = Vec::new();
    for i in 0..TOPICS {
        let id = format!("01SEEDPERF{i:04}");
        let root = format!("01SEEDPM{i:04}");
        let at = now - 6 * 86_400_000 + i * MIN;
        events.push(topic_event(json!({
            "id": id, "groupId": GROUP, "title": format!("话题 {i}"), "status": "open",
            "createdBy": ME, "createdAt": at, "rootMessageId": root, "lastSeq": 1,
            "messageCount": 1, "lastMessageAt": at
        })));
        events.push(message_event(message(&root, &id, ME, 0, at, "开始")));
        events.push(run_event(json!({
            "id": format!("01SEEDPR{i:04}"), "botId": FORGE, "groupId": GROUP, "topicId": id,
            "status": if i % 7 == 0 { "failed" } else { "done" }, "createdAt": at,
            "startedAt": at, "endedAt": at + MIN, "updatedAt": at + MIN,
            "usage": { "inputTokens": 1, "outputTokens": 1, "costUsd": 0.01 }
        })));
    }
    s.apply(events);
    let p = s.p.clone();
    let frame = || {
        let members = p.members();
        let _ = p.active_runs();
        let _ = p.asks_for_me();
        for m in &members {
            let _ = p.member_stats(&m.id);
            let _ = p.member_extras(&m.id);
        }
        let _ = p.done_today();
        let _ = p.all_artifacts();
        let _ = p.group_recent_done(GROUP);
        let _ = p.group_artifacts(GROUP);
    };
    let first = Instant::now();
    frame();
    let first_ms = first.elapsed().as_secs_f64() * 1000.;
    let start = Instant::now();
    for _ in 0..FRAMES {
        frame();
    }
    let frame_ms = start.elapsed().as_secs_f64() * 1000. / FRAMES as f64;
    let report = json!({
        "test": "page_aggregates_with_500_topics_stay_within_the_frame_budget",
        "topics": TOPICS, "frames": FRAMES, "firstFrameMs": first_ms,
        "frameMs": frame_ms, "budgetMs": BUDGET_MS, "pass": frame_ms < BUDGET_MS
    });
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../.tmp");
    let _ = std::fs::create_dir_all(&dir);
    let _ = std::fs::write(
        dir.join("selftest-page-aggregates.json"),
        serde_json::to_vec_pretty(&report).unwrap_or_default(),
    );
    assert!(
        frame_ms < BUDGET_MS,
        "a frame of page aggregates took {frame_ms:.2} ms (budget {BUDGET_MS} ms)"
    );
    // Every run of the week counts, also in 话题 beyond the newest 500 of the 群.
    let forge = p.member_stats(FORGE).expect("stats");
    assert_eq!(forge.topics_this_week as u64, TOPICS + 1);
}

#[test]
fn done_card_checks_acceptance_proof_and_video_length_come_from_the_protocol() {
    use crate::data::v2::Proof;
    let mut s = Seeded::new("proof");
    let now = s.now;
    // The PR of the done card reports three passing checks.
    let proof = s.p.message_proof(GROUP, "01SEEDDONE");
    assert!(
        proof.contains(&Proof::Checks {
            passed: 3,
            total: 3
        }),
        "{proof:?}"
    );

    // A recording in the 话题, with the length its sender reported.
    let video = proto::Message {
        attachments: Some(vec![from(json!({
            "fileId": "01SEEDVIDEO", "name": "录屏.mp4", "size": 1000, "mime": "video/mp4",
            "sha256": "a".repeat(64),
            "media": { "kind": "video", "mime": "video/mp4", "bytes": 1000, "duration": 42.5, "preview": "ready" }
        }))]),
        ..message("01SEEDVMSG", TOPIC, FORGE, 0, now - 3 * MIN, "录屏")
    };
    s.apply(vec![
        message_event(video),
        run_event(json!({
            "id": "01SEEDACC", "botId": FORGE, "groupId": GROUP, "topicId": TOPIC,
            "status": "running", "createdAt": now - 2 * MIN, "updatedAt": now - MIN,
            "acceptance": [
                { "text": "重连后没有重复", "state": "pass",
                  "evidence": { "text": "0 条重复", "fileId": "01SEEDVIDEO" } },
                { "text": "报告写好了", "state": "pass",
                  "evidence": { "artifact": { "artifactId": ARTIFACT, "version": 2 } } },
                { "text": "CI 通过", "state": "pending",
                  "evidence": { "url": "https://github.com/acme/chat/pull/61/checks" } }
            ]
        })),
    ]);
    let items = s.p.acceptance_criteria("01SEEDACC");
    assert_eq!(items.len(), 3);
    assert_eq!(items[0].evidence.as_deref(), Some("0 条重复"));
    let file = items[0].evidence_file.clone().expect("the file chip");
    assert_eq!(file.name, "录屏.mp4");
    assert_eq!(
        items[1].evidence_artifact,
        Some((ARTIFACT.to_string(), 2, "重连测试报告".to_string()))
    );
    assert_eq!(
        items[2].evidence_url.as_deref(),
        Some("https://github.com/acme/chat/pull/61/checks")
    );

    // The timeline converts the message; the length is the sender's, before any download.
    let _ = s.p.messages(GROUP, TOPIC, 50);
    assert_eq!(s.p.media_duration(&file), Some(42_500));
}

#[test]
fn group_decisions_of_one_account_are_not_shown_to_the_next() {
    use crate::data::v2::Decision;
    let s = Seeded::new("accounts");
    s.p.seed_group_decisions_for_test(
        GROUP,
        vec![Decision {
            id: "01SEEDSERVER-1".into(),
            group_id: GROUP.into(),
            topic_id: Some(TOPIC.into()),
            text: "只给 Joye 看的决定".into(),
            by: ME.into(),
            at: s.now as i64,
        }],
    );
    assert!(
        s.p.group_decisions(GROUP)
            .iter()
            .any(|d| d.id == "01SEEDSERVER-1")
    );

    // Someone else signs in on this computer, in a 群 with the same id.
    s.p.sign_out_local_for_selftest();
    let other: proto::Member = from(json!({
        "id": "01SEEDOTHER000000000000000", "kind": "human", "name": "Lin", "level": "member"
    }));
    let group: proto::GroupSummary = from(json!({
        "group": { "id": GROUP, "kind": "group", "name": "开发",
                   "memberIds": ["01SEEDOTHER000000000000000"],
                   "createdBy": ME, "createdAt": s.now - 86_400_000 },
        "lastSeq": 0, "readSeq": 0, "unread": 0, "mentions": 0
    }));
    let dir = session::account_dir(&s.root, SERVER, &other.id);
    let client = open_client(SERVER, String::new(), &dir).expect("a cache");
    client
        .store()
        .apply_welcome(&other, std::slice::from_ref(&other), &[group])
        .expect("welcome");
    s.p.install(
        SavedSession {
            server_url: SERVER.into(),
            member: other,
        },
        client,
        dir,
    );
    assert_eq!(s.p.group_decisions(GROUP), Vec::<Decision>::new());
}
