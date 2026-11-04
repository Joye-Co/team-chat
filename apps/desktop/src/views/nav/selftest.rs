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

        // The keyboard path: Cmd+K, Down, then Enter picks the second entry.
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
        // The palette shows 话题 first, then 群和成员 (DESIGN.md §13.12).
        let expected = super::switcher::display(super::switcher::entries(
            provider.as_ref(),
            services.me().as_deref(),
        ))
        .into_iter()
        .flat_map(|(_, e)| e)
        .nth(1)
        .map(|e| e.target);
        let (place, topic) = cx.update(|cx| {
            let nav = services.nav.read(cx);
            (nav.place.clone(), nav.topic.clone())
        });
        let picked = match expected {
            Some(super::switcher::Target::Group(g)) => place == Some(Place::Group(g)),
            Some(super::switcher::Target::Topic(g, t)) => topic == Some((g, t)),
            _ => false,
        };
        check("cmd_k_opens_switcher", open);
        check("switcher_enter_closes", closed);
        check("switcher_down_enter_opens_second_entry", picked);

        // The query field has the focus (the caret) when the switcher opens,
        // and Cmd+F opens it scoped to the current 群.
        let s = services.clone();
        let view = handle
            .update(cx, |_, window, cx| {
                super::switcher::open_scoped(s, None, "", window, cx)
            })
            .ok();
        pause(cx).await;
        let focused = view.is_some_and(|v| {
            handle
                .update(cx, |_, window, cx| v.read(cx).query_focused(window, cx))
                .unwrap_or(false)
        });
        check("switcher_query_has_focus", focused);
        let _ = handle.update(cx, |_, window, cx| window.close_all_dialogs(cx));
        pause(cx).await;
        let s = services.clone();
        let current = cx.update(|cx| crate::views::search::current_group(&services, cx));
        let view = handle
            .update(cx, |_, window, cx| {
                super::switcher::open_scoped(s, current.clone(), "", window, cx)
            })
            .ok();
        pause(cx).await;
        let scoped = view.is_some_and(|v| {
            handle
                .update(cx, |_, window, cx| {
                    let v = v.read(cx);
                    v.scope() == current.as_ref() && v.query_focused(window, cx)
                })
                .unwrap_or(false)
        });
        check("cmd_f_opens_switcher_scoped", current.is_some() && scoped);
        let _ = handle.update(cx, |_, window, cx| window.close_all_dialogs(cx));
        pause(cx).await;

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
            drop(super::create_group(
                CreateGroupRequest {
                    name: "自测新群".into(),
                    member_ids: vec![],
                },
                cx,
            ));
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

/// Screenshot scenes of the navigation dialogs over the main window
/// (`desktop --screenshot`, see `selftest/scenes.rs`).
pub mod scenes {
    use gpui::{AnyWindowHandle, App, Pixels, Size, Window};

    use crate::{app::Services, selftest::scenes::open_main};

    fn over_main(
        services: Services,
        content: Size<Pixels>,
        cx: &mut App,
        open: fn(Services, &mut Window, &mut App),
    ) -> Result<AnyWindowHandle, String> {
        let handle = open_main(services.clone(), content, cx)?;
        handle
            .update(cx, |_, window, cx| open(services, window, cx))
            .map_err(|e| e.to_string())?;
        Ok(handle)
    }

    /// The quick switcher with a query typed.
    pub fn switcher_query(
        services: Services,
        content: Size<Pixels>,
        cx: &mut App,
    ) -> Result<AnyWindowHandle, String> {
        over_main(services, content, cx, |services, window, cx| {
            super::super::switcher::open_with(services, "话题", window, cx)
        })
    }

    // --- v12/overlays ---
    /// The 新话题 sheet over the open 群, a first message typed.
    pub fn new_topic(
        services: Services,
        content: Size<Pixels>,
        cx: &mut App,
    ) -> Result<AnyWindowHandle, String> {
        over_main(services, content, cx, |services, window, cx| {
            let group = services.nav.read(cx).current_group();
            super::super::new_topic::open_new_topic(services, group, window, cx);
        })
    }
    // --- end v12/overlays ---

    /// The new-group sheet.
    pub fn new_group(
        services: Services,
        content: Size<Pixels>,
        cx: &mut App,
    ) -> Result<AnyWindowHandle, String> {
        over_main(services, content, cx, super::super::dialogs::open_new_group)
    }

    /// The sheet that adds members to the first 群.
    /// Opens the 群 with the fewest members and its newest 话题, so the
    /// add-members sheet over it has people to add.
    pub fn open_smallest_group(services: &Services, cx: &mut App) {
        let Some(group) = services
            .provider
            .groups()
            .into_iter()
            .filter(|g| g.kind == crate::data::GroupKind::Group)
            .min_by_key(|g| g.member_ids.len())
        else {
            return;
        };
        let topic = services.provider.topics(&group.id).into_iter().next();
        services.nav.update(cx, |nav, cx| {
            nav.open_group(group.id.clone(), cx);
            if let Some(t) = topic {
                nav.open_topic(group.id.clone(), t.id, cx);
            }
        });
    }

    pub fn add_members(
        services: Services,
        content: Size<Pixels>,
        cx: &mut App,
    ) -> Result<AnyWindowHandle, String> {
        over_main(services, content, cx, |services, window, cx| {
            // The 群 open in the sidebar, as when its menu's 添加成员 is
            // used on the selected row.
            let open = match services.nav.read(cx).place.clone() {
                Some(crate::app::Place::Group(id)) => Some(id),
                _ => None,
            };
            let group = services
                .provider
                .groups()
                .into_iter()
                .filter(|g| g.kind == crate::data::GroupKind::Group)
                .find(|g| Some(&g.id) == open.as_ref());
            if let Some(g) = group {
                super::super::dialogs::open_add_members(services, g.id, g.name, window, cx);
            }
        })
    }

    /// The rename dialog of the first 群's newest 话题.
    pub fn rename_topic(
        services: Services,
        content: Size<Pixels>,
        cx: &mut App,
    ) -> Result<AnyWindowHandle, String> {
        over_main(services, content, cx, |services, window, cx| {
            let group = services
                .provider
                .groups()
                .into_iter()
                .find(|g| g.kind == crate::data::GroupKind::Group);
            if let Some(g) = group
                && let Some(t) = services.provider.topics(&g.id).into_iter().next()
            {
                crate::views::topic_menu::open_rename(services, &g.id, &t, window, cx);
            }
        })
    }

    /// The 群 open in the sidebar, or the first 群.
    fn open_group(services: &Services, cx: &App) -> Option<crate::data::GroupSummary> {
        let open = match services.nav.read(cx).place.clone() {
            Some(crate::app::Place::Group(id)) => Some(id),
            _ => None,
        };
        let groups: Vec<_> = services
            .provider
            .groups()
            .into_iter()
            .filter(|g| g.kind == crate::data::GroupKind::Group)
            .collect();
        groups
            .iter()
            .find(|g| Some(&g.id) == open.as_ref())
            .or(groups.first())
            .cloned()
    }

    /// The menu of the open 群's sidebar row, opened by a right-click on
    /// the row once it is drawn (the real pointer does not move).
    pub fn group_menu(
        services: Services,
        content: Size<Pixels>,
        cx: &mut App,
    ) -> Result<AnyWindowHandle, String> {
        let group = open_group(&services, cx).ok_or("no 群")?;
        let mut options = crate::app::main_window_options(false, cx);
        options.window_bounds = Some(gpui::WindowBounds::Windowed(
            crate::selftest::scenes::capture_bounds(content, cx),
        ));
        options.window_min_size = None;
        let (handle, root) = gpui_kit::open_window(options, cx, move |window, cx| {
            gpui::AppContext::new(cx, |cx| {
                crate::app::RootView::new(services, None, window, cx)
            })
        })
        .map_err(|e| e.to_string())?;
        // v12/shell: the 群 headers live in the rail now.
        let _ = group;
        let rail = root.read(cx).rail().clone();
        handle
            .update(cx, |_, window, _| right_click_when_drawn(rail, 0, window))
            .map_err(|e| e.to_string())?;
        Ok(handle)
    }

    fn right_click_when_drawn(
        rail: gpui::Entity<crate::views::rail::RailView>,
        tries: u32,
        window: &mut Window,
    ) {
        window.on_next_frame(move |window, cx| {
            let Some(bounds) = rail.read(cx).first_row_bounds(true) else {
                if tries < 10 {
                    right_click_when_drawn(rail, tries + 1, window);
                }
                return;
            };
            let position = bounds.center();
            let modifiers = gpui::Modifiers::default();
            window.dispatch_event(
                gpui::PlatformInput::MouseMove(gpui::MouseMoveEvent {
                    position,
                    pressed_button: None,
                    modifiers,
                }),
                cx,
            );
            window.dispatch_event(
                gpui::PlatformInput::MouseDown(gpui::MouseDownEvent {
                    button: gpui::MouseButton::Right,
                    position,
                    modifiers,
                    click_count: 1,
                    first_mouse: false,
                }),
                cx,
            );
            window.dispatch_event(
                gpui::PlatformInput::MouseUp(gpui::MouseUpEvent {
                    button: gpui::MouseButton::Right,
                    position,
                    modifiers,
                    click_count: 1,
                }),
                cx,
            );
            window.refresh();
        });
    }

    /// The 重命名群 dialog of the open 群, its name selected.
    pub fn rename_group(
        services: Services,
        content: Size<Pixels>,
        cx: &mut App,
    ) -> Result<AnyWindowHandle, String> {
        over_main(services, content, cx, |services, window, cx| {
            if let Some(g) = open_group(&services, cx) {
                super::super::rename::open_rename_group(services, g.id, g.name, window, cx);
            }
        })
    }

    /// The 重命名群 dialog after the server refused: a new name typed and
    /// the refusal in an alert over it.
    pub fn rename_group_error(
        services: Services,
        content: Size<Pixels>,
        cx: &mut App,
    ) -> Result<AnyWindowHandle, String> {
        over_main(services, content, cx, |services, window, cx| {
            if let Some(g) = open_group(&services, cx) {
                let typed = format!("{} · 新名称", g.name);
                super::super::rename::open_with(
                    services,
                    g.id,
                    g.name,
                    Some((typed, crate::data::AdminError::Forbidden)),
                    window,
                    cx,
                );
            }
        })
    }

    /// Opens the newest 话题 of a 私聊 whose peer is a bot (`bot`) or a
    /// person, so the header shows the peer's avatar in place of `#`.
    pub fn open_dm_topic(services: &Services, bot: bool, cx: &mut App) {
        let members = services.provider.members();
        let dm = services.provider.groups().into_iter().find(|g| {
            g.kind == crate::data::GroupKind::Dm
                && g.dm_peer
                    .as_ref()
                    .is_some_and(|p| members.iter().any(|m| &m.id == p && m.is_bot() == bot))
                && !services.provider.topics(&g.id).is_empty()
        });
        let Some(dm) = dm else {
            return;
        };
        let topic = services.provider.topics(&dm.id).into_iter().next();
        services.nav.update(cx, |nav, cx| {
            nav.open_group(dm.id.clone(), cx);
            if let Some(t) = topic {
                nav.open_topic(dm.id.clone(), t.id, cx);
            }
        });
    }

    /// The member picker that starts a DM.
    pub fn start_dm(
        services: Services,
        content: Size<Pixels>,
        cx: &mut App,
    ) -> Result<AnyWindowHandle, String> {
        over_main(services, content, cx, super::super::dialogs::open_start_dm)
    }

    /// The alert before leaving the first 群.
    pub fn leave_group(
        services: Services,
        content: Size<Pixels>,
        cx: &mut App,
    ) -> Result<AnyWindowHandle, String> {
        over_main(services, content, cx, |services, window, cx| {
            let group = services
                .provider
                .groups()
                .into_iter()
                .find(|g| g.kind == crate::data::GroupKind::Group);
            if let Some(g) = group {
                super::super::dialogs::confirm_leave(services, g.id, g.name, window, cx);
            }
        })
    }
}
