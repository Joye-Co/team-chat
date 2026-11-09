//! 账号与密钥 of 团队管理 (feature "accounts"; docs/protocol.md "Machine accounts"):
//! 订阅账号 signs the machine's CLIs in from here (Claude Code: open the link, paste the code;
//! Codex: open the link, enter the one-time code), with each subscription's usage windows;
//! API 密钥 gives the machine keys, sealed to its public key on this Mac, for every run or for
//! chosen AI members. No credential is shown or kept here: keys show their last 4 only.

use std::time::Duration;

use gpui::{
    AnyElement, App, AppContext as _, ClipboardItem, Context, Entity, IntoElement,
    ParentElement as _, SharedString, Styled, Window, div, prelude::FluentBuilder as _,
};
use gpui_component::input::InputState;
use gpui_component::menu::{DropdownMenu as _, PopupMenuItem};

use chat_core::proto::machine::login_phase as phase;
use chat_core::proto::monitor::{MachineLogin, MachineSecretsResponse, MonitorCli};

use super::{AdminView, model};
use crate::{
    copy::{admin as m, fill},
    data::{AdminError, MemberKind, SecretText},
    ui::{
        MemberAvatar, Sym,
        icons::sym as sym_icon,
        native::{
            Alert, AlertTone, Badge, BadgeState, Column, Empty, NativeCheckbox, NativeTextField,
            NativeType as _, PushSize, PushStyle, StatusBadge, TruncText, card_surface,
            native_colors, progress, push_button, skeleton, table,
            tokens::{avatar, height, monitor as t, radius, space, text},
        },
        sf_symbols::SymSize,
    },
    views::settings::widgets::answer,
};

/// How often a sign-in in progress is asked about.
const LOGIN_POLL: Duration = Duration::from_millis(1500);

/// The pages of 团队管理 that are live.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Page {
    /// 机器 › <machine> › 概览.
    #[default]
    Overview,
    /// 账号与密钥.
    Accounts,
    Keys,
    /// 团队 › 成员 (the roster pane that was in Settings).
    Members,
    /// 机器: every computer as a card.
    Machines,
    /// 团队 › 活动记录.
    Activity,
    /// 团队设置.
    Ai,
    Rules,
    Memory,
}

/// A sign-in the owner started from 订阅账号.
pub struct LoginFlow {
    pub machine_id: String,
    pub runtime: String,
    pub login_id: Option<String>,
    pub status: Option<MachineLogin>,
    pub error: Option<String>,
    pub sending: bool,
}

impl LoginFlow {
    fn state(&self) -> &str {
        self.status
            .as_ref()
            .map(|s| s.state.as_str())
            .unwrap_or(phase::STARTING)
    }

    fn ended(&self) -> bool {
        self.error.is_some()
            || matches!(self.state(), phase::DONE | phase::FAILED | phase::CANCELLED)
    }
}

/// The state of 订阅账号 and API 密钥.
pub struct State {
    pub login: Option<LoginFlow>,
    code: Entity<InputState>,
    key_name: Entity<InputState>,
    key_value: Entity<InputState>,
    pub secrets: Option<MachineSecretsResponse>,
    pub secrets_for: Option<String>,
    pub secrets_error: Option<String>,
    form_open: bool,
    /// AI members the new key is for; empty = every run.
    form_members: Vec<String>,
    form_error: Option<String>,
    saving: bool,
    notice: Option<String>,
}

impl State {
    pub fn new(window: &mut Window, cx: &mut Context<AdminView>) -> State {
        State {
            login: None,
            code: cx.new(|cx| InputState::new(window, cx).placeholder(m::CODE_PLACEHOLDER)),
            key_name: cx
                .new(|cx| InputState::new(window, cx).placeholder(m::KEYS_NAME_PLACEHOLDER)),
            key_value: cx.new(|cx| {
                InputState::new(window, cx)
                    .masked(true)
                    .placeholder(m::KEYS_VALUE_PLACEHOLDER)
            }),
            secrets: None,
            secrets_for: None,
            secrets_error: None,
            form_open: false,
            form_members: vec![],
            form_error: None,
            saving: false,
            notice: None,
        }
    }
}

/// The key a CLI uses instead of a sign-in.
fn api_key_name(kind: &str) -> &'static str {
    match kind {
        "codex" => "OPENAI_API_KEY",
        _ => "ANTHROPIC_API_KEY",
    }
}

fn admin_error(e: AdminError) -> String {
    match e {
        AdminError::Conflict => m::MACHINE_UNREACHABLE.to_string(),
        e => crate::views::settings::widgets::error_text(e).to_string(),
    }
}

/// A valid key name: an environment variable name.
pub fn valid_key_name(name: &str) -> bool {
    let b = name.as_bytes();
    !b.is_empty()
        && b.len() <= 64
        && (b[0].is_ascii_uppercase() || b[0] == b'_')
        && b.iter()
            .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || *c == b'_')
}

impl AdminView {
    pub(super) fn show_page(&mut self, page: Page, cx: &mut Context<Self>) {
        if self.page != page {
            self.page = page;
            self.scroll
                .set_offset(gpui::point(gpui::Pixels::ZERO, gpui::Pixels::ZERO));
            match page {
                Page::Keys => self.fetch_secrets(cx),
                Page::Machines => self.fetch_fleet(cx),
                Page::Activity if !self.activity.loaded => self.fetch_activity(false, cx),
                Page::Memory => self.memory.update(cx, |m, cx| m.load(cx)),
                _ => {}
            }
            cx.notify();
        }
    }

    /// [`Self::show_page`] from a click: panes that need the window load here.
    pub(super) fn open_page(&mut self, page: Page, window: &mut Window, cx: &mut Context<Self>) {
        self.show_page(page, cx);
        if page == Page::Ai {
            self.ai.update(cx, |a, cx| a.refresh(window, cx));
            self.models.update(cx, |m, cx| m.refresh(cx));
        }
        if page == Page::Rules || page == Page::Members {
            self.members.update(cx, |m, cx| m.refresh_rules(window, cx));
            self.rules.update(cx, |m, cx| m.refresh_rules(window, cx));
        }
    }

    /// Reads the machine's keys (on 打开 API 密钥, after a change, and with every refresh).
    pub(super) fn fetch_secrets(&mut self, cx: &mut Context<Self>) {
        let Some(id) = self.selected.clone() else {
            return;
        };
        let reply = self.services.provider.machine_secrets(&id);
        cx.spawn(async move |this, cx| {
            let result = answer(reply).await;
            let _ = this.update(cx, |this, cx| {
                if this.selected.as_deref() != Some(id.as_str()) {
                    return;
                }
                match result {
                    Ok(s) => {
                        // The machine's list moved on: the "sent" notice has done its job.
                        if this.acct.secrets.as_ref().map(|x| &x.secrets) != Some(&s.secrets) {
                            this.acct.notice = None;
                        }
                        this.acct.secrets = Some(s);
                        this.acct.secrets_for = Some(id);
                        this.acct.secrets_error = None;
                    }
                    Err(e) => this.acct.secrets_error = Some(admin_error(e)),
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn start_login(&mut self, runtime: String, cx: &mut Context<Self>) {
        let Some(machine_id) = self.selected.clone() else {
            return;
        };
        self.acct.login = Some(LoginFlow {
            machine_id: machine_id.clone(),
            runtime: runtime.clone(),
            login_id: None,
            status: None,
            error: None,
            sending: false,
        });
        let reply = self
            .services
            .provider
            .start_machine_login(&machine_id, &runtime);
        cx.spawn(async move |this, cx| {
            let result = answer(reply).await;
            let _ = this.update(cx, |this, cx| {
                let Some(flow) = this.acct.login.as_mut().filter(|f| f.runtime == runtime) else {
                    return;
                };
                match result {
                    Ok(id) => {
                        flow.login_id = Some(id);
                        this.poll_login(cx);
                    }
                    Err(e) => flow.error = Some(admin_error(e)),
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    /// Asks where the sign-in stands until it ends or another one replaces it.
    fn poll_login(&mut self, cx: &mut Context<Self>) {
        let Some(id) = self.acct.login.as_ref().and_then(|f| f.login_id.clone()) else {
            return;
        };
        cx.spawn(async move |this, cx| {
            loop {
                let next = this.update(cx, |this, _| {
                    let flow = this.acct.login.as_ref()?;
                    (flow.login_id.as_deref() == Some(id.as_str()) && !flow.ended())
                        .then(|| this.services.provider.machine_login(&flow.machine_id, &id))
                });
                let Ok(Some(reply)) = next else { break };
                let result = answer(reply).await;
                let done = this
                    .update(cx, |this, cx| {
                        let Some(flow) = this
                            .acct
                            .login
                            .as_mut()
                            .filter(|f| f.login_id.as_deref() == Some(id.as_str()))
                        else {
                            return true;
                        };
                        if let Ok(s) = result {
                            flow.status = Some(s);
                        }
                        let ended = flow.ended();
                        if flow.state() == phase::DONE {
                            // Who is signed in and the usage arrive with the next heartbeats.
                            this.refresh(cx);
                        }
                        cx.notify();
                        ended
                    })
                    .unwrap_or(true);
                if done {
                    break;
                }
                cx.background_executor().timer(LOGIN_POLL).await;
            }
        })
        .detach();
    }

    fn send_code(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let code = self.acct.code.read(cx).value().trim().to_string();
        let Some(flow) = self.acct.login.as_mut() else {
            return;
        };
        let Some(login_id) = flow.login_id.clone() else {
            return;
        };
        if code.is_empty() || flow.sending {
            return;
        }
        flow.sending = true;
        let reply = self
            .services
            .provider
            .machine_login_code(&flow.machine_id, &login_id, &code);
        self.acct
            .code
            .update(cx, |s, cx| s.set_value("", window, cx));
        cx.spawn(async move |this, cx| {
            let result = answer(reply).await;
            let _ = this.update(cx, |this, cx| {
                if let Some(flow) = this.acct.login.as_mut() {
                    flow.sending = false;
                    if let Err(e) = result {
                        flow.error = Some(admin_error(e));
                    }
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    fn close_login(&mut self, cx: &mut Context<Self>) {
        if let Some(flow) = self.acct.login.take()
            && !flow.ended()
            && let Some(id) = flow.login_id
        {
            let reply = self
                .services
                .provider
                .cancel_machine_login(&flow.machine_id, &id);
            cx.background_executor()
                .spawn(async move {
                    let _ = answer(reply).await;
                })
                .detach();
        }
        cx.notify();
    }

    fn open_key_form(&mut self, name: Option<&str>, window: &mut Window, cx: &mut Context<Self>) {
        self.acct.form_open = true;
        self.acct.form_error = None;
        self.acct.form_members.clear();
        let name = name.unwrap_or("").to_string();
        self.acct
            .key_name
            .update(cx, |s, cx| s.set_value(name, window, cx));
        self.acct
            .key_value
            .update(cx, |s, cx| s.set_value("", window, cx));
        cx.notify();
    }

    fn save_key(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let name = self.acct.key_name.read(cx).value().trim().to_string();
        let value = self.acct.key_value.read(cx).value().trim().to_string();
        if !valid_key_name(&name) {
            self.acct.form_error = Some(m::KEYS_NAME_INVALID.into());
            cx.notify();
            return;
        }
        let (Some(id), Some(key)) = (
            self.selected.clone(),
            self.acct
                .secrets
                .as_ref()
                .and_then(|s| s.public_key.clone()),
        ) else {
            self.acct.form_error = Some(m::KEYS_NO_PUBLIC_KEY.into());
            cx.notify();
            return;
        };
        if value.is_empty() || self.acct.saving {
            return;
        }
        self.acct.saving = true;
        let reply = self.services.provider.set_machine_secret(
            &id,
            &key,
            &name,
            SecretText::new(value),
            self.acct.form_members.clone(),
        );
        self.acct
            .key_value
            .update(cx, |s, cx| s.set_value("", window, cx));
        cx.spawn(async move |this, cx| {
            let result = answer(reply).await;
            let _ = this.update(cx, |this, cx| {
                this.acct.saving = false;
                match result {
                    Ok(()) => {
                        this.acct.form_open = false;
                        this.acct.notice = Some(m::KEYS_SAVED.into());
                    }
                    Err(e) => this.acct.form_error = Some(admin_error(e)),
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    fn delete_key(&mut self, name: String, cx: &mut Context<Self>) {
        let Some(id) = self.selected.clone() else {
            return;
        };
        let reply = self.services.provider.delete_machine_secret(&id, &name);
        cx.spawn(async move |this, cx| {
            let result = answer(reply).await;
            let _ = this.update(cx, |this, cx| {
                match result {
                    Ok(()) => this.acct.notice = Some(m::KEYS_SAVED.into()),
                    Err(e) => this.acct.secrets_error = Some(admin_error(e)),
                }
                cx.notify();
            });
        })
        .detach();
    }

    // -----------------------------------------------------------------------
    // 订阅账号

    pub(super) fn accounts_page(
        &self,
        now: u64,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let Some(d) = self.current() else {
            return super::overview::page_skeleton(cx).into_any_element();
        };
        let clis: Vec<&MonitorCli> = d
            .clis
            .iter()
            .filter(|c| c.kind == "claude" || c.kind == "codex")
            .collect();
        if clis.is_empty() {
            return Empty::new(Sym::Terminal, m::ACCOUNTS_EMPTY)
                .description(m::ACCOUNTS_EMPTY_DESC)
                .wide()
                .into_any_element();
        }
        div()
            .w_full()
            .flex()
            .flex_col()
            .gap(space::S16)
            .children(clis.into_iter().map(|c| self.cli_card(c, now, window, cx)))
            .into_any_element()
    }

    fn cli_card(
        &self,
        c: &MonitorCli,
        now: u64,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let k = native_colors(cx);
        let name = model::cli_name(&c.kind, &c.runtime);
        let logged_in = c.login == "logged_in";
        let (state, word) = if !c.available {
            (BadgeState::Offline, m::LOGIN_UNKNOWN)
        } else {
            match c.login.as_str() {
                "logged_in" => (BadgeState::Online, m::LOGIN_IN),
                "logged_out" => (BadgeState::NeedsYou, m::LOGIN_OUT),
                _ => (BadgeState::Quiet, m::LOGIN_UNKNOWN),
            }
        };
        let mut meta: Vec<String> = vec![];
        match c.method.as_deref() {
            Some("subscription") => meta.push(m::METHOD_SUBSCRIPTION.into()),
            Some("api_key") => meta.push(m::METHOD_API_KEY.into()),
            _ => {}
        }
        if let Some(v) = &c.version {
            let number = v
                .split_whitespace()
                .find(|w| w.starts_with(|c: char| c.is_ascii_digit()))
                .unwrap_or(v);
            meta.push(fill(m::CLI_META, &[("version", number)]));
        }
        if let Some(e) = &c.email {
            meta.push(e.clone());
        }
        let view = cx.entity();
        let runtime = c.runtime.clone();
        let flow_here = self.acct.login.as_ref().filter(|f| {
            f.runtime == c.runtime && Some(f.machine_id.as_str()) == self.selected.as_deref()
        });
        let login_button = (flow_here.is_none() && c.available).then(|| {
            let label = if logged_in {
                m::RELOGIN
            } else if c.kind == "codex" {
                m::LOGIN_CHATGPT
            } else {
                m::LOGIN
            };
            let style = if logged_in {
                PushStyle::Outline
            } else {
                PushStyle::Default
            };
            push_button(
                SharedString::from(format!("login-{}", c.runtime)),
                label,
                style,
                PushSize::Small,
                cx,
            )
            .on_click(move |_, _, cx| {
                let rt = runtime.clone();
                view.update(cx, |v, cx| v.start_login(rt, cx))
            })
        });
        let key = api_key_name(&c.kind);
        let view = cx.entity();
        let use_key = push_button(
            SharedString::from(format!("use-key-{}", c.runtime)),
            fill(m::USE_API_KEY, &[("name", key)]),
            PushStyle::Link,
            PushSize::Small,
            cx,
        )
        .on_click(move |_, window, cx| {
            view.update(cx, |v, cx| {
                v.show_page(Page::Keys, cx);
                v.open_key_form(Some(key), window, cx);
            })
        });
        let body: Option<AnyElement> = match flow_here {
            Some(f) => Some(self.login_panel(f, window, cx)),
            None if c.login == "logged_out" && c.failed_today > 0 => Some(
                Alert::new(
                    Sym::Warning,
                    fill(m::FAILED_TODAY, &[("n", &c.failed_today.to_string())]),
                )
                .tone(AlertTone::Warning)
                .into_any_element(),
            ),
            None => None,
        };
        card_surface(cx)
            .p(space::S24)
            .flex()
            .flex_col()
            .gap(space::S16)
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(space::S12)
                    .child(
                        div()
                            .flex_none()
                            .size(height::LG)
                            .rounded(radius::CONTROL)
                            .bg(k.primary)
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(sym_icon(Sym::Terminal, SymSize::INLINE, k.on_primary)),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w(gpui::Pixels::ZERO)
                            .flex()
                            .flex_col()
                            .gap(space::S2)
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap(space::S8)
                                    .child(
                                        div()
                                            .n_title3()
                                            .font_weight(gpui::FontWeight::SEMIBOLD)
                                            .child(name),
                                    )
                                    .children(
                                        c.plan.clone().filter(|p| p != "api_key").map(Badge::new),
                                    )
                                    .child(StatusBadge::new(state, word)),
                            )
                            .child(
                                TruncText::new(meta.join(" · "))
                                    .n_footnote()
                                    .text_color(k.secondary),
                            ),
                    )
                    .children(login_button),
            )
            .children(body)
            .when(flow_here.is_none(), |d| {
                d.child(self.usage_block(c, now, cx))
            })
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(space::S4)
                    .n_footnote()
                    .text_color(k.secondary)
                    .child(m::USE_API_KEY_HINT)
                    .child(use_key),
            )
            .into_any_element()
    }

    fn usage_block(&self, c: &MonitorCli, now: u64, cx: &App) -> AnyElement {
        let k = native_colors(cx);
        let windows = c.usage.clone().unwrap_or_default();
        if windows.is_empty() {
            return div()
                .when(c.login == "logged_in", |d| {
                    d.n_footnote().text_color(k.secondary).child(m::USAGE_NONE)
                })
                .into_any_element();
        }
        div()
            .flex()
            .flex_col()
            .gap(space::S12)
            .child(
                div()
                    .flex()
                    .items_baseline()
                    .gap(space::S8)
                    .child(div().n_headline().child(m::USAGE_TITLE))
                    .child(
                        div()
                            .n_footnote()
                            .text_color(k.secondary)
                            .child(m::USAGE_HINT),
                    )
                    .child(div().flex_1())
                    .children(c.usage_at.map(|at| {
                        div().n_footnote().text_color(k.secondary).child(fill(
                            m::ACCOUNTS_USAGE_AT,
                            &[("ago", &model::ago(now.saturating_sub(at)))],
                        ))
                    })),
            )
            .children(windows.into_iter().map(|w| {
                let (title, rest) = usage_words(&w);
                let warn = w.used_pct >= 80.;
                div()
                    .flex()
                    .flex_col()
                    .gap(space::S6)
                    .child(
                        div()
                            .flex()
                            .items_baseline()
                            .gap(space::S8)
                            .child(div().n_headline().child(title))
                            .children(
                                rest.map(|r| div().n_footnote().text_color(k.secondary).child(r)),
                            )
                            .child(div().flex_1())
                            .child(
                                div()
                                    .n_title3()
                                    .font_weight(gpui::FontWeight::SEMIBOLD)
                                    .n_figures()
                                    .text_color(if warn { k.amber } else { k.label })
                                    .child(format!("{}%", w.used_pct.round() as i64)),
                            ),
                    )
                    .child(progress((w.used_pct / 100.) as f32, cx))
            }))
            .into_any_element()
    }

    fn login_panel(
        &self,
        f: &LoginFlow,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let k = native_colors(cx);
        let view = cx.entity();
        let close_label = if f.ended() { m::CLOSE } else { m::CANCEL };
        let close = push_button(
            "login-close",
            close_label,
            PushStyle::Outline,
            PushSize::Small,
            cx,
        )
        .on_click(move |_, _, cx| view.update(cx, |v, cx| v.close_login(cx)));
        let st = f.status.as_ref();
        let url = st.and_then(|s| s.url.clone());
        let link_row = |url: String, cx: &mut Context<Self>| {
            let (open, copy) = (url.clone(), url.clone());
            div()
                .flex()
                .items_center()
                .gap(space::S8)
                .child(
                    push_button(
                        "login-open",
                        m::OPEN_LINK,
                        PushStyle::Default,
                        PushSize::Small,
                        cx,
                    )
                    .on_click(move |_, _, cx| cx.open_url(&open)),
                )
                .child(
                    push_button(
                        "login-copy",
                        m::COPY_LINK,
                        PushStyle::Outline,
                        PushSize::Small,
                        cx,
                    )
                    .on_click(move |_, _, cx| {
                        cx.write_to_clipboard(ClipboardItem::new_string(copy.clone()))
                    }),
                )
                .child(
                    TruncText::new(url)
                        .flex_1()
                        .n_footnote()
                        .text_color(k.secondary),
                )
        };
        let step = |t: &'static str| div().n_headline().child(t);
        let mut body = div().flex().flex_col().gap(space::S12);
        if let Some(e) = &f.error {
            body = body.child(
                Alert::new(Sym::Failed, m::LOGIN_FAILED)
                    .tone(AlertTone::Destructive)
                    .description(e.clone()),
            );
        } else {
            match f.state() {
                phase::OPEN_URL => {
                    let view = cx.entity();
                    let sending = f.sending;
                    body = body
                        .child(step(m::LOGIN_STEP_OPEN))
                        .children(url.map(|u| link_row(u, cx)))
                        .child(step(m::LOGIN_STEP_PASTE))
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap(space::S8)
                                .child(
                                    div().flex_1().min_w(gpui::Pixels::ZERO).child(
                                        NativeTextField::new("login-code", &self.acct.code)
                                            .full_width(),
                                    ),
                                )
                                .child(
                                    crate::ui::native::loading_push(
                                        push_button(
                                            "login-submit",
                                            m::SUBMIT_CODE,
                                            PushStyle::Default,
                                            PushSize::Small,
                                            cx,
                                        ),
                                        sending,
                                        PushSize::Small,
                                    )
                                    .on_click(
                                        move |_, window, cx| {
                                            view.update(cx, |v, cx| v.send_code(window, cx))
                                        },
                                    ),
                                ),
                        );
                }
                phase::DEVICE_CODE => {
                    let code = st.and_then(|s| s.user_code.clone()).unwrap_or_default();
                    let copy = code.clone();
                    body = body
                        .child(step(m::LOGIN_STEP_OPEN))
                        .children(url.map(|u| link_row(u, cx)))
                        .child(step(m::LOGIN_STEP_CODE))
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap(space::S12)
                                .child(
                                    div()
                                        .px(space::S12)
                                        .py(space::S6)
                                        .rounded(radius::CONTROL)
                                        .bg(k.fill)
                                        .n_style(text::KPI_COMPACT)
                                        .font_weight(gpui::FontWeight::SEMIBOLD)
                                        .n_figures()
                                        .child(code),
                                )
                                .child(
                                    push_button(
                                        "login-copy-code",
                                        m::COPY_CODE,
                                        PushStyle::Outline,
                                        PushSize::Small,
                                        cx,
                                    )
                                    .on_click(
                                        move |_, _, cx| {
                                            cx.write_to_clipboard(ClipboardItem::new_string(
                                                copy.clone(),
                                            ))
                                        },
                                    ),
                                ),
                        )
                        .child(
                            div()
                                .n_footnote()
                                .text_color(k.secondary)
                                .child(m::LOGIN_WAITING_DEVICE),
                        );
                }
                phase::DONE => {
                    let who = st
                        .and_then(|s| s.account.as_ref())
                        .and_then(|a| a.email.clone().or(a.plan.clone()))
                        .unwrap_or_default();
                    body = body.child(
                        Alert::new(Sym::Done, fill(m::LOGIN_DONE, &[("who", &who)]))
                            .tone(AlertTone::Neutral),
                    );
                }
                phase::FAILED => {
                    body = body.child(
                        Alert::new(Sym::Failed, m::LOGIN_FAILED)
                            .tone(AlertTone::Destructive)
                            .description(st.and_then(|s| s.error.clone()).unwrap_or_default()),
                    );
                }
                phase::CANCELLED => {
                    body = body.child(
                        div()
                            .n_body()
                            .text_color(k.secondary)
                            .child(m::LOGIN_CANCELLED),
                    );
                }
                state => {
                    let words = if state == phase::CHECKING {
                        m::LOGIN_CHECKING
                    } else {
                        m::LOGIN_STARTING
                    };
                    body = body.child(
                        div()
                            .flex()
                            .items_center()
                            .gap(space::S8)
                            .n_body()
                            .text_color(k.secondary)
                            .child(crate::ui::native::spinner())
                            .child(words),
                    );
                }
            }
        }
        div()
            .p(space::S16)
            .rounded(radius::CONTAINER)
            .border_1()
            .border_color(k.separator)
            .bg(k.group)
            .flex()
            .flex_col()
            .gap(space::S12)
            .child(body)
            .child(div().flex().justify_end().child(close))
            .into_any_element()
    }

    // -----------------------------------------------------------------------
    // API 密钥

    pub(super) fn keys_page(
        &self,
        now: u64,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let k = native_colors(cx);
        let secrets = self
            .acct
            .secrets
            .as_ref()
            .filter(|_| self.acct.secrets_for.as_deref() == self.selected.as_deref());
        let Some(s) = secrets else {
            return match &self.acct.secrets_error {
                Some(e) => Alert::new(Sym::Failed, m::KEYS_LOADING_FAILED)
                    .tone(AlertTone::Destructive)
                    .description(e.clone())
                    .into_any_element(),
                None => div()
                    .flex()
                    .flex_col()
                    .gap(space::S12)
                    .children((0..4).map(|_| skeleton(gpui::relative(1.), height::TABLE_ROW)))
                    .into_any_element(),
            };
        };
        let view = cx.entity();
        let can_add = s.public_key.is_some();
        let add = crate::ui::native::disabled_push(
            push_button(
                "keys-add",
                m::KEYS_ADD,
                PushStyle::Default,
                PushSize::Small,
                cx,
            ),
            !can_add,
            cx,
        )
        .on_click(move |_, window, cx| view.update(cx, |v, cx| v.open_key_form(None, window, cx)));
        let names = |ids: &[String]| -> (Vec<AnyElement>, String) {
            if ids.is_empty() {
                return (vec![], m::KEYS_ALL_MEMBERS.to_string());
            }
            let members: Vec<_> = ids
                .iter()
                .filter_map(|id| self.services.provider.member(id))
                .collect();
            let avatars = members
                .iter()
                .take(3)
                .map(|x| MemberAvatar::new(Some(x), avatar::XS).into_any_element())
                .collect();
            (
                avatars,
                members
                    .iter()
                    .map(|x| x.name.clone())
                    .collect::<Vec<_>>()
                    .join("、"),
            )
        };
        let rows: Vec<Vec<AnyElement>> = s
            .secrets
            .iter()
            .map(|x| {
                let (avatars, who) = names(&x.members);
                let view = cx.entity();
                let (del, rep) = (x.name.clone(), x.name.clone());
                let view2 = view.clone();
                let more = crate::views::settings::widgets::more_button(
                    SharedString::from(format!("key-more-{}", x.name)),
                    window,
                    cx,
                )
                .dropdown_menu(move |menu, _, _| {
                    let (view, view2, del, rep) =
                        (view.clone(), view2.clone(), del.clone(), rep.clone());
                    menu.item(
                        PopupMenuItem::new(m::KEYS_REPLACE).on_click(move |_, window, cx| {
                            let n = rep.clone();
                            view2.update(cx, |v, cx| v.open_key_form(Some(&n), window, cx))
                        }),
                    )
                    .item(
                        PopupMenuItem::new(m::KEYS_DELETE).on_click(move |_, _, cx| {
                            let n = del.clone();
                            view.update(cx, |v, cx| v.delete_key(n, cx))
                        }),
                    )
                });
                vec![
                    div()
                        .n_style(text::MONO)
                        .child(x.name.clone())
                        .into_any_element(),
                    div()
                        .flex()
                        .items_center()
                        .gap(space::S8)
                        .child(div().flex().gap(space::S2).children(avatars))
                        .child(TruncText::new(who))
                        .into_any_element(),
                    div()
                        .text_color(k.secondary)
                        .child(model::ago(now.saturating_sub(x.updated_at)))
                        .into_any_element(),
                    div()
                        .n_style(text::MONO)
                        .text_color(k.secondary)
                        .child(if x.last4.is_empty() {
                            "••••".to_string()
                        } else {
                            format!("••••{}", x.last4)
                        })
                        .into_any_element(),
                    more.into_any_element(),
                ]
            })
            .collect();
        let columns = [
            Column::flex(m::KEYS_COL_NAME),
            Column::fixed(m::KEYS_COL_WHO, t::COL_MEM),
            Column::fixed(m::KEYS_COL_UPDATED, t::COL_TOKENS),
            Column::fixed(m::KEYS_COL_VALUE, t::COL_TOKENS),
            Column::fixed("", height::LG),
        ];
        div()
            .w_full()
            .flex()
            .flex_col()
            .gap(space::S16)
            .when(!can_add, |d| {
                d.child(Alert::new(Sym::Warning, m::KEYS_NO_PUBLIC_KEY).tone(AlertTone::Warning))
            })
            .child(
                div()
                    .flex()
                    .items_center()
                    .child(
                        div()
                            .flex_1()
                            .n_body()
                            .text_color(k.secondary)
                            .child(fill(m::KEYS_COUNT, &[("n", &s.secrets.len().to_string())])),
                    )
                    .children(self.acct.notice.clone().map(|n| {
                        div()
                            .mr(space::S12)
                            .n_footnote()
                            .text_color(k.secondary)
                            .child(n)
                    }))
                    .child(add),
            )
            .when(self.acct.form_open, |d| d.child(self.key_form(cx)))
            .child(if s.secrets.is_empty() {
                Empty::new(Sym::Key, m::KEYS_EMPTY)
                    .description(m::KEYS_EMPTY_DESC)
                    .dashed()
                    .into_any_element()
            } else {
                table(&columns, rows, true, m::KEYS_EMPTY, None, cx).into_any_element()
            })
            .child(
                Alert::new(Sym::Lock, m::KEYS_NOTE_TITLE)
                    .tone(AlertTone::Neutral)
                    .description(m::KEYS_NOTE_DESC),
            )
            .into_any_element()
    }

    fn key_form(&self, cx: &mut Context<Self>) -> AnyElement {
        let k = native_colors(cx);
        let bots: Vec<_> = self
            .services
            .provider
            .members()
            .into_iter()
            .filter(|x| x.kind == MemberKind::Bot && !x.deactivated)
            .collect();
        let all = self.acct.form_members.is_empty();
        let view = cx.entity();
        let all_box = NativeCheckbox::new("key-all", all)
            .label(m::KEYS_ALL_MEMBERS)
            .on_change(move |on, _, cx| {
                view.update(cx, |v, cx| {
                    if on {
                        v.acct.form_members.clear();
                    }
                    cx.notify();
                })
            });
        let boxes: Vec<AnyElement> = bots
            .iter()
            .map(|b| {
                let id = b.id.to_string();
                let on = self.acct.form_members.iter().any(|m| m == &id);
                let view = cx.entity();
                NativeCheckbox::new(SharedString::from(format!("key-member-{id}")), on)
                    .label(b.name.clone())
                    .on_change(move |on, _, cx| {
                        let id = id.clone();
                        view.update(cx, |v, cx| {
                            v.acct.form_members.retain(|m| m != &id);
                            if on {
                                v.acct.form_members.push(id);
                            }
                            cx.notify();
                        })
                    })
                    .into_any_element()
            })
            .collect();
        let (cancel_view, save_view) = (cx.entity(), cx.entity());
        let label = |t: &'static str| {
            div()
                .w(t::COL_COUNT)
                .flex_none()
                .n_body()
                .text_color(k.secondary)
                .child(t)
        };
        card_surface(cx)
            .p(space::S24)
            .flex()
            .flex_col()
            .gap(space::S16)
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(space::S4)
                    .child(
                        div()
                            .n_title3()
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .child(m::KEYS_FORM_TITLE),
                    )
                    .child(
                        div()
                            .n_body()
                            .text_color(k.secondary)
                            .child(m::KEYS_FORM_DESC),
                    ),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(space::S12)
                    .child(label(m::KEYS_FIELD_NAME))
                    .child(
                        div().flex_1().min_w(gpui::Pixels::ZERO).child(
                            NativeTextField::new("key-name", &self.acct.key_name).full_width(),
                        ),
                    ),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(space::S12)
                    .child(label(m::KEYS_FIELD_VALUE))
                    .child(div().flex_1().min_w(gpui::Pixels::ZERO).child(
                        NativeTextField::new("key-value", &self.acct.key_value).full_width(),
                    )),
            )
            .child(
                div()
                    .flex()
                    .items_start()
                    .gap(space::S12)
                    .child(label(m::KEYS_FIELD_WHO))
                    .child(
                        div()
                            .flex_1()
                            .flex()
                            .flex_wrap()
                            .gap(space::S16)
                            .child(all_box)
                            .children(boxes),
                    ),
            )
            .children(
                self.acct
                    .form_error
                    .clone()
                    .map(|e| Alert::new(Sym::Failed, e).tone(AlertTone::Destructive)),
            )
            .child(
                div()
                    .flex()
                    .justify_end()
                    .gap(space::S8)
                    .child(
                        push_button(
                            "key-cancel",
                            m::CANCEL,
                            PushStyle::Outline,
                            PushSize::Small,
                            cx,
                        )
                        .on_click(move |_, _, cx| {
                            cancel_view.update(cx, |v, cx| {
                                v.acct.form_open = false;
                                cx.notify();
                            })
                        }),
                    )
                    .child(
                        crate::ui::native::loading_push(
                            push_button(
                                "key-save",
                                m::KEYS_SAVE,
                                PushStyle::Default,
                                PushSize::Small,
                                cx,
                            ),
                            self.acct.saving,
                            PushSize::Small,
                        )
                        .on_click(move |_, window, cx| {
                            save_view.update(cx, |v, cx| v.save_key(window, cx))
                        }),
                    ),
            )
            .into_any_element()
    }
}

/// When a window resets: the time today, else the day and time.
fn reset_time(ms: u64) -> String {
    use chrono::TimeZone as _;
    let Some(t) = chrono::Local.timestamp_millis_opt(ms as i64).single() else {
        return String::new();
    };
    if t.date_naive() == crate::selftest::clock::local_now().date_naive() {
        t.format("%H:%M").to_string()
    } else {
        fill(
            m::USAGE_DAY_TIME,
            &[
                ("m", &t.format("%-m").to_string()),
                ("d", &t.format("%-d").to_string()),
                ("time", &t.format("%H:%M").to_string()),
            ],
        )
    }
}

/// A usage window's title in the app's words, and when it resets: the time the server gave, or
/// the CLI's own words after "resets".
fn usage_words(w: &chat_core::proto::machine::UsageWindow) -> (String, Option<String>) {
    let label = w.label.clone().unwrap_or_default();
    let (head, tail) = match label.split_once(" · ") {
        Some((a, b)) => (a.to_string(), Some(b.to_string())),
        None => (label.clone(), None),
    };
    let title = match w.kind.as_str() {
        "session" => m::USAGE_SESSION.to_string(),
        "week" => m::USAGE_WEEK.to_string(),
        "week_model" => match head.split_once('(').and_then(|(_, r)| r.split_once(')')) {
            Some((model, _)) => fill(m::USAGE_WEEK_MODEL, &[("model", model.trim())]),
            None => head.clone(),
        },
        "primary" | "secondary" if !head.is_empty() => fill(
            m::USAGE_WINDOW,
            &[(
                "span",
                &head.replace(" hours", " 小时").replace(" days", " 天"),
            )],
        ),
        "primary" => m::USAGE_PRIMARY.to_string(),
        "secondary" => m::USAGE_SECONDARY.to_string(),
        _ => head.clone(),
    };
    let rest = match w.resets_at {
        Some(t) => Some(fill(m::USAGE_RESETS, &[("when", &reset_time(t))])),
        None => tail
            .map(|t| t.trim_start_matches("resets").trim().to_string())
            .filter(|t| !t.is_empty())
            .map(|t| fill(m::USAGE_RESETS_AT, &[("when", &t)])),
    };
    (title, rest)
}

#[cfg(test)]
mod tests {
    #[test]
    fn key_names_are_environment_variable_names() {
        assert!(super::valid_key_name("OPENAI_API_KEY"));
        assert!(super::valid_key_name("_X1"));
        assert!(!super::valid_key_name("1X"));
        assert!(!super::valid_key_name("openai_api_key"));
        assert!(!super::valid_key_name(""));
        assert_eq!(super::api_key_name("codex"), "OPENAI_API_KEY");
        assert_eq!(super::api_key_name("claude"), "ANTHROPIC_API_KEY");
    }

    #[test]
    fn usage_windows_read_in_the_apps_words() {
        use chat_core::proto::machine::UsageWindow;
        let w = |kind: &str, label: Option<&str>| UsageWindow {
            kind: kind.into(),
            label: label.map(str::to_string),
            used_pct: 10.,
            resets_at: None,
        };
        assert_eq!(
            super::usage_words(&w(
                "session",
                Some("Current session · resets Oct 1 at 3:30pm")
            )),
            ("5 小时窗口".into(), Some("重置于 Oct 1 at 3:30pm".into()))
        );
        assert_eq!(
            super::usage_words(&w("week_model", Some("Current week (Opus) · resets Thu"))).0,
            "本周 · Opus"
        );
        assert_eq!(
            super::usage_words(&w("primary", Some("5 hours"))).0,
            "5 小时窗口"
        );
        assert_eq!(
            super::usage_words(&w("secondary", Some("7 days"))).0,
            "7 天窗口"
        );
    }
}
