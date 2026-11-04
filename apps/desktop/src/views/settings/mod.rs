//! The settings window (Cmd+,): 我的资料, 成员, 机器, 通知 and 通用 (the
//! about card with the version and update, diagnostics and the
//! developer-view switch); 成本 has no page until it exists (U43). One
//! window at a time: opening it again brings the open one to the front.
//!
//! Pages read snapshots from the provider and start requests that answer
//! through a [`crate::data::AdminReply`], awaited off the UI thread.

mod about;
pub mod appearance;
mod machines;
mod members;
// pages track: the 成员 page reuses the invite and add-member forms.
pub(crate) use members::{open_add_bot, open_invite};
mod notifications;
mod profile;
pub mod selftest;
pub(crate) mod widgets;

use gpui::{
    AnyWindowHandle, App, AppContext as _, Bounds, Context, Entity, Global, InteractiveElement,
    IntoElement, ParentElement, Render, StatefulInteractiveElement, Styled, Subscription,
    TitlebarOptions, Window, WindowBounds, WindowOptions, div, prelude::FluentBuilder as _, px,
    size,
};

use crate::{
    app::Services,
    copy::settings as s,
    data::{MemberLevel, SessionState},
    theme::{self, Type as _, space},
    ui,
};

pub use about::AboutPage;
pub use appearance::AppearancePage;
pub use machines::MachinesPage;
pub use members::MembersPage;
pub use notifications::NotificationsPage;
pub use profile::ProfilePage;

/// The pages, in sidebar order. `About` is the 通用 page (its slug stays
/// `about` for the self-tests and screenshot scenes).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Page {
    Profile,
    Members,
    Machines,
    Notifications,
    /// 外观 (1.1.0): theme and rail style.
    Appearance,
    About,
}

impl Page {
    /// 成本 has no page until it exists (U43).
    pub const ALL: [Page; 6] = [
        Page::Profile,
        Page::Members,
        Page::Machines,
        Page::Notifications,
        Page::Appearance,
        Page::About,
    ];

    pub fn title(self) -> &'static str {
        match self {
            Page::Profile => s::PAGE_PROFILE,
            Page::Members => s::PAGE_MEMBERS,
            Page::Machines => s::PAGE_MACHINES,
            Page::Notifications => s::PAGE_NOTIFICATIONS,
            Page::Appearance => crate::copy::appearance::APPEARANCE_PAGE,
            Page::About => s::PAGE_GENERAL,
        }
    }

    /// Short name for self-tests and screenshots.
    pub fn slug(self) -> &'static str {
        match self {
            Page::Profile => "profile",
            Page::Members => "members",
            Page::Machines => "machines",
            Page::Notifications => "notifications",
            Page::Appearance => "appearance",
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
    appearance: Entity<AppearancePage>,
    about: Entity<AboutPage>,
    /// The page scroll, so a scene can show the end of a long page.
    scroll: gpui::ScrollHandle,
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
        let appearance = cx.new(|cx| AppearancePage::new(services.clone(), window, cx));
        let about = cx.new(|cx| AboutPage::new(services.clone(), window, cx));
        let mut this = Self {
            page: Page::Profile,
            profile,
            members,
            machines,
            notifications,
            appearance,
            about,
            scroll: gpui::ScrollHandle::new(),
            _subscriptions: vec![theme::follow_system(window, cx)],
        };
        this.select(page, cx);
        this
    }

    pub fn page(&self) -> Page {
        self.page
    }

    pub fn select(&mut self, page: Page, cx: &mut Context<Self>) {
        if page != self.page {
            self.scroll.set_offset(gpui::point(px(0.), px(0.)));
        }
        self.page = page;
        if page == Page::Machines {
            self.machines.update(cx, |m, cx| m.refresh(cx));
        }
        cx.notify();
    }

    /// Scrolls the page to its end (the footnote under a long table).
    pub fn scroll_to_end(&self) {
        self.scroll.scroll_to_bottom();
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

    fn render_page(&self) -> gpui::AnyElement {
        match self.page {
            Page::Profile => self.profile.clone().into_any_element(),
            Page::Members => self.members.clone().into_any_element(),
            Page::Machines => self.machines.clone().into_any_element(),
            Page::Notifications => self.notifications.clone().into_any_element(),
            Page::Appearance => self.appearance.clone().into_any_element(),
            Page::About => self.about.clone().into_any_element(),
        }
    }

    /// The page primary action at the right of the page title.
    fn page_actions(&self, cx: &App) -> Option<gpui::AnyElement> {
        match self.page {
            Page::Members => Some(self.members.read(cx).header_actions(cx)),
            Page::Machines => Some(MachinesPage::header_actions(&self.machines, cx)),
            _ => None,
        }
    }
}

impl Page {
    fn icon(self) -> ui::icons::Sym {
        use ui::icons::Sym;
        match self {
            Page::Profile => Sym::Account,
            Page::Members => Sym::Members,
            Page::Machines => Sym::MacMini,
            Page::Notifications => Sym::Bell,
            Page::Appearance => Sym::Appearance,
            Page::About => Sym::Info,
        }
    }
}

/// Width of the section list (§8.4).
const SECTION_LIST: gpui::Pixels = px(200.);
/// Section list rows (§8.4).
const SECTION_ROW: gpui::Pixels = px(36.);

impl Render for SettingsView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let colors = theme::colors(cx);
        let current = self.page;
        let rows = Page::ALL.into_iter().map(|page| {
            let on = page == current;
            ui::list_row(
                gpui::SharedString::from(format!("settings-page-{}", page.slug())),
                ui::RowSurface::Sidebar,
                on,
                cx,
            )
            .h(SECTION_ROW)
            .px(space::S10)
            .gap(space::S10)
            .child(ui::sym(
                page.icon(),
                ui::SymSize::SIDEBAR,
                if on { colors.text_1 } else { colors.text_2 },
            ))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .t_ui()
                    .one_line()
                    .text_color(if on { colors.text_1 } else { colors.text_2 })
                    .when(on, |el| el.semibold())
                    .child(page.title()),
            )
            .on_click(cx.listener(move |this, _, _, cx| this.select(page, cx)))
        });
        div()
            .size_full()
            .flex()
            .bg(colors.content)
            .text_color(colors.text_1)
            .t_ui()
            .child(
                div()
                    .w(SECTION_LIST)
                    .flex_none()
                    .h_full()
                    .flex()
                    .flex_col()
                    .bg(colors.sidebar)
                    // The traffic lights sit in this band.
                    .child(ui::toolbar_band("settings-pane-toolbar"))
                    .child(
                        div()
                            .px(space::S8)
                            .pt(space::S4)
                            .flex()
                            .flex_col()
                            .gap(space::S2)
                            .children(rows),
                    ),
            )
            .child(ui::vhairline(cx))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .h_full()
                    .relative()
                    .child(
                        div()
                            .id("settings-page")
                            .size_full()
                            .overflow_y_scroll()
                            .track_scroll(&self.scroll)
                            .child(widgets::page(
                                self.page.title(),
                                self.page_actions(cx),
                                self.render_page(),
                                cx,
                            )),
                    )
                    // The top 36 pt drags the window, as a title bar does.
                    .child(
                        ui::toolbar_band("settings-page-drag")
                            .absolute()
                            .top_0()
                            .left_0()
                            .h(px(28.)),
                    ),
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

/// The settings window's size (§4.4).
pub const WINDOW_SIZE: gpui::Size<gpui::Pixels> = gpui::Size {
    width: px(960.),
    height: px(720.),
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
        window_min_size: Some(size(px(760.), px(560.))),
        titlebar: Some(TitlebarOptions {
            title: Some(s::TITLE.into()),
            appears_transparent: true,
            traffic_light_position: Some(gpui::point(
                theme::TRAFFIC_LIGHTS.0,
                theme::TRAFFIC_LIGHTS.1,
            )),
        }),
        window_background: gpui::WindowBackgroundAppearance::Opaque,
        focus,
        show: true,
        ..Default::default()
    };
    gpui_kit::open_window(options, cx, move |window, cx| {
        cx.new(|cx| SettingsView::new_at(services, page, window, cx))
    })
    .map_err(|e| e.to_string())
}
