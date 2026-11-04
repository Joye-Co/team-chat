//! Screenshot scenes of the 群 page (the group page track owns this slice).

#[allow(unused_imports)]
use crate::{
    app::RightPane,
    fake::v2 as d,
    selftest::scenes::{Scene, Start, open_main, v2_open_topic},
};

pub const SCENES: &[Scene] = &[
    Scene {
        name: "group-v2",
        about: "群 page (Group.dc.html): 开发 with 进行中, 最近完成, 群里的决定",
        start: Start::V2,
        setup: |s, cx| {
            s.nav
                .update(cx, |nav, cx| nav.open_group(d::DEV.into(), cx))
        },
        open: open_main,
    },
    // --- v13/ux-pages ---
    Scene {
        name: "group-empty",
        about: "群 page of a 群 with no 话题 yet: the Empty state with 新话题, compact empties in the right pane",
        start: Start::V2Bare,
        setup: |s, cx| {
            s.nav
                .update(cx, |nav, cx| nav.open_group(d::DEV.into(), cx))
        },
        open: open_main,
    },
    Scene {
        name: "group-loading",
        about: "群 page before the first sync: list and right-pane skeletons",
        start: Start::V2Unsynced,
        setup: |s, cx| {
            s.nav
                .update(cx, |nav, cx| nav.open_group(d::DEV.into(), cx))
        },
        open: open_main,
    },
    Scene {
        name: "group-failed",
        about: "群 page with nothing cached while the server cannot be reached: the Alert with 重试",
        start: Start::V2Unreachable,
        setup: |s, cx| {
            s.nav
                .update(cx, |nav, cx| nav.open_group(d::DEV.into(), cx))
        },
        open: open_main,
    },
    // --- end v13/ux-pages ---
    // --- v14/t3-pages ---
    Scene {
        name: "group-record",
        about: "群 page with the 记一条群里的决定 sheet open (the shared sheet form)",
        start: Start::V2,
        setup: |s, cx| {
            s.nav
                .update(cx, |nav, cx| nav.open_group(d::DEV.into(), cx))
        },
        open: |services, content, cx| {
            let handle = open_main(services.clone(), content, cx)?;
            handle
                .update(cx, |_, window, cx| {
                    super::open_record_sheet(services, d::DEV.into(), window, cx)
                })
                .map_err(|e| e.to_string())?;
            Ok(handle)
        },
    },
    Scene {
        name: "group-purpose",
        about: "群 page with the 群说明 sheet open (the shared sheet form)",
        start: Start::V2,
        setup: |s, cx| {
            s.nav
                .update(cx, |nav, cx| nav.open_group(d::DEV.into(), cx))
        },
        open: |services, content, cx| {
            let handle = open_main(services.clone(), content, cx)?;
            handle
                .update(cx, |_, window, cx| {
                    super::open_purpose_sheet(services, d::DEV.into(), window, cx)
                })
                .map_err(|e| e.to_string())?;
            Ok(handle)
        },
    },
    // --- end v14/t3-pages ---
];
