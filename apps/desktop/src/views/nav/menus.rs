//! The macOS app menus: the app menu (about, settings, hide, quit), 编辑 (so
//! text inputs get undo, cut, copy, paste and select all), 前往 (navigation
//! shortcuts), 显示 and 窗口.

use gpui::{App, KeyBinding, Menu, MenuItem, OsAction};
use gpui_component::input::{Copy, Cut, Paste, Redo, SelectAll, Undo};

use super::{
    About, NewGroup, NextUnreadTopic, PreviousUnreadTopic, QuickSwitch, ShowMainWindow, StartDm,
};
use crate::{
    app::{
        NewTopic, OpenArtifacts, OpenDesk, OpenMembers, OpenSettings, Quit, ToggleSideChat,
        ToggleSidePanel,
    },
    copy::{fill, nav, rail, settings, topic_rail},
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
        BringAllToFront
    ]
);

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
            MenuItem::action(rail::RAIL_DESK, OpenDesk),
            MenuItem::action(rail::RAIL_ARTIFACTS, OpenArtifacts),
            MenuItem::action(rail::RAIL_MEMBERS, OpenMembers),
            MenuItem::separator(),
            MenuItem::action(nav::MENU_NEW_TOPIC, NewTopic),
            MenuItem::action(nav::MENU_NEW_GROUP, NewGroup),
            MenuItem::action(nav::MENU_START_DM, StartDm),
            MenuItem::separator(),
            MenuItem::action(nav::MENU_NEXT_UNREAD, NextUnreadTopic),
            MenuItem::action(nav::MENU_PREVIOUS_UNREAD, PreviousUnreadTopic),
        ]),
        Menu::new(nav::MENU_VIEW).items([
            MenuItem::action(nav::MENU_SIDE_PANEL, ToggleSidePanel),
            MenuItem::action(topic_rail::TITLE_SIDE_CHAT, ToggleSideChat),
            MenuItem::separator(),
            MenuItem::action(nav::MENU_FULL_SCREEN, ToggleFullScreen),
        ]),
        Menu::new(nav::MENU_WINDOW).items([
            MenuItem::action(nav::MENU_CLOSE_WINDOW, CloseWindow),
            MenuItem::action(nav::MENU_MINIMIZE, Minimize),
            MenuItem::action(nav::MENU_ZOOM, Zoom),
            MenuItem::separator(),
            MenuItem::action(nav::MENU_SHOW_MAIN, ShowMainWindow),
            MenuItem::action(nav::MENU_BRING_ALL_TO_FRONT, BringAllToFront),
        ]),
    ]
}

/// Sets the menus and the bindings and handlers of the menu-only actions.
pub fn install(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("cmd-h", Hide, None),
        KeyBinding::new("cmd-alt-h", HideOthers, None),
        KeyBinding::new("cmd-m", Minimize, None),
        KeyBinding::new("cmd-w", CloseWindow, None),
        KeyBinding::new("ctrl-cmd-f", ToggleFullScreen, None),
    ]);
    cx.on_action(|_: &Hide, cx| cx.hide());
    cx.on_action(|_: &HideOthers, cx| cx.hide_other_apps());
    cx.on_action(|_: &ShowAll, cx| cx.unhide_other_apps());
    cx.on_action(|_: &Minimize, cx| with_key_window(cx, |window| window.minimize_window()));
    // Like the red traffic light: the app keeps running, and a click on the
    // Dock icon opens the main window again.
    cx.on_action(|_: &CloseWindow, cx| with_key_window(cx, |window| window.remove_window()));
    cx.on_action(|_: &Zoom, cx| with_key_window(cx, |window| window.zoom_window()));
    cx.on_action(|_: &ToggleFullScreen, cx| {
        with_key_window(cx, |window| window.toggle_fullscreen())
    });
    cx.on_action(|_: &BringAllToFront, cx| cx.activate(true));
    cx.set_menus(app_menus());
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
}
