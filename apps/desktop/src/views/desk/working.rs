//! The 首页 right pane (COMPLETENESS.md §2.5, §2.18): only what the main
//! list does not show. 排队 (what starts later, with its time) and 在哪干活
//! (the team computers the running work uses). 正在做 is in the main list
//! only, so no item shows twice on the screen.

use gpui::{
    AnyElement, Context, InteractiveElement as _, IntoElement, ParentElement, SharedString,
    StatefulInteractiveElement as _, Styled, div, px,
};

use super::{DeskPage, model::Snapshot};
use crate::{
    app::{RightPane, Services},
    copy::{desk as c, fill},
    data::Millis,
    ui::{
        MemberAvatar,
        native::{
            InspectorSection, NativeType as _, inspector_column, metrics, native_colors,
            page::{self, SkeletonKind},
        },
        sf_symbols::Sym,
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

/// The pane: a header band (`今天`) on the toolbar row, then 排队 and
/// 在哪干活, each with a compact empty when it has nothing. `loading` draws
/// skeleton rows in both sections.
pub fn render(
    snap: &Snapshot,
    loading: bool,
    services: &Services,
    cx: &mut Context<DeskPage>,
) -> AnyElement {
    let k = native_colors(cx);
    let header = div()
        .flex_none()
        .h(metrics::TOOLBAR_ROW)
        .px(metrics::INSPECTOR_PAD)
        .border_b_1()
        .border_color(k.separator)
        .flex()
        .flex_col()
        .justify_center()
        .child(
            div()
                .n_title3()
                .font_weight(gpui::FontWeight::MEDIUM)
                .text_color(k.label)
                .child(c::DESK_RAIL_TITLE),
        )
        .child(
            div()
                .n_subheadline()
                .text_color(k.secondary)
                .child(c::DESK_RAIL_LINE),
        );
    let mut col = inspector_column(cx).child(header);
    let mut body = div()
        .id("desk-inspector")
        .flex_1()
        .min_h(px(0.))
        .overflow_y_scroll()
        .pt(px(4.))
        .pb(px(24.))
        .flex()
        .flex_col();

    // 排队.
    let mut queue = InspectorSection::new("desk-ins-queue", c::DESK_QUEUE);
    if loading {
        queue = queue.row(page::skeleton_rows(
            "desk-q-sk",
            SkeletonKind::Compact,
            3,
            cx,
        ));
    } else if snap.queue.is_empty() {
        queue = queue.row(page::compact_empty(
            Sym::Waiting,
            c::DESK_QUEUE_EMPTY,
            None,
            cx,
        ));
    }
    for (i, q) in snap.queue.iter().enumerate() {
        let member = snap.member(&q.member_id);
        let full = format!("{} {}", snap.name(&q.member_id), q.title);
        queue = queue.row(
            div()
                .id(SharedString::from(format!("desk-queue-{i}")))
                .h(px(32.))
                .flex()
                .items_center()
                .gap(px(8.))
                .n_body()
                .child(MemberAvatar::new(member.as_ref(), px(16.)))
                .child(line(full.clone()).flex_1().text_color(k.label))
                .children(q.at.map(|at| {
                    div()
                        .flex_none()
                        .n_subheadline()
                        .n_figures()
                        .text_color(k.secondary)
                        .child(clock(at))
                }))
                .tooltip(move |window, cx| {
                    gpui_component::tooltip::Tooltip::new(full.clone()).build(window, cx)
                }),
        );
    }
    body = body.child(queue);

    // 在哪干活: the computers of the running work, with who uses them.
    let mut machines: Vec<(String, Vec<String>, (String, String, String))> = Vec::new();
    for r in &snap.runs {
        let Some(name) = r.run.machine_name.clone() else {
            continue;
        };
        let who = snap.name(&r.run.bot_id);
        match machines.iter_mut().find(|(n, ..)| *n == name) {
            Some((_, names, _)) => {
                if !names.contains(&who) {
                    names.push(who)
                }
            }
            None => machines.push((
                name,
                vec![who],
                (
                    r.run.group_id.clone(),
                    r.run.topic_id.clone(),
                    r.run.id.clone(),
                ),
            )),
        }
    }
    let mut where_ = InspectorSection::new("desk-ins-where", c::DESK_WHERE);
    if loading {
        where_ = where_.row(page::skeleton_rows(
            "desk-m-sk",
            SkeletonKind::Compact,
            2,
            cx,
        ));
    } else if machines.is_empty() {
        where_ = where_.row(page::compact_empty(
            Sym::Computer,
            c::DESK_WHERE_EMPTY,
            None,
            cx,
        ));
    }
    for (i, (name, names, (g, t, run))) in machines.into_iter().enumerate() {
        let glyph = if name.to_lowercase().contains("mini") {
            Sym::MacMini
        } else {
            Sym::Computer
        };
        let users = fill(c::DESK_MACHINE_USERS, &[("names", &names.join("、"))]);
        let nav = services.nav.clone();
        where_ = where_.row(
            div()
                .id(SharedString::from(format!("desk-machine-{i}")))
                .py(px(6.))
                .flex()
                .items_center()
                .gap(px(8.))
                .child(page::row_icon(glyph, k.secondary))
                .child(
                    div()
                        .flex_1()
                        .min_w(px(0.))
                        .flex()
                        .flex_col()
                        .child(line(name).n_body().text_color(k.label))
                        .child(line(users).n_subheadline().text_color(k.secondary)),
                )
                .on_click(move |_, _, cx| {
                    let (g, t, run) = (g.clone(), t.clone(), run.clone());
                    nav.update(cx, |nav, cx| {
                        nav.open_topic(g, t, cx);
                        nav.set_right_pane(RightPane::Live(run), cx);
                    })
                }),
        );
    }
    body = body.child(where_);
    col = col.child(body);
    col.into_any_element()
}
