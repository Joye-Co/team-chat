//! Screenshot scenes of the update (`desktop --screenshot <dir> --scenes update,...`).
//! The download shows in Settings › 通用, so `update`, `update-dl` and
//! `update-manual` open that pane; `update-ready` is the 重启安装 sheet over the main
//! window. The scene's window gets an [`Updater`] with a fixed phase and no client; the
//! global is removed right after the window opens so later scenes do not see it.

use gpui::{AnyWindowHandle, App, AppContext as _, Pixels, Size};

use super::{Phase, Updater, UpdaterGlobal};
use crate::{app::Services, selftest::scenes::open_main};
use chat_core::proto::UpdateInfo;

fn sample() -> UpdateInfo {
    UpdateInfo {
        version: "0.2.0".into(),
        notes: String::new(),
        size: 48_000_000,
        sha256: "0".repeat(64),
        published_at: 0,
        download_url: String::new(),
    }
}

fn open_with(
    phase: Phase,
    services: Services,
    content: Size<Pixels>,
    cx: &mut App,
) -> Result<AnyWindowHandle, String> {
    set_phase(phase, cx);
    let result = open_main(services, content, cx);
    cx.remove_global::<UpdaterGlobal>();
    result
}

/// Settings › 通用 with the updater in `phase` (its version row).
pub fn open_settings_with(
    phase: Phase,
    services: Services,
    cx: &mut App,
) -> Result<AnyWindowHandle, String> {
    set_phase(phase, cx);
    crate::views::settings::prefs::stop_saving();
    let result = crate::views::settings::open_window(
        services,
        crate::views::settings::Page::General,
        false,
        cx,
    )
    .map(|(h, _)| h);
    cx.remove_global::<UpdaterGlobal>();
    result
}

fn set_phase(phase: Phase, cx: &mut App) {
    let updater = cx.new(|_| Updater {
        source: std::sync::Arc::new(|| None),
        data_root: std::env::temp_dir(),
        phase,
        found: None,
        last_attempt: None,
        postponed: None,
    });
    cx.set_global(UpdaterGlobal(updater));
}

pub fn open_available(
    services: Services,
    _content: Size<Pixels>,
    cx: &mut App,
) -> Result<AnyWindowHandle, String> {
    open_settings_with(Phase::Available(sample()), services, cx)
}

/// The 重启安装 sheet over the main window.
pub fn open_ready(
    services: Services,
    content: Size<Pixels>,
    cx: &mut App,
) -> Result<AnyWindowHandle, String> {
    let dmg = std::env::temp_dir().join("update-preview.dmg");
    open_with(
        Phase::Ready {
            info: sample(),
            dmg,
        },
        services,
        content,
        cx,
    )
}

pub fn open_downloading(
    services: Services,
    _content: Size<Pixels>,
    cx: &mut App,
) -> Result<AnyWindowHandle, String> {
    let info = sample();
    let total = info.size;
    open_settings_with(
        Phase::Downloading {
            info,
            done: total * 42 / 100,
            total,
        },
        services,
        cx,
    )
}

pub fn open_manual(
    services: Services,
    _content: Size<Pixels>,
    cx: &mut App,
) -> Result<AnyWindowHandle, String> {
    open_settings_with(Phase::Manual(sample()), services, cx)
}

/// Settings › 通用 after 检查更新: 已是最新版本.
pub fn open_up_to_date(
    services: Services,
    _content: Size<Pixels>,
    cx: &mut App,
) -> Result<AnyWindowHandle, String> {
    open_settings_with(Phase::UpToDate, services, cx)
}
