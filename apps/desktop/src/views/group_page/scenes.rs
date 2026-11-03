//! Screenshot scenes of the 群 page (the group page track owns this slice).

#[allow(unused_imports)]
use crate::{
    app::RightPane,
    fake::v2 as d,
    selftest::scenes::{Scene, Start, open_main, v2_open_topic},
};

pub const SCENES: &[Scene] = &[Scene {
    name: "group-v2",
    about: "群 page (Group.dc.html): 开发 with 进行中, 最近完成, 群里的决定",
    start: Start::V2,
    setup: |s, cx| {
        s.nav
            .update(cx, |nav, cx| nav.open_group(d::DEV.into(), cx))
    },
    open: open_main,
}];
