//! The 首页 inspector (NATIVE.md §2, §9.3, §9.4): 正在做 as inspector rows,
//! one block per run (member, 话题, the step in progress and a 4 pt plan
//! bar in `blue`), then 排队. No terminal tiles.

use gpui::{
    AnyElement, App, Context, InteractiveElement as _, IntoElement, ParentElement, SharedString,
    StatefulInteractiveElement as _, Styled, div, px,
};

use super::{DeskPage, model::RunCard, model::Snapshot};
use crate::{
    app::{RightPane, Services},
    copy::{desk as c, fill},
    data::Millis,
    ui::{
        MemberAvatar,
        native::{InspectorSection, NativeType as _, determinate_bar, inspector_column, metrics, native_colors},
    },
    views::{group_page::rows::line, shell::clock},
};

/// `已 3 分钟`.
pub fn elapsed(since: Millis) -> Option<String> {
    let mins = (crate::selftest::clock::local_now().timestamp_millis() - since) / 60_000;
    if mins < 0 {
        return None;
    }
    Some(if mins < 60 {
        fill(c::DESK_ELAPSED_MINUTES, &[("n", &mins.max(1).to_string())])
    } else {
        fill(c::DESK_ELAPSED_HOURS, &[("n", &(mins / 60).to_string())])
    })
}

pub fn render(snap: &Snapshot, services: &Services, cx: &mut Context<DeskPage>) -> AnyElement {
    let mut col = inspector_column(cx)
        .id("desk-inspector")
        .overflow_y_scroll()
        // The first header sits under the toolbar row.
        .pt(metrics::TOOLBAR_ROW - px(12.))
        .pb(px(16.));
    if !snap.runs.is_empty() {
        let mut sec = InspectorSection::new("desk-ins-working", c::DESK_WORKING);
        for (i, r) in snap.runs.iter().enumerate() {
            sec = sec.row(run_block(i, r, snap, services, cx));
        }
        col = col.child(sec);
    }
    if !snap.queue.is_empty() {
        let k = native_colors(cx);
        let mut sec = InspectorSection::new("desk-ins-queue", c::DESK_QUEUE);
        for q in &snap.queue {
            let member = snap.member(&q.member_id);
            sec = sec.row(
                div()
                    .h(metrics::INSPECTOR_ROW)
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .n_body()
                    .child(MemberAvatar::new(member.as_ref(), px(16.)))
                    .child(
                        line(format!("{} {}", snap.name(&q.member_id), q.title))
                            .flex_1()
                            .text_color(k.label),
                    )
                    .children(q.at.map(|at| {
                        div()
                            .flex_none()
                            .n_subheadline()
                            .n_figures()
                            .text_color(k.secondary)
                            .child(clock(at))
                    })),
            );
        }
        col = col.child(sec);
    }
    col.into_any_element()
}

fn run_block(
    i: usize,
    r: &RunCard,
    snap: &Snapshot,
    services: &Services,
    cx: &App,
) -> AnyElement {
    let k = native_colors(cx);
    let member = snap.member(&r.run.bot_id);
    let (n, m) = r.step();
    let step = r.current_text();
    let since = r.step_started.and_then(elapsed);
    let detail = match (step, since) {
        (Some(s), Some(t)) => Some(format!("{s} · {t}")),
        (s, t) => s.or(t),
    };
    let nav = services.nav.clone();
    let (g, t, id) = (
        r.run.group_id.clone(),
        r.run.topic_id.clone(),
        r.run.id.clone(),
    );
    div()
        .id(SharedString::from(format!("desk-run-{i}")))
        .py(px(6.))
        .flex()
        .flex_col()
        .gap(px(2.))
        .child(
            div()
                .h(px(20.))
                .flex()
                .items_center()
                .gap(px(8.))
                .n_body()
                .child(MemberAvatar::new(member.as_ref(), px(16.)))
                .child(
                    div()
                        .flex_none()
                        .text_color(k.label)
                        .child(snap.name(&r.run.bot_id)),
                )
                .child(line(r.topic_title.clone()).flex_1().text_color(k.label))
                .child(
                    div()
                        .flex_none()
                        .n_figures()
                        .text_color(k.secondary)
                        .child(format!("{n}/{m}")),
                ),
        )
        .children(detail.map(|d| {
            line(d)
                .pl(px(24.))
                .n_subheadline()
                .text_color(k.secondary)
        }))
        .child(
            div()
                .pl(px(24.))
                .pt(px(4.))
                .child(determinate_bar(
                    if m == 0 { 0. } else { n as f32 / m as f32 },
                    Some(k.blue),
                    cx,
                )),
        )
        .on_click(move |_, _, cx| {
            let (g, t, id) = (g.clone(), t.clone(), id.clone());
            nav.update(cx, |nav, cx| {
                nav.open_topic(g, t, cx);
                nav.set_right_pane(RightPane::Live(id), cx);
            })
        })
        .into_any_element()
}
