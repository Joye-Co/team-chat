//! The settings window: my profile, 成员, 机器, 通知, 成本, diagnostics, sign out
//! and the developer-view switch. Sections other agents fill.

use gpui::{
    Context, InteractiveElement, IntoElement, ParentElement, Render, StatefulInteractiveElement,
    Styled, Subscription, Window, div, prelude::FluentBuilder as _, px,
};
use gpui_component::{
    button::{Button, ButtonVariants as _},
    switch::Switch,
};

use crate::{
    app::{Changes, Services},
    copy::{common, dev, fill, settings as s},
    data::{Change, ConnectionState, MemberLevel, SessionState},
    theme,
    views::{avatar, presence_dot},
};

pub struct SettingsView {
    services: Services,
    _subscriptions: Vec<Subscription>,
}

impl SettingsView {
    pub fn new(services: Services, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let subscriptions = vec![
            theme::follow_system(window, cx),
            cx.subscribe(&services.hub, |_, _, changes: &Changes, cx| {
                if changes
                    .any(|c| matches!(c, Change::Members | Change::Session | Change::Connection))
                {
                    cx.notify();
                }
            }),
            cx.observe(&services.nav, |_, _, cx| cx.notify()),
        ];
        Self {
            services,
            _subscriptions: subscriptions,
        }
    }
}

impl Render for SettingsView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let colors = theme::colors(cx);
        let provider = self.services.provider.clone();
        let session = provider.session();
        let me = match &session {
            SessionState::SignedIn { me, .. } => Some(me.clone()),
            _ => None,
        };
        let developer = self.services.nav.read(cx).developer_view;
        let heading = |text: &'static str| div().pt(px(16.)).pb(px(6.)).text_base().child(text);
        let members: Vec<_> = provider
            .members()
            .into_iter()
            .map(|m| {
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .py(px(4.))
                    .child(avatar(Some(&m), px(24.), cx))
                    .child(presence_dot(m.online, cx))
                    .child(div().w(px(120.)).child(m.name.clone()))
                    .child(
                        div()
                            .flex_1()
                            .text_color(colors.muted_foreground)
                            .child(m.role.clone().unwrap_or_default()),
                    )
                    .child(div().text_xs().child(match m.level {
                        MemberLevel::Owner => s::LEVEL_OWNER,
                        MemberLevel::Member => s::LEVEL_MEMBER,
                    }))
                    .when(m.deactivated, |el| {
                        el.child(
                            div()
                                .text_xs()
                                .text_color(colors.muted_foreground)
                                .child(common::MEMBER_DEACTIVATED),
                        )
                    })
                    .into_any_element()
            })
            .collect();

        div()
            .id("settings")
            .size_full()
            .overflow_y_scroll()
            .bg(colors.background)
            .text_color(colors.foreground)
            .text_sm()
            .p(px(20.))
            .child(heading(s::PROFILE))
            .when_some(me, |el, me| {
                el.child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(10.))
                        .child(avatar(Some(&me), px(40.), cx))
                        .child(
                            div().flex().flex_col().child(me.name.clone()).child(
                                div()
                                    .text_color(colors.muted_foreground)
                                    .child(me.role.clone().unwrap_or_default()),
                            ),
                        ),
                )
            })
            .child(heading(s::MEMBERS))
            .children(members)
            .child(heading(s::MACHINES))
            .child(
                div()
                    .text_color(colors.muted_foreground)
                    .child(s::MACHINES_EMPTY),
            )
            .child(heading(s::NOTIFICATIONS))
            .child(
                div()
                    .text_color(colors.muted_foreground)
                    .child(s::MUTE_GROUP),
            )
            .child(heading(s::COST))
            .child(
                div()
                    .text_color(colors.muted_foreground)
                    .child(s::COMING_SOON),
            )
            .child(heading(s::DIAGNOSTICS))
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
            .child(
                div()
                    .text_xs()
                    .text_color(colors.muted_foreground)
                    .child(s::DEVELOPER_VIEW_HINT),
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
                let server = match &session {
                    SessionState::SignedIn { server_url, .. } => server_url.clone(),
                    _ => String::new(),
                };
                el.child(
                    div()
                        .pt(px(8.))
                        .text_xs()
                        .child(fill(dev::CONNECTION, &[("state", &connection)])),
                )
                .child(
                    div()
                        .text_xs()
                        .child(fill(dev::SERVER, &[("url", &server)])),
                )
            })
            .child(
                div()
                    .pt(px(20.))
                    .flex()
                    .flex_col()
                    .gap(px(8.))
                    .child(
                        div()
                            .flex()
                            .gap(px(8.))
                            .child(
                                Button::new("sign-out")
                                    .danger()
                                    .label(s::SIGN_OUT)
                                    .on_click(cx.listener(|this, _, _, _| {
                                        this.services.provider.sign_out(false)
                                    })),
                            )
                            .child(
                                Button::new("sign-out-wipe")
                                    .danger()
                                    .outline()
                                    .label(s::SIGN_OUT_WIPE)
                                    .on_click(cx.listener(|this, _, _, _| {
                                        this.services.provider.sign_out(true)
                                    })),
                            ),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(colors.muted_foreground)
                            .child(s::SIGN_OUT_HINT),
                    ),
            )
    }
}
