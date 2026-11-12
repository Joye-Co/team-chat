//! Screenshot scenes of the 话题 page (the topic track owns this slice).

#[allow(unused_imports)]
use crate::{
    app::RightPane,
    fake::v2::{self as d, RelayShape},
    selftest::scenes::{Scene, Start, open_main, v2_open_topic},
};

pub const SCENES: &[Scene] = &[
    Scene {
        name: "topic-v2",
        about: "话题 page (Topic.dc.html): 开发 / 断线重连后消息重复 with the topic rail and the pinned review",
        start: Start::V2,
        setup: |s, cx| v2_open_topic(s, d::DEV, d::T_RECONNECT, cx),
        open: open_main,
    },
    Scene {
        name: "topic-merged",
        about: "话题 page after 批准合并: the pinned record, 合并中 in the rail and the title bar",
        start: Start::V2,
        setup: |s, cx| {
            v2_open_topic(s, d::DEV, d::T_RECONNECT, cx);
            drop(s.provider.approve_changes(d::DEV, d::T_RECONNECT));
        },
        open: open_main,
    },
    Scene {
        name: "topic-artifact-pane",
        about: "话题 page with the 产出 pane open on 重连测试报告 v2",
        start: Start::V2,
        setup: |s, cx| {
            v2_open_topic(s, d::DEV, d::T_RECONNECT, cx);
            s.nav.update(cx, |nav, cx| {
                nav.set_right_pane(
                    RightPane::Artifact {
                        id: d::ART_RECONNECT.into(),
                        version: None,
                    },
                    cx,
                )
            });
        },
        open: open_main,
    },
    Scene {
        name: "topic-done-output",
        about: "话题 page: Jojo's done card for a web 产出 (the 产出 first, the passed checks folded)",
        start: Start::V2,
        setup: |s, cx| v2_open_topic(s, d::GROWTH, d::T_REPORT, cx),
        open: open_main,
    },
    Scene {
        name: "topic-two-runs",
        about: "话题 page with two members at work (Forge builds the virtual list, Scout checks it): one plan card each",
        start: Start::V2TwoRuns,
        setup: |s, cx| v2_open_topic(s, d::DEV, d::T_VIRTUAL, cx),
        open: open_main,
    },
    // Usable (design-relay §3.6): who works here, and 按顺序做 at each state.
    Scene {
        name: "topic-participants",
        about: "话题 page with two AI members at work and the inspector open: both marked 在做 in 成员, the asker 提出",
        start: Start::V2Relay(RelayShape::TwoWorking),
        setup: |s, cx| {
            v2_open_topic(s, d::DEV, d::T_VIRTUAL, cx);
            s.nav
                .update(cx, |nav, cx| nav.set_right_pane(RightPane::TopicRail, cx));
        },
        open: open_main,
    },
    Scene {
        name: "topic-relay-working",
        about: "按顺序做 bar: Forge 做完了 → Scout 在做 → Forge, 第 2/3 步, 停止; the inspector marks Scout 在做 and Forge 参与过",
        start: Start::V2Relay(RelayShape::Working),
        setup: |s, cx| {
            v2_open_topic(s, d::DEV, d::T_VIRTUAL, cx);
            s.nav
                .update(cx, |nav, cx| nav.set_right_pane(RightPane::TopicRail, cx));
        },
        open: open_main,
    },
    Scene {
        name: "topic-relay-queued",
        about: "按顺序做 bar: Scout 排队, 等 Scout 手上的做完",
        start: Start::V2Relay(RelayShape::Queued),
        setup: |s, cx| v2_open_topic(s, d::DEV, d::T_VIRTUAL, cx),
        open: open_main,
    },
    Scene {
        name: "topic-relay-paused",
        about: "按顺序做 bar stopped at Scout (卡住了): 停在 Scout：卡住了, 重试 and 停止",
        start: Start::V2Relay(RelayShape::Paused),
        setup: |s, cx| v2_open_topic(s, d::DEV, d::T_VIRTUAL, cx),
        open: open_main,
    },
    Scene {
        name: "topic-relay-stopped",
        about: "按顺序做 bar after I stopped it: Scout 停止了, the line 我 停止了, no buttons",
        start: Start::V2Relay(RelayShape::Stopped),
        setup: |s, cx| v2_open_topic(s, d::DEV, d::T_VIRTUAL, cx),
        open: open_main,
    },
    Scene {
        name: "topic-relay-done",
        about: "按顺序做 bar when every step is done: 都做完了",
        start: Start::V2Relay(RelayShape::Done),
        setup: |s, cx| v2_open_topic(s, d::DEV, d::T_VIRTUAL, cx),
        open: open_main,
    },
    Scene {
        name: "topic-relay-order",
        about: "Composer with an order on its first line: 按顺序做：Forge → Scout → Forge above the text",
        start: Start::V2,
        setup: |s, cx| {
            draft(
                s,
                "@Forge 写 → @Scout 审 → @Forge 按意见改\n话题列表改成虚拟滚动",
            );
            v2_open_topic(s, d::DEV, d::T_VIRTUAL, cx);
        },
        open: open_main,
    },
    Scene {
        name: "topic-relay-discover",
        about: "Composer naming two AI members: 想让他们按顺序做？用 → 连起来",
        start: Start::V2,
        setup: |s, cx| {
            draft(s, "@Forge 和 @Scout 一起看看这个");
            v2_open_topic(s, d::DEV, d::T_VIRTUAL, cx);
        },
        open: open_main,
    },
    Scene {
        name: "topic-relay-refused",
        about: "Composer after the server refused my order: Scout 现在接不了这件事, in red",
        start: Start::V2Relay(RelayShape::Refused),
        setup: |s, cx| v2_open_topic(s, d::DEV, d::T_VIRTUAL, cx),
        open: open_main,
    },
];

/// Puts `text` in the composer draft of 开发 / 虚拟列表 before the window opens.
fn draft(s: &crate::app::Services, text: &str) {
    s.provider.save_draft_text(
        &crate::data::DraftKey::topic(d::DEV.to_string(), d::T_VIRTUAL.to_string()),
        text.to_string(),
        Vec::new(),
    );
}

/// The checks of the 话题 page scenes.
pub fn checks(name: &str) -> Option<crate::selftest::scenes::Checks> {
    use crate::selftest::drawn::{check, ids};
    match name {
        "topic-two-runs" => Some(|_, _| {
            let cards = ids("plan-card");
            vec![
                check(
                    cards.contains(d::RUN_VIRTUAL),
                    format!("Forge's plan card is drawn ({cards:?})"),
                ),
                check(
                    cards.contains(d::RUN_VIRTUAL_CHECK),
                    format!("Scout's plan card is drawn ({cards:?})"),
                ),
            ]
        }),
        _ => None,
    }
}
