//! The scenes of the screenshot self-test.
//!
//! A scene is one window in one state. To add a scene, add one entry to
//! [`SCENES`]: a name (the PNG is `<name>-<theme>.png`), what it shows, the
//! data it starts from, a `setup` that moves navigation (or anything else in
//! `Services`) into place, and an `open` that opens the window. Use
//! [`open_main`] or [`open_settings`], or write your own opener for a new
//! window kind. The runner forces the theme, waits for the frames, captures
//! the window and closes it.

use gpui::{
    AnyWindowHandle, App, AppContext as _, Bounds, Pixels, Size, TitlebarOptions, WindowBounds,
    WindowOptions, px, size,
};

use crate::{
    app::{Place, RootView, Services, main_window_options},
    copy, fake,
    views::settings::SettingsView,
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
        name: "settings",
        about: "The settings window",
        start: Start::SignedIn,
        setup: |_, _| {},
        open: open_settings,
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

/// The main window (login or chat) without focus, at `content`.
pub fn open_main(
    services: Services,
    content: Size<Pixels>,
    cx: &mut App,
) -> Result<AnyWindowHandle, String> {
    let mut options = main_window_options(false, cx);
    options.window_bounds = Some(WindowBounds::Windowed(Bounds::centered(None, content, cx)));
    options.window_min_size = None;
    gpui_kit::open_window(options, cx, move |window, cx| {
        cx.new(|cx| RootView::new(services, None, window, cx))
    })
    .map(|(handle, _)| handle)
    .map_err(|e| e.to_string())
}

/// The settings window at its own size (the requested size is ignored).
pub fn open_settings(
    services: Services,
    _content: Size<Pixels>,
    cx: &mut App,
) -> Result<AnyWindowHandle, String> {
    let options = WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
            None,
            size(px(640.), px(560.)),
            cx,
        ))),
        titlebar: Some(TitlebarOptions {
            title: Some(copy::settings::TITLE.into()),
            ..Default::default()
        }),
        focus: false,
        show: true,
        ..Default::default()
    };
    gpui_kit::open_window(options, cx, move |window, cx| {
        cx.new(|cx| SettingsView::new(services, window, cx))
    })
    .map(|(handle, _)| handle)
    .map_err(|e| e.to_string())
}
