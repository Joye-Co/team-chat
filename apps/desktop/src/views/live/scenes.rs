//! Screenshot scenes of 实时查看 (the live track owns this slice).

use gpui::App;

use crate::{
    app::{RightPane, Services},
    fake::v2 as d,
    selftest::scenes::{Scene, Start, open_main, v2_open_topic},
};

fn live(services: &Services, cx: &mut App) {
    v2_open_topic(services, d::DEV, d::T_VIRTUAL, cx);
    services.nav.update(cx, |nav, cx| {
        nav.set_right_pane(RightPane::Live(d::RUN_VIRTUAL.into()), cx)
    });
}

pub const SCENES: &[Scene] = &[
    Scene {
        name: "live",
        about: "实时查看 (Live.dc.html): Forge's computer on 话题列表虚拟滚动, 3/5",
        start: Start::V2,
        setup: live,
        open: open_main,
    },
    Scene {
        name: "live-takeover",
        about: "实时查看 after 接管: Forge is paused, 交还 is the primary action",
        start: Start::V2,
        setup: |s, cx| {
            live(s, cx);
            drop(
                s.provider
                    .take_over_run(d::DEV, d::T_VIRTUAL, d::RUN_VIRTUAL),
            );
        },
        open: open_main,
    },
];
