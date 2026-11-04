//! Screenshot scenes of search (`selftest/scenes.rs`): the switcher scoped
//! to the current 群 by Cmd+F with a query and its results, and a topic
//! opened at a hit far up in its history.

use gpui::{AnyWindowHandle, App, Pixels, Size};

use crate::{app::Services, fake, selftest::scenes::open_main};

/// The query the `search` scenes type.
pub const QUERY: &str = "登录";

/// Opens the main window with the Cmd+F switcher showing results for [`QUERY`].
pub fn open_dialog(
    services: Services,
    content: Size<Pixels>,
    cx: &mut App,
) -> Result<AnyWindowHandle, String> {
    let handle = open_main(services.clone(), content, cx)?;
    handle
        .update(cx, |_, window, cx| {
            super::open(services, false, QUERY, window, cx)
        })
        .map_err(|e| e.to_string())?;
    Ok(handle)
}

/// Opens the long topic at a message 5,000 messages up, as Enter on a hit
/// does: the second of seven hits of [`FAR_QUERY`] in that 话题.
pub fn open_far_hit(services: &Services, cx: &mut App) {
    let window = services.provider.messages(
        fake::LONG_TOPIC_GROUP,
        fake::LONG_TOPIC,
        fake::LONG_TOPIC_MESSAGES,
    );
    let len = window.messages.len();
    // Seven hits of the query around the message 5,000 up, newest first;
    // the second newest is the one opened.
    let found: Vec<usize> = (0..len)
        .filter(|&i| window.messages[i].body.contains(FAR_QUERY))
        .collect();
    let Some(at) = found.iter().position(|&i| i + 5_000 >= len) else {
        return;
    };
    let hits: Vec<(String, u64)> = found[at.saturating_sub(5)..(at + 2).min(found.len())]
        .iter()
        .rev()
        .map(|&i| {
            (
                window.messages[i].id.clone(),
                window.messages[i].created_seq,
            )
        })
        .collect();
    if hits.len() < 2 {
        return;
    }
    let search = crate::app::SearchContext {
        query: FAR_QUERY.into(),
        hits,
        index: 1,
    };
    services.nav.update(cx, |nav, cx| {
        nav.open_search_hit(
            fake::LONG_TOPIC_GROUP.into(),
            fake::LONG_TOPIC.into(),
            search,
            cx,
        )
    });
}

/// The query of the `search-hit` scene.
pub const FAR_QUERY: &str = "压测";
