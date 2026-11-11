//! `desktop --selftest-settings --report <path> [--live <url> --token-file <path>]`
//!
//! Opens the settings window without a Dock icon and without focus, walks
//! every page and drives the forms through the views: save the profile,
//! invite a person, add an outside bot member and a runner-backed one, add a
//! machine, change a level, deactivate and reactivate, the last-owner rule,
//! group prefs, unfollow, and diagnostics. Writes a JSON report (never a
//! secret: only whether each one has the right shape) and exits.
//!
//! Without `--live` it uses the fake provider. With `--live` it signs in to
//! that server with the chat-core provider (owner token from the file, kept in
//! memory, cache in a temporary folder that is removed at the end). Use a
//! local `wrangler dev`: the test writes members, invites and machines.

use std::{
    path::PathBuf,
    sync::Arc,
    time::{Duration, Instant},
};

use gpui::{ActivationPolicy, AnyWindowHandle, App, AsyncApp, Entity};
use gpui_component::WindowExt as _;
use serde_json::{Value, json};

use super::{
    Page, SettingsView,
    account::AccountPage,
    machines::{MachineForm, open_add_machine},
    members::{BotForm, InviteForm, open_add_bot, open_invite},
    widgets::Status,
};
use crate::{
    app::Services,
    data::{
        AdminError, ChatProvider, ConnectionState, Credential, LoginRequest, MemberLevel,
        NotifyLevel, SessionState,
    },
    fake,
    provider::{CoreProvider, ProviderConfig, session::TokenStorage},
};

pub type Finish = fn(&PathBuf, Value, bool, &mut App);

const STEP_TIMEOUT: Duration = Duration::from_secs(20);
const POLL: Duration = Duration::from_millis(25);

pub struct Args {
    pub report: PathBuf,
    pub live: Option<(String, PathBuf)>,
}

/// Parses `--selftest-settings` arguments. `None` when not asked for.
pub fn args() -> Option<Result<Args, String>> {
    let all: Vec<String> = std::env::args().skip(1).collect();
    if !all.iter().any(|a| a == "--selftest-settings") {
        return None;
    }
    let value = |flag: &str| {
        all.iter()
            .position(|a| a == flag)
            .and_then(|i| all.get(i + 1).cloned())
    };
    let Some(report) = value("--report") else {
        return Some(Err("--selftest-settings needs --report <path>".into()));
    };
    let live = match (value("--live"), value("--token-file")) {
        (Some(url), Some(file)) => Some((url, PathBuf::from(file))),
        (None, None) => None,
        _ => return Some(Err("--live needs --token-file <path>".into())),
    };
    Some(Ok(Args {
        report: report.into(),
        live,
    }))
}

async fn wait_for<T>(
    cx: &mut AsyncApp,
    mut check: impl FnMut(&mut AsyncApp) -> Option<T>,
) -> Option<T> {
    let deadline = Instant::now() + STEP_TIMEOUT;
    loop {
        if let Some(v) = check(cx) {
            return Some(v);
        }
        if Instant::now() > deadline {
            return None;
        }
        cx.background_executor().timer(POLL).await;
    }
}

/// Waits until a form left the busy state: `Ok(())` when `done`, else the
/// failure text.
async fn settled<V: 'static>(
    form: &Entity<V>,
    cx: &mut AsyncApp,
    done: impl Fn(&V) -> bool,
    status: impl Fn(&V) -> Status,
) -> Result<(), String> {
    let r = wait_for(cx, |cx| {
        cx.update(|cx| {
            let f = form.read(cx);
            if done(f) {
                Some(Ok(()))
            } else if let Status::Failed(t) = status(f) {
                Some(Err(t.to_string()))
            } else {
                None
            }
        })
    })
    .await;
    r.unwrap_or_else(|| Err("timed out".into()))
}

/// `XXXX-XXXX` (or the earlier `XXXX-XXXX-XXXX`).
fn invite_code_shape(code: &str) -> bool {
    let parts: Vec<&str> = code.split('-').collect();
    matches!(parts.len(), 2 | 3)
        && parts
            .iter()
            .all(|p| p.len() == 4 && p.chars().all(|c| c.is_ascii_alphanumeric()))
}

pub fn run(args: Args, finish: Finish, cx: &mut App) {
    cx.set_activation_policy(ActivationPolicy::Accessory);
    super::prefs::stop_saving();
    let report = args.report.clone();
    let mut live_provider: Option<Arc<CoreProvider>> = None;
    let mut data_root = None;
    let provider: Arc<dyn ChatProvider> = match &args.live {
        None => fake::FakeProvider::new(),
        Some((url, file)) => {
            let credential = std::fs::read_to_string(file)
                .ok()
                .and_then(|t| Credential::from_input(&t));
            let Some(credential @ Credential::Token(_)) = credential else {
                finish(
                    &report,
                    json!({ "ok": false, "error": "the token file must hold an owner token" }),
                    false,
                    cx,
                );
                return;
            };
            let root = std::env::temp_dir().join(format!(
                "{}-selftest-settings-{}",
                env!("PRODUCT_SLUG"),
                std::process::id()
            ));
            let core = CoreProvider::new(ProviderConfig {
                data_root: root.clone(),
                tokens: TokenStorage::memory(),
                restore: false,
            });
            core.login(LoginRequest {
                server_url: url.clone(),
                credential,
                device_name: "selftest-settings".into(),
            });
            data_root = Some(root);
            live_provider = Some(core.clone());
            core
        }
    };
    let live = args.live.is_some();
    let services = Services::new(provider, cx);

    let timeout_report = report.clone();
    cx.spawn(async move |cx| {
        cx.background_executor()
            .timer(Duration::from_secs(180))
            .await;
        cx.update(|cx| {
            finish(
                &timeout_report,
                json!({ "ok": false, "error": "timed out" }),
                false,
                cx,
            )
        });
    })
    .detach();

    cx.spawn(async move |cx| {
        let (ok, json) = drive(services, live, cx).await;
        if let Some(core) = &live_provider {
            core.sign_out_local_for_selftest();
        }
        if let Some(root) = &data_root {
            let _ = std::fs::remove_dir_all(root);
        }
        cx.update(|cx| finish(&report, json, ok, cx));
    })
    .detach();
}

async fn drive(services: Services, live: bool, cx: &mut AsyncApp) -> (bool, Value) {
    let provider = services.provider.clone();
    let mut checks = serde_json::Map::new();
    let mut errors = serde_json::Map::new();
    let mut check = |name: &str, result: Result<(), String>| {
        checks.insert(name.into(), Value::Bool(result.is_ok()));
        if let Err(e) = result {
            errors.insert(name.into(), Value::String(e));
        }
    };

    // Signed in as an owner (and online, for a live server).
    let me = wait_for(cx, |_| match provider.session() {
        SessionState::SignedIn { me, .. }
            if !live || provider.connection() == ConnectionState::Online =>
        {
            Some(me)
        }
        _ => None,
    })
    .await;
    let Some(me) = me else {
        return (false, json!({ "ok": false, "error": "not signed in" }));
    };
    check(
        "signed_in_as_owner",
        (me.level == MemberLevel::Owner)
            .then_some(())
            .ok_or("not an owner".into()),
    );
    if live {
        // The directory and groups arrive with the welcome.
        wait_for(cx, |_| (!provider.groups().is_empty()).then_some(())).await;
    }

    let opened = cx.update(|cx| super::open_window(services.clone(), Page::Account, false, cx));
    let (handle, view): (AnyWindowHandle, Entity<SettingsView>) = match opened {
        Ok(v) => v,
        Err(e) => return (false, json!({ "ok": false, "error": e })),
    };

    // Every page can be shown.
    let mut pages_ok = true;
    for page in Page::ALL {
        let shown = cx.update(|cx| {
            view.update(cx, |v, cx| v.select(page, cx));
            view.read(cx).page() == page
        });
        cx.background_executor()
            .timer(Duration::from_millis(60))
            .await;
        pages_ok &= shown;
    }
    check(
        "every_page_shows",
        pages_ok.then_some(()).ok_or("page switch".into()),
    );

    // 账号: save the profile through the pane.
    let profile = cx.update(|cx| view.read(cx).account().clone());
    let new_role = format!("自测 {}", chrono::Local::now().format("%H%M%S"));
    let old_role = me.role.clone().unwrap_or_default();
    let _ = handle.update(cx, |_, window, cx| {
        profile.update(cx, |p, cx| {
            p.set_values(&me.name, &new_role, window, cx);
            p.save(window, cx);
        })
    });
    let saved = settled(
        &profile,
        cx,
        |p| matches!(p.status(), Status::Done(_)),
        |p| p.status().clone(),
    )
    .await;
    let role_seen = wait_for(cx, |_| match provider.session() {
        SessionState::SignedIn { me, .. } if me.role.as_deref() == Some(new_role.as_str()) => {
            Some(())
        }
        _ => None,
    })
    .await;
    check(
        "profile_saves_role",
        saved.and(role_seen.ok_or("session not updated".into())),
    );
    let _ = handle.update(cx, |_, window, cx| {
        profile.update(cx, |p, cx| {
            p.set_values(&me.name, &old_role, window, cx);
            p.save(window, cx);
        })
    });
    let _ = settled(
        &profile,
        cx,
        |p| matches!(p.status(), Status::Done(_)),
        |p| p.status().clone(),
    )
    .await;

    // 成员: invite a person.
    let stamp = chrono::Local::now().format("%H%M%S").to_string();
    let invite: Option<Entity<InviteForm>> = handle
        .update(cx, |_, window, cx| {
            let form = open_invite(services.clone(), window, cx);
            form.update(cx, |f, cx| {
                f.fill(&format!("自测邀请 {stamp}"), window, cx);
                f.submit(cx);
            });
            form
        })
        .ok();
    let invite_result = match &invite {
        Some(form) => settled(form, cx, |f| f.created().is_some(), |f| f.status().clone()).await,
        None => Err("no window".into()),
    };
    let invite_shape = invite_result.and_then(|_| {
        let form = invite.as_ref().unwrap();
        cx.update(|cx| {
            let c = form.read(cx).created().cloned().unwrap();
            let link = c.download_url.expose();
            if !invite_code_shape(c.code.expose()) {
                Err("code shape".to_string())
            } else if !link.contains("/download#code=") || !link.ends_with(c.code.expose()) {
                Err("download link".to_string())
            } else {
                Ok(())
            }
        })
    });
    check("invite_shows_code_and_link", invite_shape);
    let _ = handle.update(cx, |_, window, cx| window.close_all_dialogs(cx));

    // 成员: an outside bot member gets a credential shown once.
    let bot_name = format!("自测助手 {stamp}");
    let bot = open_bot(&handle, &services, &bot_name, None, cx);
    let bot_result = match &bot {
        Some(form) => settled(form, cx, |f| f.created().is_some(), |f| f.status().clone()).await,
        None => Err("no window".into()),
    };
    let bot_member = bot
        .as_ref()
        .and_then(|f| cx.update(|cx| f.read(cx).created().cloned()));
    check(
        "bot_member_credential_shown",
        bot_result.and_then(|_| match &bot_member {
            Some(b)
                if b.credential
                    .as_ref()
                    .is_some_and(|c| c.expose().starts_with("tc_")) =>
            {
                Ok(())
            }
            _ => Err("no credential".into()),
        }),
    );
    let _ = handle.update(cx, |_, window, cx| window.close_all_dialogs(cx));

    // 成员: a runner-backed bot member needs no credential.
    let runner = open_bot(
        &handle,
        &services,
        &format!("自测跑腿 {stamp}"),
        Some("main"),
        cx,
    );
    let runner_result = match &runner {
        Some(form) => settled(form, cx, |f| f.created().is_some(), |f| f.status().clone()).await,
        None => Err("no window".into()),
    };
    check(
        "runner_member_without_credential",
        runner_result.and_then(|_| {
            let created = cx.update(|cx| runner.as_ref().unwrap().read(cx).created().cloned());
            match created {
                Some(c) if c.credential.is_none() => Ok(()),
                _ => Err("credential shown".into()),
            }
        }),
    );
    let _ = handle.update(cx, |_, window, cx| window.close_all_dialogs(cx));

    // 成员: change the level, deactivate and reactivate the outside bot member.
    let members = cx.update(|cx| view.read(cx).members().clone());
    if let Some(b) = &bot_member {
        let id = b.member.id.clone();
        let level_of = |provider: &Arc<dyn ChatProvider>, id: &str| {
            provider.member(id).map(|m| (m.level, m.deactivated))
        };
        let i = id.clone();
        cx.update(|cx| members.update(cx, |p, cx| p.set_level(i, MemberLevel::Owner, cx)));
        let up = wait_for(cx, |_| {
            (level_of(&provider, &id) == Some((MemberLevel::Owner, false))).then_some(())
        })
        .await;
        let i = id.clone();
        cx.update(|cx| members.update(cx, |p, cx| p.set_level(i, MemberLevel::Member, cx)));
        let down = wait_for(cx, |_| {
            (level_of(&provider, &id) == Some((MemberLevel::Member, false))).then_some(())
        })
        .await;
        check(
            "level_changes",
            up.and(down).ok_or("level not changed".into()),
        );
        let i = id.clone();
        cx.update(|cx| members.update(cx, |p, cx| p.set_deactivated(i, true, cx)));
        let off = wait_for(cx, |_| {
            (level_of(&provider, &id) == Some((MemberLevel::Member, true))).then_some(())
        })
        .await;
        let i = id.clone();
        cx.update(|cx| members.update(cx, |p, cx| p.set_deactivated(i, false, cx)));
        let on = wait_for(cx, |_| {
            (level_of(&provider, &id) == Some((MemberLevel::Member, false))).then_some(())
        })
        .await;
        check(
            "deactivate_and_reactivate",
            off.and(on).ok_or("not changed".into()),
        );
        // Leave the test member deactivated, so it does not clutter the team.
        let i = id.clone();
        cx.update(|cx| members.update(cx, |p, cx| p.set_deactivated(i, true, cx)));
        wait_for(cx, |_| {
            (level_of(&provider, &id) == Some((MemberLevel::Member, true))).then_some(())
        })
        .await;
    } else {
        check("level_changes", Err("no bot member".into()));
        check("deactivate_and_reactivate", Err("no bot member".into()));
    }

    // The last active owner cannot step down.
    let owners = provider
        .members()
        .iter()
        .filter(|m| m.level == MemberLevel::Owner && !m.deactivated)
        .count();
    if owners == 1 {
        let reply = provider.set_member_level(&me.id, MemberLevel::Member);
        let r = super::widgets::answer(reply).await;
        check(
            "last_owner_kept",
            (r == Err(AdminError::LastOwner))
                .then_some(())
                .ok_or(format!("{r:?}")),
        );
    }

    // 机器: add a machine; it shows in the list with its install command.
    let machine: Option<Entity<MachineForm>> = handle
        .update(cx, |_, window, cx| {
            let form = open_add_machine(services.clone(), window, cx);
            form.update(cx, |f, cx| {
                f.fill(&format!("自测机器 {stamp}"), "selftest, mac", window, cx);
                f.submit(cx);
            });
            form
        })
        .ok();
    let machine_result = match &machine {
        Some(form) => settled(form, cx, |f| f.created().is_some(), |f| f.status().clone()).await,
        None => Err("no window".into()),
    };
    let created = machine
        .as_ref()
        .and_then(|f| cx.update(|cx| f.read(cx).created().cloned()));
    check(
        "machine_credential_and_install_command",
        machine_result.and_then(|_| match &created {
            Some(c)
                if c.install_command.expose().contains(c.credential.expose()) && {
                    let mut l = c.machine.labels.clone();
                    l.sort();
                    l == ["mac", "selftest"]
                } =>
            {
                Ok(())
            }
            _ => Err("bad machine answer".into()),
        }),
    );
    let _ = handle.update(cx, |_, window, cx| window.close_all_dialogs(cx));
    let machines = cx.update(|cx| view.read(cx).machines().clone());
    cx.update(|cx| view.update(cx, |v, cx| v.select(Page::Machines, cx)));
    let listed = match &created {
        Some(c) => {
            let id = c.machine.id.clone();
            wait_for(cx, |cx| {
                cx.update(|cx| {
                    let m = machines.read(cx);
                    if m.machines().is_some_and(|l| l.iter().any(|x| x.id == id)) {
                        Some(Ok(()))
                    } else if let Status::Failed(t) = m.status() {
                        Some(Err(t.to_string()))
                    } else {
                        if !m.status().busy() {
                            machines.update(cx, |m, cx| m.refresh(cx));
                        }
                        None
                    }
                })
            })
            .await
            .unwrap_or(Err("not listed".into()))
        }
        None => Err("no machine".into()),
    };
    check("machine_listed", listed);

    // A 群's own 提醒 (set from its context menu, `views::nav::notify`):
    // mute and level of the first group, then put them back.
    if let Some(group) = provider.groups().into_iter().next() {
        let before = provider.group_prefs(&group.id);
        let gid = group.id.clone();
        let _ = provider.set_group_prefs(&gid, Some(!before.muted), None);
        let muted = wait_for(cx, |_| {
            (provider.group_prefs(&group.id).muted == !before.muted).then_some(())
        })
        .await;
        let target = if before.notify == NotifyLevel::None {
            NotifyLevel::All
        } else {
            NotifyLevel::None
        };
        let _ = provider.set_group_prefs(&gid, None, Some(target));
        let level = wait_for(cx, |_| {
            (provider.group_prefs(&group.id).notify == target).then_some(())
        })
        .await;
        check(
            "group_prefs_change",
            muted.and(level).ok_or("prefs not changed".into()),
        );
        let _ = provider.set_group_prefs(&gid, Some(before.muted), Some(before.notify));
        let restored = wait_for(cx, |_| {
            (provider.group_prefs(&group.id) == before).then_some(())
        })
        .await;
        check(
            "group_prefs_restored",
            restored.ok_or("not restored".into()),
        );
    }
    let followed = provider.followed_topics();
    if let Some(f) = followed.first() {
        let (g, t) = (f.group_id.clone(), f.topic.id.clone());
        provider.set_topic_following(&g, &t, false);
        let gone = wait_for(cx, |_| {
            (!provider.followed_topics().iter().any(|x| x.topic.id == t)).then_some(())
        })
        .await;
        check("unfollow_topic", gone.ok_or("still followed".into()));
        // Follow it again.
        provider.set_topic_following(&g, &t, true);
    }

    // 通用: send diagnostics with a description.
    let about = cx.update(|cx| view.read(cx).general().clone());
    let _ = handle.update(cx, |_, window, cx| {
        about.update(cx, |a, cx| {
            a.set_description("设置自测：发送诊断", window, cx);
            a.send(window, cx);
        })
    });
    let sent = settled(
        &about,
        cx,
        |a| matches!(a.status(), Status::Done(_)),
        |a| a.status().clone(),
    )
    .await;
    check("diagnostics_sent", sent);

    let _ = handle.update(cx, |_, window, _| window.remove_window());
    let ok = checks.values().all(|v| v == &Value::Bool(true));
    (
        ok,
        json!({
            "ok": ok,
            "provider": if live { "chat-core" } else { "fake" },
            "checks": checks,
            "errors": errors,
        }),
    )
}

fn open_bot(
    handle: &AnyWindowHandle,
    services: &Services,
    name: &str,
    runtime: Option<&str>,
    cx: &mut AsyncApp,
) -> Option<Entity<BotForm>> {
    handle
        .update(cx, |_, window, cx| {
            let form = open_add_bot(services.clone(), window, cx);
            form.update(cx, |f, cx| {
                f.fill(name, runtime, window, cx);
                f.submit(cx);
            });
            form
        })
        .ok()
}

// --- screenshot scenes (selftest/scenes.rs) ---

/// The settings window on `page`, for a scene: it never saves this Mac's
/// settings.
fn open_page(
    services: Services,
    page: Page,
    cx: &mut App,
) -> Result<(AnyWindowHandle, Entity<SettingsView>), String> {
    super::prefs::stop_saving();
    super::open_window(services, page, false, cx)
}

macro_rules! scene_opener {
    ($name:ident, $page:expr) => {
        pub fn $name(
            services: Services,
            _content: gpui::Size<gpui::Pixels>,
            cx: &mut App,
        ) -> Result<AnyWindowHandle, String> {
            open_page(services, $page, cx).map(|(h, _)| h)
        }
    };
}

scene_opener!(open_profile, Page::Account);
scene_opener!(open_members, Page::Members);
scene_opener!(open_machines, Page::Machines);
scene_opener!(open_notifications, Page::Notifications);
scene_opener!(open_memory, Page::Memory);

/// 团队约定 scrolled to its end: the kind groups and 已停用.
pub fn open_memory_end(
    services: Services,
    _content: gpui::Size<gpui::Pixels>,
    cx: &mut App,
) -> Result<AnyWindowHandle, String> {
    let (handle, view) = open_page(services, Page::Memory, cx)?;
    after_fit(handle, cx, move |_, cx| view.read(cx).scroll_to_end());
    Ok(handle)
}

fn open_memory_sheet(
    edit: bool,
    services: Services,
    cx: &mut App,
) -> Result<AnyWindowHandle, String> {
    let (handle, view) = open_page(services, Page::Memory, cx)?;
    after_fit(handle, cx, move |window, cx| {
        let board = view.read(cx).memory().clone();
        board.update(cx, |b, cx| b.open_for_scene(edit, window, cx));
    });
    Ok(handle)
}

/// 团队约定 with the 记一条 sheet open.
pub fn open_memory_add(
    services: Services,
    _content: gpui::Size<gpui::Pixels>,
    cx: &mut App,
) -> Result<AnyWindowHandle, String> {
    open_memory_sheet(false, services, cx)
}

/// 团队约定 with the 编辑 sheet of the newest convention open.
pub fn open_memory_edit(
    services: Services,
    _content: gpui::Size<gpui::Pixels>,
    cx: &mut App,
) -> Result<AnyWindowHandle, String> {
    open_memory_sheet(true, services, cx)
}
/// 通用 after 检查更新 (the version row says 已是最新版本), so it differs
/// from `settings-general`.
pub fn open_about(
    services: Services,
    content: gpui::Size<gpui::Pixels>,
    cx: &mut App,
) -> Result<AnyWindowHandle, String> {
    crate::updater::preview::open_up_to_date(services, content, cx)
}

/// 快捷键: the menus' key bindings must exist, as in the app.
pub fn open_shortcuts(
    services: Services,
    _content: gpui::Size<gpui::Pixels>,
    cx: &mut App,
) -> Result<AnyWindowHandle, String> {
    static BOUND: std::sync::Once = std::sync::Once::new();
    let mut first = false;
    BOUND.call_once(|| first = true);
    if first {
        crate::app::install_menus_and_actions(services.clone(), cx);
    }
    open_page(services, Page::Shortcuts, cx).map(|(h, _)| h)
}

/// Runs `f` on the window once it has taken its pane's height, as a sheet
/// opened by a click finds it.
fn after_fit(
    handle: AnyWindowHandle,
    cx: &mut App,
    f: impl FnOnce(&mut gpui::Window, &mut App) + 'static,
) {
    cx.spawn(async move |cx| {
        cx.background_executor()
            .timer(Duration::from_millis(60))
            .await;
        let _ = handle.update(cx, |_, window, cx| f(window, cx));
    })
    .detach();
}

/// A pane scrolled to its end after its first layout.
pub(crate) fn open_end(
    services: Services,
    page: Page,
    cx: &mut App,
) -> Result<AnyWindowHandle, String> {
    let (handle, view) = open_page(services, page, cx)?;
    handle
        .update(cx, |_, window, _| {
            window.on_next_frame(move |_, cx| view.read(cx).scroll_to_end());
        })
        .map_err(|e| e.to_string())?;
    Ok(handle)
}

/// 成员 scrolled to its end: 默认规则.
pub fn open_members_end(
    services: Services,
    _content: gpui::Size<gpui::Pixels>,
    cx: &mut App,
) -> Result<AnyWindowHandle, String> {
    open_end(services, Page::Members, cx)
}

/// 通知 with its lower groups at work: 每日摘要 and 勿扰 on (the pane fits
/// the window, so "the end" is these groups in use, not a scroll).
pub fn open_notifications_end(
    services: Services,
    _content: gpui::Size<gpui::Pixels>,
    cx: &mut App,
) -> Result<AnyWindowHandle, String> {
    let (handle, view) = open_page(services, Page::Notifications, cx)?;
    after_fit(handle, cx, move |_, cx| {
        let page = view.read(cx).notifications().clone();
        page.update(cx, |p, cx| {
            p.change(
                |p| {
                    p.digest_at = Some(crate::data::NotifyPreferences::DIGEST_DEFAULT);
                    p.quiet_enabled = true;
                },
                cx,
            )
        });
        // The pane is taller than the window: show its end (勿扰).
        view.read(cx).scroll_to_end();
    });
    Ok(handle)
}

/// 通用 › AI 服务 after 测试连接 with the saved DeepSeek key: 连接正常.
pub fn open_ai_service(
    services: Services,
    _content: gpui::Size<gpui::Pixels>,
    cx: &mut App,
) -> Result<AnyWindowHandle, String> {
    let (handle, view) = open_page(services, Page::General, cx)?;
    after_fit(handle, cx, move |_, cx| {
        let general = view.read(cx).general().clone();
        general.update(cx, |g, cx| g.show_ai_here(cx));
        let ai = general.read(cx).ai_service().clone();
        ai.update(cx, |p, cx| p.test(cx));
    });
    Ok(handle)
}

/// 通用 › AI 服务 switched to OpenAI 兼容 with a key the provider refuses: the
/// 连接失败 Alert with the provider's words.
pub fn open_ai_service_failed(
    services: Services,
    _content: gpui::Size<gpui::Pixels>,
    cx: &mut App,
) -> Result<AnyWindowHandle, String> {
    let (handle, view) = open_page(services, Page::General, cx)?;
    after_fit(handle, cx, move |window, cx| {
        let general = view.read(cx).general().clone();
        general.update(cx, |g, cx| g.show_ai_here(cx));
        let ai = general.read(cx).ai_service().clone();
        ai.update(cx, |p, cx| {
            p.pick(crate::data::AiProviderKind::OpenaiCompatible, window, cx);
            p.fill_for_test(
                "qwen-turbo",
                "https://dashscope.aliyuncs.com/compatible-mode/v1",
                "sk-bad-0000-test",
                window,
                cx,
            );
            p.test(cx);
        });
        // The Alert is under the fold: show the pane's end.
        view.read(cx).scroll_to_end();
    });
    Ok(handle)
}

/// The 编辑资料 sheet over 账号.
pub fn open_edit_profile(
    services: Services,
    _content: gpui::Size<gpui::Pixels>,
    cx: &mut App,
) -> Result<AnyWindowHandle, String> {
    let (handle, view) = open_page(services, Page::Account, cx)?;
    after_fit(handle, cx, move |window, cx| {
        let account = view.read(cx).account().clone();
        AccountPage::open_edit(account, window, cx);
    });
    Ok(handle)
}

/// The 设置登录邮箱 sheet, empty.
pub fn open_email(
    services: Services,
    _content: gpui::Size<gpui::Pixels>,
    cx: &mut App,
) -> Result<AnyWindowHandle, String> {
    let (handle, view) = open_page(services, Page::Account, cx)?;
    after_fit(handle, cx, move |window, cx| {
        let account = view.read(cx).account().clone();
        AccountPage::open_email_sheet(account, window, cx);
    });
    Ok(handle)
}

/// The 设置登录邮箱 sheet while the code is being sent: the spinner.
pub fn open_email_sending(
    services: Services,
    _content: gpui::Size<gpui::Pixels>,
    cx: &mut App,
) -> Result<AnyWindowHandle, String> {
    let (handle, view) = open_page(services, Page::Account, cx)?;
    after_fit(handle, cx, move |window, cx| {
        let account = view.read(cx).account().clone();
        AccountPage::open_email_sheet(account.clone(), window, cx);
        account.update(cx, |p, cx| p.preview_email_sending(cx));
    });
    Ok(handle)
}

/// The 设置登录邮箱 sheet after 发送验证码 to a long address.
pub fn open_email_code(
    services: Services,
    _content: gpui::Size<gpui::Pixels>,
    cx: &mut App,
) -> Result<AnyWindowHandle, String> {
    let (handle, view) = open_page(services, Page::Account, cx)?;
    after_fit(handle, cx, move |window, cx| {
        let account = view.read(cx).account().clone();
        AccountPage::open_email_sheet(account.clone(), window, cx);
        account.update(cx, |p, cx| {
            p.preview_email_sent("example.longname@example.com", window, cx)
        });
    });
    Ok(handle)
}

/// The 用手机扫码登录 sheet with a QR code waiting for the phone.
pub fn open_phone_qr(
    services: Services,
    _content: gpui::Size<gpui::Pixels>,
    cx: &mut App,
) -> Result<AnyWindowHandle, String> {
    let (handle, view) = open_page(services, Page::Account, cx)?;
    after_fit(handle, cx, move |window, cx| {
        let phone = view.read(cx).account().read(cx).phone().clone();
        super::account_phone::PhoneSignIn::open_qr_sheet(phone.clone(), window, cx);
        phone.update(cx, |p, cx| {
            p.preview_offer(
                crate::data::PhoneOffer {
                    id: "offer".into(),
                    code: "7K2M-9QPX".into(),
                    qr_text: "tc-sign-in:7K2M-9QPX?server=https%3A%2F%2Fchat.example.com".into(),
                    expires_at: 0,
                },
                crate::data::PhoneOfferState::Waiting,
                cx,
            )
        });
    });
    Ok(handle)
}

/// The 输入手机上的码 sheet.
pub fn open_phone_code(
    services: Services,
    _content: gpui::Size<gpui::Pixels>,
    cx: &mut App,
) -> Result<AnyWindowHandle, String> {
    let (handle, view) = open_page(services, Page::Account, cx)?;
    after_fit(handle, cx, move |window, cx| {
        let phone = view.read(cx).account().read(cx).phone().clone();
        super::account_phone::PhoneSignIn::open_code_sheet(phone, window, cx);
    });
    Ok(handle)
}

/// The 编辑资料 sheet after 完成 with an empty name: the field error.
pub fn open_profile_error(
    services: Services,
    _content: gpui::Size<gpui::Pixels>,
    cx: &mut App,
) -> Result<AnyWindowHandle, String> {
    let (handle, view) = open_page(services, Page::Account, cx)?;
    after_fit(handle, cx, move |window, cx| {
        let account = view.read(cx).account().clone();
        AccountPage::open_edit(account.clone(), window, cx);
        account.update(cx, |p, cx| {
            p.set_values(" ", "", window, cx);
            p.save(window, cx);
        });
    });
    Ok(handle)
}

/// The 退出登录 alert over 账号.
pub fn open_sign_out(
    services: Services,
    _content: gpui::Size<gpui::Pixels>,
    cx: &mut App,
) -> Result<AnyWindowHandle, String> {
    let (handle, view) = open_page(services, Page::Account, cx)?;
    after_fit(handle, cx, move |window, cx| {
        let account = view.read(cx).account().clone();
        AccountPage::confirm_sign_out(account, window, cx);
    });
    Ok(handle)
}

/// 账号 with a device selected and its sign-out alert open.
pub fn open_device_remove(
    services: Services,
    _content: gpui::Size<gpui::Pixels>,
    cx: &mut App,
) -> Result<AnyWindowHandle, String> {
    let (handle, view) = open_page(services, Page::Account, cx)?;
    let account = view.read(cx).account().clone();
    cx.spawn(async move |cx| {
        cx.background_executor()
            .timer(Duration::from_millis(120))
            .await;
        // The list loads off the UI thread.
        let id = wait_for(cx, |cx| {
            cx.update(|cx| {
                account
                    .read(cx)
                    .devices()
                    .map(|list| list.iter().find(|d| !d.this_mac).map(|d| d.id.clone()))
            })
        })
        .await
        .flatten();
        let _ = handle.update(cx, |_, window, cx| {
            account.update(cx, |a, cx| a.select_device(id, cx));
            AccountPage::confirm_remove_device(account.clone(), window, cx);
        });
    })
    .detach();
    Ok(handle)
}

// --- v14/t4 lists ---

/// 账号 with 登录的设备 failed to load: the Alert with 重试.
pub fn open_devices_failed(
    services: Services,
    _content: gpui::Size<gpui::Pixels>,
    cx: &mut App,
) -> Result<AnyWindowHandle, String> {
    let (handle, view) = open_page(services, Page::Account, cx)?;
    let account = view.read(cx).account().clone();
    cx.spawn(async move |cx| {
        // After the first load, so it does not replace the state.
        wait_for(cx, |cx| {
            cx.update(|cx| account.read(cx).devices().map(|_| ()))
        })
        .await;
        let _ = cx.update(|cx| {
            account.update(cx, |a, cx| {
                a.preview_devices_failed(crate::copy::settings::ERROR_OFFLINE, cx)
            })
        });
    })
    .detach();
    Ok(handle)
}

/// 机器 while the list loads: skeleton rows at the row height.
pub fn open_machines_loading(
    services: Services,
    _content: gpui::Size<gpui::Pixels>,
    cx: &mut App,
) -> Result<AnyWindowHandle, String> {
    let (handle, view) = open_page(services, Page::Machines, cx)?;
    let machines = view.read(cx).machines().clone();
    cx.spawn(async move |cx| {
        wait_for(cx, |cx| {
            cx.update(|cx| machines.read(cx).machines().map(|_| ()))
        })
        .await;
        let _ = cx.update(|cx| machines.update(cx, |m, cx| m.preview_loading(cx)));
    })
    .detach();
    Ok(handle)
}

// --- end v14/t4 lists ---

/// The longest member name and the longest 岗位 of the seeded members: the
/// worst values a sheet field can hold.
fn longest_member(services: &Services) -> (String, String) {
    let members = services.provider.members();
    let longest = |it: &mut dyn Iterator<Item = String>| {
        it.max_by_key(|s| s.chars().count()).unwrap_or_default()
    };
    (
        longest(&mut members.iter().map(|m| m.name.clone())),
        longest(&mut members.iter().filter_map(|m| m.role.clone())),
    )
}

/// A settings pane with one of its sheets open, for a scene.
fn open_with_dialog(
    services: Services,
    page: Page,
    open: fn(Services, &mut gpui::Window, &mut App),
    cx: &mut App,
) -> Result<AnyWindowHandle, String> {
    let (handle, _) = open_page(services.clone(), page, cx)?;
    after_fit(handle, cx, move |window, cx| open(services, window, cx));
    Ok(handle)
}

/// 成员 with the 邀请成员 sheet open.
pub fn open_invite_dialog(
    services: Services,
    _content: gpui::Size<gpui::Pixels>,
    cx: &mut App,
) -> Result<AnyWindowHandle, String> {
    open_with_dialog(
        services,
        Page::Members,
        |s, w, cx| {
            // Worst data: the longest name and 岗位 of the seeded members
            // in the fields (review 3 C1-12).
            let worst = crate::selftest::worst_case().then(|| longest_member(&s));
            let form = super::members::open_invite(s, w, cx);
            if let Some((name, role)) = worst {
                form.update(cx, |f, cx| {
                    f.fill(&name, w, cx);
                    f.fill_role(&role, w, cx);
                });
            }
        },
        cx,
    )
}

/// 成员 with the 添加 AI 成员 sheet open.
pub fn open_bot_dialog(
    services: Services,
    _content: gpui::Size<gpui::Pixels>,
    cx: &mut App,
) -> Result<AnyWindowHandle, String> {
    open_with_dialog(
        services,
        Page::Members,
        |s, w, cx| {
            let worst = crate::selftest::worst_case().then(|| longest_member(&s).0);
            let form = super::members::open_add_bot(s, w, cx);
            if let Some(name) = worst {
                form.update(cx, |f, cx| f.fill(&name, None, w, cx));
            }
        },
        cx,
    )
}

/// 成员 with the 添加 AI 成员 sheet open and a name the roster does not
/// know: the grid selects the first look no member uses.
pub fn open_bot_dialog_new_name(
    services: Services,
    _content: gpui::Size<gpui::Pixels>,
    cx: &mut App,
) -> Result<AnyWindowHandle, String> {
    open_with_dialog(
        services,
        Page::Members,
        |s, w, cx| {
            let form = super::members::open_add_bot(s, w, cx);
            form.update(cx, |f, cx| f.fill("发布巡检", None, w, cx));
        },
        cx,
    )
}

/// 成员 with the 添加 AI 成员 sheet set to work on the team's computers: 运行方式 lists what
/// the computers report by name, the ones not signed in marked.
pub fn open_bot_dialog_runner(
    services: Services,
    _content: gpui::Size<gpui::Pixels>,
    cx: &mut App,
) -> Result<AnyWindowHandle, String> {
    open_with_dialog(
        services,
        Page::Members,
        |s, w, cx| {
            let form = super::members::open_add_bot(s, w, cx);
            form.update(cx, |f, cx| f.fill("发布巡检", Some(""), w, cx));
        },
        cx,
    )
}

/// 成员 with the look sheet of the first AI member open.
pub fn open_character_dialog(
    services: Services,
    _content: gpui::Size<gpui::Pixels>,
    cx: &mut App,
) -> Result<AnyWindowHandle, String> {
    open_with_dialog(
        services,
        Page::Members,
        |s, w, cx| {
            // Without the character pack there is no look sheet: 成员 alone.
            let bot = s.provider.members().into_iter().find(|m| m.is_bot());
            if let Some(bot) = bot.filter(|_| crate::characters::available()) {
                super::members::open_character(s, &bot, w, cx);
            }
        },
        cx,
    )
}

/// 成员 with the edit sheet of the first AI member open.
pub fn open_edit_bot_dialog(
    services: Services,
    _content: gpui::Size<gpui::Pixels>,
    cx: &mut App,
) -> Result<AnyWindowHandle, String> {
    open_with_dialog(
        services,
        Page::Members,
        |s, w, cx| {
            let bot = s.provider.members().into_iter().find(|m| m.is_bot());
            if let Some(bot) = bot {
                super::members::open_edit_bot(s, &bot, w, cx);
            }
        },
        cx,
    )
}

/// 机器 with the 添加电脑 sheet open.
pub fn open_machine_dialog(
    services: Services,
    _content: gpui::Size<gpui::Pixels>,
    cx: &mut App,
) -> Result<AnyWindowHandle, String> {
    open_with_dialog(
        services,
        Page::Machines,
        |s, w, cx| {
            super::machines::open_add_machine(s, w, cx);
        },
        cx,
    )
}

/// 机器 after 添加电脑 succeeded: the one-time sheet with the system picker on Windows and
/// that system's install command.
pub fn open_machine_created(
    services: Services,
    _content: gpui::Size<gpui::Pixels>,
    cx: &mut App,
) -> Result<AnyWindowHandle, String> {
    open_with_dialog(
        services,
        Page::Machines,
        |s, w, cx| {
            let form = super::machines::open_add_machine(s, w, cx);
            form.update(cx, |form, cx| {
                form.fill("会议室的电脑", "windows", w, cx);
                form.submit(cx);
                form.set_target(super::machines::InstallTarget::Windows, cx);
            });
        },
        cx,
    )
}

// --- v12/settings track: the scenes of the new panes and their sheets ---

use crate::selftest::scenes::{Scene, Start};

pub const SCENES: &[Scene] = &[
    Scene {
        name: "settings-general",
        about: "Settings: 通用 (启动时, 登录时启动, 菜单栏里显示, 声音, version, developer view)",
        start: Start::SignedIn,
        setup: |_, _| {},
        open: crate::updater::preview::open_idle,
    },
    Scene {
        name: "settings-shortcuts",
        about: "Settings: 快捷键, the menus' shortcuts",
        start: Start::SignedIn,
        setup: |_, _| {},
        open: open_shortcuts,
    },
    Scene {
        name: "settings-edit-profile",
        about: "Settings: the 编辑资料 sheet over 账号",
        start: Start::SignedIn,
        setup: |_, _| {},
        open: open_edit_profile,
    },
    Scene {
        name: "settings-email",
        about: "Settings: the 设置登录邮箱 sheet before 发送验证码",
        start: Start::SignedIn,
        setup: |_, _| {},
        open: open_email,
    },
    Scene {
        name: "settings-email-sending",
        about: "Settings: the 设置登录邮箱 sheet while 发送验证码 is on its way",
        start: Start::SignedIn,
        setup: |_, _| {},
        open: open_email_sending,
    },
    Scene {
        name: "settings-email-code",
        about: "Settings: the 设置登录邮箱 sheet with the code slots after 发送验证码",
        start: Start::SignedIn,
        setup: |_, _| {},
        open: open_email_code,
    },
    Scene {
        name: "settings-phone-qr",
        about: "Settings: the 用手机扫码登录 sheet with the QR code",
        start: Start::SignedIn,
        setup: |_, _| {},
        open: open_phone_qr,
    },
    Scene {
        name: "settings-phone-code",
        about: "Settings: the 输入手机上的码 sheet",
        start: Start::SignedIn,
        setup: |_, _| {},
        open: open_phone_code,
    },
    Scene {
        name: "settings-sign-out",
        about: "Settings: the 退出登录 alert with its checkbox",
        start: Start::SignedIn,
        setup: |_, _| {},
        open: open_sign_out,
    },
    Scene {
        name: "settings-device-remove",
        about: "Settings: a device selected and its sign-out alert",
        start: Start::SignedIn,
        setup: |_, _| {},
        open: open_device_remove,
    },
    // --- v12/fx-overlays ---
    Scene {
        name: "update-ready",
        about: "The 重启安装 sheet over the main window once a downloaded update is ready",
        start: Start::SignedIn,
        setup: crate::selftest::scenes::open_first_topic,
        open: crate::updater::preview::open_ready,
    },
    // --- end v12/fx-overlays ---
    // --- v14/t4 lists ---
    Scene {
        name: "settings-devices-failed",
        about: "Settings: 账号 when 登录的设备 failed to load: the Alert with 重试",
        start: Start::SignedIn,
        setup: |_, _| {},
        open: open_devices_failed,
    },
    Scene {
        name: "settings-machine-created",
        about: "机器 › 添加电脑 done: the system picker (Mac 或 Linux / Windows) and the Windows install command",
        start: Start::SignedIn,
        setup: |_, _| {},
        open: open_machine_created,
    },
    Scene {
        name: "settings-machines-loading",
        about: "Settings: 机器 while the list loads: skeleton rows",
        start: Start::SignedIn,
        setup: |_, _| {},
        open: open_machines_loading,
    },
    // --- end v14/t4 lists ---
];

// --- end v12/settings track ---

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn invite_codes_are_checked() {
        assert!(invite_code_shape("ABCD-EFGH"));
        assert!(invite_code_shape("ABCD-EFGH-1234"));
        assert!(!invite_code_shape("ABCDEFGH"));
        assert!(!invite_code_shape("ABC-EFGH"));
        assert!(!invite_code_shape("ABC-EFGH-1234"));
    }
}
