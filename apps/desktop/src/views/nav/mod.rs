//! Navigation: key bindings and global actions (quick switcher, new group,
//! start DM, next or previous unread topic, Cmd+1..9), the app menus, the menu
//! bar item, and the place of the main window remembered across restarts.
//!
//! The actions are global (`cx.on_action`), so they work wherever the focus
//! is. Dialogs open in the main window, which `RootView::new` registers with
//! [`attach_window`].

pub mod ask_scenes;
pub mod dialogs;
pub mod menus;
pub mod rename;
pub mod selftest;
#[cfg(target_os = "macos")]
mod status_item;
pub mod switcher;
// --- v12/overlays ---
pub mod new_topic;
// --- end v12/overlays ---

use std::collections::HashSet;

use gpui::{AnyWindowHandle, App, Global, KeyBinding, Window};
use gpui_component::WindowExt as _;

use crate::{
    app::{Changes, Place, Services},
    data::{
        Change, CreateGroupRequest, GroupId, GroupKind, GroupSummary, MemberId, SavedNavigation,
        TopicId,
    },
};

gpui::actions!(
    nav,
    [
        QuickSwitch,
        NewGroup,
        StartDm,
        NextUnreadTopic,
        PreviousUnreadTopic,
        OpenGroup1,
        OpenGroup2,
        OpenGroup3,
        OpenGroup4,
        OpenGroup5,
        OpenGroup6,
        OpenGroup7,
        OpenGroup8,
        OpenGroup9,
        About,
        ShowMainWindow,
    ]
);

struct NavGlobal {
    services: Services,
    window: Option<AnyWindowHandle>,
    last_saved: Option<SavedNavigation>,
    /// A group the user asked for that the provider has not delivered yet.
    pending: Option<Pending>,
}

/// Opens a group as soon as it arrives in `Change::Groups`.
enum Pending {
    Dm(MemberId),
    Group {
        name: String,
        known: HashSet<GroupId>,
    },
}

impl Global for NavGlobal {}

/// Key bindings, global actions, the save-on-change observer and the menu bar
/// item. Call once after `gpui_kit::init`. `status_item` is false for self-tests.
pub fn install(services: Services, status_item: bool, cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("cmd-k", QuickSwitch, None),
        KeyBinding::new("cmd-shift-n", NewGroup, None),
        KeyBinding::new("alt-down", NextUnreadTopic, None),
        KeyBinding::new("alt-up", PreviousUnreadTopic, None),
        KeyBinding::new("cmd-1", OpenGroup1, None),
        KeyBinding::new("cmd-2", OpenGroup2, None),
        KeyBinding::new("cmd-3", OpenGroup3, None),
        KeyBinding::new("cmd-4", OpenGroup4, None),
        KeyBinding::new("cmd-5", OpenGroup5, None),
        KeyBinding::new("cmd-6", OpenGroup6, None),
        KeyBinding::new("cmd-7", OpenGroup7, None),
        KeyBinding::new("cmd-8", OpenGroup8, None),
        KeyBinding::new("cmd-9", OpenGroup9, None),
    ]);
    switcher::bind_keys(cx);
    crate::views::ask::bind_keys(cx);
    // v12/overlays: ⌘Return in the 新话题 sheet.
    new_topic::bind_keys(cx);
    cx.set_global(NavGlobal {
        services: services.clone(),
        window: None,
        last_saved: None,
        pending: None,
    });
    cx.subscribe(&services.hub, |_, changes: &Changes, cx| {
        if changes.any(|c| matches!(c, Change::Groups)) {
            open_pending(cx);
        }
    })
    .detach();

    cx.on_action(|_: &QuickSwitch, cx| {
        in_main_window(cx, |window, cx| {
            if !window.has_active_dialog(cx) {
                // 1.1.0: ⌘K is 交代或搜索 (the rail track's `views::ask`).
                crate::views::ask::open(services_of(cx), window, cx);
            }
        });
    });
    cx.on_action(|_: &NewGroup, cx| {
        in_main_window(cx, |window, cx| {
            if !window.has_active_dialog(cx) {
                dialogs::open_new_group(services_of(cx), window, cx);
            }
        });
    });
    cx.on_action(|_: &StartDm, cx| {
        in_main_window(cx, |window, cx| {
            if !window.has_active_dialog(cx) {
                dialogs::open_start_dm(services_of(cx), window, cx);
            }
        });
    });
    cx.on_action(|_: &NextUnreadTopic, cx| go_to_unread(true, cx));
    cx.on_action(|_: &PreviousUnreadTopic, cx| go_to_unread(false, cx));
    cx.on_action(|_: &OpenGroup1, cx| open_group_at(0, cx));
    cx.on_action(|_: &OpenGroup2, cx| open_group_at(1, cx));
    cx.on_action(|_: &OpenGroup3, cx| open_group_at(2, cx));
    cx.on_action(|_: &OpenGroup4, cx| open_group_at(3, cx));
    cx.on_action(|_: &OpenGroup5, cx| open_group_at(4, cx));
    cx.on_action(|_: &OpenGroup6, cx| open_group_at(5, cx));
    cx.on_action(|_: &OpenGroup7, cx| open_group_at(6, cx));
    cx.on_action(|_: &OpenGroup8, cx| open_group_at(7, cx));
    cx.on_action(|_: &OpenGroup9, cx| open_group_at(8, cx));
    cx.on_action(|_: &About, cx| menus::show_about(cx));
    cx.on_action(|_: &ShowMainWindow, cx| show_main_window(cx));

    // Remember the place of the main window through the provider.
    cx.observe(&services.nav, |nav, cx| {
        let nav = nav.read(cx);
        let saved = saved_from(nav.place.as_ref(), nav.topic.as_ref());
        let global = cx.global_mut::<NavGlobal>();
        if global.last_saved.as_ref() != Some(&saved) {
            global.last_saved = Some(saved.clone());
            global.services.provider.save_navigation(saved);
        }
    })
    .detach();

    #[cfg(target_os = "macos")]
    if status_item {
        status_item::install(services, cx);
    }
    #[cfg(not(target_os = "macos"))]
    let _ = status_item;
}

/// For the self-test: the menu bar item is installed and up to date.
pub fn status_item_ok(services: &Services, cx: &App) -> bool {
    #[cfg(target_os = "macos")]
    return status_item::check(services, cx);
    #[cfg(not(target_os = "macos"))]
    return true;
}

/// Registers the main window, where the navigation dialogs open.
pub fn attach_window(window: &Window, cx: &mut App) {
    if cx.has_global::<NavGlobal>() {
        cx.global_mut::<NavGlobal>().window = Some(window.window_handle());
    }
}

pub(crate) fn services_of(cx: &App) -> Services {
    cx.global::<NavGlobal>().services.clone()
}

/// Runs `f` in the main window soon. Actions arrive while a window is being
/// updated, and a window cannot be updated again from inside, so this defers.
pub(crate) fn in_main_window(cx: &mut App, f: impl FnOnce(&mut Window, &mut App) + 'static) {
    cx.defer(move |cx| {
        with_main_window(cx, f);
    });
}

/// Runs `f` in the main window. False when there is no main window.
fn with_main_window(cx: &mut App, f: impl FnOnce(&mut Window, &mut App)) -> bool {
    let Some(handle) = cx.try_global::<NavGlobal>().and_then(|g| g.window) else {
        return false;
    };
    handle.update(cx, |_, window, cx| f(window, cx)).is_ok()
}

/// Brings the main window to the front, or opens it again after it was closed.
pub fn show_main_window(cx: &mut App) {
    cx.defer(show_main_window_now);
}

fn show_main_window_now(cx: &mut App) {
    cx.activate(true);
    if !with_main_window(cx, |window, _| window.activate_window()) {
        let services = services_of(cx);
        if let Err(error) = crate::app::open_main_window(services, true, None, cx) {
            eprintln!("failed to open the window: {error}");
        }
    }
}

/// The place to save for this navigation state.
pub fn saved_from(place: Option<&Place>, topic: Option<&(GroupId, TopicId)>) -> SavedNavigation {
    let todo = place == Some(&Place::Todo);
    let group_id = match (topic, place) {
        (Some((g, _)), _) => Some(g.clone()),
        (None, Some(Place::Group(g))) => Some(g.clone()),
        _ => None,
    };
    // v2 pages (additive): a 1.0.x client reads only the fields above.
    let (page, member_id, artifact_id) = match place {
        Some(Place::Desk) if topic.is_none() => (Some("desk"), None, None),
        Some(Place::Members(m)) => (Some("members"), m.clone(), None),
        Some(Place::Artifacts) => (Some("artifacts"), None, None),
        Some(Place::Artifact { id, .. }) => (Some("artifact"), None, Some(id.clone())),
        _ => (None, None, None),
    };
    let topic = topic.filter(|_| page.is_none());
    SavedNavigation {
        todo,
        group_id,
        topic_id: topic.map(|(_, t)| t.clone()),
        page: page.map(str::to_string),
        member_id,
        artifact_id,
    }
}

/// The v2 page a saved place names, when it is not a group or 话题 page.
pub fn saved_page(saved: &SavedNavigation) -> Option<Place> {
    match saved.page.as_deref()? {
        "desk" => Some(Place::Desk),
        "members" => Some(Place::Members(saved.member_id.clone())),
        "artifacts" => Some(Place::Artifacts),
        "artifact" => saved
            .artifact_id
            .clone()
            .map(|id| Place::Artifact { id, version: None }),
        _ => None,
    }
}

/// Where to start: the saved place when it still exists, else the first group.
/// Returns (todo, group, topic).
pub fn resolve_saved(
    saved: Option<&SavedNavigation>,
    groups: &[GroupSummary],
    topic_exists: impl Fn(&str, &str) -> bool,
) -> (bool, Option<GroupId>, Option<TopicId>) {
    let first = groups.first().map(|g| g.id.clone());
    let Some(saved) = saved else {
        return (false, first, None);
    };
    let group = saved
        .group_id
        .as_ref()
        .filter(|g| groups.iter().any(|x| &x.id == *g))
        .cloned();
    let topic = match (&group, &saved.topic_id) {
        (Some(g), Some(t)) if topic_exists(g, t) => Some(t.clone()),
        _ => None,
    };
    if saved.todo {
        (true, group.filter(|_| topic.is_some()), topic)
    } else {
        (false, group.or(first), topic)
    }
}

/// Opens the saved place of the main window (call before opening it).
pub fn restore(services: &Services, cx: &mut App) {
    let provider = services.provider.clone();
    let saved = provider.saved_navigation();
    let groups = provider.groups();
    let (todo, group, topic) = resolve_saved(saved.as_ref(), &groups, |g, t| {
        provider.topic(g, t).is_some()
    });
    let page = saved.as_ref().and_then(saved_page);
    services.nav.update(cx, |nav, cx| {
        match page {
            Some(Place::Desk) => return nav.open_desk(cx),
            Some(Place::Members(m)) => return nav.open_members(m, cx),
            Some(Place::Artifacts) => return nav.open_artifacts(cx),
            Some(Place::Artifact { id, version }) => return nav.open_artifact(id, version, cx),
            _ => {}
        }
        if todo {
            nav.open_todo(cx);
        } else if let Some(g) = group.clone() {
            nav.open_group(g, cx);
        }
        if let (Some(g), Some(t)) = (group, topic) {
            nav.open_topic(g, t, cx);
        }
    });
    let nav = services.nav.read(cx);
    let saved_now = saved_from(nav.place.as_ref(), nav.topic.as_ref());
    if cx.has_global::<NavGlobal>() {
        cx.global_mut::<NavGlobal>().last_saved = Some(saved_now);
    }
}

fn dm_with(groups: &[GroupSummary], member_id: &str) -> Option<GroupId> {
    groups
        .iter()
        .find(|g| g.kind == GroupKind::Dm && g.dm_peer.as_deref() == Some(member_id))
        .map(|g| g.id.clone())
}

/// Opens a topic in the main window (a link in a card). Does nothing before
/// [`install`].
pub fn open_topic(group_id: GroupId, topic_id: TopicId, cx: &mut App) {
    if cx.try_global::<NavGlobal>().is_none() {
        return;
    }
    let services = services_of(cx);
    services
        .nav
        .update(cx, |nav, cx| nav.open_topic(group_id, topic_id, cx));
}

/// Opens the DM with a member, asking the provider to create it when needed.
pub fn open_dm_with(member_id: MemberId, cx: &mut App) {
    let services = services_of(cx);
    if let Some(g) = dm_with(&services.provider.groups(), &member_id) {
        services.nav.update(cx, |nav, cx| nav.open_group(g, cx));
        return;
    }
    cx.global_mut::<NavGlobal>().pending = Some(Pending::Dm(member_id.clone()));
    let reply = services.provider.open_dm(&member_id);
    cx.spawn(async move |cx: &mut gpui::AsyncApp| {
        let result = reply.await.unwrap_or(Err(crate::data::AdminError::Other));
        if let Err(e) = result {
            cx.update(|cx| {
                clear_pending(cx);
                crate::views::files::notify_error(
                    crate::copy::fill(
                        crate::copy::nav::DM_FAILED,
                        &[("reason", crate::views::settings::widgets::error_text(e))],
                    ),
                    cx,
                );
            });
        }
    })
    .detach();
}

/// Forgets the group or DM that was to open when it arrives (its request failed).
pub fn clear_pending(cx: &mut App) {
    if cx.try_global::<NavGlobal>().is_some() {
        cx.global_mut::<NavGlobal>().pending = None;
    }
}

/// Creates a group and opens it when it arrives. The caller shows a refusal (and then calls
/// [`clear_pending`]).
pub fn create_group(
    request: CreateGroupRequest,
    cx: &mut App,
) -> crate::data::AdminReply<crate::data::GroupId> {
    let services = services_of(cx);
    let known = services
        .provider
        .groups()
        .into_iter()
        .map(|g| g.id)
        .collect();
    cx.global_mut::<NavGlobal>().pending = Some(Pending::Group {
        name: request.name.trim().to_string(),
        known,
    });
    services.provider.create_group(request)
}

fn open_pending(cx: &mut App) {
    let Some(pending) = cx.global_mut::<NavGlobal>().pending.take() else {
        return;
    };
    let services = services_of(cx);
    let groups = services.provider.groups();
    let found = match &pending {
        Pending::Dm(member) => dm_with(&groups, member),
        Pending::Group { name, known } => groups
            .iter()
            .find(|g| g.kind == GroupKind::Group && !known.contains(&g.id) && &g.name == name)
            .map(|g| g.id.clone()),
    };
    match found {
        Some(g) => services.nav.update(cx, |nav, cx| nav.open_group(g, cx)),
        None => cx.global_mut::<NavGlobal>().pending = Some(pending),
    }
}

fn open_group_at(index: usize, cx: &mut App) {
    let services = services_of(cx);
    if let Some(group) = services.provider.groups().into_iter().nth(index) {
        services
            .nav
            .update(cx, |nav, cx| nav.open_group(group.id, cx));
    }
}

/// One topic in the order Option+Up and Option+Down walk.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WalkTopic {
    pub group_id: GroupId,
    pub topic_id: TopicId,
    pub unread: bool,
}

/// The next (or previous) unread topic after `current`, wrapping around.
/// `order` is the sidebar order of groups, and in each group the topic list
/// order. Without a current topic, the walk starts before the first topic of
/// `current_group` (or at the start).
pub fn unread_target(
    order: &[WalkTopic],
    current: Option<(&str, &str)>,
    current_group: Option<&str>,
    forward: bool,
) -> Option<(GroupId, TopicId)> {
    if order.is_empty() {
        return None;
    }
    let n = order.len();
    let at = current.and_then(|(g, t)| {
        order
            .iter()
            .position(|w| w.group_id == g && w.topic_id == t)
    });
    // `start` is the index the walk looks at first.
    let start = match at {
        Some(i) if forward => (i + 1) % n,
        Some(i) => (i + n - 1) % n,
        None => {
            let group_start = current_group
                .and_then(|g| order.iter().position(|w| w.group_id == g))
                .unwrap_or(0);
            if forward {
                group_start
            } else {
                (group_start + n - 1) % n
            }
        }
    };
    (0..n)
        .map(|step| {
            if forward {
                (start + step) % n
            } else {
                (start + n - step) % n
            }
        })
        .find(|&i| order[i].unread && Some(i) != at)
        .map(|i| (order[i].group_id.clone(), order[i].topic_id.clone()))
}

fn go_to_unread(forward: bool, cx: &mut App) {
    let services = services_of(cx);
    let provider = services.provider.clone();
    let mut order = Vec::new();
    for group in provider.groups() {
        if provider.group_muted(&group.id) {
            continue;
        }
        for topic in provider.topics(&group.id) {
            order.push(WalkTopic {
                group_id: group.id.clone(),
                topic_id: topic.id,
                unread: topic.unread > 0,
            });
        }
    }
    let nav = services.nav.read(cx);
    let current = nav.topic.clone();
    let current_group = nav.current_group();
    let target = unread_target(
        &order,
        current.as_ref().map(|(g, t)| (g.as_str(), t.as_str())),
        current_group.as_deref(),
        forward,
    );
    if let Some((g, t)) = target {
        services.nav.update(cx, |nav, cx| {
            // Leave the 待办 list for the topic's group, so the topic list shows it.
            nav.open_group(g.clone(), cx);
            nav.open_topic(g, t, cx);
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::{ChatProvider, GroupKind};
    use crate::fake::FakeProvider;

    fn walk(items: &[(&str, &str, bool)]) -> Vec<WalkTopic> {
        items
            .iter()
            .map(|(g, t, u)| WalkTopic {
                group_id: g.to_string(),
                topic_id: t.to_string(),
                unread: *u,
            })
            .collect()
    }

    fn target(s: Option<(String, String)>) -> Option<(&'static str, &'static str)> {
        s.map(|(g, t)| {
            (
                Box::leak(g.into_boxed_str()) as &str,
                Box::leak(t.into_boxed_str()) as &str,
            )
        })
    }

    #[test]
    fn unread_walk_moves_forward_and_back_with_wrap() {
        let order = walk(&[
            ("a", "1", false),
            ("a", "2", true),
            ("b", "3", false),
            ("b", "4", true),
        ]);
        assert_eq!(
            target(unread_target(&order, Some(("a", "1")), None, true)),
            Some(("a", "2"))
        );
        assert_eq!(
            target(unread_target(&order, Some(("a", "2")), None, true)),
            Some(("b", "4"))
        );
        assert_eq!(
            target(unread_target(&order, Some(("b", "4")), None, true)),
            Some(("a", "2"))
        );
        assert_eq!(
            target(unread_target(&order, Some(("a", "2")), None, false)),
            Some(("b", "4"))
        );
        assert_eq!(
            target(unread_target(&order, Some(("b", "3")), None, false)),
            Some(("a", "2"))
        );
        // No topic open: start at the current group.
        assert_eq!(
            target(unread_target(&order, None, Some("b"), true)),
            Some(("b", "4"))
        );
        assert_eq!(
            target(unread_target(&order, None, None, true)),
            Some(("a", "2"))
        );
    }

    #[test]
    fn unread_walk_skips_the_current_topic_and_stops_without_unread() {
        let order = walk(&[("a", "1", true), ("a", "2", false)]);
        // The only unread topic is open: nothing else to go to.
        assert_eq!(unread_target(&order, Some(("a", "1")), None, true), None);
        let none = walk(&[("a", "1", false)]);
        assert_eq!(unread_target(&none, None, None, true), None);
        assert_eq!(unread_target(&[], None, None, false), None);
    }

    #[test]
    fn saved_place_round_trips_and_falls_back() {
        let p = FakeProvider::new();
        let groups = p.groups();
        let exists = |g: &str, t: &str| p.topic(g, t).is_some();
        // Nothing saved: the first group.
        assert_eq!(
            resolve_saved(None, &groups, exists),
            (false, Some(groups[0].id.clone()), None)
        );
        // A group and topic that exist.
        let saved = saved_from(
            Some(&Place::Group("g-dev".into())),
            Some(&("g-dev".into(), "t-api".into())),
        );
        assert_eq!(
            resolve_saved(Some(&saved), &groups, exists),
            (false, Some("g-dev".into()), Some("t-api".into()))
        );
        // 待办 with a topic.
        let saved = saved_from(Some(&Place::Todo), Some(&("g-dev".into(), "t-api".into())));
        assert!(saved.todo);
        assert_eq!(
            resolve_saved(Some(&saved), &groups, exists),
            (true, Some("g-dev".into()), Some("t-api".into()))
        );
        // A topic that is gone keeps the group.
        let saved = saved_from(
            Some(&Place::Group("g-dev".into())),
            Some(&("g-dev".into(), "t-gone".into())),
        );
        assert_eq!(
            resolve_saved(Some(&saved), &groups, exists),
            (false, Some("g-dev".into()), None)
        );
        // A group that is gone falls back to the first group.
        let saved = saved_from(Some(&Place::Group("g-gone".into())), None);
        assert_eq!(
            resolve_saved(Some(&saved), &groups, exists),
            (false, Some(groups[0].id.clone()), None)
        );
        // The provider keeps it.
        p.save_navigation(saved.clone());
        assert_eq!(p.saved_navigation(), Some(saved));
    }

    #[test]
    fn fake_group_commands_work() {
        let p = FakeProvider::new();
        let before = p.groups().len();
        drop(p.create_group(crate::data::CreateGroupRequest {
            name: "新群".into(),
            member_ids: vec!["m-alex".into()],
        }));
        let groups = p.groups();
        assert_eq!(groups.len(), before + 1);
        let new = groups.iter().find(|g| g.name == "新群").unwrap();
        assert!(new.member_ids.contains(&crate::fake::ME.to_string()));
        // A member can be added to an existing 群 later.
        let other = p
            .members()
            .into_iter()
            .find(|m| !new.member_ids.contains(&m.id) && !m.deactivated)
            .unwrap();
        let mut added = p.add_group_members(&new.id, vec![other.id.clone()]);
        assert_eq!(added.try_recv().unwrap(), Some(Ok(())));
        let new = p.groups().into_iter().find(|g| g.name == "新群").unwrap();
        assert!(new.member_ids.contains(&other.id));
        let mut gone = p.add_group_members("g-gone", vec![other.id.clone()]);
        assert_eq!(
            gone.try_recv().unwrap(),
            Some(Err(crate::data::AdminError::NotFound))
        );
        p.set_group_muted(&new.id, true);
        assert!(p.group_muted(&new.id));
        p.set_group_muted(&new.id, false);
        assert!(!p.group_muted(&new.id));
        p.leave_group(&new.id);
        assert_eq!(p.groups().len(), before);
        // A DM with a member who has none yet is created once.
        let peer = p
            .members()
            .into_iter()
            .find(|m| {
                m.id != crate::fake::ME
                    && !p
                        .groups()
                        .iter()
                        .any(|g| g.dm_peer.as_deref() == Some(m.id.as_str()))
            })
            .unwrap();
        drop(p.open_dm(&peer.id));
        drop(p.open_dm(&peer.id));
        let dms = p
            .groups()
            .into_iter()
            .filter(|g| g.kind == GroupKind::Dm && g.dm_peer.as_deref() == Some(peer.id.as_str()))
            .count();
        assert_eq!(dms, 1);
    }
}
