//! The desk track's part of the v2 sample team (1.1.0 首页, Main.dc.html):
//! the v2 cards of three asks (Watch's release, Forge's PR #61, Taro's
//! draft), their answers and the presses of the v1 cards, kept in memory so
//! a press on the desk collapses into its record.

use std::{collections::HashMap, sync::Arc};

use chat_core::proto::CardV2;

use super::v2::*;
use crate::data::{
    CardPress, Millis,
    cards_v2::{CardV2Answer, CardV2AnswerRequest},
};

/// Answers and presses made on the sample desk.
#[derive(Default)]
pub struct DeskSamples {
    pub answers: HashMap<String, Vec<CardV2Answer>>,
    pub presses: HashMap<String, Vec<CardPress>>,
}

/// The id of the message of the release approval.
pub fn release_ask() -> String {
    format!("{T_RELEASE}-ask")
}
/// The id of Forge's done message with PR #61.
pub fn review_ask() -> String {
    format!("{T_RECONNECT}-m6")
}
/// The id of Taro's draft.
pub fn draft_ask() -> String {
    format!("{T_PROMO}-ask")
}
/// The id of Forge's choice (a v1 card).
pub fn choice_ask() -> String {
    format!("{T_INVITE}-ask")
}

fn v2(value: serde_json::Value) -> Arc<CardV2> {
    Arc::new(serde_json::from_value(value).expect("a valid sample card v2"))
}

/// The v2 card of a sample message, if it has one.
pub fn card_v2(message_id: &str) -> Option<Arc<CardV2>> {
    let id = message_id;
    if id == release_ask() {
        return Some(v2(serde_json::json!({
            "v": 2, "rev": 1, "intent": "ask",
            "title": "把 orderbot v2.14 发布到生产吗？",
            "fallback": "把 orderbot v2.14 发布到生产吗？",
            "ask": {
                "from": [ME],
                "irreversible": {
                    "verb": "发布",
                    "target": "生产",
                    "consequence": "所有群重启约 2 分钟；出问题可以一键回到 v2.13"
                }
            },
            "blocks": [
                { "type": "fields", "id": "facts", "fields": [
                    { "label": "包含", "value": "群相册分享、撤回同步、告警降噪，共 3 个 PR" },
                    { "label": "验收", "value": "Scout 今天 09:10 在 staging 验收通过" },
                    { "label": "影响", "value": "所有群重启约 2 分钟；出问题可以一键回到 v2.13" }
                ]}
            ],
            "actions": [
                { "id": "release", "label": "发布到生产", "style": "primary",
                  "confirm": { "title": "现在把 orderbot v2.14 发布到生产？", "body": "所有群重启约 2 分钟。" } },
                { "id": "hold", "label": "先不发" },
                { "id": "report", "label": "验收报告", "kind": "link",
                  "target": { "kind": "artifact", "artifactId": ART_RECONNECT } }
            ]
        })));
    }
    if id == review_ask() {
        return Some(v2(serde_json::json!({
            "v": 2, "rev": 1, "intent": "ask",
            "title": "改好了，请你审 PR #61",
            "fallback": "改好了，请你审 PR #61",
            "ask": { "from": [ME] },
            "blocks": [
                { "type": "text", "id": "why",
                  "text": "原因是客户端重连后从旧的 seq 重新拉取，服务器把已经收到的消息又推了一遍。现在按 seq 去重，重连时从最后一个确认过的 seq 继续；重连间隔也改成了指数退避。" },
                { "type": "ref", "id": "pr", "label": "#61",
                  "target": { "kind": "pr", "url": "https://github.com/example/orderbot/pull/61", "number": 61 } },
                { "type": "diff", "id": "diff", "text": "", "additions": 214, "deletions": 63 },
                { "type": "evidence", "id": "tests", "label": "测试", "state": "pass", "detail": "148 项通过" },
                { "type": "evidence", "id": "video", "label": "录屏", "state": "info", "detail": "0:42" }
            ],
            "actions": [
                { "id": "approve", "label": "批准合并", "style": "primary" },
                { "id": "changes", "label": "要求修改" },
                { "id": "diff", "label": "看改动", "kind": "link",
                  "target": { "kind": "pr", "url": "https://github.com/example/orderbot/pull/61", "number": 61 } }
            ]
        })));
    }
    if id == draft_ask() {
        return Some(v2(serde_json::json!({
            "v": 2, "rev": 1, "intent": "ask",
            "title": "这条微博确认后，周四 20:00 发出",
            "fallback": "这条微博确认后，周四 20:00 发出",
            "ask": {
                "from": [ME],
                "irreversible": { "verb": "发出", "target": "微博", "consequence": "发出后不能撤回" }
            },
            "blocks": [
                { "type": "group", "id": "post", "blocks": [
                    { "type": "image", "id": "p1", "fileId": "f-v2-promo-1" },
                    { "type": "image", "id": "p2", "fileId": "f-v2-promo-2" },
                    { "type": "image", "id": "p3", "fileId": "f-v2-promo-3" },
                    { "type": "draft", "id": "copy", "label": "拾光 3.0｜群里的照片，一键分享回群",
                      "text": "以前在群里找一张合照要翻半天。3.0 把群里的照片按活动自动整理好，点一下就能分享回群。",
                      "editable": true }
                ]}
            ],
            "actions": [
                { "id": "confirm", "label": "确认发布", "style": "primary" },
                { "id": "edit", "label": "改一下" },
                { "id": "drop", "label": "不发了", "kind": "cancel" }
            ]
        })));
    }
    None
}

impl DeskSamples {
    pub fn answer(&mut self, message_id: &str, answer: CardV2AnswerRequest, at: Millis) {
        let list = self.answers.entry(message_id.to_string()).or_default();
        // First answer per rev wins, as on the server.
        if list.iter().any(|a| a.rev == answer.rev) {
            return;
        }
        list.push(CardV2Answer {
            actor_id: ME.into(),
            rev: answer.rev,
            action_id: answer.action_id,
            inputs: answer.inputs,
            edited_values: answer.edited_values,
            at,
        });
    }

    /// 撤回: drops my last answer.
    pub fn withdraw(&mut self, message_id: &str) {
        if let Some(list) = self.answers.get_mut(message_id)
            && let Some(i) = list.iter().rposition(|a| a.actor_id == ME)
        {
            list.remove(i);
        }
    }

    pub fn press(&mut self, message_id: &str, action_id: &str, label: Option<String>, at: Millis) {
        self.presses
            .entry(message_id.to_string())
            .or_default()
            .push(CardPress {
                actor_id: ME.into(),
                action_id: action_id.into(),
                label,
                at,
            });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_desk_cards_parse_and_validate() {
        for id in [release_ask(), review_ask(), draft_ask()] {
            let card = card_v2(&id).expect("a v2 card");
            assert!(card.validate().is_ok(), "{id}: {:?}", card.validate());
        }
        assert!(card_v2(&choice_ask()).is_none());
    }
}
