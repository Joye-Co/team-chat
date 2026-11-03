//! Screenshot scenes of 单独问 (the side chat track owns this slice).

#[allow(unused_imports)]
use crate::{
    app::RightPane,
    fake::v2 as d,
    selftest::scenes::{Scene, Start, open_main, v2_open_topic},
};

pub const SCENES: &[Scene] = &[Scene {
    name: "side-chat",
    about: "单独问 (SideChat.dc.html): Jojo answers about PR #61, with the suggestion button",
    start: Start::V2,
    setup: |s, cx| {
        v2_open_topic(s, d::DEV, d::T_RECONNECT, cx);
        s.nav.update(cx, |nav, cx| {
            nav.set_right_pane(RightPane::SideChat(d::JOJO.into()), cx)
        });
    },
    open: open_main,
}];
