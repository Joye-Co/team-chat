//! The collapsible right panel of the open topic: status, owner, who it waits
//! on, its 产出, the process view, and (in the developer view) ids and seqs.

use gpui::{
    Context, IntoElement, ParentElement, Render, Styled, Subscription, Window, div,
    prelude::FluentBuilder as _, px,
};
use gpui_component::{
    Sizable as _,
    button::{Button, ButtonVariants as _},
};

use crate::{
    app::{Changes, Services},
    copy::{dev, fill, panel as p},
    data::{ArtifactKind, ArtifactSummary, Change, GroupId, Member, Topic, TopicId},
    theme,
    views::{avatar, status_chip, time_text},
};

pub struct SidePanel {
    services: Services,
    open: Option<(GroupId, TopicId)>,
    topic: Option<Topic>,
    artifacts: Vec<ArtifactSummary>,
    members: Vec<Member>,
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
                cx.notify();
            }),
        ];
        let open = services.nav.read(cx).topic.clone();
        let mut this = Self {
            services,
            open,
            topic: None,
            artifacts: Vec::new(),
            members: Vec::new(),
            _subscriptions: subscriptions,
        };
        this.refresh();
        this
    }

    fn refresh(&mut self) {
        let provider = &self.services.provider;
        self.members = provider.members();
        match &self.open {
            Some((g, t)) => {
                self.topic = provider.topic(g, t);
                self.artifacts = provider.artifacts(g, t);
            }
            None => {
                self.topic = None;
                self.artifacts.clear();
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
            .child(div().flex_1().child(p::TITLE))
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
        let artifacts: Vec<_> = self
            .artifacts
            .iter()
            .map(|a| {
                let kind = match a.kind {
                    ArtifactKind::Page => p::KIND_PAGE,
                    ArtifactKind::File => p::KIND_FILE,
                    ArtifactKind::Image => p::KIND_IMAGE,
                    ArtifactKind::Decision => p::KIND_DECISION,
                };
                div()
                    .py(px(4.))
                    .flex()
                    .flex_col()
                    .child(div().text_sm().child(a.title.clone()))
                    .child(
                        div()
                            .flex()
                            .gap(px(6.))
                            .text_xs()
                            .text_color(colors.muted_foreground)
                            .child(kind)
                            .child(fill(p::VERSION, &[("n", &a.version.to_string())]))
                            .child(time_text(a.updated_at)),
                    )
                    .into_any_element()
            })
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
                    .child(section(p::ARTIFACTS))
                    .when(artifacts.is_empty(), |el| {
                        el.child(
                            div()
                                .text_color(colors.muted_foreground)
                                .child(p::ARTIFACTS_EMPTY),
                        )
                    })
                    .children(artifacts)
                    .child(section(p::PROCESS))
                    .child(
                        div()
                            .text_color(colors.muted_foreground)
                            .child(p::PROCESS_EMPTY),
                    )
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
