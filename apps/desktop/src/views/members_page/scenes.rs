//! Screenshot scenes of the 成员 page (the members track owns this slice).

#[allow(unused_imports)]
use crate::{
    app::RightPane,
    fake::v2 as d,
    selftest::scenes::{Scene, Start, open_main, v2_open_topic},
};

pub const SCENES: &[Scene] = &[Scene {
    name: "members-v2",
    about: "成员 (Members.dc.html): the list with Forge's detail",
    start: Start::V2,
    setup: |s, cx| {
        s.nav
            .update(cx, |nav, cx| nav.open_members(Some(d::FORGE.into()), cx))
    },
    open: open_main,
}];
