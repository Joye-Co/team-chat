//! 通用: the about card (version, update status, server), 发送诊断, and the
//! developer-view switch.

use gpui::Focusable as _;
use gpui::{
    AppContext as _, Context, Entity, IntoElement, ParentElement, Render, Styled, Subscription,
    Window, div, prelude::FluentBuilder as _,
};
use gpui_component::{
    Disableable as _,
    input::{InputEvent, Textarea, TextareaState},
    switch::Switch,
};

use super::widgets::{self, Status, answer};
use crate::{
    app::{Changes, Services},
    copy::{dev, fill, settings as s},
    data::{Change, ConnectionState, SessionState},
    theme::{self, Type as _, space},
    ui,
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
        let failed = matches!(self.status, Status::Failed(_));
        let focused = self
            .description
            .read(cx)
            .focus_handle(cx)
            .is_focused(_window);
        div()
            .flex()
            .flex_col()
            .child(widgets::section_label(s::ABOUT, true, cx))
            .child(widgets::table_of(
                None,
                vec![
                    div().w_full().child(self.update.clone()),
                    widgets::table_row().child(widgets::title_cell(
                        s::SERVER,
                        Some(server.clone().into()),
                        cx,
                    )),
                ],
                cx,
            ))
            // 诊断: a form-style text area with its label, help and 发送诊断.
            .child(widgets::section_label(s::DIAGNOSTICS, false, cx))
            .child(
                div()
                    .w_full()
                    .px(space::S2)
                    .py(space::S2)
                    .rounded(theme::radius::FIELD)
                    .bg(colors.content)
                    .border_1()
                    .border_color(if failed {
                        colors.danger
                    } else if focused {
                        colors.focus_border
                    } else {
                        colors.border
                    })
                    .when(focused && !failed, |el| el.shadow(colors.focus_halo()))
                    .when(failed, |el| el.shadow(colors.error_halo()))
                    .child(
                        Textarea::new(&self.description)
                            .appearance(false)
                            .text_size(theme::type_scale::BODY.0)
                            .line_height(theme::type_scale::BODY.1),
                    ),
            )
            .child(
                div()
                    .pt(space::S8)
                    .flex()
                    .items_start()
                    .gap(space::S12)
                    .child(div().flex_1().min_w_0().pt(space::S2).child(
                        match widgets::status_line(&self.status, cx) {
                            Some(line) => line,
                            None => widgets::hint(s::DIAGNOSTICS_HINT, cx),
                        },
                    ))
                    .child(
                        ui::secondary_button("diagnostics-send", s::DIAGNOSTICS_SEND, cx)
                            .loading(busy)
                            .disabled(busy || server.is_empty())
                            .on_click(cx.listener(|this, _, window, cx| this.send(window, cx))),
                    ),
            )
            .child(
                widgets::table(cx)
                    .mt(space::S24)
                    .child(
                        widgets::table_row()
                            .child(widgets::title_cell(
                                s::DEVELOPER_VIEW,
                                Some(s::DEVELOPER_VIEW_HINT.into()),
                                cx,
                            ))
                            .child(Switch::new("developer-view").checked(developer).on_click(
                                cx.listener(|this, checked: &bool, _, cx| {
                                    let on = *checked;
                                    this.services
                                        .nav
                                        .update(cx, |nav, cx| nav.set_developer_view(on, cx));
                                }),
                            )),
                    )
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
                            div().px(space::S16).pb(space::S16).child(
                                div()
                                    .w_full()
                                    .flex()
                                    .flex_col()
                                    .gap(space::S2)
                                    .px(space::S12)
                                    .py(space::S10)
                                    .rounded(theme::radius::MD)
                                    .bg(colors.code_bg)
                                    .text_color(colors.code_text)
                                    .t_code()
                                    .child(fill(dev::CONNECTION, &[("state", &connection)]))
                                    .child(fill(dev::SERVER, &[("url", &server)]))
                                    .child(fill(dev::MEMBER_ID, &[("id", &me)])),
                            ),
                        )
                    }),
            )
    }
}
