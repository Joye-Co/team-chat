//! 隐藏 and 删除 of a 群, a 私聊 and a 话题 (docs/protocol.md, "Hide and delete"). The items
//! show only when the server lists `hide-delete/1`. 隐藏 changes nothing for others; 删除 is
//! for everyone, so it reads in the destructive color and asks first (the 离开群 alert).

use gpui::{App, ParentElement as _, Styled as _, Window, div};
use gpui_component::menu::{PopupMenu, PopupMenuItem};

use crate::{
    app::Services,
    copy::{fill, nav, realtime as r},
    data::{GroupKind, GroupSummary, Topic, is_main_topic},
    views::{
        nav::{dialogs, in_main_window},
        settings::{i_am_owner, my_id, widgets::error_text},
    },
};

/// A menu item in the destructive color (§2.11, T4-14); it still asks first.
fn destructive_item(label: &'static str) -> PopupMenuItem {
    PopupMenuItem::element(move |_: &mut Window, cx: &mut App| {
        div()
            .text_color(crate::ui::native::native_colors(cx).red)
            .child(label)
    })
}

/// Whether 删除群… shows: an owner, on a 群 (a 私聊 cannot be deleted). The server refuses
/// the default 群 (全员); the alert says so.
pub fn may_delete_group(services: &Services, g: &GroupSummary) -> bool {
    services.provider.can_hide_delete() && g.kind == GroupKind::Group && i_am_owner(services)
}

/// Whether 删除话题… shows: its creator or an owner, never for the 群聊.
pub fn may_delete_topic(services: &Services, topic: &Topic) -> bool {
    services.provider.can_hide_delete()
        && !is_main_topic(&topic.id)
        && (my_id(services).is_some_and(|me| me == topic.created_by) || i_am_owner(services))
}

/// 隐藏 for a 群 or 私聊 menu; nothing when the server cannot hide.
pub fn hide_group_item(menu: PopupMenu, services: &Services, g: &GroupSummary) -> PopupMenu {
    if !services.provider.can_hide_delete() {
        return menu;
    }
    let (s, id) = (services.clone(), g.id.clone());
    menu.item(PopupMenuItem::new(nav::HIDE).on_click(move |_, _, cx| hide_group(&s, &id, cx)))
}

/// 删除群… for an owner's 群 menu; nothing otherwise.
pub fn delete_group_item(menu: PopupMenu, services: &Services, g: &GroupSummary) -> PopupMenu {
    if !may_delete_group(services, g) {
        return menu;
    }
    let (s, id, name) = (services.clone(), g.id.clone(), g.name.clone());
    menu.item(
        destructive_item(nav::DELETE_GROUP).on_click(move |_, window, cx| {
            confirm_delete_group(s.clone(), id.clone(), name.clone(), window, cx)
        }),
    )
}

/// 隐藏话题 and 删除话题… for a 话题's menu (not the 群聊), after a separator.
pub fn topic_items(
    mut menu: PopupMenu,
    services: &Services,
    group_id: &str,
    topic: &Topic,
) -> PopupMenu {
    if !services.provider.can_hide_delete() || is_main_topic(&topic.id) {
        return menu;
    }
    let (s, g, t) = (services.clone(), group_id.to_string(), topic.id.clone());
    menu = menu.separator().item(
        PopupMenuItem::new(r::TOPIC_HIDE).on_click(move |_, _, _| s.provider.hide_topic(&g, &t)),
    );
    if may_delete_topic(services, topic) {
        let (s, g, tp) = (services.clone(), group_id.to_string(), topic.clone());
        menu = menu.item(
            destructive_item(r::TOPIC_DELETE).on_click(move |_, window, cx| {
                confirm_delete_topic(s.clone(), &g, &tp, window, cx)
            }),
        );
    }
    menu
}

/// Hides a 群 or 私聊 for me and moves away from it when it is open. A refusal shows an
/// alert.
pub fn hide_group(services: &Services, group_id: &str, cx: &mut App) {
    dialogs::move_away_from(services, group_id, cx);
    let reply = services.provider.set_group_hidden(group_id, true);
    cx.spawn(async move |cx: &mut gpui::AsyncApp| {
        if let Ok(Err(e)) = reply.await {
            cx.update(|cx| refused(nav::HIDE_GROUP_REFUSED, e, cx));
        }
    })
    .detach();
}

/// Asks before deleting a 群 for everyone; on 删除 deletes it and moves away from it.
pub fn confirm_delete_group(
    services: Services,
    group_id: String,
    name: String,
    window: &mut Window,
    cx: &mut App,
) {
    dialogs::destructive_alert(
        "delete-group",
        fill(
            nav::DELETE_GROUP_CONFIRM_TITLE,
            &[("name", &dialogs::short_name(&name))],
        ),
        nav::DELETE_GROUP_CONFIRM_BODY.to_string(),
        nav::DELETE_OK,
        move |_, cx| delete_group(&services, &group_id, cx),
        window,
        cx,
    );
}

fn delete_group(services: &Services, group_id: &str, cx: &mut App) {
    let reply = services.provider.delete_group(group_id);
    let (services, group_id) = (services.clone(), group_id.to_string());
    cx.spawn(async move |cx: &mut gpui::AsyncApp| {
        let result = reply.await.unwrap_or(Err(crate::data::AdminError::Other));
        cx.update(|cx| match result {
            Ok(()) => dialogs::move_away_from(&services, &group_id, cx),
            // An owner sees the item, so `forbidden` is the default 群 (全员).
            Err(crate::data::AdminError::Forbidden) => in_main_window(cx, |window, cx| {
                dialogs::error_alert(
                    nav::DELETE_GROUP_REFUSED,
                    nav::DELETE_GROUP_FORBIDDEN,
                    |_, _| {},
                    window,
                    cx,
                )
            }),
            Err(e) => refused(nav::DELETE_GROUP_REFUSED, e, cx),
        });
    })
    .detach();
}

/// Asks before deleting a 话题 for everyone. When it is gone, a window that shows it goes
/// back to the 群聊 (`Change::TopicDeleted`).
pub fn confirm_delete_topic(
    services: Services,
    group_id: &str,
    topic: &Topic,
    window: &mut Window,
    cx: &mut App,
) {
    let (g, t) = (group_id.to_string(), topic.id.clone());
    dialogs::destructive_alert(
        "delete-topic",
        fill(
            r::TOPIC_DELETE_CONFIRM_TITLE,
            &[("name", &dialogs::short_name(&topic.title))],
        ),
        r::TOPIC_DELETE_CONFIRM_BODY.to_string(),
        r::TOPIC_DELETE_OK,
        move |_, _| services.provider.delete_topic(&g, &t),
        window,
        cx,
    );
}

/// A refused hide or delete: an alert with the reason (a refusal blocks, no toast).
fn refused(title: &'static str, e: crate::data::AdminError, cx: &mut App) {
    in_main_window(cx, move |window, cx| {
        dialogs::error_alert(title, error_text(e), |_, _| {}, window, cx)
    });
}
