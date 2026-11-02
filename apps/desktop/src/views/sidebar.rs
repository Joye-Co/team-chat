//! The left sidebar: 待办, then 群 and 私聊 sorted by last activity, with unread
//! and mention badges, and the connection line at the bottom.

use gpui::{
    Context, ElementId, InteractiveElement, IntoElement, ParentElement, Render, SharedString,
    StatefulInteractiveElement, Styled, Subscription, Window, div, prelude::FluentBuilder as _, px,
};

use crate::{
    app::{Changes, Place, Services},
    copy::{common, nav},
    data::{Change, ConnectionState, GroupKind, GroupSummary, Member},
    theme,
    views::{avatar, count_badge, presence_dot},
};

pub struct Sidebar {
    services: Services,
    groups: Vec<GroupSummary>,
    members: Vec<Member>,
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
        self.todo_count = p.todo().len();
    }

    fn group_row(
        &self,
        group: &GroupSummary,
        selected: bool,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let colors = theme::colors(cx);
        let id = group.id.clone();
        let peer = group
            .dm_peer
            .as_ref()
            .and_then(|p| self.members.iter().find(|m| &m.id == p));
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
                el.child(avatar(peer, px(20.), cx))
                    .when_some(peer, |el, p| el.child(presence_dot(p.online, cx)))
            })
            .when(group.kind == GroupKind::Group, |el| {
                el.child(div().text_color(colors.muted_foreground).child("#"))
            })
            .child(
                div()
                    .flex_1()
                    .overflow_hidden()
                    .text_ellipsis()
                    .whitespace_nowrap()
                    .child(group.name.clone()),
            )
            .when_some(count_badge(group.mentions, true, cx), |el, b| el.child(b))
            .when_some(count_badge(group.unread, false, cx), |el, b| el.child(b))
            .on_click(cx.listener(move |this, _, _, cx| {
                let id = id.clone();
                this.services
                    .nav
                    .update(cx, |nav, cx| nav.open_group(id, cx));
            }))
    }
}

impl Render for Sidebar {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let colors = theme::colors(cx);
        let place = self.services.nav.read(cx).place.clone();
        let section = |label: &'static str| {
            div()
                .px(px(16.))
                .pt(px(12.))
                .pb(px(4.))
                .text_xs()
                .text_color(colors.muted_foreground)
                .child(label)
        };
        let groups: Vec<_> = self
            .groups
            .iter()
            .filter(|g| g.kind == GroupKind::Group)
            .map(|g| {
                self.group_row(g, place == Some(Place::Group(g.id.clone())), cx)
                    .into_any_element()
            })
            .collect();
        let dms: Vec<_> = self
            .groups
            .iter()
            .filter(|g| g.kind == GroupKind::Dm)
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
                    .px(px(8.))
                    .h(px(28.))
                    .rounded(px(6.))
                    .border_1()
                    .border_color(colors.border)
                    .flex()
                    .items_center()
                    .text_color(colors.muted_foreground)
                    .child(common::SEARCH_PLACEHOLDER),
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
                    .when_some(count_badge(todo_count, true, cx), |el, b| el.child(b))
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.services.nav.update(cx, |nav, cx| nav.open_todo(cx))
                    })),
            )
            .child(
                div()
                    .id("sidebar-groups")
                    .flex_1()
                    .overflow_y_scroll()
                    .child(section(nav::GROUPS))
                    .when(groups.is_empty() && dms.is_empty(), |el| {
                        el.child(
                            div()
                                .px(px(16.))
                                .text_color(colors.muted_foreground)
                                .child(nav::NO_GROUPS),
                        )
                    })
                    .children(groups)
                    .when(!dms.is_empty(), |el| el.child(section(nav::DMS)))
                    .children(dms),
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
