//! Screenshot scenes of the 话题 page (the topic track owns this slice).

#[allow(unused_imports)]
use crate::{
    app::RightPane,
    fake::v2 as d,
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
        name: "topic-two-runs",
        about: "话题 page with two members at work (Forge builds the virtual list, Scout checks it): one plan card each",
        start: Start::V2TwoRuns,
        setup: |s, cx| v2_open_topic(s, d::DEV, d::T_VIRTUAL, cx),
        open: open_main,
    },
];

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
