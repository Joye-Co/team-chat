//! The left sidebar (DESIGN.md §7.7): the 52 pt band with the traffic
//! lights, one search field that opens the quick switcher, then three
//! sections: 待办 (two-line rows, at most five), 群 (a `#` glyph per row) and
//! 私聊 (the peer's avatar), each with a collapse chevron, and the footer with
//! my avatar, presence and settings. Rows get their states from
//! [`ui::list_row`]; each 群 has a context menu (mute, leave).

use std::collections::HashSet;

use gpui::{
    Context, Div, ElementId, Hsla, InteractiveElement, IntoElement, ParentElement, Render,
    ScrollHandle, SharedString, StatefulInteractiveElement, Styled, Subscription, Window, div,
    prelude::FluentBuilder as _, px,
};
use gpui_component::{
    menu::{ContextMenuExt as _, PopupMenuItem},
    tooltip::Tooltip,
};

use crate::ui::NamedButton as _;
use crate::{
    app::{Changes, Place, Services},
    copy::{common, fill, nav},
    data::{Change, GroupId, GroupKind, GroupSummary, Member, TodoItem, TodoReason, TopicStatus},
    theme::{self, Type as _, size, space},
    ui::{self, BadgeKind, CountBadge, GroupGlyph, Kbd, Lucide, MemberAvatar, RowSurface},
    views::nav::{NewGroup, QuickSwitch, StartDm},
};

/// At most this many 待办 rows show in the sidebar (§7.7).
const TODO_ROWS: usize = 5;

/// Width of a context menu.
pub const MENU_WIDTH: gpui::Pixels = px(220.);

/// The sections that collapse.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum Section {
    Todo,
    Groups,
    Dms,
}

/// A menu row (§7.12): 32 pt, a 16 pt glyph in `text-2` (or `danger-text`
/// for a destructive item), 8 pt gap, `ui` 14.
pub fn menu_item(glyph: Lucide, label: &'static str, danger: bool) -> PopupMenuItem {
    PopupMenuItem::element(move |_, cx| {
        let k = theme::colors(cx);
        let (glyph_color, text) = if danger {
            (k.danger_text, k.danger_text)
        } else {
            (k.text_2, k.text_1)
        };
        div()
            .h(size::MENU_ROW)
            .flex()
            .items_center()
            .gap(space::S8)
            .t_ui()
            .text_color(text)
            .child(ui::icon(glyph, theme::icon::ROW, glyph_color))
            .child(label)
    })
}

/// Which edges of a scroll area have content beyond them: (top, bottom).
pub fn scroll_edges(handle: &ScrollHandle) -> (bool, bool) {
    let y = handle.offset().y;
    let max = handle.max_offset().y;
    (y < px(-0.5), max > px(0.5) && y > -max + px(0.5))
}

/// A 24 pt fade from `color` at the top or bottom edge of a scroll area
/// (§7.7 scroll edges), so a row cut by the edge never shows as a hard
/// sliver. Place it in a relative parent over the scroll area.
pub fn edge_fade(top: bool, color: Hsla) -> Div {
    // A 24 pt fade (§7.7): 8 pt of solid `color` at the edge, so a row cut
    // by the edge never peeks out (badges included), then an eased 16 pt
    // ramp 1.0, 0.85, 0.5, 0.15, 0 in 4 pt linear steps (a gradient has two
    // stops).
    const STOPS: [f32; 5] = [1.0, 0.85, 0.5, 0.15, 0.0];
    let el = div()
        .absolute()
        .left_0()
        .right_0()
        .h(space::S24)
        .flex()
        .when(top, |el| el.top_0().flex_col())
        .when(!top, |el| el.bottom_0().flex_col_reverse())
        .child(div().flex_none().h(space::S8).bg(color));
    el.children(STOPS.windows(2).map(|w| {
        div().flex_none().h(space::S4).bg(gpui::linear_gradient(
            if top { 180. } else { 0. },
            gpui::linear_color_stop(color.opacity(w[0]), 0.),
            gpui::linear_color_stop(color.opacity(w[1]), 1.),
        ))
    }))
}

/// Re-renders `view` on the next frame when the scroll edges in `seen` are
/// out of date (the handle learns its sizes only at layout).
pub fn watch_scroll_edges<V: 'static>(
    handle: &ScrollHandle,
    seen: (bool, bool),
    window: &mut Window,
    cx: &mut Context<V>,
) {
    let handle = handle.clone();
    let view = cx.entity().downgrade();
    window.on_next_frame(move |_, cx| {
        if scroll_edges(&handle) != seen {
            view.update(cx, |_, cx| cx.notify()).ok();
        }
    });
}

pub struct Sidebar {
    services: Services,
    scroll: ScrollHandle,
    groups: Vec<GroupSummary>,
    members: Vec<Member>,
    muted: HashSet<GroupId>,
    todo: Vec<TodoItem>,
    collapsed: HashSet<Section>,
    _subscriptions: Vec<Subscription>,
    /// Re-renders when the window becomes key or stops being key.
    activation: Option<Subscription>,
    /// The selection uses the accent tint (the window is key).
    emphasized: bool,
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
            scroll: ScrollHandle::new(),
            groups: Vec::new(),
            members: Vec::new(),
            muted: HashSet::new(),
            todo: Vec::new(),
            collapsed: HashSet::new(),
            _subscriptions: subscriptions,
            activation: None,
            emphasized: true,
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
    }

    pub fn group_count(&self) -> usize {
        self.groups.len()
    }

    fn toggle(&mut self, section: Section, cx: &mut Context<Self>) {
        if !self.collapsed.remove(&section) {
            self.collapsed.insert(section);
        }
        cx.notify();
    }

    /// A section header (§7.7): 28 pt, 16 pt above (4 for the first),
    /// padding 0 8; the chevron and title collapse the section, `on_title`
    /// replaces that for 待办 (it opens the 待办 list); a trailing `+`.
    fn section_header(
        &self,
        section: Section,
        label: &'static str,
        count: Option<u32>,
        trailing: Option<gpui::AnyElement>,
        first: bool,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let k = theme::colors(cx);
        let collapsed = self.collapsed.contains(&section);
        let chevron = if collapsed {
            Lucide::ChevronRight
        } else {
            Lucide::ChevronDown
        };
        let open_todo = section == Section::Todo;
        div()
            .flex_none()
            .w_full()
            .mt(if first { space::S4 } else { space::S16 })
            .h(size::SECTION_HEADER)
            .flex()
            .items_center()
            .child(
                div()
                    .id(ElementId::Name(SharedString::from(format!(
                        "section-chevron-{label}"
                    ))))
                    .flex_none()
                    .size(px(20.))
                    .ml(space::S2)
                    .rounded(theme::radius::SM)
                    .flex()
                    .items_center()
                    .justify_center()
                    .cursor_pointer()
                    .hover(|s| s.bg(k.sidebar_hover))
                    .child(ui::icon(chevron, theme::icon::TAG, k.text_3))
                    .on_click(cx.listener(move |this, _, _, cx| this.toggle(section, cx))),
            )
            .child(
                div()
                    .id(ElementId::Name(SharedString::from(format!(
                        "section-{label}"
                    ))))
                    .flex_1()
                    .min_w(px(0.))
                    .h_full()
                    .pl(space::S4)
                    .flex()
                    .items_center()
                    .gap(space::S6)
                    .cursor_pointer()
                    // Quieter than the rows under it (the mockup): GPUI draws
                    // 600 heavier than the browser, so the label takes 500.
                    .t_meta_medium()
                    .text_color(k.text_2)
                    .child(label)
                    .when_some(count.filter(|&n| n > 0), |el, n| {
                        el.child(
                            div()
                                .t_meta_medium()
                                .tnum()
                                .text_color(k.text_3)
                                .child(CountBadge::cap(n)),
                        )
                    })
                    .on_click(cx.listener(move |this, _, _, cx| {
                        if open_todo {
                            this.services.nav.update(cx, |nav, cx| nav.open_todo(cx));
                        } else {
                            this.toggle(section, cx);
                        }
                    })),
            )
            .children(trailing.map(|t| div().flex_none().mr(px(-2.)).child(t)))
            .into_any_element()
    }

    fn group_row(
        &self,
        group: &GroupSummary,
        selected: bool,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let k = theme::colors(cx);
        let id = group.id.clone();
        let muted = self.muted.contains(&group.id);
        let peer = group
            .dm_peer
            .as_ref()
            .and_then(|p| self.members.iter().find(|m| &m.id == p));
        let unread = (group.unread > 0 || group.mentions > 0) && !muted;
        let menu_services = self.services.clone();
        let menu_group = group.clone();
        let surface = match (selected, self.emphasized) {
            (true, true) => k.sidebar_selected,
            (true, false) => k.pressed,
            _ => k.sidebar,
        };
        let leading = match group.kind {
            GroupKind::Dm => div()
                .flex_none()
                .size(size::AVATAR_ROW)
                .child(
                    MemberAvatar::new(peer, size::AVATAR_ROW)
                        .presence(peer)
                        .on(surface)
                        .selected(selected),
                )
                .into_any_element(),
            GroupKind::Group => GroupGlyph::new()
                .unread(unread || selected && !self.emphasized)
                .selected(selected && self.emphasized)
                .into_any_element(),
        };
        // One trailing element, one rule with the topic rows (§7.6, §7.8):
        // @ mentions are red (`n` on a 群, `@n` on a DM, beside the DMs'
        // neutral counts); a muted 群 shows the bell; an unread DM shows the
        // neutral count. An unread 群 shows by weight. Counts cap at 99+.
        let trailing = if group.mentions > 0 {
            let kind = match group.kind {
                GroupKind::Group => BadgeKind::MentionCount,
                GroupKind::Dm => BadgeKind::Mention,
            };
            Some(CountBadge::new(group.mentions, kind).into_any_element())
        } else if muted {
            Some(ui::icon(Lucide::BellOff, theme::icon::CHIP, k.text_3).into_any_element())
        } else if group.kind == GroupKind::Dm && group.unread > 0 {
            Some(CountBadge::new(group.unread, BadgeKind::Unread).into_any_element())
        } else {
            None
        };
        let name = group.name.clone();
        ui::list_row(
            ElementId::Name(SharedString::from(format!("group-{}", group.id))),
            RowSurface::Sidebar,
            selected,
            cx,
        )
        .when(selected && !self.emphasized, |el| el.bg(k.pressed))
        .h(size::SIDEBAR_ROW)
        .px(space::S8)
        .gap(space::S10)
        .t_ui()
        .text_color(if unread || selected {
            k.text_1
        } else {
            k.text_2
        })
        .when(muted, |el| el.opacity(0.6))
        .child(leading)
        .child(
            div()
                .flex_1()
                .min_w(px(0.))
                .one_line()
                .when(unread || selected, |el| el.semibold())
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
            let (mute_label, mute_icon) = if muted {
                (nav::UNMUTE, Lucide::Bell)
            } else {
                (nav::MUTE, Lucide::BellOff)
            };
            let mute_services = services.clone();
            let mute_id = g.id.clone();
            let menu =
                menu.min_w(MENU_WIDTH)
                    .item(
                        menu_item(mute_icon, mute_label, false).on_click(move |_, _, _| {
                            mute_services.provider.set_group_muted(&mute_id, !muted);
                        }),
                    );
            if g.kind == GroupKind::Group {
                let add_services = services.clone();
                let (add_id, add_name) = (g.id.clone(), g.name.clone());
                // The destructive item in its own group, in `danger-text`.
                menu.item(
                    menu_item(Lucide::UserPlus, nav::ADD_MEMBERS, false).on_click(
                        move |_, window, cx| {
                            super::nav::dialogs::open_add_members(
                                add_services.clone(),
                                add_id.clone(),
                                add_name.clone(),
                                window,
                                cx,
                            );
                        },
                    ),
                )
                .separator()
                    .item(menu_item(Lucide::LogOut, nav::LEAVE_GROUP, true).on_click(
                        move |_, window, cx| {
                            super::nav::dialogs::confirm_leave(
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

    /// A 待办 row (§7.7): two lines, padding 6 8, radius 8, gap 10; a grey
    /// glyph in a 24 pt slot; title `ui` 14/20 500; context 12/16 `text-2`
    /// (`qqbot 研发 · Watch 等你回答`).
    fn todo_row(&self, ix: usize, item: &TodoItem, cx: &mut Context<Self>) -> impl IntoElement {
        let k = theme::colors(cx);
        let glyph = match (item.reason, item.topic.status) {
            (TodoReason::Mentioned, _) => Lucide::AtSign,
            (_, TopicStatus::WaitingReview) => Lucide::Eye,
            _ => Lucide::CircleQuestionMark,
        };
        let context = format!("{} · {}", item.group_name, todo_reason(item, &self.members));
        let (g, t) = (item.group_id.clone(), item.topic.id.clone());
        let selected = self.services.nav.read(cx).topic.as_ref() == Some(&(g.clone(), t.clone()));
        let title = topic_line(&item.topic.title);
        let untitled = title.is_empty();
        let title = if untitled {
            nav::UNTITLED.to_string()
        } else {
            title
        };
        // Title and context are both cut to one line; the tooltip has both.
        let tip_title = SharedString::from(title.clone());
        let tip_detail = SharedString::from(context.clone());
        ui::list_row(
            ElementId::NamedInteger("todo-row".into(), ix as u64),
            RowSurface::Sidebar,
            selected,
            cx,
        )
        .when(selected && !self.emphasized, |el| el.bg(k.pressed))
        .items_start()
        .py(space::S6)
        .px(space::S8)
        .gap(space::S10)
        .child(
            div()
                .flex_none()
                .w(size::AVATAR_ROW)
                .h(px(20.))
                .flex()
                .items_center()
                .justify_center()
                .child(ui::icon(glyph, theme::icon::ROW, k.text_3)),
        )
        .child(
            div()
                .flex_1()
                .min_w(px(0.))
                .flex()
                .flex_col()
                .child(
                    div()
                        .t_ui()
                        .medium()
                        .one_line()
                        .text_color(if untitled { k.text_2 } else { k.text_1 })
                        .when(selected, |el| el.semibold())
                        .child(title),
                )
                .child(
                    div()
                        .mt(px(1.))
                        .t_meta()
                        .text_color(k.text_2)
                        .one_line()
                        .child(context),
                ),
        )
        .tooltip(move |window, cx| {
            ui::row_tip(tip_title.clone(), tip_detail.clone()).build(window, cx)
        })
        .on_click(cx.listener(move |this, _, _, cx| {
            let (g, t) = (g.clone(), t.clone());
            this.services
                .nav
                .update(cx, |nav, cx| nav.open_topic(g, t, cx));
        }))
    }
}

/// Who waits and why, for a 待办 row: `Watch 等你回答`, `李雪 @你`.
fn todo_reason(item: &TodoItem, members: &[Member]) -> String {
    let who = item
        .topic
        .owner_id
        .as_ref()
        .or(Some(&item.topic.created_by))
        .and_then(|id| members.iter().find(|m| &m.id == id))
        .map(|m| m.name.clone());
    let reason = match (item.reason, item.topic.status) {
        (TodoReason::Mentioned, _) => nav::TODO_MENTIONED_YOU,
        (_, TopicStatus::WaitingReview) => common::STATUS_WAITING_REVIEW_ME,
        (_, TopicStatus::WaitingAnswer) => common::STATUS_WAITING_ANSWER_ME,
        _ => nav::TODO_REASON_WAITING,
    };
    match who {
        // In a DM the group name is already the person.
        Some(who) if who == item.group_name => reason.to_string(),
        Some(who) => format!("{who} {reason}"),
        None if item.reason == TodoReason::Mentioned => nav::TODO_REASON_MENTIONED.to_string(),
        None => reason.to_string(),
    }
}

/// The first line of a title; newlines become spaces.
pub fn topic_line(title: &str) -> String {
    title.split_whitespace().collect::<Vec<_>>().join(" ")
}

impl Render for Sidebar {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self.activation.is_none() {
            self.activation = Some(cx.observe_window_activation(window, |_, _, cx| cx.notify()));
        }
        self.emphasized = ui::rows::selection_emphasized(window);
        let k = theme::colors(cx);
        let edges = scroll_edges(&self.scroll);
        watch_scroll_edges(&self.scroll, edges, window, cx);
        let place = self.services.nav.read(cx).place.clone();
        let row_of = |this: &Self, g: &GroupSummary, cx: &mut Context<Self>| {
            this.group_row(g, place == Some(Place::Group(g.id.clone())), cx)
                .into_any_element()
        };
        let groups: Vec<GroupSummary> = self
            .groups
            .iter()
            .filter(|g| g.kind == GroupKind::Group)
            .cloned()
            .collect();
        let dms: Vec<GroupSummary> = self
            .groups
            .iter()
            .filter(|g| g.kind == GroupKind::Dm)
            .cloned()
            .collect();
        let group_rows: Vec<_> = if self.collapsed.contains(&Section::Groups) {
            // A collapsed section keeps the selected and unread rows (Slack).
            groups
                .iter()
                .filter(|g| {
                    place == Some(Place::Group(g.id.clone())) || g.mentions > 0 || g.unread > 0
                })
                .map(|g| row_of(self, g, cx))
                .collect()
        } else {
            groups.iter().map(|g| row_of(self, g, cx)).collect()
        };
        let dm_rows: Vec<_> = if self.collapsed.contains(&Section::Dms) {
            dms.iter()
                .filter(|g| {
                    place == Some(Place::Group(g.id.clone())) || g.mentions > 0 || g.unread > 0
                })
                .map(|g| row_of(self, g, cx))
                .collect()
        } else {
            dms.iter().map(|g| row_of(self, g, cx)).collect()
        };
        let todo_count = self.todo.len() as u32;
        let todo_open = !self.collapsed.contains(&Section::Todo);
        let todo_rows: Vec<_> = if todo_open {
            self.todo
                .iter()
                .take(TODO_ROWS)
                .enumerate()
                .map(|(ix, item)| self.todo_row(ix, item, cx).into_any_element())
                .collect()
        } else {
            Vec::new()
        };
        let more_todo = todo_open && self.todo.len() > TODO_ROWS;

        // 待办 shows only while something waits for me.
        let has_todo = !self.todo.is_empty();
        let todo_header = has_todo.then(|| {
            self.section_header(Section::Todo, nav::TODO, Some(todo_count), None, true, cx)
        });
        let groups_header = self.section_header(
            Section::Groups,
            nav::GROUPS,
            None,
            Some(
                ui::icon_button_sm("sidebar-new-group", Lucide::Plus)
                    .named(nav::NEW_GROUP)
                    .on_click(|_, window, cx| window.dispatch_action(Box::new(NewGroup), cx))
                    .into_any_element(),
            ),
            !has_todo,
            cx,
        );
        let dms_header = self.section_header(
            Section::Dms,
            nav::DMS,
            None,
            Some(
                ui::icon_button_sm("sidebar-start-dm", Lucide::Plus)
                    .named(nav::START_DM)
                    .on_click(|_, window, cx| window.dispatch_action(Box::new(StartDm), cx))
                    .into_any_element(),
            ),
            false,
            cx,
        );

        let me_id = self.services.me();
        let me = me_id
            .as_ref()
            .and_then(|id| self.members.iter().find(|m| &m.id == id))
            .cloned();
        // Footer (§7.7): 56 pt, a top hairline inset 8, 32 avatar with
        // presence, name 14/500, the presence word 12 `text-2`, settings.
        let footer = me.map(|me| {
            let presence = if me.online {
                common::ONLINE
            } else {
                common::OFFLINE
            };
            div()
                .flex_none()
                .h(size::SIDEBAR_FOOTER)
                .mx(space::S8)
                .pl(space::S12)
                .pr(space::S8)
                .border_t(theme::HAIRLINE)
                .border_color(k.sep)
                .flex()
                .items_center()
                .gap(space::S10)
                .child(
                    MemberAvatar::new(Some(&me), size::AVATAR_FOOTER)
                        .presence(Some(&me))
                        .on(k.sidebar),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w(px(0.))
                        .flex()
                        .flex_col()
                        .child(div().t_ui().medium().one_line().child(me.name.clone()))
                        .child(
                            div()
                                .t_meta()
                                .text_color(k.text_2)
                                .one_line()
                                .child(presence),
                        ),
                )
                .child(
                    ui::icon_button("sidebar-settings", Lucide::Settings)
                        .named(crate::copy::settings::TITLE)
                        .on_click(|_, window, cx| {
                            window.dispatch_action(Box::new(crate::app::OpenSettings), cx)
                        }),
                )
        });

        // The one search entry (§7.4, U35): a 36 pt field that opens the
        // switcher, with `⌘K` at the right.
        let search = div()
            .id("sidebar-search")
            .flex_none()
            .mx(space::S12)
            .mb(space::S8)
            .h(size::SEARCH_FIELD)
            .rounded(theme::radius::MD)
            .bg(k.field)
            .border_1()
            .border_color(gpui::transparent_black())
            .hover(|s| s.border_color(k.border_strong))
            .active(|s| s.bg(k.pressed))
            .cursor_text()
            .pl(px(11.))
            .pr(px(7.))
            .flex()
            .items_center()
            .gap(space::S8)
            .child(ui::icon(Lucide::Search, theme::icon::ROW, k.text_3))
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.))
                    .t_ui()
                    .one_line()
                    .text_color(k.placeholder)
                    .child(common::SEARCH_PLACEHOLDER),
            )
            .child(Kbd::new(nav::QUICK_SWITCH_SHORTCUT))
            .tooltip(|window, cx| Tooltip::new(nav::QUICK_SWITCH).build(window, cx))
            .on_click(|_, window, cx| window.dispatch_action(Box::new(QuickSwitch), cx));

        div()
            .size_full()
            .flex()
            .flex_col()
            .bg(k.sidebar)
            .text_color(k.text_1)
            .t_ui()
            // The traffic lights sit in this band.
            .child(ui::toolbar_band("sidebar-toolbar"))
            .child(search)
            .child(
                div()
                    .relative()
                    .flex_1()
                    .min_h(px(0.))
                    .child(
                        div()
                            .id("sidebar-groups")
                            .size_full()
                            .overflow_y_scroll()
                            .track_scroll(&self.scroll)
                            .px(space::S8)
                            .flex()
                            .flex_col()
                            .children(todo_header)
                            .children(todo_rows)
                            .when(more_todo, |el| {
                                el.child(
                                    div()
                                        .id("todo-all")
                                        .flex_none()
                                        .h(px(28.))
                                        .rounded(theme::radius::MD)
                                        // Under the titles: 8 + 24 + 10 pt.
                                        .pl(px(42.))
                                        .cursor_pointer()
                                        .hover(|s| s.bg(k.sidebar_hover))
                                        .active(|s| s.bg(k.pressed))
                                        .flex()
                                        .items_center()
                                        .t_meta_medium()
                                        .text_color(k.text_2)
                                        .child(fill(
                                            nav::TODO_ALL,
                                            &[("n", &CountBadge::cap(todo_count))],
                                        ))
                                        .on_click(cx.listener(|this, _, _, cx| {
                                            this.services
                                                .nav
                                                .update(cx, |nav, cx| nav.open_todo(cx))
                                        })),
                                )
                            })
                            .child(groups_header)
                            .children(group_rows)
                            // 私聊 shows with its + even with no DM yet (§8.5).
                            .child(dms_header)
                            .children(dm_rows)
                            .child(div().h(space::S8).flex_none()),
                    )
                    .when(edges.0, |el| el.child(edge_fade(true, k.sidebar)))
                    .when(edges.1, |el| el.child(edge_fade(false, k.sidebar))),
            )
            // App-level banners (update, connection) above the footer (§8.6).
            .children(crate::views::banners::slot(cx))
            .children(footer)
    }
}
