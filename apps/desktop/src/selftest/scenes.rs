//! The scenes of the screenshot self-test.
//!
//! A scene is one window in one state. To add a scene, add one entry to
//! [`SCENES`] (v1 and shared scenes) or to the `SCENES` slice of a v2
//! feature folder (`views/desk/scenes.rs`, …), which [`all`] chains: a name (the PNG is `<name>-<theme>.png`), what it shows, the
//! data it starts from, a `setup` that moves navigation (or anything else in
//! `Services`) into place, and an `open` that opens the window. Use
//! [`open_main`] or a settings opener, or write your own opener for a new
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
    /// The v2 sample team seen by a member who just joined (onboarding,
    /// `fake::v2::V2Samples::newcomer`).
    V2Newcomer,
    // --- provider track ---
    /// The real chat-core provider on a seeded cache, with no server
    /// (`provider::seeded`).
    Core,
    // --- end provider track ---
    // --- inc-refs ---
    /// The v2 sample team with the 引用提问 messages (`fake::v2::refs_samples`).
    V2Refs,
    /// The v2 sample team with Card v2 blocks that have parts in their 话题
    /// (`fake::v2::refs_samples::with_card_parts`).
    V2RefParts,
    // --- end inc-refs ---
    /// The v2 sample team as the Card v2 gallery shows it: the release I approved
    /// runs (`fake::v2::V2Samples::with_release_run`).
    V2Gallery,
    /// The v2 sample team with a second member at work in 开发 / 虚拟列表
    /// (`fake::v2::V2Samples::with_second_run`).
    V2TwoRuns,
    // --- v13/ux-pages ---
    /// The v2 sample team's 群 with nothing synced yet, still connecting:
    /// the pages draw their skeletons (`selftest::page_states`).
    V2Unsynced,
    /// The same, while the server cannot be reached: the pages draw their
    /// failure Alert with 重试.
    V2Unreachable,
    /// The v2 sample team's 群 with no 话题, 产出 or runs, online: the
    /// pages' Empty states.
    V2Bare,
    // --- end v13/ux-pages ---
    /// The sample data with one part reshaped for a state scene
    /// ([`super::shaped`]): no messages, older messages loading, first sync.
    Shaped(super::shaped::Shape),
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
        name: "main-sidebar-hidden",
        about: "The main scene with the sidebar hidden: 显示侧栏 right after the traffic lights",
        start: Start::SignedIn,
        setup: open_first_topic_sidebar_hidden,
        open: open_main,
    },
    Scene {
        name: "new-topic",
        about: "The 新话题 sheet over the first 群 (v12/overlays)",
        start: Start::SignedIn,
        setup: open_first_group,
        open: crate::views::nav::selftest::scenes::new_topic,
    },
    // 1.1.1: the 群's own chat.
    Scene {
        name: "group-chat",
        about: "The 群聊 of 产品研发: plain chat with Jojo's answer, 详情 and 话题 in the title bar",
        start: Start::SignedIn,
        setup: |s, cx| {
            s.nav
                .update(cx, |nav, cx| nav.open_group_chat("g-dev".into(), cx))
        },
        open: open_main,
    },
    Scene {
        name: "group-chat-info",
        about: "The 群聊 of 产品研发 with its 详情 open: 群资料 with 成员 first",
        start: Start::SignedIn,
        setup: |s, cx| {
            s.nav.update(cx, |nav, cx| {
                nav.open_group_chat("g-dev".into(), cx);
                nav.set_right_pane(crate::app::RightPane::GroupInfo, cx);
            })
        },
        open: open_main,
    },
    Scene {
        name: "group-chat-empty",
        about: "The 群聊 of 运维告警 before anyone wrote there: its start block with 新话题",
        start: Start::SignedIn,
        setup: |s, cx| {
            s.nav
                .update(cx, |nav, cx| nav.open_group_chat("g-ops".into(), cx))
        },
        open: open_main,
    },
    Scene {
        name: "dm-chat",
        about: "The 私聊 with Forge as a chat",
        start: Start::SignedIn,
        setup: |s, cx| {
            s.nav
                .update(cx, |nav, cx| nav.open_group_chat("d-coder".into(), cx))
        },
        open: open_main,
    },
    Scene {
        name: "new-topic-from",
        about: "开个话题: the sheet from a 群聊 message, quoting it, the title its first line",
        start: Start::SignedIn,
        setup: |s, cx| {
            s.nav
                .update(cx, |nav, cx| nav.open_group_chat("g-dev".into(), cx))
        },
        open: crate::views::nav::selftest::scenes::new_topic_from,
    },
    Scene {
        name: "todo",
        about: "首页 opened from the sidebar on the v1 sample team: what waits on me, with no 话题 open",
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
        about: "The login window",
        start: Start::SignedOut,
        setup: |_, _| {},
        // v12/fx-overlays: the login window, not the main window.
        open: |services, content, cx| open_login(services, content, "", false, cx),
    },
    Scene {
        name: "login-code",
        about: "The login view with half an invite code typed in the code slots",
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
        about: "The login view after a wrong invite code: kept in the slots, selected, error border",
        start: Start::SignedOut,
        setup: |_, _| {},
        open: |services, content, cx| open_login(services, content, "!0000-0000", false, cx),
    },
    Scene {
        name: "login-signing-in",
        about: "The login view while a complete code is checked: spinner and 正在验证…",
        start: Start::SignedOut,
        setup: |_, _| {},
        open: |services, content, cx| open_login(services, content, "^K7QM2X9A", false, cx),
    },
    Scene {
        name: "login-unreachable",
        about: "The login view after a code was sent to a server that cannot be reached: the field error under the server",
        start: Start::SignedOut,
        setup: |_, _| {},
        open: |services, content, cx| open_login(services, content, "@K7QM2X9A", false, cx),
    },
    Scene {
        name: "login-email",
        about: "The login view on 用邮箱登录 with an address typed",
        start: Start::SignedOut,
        setup: |_, _| {},
        open: |services, content, cx| open_login(services, content, "=joye@example.com", false, cx),
    },
    Scene {
        name: "login-email-code",
        about: "The login view on 输入验证码 with three of six digits typed",
        start: Start::SignedOut,
        setup: |_, _| {},
        open: |services, content, cx| {
            open_login(services, content, "#joye@example.com 042", false, cx)
        },
    },
    Scene {
        name: "login-email-error",
        about: "The login view after a wrong email code: kept in the slots, selected, error under them",
        start: Start::SignedOut,
        setup: |_, _| {},
        open: |services, content, cx| {
            open_login(services, content, "#joye@example.com 000000", false, cx)
        },
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
        name: "settings-edit-bot",
        about: "Settings: the edit sheet of a bot over 成员, its name, 岗位 and runner filled in",
        start: Start::SignedIn,
        setup: |_, _| {},
        open: settings_scenes::open_edit_bot_dialog,
    },
    Scene {
        name: "settings-add-machine",
        about: "Settings: the add-machine dialog over 机器",
        start: Start::SignedIn,
        setup: |_, _| {},
        open: settings_scenes::open_machine_dialog,
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
        about: "Settings: 通用 after 检查更新: 已是最新版本 (v12/fx-overlays)",
        start: Start::SignedIn,
        setup: |_, _| {},
        open: settings_scenes::open_about,
    },
    Scene {
        name: "settings-ai-service",
        about: "Settings: 通用 › AI 服务 (owner) after 测试连接 with the saved DeepSeek key",
        start: Start::SignedIn,
        setup: |_, _| {},
        open: settings_scenes::open_ai_service,
    },
    Scene {
        name: "settings-ai-service-failed",
        about: "Settings: 通用 › AI 服务 with OpenAI 兼容 and a refused key: 连接失败",
        start: Start::SignedIn,
        setup: |_, _| {},
        open: settings_scenes::open_ai_service_failed,
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
        open: crate::updater::preview::open_up_to_date_end,
    },
    Scene {
        name: "update",
        about: "Settings › 通用: a new version found, 更新到 … in the version row (v12/fx-overlays)",
        start: Start::SignedIn,
        setup: open_first_topic,
        open: crate::updater::preview::open_available,
    },
    Scene {
        name: "update-dl",
        about: "Settings › 通用: the download's progress in the version row (v12/fx-overlays)",
        start: Start::SignedIn,
        setup: open_first_topic,
        open: crate::updater::preview::open_downloading,
    },
    Scene {
        name: "update-manual",
        about: "Settings › 通用: the app cannot be replaced, 知道了 in the version row (v12/fx-overlays)",
        start: Start::SignedIn,
        setup: open_first_topic,
        open: crate::updater::preview::open_manual,
    },
    Scene {
        name: "update-card",
        about: "Sidebar: the update card, 新版本 with 更新 and 改了什么",
        start: Start::SignedIn,
        setup: |_, _| {},
        open: crate::updater::preview::open_card_available,
    },
    Scene {
        name: "update-card-dl",
        about: "Sidebar: the update card while it downloads",
        start: Start::SignedIn,
        setup: |_, _| {},
        open: crate::updater::preview::open_card_downloading,
    },
    Scene {
        name: "update-card-failed",
        about: "Sidebar: the update card after a failed download",
        start: Start::SignedIn,
        setup: |_, _| {},
        open: crate::updater::preview::open_card_failed,
    },
    Scene {
        name: "update-card-manual",
        about: "Sidebar: the update card when the app must be replaced by hand",
        start: Start::SignedIn,
        setup: |_, _| {},
        open: crate::updater::preview::open_card_manual,
    },
    Scene {
        name: "update-notes",
        about: "「{version} 更新内容」 over the main window, with 更新到 …",
        start: Start::SignedIn,
        setup: |_, _| {},
        open: crate::updater::preview::open_notes_sheet,
    },
    Scene {
        name: "whats-new",
        about: "「已更新到 {version}」 after an update",
        start: Start::SignedIn,
        setup: |_, _| {},
        open: crate::updater::preview::open_whats_new,
    },
    Scene {
        name: "changelog",
        about: "Settings › 通用 › 更新记录",
        start: Start::SignedIn,
        setup: |_, _| {},
        open: crate::updater::preview::open_changelog,
    },
    Scene {
        name: "files",
        about: "A 话题 with an image, files (downloading, uploading), a 产出 card and the side panel lists",
        start: Start::SignedIn,
        setup: |_, _| {},
        open: crate::views::files::preview::open,
    },
    Scene {
        name: "files-menu",
        about: "The files scene with the image's context menu: 打开, 在访达中显示, 快速查看, 共享…, 存储为…",
        start: Start::SignedIn,
        setup: |_, _| {},
        open: crate::views::files::preview::open_menu,
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
        name: "new-group-keyboard",
        about: "The new-群 sheet from the keyboard: the member list focused, a row highlighted and checked",
        start: Start::SignedIn,
        setup: open_first_topic,
        open: crate::views::nav::selftest::scenes::new_group_keyboard,
    },
    Scene {
        name: "new-group-error",
        about: "The new-群 sheet with a name over 64 characters: field and label destructive, the reason under the list",
        start: Start::SignedIn,
        setup: open_first_topic,
        open: crate::views::nav::selftest::scenes::new_group_error,
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
        about: "The menu of a 群 row in the sidebar (right-click): 静音, 提醒 ▸, 重命名群…, 添加成员, 离开群",
        start: Start::SignedIn,
        setup: open_first_topic,
        open: crate::views::nav::selftest::scenes::group_menu,
    },
    Scene {
        name: "account-menu",
        about: "The account menu of the sidebar footer: two accounts with their role, the shown one checked, 添加账号…, 设置…, 退出这个账号…",
        start: Start::SignedIn,
        setup: open_first_topic,
        open: crate::views::nav::selftest::scenes::account_menu,
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
        name: "topic-find",
        about: "⌘F in 开发 / 断线重连后消息重复: the find bar with 重连 typed, every match marked, the current one stronger",
        start: Start::V2,
        setup: |s, cx| v2_open_topic(s, crate::fake::v2::DEV, crate::fake::v2::T_RECONNECT, cx),
        open: crate::views::timeline::find::open_scene,
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
        // v12/fx-overlays: 实时查看 on the run's steps with the 停止 alert.
        open: crate::views::live::scenes::open_stop,
    },
    Scene {
        name: "stop-stopping",
        about: "The stop scene after 停止 was pressed: both show 正在停止… until the run ends",
        start: Start::SignedIn,
        setup: |_, _| {},
        // v12/fx-overlays: 实时查看 with 正在停止….
        open: crate::views::live::scenes::open_stopping,
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
        about: "The composer while reconnecting: the offline Alert above it, a quote chip, a markdown draft and the queued line",
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
        about: "The diagram viewer as it opens, at fit",
        start: Start::SignedIn,
        setup: |_, _| {},
        open: |s, c, cx| crate::diagrams::preview::open(s, c, DiagramScene::Viewer, cx),
    },
    // --- end v11/inc-diagrams ---
];

/// Every scene: the shared ones, then each v2 feature's slice, in order.
pub fn all() -> Vec<&'static Scene> {
    let slices: &[&'static [Scene]] = &[
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
        // --- inc-cards21 ---
        crate::views::cards::v21::scenes::SCENES,
        // --- end inc-cards21 ---
        // --- inc-host track ---
        HOST_SCENES,
        // --- end inc-host track ---
        // --- inc-refs ---
        crate::views::refs::scenes::SCENES,
        // --- end inc-refs ---
        // --- v12/kit ---
        crate::ui::native::scene::SCENES,
        // --- end v12/kit ---
        // --- v12/behavior ---
        crate::views::rail::scenes::BEHAVIOR_SCENES,
        // --- end v12/behavior ---
        // --- v12/shell ---
        crate::views::rail::scenes::SHELL_SCENES,
        // --- end v12/shell ---
        // --- v12/settings track ---
        crate::views::settings::selftest::SCENES,
        // --- end v12/settings track ---
        // --- v14 state scenes: one line per track, in track order ---
        super::states::SCENES,
        // --- end v14 state scenes ---
    ];
    slices.iter().copied().flatten().collect()
}

// --- provider track ---
/// Scenes on the real chat-core provider (seeded cache, no server): the v2 topic parts it
/// reads from the cache.
const CORE_SCENES: &[Scene] = &[
    Scene {
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
    },
    Scene {
        name: "core-desk",
        about: "首页 on the real provider and a seeded cache: the review ask that waits on me and Forge at work",
        start: Start::Core,
        setup: |s, cx| s.nav.update(cx, |nav, cx| nav.open_desk(cx)),
        open: open_main,
    },
    Scene {
        name: "core-group",
        about: "群 page on the real provider and a seeded cache: 开发 with its two 话题",
        start: Start::Core,
        setup: |s, cx| {
            s.nav.update(cx, |nav, cx| {
                nav.open_group(crate::provider::seeded::GROUP.into(), cx)
            })
        },
        open: open_main,
    },
    Scene {
        name: "core-members",
        about: "成员 on the real provider and a seeded cache: Forge's detail",
        start: Start::Core,
        setup: |s, cx| {
            s.nav.update(cx, |nav, cx| {
                nav.open_members(Some(crate::provider::seeded::FORGE.into()), cx)
            })
        },
        open: open_main,
    },
    Scene {
        name: "core-live",
        about: "实时查看 on the real provider and a seeded cache: Forge writing the weekly report",
        start: Start::Core,
        setup: |s, cx| {
            use crate::provider::seeded as sd;
            v2_open_topic(s, sd::GROUP, sd::TOPIC_WEEKLY, cx);
            s.nav.update(cx, |nav, cx| {
                nav.set_right_pane(crate::app::RightPane::Live(sd::RUN_WEEKLY.into()), cx)
            });
        },
        open: open_main,
    },
    Scene {
        name: "core-side-chat",
        about: "单独问 on the real provider and a seeded cache: an empty side chat with Forge",
        start: Start::Core,
        setup: |s, cx| {
            use crate::provider::seeded as sd;
            v2_open_topic(s, sd::GROUP, sd::TOPIC, cx);
            s.nav.update(cx, |nav, cx| {
                nav.set_right_pane(crate::app::RightPane::SideChat(sd::FORGE.into()), cx)
            });
        },
        open: open_main,
    },
    Scene {
        name: "core-artifact",
        about: "产出 page on the real provider and a seeded cache: 重连测试报告 v2 from the disk",
        start: Start::Core,
        setup: |s, cx| {
            s.nav.update(cx, |nav, cx| {
                nav.open_artifact(crate::provider::seeded::ARTIFACT.into(), None, cx)
            })
        },
        open: open_main,
    },
];

/// The name the provider gives the seeded member Forge (a worst-case name with `--worst-case`).
fn forge_name(s: &Services) -> String {
    s.provider
        .member(crate::provider::seeded::FORGE)
        .map_or_else(|| "Forge".into(), |m| m.name)
}

/// The checks of the seeded-cache scenes: each page drew what the real provider read.
fn core_checks(name: &str) -> Option<Checks> {
    use crate::selftest::drawn::{check, ids};
    fn one<W: AsRef<str>>(kind: &'static str, want: &[W]) -> Vec<Result<String, String>> {
        let got = ids(kind);
        want.iter()
            .map(|w| {
                let w = w.as_ref();
                check(
                    got.iter().any(|g| g.contains(w)),
                    format!("{kind} drew {w:?} ({got:?})"),
                )
            })
            .collect()
    }
    Some(match name {
        "core-desk" => |_, _| one("desk", &["asks=1", "runs=1"]),
        "core-group" => |_, _| {
            one(
                "group-page",
                &[crate::provider::seeded::GROUP, "active=2", "on_me=1"],
            )
        },
        // The member name comes from the provider: `--worst-case` renames Forge.
        "core-members" => |s, _| {
            one(
                "members-page",
                &["entries=3".into(), format!("detail={}", forge_name(s))],
            )
        },
        "core-live" => |s, _| {
            one(
                "live",
                &[forge_name(s), crate::provider::seeded::RUN_WEEKLY.into()],
            )
        },
        "core-side-chat" => |s, _| one("side-chat", &[format!("{} messages=0", forge_name(s))]),
        // The name comes from the provider: `--worst-case` renames every 产出 (C-07).
        "core-artifact" => |s, _| {
            let title = s
                .provider
                .artifact(crate::provider::seeded::ARTIFACT)
                .map(|d| d.summary.title)
                .unwrap_or_default();
            one("artifact-page", &[format!("{title} v2")])
        },
        // The draft's text runs over one line: its textarea shows the
        // wrapped lines, not only the first (review 4 K-12).
        "cards-v2-draft" => |_, _| {
            let got = ids("textarea-lines");
            let most = got
                .iter()
                .filter_map(|g| g.rsplit(' ').next()?.parse::<usize>().ok())
                .max()
                .unwrap_or(0);
            vec![check(
                most > 1,
                format!("a textarea showed {most} lines, more than 1 ({got:?})"),
            )]
        },
        _ => return None,
    })
}
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
    Scene {
        name: "artifact-beside-topic",
        about: "a 产出 clicked in a message opens in the right pane; the 话题 stays",
        start: Start::V2,
        setup: |s, cx| {
            use crate::fake::v2::{ART_RECONNECT, DEV, T_RECONNECT};
            s.nav.update(cx, |nav, cx| {
                nav.open_topic(DEV.into(), T_RECONNECT.into(), cx);
                nav.show_artifact(ART_RECONNECT.into(), Some(2), cx);
            })
        },
        open: open_main,
    },
    Scene {
        name: "artifact-page-back",
        about: "the 产出 page opened from a 话题: ‹ 回到话题 before the title",
        start: Start::V2,
        setup: |s, cx| {
            use crate::fake::v2::{ART_RECONNECT, DEV, T_RECONNECT};
            s.nav.update(cx, |nav, cx| {
                nav.open_topic(DEV.into(), T_RECONNECT.into(), cx);
                nav.open_artifact(ART_RECONNECT.into(), Some(2), cx);
            })
        },
        open: open_main,
    },
];

// --- inc-host track: the 产出 page host ---

use crate::views::artifacts::host_preview::{self, Mode as HostMode};

const HOST_SCENES: &[Scene] = &[
    Scene {
        name: "artifact_live_data",
        about: "产出 page inline in its 话题: Ledger pushes the B 站 numbers and the page redraws",
        start: Start::V2,
        setup: |_, _| {},
        open: |s, c, cx| host_preview::open(s, c, HostMode::LiveData, cx),
    },
    Scene {
        name: "artifact_reloaded_v2",
        about: "产出 page inline: a new version arrives and the page reloads in place (已更新到 v4)",
        start: Start::V2,
        setup: |_, _| {},
        open: |s, c, cx| host_preview::open(s, c, HostMode::Reloaded, cx),
    },
    Scene {
        name: "artifact_ask_pending",
        about: "产出 page inline: a real click on 数据有问题 sends one ask; the footer waits for Ledger",
        start: Start::V2,
        setup: |_, _| {},
        open: |s, c, cx| host_preview::open(s, c, HostMode::AskPending, cx),
    },
    Scene {
        name: "artifact-embed-states",
        about: "产出 embedded in a card: a page that did not open (页面没有打开 · 重试) and a 产出 that is not a web page",
        start: Start::V2,
        setup: |_, _| {},
        open: crate::views::artifacts::state_scenes::open_embed_states,
    },
    Scene {
        name: "artifact-ref-chip",
        about: "就这里提问 on a page element: the chip goes into the 话题 composer; the chip is the confirmation, so no toast shows (NATIVE.md §7.4)",
        start: Start::V2Refs,
        setup: crate::views::artifacts::state_scenes::ref_toast_setup,
        open: crate::views::artifacts::state_scenes::open_ref_toast,
    },
];

fn host_check(name: &str) -> bool {
    matches!(
        name,
        "artifact_live_data" | "artifact_reloaded_v2" | "artifact_ask_pending"
    )
}

// --- end inc-host track ---

// --- web-embed: scenes that wait for native content and check their capture ---

use crate::views::artifacts::embed_preview::{self, Mode as EmbedMode};

/// For a scene whose window shows native content (a web page) that GPUI's frames do not
/// cover: true once it is ready to capture.
pub fn ready_check(name: &str) -> Option<fn(&App) -> bool> {
    // --- inc-host track ---
    if host_check(name) {
        return Some(host_preview::ready);
    }
    if name == "artifact-ref-chip" {
        return Some(crate::views::artifacts::state_scenes::ref_toast_ready);
    }
    // --- end inc-host track ---
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
    // --- inc-refs ---
    if let Some(check) = crate::views::refs::scenes::ready(name) {
        return Some(check);
    }
    // --- end inc-refs ---
    match name {
        "artifact-v2-leave" => Some(crate::views::artifact_page::scenes::left),
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
/// A scene's check of its own capture (the PNG and the app).
pub type Verify = fn(&std::path::Path, &mut App) -> Result<(), String>;

pub fn verify_check(name: &str) -> Option<Verify> {
    // --- inc-host track ---
    if host_check(name) {
        return Some(host_preview::verify);
    }
    if name == "artifact-ref-chip" {
        return Some(crate::views::artifacts::state_scenes::ref_toast_verify);
    }
    // --- end inc-host track ---
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
        "diagram-viewer" | "state-focus-diagram-node" => {
            Some(|cx| crate::diagrams::preview::ready(DiagramScene::Viewer, cx))
        }
        // --- t5-media ---
        "diagram-loading" => Some(|cx| crate::diagrams::preview::ready(DiagramScene::Loading, cx)),
        // --- end t5-media ---
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
        // --- t5-media ---
        "media-inline-docs" => Some(|cx| {
            media_preview::ready(
                MediaSample::from_name("media-inline-docs").expect("sample"),
                cx,
            )
        }),
        "media-inline-data" => Some(|cx| {
            media_preview::ready(
                MediaSample::from_name("media-inline-data").expect("sample"),
                cx,
            )
        }),
        "media-inline-av" => Some(|cx| {
            media_preview::ready(
                MediaSample::from_name("media-inline-av").expect("sample"),
                cx,
            )
        }),
        "media-viewer-loading" => Some(|cx| {
            media_preview::ready(
                MediaSample::from_name("media-viewer-loading").expect("sample"),
                cx,
            )
        }),
        "media-viewer-failed" => Some(|cx| {
            media_preview::ready(
                MediaSample::from_name("media-viewer-failed").expect("sample"),
                cx,
            )
        }),
        // --- end t5-media ---
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

/// The checks of a scene, after its capture: each line says what held (`Ok`) or what did
/// not (`Err`, the shot fails). The report lists them under `checks`.
pub type Checks = fn(&Services, &mut App) -> Vec<Result<String, String>>;

/// The checks of scene `name`, if it has any.
pub fn checks(name: &str) -> Option<Checks> {
    core_checks(name)
        .or_else(|| crate::views::topic_page::scenes::checks(name))
        .or_else(|| crate::views::cards::v21::scenes::checks(name))
        .or_else(|| crate::views::artifact_page::scenes::checks(name))
        .or_else(|| {
            (name == "artifact-embed-states")
                .then_some(crate::views::artifacts::state_scenes::embed_states_checks as Checks)
        })
}

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

fn open_first_topic_sidebar_hidden(services: &Services, cx: &mut App) {
    open_first_topic(services, cx);
    services.nav.update(cx, |nav, cx| nav.toggle_sidebar(cx));
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
    // The 待办 list became 首页: the place opens with no 话题, so the scene
    // shows the page, not the first entry's 话题 (that is topic-dm).
    services.nav.update(cx, |nav, cx| {
        nav.open_todo(cx);
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

/// The login window (signed out), with the form filled for a scene.
/// v12/fx-overlays: the small login window at its own size (the requested
/// size is ignored).
pub fn open_login(
    services: Services,
    _content: Size<Pixels>,
    code: &'static str,
    server_open: bool,
    cx: &mut App,
) -> Result<AnyWindowHandle, String> {
    let options = crate::views::login::window_options(false, cx);
    gpui_kit::open_window(options, cx, move |window, cx| {
        let root =
            cx.new(|cx| crate::views::login::LoginWindow::new_for_scene(services, window, cx));
        let login = root.read(cx).login().clone();
        // A leading `!` submits the code, for the error scene; `^` shows it
        // while it is checked; `~` sets a custom server instead; `@` submits
        // it against a server that cannot be reached.
        // With `--worst-case` every login scene runs on a 120-character
        // server URL, and its code becomes a pasted token, a legacy code or
        // a long wrong code (C2-28).
        let worst = super::worst_case();
        let code: &str = if worst { worst_login_code(code) } else { code };
        login.update(cx, |l, cx| {
            if worst && !code.starts_with('~') {
                l.preview_server(WORST_SERVER, window, cx);
            }
            // `=` shows 用邮箱登录 with that address; `#address digits` the
            // code step with those digits typed.
            if let Some(email) = code.strip_prefix('=') {
                l.preview_email(email, None, window, cx)
            } else if let Some(rest) = code.strip_prefix('#') {
                let (email, digits) = rest.split_once(' ').unwrap_or((rest, ""));
                l.preview_email(email, Some(digits), window, cx)
            } else if let Some(code) = code.strip_prefix('!') {
                l.preview_submit(code, window, cx)
            } else if let Some(code) = code.strip_prefix('^') {
                l.preview_signing_in(code, window, cx)
            } else if let Some(server) = code.strip_prefix('~') {
                l.preview_server(server, window, cx)
            } else if let Some(code) = code.strip_prefix('@') {
                let server = if worst {
                    WORST_UNREACHABLE_SERVER
                } else {
                    UNREACHABLE_SERVER
                };
                l.preview_server(server, window, cx);
                l.preview_submit(code, window, cx)
            } else {
                l.preview(code, server_open, window, cx)
            }
        });
        root
    })
    .map(|(handle, _)| handle)
    .map_err(|e| e.to_string())
}

/// A server the fake provider cannot reach (`.invalid`, RFC 2606).
const UNREACHABLE_SERVER: &str = "https://team.northwind.invalid";

/// A long server that the fake provider cannot reach.
const WORST_UNREACHABLE_SERVER: &str = "https://team-chat.internal.northwind-industries-holdings.invalid/workspaces/product-design-and-frontend/region-ap-southeast-2";

/// A 120-character custom server URL.
const WORST_SERVER: &str = "https://team-chat.internal.northwind-industries-holdings.example.com/workspaces/product-design-and-frontend/region-ap-southeast-2";

/// The worst-case code of a login scene.
fn worst_login_code(code: &'static str) -> &'static str {
    match code {
        // Half a code becomes a pasted access token (a locked line).
        "K7QM2X" => "tc_live_9fK2mQ7xL0pR4sT8vW1yZ3bD6gH5jN2cE7aU0iO9",
        // A checked code becomes a 12-character legacy code.
        "^K7QM2X9A" => "^ABCDEFGHJKLM",
        // A wrong legacy code (a locked line) instead of a short one.
        "!0000-0000" => "!000000000000",
        // A long sign-in email on the email steps.
        "=joye@example.com" => {
            "=huang.deshiou.product-design-and-frontend@northwind-industries-holdings.example.com"
        }
        "#joye@example.com 042" => {
            "#huang.deshiou.product-design-and-frontend@northwind-industries-holdings.example.com 042"
        }
        // A long code against a long host that cannot be reached.
        "@K7QM2X9A" => "@K7QM2X9ABC3D",
        "~https://team.joye.co" => {
            "~https://team-chat.internal.northwind-industries-holdings.example.com/workspaces/product-design-and-frontend/region-ap-southeast-2"
        }
        other => other,
    }
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
