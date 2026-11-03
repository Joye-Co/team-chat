//! The scenes of the screenshot self-test.
//!
//! A scene is one window in one state. To add a scene, add one entry to
//! [`SCENES`] (v1 and shared scenes) or to the `SCENES` slice of a v2
//! feature folder (`views/desk/scenes.rs`, …), which [`all`] chains: a name (the PNG is `<name>-<theme>.png`), what it shows, the
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
    /// Signed in with the v2 sample team of the 1.1.0 artboards
    /// (`fake::v2::V2Samples`).
    V2,
    /// The v2 sample team on a quiet day: nothing waits on me, nothing runs
    /// (desk track, `desk-empty`).
    V2Quiet,
    // --- provider track ---
    /// The real chat-core provider on a seeded cache, with no server
    /// (`provider::seeded`).
    Core,
    // --- end provider track ---
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
        name: "components-v3",
        about: "The component gallery: v3 tokens (surfaces, member tints, states, shadows), buttons, tags, rings, rail rows and motion",
        start: Start::V2,
        setup: |_, _| {},
        open: |s, c, cx| open_gallery(s, c, 2, cx),
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
        about: "The 待办 list (its sidebar header selected) with the first entry that waits on me open and selected",
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
        about: "The login view after a wrong invite code: kept, caret after it, error ring",
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
        name: "settings-add-bot-new",
        about: "Settings: the add-bot dialog with a name outside the roster: the first free character is selected",
        start: Start::SignedIn,
        setup: |_, _| {},
        open: settings_scenes::open_bot_dialog_new_name,
    },
    Scene {
        name: "settings-character",
        about: "Settings: the dialog that changes a bot's character, over 成员",
        start: Start::SignedIn,
        setup: |_, _| {},
        open: settings_scenes::open_character_dialog,
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
        name: "settings-notifications-end",
        about: "Settings: 通知 scrolled to the end: the 私聊 table and 关注的话题",
        start: Start::SignedIn,
        setup: |_, _| {},
        open: settings_scenes::open_notifications_end,
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
        name: "add-members",
        about: "The sheet that adds members to the open 群 (from its menu in the sidebar)",
        start: Start::SignedIn,
        setup: crate::views::nav::selftest::scenes::open_smallest_group,
        open: crate::views::nav::selftest::scenes::add_members,
    },
    Scene {
        name: "rename-topic",
        about: "The dialog that renames a 话题 (from the topic menu)",
        start: Start::SignedIn,
        setup: open_first_topic,
        open: crate::views::nav::selftest::scenes::rename_topic,
    },
    Scene {
        name: "group-menu",
        about: "The menu of a 群 row in the sidebar (right-click): 静音, 重命名群…, 添加成员, 离开群",
        start: Start::SignedIn,
        setup: open_first_topic,
        open: crate::views::nav::selftest::scenes::group_menu,
    },
    Scene {
        name: "rename-group",
        about: "The dialog that renames a 群 (from its menu), the name prefilled and selected",
        start: Start::SignedIn,
        setup: open_first_topic,
        open: crate::views::nav::selftest::scenes::rename_group,
    },
    Scene {
        name: "rename-group-error",
        about: "The 重命名群 dialog with a new name typed and the server's refusal under the field",
        start: Start::SignedIn,
        setup: open_first_topic,
        open: crate::views::nav::selftest::scenes::rename_group_error,
    },
    Scene {
        name: "topic-dm",
        about: "A 话题 in a 私聊 with a person: the header breadcrumb shows the peer's avatar, not #",
        start: Start::SignedIn,
        setup: |s, cx| crate::views::nav::selftest::scenes::open_dm_topic(s, false, cx),
        open: open_main,
    },
    Scene {
        name: "topic-dm-bot",
        about: "A 话题 in a 私聊 with a bot: the breadcrumb shows the bot's tile with its character",
        start: Start::SignedIn,
        setup: |s, cx| crate::views::nav::selftest::scenes::open_dm_topic(s, true, cx),
        open: open_main,
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
        name: "panel-run",
        about: "The side panel of a 话题 where a bot member works: its state, latest steps and 成员",
        start: Start::SignedIn,
        setup: crate::views::process::preview::open_running_details,
        open: open_main,
    },
    Scene {
        name: "panel-failed",
        about: "The side panel of a 话题 whose bot run failed",
        start: Start::SignedIn,
        setup: crate::views::process::preview::open_failed_details,
        open: open_main,
    },
    Scene {
        name: "panel-plan",
        about: "The side panel of the sample-card 话题: a plan list from a card",
        start: Start::SignedIn,
        setup: crate::views::process::preview::open_plan_details,
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
        name: "stop",
        about: "main-* with the 过程 tab: 停止 on the pending row and in the 过程 header, for the member who asked",
        start: Start::SignedIn,
        setup: |_, _| {},
        open: crate::views::timeline::preview::open_stop,
    },
    Scene {
        name: "stop-stopping",
        about: "The stop scene after 停止 was pressed: both show 正在停止… until the run ends",
        start: Start::SignedIn,
        setup: |_, _| {},
        open: crate::views::timeline::preview::open_stopping,
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
    // --- web-embed: 产出 pages live inside the window (v11/web-embed) ---
    Scene {
        name: "web-embed",
        about: "An HTML 产出 live inline in a 话题 and in the 产出 pane, with local video and audio",
        start: Start::SignedIn,
        setup: |_, _| {},
        open: |s, c, cx| embed_preview::open(s, c, EmbedMode::Topic, cx),
    },
    Scene {
        name: "web-embed-scrolled",
        about: "web-embed with the 话题 scrolled: the inline page is cut off at the clip edge",
        start: Start::SignedIn,
        setup: |_, _| {},
        open: |s, c, cx| embed_preview::open(s, c, EmbedMode::Scrolled, cx),
    },
    Scene {
        name: "web-embed-covered",
        about: "web-embed with a menu over the inline page: the page hides and its snapshot shows",
        start: Start::SignedIn,
        setup: |_, _| {},
        open: |s, c, cx| embed_preview::open(s, c, EmbedMode::Covered, cx),
    },
    // --- end web-embed ---
    // --- v11/media: 产出 media previews (v11/media) ---
    Scene {
        name: "media-html",
        about: "HTML 产出: live inline in a 话题 and in the 产出 pane with versions and events",
        start: Start::SignedIn,
        setup: |_, _| {},
        open: |s, c, cx| {
            media_preview::open(
                s,
                c,
                MediaSample::from_name("media-html").expect("sample"),
                cx,
            )
        },
    },
    Scene {
        name: "media-markdown",
        about: "Markdown 产出: native excerpt with chips, reader with a table of contents",
        start: Start::SignedIn,
        setup: |_, _| {},
        open: |s, c, cx| {
            media_preview::open(
                s,
                c,
                MediaSample::from_name("media-markdown").expect("sample"),
                cx,
            )
        },
    },
    Scene {
        name: "media-images",
        about: "Image set: grid of 4 with +n, lightbox with filmstrip",
        start: Start::SignedIn,
        setup: |_, _| {},
        open: |s, c, cx| {
            media_preview::open(
                s,
                c,
                MediaSample::from_name("media-images").expect("sample"),
                cx,
            )
        },
    },
    Scene {
        name: "media-video",
        about: "Video: poster, chapter ticks and frame strip; player with chapters and frames",
        start: Start::SignedIn,
        setup: |_, _| {},
        open: |s, c, cx| {
            media_preview::open(
                s,
                c,
                MediaSample::from_name("media-video").expect("sample"),
                cx,
            )
        },
    },
    Scene {
        name: "media-audio",
        about: "Audio: waveform player; big waveform, speakers, speeds and transcript",
        start: Start::SignedIn,
        setup: |_, _| {},
        open: |s, c, cx| {
            media_preview::open(
                s,
                c,
                MediaSample::from_name("media-audio").expect("sample"),
                cx,
            )
        },
    },
    Scene {
        name: "media-pdf",
        about: "PDF: page thumbnails; reader with page sidebar and zoom",
        start: Start::SignedIn,
        setup: |_, _| {},
        open: |s, c, cx| {
            media_preview::open(
                s,
                c,
                MediaSample::from_name("media-pdf").expect("sample"),
                cx,
            )
        },
    },
    Scene {
        name: "media-code",
        about: "Code and table inline: the pointed lines and the first rows; the whole code file",
        start: Start::SignedIn,
        setup: |_, _| {},
        open: |s, c, cx| {
            media_preview::open(
                s,
                c,
                MediaSample::from_name("media-code").expect("sample"),
                cx,
            )
        },
    },
    Scene {
        name: "media-table",
        about: "Table: the sortable table with a fixed header",
        start: Start::SignedIn,
        setup: |_, _| {},
        open: |s, c, cx| {
            media_preview::open(
                s,
                c,
                MediaSample::from_name("media-table").expect("sample"),
                cx,
            )
        },
    },
    Scene {
        name: "media-states",
        about: "Upload and preview states: uploading, generating, ready, failed, too large",
        start: Start::SignedIn,
        setup: |_, _| {},
        open: |s, c, cx| {
            media_preview::open(
                s,
                c,
                MediaSample::from_name("media-states").expect("sample"),
                cx,
            )
        },
    },
    // --- end v11/media ---
    // --- v11/inc-diagrams: diagrams in a 话题 (Refs.dc.html §04, §06, §07) ---
    Scene {
        name: "diagram-inline",
        about: "A sequence diagram, a flowchart and a gantt drawn inline in a 话题",
        start: Start::SignedIn,
        setup: |_, _| {},
        open: |s, c, cx| crate::diagrams::preview::open(s, c, DiagramScene::Inline, cx),
    },
    Scene {
        name: "diagram-error",
        about: "Each diagram error kind, a fifth fence that stays code and a streaming body",
        start: Start::SignedIn,
        setup: |_, _| {},
        open: |s, c, cx| crate::diagrams::preview::open(s, c, DiagramScene::Error, cx),
    },
    Scene {
        name: "diagram-viewer",
        about: "The diagram viewer zoomed to 250 %",
        start: Start::SignedIn,
        setup: |_, _| {},
        open: |s, c, cx| crate::diagrams::preview::open(s, c, DiagramScene::Viewer, cx),
    },
    // --- end v11/inc-diagrams ---
];

/// Every scene: the shared ones, then each v2 feature's slice, in order.
pub fn all() -> Vec<&'static Scene> {
    let slices: [&'static [Scene]; 13] = [
        SCENES,
        crate::views::desk::scenes::SCENES,
        crate::views::rail::scenes::SCENES,
        crate::views::topic_page::scenes::SCENES,
        crate::views::side_chat::scenes::SCENES,
        crate::views::live::scenes::SCENES,
        crate::views::nav::ask_scenes::SCENES,
        crate::views::group_page::scenes::SCENES,
        crate::views::members_page::scenes::SCENES,
        crate::views::artifact_page::scenes::SCENES,
        V2_SHARED,
        // --- provider track ---
        CORE_SCENES,
        // --- end provider track ---
        // --- cards v2 track ---
        crate::views::cards::v2::scenes::SCENES,
        // --- end cards v2 track ---
    ];
    slices.into_iter().flatten().collect()
}

// --- provider track ---
/// Scenes on the real chat-core provider (seeded cache, no server): the v2 topic parts it
/// reads from the cache.
const CORE_SCENES: &[Scene] = &[Scene {
    name: "topic-core",
    about: "话题 page on the real provider and a seeded cache: 改动 from the PR link, the pinned review, 产出 with its preview, 用到的决定 and 用时和成本",
    start: Start::Core,
    setup: |s, cx| {
        v2_open_topic(
            s,
            crate::provider::seeded::GROUP,
            crate::provider::seeded::TOPIC,
            cx,
        )
    },
    open: open_main,
}];
// --- end provider track ---

/// v2 scenes whose feature lives outside its own folder.
const V2_SHARED: &[Scene] = &[
    Scene {
        name: "settings-appearance",
        about: "设置 → 外观: theme and 侧栏样式",
        start: Start::V2,
        setup: |_, _| {},
        open: crate::views::settings::appearance::open_scene,
    },
    // cards-v2: moved to the cards v2 track's slice (views/cards/v2/scenes.rs).
];

// --- web-embed: scenes that wait for native content and check their capture ---

use crate::views::artifacts::embed_preview::{self, Mode as EmbedMode};

/// For a scene whose window shows native content (a web page) that GPUI's frames do not
/// cover: true once it is ready to capture.
pub fn ready_check(name: &str) -> Option<fn(&App) -> bool> {
    // --- v11/media ---
    if let Some(check) = media_ready(name) {
        return Some(check);
    }
    // --- end v11/media ---
    // --- v11/inc-diagrams ---
    if let Some(check) = diagram_ready(name) {
        return Some(check);
    }
    // --- end v11/inc-diagrams ---
    match name {
        "web-embed" => Some(|cx| embed_preview::ready(EmbedMode::Topic, cx)),
        "web-embed-scrolled" => Some(|cx| embed_preview::ready(EmbedMode::Scrolled, cx)),
        "web-embed-covered" => Some(|cx| embed_preview::ready(EmbedMode::Covered, cx)),
        // pages track: the 产出 page shows its live page.
        "artifact-v2" | "artifact-v2-old" | "artifact-pane-v2" => {
            Some(crate::views::artifact_page::scenes::ready)
        }
        // --- r2-trust ---
        "artifact-v2-choose" | "artifact-quiet-page" => {
            Some(crate::views::artifact_page::gate_check::ready)
        }
        // --- end r2-trust ---
        _ => None,
    }
}

/// For a scene that checks its own capture: runs after the PNG is written.
pub fn verify_check(name: &str) -> Option<fn(&std::path::Path, &mut App) -> Result<(), String>> {
    // --- v11/media ---
    if let Some(check) = media_verify(name) {
        return Some(check);
    }
    // --- end v11/media ---
    match name {
        "web-embed" => Some(|png, cx| embed_preview::verify(EmbedMode::Topic, png, cx)),
        "web-embed-scrolled" => Some(|png, cx| embed_preview::verify(EmbedMode::Scrolled, png, cx)),
        "web-embed-covered" => Some(|png, cx| embed_preview::verify(EmbedMode::Covered, png, cx)),
        // --- r2-trust ---
        "artifact-v2-choose" | "artifact-quiet-page" => {
            Some(crate::views::artifact_page::gate_check::verify)
        }
        // --- end r2-trust ---
        _ => None,
    }
}

// --- end web-embed ---

// --- v11/inc-diagrams: diagram scenes wait until every diagram has an answer ---

use crate::diagrams::preview::Scene as DiagramScene;

fn diagram_ready(name: &str) -> Option<fn(&App) -> bool> {
    match name {
        "diagram-inline" => Some(|cx| crate::diagrams::preview::ready(DiagramScene::Inline, cx)),
        "diagram-error" => Some(|cx| crate::diagrams::preview::ready(DiagramScene::Error, cx)),
        "diagram-viewer" => Some(|cx| crate::diagrams::preview::ready(DiagramScene::Viewer, cx)),
        _ => None,
    }
}

// --- end v11/inc-diagrams ---

// --- v11/media: media scenes wait until their samples are read and pages drew ---

use crate::views::media::preview::{self as media_preview, Sample as MediaSample};

fn media_ready(name: &str) -> Option<fn(&App) -> bool> {
    match name {
        "media-html" => Some(|cx| {
            media_preview::ready(MediaSample::from_name("media-html").expect("sample"), cx)
        }),
        "media-markdown" => Some(|cx| {
            media_preview::ready(
                MediaSample::from_name("media-markdown").expect("sample"),
                cx,
            )
        }),
        "media-images" => Some(|cx| {
            media_preview::ready(MediaSample::from_name("media-images").expect("sample"), cx)
        }),
        "media-video" => Some(|cx| {
            media_preview::ready(MediaSample::from_name("media-video").expect("sample"), cx)
        }),
        "media-audio" => Some(|cx| {
            media_preview::ready(MediaSample::from_name("media-audio").expect("sample"), cx)
        }),
        "media-pdf" => Some(|cx| {
            media_preview::ready(MediaSample::from_name("media-pdf").expect("sample"), cx)
        }),
        "media-code" => Some(|cx| {
            media_preview::ready(MediaSample::from_name("media-code").expect("sample"), cx)
        }),
        "media-table" => Some(|cx| {
            media_preview::ready(MediaSample::from_name("media-table").expect("sample"), cx)
        }),
        "media-states" => Some(|cx| {
            media_preview::ready(MediaSample::from_name("media-states").expect("sample"), cx)
        }),
        _ => None,
    }
}

#[allow(clippy::type_complexity)]
fn media_verify(name: &str) -> Option<fn(&std::path::Path, &mut App) -> Result<(), String>> {
    match name {
        "media-html" => Some(|_, cx| {
            media_preview::verify(MediaSample::from_name("media-html").expect("sample"), cx)
        }),
        "media-markdown" => Some(|_, cx| {
            media_preview::verify(
                MediaSample::from_name("media-markdown").expect("sample"),
                cx,
            )
        }),
        "media-images" => Some(|_, cx| {
            media_preview::verify(MediaSample::from_name("media-images").expect("sample"), cx)
        }),
        "media-video" => Some(|_, cx| {
            media_preview::verify(MediaSample::from_name("media-video").expect("sample"), cx)
        }),
        "media-audio" => Some(|_, cx| {
            media_preview::verify(MediaSample::from_name("media-audio").expect("sample"), cx)
        }),
        "media-pdf" => Some(|_, cx| {
            media_preview::verify(MediaSample::from_name("media-pdf").expect("sample"), cx)
        }),
        "media-code" => Some(|_, cx| {
            media_preview::verify(MediaSample::from_name("media-code").expect("sample"), cx)
        }),
        "media-table" => Some(|_, cx| {
            media_preview::verify(MediaSample::from_name("media-table").expect("sample"), cx)
        }),
        "media-states" => Some(|_, cx| {
            media_preview::verify(MediaSample::from_name("media-states").expect("sample"), cx)
        }),
        _ => None,
    }
}

// --- end v11/media ---

pub fn find(name: &str) -> Option<&'static Scene> {
    all().into_iter().find(|s| s.name == name)
}

/// v2 setups: open a 话题 of the sample team with the default pane.
pub fn v2_open_topic(services: &Services, group: &str, topic: &str, cx: &mut App) {
    let (g, t) = (group.to_string(), topic.to_string());
    services.nav.update(cx, |nav, cx| {
        nav.open_group(g.clone(), cx);
        nav.open_topic(g, t, cx);
    });
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
    // An entry that waits on me stays in the list once read (a mention
    // leaves it), so the list shows its row selected.
    let todo = services.provider.todo();
    let first = todo
        .iter()
        .find(|item| item.reason == crate::data::TodoReason::WaitingOnMe)
        .or(todo.first())
        .cloned();
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
pub fn capture_bounds(content: Size<Pixels>, cx: &App) -> Bounds<Pixels> {
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
