//! The right column of the 首页: 正在做 (each active run, with a live terminal
//! thumbnail when the run reported tool events, else its plan progress) and
//! 排队 (only when something is queued).

use gpui::{
    AnyElement, App, Context, InteractiveElement as _, IntoElement, ParentElement, SharedString,
    StatefulInteractiveElement as _, Styled, div, prelude::FluentBuilder as _, px,
};

use super::{
    DeskPage, SIDE_WIDTH,
    model::{RunCard, Snapshot},
    side_toggle,
};
use crate::{
    app::{RightPane, Services},
    copy::{desk as c, fill},
    data::{
        Millis,
        v2::{QueuedItem, ScreenLineKind},
    },
    theme::{self, Type as _, desk_screen, space},
    ui::{self, Ring},
    views::shell::clock,
};

pub fn render(snap: &Snapshot, services: &Services, cx: &mut Context<DeskPage>) -> AnyElement {
    let k = theme::colors(cx);
    let header = div()
        .flex_none()
        .h(theme::TITLE_BAR)
        .px(space::S4)
        .flex()
        .items_center()
        .gap(space::S8)
        .child(
            div()
                .t_small()
                .semibold()
                .text_color(k.ink)
                .child(c::DESK_WORKING),
        )
        .child(
            div()
                .t_meta()
                .mono()
                .text_color(k.ink_3)
                .child(snap.runs.len().to_string()),
        )
        .child(div().flex_1())
        .child(side_toggle("desk-hide-side", true, cx));
    let cards = snap.runs.iter().enumerate().map(|(i, r)| {
        let card = run_card(r, snap, services, cx);
        ui::motion::rise(
            SharedString::from(format!("desk-run-rise-{}", r.run.id)),
            i + 2,
            card,
            cx,
        )
    });
    let queue = (!snap.queue.is_empty()).then(|| {
        ui::motion::rise(
            "desk-queue-rise",
            snap.runs.len() + 2,
            queue_card(&snap.queue, snap, cx),
            cx,
        )
    });
    div()
        .id("desk-side")
        .flex_none()
        .w(SIDE_WIDTH)
        .h_full()
        .overflow_y_scroll()
        .bg(k.rail)
        .border_l_1()
        .border_color(k.line)
        .px(space::S16)
        .pb(space::S24)
        .flex()
        .flex_col()
        .child(header)
        .child(
            div()
                .flex()
                .flex_col()
                .gap(space::S10)
                .children(cards)
                .children(queue.map(|q| div().mt(space::S6).child(q))),
        )
        .into_any_element()
}

/// `已 3 分钟`.
fn elapsed(since: Millis) -> Option<String> {
    let mins = (chrono::Local::now().timestamp_millis() - since) / 60_000;
    if mins < 0 {
        return None;
    }
    Some(if mins < 60 {
        fill(c::DESK_ELAPSED_MINUTES, &[("n", &mins.max(1).to_string())])
    } else {
        fill(c::DESK_ELAPSED_HOURS, &[("n", &(mins / 60).to_string())])
    })
}

fn run_card(
    r: &RunCard,
    snap: &Snapshot,
    services: &Services,
    cx: &App,
) -> gpui::Stateful<gpui::Div> {
    let k = theme::colors(cx);
    let member = snap.member(&r.run.bot_id);
    let (n, total) = r.step();
    let mut line = r.current_text().unwrap_or_default();
    if r.screen.is_some()
        && let Some(e) = r.step_started.and_then(elapsed)
    {
        line = format!("{line} · {e}");
    }
    let progress = (r.screen.is_none() && total > 0).then(|| {
        div()
            .mt(space::S8)
            .flex()
            .gap(px(3.))
            .children((0..total).map(|i| {
                div()
                    .flex_1()
                    .h(px(3.))
                    .rounded(px(2.))
                    .bg(if i < n { k.ink_3 } else { k.line_2 })
            }))
    });
    let body = div()
        .flex()
        .gap(space::S10)
        .p(space::S12)
        .child(
            div()
                .flex_none()
                .mt(px(1.))
                .child(ui::MemberAvatar::new(member.as_ref(), px(24.)).ring(Some(Ring::Live))),
        )
        .child(
            div()
                .flex_1()
                .min_w(px(0.))
                .flex()
                .flex_col()
                .gap(px(1.))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(space::S6)
                        .t_small()
                        .child(
                            div()
                                .flex_none()
                                .medium()
                                .text_color(k.ink)
                                .child(snap.name(&r.run.bot_id)),
                        )
                        .child(
                            div()
                                .flex_1()
                                .min_w(px(0.))
                                .one_line()
                                .text_color(k.ink_2)
                                .child(r.topic_title.clone()),
                        )
                        .when(total > 0, |el| {
                            el.child(
                                div()
                                    .flex_none()
                                    .t_meta()
                                    .mono()
                                    .text_color(k.ink_3)
                                    .child(format!("{n}/{total}")),
                            )
                        }),
                )
                .when(!line.is_empty(), |el| {
                    el.child(
                        div()
                            .text_size(px(12.))
                            .line_height(px(18.))
                            .text_color(k.ink_2)
                            .line_clamp(2)
                            .child(line),
                    )
                })
                .children(progress),
        );
    let nav = services.nav.clone();
    let (g, t, run_id) = (
        r.run.group_id.clone(),
        r.run.topic_id.clone(),
        r.run.id.clone(),
    );
    let live = r.screen.is_some();
    ui::card(cx)
        .id(SharedString::from(format!("desk-run-{}", r.run.id)))
        .overflow_hidden()
        .cursor_pointer()
        .when(!live, |el| el.hover(|s| s.bg(k.hover)))
        .flex()
        .flex_col()
        .children(r.screen.as_ref().map(|_| screen(r, cx)))
        .child(body)
        .on_click(move |_, _, cx| {
            let (g, t, run_id) = (g.clone(), t.clone(), run_id.clone());
            nav.update(cx, |nav, cx| {
                nav.open_topic(g, t, cx);
                if live {
                    nav.set_right_pane(RightPane::Live(run_id), cx);
                }
            });
        })
}

/// The live terminal thumbnail: window dots, a breathing `实时` pill, the
/// machine, and the run's latest terminal lines moving up.
fn screen(r: &RunCard, cx: &App) -> AnyElement {
    use gpui::{Animation, AnimationExt as _};
    let k = theme::colors(cx);
    let Some(s) = &r.screen else {
        return div().into_any_element();
    };
    let lines: Vec<String> = s
        .lines
        .iter()
        .rev()
        .take(12)
        .rev()
        .map(|l| match l.kind {
            ScreenLineKind::Command => format!("$ {}", l.text),
            _ => l.text.clone(),
        })
        .collect();
    let step = desk_screen::LINE.1 + desk_screen::LINE_GAP;
    let copy_h = step * lines.len() as f32;
    let visible = desk_screen::HEIGHT - desk_screen::TOP;
    let moving = copy_h > visible;
    let column = |lines: &[String]| {
        div()
            .flex()
            .flex_col()
            .gap(desk_screen::LINE_GAP)
            .pb(desk_screen::LINE_GAP)
            .mono()
            .text_size(desk_screen::LINE.0)
            .line_height(desk_screen::LINE.1)
            .text_color(k.screen_ink)
            .children(lines.iter().map(|l| div().one_line().child(l.clone())))
    };
    let list = div()
        .relative()
        .flex()
        .flex_col()
        .child(column(&lines))
        .when(moving, |el| el.child(column(&lines)));
    let list: AnyElement = if !moving {
        list.into_any_element()
    } else if cx.reduce_motion() {
        list.top(visible - copy_h).into_any_element()
    } else {
        let run = desk_screen::SCROLL_PER_LINE * lines.len() as u32;
        list.with_animation(
            SharedString::from(format!("desk-screen-scroll-{}", r.run.id)),
            // Every frame renders the page again: decorative rate only.
            Animation::new(run)
                .repeat()
                .with_max_fps(theme::motion::DECORATIVE_FPS),
            move |el, t| el.top(-copy_h * t),
        )
        .into_any_element()
    };
    let dots = theme::screen_dots();
    let mut pill_bg = k.screen_ink;
    pill_bg.a = 0.12;
    div()
        .relative()
        .h(desk_screen::HEIGHT)
        .rounded_t(theme::radius::LG - px(1.))
        .bg(k.screen)
        .overflow_hidden()
        .child(
            div()
                .absolute()
                .left(space::S10)
                .top(space::S10)
                .right(space::S10)
                .flex()
                .items_center()
                .gap(space::S6)
                .children(
                    dots.iter()
                        .map(|d| div().size(px(6.)).rounded_full().bg(*d)),
                )
                .child(
                    div()
                        .ml(space::S6)
                        .flex()
                        .items_center()
                        .gap(px(5.))
                        .h(px(16.))
                        .px(space::S6)
                        .rounded(px(4.))
                        .bg(pill_bg)
                        .text_size(px(10.))
                        .line_height(px(16.))
                        .text_color(k.screen_ink)
                        .child(ui::motion::breathe(
                            SharedString::from(format!("desk-live-dot-{}", r.run.id)),
                            div().size(px(5.)).rounded_full().bg(k.live),
                            cx,
                        ))
                        .child(c::DESK_LIVE),
                )
                .child(div().flex_1())
                .children(s.machine_name.clone().map(|m| {
                    div()
                        .max_w(px(120.))
                        .one_line()
                        .mono()
                        .text_size(desk_screen::LINE.0)
                        .line_height(desk_screen::LINE.1)
                        .text_color(k.screen_dim)
                        .child(m)
                })),
        )
        .child(
            div()
                .absolute()
                .left(space::S10)
                .right(space::S10)
                .top(desk_screen::TOP)
                .bottom_0()
                .overflow_hidden()
                .child(list),
        )
        .into_any_element()
}

fn queue_card(queue: &[QueuedItem], snap: &Snapshot, cx: &App) -> gpui::Div {
    let k = theme::colors(cx);
    ui::card(cx)
        .p(space::S12)
        .flex()
        .flex_col()
        .gap(space::S10)
        .child(
            div()
                .t_meta_medium()
                .text_color(k.ink_3)
                .child(c::DESK_QUEUE),
        )
        .children(queue.iter().map(|q| {
            let member = snap.member(&q.member_id);
            div()
                .flex()
                .items_center()
                .gap(space::S10)
                .t_small()
                .text_color(k.ink_2)
                .child(ui::MemberAvatar::new(member.as_ref(), px(18.)))
                .child(div().flex_1().min_w(px(0.)).one_line().child(format!(
                    "{} {}",
                    snap.name(&q.member_id),
                    q.title
                )))
                .children(q.at.map(|at| {
                    div()
                        .flex_none()
                        .t_meta()
                        .mono()
                        .text_color(k.ink_3)
                        .child(clock(at))
                }))
        }))
}
