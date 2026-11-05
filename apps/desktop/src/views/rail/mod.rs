//! The sidebar of the main window (NATIVE.md §9.1), inside the floating
//! panel the shell draws: the traffic-light row with the filter pull-down
//! (`line.3.horizontal.decrease`) and 新话题 (`square.and.pencil`) at its
//! trailing edge, the 30 pt `搜索` capsule (it opens ⌘K; the shortcut lives
//! in the menu bar), the destinations 首页 / 产出 / 成员 as 32 pt rows, then
//! the work in the user's sidebar style (设置 → 外观 → 侧栏样式, or the
//! filter pull-down). Nothing sits at the bottom: no avatar strip, no
//! account row; 设置 is ⌘, and the App menu.
//!
//! - A 以工作为中心: one section per 群, then 私聊; the pull-down and 显示 ›
//!   我参与的 switch between 我参与的 and 全部话题.
//! - B 以「需要我」为中心: 需要你 / 在做 / 等别人 / 完成 · 今天 (folded);
//!   the pull-down picks the 群.
//! - C 以成员为中心: one row per member (avatar 16, a state dot) under
//!   the headers 在做, 空闲 and 不在; a busy member opens to its 话题. The
//!   destinations stay 首页 / 产出 / 成员.

// --- v12/behavior ---
mod menus;
// --- end v12/behavior ---
pub mod model;
mod modes;
mod rows;
pub mod scenes;

use std::{collections::HashSet, rc::Rc};

use gpui::{
    AnyElement, App, AppContext as _, Context, Entity, InteractiveElement as _, IntoElement,
    ListAlignment, ListState, ParentElement, Render, SharedString, Styled, Subscription,
    WeakEntity, Window, div, list, px,
};
use gpui_component::{
    button::Button,
    input::InputState,
    menu::{DropdownMenu as _, PopupMenu, PopupMenuItem},
};

pub use model::RowState;

use crate::{
    app::{Changes, NewTopic, Page, Place, RailMode, Services},
    copy::{common, rail as c},
    data::{GroupId, MemberId, Topic},
    ui::{
        Sym,
        native::{SearchField, metrics, tokens::space},
        sf_symbols::Sf,
    },
    views::{nav::QuickSwitch, shell::layout},
};

use model::{Bucket, MemberSort};
use modes::{Entry, Inputs};

/// Rows drawn beyond the visible part of the rail list.
const LIST_OVERDRAW: gpui::Pixels = px(240.);

/// What the rail shows, read from the provider once per change (or when
/// what the user chose in the rail changes). A breathing ring renders the
/// rail on every frame it asks for, so render reads only this.
struct Cache {
    inputs: Inputs,
    entries: Rc<Vec<Entry>>,
    /// My ask count on 首页.
    asks: usize,
    /// B: the 群 of the filter pull-down.
    groups: Vec<(GroupId, String)>,
    /// Some 群 has a 话题 in the cache (T-13: the first sync is over for
    /// the sidebar once one shows).
    has_topics: bool,
}

/// The traffic-light row at the top of the sidebar panel: the lights are
/// centred on it (18 pt into the panel, 26 pt from the window top).
const LIGHTS_ROW: gpui::Pixels = px(36.);
/// The sidebar tools in that row (filter, 新话题): 28 pt circles.
const TOOL: gpui::Pixels = px(28.);

pub struct RailView {
    services: Services,
    /// A: 全部话题 instead of 我参与的.
    show_all: bool,
    /// A: 群 folded by their chevron.
    folded_groups: HashSet<GroupId>,
    /// B: the 群 chip (`None` = 全部).
    chip: Option<GroupId>,
    /// B: folded buckets (完成 · 今天 starts folded).
    folded_buckets: HashSet<Bucket>,
    /// C: open member rows; `None` until the user opens or closes one, which
    /// opens the first busy member.
    open_members: Option<HashSet<MemberId>>,
    /// C: the order of members.
    member_sort: MemberSort,
    cache: Option<Cache>,
    /// The virtual list of [`Cache::entries`].
    list: ListState,
    // --- v12/behavior ---
    /// The rail's key focus: ↑/↓ move the selection, Return opens it.
    focus: gpui::FocusHandle,
    // --- end v12/behavior ---
    /// The `搜索` capsule (a click opens ⌘K).
    search: Entity<InputState>,
    /// Draw the inactive-window look (the `window-inactive` scene).
    show_inactive: bool,
    /// Where the sidebar was drawn last frame (window coordinates): its
    /// context menus stay inside it (T-20).
    panel_bounds: std::rc::Rc<std::cell::Cell<Option<gpui::Bounds<gpui::Pixels>>>>,
    _subscriptions: Vec<Subscription>,
}

/// The 话题 whose every ask for me has my answer on 首页 that the
/// provider does not show yet.
fn answered_topics(services: &Services, cx: &App) -> std::collections::HashSet<String> {
    let Some(local) = cx.try_global::<crate::views::desk::model::DeskAnswers>() else {
        return Default::default();
    };
    let key = |a: &crate::data::v2::AskItem| {
        a.message_id
            .clone()
            .unwrap_or_else(|| format!("topic-{}", a.topic_id))
    };
    let asks = services.provider.asks_for_me();
    asks.iter()
        .filter(|a| local.keys.contains(&key(a)))
        .map(|a| a.topic_id.clone())
        .filter(|t| {
            asks.iter()
                .filter(|a| &a.topic_id == t)
                .all(|a| local.keys.contains(&key(a)))
        })
        .collect()
}

impl RailView {
    /// The sidebar list's key focus (a region of the window, F6).
    pub fn focus_handle(&self) -> gpui::FocusHandle {
        self.focus.clone()
    }

    pub fn new(services: Services, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let search =
            cx.new(|cx| InputState::new(window, cx).placeholder(common::SEARCH_PLACEHOLDER));
        let subscriptions = vec![
            cx.subscribe(&services.hub, |this, _, _: &Changes, cx| {
                this.cache = None;
                cx.notify();
            }),
            cx.observe(&services.nav, |_, _, cx| cx.notify()),
            // An answer made on 首页 changes the 首页 count at once.
            cx.observe_global::<crate::views::desk::model::DeskAnswers>(|this, cx| {
                this.cache = None;
                cx.notify();
            }),
        ];
        Self {
            _subscriptions: subscriptions,
            services,
            cache: None,
            list: ListState::new(0, ListAlignment::Top, LIST_OVERDRAW),
            // Tab reaches the rows themselves (FocusRow); the list keeps a
            // handle for the arrow keys when it is focused as a whole.
            focus: cx.focus_handle(),
            search,
            show_inactive: false,
            panel_bounds: Default::default(),
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

    /// What the lists depend on now, besides the provider.
    fn inputs(&self, cx: &App) -> Inputs {
        let nav = self.services.nav.read(cx);
        Inputs {
            sync: modes::Sync::of(&self.services.provider.connection()),
            mode: nav.rail_mode,
            me: self.me(),
            open: nav.topic.clone(),
            // The 群 is selected on its 群 page and in its 群聊.
            group_page: match (&nav.place, nav.page()) {
                (Some(Place::Group(g)), Page::Group) => Some(g.clone()),
                _ => nav.chat_open().cloned(),
            },
            show_all: self.show_all,
            chip: self.chip.clone(),
            member_sort: self.member_sort,
            folded_groups: self.folded_groups.clone(),
            folded_buckets: self.folded_buckets.clone(),
            open_members: self.open_members.clone(),
        }
    }

    /// Reads the rail from the provider when a change came in or the inputs
    /// changed; otherwise keeps what it has.
    fn refresh(&mut self, cx: &App) {
        let mut inputs = self.inputs(cx);
        if self.cache.as_ref().is_some_and(|c| c.inputs == inputs) {
            return;
        }
        let p = self.services.provider.clone();
        let side = p.side_chat_topics();
        let mut entries = match inputs.mode {
            RailMode::Work => modes::work(p.as_ref(), &inputs),
            RailMode::Attention => modes::attention(p.as_ref(), &inputs, &side),
            RailMode::Member => modes::by_member(p.as_ref(), &mut inputs, &side),
        };
        // R2-04, B1-06: an ask I answered on 首页 (sent, queued or still in
        // its undo window) no longer waits on me here either: the row reads
        // 已回答 until the server's state follows.
        let answered = answered_topics(&self.services, cx);
        for e in &mut entries {
            if let modes::Item::Topic(t) = &mut e.item
                && t.line.badge == crate::ui::native::BadgeState::NeedsYou
                && answered.contains(&t.topic.id)
            {
                t.line.badge = crate::ui::native::BadgeState::Done;
                t.line.short = c::RAIL_ANSWERED.to_string();
                t.line.text = c::RAIL_ANSWERED.to_string();
            }
        }
        // C opens its first busy member on the first read.
        self.open_members = inputs.open_members.clone();
        let old = self
            .cache
            .as_ref()
            .map(|c| (c.inputs.mode, c.entries.len()));
        match old {
            Some((mode, n)) if mode == inputs.mode => {
                // Keep the scroll position: replace only the rows past the
                // old end, and measure the others again.
                let keep = n.min(entries.len());
                if n != entries.len() {
                    self.list.splice(keep..n, entries.len() - keep);
                }
                self.list.remeasure_items(0..keep);
            }
            _ => self.list.reset(entries.len()),
        }
        self.cache = Some(Cache {
            // One needs-you query for this count and the 首页 subtitle.
            asks: crate::views::desk::model::waiting_on_me(&self.services, cx),
            groups: model::groups(p.as_ref()),
            has_topics: p.groups().iter().any(|g| !p.topics(&g.id).is_empty()),
            entries: Rc::new(entries),
            inputs,
        });
    }

    /// One row of the virtual list.
    fn entry(&self, ix: usize, cx: &mut Context<Self>) -> AnyElement {
        let Some(entries) = self.cache.as_ref().map(|c| c.entries.clone()) else {
            return div().into_any_element();
        };
        let Some(entry) = entries.get(ix) else {
            return div().into_any_element();
        };
        div()
            .w_full()
            .pt(entry.gap)
            .child(self.item(&entry.item, cx))
            .into_any_element()
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
    /// The menu of the list header: what the list shows (A: 我参与的 or
    /// 全部话题, B: the 群, C: the member order) and the rail style.
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
            RailMode::Attention => {
                // The same 群 filter as the chips.
                menu = menu.label(c::RAIL_SHOW_LABEL);
                for (id, name) in self.group_choices() {
                    let this = this.clone();
                    menu = menu.item(
                        PopupMenuItem::new(SharedString::from(name))
                            .checked(self.chip == id)
                            .on_click(move |_, _, cx| {
                                let id = id.clone();
                                let _ = this.update(cx, |this, cx| {
                                    this.chip = id;
                                    cx.notify();
                                });
                            }),
                    );
                }
                menu = menu.separator();
            }
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

    /// A 28 pt sidebar tool in the traffic-light row (§9.1): a borderless
    /// circle with a 16 pt toolbar symbol, `fillHover` on hover. Named: the
    /// name is its tooltip and what VoiceOver reads.
    fn tool(&self, id: &'static str, sym: Sym, name: &'static str, cx: &App) -> Button {
        crate::ui::native::toolbar_item_sym(id, sym, name, false, !self.show_inactive, cx)
    }

    /// The filter pull-down (`line.3.horizontal.decrease`): what the list
    /// shows (我参与的 / 全部话题, the 群, the member order) and the sidebar
    /// style.
    fn filter_button(&self, mode: RailMode, cx: &mut Context<Self>) -> AnyElement {
        let this = cx.entity().downgrade();
        let services = self.services.clone();
        self.tool("rail-filter", Sym::Filter, c::RAIL_OPTIONS, cx)
            .dropdown_menu_with_anchor(gpui::Anchor::TopLeft, move |menu, _, cx| {
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
        sf: Sf,
        label: &'static str,
        selected: bool,
        count: Option<usize>,
        go: fn(&mut crate::app::Navigation, &mut Context<crate::app::Navigation>),
    ) -> AnyElement {
        let nav = self.services.nav.clone();
        self.row(id, label)
            .symbol(sf)
            .selected(selected)
            .count(count.unwrap_or(0) as u32)
            .on_click(move |_, _, cx| nav.update(cx, go))
            .into_any_element()
    }

    /// The pinned user footer (F04): me, my role, the offline state when
    /// the connection dropped; it opens the account menu.
    fn footer(&self, cx: &App) -> Option<AnyElement> {
        use crate::{
            data::{ConnectionState, SessionState},
            ui::native::{BadgeState, SidebarFooter, StatusBadge},
        };
        let SessionState::SignedIn { me, .. } = self.services.provider.session() else {
            return None;
        };
        let avatar = crate::ui::MemberAvatar::new(Some(&me), crate::ui::native::tokens::avatar::MD);
        let services = self.services.clone();
        let footer = SidebarFooter::new("rail-footer", avatar, me.name.clone())
            .menu(move |menu, _, cx| account_menu(menu, &services, cx));
        // §2.1: the second line is the role, never a server address.
        let role = me.role.clone().filter(|r| !r.trim().is_empty());
        let footer = match self.services.provider.connection() {
            // R2-02, §2.15: while it connects, the second line says so.
            ConnectionState::Connecting => footer.detail(crate::copy::connection::CONNECTING, cx),
            ConnectionState::Online => match role {
                Some(role) => footer.detail(role, cx),
                None => footer,
            },
            _ => footer.second(StatusBadge::new(
                BadgeState::Offline,
                crate::copy::connection::OFFLINE,
            )),
        };
        Some(footer.into_any_element())
    }

    /// The offline Alert row above the footer (§2.1, §2.15): the state and
    /// when it tries again, with 立即重连. `None` while connected or
    /// connecting, and on a 话题 page (its composer docks the Alert).
    fn offline_row(&self, page: Page, cx: &mut Context<Self>) -> Option<AnyElement> {
        use crate::data::{ConnectionState, SessionState};
        let p = &self.services.provider;
        let state = p.connection();
        if page == Page::Topic
            || matches!(state, ConnectionState::Connecting)
            || !matches!(p.session(), SessionState::SignedIn { .. })
        {
            return None;
        }
        let (title, detail) = crate::views::connection_banner::banner_lines(state.clone())?;
        let retry = crate::ui::native::push_button(
            "rail-reconnect",
            if state == ConnectionState::TokenUnreadable {
                crate::copy::connection::RETRY_TOKEN
            } else {
                crate::copy::connection::RECONNECT
            },
            crate::ui::native::PushStyle::Outline,
            crate::ui::native::PushSize::Small,
            cx,
        )
        .on_click(cx.listener(|this, _, _, _| this.services.provider.reconnect_now()));
        Some(
            div()
                .id("rail-offline")
                .flex_none()
                .mx(metrics::SIDEBAR_ROW_INSET)
                .pb(space::S8)
                .child(
                    crate::ui::native::Alert::new(Sym::Offline, title)
                        .tone(crate::ui::native::AlertTone::Warning)
                        .description(detail)
                        .action(retry),
                )
                .into_any_element(),
        )
    }

    /// Draws the inactive-window look (gray selection, `secondary` tools).
    pub fn set_show_inactive(&mut self, inactive: bool, cx: &mut Context<Self>) {
        if self.show_inactive != inactive {
            self.show_inactive = inactive;
            cx.notify();
        }
    }
}

impl Render for RailView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.refresh(cx);
        let nav = self.services.nav.read(cx);
        let page = nav.page();
        let mode = nav.rail_mode;
        let asks = self.cache.as_ref().map_or(0, |c| c.asks);
        // R2-02, T-13: the first sync runs while it connects and no 话题 is
        // cached yet (私聊 and 群 names alone are not the sidebar's data):
        // no 首页 count, no create button, the sidebar skeleton.
        let sync = self
            .cache
            .as_ref()
            .map_or(modes::Sync::Syncing, |c| c.inputs.sync);
        let syncing =
            sync == modes::Sync::Syncing && self.cache.as_ref().is_none_or(|c| !c.has_topics);
        let view = cx.entity();
        let rows = list(self.list.clone(), move |ix, _, cx| {
            view.update(cx, |this, cx| this.entry(ix, cx))
        })
        .size_full();
        // The same three destinations in every sidebar style (§9.1).
        let destinations: Vec<AnyElement> = vec![
            self.destination(
                "rail-desk",
                Sf::Desk,
                c::RAIL_DESK,
                page == Page::Desk,
                (!syncing).then_some(asks),
                |n, cx| n.open_desk(cx),
            ),
            self.destination(
                "rail-artifacts",
                Sf::Outputs,
                c::RAIL_ARTIFACTS,
                matches!(page, Page::Artifacts | Page::Artifact),
                None,
                |n, cx| n.open_artifacts(cx),
            ),
            self.destination(
                "rail-members",
                Sf::Members,
                c::RAIL_MEMBERS,
                page == Page::Members,
                None,
                |n, cx| n.open_members(None, cx),
            ),
        ];
        let filter = self.filter_button(mode, cx);
        let no_rows = self.cache.as_ref().is_none_or(|c| c.entries.is_empty());
        let first_sync = syncing;
        // §2.14, A1-13: no create button while the first sync runs.
        // 新建 (onboarding): 新话题, 新建群 and 发起私聊 in one pull-down, so a
        // new member finds where a 群 or a 私聊 starts without the menu bar.
        // ⌘N still starts a 话题 directly.
        let new_topic = crate::ui::native::disabled_push(
            self.tool("rail-new-topic", Sym::Compose, c::RAIL_NEW, cx),
            first_sync,
            cx,
        )
        .dropdown_menu_with_anchor(gpui::Anchor::TopRight, |menu, _, _| {
            let go = |label: &'static str, action: fn() -> Box<dyn gpui::Action>| {
                PopupMenuItem::new(label)
                    .on_click(move |_, window, cx| window.dispatch_action(action(), cx))
            };
            menu.min_w(px(180.))
                .item(go(c::RAIL_NEW_TOPIC, || Box::new(NewTopic)))
                .separator()
                .item(go(c::RAIL_NEW_GROUP, || {
                    Box::new(crate::views::nav::NewGroup)
                }))
                .item(go(c::RAIL_START_DM, || {
                    Box::new(crate::views::nav::StartDm)
                }))
        });
        let sidebar_fill = crate::views::shell::chrome::colors(cx).sidebar;
        div()
            .id("rail")
            .size_full()
            .flex()
            .flex_col()
            .relative()
            .track_focus(&self.focus)
            .key_context(KEY_CONTEXT)
            // The sidebar's bounds, for its context menus (T-20).
            .child({
                let panel = self.panel_bounds.clone();
                gpui::canvas(move |bounds, _, _| panel.set(Some(bounds)), |_, _, _, _| ())
                    .absolute()
                    .size_full()
            })
            // R2-12 with the kit's FocusRow (R1-01): while a row has the
            // keyboard focus, ↑↓ move that focus and Enter / Space open the
            // row (the row's own keys); with the list itself focused, ↑↓
            // move the selection and open it, as a Mac sidebar does.
            .on_action(
                cx.listener(|this, _: &SelectPrevious, window, cx| this.list_key(-1, window, cx)),
            )
            .on_action(cx.listener(|this, _: &SelectNext, window, cx| this.list_key(1, window, cx)))
            .on_action(
                cx.listener(|this, _: &OpenSelected, window, cx| this.list_key(0, window, cx)),
            )
            // §2.1: the traffic-light band holds the lights only (drawn by
            // the system). Dragging it moves the window, as a title bar does.
            .child(
                div()
                    .id("rail-lights-row")
                    .flex_none()
                    .h(LIGHTS_ROW)
                    .pr(metrics::SIDEBAR_ROW_INSET)
                    .flex()
                    .items_center()
                    .justify_end()
                    .on_mouse_down(gpui::MouseButton::Left, |event, window, _| {
                        if event.click_count >= 2 {
                            window.titlebar_double_click();
                        } else {
                            window.start_window_move();
                        }
                    })
                    // 隐藏侧栏 (⌃⌘S), at the trailing end of the lights row as
                    // in Mail and Notes; a press here never moves the window.
                    .child(
                        div()
                            .on_mouse_down(gpui::MouseButton::Left, |_, _, cx| {
                                cx.stop_propagation()
                            })
                            .child({
                                let nav = self.services.nav.clone();
                                self.tool(
                                    "rail-hide-sidebar",
                                    Sym::SidebarToggle,
                                    crate::copy::nav::SIDEBAR_HIDE,
                                    cx,
                                )
                                .on_click(move |_, _, cx| {
                                    nav.update(cx, |nav, cx| nav.toggle_sidebar(cx))
                                })
                            }),
                    ),
            )
            // The search field, then the filter pull-down and 新话题 on its
            // center line.
            .child(
                div()
                    .flex_none()
                    .pr(metrics::SIDEBAR_ROW_INSET)
                    .flex()
                    .items_center()
                    .gap(space::S4)
                    .child(div().flex_1().min_w_0().child(self.search_field()))
                    .child(filter)
                    .child(new_topic),
            )
            .child(
                div()
                    .flex_none()
                    .mt(px(10.))
                    .flex()
                    .flex_col()
                    .children(destinations),
            )
            .child(
                div()
                    .flex_1()
                    .min_h(px(0.))
                    .relative()
                    // A click in the list makes it take the arrow keys (§10).
                    .on_mouse_down(
                        gpui::MouseButton::Left,
                        cx.listener(|this, _, window, cx| window.focus(&this.focus, cx)),
                    )
                    .child(if no_rows && !first_sync {
                        // §2.13: an empty sidebar says what will show here.
                        div()
                            .px(metrics::SIDEBAR_ROW_INSET)
                            .pt(space::S8)
                            .child(
                                div().px(space::S8).child(
                                    crate::ui::native::EmptyLine::new(c::RAIL_EMPTY_HINT)
                                        .icon(Sym::Topic),
                                ),
                            )
                            .into_any_element()
                    } else if first_sync {
                        // §2.14: the first sync shows the sidebar's own
                        // shapes, never an empty panel.
                        crate::ui::native::skeleton_rows(
                            crate::ui::native::SkeletonKind::SidebarItem,
                            6,
                        )
                        .px(metrics::SIDEBAR_ROW_INSET)
                        .pt(space::S8)
                        .into_any_element()
                    } else {
                        rows.into_any_element()
                    })
                    // §2.1, A4: the list scrolls under a 24 edge, so the row
                    // under 设置 fades out instead of being cut mid-glyph.
                    .child(
                        div()
                            .absolute()
                            .left_0()
                            .right_0()
                            .bottom_0()
                            .h(space::S24)
                            .bg(gpui::linear_gradient(
                                0.,
                                gpui::linear_color_stop(sidebar_fill, 0.),
                                gpui::linear_color_stop(sidebar_fill.opacity(0.), 1.),
                            )),
                    ),
            )
            // §2.1, §2.15, T-13: offline, an Alert row above the footer.
            // A 话题 page docks the same Alert above its composer instead.
            .children(self.offline_row(page, cx))
            // An update that runs while its sheet is hidden.
            .children(
                cx.try_global::<crate::views::banners::AppBanners>()
                    .map(|b| b.update.clone()),
            )
            // The pinned user footer is the one way into 设置 (it opens and
            // closes the settings window); no second 设置 row above it.
            .children(self.footer(cx))
    }
}

/// The account menu of the sidebar footer: every account signed in on this
/// Mac (name over its server's host, a check on the one shown), then
/// 添加账号… and 设置…, then 退出这个账号….
fn account_menu(
    mut menu: PopupMenu,
    services: &Services,
    cx: &mut Context<PopupMenu>,
) -> PopupMenu {
    use crate::ui::native::{NativeType as _, native_colors};
    let accounts = services.provider.accounts();
    for account in accounts {
        let k = native_colors(cx);
        let host = crate::views::login::server_host(&account.server_url).to_string();
        let name = SharedString::from(account.name.clone());
        let line = SharedString::from(match &account.role {
            Some(role) => format!("{role} · {host}"),
            None => host,
        });
        let services = services.clone();
        let id = account.id.clone();
        menu = menu.item(
            PopupMenuItem::element(move |_, _| {
                div()
                    .min_w(px(0.))
                    .flex()
                    .flex_col()
                    .py(space::S2)
                    .child(
                        crate::ui::truncate::TruncText::new(name.clone())
                            .n_body()
                            .text_color(k.label),
                    )
                    .child(
                        crate::ui::truncate::TruncText::new(line.clone())
                            .n_footnote()
                            .text_color(k.secondary),
                    )
            })
            .checked(account.active)
            .on_click(move |_, _, _| services.provider.switch_account(&id)),
        );
    }
    let add = services.clone();
    let out = services.clone();
    menu.separator()
        .item(
            PopupMenuItem::new(c::ACCOUNT_ADD).on_click(move |_, _, _| add.provider.add_account()),
        )
        .item(
            PopupMenuItem::new(crate::copy::settings::MENU_SETTINGS).on_click(|_, window, cx| {
                window.dispatch_action(Box::new(crate::app::ToggleSettings), cx)
            }),
        )
        .separator()
        .item(
            PopupMenuItem::new(c::ACCOUNT_SIGN_OUT).on_click(move |_, window, cx| {
                let services = out.clone();
                crate::views::nav::dialogs::destructive_alert(
                    "account-sign-out",
                    c::ACCOUNT_SIGN_OUT_TITLE.to_string(),
                    c::ACCOUNT_SIGN_OUT_BODY.to_string(),
                    c::ACCOUNT_SIGN_OUT_CONFIRM,
                    move |_, _| services.provider.sign_out(false),
                    window,
                    cx,
                );
            }),
        )
}

// --- v12/behavior ---
gpui::actions!(rail, [SelectPrevious, SelectNext, OpenSelected]);

/// The key context of the rail.
const KEY_CONTEXT: &str = "Rail";

/// ↑/↓ move the selection in the rail's 话题 and 私聊 rows, Return opens
/// the selected row (call once at start-up).
pub fn bind_keys(cx: &mut App) {
    cx.bind_keys([
        gpui::KeyBinding::new("up", SelectPrevious, Some(KEY_CONTEXT)),
        gpui::KeyBinding::new("down", SelectNext, Some(KEY_CONTEXT)),
        gpui::KeyBinding::new("enter", OpenSelected, Some(KEY_CONTEXT)),
    ]);
}

impl RailView {
    /// Where the first 话题 row (or, with `group_header`, the first 群
    /// header) was drawn last frame, in window coordinates (the screenshot
    /// scenes right-click it).
    pub fn first_row_bounds(&self, group_header: bool) -> Option<gpui::Bounds<gpui::Pixels>> {
        let entries = self.cache.as_ref()?.entries.clone();
        let ix = entries.iter().position(|e| match e.item {
            modes::Item::GroupHeader { .. } => group_header,
            modes::Item::Topic(_) => !group_header,
            _ => false,
        })?;
        self.list.bounds_for_item(ix)
    }

    /// A: whether the list shows 全部话题 (else 我参与的).
    pub fn shows_all_topics(&self) -> bool {
        self.show_all
    }

    /// A: switches between 我参与的 and 全部话题 (显示 menu and the list
    /// header menu).
    pub fn set_show_all_topics(&mut self, all: bool, cx: &mut Context<Self>) {
        if self.show_all != all {
            self.show_all = all;
            cx.notify();
        }
    }

    /// ↑↓ in the sidebar (R2-12): selects the row `step` rows from the
    /// selected one (the first or last when none is) and opens it, as a
    /// click does, over every row that opens something: 首页, 产出, 成员,
    /// then the 群 headers (their pages), 话题 and 私聊. A step of 0 opens
    /// the selected row again.
    /// An arrow or Return in the sidebar: the list's own selection when the
    /// list is focused, else the focused row handles it.
    fn list_key(&mut self, step: isize, window: &Window, cx: &mut Context<Self>) {
        if self.focus.is_focused(window) {
            self.move_selection(step, cx);
        } else {
            cx.propagate();
        }
    }

    fn move_selection(&mut self, step: isize, cx: &mut Context<Self>) {
        self.refresh(cx);
        let Some(entries) = self.cache.as_ref().map(|c| c.entries.clone()) else {
            return;
        };
        #[derive(Clone, Copy, PartialEq)]
        enum Stop {
            Desk,
            Artifacts,
            Members,
            Row(usize),
        }
        let page = self.services.nav.read(cx).page();
        let mut stops = vec![Stop::Desk, Stop::Artifacts, Stop::Members];
        stops.extend(
            entries
                .iter()
                .enumerate()
                .filter(|(_, e)| {
                    matches!(
                        e.item,
                        modes::Item::Topic(_)
                            | modes::Item::Dm { .. }
                            | modes::Item::GroupHeader { .. }
                    )
                })
                .map(|(ix, _)| Stop::Row(ix)),
        );
        let current = stops.iter().position(|s| match *s {
            Stop::Desk => page == Page::Desk,
            Stop::Artifacts => matches!(page, Page::Artifacts | Page::Artifact),
            Stop::Members => page == Page::Members,
            Stop::Row(ix) => match &entries[ix].item {
                modes::Item::Topic(t) => t.selected,
                modes::Item::Dm { selected, .. } => *selected,
                modes::Item::GroupHeader { selected, .. } => *selected,
                _ => false,
            },
        });
        let last = stops.len() - 1;
        let next = match current {
            Some(p) => p.saturating_add_signed(step).min(last),
            None if step < 0 => last,
            None => 0,
        };
        let nav = self.services.nav.clone();
        match stops[next] {
            Stop::Desk => nav.update(cx, |nav, cx| nav.open_desk(cx)),
            Stop::Artifacts => nav.update(cx, |nav, cx| nav.open_artifacts(cx)),
            Stop::Members => nav.update(cx, |nav, cx| nav.open_members(None, cx)),
            Stop::Row(ix) => {
                self.list.scroll_to_reveal_item(ix);
                match &entries[ix].item {
                    modes::Item::Topic(t) => {
                        let (g, id) = (t.topic.group_id.clone(), t.topic.id.clone());
                        nav.update(cx, |nav, cx| nav.open_topic(g, id, cx));
                    }
                    modes::Item::Dm { id, .. } | modes::Item::GroupHeader { id, .. } => {
                        let id = id.clone();
                        nav.update(cx, |nav, cx| nav.open_group_chat(id, cx));
                    }
                    _ => {}
                }
            }
        }
    }
}
// --- end v12/behavior ---

impl RailView {
    /// The `搜索` capsule (§5.6): 30 pt, under the traffic-light row. A
    /// click opens ⌘K 搜索或交代; the shortcut lives in the menu bar.
    fn search_field(&self) -> AnyElement {
        div()
            .flex_none()
            .px(metrics::SIDEBAR_ROW_INSET)
            .relative()
            .child(SearchField::new("rail-search", &self.search).sidebar())
            .child(
                div()
                    .id("rail-search-hit")
                    .absolute()
                    .inset_0()
                    .on_mouse_down(gpui::MouseButton::Left, |_, window, cx| {
                        cx.stop_propagation();
                        window.dispatch_action(Box::new(QuickSwitch), cx);
                    }),
            )
            .into_any_element()
    }

    /// B's 群 filter, as the pull-down shows it: 全部 (`None`), then each
    /// 群.
    fn group_choices(&self) -> Vec<(Option<GroupId>, String)> {
        let groups = self
            .cache
            .as_ref()
            .map(|c| c.groups.clone())
            .unwrap_or_default();
        std::iter::once((None, c::RAIL_FILTER_ALL_GROUPS.to_string()))
            .chain(groups.into_iter().map(|(id, name)| (Some(id), name)))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::{
        app::Changes,
        data::{Change, ChatProvider},
        fake::v2::V2Samples,
    };

    /// B's pull-down offers the 群 filter: 全部 first, then every 群 of the
    /// rail.
    #[gpui::test]
    fn the_b_menu_offers_the_group_filter(cx: &mut gpui::TestAppContext) {
        cx.update(gpui_component::init);
        let provider: Arc<dyn ChatProvider> = V2Samples::new();
        let (view, cx) = cx.add_window_view(move |window, cx| {
            let services = Services::new(provider, cx);
            RailView::new(services, window, cx)
        });
        cx.run_until_parked();
        let (choices, groups) = view.update(cx, |this, _| {
            let groups = this.cache.as_ref().map(|c| c.groups.clone()).unwrap();
            (this.group_choices(), groups)
        });
        assert!(!groups.is_empty());
        assert_eq!(choices[0], (None, c::RAIL_FILTER_ALL_GROUPS.to_string()));
        let rest: Vec<_> = choices[1..]
            .iter()
            .map(|(id, name)| (id.clone().expect("a 群"), name.clone()))
            .collect();
        assert_eq!(rest, groups);
    }

    /// Frames (a breathing ring asks for one 24 times a second) draw the
    /// rail from memory; only a provider change reads it again.
    #[gpui::test]
    fn the_rail_reads_the_provider_once_per_change(cx: &mut gpui::TestAppContext) {
        cx.update(gpui_component::init);
        let provider: Arc<dyn ChatProvider> = V2Samples::new();
        let (view, cx) = cx.add_window_view(move |window, cx| {
            let services = Services::new(provider, cx);
            RailView::new(services, window, cx)
        });
        cx.run_until_parked();
        let entries = |view: &gpui::Entity<RailView>, cx: &mut gpui::VisualTestContext| {
            view.update(cx, |this, _| this.cache.as_ref().map(|c| c.entries.clone()))
                .expect("the rail read its lists")
        };
        let first = entries(&view, cx);
        assert!(!first.is_empty());
        for _ in 0..3 {
            view.update(cx, |_, cx| cx.notify());
            cx.run_until_parked();
        }
        assert!(
            Rc::ptr_eq(&first, &entries(&view, cx)),
            "frames reuse the cache"
        );
        view.update(cx, |this, cx| {
            this.services
                .hub
                .update(cx, |_, cx| cx.emit(Changes(vec![Change::Todo])))
        });
        cx.run_until_parked();
        assert!(
            !Rc::ptr_eq(&first, &entries(&view, cx)),
            "a change reads again"
        );
    }

    // --- v12/behavior ---
    /// With the rail focused, ↓ selects and opens the next row (Mac sidebar
    /// behavior, NATIVE.md §10): from 首页 through 产出, 成员 and the 群
    /// headers to the 话题 rows (R2-12); ↑ goes back, Return keeps it.
    #[gpui::test]
    fn arrow_keys_move_the_selection_through_the_rows(cx: &mut gpui::TestAppContext) {
        cx.update(|cx| {
            gpui_component::init(cx);
            bind_keys(cx);
        });
        let provider: Arc<dyn ChatProvider> = V2Samples::new();
        let (view, cx) = cx.add_window_view(move |window, cx| {
            let services = Services::new(provider, cx);
            RailView::new(services, window, cx)
        });
        cx.run_until_parked();
        let rows = |view: &gpui::Entity<RailView>, cx: &mut gpui::VisualTestContext| {
            view.update(cx, |this, _| {
                this.cache
                    .as_ref()
                    .unwrap()
                    .entries
                    .iter()
                    .filter_map(|e| match &e.item {
                        modes::Item::Topic(t) => {
                            Some((t.topic.group_id.clone(), t.topic.id.clone()))
                        }
                        _ => None,
                    })
                    .collect::<Vec<_>>()
            })
        };
        let open = |view: &gpui::Entity<RailView>, cx: &mut gpui::VisualTestContext| {
            view.update(cx, |this, cx| this.services.nav.read(cx).topic.clone())
        };
        let topics = rows(&view, cx);
        assert!(topics.len() >= 2, "the sample rail lists 话题");
        view.update_in(cx, |this, window, cx| window.focus(&this.focus, cx));
        // 产出, 成员 and the first 群 header come before the first 话题.
        for _ in 0..8 {
            cx.simulate_keystrokes("down");
            cx.run_until_parked();
            if open(&view, cx).is_some() {
                break;
            }
        }
        let first = open(&view, cx).expect("↓ opened a row");
        cx.simulate_keystrokes("down");
        cx.run_until_parked();
        let second = open(&view, cx).expect("↓ opened the next row");
        assert_ne!(first, second, "↓ moved the selection");
        cx.simulate_keystrokes("enter");
        cx.run_until_parked();
        assert_eq!(
            open(&view, cx),
            Some(second.clone()),
            "Return keeps it open"
        );
        cx.simulate_keystrokes("up");
        cx.run_until_parked();
        assert_eq!(open(&view, cx), Some(first), "↑ went back");
    }
    // --- end v12/behavior ---
}
