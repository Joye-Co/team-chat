//! 首页: the desk (Main.dc.html). Jojo's one-line priority, 需要你拍板 (asks
//! for me), 做完了 today, and at the right 正在做 and 排队.
//!
//! Foundation stub: the sections read the provider and show their data in
//! plain cards; the desk track renders the real interactive cards, proof
//! chips and the live terminal thumbnail.

pub mod scenes;

use gpui::{
    App, Context, InteractiveElement as _, IntoElement, ParentElement, Render, SharedString,
    StatefulInteractiveElement as _, Styled, Subscription, Window, div,
    prelude::FluentBuilder as _, px,
};

use crate::{
    app::Services,
    copy::desk as c,
    data::PlanState,
    theme::{self, Type as _, space},
    ui::{self, Ring},
    views::shell::{
        PageView, TitleBarSpec, clock, identity, panes::watch, section_label, topic_path,
    },
};

pub struct DeskPage {
    services: Services,
    _subscriptions: Vec<Subscription>,
}

impl DeskPage {
    pub fn new(services: Services, cx: &mut Context<Self>) -> Self {
        Self {
            _subscriptions: watch(&services, cx),
            services,
        }
    }
}

/// `11 月 3 日 周一`.
pub fn today_line() -> String {
    use chrono::Datelike as _;
    let now = chrono::Local::now();
    const DAYS: [&str; 7] = [
        c::WEEKDAY_1,
        c::WEEKDAY_2,
        c::WEEKDAY_3,
        c::WEEKDAY_4,
        c::WEEKDAY_5,
        c::WEEKDAY_6,
        c::WEEKDAY_7,
    ];
    crate::copy::fill(
        c::DESK_DATE,
        &[
            ("m", &now.month().to_string()),
            ("d", &now.day().to_string()),
            (
                "weekday",
                DAYS[now.weekday().num_days_from_monday() as usize],
            ),
        ],
    )
}

impl PageView for DeskPage {
    fn title_bar(&self, _cx: &App) -> TitleBarSpec {
        TitleBarSpec {
            subtitle: Some(today_line()),
            ..TitleBarSpec::page(c::DESK_TITLE)
        }
    }
}

impl Render for DeskPage {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let k = theme::colors(cx);
        let p = self.services.provider.clone();
        let note = p.desk_note().map(|n| {
            div()
                .flex()
                .items_center()
                .gap(space::S8)
                .child(identity(&self.services, &n.member_id, None, cx))
                .child(div().t_small().text_color(k.ink_2).child(n.text))
        });
        let asks = p.asks_for_me();
        let ask_cards = asks.iter().enumerate().map(|(i, a)| {
            let nav = self.services.nav.clone();
            let (g, t) = (a.group_id.clone(), a.topic_id.clone());
            let card = ui::card(cx)
                .id(SharedString::from(format!("desk-ask-{}", a.topic_id)))
                .cursor_pointer()
                .p(space::S16)
                .flex()
                .flex_col()
                .gap(space::S10)
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(space::S8)
                        .child(identity(&self.services, &a.from, None, cx))
                        .child(
                            div()
                                .flex_1()
                                .one_line()
                                .t_meta()
                                .text_color(k.ink_3)
                                .child(topic_path(&self.services, &a.group_id, &a.topic_id)),
                        )
                        .child(div().t_mono().text_color(k.ink_3).child(clock(a.at))),
                )
                .child(
                    div()
                        .t_card_title()
                        .text_color(k.ink)
                        .child(a.title.clone()),
                )
                .on_click(move |_, _, cx| {
                    let (g, t) = (g.clone(), t.clone());
                    nav.update(cx, |nav, cx| nav.open_topic(g, t, cx))
                });
            ui::motion::rise(
                SharedString::from(format!("desk-ask-rise-{i}")),
                i,
                card,
                cx,
            )
        });
        let done = p.done_today();
        let done_rows = done.iter().enumerate().map(|(i, d)| {
            crate::views::shell::stub_row(
                SharedString::from(format!("desk-done-{i}")),
                asks.len() + i,
                ui::MemberAvatar::new(p.member(&d.member_id).as_ref(), px(16.)).into_any_element(),
                format!(
                    "{} {}",
                    crate::views::shell::member_name(&self.services, &d.member_id),
                    d.title
                ),
                Some(clock(d.at)),
                cx,
            )
        });
        let runs = p.active_runs();
        let run_cards = runs.iter().enumerate().map(|(i, r)| {
            let done = r.plan.iter().filter(|x| x.state == PlanState::Done).count();
            let current = r
                .plan
                .iter()
                .find(|x| matches!(x.state, PlanState::Current | PlanState::Waiting))
                .map(|x| x.text.clone())
                .unwrap_or_default();
            let title = p
                .topic(&r.group_id, &r.topic_id)
                .map(|t| t.title)
                .unwrap_or_default();
            let card = ui::rail_card(cx)
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(space::S8)
                        .child(identity(&self.services, &r.bot_id, Some(Ring::Live), cx))
                        .child(
                            div()
                                .flex_1()
                                .one_line()
                                .t_small()
                                .text_color(k.ink_2)
                                .child(title),
                        )
                        .child(div().t_mono().text_color(k.ink_3).child(format!(
                            "{}/{}",
                            (done + 1).min(r.plan.len()),
                            r.plan.len()
                        ))),
                )
                .child(div().t_small().text_color(k.ink_2).child(current));
            ui::motion::rise(SharedString::from(format!("desk-run-{i}")), i, card, cx)
        });
        let queue = p.queue();
        let queue_card = (!queue.is_empty()).then(|| {
            ui::rail_card(cx)
                .child(section_label(c::DESK_QUEUE, None, cx))
                .children(queue.iter().map(|q| {
                    div()
                        .flex()
                        .items_center()
                        .gap(space::S8)
                        .child(ui::MemberAvatar::new(
                            p.member(&q.member_id).as_ref(),
                            px(16.),
                        ))
                        .child(div().flex_1().t_small().text_color(k.ink).child(format!(
                            "{} {}",
                            crate::views::shell::member_name(&self.services, &q.member_id),
                            q.title
                        )))
                        .children(q.at.map(|a| div().t_mono().text_color(k.ink_3).child(clock(a))))
                }))
        });
        div()
            .size_full()
            .flex()
            .child(
                div()
                    .id("desk-main")
                    .flex_1()
                    .min_w(px(0.))
                    .h_full()
                    .overflow_y_scroll()
                    .px(space::S32)
                    .pb(space::S32)
                    .child(
                        div()
                            .max_w(theme::PAGE_MEASURE)
                            .mx_auto()
                            .flex()
                            .flex_col()
                            .gap(space::S12)
                            .children(note)
                            .child(section_label(c::DESK_NEEDS_YOU, Some(asks.len()), cx))
                            .children(ask_cards)
                            .when(!done.is_empty(), |el| {
                                el.child(section_label(c::DESK_DONE_TODAY, None, cx))
                            })
                            .children(done_rows),
                    ),
            )
            .child(
                div()
                    .id("desk-side")
                    .flex_none()
                    .w(theme::DESK_SIDE_WIDTH + space::S32)
                    .h_full()
                    .overflow_y_scroll()
                    .bg(k.rail)
                    .p(space::S16)
                    .flex()
                    .flex_col()
                    .gap(space::S10)
                    .child(section_label(c::DESK_WORKING, Some(runs.len()), cx))
                    .children(run_cards)
                    .children(queue_card),
            )
    }
}
