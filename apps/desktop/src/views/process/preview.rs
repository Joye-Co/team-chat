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

/// The failed run after 查看日志: the developer view, at the raw events.
pub fn open_failed_log(services: &Services, cx: &mut App) {
    open_failed(services, cx);
    REVEAL_LOG.store(true, std::sync::atomic::Ordering::Relaxed);
    services
        .nav
        .update(cx, |nav, cx| nav.set_developer_view(true, cx));
}

/// The scene's stand-in for the press of 查看日志: the view opens as if pressed.
static REVEAL_LOG: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Whether a new view starts as if 查看日志 was pressed (once).
pub(super) fn take_reveal_log() -> bool {
    REVEAL_LOG.swap(false, std::sync::atomic::Ordering::Relaxed)
}

/// The inspector of the topic where a bot member works.
pub fn open_running_details(services: &Services, cx: &mut App) {
    open_details(services, TOPIC, cx);
}

/// The inspector of the topic whose bot run failed.
pub fn open_failed_details(services: &Services, cx: &mut App) {
    open_details(services, FAILED_TOPIC, cx);
}

/// The inspector of a 话题 whose run carries a plan (the v2 sample team: Forge on
/// 话题列表虚拟滚动), so the 计划 section shows: progress, steps and their times.
pub fn open_plan_details(services: &Services, cx: &mut App) {
    use crate::fake::v2 as d;
    crate::selftest::scenes::v2_open_topic(services, d::DEV, d::T_VIRTUAL, cx);
    services
        .nav
        .update(cx, |nav, cx| nav.set_right_pane(RightPane::TopicRail, cx));
}

fn open_details(services: &Services, topic: &str, cx: &mut App) {
    services.nav.update(cx, |nav, cx| {
        nav.open_group(GROUP.into(), cx);
        nav.open_topic(GROUP.into(), topic.into(), cx);
        nav.set_right_pane(RightPane::TopicRail, cx);
    });
}
