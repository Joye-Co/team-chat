//! The settings window (Cmd+,): 个人, 成员, 机器, 通知, 成本 and 关于 (with
//! diagnostics and the developer-view switch). One window at a time: opening
//! it again brings the open one to the front.
//!
//! Pages read snapshots from the provider and start requests that answer
//! through a [`crate::data::AdminReply`], awaited off the UI thread.

mod about;
mod machines;
mod members;
mod notifications;
mod profile;
pub mod selftest;
mod widgets;

use gpui::{
    AnyWindowHandle, App, AppContext as _, Bounds, Context, Entity, Global, InteractiveElement,
    IntoElement, ParentElement, Render, StatefulInteractiveElement, Styled, Subscription,
    TitlebarOptions, Window, WindowBounds, WindowOptions, div, px, size,
};
use gpui_component::sidebar::{Sidebar, SidebarMenu, SidebarMenuItem};

use crate::{
    app::Services,
    copy::settings as s,
    data::{MemberLevel, SessionState},
    theme,
};

pub use about::AboutPage;
pub use machines::MachinesPage;
pub use members::MembersPage;
pub use notifications::NotificationsPage;
pub use profile::ProfilePage;

/// The pages, in sidebar order.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Page {
    Profile,
    Members,
    Machines,
    Notifications,
    Cost,
    About,
}

impl Page {
    pub const ALL: [Page; 6] = [
        Page::Profile,
        Page::Members,
        Page::Machines,
        Page::Notifications,
        Page::Cost,
        Page::About,
    ];

    pub fn title(self) -> &'static str {
        match self {
            Page::Profile => s::PAGE_PROFILE,
            Page::Members => s::PAGE_MEMBERS,
            Page::Machines => s::PAGE_MACHINES,
            Page::Notifications => s::PAGE_NOTIFICATIONS,
            Page::Cost => s::PAGE_COST,
            Page::About => s::PAGE_ABOUT,
        }
    }

    /// Short name for self-tests and screenshots.
    pub fn slug(self) -> &'static str {
        match self {
            Page::Profile => "profile",
            Page::Members => "members",
            Page::Machines => "machines",
            Page::Notifications => "notifications",
            Page::Cost => "cost",
            Page::About => "about",
        }
    }

    pub fn from_slug(slug: &str) -> Option<Page> {
        Page::ALL.into_iter().find(|p| p.slug() == slug)
    }
}

/// True when I am signed in as an owner (the admin actions show).
pub fn i_am_owner(services: &Services) -> bool {
    matches!(
        services.provider.session(),
        SessionState::SignedIn { me, .. } if me.level == MemberLevel::Owner
    )
}

pub struct SettingsView {
    page: Page,
    profile: Entity<ProfilePage>,
    members: Entity<MembersPage>,
    machines: Entity<MachinesPage>,
    notifications: Entity<NotificationsPage>,
    about: Entity<AboutPage>,
    _subscriptions: Vec<Subscription>,
}

impl SettingsView {
    pub fn new(services: Services, window: &mut Window, cx: &mut Context<Self>) -> Self {
        Self::new_at(services, Page::Profile, window, cx)
    }

    pub fn new_at(
        services: Services,
        page: Page,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let profile = cx.new(|cx| ProfilePage::new(services.clone(), window, cx));
        let members = cx.new(|cx| MembersPage::new(services.clone(), window, cx));
        let machines = cx.new(|cx| MachinesPage::new(services.clone(), window, cx));
        let notifications = cx.new(|cx| NotificationsPage::new(services.clone(), window, cx));
        let about = cx.new(|cx| AboutPage::new(services.clone(), window, cx));
        let mut this = Self {
            page: Page::Profile,
            profile,
            members,
            machines,
            notifications,
            about,
            _subscriptions: vec![theme::follow_system(window, cx)],
        };
        this.select(page, cx);
        this
    }

    pub fn page(&self) -> Page {
        self.page
    }

    pub fn select(&mut self, page: Page, cx: &mut Context<Self>) {
        self.page = page;
        if page == Page::Machines {
            self.machines.update(cx, |m, cx| m.refresh(cx));
        }
        cx.notify();
    }

    pub fn members(&self) -> &Entity<MembersPage> {
        &self.members
    }

    pub fn machines(&self) -> &Entity<MachinesPage> {
        &self.machines
    }

    pub fn profile(&self) -> &Entity<ProfilePage> {
        &self.profile
    }

    pub fn notifications(&self) -> &Entity<NotificationsPage> {
        &self.notifications
    }

    pub fn about(&self) -> &Entity<AboutPage> {
        &self.about
    }

    fn render_page(&self, cx: &App) -> gpui::AnyElement {
        match self.page {
            Page::Profile => self.profile.clone().into_any_element(),
            Page::Members => self.members.clone().into_any_element(),
            Page::Machines => self.machines.clone().into_any_element(),
            Page::Notifications => self.notifications.clone().into_any_element(),
            Page::About => self.about.clone().into_any_element(),
            Page::Cost => {
                let colors = theme::colors(cx);
                div()
                    .flex()
                    .flex_col()
                    .child(widgets::page_title(s::COST, cx))
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .items_center()
                            .gap(px(6.))
                            .py(px(48.))
                            .child(div().text_lg().child(s::COMING_SOON))
                            .child(
                                div()
                                    .text_sm()
                                    .text_color(colors.muted_foreground)
                                    .child(s::COST_HINT),
                            ),
                    )
                    .into_any_element()
            }
        }
    }
}

impl Render for SettingsView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let colors = theme::colors(cx);
        let current = self.page;
        let entity = cx.entity().downgrade();
        let menu = SidebarMenu::new().children(Page::ALL.into_iter().map(|page| {
            let entity = entity.clone();
            SidebarMenuItem::new(page.title())
                .active(page == current)
                .on_click(move |_, _, cx| {
                    let _ = entity.update(cx, |this, cx| this.select(page, cx));
                })
        }));
        div()
            .size_full()
            .flex()
            .bg(colors.background)
            .text_color(colors.foreground)
            .text_sm()
            .child(
                div()
                    .w(px(168.))
                    .flex_none()
                    .h_full()
                    .border_r_1()
                    .border_color(colors.border)
                    .child(
                        Sidebar::new("settings-pages")
                            .collapsible(false)
                            .w_full()
                            .child(menu),
                    ),
            )
            .child(
                div()
                    .id("settings-page")
                    .flex_1()
                    .min_w_0()
                    .h_full()
                    .overflow_y_scroll()
                    .px(px(24.))
                    .py(px(20.))
                    .child(self.render_page(cx)),
            )
    }
}

/// The open settings window, so Cmd+, does not open a second one.
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
    if let Ok((handle, _)) = open_window(services, Page::Profile, true, cx) {
        cx.set_global(SettingsWindow(Some(handle)));
    }
}

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
            size(px(760.), px(600.)),
            cx,
        ))),
        window_min_size: Some(size(px(620.), px(440.))),
        titlebar: Some(TitlebarOptions {
            title: Some(s::TITLE.into()),
            ..Default::default()
        }),
        focus,
        show: true,
        ..Default::default()
    };
    gpui_kit::open_window(options, cx, move |window, cx| {
        cx.new(|cx| SettingsView::new_at(services, page, window, cx))
    })
    .map_err(|e| e.to_string())
}
