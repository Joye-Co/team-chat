//! 通知: mute and notify level per 群, and the topics I follow.

use gpui::{
    Context, ElementId, IntoElement, ParentElement, Render, SharedString, Styled, Subscription,
    Window, div, px,
};
use gpui_component::{
    Sizable as _,
    button::{Button, ButtonVariants as _},
    switch::Switch,
};

use super::widgets::{self, Status, answer};
use crate::{
    app::{Changes, Services},
    copy::settings as s,
    data::{AdminReply, Change, NotifyLevel},
    theme,
};

const LEVELS: [NotifyLevel; 3] = [NotifyLevel::All, NotifyLevel::Mentions, NotifyLevel::None];

pub struct NotificationsPage {
    services: Services,
    status: Status,
    _subscriptions: Vec<Subscription>,
}

impl NotificationsPage {
    pub fn new(services: Services, _window: &mut Window, cx: &mut Context<Self>) -> Self {
        let subscriptions = vec![cx.subscribe(&services.hub, |_, _, changes: &Changes, cx| {
            if changes.any(|c| matches!(c, Change::Groups | Change::Topics { .. } | Change::Todo)) {
                cx.notify();
            }
        })];
        Self {
            services,
            status: Status::Idle,
            _subscriptions: subscriptions,
        }
    }

    pub fn status(&self) -> &Status {
        &self.status
    }

    fn run(&mut self, reply: AdminReply<()>, cx: &mut Context<Self>) {
        self.status = Status::Idle;
        cx.spawn(async move |this, cx| {
            let result = answer(reply).await;
            let _ = this.update(cx, |this, cx| {
                this.status = match result {
                    Ok(()) => Status::Idle,
                    Err(e) => Status::from_error(e),
                };
                cx.notify();
            });
        })
        .detach();
    }

    pub fn set_muted(&mut self, group_id: &str, muted: bool, cx: &mut Context<Self>) {
        let reply = self
            .services
            .provider
            .set_group_prefs(group_id, Some(muted), None);
        self.run(reply, cx);
    }

    pub fn set_level(&mut self, group_id: &str, level: NotifyLevel, cx: &mut Context<Self>) {
        let reply = self
            .services
            .provider
            .set_group_prefs(group_id, None, Some(level));
        self.run(reply, cx);
    }

    pub fn unfollow(&mut self, group_id: &str, topic_id: &str, cx: &mut Context<Self>) {
        self.services
            .provider
            .set_topic_following(group_id, topic_id, false);
        self.status = Status::Idle;
        cx.notify();
    }
}

fn level_ix(level: NotifyLevel) -> usize {
    LEVELS.iter().position(|l| *l == level).unwrap_or(1)
}

impl Render for NotificationsPage {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let colors = theme::colors(cx);
        let provider = self.services.provider.clone();
        let groups = provider.groups();
        let page = cx.entity().downgrade();
        let group_rows: Vec<_> = groups
            .iter()
            .map(|g| {
                let prefs = provider.group_prefs(&g.id);
                let gid = g.id.clone();
                let gid2 = g.id.clone();
                let page = page.clone();
                let page2 = page.clone();
                div()
                    .flex()
                    .items_center()
                    .gap(px(12.))
                    .py(px(6.))
                    .border_b_1()
                    .border_color(colors.border)
                    .child(div().flex_1().min_w_0().truncate().child(g.name.clone()))
                    .child(widgets::choice(
                        SharedString::from(format!("notify-{}", g.id)),
                        &[s::NOTIFY_ALL, s::NOTIFY_MENTIONS, s::NOTIFY_NONE],
                        level_ix(prefs.notify),
                        false,
                        move |ix, _, cx| {
                            let level = LEVELS[ix.min(2)];
                            let _ = page.update(cx, |this, cx| this.set_level(&gid, level, cx));
                        },
                    ))
                    .child(
                        Switch::new(ElementId::Name(format!("mute-{}", g.id).into()))
                            .checked(prefs.muted)
                            .label(s::MUTE)
                            .on_click(move |on: &bool, _, cx| {
                                let on = *on;
                                let _ = page2.update(cx, |this, cx| this.set_muted(&gid2, on, cx));
                            }),
                    )
            })
            .collect();
        let followed = provider.followed_topics();
        let topic_rows: Vec<_> = followed
            .iter()
            .map(|f| {
                let gid = f.group_id.clone();
                let tid = f.topic.id.clone();
                div()
                    .flex()
                    .items_center()
                    .gap(px(12.))
                    .py(px(6.))
                    .border_b_1()
                    .border_color(colors.border)
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .child(div().truncate().child(f.topic.title.clone()))
                            .child(
                                div()
                                    .text_xs()
                                    .truncate()
                                    .text_color(colors.muted_foreground)
                                    .child(f.group_name.clone()),
                            ),
                    )
                    .child(
                        Button::new(ElementId::Name(format!("unfollow-{}", f.topic.id).into()))
                            .ghost()
                            .small()
                            .label(s::UNFOLLOW)
                            .on_click(
                                cx.listener(move |this, _, _, cx| this.unfollow(&gid, &tid, cx)),
                            ),
                    )
            })
            .collect();
        div()
            .flex()
            .flex_col()
            .child(widgets::page_title(s::NOTIFICATIONS, cx))
            .child(widgets::hint(s::NOTIFICATIONS_HINT, cx))
            .children(widgets::status_line(&self.status, cx))
            .child(
                widgets::section(s::GROUP_NOTIFY, cx).child(if group_rows.is_empty() {
                    widgets::hint(s::NO_GROUPS, cx)
                } else {
                    div().flex().flex_col().children(group_rows)
                }),
            )
            .child(
                widgets::section(s::FOLLOWED_TOPICS, cx).child(if topic_rows.is_empty() {
                    widgets::hint(s::FOLLOWED_EMPTY, cx)
                } else {
                    div().flex().flex_col().children(topic_rows)
                }),
            )
    }
}
