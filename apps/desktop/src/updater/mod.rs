//! In-app update.
//!
//! - [`Updater`] is an app-wide entity (a GPUI global, see [`install`]). It checks
//!   `GET /v1/updates/latest` through chat-core when the app starts (as soon as a
//!   session exists) and every 6 hours, downloads the DMG with progress (chat-core checks
//!   size and SHA-256) and stops at [`Phase::Ready`]: the user chooses 重启安装 (a
//!   sheet, Settings › 通用 or the app menu). Then it waits until no send is
//!   pending, installs on a background thread ([`install::install`]) and relaunches.
//!   The progress shows in Settings › 通用 (the version row) and in the app menu.
//! - When the running bundle cannot be replaced (not writable, or no bundle under
//!   `cargo run`), it opens the DMG in Finder and the banner explains what to do.
//! - [`banner::UpdateBanner`] (main window; the settings 通用 pane draws its own version row)
//!   renders the state. It reads only this entity; the network and the cache stay here.
//! - On start, [`install::finish_on_launch`] removes the old bundle of the previous update.

pub mod banner;
pub mod install;
pub mod notes;
pub mod preview;
pub mod selftest;
pub mod version;

use std::{
    path::PathBuf,
    sync::Arc,
    time::{Duration, Instant},
};

use futures::{StreamExt as _, channel::mpsc};
use gpui::{App, AppContext as _, Context, Entity, Global};

use chat_core::{Client, proto::UpdateInfo, store::OutboxState};

/// Time between two background checks.
pub const CHECK_EVERY: Duration = Duration::from_secs(6 * 3600);
/// Time before a failed background check runs again.
const RETRY_FAILED_CHECK: Duration = Duration::from_secs(30 * 60);
/// How often the scheduler looks whether a check is due (also catches sign-in).
const TICK: Duration = Duration::from_secs(20);
/// "稍后" hides the banner for the same version this long.
const POSTPONE_FOR: Duration = Duration::from_secs(24 * 3600);
/// How often a waiting install looks at the outbox again.
const SEND_POLL: Duration = Duration::from_secs(2);

/// Gives the chat-core client of the signed-in account, or `None` when signed out.
pub type ClientSource = Arc<dyn Fn() -> Option<Arc<Client>> + Send + Sync>;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Failure {
    Check,
    Download,
    Checksum,
    Install,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Phase {
    Idle,
    Checking,
    UpToDate,
    Available(UpdateInfo),
    Downloading {
        info: UpdateInfo,
        done: u64,
        total: u64,
    },
    /// Downloaded and verified: waits for the user's 重启安装.
    Ready {
        info: UpdateInfo,
        dmg: PathBuf,
    },
    /// Downloaded; waits for pending sends before it installs.
    WaitingForSends(UpdateInfo),
    Installing(UpdateInfo),
    /// The bundle cannot be replaced: the DMG is open in Finder.
    Manual(UpdateInfo),
    Failed {
        info: Option<UpdateInfo>,
        failure: Failure,
    },
}

impl Phase {
    /// The update this phase is about, if any.
    pub fn info(&self) -> Option<&UpdateInfo> {
        match self {
            Phase::Available(i)
            | Phase::Downloading { info: i, .. }
            | Phase::Ready { info: i, .. }
            | Phase::WaitingForSends(i)
            | Phase::Installing(i)
            | Phase::Manual(i) => Some(i),
            Phase::Failed { info, .. } => info.as_ref(),
            _ => None,
        }
    }

    fn busy(&self) -> bool {
        matches!(
            self,
            Phase::Checking
                | Phase::Downloading { .. }
                | Phase::WaitingForSends(_)
                | Phase::Installing(_)
        )
    }
}

pub struct Updater {
    source: ClientSource,
    data_root: PathBuf,
    phase: Phase,
    /// The last update found (kept when a later check fails).
    found: Option<UpdateInfo>,
    last_attempt: Option<(Instant, bool)>,
    postponed: Option<(String, Instant)>,
    /// The running download, so 取消下载 can stop it.
    download: Option<tokio::task::AbortHandle>,
}

struct UpdaterGlobal(Entity<Updater>);

impl Global for UpdaterGlobal {}

/// The updater entity, when the app runs on chat-core.
pub fn global(cx: &App) -> Option<Entity<Updater>> {
    cx.try_global::<UpdaterGlobal>().map(|g| g.0.clone())
}

/// Folder for downloaded DMGs and the update marker.
fn updates_dir(data_root: &std::path::Path) -> PathBuf {
    data_root.join("updates")
}

fn marker_path(data_root: &std::path::Path) -> PathBuf {
    updates_dir(data_root).join("pending.json")
}

/// The running `.app` bundle, if the app runs from one.
pub fn running_bundle() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let exe = std::fs::canonicalize(&exe).unwrap_or(exe);
    install::bundle_from_exe(&exe)
}

/// Creates the updater, finishes a previous update and starts the schedule. Call once
/// at start on the chat-core provider.
/// The app menu's update item for `phase`: its title and whether it can be
/// chosen. The download shows its progress here (NATIVE.md §7.4: no
/// sheet or banner while it runs).
pub fn menu_item(phase: &Phase) -> (String, bool) {
    use crate::copy::{fill, overlays as o};
    match phase {
        Phase::Checking => (crate::copy::update::CHECKING.to_string(), false),
        Phase::Available(i) | Phase::Failed { info: Some(i), .. } => {
            (fill(o::MENU_UPDATE_TO, &[("version", &i.version)]), true)
        }
        Phase::Downloading { info, done, total } => {
            let total = if *total == 0 { info.size } else { *total };
            let pct = version::percent(*done, total).to_string();
            (
                fill(
                    o::MENU_DOWNLOADING,
                    &[("version", &info.version), ("percent", &pct)],
                ),
                false,
            )
        }
        Phase::Ready { info, .. } => (fill(o::MENU_RESTART, &[("version", &info.version)]), true),
        Phase::WaitingForSends(i) | Phase::Installing(i) => (
            fill(o::UPDATE_INSTALLING_TITLE, &[("version", &i.version)]),
            false,
        ),
        Phase::Manual(i) => (
            fill(o::UPDATE_MANUAL_TITLE, &[("version", &i.version)]),
            true,
        ),
        Phase::Idle | Phase::UpToDate | Phase::Failed { info: None, .. } => {
            (o::MENU_CHECK.to_string(), true)
        }
    }
}

gpui::actions!(updater, [UpdateCommand]);

/// The app menu's update item: installs a downloaded update, starts an
/// available one, or opens Settings › 通用 and checks.
fn on_update_command(cx: &mut App) {
    let Some(updater) = global(cx) else { return };
    let phase = updater.read(cx).phase().clone();
    match phase {
        Phase::Ready { .. } => updater.update(cx, |u, cx| u.install_now(cx)),
        Phase::Available(_) | Phase::Failed { info: Some(_), .. } => {
            updater.update(cx, |u, cx| u.start_update(cx))
        }
        _ => {
            updater.update(cx, |u, cx| {
                if !u.phase.busy() && !matches!(u.phase, Phase::Manual(_)) {
                    u.check_now(cx)
                }
            });
            let services = crate::views::nav::services_of(cx);
            crate::views::settings::open_page(services, crate::views::settings::Page::General, cx);
        }
    }
}

/// True when this start finishes an update the app installed itself (its
/// marker names the running version), so 已更新到 may show.
static JUST_UPDATED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

pub fn just_updated() -> bool {
    JUST_UPDATED.load(std::sync::atomic::Ordering::SeqCst)
}

pub fn install(source: ClientSource, data_root: PathBuf, cx: &mut App) -> Entity<Updater> {
    let marker = marker_path(&data_root);
    if install::marker_version(&marker).as_deref() == Some(version::CURRENT) {
        JUST_UPDATED.store(true, std::sync::atomic::Ordering::SeqCst);
    }
    cx.background_executor()
        .spawn(async move {
            let bundle = running_bundle();
            let done = install::finish_on_launch(&marker, bundle.as_deref(), version::CURRENT);
            tracing::info!(?done, "update finish on launch");
        })
        .detach();
    let updater = cx.new(|_| Updater {
        source,
        data_root,
        phase: Phase::Idle,
        found: None,
        last_attempt: None,
        postponed: None,
        download: None,
    });
    cx.set_global(UpdaterGlobal(updater.clone()));
    cx.on_action(|_: &UpdateCommand, cx| on_update_command(cx));
    // The app menu's item follows the phase.
    cx.observe(&updater, |_, cx| {
        crate::views::nav::menus::refresh_menus(cx)
    })
    .detach();
    let weak = updater.downgrade();
    cx.spawn(async move |cx| {
        loop {
            let alive = weak.update(cx, |u, cx| u.tick(cx)).is_ok();
            if !alive {
                break;
            }
            cx.background_executor().timer(TICK).await;
        }
    })
    .detach();
    updater
}

impl Updater {
    pub fn phase(&self) -> &Phase {
        &self.phase
    }

    /// True when the main window's banner shows.
    pub fn banner_visible(&self) -> bool {
        match &self.phase {
            Phase::Available(info) => !version::postponed(
                &info.version,
                self.postponed.as_ref(),
                Instant::now(),
                POSTPONE_FOR,
            ),
            Phase::Downloading { .. }
            | Phase::Ready { .. }
            | Phase::WaitingForSends(_)
            | Phase::Installing(_)
            | Phase::Manual(_) => true,
            Phase::Failed { info, failure } => info.is_some() && *failure != Failure::Check,
            _ => false,
        }
    }

    fn tick(&mut self, cx: &mut Context<Self>) {
        // A downloaded update waits for 重启安装; no check replaces it.
        if self.phase.busy() || matches!(self.phase, Phase::Ready { .. }) {
            return;
        }
        let due = match self.last_attempt {
            None => true,
            Some((at, ok)) => at.elapsed() >= if ok { CHECK_EVERY } else { RETRY_FAILED_CHECK },
        };
        if due && (self.source)().is_some() {
            self.check(false, cx);
        }
    }

    /// Checks now (the 检查更新 button).
    pub fn check_now(&mut self, cx: &mut Context<Self>) {
        if !self.phase.busy() {
            self.check(true, cx);
        }
    }

    fn check(&mut self, by_user: bool, cx: &mut Context<Self>) {
        let Some(client) = (self.source)() else {
            if by_user {
                self.set_phase(
                    Phase::Failed {
                        info: None,
                        failure: Failure::Check,
                    },
                    cx,
                );
            }
            return;
        };
        let before = std::mem::replace(&mut self.phase, Phase::Checking);
        cx.notify();
        let job = client.check_update(version::CURRENT);
        cx.spawn(async move |this, cx| {
            let result = job.await;
            let _ = this.update(cx, |u, cx| {
                let now = Instant::now();
                let phase = match result {
                    Ok(Ok(Some(info))) => {
                        u.last_attempt = Some((now, true));
                        tracing::info!(version = %info.version, "update available");
                        u.found = Some(info.clone());
                        Phase::Available(info)
                    }
                    Ok(Ok(None)) => {
                        u.last_attempt = Some((now, true));
                        u.found = None;
                        Phase::UpToDate
                    }
                    Ok(Err(e)) => {
                        u.last_attempt = Some((now, false));
                        tracing::warn!(error = %e, "update check failed");
                        u.after_failed_check(before, by_user)
                    }
                    Err(e) => {
                        u.last_attempt = Some((now, false));
                        tracing::warn!(error = %e, "update check task failed");
                        u.after_failed_check(before, by_user)
                    }
                };
                let found = matches!(phase, Phase::Available(_));
                u.set_phase(phase, cx);
                // A found update downloads in the background (unless 有新版本时
                // 自动下载 is off); its progress shows in the sidebar card,
                // Settings › 通用 and the app menu, and an alert asks for
                // 重启安装 once it is ready.
                if found && crate::views::settings::prefs::get().auto_download {
                    u.start_update(cx);
                }
            });
        })
        .detach();
    }

    /// A failed background check keeps a found update on screen.
    fn after_failed_check(&self, before: Phase, by_user: bool) -> Phase {
        match (&self.found, by_user) {
            (Some(info), false) => Phase::Available(info.clone()),
            (_, true) => Phase::Failed {
                info: None,
                failure: Failure::Check,
            },
            (None, false) => match before {
                Phase::Checking => Phase::Idle,
                other => other,
            },
        }
    }

    fn set_phase(&mut self, phase: Phase, cx: &mut Context<Self>) {
        if self.phase != phase {
            self.phase = phase;
            cx.notify();
        }
    }

    /// The app menu's update item: its title and whether it can be chosen.
    pub fn menu_item(&self) -> (String, bool) {
        menu_item(&self.phase)
    }

    /// "稍后": hides the banner for this version for a day; stops waiting for sends.
    pub fn postpone(&mut self, cx: &mut Context<Self>) {
        let Some(info) = self.phase.info().cloned().or_else(|| self.found.clone()) else {
            return;
        };
        if matches!(self.phase, Phase::Installing(_) | Phase::Downloading { .. }) {
            return;
        }
        // A downloaded update stays downloaded: 稍后 only hides the sheet.
        if matches!(self.phase, Phase::Ready { .. }) {
            self.postponed = Some((info.version.clone(), Instant::now()));
            cx.notify();
            return;
        }
        self.postponed = Some((info.version.clone(), Instant::now()));
        self.phase = Phase::Available(info);
        cx.notify();
    }

    /// "更新": downloads, verifies, waits for sends, installs and relaunches.
    pub fn start_update(&mut self, cx: &mut Context<Self>) {
        let Some(info) = self.phase.info().cloned() else {
            return;
        };
        if matches!(
            self.phase,
            Phase::Downloading { .. }
                | Phase::Ready { .. }
                | Phase::Installing(_)
                | Phase::WaitingForSends(_)
        ) {
            return;
        }
        let Some(client) = (self.source)() else {
            self.set_phase(
                Phase::Failed {
                    info: Some(info),
                    failure: Failure::Download,
                },
                cx,
            );
            return;
        };
        let dir = updates_dir(&self.data_root);
        let dest = dir.join(format!("{}-{}.dmg", env!("PRODUCT_NAME"), info.version));
        self.postponed = None;
        self.phase = Phase::Downloading {
            info: info.clone(),
            done: 0,
            total: info.size,
        };
        cx.notify();

        let (tx, mut rx) = mpsc::unbounded::<(u64, u64)>();
        let progress: chat_core::ProgressFn = Arc::new(move |p: chat_core::Progress| {
            let _ = tx.unbounded_send((p.done, p.total));
        });
        let job = client.download_update(info.clone(), &dest, progress);
        self.download = Some(job.abort_handle());
        cx.spawn(async move |this, cx| {
            while let Some((done, total)) = rx.next().await {
                let alive = this
                    .update(cx, |u, cx| {
                        if let Phase::Downloading { info, .. } = &u.phase {
                            let info = info.clone();
                            u.set_phase(Phase::Downloading { info, done, total }, cx);
                        }
                    })
                    .is_ok();
                if !alive {
                    break;
                }
            }
        })
        .detach();
        cx.spawn(async move |this, cx| {
            let result = job.await;
            let _ = this.update(cx, |u, cx| {
                u.download = None;
                u.finish_download(info, result, cx)
            });
        })
        .detach();
    }

    /// "取消下载": stops the download and offers the update again.
    pub fn cancel_download(&mut self, cx: &mut Context<Self>) {
        if let Phase::Downloading { info, .. } = &self.phase {
            let info = info.clone();
            if let Some(job) = self.download.take() {
                job.abort();
            }
            self.set_phase(Phase::Available(info), cx);
        }
    }

    fn finish_download(
        &mut self,
        info: UpdateInfo,
        result: Result<Result<PathBuf, chat_core::ClientError>, tokio::task::JoinError>,
        cx: &mut Context<Self>,
    ) {
        let u = self;
        match result {
            Ok(Ok(path)) => {
                remove_other_dmgs(&path);
                // The user chooses when the app restarts.
                u.set_phase(Phase::Ready { info, dmg: path }, cx)
            }
            Ok(Err(e)) => {
                tracing::warn!(error = %e, "update download failed");
                let failure = match e {
                    chat_core::ClientError::Api(chat_core::ApiError::Checksum) => Failure::Checksum,
                    _ => Failure::Download,
                };
                u.set_phase(
                    Phase::Failed {
                        info: Some(info),
                        failure,
                    },
                    cx,
                );
            }
            // 取消下载 already put the update back.
            Err(e) if e.is_cancelled() => {}
            Err(e) => {
                tracing::warn!(error = %e, "update download task failed");
                u.set_phase(
                    Phase::Failed {
                        info: Some(info),
                        failure: Failure::Download,
                    },
                    cx,
                );
            }
        }
    }

    /// "重启安装": installs a downloaded update (after pending sends) and
    /// relaunches.
    pub fn install_now(&mut self, cx: &mut Context<Self>) {
        if let Phase::Ready { info, dmg } = self.phase.clone() {
            self.postponed = None;
            self.install_when_idle(info, dmg, cx);
        }
    }

    /// Number of messages and other writes still waiting in the outbox.
    fn pending_sends(&self) -> usize {
        (self.source)()
            .and_then(|c| c.store().outbox_all().ok())
            .map(|items| {
                items
                    .iter()
                    .filter(|i| i.state == OutboxState::Pending)
                    .count()
            })
            .unwrap_or(0)
    }

    fn install_when_idle(&mut self, info: UpdateInfo, dmg: PathBuf, cx: &mut Context<Self>) {
        if self.pending_sends() > 0 {
            self.set_phase(Phase::WaitingForSends(info.clone()), cx);
            cx.spawn(async move |this, cx| {
                loop {
                    cx.background_executor().timer(SEND_POLL).await;
                    let go_on = this.update(cx, |u, cx| {
                        // The user postponed: stop waiting.
                        if !matches!(&u.phase, Phase::WaitingForSends(i) if *i == info) {
                            return false;
                        }
                        if u.pending_sends() > 0 {
                            return true;
                        }
                        u.run_install(info.clone(), dmg.clone(), cx);
                        false
                    });
                    if !matches!(go_on, Ok(true)) {
                        break;
                    }
                }
            })
            .detach();
        } else {
            self.run_install(info, dmg, cx);
        }
    }

    fn run_install(&mut self, info: UpdateInfo, dmg: PathBuf, cx: &mut Context<Self>) {
        self.set_phase(Phase::Installing(info.clone()), cx);
        let Some(bundle) = running_bundle() else {
            self.open_manually(info, &dmg, cx);
            return;
        };
        let plan = install::Plan {
            bundle,
            dmg: dmg.clone(),
            version: info.version.clone(),
            sha256: info.sha256.clone(),
            bundle_id: env!("PRODUCT_BUNDLE_ID").to_string(),
            app_name: Some(format!("{}.app", env!("PRODUCT_NAME"))),
            marker: marker_path(&self.data_root),
        };
        let task = cx
            .background_executor()
            .spawn(async move { install::install(&plan).map(|m| (m, plan)) });
        cx.spawn(async move |this, cx| {
            let result = task.await;
            let _ = this.update(cx, |u, cx| match result {
                Ok((marker, plan)) => {
                    tracing::info!(version = %marker.version, "update installed, relaunching");
                    let _ = std::fs::remove_file(&plan.dmg);
                    match install::relaunch(
                        std::process::id(),
                        &install::Launch::Open(marker.bundle),
                    ) {
                        Ok(()) => cx.quit(),
                        Err(e) => {
                            // The new bundle is in place; the next start uses it.
                            tracing::warn!(error = %e, "relaunch failed");
                            u.set_phase(
                                Phase::Failed {
                                    info: Some(info),
                                    failure: Failure::Install,
                                },
                                cx,
                            );
                        }
                    }
                }
                Err(install::InstallError::NotWritable) => u.open_manually(info, &dmg, cx),
                Err(install::InstallError::Checksum) => {
                    tracing::warn!("update checksum changed on disk");
                    let _ = std::fs::remove_file(&dmg);
                    u.set_phase(
                        Phase::Failed {
                            info: Some(info),
                            failure: Failure::Checksum,
                        },
                        cx,
                    );
                }
                Err(e) => {
                    tracing::warn!(error = %e, "update install failed");
                    u.set_phase(
                        Phase::Failed {
                            info: Some(info),
                            failure: Failure::Install,
                        },
                        cx,
                    );
                }
            });
        })
        .detach();
    }

    /// Opens the DMG in Finder so the user can drag the app over the old one.
    fn open_manually(&mut self, info: UpdateInfo, dmg: &std::path::Path, cx: &mut Context<Self>) {
        tracing::info!("bundle not replaceable; opening the image");
        let opened = std::process::Command::new("/usr/bin/open")
            .arg(dmg)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .is_ok();
        let phase = if opened {
            Phase::Manual(info)
        } else {
            Phase::Failed {
                info: Some(info),
                failure: Failure::Install,
            }
        };
        self.set_phase(phase, cx);
    }

    /// "知道了" on the manual-install banner.
    pub fn dismiss(&mut self, cx: &mut Context<Self>) {
        if let Phase::Manual(info)
        | Phase::Failed {
            info: Some(info), ..
        } = &self.phase
        {
            self.postponed = Some((info.version.clone(), Instant::now()));
            self.phase = Phase::Available(info.clone());
            cx.notify();
        }
    }
}

/// Keeps only the newest download in the updates folder.
fn remove_other_dmgs(keep: &std::path::Path) {
    let Some(dir) = keep.parent() else { return };
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for e in entries.flatten() {
        let p = e.path();
        if p != keep && p.extension().is_some_and(|x| x == "dmg" || x == "part") {
            let _ = std::fs::remove_file(p);
        }
    }
}
