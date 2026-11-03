//! Screenshot scenes of the 首页 (the desk track owns this slice).

#[allow(unused_imports)]
use crate::{
    app::RightPane,
    fake::v2 as d,
    selftest::scenes::{Scene, Start, open_main, v2_open_topic},
};

pub const SCENES: &[Scene] = &[Scene {
    name: "desk",
    about: "首页 (Main.dc.html): Jojo's priority, 需要你拍板 4, 做完了 today, 正在做 and 排队",
    start: Start::V2,
    setup: |s, cx| s.nav.update(cx, |nav, cx| nav.open_desk(cx)),
    open: open_main,
}];
