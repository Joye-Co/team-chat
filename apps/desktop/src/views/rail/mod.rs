//! The left rail of the v2 shell (RAIL2.html, RailOptions.dc.html): title
//! row with 新话题, 搜索或交代 ⌘K, destinations 首页 / 产出 / 成员, then the
//! work in the user's rail mode (设置 → 外观 → 侧栏样式, or the menu of the
//! list header), the team strip and me.
//!
//! - A 以工作为中心: 话题 by 群 (then 私聊), each row with its state slot
//!   and meta; the header filter 我参与的 / 全部话题.
//! - B 以「需要我」为中心: 群 chips, then 需要你 / 在做 / 等别人 /
//!   做完了 · 今天 (collapsed).
//! - C 以成员为中心: members that are busy first, each expandable to its
//!   话题; idle members in one row; 同事 below. 话题 replaces 成员 among the
//!   destinations and the team strip is left out.

pub mod model;
mod modes;
mod rows;
pub mod scenes;

use std::collections::HashSet;

use gpui::{
    AnyElement, App, Context, InteractiveElement as _, IntoElement, ParentElement, Render,
    SharedString, StatefulInteractiveElement as _, Styled, Subscription, WeakEntity, Window, div,
    prelude::FluentBuilder as _, px,
};
use gpui_component::{
    button::{Button, ButtonVariants as _},
    menu::{DropdownMenu as _, PopupMenu, PopupMenuItem},
};

pub use model::RowState;

use crate::{
    app::{NewTopic, Page, Place, RailMode, Services},
    copy::{fill, rail as c},
    data::{GroupId, MemberId, Topic},
    theme::{self, Type as _, space},
    ui::{self, Lucide, MemberAvatar, NamedButton as _, Ring, RowSurface},
    views::{
        nav::QuickSwitch,
        shell::{layout, panes::watch},
    },
};

use model::{Bucket, MemberSort};

/// The team strip shows at most this many members.
const STRIP_MAX: usize = 5;
/// Team strip tiles.
const STRIP_AVATAR: gpui::Pixels = px(18.);
/// Member tiles in mode C.
const MEMBER_AVATAR: gpui::Pixels = px(18.);
/// The me circle in the footer.
const ME_AVATAR: gpui::Pixels = px(22.);
/// The search field.
const SEARCH_HEIGHT: gpui::Pixels = px(34.);
const SEARCH_RADIUS: gpui::Pixels = px(9.);
/// The list header (`话题 · 我参与的 ⌄`) and the B chips.
const LIST_HEADER: gpui::Pixels = px(28.);
const CHIP_HEIGHT: gpui::Pixels = px(24.);

pub struct RailView {
    services: Services,
    /// A: 全部话题 instead of 我参与的.
    show_all: bool,
    /// A: 群 folded by their chevron.
    folded_groups: HashSet<GroupId>,
    /// B: the 群 chip (`None` = 全部).
    chip: Option<GroupId>,
    /// B: folded buckets (做完了 · 今天 starts folded).
    folded_buckets: HashSet<Bucket>,
    /// C: open member rows; `None` until the user opens or closes one, which
    /// opens the first busy member.
    open_members: Option<HashSet<MemberId>>,
    /// C: the order of members.
    member_sort: MemberSort,
    _subscriptions: Vec<Subscription>,
}

impl RailView {
    pub fn new(services: Services, cx: &mut Context<Self>) -> Self {
        Self {
            _subscriptions: watch(&services, cx),
            services,
            show_all: false,
            folded_groups: HashSet::new(),
            chip: None,
            folded_buckets: HashSet::from([Bucket::DoneToday]),
            open_members: None,
            member_sort: MemberSort::default(),
        }
    }

    fn me(&self) -> Option<String> {
        self.services.me()
    }
}

/// The state of a 话题 row and its meta text, for other pages (the 群
/// page) that show the same slot.
pub fn row_state(services: &Services, topic: &Topic) -> (RowState, Option<String>) {
    let me = services.me();
    let (state, meta) = model::row_state(services.provider.as_ref(), me.as_deref(), topic);
    (state, meta.map(|m| m.text))
}

/// Sets the rail mode and keeps it in `layout.json`.
pub fn choose_mode(services: &Services, mode: RailMode, cx: &mut App) {
    services
        .nav
        .update(cx, |nav, cx| nav.set_rail_mode(mode, cx));
    layout::save_rail_mode(mode);
}

impl RailView {
    /// The menu of the list header: what the list shows (mode A and C) and
    /// the rail style.
    fn options_menu(
        &self,
        menu: PopupMenu,
        mode: RailMode,
        this: WeakEntity<Self>,
        services: Services,
    ) -> PopupMenu {
        let mut menu = menu.min_w(px(200.));
        match mode {
            RailMode::Work => {
                for (all, label) in [(false, c::RAIL_FILTER_MINE), (true, c::RAIL_FILTER_ALL)] {
                    let this = this.clone();
                    menu = menu.item(
                        PopupMenuItem::new(label)
                            .checked(self.show_all == all)
                            .on_click(move |_, _, cx| {
                                let _ = this.update(cx, |this, cx| {
                                    this.show_all = all;
                                    cx.notify();
                                });
                            }),
                    );
                }
                menu = menu.separator();
            }
            RailMode::Member => {
                for (sort, label) in [
                    (MemberSort::Busy, c::RAIL_SORT_BUSY),
                    (MemberSort::Roster, c::RAIL_SORT_ROSTER),
                ] {
                    let this = this.clone();
                    menu = menu.item(
                        PopupMenuItem::new(label)
                            .checked(self.member_sort == sort)
                            .on_click(move |_, _, cx| {
                                let _ = this.update(cx, |this, cx| {
                                    this.member_sort = sort;
                                    cx.notify();
                                });
                            }),
                    );
                }
                menu = menu.separator();
            }
            RailMode::Attention => {}
        }
        menu = menu.label(c::RAIL_MODE_LABEL);
        for (m, label) in [
            (RailMode::Work, c::RAIL_MODE_WORK),
            (RailMode::Attention, c::RAIL_MODE_ATTENTION),
            (RailMode::Member, c::RAIL_MODE_MEMBER),
        ] {
            let services = services.clone();
            menu = menu.item(
                PopupMenuItem::new(label)
                    .checked(mode == m)
                    .on_click(move |_, _, cx| choose_mode(&services, m, cx)),
            );
        }
        menu
    }

    /// `我参与的 ⌄`: the header menu button.
    fn menu_button(
        &self,
        id: &'static str,
        label: Option<&'static str>,
        mode: RailMode,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let k = theme::colors(cx);
        let this = cx.entity().downgrade();
        let services = self.services.clone();
        let button = Button::new(id)
            .ghost()
            .h(px(22.))
            .rounded(px(6.))
            .when(label.is_some(), |b| b.pl(space::S6).pr(space::S4))
            .when(label.is_none(), |b| b.w(px(24.)))
            .text_color(k.ink_3)
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(space::S2)
                    .t_meta()
                    .text_color(k.ink_3)
                    .children(label)
                    .child(ui::icon(
                        if label.is_some() {
                            Lucide::ChevronDown
                        } else {
                            Lucide::SlidersHorizontal
                        },
                        if label.is_some() {
                            px(12.)
                        } else {
                            theme::icon::TAG
                        },
                        k.ink_3,
                    )),
            )
            .when(label.is_none(), |b| b.named(c::RAIL_OPTIONS));
        button
            .dropdown_menu_with_anchor(gpui::Anchor::TopRight, move |menu, _, cx| {
                let Some(view) = this.upgrade() else {
                    return menu;
                };
                view.read(cx)
                    .options_menu(menu, mode, this.clone(), services.clone())
            })
            .into_any_element()
    }

    /// A destination row: 首页 (with my ask count), 产出, 成员 or 话题.
    #[allow(clippy::too_many_arguments)]
    fn destination(
        &self,
        id: &'static str,
        icon: Lucide,
        label: &'static str,
        selected: bool,
        count: Option<usize>,
        go: fn(&mut crate::app::Navigation, &mut Context<crate::app::Navigation>),
        cx: &App,
    ) -> AnyElement {
        let k = theme::colors(cx);
        let nav = self.services.nav.clone();
        ui::list_row(id, RowSurface::Rail, selected, cx)
            .h(theme::RAIL_ROW)
            .px(space::S10)
            .gap(space::S10)
            .child(ui::icon(
                icon,
                theme::icon::ROW,
                if selected { k.ink } else { k.ink_2 },
            ))
            .child(
                div()
                    .flex_1()
                    .t_small()
                    .text_color(if selected { k.ink } else { k.ink_2 })
                    .when(selected, |el| el.medium())
                    .child(label),
            )
            .children(
                count
                    .filter(|n| *n > 0)
                    .map(|n| div().t_mono().text_color(k.you_ink).child(n.to_string())),
            )
            .on_click(move |_, _, cx| nav.update(cx, go))
            .into_any_element()
    }

    /// The team strip (A and B): who waits on me, who works, then `+n`.
    fn team_strip(&self, cx: &App) -> Option<AnyElement> {
        let k = theme::colors(cx);
        let me = self.me();
        let (shown, rest) =
            model::team_strip(self.services.provider.as_ref(), me.as_deref(), STRIP_MAX);
        if shown.is_empty() {
            return None;
        }
        let nav = self.services.nav.clone();
        let total = shown.len() + rest;
        Some(
            ui::list_row("rail-team-strip", RowSurface::Rail, false, cx)
                .mt(space::S8)
                .h(px(36.))
                .px(space::S10)
                .gap(space::S4)
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(space::S6)
                        .children(shown.iter().map(|(m, you)| {
                            MemberAvatar::new(Some(m), STRIP_AVATAR).ring(Some(if *you {
                                Ring::You
                            } else {
                                Ring::Live
                            }))
                        })),
                )
                .when(rest > 0, |el| {
                    el.child(
                        div()
                            .ml(space::S6)
                            .t_mono()
                            .text_color(k.ink_3)
                            .child(fill(c::RAIL_MORE_MEMBERS, &[("n", &rest.to_string())])),
                    )
                })
                .tooltip(move |window, cx| {
                    gpui_component::tooltip::Tooltip::new(fill(
                        c::RAIL_TEAM_STRIP,
                        &[
                            ("busy", &(total - rest).to_string()),
                            ("n", &total.to_string()),
                        ],
                    ))
                    .build(window, cx)
                })
                .on_click(move |_, _, cx| nav.update(cx, |nav, cx| nav.open_members(None, cx)))
                .into_any_element(),
        )
    }

    /// Me: the ink circle with my initial (or photo), my name, 设置.
    fn me_row(&self, cx: &App) -> AnyElement {
        let k = theme::colors(cx);
        let me = self.me().and_then(|m| self.services.provider.member(&m));
        let photo = me
            .as_ref()
            .and_then(|m| m.avatar_url.as_deref())
            .is_some_and(|p| p.strip_prefix("file://").unwrap_or(p).starts_with('/'));
        let face: AnyElement = if photo {
            MemberAvatar::new(me.as_ref(), ME_AVATAR).into_any_element()
        } else {
            div()
                .flex_none()
                .size(ME_AVATAR)
                .rounded_full()
                .bg(k.btn)
                .text_color(k.btn_ink)
                .flex()
                .items_center()
                .justify_center()
                .text_size(px(11.))
                .semibold()
                .children(
                    me.as_ref()
                        .map(|m| crate::ui::avatar::monogram(&m.name, px(16.))),
                )
                .into_any_element()
        };
        div()
            .flex_none()
            .h(px(40.))
            .pl(space::S10)
            .pr(space::S6)
            .flex()
            .items_center()
            .gap(space::S10)
            .child(face)
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .one_line()
                    .t_small()
                    .medium()
                    .text_color(k.ink)
                    .children(me.map(|m| m.name)),
            )
            .child(
                ui::icon_button_sm("rail-settings", Lucide::SlidersHorizontal)
                    .named(c::RAIL_SETTINGS)
                    .on_click(|_, window, cx| {
                        window.dispatch_action(Box::new(crate::app::OpenSettings), cx)
                    }),
            )
            .into_any_element()
    }
}

impl Render for RailView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let k = theme::colors(cx);
        let nav = self.services.nav.read(cx);
        let page = nav.page();
        let mode = nav.rail_mode;
        let open = nav.topic.clone();
        let group_page = match (&nav.place, page) {
            (Some(Place::Group(g)), Page::Group) => Some(g.clone()),
            _ => None,
        };
        let asks = self.services.provider.asks_for_me().len();
        let list: Vec<AnyElement> = match mode {
            RailMode::Work => self.work(open.as_ref(), group_page.as_deref(), cx),
            RailMode::Attention => self.attention(open.as_ref(), cx),
            RailMode::Member => self.by_member(open.as_ref(), cx),
        };
        // C trades 成员 for 话题 among the destinations: its list is members.
        let destinations: Vec<AnyElement> = match mode {
            RailMode::Member => vec![
                self.destination(
                    "rail-desk",
                    Lucide::House,
                    c::RAIL_DESK,
                    page == Page::Desk,
                    Some(asks),
                    |n, cx| n.open_desk(cx),
                    cx,
                ),
                self.destination(
                    "rail-topics",
                    Lucide::TextAlignStart,
                    c::RAIL_TOPICS,
                    matches!(page, Page::Topic | Page::Group),
                    None,
                    choose_topics,
                    cx,
                ),
                self.destination(
                    "rail-artifacts",
                    Lucide::Layers,
                    c::RAIL_ARTIFACTS,
                    matches!(page, Page::Artifacts | Page::Artifact),
                    None,
                    |n, cx| n.open_artifacts(cx),
                    cx,
                ),
            ],
            _ => vec![
                self.destination(
                    "rail-desk",
                    Lucide::House,
                    c::RAIL_DESK,
                    page == Page::Desk,
                    Some(asks),
                    |n, cx| n.open_desk(cx),
                    cx,
                ),
                self.destination(
                    "rail-artifacts",
                    Lucide::Layers,
                    c::RAIL_ARTIFACTS,
                    matches!(page, Page::Artifacts | Page::Artifact),
                    None,
                    |n, cx| n.open_artifacts(cx),
                    cx,
                ),
                self.destination(
                    "rail-members",
                    Lucide::Users,
                    c::RAIL_MEMBERS,
                    page == Page::Members,
                    None,
                    |n, cx| n.open_members(None, cx),
                    cx,
                ),
            ],
        };
        let header: AnyElement = match mode {
            RailMode::Work => self.list_header(
                c::RAIL_TOPICS,
                self.menu_button(
                    "rail-filter",
                    Some(if self.show_all {
                        c::RAIL_FILTER_ALL
                    } else {
                        c::RAIL_FILTER_MINE
                    }),
                    mode,
                    cx,
                ),
                cx,
            ),
            RailMode::Attention => self.chips(cx),
            RailMode::Member => self.list_header(
                c::RAIL_MEMBERS,
                self.menu_button(
                    "rail-sort",
                    Some(match self.member_sort {
                        MemberSort::Busy => c::RAIL_SORT_BUSY,
                        MemberSort::Roster => c::RAIL_SORT_ROSTER,
                    }),
                    mode,
                    cx,
                ),
                cx,
            ),
        };
        let strip = (mode != RailMode::Member)
            .then(|| self.team_strip(cx))
            .flatten();
        div()
            .id("rail")
            .size_full()
            .flex()
            .flex_col()
            .bg(k.rail)
            .px(space::S10)
            .pb(space::S10)
            // Title row: the traffic lights, then 新话题.
            .child(
                div()
                    .flex_none()
                    .h(theme::TITLE_BAR)
                    .flex()
                    .items_center()
                    .justify_end()
                    .child(
                        ui::icon_button_sm("rail-new-topic", Lucide::SquarePen)
                            .named(c::RAIL_NEW_TOPIC)
                            .on_click(|_, window, cx| {
                                window.dispatch_action(Box::new(NewTopic), cx)
                            }),
                    ),
            )
            .child(self.search_field(cx))
            .child(
                div()
                    .flex_none()
                    .mt(space::S10)
                    .flex()
                    .flex_col()
                    .gap(px(1.))
                    .children(destinations),
            )
            .child(header)
            .child(
                div()
                    .id(SharedString::from(format!("rail-list-{}", mode.slug())))
                    .flex_1()
                    .min_h(px(0.))
                    .overflow_y_scroll()
                    .flex()
                    .flex_col()
                    .gap(if mode == RailMode::Member {
                        px(0.)
                    } else {
                        space::S6
                    })
                    .when(mode == RailMode::Attention, |el| el.mt(space::S8))
                    .children(list),
            )
            // App banners (connection, update) sit above the footer.
            .children(crate::views::banners::slot(cx))
            .children(strip)
            .child(self.me_row(cx))
    }
}

/// C's 话题 destination: the 群 of the open 话题, else the first 群.
fn choose_topics(nav: &mut crate::app::Navigation, cx: &mut Context<crate::app::Navigation>) {
    let group = match (&nav.place, &nav.topic) {
        (_, Some((g, _))) => Some(g.clone()),
        (Some(Place::Group(g)), None) => Some(g.clone()),
        _ => None,
    };
    if let Some(g) = group {
        nav.place = None;
        nav.open_group(g, cx);
    } else {
        nav.place = Some(Place::Todo);
        nav.topic = None;
        cx.notify();
    }
}

impl RailView {
    /// 搜索或交代 ⌘K.
    fn search_field(&self, cx: &App) -> AnyElement {
        let k = theme::colors(cx);
        div()
            .id("rail-search")
            .flex_none()
            .h(SEARCH_HEIGHT)
            .pl(space::S10)
            .pr(space::S8)
            .rounded(SEARCH_RADIUS)
            .bg(k.bg)
            .border_1()
            .border_color(k.line)
            .shadow(k.sec_hi())
            .flex()
            .items_center()
            .gap(space::S8)
            .cursor_pointer()
            .hover(|s| s.border_color(k.line_2))
            .child(ui::icon(Lucide::Search, theme::icon::CHIP, k.ink_3))
            .child(
                div()
                    .flex_1()
                    .t_small()
                    .text_color(k.ink_3)
                    .child(c::RAIL_SEARCH),
            )
            .child(
                div()
                    .flex_none()
                    .h(px(18.))
                    .px(px(5.))
                    .rounded(px(5.))
                    .bg(k.sunken)
                    .flex()
                    .items_center()
                    .t_mono()
                    .text_size(px(11.))
                    .text_color(k.ink_3)
                    .child("⌘K"),
            )
            .on_click(|_, window, cx| window.dispatch_action(Box::new(QuickSwitch), cx))
            .into_any_element()
    }

    /// `话题 … 我参与的 ⌄`.
    fn list_header(&self, label: &'static str, trailing: AnyElement, cx: &App) -> AnyElement {
        let k = theme::colors(cx);
        div()
            .flex_none()
            .mt(space::S16)
            .h(LIST_HEADER)
            .pl(space::S10)
            .pr(space::S6)
            .flex()
            .items_center()
            .child(
                div()
                    .flex_1()
                    .t_meta_medium()
                    .text_color(k.ink_3)
                    .child(label),
            )
            .child(trailing)
            .into_any_element()
    }

    /// B: 全部 and one chip per 群, then the options menu.
    fn chips(&self, cx: &mut Context<Self>) -> AnyElement {
        let k = theme::colors(cx);
        let groups = model::groups(self.services.provider.as_ref());
        let mut chips: Vec<AnyElement> = Vec::new();
        let entries = std::iter::once((None, c::RAIL_FILTER_ALL_GROUPS.to_string()))
            .chain(groups.into_iter().map(|(id, name)| (Some(id), name)));
        for (i, (id, name)) in entries.enumerate() {
            let on = self.chip == id;
            let this = cx.entity().downgrade();
            chips.push(
                div()
                    .id(("rail-chip", i))
                    .flex_none()
                    .h(CHIP_HEIGHT)
                    .px(space::S8)
                    .rounded(px(6.))
                    .flex()
                    .items_center()
                    .t_meta()
                    .cursor_pointer()
                    .when(on, |el| el.bg(k.sel).text_color(k.ink).medium())
                    .when(!on, |el| {
                        el.text_color(k.ink_2)
                            .hover(|s| s.bg(k.hover))
                            .active(|s| s.bg(k.pressed))
                    })
                    .child(name)
                    .on_click(move |_, _, cx| {
                        let id = id.clone();
                        let _ = this.update(cx, |this, cx| {
                            this.chip = id;
                            cx.notify();
                        });
                    })
                    .into_any_element(),
            );
        }
        div()
            .flex_none()
            .mt(space::S16)
            .flex()
            .items_center()
            .gap(space::S2)
            .child(
                div()
                    .id("rail-chips")
                    .flex_1()
                    .min_w_0()
                    .overflow_x_scroll()
                    .flex()
                    .items_center()
                    .gap(space::S2)
                    .children(chips),
            )
            .child(div().flex_none().child(self.menu_button(
                "rail-options",
                None,
                RailMode::Attention,
                cx,
            )))
            .into_any_element()
    }
}
