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
    Sizable as _,
    button::{Button, ButtonVariants as _},
    menu::{ContextMenuExt as _, DropdownMenu as _, PopupMenuItem},
    tooltip::Tooltip,
};

use crate::{
    app::{Changes, Place, Services},
    copy::{common, fill, nav, search as search_copy},
    data::{Change, ConnectionState, GroupId, GroupKind, GroupSummary, Member, TodoItem, TodoReason},
    theme::{self, Type as _, space},
    ui::{self, BadgeKind, CountBadge, GroupTile, KbdHint, Lucide, MemberAvatar, SectionHeader},
    views::{
        nav::{NewGroup, QuickSwitch, StartDm, dialogs},
        search::SearchEverywhere,
        status_chip_text,
    },
};

/// At most this many 待办 rows show in the sidebar (§5.2).
const TODO_ROWS: usize = 5;

pub struct Sidebar {
    services: Services,
    groups: Vec<GroupSummary>,
    members: Vec<Member>,
    muted: HashSet<GroupId>,
    todo_count: usize,
    todo: Vec<TodoItem>,
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
            todo: Vec::new(),
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
        self.todo = p.todo();
        self.todo_count = self.todo.len();
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
        let leading = match group.kind {
            GroupKind::Dm => div()
                .flex_none()
                .child(
                    MemberAvatar::new(peer, px(20.))
                        .presence(peer)
                        .on(if selected { colors.sidebar } else { colors.sidebar }),
                )
                .into_any_element(),
            GroupKind::Group => GroupTile::new(group.id.clone(), group.name.clone(), px(20.))
                .into_any_element(),
        };
        let trailing = if group.mentions > 0 {
            Some(CountBadge::new(group.mentions, BadgeKind::Mention).into_any_element())
        } else if muted {
            Some(ui::icon(Lucide::BellOff, theme::icon::SMALL, colors.text_3).into_any_element())
        } else if group.unread > 0 {
            Some(CountBadge::new(group.unread, BadgeKind::Unread).into_any_element())
        } else {
            None
        };
        let name = group.name.clone();
        div()
            .id(ElementId::Name(SharedString::from(format!(
                "group-{}",
                group.id
            ))))
            .flex_none()
            .h(theme::ROW_HEIGHT)
            .mx(space::S10)
            .px(space::S8)
            .rounded(theme::radius::MD)
            .flex()
            .items_center()
            .gap(space::S8)
            .when(selected, |el| el.bg(colors.sidebar_selected))
            .active(|el| el.bg(colors.fill_pressed))
            .child(leading)
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.))
                    .t_ui()
                    .one_line()
                    .when(bold, |el| el.semibold())
                    .when(muted, |el| el.text_color(colors.text_2))
                    .child(name.clone()),
            )
            .children(trailing)
            .tooltip(move |window, cx| Tooltip::new(name.clone()).build(window, cx))
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

impl Sidebar {
    fn todo_row(&self, ix: usize, item: &TodoItem, cx: &mut Context<Self>) -> impl IntoElement {
        let colors = theme::colors(cx);
        let me = self.services.me();
        let (glyph, glyph_color) = match (item.reason, item.topic.status) {
            (TodoReason::Mentioned, _) => (Lucide::AtSign, colors.accent),
            (_, crate::data::TopicStatus::WaitingReview) => (Lucide::Eye, colors.glyph_wait),
            _ => (Lucide::CircleQuestionMark, colors.glyph_wait),
        };
        let reason = match item.reason {
            TodoReason::Mentioned => nav::TODO_REASON_MENTIONED.to_string(),
            TodoReason::WaitingOnMe => {
                status_chip_text(&item.topic, me.as_deref(), &self.members)
                    .unwrap_or_else(|| nav::TODO_REASON_WAITING.to_string())
            }
        };
        let context = format!("{} · {}", item.group_name, reason);
        let (g, t) = (item.group_id.clone(), item.topic.id.clone());
        let selected = self.services.nav.read(cx).topic.as_ref() == Some(&(g.clone(), t.clone()));
        let title = topic_line(&item.topic.title);
        div()
            .id(ElementId::NamedInteger("todo-row".into(), ix as u64))
            .flex_none()
            .h(px(40.))
            .mx(space::S10)
            .pr(space::S8)
            .rounded(theme::radius::MD)
            .flex()
            .items_center()
            .when(selected, |el| el.bg(colors.sidebar_selected))
            .active(|el| el.bg(colors.fill_pressed))
            .child(
                div()
                    .w(px(20.))
                    .ml(space::S6)
                    .mr(space::S6)
                    .flex_none()
                    .flex()
                    .justify_center()
                    .child(ui::icon(glyph, theme::icon::TODO, glyph_color)),
            )
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.))
                    .flex()
                    .flex_col()
                    .child(div().t_ui().medium().one_line().child(title))
                    .child(
                        div()
                            .type_role(theme::type_scale::CAPTION)
                            .text_color(colors.text_2)
                            .one_line()
                            .child(context),
                    ),
            )
            .on_click(cx.listener(move |this, _, _, cx| {
                let (g, t) = (g.clone(), t.clone());
                this.services
                    .nav
                    .update(cx, |nav, cx| nav.open_topic(g, t, cx));
            }))
    }
}

/// The first line of a title; newlines become spaces (§12).
pub fn topic_line(title: &str) -> String {
    title.split_whitespace().collect::<Vec<_>>().join(" ")
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
        let todo_rows: Vec<_> = self
            .todo
            .iter()
            .take(TODO_ROWS)
            .enumerate()
            .map(|(ix, item)| self.todo_row(ix, item, cx).into_any_element())
            .collect();
        let more_todo = self.todo.len() > TODO_ROWS;

        let todo_header = div()
            .id("todo")
            .mx(space::S10)
            .px(space::S8)
            .pt(space::S6)
            .pb(space::S6)
            .rounded(theme::radius::MD)
            .when(place == Some(Place::Todo), |el| el.bg(colors.sidebar_selected))
            .child(
                SectionHeader::new(nav::TODO).trailing(
                    div()
                        .when(todo_count > 0, |el| {
                            el.child(CountBadge::new(todo_count, BadgeKind::Todo))
                        }),
                ),
            )
            .on_click(cx.listener(|this, _, _, cx| {
                this.services.nav.update(cx, |nav, cx| nav.open_todo(cx))
            }));

        let groups_header = div().mx(space::S10).px(space::S8).pt(space::S14).pb(space::S6).child(
            SectionHeader::new(nav::GROUPS).trailing(
                Button::new("sidebar-new")
                    .ghost()
                    .xsmall()
                    .icon(gpui_component::Icon::new(Lucide::Plus))
                    .tooltip(nav::NEW_MENU)
                    .dropdown_menu(|menu, _, _| {
                        menu.menu(nav::NEW_GROUP, Box::new(NewGroup))
                            .menu(nav::START_DM, Box::new(StartDm))
                    }),
            ),
        );

        let empty_state = div().h(px(240.)).child(
            ui::EmptyState::new(Lucide::UsersRound, nav::NO_GROUPS)
                .description(nav::NO_GROUPS_HINT)
                .action(
                    div()
                        .flex()
                        .gap(space::S8)
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
                ),
        );

        let me_id = self.services.me();
        let me = me_id
            .as_ref()
            .and_then(|id| self.members.iter().find(|m| &m.id == id))
            .cloned();
        let footer = me.map(|me| {
            div()
                .flex_none()
                .h(theme::TOOLBAR_HEIGHT)
                .px(space::S16)
                .flex()
                .items_center()
                .gap(space::S10)
                .child(MemberAvatar::new(Some(&me), px(26.)).presence(Some(&me)).on(colors.sidebar))
                .child(
                    div()
                        .flex_1()
                        .min_w(px(0.))
                        .flex()
                        .flex_col()
                        .child(div().t_ui().medium().one_line().child(me.name.clone()))
                        .when_some(me.role.clone(), |el, role| {
                            el.child(
                                div()
                                    .type_role(theme::type_scale::CAPTION)
                                    .text_color(colors.text_2)
                                    .one_line()
                                    .child(role),
                            )
                        }),
                )
                .child(
                    ui::icon_button("sidebar-settings", Lucide::Settings)
                        .tooltip(crate::copy::settings::TITLE)
                        .on_click(|_, window, cx| {
                            window.dispatch_action(Box::new(crate::app::OpenSettings), cx)
                        }),
                )
        });

        div()
            .size_full()
            .flex()
            .flex_col()
            .bg(colors.sidebar)
            .text_color(colors.text_1)
            .t_ui()
            // The traffic lights sit in this band.
            .child(ui::toolbar_band("sidebar-toolbar"))
            .when_some(connection, |el, text| {
                el.child(
                    div().mx(space::S10).mb(space::S8).child(
                        ui::Band::new(Lucide::WifiOff, text).compact(),
                    ),
                )
            })
            .child(
                div()
                    .flex_none()
                    .mx(space::S10)
                    .mb(space::S10)
                    .flex()
                    .items_center()
                    .gap(space::S4)
                    .child(
                        div()
                            .id("sidebar-search")
                            .flex_1()
                            .min_w(px(0.))
                            .px(space::S8)
                            .h(px(28.))
                            .rounded(theme::radius::MD)
                            .bg(colors.fill_field)
                            .flex()
                            .items_center()
                            .gap(space::S6)
                            .active(|el| el.bg(colors.fill_pressed))
                            .child(ui::icon(Lucide::Search, theme::icon::BUTTON, colors.text_2))
                            .child(
                                div()
                                    .flex_1()
                                    .min_w(px(0.))
                                    .one_line()
                                    .text_color(colors.placeholder)
                                    .child(search_copy::SIDEBAR_FIELD),
                            )
                            .child(KbdHint::new(&["shift-cmd-f"]))
                            .on_click(|_, window, cx| {
                                window.dispatch_action(Box::new(SearchEverywhere), cx)
                            }),
                    )
                    .child(
                        ui::icon_button("sidebar-quick-switch", Lucide::ChevronsUpDown)
                            .tooltip(nav::QUICK_SWITCH)
                            .on_click(|_, window, cx| {
                                window.dispatch_action(Box::new(QuickSwitch), cx)
                            }),
                    ),
            )
            .child(
                div()
                    .id("sidebar-groups")
                    .flex_1()
                    .min_h(px(0.))
                    .overflow_y_scroll()
                    .flex()
                    .flex_col()
                    .child(todo_header)
                    .children(todo_rows)
                    .when(more_todo, |el| {
                        el.child(
                            div()
                                .id("todo-all")
                                .flex_none()
                                .h(px(22.))
                                .mx(space::S10)
                                .pl(px(40.))
                                .flex()
                                .items_center()
                                .t_callout()
                                .text_color(colors.text_2)
                                .child(fill(
                                    nav::TODO_ALL,
                                    &[("n", &self.todo.len().to_string())],
                                ))
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.services.nav.update(cx, |nav, cx| nav.open_todo(cx))
                                })),
                        )
                    })
                    .child(groups_header)
                    .when(empty, |el| el.child(empty_state))
                    .children(rows)
                    .child(div().h(space::S8).flex_none()),
            )
            .children(footer)
    }
}
