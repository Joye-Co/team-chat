//! A thin banner above the topic view while the connection is not up:
//! connecting, reconnecting (with the next retry) or offline. Hidden when
//! online or signed out.

use gpui::{
    Context, IntoElement, ParentElement, Render, Styled, Subscription, Window, div,
    prelude::FluentBuilder as _, px,
};
use gpui_component::{
    Sizable as _,
    button::{Button, ButtonVariants as _},
};

use crate::{
    app::{Changes, Services},
    copy::{connection as c, fill},
    data::{Change, ConnectionState, SessionState},
    theme,
};

pub struct ConnectionBanner {
    services: Services,
    _subscriptions: Vec<Subscription>,
}

impl ConnectionBanner {
    pub fn new(services: Services, cx: &mut Context<Self>) -> Self {
        let subscriptions = vec![cx.subscribe(&services.hub, |_, _, changes: &Changes, cx| {
            if changes.any(|c| matches!(c, Change::Connection | Change::Session)) {
                cx.notify();
            }
        })];
        Self {
            services,
            _subscriptions: subscriptions,
        }
    }
}

/// The banner text for a state, or `None` when no banner shows.
pub fn banner_text(state: ConnectionState) -> Option<String> {
    match state {
        ConnectionState::Online => None,
        ConnectionState::Connecting => Some(c::CONNECTING.to_string()),
        ConnectionState::Reconnecting { retry_in_ms, .. } if retry_in_ms >= 1000 => {
            let s = retry_in_ms.div_ceil(1000).to_string();
            Some(fill(c::RECONNECTING_IN, &[("s", &s)]))
        }
        ConnectionState::Reconnecting { .. } => Some(c::RECONNECTING_NOW.to_string()),
        ConnectionState::Offline => Some(c::OFFLINE.to_string()),
    }
}

impl Render for ConnectionBanner {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let provider = &self.services.provider;
        let signed_in = matches!(provider.session(), SessionState::SignedIn { .. });
        let state = provider.connection();
        let Some(text) = banner_text(state).filter(|_| signed_in) else {
            return div().into_any_element();
        };
        let colors = theme::colors(cx);
        let can_retry = !matches!(state, ConnectionState::Connecting);
        div()
            .flex_none()
            .w_full()
            .px(px(16.))
            .py(px(6.))
            .flex()
            .items_center()
            .gap(px(8.))
            .bg(colors.panel)
            .border_b_1()
            .border_color(colors.border)
            .text_sm()
            .child(div().text_color(colors.danger).child(text))
            .child(
                div()
                    .flex_1()
                    .text_xs()
                    .text_color(colors.muted_foreground)
                    .child(c::QUEUED_HINT),
            )
            .when(can_retry, |el| {
                el.child(
                    Button::new("reconnect-now")
                        .small()
                        .ghost()
                        .label(c::RECONNECT)
                        .on_click(cx.listener(|this, _, _, _| {
                            this.services.provider.reconnect_now()
                        })),
                )
            })
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn banner_texts() {
        assert_eq!(banner_text(ConnectionState::Online), None);
        assert_eq!(
            banner_text(ConnectionState::Reconnecting {
                attempt: 2,
                retry_in_ms: 1500
            })
            .as_deref(),
            Some("连接已断开，2 秒后重连")
        );
        assert_eq!(
            banner_text(ConnectionState::Reconnecting {
                attempt: 2,
                retry_in_ms: 0
            })
            .as_deref(),
            Some(c::RECONNECTING_NOW)
        );
    }
}
