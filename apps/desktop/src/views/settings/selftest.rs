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

    let opened = cx.update(|cx| super::open_window(services.clone(), Page::Profile, false, cx));
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

    // 个人: save the profile through the page.
    let profile = cx.update(|cx| view.read(cx).profile().clone());
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

    // 通知: mute and level of the first group, then put them back.
    let notifications = cx.update(|cx| view.read(cx).notifications().clone());
    if let Some(group) = provider.groups().into_iter().next() {
        let before = provider.group_prefs(&group.id);
        let gid = group.id.clone();
        cx.update(|cx| {
            notifications.update(cx, |p, cx| {
                p.set_muted(&gid, !before.muted, cx);
            })
        });
        let muted = wait_for(cx, |_| {
            (provider.group_prefs(&group.id).muted == !before.muted).then_some(())
        })
        .await;
        let target = if before.notify == NotifyLevel::None {
            NotifyLevel::All
        } else {
            NotifyLevel::None
        };
        cx.update(|cx| notifications.update(cx, |p, cx| p.set_level(&gid, target, cx)));
        let level = wait_for(cx, |_| {
            (provider.group_prefs(&group.id).notify == target).then_some(())
        })
        .await;
        check(
            "group_prefs_change",
            muted.and(level).ok_or("prefs not changed".into()),
        );
        cx.update(|cx| {
            notifications.update(cx, |p, cx| {
                p.set_muted(&gid, before.muted, cx);
            })
        });
        wait_for(cx, |_| {
            (provider.group_prefs(&group.id).muted == before.muted).then_some(())
        })
        .await;
        cx.update(|cx| notifications.update(cx, |p, cx| p.set_level(&gid, before.notify, cx)));
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
        cx.update(|cx| notifications.update(cx, |p, cx| p.unfollow(&g, &t, cx)));
        let gone = wait_for(cx, |_| {
            (!provider.followed_topics().iter().any(|x| x.topic.id == t)).then_some(())
        })
        .await;
        check("unfollow_topic", gone.ok_or("still followed".into()));
        // Follow it again.
        provider.set_topic_following(&g, &t, true);
    }

    // 关于: send diagnostics with a description.
    let about = cx.update(|cx| view.read(cx).about().clone());
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

macro_rules! scene_opener {
    ($name:ident, $page:expr) => {
        pub fn $name(
            services: Services,
            _content: gpui::Size<gpui::Pixels>,
            cx: &mut App,
        ) -> Result<AnyWindowHandle, String> {
            super::open_window(services, $page, false, cx).map(|(h, _)| h)
        }
    };
}

scene_opener!(open_profile, Page::Profile);
scene_opener!(open_members, Page::Members);
scene_opener!(open_machines, Page::Machines);
scene_opener!(open_notifications, Page::Notifications);
scene_opener!(open_about, Page::About);

/// 成员 scrolled to its end: the last rows and the footnote under the table.
pub fn open_members_end(
    services: Services,
    _content: gpui::Size<gpui::Pixels>,
    cx: &mut App,
) -> Result<AnyWindowHandle, String> {
    let (handle, view) = super::open_window(services, Page::Members, false, cx)?;
    handle
        .update(cx, |_, window, _| {
            // After the first layout, when the page knows its height.
            window.on_next_frame(move |_, cx| view.read(cx).scroll_to_end());
        })
        .map_err(|e| e.to_string())?;
    Ok(handle)
}

/// 通知 scrolled to its end: the 私聊 table and 关注的话题 under it.
pub fn open_notifications_end(
    services: Services,
    _content: gpui::Size<gpui::Pixels>,
    cx: &mut App,
) -> Result<AnyWindowHandle, String> {
    let (handle, view) = super::open_window(services, Page::Notifications, false, cx)?;
    handle
        .update(cx, |_, window, _| {
            // After the first layout, when the page knows its height.
            window.on_next_frame(move |_, cx| view.read(cx).scroll_to_end());
        })
        .map_err(|e| e.to_string())?;
    Ok(handle)
}

/// 我的资料 after saving an empty name: the field error state.
pub fn open_profile_error(
    services: Services,
    _content: gpui::Size<gpui::Pixels>,
    cx: &mut App,
) -> Result<AnyWindowHandle, String> {
    let (handle, view) = super::open_window(services, Page::Profile, false, cx)?;
    handle
        .update(cx, |_, window, cx| {
            let profile = view.read(cx).profile().clone();
            profile.update(cx, |p, cx| {
                p.set_values(" ", "", window, cx);
                p.save(window, cx);
            });
        })
        .map_err(|e| e.to_string())?;
    Ok(handle)
}

/// A settings page with one of its dialogs open, for a scene.
fn open_with_dialog(
    services: Services,
    page: Page,
    open: fn(Services, &mut gpui::Window, &mut App),
    cx: &mut App,
) -> Result<AnyWindowHandle, String> {
    let (handle, _) = super::open_window(services.clone(), page, false, cx)?;
    handle
        .update(cx, |_, window, cx| open(services, window, cx))
        .map_err(|e| e.to_string())?;
    Ok(handle)
}

/// 成员 with the invite dialog open.
pub fn open_invite_dialog(
    services: Services,
    _content: gpui::Size<gpui::Pixels>,
    cx: &mut App,
) -> Result<AnyWindowHandle, String> {
    open_with_dialog(
        services,
        Page::Members,
        |s, w, cx| {
            super::members::open_invite(s, w, cx);
        },
        cx,
    )
}

/// 成员 with the add-bot dialog open.
pub fn open_bot_dialog(
    services: Services,
    _content: gpui::Size<gpui::Pixels>,
    cx: &mut App,
) -> Result<AnyWindowHandle, String> {
    open_with_dialog(
        services,
        Page::Members,
        |s, w, cx| {
            super::members::open_add_bot(s, w, cx);
        },
        cx,
    )
}

/// 成员 with the add-bot dialog open and a name typed that the roster does not know: the
/// grid selects the first character no member uses.
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

/// 成员 with the character dialog of the first bot open.
pub fn open_character_dialog(
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
                super::members::open_character(s, &bot, w, cx);
            }
        },
        cx,
    )
}

/// 机器 with the add-machine dialog open.
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
