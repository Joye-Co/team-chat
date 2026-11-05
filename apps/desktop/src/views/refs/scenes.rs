//! Screenshot scenes of 引用提问 (the inc-refs track owns this slice; Refs.dc.html §01, §03).

use gpui::App;

use super::{PartTarget, RefSource, hub, parts};
use crate::{
    app::{RightPane, Services},
    data::{DraftKey, DraftRef},
    fake::v2::{self as d, refs_samples as r},
    selftest::scenes::{Scene, Start, open_main, v2_open_topic},
    views::cards::v2::samples as cards_v2,
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
        name: "refs-card-chart",
        about: "引用提问 on Card v2 parts: the pointer rests on the 10-27 point of 日活 · 近 4 周 in 10 月的三个数; the column lights up and a tooltip with the value and 问这里 shows",
        start: Start::V2RefParts,
        setup: |s, cx| {
            reset(cx);
            open_report(s, cx);
            hub(cx).update(cx, |h, _| {
                h.forced_hover = Some(format!("{}-p-trend-p0-3", cards_v2::METRICS));
            });
        },
        open: open_main,
    },
    Scene {
        name: "refs-card-step",
        about: "引用提问 on Card v2 parts: the pointer rests on step 3 (按 seq 去重…) of Forge's done plan; the row lights up and 问这里 follows its text; the evidence strip below has a part per proof",
        start: Start::V2RefParts,
        setup: |s, cx| {
            reset(cx);
            v2_open_topic(s, d::DEV, d::T_RECONNECT, cx);
            hub(cx).update(cx, |h, _| {
                h.forced_hover = Some(format!("{}-p-plan-s2", cards_v2::PLAN_DONE));
            });
        },
        open: open_main,
    },
    Scene {
        name: "refs-card-keyboard",
        about: "引用提问 by keyboard (REFS §A3): Tab put focus on Forge's done plan and ↓ moved the part focus to step 2 找原因; ⌥↵ would add it, ⇧⌥↵ the whole plan, Esc leaves",
        start: Start::V2RefParts,
        setup: |s, cx| {
            reset(cx);
            v2_open_topic(s, d::DEV, d::T_RECONNECT, cx);
            let block = format!("{}-k-plan", cards_v2::PLAN_DONE);
            let part = format!("{}-p-plan-s1", cards_v2::PLAN_DONE);
            hub(cx).update(cx, |h, _| {
                h.forced_focus = Some(block.clone());
                h.part_focus = Some((block, part));
            });
        },
        open: open_main,
    },
    Scene {
        name: "refs-card-metric",
        about: "引用提问 on Card v2 parts: the pointer rests on the 日活 metric of 10 月的三个数; the metric lights up with 问这里 in its corner",
        start: Start::V2RefParts,
        setup: |s, cx| {
            reset(cx);
            open_report(s, cx);
            hub(cx).update(cx, |h, _| {
                h.forced_hover = Some(format!("{}-p-dau-m", cards_v2::METRICS));
            });
        },
        open: open_main,
    },
    Scene {
        name: "refs-diagram-viewer",
        about: "The diagram viewer as it opens (at fit) with a node under the pointer: its outline and 问这里",
        start: Start::SignedIn,
        setup: |_, cx| reset(cx),
        open: |s, c, cx| {
            crate::diagrams::viewer::preview_hover_next(1);
            crate::diagrams::preview::open(s, c, crate::diagrams::preview::Scene::Viewer, cx)
        },
    },
];

// --- t5-media ---
/// `refs-too-many`: the five chips of `refs-composer`, then the sixth part refused with
/// the 最多引用 5 处 Toast bottom-right.
pub fn too_many_setup(s: &Services, cx: &mut App) {
    reset(cx);
    open_report(s, cx);
    let key = DraftKey::topic(d::GROWTH, d::T_REPORT);
    for t in five(crate::selftest::worst_case()) {
        s.provider.add_draft_ref(&key, chip(&t));
    }
}

/// Opens the main window, then pushes the Toast the sixth reference shows.
pub fn open_too_many(
    services: Services,
    content: gpui::Size<gpui::Pixels>,
    cx: &mut App,
) -> Result<gpui::AnyWindowHandle, String> {
    use gpui_component::WindowExt as _;
    let handle = open_main(services, content, cx)?;
    handle
        .update(cx, |_, window, cx| {
            window.push_notification(super::too_many_toast(|_, _| {}).notification(), cx)
        })
        .map_err(|e| e.to_string())?;
    Ok(handle)
}
// --- end t5-media ---

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
