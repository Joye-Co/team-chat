//! The right panel of the open topic (DESIGN.md §8.1, GrokBot's right pane):
//! a 92 pt header band (`话题详情` and close, then one context line), then
//! flat sections: the responsible bot's progress, 产出, 文件 and 成员. The
//! process view (过程, `views/process`) is a sub-page with a back button.
//! `Navigation::side_panel_tab` keeps which page shows (0 详情, 1 过程).

use gpui::{
    AppContext as _, Context, Entity, InteractiveElement as _, IntoElement, ParentElement, Render,
    StatefulInteractiveElement as _, Styled, Subscription, Window, div,
    prelude::FluentBuilder as _, px,
};

use crate::{
    app::{Changes, Services},
    copy::{dev, fill, panel as p, process as pc},
    data::{Change, GroupId, Member, Topic, TopicId},
    theme::{self, Type as _, size, space},
    ui::{self, Lucide, MemberAvatar},
    views::{
        artifacts::panel::TopicOutputs,
        process::{ProcessView, duration_text, run_duration, status_text},
        time_text,
    },
};

pub struct SidePanel {
    services: Services,
    open: Option<(GroupId, TopicId)>,
    topic: Option<Topic>,
    members: Vec<Member>,
    /// Member ids of the topic's 群, for the 成员 section.
    group_members: Vec<String>,
    /// The 产出 and files of the topic (views/artifacts/panel.rs).
    outputs: Entity<TopicOutputs>,
    process: Entity<ProcessView>,
    _subscriptions: Vec<Subscription>,
}

impl SidePanel {
    pub fn new(services: Services, cx: &mut Context<Self>) -> Self {
        let subscriptions = vec![
            cx.subscribe(&services.hub, |this, _, changes: &Changes, cx| {
                let Some((g, t)) = this.open.clone() else {
                    return;
                };
                if changes.touches_topic(&g, &t)
                    || changes.any(|c| {
                        matches!(c, Change::Topics { group_id } if *group_id == g)
                            || matches!(c, Change::Members)
                    })
                {
                    this.refresh();
                    cx.notify();
                }
            }),
            cx.observe(&services.nav, |this, nav, cx| {
                this.open = nav.read(cx).topic.clone();
                this.refresh();
                this.sync_process_visibility(cx);
                cx.notify();
            }),
        ];
        let open = services.nav.read(cx).topic.clone();
        let outputs = cx.new(|cx| TopicOutputs::new(services.clone(), cx));
        let process = cx.new(|cx| ProcessView::new(services.clone(), cx));
        cx.observe(&process, |_, _, cx| cx.notify()).detach();
        let mut this = Self {
            outputs,
            services,
            open,
            topic: None,
            members: Vec::new(),
            group_members: Vec::new(),
            process,
            _subscriptions: subscriptions,
        };
        this.refresh();
        this.sync_process_visibility(cx);
        this
    }

    /// Switches between 详情 (0) and the 过程 sub-page (1).
    pub fn set_tab(&mut self, tab: usize, cx: &mut Context<Self>) {
        self.services
            .nav
            .update(cx, |nav, cx| nav.set_side_panel_tab(tab.min(1), cx));
    }

    pub fn process(&self) -> &Entity<ProcessView> {
        &self.process
    }

    fn sync_process_visibility(&mut self, cx: &mut Context<Self>) {
        let nav = self.services.nav.read(cx);
        let visible = nav.side_panel_tab == 1 && nav.side_panel_open;
        self.process
            .update(cx, |view, cx| view.set_visible(visible, cx));
    }

    fn refresh(&mut self) {
        let provider = &self.services.provider;
        self.members = provider.members();
        match &self.open {
            Some((g, t)) => {
                self.topic = provider.topic(g, t);
                self.group_members = provider
                    .groups()
                    .into_iter()
                    .find(|x| &x.id == g)
                    .map(|x| x.member_ids)
                    .unwrap_or_default();
            }
            None => {
                self.topic = None;
                self.group_members.clear();
            }
        }
    }

    fn member(&self, id: &str) -> Option<&Member> {
        self.members.iter().find(|m| m.id == id)
    }

    fn name(&self, id: &str) -> String {
        self.member(id).map(|m| m.name.clone()).unwrap_or_default()
    }

    /// The one context line of the header (§8.1): what the timeline does not
    /// show. `Taro 负责 · 等 Joye`, or `Taro 负责 · 更新于 14:31`.
    fn context_line(&self, topic: &Topic) -> String {
        let mut parts = Vec::new();
        match topic.owner_id.as_deref() {
            Some(owner) => parts.push(fill(p::CONTEXT_OWNER, &[("name", &self.name(owner))])),
            None => parts.push(fill(
                p::CONTEXT_STARTED,
                &[("name", &self.name(&topic.created_by))],
            )),
        }
        if !topic.waiting_on.is_empty() {
            let names: Vec<String> = topic.waiting_on.iter().map(|id| self.name(id)).collect();
            parts.push(fill(
                p::CONTEXT_WAITING,
                &[("names", &names.join(p::NAME_JOIN))],
            ));
        } else {
            parts.push(fill(
                p::CONTEXT_UPDATED,
                &[("time", &time_text(topic.last_activity_at))],
            ));
        }
        parts.join(" · ")
    }

    fn header(&self, topic: Option<&Topic>, sub: bool, cx: &mut Context<Self>) -> gpui::Div {
        let k = theme::colors(cx);
        let title_row = ui::toolbar_band("panel-toolbar")
            .pl(if sub { space::S8 } else { space::S16 })
            .pr(space::S10)
            .gap(space::S4)
            .when(sub, |el| {
                el.child(
                    ui::icon_button("panel-back", Lucide::ChevronLeft)
                        .tooltip(p::BACK)
                        .on_click(cx.listener(|this, _, _, cx| {
                            cx.stop_propagation();
                            this.set_tab(0, cx)
                        })),
                )
            })
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.))
                    .t_title()
                    .text_color(k.text_1)
                    .one_line()
                    .child(if sub { pc::TAB_PROCESS } else { p::HEADER }),
            )
            .child(
                ui::icon_button("panel-close", Lucide::X)
                    .tooltip(p::CLOSE)
                    .on_click(cx.listener(|this, _, _, cx| {
                        cx.stop_propagation();
                        this.services
                            .nav
                            .update(cx, |nav, cx| nav.toggle_side_panel(cx));
                    })),
            );
        let context = topic.map(|t| self.context_line(t)).unwrap_or_default();
        ui::header_band(cx).child(title_row).child(
            div()
                .flex_none()
                .h(theme::HEADER_SECOND_ROW - theme::HAIRLINE)
                .px(space::S16)
                .flex()
                .items_center()
                .t_small()
                .text_color(k.text_2)
                .child(div().min_w(px(0.)).one_line().child(context)),
        )
    }

    /// The responsible bot's live state (§8.1 Progress): flat, a 36 pt tile,
    /// the name 14/600, the state 13/20 `text-2`, and `查看过程`.
    fn progress(&self, owner: &Member, cx: &mut Context<Self>) -> gpui::AnyElement {
        let k = theme::colors(cx);
        let process = self.process.read(cx);
        let now = chrono::Utc::now().timestamp_millis();
        let run = process
            .runs()
            .iter()
            .filter(|r| r.bot_id == owner.id)
            .max_by_key(|r| r.started_at.unwrap_or(r.created_at))
            .cloned();
        let state = match &run {
            Some(r) => {
                let mut s = status_text(r.status).to_string();
                if let Some(ms) = run_duration(r, now) {
                    s.push_str(" · ");
                    s.push_str(&duration_text(ms));
                }
                s
            }
            None => p::PROGRESS_IDLE.to_string(),
        };
        let summary = run.as_ref().and_then(|r| r.summary.clone());
        div()
            .flex()
            .flex_col()
            .child(ui::panel_label(
                fill(p::PROGRESS, &[("name", &owner.name)]),
                None,
                cx,
            ))
            .child(
                div()
                    .mt(space::S8)
                    .flex()
                    .items_center()
                    .gap(space::S12)
                    .child(MemberAvatar::new(Some(owner), size::AVATAR_MESSAGE).on(k.content))
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(0.))
                            .flex()
                            .flex_col()
                            .child(
                                div()
                                    .t_ui()
                                    .semibold()
                                    .text_color(k.text_1)
                                    .one_line()
                                    .child(owner.name.clone()),
                            )
                            .child(
                                div()
                                    .t_small()
                                    .tnum()
                                    .text_color(k.text_2)
                                    .one_line()
                                    .child(state),
                            ),
                    ),
            )
            .when_some(summary, |el, s| {
                el.child(
                    div()
                        .mt(space::S8)
                        .t_small()
                        .text_color(k.text_2)
                        .line_clamp(3)
                        .child(s),
                )
            })
            .when(run.is_some(), |el| {
                el.child(
                    div().mt(space::S6).ml(-space::S6).flex().child(
                        ui::text_button("panel-view-process", p::VIEW_PROCESS, cx)
                            .on_click(cx.listener(|this, _, _, cx| this.set_tab(1, cx))),
                    ),
                )
            })
            .into_any_element()
    }

    /// 成员 · n (§8.1): 40 pt rows, a 24 pt avatar, the name `ui` 14 and the
    /// 岗位 trailing in 13/20 `text-3` (a 12 pt `bot` glyph before it for
    /// bots); the owner reads `负责人`.
    fn members_section(&self, topic: &Topic, cx: &mut Context<Self>) -> gpui::AnyElement {
        let k = theme::colors(cx);
        let mut ids: Vec<&str> = self.group_members.iter().map(String::as_str).collect();
        // The owner first, then people, then bots, each by name.
        ids.sort_by_key(|id| {
            let m = self.member(id);
            (
                topic.owner_id.as_deref() != Some(*id),
                m.is_some_and(|m| m.is_bot()),
                m.map(|m| m.name.clone()).unwrap_or_default(),
            )
        });
        let rows = ids.iter().filter_map(|id| self.member(id)).map(|m| {
            let owner = topic.owner_id.as_deref() == Some(m.id.as_str());
            let role = if owner {
                Some(p::ROLE_OWNER.to_string())
            } else {
                m.role.clone()
            };
            div()
                .h(size::PANEL_ROW)
                .flex()
                .items_center()
                .gap(space::S10)
                .child(
                    MemberAvatar::new(Some(m), size::AVATAR_ROW)
                        .presence(Some(m))
                        .on(k.content),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w(px(0.))
                        .t_ui()
                        .text_color(k.text_1)
                        .one_line()
                        .child(m.name.clone()),
                )
                .when_some(role, |el, role| {
                    el.child(
                        div()
                            .flex_none()
                            .max_w(px(120.))
                            .flex()
                            .items_center()
                            .gap(space::S4)
                            .t_small()
                            .text_color(k.text_3)
                            .when(m.is_bot() && !owner, |el| {
                                el.child(ui::icon(Lucide::Bot, theme::icon::TAG, k.text_3))
                            })
                            .child(div().min_w(px(0.)).one_line().child(role)),
                    )
                })
        });
        div()
            .flex()
            .flex_col()
            .child(div().mb(space::S4).child(ui::panel_label(
                p::MEMBERS,
                Some(ids.len() as u32),
                cx,
            )))
            .children(rows)
            .into_any_element()
    }
}

impl Render for SidePanel {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let k = theme::colors(cx);
        let developer = self.services.nav.read(cx).developer_view;
        let tab = self.services.nav.read(cx).side_panel_tab;
        let topic = self.topic.clone();
        let page = div().size_full().flex().flex_col().bg(k.content).t_ui();
        let Some(topic) = topic else {
            return page.child(self.header(None, false, cx));
        };
        if tab == 1 {
            return page
                .child(self.header(Some(&topic), true, cx))
                .child(div().flex_1().min_h(px(0.)).child(self.process.clone()));
        }
        let owner_bot = topic
            .owner_id
            .as_deref()
            .and_then(|id| self.member(id))
            .filter(|m| m.is_bot())
            .cloned();
        let progress = owner_bot.map(|m| self.progress(&m, cx));
        let members = self.members_section(&topic, cx);
        page.child(self.header(Some(&topic), false, cx)).child(
            div()
                .id("panel-body")
                .flex_1()
                .min_h(px(0.))
                .overflow_y_scroll()
                .px(space::S16)
                .pt(space::S16)
                .pb(space::S24)
                .flex()
                .flex_col()
                .gap(space::S16)
                .children(progress)
                .child(self.outputs.clone())
                .child(members)
                .when(developer, |el| {
                    el.child(
                        div()
                            .flex()
                            .flex_col()
                            .gap(space::S8)
                            .child(ui::panel_label(dev::TITLE, None, cx))
                            .child(
                                div()
                                    .flex()
                                    .flex_col()
                                    .t_code()
                                    .text_color(k.text_2)
                                    .child(fill(dev::GROUP_ID, &[("id", &topic.group_id)]))
                                    .child(fill(dev::TOPIC_ID, &[("id", &topic.id)]))
                                    .child(fill(
                                        dev::LAST_SEQ,
                                        &[("seq", &topic.last_seq.to_string())],
                                    )),
                            ),
                    )
                }),
        )
    }
}
