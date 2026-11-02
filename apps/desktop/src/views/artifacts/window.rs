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
        use artifact_view::{Content, VersionSpec};
        let spec = VersionSpec::new(folder.version, Content::Folder(folder.dir.clone()))
            .with_entry(folder.entry.clone());
        let (title, items) = version_items(provider, &folder.artifact_id);
        let items: Vec<artifact_view::VersionItem> = items
            .into_iter()
            .map(|(version, label)| artifact_view::VersionItem { version, label })
            .collect();
        if let Some(win) = windows(cx).open.get(&folder.artifact_id)
            && !win.is_closed()
        {
            win.set_title(&title);
            win.set_versions(&items);
            if let Err(e) = win.show_version(spec) {
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
        open_window_with(None, id, file.name.clone(), spec, Vec::new(), cx);
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
    cx: &mut App,
) {
    open_window_with(Some(provider), artifact_id, title, spec, items, cx);
}

#[cfg(target_os = "macos")]
fn open_window_with(
    provider: Option<Arc<dyn ChatProvider>>,
    artifact_id: String,
    title: String,
    spec: artifact_view::VersionSpec,
    items: Vec<artifact_view::VersionItem>,
    cx: &mut App,
) {
    use artifact_view::{ArtifactWindow, Capabilities, OpenOptions};
    use futures::StreamExt as _;
    use objc2_foundation::MainThreadMarker;

    let Some(mtm) = MainThreadMarker::new() else {
        tracing::error!("the 产出 window must open on the main thread");
        return;
    };
    let mut options = OpenOptions::new(title, artifact_id.clone(), spec);
    // Pages from teammates get no bridge capabilities.
    options.capabilities = Capabilities::NONE;
    options.versions = items;
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
        E::Loaded { .. } | E::Emit { .. } => {}
    }
}

/// Only one link prompt shows at a time; links clicked meanwhile are dropped.
static PROMPTING: AtomicBool = AtomicBool::new(false);

/// Asks with a native alert before a 产出 page opens a web link. The alert
/// runs on a later main-queue turn, outside any GPUI update.
#[cfg(target_os = "macos")]
fn ask_to_open_link(url: String) {
    if PROMPTING.swap(true, Ordering::SeqCst) {
        return;
    }
    dispatch2::DispatchQueue::main().exec_async(move || {
        use objc2_app_kit::{NSAlert, NSAlertFirstButtonReturn, NSAlertStyle};
        use objc2_foundation::{MainThreadMarker, NSString};
        let open = MainThreadMarker::new().is_some_and(|mtm| {
            let alert = NSAlert::new(mtm);
            alert.setAlertStyle(NSAlertStyle::Warning);
            alert.setMessageText(&NSString::from_str(ca::LINK_TITLE));
            alert.setInformativeText(&NSString::from_str(&fill(
                ca::LINK_DETAIL,
                &[("url", &url)],
            )));
            alert.addButtonWithTitle(&NSString::from_str(ca::LINK_OPEN));
            alert.addButtonWithTitle(&NSString::from_str(ca::LINK_CANCEL));
            alert.runModal() == NSAlertFirstButtonReturn
        });
        if open && !artifact_view::open_external_url(&url) {
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
}
