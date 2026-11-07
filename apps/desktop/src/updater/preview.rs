//! Screenshot scenes of the update (`desktop --screenshot <dir> --scenes update,...`).
//! The download shows in Settings › 通用, so `update`, `update-dl` and
//! `update-manual` open that pane; `update-ready` is the 重启安装 sheet over the main
//! window. The scene's window gets an [`Updater`] with a fixed phase and no client; the
//! global is removed right after the window opens so later scenes do not see it.

use gpui::{AnyWindowHandle, App, AppContext as _, Pixels, Size};

use super::{Phase, Updater, UpdaterGlobal};
use crate::{app::Services, selftest::scenes::open_main};
use chat_core::proto::UpdateInfo;

/// One patch past the running version, so the scene never offers a
/// downgrade (R28: it offered 0.2.0 while Settings showed 1.0.1).
fn next_version() -> String {
    let current = env!("CARGO_PKG_VERSION");
    let mut parts: Vec<u64> = current
        .split(['.', '-'])
        .take(3)
        .map(|p| p.parse().unwrap_or(0))
        .collect();
    parts.resize(3, 0);
    parts[2] += 1;
    format!("{}.{}.{}", parts[0], parts[1], parts[2])
}

fn sample() -> UpdateInfo {
    UpdateInfo {
        version: next_version(),
        notes: crate::copy::update::PREVIEW_NOTES.to_string(),
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
        download: None,
        demo: false,
    });
    cx.set_global(UpdaterGlobal(updater));
}

/// `desktop --fake` with `UPDATE_PREVIEW=available|downloading|ready|failed|manual|whats-new`:
/// an updater without a server in that state, so the sidebar card, the notes and
/// 已更新到 can be tried by hand. 更新 plays a short download, 重启安装 a short install.
pub fn install_for_fake(cx: &mut App) {
    let Ok(want) = std::env::var("UPDATE_PREVIEW") else {
        return;
    };
    let mut info = sample();
    info.notes = sample_notes(&info.version);
    info.published_at = chrono::Utc::now().timestamp_millis() as u64;
    let phase = match want.as_str() {
        "downloading" => Phase::Downloading {
            done: info.size * 42 / 100,
            total: info.size,
            info,
        },
        "ready" => Phase::Ready {
            info,
            dmg: std::env::temp_dir().join("update-preview.dmg"),
        },
        "failed" => Phase::Failed {
            info: Some(info),
            failure: super::Failure::Download,
        },
        "manual" => Phase::Manual(info),
        "whats-new" => {
            WHATS_NEW_DEMO.store(true, std::sync::atomic::Ordering::SeqCst);
            Phase::UpToDate
        }
        _ => Phase::Available(info),
    };
    let updater = cx.new(|_| Updater {
        source: std::sync::Arc::new(|| None),
        data_root: std::env::temp_dir(),
        phase,
        found: None,
        last_attempt: Some((std::time::Instant::now(), true)),
        postponed: None,
        download: None,
        demo: true,
    });
    cx.set_global(UpdaterGlobal(updater));
}

/// `UPDATE_PREVIEW=whats-new`: 已更新到 shows on this start.
static WHATS_NEW_DEMO: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

pub fn whats_new_demo() -> bool {
    WHATS_NEW_DEMO.load(std::sync::atomic::Ordering::SeqCst)
}

/// The demo download: about 4 s from 0 to 100 %, then 已经下载好了.
pub(super) fn play_download(info: UpdateInfo, cx: &mut gpui::Context<Updater>) {
    let total = info.size;
    cx.spawn(async move |this, cx| {
        for step in 0..=40u64 {
            let info = info.clone();
            let alive = this
                .update(cx, |u, cx| {
                    let next = if step == 40 {
                        Phase::Ready {
                            info,
                            dmg: std::env::temp_dir().join("update-preview.dmg"),
                        }
                    } else {
                        Phase::Downloading {
                            info,
                            done: total * step / 40,
                            total,
                        }
                    };
                    // 取消下载 stops the demo.
                    let going = step == 0 || matches!(u.phase, Phase::Downloading { .. });
                    if going {
                        u.set_phase(next, cx);
                    }
                    going
                })
                .unwrap_or(false);
            if !alive {
                break;
            }
            cx.background_executor()
                .timer(std::time::Duration::from_millis(100))
                .await;
        }
    })
    .detach();
}

/// The demo install: 正在安装 for 2 s, then 已是最新版本 (no relaunch).
pub(super) fn play_install(u: &mut Updater, info: UpdateInfo, cx: &mut gpui::Context<Updater>) {
    u.set_phase(Phase::Installing(info), cx);
    cx.spawn(async move |this, cx| {
        cx.background_executor()
            .timer(std::time::Duration::from_secs(2))
            .await;
        let _ = this.update(cx, |u, cx| u.set_phase(Phase::UpToDate, cx));
    })
    .detach();
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

/// Settings › 通用 with 已是最新版本, scrolled to its end: the 开发者
/// section (the `settings-about-dev` scene).
pub fn open_up_to_date_end(
    services: Services,
    _content: Size<Pixels>,
    cx: &mut App,
) -> Result<AnyWindowHandle, String> {
    set_phase(Phase::UpToDate, cx);
    crate::views::settings::prefs::stop_saving();
    let result = crate::views::settings::selftest::open_end(
        services,
        crate::views::settings::Page::General,
        cx,
    );
    cx.remove_global::<UpdaterGlobal>();
    result
}

/// Settings › 通用 at rest: the updater is idle, so the version row has
/// its 检查更新 button, as in the app (review 4 K-15).
pub fn open_idle(
    services: Services,
    _content: Size<Pixels>,
    cx: &mut App,
) -> Result<AnyWindowHandle, String> {
    open_settings_with(Phase::Idle, services, cx)
}

/// Settings › 通用 after 检查更新: 已是最新版本.
pub fn open_up_to_date(
    services: Services,
    _content: Size<Pixels>,
    cx: &mut App,
) -> Result<AnyWindowHandle, String> {
    open_settings_with(Phase::UpToDate, services, cx)
}

// --- product updates: the sidebar card and the notes sheets ---

/// The main window (no topic open) with the sidebar card in `phase`.
fn open_card(
    phase: Phase,
    services: Services,
    content: Size<Pixels>,
    cx: &mut App,
) -> Result<AnyWindowHandle, String> {
    open_with(phase, services, content, cx)
}

pub fn open_card_available(
    services: Services,
    content: Size<Pixels>,
    cx: &mut App,
) -> Result<AnyWindowHandle, String> {
    open_card(Phase::Available(sample()), services, content, cx)
}

pub fn open_card_downloading(
    services: Services,
    content: Size<Pixels>,
    cx: &mut App,
) -> Result<AnyWindowHandle, String> {
    let info = sample();
    let total = info.size;
    open_card(
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

pub fn open_card_failed(
    services: Services,
    content: Size<Pixels>,
    cx: &mut App,
) -> Result<AnyWindowHandle, String> {
    open_card(
        Phase::Failed {
            info: Some(sample()),
            failure: super::Failure::Download,
        },
        services,
        content,
        cx,
    )
}

pub fn open_card_manual(
    services: Services,
    content: Size<Pixels>,
    cx: &mut App,
) -> Result<AnyWindowHandle, String> {
    open_card(Phase::Manual(sample()), services, content, cx)
}

/// 「{version} 更新内容」 over the main window, with 更新到 ….
pub fn open_notes_sheet(
    services: Services,
    content: Size<Pixels>,
    cx: &mut App,
) -> Result<AnyWindowHandle, String> {
    let mut info = sample();
    info.notes = sample_notes(&info.version);
    info.published_at = 1_794_000_000_000;
    set_phase(Phase::Available(info.clone()), cx);
    let handle = open_main(services, content, cx)?;
    handle
        .update(cx, |_, window, cx| {
            super::notes::open_notes(info, super::notes::Place::Main, window, cx)
        })
        .map_err(|e| e.to_string())?;
    // The sheet reads the updater when it draws: the global stays for this
    // scene (the next scene sets its own).
    Ok(handle)
}

/// 「已更新到 {version}」 over the main window, from the bundled notes.
pub fn open_whats_new(
    services: Services,
    content: Size<Pixels>,
    cx: &mut App,
) -> Result<AnyWindowHandle, String> {
    let handle = open_main(services, content, cx)?;
    let notes = super::notes::bundled(super::version::CURRENT)
        .unwrap_or_else(|| super::notes::Notes::parse("", crate::copy::update::PREVIEW_NOTES));
    handle
        .update(cx, |_, window, cx| {
            super::notes::open_whats_new(notes, super::notes::Place::Main, window, cx)
        })
        .map_err(|e| e.to_string())?;
    Ok(handle)
}

/// 更新记录 over Settings › 通用.
pub fn open_changelog(
    services: Services,
    _content: Size<Pixels>,
    cx: &mut App,
) -> Result<AnyWindowHandle, String> {
    let handle = open_settings_with(Phase::UpToDate, services, cx)?;
    handle
        .update(cx, |_, window, cx| {
            super::notes::open_changelog(super::notes::Place::Settings, window, cx)
        })
        .map_err(|e| e.to_string())?;
    Ok(handle)
}

/// Notes in the release-notes Markdown, from the newest bundled version
/// (the sample has to look like a real one).
fn sample_notes(version: &str) -> String {
    super::notes::RELEASES
        .first()
        .map(|(v, text)| text.replace(v, version))
        .unwrap_or_else(|| crate::copy::update::PREVIEW_NOTES.to_string())
}

#[cfg(test)]
mod tests {
    #[test]
    fn the_preview_offers_a_newer_version() {
        let next = super::next_version();
        assert!(super::super::version::is_newer(
            &next,
            env!("CARGO_PKG_VERSION")
        ));
    }
}
