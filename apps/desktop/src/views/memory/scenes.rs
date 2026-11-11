//! Screenshot scenes of 记得的事 (the 成员 page) and 团队约定 (设置), in every state.

use crate::{
    fake::{
        memory::{MemoryShape, MemoryTeam},
        v2 as d,
    },
    selftest::scenes::{Scene, Start, open_main},
    views::settings::selftest as settings_scenes,
};

use crate::views::members_page::{SceneScroll, set_scene_scroll};

/// Forge's page, scrolled to its 记得的事 card.
fn forge_memory(s: &crate::app::Services, cx: &mut gpui::App) {
    set_scene_scroll(SceneScroll::Memory);
    s.nav
        .update(cx, |nav, cx| nav.open_members(Some(d::FORGE.into()), cx))
}

/// Forge's page, scrolled to its end: the last kinds and 已停用.
fn forge_memory_end(s: &crate::app::Services, cx: &mut gpui::App) {
    set_scene_scroll(SceneScroll::End);
    s.nav
        .update(cx, |nav, cx| nav.open_members(Some(d::FORGE.into()), cx))
}

/// 开发's 群 page, scrolled to its 群里的约定 card.
fn dev_group_memory(s: &crate::app::Services, cx: &mut gpui::App) {
    crate::views::group_page::scroll_to_memory_for_scene();
    s.nav
        .update(cx, |nav, cx| nav.open_group(d::DEV.into(), cx))
}

pub const SCENES: &[Scene] = &[
    Scene {
        name: "group-memory",
        about: "群: 开发's 群里的约定 under its 话题: today's 记忆更新 with 撤回, 约定, 事实 and 已停用",
        start: Start::V2Memory(MemoryShape::Normal),
        setup: dev_group_memory,
        open: open_main,
    },
    Scene {
        name: "group-memory-empty",
        about: "群: 群里的约定 before anything is kept (compact empty with 记一条)",
        start: Start::V2Memory(MemoryShape::Empty),
        setup: dev_group_memory,
        open: open_main,
    },
    Scene {
        name: "group-memory-solo",
        about: "群: 群里的约定 when one person uses the server: shown because an AI member kept something, no 记一条",
        start: Start::V2MemoryTeam(MemoryShape::Normal, MemoryTeam::SOLO),
        setup: dev_group_memory,
        open: open_main,
    },
    Scene {
        name: "group-memory-long",
        about: "群: 群里的约定 at its longest (break-ui)",
        start: Start::V2Memory(MemoryShape::Long),
        setup: dev_group_memory,
        open: open_main,
    },
    Scene {
        name: "settings-mine",
        about: "Settings: 我的偏好 with today's 记忆更新, 偏好, 事实 and one 明天生效",
        start: Start::V2MemoryTeam(MemoryShape::Normal, MemoryTeam::TEAM),
        setup: |_, _| {},
        open: settings_scenes::open_mine,
    },
    Scene {
        name: "settings-mine-empty",
        about: "Settings: 我的偏好 before anything is kept",
        start: Start::V2MemoryTeam(MemoryShape::Empty, MemoryTeam::TEAM),
        setup: |_, _| {},
        open: settings_scenes::open_mine,
    },
    Scene {
        name: "settings-mine-add",
        about: "Settings: 我的偏好 with the 记一条 sheet open (偏好 first)",
        start: Start::V2MemoryTeam(MemoryShape::Normal, MemoryTeam::TEAM),
        setup: |_, _| {},
        open: settings_scenes::open_mine_add,
    },
    Scene {
        name: "settings-mine-solo",
        about: "Settings: 我的约定 when one person uses the server: the team's and my own on one board",
        start: Start::V2MemoryTeam(MemoryShape::Normal, MemoryTeam::SOLO),
        setup: |_, _| {},
        open: settings_scenes::open_mine,
    },
    Scene {
        name: "settings-mine-solo-add",
        about: "Settings: 我的约定 with the 记一条 sheet open (约定, 偏好, 事实)",
        start: Start::V2MemoryTeam(MemoryShape::Normal, MemoryTeam::SOLO),
        setup: |_, _| {},
        open: settings_scenes::open_mine_add,
    },
    Scene {
        name: "settings-mine-no-ai",
        about: "Settings: 我的偏好 on a server without an AI 服务: the notice with 去设置 (owner)",
        start: Start::V2MemoryTeam(MemoryShape::Normal, MemoryTeam::NO_AI),
        setup: |_, _| {},
        open: settings_scenes::open_mine,
    },
    Scene {
        name: "members-memory-no-ai",
        about: "成员: Forge's 记得的事 on a server without an AI 服务: the notice with 去设置 (owner)",
        start: Start::V2MemoryTeam(MemoryShape::Normal, MemoryTeam::NO_AI),
        setup: forge_memory,
        open: open_main,
    },
    Scene {
        name: "members-memory",
        about: "成员: Forge's 记得的事: today's 记忆更新 with 撤回, 经验, 做法, 其他 and 已停用",
        start: Start::V2Memory(MemoryShape::Normal),
        setup: forge_memory,
        open: open_main,
    },
    Scene {
        name: "members-memory-end",
        about: "成员: Forge's 记得的事 scrolled to its end: 做法, 其他 and 已停用 (folded)",
        start: Start::V2Memory(MemoryShape::Normal),
        setup: forge_memory_end,
        open: open_main,
    },
    Scene {
        name: "members-memory-empty",
        about: "成员: 记得的事 of a member with nothing kept yet (compact empty)",
        start: Start::V2Memory(MemoryShape::Empty),
        setup: forge_memory,
        open: open_main,
    },
    Scene {
        name: "members-memory-loading",
        about: "成员: 记得的事 while it loads (skeleton rows in the card)",
        start: Start::V2Memory(MemoryShape::Loading),
        setup: forge_memory,
        open: open_main,
    },
    Scene {
        name: "members-memory-failed",
        about: "成员: 记得的事 could not be read: the Alert with 重试 in the card",
        start: Start::V2Memory(MemoryShape::Failed),
        setup: forge_memory,
        open: open_main,
    },
    Scene {
        name: "members-memory-long",
        about: "成员: 记得的事 at its longest (500-character texts, long 适用 and 依据, many items)",
        start: Start::V2Memory(MemoryShape::Long),
        setup: forge_memory,
        open: open_main,
    },
    Scene {
        name: "settings-memory",
        about: "Settings: 团队约定 with today's 记忆更新 and the conventions (owner)",
        start: Start::Memory(MemoryShape::Normal),
        setup: |_, _| {},
        open: settings_scenes::open_memory,
    },
    Scene {
        name: "settings-memory-end",
        about: "Settings: 团队约定 scrolled to its end: 事实 and 已停用",
        start: Start::Memory(MemoryShape::Normal),
        setup: |_, _| {},
        open: settings_scenes::open_memory_end,
    },
    Scene {
        name: "settings-memory-empty",
        about: "Settings: 团队约定 before anything is kept: the empty group with 记一条",
        start: Start::Memory(MemoryShape::Empty),
        setup: |_, _| {},
        open: settings_scenes::open_memory,
    },
    Scene {
        name: "settings-memory-loading",
        about: "Settings: 团队约定 while it loads (skeleton rows)",
        start: Start::Memory(MemoryShape::Loading),
        setup: |_, _| {},
        open: settings_scenes::open_memory,
    },
    Scene {
        name: "settings-memory-failed",
        about: "Settings: 团队约定 could not be read: the Alert with 重试",
        start: Start::Memory(MemoryShape::Failed),
        setup: |_, _| {},
        open: settings_scenes::open_memory,
    },
    Scene {
        name: "settings-memory-long",
        about: "Settings: 团队约定 at its longest (break-ui)",
        start: Start::Memory(MemoryShape::Long),
        setup: |_, _| {},
        open: settings_scenes::open_memory,
    },
    Scene {
        name: "settings-memory-add",
        about: "Settings: 团队约定 with the 记一条 sheet open",
        start: Start::Memory(MemoryShape::Normal),
        setup: |_, _| {},
        open: settings_scenes::open_memory_add,
    },
    Scene {
        name: "settings-memory-edit",
        about: "Settings: 团队约定 with the 编辑 sheet of a convention open",
        start: Start::Memory(MemoryShape::Normal),
        setup: |_, _| {},
        open: settings_scenes::open_memory_edit,
    },
];
