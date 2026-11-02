//! The collapsible right panel of the open topic, with two tabs: 详情
//! (status, owner, who it waits on, its 产出, and in the developer view ids
//! and seqs) and 过程 (the process view of bot runs, `views/process`).

use gpui::{
    AppContext as _, Context, Entity, IntoElement, ParentElement, Render, Styled, Subscription,
    Window, div, prelude::FluentBuilder as _, px,
};
use gpui_component::{
    Sizable as _,
    button::{Button, ButtonVariants as _},
    tab::{Tab, TabBar},
};

use crate::{
    app::{Changes, Services},
    copy::{dev, fill, panel as p, process as pc},
    data::{Change, GroupId, Member, Topic, TopicId},
    theme,
    views::{artifacts::panel::TopicOutputs, avatar, process::ProcessView, status_chip},
};

pub struct SidePanel {
    services: Services,
    open: Option<(GroupId, TopicId)>,
    topic: Option<Topic>,
    members: Vec<Member>,
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
        let mut this = Self {
            outputs,
            services,
            open,
            topic: None,
            members: Vec::new(),
            process,
            _subscriptions: subscriptions,
        };
        this.refresh();
        this.sync_process_visibility(cx);
        this
    }

    /// Switches between 详情 (0) and 过程 (1).
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
            }
            None => {
                self.topic = None;
            }
        }
    }

    fn member(&self, id: &str) -> Option<&Member> {
        self.members.iter().find(|m| m.id == id)
    }
}

impl Render for SidePanel {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let colors = theme::colors(cx);
        let developer = self.services.nav.read(cx).developer_view;
        let tab = self.services.nav.read(cx).side_panel_tab;
        let section = |title: &'static str| {
            div()
                .pt(px(12.))
                .pb(px(4.))
                .text_xs()
                .text_color(colors.muted_foreground)
                .child(title)
        };
        let me = self.services.me();
        let header = div()
            .h(px(48.))
            .px(px(12.))
            .flex()
            .items_center()
            .border_b_1()
            .border_color(colors.border)
            .child(
                div().flex_1().child(
                    TabBar::new("side-panel-tabs")
                        .segmented()
                        .small()
                        .selected_index(tab)
                        .on_click(cx.listener(|this, ix: &usize, _, cx| this.set_tab(*ix, cx)))
                        .child(Tab::new().label(pc::TAB_DETAILS))
                        .child(Tab::new().label(pc::TAB_PROCESS)),
                ),
            )
            .child(
                Button::new("collapse-panel")
                    .small()
                    .ghost()
                    .label(p::COLLAPSE)
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.services
                            .nav
                            .update(cx, |nav, cx| nav.toggle_side_panel(cx))
                    })),
            );
        let Some(topic) = self.topic.clone() else {
            return div().size_full().bg(colors.panel).child(header);
        };
        if tab == 1 {
            return div()
                .size_full()
                .flex()
                .flex_col()
                .bg(colors.panel)
                .text_sm()
                .child(header)
                .child(div().flex_1().min_h(px(0.)).child(self.process.clone()));
        }
        let person = |m: Option<&Member>, cx: &mut Context<Self>| {
            div()
                .flex()
                .items_center()
                .gap(px(6.))
                .py(px(2.))
                .child(avatar(m, px(20.), cx))
                .child(m.map(|m| m.name.clone()).unwrap_or_default())
                .when_some(m.and_then(|m| m.role.clone()), |el, role| {
                    el.child(
                        div()
                            .text_xs()
                            .text_color(colors.muted_foreground)
                            .child(role),
                    )
                })
        };
        let owner = topic
            .owner_id
            .as_deref()
            .map(|id| person(self.member(id), cx).into_any_element());
        let waiting: Vec<_> = topic
            .waiting_on
            .iter()
            .map(|id| person(self.member(id), cx).into_any_element())
            .collect();

        div()
            .size_full()
            .flex()
            .flex_col()
            .bg(colors.panel)
            .text_sm()
            .child(header)
            .child(
                div()
                    .px(px(12.))
                    .flex()
                    .flex_col()
                    .child(section(p::STATUS))
                    .when_some(
                        status_chip(&topic, me.as_deref(), &self.members, cx),
                        |el, chip| el.child(chip),
                    )
                    .when_some(owner, |el, o| el.child(section(p::OWNER)).child(o))
                    .when(!waiting.is_empty(), |el| {
                        el.child(section(p::WAITING_ON)).children(waiting)
                    })
                    .child(self.outputs.clone())
                    .when(developer, |el| {
                        el.child(
                            div()
                                .pt(px(12.))
                                .text_xs()
                                .text_color(colors.muted_foreground)
                                .child(dev::TITLE),
                        )
                        .child(
                            div()
                                .text_xs()
                                .child(fill(dev::GROUP_ID, &[("id", &topic.group_id)])),
                        )
                        .child(
                            div()
                                .text_xs()
                                .child(fill(dev::TOPIC_ID, &[("id", &topic.id)])),
                        )
                        .child(
                            div().text_xs().child(fill(
                                dev::LAST_SEQ,
                                &[("seq", &topic.last_seq.to_string())],
                            )),
                        )
                    }),
            )
    }
}
