//! Screenshot scenes of 记得的事 (the 成员 page) and 团队约定 (设置), in every state.

use crate::{
    fake::{memory::MemoryShape, v2 as d},
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

pub const SCENES: &[Scene] = &[
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
