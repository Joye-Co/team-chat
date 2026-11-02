//! The macOS app menus: the app menu (about, settings, hide, quit), 编辑 (so
//! text inputs get undo, cut, copy, paste and select all), 前往 (navigation
//! shortcuts) and 窗口.

use gpui::{App, KeyBinding, Menu, MenuItem, OsAction};
use gpui_component::input::{Copy, Cut, Paste, Redo, SelectAll, Undo};

use super::{
    About, NewGroup, NextUnreadTopic, PreviousUnreadTopic, QuickSwitch, ShowMainWindow, StartDm,
};
use crate::{
    app::{NewTopic, OpenSettings, Quit},
    copy::{fill, nav, settings},
};

gpui::actions!(nav_menu, [Hide, HideOthers, ShowAll, Minimize]);

fn product(template: &str) -> String {
    fill(template, &[("product", env!("PRODUCT_NAME"))])
}

/// The menus of the menu bar.
pub fn app_menus() -> Vec<Menu> {
    vec![
        Menu::new(env!("PRODUCT_NAME")).items([
            MenuItem::action(product(nav::MENU_ABOUT), About),
            MenuItem::separator(),
            MenuItem::action(settings::MENU_SETTINGS, OpenSettings),
            MenuItem::separator(),
            MenuItem::action(product(nav::MENU_HIDE), Hide),
            MenuItem::action(nav::MENU_HIDE_OTHERS, HideOthers),
            MenuItem::action(nav::MENU_SHOW_ALL, ShowAll),
            MenuItem::separator(),
            MenuItem::action(settings::MENU_QUIT, Quit),
        ]),
        Menu::new(nav::MENU_EDIT).items([
            MenuItem::os_action(nav::MENU_UNDO, Undo, OsAction::Undo),
            MenuItem::os_action(nav::MENU_REDO, Redo, OsAction::Redo),
            MenuItem::separator(),
            MenuItem::os_action(nav::MENU_CUT, Cut, OsAction::Cut),
            MenuItem::os_action(nav::MENU_COPY, Copy, OsAction::Copy),
            MenuItem::os_action(nav::MENU_PASTE, Paste, OsAction::Paste),
            MenuItem::os_action(nav::MENU_SELECT_ALL, SelectAll, OsAction::SelectAll),
        ]),
        Menu::new(nav::MENU_GO).items([
            MenuItem::action(nav::MENU_QUICK_SWITCH, QuickSwitch),
            MenuItem::separator(),
            MenuItem::action(nav::MENU_NEW_TOPIC, NewTopic),
            MenuItem::action(nav::MENU_NEW_GROUP, NewGroup),
            MenuItem::action(nav::MENU_START_DM, StartDm),
            MenuItem::separator(),
            MenuItem::action(nav::MENU_NEXT_UNREAD, NextUnreadTopic),
            MenuItem::action(nav::MENU_PREVIOUS_UNREAD, PreviousUnreadTopic),
        ]),
        Menu::new(nav::MENU_WINDOW).items([
            MenuItem::action(nav::MENU_MINIMIZE, Minimize),
            MenuItem::action(nav::MENU_SHOW_MAIN, ShowMainWindow),
        ]),
    ]
}

/// Sets the menus and the bindings and handlers of the menu-only actions.
pub fn install(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("cmd-h", Hide, None),
        KeyBinding::new("cmd-alt-h", HideOthers, None),
        KeyBinding::new("cmd-m", Minimize, None),
    ]);
    cx.on_action(|_: &Hide, cx| cx.hide());
    cx.on_action(|_: &HideOthers, cx| cx.hide_other_apps());
    cx.on_action(|_: &ShowAll, cx| cx.unhide_other_apps());
    cx.on_action(|_: &Minimize, cx| {
        if let Some(window) = cx.active_window() {
            let _ = window.update(cx, |_, window, _| window.minimize_window());
        }
    });
    cx.set_menus(app_menus());
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
