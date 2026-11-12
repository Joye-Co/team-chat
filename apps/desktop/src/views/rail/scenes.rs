//! Screenshot scenes of the left rail (the rail track owns this slice).

use gpui::App;

use crate::{
    app::{RailMode, Services},
    selftest::scenes::{Scene, Start, open_main},
};

/// The rail over the 首页, as the artboards show it.
fn rail(services: &Services, mode: RailMode, cx: &mut App) {
    services.nav.update(cx, |nav, cx| nav.open_desk(cx));
    services
        .nav
        .update(cx, |nav, cx| nav.set_rail_mode(mode, cx));
}

pub const SCENES: &[Scene] = &[
    Scene {
        name: "rail-work",
        about: "Rail A 以工作为中心 (default): 话题 by 群 with live state over the 首页",
        start: Start::V2,
        setup: |s, cx| rail(s, RailMode::Work, cx),
        open: open_main,
    },
    Scene {
        name: "rail-muted",
        about: "Rail A with a muted 群, 私聊 and 话题: bell.slash after each name",
        start: Start::V2,
        setup: |s, cx| {
            rail(s, RailMode::Work, cx);
            let p = &s.provider;
            let groups = p.groups();
            if let Some(g) = groups
                .iter()
                .find(|g| g.kind == crate::data::GroupKind::Group)
            {
                p.set_group_muted(&groups[1.min(groups.len() - 1)].id, true);
                if let Some(t) = p.topics(&g.id).first() {
                    p.set_topic_muted(&g.id, &t.id, true);
                }
            }
            if let Some(dm) = groups.iter().find(|g| g.kind == crate::data::GroupKind::Dm) {
                p.set_group_muted(&dm.id, true);
            }
        },
        open: open_main,
    },
    Scene {
        name: "rail-attention",
        about: "Rail B 以「需要我」为中心: 需要你 / 在做 / 完成 · 今天",
        start: Start::V2,
        setup: |s, cx| rail(s, RailMode::Attention, cx),
        open: open_main,
    },
    Scene {
        name: "rail-member",
        about: "Rail C 以成员为中心: members with the 话题 they work on",
        start: Start::V2,
        setup: |s, cx| rail(s, RailMode::Member, cx),
        open: open_main,
    },
];

// --- v12/behavior ---
/// Context menus of the rail (NATIVE.md §7.3, §10): a right click on a 话题
/// row and on a 群 header, over the 首页.
pub const BEHAVIOR_SCENES: &[Scene] = &[
    Scene {
        name: "rail-topic-menu",
        about: "Right click on a 话题 in the rail: the topic menu (改名, 状态, 等谁, 负责, 关注), no shortcuts",
        start: Start::V2,
        setup: |s, cx| rail(s, RailMode::Work, cx),
        open: |s, c, cx| open_with_menu(s, c, false, cx),
    },
    Scene {
        name: "rail-group-menu",
        about: "Right click on a 群 header in the rail: 静音, 提醒 ▸, 重命名群…, 添加成员, 隐藏, 离开群, 删除群",
        start: Start::V2,
        setup: |s, cx| rail(s, RailMode::Work, cx),
        open: |s, c, cx| open_with_menu(s, c, true, cx),
    },
];

fn open_with_menu(
    services: Services,
    content: gpui::Size<gpui::Pixels>,
    group_header: bool,
    cx: &mut App,
) -> Result<gpui::AnyWindowHandle, String> {
    let mut options = crate::app::main_window_options(false, cx);
    options.window_bounds = Some(gpui::WindowBounds::Windowed(
        crate::selftest::scenes::capture_bounds(content, cx),
    ));
    options.window_min_size = None;
    let (handle, root) = gpui_kit::open_window(options, cx, move |window, cx| {
        gpui::AppContext::new(cx, |cx| {
            crate::app::RootView::new(services, None, window, cx)
        })
    })
    .map_err(|e| e.to_string())?;
    let rail = root.read(cx).rail().clone();
    handle
        .update(cx, |_, window, _| {
            right_click_when_drawn(rail, group_header, 0, window)
        })
        .map_err(|e| e.to_string())?;
    Ok(handle)
}

/// Right-clicks the row once the rail has drawn it.
fn right_click_when_drawn(
    rail: gpui::Entity<super::RailView>,
    group_header: bool,
    tries: u32,
    window: &mut gpui::Window,
) {
    window.on_next_frame(move |window, cx| {
        let Some(bounds) = rail.read(cx).first_row_bounds(group_header) else {
            if tries < 10 {
                right_click_when_drawn(rail, group_header, tries + 1, window);
            }
            return;
        };
        let position = bounds.center();
        let modifiers = gpui::Modifiers::default();
        window.dispatch_event(
            gpui::PlatformInput::MouseMove(gpui::MouseMoveEvent {
                position,
                pressed_button: None,
                modifiers,
            }),
            cx,
        );
        window.dispatch_event(
            gpui::PlatformInput::MouseDown(gpui::MouseDownEvent {
                button: gpui::MouseButton::Right,
                position,
                modifiers,
                click_count: 1,
                first_mouse: false,
            }),
            cx,
        );
        window.dispatch_event(
            gpui::PlatformInput::MouseUp(gpui::MouseUpEvent {
                button: gpui::MouseButton::Right,
                position,
                modifiers,
                click_count: 1,
            }),
            cx,
        );
        window.refresh();
    });
}
// --- end v12/behavior ---

// --- v12/shell ---
/// The main window as an inactive window (NATIVE.md §4, §10): the sidebar
/// selection turns gray and the toolbar title and symbols `secondary`.
pub const SHELL_SCENES: &[Scene] = &[Scene {
    name: "window-inactive",
    about: "The main window when another window is key: gray selection, secondary toolbar symbols",
    start: Start::SignedIn,
    setup: crate::selftest::scenes::open_first_topic,
    open: open_inactive,
}];

fn open_inactive(
    services: Services,
    content: gpui::Size<gpui::Pixels>,
    cx: &mut App,
) -> Result<gpui::AnyWindowHandle, String> {
    let mut options = crate::app::main_window_options(false, cx);
    options.window_bounds = Some(gpui::WindowBounds::Windowed(
        crate::selftest::scenes::capture_bounds(content, cx),
    ));
    options.window_min_size = None;
    let (handle, root) = gpui_kit::open_window(options, cx, move |window, cx| {
        gpui::AppContext::new(cx, |cx| {
            crate::app::RootView::new(services, None, window, cx)
        })
    })
    .map_err(|e| e.to_string())?;
    root.update(cx, |root, cx| root.show_inactive(cx));
    Ok(handle)
}
// --- end v12/shell ---
