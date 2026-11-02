//! The middle column: the topics of the selected 群 (or the 待办 list), with
//! title, status chip, last activity and unread count, and the 新话题 button.
//! Rows have one height, so the list is a `uniform_list` and scales to many topics.

use gpui::{
    Context, ElementId, InteractiveElement, IntoElement, ParentElement, Render, SharedString,
    StatefulInteractiveElement, Styled, Subscription, Window, div, prelude::FluentBuilder as _, px,
    uniform_list,
};
use gpui_component::{
    Sizable as _,
    button::{Button, ButtonVariants as _},
};

use crate::{
    app::{Changes, Place, Services},
    copy::{fill, nav},
    data::{Change, GroupId, Member, Topic},
    theme,
    views::{count_badge, status_chip, time_text},
};

/// One row: a topic and the group it belongs to (the 待办 list mixes groups).
#[derive(Clone)]
struct Row {
    group_id: GroupId,
    group_name: Option<String>,
    topic: Topic,
}

pub struct TopicListView {
    services: Services,
    place: Option<Place>,
    title: String,
    rows: Vec<Row>,
    members: Vec<Member>,
    _subscriptions: Vec<Subscription>,
}

impl TopicListView {
    pub fn new(services: Services, cx: &mut Context<Self>) -> Self {
        let subscriptions = vec![
            cx.subscribe(&services.hub, |this, _, changes: &Changes, cx| {
                let relevant = changes.any(|c| match (c, &this.place) {
                    (Change::Topics { group_id }, Some(Place::Group(g))) => g == group_id,
                    (Change::Todo | Change::Topics { .. }, Some(Place::Todo)) => true,
                    (Change::Members | Change::Groups, _) => true,
                    _ => false,
                });
                if relevant {
                    this.refresh(cx);
                    cx.notify();
                }
            }),
            cx.observe(&services.nav, |this, _, cx| {
                this.refresh(cx);
                cx.notify();
            }),
        ];
        let mut this = Self {
            services,
            place: None,
            title: String::new(),
            rows: Vec::new(),
            members: Vec::new(),
            _subscriptions: subscriptions,
        };
        this.refresh(cx);
        this
    }

    fn refresh(&mut self, cx: &mut Context<Self>) {
        let p = self.services.provider.clone();
        self.place = self.services.nav.read(cx).place.clone();
        self.members = p.members();
        match &self.place {
            Some(Place::Todo) => {
                self.title = nav::TODO.to_string();
                self.rows = p
                    .todo()
                    .into_iter()
                    .map(|t| Row {
                        group_id: t.group_id,
                        group_name: Some(t.group_name),
                        topic: t.topic,
                    })
                    .collect();
            }
            Some(Place::Group(group_id)) => {
                self.title = p
                    .groups()
                    .into_iter()
                    .find(|g| &g.id == group_id)
                    .map(|g| g.name)
                    .unwrap_or_default();
                self.rows = p
                    .topics(group_id)
                    .into_iter()
                    .map(|topic| Row {
                        group_id: group_id.clone(),
                        group_name: None,
                        topic,
                    })
                    .collect();
            }
            None => {
                self.title.clear();
                self.rows.clear();
            }
        }
    }

    pub fn row_count(&self) -> usize {
        self.rows.len()
    }
}

impl Render for TopicListView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let colors = theme::colors(cx);
        let empty_text = match &self.place {
            None => Some(nav::SELECT_GROUP),
            Some(Place::Todo) if self.rows.is_empty() => Some(nav::TODO_EMPTY),
            Some(Place::Group(_)) if self.rows.is_empty() => Some(nav::TOPICS_EMPTY),
            _ => None,
        };
        let is_group = matches!(self.place, Some(Place::Group(_)));
        let header =
            div()
                .h(px(48.))
                .px(px(12.))
                .flex()
                .items_center()
                .gap(px(8.))
                .border_b_1()
                .border_color(colors.border)
                .child(
                    div()
                        .flex_1()
                        .flex()
                        .flex_col()
                        .child(
                            div()
                                .text_base()
                                .overflow_hidden()
                                .whitespace_nowrap()
                                .text_ellipsis()
                                .child(self.title.clone()),
                        )
                        .when(is_group, |el| {
                            el.child(div().text_xs().text_color(colors.muted_foreground).child(
                                fill(nav::TOPIC_COUNT, &[("n", &self.rows.len().to_string())]),
                            ))
                        }),
                )
                .when(is_group, |el| {
                    el.child(
                        Button::new("new-topic")
                            .small()
                            .primary()
                            .label(nav::NEW_TOPIC)
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.services.nav.update(cx, |nav, cx| nav.new_topic(cx))
                            })),
                    )
                });

        let selected = self.services.nav.read(cx).topic.clone();
        let me = self.services.me();
        let body = match empty_text {
            Some(text) => div()
                .flex_1()
                .p(px(16.))
                .text_sm()
                .text_color(colors.muted_foreground)
                .child(text)
                .into_any_element(),
            None => uniform_list(
                "topic-rows",
                self.rows.len(),
                cx.processor(move |this, range: std::ops::Range<usize>, _window, cx| {
                    let colors = theme::colors(cx);
                    range
                        .filter_map(|ix| this.rows.get(ix).cloned().map(|row| (ix, row)))
                        .map(|(ix, row)| {
                            let is_selected = selected.as_ref()
                                == Some(&(row.group_id.clone(), row.topic.id.clone()));
                            let chip = status_chip(&row.topic, me.as_deref(), &this.members, cx);
                            let (g, t) = (row.group_id.clone(), row.topic.id.clone());
                            let unread = count_badge(row.topic.unread, row.topic.mentions > 0, cx);
                            div()
                                .id(ElementId::NamedInteger(
                                    SharedString::from("topic-row"),
                                    ix as u64,
                                ))
                                .h(px(56.))
                                .px(px(12.))
                                .flex()
                                .flex_col()
                                .justify_center()
                                .gap(px(2.))
                                .border_b_1()
                                .border_color(colors.border)
                                .when(is_selected, |el| el.bg(colors.row_selected))
                                .hover(|el| el.bg(colors.row_hover))
                                .child(
                                    div()
                                        .flex()
                                        .items_center()
                                        .gap(px(6.))
                                        .child(
                                            div()
                                                .flex_1()
                                                .text_sm()
                                                .overflow_hidden()
                                                .whitespace_nowrap()
                                                .text_ellipsis()
                                                .when(row.topic.unread > 0, |el| {
                                                    el.font_weight(gpui::FontWeight::SEMIBOLD)
                                                })
                                                .child(row.topic.title.clone()),
                                        )
                                        .when_some(unread, |el, b| el.child(b)),
                                )
                                .child(
                                    div()
                                        .flex()
                                        .items_center()
                                        .gap(px(6.))
                                        .text_xs()
                                        .text_color(colors.muted_foreground)
                                        .when_some(chip, |el, chip| el.child(chip))
                                        .when_some(row.group_name.clone(), |el, name| {
                                            el.child(name)
                                        })
                                        .child(time_text(row.topic.last_activity_at)),
                                )
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    let (g, t) = (g.clone(), t.clone());
                                    this.services
                                        .nav
                                        .update(cx, |nav, cx| nav.open_topic(g, t, cx));
                                }))
                        })
                        .collect::<Vec<_>>()
                }),
            )
            .flex_1()
            .into_any_element(),
        };

        div()
            .size_full()
            .flex()
            .flex_col()
            .child(header)
            .child(body)
    }
}
