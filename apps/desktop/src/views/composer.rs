//! The composer: a multi-line field (Chinese IME works through the platform
//! input handler). Enter sends, Shift+Enter adds a line. Without an open topic
//! it starts a new topic in the current 群; the first line becomes the title.

use gpui::{
    AppContext, Context, Entity, IntoElement, ParentElement, Render, Styled, Subscription, Window,
    div, prelude::FluentBuilder as _, px,
};
use gpui_component::{
    Disableable as _,
    button::{Button, ButtonVariants as _},
    input::{InputEvent, Textarea, TextareaState},
};

use crate::{
    app::Services,
    copy::composer as c,
    data::{SendRequest, StartTopicRequest},
    theme,
};

/// Longest body the protocol accepts.
pub const MAX_BODY_CHARS: usize = 32_768;

pub struct ComposerView {
    services: Services,
    input: Entity<TextareaState>,
    too_long: bool,
    _subscriptions: Vec<Subscription>,
}

impl ComposerView {
    pub fn new(services: Services, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let input = cx.new(|cx| {
            TextareaState::new(window, cx)
                .auto_grow(1, 8)
                .submit_on_enter(true)
                .placeholder(c::PLACEHOLDER)
        });
        let subscriptions =
            vec![
                cx.subscribe_in(&input, window, |this, _, event: &InputEvent, window, cx| {
                    match event {
                        InputEvent::PressEnter { shift: false, .. } => this.submit(window, cx),
                        InputEvent::Change => {
                            this.too_long = false;
                            this.typing(cx);
                        }
                        _ => {}
                    }
                }),
                cx.observe_in(&services.nav, window, |this, nav, window, cx| {
                    let placeholder = if nav.read(cx).topic.is_some() {
                        c::PLACEHOLDER
                    } else {
                        c::PLACEHOLDER_NEW_TOPIC
                    };
                    this.input.update(cx, |state, cx| {
                        state.set_placeholder(placeholder, window, cx)
                    });
                    cx.notify();
                }),
            ];
        Self {
            services,
            input,
            too_long: false,
            _subscriptions: subscriptions,
        }
    }

    fn typing(&self, cx: &mut Context<Self>) {
        if let Some((g, t)) = self.services.nav.read(cx).topic.clone() {
            self.services.provider.set_typing(&g, &t);
        }
    }

    /// Sends the text, or starts a topic when none is open.
    pub fn submit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let body = self.input.read(cx).value().trim().to_string();
        if body.is_empty() {
            return;
        }
        if body.chars().count() > MAX_BODY_CHARS {
            self.too_long = true;
            cx.notify();
            return;
        }
        let nav = self.services.nav.read(cx);
        let open = nav.topic.clone();
        let group = nav.current_group();
        let provider = self.services.provider.clone();
        match (open, group) {
            (Some((group_id, topic_id)), _) => {
                provider.send(SendRequest {
                    group_id,
                    topic_id,
                    body,
                    mentions: Vec::new(),
                    quote_of: None,
                });
            }
            (None, Some(group_id)) => {
                provider.start_topic(StartTopicRequest {
                    group_id: group_id.clone(),
                    title: None,
                    body,
                    mentions: Vec::new(),
                });
                // Open the new topic: it is the newest one in the group.
                if let Some(topic) = provider.topics(&group_id).into_iter().next() {
                    self.services
                        .nav
                        .update(cx, |nav, cx| nav.open_topic(group_id, topic.id, cx));
                }
            }
            (None, None) => return,
        }
        self.input
            .update(cx, |state, cx| state.set_value("", window, cx));
    }
}

impl Render for ComposerView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let colors = theme::colors(cx);
        let enabled = self.services.nav.read(cx).current_group().is_some();
        div()
            .p(px(12.))
            .border_t_1()
            .border_color(colors.border)
            .flex()
            .flex_col()
            .gap(px(6.))
            .when(self.too_long, |el| {
                el.child(div().text_xs().text_color(colors.danger).child(c::TOO_LONG))
            })
            .child(
                div()
                    .flex()
                    .items_end()
                    .gap(px(8.))
                    .child(
                        div()
                            .flex_1()
                            .child(Textarea::new(&self.input).disabled(!enabled)),
                    )
                    .child(
                        Button::new("send")
                            .primary()
                            .label(c::SEND)
                            .disabled(!enabled)
                            .on_click(cx.listener(|this, _, window, cx| this.submit(window, cx))),
                    ),
            )
    }
}
