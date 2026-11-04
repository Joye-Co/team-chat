//! Screenshot scenes of the 成员 page (the members track owns this slice).

#[allow(unused_imports)]
use crate::{
    app::RightPane,
    fake::v2 as d,
    selftest::scenes::{Scene, Start, open_main, v2_open_topic},
};

pub const SCENES: &[Scene] = &[
    Scene {
        name: "members-v2",
        about: "成员 (Members.dc.html): the list with Forge's detail",
        start: Start::V2,
        setup: |s, cx| {
            s.nav
                .update(cx, |nav, cx| nav.open_members(Some(d::FORGE.into()), cx))
        },
        open: open_main,
    },
    // --- v13/ux-pages ---
    Scene {
        name: "members-person",
        about: "成员: a person's detail (小王): 私聊, every card with its compact empty",
        start: Start::V2,
        setup: |s, cx| {
            s.nav
                .update(cx, |nav, cx| nav.open_members(Some(d::XIAOWANG.into()), cx))
        },
        open: open_main,
    },
    Scene {
        name: "members-none",
        about: "成员 with no member picked: the Empty state in the detail",
        start: Start::V2,
        setup: |s, cx| s.nav.update(cx, |nav, cx| nav.open_members(None, cx)),
        open: open_main,
    },
    Scene {
        name: "members-loading",
        about: "成员 before the first sync: list and detail skeletons",
        start: Start::V2Unsynced,
        setup: |s, cx| s.nav.update(cx, |nav, cx| nav.open_members(None, cx)),
        open: open_main,
    },
    Scene {
        name: "members-failed",
        about: "成员 with nothing cached while the server cannot be reached: the Alert with 重试",
        start: Start::V2Unreachable,
        setup: |s, cx| s.nav.update(cx, |nav, cx| nav.open_members(None, cx)),
        open: open_main,
    },
    // --- end v13/ux-pages ---
];
