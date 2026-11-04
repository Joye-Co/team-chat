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
        ToggleSidePanel, ToggleSidebar,
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
    app_menus_with(MenuState::default())
}

/// The menus of the menu bar, with the check marks of `state`.
pub fn app_menus_with(state: MenuState) -> Vec<Menu> {
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
        ShowTopicChanges
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
                cx.set_menus(app_menus_with(state));
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
