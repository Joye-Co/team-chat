//! Screenshot scenes of 实时查看 (the live track owns this slice).

use gpui::App;

use crate::{
    app::{RightPane, Services},
    fake::v2 as d,
    selftest::scenes::{Scene, Start, open_main, v2_open_topic},
};

fn live(services: &Services, cx: &mut App) {
    v2_open_topic(services, d::DEV, d::T_VIRTUAL, cx);
    services.nav.update(cx, |nav, cx| {
        nav.set_right_pane(RightPane::Live(d::RUN_VIRTUAL.into()), cx)
    });
}

// --- v12/fx-overlays: the stop scenes ---

/// The main window on the running sample run's steps, with its own
/// provider: `hold` keeps a stop unanswered (`正在停止…`).
fn open_stop_with(
    hold: bool,
    content: gpui::Size<gpui::Pixels>,
    cx: &mut App,
) -> Result<(gpui::AnyWindowHandle, Services), String> {
    use crate::views::process::preview as p;
    let fake = crate::fake::FakeProvider::new();
    if hold {
        fake.hold_stop(p::RUN);
    }
    // With `--worst-case` the stop alert and 正在停止… read worst-case names.
    let provider: std::sync::Arc<dyn crate::data::ChatProvider> = if crate::selftest::worst_case() {
        crate::selftest::worst_case::WorstCase::new(fake)
    } else {
        fake
    };
    let services = Services::new(provider, cx);
    p::open_running(&services, cx);
    let handle = open_main(services.clone(), content, cx)?;
    Ok((handle, services))
}

/// 停止 pressed: the alert that asks first.
pub fn open_stop(
    _services: Services,
    content: gpui::Size<gpui::Pixels>,
    cx: &mut App,
) -> Result<gpui::AnyWindowHandle, String> {
    use crate::views::process::preview as p;
    let (handle, services) = open_stop_with(false, content, cx)?;
    let run = services
        .provider
        .runs(p::GROUP, p::TOPIC)
        .into_iter()
        .find(|r| r.id == p::RUN)
        .ok_or("no sample run")?;
    handle
        .update(cx, |_, window, cx| {
            crate::views::process::confirm_stop(&services, &run, window, cx)
        })
        .map_err(|e| e.to_string())?;
    Ok(handle)
}

/// After 停止: `正在停止…` until the run ends.
pub fn open_stopping(
    _services: Services,
    content: gpui::Size<gpui::Pixels>,
    cx: &mut App,
) -> Result<gpui::AnyWindowHandle, String> {
    open_stop_with(true, content, cx).map(|(h, _)| h)
}

// --- end v12/fx-overlays ---

pub const SCENES: &[Scene] = &[
    Scene {
        name: "live",
        about: "实时查看 (Live.dc.html): Forge's computer on 话题列表虚拟滚动, 3/5",
        start: Start::V2,
        setup: live,
        open: open_main,
    },
    Scene {
        name: "live-takeover",
        about: "实时查看 after 接管: Forge is paused, 交还 is the primary action",
        start: Start::V2,
        setup: |s, cx| {
            live(s, cx);
            drop(
                s.provider
                    .take_over_run(d::DEV, d::T_VIRTUAL, d::RUN_VIRTUAL),
            );
        },
        open: open_main,
    },
];
