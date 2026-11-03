//! Screenshot scenes of the left rail (the rail track owns this slice).

use gpui::App;

use crate::{
    app::{RailMode, Services},
    selftest::scenes::{Scene, Start, open_main},
};

/// The rail over the 首页, as the artboards show it.
fn rail(services: &Services, mode: RailMode, cx: &mut App) {
    services.nav.update(cx, |nav, cx| nav.open_desk(cx));
    services
        .nav
        .update(cx, |nav, cx| nav.set_rail_mode(mode, cx));
}

pub const SCENES: &[Scene] = &[
    Scene {
        name: "rail-work",
        about: "Rail A 以工作为中心 (default): 话题 by 群 with live state over the 首页",
        start: Start::V2,
        setup: |s, cx| rail(s, RailMode::Work, cx),
        open: open_main,
    },
    Scene {
        name: "rail-attention",
        about: "Rail B 以「需要我」为中心: 需要你 / 在做 / 做完了 · 今天",
        start: Start::V2,
        setup: |s, cx| rail(s, RailMode::Attention, cx),
        open: open_main,
    },
    Scene {
        name: "rail-member",
        about: "Rail C 以成员为中心: members with the 话题 they work on",
        start: Start::V2,
        setup: |s, cx| rail(s, RailMode::Member, cx),
        open: open_main,
    },
];
