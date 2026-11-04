//! Screenshot scenes of ⌘K 交代或搜索 (Ask.dc.html) and the states of the
//! command palette (COMPLETENESS 2.11): no match, loading, failed.

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
        .update(cx, |_, window, cx| {
            crate::views::ask::open(services, window, cx)
        })
        .map_err(|e| e.to_string())?;
    Ok(handle)
}

/// A query that matches nothing in the sample team.
const NO_MATCH: &str = "没有这个词的话题";
/// A query whose message search the scenes hold or fail.
const MESSAGES: &str = "登录";
/// A sentence that names someone who is not a member.
const NOBODY: &str = "让 Nadia 拉一下 10 月各渠道的新增";

fn open_switcher_state(
    services: Services,
    content: Size<Pixels>,
    query: &'static str,
    error: Option<Option<crate::data::SearchError>>,
    cx: &mut App,
) -> Result<AnyWindowHandle, String> {
    let handle = open_main(services.clone(), content, cx)?;
    handle
        .update(cx, |_, window, cx| {
            let view = crate::views::nav::switcher::open_with_view(services, query, window, cx);
            if let Some(error) = error {
                view.update(cx, |s, cx| s.hold_search(error, cx));
            }
        })
        .map_err(|e| e.to_string())?;
    Ok(handle)
}

fn open_ask_nobody(
    services: Services,
    content: Size<Pixels>,
    cx: &mut App,
) -> Result<AnyWindowHandle, String> {
    let handle = open_main(services.clone(), content, cx)?;
    handle
        .update(cx, |_, window, cx| {
            crate::views::ask::open_with(services, NOBODY, Some(d::GROWTH.into()), window, cx);
        })
        .map_err(|e| e.to_string())?;
    Ok(handle)
}

pub const SCENES: &[Scene] = &[
    Scene {
        name: "switcher-no-match",
        about: "The quick switcher with a query that finds nothing: 没有找到 with 清空搜索",
        start: Start::SignedIn,
        setup: crate::selftest::scenes::open_first_topic,
        open: |s, c, cx| open_switcher_state(s, c, NO_MATCH, None, cx),
    },
    Scene {
        name: "switcher-loading",
        about: "The quick switcher while the message search is out: 3 skeleton rows under 消息",
        start: Start::SignedIn,
        setup: crate::selftest::scenes::open_first_topic,
        open: |s, c, cx| open_switcher_state(s, c, MESSAGES, Some(None), cx),
    },
    Scene {
        name: "switcher-failed",
        about: "The quick switcher after the message search failed: an Alert row with 重试",
        start: Start::SignedIn,
        setup: crate::selftest::scenes::open_first_topic,
        open: |s, c, cx| {
            open_switcher_state(
                s,
                c,
                MESSAGES,
                Some(Some(crate::data::SearchError::TimedOut)),
                cx,
            )
        },
    },
    Scene {
        name: "ask-nobody",
        about: "⌘K 交代或搜索 naming someone who is not a member: 没有叫 … 的成员 above 交给 Jojo 安排",
        start: Start::V2,
        setup: |s, cx| s.nav.update(cx, |nav, cx| nav.open_desk(cx)),
        open: open_ask_nobody,
    },
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
