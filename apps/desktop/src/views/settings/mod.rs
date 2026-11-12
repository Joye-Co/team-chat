//! The settings window (⌘, and App menu › 设置…; NATIVE.md §8): toolbar tabs
//! in the Mail Settings style over one pane of grouped forms. The window is
//! 640 wide, its height follows the pane, its title is the pane name and it
//! reopens on the pane shown last. Changes apply at once; there is no Save.
//! One window at a time: opening it again brings the open one to the front.
//!
//! Panes read snapshots from the provider and start requests that answer
//! through a [`crate::data::AdminReply`], awaited off the UI thread.

mod account;
mod account_phone;
pub mod ai_service;
pub mod appearance;
pub(crate) mod bot_quick;
pub(crate) mod general;
pub mod login_item;
pub(crate) mod machine_actions;
pub(crate) mod machines;
pub(crate) mod members;
pub mod worker_models;
// pages track: the 成员 page reuses the invite and add-member forms.
pub(crate) use members::{open_add_bot, open_invite};
mod notifications;
pub mod prefs;
pub mod selftest;
mod shortcuts;
pub use shortcuts::apply_saved as apply_saved_shortcuts;
pub(crate) mod widgets;
mod window_fit;

use std::rc::Rc;

use gpui::{
    AnyWindowHandle, App, AppContext as _, Bounds, Context, Entity, Global, InteractiveElement,
    IntoElement, MouseButton, ParentElement, Render, StatefulInteractiveElement as _, Styled,
    Subscription, TitlebarOptions, Window, WindowBounds, WindowOptions, canvas, div, px, size,
};
use gpui_component::button::Button;

use crate::{
    app::Services,
    copy::settings as s,
    data::{MemberLevel, SessionState, TeamRole},
    theme,
    ui::{
        Sym,
        native::{NativeType as _, emphasized, native_colors},
    },
};

pub use account::AccountPage;
pub use appearance::AppearancePage;
pub use general::GeneralPage;
pub use machines::MachinesPage;
pub use members::MembersPage;
pub use notifications::NotificationsPage;
pub use shortcuts::ShortcutsPage;

/// The panes, in toolbar order (§8).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Page {
    General,
    Account,
    Notifications,
    Members,
    /// 团队约定 (沉淀, team scope).
    Memory,
    /// 我的偏好 (沉淀, my own scope); 我的约定 when one person uses the server.
    Mine,
    Machines,
    Appearance,
    Shortcuts,
}

impl Page {
    pub const ALL: [Page; 9] = [
        Page::General,
        Page::Account,
        Page::Notifications,
        Page::Members,
        Page::Memory,
        Page::Mine,
        Page::Machines,
        Page::Appearance,
        Page::Shortcuts,
    ];

    /// The pane name: the tab label and the window title. [`SettingsView::title_of`]
    /// says 我的约定 for [`Page::Mine`] when one person uses the server.
    pub fn title(self) -> &'static str {
        match self {
            Page::General => s::PANE_GENERAL,
            Page::Account => s::PANE_ACCOUNT,
            Page::Notifications => s::PANE_NOTIFICATIONS,
            Page::Members => s::PANE_MEMBERS,
            Page::Memory => crate::copy::memory::TEAM_TITLE,
            Page::Mine => crate::copy::memory::PERSON_TITLE,
            Page::Machines => s::PANE_MACHINES,
            Page::Appearance => s::PANE_APPEARANCE,
            Page::Shortcuts => s::PANE_SHORTCUTS,
        }
    }

    /// The tab symbol (§6 Settings tabs).
    pub fn symbol(self) -> Sym {
        match self {
            Page::General => Sym::General,
            Page::Account => Sym::Account,
            Page::Notifications => Sym::Bell,
            Page::Members => Sym::Members,
            Page::Memory | Page::Mine => Sym::Book,
            // desktopcomputer: its ink fills the 18 box; macmini is 22 × 10
            // (2.19, C1-09).
            Page::Machines => Sym::Computer,
            Page::Appearance => Sym::Appearance,
            Page::Shortcuts => Sym::Shortcuts,
        }
    }

    /// Short name for self-tests, screenshots and the saved last pane.
    pub fn slug(self) -> &'static str {
        match self {
            Page::General => "general",
            Page::Account => "account",
            Page::Notifications => "notifications",
            Page::Members => "members",
            Page::Memory => "memory",
            Page::Mine => "mine",
            Page::Machines => "machines",
            Page::Appearance => "appearance",
            Page::Shortcuts => "shortcuts",
        }
    }

    /// The tabs of the settings window: my own preferences, and 电脑, where every member adds
    /// their own computers (design-roles 3.9). 成员 (with 默认规则) and 团队约定 live in 团队管理
    /// (`views::admin`); their panes stay here for the self-tests.
    pub const TABS: [Page; 7] = [
        Page::General,
        Page::Account,
        Page::Notifications,
        Page::Mine,
        Page::Machines,
        Page::Appearance,
        Page::Shortcuts,
    ];

    /// The pane belongs to 团队管理, not to the settings window.
    pub fn in_admin(self) -> bool {
        !Self::TABS.contains(&self)
    }

    pub fn from_slug(slug: &str) -> Option<Page> {
        Page::ALL.into_iter().find(|p| p.slug() == slug)
    }
}

/// True when I am signed in as a 管理员 or a 所有者 (the admin actions show). The name stays
/// from before roles: the wire level `owner` is both (design-roles 3.9).
pub fn i_am_owner(services: &Services) -> bool {
    matches!(
        services.provider.session(),
        SessionState::SignedIn { me, .. } if me.level == MemberLevel::Owner
    )
}

/// My member id, `None` when signed out.
pub fn my_id(services: &Services) -> Option<String> {
    match services.provider.session() {
        SessionState::SignedIn { me, .. } => Some(me.id),
        _ => None,
    }
}

/// My 身份, `None` when signed out.
pub fn my_role(services: &Services) -> Option<TeamRole> {
    let SessionState::SignedIn { me, .. } = services.provider.session() else {
        return None;
    };
    // The directory has the newest copy of me, mark included.
    let all = services.provider.members();
    let me = all.iter().find(|m| m.id == me.id).cloned().unwrap_or(me);
    Some(TeamRole::of(&me, &all))
}

/// True when I am the team's 所有者: I change 身份 and stop 管理员 and 所有者.
pub fn i_am_team_owner(services: &Services) -> bool {
    my_role(services) == Some(TeamRole::Owner)
}

/// One person uses this team: exactly one active human member. Then nothing about 身份 (and,
/// for the computers, nothing about 团队 or 我的) shows (design-roles 3.9, 3.10).
pub fn solo(services: &Services) -> bool {
    solo_in(&services.provider.members())
}

pub fn solo_in(members: &[crate::data::Member]) -> bool {
    members
        .iter()
        .filter(|m| !m.deactivated && !m.is_bot())
        .count()
        == 1
}

/// The word of a 身份: 所有者, 管理员 or 普通成员.
pub fn role_text(role: TeamRole) -> &'static str {
    match role {
        TeamRole::Owner => s::ROLE_TEAM_OWNER,
        TeamRole::Admin => s::LEVEL_OWNER,
        TeamRole::Member => s::LEVEL_MEMBER,
    }
}

/// The 「设为…」 item of a 身份.
pub fn set_role_text(role: TeamRole) -> &'static str {
    match role {
        TeamRole::Owner => s::SET_ROLE_OWNER,
        TeamRole::Admin => s::SET_ROLE_ADMIN,
        TeamRole::Member => s::SET_ROLE_MEMBER,
    }
}

/// The 身份 a 所有者 can give `target` from its row (design-roles 3.9): the other two, and never
/// 所有者 to an AI member. Empty for myself, a deactivated member, or when I am not the 所有者.
pub fn roles_to_offer(
    target: &crate::data::Member,
    current: TeamRole,
    me: Option<&str>,
    i_am_team_owner: bool,
) -> Vec<TeamRole> {
    if !i_am_team_owner || target.deactivated || me == Some(target.id.as_str()) {
        return Vec::new();
    }
    [TeamRole::Owner, TeamRole::Admin, TeamRole::Member]
        .into_iter()
        .filter(|r| *r != current && !(target.is_bot() && *r == TeamRole::Owner))
        .collect()
}

/// True when I may stop (or bring back) `target`: a 管理员 handles 普通成员 and AI members at
/// 普通成员; a 管理员 or 所有者 target needs the 所有者 (design-roles 3.2).
pub fn may_deactivate(target_role: TeamRole, i_am_team_owner: bool) -> bool {
    i_am_team_owner || target_role == TeamRole::Member
}

/// True when a member's "⋯" menu has an item: an active AI member's own items, a 「设为…」, or
/// 停用 / 恢复. A deactivated AI member a 管理员 may not bring back has none, so no button.
pub fn has_member_menu(target: &crate::data::Member, offer: &[TeamRole], may_stop: bool) -> bool {
    may_stop || !offer.is_empty() || (target.is_bot() && !target.deactivated)
}

/// The text of a failed 身份 change: refused means only the 所有者 may do it.
pub fn role_error_text(e: crate::data::AdminError) -> &'static str {
    match e {
        crate::data::AdminError::Forbidden => s::ERR_OWNER_ONLY,
        e => widgets::error_text(e),
    }
}

/// The title row that holds the traffic lights and the window title.
const TITLE_BAND: gpui::Pixels = px(32.);
/// Toolbar tabs: symbol 18 pt over an 11 pt label, 56 pt tall (§8).
const TAB_HEIGHT: gpui::Pixels = px(56.);
const TAB_MIN_WIDTH: gpui::Pixels = px(60.);
const TAB_RADIUS: gpui::Pixels = px(8.);
const TAB_GAP: gpui::Pixels = px(2.);
/// Space under the tabs, above the hairline.
const TABS_BOTTOM: gpui::Pixels = px(6.);
/// The whole toolbar: title band, tabs and the hairline under them.
pub const TOOLBAR_HEIGHT: gpui::Pixels = px(32. + 56. + 6. + 1.);
/// The traffic lights' origin: their centers on the title band's middle.
const TRAFFIC_LIGHTS: (gpui::Pixels, gpui::Pixels) = (px(13.), px(10.));
/// Window width (§8) and the height range the pane may make it.
pub const WINDOW_WIDTH: gpui::Pixels = px(640.);
pub const MIN_HEIGHT: gpui::Pixels = px(240.);
pub const MAX_HEIGHT: gpui::Pixels = px(760.);

pub struct SettingsView {
    page: Page,
    general: Entity<GeneralPage>,
    account: Entity<AccountPage>,
    notifications: Entity<NotificationsPage>,
    members: Entity<MembersPage>,
    memory: Entity<crate::views::memory::MemoryBoard>,
    /// 我的偏好, or 我的约定 when one person uses the server.
    mine: Entity<crate::views::memory::MemoryBoard>,
    machines: Entity<MachinesPage>,
    appearance: Entity<AppearancePage>,
    shortcuts: Entity<ShortcutsPage>,
    /// The pane scroll, so a scene can show the end of a long pane.
    scroll: gpui::ScrollHandle,
    /// The pane's own height, measured as it is laid out.
    fit: Rc<window_fit::Fit>,
    /// The window's own focus, on its root: a fresh window focuses it, so
    /// Tab goes through the `FocusNext` handler to the first control (it is
    /// not a tab stop itself).
    focus: gpui::FocusHandle,
    /// The pane whose name the window title shows, so the title is set when
    /// the pane changes, not on every frame.
    titled: Option<&'static str>,
    _subscriptions: Vec<Subscription>,
}

impl SettingsView {
    pub fn new(services: Services, window: &mut Window, cx: &mut Context<Self>) -> Self {
        Self::new_at(services, last_page(), window, cx)
    }

    pub fn new_at(
        services: Services,
        page: Page,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let general = cx.new(|cx| GeneralPage::new(services.clone(), window, cx));
        let account = cx.new(|cx| AccountPage::new(services.clone(), window, cx));
        let notifications = cx.new(|cx| NotificationsPage::new(services.clone(), window, cx));
        let members = cx.new(|cx| MembersPage::new(services.clone(), window, cx));
        let memory = cx.new(|cx| {
            crate::views::memory::MemoryBoard::new(
                services.clone(),
                crate::data::memory::MemoryScope::Team,
                crate::views::memory::Host::Settings,
                String::new(),
                cx,
            )
        });
        // The board follows the signed-in person itself: this window outlives an account
        // change, and before sign-in it reads nothing.
        let mine = cx.new(|cx| {
            crate::views::memory::MemoryBoard::new(
                services.clone(),
                crate::data::memory::MemoryScope::Person(services.me().unwrap_or_default()),
                crate::views::memory::Host::Settings,
                String::new(),
                cx,
            )
        });
        // Its name (我的偏好 or 我的约定) is the tab's and the window's.
        let mine_changed = cx.observe(&mine, |_, _, cx| cx.notify());
        let machines = cx.new(|cx| MachinesPage::new(services.clone(), window, cx));
        let appearance = cx.new(|cx| AppearancePage::new(services.clone(), window, cx));
        let shortcuts = cx.new(|cx| ShortcutsPage::new(window, cx));
        let focus = cx.focus_handle();
        window.focus(&focus, cx);
        let mut this = Self {
            page,
            general,
            account,
            notifications,
            members,
            memory,
            mine,
            machines,
            appearance,
            shortcuts,
            scroll: gpui::ScrollHandle::new(),
            fit: Rc::new(window_fit::Fit::default()),
            focus,
            titled: None,
            _subscriptions: vec![
                mine_changed,
                theme::follow_system(window, cx),
                // Back in front: the login item may have changed in 系统设置.
                cx.observe_window_activation(window, |this, window, cx| {
                    if window.is_window_active() && this.page == Page::General {
                        login_item::refresh();
                        cx.notify();
                    }
                }),
            ],
        };
        window_fit::dim_window_buttons(window);
        window_fit::pin_content_to_top(window);
        window_fit::hide_until_fitted(window);
        this.show(page, window, cx);
        this
    }

    pub fn page(&self) -> Page {
        self.page
    }

    /// Shows `page`: the window title becomes its name and it is the pane
    /// the window opens on next time.
    pub fn show(&mut self, page: Page, window: &mut Window, cx: &mut Context<Self>) {
        if page != self.page {
            self.scroll.set_offset(gpui::point(px(0.), px(0.)));
        }
        self.page = page;
        let title = self.title_of(page, cx);
        window.set_window_title(title);
        self.titled = Some(title);
        prefs::update(|p| p.last_pane = Some(page.slug().to_string()));
        match page {
            Page::General => {
                login_item::refresh();
            }
            Page::Machines => self.machines.update(cx, |m, cx| m.refresh(cx)),
            Page::Account => self.account.update(cx, |a, cx| {
                a.refresh_devices(cx);
                a.refresh_email(cx);
            }),
            Page::Notifications => self.notifications.update(cx, |n, cx| n.refresh(cx)),
            Page::Members => self.members.update(cx, |m, cx| m.refresh_rules(window, cx)),
            Page::Memory => self.memory.update(cx, |m, cx| m.load(cx)),
            Page::Mine => self.mine.update(cx, |m, cx| m.load(cx)),
            _ => {}
        }
        cx.notify();
    }

    /// [`Self::show`] for callers without the window (the self-test walks
    /// every pane): the title follows on the next frame.
    pub fn select(&mut self, page: Page, cx: &mut Context<Self>) {
        self.page = page;
        cx.notify();
    }

    /// Scrolls the pane so that `y` from its top is at the top (self-tests).
    pub fn scroll_to_y(&self, y: gpui::Pixels) {
        self.scroll.set_offset(gpui::point(px(0.), -y));
    }

    /// Scrolls the pane to its end.
    pub fn scroll_to_end(&self) {
        self.scroll.scroll_to_bottom();
    }

    pub fn general(&self) -> &Entity<GeneralPage> {
        &self.general
    }

    pub fn account(&self) -> &Entity<AccountPage> {
        &self.account
    }

    pub fn notifications(&self) -> &Entity<NotificationsPage> {
        &self.notifications
    }

    pub fn members(&self) -> &Entity<MembersPage> {
        &self.members
    }

    pub fn memory(&self) -> &Entity<crate::views::memory::MemoryBoard> {
        &self.memory
    }

    /// 我的偏好 (我的约定 when one person uses the server).
    pub fn mine(&self) -> &Entity<crate::views::memory::MemoryBoard> {
        &self.mine
    }

    pub fn machines(&self) -> &Entity<MachinesPage> {
        &self.machines
    }

    /// The name of `page` as the tab and the window title say it.
    fn title_of(&self, page: Page, cx: &App) -> &'static str {
        match page {
            Page::Mine => self.mine.read(cx).title(),
            _ => page.title(),
        }
    }

    fn render_page(&self) -> gpui::AnyElement {
        match self.page {
            Page::General => self.general.clone().into_any_element(),
            Page::Account => self.account.clone().into_any_element(),
            Page::Notifications => self.notifications.clone().into_any_element(),
            Page::Members => self.members.clone().into_any_element(),
            Page::Memory => self.memory.clone().into_any_element(),
            Page::Mine => self.mine.clone().into_any_element(),
            Page::Machines => self.machines.clone().into_any_element(),
            Page::Appearance => self.appearance.clone().into_any_element(),
            Page::Shortcuts => self.shortcuts.clone().into_any_element(),
        }
    }

    /// One toolbar tab (§8): a gpui-component button with the 18 pt symbol
    /// over the 11 pt label; the selected one has the `selection` fill and
    /// `accent_text` ink. No hover fill; pressed darkens.
    fn tab(&self, page: Page, window: &Window, cx: &mut Context<Self>) -> Button {
        crate::ui::native::toolbar_tab(
            gpui::SharedString::from(format!("settings-page-{}", page.slug())),
            page.symbol(),
            self.title_of(page, cx),
            page == self.page,
            emphasized(window),
            cx,
        )
        .on_click(cx.listener(move |this, _, window, cx| this.show(page, window, cx)))
    }
}

impl Render for SettingsView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let body = self.window_body(window, cx).into_any_element();
        crate::ui::native::window_chrome::framed_fixed(
            self.title_of(self.page, cx),
            body,
            window,
            cx,
        )
    }
}

impl SettingsView {
    /// The window's content, under the caption strip off the Mac.
    fn window_body(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let k = native_colors(cx);
        let active = emphasized(window);
        // The self-test walks the panes without the window: keep the title.
        let title = self.title_of(self.page, cx);
        if self.titled != Some(title) {
            window.set_window_title(title);
            self.titled = Some(title);
        }
        let tabs: Vec<Button> = Page::TABS
            .into_iter()
            .map(|p| self.tab(p, window, cx))
            .collect();
        let fit = self.fit.clone();
        let handle = window.window_handle();
        // Nothing drawn has the focus (a closed sheet held it): the root
        // takes it back, so Tab still walks the pane.
        if window.focused(cx).is_none() {
            window.focus(&self.focus, cx);
        }
        div()
            .id("settings-root")
            .track_focus(&self.focus)
            // Tab and ⇧Tab walk the toolbar tabs and the pane's rows and
            // controls (E3), as in the main window.
            .on_action(|_: &crate::app::FocusNext, window, cx| window.focus_next(cx))
            .on_action(|_: &crate::app::FocusPrev, window, cx| window.focus_prev(cx))
            .size_full()
            .flex()
            .flex_col()
            .bg(k.window)
            .text_color(k.label)
            .n_body()
            .child(
                // The toolbar: the title band (traffic lights and title) and
                // the tabs; dragging it moves the window, as a title bar does.
                div()
                    .id("settings-toolbar")
                    .flex_none()
                    .w_full()
                    .flex()
                    .flex_col()
                    .on_mouse_down(MouseButton::Left, |event, window, _| {
                        if event.click_count >= 2 {
                            window.titlebar_double_click();
                        } else {
                            window.start_window_move();
                        }
                    })
                    .child(
                        div()
                            .h(TITLE_BAND)
                            .w_full()
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(
                                div()
                                    .n_headline()
                                    .text_color(if active { k.label } else { k.secondary })
                                    .child(title),
                            ),
                    )
                    .child(
                        div()
                            .h(TAB_HEIGHT)
                            .w_full()
                            .flex()
                            .justify_center()
                            .gap(TAB_GAP)
                            .children(tabs),
                    )
                    .child(div().h(TABS_BOTTOM)),
            )
            .child(
                div()
                    .flex_none()
                    .h(crate::ui::native::tokens::layout::BORDER)
                    .w_full()
                    .bg(k.separator),
            )
            .child(
                div()
                    .id("settings-page")
                    .flex_1()
                    .min_h(px(0.))
                    .overflow_y_scroll()
                    .track_scroll(&self.scroll)
                    .child(
                        div()
                            .w_full()
                            .relative()
                            // A pane fades in when the tab changes, instead
                            // of swapping in one frame. It does not rise: the
                            // window's height already moves, and a second
                            // motion under it read as a jitter.
                            .child(crate::ui::motion::fade_in(
                                gpui::ElementId::Name(
                                    format!("settings-pane-{}", self.page.slug()).into(),
                                ),
                                crate::theme::motion::NORMAL,
                                div().relative().w_full().child(self.render_page()),
                                cx,
                            ))
                            // Measures the pane as laid out; the window then
                            // takes its height (§8: height follows the pane).
                            .child(
                                canvas(
                                    move |bounds: Bounds<gpui::Pixels>, window, cx| {
                                        fit.measured(
                                            TOOLBAR_HEIGHT + bounds.size.height,
                                            handle,
                                            window,
                                            cx,
                                        );
                                    },
                                    |_, _, _, _| {},
                                )
                                .absolute()
                                .top_0()
                                .left_0()
                                .size_full(),
                            ),
                    ),
            )
    }
}

/// The pane the window opens on: the one shown last, else 通用.
fn last_page() -> Page {
    prefs::get()
        .last_pane
        .as_deref()
        .and_then(Page::from_slug)
        .filter(|p| !p.in_admin())
        .unwrap_or(Page::General)
}

/// The open settings window, so ⌘, does not open a second one.
#[derive(Default)]
struct SettingsWindow(Option<AnyWindowHandle>);

impl Global for SettingsWindow {}

/// Opens the settings window, or brings the open one to the front.
pub fn open(services: Services, cx: &mut App) {
    if let Some(handle) = cx.try_global::<SettingsWindow>().and_then(|w| w.0)
        && handle
            .update(cx, |_, window, _| window.activate_window())
            .is_ok()
    {
        return;
    }
    if let Ok((handle, _)) = open_window(services, last_page(), true, cx) {
        cx.set_global(SettingsWindow(Some(handle)));
    }
}

/// The sidebar's settings entry: opens the settings window, or closes it
/// when it is open and in plain sight (a second click on the same button
/// closes it). One that another window covers comes to the front instead:
/// the click that reaches the entry raises the main window over it, and
/// closing a window nobody can see read as the entry doing nothing.
pub fn toggle(services: Services, cx: &mut App) {
    // After the action: it runs inside the main window's update, whose
    // bounds `covered` cannot read until then.
    cx.defer(move |cx| toggle_now(services, cx));
}

fn toggle_now(services: Services, cx: &mut App) {
    if let Some(handle) = cx.try_global::<SettingsWindow>().and_then(|w| w.0)
        && !covered(handle, cx)
        && handle
            .update(cx, |_, window, _| window.remove_window())
            .is_ok()
    {
        cx.set_global(SettingsWindow(None));
        return;
    }
    open(services, cx);
}

/// Whether a window above `handle` overlaps it (front to back order from the
/// platform; `false` where the platform cannot tell).
fn covered(handle: AnyWindowHandle, cx: &mut App) -> bool {
    let Some(stack) = cx.window_stack() else {
        return false;
    };
    let Ok(mine) = handle.update(cx, |_, window, _| window.bounds()) else {
        return false;
    };
    stack
        .into_iter()
        .take_while(|w| *w != handle)
        .filter_map(|w| w.update(cx, |_, window, _| window.bounds()).ok())
        .any(|b| b.intersects(&mine))
}

/// v12/fx-overlays: opens the settings window at `page` (the app menu's
/// 检查更新… opens 通用), or brings the open one to the front.
pub fn open_page(services: Services, page: Page, cx: &mut App) {
    // 成员, 团队约定 and 机器 moved to 团队管理.
    // Members other than owners have the read-only 成员 page in the main window.
    let page = if page.in_admin() && !i_am_owner(&services) {
        Page::General
    } else {
        page
    };
    if page.in_admin() {
        let target = match page {
            Page::Memory => crate::views::admin::AdminPage::Memory,
            Page::Machines => crate::views::admin::AdminPage::Machines,
            _ => crate::views::admin::AdminPage::Members,
        };
        return crate::views::admin::open_at(services, target, cx);
    }
    if let Some(handle) = cx.try_global::<SettingsWindow>().and_then(|w| w.0)
        && handle
            .update(cx, |_, window, _| window.activate_window())
            .is_ok()
    {
        return;
    }
    if let Ok((handle, _)) = open_window(services, page, true, cx) {
        cx.set_global(SettingsWindow(Some(handle)));
    }
}

/// The size the window opens at; it then takes the pane's height.
pub const WINDOW_SIZE: gpui::Size<gpui::Pixels> = gpui::Size {
    width: WINDOW_WIDTH,
    height: px(480.),
};

/// Opens a settings window at `page`. Self-tests pass `focus: false`.
pub fn open_window(
    services: Services,
    page: Page,
    focus: bool,
    cx: &mut App,
) -> Result<(AnyWindowHandle, Entity<SettingsView>), String> {
    let options = WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
            None,
            WINDOW_SIZE,
            cx,
        ))),
        window_min_size: Some(size(WINDOW_WIDTH, MIN_HEIGHT)),
        titlebar: Some(TitlebarOptions {
            title: Some(page.title().into()),
            appears_transparent: true,
            traffic_light_position: Some(gpui::point(TRAFFIC_LIGHTS.0, TRAFFIC_LIGHTS.1)),
        }),
        window_background: gpui::WindowBackgroundAppearance::Opaque,
        // §8: minimize and zoom dimmed; the pane sets the height.
        is_resizable: false,
        is_minimizable: false,
        focus,
        show: true,
        ..Default::default()
    };
    gpui_kit::open_window(options, cx, move |window, cx| {
        cx.new(|cx| SettingsView::new_at(services, page, window, cx))
    })
    .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use gpui::{TestAppContext, VisualTestContext};

    use super::*;

    fn member(kind: crate::data::MemberKind, deactivated: bool) -> crate::data::Member {
        crate::data::Member {
            id: "m-1".into(),
            name: "Scout".into(),
            kind,
            level: crate::data::MemberLevel::Owner,
            role: None,
            avatar_url: None,
            deactivated,
            online: false,
            character: None,
            team_owner: false,
            owner_id: None,
            deleted: false,
        }
    }

    /// No "⋯" for a deactivated AI 管理员 that a 管理员 (not the 所有者) sees: nothing to pick.
    #[test]
    fn member_menu_only_when_it_has_items() {
        use crate::data::MemberKind::{Bot, Human};
        let off_bot = member(Bot, true);
        let may_stop = may_deactivate(TeamRole::Admin, false);
        let offer = roles_to_offer(&off_bot, TeamRole::Admin, Some("me"), false);
        assert!(!has_member_menu(&off_bot, &offer, may_stop));
        assert!(
            has_member_menu(&off_bot, &[], true),
            "the 所有者 can bring it back"
        );
        assert!(
            has_member_menu(&member(Bot, false), &[], false),
            "an active AI member"
        );
        assert!(!has_member_menu(&member(Human, false), &[], false));
        assert!(has_member_menu(
            &member(Human, false),
            &[TeamRole::Member],
            false
        ));
    }

    /// Tab walks the Settings window (E3): the root holds the focus in a
    /// fresh window, Tab moves it to a control, and ⇧Tab moves it on.
    #[gpui::test]
    fn tab_walks_the_settings_window(cx: &mut TestAppContext) {
        cx.update(|cx| {
            gpui_component::init(cx);
            crate::app::bind_view_keys(cx);
        });
        let provider: Arc<dyn crate::data::ChatProvider> = crate::fake::FakeProvider::new();
        let keep = Rc::new(std::cell::RefCell::new(None));
        let slot = keep.clone();
        let window = cx
            .update(|cx| {
                let services = Services::new(provider, cx);
                cx.open_window(Default::default(), move |window, cx| {
                    let view =
                        cx.new(|cx| SettingsView::new_at(services, Page::General, window, cx));
                    *slot.borrow_mut() = Some(view.clone());
                    cx.new(|cx| gpui_base::Root::new(view, window, cx))
                })
            })
            .expect("the window opens");
        let cx = &mut VisualTestContext::from_window(window.into(), cx);
        cx.update(|window, _| window.activate_window());
        cx.run_until_parked();
        let view = keep.borrow().clone().expect("the settings view");
        let root = view.read_with(cx, |v, _| v.focus.clone());
        cx.update(|window, _| assert!(root.is_focused(window), "the root holds the focus"));
        cx.simulate_keystrokes("tab");
        let first = cx.update(|window, cx| window.focused(cx));
        assert!(first.is_some(), "Tab focused something");
        cx.update(|window, _| assert!(!root.is_focused(window), "Tab left the root"));
        cx.simulate_keystrokes("tab");
        let second = cx.update(|window, cx| window.focused(cx));
        assert!(second.is_some() && second != first, "a second Tab moves on");
        cx.simulate_keystrokes("shift-tab");
        let back = cx.update(|window, cx| window.focused(cx));
        assert_eq!(back, first, "⇧Tab goes back");
    }
}
