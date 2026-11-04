//! The connection Alert docked above the composer while the connection is
//! not up (COMPLETENESS.md §2.15): connecting, reconnecting (with the next
//! retry) or offline. Hidden when online or signed out.

use gpui::{
    Context, InteractiveElement as _, IntoElement, ParentElement as _, Render, Styled as _,
    Subscription, Window, div,
};

use crate::{
    app::{Changes, Services},
    copy::{connection as c, fill},
    data::{Change, ConnectionState, SessionState},
    ui::Sym,
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
        ConnectionState::TokenUnreadable => Some(c::TOKEN_UNREADABLE.to_string()),
    }
}

/// The banner's title and detail: the sidebar is narrow, so the title is the
/// short state and the detail says what happens next.
pub fn banner_lines(state: ConnectionState) -> Option<(&'static str, String)> {
    Some(match state {
        ConnectionState::Online => return None,
        ConnectionState::Connecting => (c::CONNECTING, c::QUEUED_HINT.to_string()),
        ConnectionState::Reconnecting { retry_in_ms, .. } if retry_in_ms >= 1000 => (
            c::BANNER_DISCONNECTED,
            fill(
                c::BANNER_RETRY_IN,
                &[("s", &retry_in_ms.div_ceil(1000).to_string())],
            ),
        ),
        ConnectionState::Reconnecting { .. } => {
            (c::BANNER_DISCONNECTED, c::RECONNECTING_NOW.to_string())
        }
        ConnectionState::Offline => (c::OFFLINE, c::QUEUED_HINT.to_string()),
        ConnectionState::TokenUnreadable => {
            (c::TOKEN_UNREADABLE, c::TOKEN_UNREADABLE_HINT.to_string())
        }
    })
}

impl Render for ConnectionBanner {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let provider = &self.services.provider;
        let signed_in = matches!(provider.session(), SessionState::SignedIn { .. });
        let state = provider.connection();
        let Some((title, detail)) = banner_lines(state).filter(|_| signed_in) else {
            return div().into_any_element();
        };
        // COMPLETENESS.md §2.15: a warning Alert docked above the composer
        // (title, the retry time, an outline sm "立即重连"); never red.
        let can_retry = !matches!(state, ConnectionState::Connecting);
        let retry = can_retry.then(|| {
            crate::ui::native::push_button(
                "reconnect-now",
                if state == ConnectionState::TokenUnreadable {
                    c::RETRY_TOKEN
                } else {
                    c::RECONNECT
                },
                crate::ui::native::PushStyle::Outline,
                crate::ui::native::PushSize::Small,
                cx,
            )
            .on_click(cx.listener(|this, _, _, _| this.services.provider.reconnect_now()))
        });
        let mut alert = crate::ui::native::Alert::new(Sym::Offline, title)
            .tone(crate::ui::native::AlertTone::Warning)
            .description(detail);
        if let Some(r) = retry {
            alert = alert.action(r);
        }
        div()
            .id("connection-status")
            .flex_none()
            .w_full()
            .child(alert)
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
        assert_eq!(
            banner_lines(ConnectionState::TokenUnreadable).map(|(t, _)| t),
            Some(c::TOKEN_UNREADABLE)
        );
    }
}
