//! Connects the chat-core provider to the running app: notifications while
//! the app is in the background, opening a topic from a clicked notification,
//! and reconnecting at once on wake from sleep and network changes.

use std::sync::Arc;

use futures::StreamExt as _;
use gpui::App;

use super::CoreProvider;
use crate::app::Services;

/// Call once after the main window's services exist.
pub fn install(core: Arc<CoreProvider>, services: Services, cx: &mut App) {
    #[cfg(target_os = "macos")]
    {
        use super::macos;
        let Some(mtm) = objc2::MainThreadMarker::new() else {
            return;
        };
        let mode = macos::install_notifications(mtm);
        tracing::info!(?mode, "notifications");

        let reconnect = core.clone();
        macos::watch_system(Arc::new(move |why| {
            tracing::info!(why, "system change");
            reconnect.reconnect_now_inner();
        }));

        let mut notices = core.notices();
        cx.spawn(async move |_| {
            while let Some(notice) = notices.next().await {
                // Runs on the main thread.
                if let Some(mtm) = objc2::MainThreadMarker::new()
                    && !macos::app_is_active(mtm)
                {
                    macos::post(&notice, crate::views::settings::prefs::get().sound, mtm);
                }
            }
        })
        .detach();

        let mut clicks = macos::clicks();
        cx.spawn(async move |cx| {
            while let Some((group_id, topic_id)) = clicks.next().await {
                let nav = services.nav.clone();
                cx.update(|cx| {
                    nav.update(cx, |nav, cx| {
                        if topic_id.is_empty() {
                            nav.open_group_chat(group_id, cx)
                        } else {
                            nav.open_topic(group_id, topic_id, cx)
                        }
                    });
                    // The user clicked the notification: bring the app forward.
                    cx.activate(true);
                });
            }
        })
        .detach();
    }
    #[cfg(target_os = "windows")]
    {
        use super::windows;
        let mode = windows::install_notifications(cx);
        tracing::info!(?mode, "notifications");

        let reconnect = core.clone();
        windows::watch_system(
            Arc::new(move |why| {
                tracing::info!(why, "system change");
                reconnect.reconnect_now_inner();
            }),
            cx,
        );

        // A click on a toast opens its 话题 through the toast's link, which reaches the
        // app as a launch (`provider::instance`), also after the app quit.
        let mut notices = core.notices();
        cx.spawn(async move |cx| {
            while let Some(notice) = notices.next().await {
                // Runs on the main thread.
                if !windows::app_is_active() {
                    cx.update(|cx| {
                        let group_name = services
                            .provider
                            .groups()
                            .into_iter()
                            .find(|g| g.id == notice.group_id)
                            .map(|g| g.name)
                            .unwrap_or_default();
                        windows::post(
                            &notice,
                            &group_name,
                            crate::views::settings::prefs::get().sound,
                            cx,
                        )
                    });
                }
            }
        })
        .detach();
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        let _ = (core, services, cx);
    }
}
