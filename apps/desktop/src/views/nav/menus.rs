//! The macOS app menus: the app menu (about, settings, 服务, hide, quit),
//! 文件 (new things, close), 编辑 (so text inputs get undo, cut, copy, paste,
//! select all and 表情与符号), 前往 (navigation shortcuts), 显示, 窗口 (with
//! the system's window list) and 帮助 (with the system's menu search), plus
//! the Dock icon's menu.

use gpui::{App, KeyBinding, Menu, MenuItem, OsAction, SystemMenuType};
use gpui_component::input::{Copy, Cut, Paste, Redo, SelectAll, Undo};

use super::{
    About, NewGroup, NextUnreadTopic, PreviousUnreadTopic, QuickSwitch, ShowMainWindow, StartDm,
};
use crate::{
    app::{
        NewTopic, OpenArtifacts, OpenDesk, OpenMembers, OpenSettings, Quit, ToggleSideChat,
        ToggleSidePanel, ToggleSidebar,
    },
    copy::{connection, fill, nav, rail, settings, topic_rail},
};

gpui::actions!(
    nav_menu,
    [
        Hide,
        HideOthers,
        ShowAll,
        Minimize,
        CloseWindow,
        Zoom,
        ToggleFullScreen,
        BringAllToFront,
        ShowCharacterPalette
    ]
);

fn product(template: &str) -> String {
    fill(template, &[("product", env!("PRODUCT_NAME"))])
}

/// The menus of the menu bar.
pub fn app_menus() -> Vec<Menu> {
    app_menus_with(MenuState::default())
}

/// The menus of the menu bar, with the check marks of `state`.
pub fn app_menus_with(state: MenuState) -> Vec<Menu> {
    vec![
        Menu::new(env!("PRODUCT_NAME")).items([
            MenuItem::action(product(nav::MENU_ABOUT), About),
            // v12/fx-overlays: 检查更新… and the download's progress.
            update_item(),
            MenuItem::separator(),
            MenuItem::action(settings::MENU_SETTINGS, OpenSettings),
            server_menu(),
            MenuItem::separator(),
            MenuItem::os_submenu(nav::MENU_SERVICES, SystemMenuType::Services),
            MenuItem::separator(),
            MenuItem::action(product(nav::MENU_HIDE), Hide),
            MenuItem::action(nav::MENU_HIDE_OTHERS, HideOthers),
            MenuItem::action(nav::MENU_SHOW_ALL, ShowAll),
            MenuItem::separator(),
            MenuItem::action(settings::MENU_QUIT, Quit),
        ]),
        Menu::new(nav::MENU_FILE).items([
            MenuItem::action(nav::MENU_NEW_TOPIC, NewTopic),
            MenuItem::action(nav::MENU_NEW_GROUP, NewGroup),
            MenuItem::action(nav::MENU_START_DM, StartDm),
            MenuItem::separator(),
            MenuItem::action(nav::MENU_CLOSE_WINDOW, CloseWindow),
        ]),
        Menu::new(nav::MENU_EDIT).items([
            MenuItem::os_action(nav::MENU_UNDO, Undo, OsAction::Undo),
            MenuItem::os_action(nav::MENU_REDO, Redo, OsAction::Redo),
            MenuItem::separator(),
            MenuItem::os_action(nav::MENU_CUT, Cut, OsAction::Cut),
            MenuItem::os_action(nav::MENU_COPY, Copy, OsAction::Copy),
            MenuItem::os_action(nav::MENU_PASTE, Paste, OsAction::Paste),
            MenuItem::os_action(nav::MENU_SELECT_ALL, SelectAll, OsAction::SelectAll),
            MenuItem::separator(),
            MenuItem::action(
                crate::copy::timeline::MENU_FIND,
                crate::views::timeline::find::FindInTopic,
            ),
            MenuItem::action(
                crate::copy::timeline::MENU_FIND_NEXT,
                crate::views::timeline::find::FindNext,
            ),
            MenuItem::action(
                crate::copy::timeline::MENU_FIND_PREVIOUS,
                crate::views::timeline::find::FindPrevious,
            ),
            MenuItem::separator(),
            MenuItem::action(nav::MENU_EMOJI, ShowCharacterPalette),
        ]),
        Menu::new(nav::MENU_GO).items([
            MenuItem::action(nav::MENU_QUICK_SWITCH, QuickSwitch),
            MenuItem::separator(),
            MenuItem::action(rail::RAIL_DESK, OpenDesk),
            MenuItem::action(rail::RAIL_ARTIFACTS, OpenArtifacts),
            MenuItem::action(rail::RAIL_MEMBERS, OpenMembers),
            MenuItem::separator(),
            MenuItem::action(nav::MENU_NEXT_UNREAD, NextUnreadTopic),
            MenuItem::action(nav::MENU_PREVIOUS_UNREAD, PreviousUnreadTopic),
        ]),
        // Every toolbar item and the rail's 我参与的 filter are 显示
        // commands (NATIVE.md §5.1, §9.1).
        Menu::new(nav::MENU_VIEW).items([
            MenuItem::action(nav::MENU_SIDEBAR, ToggleSidebar).checked(state.sidebar_shown),
            MenuItem::action(nav::MENU_SIDE_PANEL, ToggleSidePanel),
            MenuItem::separator(),
            MenuItem::action(rail::RAIL_FILTER_MINE, ToggleMineTopics).checked(state.mine_topics),
            MenuItem::separator(),
            MenuItem::action(topic_rail::TITLE_SIDE_CHAT, ToggleSideChat),
            MenuItem::action(topic_rail::TITLE_OUTPUTS, ShowTopicOutputs),
            MenuItem::action(topic_rail::TITLE_MACHINE, ShowTopicMachine),
            MenuItem::action(topic_rail::TITLE_CHANGES, ShowTopicChanges),
            MenuItem::separator(),
            // v12/fx-shell: the connection state lives in the toolbar
            // subtitle; reconnecting now is a command, not a sidebar row.
            MenuItem::action(connection::RECONNECT, ReconnectNow),
            MenuItem::separator(),
            MenuItem::action(nav::MENU_FULL_SCREEN, ToggleFullScreen),
        ]),
        Menu::new(nav::MENU_WINDOW).items([
            MenuItem::action(nav::MENU_MINIMIZE, Minimize),
            MenuItem::action(nav::MENU_ZOOM, Zoom),
            MenuItem::separator(),
            MenuItem::action(nav::MENU_SHOW_MAIN, ShowMainWindow),
            MenuItem::action(nav::MENU_BRING_ALL_TO_FRONT, BringAllToFront),
        ]),
        // Empty on purpose: macOS puts its menu search here once it is the
        // app's help menu (`register_system_menus`).
        Menu::new(nav::MENU_HELP),
    ]
}

/// The Dock icon's menu (right click on the icon).
fn dock_menu() -> Vec<MenuItem> {
    vec![
        MenuItem::action(nav::MENU_NEW_TOPIC, NewTopic),
        MenuItem::action(nav::MENU_NEW_GROUP, NewGroup),
        MenuItem::action(nav::MENU_START_DM, StartDm),
        MenuItem::separator(),
        MenuItem::action(nav::MENU_SHOW_MAIN, ShowMainWindow),
    ]
}

/// Sets the menu bar and tells AppKit which menus are 窗口 and 帮助: GPUI
/// only knows an English "Window" menu and no help menu, so without this the
/// window list and the menu search are missing.
fn set_menus(menus: Vec<Menu>, cx: &mut App) {
    cx.set_menus(menus);
    register_system_menus();
}

#[cfg(target_os = "macos")]
fn register_system_menus() {
    use objc2_app_kit::NSApplication;
    use objc2_foundation::NSString;
    let Some(mtm) = objc2::MainThreadMarker::new() else {
        return;
    };
    let app = NSApplication::sharedApplication(mtm);
    let Some(main) = app.mainMenu() else {
        return;
    };
    let submenu = |title: &str| {
        main.itemWithTitle(&NSString::from_str(title))
            .and_then(|item| item.submenu())
    };
    if let Some(menu) = submenu(nav::MENU_WINDOW) {
        app.setWindowsMenu(Some(&menu));
    }
    if let Some(menu) = submenu(nav::MENU_HELP) {
        app.setHelpMenu(Some(&menu));
    }
}

#[cfg(not(target_os = "macos"))]
fn register_system_menus() {}

/// 编辑 › 表情与符号: the system's character viewer, which types into the
/// focused field.
fn show_character_palette(cx: &mut App) {
    let _ = cx;
    #[cfg(target_os = "macos")]
    if let Some(mtm) = objc2::MainThreadMarker::new() {
        objc2_app_kit::NSApplication::sharedApplication(mtm).orderFrontCharacterPalette(None);
    }
}

/// Sets the menus and the bindings and handlers of the menu-only actions.
pub fn install(cx: &mut App) {
    // The Mac's own window and app keys. Windows has its own for these
    // (Win+↓, Win+., Alt+F4), so there only closing and full screen are bound.
    #[cfg(target_os = "macos")]
    cx.bind_keys([
        KeyBinding::new("cmd-h", Hide, None),
        KeyBinding::new("cmd-alt-h", HideOthers, None),
        KeyBinding::new("cmd-m", Minimize, None),
        KeyBinding::new("cmd-w", CloseWindow, None),
        KeyBinding::new("ctrl-cmd-f", ToggleFullScreen, None),
        KeyBinding::new("ctrl-cmd-space", ShowCharacterPalette, None),
    ]);
    #[cfg(not(target_os = "macos"))]
    cx.bind_keys([
        KeyBinding::new("ctrl-w", CloseWindow, None),
        KeyBinding::new("f11", ToggleFullScreen, None),
    ]);
    install_server_actions(cx);
    cx.on_action(|_: &Hide, cx| cx.hide());
    cx.on_action(|_: &HideOthers, cx| cx.hide_other_apps());
    cx.on_action(|_: &ShowAll, cx| cx.unhide_other_apps());
    cx.on_action(|_: &Minimize, cx| with_key_window(cx, |window| window.minimize_window()));
    // Like the red traffic light: the app keeps running, and a click on the
    // Dock icon opens the main window again.
    cx.on_action(|_: &CloseWindow, cx| {
        // Off the Mac the app ends with its last window: Ctrl+W never closes
        // that one (the close button and Alt+F4 do).
        if cfg!(not(target_os = "macos")) && cx.windows().len() <= 1 {
            return;
        }
        with_key_window(cx, |window| window.remove_window())
    });
    cx.on_action(|_: &Zoom, cx| with_key_window(cx, |window| window.zoom_window()));
    cx.on_action(|_: &ToggleFullScreen, cx| {
        with_key_window(cx, |window| window.toggle_fullscreen())
    });
    cx.on_action(|_: &BringAllToFront, cx| cx.activate(true));
    cx.on_action(|_: &ShowCharacterPalette, cx| show_character_palette(cx));
    set_menus(app_menus(), cx);
    cx.set_dock_menu(dock_menu());
}

/// Runs `f` on the main GPUI window. When a 产出 window (not a GPUI window)
/// is the main window there is none, so nothing happens.
/// Deferred: a key binding dispatches while that window is being updated.
fn with_key_window(cx: &mut App, f: impl FnOnce(&mut gpui::Window) + 'static) {
    if let Some(window) = cx.active_window() {
        cx.defer(move |cx| {
            let _ = window.update(cx, |_, window, _| f(window));
        });
    }
}

/// The standard macOS about panel (name, version and icon from the bundle).
pub fn show_about(cx: &mut App) {
    cx.activate(true);
    #[cfg(target_os = "macos")]
    if let Some(mtm) = objc2::MainThreadMarker::new() {
        let app = objc2_app_kit::NSApplication::sharedApplication(mtm);
        app.orderFrontStandardAboutPanel(None);
    }
}

// --- v12/behavior ---
gpui::actions!(
    nav_view,
    [
        /// 显示 › 我参与的: the rail lists only my 话题, or all of them.
        ToggleMineTopics,
        /// 显示 › 产出: the toolbar's 产出 pane of the open 话题.
        ShowTopicOutputs,
        /// 显示 › 电脑: the toolbar's 电脑 pane (a member's computer at work).
        ShowTopicMachine,
        /// 显示 › 改动: the toolbar's 改动 pane (the 话题's PR).
        ShowTopicChanges,
        /// 显示 › 立即重连: try the connection again now.
        ReconnectNow
    ]
);

/// The check marks of the 显示 menu.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MenuState {
    pub sidebar_shown: bool,
    pub mine_topics: bool,
}

impl Default for MenuState {
    fn default() -> Self {
        Self {
            sidebar_shown: true,
            mine_topics: true,
        }
    }
}

// --- v12/fx-overlays: the update item of the app menu ---

thread_local! {
    /// The check marks the menus were last set with.
    static LAST_STATE: std::cell::Cell<MenuState> = std::cell::Cell::new(MenuState::default());
}

/// Sets the menus again with the current check marks, so they show the
/// shortcuts as now bound (after a change in Settings › 快捷键).
pub fn refresh(cx: &mut App) {
    set_menus(app_menus_with(LAST_STATE.with(|s| s.get())), cx);
}

/// The app menu's update item: 检查更新…, the download's progress, or
/// 重启安装 (NATIVE.md: the download shows here and in Settings › 通用).
fn update_item() -> MenuItem {
    let (title, enabled) = UPDATE_ITEM.with(|i| i.borrow().clone());
    MenuItem::action(title, crate::updater::UpdateCommand).disabled(!enabled)
}

thread_local! {
    static UPDATE_ITEM: std::cell::RefCell<(String, bool)> = std::cell::RefCell::new((
        crate::copy::overlays::MENU_CHECK.to_string(),
        true,
    ));
}

/// Sets the menus again when the update item changed (the updater calls
/// this on every phase change).
pub fn refresh_menus(cx: &mut App) {
    let item = crate::updater::global(cx)
        .map(|u| u.read(cx).menu_item())
        .unwrap_or_else(|| (crate::copy::overlays::MENU_CHECK.to_string(), true));
    let changed = UPDATE_ITEM.with(|i| {
        let mut i = i.borrow_mut();
        let changed = *i != item;
        *i = item;
        changed
    });
    if changed {
        set_menus(app_menus_with(LAST_STATE.with(|s| s.get())), cx);
    }
}

// --- end v12/fx-overlays ---

gpui::actions!(nav_menu, [UseProductionServer, UseStagingServer]);

thread_local! {
    /// The server in use, for the check marks of the 服务器 submenu.
    static CURRENT_SERVER: std::cell::RefCell<Option<String>> =
        const { std::cell::RefCell::new(None) };
}

fn same_server(a: &str, b: &str) -> bool {
    !a.is_empty() && a.trim_end_matches('/') == b.trim_end_matches('/')
}

/// The app menu's 服务器 submenu: 正式服务器 and 测试服务器 with a check on
/// the one in use (a session per server stays saved, so switching back
/// needs no new login), another server's host when that one is in use, and
/// the login window's 使用其他服务器… (as Mail's 添加账户…; off while no
/// login window has the focus).
fn server_menu() -> MenuItem {
    use crate::copy::login as l;
    let (production, staging) = (env!("PRODUCT_PRODUCTION_URL"), env!("PRODUCT_STAGING_URL"));
    let current = CURRENT_SERVER
        .with(|c| c.borrow().clone())
        .unwrap_or_default();
    let mut items = Vec::new();
    if !production.is_empty() {
        items.push(
            MenuItem::action(l::SERVER_PRODUCTION, UseProductionServer)
                .checked(same_server(production, &current)),
        );
    }
    if !staging.is_empty() {
        items.push(
            MenuItem::action(l::SERVER_STAGING, UseStagingServer)
                .checked(same_server(staging, &current)),
        );
    }
    if !current.is_empty() && !same_server(production, &current) && !same_server(staging, &current)
    {
        let host = crate::views::login::server_host(&current).to_string();
        items.push(
            MenuItem::action(host, crate::views::login::UseOtherServer)
                .checked(true)
                .disabled(true),
        );
    }
    items.push(MenuItem::separator());
    items.push(MenuItem::action(
        l::OTHER_SERVER,
        crate::views::login::UseOtherServer,
    ));
    MenuItem::submenu(Menu::new(l::MENU_SERVER).items(items))
}

/// Switches to `url` and keeps the 服务器 check marks in step.
fn switch_server(url: &str, cx: &mut App) {
    if url.is_empty() {
        return;
    }
    let Some(nav) = cx.try_global::<super::NavGlobal>() else {
        return;
    };
    nav.services.provider.switch_server(url);
    refresh_server_menu(cx);
}

/// Sets the menus again when the server in use changed (the app calls this
/// on every session change).
pub fn refresh_server_menu(cx: &mut App) {
    let Some(nav) = cx.try_global::<super::NavGlobal>() else {
        return;
    };
    let current = nav.services.provider.current_server();
    let changed = CURRENT_SERVER.with(|c| c.replace(current.clone()) != current);
    if changed {
        set_menus(app_menus_with(LAST_STATE.with(|s| s.get())), cx);
    }
}

/// Handles 正式服务器 and 测试服务器 from any window.
fn install_server_actions(cx: &mut App) {
    cx.on_action(|_: &UseProductionServer, cx| switch_server(env!("PRODUCT_PRODUCTION_URL"), cx));
    cx.on_action(|_: &UseStagingServer, cx| switch_server(env!("PRODUCT_STAGING_URL"), cx));
}

/// The menu state of the main window now.
fn menu_state(
    services: &crate::app::Services,
    rail: &gpui::Entity<crate::views::rail::RailView>,
    cx: &App,
) -> MenuState {
    MenuState {
        sidebar_shown: !services.nav.read(cx).sidebar_hidden,
        mine_topics: !rail.read(cx).shows_all_topics(),
    }
}

/// Keeps the check marks of the 显示 menu in step with the rail and the
/// sidebar; the menus are set again only when a mark changes.
pub fn watch_menu_state<V: 'static>(
    services: &crate::app::Services,
    rail: &gpui::Entity<crate::views::rail::RailView>,
    cx: &mut gpui::Context<V>,
) -> Vec<gpui::Subscription> {
    let last = std::rc::Rc::new(std::cell::Cell::new(MenuState::default()));
    let refresh = {
        let (services, rail) = (services.clone(), rail.clone());
        std::rc::Rc::new(move |cx: &mut App| {
            let state = menu_state(&services, &rail, cx);
            if last.replace(state) != state {
                LAST_STATE.with(|s| s.set(state));
                set_menus(app_menus_with(state), cx);
            }
        })
    };
    let (on_rail, on_nav) = (refresh.clone(), refresh);
    vec![
        cx.observe(rail, move |_, _, cx| on_rail(cx)),
        cx.observe(&services.nav, move |_, _, cx| on_nav(cx)),
    ]
}

/// Handles the 显示 commands that act on the main window's rail and panes;
/// the root view of the main window calls this on its element.
pub fn on_view_commands<E: gpui::InteractiveElement>(
    el: E,
    services: &crate::app::Services,
    rail: &gpui::Entity<crate::views::rail::RailView>,
) -> E {
    use crate::views::shell::TitleButtonKind as Kind;
    let toggle = |kind: Kind| {
        let services = services.clone();
        move |services_cx: &mut App| toggle_pane_of(&services, kind, services_cx)
    };
    let (outputs, machine, changes) = (
        toggle(Kind::Outputs),
        toggle(Kind::Machine),
        toggle(Kind::Changes),
    );
    let rail = rail.clone();
    el.on_action(move |_: &ToggleMineTopics, _, cx| {
        rail.update(cx, |r, cx| {
            let all = r.shows_all_topics();
            r.set_show_all_topics(!all, cx)
        })
    })
    .on_action(move |_: &ShowTopicOutputs, _, cx| outputs(cx))
    .on_action(move |_: &ShowTopicMachine, _, cx| machine(cx))
    .on_action(move |_: &ShowTopicChanges, _, cx| changes(cx))
    .on_action({
        let services = services.clone();
        move |_: &ReconnectNow, _, _| services.provider.reconnect_now()
    })
}

/// Toggles the pane of the open 话题's toolbar item `kind`, as a click on
/// the item does; nothing when the 话题 has no such item.
fn toggle_pane_of(
    services: &crate::app::Services,
    kind: crate::views::shell::TitleButtonKind,
    cx: &mut App,
) {
    let Some((g, t)) = services.nav.read(cx).topic.clone() else {
        return;
    };
    let Some(button) = crate::views::topic_page::pane_buttons(services, &g, &t)
        .into_iter()
        .find(|b| b.kind == kind)
    else {
        return;
    };
    services
        .nav
        .update(cx, |nav, cx| nav.toggle_pane(button.pane, cx));
}
// --- end v12/behavior ---

#[cfg(test)]
mod tests {
    use super::*;

    struct Empty;

    impl gpui::Render for Empty {
        fn render(
            &mut self,
            _: &mut gpui::Window,
            _: &mut gpui::Context<Self>,
        ) -> impl gpui::IntoElement {
            gpui::div()
        }
    }

    #[gpui::test]
    fn cmd_w_closes_the_window_and_the_menus_name_the_mac_commands(cx: &mut gpui::TestAppContext) {
        let names: Vec<String> = app_menus().iter().map(|m| m.name.to_string()).collect();
        assert!(names.iter().any(|n| n == nav::MENU_VIEW));
        cx.update(|cx| {
            cx.bind_keys([KeyBinding::new("cmd-w", CloseWindow, None)]);
            cx.on_action(|_: &CloseWindow, cx| {
                with_key_window(cx, |window| window.remove_window())
            });
        });
        let window = cx.add_window(|_, _| Empty);
        cx.run_until_parked();
        window
            .update(cx, |_, window, _| window.activate_window())
            .unwrap();
        cx.run_until_parked();
        let mut vcx = gpui::VisualTestContext::from_window(window.into(), cx);
        vcx.simulate_keystrokes("cmd-w");
        cx.run_until_parked();
        assert!(cx.update(|cx| cx.windows().is_empty()), "cmd-w closed it");
    }

    // --- v12/behavior ---
    /// Every toolbar item of a 话题 and the rail's 我参与的 filter are 显示
    /// commands, and the command flips the rail's filter (NATIVE.md §5.1,
    /// §9.1).
    #[gpui::test]
    fn the_view_menu_runs_the_toolbar_and_filter_commands(cx: &mut gpui::TestAppContext) {
        use std::sync::Arc;

        use gpui::{Action as _, AppContext as _, InteractiveElement as _, ParentElement as _};

        use crate::{
            app::Services, data::ChatProvider, fake::v2::V2Samples, views::rail::RailView,
        };

        let menus = app_menus();
        let view_menu = menus
            .iter()
            .find(|m| m.name.as_ref() == nav::MENU_VIEW)
            .expect("a 显示 menu");
        let actions: Vec<String> = view_menu
            .items
            .iter()
            .filter_map(|i| match i {
                MenuItem::Action { action, .. } => Some(action.name().to_string()),
                _ => None,
            })
            .collect();
        for action in [
            ToggleMineTopics.name(),
            ShowTopicOutputs.name(),
            ShowTopicMachine.name(),
            ShowTopicChanges.name(),
            ToggleSideChat.name(),
            ToggleSidePanel.name(),
            ToggleSidebar.name(),
            ReconnectNow.name(),
        ] {
            assert!(actions.iter().any(|a| a == action), "{action} is in 显示");
        }

        struct Root {
            services: Services,
            rail: gpui::Entity<RailView>,
            focus: gpui::FocusHandle,
        }
        impl gpui::Render for Root {
            fn render(
                &mut self,
                _: &mut gpui::Window,
                _: &mut gpui::Context<Self>,
            ) -> impl gpui::IntoElement {
                on_view_commands(
                    gpui::div().id("root").track_focus(&self.focus),
                    &self.services,
                    &self.rail,
                )
                .child(self.rail.clone())
            }
        }
        cx.update(gpui_component::init);
        let provider: Arc<dyn ChatProvider> = V2Samples::new();
        let (root, cx) = cx.add_window_view(move |window, cx| {
            let services = Services::new(provider, cx);
            let rail = cx.new(|cx| RailView::new(services.clone(), window, cx));
            Root {
                services,
                rail,
                focus: cx.focus_handle(),
            }
        });
        cx.run_until_parked();
        let all = |root: &gpui::Entity<Root>, cx: &mut gpui::VisualTestContext| {
            root.update(cx, |this, cx| this.rail.read(cx).shows_all_topics())
        };
        assert!(!all(&root, cx), "我参与的 at first");
        root.update_in(cx, |this, window, cx| window.focus(&this.focus, cx));
        cx.dispatch_action(ToggleMineTopics);
        cx.run_until_parked();
        assert!(all(&root, cx), "the command shows all 话题");
        cx.dispatch_action(ToggleMineTopics);
        cx.run_until_parked();
        assert!(!all(&root, cx), "and back to 我参与的");
    }
    // --- end v12/behavior ---
}
