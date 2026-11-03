//! A neutral band above the composer while the connection is not up:
//! connecting, reconnecting (with the next retry) or offline. Hidden when
//! online or signed out.

use gpui::{
    Context, IntoElement, ParentElement, Render, Styled, Subscription, Window, div,
    prelude::FluentBuilder as _,
};

use crate::{
    app::{Changes, Services},
    copy::{connection as c, fill},
    data::{Change, ConnectionState, SessionState},
    theme::{self, space},
    ui::{self, Lucide},
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
        // A neutral band (DESIGN.md §11, §13.14): never red, no modal; the
        // composer stays usable and queued messages show 发送中.
        let icon = match state {
            ConnectionState::Offline => Lucide::WifiOff,
            _ => Lucide::Loader,
        };
        let can_retry = !matches!(state, ConnectionState::Connecting);
        div()
            .flex_none()
            .w_full()
            .px(space::S20)
            .pb(space::S8)
            .bg(theme::colors(cx).content)
            .child(ui::Band::new(icon, text).when(can_retry, |band| {
                band.trailing(
                    crate::views::timeline::text_action(
                        "reconnect-now",
                        c::RECONNECT,
                        theme::colors(cx).accent_text,
                    )
                    .on_click(cx.listener(
                        |this, _, _, _| this.services.provider.reconnect_now(),
                    )),
                )
            }))
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
            Some("网络已断开，2 秒后重新连接…")
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
