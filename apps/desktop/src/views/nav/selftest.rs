//! `desktop --selftest-nav --report <path>`: opens the main window with fake
//! data without taking focus, drives the navigation actions and dialogs, and
//! writes a JSON report. It never activates the app.

use std::{path::PathBuf, time::Duration};

use gpui::{Action, ActivationPolicy, AnyWindowHandle, App, AsyncApp};
use gpui_component::WindowExt as _;

use super::{
    NavGlobal, NewGroup, OpenGroup2, QuickSwitch, StartDm, WalkTopic, saved_from, unread_target,
};
use crate::{
    app::{self, Place, Services},
    data::{CreateGroupRequest, GroupKind},
};

pub type Finish = fn(&PathBuf, serde_json::Value, bool, &mut App);

async fn pause(cx: &mut AsyncApp) {
    cx.background_executor()
        .timer(Duration::from_millis(250))
        .await;
}

fn key(handle: AnyWindowHandle, keystroke: &'static str, cx: &mut AsyncApp) {
    if let Ok(k) = gpui::Keystroke::parse(keystroke) {
        let _ = handle.update(cx, |_, window, cx| window.dispatch_keystroke(k, cx));
    }
}

fn dispatch(handle: AnyWindowHandle, action: Box<dyn Action>, cx: &mut AsyncApp) {
    let _ = handle.update(cx, |_, window, cx| window.dispatch_action(action, cx));
}

/// Opens a dialog with `action`, checks that it shows, and closes it.
async fn dialog_opens(handle: AnyWindowHandle, action: Box<dyn Action>, cx: &mut AsyncApp) -> bool {
    dispatch(handle, action, cx);
    pause(cx).await;
    let open = handle
        .update(cx, |_, window, cx| window.has_active_dialog(cx))
        .unwrap_or(false);
    let _ = handle.update(cx, |_, window, cx| window.close_all_dialogs(cx));
    pause(cx).await;
    open
}

pub fn run(services: Services, report: PathBuf, finish: Finish, cx: &mut App) {
    cx.set_activation_policy(ActivationPolicy::Accessory);
    super::restore(&services, cx);
    if let Err(error) = app::open_main_window(services.clone(), false, None, cx) {
        finish(
            &report,
            serde_json::json!({ "ok": false, "error": error }),
            false,
            cx,
        );
        return;
    }
    let timeout_report = report.clone();
    cx.spawn(async move |cx| {
        cx.background_executor()
            .timer(Duration::from_secs(60))
            .await;
        cx.update(|cx| {
            let json = serde_json::json!({ "ok": false, "error": "timed out" });
            finish(&timeout_report, json, false, cx)
        });
    })
    .detach();

    cx.spawn(async move |cx| {
        pause(cx).await;
        let Some(handle) = cx.update(|cx| cx.global::<NavGlobal>().window) else {
            cx.update(|cx| {
                finish(
                    &report,
                    serde_json::json!({ "ok": false, "error": "no main window" }),
                    false,
                    cx,
                )
            });
            return;
        };
        let provider = services.provider.clone();
        let mut checks = serde_json::Map::new();
        let mut check = |name: &str, ok: bool| {
            checks.insert(name.to_string(), serde_json::Value::Bool(ok));
        };

        let restored = cx.update(|cx| services.nav.read(cx).place.clone());
        check(
            "restore_opens_first_group",
            restored
                == provider
                    .groups()
                    .first()
                    .map(|g| Place::Group(g.id.clone())),
        );
        check(
            "quick_switcher_opens",
            dialog_opens(handle, Box::new(QuickSwitch), cx).await,
        );
        check(
            "new_group_dialog_opens",
            dialog_opens(handle, Box::new(NewGroup), cx).await,
        );
        check(
            "start_dm_picker_opens",
            dialog_opens(handle, Box::new(StartDm), cx).await,
        );

        // The keyboard path: Cmd+K, then Enter picks the first 群.
        key(handle, "cmd-k", cx);
        pause(cx).await;
        let open = handle
            .update(cx, |_, window, cx| window.has_active_dialog(cx))
            .unwrap_or(false);
        key(handle, "down", cx);
        key(handle, "enter", cx);
        pause(cx).await;
        let closed = !handle
            .update(cx, |_, window, cx| window.has_active_dialog(cx))
            .unwrap_or(true);
        let _ = handle.update(cx, |_, window, cx| window.close_all_dialogs(cx));
        let expected = super::switcher::entries(provider.as_ref(), services.me().as_deref())
            .into_iter()
            .flat_map(|(_, e)| e)
            .nth(1)
            .map(|e| e.target);
        let place = cx.update(|cx| services.nav.read(cx).place.clone());
        let picked = match expected {
            Some(super::switcher::Target::Group(g)) => place == Some(Place::Group(g)),
            _ => false,
        };
        check("cmd_k_opens_switcher", open);
        check("switcher_enter_closes", closed);
        check("switcher_down_enter_opens_second_entry", picked);

        key(handle, "cmd-1", cx);
        pause(cx).await;
        let first = provider
            .groups()
            .first()
            .map(|g| Place::Group(g.id.clone()));
        let place = cx.update(|cx| services.nav.read(cx).place.clone());
        check(
            "cmd_1_key_opens_first_group",
            first.is_some() && place == first,
        );

        dispatch(handle, Box::new(OpenGroup2), cx);
        pause(cx).await;
        let second = provider.groups().get(1).map(|g| Place::Group(g.id.clone()));
        let place = cx.update(|cx| services.nav.read(cx).place.clone());
        check(
            "cmd_2_opens_second_group",
            second.is_some() && place == second,
        );

        // Option+Down goes where the walk says.
        let order: Vec<WalkTopic> = provider
            .groups()
            .iter()
            .filter(|g| !provider.group_muted(&g.id))
            .flat_map(|g| {
                provider.topics(&g.id).into_iter().map(|t| WalkTopic {
                    group_id: g.id.clone(),
                    topic_id: t.id,
                    unread: t.unread > 0,
                })
            })
            .collect();
        let (current, group) = cx.update(|cx| {
            let nav = services.nav.read(cx);
            (nav.topic.clone(), nav.current_group())
        });
        let expected = unread_target(
            &order,
            current.as_ref().map(|(g, t)| (g.as_str(), t.as_str())),
            group.as_deref(),
            true,
        );
        key(handle, "alt-down", cx);
        pause(cx).await;
        let topic = cx.update(|cx| services.nav.read(cx).topic.clone());
        check(
            "option_down_opens_next_unread",
            expected.is_some() && topic == expected,
        );

        // Group context menu: the leave confirmation shows.
        let leave_target = provider
            .groups()
            .into_iter()
            .find(|g| g.kind == GroupKind::Group);
        let leave_dialog = if let Some(g) = leave_target {
            let s = services.clone();
            let _ = handle.update(cx, |_, window, cx| {
                super::dialogs::confirm_leave(s, g.id.clone(), g.name.clone(), window, cx)
            });
            pause(cx).await;
            let open = handle
                .update(cx, |_, window, cx| window.has_active_dialog(cx))
                .unwrap_or(false);
            let _ = handle.update(cx, |_, window, cx| window.close_all_dialogs(cx));
            pause(cx).await;
            open
        } else {
            false
        };
        check("leave_confirmation_opens", leave_dialog);

        // A new group opens once the provider delivers it.
        cx.update(|cx| {
            super::create_group(
                CreateGroupRequest {
                    name: "自测新群".into(),
                    member_ids: vec![],
                },
                cx,
            )
        });
        pause(cx).await;
        let new_id = provider
            .groups()
            .into_iter()
            .find(|g| g.name == "自测新群")
            .map(|g| g.id);
        let place = cx.update(|cx| services.nav.read(cx).place.clone());
        check(
            "new_group_opens_after_create",
            new_id.is_some() && place == new_id.clone().map(Place::Group),
        );

        // Mute changes the provider's state; the sidebar re-reads it.
        if let Some(id) = &new_id {
            provider.set_group_muted(id, true);
            check("mute_sets_preference", provider.group_muted(id));
            provider.set_group_muted(id, false);
        }

        // Picking a member with no DM opens a new DM.
        let me = services.me();
        let peer = provider.members().into_iter().find(|m| {
            Some(&m.id) != me.as_ref()
                && !m.deactivated
                && !provider
                    .groups()
                    .iter()
                    .any(|g| g.dm_peer.as_deref() == Some(m.id.as_str()))
        });
        if let Some(peer) = peer {
            let id = peer.id.clone();
            cx.update(|cx| super::open_dm_with(id, cx));
            pause(cx).await;
            let dm = provider
                .groups()
                .into_iter()
                .find(|g| g.dm_peer.as_deref() == Some(peer.id.as_str()))
                .map(|g| Place::Group(g.id));
            let place = cx.update(|cx| services.nav.read(cx).place.clone());
            check("member_opens_new_dm", dm.is_some() && place == dm);
        } else {
            check("member_opens_new_dm", false);
        }

        // The provider holds the current place for the next start.
        let saved = cx.update(|cx| {
            let nav = services.nav.read(cx);
            Some(saved_from(nav.place.as_ref(), nav.topic.as_ref()))
        });
        check(
            "place_saved_through_provider",
            saved.is_some() && provider.saved_navigation() == saved,
        );

        let status = cx.update(|cx| super::status_item_ok(&services, cx));
        check("menu_bar_item_shows_unread_state", status);

        let ok = checks.values().all(|v| v.as_bool() == Some(true));
        let json = serde_json::json!({ "ok": ok, "checks": checks });
        cx.update(|cx| finish(&report, json, ok, cx));
    })
    .detach();
}
