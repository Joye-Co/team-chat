//! The login view (DESIGN.md §8.3): one centered column with the app icon,
//! the welcome, the invite code as eight boxes and `加入`. The built-in server
//! sits behind `使用其他服务器`; a custom one shows as `host · 更改` (U39). A
//! pasted login credential (`tc_…`) or a legacy 12-character code is accepted
//! in the same place and shows as one locked field. The server URL defaults to
//! the production URL from product.json, then staging.

use gpui::{
    AppContext, Context, Entity, Focusable as _, InteractiveElement as _, IntoElement,
    ParentElement, Render, SharedString, StatefulInteractiveElement as _, Styled, Subscription,
    Window, div, img, prelude::FluentBuilder as _, px,
};
use gpui_component::{
    ActiveTheme as _,
    input::{Input, InputEvent, InputState},
};

use crate::{
    app::{Changes, Services},
    copy::login as l,
    data::{Change, Credential, LoginError, LoginRequest, SessionState},
    theme::{self, Type as _, space},
    ui::{self, ButtonSpec as _, Lucide},
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
/// Characters per group of boxes.
const GROUP: usize = 4;
/// One code box (§7.4): 42 x 48.
const BOX: (gpui::Pixels, gpui::Pixels) = (px(42.), px(48.));
/// The column (§8.3): 8 boxes, 6 gaps of 6 and one 18 pt dash slot = 390.
const COLUMN: gpui::Pixels = px(390.);

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
        let code = cx.new(|cx| InputState::new(window, cx));
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
                        this.submit(window, cx);
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
        self.server_open = server_open;
        // An open server field has the focus, as after `使用其他服务器`.
        let focus = if server_open {
            self.server.read(cx).focus_handle(cx)
        } else {
            self.code.read(cx).focus_handle(cx)
        };
        focus.focus(window, cx);
        cx.notify();
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

    /// The invite code boxes, 4-4 with a dash between the groups, over the
    /// real input (transparent), so typing, paste, IME and Backspace work.
    fn render_boxes(
        &self,
        input: &CodeInput,
        focused: bool,
        error: bool,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let k = theme::colors(cx);
        // The whole code is selected after a wrong try: typing replaces it.
        let selected = focused && {
            let st = self.code.read(cx);
            let range = st.selected_range();
            !st.value().is_empty() && range.start == 0 && range.end >= st.value().len()
        };
        // The text-selection color of the theme (§2.8 maps it).
        let selection = cx.theme().selection;
        let locked = match input {
            CodeInput::Token(_) => Some(l::CODE_TOKEN),
            CodeInput::Legacy(_) => Some(l::CODE_LEGACY),
            CodeInput::Code(_) => None,
        };
        let row = match (input, locked) {
            (_, Some(locked)) => div()
                .w_full()
                .h(BOX.1)
                .px(space::S12)
                .rounded(theme::radius::FIELD)
                .bg(k.content)
                .border_1()
                .border_color(if error {
                    k.danger
                } else if focused {
                    k.focus_border
                } else {
                    k.border
                })
                .when(focused && !error, |el| el.shadow(k.focus_halo()))
                .when(error, |el| el.shadow(k.error_halo()))
                .when(selected, |el| el.bg(selection))
                .flex()
                .items_center()
                .gap(space::S8)
                .child(ui::icon(Lucide::Lock, theme::icon::ROW, k.text_3))
                .child(div().t_ui().text_color(k.text_2).child(locked)),
            (CodeInput::Code(chars), None) => {
                let chars: Vec<char> = chars.chars().collect();
                let cursor = chars.len().min(CODE_LEN - 1);
                let mut row = div().w_full().flex().items_center().gap(space::S6);
                for ix in 0..CODE_LEN {
                    if ix > 0 && ix % GROUP == 0 {
                        row = row.child(
                            div()
                                .flex_none()
                                .w(space::S6)
                                .h(px(2.))
                                .rounded(px(1.))
                                .bg(k.border_strong),
                        );
                    }
                    let active = focused && !selected && ix == cursor;
                    let ch = chars.get(ix).copied();
                    // With all eight in, the caret sits after the last one.
                    let caret_after = active && ch.is_some() && chars.len() == CODE_LEN;
                    row = row.child(
                        div()
                            .flex_none()
                            .w(BOX.0)
                            .h(BOX.1)
                            .rounded(theme::radius::FIELD)
                            .bg(k.content)
                            .border_1()
                            .border_color(if error {
                                k.danger
                            } else if active {
                                k.focus_border
                            } else {
                                k.border
                            })
                            .when(active && !error, |el| el.shadow(k.focus_halo()))
                            .flex()
                            .items_center()
                            .justify_center()
                            .t_code_entry()
                            .text_color(k.text_1)
                            // Selected characters sit on a text-selection
                            // chip, as selected text does in a field.
                            .children(ch.map(|c| {
                                div()
                                    .relative()
                                    .px(space::S6)
                                    .rounded(theme::radius::MARK)
                                    .when(selected, |el| el.bg(selection))
                                    .child(c.to_string())
                                    .when(caret_after, |el| {
                                        el.child(
                                            div()
                                                .absolute()
                                                .top_0()
                                                .bottom_0()
                                                .right(px(1.))
                                                .flex()
                                                .items_center()
                                                .child(
                                                    div()
                                                        .w(px(2.))
                                                        .h(px(22.))
                                                        .rounded(px(1.))
                                                        .bg(k.accent),
                                                ),
                                        )
                                    })
                            }))
                            // The caret sits in the next empty box.
                            .when(active && ch.is_none(), |el| {
                                el.child(div().w(px(2.)).h(px(22.)).rounded(px(1.)).bg(k.accent))
                            }),
                    );
                }
                row
            }
            (_, None) => div(),
        };
        let code = self.code.clone();
        div()
            .id("login-code")
            .relative()
            .w_full()
            .cursor_text()
            .child(row)
            // The real input covers the boxes: it takes clicks, keys and the
            // IME; its own text and caret are not drawn.
            .child(
                div()
                    .absolute()
                    .inset_0()
                    .opacity(0.)
                    .child(Input::new(&self.code).appearance(false).h(BOX.1)),
            )
            .on_click(cx.listener(move |_, _, window, cx| {
                code.read(cx).focus_handle(cx).focus(window, cx);
            }))
    }
}

impl Render for LoginView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let k = theme::colors(cx);
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
        let code_error = error == Some(l::ERROR_BAD_CREDENTIAL);
        let input = self.input(cx);
        let complete = input.credential().is_some();
        let focused = self.code.read(cx).focus_handle(cx).is_focused(window);
        let icon = if theme::is_dark(cx) {
            ui::icons::APP_ICON_DARK_72
        } else {
            ui::icons::APP_ICON_72
        };
        let server_url = self.server.read(cx).value().to_string();
        let host: SharedString = server_host(&server_url).to_string().into();
        let open_server = cx.listener(|this, _, window, cx| {
            this.server_open = true;
            this.server.read(cx).focus_handle(cx).focus(window, cx);
            cx.notify();
        });
        // The built-in server is not worth a line of its own (U39): a quiet
        // `使用其他服务器` opens the field. A custom server shows its host.
        let custom =
            server_url.trim().trim_end_matches('/') != default_server_url().trim_end_matches('/');
        let server_line = if custom {
            div()
                .pt(space::S16)
                .flex()
                .items_center()
                .justify_center()
                .gap(space::S6)
                .t_small()
                .text_color(k.text_3)
                .child(ui::icon(Lucide::Server, theme::icon::CHIP, k.text_3))
                .child(div().min_w_0().one_line().child(host))
                .child("·")
                .child(
                    ui::text_button("login-server-change", l::CHANGE_SERVER, cx)
                        .on_click(open_server),
                )
        } else {
            div().pt(space::S12).flex().justify_center().child(
                ui::text_button("login-server-other", l::OTHER_SERVER, cx).on_click(open_server),
            )
        };
        let server_field = div()
            .pt(space::S16)
            .w_full()
            .flex()
            .flex_col()
            .child(ui::field_label(l::SERVER_LABEL, cx))
            .child(ui::TextField::new(
                "login-server",
                &self.server,
                ui::FieldKind::Form,
            ));
        div()
            .size_full()
            .relative()
            .bg(k.content)
            .child(
                div()
                    .size_full()
                    .flex()
                    .items_center()
                    .justify_center()
                    // 24 pt above the optical center (§8.3).
                    .pb(space::S44)
                    .child(
                        div()
                            .w(COLUMN)
                            .flex()
                            .flex_col()
                            .items_center()
                            .child(
                                // Drawn bare: the squircle with a soft drop shadow.
                                div()
                                    .size(px(72.))
                                    .rounded(px(16.))
                                    .shadow(vec![gpui::BoxShadow {
                                        color: gpui::hsla(220. / 360., 0.14, 0.07, 0.12),
                                        offset: gpui::point(px(0.), px(4.)),
                                        blur_radius: px(12.),
                                        spread_radius: px(0.),
                                        inset: false,
                                    }])
                                    .child(img(icon).size(px(72.))),
                            )
                            .child(
                                div()
                                    .pt(space::S24)
                                    .t_display()
                                    .text_color(k.text_1)
                                    .child(l::WELCOME),
                            )
                            .child(
                                div()
                                    .pt(space::S6)
                                    .t_body()
                                    .text_color(k.text_2)
                                    .child(l::LEAD),
                            )
                            .child(
                                div()
                                    .pt(px(28.))
                                    .w_full()
                                    .child(ui::field_label(l::CODE_LABEL, cx)),
                            )
                            .child(self.render_boxes(&input, focused, code_error, cx))
                            .when_some(error, |el, e| {
                                el.child(div().w_full().child(ui::field_help(e, true, cx)))
                            })
                            .child(
                                div().pt(space::S24).w_full().child(
                                    ui::button(
                                        "login",
                                        if signing_in { l::SIGNING_IN } else { l::SUBMIT },
                                        ui::ButtonKind::Primary,
                                        ui::ButtonSize::Lg,
                                        cx,
                                    )
                                    .w_full()
                                    .loading(signing_in)
                                    .disabled_fill(!complete && !signing_in, cx)
                                    .on_click(
                                        cx.listener(|this, _, window, cx| this.submit(window, cx)),
                                    ),
                                ),
                            )
                            .map(|el| {
                                if self.server_open {
                                    el.child(server_field)
                                } else {
                                    el.child(server_line)
                                }
                            }),
                    ),
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
