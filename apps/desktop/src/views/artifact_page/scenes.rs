//! Screenshot scenes of the 产出 page (the artifact track owns this slice).

#[allow(unused_imports)]
use crate::{
    app::RightPane,
    fake::v2 as d,
    selftest::scenes::{Scene, Start, open_main, v2_open_topic},
};

pub const SCENES: &[Scene] = &[Scene {
    name: "artifact-v2",
    about: "产出 (Artifact.dc.html): 10 月数据月报 v3 with 来龙去脉",
    start: Start::V2,
    setup: |s, cx| {
        s.nav.update(cx, |nav, cx| {
            nav.open_artifact(d::ART_REPORT.into(), None, cx)
        })
    },
    open: open_main,
}];
