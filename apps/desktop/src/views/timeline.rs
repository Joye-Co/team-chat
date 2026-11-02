//! The topic view: header (title, status chip, panel toggle) and the message
//! timeline. The timeline is a GPUI `list` (variable row heights, virtualized,
//! anchored at the bottom). It keeps a window of the newest messages and grows
//! it with `load_older`.

use gpui::{
    Context, Entity, InteractiveElement, IntoElement, ListAlignment, ListState, ParentElement,
    Render, Styled, Subscription, Window, div, list, prelude::FluentBuilder as _, px,
};
use gpui_component::{
    Sizable as _,
    button::{Button, ButtonVariants as _},
};

use crate::{
    app::{Changes, Services},
    copy::{fill, timeline as t},
    data::{Change, GroupId, Member, Message, MessageWindow, SendState, Topic, TopicId},
    theme,
    views::{avatar, status_chip, time_text},
};

/// Messages read from the provider when a topic opens.
pub const WINDOW: usize = 100;

pub struct TimelineView {
    services: Services,
    open: Option<(GroupId, TopicId)>,
    topic: Option<Topic>,
    window: MessageWindow,
    members: Vec<Member>,
    typing: Vec<String>,
    list: ListState,
    _subscriptions: Vec<Subscription>,
}

impl TimelineView {
    pub fn new(services: Services, cx: &mut Context<Self>) -> Self {
        let subscriptions = vec![
            cx.subscribe(&services.hub, |this, _, changes: &Changes, cx| {
                let Some((g, tid)) = this.open.clone() else {
                    return;
                };
                let touches = changes.touches_topic(&g, &tid)
                    || changes.any(|c| matches!(c, Change::Topics { group_id } if *group_id == g))
                    || changes.any(|c| matches!(c, Change::Members));
                if touches {
                    this.refresh(cx);
                    cx.notify();
                }
            }),
            cx.observe(&services.nav, |this, nav, cx| {
                let open = nav.read(cx).topic.clone();
                if open != this.open {
                    this.open = open;
                    this.window = MessageWindow::default();
                    this.list.reset(0);
                    this.refresh(cx);
                }
                cx.notify();
            }),
        ];
        let open = services.nav.read(cx).topic.clone();
        let mut this = Self {
            services,
            open,
            topic: None,
            window: MessageWindow::default(),
            members: Vec::new(),
            typing: Vec::new(),
            list: ListState::new(0, ListAlignment::Bottom, px(800.)),
            _subscriptions: subscriptions,
        };
        this.refresh(cx);
        this
    }

    /// Re-reads the snapshot of the open topic and updates the list in place.
    fn refresh(&mut self, _cx: &mut Context<Self>) {
        let p = self.services.provider.clone();
        let Some((g, tid)) = self.open.clone() else {
            self.topic = None;
            self.window = MessageWindow::default();
            self.list.reset(0);
            return;
        };
        self.members = p.members();
        self.topic = p.topic(&g, &tid);
        self.typing = p.typing(&g, &tid);
        let limit = self.window.messages.len().max(WINDOW);
        let new = p.messages(&g, &tid, limit);
        let old = std::mem::take(&mut self.window);
        apply_window_change(&self.list, &old.messages, &new.messages);
        self.window = new;
        if let Some(topic) = &self.topic
            && topic.unread > 0
        {
            p.mark_read(&g, &tid, topic.last_seq);
        }
    }

    /// Number of messages in the window (for self-tests).
    pub fn message_count(&self) -> usize {
        self.window.messages.len()
    }

    fn load_older(&mut self, cx: &mut Context<Self>) {
        if let Some((g, tid)) = &self.open {
            self.services.provider.load_older(g, tid);
        }
        cx.notify();
    }

    fn render_message(&self, message: &Message, cx: &mut Context<Self>) -> impl IntoElement {
        let colors = theme::colors(cx);
        let author = self.members.iter().find(|m| m.id == message.author_id);
        let name = author.map(|m| m.name.clone()).unwrap_or_default();
        let deleted = message.deleted_at.is_some();
        let state = match message.send_state {
            SendState::Sent => None,
            SendState::Pending => Some((t::PENDING, colors.muted_foreground)),
            SendState::Failed => Some((t::FAILED, colors.danger)),
        };
        div()
            .px(px(16.))
            .py(px(6.))
            .flex()
            .gap(px(10.))
            .child(avatar(author, theme::AVATAR_SIZE, cx))
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.))
                    .flex()
                    .flex_col()
                    .gap(px(2.))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(6.))
                            .text_xs()
                            .child(
                                div()
                                    .text_sm()
                                    .font_weight(gpui::FontWeight::SEMIBOLD)
                                    .child(name),
                            )
                            .when_some(author.and_then(|a| a.role.clone()), |el, role| {
                                el.child(div().text_color(colors.muted_foreground).child(role))
                            })
                            .child(
                                div()
                                    .text_color(colors.muted_foreground)
                                    .child(time_text(message.created_at)),
                            )
                            .when(message.edited_at.is_some() && !deleted, |el| {
                                el.child(div().text_color(colors.muted_foreground).child(t::EDITED))
                            })
                            .when_some(state, |el, (text, color)| {
                                el.child(div().text_color(color).child(text))
                            }),
                    )
                    .child(if deleted {
                        div()
                            .text_sm()
                            .italic()
                            .text_color(colors.muted_foreground)
                            .child(t::DELETED)
                    } else {
                        div().text_sm().child(message.body.clone())
                    })
                    .when(message.streaming, |el| {
                        el.child(
                            div()
                                .text_xs()
                                .text_color(colors.muted_foreground)
                                .child(t::STREAMING),
                        )
                    }),
            )
    }
}

/// Updates `list` for a new window: prepends, appends and changed rows become
/// splices so the scroll position holds; anything else resets the list.
fn apply_window_change(list: &ListState, old: &[Message], new: &[Message]) {
    let same = |a: &Message, b: &Message| a.client_msg_id == b.client_msg_id;
    if old.is_empty() || new.is_empty() {
        list.reset(new.len());
        return;
    }
    let Some(start) = new.iter().position(|m| same(m, &old[0])) else {
        list.reset(new.len());
        return;
    };
    if new.len() < start + old.len() || !old.iter().zip(&new[start..]).all(|(a, b)| same(a, b)) {
        list.reset(new.len());
        return;
    }
    // Changed rows (edit, delete, ack, stream) are measured again.
    for (ix, (a, b)) in old.iter().zip(&new[start..]).enumerate() {
        if a != b {
            list.splice(ix..ix + 1, 1);
        }
    }
    let appended = new.len() - start - old.len();
    if appended > 0 {
        list.splice(old.len()..old.len(), appended);
    }
    if start > 0 {
        list.splice(0..0, start);
    }
}

impl Render for TimelineView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let colors = theme::colors(cx);
        let Some(topic) = self.topic.clone() else {
            let hint = if self.services.nav.read(cx).current_group().is_some() {
                t::NEW_TOPIC_HINT
            } else {
                t::SELECT_TOPIC
            };
            return div()
                .size_full()
                .flex()
                .items_center()
                .justify_center()
                .text_color(colors.muted_foreground)
                .child(hint)
                .into_any_element();
        };
        let me = self.services.me();
        let header = div()
            .h(px(48.))
            .px(px(16.))
            .flex()
            .items_center()
            .gap(px(8.))
            .border_b_1()
            .border_color(colors.border)
            .child(
                div()
                    .text_base()
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .text_ellipsis()
                    .child(topic.title.clone()),
            )
            .when_some(
                status_chip(&topic, me.as_deref(), &self.members, cx),
                |el, chip| el.child(chip),
            )
            .child(
                div()
                    .text_xs()
                    .text_color(colors.muted_foreground)
                    .child(fill(
                        t::MESSAGE_COUNT,
                        &[("n", &topic.message_count.to_string())],
                    )),
            )
            .child(div().flex_1())
            .child(
                Button::new("toggle-panel")
                    .small()
                    .ghost()
                    .label(t::TOGGLE_PANEL)
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.services
                            .nav
                            .update(cx, |nav, cx| nav.toggle_side_panel(cx))
                    })),
            );

        let older = if self.window.has_older {
            Some(
                div().flex().justify_center().py(px(4.)).child(
                    Button::new("load-older")
                        .small()
                        .ghost()
                        .label(if self.window.loading_older {
                            t::LOADING_OLDER
                        } else {
                            t::LOAD_OLDER
                        })
                        .on_click(cx.listener(|this, _, _, cx| this.load_older(cx))),
                ),
            )
        } else {
            None
        };

        let typing = match self.typing.as_slice() {
            [] => None,
            [one] => {
                let name = self
                    .members
                    .iter()
                    .find(|m| &m.id == one)
                    .map(|m| m.name.clone())
                    .unwrap_or_default();
                Some(fill(t::TYPING_ONE, &[("name", &name)]))
            }
            many => Some(fill(t::TYPING_MANY, &[("n", &many.len().to_string())])),
        };

        let view: Entity<Self> = cx.entity();
        let timeline = list(self.list.clone(), move |ix, _window, cx| {
            view.update(cx, |this, cx| match this.window.messages.get(ix).cloned() {
                Some(m) => this.render_message(&m, cx).into_any_element(),
                None => div().into_any_element(),
            })
        })
        .flex_1();

        div()
            .id("timeline")
            .size_full()
            .flex()
            .flex_col()
            .child(header)
            .when_some(older, |el, b| el.child(b))
            .child(timeline)
            .child(
                div()
                    .h(px(18.))
                    .px(px(16.))
                    .text_xs()
                    .text_color(colors.muted_foreground)
                    .when_some(typing, |el, text| el.child(text)),
            )
            .into_any_element()
    }
}
