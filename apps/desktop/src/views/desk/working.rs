//! The 首页 right pane (COMPLETENESS.md §2.5, §2.18): only what the main
//! list does not show. 排队 (what starts later, with its time) and 在哪干活
//! (the team computers the running work uses). 正在做 is in the main list
//! only, so no item shows twice on the screen.

use gpui::{
    AnyElement, Context, InteractiveElement as _, IntoElement, ParentElement, SharedString,
    StatefulInteractiveElement as _, Styled, div,
};

use super::{DeskPage, model::Snapshot};
use crate::ui::native::FocusRow as _;
use crate::ui::native::tokens::space as tsp;
use crate::ui::{
    native::{borderless_button, page::PageLoad},
    sf_symbols::Sf,
};
use crate::{
    app::{RightPane, Services},
    copy::{desk as c, fill},
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

/// The pane (§2.5): a header band (`今天` and its close button) on the
/// toolbar row, then 这周 (DescriptionList), 排队, 最近的产出 and 在哪干活,
/// each with a compact empty when it has nothing. While the first sync runs
/// every section draws skeleton rows; when it failed, one muted line.
pub fn render(
    snap: &Snapshot,
    load: PageLoad,
    services: &Services,
    window: &mut gpui::Window,
    cx: &mut Context<DeskPage>,
) -> AnyElement {
    let k = native_colors(cx);
    let loading = load == PageLoad::Loading;
    let failed = load == PageLoad::Failed;
    let close = borderless_button("desk-rail-close", Sf::Close, c::DESK_HIDE_SIDE, window, cx)
        .on_click(cx.listener(|this, _, _, cx| this.hide_side(cx)));
    let header = div()
        .flex_none()
        .h(metrics::TOOLBAR_ROW)
        .pl(metrics::INSPECTOR_PAD)
        .pr(tsp::S8)
        .border_b_1()
        .border_color(k.separator)
        .flex()
        .items_center()
        .gap(tsp::S8)
        .child(
            div()
                .flex_1()
                .min_w_0()
                .flex()
                .flex_col()
                .child(
                    div()
                        .n_title3()
                        .font_weight(gpui::FontWeight::MEDIUM)
                        .text_color(k.label)
                        .child(c::DESK_RAIL_TITLE),
                )
                .child(
                    line(c::DESK_RAIL_LINE_WEEK)
                        .n_subheadline()
                        .text_color(k.secondary),
                ),
        )
        .child(close);
    let mut col = inspector_column(cx).child(header);
    let mut body = div()
        .id("desk-inspector")
        .flex_1()
        .min_h_0()
        .overflow_y_scroll()
        .pb(tsp::S24)
        .flex()
        .flex_col()
        // Pull the first GroupLabel up so its ink starts at y 68 (B2):
        // the section's own 16 top sits under the header band.
        .child(div().flex_none().mt(-tsp::S12));
    // One offline recipe with the 群 inspector (§2.13): the section's own
    // icon and the muted line.
    let offline = |sym: Sym| page::compact_empty(sym, c::DESK_RAIL_OFFLINE, None, cx);

    // 这周: the team's week as a DescriptionList.
    let mut week = InspectorSection::new("desk-ins-week", c::DESK_WEEK);
    if loading {
        week = week.row(page::skeleton_rows(
            "desk-w-sk",
            SkeletonKind::Compact,
            3,
            cx,
        ));
    } else if failed {
        week = week.row(offline(Sym::Chart));
    } else {
        let w = &snap.week;
        let cost = if w.cost.is_empty() {
            None
        } else {
            Some(
                w.cost
                    .iter()
                    .map(|(cur, v)| crate::views::topic_rail::money_text(*v, *cur))
                    .collect::<Vec<_>>()
                    .join(" · "),
            )
        };
        week = week
            .row(page::desc_row(c::DESK_WEEK_DONE, w.done.to_string(), cx))
            .row(page::desc_row(
                c::DESK_WEEK_OUTPUTS,
                w.outputs.to_string(),
                cx,
            ))
            .row(page::desc_row(
                c::DESK_WEEK_COST,
                match cost {
                    Some(v) => div().child(v),
                    None => div().text_color(k.secondary).child(c::DESK_WEEK_NO_COST),
                },
                cx,
            ));
    }
    body = body.child(week);

    // 排队.
    let mut queue = InspectorSection::new("desk-ins-queue", c::DESK_QUEUE);
    if failed {
        queue = queue.row(offline(Sym::Waiting));
    } else if loading {
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
    for (i, q) in snap.queue.iter().enumerate().filter(|_| !failed) {
        let member = snap.member(&q.member_id);
        let full = format!("{} {}", snap.name_by_avatar(&q.member_id), q.title);
        queue = queue.row(
            div()
                .id(SharedString::from(format!("desk-queue-{i}")))
                .focus_row(false, cx)
                .h(tsp::S32)
                .flex()
                .items_center()
                .gap(tsp::S8)
                .n_body()
                .child(MemberAvatar::new(member.as_ref(), tsp::S16))
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

    // 最近的产出: what the team made lately (the main list shows no 产出).
    let mut outputs = InspectorSection::new("desk-ins-outputs", c::DESK_RECENT_OUTPUTS);
    let mut recent = if loading || failed {
        Vec::new()
    } else {
        services.provider.all_artifacts()
    };
    recent.sort_by(|a, b| b.1.updated_at.cmp(&a.1.updated_at));
    if failed {
        outputs = outputs.row(offline(Sym::Document));
    } else if loading {
        outputs = outputs.row(page::skeleton_rows(
            "desk-o-sk",
            SkeletonKind::Compact,
            3,
            cx,
        ));
    } else if recent.is_empty() {
        outputs = outputs.row(page::compact_empty(
            Sym::Document,
            c::DESK_RECENT_OUTPUTS_EMPTY,
            None,
            cx,
        ));
    }
    let now = services.provider.clock_now();
    for (_, a) in recent.into_iter().take(4) {
        let glyph = match a.kind {
            crate::data::ArtifactKind::Image => Sym::Image,
            crate::data::ArtifactKind::File => Sym::File,
            _ => Sym::Document,
        };
        let who = crate::views::shell::member_name(services, &a.created_by);
        let tip = format!(
            "{} · {}",
            a.title,
            fill(
                c::DESK_OUTPUT_META,
                &[("v", &a.version.to_string()), ("name", &who)]
            )
        );
        let nav = services.nav.clone();
        let id = a.id.clone();
        outputs = outputs.row(
            div()
                .id(SharedString::from(format!("desk-output-{}", a.id)))
                .focus_row(false, cx)
                .h(tsp::S32)
                .flex()
                .items_center()
                .gap(tsp::S8)
                .n_body()
                .child(page::row_icon(glyph, k.secondary))
                .child(line(a.title.clone()).flex_1().text_color(k.label))
                .child(
                    div()
                        .flex_none()
                        .n_subheadline()
                        .n_figures()
                        .text_color(k.secondary)
                        .child(crate::views::group_page::short_time(now, a.updated_at)),
                )
                .tooltip(move |window, cx| {
                    gpui_component::tooltip::Tooltip::new(tip.clone()).build(window, cx)
                })
                .on_click(move |_, _, cx| {
                    let id = id.clone();
                    nav.update(cx, |n, cx| n.open_artifact(id, None, cx))
                }),
        );
    }
    body = body.child(outputs);

    // 在哪干活: the computers of the running work, with who uses them.
    let mut machines: Vec<(String, Vec<String>, (String, String, String))> = Vec::new();
    for r in &snap.runs {
        // A5: a run with no known computer still shows, as 未知电脑.
        let name = r
            .run
            .machine_name
            .clone()
            .unwrap_or_else(|| crate::copy::group_page::GROUP_PAGE_UNKNOWN_MACHINE.to_string());
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
    if failed {
        where_ = where_.row(offline(Sym::Computer));
    } else if loading {
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
        let glyph = crate::views::group_page::kit::machine_sym(&name, None);
        let users = fill(c::DESK_MACHINE_USERS, &[("names", &names.join("、"))]);
        let nav = services.nav.clone();
        where_ = where_.row(
            div()
                .id(SharedString::from(format!("desk-machine-{i}")))
                .focus_row(false, cx)
                .py(tsp::S6)
                .flex()
                .items_center()
                .gap(tsp::S8)
                .child(page::row_icon(glyph, k.secondary))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
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
