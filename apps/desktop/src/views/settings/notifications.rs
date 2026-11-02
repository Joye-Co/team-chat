//! 通知: mute and notify level per 群, and the topics I follow.

use gpui::{
    App, Context, ElementId, IntoElement, ParentElement, Render, SharedString, Styled,
    Subscription, Window, div,
};
use gpui_component::switch::Switch;

use super::widgets::{self, Status, answer};
use crate::{
    app::{Changes, Services},
    copy::{fill, settings as s},
    data::{AdminReply, Change, GroupKind, NotifyLevel},
    theme::{self, Type as _, space},
    ui,
};

/// The 提醒 column: the segmented control.
const SEGMENTS: gpui::Pixels = gpui::px(216.);
/// The 静音 column: the switch.
const MUTE_COL: gpui::Pixels = gpui::px(44.);

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

impl NotificationsPage {
    /// One 群 or 私聊 row: its glyph or avatar, the name, the segmented
    /// level control and the mute switch.
    fn group_row(
        &self,
        g: &crate::data::GroupSummary,
        members: &[crate::data::Member],
        cx: &mut Context<Self>,
    ) -> gpui::Div {
        let colors = theme::colors(cx);
        let prefs = self.services.provider.group_prefs(&g.id);
        let gid = g.id.clone();
        let gid2 = g.id.clone();
        let page = cx.entity().downgrade();
        let page2 = page.clone();
        let leading = if g.kind == GroupKind::Dm {
            let peer = g
                .dm_peer
                .as_ref()
                .and_then(|p| members.iter().find(|m| &m.id == p));
            ui::MemberAvatar::new(peer, theme::size::AVATAR_ROW)
                .on(colors.content)
                .into_any_element()
        } else {
            ui::GroupGlyph::new().into_any_element()
        };
        widgets::table_row()
            .h(theme::size::SETTINGS_ROW)
            .py_0()
            .child(div().flex_none().child(leading))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .t_ui()
                    .medium()
                    .text_color(if prefs.muted {
                        colors.text_2
                    } else {
                        colors.text_1
                    })
                    .one_line()
                    .child(g.name.clone()),
            )
            .child(div().w(SEGMENTS).flex_none().child(widgets::choice(
                SharedString::from(format!("notify-{}", g.id)),
                &[s::NOTIFY_ALL, s::NOTIFY_MENTIONS, s::NOTIFY_NONE],
                level_ix(prefs.notify),
                false,
                move |ix, _, cx| {
                    let level = LEVELS[ix.min(2)];
                    let _ = page.update(cx, |this, cx| this.set_level(&gid, level, cx));
                },
                cx,
            )))
            .child(
                div().w(MUTE_COL).flex_none().child(
                    Switch::new(ElementId::Name(format!("mute-{}", g.id).into()))
                        .checked(prefs.muted)
                        .tooltip(s::MUTE_GROUP)
                        .on_click(move |on: &bool, _, cx| {
                            let on = *on;
                            let _ = page2.update(cx, |this, cx| this.set_muted(&gid2, on, cx));
                        }),
                ),
            )
    }
}

impl Render for NotificationsPage {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let provider = self.services.provider.clone();
        let groups = provider.groups();
        let members = provider.members();
        let (dms, groups): (Vec<_>, Vec<_>) =
            groups.into_iter().partition(|g| g.kind == GroupKind::Dm);
        let group_rows: Vec<_> = groups
            .iter()
            .map(|g| self.group_row(g, &members, cx))
            .collect();
        let dm_rows: Vec<_> = dms
            .iter()
            .map(|g| self.group_row(g, &members, cx))
            .collect();
        let followed = provider.followed_topics();
        let topic_rows: Vec<_> = followed
            .iter()
            .map(|f| {
                let gid = f.group_id.clone();
                let tid = f.topic.id.clone();
                widgets::table_row()
                    .child(widgets::title_cell(
                        f.topic.title.replace('\n', " "),
                        Some(f.group_name.clone().into()),
                        cx,
                    ))
                    .child(
                        ui::secondary_button(
                            ElementId::Name(format!("unfollow-{}", f.topic.id).into()),
                            s::UNFOLLOW,
                            cx,
                        )
                        .on_click(cx.listener(move |this, _, _, cx| this.unfollow(&gid, &tid, cx))),
                    )
            })
            .collect();
        // 群 and 私聊 tables name their columns once, so the rows carry
        // controls without repeated labels.
        let header = |first: String, cx: &App| {
            widgets::table_header(cx)
                .gap(space::S12)
                .child(div().flex_1().min_w_0().tnum().child(first))
                .child(div().w(SEGMENTS).flex_none().child(s::NOTIFY_LEVEL))
                .child(div().w(MUTE_COL).flex_none().child(s::MUTE))
        };
        let table =
            |header: Option<gpui::Div>, rows: Vec<gpui::Div>, empty: &'static str, cx: &App| {
                if rows.is_empty() {
                    widgets::table_empty(None, empty, cx)
                } else {
                    widgets::table_of(header, rows, cx)
                }
            };
        let group_header = header(
            fill(s::GROUPS_HEADER, &[("n", &group_rows.len().to_string())]),
            cx,
        );
        let dm_header = header(
            fill(s::DMS_HEADER, &[("n", &dm_rows.len().to_string())]),
            cx,
        );
        div()
            .flex()
            .flex_col()
            .children(widgets::status_line(&self.status, cx).map(|l| l.pb(space::S12)))
            .child(table(Some(group_header), group_rows, s::NO_GROUPS, cx))
            .child(widgets::footnote(s::NOTIFICATIONS_HINT, cx))
            .child(div().h(space::S24))
            .child(table(Some(dm_header), dm_rows, s::NO_DMS, cx))
            .child(widgets::section_label(s::FOLLOWED_TOPICS, false, cx))
            .child(table(None, topic_rows, s::FOLLOWED_EMPTY, cx))
    }
}
