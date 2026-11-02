//! The app-level connection banner above the sidebar footer while the connection is not up:
//! connecting, reconnecting (with the next retry) or offline. Hidden when
//! online or signed out.

use gpui::{Context, IntoElement, Render, Subscription, Window, div};
use gpui_component::button::{ButtonCustomVariant, ButtonVariants as _};

use crate::{
    app::{Changes, Services},
    copy::{connection as c, fill},
    data::{Change, ConnectionState, SessionState},
    theme,
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
        // An app-level banner (DESIGN.md §8.6): a neutral 48 pt card above
        // the sidebar footer. Never red: being offline is not an error the
        // user made, and messages still queue.
        let can_retry = !matches!(state, ConnectionState::Connecting);
        let colors = theme::colors(cx);
        let actions = if can_retry {
            vec![
                ui::button(
                    "reconnect-now",
                    c::RECONNECT,
                    ui::ButtonKind::Ghost,
                    ui::ButtonSize::Sm,
                    cx,
                )
                .custom(
                    ButtonCustomVariant::new(cx)
                        .foreground(colors.accent_text)
                        .hover(colors.hover)
                        .active(colors.pressed),
                )
                .on_click(cx.listener(|this, _, _, _| this.services.provider.reconnect_now()))
                .into_any_element(),
            ]
        } else {
            Vec::new()
        };
        crate::views::banners::Card {
            id: "connection-banner",
            glyph: ui::icon(Lucide::WifiOff, theme::icon::ROW, colors.text_2).into_any_element(),
            title: title.into(),
            detail: Some(crate::views::banners::detail_text(detail)),
            actions,
            tooltip: None,
        }
        .render(cx)
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
