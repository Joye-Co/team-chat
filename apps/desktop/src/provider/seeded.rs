//! A signed-in [`CoreProvider`] on a seeded chat-core cache, with no server: the
//! `topic-core` screenshot scene and the provider tests run the real provider on it.
//!
//! The cache holds one 群 with a 话题 that has a PR link, an answered ask marked `record`,
//! an open review ask for me, a 产出 with two versions (the second one on disk), a done run
//! with its usage, and a second 话题 that cites the 产出. The client is never started, so
//! nothing goes to the network except calls that fail at once on a closed local port.

use std::{path::Path, sync::Arc};

use chat_core::proto::{
    self, CardAnsweredData, Event, EventData, MessageCreatedData, RunUpdatedData, TopicCreatedData,
    TopicLinkedData,
};
use serde_json::json;

use super::{
    CoreProvider, ProviderConfig, open_client,
    session::{self, SavedSession, TokenStorage},
};
use crate::data::ConnectionState;

/// A port nothing listens on: calls fail at once.
const SERVER: &str = "http://127.0.0.1:9";

pub const ME: &str = "01SEEDME000000000000000000";
pub const FORGE: &str = "01SEEDFORGE000000000000000";
/// A bot that has not run this week.
pub const SCOUT: &str = "01SEEDSCOUT000000000000000";
pub const GROUP: &str = "01SEEDGROUP000000000000000";
pub const TOPIC: &str = "01SEEDTOPIC000000000000000";
pub const TOPIC_WEEKLY: &str = "01SEEDWEEKLY00000000000000";
pub const ARTIFACT: &str = "01SEEDART00000000000000000";
pub const RUN: &str = "01SEEDRUN00000000000000000";
/// Forge's run on the weekly report, at work now (实时查看, the desk's running list).
pub const RUN_WEEKLY: &str = "01SEEDRUNWEEKLY00000000000";
pub const RECORD_ASK: &str = "01SEEDASK00000000000000000";
pub const REVIEW_ASK: &str = "01SEEDREVIEW00000000000000";

fn from<T: serde::de::DeserializeOwned>(v: serde_json::Value) -> T {
    serde_json::from_value(v).expect("seed values match the protocol")
}

fn message(id: &str, topic: &str, author: &str, seq: u64, at: u64, body: &str) -> proto::Message {
    from(json!({
        "id": id, "clientMsgId": format!("c-{id}"), "groupId": GROUP, "topicId": topic,
        "authorId": author, "body": body, "mentions": [], "createdSeq": seq, "createdAt": at
    }))
}

fn card(
    id: &str,
    topic: &str,
    seq: u64,
    at: u64,
    fallback: &str,
    card: serde_json::Value,
) -> proto::Message {
    proto::Message {
        kind: Some(proto::MessageKind::Card),
        card_v2: Some(from(card)),
        ..message(id, topic, FORGE, seq, at, fallback)
    }
}

/// The page of version 2 of the 产出.
const REPORT_PAGE: &str = r#"<!doctype html><html><head><meta charset="utf-8"><title>重连测试报告</title></head>
<body><h1>重连测试报告</h1><p>修复后的客户端，每个场景跑 20 次，记录重连耗时和重复消息的条数。</p>
<h2>逐项结果</h2><table><tr><th>场景</th><th>结果</th><th>重复</th></tr>
<tr><td>断网 5 秒</td><td>✓</td><td>0</td></tr><tr><td>断网 60 秒</td><td>✓</td><td>0</td></tr>
<tr><td>合盖休眠 10 分钟</td><td>✓</td><td>0</td></tr><tr><td>切换 Wi-Fi</td><td>✓</td><td>0</td></tr>
<tr><td>服务器重启</td><td>✓</td><td>0</td></tr><tr><td>弱网（丢包 20%）</td><td>✓</td><td>0</td></tr></table>
</body></html>"#;

/// Makes the provider under `root` (emptied first). `now` is the time the 话题 ends at.
pub fn seeded(root: &Path, now: u64) -> Arc<CoreProvider> {
    let _ = std::fs::remove_dir_all(root);
    let provider = CoreProvider::new(ProviderConfig {
        data_root: root.to_path_buf(),
        tokens: TokenStorage::memory(),
        restore: false,
    });
    let me: proto::Member = from(json!({
        "id": ME, "kind": "human", "name": "Joye", "role": "负责人", "level": "owner"
    }));
    let forge: proto::Member = from(json!({
        "id": FORGE, "kind": "bot", "name": "Forge", "role": "工程师", "level": "member"
    }));
    let scout: proto::Member = from(json!({
        "id": SCOUT, "kind": "bot", "name": "Scout", "role": "调研", "level": "member"
    }));
    let group: proto::GroupSummary = from(json!({
        "group": { "id": GROUP, "kind": "group", "name": "开发", "memberIds": [ME, FORGE],
                   "createdBy": ME, "createdAt": now - 86_400_000 },
        "lastSeq": 15, "readSeq": 15, "unread": 0, "mentions": 0
    }));
    let dir = session::account_dir(root, SERVER, ME);
    let client = open_client(SERVER, String::new(), &dir).expect("a cache");
    let store = client.store();
    store
        .apply_welcome(&me, &[me.clone(), forge, scout], &[group])
        .expect("welcome");

    let min = 60_000;
    let t0 = now - 70 * min;
    let topic: proto::Topic = from(json!({
        "id": TOPIC, "groupId": GROUP, "title": "断线重连后消息重复", "status": "waiting_review",
        "createdBy": ME, "createdAt": t0, "rootMessageId": "01SEEDM1", "lastSeq": 11,
        "messageCount": 6, "waitingOn": [ME], "ownerId": FORGE, "lastMessageAt": now - min
    }));
    let weekly: proto::Topic = from(json!({
        "id": TOPIC_WEEKLY, "groupId": GROUP, "title": "本周周报", "status": "open",
        "createdBy": ME, "createdAt": t0, "rootMessageId": "01SEEDW1", "lastSeq": 14,
        "messageCount": 2
    }));
    let artifact = |version: u32, at: u64| -> proto::Artifact {
        from(json!({
            "id": ARTIFACT, "groupId": GROUP, "topicId": TOPIC, "title": "重连测试报告",
            "kind": "html", "currentVersion": version, "createdBy": FORGE,
            "createdAt": t0 + 30 * min, "updatedAt": at
        }))
    };
    let version = |version: u32, at: u64, note: &str| -> proto::ArtifactVersion {
        from(json!({
            "artifactId": ARTIFACT, "version": version, "entry": "index.html",
            "files": [{ "path": "index.html", "fileId": format!("01SEEDFILE{version}"),
                        "size": REPORT_PAGE.len(), "mime": "text/html" }],
            "note": note, "createdBy": FORGE, "createdAt": at, "runId": RUN
        }))
    };
    let run: proto::BotRun = from(json!({
        "id": RUN, "botId": FORGE, "groupId": GROUP, "topicId": TOPIC, "status": "done",
        "requestedBy": ME, "createdAt": t0 + min, "startedAt": t0 + min,
        "endedAt": t0 + 42 * min, "updatedAt": t0 + 42 * min,
        "usage": { "inputTokens": 182_000, "outputTokens": 21_000, "costUsd": 0.86 },
        "durationMs": 41 * min, "resultMessageId": "01SEEDDONE"
    }));
    let weekly_run: proto::BotRun = from(json!({
        "id": RUN_WEEKLY, "botId": FORGE, "groupId": GROUP, "topicId": TOPIC_WEEKLY,
        "status": "running", "requestedBy": ME, "createdAt": now - 9 * min,
        "startedAt": now - 9 * min, "updatedAt": now - min,
        "plan": [
            { "text": "收集本周合并的 PR", "state": "done" },
            { "text": "写周报正文", "state": "current" },
            { "text": "发到话题", "state": "pending" }
        ]
    }));
    let pr = proto::TopicLink::Pr(proto::PrLink {
        merge: None,
        repo: "acme/chat".into(),
        number: 61,
        url: "https://github.com/acme/chat/pull/61".into(),
        title: "按 seq 去重，从最后确认的 seq 续传".into(),
        state: "open".into(),
        checks: ["build", "test", "lint"]
            .iter()
            .map(|n| proto::PrCheck {
                name: (*n).into(),
                conclusion: "success".into(),
            })
            .collect(),
        additions: Some(214),
        deletions: Some(63),
        changed_files: Some(12),
    });
    let record_ask = json!({
        "v": 2, "rev": 1, "intent": "ask", "title": "重试间隔怎么改？", "fallback": "重试间隔怎么改？",
        "ask": { "from": [ME], "record": true },
        "blocks": [{ "type": "decision", "id": "choice", "options": [
            { "id": "backoff", "label": "改成指数退避，放进同一个 PR" },
            { "id": "later", "label": "下一个 PR 再改" }
        ] }],
        "actions": [{ "id": "ok", "label": "确定", "style": "primary" }]
    });
    let done = json!({
        "v": 2, "rev": 1, "intent": "done", "title": "修好了断线重连后的重复消息",
        "fallback": "修好了断线重连后的重复消息", "data": { "runId": RUN },
        "blocks": [
            { "type": "text", "id": "result", "text": "修好了断线重连后的重复消息" },
            { "type": "evidence", "id": "tests", "label": "测试：148 个通过", "state": "pass" },
            { "type": "ref", "id": "pr-1", "label": "acme/chat#61",
              "target": { "kind": "pr", "url": "https://github.com/acme/chat/pull/61",
                          "repo": "acme/chat", "number": 61 } },
            { "type": "ref", "id": "artifact-1", "label": "重连测试报告（第 2 版）",
              "target": { "kind": "artifact", "artifactId": ARTIFACT, "version": 2 } }
        ]
    });
    let review = json!({
        "v": 2, "rev": 1, "intent": "ask", "title": "审 PR #61：按 seq 去重", "fallback": "请审 PR #61",
        "ask": { "from": [ME] },
        "blocks": [
            { "type": "text", "id": "why", "text": "12 个文件，测试都过了。" },
            { "type": "ref", "id": "pr", "label": "acme/chat#61",
              "target": { "kind": "pr", "url": "https://github.com/acme/chat/pull/61",
                          "repo": "acme/chat", "number": 61 } }
        ],
        "actions": [
            { "id": "approve", "label": "批准合并", "style": "primary" },
            { "id": "changes", "label": "要求修改" }
        ]
    });
    let cite = json!({
        "v": 2, "rev": 1, "intent": "info", "title": "本周周报", "fallback": "本周周报",
        "blocks": [{ "type": "ref", "id": "r", "label": "重连测试报告",
                     "target": { "kind": "artifact", "artifactId": ARTIFACT, "version": 2 } }]
    });
    let ev = |seq: u64, actor: &str, at: u64, data: EventData| Event {
        seq,
        group_id: GROUP.into(),
        at,
        actor_id: actor.into(),
        data,
    };
    let created = |m: proto::Message| {
        let (seq, at, actor) = (m.created_seq, m.created_at, m.author_id.clone());
        ev(
            seq,
            &actor,
            at,
            EventData::MessageCreated(Box::new(MessageCreatedData { message: m })),
        )
    };
    let events = vec![
        ev(
            1,
            ME,
            t0,
            EventData::TopicCreated(TopicCreatedData { topic }),
        ),
        created(message(
            "01SEEDM1",
            TOPIC,
            ME,
            2,
            t0,
            "断线重连以后，同一条消息会出现两次。",
        )),
        created(message(
            "01SEEDM2",
            TOPIC,
            FORGE,
            3,
            t0 + 2 * min,
            "找到了：重连后从头拉了一遍。改成按 seq 去重，从最后确认的 seq 续传。",
        )),
        created(card(
            RECORD_ASK,
            TOPIC,
            4,
            t0 + 10 * min,
            "重试间隔怎么改？",
            record_ask,
        )),
        ev(
            5,
            ME,
            t0 + 12 * min,
            EventData::CardAnswered(CardAnsweredData {
                message_id: RECORD_ASK.into(),
                rev: 1,
                action_id: "ok".into(),
                inputs: Some([("choice".to_string(), json!("backoff"))].into()),
                edited_values: None,
                ..Default::default()
            }),
        ),
        ev(
            6,
            FORGE,
            t0 + 30 * min,
            EventData::ArtifactCreated(proto::ArtifactVersionResponse {
                artifact: artifact(1, t0 + 30 * min),
                version: version(1, t0 + 30 * min, "第一版"),
            }),
        ),
        ev(
            7,
            FORGE,
            t0 + 40 * min,
            EventData::ArtifactVersionAdded(proto::ArtifactVersionResponse {
                artifact: artifact(2, t0 + 40 * min),
                version: version(2, t0 + 40 * min, "补了弱网场景"),
            }),
        ),
        ev(
            8,
            FORGE,
            t0 + 41 * min,
            EventData::TopicLinked(TopicLinkedData {
                topic_id: TOPIC.into(),
                link: pr,
            }),
        ),
        created(card(
            "01SEEDDONE",
            TOPIC,
            9,
            t0 + 42 * min,
            "修好了断线重连后的重复消息",
            done,
        )),
        ev(
            10,
            FORGE,
            t0 + 42 * min,
            EventData::RunUpdated(RunUpdatedData { run }),
        ),
        created(card(
            REVIEW_ASK,
            TOPIC,
            11,
            now - min,
            "请审 PR #61",
            review,
        )),
        ev(
            12,
            ME,
            t0,
            EventData::TopicCreated(TopicCreatedData { topic: weekly }),
        ),
        created(message(
            "01SEEDW1",
            TOPIC_WEEKLY,
            ME,
            13,
            t0,
            "这周的周报。",
        )),
        created(card(
            "01SEEDW2",
            TOPIC_WEEKLY,
            14,
            t0 + 50 * min,
            "本周周报",
            cite,
        )),
        ev(
            15,
            FORGE,
            now - min,
            EventData::RunUpdated(RunUpdatedData { run: weekly_run }),
        ),
    ];
    store.apply_events(GROUP, &events).expect("events");

    // Version 2 is on disk, as after it was opened once.
    if let Ok(dir) =
        chat_core::artifacts::version_dir(&client.config().artifacts_dir(), ARTIFACT, 2)
    {
        let _ = std::fs::create_dir_all(&dir);
        let _ = std::fs::write(dir.join("index.html"), REPORT_PAGE);
        let mut marker = dir.as_os_str().to_owned();
        marker.push(".complete");
        let _ = std::fs::write(std::path::PathBuf::from(marker), b"");
    }

    provider.install(
        SavedSession {
            server_url: SERVER.into(),
            member: me,
        },
        client,
        dir,
    );
    provider.state().connection = Some(ConnectionState::Online);
    provider
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::{
        ChatProvider as _,
        v2::{AskKind, ChangeState},
    };

    #[test]
    fn the_real_provider_reads_the_v2_topic_parts_from_the_cache() {
        let root = std::env::temp_dir().join(format!("seeded-provider-{}", std::process::id()));
        let now = chrono::Utc::now().timestamp_millis() as u64;
        let p = seeded(&root, now);

        let ch = p
            .topic_changes(GROUP, TOPIC)
            .expect("改动 from the PR link");
        assert_eq!(
            (ch.number, ch.additions, ch.deletions, ch.files),
            (61, 214, 63, 12)
        );
        assert_eq!((ch.checks_passed, ch.checks_total), (3, 3));
        assert_eq!(ch.state, ChangeState::Open);

        let decisions = p.topic_decisions(GROUP, TOPIC);
        assert_eq!(decisions.len(), 1);
        assert!(decisions[0].text.contains("改成指数退避，放进同一个 PR"));
        assert_eq!(decisions[0].by, ME);

        let usage = p.run_usage(RUN).expect("usage");
        assert_eq!(usage.duration_ms, 41 * 60_000);
        assert_eq!(usage.input_tokens, Some(182_000));

        let story = p.artifact_story(ARTIFACT).expect("来龙去脉");
        assert_eq!(story.topic, Some((GROUP.into(), TOPIC.into())));
        assert_eq!(story.made_by.as_deref(), Some(FORGE));
        assert_eq!(story.asked_by.as_deref(), Some(ME));
        assert_eq!(
            story.version_notes,
            [(2, "补了弱网场景".to_string()), (1, "第一版".to_string())]
        );
        assert_eq!(story.decisions.len(), 1);
        assert_eq!(
            story.cited_by,
            [(GROUP.into(), TOPIC_WEEKLY.into(), "本周周报".into())]
        );

        let preview = p.artifact_preview(ARTIFACT, 2).expect("a preview of v2");
        assert_eq!(preview.title, "重连测试报告");
        assert_eq!(preview.rows.len(), 6);
        assert!(preview.rows.iter().all(|r| r.ok == Some(true)));
        assert_eq!(p.artifact_thumb(ARTIFACT).map(|t| t.rows.len()), Some(4));

        // The pinned card is the review ask; 批准合并 answers it and the change merges.
        let ask = p.pinned_ask(GROUP, TOPIC).expect("an ask for me");
        assert_eq!(
            (ask.kind, ask.message_id.as_deref()),
            (AskKind::Review, Some(REVIEW_ASK))
        );
        assert_eq!(p.changes_approval(GROUP, TOPIC), None);
        drop(p.approve_changes(GROUP, TOPIC));
        let pending = p.card_v2_pending(GROUP, REVIEW_ASK).expect("my answer");
        assert_eq!(pending.action_id, "approve");
        let (by, _) = p.changes_approval(GROUP, TOPIC).expect("approved");
        assert_eq!(by, ME);
        assert_eq!(
            p.topic_changes(GROUP, TOPIC).map(|c| c.state),
            Some(ChangeState::Merging)
        );

        // Without a review card there is nothing to approve.
        assert_eq!(p.changes_approval(GROUP, TOPIC_WEEKLY), None);
        p.sign_out_local_for_selftest();
        let _ = std::fs::remove_dir_all(&root);
    }
    #[test]
    fn an_anonymous_run_names_no_requester() {
        let root = std::env::temp_dir().join(format!("seeded-anon-{}", std::process::id()));
        let now = chrono::Utc::now().timestamp_millis() as u64;
        let p = seeded(&root, now);
        assert_eq!(
            p.runs(GROUP, TOPIC)
                .iter()
                .find(|r| r.id == RUN)
                .and_then(|r| r.requested_by.clone())
                .as_deref(),
            Some(ME)
        );

        // An answer to a private card started the run: the server names nobody.
        let run: proto::BotRun = from(json!({
            "id": RUN, "botId": FORGE, "groupId": GROUP, "topicId": TOPIC, "status": "done",
            "requestedBy": "anonymous", "createdAt": now - 60_000, "updatedAt": now
        }));
        let event = Event {
            seq: 16,
            group_id: GROUP.into(),
            at: now,
            actor_id: FORGE.into(),
            data: EventData::RunUpdated(RunUpdatedData { run }),
        };
        let a = p.account().expect("signed in");
        a.client
            .store()
            .apply_events(GROUP, &[event])
            .expect("event");
        let runs = p.runs(GROUP, TOPIC);
        let r = runs.iter().find(|r| r.id == RUN).expect("the run");
        assert_eq!(r.requested_by, None);
        let story = p.artifact_story(ARTIFACT).expect("来龙去脉");
        assert_eq!(story.asked_by, None);
        p.sign_out_local_for_selftest();
        let _ = std::fs::remove_dir_all(&root);
    }
}

// desktop-data: the page aggregates on the seeded cache (成员, 首页, 群, 产出).
#[cfg(test)]
mod data_tests;
