//! Message search lives in the one switcher (DESIGN.md §7.12, U35): Cmd+F
//! (on a page with no 话题 open; in a 话题 it finds in the transcript,
//! `timeline::find`) opens it scoped to the current 群 with a removable
//! `在 orderbot 研发` token,
//! Cmd+Shift+F opens it over all of mine. The switcher asks the server
//! through [`crate::data::ChatProvider::search`] and Enter on a hit opens
//! the topic at that message (`Navigation::open_message`).

pub mod plain;
pub mod preview;

use gpui::{App, KeyBinding};
use gpui_component::WindowExt as _;

use crate::{
    app::{Place, Services},
    data::GroupId,
};

gpui::actions!(search, [SearchInGroup, SearchEverywhere]);

/// Key bindings and the two global actions. Call once after `nav::install`.
pub fn install(cx: &mut App) {
    // ⌘F finds in the open 话题 and falls back to SearchInGroup elsewhere
    // (`timeline::find`).
    cx.bind_keys([KeyBinding::new("cmd-shift-f", SearchEverywhere, None)]);
    crate::views::timeline::find::install(cx);
    cx.on_action(|_: &SearchInGroup, cx| open_deferred(false, cx));
    cx.on_action(|_: &SearchEverywhere, cx| open_deferred(true, cx));
}

fn open_deferred(everywhere: bool, cx: &mut App) {
    super::nav::in_main_window(cx, move |window, cx| {
        if !window.has_active_dialog(cx) {
            open(super::nav::services_of(cx), everywhere, "", window, cx);
        }
    });
}

/// The group Cmd+F searches: the open topic's group or the selected group.
pub fn current_group(services: &Services, cx: &App) -> Option<GroupId> {
    let nav = services.nav.read(cx);
    match (&nav.topic, &nav.place) {
        (Some((g, _)), _) => Some(g.clone()),
        (None, Some(Place::Group(g))) => Some(g.clone()),
        _ => None,
    }
}

/// Opens the switcher with `query` typed: scoped to the current 群 unless
/// `everywhere` (or no 群 is open).
pub fn open(
    services: Services,
    everywhere: bool,
    query: &str,
    window: &mut gpui::Window,
    cx: &mut App,
) {
    let scope = if everywhere {
        None
    } else {
        current_group(&services, cx)
    };
    super::nav::switcher::open_scoped(services, scope, query, window, cx);
}
