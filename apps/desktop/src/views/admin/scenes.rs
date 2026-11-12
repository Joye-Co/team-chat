//! Screenshot scenes of 团队管理 (`desktop --screenshot <dir> --scenes admin-…`), on the v1
//! sample team with the 团队管理 samples of `fake/admin.rs`. The window opens without focus.

use gpui::{AnyWindowHandle, App, Pixels, Size};

use super::{AdminPage, AdminView, usage::UsageRange};
use crate::{
    app::Services,
    selftest::scenes::{Scene, Start},
};

fn open(services: Services, page: AdminPage, cx: &mut App) -> Result<AnyWindowHandle, String> {
    open_with(services, page, |_| {}, cx)
}

/// Opens 团队管理 at `page` after `before` set the view up (a range, a selection).
fn open_with(
    services: Services,
    page: AdminPage,
    before: impl FnOnce(&mut AdminView),
    cx: &mut App,
) -> Result<AnyWindowHandle, String> {
    let (handle, view) = super::open_window(services, false, cx)?;
    handle
        .update(cx, |_, window, cx| {
            view.update(cx, |v, cx| {
                before(v);
                v.open_page(page, window, cx)
            })
        })
        .map_err(|e| e.to_string())?;
    Ok(handle)
}

fn open_home_month(
    services: Services,
    _content: Size<Pixels>,
    cx: &mut App,
) -> Result<AnyWindowHandle, String> {
    open_with(
        services,
        AdminPage::Home,
        |v| v.usage.range = UsageRange::Month,
        cx,
    )
}

fn open_usage_last_month(
    services: Services,
    _content: Size<Pixels>,
    cx: &mut App,
) -> Result<AnyWindowHandle, String> {
    open_with(
        services,
        AdminPage::Usage,
        |v| v.usage.range = UsageRange::LastMonth,
        cx,
    )
}

/// 用量 with the sidebar scrolled to its end: 团队记忆 sits above the account footer. Run with
/// `--size 1216x640` to see the list scroll in a short window.
fn open_nav_end(
    services: Services,
    _content: Size<Pixels>,
    cx: &mut App,
) -> Result<AnyWindowHandle, String> {
    let (handle, view) = super::open_window(services, false, cx)?;
    handle
        .update(cx, |_, window, cx| {
            view.update(cx, |v, cx| v.open_page(AdminPage::Usage, window, cx))
        })
        .map_err(|e| e.to_string())?;
    // After the machines came in and the list grew to its full length.
    cx.spawn(async move |cx| {
        cx.background_executor()
            .timer(std::time::Duration::from_millis(150))
            .await;
        let _ = view.update(cx, |v, cx| {
            v.nav_scroll.scroll_to_bottom();
            cx.notify();
        });
    })
    .detach();
    Ok(handle)
}

/// 改每周预算… from a budget card: 用量 with Scout's budget editor open.
fn open_budget_edit(
    services: Services,
    _content: Size<Pixels>,
    cx: &mut App,
) -> Result<AnyWindowHandle, String> {
    open_with(
        services,
        AdminPage::Usage,
        |v| v.usage.budget_for = Some("m-reviewer".into()),
        cx,
    )
}

fn open_member_person(
    services: Services,
    _content: Size<Pixels>,
    cx: &mut App,
) -> Result<AnyWindowHandle, String> {
    open_with(
        services,
        AdminPage::Members,
        |v| v.team.selected = Some("m-xiaowang".into()),
        cx,
    )
}

fn open_member_ai(
    services: Services,
    _content: Size<Pixels>,
    cx: &mut App,
) -> Result<AnyWindowHandle, String> {
    open_with(
        services,
        AdminPage::Members,
        |v| v.team.selected = Some("m-coder".into()),
        cx,
    )
}

/// 成员 with the ⋯ menu of an AI member's row open: 更换电脑…, 编辑…, 停用. `TC_CLICK=x,y`
/// moves the click while you look for the spot.
fn open_member_ai_menu(
    services: Services,
    _content: Size<Pixels>,
    cx: &mut App,
) -> Result<AnyWindowHandle, String> {
    let handle = open(services, AdminPage::Members, cx)?;
    crate::views::settings::selftest::click_when_drawn(
        handle,
        (817., 220.),
        gpui::MouseButton::Left,
        cx,
    )?;
    Ok(handle)
}

/// 机器 with this computer's three lines (用这台电脑) in the state `inputs` describe.
fn open_this_computer(
    services: Services,
    inputs: super::this_computer::Inputs,
    cx: &mut App,
) -> Result<AnyWindowHandle, String> {
    open_with(
        services,
        AdminPage::Machines,
        |v| v.fleet.this.pinned = Some(inputs),
        cx,
    )
}

fn login(kind: &str, state: &str) -> chat_core::proto::monitor::MonitorCli {
    chat_core::proto::monitor::MonitorCli {
        runtime: kind.into(),
        kind: kind.into(),
        available: true,
        version: None,
        login: state.into(),
        failed_today: 0,
        method: None,
        email: None,
        plan: None,
        usage: None,
        usage_at: None,
    }
}

/// The service is in, but this computer has neither Claude Code nor Codex.
fn open_this_computer_no_cli(
    services: Services,
    _content: Size<Pixels>,
    cx: &mut App,
) -> Result<AnyWindowHandle, String> {
    let inputs = super::this_computer::Inputs {
        installed: true,
        local: Some(vec![]),
        ..Default::default()
    };
    open_this_computer(services, inputs, cx)
}

/// Online with Claude Code, which is not signed in.
fn open_this_computer_sign_in(
    services: Services,
    _content: Size<Pixels>,
    cx: &mut App,
) -> Result<AnyWindowHandle, String> {
    let inputs = super::this_computer::Inputs {
        installed: true,
        connected: true,
        reported: vec![("claude".into(), "claude".into())],
        logins: Some(vec![login("claude", "logged_out")]),
        ..Default::default()
    };
    open_this_computer(services, inputs, cx)
}

/// All three done, right after 用这台电脑.
fn open_this_computer_ready(
    services: Services,
    _content: Size<Pixels>,
    cx: &mut App,
) -> Result<AnyWindowHandle, String> {
    let inputs = super::this_computer::Inputs {
        installed: true,
        connected: true,
        reported: vec![
            ("claude".into(), "claude".into()),
            ("codex".into(), "codex".into()),
        ],
        logins: Some(vec![
            login("claude", "logged_in"),
            login("codex", "logged_out"),
        ]),
        ..Default::default()
    };
    open_this_computer(services, inputs, cx)
}

macro_rules! opener {
    ($name:ident, $page:expr) => {
        fn $name(
            services: Services,
            _content: Size<Pixels>,
            cx: &mut App,
        ) -> Result<AnyWindowHandle, String> {
            open(services, $page, cx)
        }
    };
}

opener!(open_home, AdminPage::Home);
opener!(open_members, AdminPage::Members);
opener!(open_usage, AdminPage::Usage);
opener!(open_activity, AdminPage::Activity);
opener!(open_machines, AdminPage::Machines);
opener!(open_overview, AdminPage::Overview);
opener!(open_accounts, AdminPage::Accounts);
opener!(open_keys, AdminPage::Keys);
opener!(open_rules, AdminPage::Rules);

/// The 身份 scenes (design-roles 3.9): 成员 as each viewer, with 陈晨 (or me, alone) selected so
/// the inspector shows the 身份 line.
fn open_roles(
    services: Services,
    _content: Size<Pixels>,
    cx: &mut App,
) -> Result<AnyWindowHandle, String> {
    let alone = crate::views::settings::solo(&services);
    open_with(
        services,
        AdminPage::Members,
        |v| {
            v.team.selected = Some(if alone { "m-joye" } else { "m-chenchen" }.into());
            v.team.sort = (super::team::TeamSortKey::Level, true);
        },
        cx,
    )
}

/// 成员 as the 所有者 with 陈晨's ⋯ menu open: 设为所有者, 设为普通成员, 停用. `TC_CLICK=x,y` moves
/// the click while you look for the spot.
fn open_roles_menu(
    services: Services,
    _content: Size<Pixels>,
    cx: &mut App,
) -> Result<AnyWindowHandle, String> {
    let handle = open_with(
        services,
        AdminPage::Members,
        |v| v.team.sort = (super::team::TeamSortKey::Level, true),
        cx,
    )?;
    crate::views::settings::selftest::click_when_drawn(
        handle,
        (818., 317.),
        gpui::MouseButton::Left,
        cx,
    )?;
    Ok(handle)
}

macro_rules! scene {
    ($name:literal, $about:literal, $open:ident) => {
        Scene {
            name: $name,
            about: $about,
            start: Start::SignedIn,
            setup: |_, _| {},
            open: $open,
        }
    };
}

/// The monitor scenes of the computer's GitHub sign-in (design-github-identity.zh.md §3.7).
macro_rules! github_scene {
    ($name:literal, $about:literal, $shape:literal) => {
        Scene {
            name: $name,
            about: $about,
            start: Start::SignedIn,
            setup: |_, _| {
                crate::fake::admin::MONITOR_GITHUB
                    .store($shape, std::sync::atomic::Ordering::Relaxed)
            },
            open: open_overview,
        }
    };
}

macro_rules! roles_scene {
    ($name:literal, $about:literal, $view:ident, $open:ident) => {
        Scene {
            name: $name,
            about: $about,
            start: Start::Roles(crate::fake::RoleView::$view),
            setup: |_, _| {},
            open: $open,
        }
    };
}

pub const SCENES: &[Scene] = &[
    roles_scene!(
        "roles-owner",
        "团队管理 › 成员 as the 所有者: the 身份 column and 陈晨 (管理员) selected with the 身份 pull-down",
        Owner,
        open_roles
    ),
    roles_scene!(
        "roles-owner-menu",
        "团队管理 › 成员 as the 所有者 with 陈晨's ⋯ menu open: 设为所有者, 设为普通成员, 停用",
        Owner,
        open_roles_menu
    ),
    roles_scene!(
        "roles-admin",
        "团队管理 › 成员 as a 管理员: 陈晨 is the 所有者, read-only, and has no ⋯ menu",
        Admin,
        open_roles
    ),
    roles_scene!(
        "roles-member",
        "团队管理 as a 普通成员: only 所有者 and 管理员 open it",
        Member,
        open_roles
    ),
    roles_scene!(
        "roles-alone",
        "团队管理 › 成员 for one person alone: no 身份 column or line",
        Alone,
        open_roles
    ),
    scene!(
        "admin-home",
        "团队管理 › 总览: KPIs, 谁在用, 要注意 and the machine cards",
        open_home
    ),
    scene!(
        "admin-members",
        "团队管理 › 成员: the roster table with its tabs",
        open_members
    ),
    scene!(
        "admin-member-person",
        "团队管理 › 成员 with 小王 selected: 常找的 AI 成员 and devices",
        open_member_person
    ),
    scene!(
        "admin-member-ai",
        "团队管理 › 成员 with Forge selected: 最常找它的人",
        open_member_ai
    ),
    scene!(
        "admin-member-ai-menu",
        "团队管理 › 成员 with an AI member's ⋯ menu open: 更换电脑…, 编辑…, 停用",
        open_member_ai_menu
    ),
    scene!(
        "admin-nav-end",
        "团队管理 › 用量 with the sidebar scrolled to its end (try --size 1216x640)",
        open_nav_end
    ),
    scene!(
        "admin-budget-edit",
        "改每周预算… from a budget card: 用量 with Scout's budget editor open (Scout is paused at its budget)",
        open_budget_edit
    ),
    scene!(
        "admin-home-month",
        "团队管理 › 总览 switched to 本月",
        open_home_month
    ),
    scene!(
        "admin-usage-last-month",
        "团队管理 › 用量 switched to 上个月",
        open_usage_last_month
    ),
    scene!(
        "admin-usage",
        "团队管理 › 用量: spend against the budget, by day and by member",
        open_usage
    ),
    scene!(
        "admin-activity",
        "团队管理 › 活动记录: the events grouped by day",
        open_activity
    ),
    scene!(
        "admin-machines",
        "团队管理 › 机器: a card per computer and 添加电脑",
        open_machines
    ),
    scene!(
        "admin-this-computer-no-cli",
        "团队管理 › 机器 after 用这台电脑 on a computer without Claude Code or Codex: the install command and 重新检查",
        open_this_computer_no_cli
    ),
    scene!(
        "admin-this-computer-sign-in",
        "团队管理 › 机器 after 用这台电脑: Claude Code found but not signed in, with 登录",
        open_this_computer_sign_in
    ),
    scene!(
        "admin-this-computer-ready",
        "团队管理 › 机器 after 用这台电脑: all three lines done",
        open_this_computer_ready
    ),
    scene!(
        "admin-overview",
        "团队管理 › 机器 › 办公室 Mac mini › 概览: the monitor",
        open_overview
    ),
    github_scene!(
        "admin-overview-github-out",
        "团队管理 › 概览 of a VM computer not signed in to GitHub: 未登录 and the sudo command",
        1
    ),
    github_scene!(
        "admin-overview-github-no-gh",
        "团队管理 › 概览 of a computer without gh: 没装 gh and what to run",
        2
    ),
    scene!(
        "admin-accounts",
        "团队管理 › 订阅账号 of the Mac mini",
        open_accounts
    ),
    scene!(
        "admin-rules",
        "团队管理 › 默认规则: the note on who approves the gated actions above 其他要确认的事谁来批准",
        open_rules
    ),
    scene!(
        "admin-keys",
        "团队管理 › API 密钥 of the Mac mini",
        open_keys
    ),
];
