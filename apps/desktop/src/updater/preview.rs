//! Screenshot scenes of the update banner (`desktop --screenshot <dir> --scenes update,...`).
//! The scene's window gets an [`Updater`] with a fixed phase and no client; the global is
//! removed right after the window opens so later scenes do not see it.

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
    let updater = cx.new(|_| Updater {
        source: std::sync::Arc::new(|| None),
        data_root: std::env::temp_dir(),
        phase,
        found: None,
        last_attempt: None,
        postponed: None,
    });
    cx.set_global(UpdaterGlobal(updater));
    let result = open_main(services, content, cx);
    cx.remove_global::<UpdaterGlobal>();
    result
}

pub fn open_available(
    services: Services,
    content: Size<Pixels>,
    cx: &mut App,
) -> Result<AnyWindowHandle, String> {
    open_with(Phase::Available(sample()), services, content, cx)
}

pub fn open_downloading(
    services: Services,
    content: Size<Pixels>,
    cx: &mut App,
) -> Result<AnyWindowHandle, String> {
    let info = sample();
    let total = info.size;
    open_with(
        Phase::Downloading {
            info,
            done: total * 42 / 100,
            total,
        },
        services,
        content,
        cx,
    )
}

pub fn open_manual(
    services: Services,
    content: Size<Pixels>,
    cx: &mut App,
) -> Result<AnyWindowHandle, String> {
    open_with(Phase::Manual(sample()), services, content, cx)
}
