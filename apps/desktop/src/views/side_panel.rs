//! The collapsible right panel of the open topic, with two tabs: 详情
//! (status, owner, who it waits on, its 产出, and in the developer view ids
//! and seqs) and 过程 (the process view of bot runs, `views/process`).

use gpui::{
    AppContext as _, Context, Entity, InteractiveElement as _, IntoElement, ParentElement, Render,
    StatefulInteractiveElement as _, Styled, Subscription, Window, div,
    prelude::FluentBuilder as _, px,
};
use gpui_component::{
    Sizable as _,
    tab::{Tab, TabBar},
};

use crate::{
    app::{Changes, Services},
    copy::{dev, fill, panel as p, process as pc},
    data::{Change, GroupId, Member, Topic, TopicId},
    theme::{self, Type as _, space},
    ui::{self, MemberAvatar},
    views::{artifacts::panel::TopicOutputs, process::ProcessView, status_chip},
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
                .pt(space::S16)
                .pb(space::S6)
                .child(ui::SectionHeader::new(title))
        };
        let header = ui::toolbar_band("panel-toolbar").px(space::S12).child(
            TabBar::new("side-panel-tabs")
                .segmented()
                .small()
                .w_full()
                .selected_index(tab)
                .on_click(cx.listener(|this, ix: &usize, _, cx| this.set_tab(*ix, cx)))
                .child(Tab::new().flex_1().label(pc::TAB_DETAILS))
                .child(Tab::new().flex_1().label(pc::TAB_PROCESS)),
        );
        let Some(topic) = self.topic.clone() else {
            return div()
                .size_full()
                .flex()
                .flex_col()
                .bg(colors.panel)
                .child(header);
        };
        if tab == 1 {
            return div()
                .size_full()
                .flex()
                .flex_col()
                .bg(colors.panel)
                .t_ui()
                .child(header)
                .child(div().flex_1().min_h(px(0.)).child(self.process.clone()));
        }
        let person = |m: Option<&Member>| {
            div()
                .min_w(px(0.))
                .flex()
                .items_center()
                .gap(space::S6)
                .child(MemberAvatar::new(m, px(20.)).on(colors.panel))
                .child(
                    div()
                        .min_w(px(0.))
                        .one_line()
                        .child(m.map(|m| m.name.clone()).unwrap_or_default()),
                )
        };
        // 谁在做什么: label column 56 pt, then the members (§13.7).
        let row = |label: &'static str, value: gpui::AnyElement| {
            div()
                .min_h(px(28.))
                .flex()
                .items_center()
                .gap(space::S8)
                .child(
                    div()
                        .w(px(56.))
                        .flex_none()
                        .text_color(colors.text_2)
                        .child(label),
                )
                .child(div().flex_1().min_w(px(0.)).child(value))
        };
        let me = self.services.me();
        let chip = status_chip(&topic, me.as_deref(), &self.members, cx);
        let owner = topic
            .owner_id
            .as_deref()
            .map(|id| row(p::OWNER, person(self.member(id)).into_any_element()));
        let waiting = (!topic.waiting_on.is_empty()).then(|| {
            row(
                p::WAITING_ON,
                div()
                    .py(space::S4)
                    .flex()
                    .flex_col()
                    .gap(space::S4)
                    .children(topic.waiting_on.iter().map(|id| person(self.member(id))))
                    .into_any_element(),
            )
        });

        div()
            .size_full()
            .flex()
            .flex_col()
            .bg(colors.panel)
            .t_ui()
            .child(header)
            .child(
                div()
                    .id("panel-body")
                    .flex_1()
                    .min_h(px(0.))
                    .overflow_y_scroll()
                    .px(space::S16)
                    .pb(space::S16)
                    .flex()
                    .flex_col()
                    .child(div().h(space::S12))
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .children(chip.map(|c| {
                                row(p::STATUS, div().flex().child(c).into_any_element())
                            }))
                            .children(owner)
                            .children(waiting),
                    )
                    .child(self.outputs.clone())
                    .when(developer, |el| {
                        el.child(section(dev::TITLE)).child(
                            div()
                                .flex()
                                .flex_col()
                                .t_code()
                                .text_color(colors.text_2)
                                .child(fill(dev::GROUP_ID, &[("id", &topic.group_id)]))
                                .child(fill(dev::TOPIC_ID, &[("id", &topic.id)]))
                                .child(fill(
                                    dev::LAST_SEQ,
                                    &[("seq", &topic.last_seq.to_string())],
                                )),
                        )
                    }),
            )
    }
}
