//! The left sidebar: the message search field with a quick switcher button, 待办, then
//! one list of 群 (groups and DMs together) sorted by last activity, with
//! unread and mention badges, a menu to start a group or a DM, a context menu
//! per row (mute, leave), and the connection line at the bottom.

use std::collections::HashSet;

use gpui::{
    Context, ElementId, InteractiveElement, IntoElement, ParentElement, Render, SharedString,
    StatefulInteractiveElement, Styled, Subscription, Window, div, prelude::FluentBuilder as _, px,
};
use gpui_component::{
    IconName, Sizable as _,
    button::{Button, ButtonVariants as _},
    menu::{ContextMenuExt as _, DropdownMenu as _, PopupMenuItem},
};

use crate::{
    app::{Changes, Place, Services},
    copy::{common, nav, search as search_copy},
    data::{Change, ConnectionState, GroupId, GroupKind, GroupSummary, Member},
    theme,
    views::{
        avatar, count_badge,
        nav::{NewGroup, QuickSwitch, StartDm, dialogs},
        presence_dot,
        search::SearchEverywhere,
    },
};

pub struct Sidebar {
    services: Services,
    groups: Vec<GroupSummary>,
    members: Vec<Member>,
    muted: HashSet<GroupId>,
    todo_count: usize,
    _subscriptions: Vec<Subscription>,
}

impl Sidebar {
    pub fn new(services: Services, cx: &mut Context<Self>) -> Self {
        let subscriptions = vec![
            cx.subscribe(&services.hub, |this, _, changes: &Changes, cx| {
                if changes.any(|c| {
                    matches!(
                        c,
                        Change::Groups
                            | Change::Members
                            | Change::Todo
                            | Change::Connection
                            | Change::Session
                    )
                }) {
                    this.refresh();
                    cx.notify();
                }
            }),
            cx.observe(&services.nav, |_, _, cx| cx.notify()),
        ];
        let mut this = Self {
            services,
            groups: Vec::new(),
            members: Vec::new(),
            muted: HashSet::new(),
            todo_count: 0,
            _subscriptions: subscriptions,
        };
        this.refresh();
        this
    }

    fn refresh(&mut self) {
        let p = &self.services.provider;
        self.groups = p.groups();
        self.members = p.members();
        self.muted = self
            .groups
            .iter()
            .filter(|g| p.group_muted(&g.id))
            .map(|g| g.id.clone())
            .collect();
        self.todo_count = p.todo().len();
    }

    pub fn group_count(&self) -> usize {
        self.groups.len()
    }

    fn group_row(
        &self,
        group: &GroupSummary,
        selected: bool,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let colors = theme::colors(cx);
        let id = group.id.clone();
        let muted = self.muted.contains(&group.id);
        let peer = group
            .dm_peer
            .as_ref()
            .and_then(|p| self.members.iter().find(|m| &m.id == p));
        let bold = group.unread > 0 && !muted;
        let menu_services = self.services.clone();
        let menu_group = group.clone();
        div()
            .id(ElementId::Name(SharedString::from(format!(
                "group-{}",
                group.id
            ))))
            .h(theme::ROW_HEIGHT)
            .mx(px(8.))
            .px(px(8.))
            .rounded(px(6.))
            .flex()
            .items_center()
            .gap(px(8.))
            .when(selected, |el| el.bg(colors.row_selected))
            .hover(|el| el.bg(colors.row_hover))
            .when(group.kind == GroupKind::Dm, |el| {
                el.child(
                    div()
                        .relative()
                        .flex_none()
                        .child(avatar(peer, px(20.), cx))
                        .when_some(peer, |el, p| {
                            el.child(
                                div()
                                    .absolute()
                                    .bottom(px(-1.))
                                    .left(px(-1.))
                                    .child(presence_dot(p.online, cx)),
                            )
                        }),
                )
            })
            .when(group.kind == GroupKind::Group, |el| {
                el.child(
                    div()
                        .w(px(20.))
                        .flex_none()
                        .text_color(colors.muted_foreground)
                        .child(nav::GROUP_MARK),
                )
            })
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.))
                    .overflow_hidden()
                    .text_ellipsis()
                    .whitespace_nowrap()
                    .when(bold, |el| el.font_weight(gpui::FontWeight::SEMIBOLD))
                    .when(muted, |el| el.text_color(colors.muted_foreground))
                    .child(group.name.clone()),
            )
            .when(muted, |el| {
                el.child(
                    div()
                        .flex_none()
                        .text_xs()
                        .text_color(colors.muted_foreground)
                        .child(nav::MUTED),
                )
            })
            .when_some(count_badge(group.mentions, true, cx), |el, b| el.child(b))
            .when(!muted, |el| {
                el.when_some(count_badge(group.unread, false, cx), |el, b| el.child(b))
            })
            .on_click(cx.listener(move |this, _, _, cx| {
                let id = id.clone();
                this.services
                    .nav
                    .update(cx, |nav, cx| nav.open_group(id, cx));
            }))
            .context_menu(move |menu, _, _| {
                let services = menu_services.clone();
                let g = menu_group.clone();
                let mute_label = if muted { nav::UNMUTE } else { nav::MUTE };
                let mute_services = services.clone();
                let mute_id = g.id.clone();
                let menu = menu.item(PopupMenuItem::new(mute_label).on_click(move |_, _, _| {
                    mute_services.provider.set_group_muted(&mute_id, !muted);
                }));
                if g.kind == GroupKind::Group {
                    menu.separator()
                        .item(PopupMenuItem::new(nav::LEAVE_GROUP).on_click(
                            move |_, window, cx| {
                                dialogs::confirm_leave(
                                    services.clone(),
                                    g.id.clone(),
                                    g.name.clone(),
                                    window,
                                    cx,
                                );
                            },
                        ))
                } else {
                    menu
                }
            })
    }
}

impl Render for Sidebar {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let colors = theme::colors(cx);
        let place = self.services.nav.read(cx).place.clone();
        let rows: Vec<_> = self
            .groups
            .iter()
            .map(|g| {
                self.group_row(g, place == Some(Place::Group(g.id.clone())), cx)
                    .into_any_element()
            })
            .collect();
        let connection = match self.services.provider.connection() {
            ConnectionState::Online => None,
            ConnectionState::Connecting => Some(common::CONNECTING),
            ConnectionState::Reconnecting { .. } => Some(common::RECONNECTING),
            ConnectionState::Offline => Some(common::CONNECTION_OFFLINE),
        };
        let todo_count = self.todo_count as u32;
        let empty = rows.is_empty();

        let header = div()
            .px(px(16.))
            .pt(px(12.))
            .pb(px(4.))
            .flex()
            .items_center()
            .child(
                div()
                    .flex_1()
                    .text_xs()
                    .text_color(colors.muted_foreground)
                    .child(nav::GROUPS),
            )
            .child(
                Button::new("sidebar-new")
                    .ghost()
                    .xsmall()
                    .icon(IconName::Plus)
                    .tooltip(nav::NEW_MENU)
                    .dropdown_menu(|menu, _, _| {
                        menu.menu(nav::NEW_GROUP, Box::new(NewGroup))
                            .menu(nav::START_DM, Box::new(StartDm))
                    }),
            );

        let empty_state = div()
            .px(px(16.))
            .py(px(8.))
            .flex()
            .flex_col()
            .gap(px(8.))
            .child(div().child(nav::NO_GROUPS))
            .child(
                div()
                    .text_xs()
                    .text_color(colors.muted_foreground)
                    .child(nav::NO_GROUPS_HINT),
            )
            .child(
                div()
                    .flex()
                    .gap(px(8.))
                    .child(
                        Button::new("empty-new-group")
                            .small()
                            .primary()
                            .label(nav::NEW_GROUP)
                            .on_click(|_, window, cx| {
                                window.dispatch_action(Box::new(NewGroup), cx)
                            }),
                    )
                    .child(
                        Button::new("empty-start-dm")
                            .small()
                            .label(nav::START_DM)
                            .on_click(|_, window, cx| {
                                window.dispatch_action(Box::new(StartDm), cx)
                            }),
                    ),
            );

        div()
            .size_full()
            .flex()
            .flex_col()
            .bg(colors.sidebar)
            .text_color(colors.sidebar_foreground)
            .text_sm()
            .child(
                div()
                    .m(px(8.))
                    .mt(px(12.))
                    .flex()
                    .items_center()
                    .gap(px(4.))
                    .child(
                        div()
                            .id("sidebar-search")
                            .flex_1()
                            .px(px(8.))
                            .h(px(28.))
                            .rounded(px(6.))
                            .border_1()
                            .border_color(colors.border)
                            .flex()
                            .items_center()
                            .text_color(colors.muted_foreground)
                            .hover(|el| el.bg(colors.row_hover))
                            .child(div().flex_1().child(search_copy::SIDEBAR_FIELD))
                            .child(div().text_xs().child(search_copy::SIDEBAR_SHORTCUT))
                            .on_click(|_, window, cx| {
                                window.dispatch_action(Box::new(SearchEverywhere), cx)
                            }),
                    )
                    .child(
                        Button::new("sidebar-quick-switch")
                            .ghost()
                            .small()
                            .label(nav::QUICK_SWITCH_SHORTCUT)
                            .tooltip(nav::QUICK_SWITCH)
                            .on_click(|_, window, cx| {
                                window.dispatch_action(Box::new(QuickSwitch), cx)
                            }),
                    ),
            )
            .child(
                div()
                    .id("todo")
                    .h(theme::ROW_HEIGHT)
                    .mx(px(8.))
                    .px(px(8.))
                    .rounded(px(6.))
                    .flex()
                    .items_center()
                    .when(place == Some(Place::Todo), |el| el.bg(colors.row_selected))
                    .hover(|el| el.bg(colors.row_hover))
                    .child(div().flex_1().child(nav::TODO))
                    .when_some(count_badge(todo_count, false, cx), |el, b| el.child(b))
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.services.nav.update(cx, |nav, cx| nav.open_todo(cx))
                    })),
            )
            .child(
                div()
                    .id("sidebar-groups")
                    .flex_1()
                    .overflow_y_scroll()
                    .child(header)
                    .when(empty, |el| el.child(empty_state))
                    .children(rows),
            )
            .when_some(connection, |el, text| {
                el.child(
                    div()
                        .px(px(16.))
                        .py(px(8.))
                        .text_xs()
                        .text_color(colors.danger)
                        .child(text),
                )
            })
    }
}
