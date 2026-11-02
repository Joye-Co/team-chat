//! The login view (DESIGN.md §8.3): one centered column with the app icon,
//! the welcome, the invite code as boxes and `加入`. The server sits behind a
//! `更改` disclosure (U39); a pasted login credential (`tc_…`) is accepted in
//! the same place. The server URL defaults to the production URL from
//! product.json, then staging.

use gpui::{
    AppContext, Context, Entity, Focusable as _, InteractiveElement as _, IntoElement,
    ParentElement, Render, SharedString, StatefulInteractiveElement as _, Styled, Subscription,
    Window, div, img, prelude::FluentBuilder as _, px,
};
use gpui_component::input::{Input, InputEvent, InputState};

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

/// Characters in an invite code (`XXXX-XXXX-XXXX`, server `newInviteCode`).
pub const CODE_LEN: usize = 12;
/// Characters per group of boxes.
const GROUP: usize = 4;
/// One code box (§7.4, widened to 12 characters: 30 x 48).
const BOX: (gpui::Pixels, gpui::Pixels) = (px(30.), px(48.));
/// The column: 12 boxes, 9 gaps of 6 and two 18 pt dash slots = 450.
const COLUMN: gpui::Pixels = px(450.);

/// What the code field holds.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CodeInput {
    /// The invite code characters typed so far, upper case, at most 12.
    Code(String),
    /// A pasted login credential.
    Token(String),
}

impl CodeInput {
    /// Reads the field: a `tc_` credential, or the letters and digits of an
    /// invite code (dashes and spaces dropped, upper case, at most 12).
    pub fn parse(text: &str) -> Self {
        let text = text.trim();
        if text.starts_with("tc_") {
            return CodeInput::Token(text.to_string());
        }
        CodeInput::Code(
            text.chars()
                .filter(|c| c.is_ascii_alphanumeric())
                .map(|c| c.to_ascii_uppercase())
                .take(CODE_LEN)
                .collect(),
        )
    }

    /// The credential to send, once the field is complete.
    pub fn credential(&self) -> Option<Credential> {
        match self {
            CodeInput::Token(t) => Credential::from_input(t),
            CodeInput::Code(c) if c.chars().count() == CODE_LEN => {
                let groups: Vec<String> = c
                    .as_bytes()
                    .chunks(GROUP)
                    .map(|g| String::from_utf8_lossy(g).into_owned())
                    .collect();
                Some(Credential::InviteCode(groups.join("-")))
            }
            CodeInput::Code(_) => None,
        }
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
        let subscriptions = vec![
            cx.subscribe_in(
                &code,
                window,
                |this, _, event: &InputEvent, window, cx| match event {
                    InputEvent::PressEnter { .. } => this.submit(window, cx),
                    InputEvent::Change => {
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
            cx.subscribe(&services.hub, |this, _, changes: &Changes, cx| {
                if changes.any(|c| matches!(c, Change::Session)) {
                    this.show_error = true;
                    if matches!(
                        this.services.provider.session(),
                        SessionState::SignedOut {
                            last_error: Some(LoginError::ServerUnreachable)
                        }
                    ) {
                        this.server_open = true;
                    }
                    cx.notify();
                }
            }),
        ];
        Self {
            services,
            server,
            code,
            server_open: false,
            local_error: None,
            show_error: true,
            _subscriptions: subscriptions,
        }
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
        self.code.read(cx).focus_handle(cx).focus(window, cx);
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
        self.services.provider.login(LoginRequest {
            server_url,
            credential,
            device_name: crate::provider::session::device_name(),
        });
        // The secret is not kept in the field after a try.
        self.code
            .update(cx, |state, cx| state.set_value("", window, cx));
        cx.notify();
    }

    /// The invite code boxes, 4-4-4 with a dash between the groups, over the
    /// real input (transparent), so typing, paste, IME and Backspace work.
    fn render_boxes(
        &self,
        input: &CodeInput,
        focused: bool,
        error: bool,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let k = theme::colors(cx);
        let row = match input {
            CodeInput::Token(_) => div()
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
                .flex()
                .items_center()
                .gap(space::S8)
                .child(ui::icon(Lucide::Lock, theme::icon::ROW, k.text_3))
                .child(div().t_ui().text_color(k.text_2).child(l::CODE_TOKEN)),
            CodeInput::Code(chars) => {
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
                    let active = focused && ix == cursor;
                    let ch = chars.get(ix).copied();
                    row = row.child(
                        div()
                            .flex_none()
                            .w(BOX.0)
                            .h(BOX.1)
                            .rounded(theme::radius::MD)
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
                            .children(ch.map(|c| c.to_string()))
                            // The caret sits in the next empty box.
                            .when(active && ch.is_none(), |el| {
                                el.child(div().w(px(2.)).h(px(22.)).rounded(px(1.)).bg(k.accent))
                            }),
                    );
                }
                row
            }
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
        let server_line = div()
            .pt(space::S16)
            .flex()
            .items_center()
            .justify_center()
            .gap(space::S6)
            .t_small()
            .text_color(k.text_3)
            .child(ui::icon(Lucide::Server, theme::icon::CHIP, k.text_3))
            .child(div().one_line().child(host))
            .child("·")
            .child(
                ui::text_button("login-server-change", l::CHANGE_SERVER, cx).on_click(cx.listener(
                    |this, _, window, cx| {
                        this.server_open = true;
                        this.server.read(cx).focus_handle(cx).focus(window, cx);
                        cx.notify();
                    },
                )),
            );
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
        assert_eq!(
            CodeInput::parse(" k7qm-2x9a bc3d "),
            CodeInput::Code("K7QM2X9ABC3D".into())
        );
        assert_eq!(
            CodeInput::parse("K7QM2X9ABC3DEXTRA"),
            CodeInput::Code("K7QM2X9ABC3D".into())
        );
        assert_eq!(
            CodeInput::parse("k7qm2x9abc3d").credential(),
            Some(Credential::InviteCode("K7QM-2X9A-BC3D".into()))
        );
        assert_eq!(CodeInput::parse("K7QM-2X").credential(), None);
        assert!(matches!(
            CodeInput::parse(" tc_abc ").credential(),
            Some(Credential::Token(_))
        ));
    }

    #[test]
    fn server_line_shows_the_host() {
        assert_eq!(server_host("https://team.joye.co/"), "team.joye.co");
        assert_eq!(server_host("http://127.0.0.1:8787"), "127.0.0.1:8787");
    }
}
