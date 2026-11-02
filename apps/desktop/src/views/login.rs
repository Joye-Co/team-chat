//! The login view: server URL and an invite code or a pasted credential.
//! The server URL defaults to the production URL from product.json, then staging.

use gpui::{
    AppContext, Context, Entity, IntoElement, ParentElement, Render, Styled, Subscription, Window,
    div, img, prelude::FluentBuilder as _, px,
};
use gpui_component::{
    Disableable as _, Sizable as _,
    button::{Button, ButtonVariants as _},
    input::{Input, InputEvent, InputState},
};

use crate::{
    app::{Changes, Services},
    copy::{fill, login as l},
    data::{Change, Credential, LoginError, LoginRequest, SessionState},
    theme::{self, Type as _, space},
    ui,
};

pub fn default_server_url() -> &'static str {
    let production = env!("PRODUCT_PRODUCTION_URL");
    if production.is_empty() {
        env!("PRODUCT_STAGING_URL")
    } else {
        production
    }
}

pub struct LoginView {
    services: Services,
    server: Entity<InputState>,
    code: Entity<InputState>,
    local_error: Option<&'static str>,
    _subscriptions: Vec<Subscription>,
}

impl LoginView {
    pub fn new(services: Services, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let server = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder(l::SERVER_PLACEHOLDER)
                .default_value(default_server_url())
        });
        let code = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder(l::CODE_PLACEHOLDER)
                .masked(true)
        });
        let subscriptions = vec![
            cx.subscribe_in(&code, window, |this, _, event: &InputEvent, window, cx| {
                if let InputEvent::PressEnter { .. } = event {
                    this.submit(window, cx);
                }
            }),
            cx.subscribe(&services.hub, |_, _, changes: &Changes, cx| {
                if changes.any(|c| matches!(c, Change::Session)) {
                    cx.notify();
                }
            }),
        ];
        Self {
            services,
            server,
            code,
            local_error: None,
            _subscriptions: subscriptions,
        }
    }

    fn submit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let server_url = self.server.read(cx).value().trim().to_string();
        let credential = Credential::from_input(&self.code.read(cx).value());
        let (Some(credential), false) = (credential, server_url.is_empty()) else {
            self.local_error = Some(l::ERROR_EMPTY);
            cx.notify();
            return;
        };
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
}

impl Render for LoginView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let colors = theme::colors(cx);
        let session = self.services.provider.session();
        let signing_in = matches!(session, SessionState::SigningIn);
        let error = self.local_error.or(match session {
            SessionState::SignedOut {
                last_error: Some(e),
            } => Some(match e {
                LoginError::BadCredential => l::ERROR_BAD_CREDENTIAL,
                LoginError::ServerUnreachable => l::ERROR_UNREACHABLE,
                LoginError::Unsupported => l::ERROR_UNSUPPORTED,
                LoginError::Other => l::ERROR_OTHER,
            }),
            _ => None,
        });
        let label = |text: &'static str| {
            div()
                .pt(space::S12)
                .pb(space::S6)
                .t_callout()
                .medium()
                .text_color(colors.text_2)
                .child(text)
        };
        let footer = fill(l::FOOTER, &[("version", crate::updater::version::CURRENT)]);
        div()
            .size_full()
            .flex()
            .flex_col()
            .bg(colors.content)
            .child(ui::toolbar_band("login-toolbar"))
            .child(
                div()
                    .flex_1()
                    .min_h(px(0.))
                    .flex()
                    .flex_col()
                    .items_center()
                    .child(
                        div()
                            .w(px(344.))
                            .flex()
                            .flex_col()
                            .child(
                                div()
                                    .pt(space::S28)
                                    .flex()
                                    .justify_center()
                                    .child(img(ui::icons::APP_ICON_88).size(px(88.))),
                            )
                            .child(
                                div()
                                    .pt(space::S20)
                                    .text_center()
                                    .t_display()
                                    .child(fill(l::WELCOME, &[("product", env!("PRODUCT_NAME"))])),
                            )
                            .child(
                                div()
                                    .pt(space::S6)
                                    .pb(space::S16)
                                    .text_center()
                                    .t_ui()
                                    .text_color(colors.text_2)
                                    .child(l::LEAD),
                            )
                            .child(label(l::SERVER_LABEL))
                            .child(Input::new(&self.server))
                            .child(label(l::CODE_LABEL))
                            .child(Input::new(&self.code))
                            .child(
                                div()
                                    .pt(space::S8)
                                    .t_callout()
                                    .text_color(colors.text_2)
                                    .child(l::CODE_HINT),
                            )
                            .when_some(error, |el, e| {
                                el.child(
                                    div()
                                        .pt(space::S8)
                                        .t_callout()
                                        .text_color(colors.danger_text)
                                        .child(e),
                                )
                            })
                            .child(
                                div().pt(space::S20).child(
                                    Button::new("login")
                                        .primary()
                                        .large()
                                        .w_full()
                                        .label(if signing_in { l::SIGNING_IN } else { l::SUBMIT })
                                        .disabled(signing_in)
                                        .on_click(cx.listener(|this, _, window, cx| {
                                            this.submit(window, cx)
                                        })),
                                ),
                            ),
                    ),
            )
            .child(
                div()
                    .flex_none()
                    .pb(space::S16)
                    .text_center()
                    .t_caption()
                    .font_weight(gpui::FontWeight::NORMAL)
                    .tnum()
                    .text_color(colors.text_3)
                    .child(footer),
            )
    }
}
