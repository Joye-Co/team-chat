//! 单独问 (SideChat.dc.html, ⌘;): a private aside with one member about the
//! open 话题. It reads the 话题 up to now; answers stay private; a suggestion
//! posts into the 话题 only when pressed.
//!
//! Foundation stub: the thread, the suggestion button and a plain input are
//! wired to the provider; the side chat track builds the artboard panel
//! (member picker chip, ref chips, the 96 pt composer, answer rise-in).

pub mod scenes;

use gpui::{
    AppContext as _, Context, Entity, InteractiveElement as _, IntoElement, ParentElement, Pixels,
    Render, SharedString, StatefulInteractiveElement as _, Styled, Subscription, Window, div, px,
};
use gpui_component::input::{Input, InputEvent, InputState};

use crate::{
    app::{RightPane, Services},
    copy::{fill, side_chat as c},
    theme::{self, Type as _, space},
    ui::{self, ButtonKind, ButtonSize},
    views::shell::{PaneView, clock, identity, member_name, panes::watch},
};

pub struct SideChat {
    services: Services,
    input: Entity<InputState>,
    _subscriptions: Vec<Subscription>,
}

impl SideChat {
    pub fn new(services: Services, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let name = services
            .provider
            .side_chat_default_member()
            .map(|m| member_name(&services, &m))
            .unwrap_or_default();
        let placeholder = fill(c::SIDE_CHAT_PLACEHOLDER, &[("name", &name)]);
        let input = cx.new(|cx| InputState::new(window, cx).placeholder(placeholder));
        let mut subs = watch(&services, cx);
        subs.push(cx.subscribe_in(
            &input,
            window,
            |this: &mut Self, _, event: &InputEvent, window, cx| {
                if let InputEvent::PressEnter { .. } = event {
                    this.send(window, cx);
                }
            },
        ));
        Self {
            services,
            input,
            _subscriptions: subs,
        }
    }

    /// The (group, topic, member) of the open aside.
    fn target(&self, cx: &gpui::App) -> Option<(String, String, String)> {
        let nav = self.services.nav.read(cx);
        let (g, t) = nav.topic.clone()?;
        match nav.visible_pane() {
            RightPane::SideChat(m) => Some((g, t, m)),
            _ => None,
        }
    }

    fn send(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let body = self.input.read(cx).value().trim().to_string();
        let Some((g, t, m)) = self.target(cx) else {
            return;
        };
        if body.is_empty() {
            return;
        }
        self.services.provider.send_side_chat(&g, &t, &m, body);
        self.input.update(cx, |s, cx| s.set_value("", window, cx));
    }
}

impl PaneView for SideChat {
    fn width(&self, _window_w: Pixels) -> Pixels {
        theme::SIDE_CHAT_WIDTH
    }
}

impl Render for SideChat {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let k = theme::colors(cx);
        let Some((g, t, m)) = self.target(cx) else {
            return div().into_any_element();
        };
        let name = member_name(&self.services, &m);
        let thread = self.services.provider.side_chat(&g, &t, &m);
        let messages = thread.messages.iter().enumerate().map(|(i, msg)| {
            let suggestion = msg.suggestion.as_ref().map(|s| match s.posted_at {
                Some(at) => div()
                    .t_meta()
                    .text_color(k.ink_3)
                    .child(fill(c::SIDE_CHAT_POSTED, &[("time", &clock(at))]))
                    .into_any_element(),
                None => {
                    let (services, g, t, m, id) = (
                        self.services.clone(),
                        g.clone(),
                        t.clone(),
                        m.clone(),
                        msg.id.clone(),
                    );
                    ui::button(
                        SharedString::from(format!("side-chat-suggest-{}", msg.id)),
                        s.text.clone(),
                        ButtonKind::Secondary,
                        ButtonSize::Sm,
                        cx,
                    )
                    .on_click(move |_, _, _| {
                        services.provider.post_side_chat_suggestion(&g, &t, &m, &id)
                    })
                    .into_any_element()
                }
            });
            let row = div()
                .flex()
                .flex_col()
                .gap(space::S6)
                .child(identity(&self.services, &msg.author_id, None, cx))
                .child(div().t_body().text_color(k.ink).child(msg.body.clone()))
                .children(msg.refs.iter().map(|r| {
                    div()
                        .t_mono()
                        .text_color(k.ink_2)
                        .px(space::S6)
                        .rounded(theme::TAG_RADIUS)
                        .bg(k.sunken)
                        .child(r.clone())
                }))
                .children(suggestion);
            ui::motion::rise(
                SharedString::from(format!("side-chat-{}", msg.id)),
                i,
                row,
                cx,
            )
        });
        let empty = thread.messages.is_empty().then(|| {
            div()
                .t_small()
                .text_color(k.ink_3)
                .child(fill(c::SIDE_CHAT_EMPTY, &[("name", &name)]))
        });
        div()
            .size_full()
            .flex()
            .flex_col()
            .bg(k.bg)
            .child(
                div()
                    .flex_none()
                    .h(theme::TITLE_BAR)
                    .px(space::S16)
                    .flex()
                    .items_center()
                    .gap(space::S8)
                    .child(
                        div()
                            .t_ui()
                            .semibold()
                            .text_color(k.ink)
                            .child(c::SIDE_CHAT_TITLE),
                    )
                    .child(identity(&self.services, &m, None, cx)),
            )
            .child(
                div()
                    .px(space::S16)
                    .t_meta()
                    .text_color(k.ink_3)
                    .child(c::SIDE_CHAT_PRIVATE),
            )
            .child(
                div()
                    .id("side-chat-thread")
                    .flex_1()
                    .min_h(px(0.))
                    .overflow_y_scroll()
                    .p(space::S16)
                    .flex()
                    .flex_col()
                    .gap(space::S16)
                    .children(empty)
                    .children(messages),
            )
            .child(
                div()
                    .flex_none()
                    .p(space::S12)
                    .child(Input::new(&self.input)),
            )
            .into_any_element()
    }
}
