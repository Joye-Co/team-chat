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
