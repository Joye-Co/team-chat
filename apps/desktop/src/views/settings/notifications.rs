//! 通知: mute and notify level per 群, and the topics I follow.

use gpui::{
    Context, ElementId, IntoElement, ParentElement, Render, SharedString, Styled, Subscription,
    Window, div, prelude::FluentBuilder as _, px,
};
use gpui_component::{
    Sizable as _,
    button::Button,
    switch::Switch,
};

use super::widgets::{self, Status, answer};
use crate::{
    app::{Changes, Services},
    copy::settings as s,
    data::{AdminReply, Change, NotifyLevel},
    theme::{self, Type as _, space},
    ui,
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
                widgets::group_row()
                    .child(ui::GroupTile::new(g.id.clone(), g.name.clone(), px(20.)))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .t_ui()
                            .when(prefs.muted, |el| el.text_color(colors.text_2))
                            .one_line()
                            .child(g.name.clone()),
                    )
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
                            .small()
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
                widgets::group_row()
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .child(
                                div()
                                    .t_ui()
                                    .medium()
                                    .one_line()
                                    .child(f.topic.title.replace('\n', " ")),
                            )
                            .child(
                                div()
                                    .t_callout()
                                    .one_line()
                                    .text_color(colors.text_2)
                                    .child(f.group_name.clone()),
                            ),
                    )
                    .child(
                        Button::new(ElementId::Name(format!("unfollow-{}", f.topic.id).into()))
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
            .children(widgets::status_line(&self.status, cx).map(|l| l.px(space::S12).pb(space::S8)))
            .child(widgets::section_header(s::GROUP_NOTIFY, cx).pt(space::S4))
            .child(if group_rows.is_empty() {
                widgets::group_empty(s::NO_GROUPS, cx)
            } else {
                // Inset to the name: 12 padding + 20 tile + 10 gap.
                widgets::group_rows(group_rows, px(42.), cx)
            })
            .child(widgets::footnote(s::NOTIFICATIONS_HINT, cx))
            .child(widgets::section_header(s::FOLLOWED_TOPICS, cx))
            .child(if topic_rows.is_empty() {
                widgets::group_empty(s::FOLLOWED_EMPTY, cx)
            } else {
                widgets::group_rows(topic_rows, space::S12, cx)
            })
    }
}
