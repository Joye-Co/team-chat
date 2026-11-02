//! In-app update.
//!
//! - [`Updater`] is an app-wide entity (a GPUI global, see [`install`]). It checks
//!   `GET /v1/updates/latest` through chat-core when the app starts (as soon as a
//!   session exists) and every 6 hours, downloads the DMG with progress (chat-core checks
//!   size and SHA-256), waits until no send is pending, then installs on a background
//!   thread ([`install::install`]) and relaunches.
//! - When the running bundle cannot be replaced (not writable, or no bundle under
//!   `cargo run`), it opens the DMG in Finder and the banner explains what to do.
//! - [`banner::UpdateBanner`] (main window) and [`banner::AboutUpdate`] (settings, 关于)
//!   render the state. They read only this entity; the network and the cache stay here.
//! - On start, [`install::finish_on_launch`] removes the old bundle of the previous update.

pub mod banner;
pub mod install;
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
pub fn install(source: ClientSource, data_root: PathBuf, cx: &mut App) -> Entity<Updater> {
    let marker = marker_path(&data_root);
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
    });
    cx.set_global(UpdaterGlobal(updater.clone()));
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
            | Phase::WaitingForSends(_)
            | Phase::Installing(_)
            | Phase::Manual(_) => true,
            Phase::Failed { info, failure } => info.is_some() && *failure != Failure::Check,
            _ => false,
        }
    }

    fn tick(&mut self, cx: &mut Context<Self>) {
        if self.phase.busy() {
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
                u.set_phase(phase, cx);
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

    /// "稍后": hides the banner for this version for a day; stops waiting for sends.
    pub fn postpone(&mut self, cx: &mut Context<Self>) {
        let Some(info) = self.phase.info().cloned().or_else(|| self.found.clone()) else {
            return;
        };
        if matches!(self.phase, Phase::Installing(_) | Phase::Downloading { .. }) {
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
            Phase::Downloading { .. } | Phase::Installing(_) | Phase::WaitingForSends(_)
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
            let _ = this.update(cx, |u, cx| match result {
                Ok(Ok(path)) => {
                    remove_other_dmgs(&path);
                    u.install_when_idle(info, path, cx)
                }
                Ok(Err(e)) => {
                    tracing::warn!(error = %e, "update download failed");
                    let failure = match e {
                        chat_core::ClientError::Api(chat_core::ApiError::Checksum) => {
                            Failure::Checksum
                        }
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
            });
        })
        .detach();
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
                    match install::relaunch(std::process::id(), &install::Launch::Open(marker.bundle))
                    {
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
        if let Phase::Manual(info) | Phase::Failed { info: Some(info), .. } = &self.phase {
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
