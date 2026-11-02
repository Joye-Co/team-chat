//! Screenshot scenes of the process view (`selftest/scenes.rs`).

use gpui::App;

use crate::app::Services;

/// The sample topic where a bot member is at work.
pub const GROUP: &str = "g-dev";
pub const TOPIC: &str = "t-login";

/// Opens the topic with a running bot run and the 过程 tab.
pub fn open_running(services: &Services, cx: &mut App) {
    services.nav.update(cx, |nav, cx| {
        nav.open_group(GROUP.into(), cx);
        nav.open_topic(GROUP.into(), TOPIC.into(), cx);
        nav.set_side_panel_tab(1, cx);
        nav.set_side_panel_open(true, cx);
    });
}

/// Like [`open_running`], with the developer view on.
pub fn open_running_developer(services: &Services, cx: &mut App) {
    open_running(services, cx);
    services
        .nav
        .update(cx, |nav, cx| nav.set_developer_view(true, cx));
}

/// The topic whose bot run failed.
pub fn open_failed(services: &Services, cx: &mut App) {
    services.nav.update(cx, |nav, cx| {
        nav.open_group(GROUP.into(), cx);
        nav.open_topic(GROUP.into(), "t-blocked".into(), cx);
        nav.set_side_panel_tab(1, cx);
        nav.set_side_panel_open(true, cx);
    });
}

/// The 详情 page of the topic where a bot member works: its state, its
/// latest steps and `查看过程`.
pub fn open_running_details(services: &Services, cx: &mut App) {
    open_details(services, TOPIC, cx);
}

/// The 详情 page of the topic whose bot run failed.
pub fn open_failed_details(services: &Services, cx: &mut App) {
    open_details(services, "t-blocked", cx);
}

/// The 详情 page of the sample-card topic: a plan list from a card.
pub fn open_plan_details(services: &Services, cx: &mut App) {
    open_details(services, crate::views::cards::samples::TOPIC, cx);
}

fn open_details(services: &Services, topic: &str, cx: &mut App) {
    services.nav.update(cx, |nav, cx| {
        nav.open_group(GROUP.into(), cx);
        nav.open_topic(GROUP.into(), topic.into(), cx);
        nav.set_side_panel_tab(0, cx);
        nav.set_side_panel_open(true, cx);
    });
}
