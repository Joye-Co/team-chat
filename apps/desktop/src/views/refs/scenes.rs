//! Screenshot scenes of 引用提问 (the inc-refs track owns this slice; Refs.dc.html §01, §03).

use gpui::App;

use super::{PartTarget, RefSource, hub, parts};
use crate::{
    app::{RightPane, Services},
    data::{DraftKey, DraftRef},
    fake::v2::{self as d, refs_samples as r},
    selftest::scenes::{Scene, Start, open_main, v2_open_topic},
};

pub const SCENES: &[Scene] = &[
    Scene {
        name: "refs-hover",
        about: "引用提问: the pointer rests on B 站 in Ledger's 渠道表; the row lights up and 问这里 shows at its end",
        start: Start::V2Refs,
        setup: |s, cx| {
            reset(cx);
            open_report(s, cx);
            hub(cx).update(cx, |h, _| {
                h.forced_hover = Some(format!("{}-v1:1-c2-1", r::CARD_MESSAGE));
            });
        },
        open: open_main,
    },
    Scene {
        name: "refs-markdown",
        about: "引用提问 on a Markdown table: the pointer rests on 10-13 in Ledger's weekly table; 问这里 at the row's end",
        start: Start::V2Refs,
        setup: |s, cx| {
            reset(cx);
            open_report(s, cx);
            hub(cx).update(cx, |h, _| {
                h.forced_hover = Some(format!("{}-md:1-c1-1", r::REPLY_MESSAGE));
            });
        },
        open: open_main,
    },
    Scene {
        name: "refs-composer",
        about: "引用提问: five reference chips in the 话题 composer (a cell, a point, a metric, a step, a 产出 element), @Ledger and the question",
        start: Start::V2Refs,
        setup: |s, cx| {
            reset(cx);
            open_report(s, cx);
            let key = DraftKey::topic(d::GROWTH, d::T_REPORT);
            for t in five(crate::selftest::worst_case()) {
                s.provider.add_draft_ref(&key, chip(&t));
            }
            s.provider.save_draft_text(
                &key,
                "@Ledger 这几处放在一起看：B 站断更那两周，是不是也把日活拖下来了？".into(),
                vec![d::LEDGER.into()],
            );
        },
        open: open_main,
    },
    Scene {
        name: "refs-sent",
        about: "引用提问: my question with one reference above its body, Ledger's answer, and a message with three references (one source deleted: 原消息已删除)",
        start: Start::V2Refs,
        setup: |s, cx| {
            reset(cx);
            open_report(s, cx);
        },
        open: open_main,
    },
    Scene {
        name: "refs-side-chat",
        about: "引用提问 in 单独问: the B 站 cell as a chip in the composer of the aside with Ledger",
        start: Start::V2Refs,
        setup: |s, cx| {
            reset(cx);
            open_report(s, cx);
            s.nav.update(cx, |nav, cx| {
                nav.set_right_pane(RightPane::SideChat(d::LEDGER.into()), cx)
            });
            let key = (
                d::GROWTH.to_string(),
                d::T_REPORT.to_string(),
                d::LEDGER.to_string(),
            );
            let h = hub(cx);
            h.update(cx, |h, cx| {
                h.side_chat_focused = true;
                h.add_side(key, chip(&r::bili_cell()), cx);
            });
        },
        open: open_main,
    },
    Scene {
        name: "refs-diagram-viewer",
        about: "The diagram viewer zoomed to 250 % with a node under the pointer: its ring and 问这里",
        start: Start::SignedIn,
        setup: |_, cx| reset(cx),
        open: |s, c, cx| {
            crate::diagrams::viewer::preview_hover_next(1);
            crate::diagrams::preview::open(s, c, crate::diagrams::preview::Scene::Viewer, cx)
        },
    },
];

/// Scenes that wait for something before the capture.
pub fn ready(name: &str) -> Option<fn(&App) -> bool> {
    match name {
        "refs-diagram-viewer" => {
            Some(|cx| crate::diagrams::preview::ready(crate::diagrams::preview::Scene::Viewer, cx))
        }
        _ => None,
    }
}

/// The shared state starts empty in every scene (scenes run one after another in one app).
fn reset(cx: &mut App) {
    let h = hub(cx);
    h.update(cx, |h, _| *h = super::RefsHub::default());
}

fn open_report(s: &Services, cx: &mut App) {
    v2_open_topic(s, d::GROWTH, d::T_REPORT, cx);
}

fn chip(t: &PartTarget) -> DraftRef {
    DraftRef {
        input: t.input(),
        group_id: d::GROWTH.into(),
        topic_id: d::T_REPORT.into(),
        lead_chars: super::label::lead_of(&t.label),
    }
}

fn source(message: &str) -> RefSource {
    RefSource::Message {
        group_id: d::GROWTH.into(),
        topic_id: d::T_REPORT.into(),
        message_id: message.into(),
    }
}

/// Five references of different kinds (Refs.dc.html §03 and §02).
fn five(worst: bool) -> Vec<PartTarget> {
    let (title, chart_title) = if worst {
        (
            "第四季度各渠道新增群和留存的完整月报（含去重说明）",
            "按周的日活和九月同期对比折线",
        )
    } else {
        ("10 月的三个数", "日活折线")
    };
    let x = ["10-06", "10-13", "10-20", "10-27"].map(String::from);
    let series = [parts::ChartSeriesData {
        key: "dau".into(),
        name: "日活".into(),
        values: vec![Some(1980.), Some(2105.), Some(2210.), Some(2340.)],
    }];
    let chart = parts::ChartData {
        bar: false,
        x: &x,
        series: &series,
        unit: None,
    };
    let point = parts::chart(&chart, parts::ChartPick::Point { series: 0, x: 3 })
        .expect("sample point")
        .target(
            source(r::CARD_MESSAGE),
            title,
            "dau",
            Some(chart_title),
            Some(1),
        );
    let metric = parts::metric("新增群", 38., None, Some(12.), None).target(
        source(r::CARD_MESSAGE),
        title,
        "new-groups",
        None,
        Some(1),
    );
    let step = parts::step(2, "跑一万条消息的压测", "working", None).target(
        source(r::CARD_MESSAGE),
        "话题列表虚拟滚动",
        "plan",
        None,
        None,
    );
    let page = PartTarget {
        source: RefSource::Artifact {
            group_id: d::GROWTH.into(),
            topic_id: d::T_REPORT.into(),
            artifact_id: d::ART_REPORT.into(),
            version: 3,
        },
        block_id: "page".into(),
        part: Some(chat_core::proto::card2::PartRef {
            kind: "element".into(),
            keys: Some(
                [(
                    "selector".to_string(),
                    chat_core::proto::card2::PartKey::Key("#budget-b".into()),
                )]
                .into(),
            ),
        }),
        label: super::label::build(
            &parts::artifact_source("10 月数据月报", 3),
            Some("预算"),
            Some("选项 B 站"),
        )
        .text,
        value: Some(serde_json::json!({ "text": "B 站" })),
        card_rev: None,
    };
    vec![r::bili_cell(), point, metric, step, page]
}
