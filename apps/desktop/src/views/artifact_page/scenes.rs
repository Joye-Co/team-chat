//! Screenshot scenes of the 产出 page (the artifact track owns this slice).

#[allow(unused_imports)]
use crate::{
    app::RightPane,
    fake::v2 as d,
    selftest::scenes::{Scene, Start, open_main, v2_open_topic},
};

pub const SCENES: &[Scene] = &[
    Scene {
        name: "artifact-v2",
        about: "产出 (Artifact.dc.html): 10 月数据月报 v3 live in the window, with 来龙去脉",
        start: Start::V2,
        setup: |s, cx| {
            s.nav.update(cx, |nav, cx| {
                nav.open_artifact(d::ART_REPORT.into(), None, cx)
            })
        },
        open: open_main,
    },
    Scene {
        name: "artifact-v2-old",
        about: "产出: 10 月数据月报 at v2, the old-version band and v2 picked in 版本",
        start: Start::V2,
        setup: |s, cx| {
            s.nav.update(cx, |nav, cx| {
                nav.open_artifact(d::ART_REPORT.into(), Some(2), cx)
            })
        },
        open: open_main,
    },
    Scene {
        name: "artifact-pane-v2",
        about: "The 产出 pane beside 10 月数据月报: the page live in the pane",
        start: Start::V2,
        setup: |s, cx| {
            v2_open_topic(s, d::GROWTH, d::T_REPORT, cx);
            s.nav.update(cx, |nav, cx| {
                nav.set_right_pane(
                    RightPane::Artifact {
                        id: d::ART_REPORT.into(),
                        version: None,
                    },
                    cx,
                )
            })
        },
        open: open_main,
    },
    // --- r2-trust ---
    Scene {
        name: "artifact-v2-choose",
        about: "产出: a real click on 抖音 in 10 月数据月报 sends exactly one choose event to the member",
        start: Start::V2,
        setup: super::gate_check::setup_choose,
        open: open_main,
    },
    Scene {
        name: "artifact-quiet-page",
        about: "产出: a page that calls emitEvent on load, from a timer and a script click sends nothing",
        start: Start::V2,
        setup: |_, _| {},
        open: super::gate_check::open_quiet,
    },
    // --- end r2-trust ---
    Scene {
        name: "artifacts-v2",
        about: "产出 destination: every 产出 with its thumbnail",
        start: Start::V2,
        setup: |s, cx| s.nav.update(cx, |nav, cx| nav.open_artifacts(cx)),
        open: open_main,
    },
];

/// True once the 产出 page's web view loaded and shows.
pub fn ready(cx: &gpui::App) -> bool {
    crate::ui::web_embed::live(cx).iter().any(|e| {
        let s = e.read(cx);
        s.artifact_id() == d::ART_REPORT
            && *s.phase() == crate::ui::web_embed::Phase::Ready
            && s.shown()
    })
}
