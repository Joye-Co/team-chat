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

/// Opens the long topic at a message 5,000 messages up, as Enter on a hit does.
pub fn open_far_hit(services: &Services, cx: &mut App) {
    let window = services.provider.messages(
        fake::LONG_TOPIC_GROUP,
        fake::LONG_TOPIC,
        fake::LONG_TOPIC_MESSAGES,
    );
    let Some(target) = window
        .messages
        .get(window.messages.len().saturating_sub(5_000))
    else {
        return;
    };
    let (id, seq) = (target.id.clone(), target.created_seq);
    services.nav.update(cx, |nav, cx| {
        nav.open_message(
            fake::LONG_TOPIC_GROUP.into(),
            fake::LONG_TOPIC.into(),
            id,
            seq,
            cx,
        )
    });
}
