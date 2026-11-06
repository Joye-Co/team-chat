//! The settings window (⌘, and App menu › 设置…; NATIVE.md §8): toolbar tabs
//! in the Mail Settings style over one pane of grouped forms. The window is
//! 640 wide, its height follows the pane, its title is the pane name and it
//! reopens on the pane shown last. Changes apply at once; there is no Save.
//! One window at a time: opening it again brings the open one to the front.
//!
//! Panes read snapshots from the provider and start requests that answer
//! through a [`crate::data::AdminReply`], awaited off the UI thread.

mod account;
pub mod ai_service;
pub mod appearance;
mod general;
mod machines;
mod members;
// pages track: the 成员 page reuses the invite and add-member forms.
pub(crate) use members::{open_add_bot, open_invite};
mod notifications;
pub mod prefs;
pub mod selftest;
mod shortcuts;
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
    data::{MemberLevel, SessionState},
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
    Machines,
    Appearance,
    Shortcuts,
}

impl Page {
    pub const ALL: [Page; 7] = [
        Page::General,
        Page::Account,
        Page::Notifications,
        Page::Members,
        Page::Machines,
        Page::Appearance,
        Page::Shortcuts,
    ];

    /// The pane name: the tab label and the window title.
    pub fn title(self) -> &'static str {
        match self {
            Page::General => s::PANE_GENERAL,
            Page::Account => s::PANE_ACCOUNT,
            Page::Notifications => s::PANE_NOTIFICATIONS,
            Page::Members => s::PANE_MEMBERS,
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
            Page::Machines => "machines",
            Page::Appearance => "appearance",
            Page::Shortcuts => "shortcuts",
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
            machines,
            appearance,
            shortcuts,
            scroll: gpui::ScrollHandle::new(),
            fit: Rc::new(window_fit::Fit::default()),
            focus,
            _subscriptions: vec![theme::follow_system(window, cx)],
        };
        window_fit::dim_window_buttons(window);
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
        window.set_window_title(page.title());
        prefs::update(|p| p.last_pane = Some(page.slug().to_string()));
        match page {
            Page::Machines => self.machines.update(cx, |m, cx| m.refresh(cx)),
            Page::Account => self.account.update(cx, |a, cx| {
                a.refresh_devices(cx);
                a.refresh_email(cx);
            }),
            Page::Notifications => self.notifications.update(cx, |n, cx| n.refresh(cx)),
            Page::Members => self.members.update(cx, |m, cx| m.refresh_rules(window, cx)),
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

    pub fn machines(&self) -> &Entity<MachinesPage> {
        &self.machines
    }

    fn render_page(&self) -> gpui::AnyElement {
        match self.page {
            Page::General => self.general.clone().into_any_element(),
            Page::Account => self.account.clone().into_any_element(),
            Page::Notifications => self.notifications.clone().into_any_element(),
            Page::Members => self.members.clone().into_any_element(),
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
            page.title(),
            page == self.page,
            emphasized(window),
            cx,
        )
        .on_click(cx.listener(move |this, _, window, cx| this.show(page, window, cx)))
    }
}

impl Render for SettingsView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let k = native_colors(cx);
        let active = emphasized(window);
        // The self-test walks the panes without the window: keep the title.
        window.set_window_title(self.page.title());
        let tabs: Vec<Button> = Page::ALL
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
                                    .child(self.page.title()),
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
                            // A pane fades in with a 4 pt rise when the tab
                            // changes, instead of swapping in one frame.
                            .child(crate::ui::motion::enter(
                                gpui::ElementId::Name(
                                    format!("settings-pane-{}", self.page.slug()).into(),
                                ),
                                crate::theme::motion::NORMAL,
                                div().relative().w_full().child(self.render_page()),
                                |el, t| el.top(crate::theme::motion::TRAVEL * (1. - t)),
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
/// when it is open (a second click on the same button closes it).
pub fn toggle(services: Services, cx: &mut App) {
    if let Some(handle) = cx.try_global::<SettingsWindow>().and_then(|w| w.0)
        && handle
            .update(cx, |_, window, _| window.remove_window())
            .is_ok()
    {
        cx.set_global(SettingsWindow(None));
        return;
    }
    open(services, cx);
}

/// v12/fx-overlays: opens the settings window at `page` (the app menu's
/// 检查更新… opens 通用), or brings the open one to the front.
pub fn open_page(services: Services, page: Page, cx: &mut App) {
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
