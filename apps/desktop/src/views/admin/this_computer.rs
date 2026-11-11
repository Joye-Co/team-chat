//! 用这台电脑 tells the truth (机 1.2, 1.3; 模 5.1): after the background service is set up,
//! three lines say what this computer still lacks, read from what it reports: the service
//! runs (the computer is online), Claude Code or Codex was found, and one of them is signed
//! in. A missing line has its next step: the install command for this system with 重新检查,
//! or 登录, which opens the sign-in of 账号与密钥.
//!
//! The daemon adds a runtime for each CLI it finds when it starts and stops when there is
//! none, so a computer without a CLI never comes online and reports nothing: the app then looks
//! for the CLIs itself (`Provider::this_computer_clis`), the way the daemon does.

use gpui::{
    AnyElement, Context, IntoElement, ParentElement as _, SharedString, Styled, div,
    prelude::FluentBuilder as _, px,
};

use chat_core::proto::monitor::MonitorCli;

use super::{AdminView, accounts::Page, model};
use crate::{
    copy::{admin as m, fill},
    data::{MachineInfo, ThisMac},
    theme,
    ui::{
        Sym,
        icons::sym as sym_icon,
        native::{
            NativeType as _, PushSize, PushStyle, card_surface, loading_push, metrics,
            native_colors, push_button,
            tokens::{space, text},
        },
        sf_symbols::SymSize,
    },
    views::settings::widgets::answer,
};

/// Where one of the three lines stands.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Check {
    Done,
    /// Something to do: the line shows its next step.
    Todo,
    /// Not known yet, or waiting for an earlier line.
    Waiting,
}

/// What the three lines are read from.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Inputs {
    /// The service runs the app's own daemon.
    pub installed: bool,
    /// The last 用这台电脑 failed.
    pub failed: bool,
    /// This computer is online.
    pub connected: bool,
    /// (kind, runtime) of the CLIs the online computer reports.
    pub reported: Vec<(String, String)>,
    /// The sign-ins of its CLIs, once its monitor came in.
    pub logins: Option<Vec<MonitorCli>>,
    /// The CLIs the app found on this computer ("claude", "codex"); `None` before it looked.
    pub local: Option<Vec<String>>,
}

/// The three lines.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Readiness {
    pub service: Check,
    /// The service waits for a CLI: it stops at start without one.
    pub service_blocked: bool,
    pub found: Check,
    /// The CLIs found, as people know them.
    pub found_names: Vec<String>,
    pub signed_in: Check,
    /// The CLI that is signed in, or the one to sign in: (runtime, name).
    pub login: Option<(String, String)>,
}

impl Readiness {
    pub fn ready(&self) -> bool {
        self.service == Check::Done && self.found == Check::Done && self.signed_in == Check::Done
    }
}

/// A coding CLI the sign-in flow knows.
fn signs_in(kind: &str) -> bool {
    kind == "claude" || kind == "codex"
}

pub fn readiness(i: &Inputs) -> Readiness {
    let mut found_names: Vec<String> = vec![];
    let mut add = |name: String| {
        if !found_names.contains(&name) {
            found_names.push(name);
        }
    };
    let found = if i.connected {
        for (kind, runtime) in &i.reported {
            add(model::cli_name(kind, runtime));
        }
        if i.reported.is_empty() {
            Check::Todo
        } else {
            Check::Done
        }
    } else {
        match &i.local {
            Some(list) if !list.is_empty() => {
                for kind in list {
                    add(model::cli_name(kind, kind));
                }
                Check::Done
            }
            Some(_) => Check::Todo,
            None => Check::Waiting,
        }
    };
    let service_blocked = !i.connected && found == Check::Todo;
    let service = if i.connected {
        Check::Done
    } else if i.installed && !i.failed {
        Check::Waiting
    } else {
        Check::Todo
    };
    let (signed_in, login) = match (&i.logins, found, i.connected) {
        (Some(logins), Check::Done, true) => {
            let usable: Vec<&MonitorCli> = logins
                .iter()
                .filter(|c| c.available && signs_in(&c.kind))
                .collect();
            let pick = |state: &str| {
                usable
                    .iter()
                    .find(|c| c.login == state)
                    .map(|c| (c.runtime.clone(), model::cli_name(&c.kind, &c.runtime)))
            };
            if let Some(c) = pick("logged_in") {
                (Check::Done, Some(c))
            } else if let Some(c) = pick("logged_out") {
                (Check::Todo, Some(c))
            } else {
                (Check::Waiting, None)
            }
        }
        _ => (Check::Waiting, None),
    };
    Readiness {
        service,
        service_blocked,
        found,
        found_names,
        signed_in,
        login,
    }
}

/// The install command of Claude Code for this system.
pub fn install_command() -> &'static str {
    if cfg!(windows) {
        m::THIS_PC_INSTALL_WINDOWS
    } else {
        m::THIS_PC_INSTALL_MAC
    }
}

/// The state of the three lines in 团队管理 › 机器.
#[derive(Default)]
pub struct ThisComputer {
    /// What the app found on this computer; `None` until it looked.
    pub local: Option<Vec<String>>,
    looking: bool,
    /// The last 用这台电脑 failed.
    pub failed: bool,
    /// 用这台电脑 ran in this window: the lines stay until the window closes, also once all
    /// three are done.
    pub set_up_here: bool,
    /// A screenshot scene's inputs, in place of the live ones.
    pub pinned: Option<Inputs>,
}

impl AdminView {
    /// Looks for Claude Code and Codex on this computer, once per window (重新检查 looks again).
    pub(super) fn look_on_this_computer(&mut self, cx: &mut Context<Self>) {
        let this = &mut self.fleet.this;
        if this.looking || this.local.is_some() {
            return;
        }
        let installed = self.services.provider.this_mac() == ThisMac::Working;
        if !installed && !this.failed {
            return;
        }
        this.looking = true;
        let reply = self.services.provider.this_computer_clis();
        cx.spawn(async move |view, cx| {
            let found = answer(reply).await;
            let _ = view.update(cx, |v, cx| {
                v.fleet.this.looking = false;
                // A build that cannot look says nothing about the computer.
                v.fleet.this.local = Some(found.unwrap_or_default());
                cx.notify();
            });
        })
        .detach();
    }

    /// 重新检查: sets the service up again (it starts with the PATH of now, where a CLI just
    /// installed is), then looks again.
    fn recheck_this_computer(&mut self, cx: &mut Context<Self>) {
        self.fleet.this.local = None;
        self.use_this_mac(cx);
    }

    /// 登录 on the third line: 账号与密钥 of this computer, with the sign-in started.
    fn sign_in_this_computer(&mut self, runtime: String, cx: &mut Context<Self>) {
        let Some(id) = self.services.provider.this_machine_id() else {
            return;
        };
        self.select_machine(id, cx);
        self.show_page(Page::Accounts, cx);
        self.start_login(runtime, cx);
    }

    fn this_inputs(&self) -> Option<Inputs> {
        if let Some(p) = &self.fleet.this.pinned {
            return Some(p.clone());
        }
        let installed = self.services.provider.this_mac() == ThisMac::Working;
        let failed = self.fleet.this.failed;
        if !installed && !failed {
            return None;
        }
        let id = self.services.provider.this_machine_id();
        let machine: Option<&MachineInfo> = id
            .as_deref()
            .and_then(|id| self.machines.as_ref()?.iter().find(|x| x.id == id));
        if installed && machine.is_none() && !failed {
            // Set up for another server, or the list has not come in.
            return None;
        }
        let connected = machine.is_some_and(|x| x.connected);
        Some(Inputs {
            installed,
            failed,
            connected,
            reported: machine
                .map(|x| {
                    x.clis
                        .iter()
                        .map(|c| (c.kind.clone(), c.runtime.clone()))
                        .collect()
                })
                .unwrap_or_default(),
            logins: id
                .as_deref()
                .and_then(|id| self.fleet.monitors.get(id))
                .map(|d| d.clis.clone()),
            local: self.fleet.this.local.clone(),
        })
    }

    /// The three lines, when this computer is set up (or failed to be) and something is
    /// missing, or it was set up in this window.
    pub(super) fn this_computer_card(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let inputs = self.this_inputs()?;
        let r = readiness(&inputs);
        if r.ready() && !self.fleet.this.set_up_here && self.fleet.this.pinned.is_none() {
            return None;
        }
        let k = native_colors(cx);
        let busy = self.fleet.using;
        let view = cx.entity();
        let retry = (r.service == Check::Todo && !r.service_blocked).then(|| {
            let view = view.clone();
            loading_push(
                push_button(
                    "this-pc-retry",
                    m::THIS_PC_RETRY,
                    PushStyle::Outline,
                    PushSize::Small,
                    cx,
                ),
                busy,
                PushSize::Small,
            )
            .on_click(move |_, _, cx| view.update(cx, |v, cx| v.recheck_this_computer(cx)))
        });
        let service_desc: SharedString = match r.service {
            Check::Done => m::THIS_PC_SERVICE_OK.into(),
            _ if r.service_blocked => m::THIS_PC_SERVICE_BLOCKED.into(),
            Check::Waiting => m::THIS_PC_SERVICE_CONNECTING.into(),
            Check::Todo => match self.fleet.using_note.clone() {
                Some(why) => fill(m::THIS_PC_SERVICE_FAILED_WHY, &[("why", &why)]).into(),
                None => m::THIS_PC_SERVICE_FAILED.into(),
            },
        };
        let found_desc: SharedString = match r.found {
            Check::Done => fill(
                m::THIS_PC_FOUND_OK,
                &[("names", &r.found_names.join(m::THIS_PC_AND))],
            )
            .into(),
            Check::Todo => m::THIS_PC_FOUND_MISSING.into(),
            Check::Waiting => m::THIS_PC_FOUND_CHECKING.into(),
        };
        let install = (r.found == Check::Todo).then(|| {
            let view = view.clone();
            let command = install_command();
            div()
                .flex()
                .flex_col()
                .gap(space::S8)
                .child(
                    div()
                        .w_full()
                        .min_h(metrics::FIELD)
                        .pl(metrics::FIELD_PAD)
                        .pr(px(2.))
                        .py(px(3.))
                        .rounded(metrics::FIELD_RADIUS)
                        .bg(k.content)
                        .border_1()
                        .border_color(k.separator)
                        .flex()
                        .items_center()
                        .gap(space::S6)
                        .child(
                            div()
                                .flex_1()
                                .min_w(px(0.))
                                .font_family(theme::MONO_FONT_FAMILY)
                                .text_size(text::META.0)
                                .line_height(px(18.))
                                .text_color(k.label)
                                .child(command),
                        )
                        .child(
                            gpui_component::clipboard::Clipboard::new("this-pc-install-copy")
                                .tooltip(m::THIS_PC_COPY)
                                .value(command),
                        ),
                )
                .child(
                    div().flex().child(
                        loading_push(
                            push_button(
                                "this-pc-recheck",
                                m::THIS_PC_RECHECK,
                                PushStyle::Default,
                                PushSize::Small,
                                cx,
                            ),
                            busy || self.fleet.this.looking,
                            PushSize::Small,
                        )
                        .on_click(move |_, _, cx| {
                            view.update(cx, |v, cx| v.recheck_this_computer(cx))
                        }),
                    ),
                )
        });
        let name = r.login.as_ref().map(|(_, n)| n.clone()).unwrap_or_default();
        let signed_desc: SharedString = match r.signed_in {
            Check::Done => fill(m::THIS_PC_SIGNED_IN_OK, &[("name", &name)]).into(),
            Check::Todo => fill(m::THIS_PC_SIGNED_IN_MISSING, &[("name", &name)]).into(),
            Check::Waiting if r.found == Check::Done && r.service == Check::Done => {
                m::THIS_PC_SIGNED_IN_READING.into()
            }
            Check::Waiting => m::THIS_PC_SIGNED_IN_WAITING.into(),
        };
        let sign_in = match (&r.signed_in, &r.login) {
            (Check::Todo, Some((runtime, _))) => {
                let (view, runtime) = (view.clone(), runtime.clone());
                Some(
                    push_button(
                        "this-pc-sign-in",
                        m::LOGIN,
                        PushStyle::Default,
                        PushSize::Small,
                        cx,
                    )
                    .on_click(move |_, _, cx| {
                        let rt = runtime.clone();
                        view.update(cx, |v, cx| v.sign_in_this_computer(rt, cx))
                    }),
                )
            }
            _ => None,
        };
        let (title, sub) = if r.ready() {
            (m::THIS_PC_TITLE_READY, m::THIS_PC_READY_DESC)
        } else {
            (m::THIS_PC_TITLE, m::THIS_PC_DESC)
        };
        Some(
            card_surface(cx)
                .p(space::S24)
                .flex()
                .flex_col()
                .gap(space::S16)
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap(space::S2)
                        .child(div().n_headline().text_color(k.label).child(title))
                        .child(div().n_footnote().text_color(k.secondary).child(sub)),
                )
                .child(line(
                    r.service,
                    m::THIS_PC_SERVICE,
                    service_desc,
                    retry.map(|b| div().flex().child(b).into_any_element()),
                    cx,
                ))
                .child(line(
                    r.found,
                    m::THIS_PC_FOUND,
                    found_desc,
                    install.map(IntoElement::into_any_element),
                    cx,
                ))
                .child(line(
                    r.signed_in,
                    m::THIS_PC_SIGNED_IN,
                    signed_desc,
                    sign_in.map(|b| div().flex().child(b).into_any_element()),
                    cx,
                ))
                .into_any_element(),
        )
    }
}

/// One line: a mark, what it checks, where it stands and its next step.
fn line(
    check: Check,
    label: &'static str,
    desc: SharedString,
    step: Option<AnyElement>,
    cx: &mut Context<AdminView>,
) -> AnyElement {
    let k = native_colors(cx);
    let (glyph, color) = match check {
        Check::Done => (Sym::Done, k.green),
        Check::Todo => (Sym::NeedsYou, k.amber),
        Check::Waiting => (Sym::Todo, k.tertiary),
    };
    div()
        .flex()
        .items_start()
        .gap(space::S12)
        .child(
            div()
                .flex_none()
                .pt(space::S2)
                .child(sym_icon(glyph, SymSize::INLINE, color)),
        )
        .child(
            div()
                .flex_1()
                .min_w(px(0.))
                .flex()
                .flex_col()
                .gap(space::S2)
                .child(
                    div()
                        .n_body()
                        .text_color(if check == Check::Waiting {
                            k.secondary
                        } else {
                            k.label
                        })
                        .child(label),
                )
                .child(div().n_footnote().text_color(k.secondary).child(desc))
                .when_some(step, |d, s| d.child(div().pt(space::S6).child(s))),
        )
        .into_any_element()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cli(kind: &str, login: &str) -> MonitorCli {
        MonitorCli {
            runtime: kind.into(),
            kind: kind.into(),
            available: true,
            version: None,
            login: login.into(),
            failed_today: 0,
            method: None,
            email: None,
            plan: None,
            usage: None,
            usage_at: None,
        }
    }

    fn online(reported: &[&str], logins: Option<Vec<MonitorCli>>) -> Inputs {
        Inputs {
            installed: true,
            connected: true,
            reported: reported
                .iter()
                .map(|k| (k.to_string(), k.to_string()))
                .collect(),
            logins,
            ..Default::default()
        }
    }

    #[test]
    fn an_installed_service_alone_is_not_ready() {
        // Installed, not online yet, the app has not looked for the CLIs.
        let r = readiness(&Inputs {
            installed: true,
            ..Default::default()
        });
        assert_eq!(r.service, Check::Waiting);
        assert_eq!(r.found, Check::Waiting);
        assert_eq!(r.signed_in, Check::Waiting);
        assert!(!r.ready());
    }

    #[test]
    fn without_a_cli_the_service_waits_for_one() {
        let r = readiness(&Inputs {
            installed: true,
            local: Some(vec![]),
            ..Default::default()
        });
        assert_eq!(r.found, Check::Todo);
        assert!(r.service_blocked);
        assert_eq!(r.service, Check::Waiting);
        // Windows does not register a service without a CLI: 用这台电脑 failed.
        let r = readiness(&Inputs {
            failed: true,
            local: Some(vec![]),
            ..Default::default()
        });
        assert_eq!(r.service, Check::Todo);
        assert!(r.service_blocked);
    }

    #[test]
    fn a_cli_found_here_counts_until_the_computer_reports() {
        let r = readiness(&Inputs {
            installed: true,
            local: Some(vec!["claude".into(), "codex".into()]),
            ..Default::default()
        });
        assert_eq!(r.found, Check::Done);
        assert_eq!(r.found_names, ["Claude Code", "Codex"]);
        assert_eq!(r.service, Check::Waiting);
        assert!(!r.service_blocked);
        assert_eq!(r.signed_in, Check::Waiting);
    }

    #[test]
    fn the_sign_in_is_read_from_the_computer() {
        let r = readiness(&online(&["claude"], None));
        assert_eq!(r.signed_in, Check::Waiting, "monitor not in yet");
        let r = readiness(&online(
            &["claude"],
            Some(vec![cli("claude", "logged_out")]),
        ));
        assert_eq!(r.signed_in, Check::Todo);
        assert_eq!(r.login, Some(("claude".into(), "Claude Code".into())));
        assert!(!r.ready());
        let r = readiness(&online(
            &["claude", "codex"],
            Some(vec![cli("claude", "logged_out"), cli("codex", "logged_in")]),
        ));
        assert_eq!(r.signed_in, Check::Done);
        assert_eq!(r.login, Some(("codex".into(), "Codex".into())));
        assert!(r.ready());
        let r = readiness(&online(&["claude"], Some(vec![cli("claude", "unknown")])));
        assert_eq!(r.signed_in, Check::Waiting);
    }

    #[test]
    fn an_online_computer_without_a_cli_needs_one() {
        let r = readiness(&online(&[], Some(vec![])));
        assert_eq!(r.service, Check::Done);
        assert_eq!(r.found, Check::Todo);
        assert_eq!(r.signed_in, Check::Waiting);
    }

    #[test]
    fn the_install_command_is_the_official_one_for_this_system() {
        let c = install_command();
        assert!(c.contains("https://claude.ai/install."));
        assert_eq!(c.ends_with("| iex"), cfg!(windows));
    }
}
