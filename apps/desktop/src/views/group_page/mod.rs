//! 群 page (Group.dc.html): title with purpose and member stack, the
//! composer that starts a 话题, 进行中 cards, 最近完成, and at the right
//! 群里的决定 and 最近的产出.
//!
//! Foundation stub: the sections read the provider; the composer is the
//! shared `ComposerView` (it starts a new 话题 in this 群). The group page
//! track builds the artboard cards and the member stack.

pub mod scenes;

use gpui::{
    App, Context, Entity, InteractiveElement as _, IntoElement, ParentElement, Render,
    SharedString, StatefulInteractiveElement as _, Styled, Subscription, Window, div, px,
};

use crate::{
    app::{Place, Services},
    copy::{fill, group_page as c},
    data::TopicStatus,
    theme::{self, Type as _, space},
    ui,
    views::{
        composer::ComposerView,
        shell::{
            PageView, TitleBarSpec, clock, member_name, panes::watch, section_label, stub_row,
        },
    },
};

pub struct GroupPage {
    services: Services,
    composer: Entity<ComposerView>,
    _subscriptions: Vec<Subscription>,
}

impl GroupPage {
    pub fn new(services: Services, composer: Entity<ComposerView>, cx: &mut Context<Self>) -> Self {
        Self {
            _subscriptions: watch(&services, cx),
            services,
            composer,
        }
    }

    fn group_id(&self, cx: &App) -> Option<String> {
        match &self.services.nav.read(cx).place {
            Some(Place::Group(g)) => Some(g.clone()),
            _ => None,
        }
    }
}

impl PageView for GroupPage {
    fn title_bar(&self, cx: &App) -> TitleBarSpec {
        let Some(g) = self.group_id(cx) else {
            return TitleBarSpec::default();
        };
        let p = &self.services.provider;
        let group = p.groups().into_iter().find(|x| x.id == g);
        TitleBarSpec {
            subtitle: p.group_purpose(&g).or_else(|| {
                group.as_ref().map(|x| {
                    fill(
                        c::GROUP_PAGE_MEMBERS,
                        &[("n", &x.member_ids.len().to_string())],
                    )
                })
            }),
            ..TitleBarSpec::page(group.map(|x| x.name).unwrap_or_default())
        }
    }
}

impl Render for GroupPage {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let k = theme::colors(cx);
        let Some(g) = self.group_id(cx) else {
            return div().into_any_element();
        };
        let p = self.services.provider.clone();
        let topics = p.topics(&g);
        let active: Vec<_> = topics
            .iter()
            .filter(|t| t.status != TopicStatus::Done)
            .collect();
        let done: Vec<_> = topics
            .iter()
            .filter(|t| t.status == TopicStatus::Done)
            .take(3)
            .collect();
        let cards = active.iter().enumerate().map(|(i, t)| {
            let (state, meta) = crate::views::rail::row_state(&self.services, t);
            let _ = state;
            let nav = self.services.nav.clone();
            let (gid, tid) = (t.group_id.clone(), t.id.clone());
            let card = ui::card(cx)
                .id(SharedString::from(format!("group-topic-{}", t.id)))
                .cursor_pointer()
                .p(space::S14)
                .flex()
                .flex_col()
                .gap(space::S6)
                .child(
                    div()
                        .t_ui()
                        .medium()
                        .text_color(k.ink)
                        .child(t.title.clone()),
                )
                .child(
                    div()
                        .flex()
                        .justify_between()
                        .t_meta()
                        .text_color(k.ink_3)
                        .child(
                            t.owner_id
                                .as_deref()
                                .map(|o| member_name(&self.services, o))
                                .unwrap_or_default(),
                        )
                        .children(meta.map(|m| div().t_mono().child(m))),
                )
                .on_click(move |_, _, cx| {
                    let (gid, tid) = (gid.clone(), tid.clone());
                    nav.update(cx, |nav, cx| nav.open_topic(gid, tid, cx))
                });
            ui::motion::rise(
                SharedString::from(format!("group-card-{i}")),
                i,
                card.w(px(300.)),
                cx,
            )
        });
        let done_rows = done.iter().enumerate().map(|(i, t)| {
            stub_row(
                SharedString::from(format!("group-done-{i}")),
                i,
                ui::icon(ui::Lucide::Check, theme::icon::TAG, k.ok).into_any_element(),
                t.title.clone(),
                Some(clock(t.last_activity_at)),
                cx,
            )
        });
        let decisions = p.group_decisions(&g);
        let outputs: Vec<_> = p
            .all_artifacts()
            .into_iter()
            .filter(|(gid, _)| *gid == g)
            .take(3)
            .collect();
        div()
            .size_full()
            .flex()
            .child(
                div()
                    .id("group-main")
                    .flex_1()
                    .min_w(px(0.))
                    .h_full()
                    .overflow_y_scroll()
                    .px(space::S32)
                    .pb(space::S32)
                    .flex()
                    .flex_col()
                    .gap(space::S12)
                    .child(self.composer.clone())
                    .child(section_label(c::GROUP_PAGE_ACTIVE, Some(active.len()), cx))
                    .child(div().flex().flex_wrap().gap(space::S12).children(cards))
                    .child(section_label(c::GROUP_PAGE_RECENT_DONE, None, cx))
                    .children(done_rows),
            )
            .child(
                div()
                    .flex_none()
                    .w(theme::DESK_SIDE_WIDTH + space::S32)
                    .h_full()
                    .bg(k.rail)
                    .p(space::S16)
                    .flex()
                    .flex_col()
                    .gap(space::S10)
                    .child(
                        ui::rail_card(cx)
                            .child(section_label(c::GROUP_PAGE_DECISIONS, None, cx))
                            .child(
                                div()
                                    .t_meta()
                                    .text_color(k.ink_3)
                                    .child(c::GROUP_PAGE_DECISIONS_NOTE),
                            )
                            .children(decisions.iter().map(|d| {
                                div()
                                    .flex()
                                    .gap(space::S8)
                                    .child(div().t_mono().text_color(k.ink_3).child(clock(d.at)))
                                    .child(div().t_small().text_color(k.ink).child(format!(
                                        "{}：{}",
                                        member_name(&self.services, &d.by),
                                        d.text
                                    )))
                            })),
                    )
                    .child(
                        ui::rail_card(cx)
                            .child(section_label(c::GROUP_PAGE_RECENT_OUTPUTS, None, cx))
                            .children(outputs.iter().map(|(_, a)| {
                                div().t_small().text_color(k.ink).child(a.title.clone())
                            })),
                    ),
            )
            .into_any_element()
    }
}
