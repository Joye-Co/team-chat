//! 关于: version, server, update status, 发送诊断, and the developer-view
//! switch.

use gpui::{
    AppContext as _, Context, Entity, IntoElement, ParentElement, Render, Styled, Subscription,
    Window, div, prelude::FluentBuilder as _, px,
};
use gpui_component::{
    Disableable as _,
    button::Button,
    input::{InputEvent, Textarea, TextareaState},
    switch::Switch,
};

use super::widgets::{self, Status, answer};
use crate::{
    app::{Changes, Services},
    copy::{dev, fill, settings as s},
    data::{Change, ConnectionState, SessionState},
    theme,
    updater::banner::AboutUpdate,
};

pub const DIAGNOSTICS_MAX: usize = 2000;

pub struct AboutPage {
    services: Services,
    /// The updater's 关于 section: version, update state and 检查更新.
    update: Entity<AboutUpdate>,
    description: Entity<TextareaState>,
    status: Status,
    _subscriptions: Vec<Subscription>,
}

impl AboutPage {
    pub fn new(services: Services, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let description = cx.new(|cx| {
            TextareaState::new(window, cx)
                .auto_grow(3, 8)
                .placeholder(s::DIAGNOSTICS_PLACEHOLDER)
        });
        let subscriptions = vec![
            cx.subscribe(&services.hub, |_, _, changes: &Changes, cx| {
                if changes.any(|c| matches!(c, Change::Session | Change::Connection)) {
                    cx.notify();
                }
            }),
            cx.observe(&services.nav, |_, _, cx| cx.notify()),
            cx.subscribe(&description, |this, _, e: &InputEvent, cx| {
                if matches!(e, InputEvent::Change) && matches!(this.status, Status::Failed(_)) {
                    this.status = Status::Idle;
                    cx.notify();
                }
            }),
        ];
        Self {
            services,
            update: cx.new(AboutUpdate::new),
            description,
            status: Status::Idle,
            _subscriptions: subscriptions,
        }
    }

    pub fn status(&self) -> &Status {
        &self.status
    }

    pub fn set_description(&mut self, text: &str, window: &mut Window, cx: &mut Context<Self>) {
        self.description
            .update(cx, |st, cx| st.set_value(text.to_string(), window, cx));
    }

    pub fn send(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.status.busy() {
            return;
        }
        let text = self.description.read(cx).value().trim().to_string();
        if text.is_empty() {
            self.status = Status::Failed(s::DIAGNOSTICS_REQUIRED.into());
            cx.notify();
            return;
        }
        if let Err(e) = widgets::check_len(&text, DIAGNOSTICS_MAX) {
            self.status = Status::Failed(e.into());
            cx.notify();
            return;
        }
        self.status = Status::Busy(s::DIAGNOSTICS_SENDING.into());
        cx.notify();
        let reply = self.services.provider.send_diagnostics(text);
        let description = self.description.downgrade();
        let window_handle = window.window_handle();
        cx.spawn(async move |this, cx| {
            let result = answer(reply).await;
            let sent = result.is_ok();
            let _ = this.update(cx, |this, cx| {
                this.status = match result {
                    Ok(id) => Status::Done(fill(s::DIAGNOSTICS_SENT, &[("id", &id)]).into()),
                    Err(e) => Status::from_error(e),
                };
                cx.notify();
            });
            if sent {
                let _ = window_handle.update(cx, |_, window, cx| {
                    if let Some(d) = description.upgrade() {
                        d.update(cx, |st, cx| st.set_value(String::new(), window, cx));
                    }
                });
            }
        })
        .detach();
    }
}

impl Render for AboutPage {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let colors = theme::colors(cx);
        let provider = self.services.provider.clone();
        let session = provider.session();
        let server = match &session {
            SessionState::SignedIn { server_url, .. } => server_url.clone(),
            _ => String::new(),
        };
        let developer = self.services.nav.read(cx).developer_view;
        let busy = self.status.busy();
        let value = |text: String| div().flex_1().min_w_0().truncate().child(text);
        div()
            .flex()
            .flex_col()
            .child(self.update.clone())
            .child(
                div()
                    .pt(px(8.))
                    .child(widgets::field(s::SERVER, value(server.clone()), cx)),
            )
            .child(
                widgets::section(s::DIAGNOSTICS, cx)
                    .child(widgets::hint(s::DIAGNOSTICS_HINT, cx))
                    .child(Textarea::new(&self.description))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(12.))
                            .child(
                                Button::new("diagnostics-send")
                                    .outline()
                                    .label(s::DIAGNOSTICS_SEND)
                                    .loading(busy)
                                    .disabled(busy || server.is_empty())
                                    .on_click(
                                        cx.listener(|this, _, window, cx| this.send(window, cx)),
                                    ),
                            )
                            .children(widgets::status_line(&self.status, cx)),
                    ),
            )
            .child(
                widgets::section(s::DEVELOPER_VIEW, cx)
                    .child(
                        Switch::new("developer-view")
                            .checked(developer)
                            .label(s::DEVELOPER_VIEW)
                            .on_click(cx.listener(|this, checked: &bool, _, cx| {
                                let on = *checked;
                                this.services
                                    .nav
                                    .update(cx, |nav, cx| nav.set_developer_view(on, cx));
                            })),
                    )
                    .child(widgets::hint(s::DEVELOPER_VIEW_HINT, cx))
                    .when(developer, |el| {
                        let connection = match provider.connection() {
                            ConnectionState::Offline => dev::STATE_OFFLINE.to_string(),
                            ConnectionState::Connecting => dev::STATE_CONNECTING.to_string(),
                            ConnectionState::Online => dev::STATE_ONLINE.to_string(),
                            ConnectionState::Reconnecting {
                                attempt,
                                retry_in_ms,
                            } => fill(
                                dev::STATE_RECONNECTING,
                                &[
                                    ("attempt", &attempt.to_string()),
                                    ("ms", &retry_in_ms.to_string()),
                                ],
                            ),
                        };
                        let me = match &session {
                            SessionState::SignedIn { me, .. } => me.id.clone(),
                            _ => String::new(),
                        };
                        el.child(
                            div()
                                .flex()
                                .flex_col()
                                .gap(px(2.))
                                .p(px(8.))
                                .rounded(px(6.))
                                .bg(colors.code_background)
                                .font_family("Menlo")
                                .text_xs()
                                .child(fill(dev::CONNECTION, &[("state", &connection)]))
                                .child(fill(dev::SERVER, &[("url", &server)]))
                                .child(fill(dev::MEMBER_ID, &[("id", &me)])),
                        )
                    }),
            )
    }
}
