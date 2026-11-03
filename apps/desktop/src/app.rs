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
    App, AppContext, Bounds, Context, Entity, EventEmitter, InteractiveElement, IntoElement,
    KeyBinding, ParentElement, Render, Styled, Subscription, TitlebarOptions, Window,
    WindowBackgroundAppearance, WindowBounds, WindowOptions, div, point, px, size,
};

use crate::{
    data::{Change, ChatProvider, GroupId, MemberId, SessionState, TopicId},
    theme, ui,
    views::{
        composer::ComposerView,
        connection_banner::ConnectionBanner,
        login::LoginView,
        rail::RailView,
        shell::{pages::Pages, panes::Panes},
        sidebar::Sidebar,
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
        let serial = self.focus.as_ref().map_or(0, |f| f.serial) + 1;
        self.place = Some(Place::Group(group_id.clone()));
        self.topic = Some((group_id.clone(), topic_id.clone()));
        self.focus = Some(FocusRequest {
            group_id,
            topic_id,
            message_id,
            seq,
            serial,
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
        // Pages area (1.1.0 fixes): the ink disc of my own avatar.
        crate::ui::avatar::track_viewer(&services, cx);
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
    /// The v1 sidebar: not drawn, kept for the navigation self-test until
    /// the cutover deletes it.
    sidebar: Entity<Sidebar>,
    timeline: Entity<TimelineView>,
    composer: Entity<ComposerView>,
    pages: Pages,
    panes: Panes,
    login: Entity<LoginView>,
    banner: Entity<ConnectionBanner>,
    update_banner: Entity<crate::updater::banner::UpdateBanner>,
    first_frame: Option<FirstFrameHook>,
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
        let subscriptions = vec![
            theme::follow_system(window, cx),
            cx.subscribe(&services.hub, |_, _, changes: &Changes, cx| {
                // Groups too: the first 群 replaces the welcome (§8.5).
                if changes.any(|c| matches!(c, Change::Session | Change::Groups)) {
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
        let pages = Pages::new(&services, timeline.clone(), composer.clone(), cx);
        let panes = Panes::new(&services, window, cx);
        Self {
            rail: cx.new(|cx| RailView::new(services.clone(), cx)),
            sidebar: cx.new(|cx| Sidebar::new(services.clone(), cx)),
            timeline,
            composer,
            pages,
            panes,
            login: cx.new(|cx| LoginView::new(services.clone(), window, cx)),
            banner,
            update_banner,
            services,
            first_frame,
            _subscriptions: subscriptions,
        }
    }

    pub fn services(&self) -> &Services {
        &self.services
    }

    pub fn login(&self) -> &Entity<LoginView> {
        &self.login
    }

    pub fn sidebar(&self) -> &Entity<Sidebar> {
        &self.sidebar
    }

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

impl Render for RootView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if let Some(hook) = self.first_frame.take() {
            window.on_next_frame(hook);
        }
        let k = theme::colors(cx);
        let root = div()
            .id("root")
            .size_full()
            .bg(k.bg)
            .text_color(k.ink)
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
            .on_action(|_: &FocusPrev, window, cx| window.focus_prev(cx))
            .on_action(cx.listener(|this, _: &ToggleSidebar, _, cx| {
                this.services
                    .nav
                    .update(cx, |nav, cx| nav.toggle_sidebar(cx));
            }))
            .on_action(cx.listener(|this, _: &NewTopic, _, cx| {
                this.services.nav.update(cx, |nav, cx| nav.new_topic(cx));
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
        let nav = self.services.nav.read(cx);
        let rail_shown = !nav.sidebar_hidden;
        let rail = rail_shown.then(|| {
            div()
                .w(theme::RAIL_WIDTH)
                .h_full()
                .flex_none()
                .relative()
                .child(self.rail.clone())
                .child(ui::column_edge(cx))
                .into_any_element()
        });
        // No 群 yet: the rail and one welcome, nothing else (§8.5, U06).
        if self.services.provider.groups().is_empty() {
            return root.flex().flex_row().children(rail).child(
                div()
                    .flex_1()
                    .h_full()
                    .child(crate::views::welcome::Welcome),
            );
        }
        let page = nav.page();
        let pane = nav.visible_pane();
        let window_w = window.viewport_size().width;
        let rail_w = if rail_shown {
            theme::RAIL_WIDTH
        } else {
            px(0.)
        };
        let pane_w = self
            .panes
            .fitted_width(&pane, window_w, window_w - rail_w, cx);
        // A pane takes its own column while the page keeps its minimum;
        // in a narrower window it floats over the page (level 2).
        let floats = window_w - rail_w - pane_w < theme::PAGE_MIN_WIDTH;
        let leading = if rail_shown {
            px(0.)
        } else {
            theme::TRAFFIC_LIGHTS_INSET
        };
        let page_el = self.pages.render(page, &pane, leading, &self.services, cx);
        let pane_el = self.panes.render(&pane, pane_w, floats, cx);
        root.flex()
            .flex_row()
            .relative()
            .children(rail)
            .child(page_el)
            .children(pane_el)
    }
}

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
            // Each column draws its own 52 pt toolbar band; the traffic
            // lights sit in the sidebar's band (§5.1).
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

/// Key bindings, menus and global actions. Call once after `gpui_kit::init`.
pub fn install_menus_and_actions(services: Services, cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("cmd-q", Quit, None),
        KeyBinding::new("cmd-,", OpenSettings, None),
        KeyBinding::new("cmd-alt-i", ToggleSidePanel, None),
        KeyBinding::new("cmd-shift-s", ToggleSidebar, None),
        KeyBinding::new("cmd-n", NewTopic, None),
        KeyBinding::new("cmd-;", ToggleSideChat, None),
        KeyBinding::new("cmd-alt-r", ToggleTopicRail, None),
        KeyBinding::new("cmd-shift-h", OpenDesk, None),
        KeyBinding::new("cmd-shift-m", OpenMembers, None),
        KeyBinding::new("cmd-shift-o", OpenArtifacts, None),
        // Tab moves keyboard focus through the window's tab stops (§11);
        // inputs keep their own Tab binding.
        KeyBinding::new("tab", FocusNext, None),
        KeyBinding::new("shift-tab", FocusPrev, None),
    ]);
    cx.on_action(|_: &Quit, cx| cx.quit());
    cx.on_action(move |_: &OpenSettings, cx| open_settings_window(services.clone(), cx));
    crate::views::nav::menus::install(cx);
    crate::views::topic_list::bind_keys(cx);
    crate::ui::motion::watch_reduce_motion(cx);
}
