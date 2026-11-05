//! The root view, the shared services that every view gets, navigation state,
//! the change hub, windows, menus and actions.
//!
//! Data flow:
//! - `Services::provider` is the [`ChatProvider`]: views read snapshots from it
//!   and call its commands. It never blocks.
//! - `Services::hub` is a [`ChangeHub`] entity. The app drains
//!   `provider.subscribe()` on the main thread and emits each batch as a
//!   [`Changes`] event. A view subscribes with
//!   `cx.subscribe(&services.hub, |this, _, changes: &Changes, cx| ...)`,
//!   re-reads what changed, and calls `cx.notify()`.
//! - `Services::nav` is the [`Navigation`] entity (selected place and topic,
//!   panel state). Views observe it with `cx.observe(&services.nav, ...)` and
//!   change it with `services.nav.update(cx, |nav, cx| nav.open_topic(..., cx))`.

use std::sync::Arc;

use futures::StreamExt;
use gpui::{
    App, AppContext, Bounds, Context, Entity, EventEmitter, FocusHandle, InteractiveElement,
    IntoElement, KeyBinding, ParentElement, Render, Styled, Subscription, TitlebarOptions, Window,
    WindowBackgroundAppearance, WindowBounds, WindowOptions, div, point, px, size,
};

use crate::{
    data::{Change, ChatProvider, GroupId, MemberId, SessionState, TopicId},
    theme,
    views::{
        composer::ComposerView,
        connection_banner::ConnectionBanner,
        login::LoginView,
        rail::RailView,
        shell::{pages::Pages, panes::Panes},
        timeline::TimelineView,
    },
};

gpui::actions!(
    desktop,
    [
        Quit,
        OpenSettings,
        ToggleSidePanel,
        ToggleSidebar,
        NewTopic,
        MentionInComposer,
        FocusNext,
        FocusPrev,
        // K-10: F6 / ⌥Tab jump between the window's regions.
        FocusNextRegion,
        FocusPrevRegion,
        // v2 shell (1.1.0)
        ToggleSideChat,
        ToggleTopicRail,
        OpenDesk,
        OpenMembers,
        OpenArtifacts
    ]
);

/// A batch of provider changes, emitted by [`ChangeHub`].
#[derive(Clone, Debug)]
pub struct Changes(pub Vec<Change>);

impl Changes {
    pub fn any(&self, f: impl Fn(&Change) -> bool) -> bool {
        self.0.iter().any(f)
    }

    /// True when the batch touches the messages, typing or 产出 of this topic.
    pub fn touches_topic(&self, group_id: &str, topic_id: &str) -> bool {
        self.any(|c| match c {
            Change::Messages {
                group_id: g,
                topic_id: t,
            }
            | Change::Typing {
                group_id: g,
                topic_id: t,
            }
            | Change::Artifacts {
                group_id: g,
                topic_id: t,
            } => g == group_id && t == topic_id,
            _ => false,
        })
    }
}

/// Re-emits provider change batches on the main thread.
pub struct ChangeHub;

impl EventEmitter<Changes> for ChangeHub {}

impl ChangeHub {
    pub fn start(provider: &Arc<dyn ChatProvider>, cx: &mut App) -> Entity<ChangeHub> {
        let hub = cx.new(|_| ChangeHub);
        let mut rx = provider.subscribe();
        let weak = hub.downgrade();
        cx.spawn(async move |cx| {
            while let Some(batch) = rx.next().await {
                if weak.update(cx, |_, cx| cx.emit(Changes(batch))).is_err() {
                    break;
                }
            }
        })
        .detach();
        hub
    }
}

/// Where the user is.
///
/// v2 (1.1.0): the page follows from the place and the open 话题, see
/// [`Navigation::page`]. A 话题 page is "a 话题 is open" with a `Group`
/// place; `Todo` is the v1 待办 list, which v2 shows as the desk.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Place {
    /// The 待办 list (v1). v2 draws it as the 首页 desk.
    Todo,
    Group(GroupId),
    /// 首页: the desk.
    Desk,
    /// 成员, with one member's detail open.
    Members(Option<MemberId>),
    /// 产出: every 产出.
    Artifacts,
    /// One 产出 (`version: None` is the current one).
    Artifact {
        id: String,
        version: Option<u32>,
    },
}

/// The page the main window shows (v2 shell: rail | page | pane).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Page {
    Desk,
    Topic,
    Group,
    Members,
    Artifacts,
    Artifact,
}

/// The pane at the right of a page. Panes show only on 话题 pages, except
/// [`RightPane::Artifact`], which any page can open.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub enum RightPane {
    None,
    /// The topic rail (负责, 计划, 改动, 产出 …): the default.
    #[default]
    TopicRail,
    /// 单独问 with this member (⌘;).
    SideChat(MemberId),
    /// 实时查看 of this run.
    Live(String),
    /// A 产出 beside the 话题.
    Artifact {
        id: String,
        version: Option<u32>,
    },
    /// 改动 (the PR of the 话题).
    Changes,
}

impl RightPane {
    /// Same kind of pane, whatever it shows.
    pub fn same_kind(&self, other: &RightPane) -> bool {
        std::mem::discriminant(self) == std::mem::discriminant(other)
    }
}

/// How the left rail lists work (设置 → 外观 → 侧栏样式). Kept locally in
/// `layout.json`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum RailMode {
    /// A: 话题 grouped by 群, each row with its live state (default).
    #[default]
    Work,
    /// B: 需要你 / 在做 / 做完了 · 今天, with 群 as filter chips.
    Attention,
    /// C: members, each expandable to the 话题 it works on.
    Member,
}

impl RailMode {
    pub const ALL: [RailMode; 3] = [RailMode::Work, RailMode::Attention, RailMode::Member];

    /// The name in `layout.json`.
    pub fn slug(self) -> &'static str {
        match self {
            RailMode::Work => "work",
            RailMode::Attention => "attention",
            RailMode::Member => "member",
        }
    }

    pub fn from_slug(slug: &str) -> Option<RailMode> {
        RailMode::ALL.into_iter().find(|m| m.slug() == slug)
    }
}

/// Navigation state shared by the views of one main window.
pub struct Navigation {
    pub place: Option<Place>,
    /// The open topic: (group id, topic id). `None` with a group place means
    /// the composer starts a new topic.
    pub topic: Option<(GroupId, TopicId)>,
    /// Closed by default; opens by hand, or by itself once per topic
    /// (DESIGN.md §8.1). Never shown with no topic open.
    pub side_panel_open: bool,
    /// Topics where the side panel no longer opens by itself: it did once,
    /// or the user closed it there.
    side_panel_settled: std::collections::HashSet<(GroupId, TopicId)>,
    pub developer_view: bool,
    /// The side panel tab: 0 = 详情, 1 = 过程.
    pub side_panel_tab: usize,
    /// A message to scroll to and highlight (from search).
    pub focus: Option<FocusRequest>,
    /// When the side panel last opened by itself: its content fades in
    /// (§12). `None` after a manual open.
    pub side_panel_auto_at: Option<std::time::Instant>,
    /// The pointer is over the topic header: the side panel does not open by
    /// itself then, so the header buttons never move under the pointer.
    pub header_hovered: bool,
    /// ⌘⇧S hid the sidebar (§4.4).
    pub sidebar_hidden: bool,
    /// v2: the pane at the right of a 话题 page.
    pub right_pane: RightPane,
    /// v2: how the rail lists work.
    pub rail_mode: RailMode,
}

/// Scroll the open topic to this message and highlight it. `serial` grows
/// with every request, so the same message can be asked for again.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FocusRequest {
    pub group_id: GroupId,
    pub topic_id: TopicId,
    pub message_id: String,
    /// The message's `created_seq` (0 when unknown).
    pub seq: u64,
    pub serial: u64,
    /// The search that found it, when it was opened from search results:
    /// the 话题 shows the search bar (query, `2 / 7`, previous and next).
    pub search: Option<SearchContext>,
}

/// The hits of one search in one 话题, newest first, and the one shown.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SearchContext {
    pub query: String,
    /// `(message id, created_seq)` of every hit in this 话题.
    pub hits: Vec<(String, u64)>,
    pub index: usize,
}

impl Navigation {
    pub fn new() -> Self {
        Self {
            place: None,
            topic: None,
            side_panel_open: false,
            side_panel_settled: Default::default(),
            developer_view: false,
            side_panel_tab: 0,
            focus: None,
            side_panel_auto_at: None,
            header_hovered: false,
            sidebar_hidden: false,
            right_pane: RightPane::default(),
            rail_mode: RailMode::default(),
        }
    }

    /// The page to show (v2 shell).
    pub fn page(&self) -> Page {
        match (&self.place, &self.topic) {
            (Some(Place::Members(_)), _) => Page::Members,
            (Some(Place::Artifacts), _) => Page::Artifacts,
            (Some(Place::Artifact { .. }), _) => Page::Artifact,
            (_, Some(_)) => Page::Topic,
            (Some(Place::Group(_)), None) => Page::Group,
            (Some(Place::Desk | Place::Todo) | None, None) => Page::Desk,
        }
    }

    /// 首页.
    pub fn open_desk(&mut self, cx: &mut Context<Self>) {
        self.place = Some(Place::Desk);
        self.topic = None;
        cx.notify();
    }

    /// 成员, with `member` selected.
    pub fn open_members(&mut self, member: Option<MemberId>, cx: &mut Context<Self>) {
        self.place = Some(Place::Members(member));
        self.topic = None;
        cx.notify();
    }

    /// 产出: the list.
    pub fn open_artifacts(&mut self, cx: &mut Context<Self>) {
        self.place = Some(Place::Artifacts);
        self.topic = None;
        cx.notify();
    }

    /// One 产出 page.
    pub fn open_artifact(&mut self, id: String, version: Option<u32>, cx: &mut Context<Self>) {
        self.place = Some(Place::Artifact { id, version });
        self.topic = None;
        cx.notify();
    }

    /// The pane that shows now: the chosen one on a 话题 page, an 产出 pane
    /// anywhere, else none.
    pub fn visible_pane(&self) -> RightPane {
        match (&self.right_pane, self.page()) {
            (pane, Page::Topic) => pane.clone(),
            (RightPane::Artifact { .. }, _) => self.right_pane.clone(),
            _ => RightPane::None,
        }
    }

    /// Sets the pane by hand. Closing it remembers the 话题, so nothing opens
    /// there by itself again.
    pub fn set_right_pane(&mut self, pane: RightPane, cx: &mut Context<Self>) {
        if pane == RightPane::None
            && let Some(topic) = self.topic.clone()
        {
            self.side_panel_settled.insert(topic);
        }
        self.side_panel_auto_at = None;
        if self.right_pane != pane {
            self.right_pane = pane;
            cx.notify();
        }
    }

    /// A title bar button: opens `pane`, or closes it when that kind of pane
    /// shows already (the rail toggle, 单独问 ⌘;, 产出, 电脑, 改动).
    pub fn toggle_pane(&mut self, pane: RightPane, cx: &mut Context<Self>) {
        let next = if self.visible_pane().same_kind(&pane) {
            RightPane::None
        } else {
            pane
        };
        self.set_right_pane(next, cx);
    }

    /// ⌘;: 单独问 with `member` (the default member when `None`).
    pub fn toggle_side_chat(&mut self, member: Option<MemberId>, cx: &mut Context<Self>) {
        match member {
            Some(m) => self.toggle_pane(RightPane::SideChat(m), cx),
            None if matches!(self.visible_pane(), RightPane::SideChat(_)) => {
                self.set_right_pane(RightPane::None, cx)
            }
            None => {}
        }
    }

    /// Opens `pane` by itself (a run started, the first 产出 came), once per
    /// 话题 and never after the user closed the pane there (§8.1 rule).
    pub fn auto_open_pane(&mut self, pane: RightPane, cx: &mut Context<Self>) {
        let Some(topic) = self.topic.clone() else {
            return;
        };
        if self.header_hovered || self.side_panel_settled.contains(&topic) {
            return;
        }
        self.side_panel_settled.insert(topic);
        if self.right_pane != pane {
            self.right_pane = pane;
            self.side_panel_auto_at = Some(std::time::Instant::now());
            cx.notify();
        }
    }

    /// Changes how the rail lists work (saved by the caller, see
    /// `views::shell::layout`).
    pub fn set_rail_mode(&mut self, mode: RailMode, cx: &mut Context<Self>) {
        if self.rail_mode != mode {
            self.rail_mode = mode;
            cx.notify();
        }
    }

    pub fn open_todo(&mut self, cx: &mut Context<Self>) {
        self.place = Some(Place::Todo);
        cx.notify();
    }

    pub fn open_group(&mut self, group_id: GroupId, cx: &mut Context<Self>) {
        if self.place.as_ref() != Some(&Place::Group(group_id.clone())) {
            self.place = Some(Place::Group(group_id));
            self.topic = None;
            cx.notify();
        }
    }

    pub fn open_topic(&mut self, group_id: GroupId, topic_id: TopicId, cx: &mut Context<Self>) {
        if self.place != Some(Place::Todo) {
            self.place = Some(Place::Group(group_id.clone()));
        }
        self.topic = Some((group_id, topic_id));
        cx.notify();
    }

    /// Opens a topic in its group and asks the timeline to show one message.
    pub fn open_message(
        &mut self,
        group_id: GroupId,
        topic_id: TopicId,
        message_id: String,
        seq: u64,
        cx: &mut Context<Self>,
    ) {
        self.open_message_found(group_id, topic_id, message_id, seq, None, cx)
    }

    /// Opens a topic at a search hit: like [`Self::open_message`], with the
    /// search that found it, so the 话题 shows its search bar.
    pub fn open_search_hit(
        &mut self,
        group_id: GroupId,
        topic_id: TopicId,
        search: SearchContext,
        cx: &mut Context<Self>,
    ) {
        let Some((message_id, seq)) = search.hits.get(search.index).cloned() else {
            return;
        };
        self.open_message_found(group_id, topic_id, message_id, seq, Some(search), cx)
    }

    fn open_message_found(
        &mut self,
        group_id: GroupId,
        topic_id: TopicId,
        message_id: String,
        seq: u64,
        search: Option<SearchContext>,
        cx: &mut Context<Self>,
    ) {
        let serial = self.focus.as_ref().map_or(0, |f| f.serial) + 1;
        self.place = Some(Place::Group(group_id.clone()));
        self.topic = Some((group_id.clone(), topic_id.clone()));
        self.focus = Some(FocusRequest {
            group_id,
            topic_id,
            message_id,
            seq,
            serial,
            search,
        });
        cx.notify();
    }

    /// Clears the open topic so the composer starts a new one in the current group.
    pub fn new_topic(&mut self, cx: &mut Context<Self>) {
        self.topic = None;
        cx.notify();
    }

    /// The group the composer writes into.
    pub fn current_group(&self) -> Option<GroupId> {
        match (&self.topic, &self.place) {
            (Some((g, _)), _) => Some(g.clone()),
            (None, Some(Place::Group(g))) => Some(g.clone()),
            _ => None,
        }
    }

    pub fn toggle_side_panel(&mut self, cx: &mut Context<Self>) {
        self.set_side_panel_open(!self.side_panel_open, cx);
    }

    /// Opens or closes the side panel by hand. A close is remembered for the
    /// open topic: the panel does not open by itself there again.
    pub fn set_side_panel_open(&mut self, open: bool, cx: &mut Context<Self>) {
        if !open && let Some(topic) = self.topic.clone() {
            self.side_panel_settled.insert(topic);
        }
        self.side_panel_auto_at = None;
        if self.side_panel_open != open {
            self.side_panel_open = open;
            cx.notify();
        }
    }

    /// Opens the side panel by itself (a bot started work or the first 产出
    /// arrived), once per topic and never after the user closed it there.
    pub fn auto_open_side_panel(&mut self, cx: &mut Context<Self>) {
        let Some(topic) = self.topic.clone() else {
            return;
        };
        // Wait while the pointer is on the topic header: opening would move
        // its buttons under the pointer. The next change tries again.
        if self.header_hovered || self.side_panel_settled.contains(&topic) {
            return;
        }
        self.side_panel_settled.insert(topic);
        if !self.side_panel_open {
            self.side_panel_open = true;
            self.side_panel_auto_at = Some(std::time::Instant::now());
            cx.notify();
        }
    }

    /// The pointer entered or left the topic header.
    pub fn set_header_hovered(&mut self, hovered: bool) {
        self.header_hovered = hovered;
    }

    /// ⌘⇧S: hides or shows the sidebar.
    pub fn toggle_sidebar(&mut self, cx: &mut Context<Self>) {
        self.sidebar_hidden = !self.sidebar_hidden;
        cx.notify();
    }

    pub fn set_side_panel_tab(&mut self, tab: usize, cx: &mut Context<Self>) {
        if self.side_panel_tab != tab {
            self.side_panel_tab = tab;
            cx.notify();
        }
    }

    pub fn set_developer_view(&mut self, on: bool, cx: &mut Context<Self>) {
        self.developer_view = on;
        cx.notify();
    }
}

impl Default for Navigation {
    fn default() -> Self {
        Self::new()
    }
}

/// What every view gets.
#[derive(Clone)]
pub struct Services {
    pub provider: Arc<dyn ChatProvider>,
    pub hub: Entity<ChangeHub>,
    pub nav: Entity<Navigation>,
    /// Keeps the live statuses of member characters (`characters::StatusBoard`).
    pub characters: Entity<crate::characters::StatusTracker>,
}

impl Services {
    pub fn new(provider: Arc<dyn ChatProvider>, cx: &mut App) -> Self {
        let hub = ChangeHub::start(&provider, cx);
        let nav = cx.new(|_| Navigation::new());
        let characters = crate::characters::StatusTracker::start(&provider, &hub, cx);
        let services = Self {
            provider,
            hub,
            nav,
            characters,
        };
        // --- inc-host track: the 产出 page host (data, asks, new versions) ---
        crate::views::artifacts::host::start(&services, cx);
        // --- end inc-host track ---
        // inc-refs: 引用提问 reaches the provider and the navigation from element callbacks.
        crate::views::refs::install(&services, cx);
        services
    }

    /// My member id, when signed in.
    pub fn me(&self) -> Option<String> {
        match self.provider.session() {
            SessionState::SignedIn { me, .. } => Some(me.id),
            _ => None,
        }
    }
}

/// Called once per frame after the first frame; see `main.rs` self-tests.
pub type FirstFrameHook = Box<dyn FnOnce(&mut Window, &mut App)>;

/// The root view of the main window: login, or the v2 shell (1.1.0): the
/// left rail | the page (title bar + body) | the right pane. See
/// `views/shell` for the pages and panes and who owns each.
///
/// The v1 sidebar, topic list and side panel (`views/sidebar.rs`,
/// `topic_list.rs`, `side_panel.rs`) still build but are not drawn; the
/// final 1.1.0 integration deletes them.
pub struct RootView {
    services: Services,
    rail: Entity<RailView>,
    timeline: Entity<TimelineView>,
    composer: Entity<ComposerView>,
    pages: Pages,
    panes: Panes,
    login: Entity<LoginView>,
    banner: Entity<ConnectionBanner>,
    update_banner: Entity<crate::updater::banner::UpdateBanner>,
    first_frame: Option<FirstFrameHook>,
    /// v12/shell: draw as an inactive window (a self-test scene).
    show_inactive: bool,
    /// The app's own focus, on its root: a fresh window focuses it, so the
    /// first Tab goes through the app's `FocusNext` handler (it is not a
    /// tab stop, so that Tab lands on the first control).
    focus: FocusHandle,
    _subscriptions: Vec<Subscription>,
}

impl RootView {
    pub fn new(
        services: Services,
        first_frame: Option<FirstFrameHook>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        crate::views::nav::attach_window(window, cx);
        // Focus the app root before the views below build: a view that takes
        // the focus itself (the login field) still wins.
        let focus = cx.focus_handle();
        window.focus(&focus, cx);
        let mut subscriptions = vec![
            theme::follow_system(window, cx),
            cx.subscribe(&services.hub, |_, _, changes: &Changes, cx| {
                // Groups too: the first 群 replaces the welcome (§8.5).
                // v12/fx-shell: Connection too: the toolbar subtitle says
                // 连接已断开 · n 秒后重连 while it is down.
                if changes
                    .any(|c| matches!(c, Change::Session | Change::Groups | Change::Connection))
                {
                    cx.notify();
                }
            }),
            cx.observe(&services.nav, |_, _, cx| cx.notify()),
        ];
        // App-level banners (connection, update).
        let banner = cx.new(|cx| ConnectionBanner::new(services.clone(), cx));
        let update_banner = cx.new(crate::updater::banner::UpdateBanner::new);
        crate::views::banners::register(update_banner.clone(), banner.clone(), cx);
        let timeline = cx.new(|cx| TimelineView::new(services.clone(), cx));
        let composer = cx.new(|cx| ComposerView::new(services.clone(), window, cx));
        // 回复 and a click on an avatar land in the composer.
        let composer_for_timeline = composer.clone();
        subscriptions.push(cx.subscribe_in(
            &timeline,
            window,
            move |_, _, event: &crate::views::timeline::TimelineEvent, window, cx| {
                use crate::views::timeline::TimelineEvent;
                composer_for_timeline.update(cx, |composer, cx| match event {
                    TimelineEvent::Quote {
                        group_id,
                        topic_id,
                        message_id,
                    } => composer.quote_message(group_id, topic_id, message_id.clone(), window, cx),
                    TimelineEvent::Mention { member_id } => {
                        composer.mention_member(member_id, window, cx)
                    }
                });
            },
        ));
        let pages = Pages::new(&services, timeline.clone(), composer.clone(), cx);
        let panes = Panes::new(&services, window, cx);
        let rail = cx.new(|cx| RailView::new(services.clone(), window, cx));
        // --- v12/behavior ---
        subscriptions.extend(crate::views::nav::menus::watch_menu_state(
            &services, &rail, cx,
        ));
        // --- end v12/behavior ---
        Self {
            rail,
            timeline,
            composer,
            pages,
            panes,
            login: cx.new(|cx| LoginView::new(services.clone(), window, cx)),
            banner,
            update_banner,
            services,
            first_frame,
            show_inactive: false,
            focus,
            _subscriptions: subscriptions,
        }
    }

    pub fn services(&self) -> &Services {
        &self.services
    }

    pub fn login(&self) -> &Entity<LoginView> {
        &self.login
    }

    // --- v12/behavior ---
    /// The left rail (the screenshot scenes right-click its rows).
    pub fn rail(&self) -> &Entity<RailView> {
        &self.rail
    }
    // --- end v12/behavior ---

    pub fn timeline(&self) -> &Entity<TimelineView> {
        &self.timeline
    }

    pub fn composer(&self) -> &Entity<ComposerView> {
        &self.composer
    }

    pub fn pages(&self) -> &Pages {
        &self.pages
    }

    pub fn panes(&self) -> &Panes {
        &self.panes
    }
}

impl RootView {
    /// K-10, E3: F6 / ⌥Tab move the focus to the next region of the window
    /// (⇧ to the previous one): the sidebar, then on a 话题 page the thread
    /// and its composer, so a list is a keystroke away, not 26 Tabs.
    fn jump_region(&mut self, step: isize, window: &mut Window, cx: &mut Context<Self>) {
        let mut regions = Vec::new();
        if !self.services.nav.read(cx).sidebar_hidden {
            regions.push(self.rail.read(cx).focus_handle());
        }
        if self.services.nav.read(cx).page() == Page::Topic {
            regions.push(self.timeline.read(cx).focus_handle());
            regions.push(self.composer.read(cx).input_focus(cx));
        }
        if regions.is_empty() {
            return;
        }
        let current = regions.iter().position(|h| h.contains_focused(window, cx));
        let n = regions.len() as isize;
        let next = match current {
            Some(i) => (i as isize + step).rem_euclid(n) as usize,
            None if step < 0 => regions.len() - 1,
            None => 0,
        };
        window.focus(&regions[next], cx);
        cx.notify();
    }
}

impl Render for RootView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if let Some(hook) = self.first_frame.take() {
            window.on_next_frame(hook);
        }
        // T-04: the toasts rest on the composers this window painted last.
        crate::views::composer::floor_frame(cx);
        let k = theme::colors(cx);
        let root = div()
            .id("root")
            .track_focus(&self.focus)
            .size_full()
            .bg(k.bg)
            .text_color(k.ink);
        // v12/behavior: the 显示 commands of the rail and the toolbar panes.
        let root =
            crate::views::nav::menus::on_view_commands(root, &self.services, &self.rail)
                .on_action(cx.listener(|this, _: &ToggleSidePanel, _, cx| {
                    // v2: ⌘⌥I toggles the topic rail.
                    this.services
                        .nav
                        .update(cx, |nav, cx| nav.toggle_pane(RightPane::TopicRail, cx));
                }))
                .on_action(cx.listener(|this, _: &ToggleTopicRail, _, cx| {
                    this.services
                        .nav
                        .update(cx, |nav, cx| nav.toggle_pane(RightPane::TopicRail, cx));
                }))
                .on_action(cx.listener(|this, _: &ToggleSideChat, _, cx| {
                    let member = this.services.provider.side_chat_default_member();
                    // inc-refs: ⌘; takes the 话题 composer's chips into 单独问.
                    crate::views::refs::carry_to_side_chat(&this.services, member.as_deref(), cx);
                    this.services
                        .nav
                        .update(cx, |nav, cx| nav.toggle_side_chat(member, cx));
                }))
                .on_action(cx.listener(|this, _: &OpenDesk, _, cx| {
                    this.services.nav.update(cx, |nav, cx| nav.open_desk(cx));
                }))
                .on_action(cx.listener(|this, _: &OpenMembers, _, cx| {
                    this.services
                        .nav
                        .update(cx, |nav, cx| nav.open_members(None, cx));
                }))
                .on_action(cx.listener(|this, _: &OpenArtifacts, _, cx| {
                    this.services
                        .nav
                        .update(cx, |nav, cx| nav.open_artifacts(cx));
                }))
                .on_action(|_: &FocusNext, window, cx| window.focus_next(cx))
                .on_action(cx.listener(|this, _: &FocusNextRegion, window, cx| {
                    this.jump_region(1, window, cx)
                }))
                .on_action(cx.listener(|this, _: &FocusPrevRegion, window, cx| {
                    this.jump_region(-1, window, cx)
                }))
                .on_action(|_: &FocusPrev, window, cx| window.focus_prev(cx))
                .on_action(cx.listener(|this, _: &ToggleSidebar, _, cx| {
                    this.services
                        .nav
                        .update(cx, |nav, cx| nav.toggle_sidebar(cx));
                }))
                .on_action(cx.listener(|this, _: &NewTopic, window, cx| {
                    // v12/overlays: ⌘N opens the 新话题 sheet (NATIVE.md §7.1).
                    let group = this.services.nav.read(cx).current_group();
                    crate::views::nav::new_topic::open_new_topic(
                        this.services.clone(),
                        group,
                        window,
                        cx,
                    );
                }))
                .on_action(cx.listener(|this, _: &MentionInComposer, window, cx| {
                    this.composer
                        .update(cx, |composer, cx| composer.insert_text("@", window, cx));
                }));
        if !matches!(
            self.services.provider.session(),
            SessionState::SignedIn { .. }
        ) {
            return root.child(self.login.clone());
        }
        // Signed in, nothing drawn has the focus: none at all, or the login
        // field that took it when the window was made (it is not drawn now).
        // The app root takes it, so the first Tab reaches the first control.
        if window.focused(cx).is_none() || self.login.read(cx).has_focus(window, cx) {
            window.focus(&self.focus, cx);
        }
        // v12/shell: the main window of NATIVE.md §2: the floating sidebar
        // (traffic lights inside it), the content running under its 8 pt
        // gap, the page column with its toolbar row, and the inspector
        // edge to edge.
        let nav = self.services.nav.read(cx);
        let rail_shown = !nav.sidebar_hidden;
        let active = !self.show_inactive && crate::ui::native::tokens::emphasized(window);
        let root = root.bg(crate::views::shell::chrome::colors(cx).content);
        let rail = rail_shown.then(|| {
            crate::ui::native::floating_sidebar(cx)
                .child(crate::views::shell::chrome::menu_accent(self.rail.clone()))
                .into_any_element()
        });
        // No 群 yet: the sidebar and the 首页 empty state (v12/fx-shell:
        // centered text and two push buttons, no illustration).
        if self.services.provider.groups().is_empty() {
            let leading = if rail_shown {
                crate::ui::native::metrics::CONTENT_LEADING
            } else {
                theme::TRAFFIC_LIGHTS_INSET
            };
            // R2-02: while the first sync runs, the 群 may still arrive: the
            // 首页 skeleton, never "还没有加入群" and its create buttons.
            let syncing = matches!(
                self.services.provider.connection(),
                crate::data::ConnectionState::Connecting
            );
            let page = if syncing {
                crate::views::shell::pages::first_sync_page(leading, active, cx)
            } else {
                crate::views::shell::pages::empty_page(leading, active, window, cx)
            };
            return root.flex().flex_row().children(rail).child(page);
        }
        let page = nav.page();
        let pane = nav.visible_pane();
        let window_w = window.viewport_size().width;
        let rail_w = if rail_shown {
            crate::views::shell::chrome::sidebar_room()
        } else {
            px(0.)
        };
        let pane_w = self
            .panes
            .fitted_width(&pane, window_w, window_w - rail_w, cx);
        // A pane takes its own column while the page keeps its minimum;
        // in a narrower window it floats over the page (level 2).
        let floats = window_w - rail_w - pane_w < theme::PAGE_MIN_WIDTH;
        // v12/fx-shell: one leading content inset. The title starts 20 pt
        // after the visible sidebar edge (`CONTENT_LEADING` from the page
        // column), or past the traffic lights when the sidebar is hidden;
        // then the page column shifts by the sidebar's 8 pt gap so lists
        // still start 20 pt from the window edge.
        let leading = if rail_shown {
            crate::ui::native::metrics::CONTENT_LEADING
        } else {
            theme::TRAFFIC_LIGHTS_INSET - crate::ui::native::metrics::SIDEBAR_INSET
        };
        let page_el = self
            .pages
            .render(page, &pane, leading, active, &self.services, window, cx);
        let page_el = if rail_shown {
            page_el
        } else {
            div()
                .flex_1()
                .min_w(px(0.))
                .h_full()
                .flex()
                .pl(crate::ui::native::metrics::SIDEBAR_INSET)
                .bg(crate::views::shell::chrome::colors(cx).content)
                .child(page_el)
                .into_any_element()
        };
        let pane_el = self.panes.render(&pane, pane_w, floats, cx);
        root.flex()
            .flex_row()
            .relative()
            .children(rail)
            .child(page_el)
            .children(pane_el)
    }
}

// --- v12/shell ---
impl RootView {
    /// Draws the window as an inactive window (the `window-inactive`
    /// scene): gray selections and `secondary` toolbar symbols (§4).
    pub fn show_inactive(&mut self, cx: &mut Context<Self>) {
        self.show_inactive = true;
        self.rail
            .update(cx, |rail, cx| rail.set_show_inactive(true, cx));
        cx.notify();
    }
}
// --- end v12/shell ---

/// Options of the main window. `focus: false` keeps self-tests from taking focus.
pub fn main_window_options(focus: bool, cx: &App) -> WindowOptions {
    WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
            None,
            size(theme::WINDOW_DEFAULT_WIDTH, theme::WINDOW_DEFAULT_HEIGHT),
            cx,
        ))),
        titlebar: Some(TitlebarOptions {
            title: Some(env!("PRODUCT_NAME").into()),
            // v12/shell: a full-size content view; the traffic lights sit
            // inside the floating sidebar, on the toolbar row's centre line
            // (NATIVE.md §2).
            appears_transparent: true,
            traffic_light_position: Some(point(theme::TRAFFIC_LIGHTS.0, theme::TRAFFIC_LIGHTS.1)),
        }),
        window_background: WindowBackgroundAppearance::Opaque,
        window_min_size: Some(size(theme::WINDOW_MIN_WIDTH, theme::WINDOW_MIN_HEIGHT)),
        focus,
        show: true,
        ..Default::default()
    }
}

/// Opens the main window with a Root from gpui-component (needed for dialogs,
/// notifications, popovers and inputs).
pub fn open_main_window(
    services: Services,
    focus: bool,
    first_frame: Option<FirstFrameHook>,
    cx: &mut App,
) -> Result<Entity<RootView>, String> {
    let options = main_window_options(focus, cx);
    let (_, view) = gpui_kit::open_window(options, cx, move |window, cx| {
        // The app keeps the rail mode and theme of 设置 → 外观; self-tests
        // (no focus) always draw the defaults.
        if focus {
            let prefs = crate::views::shell::layout::load();
            theme::set_theme_choice(prefs.theme);
            services
                .nav
                .update(cx, |nav, cx| nav.set_rail_mode(prefs.rail_mode, cx));
        }
        cx.new(|cx| RootView::new(services, first_frame, window, cx))
    })
    .map_err(|e| e.to_string())?;
    Ok(view)
}

/// Opens the settings window, or brings the open one to the front.
pub fn open_settings_window(services: Services, cx: &mut App) {
    crate::views::settings::open(services, cx);
}

/// The keys of the views that need no services: Tab through the window's
/// tab stops and the arrow keys of the lists. The screenshot self-test binds
/// them too, so its key scenes press the app's real keys.
pub fn bind_view_keys(cx: &mut App) {
    cx.bind_keys([
        // Tab moves keyboard focus through the window's tab stops (§11);
        // inputs keep their own Tab binding.
        KeyBinding::new("tab", FocusNext, None),
        KeyBinding::new("shift-tab", FocusPrev, None),
        KeyBinding::new("f6", FocusNextRegion, None),
        KeyBinding::new("shift-f6", FocusPrevRegion, None),
        KeyBinding::new("alt-tab", FocusNextRegion, None),
        KeyBinding::new("alt-shift-tab", FocusPrevRegion, None),
    ]);
    crate::views::topic_list::bind_keys(cx);
    // --- v12/behavior ---
    crate::views::rail::bind_keys(cx);
    crate::views::timeline::bind_keys(cx);
    crate::views::members_page::bind_keys(cx);
    // --- end v12/behavior ---
}

/// Key bindings, menus and global actions. Call once after `gpui_kit::init`.
pub fn install_menus_and_actions(services: Services, cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("cmd-q", Quit, None),
        KeyBinding::new("cmd-,", OpenSettings, None),
        KeyBinding::new("cmd-alt-i", ToggleSidePanel, None),
        // ⌃⌘S is the Mac's 显示侧栏; ⇧⌘S stays 另存为 (NATIVE.md §10).
        KeyBinding::new("ctrl-cmd-s", ToggleSidebar, None),
        KeyBinding::new("cmd-n", NewTopic, None),
        KeyBinding::new("cmd-;", ToggleSideChat, None),
        KeyBinding::new("cmd-alt-r", ToggleTopicRail, None),
        KeyBinding::new("cmd-shift-h", OpenDesk, None),
        KeyBinding::new("cmd-shift-m", OpenMembers, None),
        KeyBinding::new("cmd-shift-o", OpenArtifacts, None),
    ]);
    cx.on_action(|_: &Quit, cx| cx.quit());
    cx.on_action(move |_: &OpenSettings, cx| open_settings_window(services.clone(), cx));
    crate::views::nav::menus::install(cx);
    bind_view_keys(cx);
    // --- v12/shell: the accent of System Settings (NATIVE.md §4) ---
    theme::watch_system_accent(cx);
    // --- end v12/shell ---
    crate::ui::motion::watch_reduce_motion(cx);
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use gpui::{TestAppContext, VisualTestContext};

    use super::*;

    /// The first Tab in a fresh signed-in window reaches a control: the
    /// login field that took the focus when the window was made is not
    /// drawn, so the app root takes it back (v15 tail, R1 report).
    #[gpui::test]
    fn the_first_tab_reaches_a_control(cx: &mut TestAppContext) {
        cx.update(|cx| {
            gpui_component::init(cx);
            bind_view_keys(cx);
        });
        let provider: Arc<dyn ChatProvider> = crate::fake::FakeProvider::new();
        let root = std::rc::Rc::new(std::cell::RefCell::new(None));
        let keep = root.clone();
        let window = cx
            .update(|cx| {
                let services = Services::new(provider, cx);
                cx.open_window(Default::default(), move |window, cx| {
                    let view = cx.new(|cx| RootView::new(services, None, window, cx));
                    *keep.borrow_mut() = Some(view.clone());
                    cx.new(|cx| gpui_base::Root::new(view, window, cx))
                })
            })
            .expect("the window opens");
        let cx = &mut VisualTestContext::from_window(window.into(), cx);
        cx.update(|window, _| window.activate_window());
        cx.run_until_parked();
        let view = root.borrow().clone().expect("the root view");
        let (app_focus, login) = view.read_with(cx, |v, _| (v.focus.clone(), v.login.clone()));
        cx.update(|window, _| {
            assert!(app_focus.is_focused(window), "the app root holds the focus");
        });
        cx.simulate_keystrokes("tab");
        cx.update(|window, cx| {
            assert!(window.focused(cx).is_some(), "Tab focused something");
            assert!(!app_focus.is_focused(window), "Tab left the app root");
            assert!(
                !login.read(cx).has_focus(window, cx),
                "not the hidden login field"
            );
        });
    }

    /// K-10: F6 walks the regions (sidebar, thread, composer) and ⇧F6 walks
    /// back, so a list is one keystroke away.
    #[gpui::test]
    fn f6_jumps_between_regions(cx: &mut TestAppContext) {
        cx.update(|cx| {
            gpui_component::init(cx);
            bind_view_keys(cx);
        });
        let provider: Arc<dyn ChatProvider> = crate::fake::FakeProvider::new();
        let root = std::rc::Rc::new(std::cell::RefCell::new(None));
        let keep = root.clone();
        let window = cx
            .update(|cx| {
                let services = Services::new(provider, cx);
                services.nav.update(cx, |nav, cx| {
                    nav.open_topic("g-dev".into(), "t-api".into(), cx)
                });
                cx.open_window(Default::default(), move |window, cx| {
                    let view = cx.new(|cx| RootView::new(services, None, window, cx));
                    *keep.borrow_mut() = Some(view.clone());
                    cx.new(|cx| gpui_base::Root::new(view, window, cx))
                })
            })
            .expect("the window opens");
        let cx = &mut VisualTestContext::from_window(window.into(), cx);
        cx.update(|window, _| window.activate_window());
        cx.run_until_parked();
        let view = root.borrow().clone().expect("the root view");
        let (rail, thread, composer) = view.read_with(cx, |v, cx| {
            (
                v.rail.read(cx).focus_handle(),
                v.timeline.read(cx).focus_handle(),
                v.composer.read(cx).input_focus(cx),
            )
        });
        let at = |h: &FocusHandle, cx: &mut VisualTestContext| {
            cx.update(|window, cx| h.contains_focused(window, cx))
        };
        cx.simulate_keystrokes("f6");
        assert!(at(&rail, cx), "F6 goes to the sidebar first");
        cx.simulate_keystrokes("f6");
        assert!(at(&thread, cx), "then the thread");
        cx.simulate_keystrokes("f6");
        assert!(at(&composer, cx), "then the composer");
        cx.simulate_keystrokes("shift-f6");
        assert!(at(&thread, cx), "shift-F6 goes back");
    }
}
