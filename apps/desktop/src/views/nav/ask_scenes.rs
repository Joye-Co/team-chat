//! Screenshot scenes of ⌘K 交代或搜索 (Ask.dc.html; the rail track owns
//! this slice).

use gpui::{AnyWindowHandle, App, Pixels, Size};

use crate::{
    app::Services,
    fake::v2 as d,
    selftest::scenes::{Scene, Start, open_main},
};

/// The artboard's sentence: Ledger is named, so 交给 Ledger comes first.
pub const QUERY: &str = "让 Ledger 拉一下 10 月各渠道的新增";

fn open_ask(
    services: Services,
    content: Size<Pixels>,
    cx: &mut App,
) -> Result<AnyWindowHandle, String> {
    let handle = open_main(services.clone(), content, cx)?;
    handle
        .update(cx, |_, window, cx| {
            crate::views::ask::open_with(services, QUERY, Some(d::GROWTH.into()), window, cx);
        })
        .map_err(|e| e.to_string())?;
    Ok(handle)
}

fn open_ask_empty(
    services: Services,
    content: Size<Pixels>,
    cx: &mut App,
) -> Result<AnyWindowHandle, String> {
    let handle = open_main(services.clone(), content, cx)?;
    handle
        .update(cx, |_, window, cx| crate::views::ask::open(services, window, cx))
        .map_err(|e| e.to_string())?;
    Ok(handle)
}

pub const SCENES: &[Scene] = &[
    Scene {
        name: "ask",
        about: "⌘K 交代或搜索 over the 首页 with a sentence typed: 交给 Ledger, 交给 Jojo 安排, then matches",
        start: Start::V2,
        setup: |s, cx| s.nav.update(cx, |nav, cx| nav.open_desk(cx)),
        open: open_ask,
    },
    Scene {
        name: "ask-empty",
        about: "⌘K 交代或搜索 with no query: 最近, 群, 私聊",
        start: Start::V2,
        setup: |s, cx| s.nav.update(cx, |nav, cx| nav.open_desk(cx)),
        open: open_ask_empty,
    },
];
