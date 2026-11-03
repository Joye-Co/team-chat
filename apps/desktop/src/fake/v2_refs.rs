//! 引用提问 samples of the v2 team (inc-refs, Refs.dc.html §01 and §03): Ledger's second
//! report card with the 渠道表 in 增长 / 10 月数据月报, my question about B 站 and Ledger's
//! answer, and a message with three references, one to a message that was deleted since.
//!
//! Only [`V2Samples::with_refs`] has them, so other scenes of the sample team stay as they are.

use std::sync::Arc;

use chat_core::proto::{ArtifactVersionRef, BlockRefInput, PartKey, PartRef};

use super::*;
use crate::{
    fake::refs::FakeRefs,
    views::refs::{RefSource, parts},
};

/// The report card message (Ledger, 09:40).
pub const CARD_MESSAGE: &str = "t-v2-report-refs-card";
/// The weekly table Ledger posted and deleted later.
pub const DELETED_MESSAGE: &str = "t-v2-report-refs-weekly";
/// My question with one reference.
pub const ASKED_MESSAGE: &str = "t-v2-report-refs-ask";
/// My message with three references.
pub const THREE_MESSAGE: &str = "t-v2-report-refs-three";

/// Ledger's answer: a sentence and the weekly table (a Markdown table, `md:1`).
pub const REPLY_BODY: &str = "B 站 10 月新增 9 个群，9 月是 10 个，少了 1 个。少在月中：`10-12` 到 `10-23` 停更了 12 天，这两周只来了 2 个群；恢复更新后一周就来了 4 个。\n\n| 周 | B 站新增群 | 更新 |\n| --- | ---: | --- |\n| 10-06 | 3 | 正常 |\n| 10-13 | 1 | 停更 |\n| 10-20 | 1 | 停更 |\n| 10-27 | 4 | 恢复 |";
/// Ledger's answer (its Markdown table is referenceable).
pub const REPLY_MESSAGE: &str = "t-v2-report-m6";

/// The 渠道表 of the card (Refs.dc.html §01).
pub fn channel_table() -> (Vec<String>, Vec<Vec<String>>) {
    let s = |v: &[&str]| v.iter().map(|x| x.to_string()).collect::<Vec<_>>();
    (
        s(&["渠道", "新增群", "占比", "比 9 月"]),
        vec![
            s(&["小红书", "14", "37%", "+6"]),
            s(&["群内邀请", "10", "26%", "+1"]),
            s(&["B 站", "9", "24%", "-1"]),
            s(&["抖音", "5", "13%", "-2"]),
            s(&["合计", "38", "100%", "+4"]),
        ],
    )
}

/// The reference to the B 站 / 新增群 cell, as the table renders it.
pub fn bili_cell() -> crate::views::refs::PartTarget {
    let (columns, rows) = channel_table();
    let data = parts::TableData {
        columns: &columns,
        rows: &rows,
        row_keys: None,
    };
    parts::table(&data, parts::TablePick::Cell { row: 2, col: 1 })
        .expect("the sample cell")
        .target(
            RefSource::Message {
                group_id: GROWTH.into(),
                topic_id: T_REPORT.into(),
                message_id: CARD_MESSAGE.into(),
            },
            "新增群从哪来",
            "v1:1",
            None,
            None,
        )
}

fn element_ref() -> BlockRefInput {
    BlockRefInput {
        artifact: Some(ArtifactVersionRef {
            artifact_id: ART_REPORT.into(),
            version: 3,
        }),
        block_id: "page".into(),
        part: Some(PartRef {
            kind: "element".into(),
            keys: Some([("selector".to_string(), PartKey::Key("#budget-b".into()))].into()),
        }),
        label: "10 月数据月报 v3 · 预算 · 选项 B 站".into(),
        value: Some(serde_json::json!({ "text": "B 站" })),
        ..Default::default()
    }
}

fn weekly_ref() -> BlockRefInput {
    BlockRefInput {
        message_id: Some(DELETED_MESSAGE.into()),
        block_id: "md:1".into(),
        part: Some(PartRef {
            kind: "cell".into(),
            keys: Some(
                [
                    ("row".to_string(), PartKey::Index(3)),
                    ("col".to_string(), PartKey::Index(1)),
                ]
                .into(),
            ),
        }),
        label: "Ledger · 10-27 · 日活 2,340".into(),
        value: Some(
            serde_json::json!({ "row": "10-27", "column": "日活", "display": "2,340", "raw": 2340 }),
        ),
        ..Default::default()
    }
}

impl V2Samples {
    /// The sample team with the 引用提问 messages in 10 月数据月报.
    pub fn with_refs() -> Arc<Self> {
        let p = Self::new();
        let cell = bili_cell().input();
        p.with(|d| {
            let t = T_REPORT;
            let mut card_msg = message(GROWTH, t, 3, LEDGER, at(9, 40), "新增群从哪来");
            card_msg.id = CARD_MESSAGE.into();
            let (columns, rows) = channel_table();
            d.cards.insert(
                card_msg.id.clone(),
                card(serde_json::json!({
                    "v": 1,
                    "title": "新增群从哪来",
                    "tone": "success",
                    "status": { "label": "按渠道拆开" },
                    "blocks": [
                        { "type": "text", "text": "10 月新增 38 个群，小红书带来的最多。" },
                        { "type": "table", "columns": columns, "rows": rows }
                    ]
                })),
            );
            let mut weekly = message(GROWTH, t, 4, LEDGER, at(9, 46), "");
            weekly.id = DELETED_MESSAGE.into();
            weekly.deleted_at = Some(at(10, 5));
            let mut asked = message(GROWTH, t, 5, ME, at(10, 31), "");
            asked.id = ASKED_MESSAGE.into();
            asked.mentions = vec![LEDGER.into()];
            let reply = message(GROWTH, t, 6, LEDGER, at(10, 32), REPLY_BODY);
            let mut three = message(GROWTH, t, 7, ME, at(10, 40), "");
            three.id = THREE_MESSAGE.into();
            let list = d.messages.entry(t.into()).or_default();
            list.extend([card_msg, weekly, asked, reply, three]);
        });
        // What the server stored: the references and the quote lines in front of the bodies.
        p.with(|d| {
            let list = d.messages.get_mut(T_REPORT).expect("the report topic");
            let all: Vec<&Message> = list.iter().collect();
            let one = FakeRefs::stored(std::slice::from_ref(&cell), &all, T_REPORT);
            let three =
                FakeRefs::stored(&[cell.clone(), element_ref(), weekly_ref()], &all, T_REPORT);
            let b1 = p
                .refs
                .keep(ASKED_MESSAGE, one, "@Ledger 为什么比 9 月少了？");
            let b3 = p.refs.keep(
                THREE_MESSAGE,
                three,
                "这三个放在一起看：B 站断更那两周，是不是也把日活拖下来了？",
            );
            for m in list.iter_mut() {
                if m.id == ASKED_MESSAGE {
                    m.body = b1.clone();
                } else if m.id == THREE_MESSAGE {
                    m.body = b3.clone();
                }
            }
            let mut all: Vec<&mut Message> = list.iter_mut().collect();
            p.refs.source_deleted(DELETED_MESSAGE, &mut all);
        });
        p
    }
}

/// `send.blockRefs` on the sample team: the server's prefix and stored references.
pub fn send(p: &V2Samples, request: SendRequest, refs: Vec<BlockRefInput>) -> ClientMsgId {
    if refs.is_empty() {
        return p.send(request);
    }
    let stored = p.with(|d| {
        let all: Vec<&Message> = d.messages.values().flat_map(|l| l.iter()).collect();
        FakeRefs::stored(&refs, &all, &request.topic_id)
    });
    let body = crate::views::refs::with_prefix(&request.body, &stored);
    let (topic, group) = (request.topic_id.clone(), request.group_id.clone());
    let cid = p.send(SendRequest { body, ..request });
    let id = p.with(|d| {
        d.messages
            .get(&topic)
            .and_then(|l| l.iter().rev().find(|m| m.client_msg_id == cid))
            .map(|m| m.id.clone())
    });
    p.refs.keep(&id.unwrap_or_else(|| cid.clone()), stored, "");
    p.emit(vec![Change::Messages {
        group_id: group,
        topic_id: topic,
    }]);
    cid
}

// --- pb-refs: parts of Card v2 blocks (REFS §A2.2) ---

impl V2Samples {
    /// The sample team with Card v2 messages whose blocks have parts: 10 月的三个数
    /// (metrics and a line chart) in 增长 / 10 月数据月报, and the done plan and the
    /// evidence strip of 断线重连后消息重复 in its 话题.
    pub fn with_card_parts() -> Arc<Self> {
        use crate::views::cards::v2::samples as s;
        let p = Self::new();
        p.with(|d| {
            let mut add = |group: &str, topic: &str, author: &str, id: &str, when| {
                let list = d.messages.entry(topic.into()).or_default();
                let seq = list.iter().map(|m| m.created_seq).max().unwrap_or(0) + 1;
                let mut m = message(group, topic, seq, author, when, "");
                m.id = id.into();
                list.push(m);
            };
            add(GROWTH, T_REPORT, LEDGER, s::METRICS, at(9, 41));
            add(DEV, T_RECONNECT, FORGE, s::PLAN_DONE, at(10, 20));
            add(DEV, T_RECONNECT, FORGE, s::EVIDENCE, at(10, 21));
        });
        p
    }
}
