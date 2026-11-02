//! The scenes of the screenshot self-test.
//!
//! A scene is one window in one state. To add a scene, add one entry to
//! [`SCENES`]: a name (the PNG is `<name>-<theme>.png`), what it shows, the
//! data it starts from, a `setup` that moves navigation (or anything else in
//! `Services`) into place, and an `open` that opens the window. Use
//! [`open_main`] or [`open_settings`], or write your own opener for a new
//! window kind. The runner forces the theme, waits for the frames, captures
//! the window and closes it.

use gpui::{AnyWindowHandle, App, AppContext as _, Bounds, Pixels, Size, WindowBounds};

use crate::{
    app::{Place, RootView, Services, main_window_options},
    fake,
    views::settings::selftest as settings_scenes,
};

/// What a scene's provider starts with.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Start {
    /// Signed in with the sample data (worst-case data with `--worst-case`).
    SignedIn,
    /// Signed out: the login view.
    SignedOut,
    /// Signed in, but no 群, 话题 or messages: a new team on its first day.
    Empty,
    /// Signed in with the sample data while the connection is down.
    Offline,
}

pub struct Scene {
    pub name: &'static str,
    pub about: &'static str,
    pub start: Start,
    /// Runs before the window opens.
    pub setup: fn(&Services, &mut App),
    /// Opens the window with the requested content size.
    pub open: fn(Services, Size<Pixels>, &mut App) -> Result<AnyWindowHandle, String>,
}

/// Every scene, in the default order.
pub const SCENES: &[Scene] = &[
    Scene {
        name: "components",
        about: "The component gallery: every shared component of DESIGN.md §7 in its states",
        start: Start::SignedIn,
        setup: |_, _| {},
        open: |s, c, cx| open_gallery(s, c, 0, cx),
    },
    Scene {
        name: "components-2",
        about: "The component gallery: the composer, overlays and surfaces",
        start: Start::SignedIn,
        setup: |_, _| {},
        open: |s, c, cx| open_gallery(s, c, 1, cx),
    },
    Scene {
        name: "main",
        about: "First 群 of the sidebar with its newest ordinary 话题 open",
        start: Start::SignedIn,
        setup: open_first_topic,
        open: open_main,
    },
    Scene {
        name: "new-topic",
        about: "First 群 with no 话题 open: the composer starts a new 话题",
        start: Start::SignedIn,
        setup: open_first_group,
        open: open_main,
    },
    Scene {
        name: "todo",
        about: "The 待办 list with its first entry open",
        start: Start::SignedIn,
        setup: open_todo,
        open: open_main,
    },
    Scene {
        name: "long",
        about: "The 10,000-message 话题, anchored at the bottom",
        start: Start::SignedIn,
        setup: open_long_topic,
        open: open_main,
    },
    Scene {
        name: "empty",
        about: "Signed in with no 群 at all",
        start: Start::Empty,
        setup: open_first_topic,
        open: open_main,
    },
    Scene {
        name: "login",
        about: "The login view",
        start: Start::SignedOut,
        setup: |_, _| {},
        open: open_main,
    },
    Scene {
        name: "login-code",
        about: "The login view with half an invite code typed and the boxes focused",
        start: Start::SignedOut,
        setup: |_, _| {},
        open: |services, content, cx| open_login(services, content, "K7QM2X", false, cx),
    },
    Scene {
        name: "login-server",
        about: "The login view with the server field open",
        start: Start::SignedOut,
        setup: |_, _| {},
        open: |services, content, cx| open_login(services, content, "", true, cx),
    },
    Scene {
        name: "login-custom-server",
        about: "The login view on a custom server: host · 更改",
        start: Start::SignedOut,
        setup: |_, _| {},
        open: |services, content, cx| {
            open_login(services, content, "~https://team.joye.co", false, cx)
        },
    },
    Scene {
        name: "login-error",
        about: "The login view after a wrong invite code: kept, selected, error ring",
        start: Start::SignedOut,
        setup: |_, _| {},
        open: |services, content, cx| open_login(services, content, "!0000-0000", false, cx),
    },
    Scene {
        name: "offline",
        about: "The connection banner above the sidebar footer while reconnecting",
        start: Start::Offline,
        setup: open_first_topic,
        open: open_main,
    },
    Scene {
        name: "settings-profile",
        about: "Settings: 我的资料",
        start: Start::SignedIn,
        setup: |_, _| {},
        open: settings_scenes::open_profile,
    },
    Scene {
        name: "settings-invite",
        about: "Settings: the invite dialog over 成员",
        start: Start::SignedIn,
        setup: |_, _| {},
        open: settings_scenes::open_invite_dialog,
    },
    Scene {
        name: "settings-add-bot",
        about: "Settings: the add-bot dialog over 成员",
        start: Start::SignedIn,
        setup: |_, _| {},
        open: settings_scenes::open_bot_dialog,
    },
    Scene {
        name: "settings-add-machine",
        about: "Settings: the add-machine dialog over 机器",
        start: Start::SignedIn,
        setup: |_, _| {},
        open: settings_scenes::open_machine_dialog,
    },
    Scene {
        name: "settings",
        about: "The settings window",
        start: Start::SignedIn,
        setup: |_, _| {},
        open: open_settings,
    },
    Scene {
        name: "settings-members",
        about: "Settings: 成员 with the owner actions",
        start: Start::SignedIn,
        setup: |_, _| {},
        open: settings_scenes::open_members,
    },
    Scene {
        name: "settings-machines",
        about: "Settings: 机器 and their health",
        start: Start::SignedIn,
        setup: |_, _| {},
        open: settings_scenes::open_machines,
    },
    Scene {
        name: "settings-notifications",
        about: "Settings: 通知 per 群 and followed 话题",
        start: Start::SignedIn,
        setup: |_, _| {},
        open: settings_scenes::open_notifications,
    },
    Scene {
        name: "settings-cost",
        about: "Settings: 成本 placeholder",
        start: Start::SignedIn,
        setup: |_, _| {},
        open: settings_scenes::open_cost,
    },
    Scene {
        name: "settings-about",
        about: "Settings: 通用 (about card, diagnostics, developer view)",
        start: Start::SignedIn,
        setup: |_, _| {},
        open: settings_scenes::open_about,
    },
    Scene {
        name: "settings-members-end",
        about: "Settings: 成员 scrolled to the end, with the footnote under the table",
        start: Start::SignedIn,
        setup: |_, _| {},
        open: settings_scenes::open_members_end,
    },
    Scene {
        name: "settings-profile-error",
        about: "Settings: 我的资料 with the error of an empty name",
        start: Start::SignedIn,
        setup: |_, _| {},
        open: settings_scenes::open_profile_error,
    },
    Scene {
        name: "settings-about-dev",
        about: "Settings: 通用 with the developer view on",
        start: Start::SignedIn,
        setup: |services, cx| {
            services
                .nav
                .update(cx, |nav, cx| nav.set_developer_view(true, cx))
        },
        open: settings_scenes::open_about,
    },
    Scene {
        name: "update",
        about: "The quiet banner of a new version above the 话题",
        start: Start::SignedIn,
        setup: open_first_topic,
        open: crate::updater::preview::open_available,
    },
    Scene {
        name: "update-dl",
        about: "The update banner while the new version downloads",
        start: Start::SignedIn,
        setup: open_first_topic,
        open: crate::updater::preview::open_downloading,
    },
    Scene {
        name: "update-manual",
        about: "The update banner when the app cannot be replaced",
        start: Start::SignedIn,
        setup: open_first_topic,
        open: crate::updater::preview::open_manual,
    },
    Scene {
        name: "files",
        about: "A 话题 with an image, files (downloading, uploading), a 产出 card and the side panel lists",
        start: Start::SignedIn,
        setup: |_, _| {},
        open: crate::views::files::preview::open,
    },
    Scene {
        name: "cards",
        about: "The 话题 of sample cards: status, fields, table, progress, list, code and actions",
        start: Start::SignedIn,
        setup: |services, cx| {
            services.nav.update(cx, |nav, cx| {
                nav.open_topic(
                    crate::views::cards::samples::GROUP.into(),
                    crate::views::cards::samples::TOPIC.into(),
                    cx,
                )
            })
        },
        open: open_main,
    },
    Scene {
        name: "switcher",
        about: "The Cmd+K quick switcher over the main window, with no query",
        start: Start::SignedIn,
        setup: open_first_topic,
        open: open_switcher,
    },
    Scene {
        name: "switcher-query",
        about: "The quick switcher with a query: marked matches in 话题, 群 and 消息",
        start: Start::SignedIn,
        setup: open_first_topic,
        open: crate::views::nav::selftest::scenes::switcher_query,
    },
    Scene {
        name: "new-group",
        about: "The new-群 sheet: name, member picker and buttons",
        start: Start::SignedIn,
        setup: open_first_topic,
        open: crate::views::nav::selftest::scenes::new_group,
    },
    Scene {
        name: "start-dm",
        about: "The member picker that starts a DM",
        start: Start::SignedIn,
        setup: open_first_topic,
        open: crate::views::nav::selftest::scenes::start_dm,
    },
    Scene {
        name: "leave-group",
        about: "The alert before leaving a 群",
        start: Start::SignedIn,
        setup: open_first_topic,
        open: crate::views::nav::selftest::scenes::leave_group,
    },
    Scene {
        name: "search",
        about: "Cmd+F: the switcher scoped to the current 群 with a Chinese query",
        start: Start::SignedIn,
        setup: open_first_topic,
        open: crate::views::search::preview::open_dialog,
    },
    Scene {
        name: "search-hit",
        about: "A search hit 5,000 messages up opened in its 话题 and highlighted",
        start: Start::SignedIn,
        setup: crate::views::search::preview::open_far_hit,
        open: open_main,
    },
    Scene {
        name: "process",
        about: "The 过程 tab of a 话题 where a bot member works",
        start: Start::SignedIn,
        setup: crate::views::process::preview::open_running,
        open: open_main,
    },
    Scene {
        name: "process-failed",
        about: "The 过程 tab of a failed bot run",
        start: Start::SignedIn,
        setup: crate::views::process::preview::open_failed,
        open: open_main,
    },
    Scene {
        name: "process-dev",
        about: "The 过程 tab with the developer view on: ids and raw events",
        start: Start::SignedIn,
        setup: crate::views::process::preview::open_running_developer,
        open: open_main,
    },
    Scene {
        name: "topic",
        about: "main-*: a long 话题 with a date divider, a steps row, a 产出, an ask card, a bot at work and the @ list",
        start: Start::SignedIn,
        setup: |_, _| {},
        open: crate::views::timeline::preview::open_main,
    },
    Scene {
        name: "topic-short",
        about: "short-topic-*: the start block, a draft card and the hover toolbar",
        start: Start::SignedIn,
        setup: |_, _| {},
        open: crate::views::timeline::preview::open_short,
    },
    Scene {
        name: "topic-worst",
        about: "worst-case-*: a long title, a URL, jumbo emoji, a big download, a failed send, many typing",
        start: Start::SignedIn,
        setup: |_, _| {},
        open: crate::views::timeline::preview::open_worst,
    },
    Scene {
        name: "topic-react",
        about: "short-topic-* with the hover toolbar's emoji picker open (add reaction)",
        start: Start::SignedIn,
        setup: |_, _| {},
        open: crate::views::timeline::preview::open_react,
    },
    Scene {
        name: "topic-compose",
        about: "The composer while reconnecting: the sidebar banner, a quote chip, the format bar and the emoji picker",
        start: Start::SignedIn,
        setup: |_, _| {},
        open: crate::views::timeline::preview::open_compose,
    },
];

pub fn find(name: &str) -> Option<&'static Scene> {
    SCENES.iter().find(|s| s.name == name)
}

// --- setups ---

pub fn open_first_group(services: &Services, cx: &mut App) {
    if let Some(group) = services.provider.groups().first() {
        let id = group.id.clone();
        services.nav.update(cx, |nav, cx| nav.open_group(id, cx));
    }
}

pub fn open_first_topic(services: &Services, cx: &mut App) {
    open_first_group(services, cx);
    let Some(group) = services.provider.groups().first().cloned() else {
        return;
    };
    // Skip the 10,000-message topic: the `long` scene shows it.
    let topics = services.provider.topics(&group.id);
    let topic = topics
        .iter()
        .find(|t| t.id != fake::LONG_TOPIC)
        .or(topics.first());
    if let Some(topic) = topic {
        let topic_id = topic.id.clone();
        services
            .nav
            .update(cx, |nav, cx| nav.open_topic(group.id.clone(), topic_id, cx));
    }
}

pub fn open_todo(services: &Services, cx: &mut App) {
    let first = services.provider.todo().into_iter().next();
    services.nav.update(cx, |nav, cx| {
        nav.open_todo(cx);
        if let Some(item) = first {
            nav.open_topic(item.group_id, item.topic.id, cx);
        }
        debug_assert_eq!(nav.place, Some(Place::Todo));
    });
}

pub fn open_long_topic(services: &Services, cx: &mut App) {
    services.nav.update(cx, |nav, cx| {
        nav.open_topic(fake::LONG_TOPIC_GROUP.into(), fake::LONG_TOPIC.into(), cx)
    });
}

// --- openers ---

/// `content` centered on the main display below the menu bar. The Dock may
/// cover the bottom edge: it does not show in a capture of the window, and
/// `Bounds::centered` would clip a 900 pt window to a visible frame that the
/// Dock shortens (887 pt on a 982 pt display).
fn capture_bounds(content: Size<Pixels>, cx: &App) -> Bounds<Pixels> {
    let Some(display) = cx.primary_display() else {
        return Bounds::centered(None, content, cx);
    };
    let full = display.bounds();
    let top = display.visible_bounds().origin.y;
    let room = gpui::size(full.size.width, full.bottom() - top);
    let size = content.min(&room);
    // Whole points, so the capture is exactly `content` at 2x.
    let origin = gpui::point(
        (full.origin.x + (room.width - size.width) / 2.).round(),
        (top + (room.height - size.height) / 2.).round(),
    );
    Bounds { origin, size }
}

/// The main window (login or chat) without focus, at `content`.
pub fn open_main(
    services: Services,
    content: Size<Pixels>,
    cx: &mut App,
) -> Result<AnyWindowHandle, String> {
    let mut options = main_window_options(false, cx);
    options.window_bounds = Some(WindowBounds::Windowed(capture_bounds(content, cx)));
    options.window_min_size = None;
    gpui_kit::open_window(options, cx, move |window, cx| {
        cx.new(|cx| RootView::new(services, None, window, cx))
    })
    .map(|(handle, _)| handle)
    .map_err(|e| e.to_string())
}

/// The main window signed out, with the login form filled for a scene.
pub fn open_login(
    services: Services,
    content: Size<Pixels>,
    code: &'static str,
    server_open: bool,
    cx: &mut App,
) -> Result<AnyWindowHandle, String> {
    let mut options = main_window_options(false, cx);
    options.window_bounds = Some(WindowBounds::Windowed(capture_bounds(content, cx)));
    options.window_min_size = None;
    gpui_kit::open_window(options, cx, move |window, cx| {
        let root = cx.new(|cx| RootView::new(services, None, window, cx));
        let login = root.read(cx).login().clone();
        // A leading `!` submits the code, for the error scene.
        // A leading `!` submits the code, for the error scene; a leading `~`
        // sets a custom server instead.
        login.update(cx, |l, cx| {
            if let Some(code) = code.strip_prefix('!') {
                l.preview_submit(code, window, cx)
            } else if let Some(server) = code.strip_prefix('~') {
                l.preview_server(server, window, cx)
            } else {
                l.preview(code, server_open, window, cx)
            }
        });
        root
    })
    .map(|(handle, _)| handle)
    .map_err(|e| e.to_string())
}

/// The component gallery (DESIGN.md §16) at 1440 x 900.
pub fn open_gallery(
    _services: Services,
    content: Size<Pixels>,
    page: u8,
    cx: &mut App,
) -> Result<AnyWindowHandle, String> {
    let mut options = main_window_options(false, cx);
    options.window_bounds = Some(WindowBounds::Windowed(capture_bounds(content, cx)));
    options.window_min_size = None;
    gpui_kit::open_window(options, cx, move |window, cx| {
        cx.new(|cx| crate::ui::gallery::Gallery::new(page, window, cx))
    })
    .map(|(handle, _)| handle)
    .map_err(|e| e.to_string())
}

/// The settings window at its own size (the requested size is ignored).
pub fn open_settings(
    services: Services,
    content: Size<Pixels>,
    cx: &mut App,
) -> Result<AnyWindowHandle, String> {
    settings_scenes::open_profile(services, content, cx)
}

/// The main window with the quick switcher open (DESIGN.md §13.12).
pub fn open_switcher(
    services: Services,
    content: Size<Pixels>,
    cx: &mut App,
) -> Result<AnyWindowHandle, String> {
    let handle = open_main(services.clone(), content, cx)?;
    handle
        .update(cx, |_, window, cx| {
            crate::views::nav::switcher::open(services, window, cx)
        })
        .map_err(|e| e.to_string())?;
    Ok(handle)
}
