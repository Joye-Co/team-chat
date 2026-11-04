//! Screenshot scenes of the process view (`selftest/scenes.rs`).
//!
//! v12/fx-overlays: the side panel's 过程 tab is gone. A run's steps show in
//! 实时查看 (the right pane) when the run has no screen, so the `process*`
//! scenes open that pane on the run; the `panel-*` scenes open the topic
//! inspector, which took the side panel's 详情 page.

use gpui::App;

use crate::app::{RightPane, Services};

/// The sample topic where a bot member is at work.
pub const GROUP: &str = "g-dev";
pub const TOPIC: &str = "t-login";
/// Its running run, and the run of the topic that failed.
pub const RUN: &str = "run-login-1";
pub const FAILED_TOPIC: &str = "t-blocked";
pub const FAILED_RUN: &str = "run-blocked-1";

/// Opens `topic` with 实时查看 on `run`: its steps.
pub fn open_run(services: &Services, topic: &str, run: &str, cx: &mut App) {
    services.nav.update(cx, |nav, cx| {
        nav.open_group(GROUP.into(), cx);
        nav.open_topic(GROUP.into(), topic.into(), cx);
        nav.set_right_pane(RightPane::Live(run.into()), cx);
    });
}

/// Opens the topic with a running bot run and its steps.
pub fn open_running(services: &Services, cx: &mut App) {
    open_run(services, TOPIC, RUN, cx);
}

/// Like [`open_running`], with the developer view on: ids and raw events.
pub fn open_running_developer(services: &Services, cx: &mut App) {
    open_running(services, cx);
    services
        .nav
        .update(cx, |nav, cx| nav.set_developer_view(true, cx));
}

/// The topic whose bot run failed, with its steps.
pub fn open_failed(services: &Services, cx: &mut App) {
    open_run(services, FAILED_TOPIC, FAILED_RUN, cx);
}

/// The inspector of the topic where a bot member works.
pub fn open_running_details(services: &Services, cx: &mut App) {
    open_details(services, TOPIC, cx);
}

/// The inspector of the topic whose bot run failed.
pub fn open_failed_details(services: &Services, cx: &mut App) {
    open_details(services, FAILED_TOPIC, cx);
}

/// The inspector of the sample-card topic: a plan from a card.
pub fn open_plan_details(services: &Services, cx: &mut App) {
    open_details(services, crate::views::cards::samples::TOPIC, cx);
}

fn open_details(services: &Services, topic: &str, cx: &mut App) {
    services.nav.update(cx, |nav, cx| {
        nav.open_group(GROUP.into(), cx);
        nav.open_topic(GROUP.into(), topic.into(), cx);
        nav.set_right_pane(RightPane::TopicRail, cx);
    });
}
