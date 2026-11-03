//! Screenshot scenes of ⌘K 交代或搜索 (Ask.dc.html; the switcher track owns
//! this slice).

use gpui::{AnyWindowHandle, App, Pixels, Size};

use crate::{
    app::Services,
    selftest::scenes::{Scene, Start, open_main},
};

fn open_ask(
    services: Services,
    content: Size<Pixels>,
    cx: &mut App,
) -> Result<AnyWindowHandle, String> {
    let handle = open_main(services.clone(), content, cx)?;
    handle
        .update(cx, |_, window, cx| {
            super::switcher::open(services, window, cx)
        })
        .map_err(|e| e.to_string())?;
    Ok(handle)
}

pub const SCENES: &[Scene] = &[Scene {
    name: "ask",
    about: "⌘K 交代或搜索 over the 首页 (stub: today's switcher)",
    start: Start::V2,
    setup: |s, cx| s.nav.update(cx, |nav, cx| nav.open_desk(cx)),
    open: open_ask,
}];
