//! Sample cards and the in-memory card state of the fake provider.
//!
//! The samples cover every block type, every tone, every button style, a
//! confirm question, an unknown block type, long tables and lists, presses,
//! a press that waits for its answer and one the server refused.
//!
//! Fake presses: an action id that starts with `fail` is refused, one that
//! starts with `gone` is refused as gone, and any other press is recorded
//! and answered by the "bot" with an edit of the card (status 已处理, no
//! buttons), as real bots usually do.

use std::{collections::HashMap, sync::Arc};

use serde_json::json;

use crate::data::{
    CardPress, CardPressError, CardPressPhase, CardPressState, Millis,
    card::{Card, CardStatus, CardTone},
};

pub const GROUP: &str = "g-dev";
pub const TOPIC: &str = "t-cards";
pub const TOPIC_TITLE: &str = "卡片样例：发布与评审";

/// A sample card message: author, card, minutes ago.
pub struct Sample {
    pub author: &'static str,
    pub card: Card,
    pub minutes_ago: i64,
    /// Presses already made: (member, action id, minutes ago).
    pub presses: Vec<(&'static str, &'static str, i64)>,
    pub state: Option<CardPressState>,
}

fn card(value: serde_json::Value) -> Card {
    serde_json::from_value(value).expect("sample card parses")
}

/// Every sample card, oldest first.
pub fn samples() -> Vec<Sample> {
    let table_rows: Vec<serde_json::Value> = (1..=14)
        .map(|i| {
            json!([
                format!("crates/chat-core/src/mod_{i}.rs"),
                format!("{}", 40 + i * 7),
                if i % 4 == 0 { "失败" } else { "通过" },
                format!("{}.{} s", i % 3, i * 3 % 10),
            ])
        })
        .collect();
    let list_items: Vec<serde_json::Value> = (1..=13)
        .map(|i| {
            let status = ["done", "done", "in_progress", "todo", "failed", "skipped"][i % 6];
            json!({ "text": format!("第 {i} 步：检查第 {i} 个模块"), "status": status })
        })
        .collect();
    vec![
        Sample {
            author: "m-deploy",
            minutes_ago: 50,
            card: card(json!({
                "v": 1,
                "title": "发布 v0.9.2 到预发环境",
                "tone": "info",
                "status": { "label": "等待批准" },
                "blocks": [
                    { "type": "text", "text": "本次发布包含 **12 个** 修复，详见 [变更说明](https://example.com/changes)。" },
                    { "type": "fields", "fields": [
                        { "label": "版本", "value": "v0.9.2" },
                        { "label": "提交", "value": "a2a9c25" },
                        { "label": "申请人", "value": "小王" },
                        { "label": "预计耗时", "value": "6 分钟" }
                    ]},
                    { "type": "progress", "value": 0.35, "label": "构建中：35 / 100 个任务" },
                    { "type": "divider" },
                    { "type": "link", "label": "构建日志", "target": { "kind": "url", "url": "https://example.com/build/42" } },
                    { "type": "link", "label": "评审话题", "target": { "kind": "topic", "groupId": "g-dev", "topicId": "t-login" } },
                    { "type": "link", "label": "发布说明", "target": { "kind": "artifact", "artifactId": "art-1" } }
                ],
                "actions": [
                    { "id": "approve", "label": "批准", "style": "primary" },
                    { "id": "later", "label": "稍后" },
                    { "id": "reject", "label": "拒绝", "style": "danger", "confirm": "确定拒绝这次发布？" }
                ]
            })),
            presses: vec![],
            state: None,
        },
        Sample {
            author: "m-coder",
            minutes_ago: 40,
            card: card(json!({
                "v": 1,
                "title": "测试结果：14 个文件",
                "tone": "warning",
                "status": { "label": "3 个失败" },
                "blocks": [
                    { "type": "table", "columns": ["文件", "用例", "结果", "耗时"], "rows": table_rows },
                    { "type": "code", "lang": "rust", "text": "assert_eq!(window.len(), 100);\n// left: 99, right: 100" },
                    { "type": "future_block", "data": "新版本的块类型：旧客户端什么也不显示" }
                ],
                "actions": [
                    { "id": "rerun", "label": "重新运行", "style": "primary" },
                    { "id": "fail-open", "label": "打开报告" }
                ]
            })),
            presses: vec![("m-xiaowang", "rerun", 30)],
            state: Some(CardPressState {
                action_id: "fail-open".into(),
                phase: CardPressPhase::Failed(CardPressError::Other),
            }),
        },
        Sample {
            author: "m-reviewer",
            minutes_ago: 30,
            card: card(json!({
                "v": 1,
                "title": "上线检查清单",
                "blocks": [
                    { "type": "list", "items": list_items },
                    { "type": "image", "fileId": "f-screenshot", "alt": "登录页截图" }
                ],
                "actions": [
                    { "id": "ack", "label": "知道了" },
                    { "id": "assign", "label": "交给我", "style": "primary" }
                ]
            })),
            presses: vec![],
            state: Some(CardPressState {
                action_id: "assign".into(),
                phase: CardPressPhase::Sending,
            }),
        },
        Sample {
            author: "m-sentry",
            minutes_ago: 20,
            card: card(json!({
                "v": 1,
                "title": "线上报错：登录接口 5xx 激增",
                "tone": "danger",
                "status": { "label": "已处理" },
                "blocks": [
                    { "type": "fields", "fields": [
                        { "label": "接口", "value": "POST /v1/login" },
                        { "label": "错误率", "value": "12.4%" }
                    ]},
                    { "type": "text", "text": "已回滚到 v0.9.1。" }
                ]
            })),
            presses: vec![
                ("m-lixue", "ack", 19),
                ("m-chenchen", "ack", 18),
                ("m-joye", "rollback", 17),
                ("m-xiaowang", "ack", 16),
                ("m-unknown", "ack", 15),
            ],
            state: None,
        },
        Sample {
            author: "m-deploy",
            minutes_ago: 5,
            card: card(json!({
                "v": 1,
                "title": "数据库迁移完成",
                "tone": "success",
                "status": { "label": "完成" },
                "blocks": [
                    { "type": "progress", "value": 1 },
                    { "type": "text", "text": "迁移 `ext-1` 已在 3 台机器上完成。" }
                ],
                "actions": [
                    { "id": "gone-details", "label": "查看详情" }
                ]
            })),
            presses: vec![],
            state: None,
        },
        Sample {
            author: "m-taro",
            minutes_ago: 3,
            card: card(json!({
                "v": 1,
                "title": "发布预览",
                "tone": "warning",
                "status": { "label": "等你审" },
                "blocks": [
                    { "type": "fields", "fields": [
                        { "label": "平台", "value": "小红书" },
                        { "label": "时间", "value": "周四 20:00" },
                        { "label": "标题", "value": "夹夹 3.0：粉丝群的照片，一键分享回群里" }
                    ]},
                    { "type": "text", "text": "以前在群里找一张合照要翻半天。3.0 把群里的照片按活动自动整理好，点一下就能分享回群。" }
                ],
                "actions": [
                    { "id": "publish", "label": "发布", "style": "primary" },
                    { "id": "edit", "label": "修改" },
                    { "id": "drop", "label": "放弃" }
                ]
            })),
            presses: vec![],
            state: None,
        },
        Sample {
            author: "m-sentry",
            minutes_ago: 1,
            card: card(json!({
                "v": 1,
                "title": "要在 staging 先打开合并规则吗？",
                "tone": "warning",
                "status": { "label": "等你回答" },
                "blocks": [
                    { "type": "text", "text": "你选之前我不会改任何环境。生产要等 Scout 验收后再单独确认。" }
                ],
                "actions": [
                    { "id": "staging-now", "label": "现在在 staging 打开" },
                    { "id": "after-scout", "label": "等 Scout 验收完再开" },
                    { "id": "not-now", "label": "先不改，回到讨论" }
                ]
            })),
            presses: vec![],
            state: None,
        },
    ]
}

/// Labels of the actions that sample presses refer to but whose buttons the
/// "bot" already removed.
fn sample_label(action_id: &str) -> Option<&'static str> {
    match action_id {
        "ack" => Some("知道了"),
        "rollback" => Some("回滚"),
        _ => None,
    }
}

/// Card state of the fake provider.
#[derive(Default)]
pub struct FakeCards {
    cards: HashMap<String, Arc<Card>>,
    presses: HashMap<String, Vec<CardPress>>,
    states: HashMap<String, CardPressState>,
}

/// What a fake press did.
pub enum FakeOutcome {
    /// The press was refused; the state shows it.
    Refused,
    /// The press was recorded and the "bot" replaced the card.
    Edited,
    /// Unknown message or action.
    Ignored,
}

impl FakeCards {
    /// Adds a sample's card to the message `message_id`.
    pub fn add(&mut self, message_id: &str, sample: &Sample, now: Millis) {
        self.cards
            .insert(message_id.into(), Arc::new(sample.card.clone()));
        let presses = sample
            .presses
            .iter()
            .map(|(actor, action, ago)| CardPress {
                actor_id: (*actor).into(),
                action_id: (*action).into(),
                label: sample_label(action)
                    .map(str::to_string)
                    .or_else(|| label_of(&sample.card, action)),
                at: now - ago * 60_000,
            })
            .collect::<Vec<_>>();
        if !presses.is_empty() {
            self.presses.insert(message_id.into(), presses);
        }
        if let Some(state) = &sample.state {
            self.states.insert(message_id.into(), state.clone());
        }
    }

    pub fn card(&self, message_id: &str) -> Option<Arc<Card>> {
        self.cards.get(message_id).cloned()
    }

    pub fn presses(&self, message_id: &str) -> Vec<CardPress> {
        self.presses.get(message_id).cloned().unwrap_or_default()
    }

    pub fn state(&self, message_id: &str) -> Option<CardPressState> {
        self.states.get(message_id).cloned()
    }

    /// Replaces a card, as a bot's edit does.
    pub fn replace(&mut self, message_id: &str, card: Card) {
        self.cards.insert(message_id.into(), Arc::new(card));
    }

    pub fn remove(&mut self, message_id: &str) {
        self.cards.remove(message_id);
        self.presses.remove(message_id);
        self.states.remove(message_id);
    }

    /// Presses `action_id` on the card of `message_id` as `me`.
    pub fn press(
        &mut self,
        message_id: &str,
        action_id: &str,
        me: &str,
        now: Millis,
    ) -> FakeOutcome {
        let Some(card) = self.cards.get(message_id).cloned() else {
            return FakeOutcome::Ignored;
        };
        let Some(label) = label_of(&card, action_id) else {
            self.states.insert(
                message_id.into(),
                CardPressState {
                    action_id: action_id.into(),
                    phase: CardPressPhase::Failed(CardPressError::Gone),
                },
            );
            return FakeOutcome::Refused;
        };
        let refused = if action_id.starts_with("fail") {
            Some(CardPressError::Other)
        } else if action_id.starts_with("gone") {
            Some(CardPressError::Gone)
        } else {
            None
        };
        if let Some(error) = refused {
            self.states.insert(
                message_id.into(),
                CardPressState {
                    action_id: action_id.into(),
                    phase: CardPressPhase::Failed(error),
                },
            );
            return FakeOutcome::Refused;
        }
        self.states.remove(message_id);
        self.presses
            .entry(message_id.into())
            .or_default()
            .push(CardPress {
                actor_id: me.into(),
                action_id: action_id.into(),
                label: Some(label.clone()),
                at: now,
            });
        let mut next = (*card).clone();
        next.actions = None;
        next.tone = Some(CardTone::Success);
        next.status = Some(CardStatus {
            label: format!("已处理：{label}"),
        });
        self.cards.insert(message_id.into(), Arc::new(next));
        FakeOutcome::Edited
    }
}

fn label_of(card: &Card, action_id: &str) -> Option<String> {
    card.actions
        .as_deref()
        .unwrap_or(&[])
        .iter()
        .find(|a| a.id == action_id)
        .map(|a| a.label.clone())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::card::CardBlock;

    #[test]
    fn samples_are_valid_cards_except_the_unknown_block() {
        for s in samples() {
            let mut card = s.card.clone();
            card.blocks.retain(|b| !matches!(b, CardBlock::Unknown));
            card.validate()
                .unwrap_or_else(|e| panic!("{}: {e:?}", s.card.title));
        }
    }

    #[test]
    fn samples_cover_every_block_type() {
        let names: std::collections::HashSet<_> = samples()
            .iter()
            .flat_map(|s| s.card.blocks.iter().map(super::super::blocks::type_name))
            .collect();
        for name in [
            "text", "fields", "table", "list", "progress", "code", "image", "divider", "link",
            "unknown",
        ] {
            assert!(names.contains(name), "no sample has a {name} block");
        }
    }

    #[test]
    fn press_records_and_edits_or_refuses() {
        let mut cards = FakeCards::default();
        let all = samples();
        cards.add("m1", &all[0], 0);
        assert!(matches!(
            cards.press("m1", "approve", "me", 5),
            FakeOutcome::Edited
        ));
        assert_eq!(cards.presses("m1")[0].label.as_deref(), Some("批准"));
        assert!(cards.card("m1").unwrap().actions.is_none());
        // The button is gone now.
        assert!(matches!(
            cards.press("m1", "approve", "me", 6),
            FakeOutcome::Refused
        ));
        assert_eq!(
            cards.state("m1").unwrap().phase,
            CardPressPhase::Failed(CardPressError::Gone)
        );

        cards.add("m2", &all[1], 0);
        assert!(matches!(
            cards.press("m2", "fail-open", "me", 5),
            FakeOutcome::Refused
        ));
        assert!(matches!(
            cards.press("m2", "rerun", "me", 5),
            FakeOutcome::Edited
        ));
        assert!(cards.state("m2").is_none());
    }
}
