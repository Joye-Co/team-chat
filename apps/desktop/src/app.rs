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
    KeyBinding, ParentElement, Render, Styled, Subscription, TitlebarOptions, Window, WindowBounds,
    WindowOptions, div, px, size,
};

use crate::{
    copy,
    data::{Change, ChatProvider, GroupId, SessionState, TopicId},
    theme,
    views::{
        composer::ComposerView, connection_banner::ConnectionBanner, login::LoginView, settings::SettingsView, side_panel::SidePanel,
        sidebar::Sidebar, timeline::TimelineView, topic_list::TopicListView,
    },
};

gpui::actions!(desktop, [Quit, OpenSettings, ToggleSidePanel, NewTopic]);

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
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Place {
    /// The 待办 list.
    Todo,
    Group(GroupId),
}

/// Navigation state shared by the views of one main window.
pub struct Navigation {
    pub place: Option<Place>,
    /// The open topic: (group id, topic id). `None` with a group place means
    /// the composer starts a new topic.
    pub topic: Option<(GroupId, TopicId)>,
    pub side_panel_open: bool,
    pub developer_view: bool,
    /// The side panel tab: 0 = 详情, 1 = 过程.
    pub side_panel_tab: usize,
    /// A message to scroll to and highlight (from search).
    pub focus: Option<FocusRequest>,
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
            side_panel_open: true,
            developer_view: false,
            side_panel_tab: 0,
            focus: None,
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
        self.side_panel_open = !self.side_panel_open;
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
}

impl Services {
    pub fn new(provider: Arc<dyn ChatProvider>, cx: &mut App) -> Self {
        let hub = ChangeHub::start(&provider, cx);
        let nav = cx.new(|_| Navigation::new());
        Self { provider, hub, nav }
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

/// The root view of the main window: login, or sidebar + topic list + topic view + side panel.
pub struct RootView {
    services: Services,
    sidebar: Entity<Sidebar>,
    topic_list: Entity<TopicListView>,
    timeline: Entity<TimelineView>,
    composer: Entity<ComposerView>,
    side_panel: Entity<SidePanel>,
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
                if changes.any(|c| matches!(c, Change::Session)) {
                    cx.notify();
                }
            }),
            cx.observe(&services.nav, |_, _, cx| cx.notify()),
        ];
        Self {
            sidebar: cx.new(|cx| Sidebar::new(services.clone(), cx)),
            topic_list: cx.new(|cx| TopicListView::new(services.clone(), cx)),
            timeline: cx.new(|cx| TimelineView::new(services.clone(), cx)),
            composer: cx.new(|cx| ComposerView::new(services.clone(), window, cx)),
            side_panel: cx.new(|cx| SidePanel::new(services.clone(), cx)),
            login: cx.new(|cx| LoginView::new(services.clone(), window, cx)),
            banner: cx.new(|cx| ConnectionBanner::new(services.clone(), cx)),
            update_banner: cx.new(crate::updater::banner::UpdateBanner::new),
            services,
            first_frame,
            _subscriptions: subscriptions,
        }
    }

    pub fn services(&self) -> &Services {
        &self.services
    }

    pub fn timeline(&self) -> &Entity<TimelineView> {
        &self.timeline
    }
}

impl Render for RootView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if let Some(hook) = self.first_frame.take() {
            window.on_next_frame(hook);
        }
        let colors = theme::colors(cx);
        let root = div()
            .id("root")
            .size_full()
            .bg(colors.background)
            .text_color(colors.foreground)
            .on_action(cx.listener(|this, _: &ToggleSidePanel, _, cx| {
                this.services
                    .nav
                    .update(cx, |nav, cx| nav.toggle_side_panel(cx));
            }))
            .on_action(cx.listener(|this, _: &NewTopic, _, cx| {
                this.services.nav.update(cx, |nav, cx| nav.new_topic(cx));
            }));
        if !matches!(
            self.services.provider.session(),
            SessionState::SignedIn { .. }
        ) {
            return root.child(self.login.clone());
        }
        let panel_open = self.services.nav.read(cx).side_panel_open;
        root.flex()
            .flex_row()
            .children([
                div()
                    .w(theme::SIDEBAR_WIDTH)
                    .h_full()
                    .flex_none()
                    .child(self.sidebar.clone())
                    .into_any_element(),
                div()
                    .w(theme::TOPIC_LIST_WIDTH)
                    .h_full()
                    .flex_none()
                    .border_r_1()
                    .border_color(colors.border)
                    .child(self.topic_list.clone())
                    .into_any_element(),
                div()
                    .flex_1()
                    .min_w(px(360.))
                    .h_full()
                    .flex()
                    .flex_col()
                    .child(self.banner.clone())
                    .child(self.update_banner.clone())
                    .child(div().flex_1().min_h(px(0.)).child(self.timeline.clone()))
                    .child(self.composer.clone())
                    .into_any_element(),
            ])
            .when_some(panel_open.then(|| self.side_panel.clone()), |el, panel| {
                el.child(
                    div()
                        .w(theme::SIDE_PANEL_WIDTH)
                        .h_full()
                        .flex_none()
                        .border_l_1()
                        .border_color(colors.border)
                        .child(panel),
                )
            })
    }
}

use gpui::prelude::FluentBuilder as _;

/// Options of the main window. `focus: false` keeps self-tests from taking focus.
pub fn main_window_options(focus: bool, cx: &App) -> WindowOptions {
    WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
            None,
            size(px(1200.), px(780.)),
            cx,
        ))),
        titlebar: Some(TitlebarOptions {
            title: Some(env!("PRODUCT_NAME").into()),
            ..Default::default()
        }),
        window_min_size: Some(size(px(900.), px(560.))),
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
        cx.new(|cx| RootView::new(services, first_frame, window, cx))
    })
    .map_err(|e| e.to_string())?;
    Ok(view)
}

/// Opens the settings window.
pub fn open_settings_window(services: Services, cx: &mut App) {
    let options = WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
            None,
            size(px(640.), px(560.)),
            cx,
        ))),
        titlebar: Some(TitlebarOptions {
            title: Some(copy::settings::TITLE.into()),
            ..Default::default()
        }),
        ..Default::default()
    };
    let _ = gpui_kit::open_window(options, cx, move |window, cx| {
        cx.new(|cx| SettingsView::new(services, window, cx))
    });
}

/// Key bindings, menus and global actions. Call once after `gpui_kit::init`.
pub fn install_menus_and_actions(services: Services, cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("cmd-q", Quit, None),
        KeyBinding::new("cmd-,", OpenSettings, None),
        KeyBinding::new("cmd-alt-i", ToggleSidePanel, None),
        KeyBinding::new("cmd-n", NewTopic, None),
    ]);
    cx.on_action(|_: &Quit, cx| cx.quit());
    cx.on_action(move |_: &OpenSettings, cx| open_settings_window(services.clone(), cx));
    crate::views::nav::menus::install(cx);
}
