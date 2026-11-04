//! The 产出 window: one native window per 产出 (crates/artifact-view), with
//! its title and a version list.
//!
//! The provider puts a version on disk ([`ChatProvider::open_artifact_version`]);
//! this module opens or updates the window on the main thread. Window events
//! come back through a channel that a GPUI task drains, so the handlers run
//! with the app context. A link to the web asks first, one prompt at a time.

use std::{
    collections::{HashMap, HashSet},
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

use gpui::{App, AsyncApp, Global};

use crate::{
    copy::{artifacts as ca, fill},
    data::{ArtifactFolder, ChatProvider, FileAttachment, FileError},
    views::files::{error_text, is_html, notify_error, opens_unsafely},
};

/// Open 产出 windows and the 产出 being fetched.
#[derive(Default)]
pub struct ArtifactWindows {
    #[cfg(target_os = "macos")]
    open: HashMap<String, artifact_view::ArtifactWindow>,
    /// 产出 ids whose version is being fetched (cards show 正在打开…).
    opening: HashSet<String>,
    /// inc-host: the window's own title (without the 已更新到 / 有 v{n} note).
    titles: HashMap<String, String>,
}

impl Global for ArtifactWindows {}

/// True while the version of this 产出 is being fetched to open it.
pub fn is_opening(artifact_id: &str, cx: &App) -> bool {
    cx.try_global::<ArtifactWindows>()
        .is_some_and(|w| w.opening.contains(artifact_id))
}

fn windows(cx: &mut App) -> &mut ArtifactWindows {
    cx.default_global::<ArtifactWindows>()
}

/// The version list of the title bar: newest first.
fn version_items(
    provider: &Arc<dyn ChatProvider>,
    artifact_id: &str,
) -> (String, Vec<(u32, String)>) {
    match provider.artifact(artifact_id) {
        Some(d) => {
            let items = d
                .versions
                .iter()
                .map(|v| {
                    let label = match v.note.as_deref().map(str::trim).filter(|n| !n.is_empty()) {
                        Some(note) => {
                            let short: String = note.chars().take(40).collect();
                            fill(
                                ca::VERSION_ITEM_NOTE,
                                &[("n", &v.version.to_string()), ("note", &short)],
                            )
                        }
                        None => fill(ca::VERSION_ITEM, &[("n", &v.version.to_string())]),
                    };
                    (v.version, label)
                })
                .collect();
            (d.summary.title, items)
        }
        None => (crate::copy::panel::ARTIFACTS.to_string(), Vec::new()),
    }
}

/// Opens a 产出 at `version` (`None` = current), or brings its window to
/// the front and switches the version.
pub fn open_artifact(
    provider: &Arc<dyn ChatProvider>,
    artifact_id: &str,
    version: Option<u32>,
    cx: &mut App,
) {
    #[cfg(target_os = "macos")]
    if let Some(win) = windows(cx).open.get(artifact_id)
        && !win.is_closed()
    {
        win.focus();
        if version.is_none_or(|v| v == win.version()) {
            return;
        }
    }
    if !windows(cx).opening.insert(artifact_id.to_string()) {
        return;
    }
    let rx = provider.open_artifact_version(artifact_id, version);
    let provider = provider.clone();
    let id = artifact_id.to_string();
    cx.refresh_windows();
    cx.spawn(async move |cx: &mut AsyncApp| {
        let result = rx.await.unwrap_or(Err(FileError::Other));
        cx.update(|cx| {
            windows(cx).opening.remove(&id);
            cx.refresh_windows();
            match result {
                Ok(folder) => show_folder(&provider, folder, cx),
                Err(e) => notify_error(fill(ca::OPEN_FAILED, &[("reason", error_text(e))]), cx),
            }
        })
    })
    .detach();
}

/// Shows a version folder: a page in the 产出 window, a single file with the
/// default app. A file 产出 whose entry is a page (HTML, SVG, ...) also opens
/// in the 产出 window: a browser would run it with network access.
fn show_folder(provider: &Arc<dyn ChatProvider>, folder: ArtifactFolder, cx: &mut App) {
    // --- v11/media: media the app can show opens in the 产出 viewer, not a system app ---
    if !opens_in_window(&folder) && media_viewer(provider, &folder, cx) {
        return;
    }
    // --- end v11/media ---
    if !opens_in_window(&folder) {
        let path = folder.dir.join(&folder.entry);
        if opens_unsafely(&folder.entry) {
            cx.reveal_path(&path);
        } else {
            cx.open_with_system(&path);
        }
        return;
    }
    #[cfg(target_os = "macos")]
    {
        let (title, items) = version_items(provider, &folder.artifact_id);
        let items: Vec<artifact_view::VersionItem> = items
            .into_iter()
            .map(|(version, label)| artifact_view::VersionItem { version, label })
            .collect();
        // inc-host: bridge capabilities by author (R10), declarations and live data.
        // They follow the version shown, also in a window that is open already.
        let (spec, caps) = super::host::version_spec(
            provider.as_ref(),
            &folder.artifact_id,
            folder.version,
            folder.dir.clone(),
            folder.entry.clone(),
        );
        windows(cx)
            .titles
            .insert(folder.artifact_id.clone(), title.clone());
        if let Some(win) = windows(cx).open.get(&folder.artifact_id)
            && !win.is_closed()
        {
            win.set_title(&title);
            win.set_versions(&items);
            if let Err(e) = win.show_version(spec, caps) {
                tracing::warn!(error = %e, "cannot show the version");
            }
            win.focus();
            return;
        }
        open_window(
            provider.clone(),
            folder.artifact_id.clone(),
            title,
            spec,
            items,
            caps,
            cx,
        );
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = provider;
        cx.open_with_system(&folder.dir.join(&folder.entry));
    }
}

/// True when the version shows in the isolated 产出 window.
fn opens_in_window(folder: &ArtifactFolder) -> bool {
    folder.is_page || is_html(&folder.entry)
}

/// Opens an HTML file attached to a message in its own isolated window
/// (one version, no version list).
pub fn open_html_file(file: &FileAttachment, path: &Path, cx: &mut App) {
    #[cfg(target_os = "macos")]
    {
        use artifact_view::{Content, VersionSpec};
        let Some(dir) = path.parent() else { return };
        let Some(entry) = path.file_name().map(|n| n.to_string_lossy().into_owned()) else {
            return;
        };
        let id: String = format!("f-{}", file.file_id)
            .chars()
            .filter(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_')
            .take(64)
            .collect();
        if let Some(win) = windows(cx).open.get(&id)
            && !win.is_closed()
        {
            win.focus();
            return;
        }
        let spec = VersionSpec::new(1, Content::Folder(dir.to_path_buf())).with_entry(entry);
        open_window_with(
            None,
            id,
            file.name.clone(),
            spec,
            Vec::new(),
            artifact_view::Capabilities::NONE,
            cx,
        );
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = file;
        cx.reveal_path(path);
    }
}

#[cfg(target_os = "macos")]
fn open_window(
    provider: Arc<dyn ChatProvider>,
    artifact_id: String,
    title: String,
    spec: artifact_view::VersionSpec,
    items: Vec<artifact_view::VersionItem>,
    caps: artifact_view::Capabilities,
    cx: &mut App,
) {
    open_window_with(Some(provider), artifact_id, title, spec, items, caps, cx);
}

#[cfg(target_os = "macos")]
fn open_window_with(
    provider: Option<Arc<dyn ChatProvider>>,
    artifact_id: String,
    title: String,
    spec: artifact_view::VersionSpec,
    items: Vec<artifact_view::VersionItem>,
    caps: artifact_view::Capabilities,
    cx: &mut App,
) {
    use artifact_view::{ArtifactWindow, OpenOptions};
    use futures::StreamExt as _;
    use objc2_foundation::MainThreadMarker;

    let Some(mtm) = MainThreadMarker::new() else {
        tracing::error!("the 产出 window must open on the main thread");
        return;
    };
    let mut options = OpenOptions::new(title, artifact_id.clone(), spec);
    // Only a bot's version that declares events may send them (R10).
    options.capabilities = caps;
    options.versions = items;
    if let Some(p) = &provider {
        options.viewer = super::host::viewer(p.as_ref());
        options.ref_menu = Some(super::host::ref_menu(cx));
    }
    let (tx, mut rx) = futures::channel::mpsc::unbounded();
    let win = match ArtifactWindow::open(mtm, options, move |event| {
        let _ = tx.unbounded_send(event);
    }) {
        Ok(w) => w,
        Err(e) => {
            tracing::warn!(error = %e, "cannot open the 产出 window");
            notify_error(
                fill(ca::OPEN_FAILED, &[("reason", error_text(FileError::Other))]),
                cx,
            );
            return;
        }
    };
    windows(cx).open.insert(artifact_id.clone(), win);
    cx.spawn(async move |cx: &mut AsyncApp| {
        while let Some(event) = rx.next().await {
            let closed = matches!(event, artifact_view::ArtifactEvent::Closed);
            cx.update(|cx| on_event(provider.as_ref(), &artifact_id, event, cx));
            if closed {
                break;
            }
        }
    })
    .detach();
}

#[cfg(target_os = "macos")]
fn on_event(
    provider: Option<&Arc<dyn ChatProvider>>,
    artifact_id: &str,
    event: artifact_view::ArtifactEvent,
    cx: &mut App,
) {
    use artifact_view::ArtifactEvent as E;
    match event {
        E::Closed => {
            windows(cx).open.remove(artifact_id);
        }
        E::VersionSelected { version } => {
            if let Some(p) = provider {
                open_artifact(p, artifact_id, Some(version), cx);
            }
        }
        E::ExternalLink { url } => ask_to_open_link(url),
        E::LoadFailed { version, reason } => {
            tracing::warn!(artifact_id, version, %reason, "产出 page failed to load");
            notify_error(ca::LOAD_FAILED.to_string(), cx);
        }
        // inc-host: loads get the data; emits, asks and refs go to the page host.
        e @ (E::Loaded { .. } | E::Emit { .. } | E::Ask { .. } | E::Ref { .. }) => {
            if provider.is_some() {
                super::host::on_page_event(
                    super::host::PageRef::Window(artifact_id.to_string()),
                    artifact_id,
                    &e,
                    None,
                    cx,
                );
            }
        }
        E::FocusChanged { .. } => {}
    }
}

// --- inc-host track: the page host reaches the 产出 windows ---

/// Gives the window of `artifact_id` a data snapshot when it shows `version`.
pub(crate) fn push_to_window(
    artifact_id: &str,
    version: u32,
    snap: &artifact_view::Snapshot,
    cx: &mut App,
) -> bool {
    #[cfg(target_os = "macos")]
    if let Some(win) = windows(cx).open.get(artifact_id)
        && !win.is_closed()
    {
        return win.push_data(version, snap.clone());
    }
    let _ = (artifact_id, version, snap, cx);
    false
}

/// Finishes an open `emit` / `ask` of the page in the window of `artifact_id`.
pub(crate) fn finish_in_window(
    artifact_id: &str,
    version: u32,
    reply_id: &serde_json::Value,
    result: Result<serde_json::Value, artifact_view::Reject>,
    cx: &mut App,
) -> bool {
    #[cfg(target_os = "macos")]
    if let Some(win) = windows(cx).open.get(artifact_id)
        && !win.is_closed()
    {
        return win.finish(version, reply_id, result);
    }
    let _ = (artifact_id, version, reply_id, result, cx);
    false
}

/// A new version exists: a window that showed the latest (`before`) reloads in place and
/// says 已更新到 v{n} in its title for 3 s; one on an older version says 有 v{n}.
pub(crate) fn on_version_added(artifact_id: &str, before: u32, version: u32, cx: &mut App) {
    #[cfg(target_os = "macos")]
    {
        use crate::copy::page_host as cp;
        let Some(shown) = windows(cx)
            .open
            .get(artifact_id)
            .filter(|w| !w.is_closed())
            .map(|w| w.version())
        else {
            return;
        };
        let title = windows(cx)
            .titles
            .get(artifact_id)
            .cloned()
            .unwrap_or_default();
        if shown == before {
            if let Some(p) = super::host::provider(cx) {
                reload_window(&p, artifact_id, version, cx);
            }
            let note = fill(cp::PAGE_UPDATED_TO, &[("n", &version.to_string())]);
            if let Some(w) = windows(cx).open.get(artifact_id) {
                w.set_title(&format!("{title} · {note}"));
            }
            let id = artifact_id.to_string();
            cx.spawn(async move |cx: &mut AsyncApp| {
                cx.background_executor()
                    .timer(super::host::UPDATED_FOR)
                    .await;
                cx.update(|cx| {
                    let t = windows(cx).titles.get(&id).cloned().unwrap_or_default();
                    if let Some(w) = windows(cx).open.get(&id) {
                        w.set_title(&t);
                    }
                });
            })
            .detach();
        } else {
            let note = fill(cp::PAGE_NEWER, &[("n", &version.to_string())]);
            if let Some(w) = windows(cx).open.get(artifact_id) {
                w.set_title(&format!("{title} · {note}"));
            }
        }
    }
    #[cfg(not(target_os = "macos"))]
    let _ = (artifact_id, before, version, cx);
}

/// Loads `version` into the open window of `artifact_id` (reload in place).
#[cfg(target_os = "macos")]
fn reload_window(provider: &Arc<dyn ChatProvider>, artifact_id: &str, version: u32, cx: &mut App) {
    let rx = provider.open_artifact_version(artifact_id, Some(version));
    let provider = provider.clone();
    cx.spawn(async move |cx: &mut AsyncApp| {
        if let Ok(Ok(folder)) = rx.await {
            cx.update(|cx| {
                let (title, items) = version_items(&provider, &folder.artifact_id);
                let items: Vec<artifact_view::VersionItem> = items
                    .into_iter()
                    .map(|(version, label)| artifact_view::VersionItem { version, label })
                    .collect();
                let _ = title;
                let (spec, caps) = super::host::version_spec(
                    provider.as_ref(),
                    &folder.artifact_id,
                    folder.version,
                    folder.dir.clone(),
                    folder.entry.clone(),
                );
                if let Some(win) = windows(cx).open.get(&folder.artifact_id)
                    && !win.is_closed()
                {
                    win.set_versions(&items);
                    if let Err(e) = win.show_version(spec, caps) {
                        tracing::warn!(error = %e, "cannot reload the version");
                    }
                }
            });
        }
    })
    .detach();
}

// --- end inc-host track ---

/// Asks before a 产出 page opens a web link (also for pages embedded in the
/// main window).
pub(crate) fn ask_before_link(url: String) {
    #[cfg(target_os = "macos")]
    ask_to_open_link(url);
    #[cfg(not(target_os = "macos"))]
    let _ = url;
}

/// Only one link prompt shows at a time; links clicked meanwhile are dropped.
static PROMPTING: AtomicBool = AtomicBool::new(false);

/// Asks with a native alert before a 产出 page opens a web link. The alert
/// runs on a later main-queue turn, outside any GPUI update.
#[cfg(target_os = "macos")]
fn ask_to_open_link(url: String) {
    let ask = Ask {
        title: ca::LINK_TITLE.to_string(),
        detail: fill(ca::LINK_DETAIL, &[("url", &url)]),
        open: ca::LINK_OPEN,
        cancel: ca::LINK_CANCEL,
    };
    ask_then_open(ask, url, artifact_view::open_external_url);
}

/// Asks before a machine's screen sharing address (接管) opens, and opens
/// only a `vnc`, `http` or `https` address.
pub(crate) fn ask_before_screen_share(name: &str, url: String) {
    if !artifact_view::is_screen_share_url(&url) {
        tracing::warn!("screen sharing address refused");
        return;
    }
    #[cfg(target_os = "macos")]
    {
        use crate::copy::live as cl;
        let ask = Ask {
            title: cl::LIVE_SHARE_ASK_TITLE.to_string(),
            detail: fill(cl::LIVE_SHARE_ASK_DETAIL, &[("name", name), ("url", &url)]),
            open: cl::LIVE_SHARE_ASK_OPEN,
            cancel: cl::LIVE_SHARE_ASK_CANCEL,
        };
        ask_then_open(ask, url, artifact_view::open_screen_share_url);
    }
    #[cfg(not(target_os = "macos"))]
    let _ = name;
}

/// The text of one open-a-link alert.
#[cfg(target_os = "macos")]
struct Ask {
    title: String,
    detail: String,
    open: &'static str,
    cancel: &'static str,
}

/// Shows `ask` as a native alert on a later main-queue turn (outside any
/// GPUI update) and calls `open` with `url` only when the user agrees.
#[cfg(target_os = "macos")]
fn ask_then_open(ask: Ask, url: String, open: fn(&str) -> bool) {
    if PROMPTING.swap(true, Ordering::SeqCst) {
        return;
    }
    dispatch2::DispatchQueue::main().exec_async(move || {
        use objc2_app_kit::{NSAlert, NSAlertFirstButtonReturn, NSAlertStyle};
        use objc2_foundation::{MainThreadMarker, NSString};
        let agreed = MainThreadMarker::new().is_some_and(|mtm| {
            let alert = NSAlert::new(mtm);
            alert.setAlertStyle(NSAlertStyle::Warning);
            alert.setMessageText(&NSString::from_str(&ask.title));
            alert.setInformativeText(&NSString::from_str(&ask.detail));
            alert.addButtonWithTitle(&NSString::from_str(ask.open));
            alert.addButtonWithTitle(&NSString::from_str(ask.cancel));
            alert.runModal() == NSAlertFirstButtonReturn
        });
        if agreed && !open(&url) {
            tracing::warn!("external link refused");
        }
        PROMPTING.store(false, Ordering::SeqCst);
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn folder(entry: &str, is_page: bool) -> ArtifactFolder {
        ArtifactFolder {
            artifact_id: "a1".into(),
            version: 1,
            dir: "/tmp/a1".into(),
            entry: entry.into(),
            is_page,
        }
    }

    #[test]
    fn file_artifacts_that_are_pages_open_in_the_isolated_window() {
        assert!(opens_in_window(&folder("index.html", true)));
        for entry in ["report.html", "docs/a.XHTML", "logo.svg"] {
            assert!(opens_in_window(&folder(entry, false)), "{entry}");
        }
        assert!(!opens_in_window(&folder("report.pdf", false)));
    }

    /// Trust boundary: the separate 产出 window's page events go through the same
    /// user-activation gate as a page in the main window. An `emitEvent` or `ask` the page
    /// sends without the viewer's input in that window is dropped; with it, it is sent.
    #[cfg(target_os = "macos")]
    #[gpui::test]
    fn window_page_events_need_the_viewer_s_input(cx: &mut gpui::TestAppContext) {
        use crate::data::page_host::PageEventKind;
        use artifact_view::ArtifactEvent as E;
        use serde_json::json;
        const ART: &str = crate::fake::v2::ART_REPORT;
        cx.update(|cx| {
            let provider: Arc<dyn ChatProvider> = crate::fake::v2::V2Samples::new();
            super::super::host::reset_for_test(provider.clone(), cx);
            let emit = |user_action: bool, cx: &mut App| {
                on_event(
                    Some(&provider),
                    ART,
                    E::Emit {
                        version: 3,
                        name: "filter".into(),
                        payload: json!({ "channel": "B 站" }),
                        user_action,
                        reply_id: Some(json!(1)),
                    },
                    cx,
                )
            };
            let ask = |user_action: bool, cx: &mut App| {
                on_event(
                    Some(&provider),
                    ART,
                    E::Ask {
                        version: 3,
                        reply_id: json!(2),
                        name: "question".into(),
                        payload: json!({ "q": "B 站" }),
                        timeout_ms: 60_000,
                        user_action,
                    },
                    cx,
                )
            };
            emit(false, cx);
            ask(false, cx);
            assert!(
                super::super::host::sent(cx).is_empty(),
                "events without input in the window must be dropped"
            );
            emit(true, cx);
            ask(true, cx);
            let kinds: Vec<PageEventKind> = super::super::host::sent(cx)
                .into_iter()
                .map(|(a, v, _, k)| {
                    assert_eq!((a.as_str(), v), (ART, 3));
                    k
                })
                .collect();
            assert_eq!(kinds, [PageEventKind::Emit, PageEventKind::Ask]);
        });
    }
}

// --- v11/media ---
/// Opens a non-page version in the 产出 viewer when it is media the app shows (Markdown,
/// images, video, audio, PDF, code, tables). False leaves it to the system app.
fn media_viewer(provider: &Arc<dyn ChatProvider>, folder: &ArtifactFolder, cx: &mut App) -> bool {
    use crate::views::media::{self, MediaKind, store::MediaKey, viewer};
    let kind = media::file_kind(&folder.entry, media::mime_for(&folder.entry));
    if matches!(kind, MediaKind::File | MediaKind::Html) {
        return false;
    }
    let key = MediaKey::new(folder.artifact_id.clone(), folder.version);
    let title = provider
        .artifact(&folder.artifact_id)
        .map(|d| d.summary.title)
        .unwrap_or_else(|| folder.entry.clone());
    media::store::request_folder(&key, &title, folder.dir.clone(), folder.entry.clone(), cx);
    viewer::open(
        key,
        title,
        viewer::target_for(provider, &folder.artifact_id),
        viewer::Opening::default(),
        cx,
    );
    true
}
// --- end v11/media ---
