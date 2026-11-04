//! The login window (NATIVE.md §9.7): the app icon 96, a Title 2 welcome
//! line, one 28 pt standalone field at a time (the invite code, or the
//! server), the default `继续` at the bottom trailing edge and the way to the
//! other step at the bottom leading edge. A custom server shows its host
//! under the code field. A
//! pasted login credential (`tc_…`) or a legacy 12-character code is accepted
//! in the same place and shows as one locked field. The server URL defaults to
//! the production URL from product.json, then staging.

use gpui::{
    AppContext, Context, Entity, Focusable as _, InteractiveElement as _, IntoElement,
    ParentElement, Render, SharedString, Styled, Subscription, Window, div,
    prelude::FluentBuilder as _, px,
};
use gpui_component::input::{Input, InputEvent, InputState};

use crate::{
    app::{Changes, Services},
    copy::{common, login as l, overlays as o},
    data::{Change, Credential, LoginError, LoginRequest, SessionState},
    theme::Type as _,
    ui::{
        self, Sym,
        native::{
            FieldSize, NativeTextField, NativeType as _, PushSize, PushStyle, disabled_push,
            metrics, native_colors, push_button,
            overlays::{app_icon, spot},
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

/// Characters in an invite code (`XXXX-XXXX`, server `newInviteCode`).
pub const CODE_LEN: usize = 8;
/// Characters of a code made before the 8-character format
/// (`XXXX-XXXX-XXXX`); the server still redeems them.
pub const LEGACY_CODE_LEN: usize = 12;
/// Characters per group of a code (`XXXX-XXXX`).
const GROUP: usize = 4;
/// The code field fits `XXXX-XXXX` and a pasted credential's lock line
/// (§5.6: width fits the expected text).
const CODE_FIELD: gpui::Pixels = px(200.);
/// The server field fits a URL.
const SERVER_FIELD: gpui::Pixels = px(320.);

/// What the code field holds.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CodeInput {
    /// The invite code characters typed so far, upper case, at most 8.
    Code(String),
    /// A pasted 12-character code of the earlier format, upper case.
    Legacy(String),
    /// A pasted login credential.
    Token(String),
}

/// The letters and digits of a code, upper case (dashes and spaces dropped).
fn code_chars(text: &str) -> String {
    text.chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .map(|c| c.to_ascii_uppercase())
        .collect()
}

impl CodeInput {
    /// Reads the field: a `tc_` credential, a pasted legacy code of exactly
    /// 12 characters, or the first 8 letters and digits of an invite code.
    pub fn parse(text: &str) -> Self {
        let text = text.trim();
        if text.starts_with("tc_") {
            return CodeInput::Token(text.to_string());
        }
        let chars = code_chars(text);
        if chars.len() == LEGACY_CODE_LEN {
            return CodeInput::Legacy(chars);
        }
        CodeInput::Code(chars.chars().take(CODE_LEN).collect())
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
            CodeInput::Token(t) => Credential::from_input(t),
            CodeInput::Legacy(c) => Some(grouped(c)),
            CodeInput::Code(c) if c.chars().count() == CODE_LEN => Some(grouped(c)),
            CodeInput::Code(_) => None,
        }
    }

    /// The field text to keep when typing went past 8 characters: typing
    /// stops at the eighth box, so only a paste reaches a legacy code.
    fn clamped(text: &str) -> Option<String> {
        let trimmed = text.trim();
        if trimmed.starts_with("tc_") {
            return None;
        }
        let chars = code_chars(trimmed);
        (chars.len() > CODE_LEN && chars.len() != LEGACY_CODE_LEN)
            .then(|| chars.chars().take(CODE_LEN).collect())
    }
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
    code: Entity<InputState>,
    /// The server field is open (`更改`, or after an unreachable server).
    server_open: bool,
    local_error: Option<&'static str>,
    /// The last error stays until the user types again.
    show_error: bool,
    /// The wrong code after a failed try: the next typed character or paste
    /// starts a new code from box 1, Backspace edits it from the end.
    rejected: Option<String>,
    _subscriptions: Vec<Subscription>,
}

impl LoginView {
    pub fn new(services: Services, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let server = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder(l::SERVER_PLACEHOLDER)
                .default_value(default_server_url())
        });
        let code = cx.new(|cx| InputState::new(window, cx).placeholder(l::CODE_PLACEHOLDER));
        // The first box has the caret when the view opens.
        code.read(cx).focus_handle(cx).focus(window, cx);
        let subscriptions = vec![
            cx.subscribe_in(
                &code,
                window,
                |this, _, event: &InputEvent, window, cx| match event {
                    InputEvent::PressEnter { .. } => this.submit(window, cx),
                    InputEvent::Change => {
                        let mut value = this.code.read(cx).value().to_string();
                        if let Some(old) = this.rejected.take()
                            && value.len() > old.len()
                            && let Some(fresh) = value.strip_prefix(old.as_str())
                        {
                            value = fresh.to_string();
                            this.code
                                .update(cx, |st, cx| st.set_value(value.clone(), window, cx));
                        }
                        if let Some(kept) = CodeInput::clamped(&value) {
                            this.code
                                .update(cx, |st, cx| st.set_value(kept, window, cx));
                        }
                        this.show_error = false;
                        this.local_error = None;
                        cx.notify();
                    }
                    _ => cx.notify(),
                },
            ),
            cx.subscribe_in(
                &server,
                window,
                |this, _, event: &InputEvent, window, cx| {
                    if let InputEvent::PressEnter { .. } = event {
                        this.continue_(window, cx);
                    }
                },
            ),
            cx.subscribe_in(
                &services.hub,
                window,
                |this, _, changes: &Changes, window, cx| {
                    if !changes.any(|c| matches!(c, Change::Session)) {
                        return;
                    }
                    this.show_error = true;
                    match this.services.provider.session() {
                        SessionState::SignedOut {
                            last_error: Some(LoginError::ServerUnreachable),
                        } => {
                            this.server_open = true;
                            this.server.read(cx).focus_handle(cx).focus(window, cx);
                        }
                        // A wrong code stays in the boxes with the caret
                        // after it (§8.3).
                        SessionState::SignedOut {
                            last_error: Some(LoginError::BadCredential),
                        } => this.reject_code(window, cx),
                        _ => {}
                    }
                    cx.notify();
                },
            ),
        ];
        Self {
            services,
            server,
            code,
            server_open: false,
            local_error: None,
            show_error: true,
            rejected: None,
            _subscriptions: subscriptions,
        }
    }

    /// Keeps a wrong code in the boxes, focused, with the caret after its
    /// last character; the next character typed or pasted starts a new code.
    fn reject_code(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let value = self.code.read(cx).value().to_string();
        let end = value.len();
        self.code.update(cx, |st, cx| {
            st.focus(window, cx);
            st.set_selected_range(end..end, cx);
        });
        self.rejected = Some(value);
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
        cx.notify();
    }

    fn input(&self, cx: &gpui::App) -> CodeInput {
        CodeInput::parse(&self.code.read(cx).value())
    }

    /// Fills the form for a screenshot scene: the code typed so far, the
    /// server field open or not, and focus in the code boxes.
    pub fn preview(
        &mut self,
        code: &str,
        server_open: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.code
            .update(cx, |st, cx| st.set_value(code.to_string(), window, cx));
        // An open server field has the focus and its URL selected, as after
        // `使用其他服务器`.
        if server_open {
            self.open_server_field(window, cx);
        } else {
            self.server_open = false;
            self.code.read(cx).focus_handle(cx).focus(window, cx);
            cx.notify();
        }
    }

    fn submit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if matches!(self.services.provider.session(), SessionState::SigningIn) {
            return;
        }
        let server_url = self.server.read(cx).value().trim().to_string();
        let Some(credential) = self.input(cx).credential() else {
            return;
        };
        if server_url.is_empty() {
            self.local_error = Some(l::ERROR_EMPTY);
            self.server_open = true;
            cx.notify();
            return;
        }
        self.local_error = None;
        self.show_error = false;
        self.services.provider.login(LoginRequest {
            server_url,
            credential,
            device_name: crate::provider::session::device_name(),
        });
        // A wrong code stays in the boxes so it can be checked (§8.3); the
        // field lives only in this view and is never logged.
        let _ = window;
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

    /// The code field: a 28 pt standalone field (§5.6). A pasted credential
    /// or legacy code shows as a locked line (the secret is never drawn);
    /// the real input sits on top of it so paste and Backspace still work.
    fn render_code_field(
        &self,
        input: &CodeInput,
        focused: bool,
        error: bool,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let k = native_colors(cx);
        let locked = match input {
            CodeInput::Token(_) => Some(l::CODE_TOKEN),
            CodeInput::Legacy(_) => Some(l::CODE_LEGACY),
            CodeInput::Code(_) => None,
        };
        match locked {
            None => NativeTextField::new("login-code", &self.code)
                .size(FieldSize::Large)
                .width(CODE_FIELD)
                .error(error)
                .into_any_element(),
            Some(text) => div()
                .id("login-code")
                .relative()
                .w(CODE_FIELD)
                .h(metrics::FIELD_LARGE)
                .px(metrics::FIELD_PAD)
                .rounded(metrics::FIELD_RADIUS)
                .bg(k.content)
                .border_1()
                .border_color(if error { k.red } else { k.separator })
                .when(focused, |d| d.shadow(k.focus_ring()))
                .flex()
                .items_center()
                .gap(px(6.))
                .n_body()
                .text_color(k.secondary)
                .child(Sym::Lock.icon(SymSize::INLINE).text_color(k.secondary))
                .child(text)
                .child(
                    div()
                        .absolute()
                        .inset_0()
                        .opacity(0.)
                        .child(Input::new(&self.code).appearance(false).h_full()),
                )
                .into_any_element(),
        }
    }

    /// 继续: on the code step it joins; on the server step it goes back to
    /// the code, or joins when the code is complete.
    fn continue_(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.server_open && self.input(cx).credential().is_none() {
            self.close_server_field(window, cx);
        } else {
            self.submit(window, cx);
        }
    }

    fn close_server_field(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.server_open = false;
        self.local_error = None;
        self.code.read(cx).focus_handle(cx).focus(window, cx);
        cx.notify();
    }
}

impl Render for LoginView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let k = native_colors(cx);
        let session = self.services.provider.session();
        let signing_in = matches!(session, SessionState::SigningIn);
        let session_error = match session {
            SessionState::SignedOut {
                last_error: Some(e),
            } => Some(match e {
                LoginError::BadCredential => l::ERROR_BAD_CREDENTIAL,
                LoginError::ServerUnreachable => l::ERROR_UNREACHABLE,
                LoginError::Unsupported => l::ERROR_UNSUPPORTED,
                LoginError::Other => l::ERROR_OTHER,
            }),
            _ => None,
        };
        let error = self
            .local_error
            .or(session_error.filter(|_| self.show_error));
        let input = self.input(cx);
        let complete = input.credential().is_some();
        let focused = self.code.read(cx).focus_handle(cx).is_focused(window);
        let server_url = self.server.read(cx).value().to_string();
        let host: SharedString = server_host(&server_url).to_string().into();
        let custom =
            server_url.trim().trim_end_matches('/') != default_server_url().trim_end_matches('/');
        // One field at a time: the server, or the invite code.
        let server_step = self.server_open;
        let field: gpui::AnyElement = if server_step {
            NativeTextField::new("login-server", &self.server)
                .size(FieldSize::Large)
                .width(SERVER_FIELD)
                .error(error.is_some())
                .into_any_element()
        } else {
            self.render_code_field(&input, focused, error == Some(l::ERROR_BAD_CREDENTIAL), cx)
        };
        // Under the field: the error, or (on the code step) the custom server.
        let note: Option<gpui::Div> = match error {
            Some(e) => Some(div().n_subheadline().text_color(k.red).child(e)),
            None if !server_step && custom => Some(
                div()
                    .flex()
                    .items_center()
                    .gap(px(4.))
                    .n_subheadline()
                    .text_color(k.secondary)
                    .child(Sym::MacMini.icon(SymSize::INLINE_SMALL).text_color(k.secondary))
                    .child(div().min_w_0().one_line().child(host)),
            ),
            None => None,
        };
        let ready = if server_step {
            !server_url.trim().is_empty()
        } else {
            complete
        };
        let go = push_button(
            "login",
            if signing_in { l::SIGNING_IN } else { o::LOGIN_CONTINUE },
            PushStyle::Default,
            PushSize::Large,
            cx,
        );
        let go = disabled_push(go, !ready || signing_in, cx)
            .loading(signing_in)
            .on_click(cx.listener(|this, _, window, cx| this.continue_(window, cx)));
        // Bottom leading: the way to the server step, or back from it.
        let other = if server_step {
            push_button(
                "login-server-cancel",
                common::CANCEL,
                PushStyle::Standard,
                PushSize::Large,
                cx,
            )
            .on_click(cx.listener(|this, _, window, cx| this.close_server_field(window, cx)))
        } else {
            push_button(
                "login-server-other",
                l::OTHER_SERVER,
                PushStyle::Standard,
                PushSize::Large,
                cx,
            )
            .on_click(cx.listener(|this, _, window, cx| this.open_server_field(window, cx)))
        };
        div()
            .size_full()
            .relative()
            .bg(k.window)
            .child(
                div()
                    .size_full()
                    .flex()
                    .items_center()
                    .justify_center()
                    // A little above the optical center.
                    .pb(px(60.))
                    .child(
                        div()
                            .w(SERVER_FIELD)
                            .flex()
                            .flex_col()
                            .items_center()
                            .child(app_icon(spot::LOGIN_ICON))
                            .child(
                                div()
                                    .pt(px(16.))
                                    .n_title2()
                                    .text_color(k.label)
                                    .child(l::WELCOME),
                            )
                            .child(
                                div()
                                    .pt(px(4.))
                                    .n_body()
                                    .text_color(k.secondary)
                                    .child(if server_step { l::SERVER_LABEL } else { l::LEAD }),
                            )
                            .child(div().pt(px(24.)).child(field))
                            // The note slot keeps its height, so nothing
                            // moves when an error shows.
                            .child(
                                div()
                                    .pt(px(8.))
                                    .h(px(22.))
                                    .max_w(SERVER_FIELD)
                                    .children(note),
                            ),
                    ),
            )
            .child(
                div()
                    .absolute()
                    .left(px(20.))
                    .right(px(20.))
                    .bottom(px(20.))
                    .flex()
                    .items_center()
                    .gap(metrics::SHEET_BUTTON_GAP)
                    .child(other)
                    .child(div().flex_1())
                    .child(go),
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn code_input_reads_codes_and_tokens() {
        for pasted in ["K7QM-2X9A", "K7QM2X9A", "k7qm-2x9a", " k7qm 2x9a "] {
            assert_eq!(CodeInput::parse(pasted), CodeInput::Code("K7QM2X9A".into()));
            assert_eq!(
                CodeInput::parse(pasted).credential(),
                Some(Credential::InviteCode("K7QM-2X9A".into()))
            );
        }
        assert_eq!(CodeInput::parse("K7QM-2X").credential(), None);
        assert_eq!(
            CodeInput::parse("K7QM2X9AB"),
            CodeInput::Code("K7QM2X9A".into())
        );
        // A legacy 12-character code is one locked field and still signs in.
        assert_eq!(
            CodeInput::parse(" k7qm-2x9a bc3d "),
            CodeInput::Legacy("K7QM2X9ABC3D".into())
        );
        assert_eq!(
            CodeInput::parse("k7qm2x9abc3d").credential(),
            Some(Credential::InviteCode("K7QM-2X9A-BC3D".into()))
        );
        assert!(matches!(
            CodeInput::parse(" tc_abc ").credential(),
            Some(Credential::Token(_))
        ));
    }

    #[test]
    fn typing_stops_at_the_eighth_box() {
        assert_eq!(CodeInput::clamped("K7QM2X9A"), None);
        assert_eq!(CodeInput::clamped("K7QM2X9AB"), Some("K7QM2X9A".into()));
        assert_eq!(CodeInput::clamped("K7QM-2X9A-BC3D"), None);
        assert_eq!(CodeInput::clamped("tc_abcdefghijklmnop"), None);
    }

    #[test]
    fn server_line_shows_the_host() {
        assert_eq!(server_host("https://team.joye.co/"), "team.joye.co");
        assert_eq!(server_host("http://127.0.0.1:8787"), "127.0.0.1:8787");
    }
}
