//! The login window (COMPLETENESS 2.20): a small window of its own (480 x
//! 480, not resizable) whose surface is the window itself, no card. One
//! centered column: the app icon 56, the title, one muted line, the invite
//! code as eight one-time-code slots (4 – 4, `CodeInput`), the error under
//! the slots (a destructive icon and neutral text), the full-width `继续`
//! (lg; primary at 30 % until all eight slots are filled; a spinner and
//! `正在验证…` while the code is checked) and a small ghost link
//! `使用其他服务器…` (also in the app menu). A custom server shows its host
//! with `更改` in that link row. The server step uses the same column. A
//! refused code stays in the slots, selected, with the error border; no
//! alert. A pasted login credential (`tc_…`) or a legacy 12-character code
//! is accepted in the same place and shows as one locked line. The server
//! URL defaults to the production URL from product.json, then staging.
//!
//! `用邮箱登录` in the link row switches the column to the email steps: the
//! address (`发送验证码`), then the 6-digit code in six slots (3 – 3), which
//! signs in as soon as all six are typed. `使用邀请码登录` goes back.

use gpui::{
    AppContext, Context, Entity, Focusable as _, InteractiveElement as _, IntoElement,
    ParentElement, Render, SharedString, Styled, Subscription, Window, div, px,
};
use gpui_component::input::{InputEvent, InputState};

use crate::{
    app::{Changes, Services},
    copy::{common, login as l, overlays as o},
    data::{Change, Credential, EmailError, LoginError, LoginRequest, SessionState},
    ui::{
        self, Sym,
        native::{
            CodeInput, CodeInputEvent, FieldSize, NativeTextField, NativeType as _, PushSize,
            PushStyle, borderless_text,
            code_input::code_chars,
            code_input_width, loading_push, native_colors,
            overlays::{app_icon, spot},
            push_button, waiting_push,
        },
        sf_symbols::SymSize,
    },
};

pub fn default_server_url() -> &'static str {
    let production = env!("PRODUCT_PRODUCTION_URL");
    if production.is_empty() {
        env!("PRODUCT_STAGING_URL")
    } else {
        production
    }
}

/// The servers a build knows by name, for the login window's server step: 正式服务器 and
/// 测试服务器, each only when the build has its URL, and the second only when it is another
/// server than the first. A build without URLs (an open-source build) has none.
pub fn server_presets(
    production: &'static str,
    staging: &'static str,
) -> Vec<(&'static str, &'static str, &'static str)> {
    let mut out = Vec::new();
    if !production.is_empty() {
        out.push(("login-server-production", l::SERVER_PRODUCTION, production));
    }
    if !staging.is_empty() && staging != production {
        out.push(("login-server-staging", l::SERVER_STAGING, staging));
    }
    out
}

/// Characters in an invite code (`XXXX-XXXX`, server `newInviteCode`).
pub const CODE_LEN: usize = 8;
/// Characters of a code made before the 8-character format
/// (`XXXX-XXXX-XXXX`); the server still redeems them.
pub const LEGACY_CODE_LEN: usize = 12;
/// Characters per group of a code (`XXXX-XXXX`).
const GROUP: usize = 4;
/// Digits of an email code, in groups of three (`123-456`).
pub const EMAIL_CODE_LEN: usize = 6;
const EMAIL_GROUP: usize = 3;

/// Which step the login column shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Step {
    /// The invite code (or a pasted credential).
    Invite,
    /// The sign-in email.
    Email,
    /// The code sent to the email.
    EmailCode,
}
/// The login window: small, fixed, not resizable.
pub const WINDOW_SIZE: gpui::Size<gpui::Pixels> = gpui::Size {
    width: px(480.),
    height: px(480.),
};

gpui::actions!(login, [UseOtherServer]);

/// The login column: as wide as the eight code slots.
fn column_width() -> gpui::Pixels {
    code_input_width(CODE_LEN, GROUP)
}

/// What the code field holds.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CodeEntry {
    /// The invite code characters typed so far, upper case, at most 8.
    Code(String),
    /// A pasted 12-character code of the earlier format, upper case.
    Legacy(String),
    /// A pasted login credential.
    Token(String),
}

impl CodeEntry {
    /// Reads the field: a `tc_` credential, a pasted legacy code of exactly
    /// 12 characters, or the first 8 letters and digits of an invite code.
    pub fn parse(text: &str) -> Self {
        let text = text.trim();
        if text.starts_with("tc_") {
            return CodeEntry::Token(text.to_string());
        }
        let chars = code_chars(text);
        if chars.len() == LEGACY_CODE_LEN {
            return CodeEntry::Legacy(chars);
        }
        CodeEntry::Code(chars.chars().take(CODE_LEN).collect())
    }

    /// The credential to send, once the field is complete.
    pub fn credential(&self) -> Option<Credential> {
        let grouped = |c: &str| {
            let groups: Vec<String> = c
                .as_bytes()
                .chunks(GROUP)
                .map(|g| String::from_utf8_lossy(g).into_owned())
                .collect();
            Credential::InviteCode(groups.join("-"))
        };
        match self {
            CodeEntry::Token(t) => Credential::from_input(t),
            CodeEntry::Legacy(c) => Some(grouped(c)),
            CodeEntry::Code(c) if c.chars().count() == CODE_LEN => Some(grouped(c)),
            CodeEntry::Code(_) => None,
        }
    }

    /// The locked line of a value kept whole (the secret is never drawn).
    fn locked(&self) -> Option<SharedString> {
        match self {
            CodeEntry::Token(_) => Some(l::CODE_TOKEN.into()),
            CodeEntry::Legacy(_) => Some(l::CODE_LEGACY.into()),
            CodeEntry::Code(_) => None,
        }
    }
}

/// A paste the slots keep whole: a login credential, or a code of the
/// earlier 12-character format. Typing stops at the eighth slot, so only a
/// paste reaches them.
fn keeps_whole(text: &str) -> bool {
    !matches!(CodeEntry::parse(text), CodeEntry::Code(_))
}

/// The host part of a server URL, for the server line (`team.joye.co`).
pub fn server_host(url: &str) -> &str {
    let rest = url
        .trim()
        .trim_start_matches("https://")
        .trim_start_matches("http://");
    rest.split('/').next().unwrap_or(rest)
}

pub struct LoginView {
    services: Services,
    server: Entity<InputState>,
    code: Entity<CodeInput>,
    email: Entity<InputState>,
    email_code: Entity<CodeInput>,
    step: Step,
    /// The address the last code was sent to.
    sent_to: String,
    /// `发送验证码` is waiting for the server.
    sending: bool,
    /// The last refusal of an email request (sending a code).
    email_error: Option<EmailError>,
    /// The server field is open (`更改`, or after an unreachable server).
    server_open: bool,
    /// Drawn inside a main window (signed out there): a sign-out moves to
    /// the login window.
    embedded: bool,
    /// The last refusal, shown under the field until the field changes.
    error: Option<LoginError>,
    /// The field text the refusal was about.
    error_for: String,
    /// Draws the signing-in state (a screenshot scene).
    preview_busy: bool,
    _subscriptions: Vec<Subscription>,
}

impl LoginView {
    /// One of the login fields has the keyboard focus (it took it when the
    /// view was made, drawn or not).
    pub fn has_focus(&self, window: &Window, cx: &gpui::App) -> bool {
        self.code.read(cx).is_focused(window, cx)
            || self.server.read(cx).focus_handle(cx).is_focused(window)
            || self.email.read(cx).focus_handle(cx).is_focused(window)
            || self.email_code.read(cx).is_focused(window, cx)
    }

    pub fn new(services: Services, window: &mut Window, cx: &mut Context<Self>) -> Self {
        // The server in use (after 服务器 › 测试服务器, or a sign-out), else
        // the default one.
        let start_url = services
            .provider
            .current_server()
            .unwrap_or_else(|| default_server_url().to_string());
        let server = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder(l::SERVER_PLACEHOLDER)
                .default_value(start_url)
        });
        let code = cx.new(|cx| CodeInput::new(CODE_LEN, GROUP, window, cx).keep_raw(keeps_whole));
        let email = cx.new(|cx| InputState::new(window, cx).placeholder(l::EMAIL_PLACEHOLDER));
        let email_code = cx.new(|cx| CodeInput::new(EMAIL_CODE_LEN, EMAIL_GROUP, window, cx));
        // The first slot has the caret when the view opens.
        code.read(cx).focus_handle(cx).focus(window, cx);
        let subscriptions =
            vec![
                cx.subscribe_in(
                    &code,
                    window,
                    |this, _, event: &CodeInputEvent, window, cx| match event {
                        CodeInputEvent::Submit => this.submit(window, cx),
                        CodeInputEvent::Change => {
                            this.clear_stale_error(cx);
                            this.sync_code(cx);
                            cx.notify();
                        }
                    },
                ),
                cx.subscribe_in(&email, window, |this, _, event: &InputEvent, window, cx| {
                    match event {
                        InputEvent::PressEnter { .. } => this.continue_(window, cx),
                        InputEvent::Change => {
                            this.email_error = None;
                            cx.notify();
                        }
                        _ => {}
                    }
                }),
                cx.subscribe_in(
                    &email_code,
                    window,
                    |this, _, event: &CodeInputEvent, window, cx| match event {
                        CodeInputEvent::Submit => this.submit(window, cx),
                        CodeInputEvent::Change => {
                            this.clear_stale_error(cx);
                            this.email_error = None;
                            this.sync_code(cx);
                            // All six digits: sign in at once, as a one-time code does.
                            if this.email_code_digits(cx).len() == EMAIL_CODE_LEN
                                && this.error.is_none()
                            {
                                this.submit(window, cx);
                            }
                            cx.notify();
                        }
                    },
                ),
                cx.subscribe_in(
                    &server,
                    window,
                    |this, _, event: &InputEvent, window, cx| match event {
                        InputEvent::PressEnter { .. } => this.continue_(window, cx),
                        InputEvent::Change => {
                            this.clear_stale_error(cx);
                            cx.notify();
                        }
                        _ => {}
                    },
                ),
                cx.subscribe_in(
                    &services.hub,
                    window,
                    |this, _, changes: &Changes, window, cx| {
                        if !changes.any(|c| matches!(c, Change::Session)) {
                            return;
                        }
                        let session = this.services.provider.session();
                        // A switch to a server without a saved session: the
                        // login window shows that server.
                        if matches!(session, SessionState::SignedOut { last_error: None })
                            && let Some(url) = this.services.provider.current_server()
                            && this.server.read(cx).value().trim() != url
                        {
                            this.server
                                .update(cx, |st, cx| st.set_value(url, window, cx));
                        }
                        match &session {
                            SessionState::SignedOut {
                                last_error: Some(LoginError::ServerUnreachable),
                            } => {
                                this.server_open = true;
                                this.server.read(cx).focus_handle(cx).focus(window, cx);
                            }
                            // A wrong code stays in the slots, selected: the
                            // next character typed starts a new code (§8.3).
                            SessionState::SignedOut {
                                last_error: Some(LoginError::BadCredential),
                            } => {
                                if this.step == Step::EmailCode {
                                    this.email_code.update(cx, |c, cx| c.select_all(window, cx));
                                } else {
                                    this.code.update(cx, |c, cx| c.select_all(window, cx));
                                }
                            }
                            _ => {}
                        }
                        // A refusal marks the field and says why under it.
                        if let SessionState::SignedOut {
                            last_error: Some(e),
                        } = session
                        {
                            this.error = Some(e);
                            this.error_for = this.field_text(cx);
                        }
                        this.sync_code(cx);
                        if this.embedded && matches!(session, SessionState::SignedOut { .. }) {
                            move_to_login_window(this.services.clone(), window, cx);
                        }
                        cx.notify();
                    },
                ),
            ];
        Self {
            services,
            server,
            code,
            email,
            email_code,
            step: Step::Invite,
            sent_to: String::new(),
            sending: false,
            email_error: None,
            server_open: false,
            embedded: true,
            error: None,
            error_for: String::new(),
            preview_busy: false,
            _subscriptions: subscriptions,
        }
    }

    /// Opens the server field and moves the focus into it with the whole
    /// URL selected, as a macOS field does when it takes the focus.
    fn open_server_field(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.server_open = true;
        self.server.update(cx, |st, cx| {
            let end = st.value().len();
            st.focus(window, cx);
            st.set_selected_range(0..end, cx);
        });
        self.sync_code(cx);
        cx.notify();
    }

    /// The text of the field that is up: the server, the code, or the email code.
    fn field_text(&self, cx: &gpui::App) -> String {
        if self.server_open {
            self.server.read(cx).value().to_string()
        } else if self.step == Step::EmailCode {
            self.email_code.read(cx).value(cx).to_string()
        } else {
            self.code.read(cx).value(cx).to_string()
        }
    }

    /// The digits typed in the email code slots.
    fn email_code_digits(&self, cx: &gpui::App) -> String {
        self.email_code
            .read(cx)
            .value(cx)
            .chars()
            .filter(char::is_ascii_digit)
            .take(EMAIL_CODE_LEN)
            .collect()
    }

    /// The address typed, trimmed and in lower case.
    fn email_text(&self, cx: &gpui::App) -> String {
        self.email.read(cx).value().trim().to_lowercase()
    }

    /// Shows `step` with the focus in its field.
    pub fn go_to(&mut self, step: Step, window: &mut Window, cx: &mut Context<Self>) {
        self.step = step;
        self.server_open = false;
        self.error = None;
        self.email_error = None;
        match step {
            Step::Invite => self.code.read(cx).focus_handle(cx).focus(window, cx),
            Step::Email => self.email.update(cx, |st, cx| {
                let end = st.value().len();
                st.focus(window, cx);
                st.set_selected_range(0..end, cx);
            }),
            Step::EmailCode => {
                self.email_code
                    .update(cx, |c, cx| c.set_value("", window, cx));
                let code = self.email_code.clone();
                code.read(cx).focus_handle(cx).focus(window, cx);
            }
        }
        self.sync_code(cx);
        cx.notify();
    }

    /// `发送验证码` (and `重新发送`): asks the server for a code at the address.
    fn send_email_code(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.sending {
            return;
        }
        let email = self.email_text(cx);
        if !chat_core::proto::is_email_address(&email) {
            self.email_error = Some(EmailError::BadAddress);
            cx.notify();
            return;
        }
        let server_url = self.server.read(cx).value().trim().to_string();
        if server_url.is_empty() {
            self.open_server_field(window, cx);
            return;
        }
        self.sending = true;
        self.email_error = None;
        let reply = self
            .services
            .provider
            .request_sign_in_code(&server_url, &email);
        cx.spawn_in(window, async move |this, cx| {
            let result = reply.await.unwrap_or(Err(EmailError::Other));
            let _ = this.update_in(cx, |this, window, cx| {
                this.sending = false;
                match result {
                    Ok(()) => {
                        this.sent_to = email;
                        this.go_to(Step::EmailCode, window, cx);
                    }
                    Err(e) => this.email_error = Some(e),
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    /// An edit of the field the refusal was about takes the error away.
    fn clear_stale_error(&mut self, cx: &gpui::App) {
        if self.error.is_some() && self.field_text(cx) != self.error_for {
            self.error = None;
        }
    }

    /// Hands the code slots their locked line and error border.
    fn sync_code(&mut self, cx: &mut Context<Self>) {
        let locked = self.input(cx).locked();
        let error = self.error.is_some() && !self.server_open;
        let (invite_error, email_error) = match self.step {
            Step::EmailCode => (false, error),
            _ => (error, false),
        };
        self.code.update(cx, |c, cx| {
            c.set_locked(locked, cx);
            c.set_error(invite_error, cx);
        });
        self.email_code
            .update(cx, |c, cx| c.set_error(email_error, cx));
    }

    /// Draws the signing-in state with `code` typed, for a screenshot scene.
    pub fn preview_signing_in(&mut self, code: &str, window: &mut Window, cx: &mut Context<Self>) {
        self.preview(code, false, window, cx);
        self.preview_busy = true;
        cx.notify();
    }

    fn input(&self, cx: &gpui::App) -> CodeEntry {
        CodeEntry::parse(&self.code.read(cx).value(cx))
    }

    /// Fills the form for a screenshot scene: the code typed so far, the
    /// server field open or not, and focus in the code slots.
    pub fn preview(
        &mut self,
        code: &str,
        server_open: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.code.update(cx, |c, cx| c.set_value(code, window, cx));
        // An open server field has the focus and its URL selected, as after
        // `使用其他服务器`.
        if server_open {
            self.open_server_field(window, cx);
        } else {
            self.server_open = false;
            self.code.read(cx).focus_handle(cx).focus(window, cx);
            self.sync_code(cx);
            cx.notify();
        }
    }

    fn submit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if matches!(self.services.provider.session(), SessionState::SigningIn) {
            return;
        }
        let server_url = self.server.read(cx).value().trim().to_string();
        let credential = match self.step {
            Step::Invite => self.input(cx).credential(),
            Step::Email => {
                self.send_email_code(window, cx);
                return;
            }
            Step::EmailCode => {
                let code = self.email_code_digits(cx);
                (code.len() == EMAIL_CODE_LEN).then(|| Credential::EmailCode {
                    email: self.sent_to.clone(),
                    code,
                })
            }
        };
        let Some(credential) = credential else {
            return;
        };
        // No server: the server step, whose 继续 is off until it has one.
        if server_url.is_empty() {
            self.open_server_field(window, cx);
            return;
        }
        // A new try takes the last refusal away until the server answers.
        self.error = None;
        self.sync_code(cx);
        self.services.provider.login(LoginRequest {
            server_url,
            credential,
            device_name: crate::provider::session::device_name(),
        });
        // A wrong code stays in the slots so it can be checked (§8.3); the
        // field lives only in this view and is never logged.
        cx.notify();
    }

    /// The email steps for a screenshot scene: the address typed, and with
    /// `code` the code step after a code was sent, with those digits typed
    /// (`000000` is refused).
    pub fn preview_email(
        &mut self,
        email: &str,
        code: Option<&str>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.email
            .update(cx, |st, cx| st.set_value(email.to_string(), window, cx));
        match code {
            None => self.go_to(Step::Email, window, cx),
            Some(code) => {
                self.sent_to = email.to_string();
                self.go_to(Step::EmailCode, window, cx);
                self.email_code
                    .update(cx, |c, cx| c.set_value(code, window, cx));
                if self.email_code_digits(cx).len() == EMAIL_CODE_LEN {
                    self.submit(window, cx);
                }
            }
        }
        cx.notify();
    }

    /// Sets a custom server, for a screenshot scene.
    pub fn preview_server(&mut self, url: &str, window: &mut Window, cx: &mut Context<Self>) {
        self.server
            .update(cx, |st, cx| st.set_value(url.to_string(), window, cx));
        cx.notify();
    }

    /// Fills the code and submits it, for the error screenshot scene.
    pub fn preview_submit(&mut self, code: &str, window: &mut Window, cx: &mut Context<Self>) {
        self.preview(code, false, window, cx);
        self.submit(window, cx);
    }

    /// 继续: on the code step it joins; on the server step it goes back to
    /// the code, or joins when the code is complete.
    fn continue_(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.server_open && (self.step != Step::Invite || self.input(cx).credential().is_none())
        {
            self.close_server_field(window, cx);
        } else {
            self.submit(window, cx);
        }
    }

    fn close_server_field(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let step = self.step;
        self.go_to(step, window, cx);
    }

    /// The link row under 继续: on the code step `使用其他服务器…`, or a
    /// custom server's host with `更改`; on the server step `取消`.
    fn render_link_row(
        &self,
        server_step: bool,
        custom: Option<SharedString>,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        use crate::ui::native::tokens::space;
        let k = native_colors(cx);
        let link = |id: &'static str, label: &'static str, cx: &mut Context<Self>| {
            borderless_text(id, label, cx)
                .text_color(k.secondary)
                .font_weight(gpui::FontWeight::NORMAL)
        };
        if server_step {
            let cancel = link("login-server-cancel", common::CANCEL, cx)
                .on_click(cx.listener(|this, _, window, cx| this.close_server_field(window, cx)));
            // The servers this build knows by name. On the Mac they are in the app menu's
            // 服务器 submenu, which the menu bar shows at the login window too; off the Mac
            // the login window has no menu bar, so they are links here, next to 取消.
            let presets = if crate::ui::native::window_chrome::drawn() {
                server_presets(env!("PRODUCT_PRODUCTION_URL"), env!("PRODUCT_STAGING_URL"))
            } else {
                Vec::new()
            };
            if presets.is_empty() {
                return cancel.into_any_element();
            }
            return div()
                .flex()
                .items_center()
                .justify_center()
                .gap(space::S16)
                .children(presets.into_iter().map(|(id, label, url)| {
                    link(id, label, cx).on_click(cx.listener(move |this, _, window, cx| {
                        this.server
                            .update(cx, |st, cx| st.set_value(url.to_string(), window, cx));
                        this.close_server_field(window, cx);
                    }))
                }))
                .child(cancel)
                .into_any_element();
        }
        // The email steps: back to the invite code, and on the code step
        // 重新发送 and 换一个邮箱.
        match self.step {
            Step::Invite => {}
            Step::Email => {
                return link("login-use-invite", l::USE_INVITE, cx)
                    .on_click(
                        cx.listener(|this, _, window, cx| this.go_to(Step::Invite, window, cx)),
                    )
                    .into_any_element();
            }
            Step::EmailCode => {
                return div()
                    .flex()
                    .items_center()
                    .justify_center()
                    .gap(space::S16)
                    .child(link("login-email-resend", l::EMAIL_RESEND, cx).on_click(
                        cx.listener(|this, _, window, cx| this.send_email_code(window, cx)),
                    ))
                    .child(link("login-email-change", l::EMAIL_CHANGE, cx).on_click(
                        cx.listener(|this, _, window, cx| this.go_to(Step::Email, window, cx)),
                    ))
                    .into_any_element();
            }
        }
        let open = cx.listener(|this, _, window, cx| this.open_server_field(window, cx));
        let email = link("login-email", l::EMAIL_SIGN_IN, cx)
            .on_click(cx.listener(|this, _, window, cx| this.go_to(Step::Email, window, cx)));
        let row = match custom {
            None => div()
                .flex()
                .items_center()
                .justify_center()
                .gap(space::S16)
                .child(email)
                .child(link("login-server-other", l::OTHER_SERVER, cx).on_click(open))
                .into_any_element(),
            // A long host is cut in the middle (both ends name it), with the
            // full host in a tooltip (2.21).
            Some(host) => div()
                .max_w_full()
                .min_w_0()
                .flex()
                .items_center()
                .justify_center()
                .gap(space::S4)
                .n_body()
                .text_color(k.secondary)
                .child(
                    Sym::MacMini
                        .icon(SymSize::INLINE_SMALL)
                        .text_color(k.secondary),
                )
                .child(
                    crate::ui::truncate::TruncText::new(host)
                        .id("login-host")
                        .middle()
                        .min_w_0(),
                )
                .child(
                    link("login-server-change", l::CHANGE_SERVER, cx)
                        .text_color(k.label)
                        .on_click(open),
                )
                .child(div().ml(space::S12).child(email))
                .into_any_element(),
        };
        // 添加账号…: 取消 goes back to the account that stays signed in.
        if !self.services.provider.adding_account() {
            return row;
        }
        let services = self.services.clone();
        div()
            .max_w_full()
            .min_w_0()
            .flex()
            .items_center()
            .justify_center()
            .gap(space::S16)
            .child(row)
            .child(
                link("login-add-cancel", common::CANCEL, cx)
                    .on_click(move |_, _, _| services.provider.cancel_add_account()),
            )
            .into_any_element()
    }
}

impl Render for LoginView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        use crate::ui::native::tokens::space;
        let k = native_colors(cx);
        let session = self.services.provider.session();
        let signing_in =
            self.preview_busy || self.sending || matches!(session, SessionState::SigningIn);
        let step = self.step;
        let complete = match step {
            Step::Invite => self.input(cx).credential().is_some(),
            Step::Email => !self.email_text(cx).is_empty(),
            Step::EmailCode => self.email_code_digits(cx).len() == EMAIL_CODE_LEN,
        };
        let server_url = self.server.read(cx).value().to_string();
        let custom = (server_url.trim().trim_end_matches('/')
            != default_server_url().trim_end_matches('/'))
        .then(|| SharedString::from(server_host(&server_url).to_string()));
        // One field at a time: the server, or the invite code.
        let server_step = self.server_open;
        let error_text: Option<&'static str> = if signing_in {
            None
        } else if let Some(e) = self.email_error.filter(|_| !server_step) {
            Some(email_error_text(e))
        } else {
            self.error.map(|e| match (step, e) {
                (Step::EmailCode, LoginError::BadCredential) => l::EMAIL_ERROR_WRONG_CODE,
                _ => error_text(e),
            })
        };
        let error = error_text;
        let field: gpui::AnyElement = if server_step {
            NativeTextField::new("login-server", &self.server)
                .size(FieldSize::Large)
                .full_width()
                .error(error.is_some())
                .into_any_element()
        } else {
            match step {
                Step::Invite => self.code.clone().into_any_element(),
                Step::Email => NativeTextField::new("login-email-field", &self.email)
                    .size(FieldSize::Large)
                    .full_width()
                    .error(error.is_some())
                    .into_any_element(),
                Step::EmailCode => self.email_code.clone().into_any_element(),
            }
        };
        // Under the field: the error (owner decision v15: neutral 12 text
        // after a 16 destructive icon, as in every sheet footer). The slot
        // keeps its height, so nothing moves.
        let under = error.map(|text| {
            crate::ui::native::sheet::sheet_status_error(text, cx)
                .id("login-error")
                .max_w_full()
        });
        let ready = if server_step {
            !server_url.trim().is_empty()
        } else {
            complete
        };
        let go = push_button(
            "login",
            match (signing_in, step) {
                (true, Step::Email) | (true, Step::EmailCode) if self.sending => l::EMAIL_SENDING,
                (true, _) => l::SIGNING_IN,
                (false, Step::Email) if !server_step => l::EMAIL_SEND,
                (false, _) => o::LOGIN_CONTINUE,
            },
            PushStyle::Default,
            PushSize::Large,
            cx,
        )
        .w_full();
        // Busy: a 16 spinner before 正在验证…, at the same width, solid.
        let go = loading_push(
            waiting_push(go, !ready && !signing_in),
            signing_in,
            PushSize::Large,
        )
        .on_click(cx.listener(|this, _, window, cx| this.continue_(window, cx)));
        let link = self.render_link_row(server_step, custom.filter(|_| !server_step), cx);
        let (title, description): (&'static str, SharedString) = if server_step {
            (l::SERVER_TITLE, l::SERVER_LEAD.into())
        } else {
            match step {
                Step::Invite if self.services.provider.adding_account() => {
                    (l::ADD_TITLE, l::ADD_LEAD.into())
                }
                Step::Invite => (l::WELCOME, l::LEAD.into()),
                Step::Email => (l::EMAIL_SIGN_IN, l::EMAIL_LEAD.into()),
                Step::EmailCode => (
                    l::EMAIL_CODE_TITLE,
                    crate::copy::fill(
                        l::EMAIL_CODE_LEAD,
                        &[("email", &short_email(&self.sent_to))],
                    )
                    .into(),
                ),
            }
        };
        let column = div()
            .w(column_width())
            .flex()
            .flex_col()
            .items_center()
            .child(app_icon(spot::LOGIN_ICON))
            .child(
                div()
                    .mt(space::S16)
                    .n_title2()
                    .text_color(k.label)
                    .text_center()
                    .child(title),
            )
            .child(
                div()
                    .mt(space::S4)
                    .n_body()
                    .text_color(k.secondary)
                    .text_center()
                    .child(description),
            )
            .child(
                div()
                    .mt(space::S24)
                    .w_full()
                    .flex()
                    .justify_center()
                    .child(field),
            )
            .child(
                div()
                    .mt(space::S8)
                    .h(crate::ui::native::tokens::text::META.1)
                    .w_full()
                    .min_w_0()
                    .flex()
                    .items_center()
                    .justify_center()
                    .children(under),
            )
            .child(div().mt(space::S16).w_full().child(go))
            .child(
                div()
                    .mt(space::S12)
                    .w_full()
                    .min_w_0()
                    .flex()
                    .justify_center()
                    .child(link),
            );
        div()
            .size_full()
            .relative()
            .bg(k.window)
            .on_action(cx.listener(|this, _: &UseOtherServer, window, cx| {
                this.open_server_field(window, cx)
            }))
            .on_key_down(cx.listener(|this, e: &gpui::KeyDownEvent, window, cx| {
                if e.keystroke.key != "escape" {
                    return;
                }
                if this.server_open {
                    this.close_server_field(window, cx);
                } else if this.services.provider.adding_account() {
                    this.services.provider.cancel_add_account();
                } else {
                    return;
                }
                cx.stop_propagation();
            }))
            .child(
                div()
                    .size_full()
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(column),
            )
            // The top band drags the window, as a title bar does.
            .child(
                ui::toolbar_band("login-toolbar")
                    .absolute()
                    .top_0()
                    .left_0(),
            )
    }
}

/// A long address cut in the middle (both ends name it), so the lead stays
/// at two lines.
fn short_email(email: &str) -> String {
    const MAX: usize = 32;
    let chars: Vec<char> = email.chars().collect();
    if chars.len() <= MAX {
        return email.to_string();
    }
    let head: String = chars[..14].iter().collect();
    let tail: String = chars[chars.len() - 17..].iter().collect();
    format!("{head}…{tail}")
}

/// The text of a refused email request.
fn email_error_text(e: EmailError) -> &'static str {
    match e {
        EmailError::BadAddress => l::EMAIL_ERROR_BAD_ADDRESS,
        EmailError::WrongCode => l::EMAIL_ERROR_WRONG_CODE,
        EmailError::RateLimited => l::EMAIL_ERROR_RATE,
        EmailError::NotSetUp => l::EMAIL_ERROR_NOT_SET_UP,
        EmailError::Offline => l::ERROR_UNREACHABLE,
        EmailError::Taken | EmailError::Other => l::ERROR_OTHER,
    }
}

/// The text of a refused login.
fn error_text(e: LoginError) -> &'static str {
    match e {
        LoginError::BadCredential => l::ERROR_BAD_CREDENTIAL,
        LoginError::ServerUnreachable => l::ERROR_UNREACHABLE,
        LoginError::Unsupported => l::ERROR_UNSUPPORTED,
        LoginError::Other => l::ERROR_OTHER,
    }
}

// --- the login window ---

/// The login window while it is open.
struct LoginWindowGlobal(Option<gpui::AnyWindowHandle>);

impl gpui::Global for LoginWindowGlobal {}

/// Set by the app (not the self-tests): signing out in a main window moves
/// to the login window.
static STANDALONE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// The root of the login window: the form, and the move to the main window
/// once the session is signed in.
pub struct LoginWindow {
    login: Entity<LoginView>,
    _subscriptions: Vec<Subscription>,
}

impl LoginWindow {
    fn new(services: Services, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let login = cx.new(|cx| {
            let mut view = LoginView::new(services.clone(), window, cx);
            view.embedded = false;
            view
        });
        let hub = services.hub.clone();
        let subscriptions =
            vec![
                cx.subscribe_in(&hub, window, move |_, _, changes: &Changes, window, cx| {
                    if !changes.any(|c| matches!(c, Change::Session)) {
                        return;
                    }
                    if matches!(services.provider.session(), SessionState::SignedIn { .. }) {
                        let services = services.clone();
                        window.defer(cx, move |window, cx| {
                            if let Err(e) = crate::app::open_main_window(services, true, None, cx) {
                                eprintln!("failed to open the window: {e}");
                                return;
                            }
                            cx.set_global(LoginWindowGlobal(None));
                            window.remove_window();
                        });
                    }
                }),
            ];
        Self {
            login,
            _subscriptions: subscriptions,
        }
    }

    pub fn login(&self) -> &Entity<LoginView> {
        &self.login
    }

    /// The window of a screenshot scene.
    pub fn new_for_scene(services: Services, window: &mut Window, cx: &mut Context<Self>) -> Self {
        Self::new(services, window, cx)
    }
}

impl Render for LoginWindow {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let body = div().size_full().child(self.login.clone());
        crate::ui::native::window_chrome::framed_fixed(env!("PRODUCT_NAME"), body, window, cx)
    }
}

/// The options of the login window: 480 x 520, centred, not resizable, a
/// transparent title bar (the content draws under it).
pub fn window_options(focus: bool, cx: &gpui::App) -> gpui::WindowOptions {
    gpui::WindowOptions {
        window_bounds: Some(gpui::WindowBounds::Windowed(gpui::Bounds::centered(
            None,
            WINDOW_SIZE,
            cx,
        ))),
        titlebar: Some(gpui::TitlebarOptions {
            title: Some(env!("PRODUCT_NAME").into()),
            appears_transparent: true,
            traffic_light_position: None,
        }),
        window_background: gpui::WindowBackgroundAppearance::Opaque,
        is_resizable: false,
        focus,
        show: true,
        ..Default::default()
    }
}

/// Opens the login window, or brings the open one to the front.
pub fn open_window(
    services: Services,
    focus: bool,
    cx: &mut gpui::App,
) -> Result<(gpui::AnyWindowHandle, Entity<LoginWindow>), String> {
    let (handle, view) = gpui_kit::open_window(window_options(focus, cx), cx, move |window, cx| {
        cx.new(|cx| LoginWindow::new(services, window, cx))
    })
    .map_err(|e| e.to_string())?;
    cx.set_global(LoginWindowGlobal(Some(handle)));
    Ok((handle, view))
}

/// The app's start and the Dock icon: while signed out, the login window
/// (brought to the front, or opened again). Returns false when signed in.
pub fn show_login_window(services: Services, cx: &mut gpui::App) -> bool {
    STANDALONE.store(true, std::sync::atomic::Ordering::Relaxed);
    if matches!(services.provider.session(), SessionState::SignedIn { .. }) {
        return false;
    }
    let open = cx
        .try_global::<LoginWindowGlobal>()
        .and_then(|g| g.0)
        .is_some_and(|h| {
            h.update(cx, |_, window, _| window.activate_window())
                .is_ok()
        });
    if !open && let Err(e) = open_window(services, true, cx) {
        eprintln!("failed to open the login window: {e}");
    }
    true
}

/// Signed out in a main window: the login window takes its place.
fn move_to_login_window(services: Services, window: &mut Window, cx: &mut gpui::App) {
    if !STANDALONE.load(std::sync::atomic::Ordering::Relaxed) {
        return;
    }
    window.defer(cx, move |window, cx| {
        show_login_window(services, cx);
        window.remove_window();
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_login_window_names_only_the_servers_the_build_has() {
        let names = |production, staging| -> Vec<&'static str> {
            server_presets(production, staging)
                .into_iter()
                .map(|(_, label, _)| label)
                .collect()
        };
        // An open-source build has no server of its own: nothing to pick.
        assert!(names("", "").is_empty());
        assert_eq!(
            names("https://chat.example", "https://staging.example"),
            [l::SERVER_PRODUCTION, l::SERVER_STAGING]
        );
        assert_eq!(names("https://chat.example", ""), [l::SERVER_PRODUCTION]);
        assert_eq!(names("", "https://staging.example"), [l::SERVER_STAGING]);
        // One server under both names is offered once.
        assert_eq!(
            names("https://chat.example", "https://chat.example"),
            [l::SERVER_PRODUCTION]
        );
        // Each link fills in its own URL.
        let urls: Vec<_> = server_presets("https://a.example", "https://b.example")
            .into_iter()
            .map(|(_, _, url)| url)
            .collect();
        assert_eq!(urls, ["https://a.example", "https://b.example"]);
    }

    #[test]
    fn code_input_reads_codes_and_tokens() {
        for pasted in ["K7QM-2X9A", "K7QM2X9A", "k7qm-2x9a", " k7qm 2x9a "] {
            assert_eq!(CodeEntry::parse(pasted), CodeEntry::Code("K7QM2X9A".into()));
            assert_eq!(
                CodeEntry::parse(pasted).credential(),
                Some(Credential::InviteCode("K7QM-2X9A".into()))
            );
        }
        assert_eq!(CodeEntry::parse("K7QM-2X").credential(), None);
        assert_eq!(
            CodeEntry::parse("K7QM2X9AB"),
            CodeEntry::Code("K7QM2X9A".into())
        );
        // A legacy 12-character code is one locked field and still signs in.
        assert_eq!(
            CodeEntry::parse(" k7qm-2x9a bc3d "),
            CodeEntry::Legacy("K7QM2X9ABC3D".into())
        );
        assert_eq!(
            CodeEntry::parse("k7qm2x9abc3d").credential(),
            Some(Credential::InviteCode("K7QM-2X9A-BC3D".into()))
        );
        assert!(matches!(
            CodeEntry::parse(" tc_abc ").credential(),
            Some(Credential::Token(_))
        ));
    }

    #[test]
    fn only_a_credential_or_a_legacy_code_is_kept_whole() {
        assert!(!keeps_whole("K7QM2X9A"));
        assert!(!keeps_whole("K7QM2X9AB"));
        assert!(keeps_whole("K7QM-2X9A-BC3D"));
        assert!(keeps_whole("tc_abcdefghijklmnop"));
    }

    #[test]
    fn a_long_email_is_cut_in_the_middle() {
        assert_eq!(short_email("joye@example.com"), "joye@example.com");
        let cut = short_email("huang.deshiou.product-design@northwind-holdings.example.com");
        assert_eq!(cut.chars().count(), 32);
        assert!(cut.starts_with("huang.deshiou.") && cut.ends_with(".example.com"));
    }

    #[test]
    fn server_line_shows_the_host() {
        assert_eq!(server_host("https://team.joye.co/"), "team.joye.co");
        assert_eq!(server_host("http://127.0.0.1:8787"), "127.0.0.1:8787");
    }
}
