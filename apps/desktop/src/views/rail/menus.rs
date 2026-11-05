//! Context menus of the rail (NATIVE.md §7.3, §10): a right click on a 话题,
//! a 群 header or a DM opens the commands that the row's page already has.
//! No keyboard shortcuts show in these menus.

use gpui::{Context, Window};
use gpui_component::menu::{PopupMenu, PopupMenuItem};

use crate::{
    app::Services,
    copy::nav,
    data::{GroupKind, Topic},
    views::{nav as nav_views, topic_menu},
};

/// The menu of a 话题 row: the topic menu of the topic header (rename,
/// status, waiting on, owner, follow), read fresh when it opens.
pub(super) fn topic(
    menu: PopupMenu,
    services: &Services,
    topic: &Topic,
    window: &mut Window,
    cx: &mut Context<PopupMenu>,
) -> PopupMenu {
    let fresh = services
        .provider
        .topic(&topic.group_id, &topic.id)
        .unwrap_or_else(|| topic.clone());
    topic_menu::build(menu, services, &topic.group_id, &fresh, window, cx)
}

/// The menu of a 群 header or a DM row: 静音 or 取消静音 and the 提醒
/// submenu (this conversation's own notify level); for a 群 also
/// 重命名群…, 添加成员 and, apart, 离开群 (it asks first).
pub(super) fn group(
    menu: PopupMenu,
    services: &Services,
    group_id: &str,
    window: &mut Window,
    cx: &mut Context<PopupMenu>,
) -> PopupMenu {
    let p = &services.provider;
    let Some(g) = p.groups().into_iter().find(|g| g.id == group_id) else {
        return menu;
    };
    let menu = nav_views::notify::items(menu, services, &g.id, window, cx);
    if g.kind != GroupKind::Group {
        return menu;
    }
    let menu = menu.separator();
    let (rename, add, leave) = (services.clone(), services.clone(), services.clone());
    let (g1, g2, g3) = (g.clone(), g.clone(), g);
    menu.item(
        PopupMenuItem::new(nav::RENAME_GROUP).on_click(move |_, window, cx| {
            nav_views::rename::open_rename_group(
                rename.clone(),
                g1.id.clone(),
                g1.name.clone(),
                window,
                cx,
            )
        }),
    )
    .item(
        PopupMenuItem::new(nav::ADD_MEMBERS).on_click(move |_, window, cx| {
            nav_views::dialogs::open_add_members(
                add.clone(),
                g2.id.clone(),
                g2.name.clone(),
                window,
                cx,
            )
        }),
    )
    .separator()
    .item(
        // T4-14: a destructive item reads in the destructive color (the
        // Mac menu's red item), and it still asks first.
        PopupMenuItem::element(|_: &mut gpui::Window, cx: &mut gpui::App| {
            use gpui::{ParentElement as _, Styled as _};
            gpui::div()
                .text_color(crate::ui::native::native_colors(cx).red)
                .child(nav::LEAVE_GROUP)
        })
        .on_click(move |_, window, cx| {
            nav_views::dialogs::confirm_leave(
                leave.clone(),
                g3.id.clone(),
                g3.name.clone(),
                window,
                cx,
            )
        }),
    )
}
