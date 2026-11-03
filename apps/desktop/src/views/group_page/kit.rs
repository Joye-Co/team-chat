//! Parts the 群, 成员 and 产出 pages share (pages track): their own 52 pt
//! title bars, the side column with its header and cards, the live
//! terminal thumbnail, plan step bars, 产出 thumbnails and time words.

use gpui::{
    AnyElement, App, Div, ElementId, Hsla, InteractiveElement as _, IntoElement, ParentElement,
    Pixels, SharedString, Stateful, StatefulInteractiveElement as _, Styled, div,
    prelude::FluentBuilder as _, px,
};

use crate::{
    app::Services,
    copy::{common, fill, group_page as c},
    data::{ArtifactSummary, Millis, PlanItem, PlanState, v2::ScreenLineKind},
    theme::{self, Type as _, space},
    ui::{self, Lucide},
};

/// 12 / 18: the v3 meta line (card meta, footers, rail card labels).
pub const META: (Pixels, Pixels) = (px(12.), px(18.));
/// 13 / 20.
pub const SMALL: (Pixels, Pixels) = (px(13.), px(20.));
/// 11 / 16: plan step names and option labels inside previews.
pub const TINY: (Pixels, Pixels) = (px(11.), px(16.));
/// The 群 and 产出 side column (Group.dc.html `aside`, Artifact.dc.html).
pub const ASIDE_WIDTH: Pixels = px(320.);
/// The 成员 side column (工作环境).
pub const MEMBER_ASIDE_WIDTH: Pixels = px(300.);
/// The 成员 list column.
pub const MEMBER_LIST_WIDTH: Pixels = px(340.);
/// The measure of a page's main column.
pub const MEASURE: Pixels = px(760.);
/// The height of a preview inside a 进行中 card.
pub const PREVIEW_HEIGHT: Pixels = px(76.);

/// The inset at the start of a title bar: the traffic lights when the rail
/// is hidden.
pub fn leading(services: &Services, cx: &App) -> Pixels {
    if services.nav.read(cx).sidebar_hidden {
        theme::TRAFFIC_LIGHTS_INSET
    } else {
        px(0.)
    }
}

/// A 52 pt title bar row.
pub fn bar() -> Div {
    div()
        .flex_none()
        .h(theme::TITLE_BAR)
        .flex()
        .items_center()
        .gap(space::S8)
        .min_w_0()
}

/// A page title (17/600).
pub fn page_title(text: impl Into<SharedString>, cx: &App) -> Div {
    let k = theme::colors(cx);
    div()
        .flex_none()
        .t_page_title()
        .text_color(k.ink)
        .child(text.into())
}

/// A 28 pt icon button of a title bar; `active` has the grey `sel` fill.
pub fn icon_button(
    id: impl Into<ElementId>,
    icon: Lucide,
    tip: &'static str,
    active: bool,
    cx: &App,
) -> Stateful<Div> {
    let k = theme::colors(cx);
    div()
        .id(id.into())
        .flex_none()
        .size(theme::TITLE_BUTTON)
        .rounded(px(7.))
        .flex()
        .items_center()
        .justify_center()
        .cursor_pointer()
        .relative()
        .when(active, |el| el.bg(k.sel))
        .when(!active, |el| el.hover(|s| s.bg(k.hover)))
        .child(ui::icon(
            icon,
            theme::icon::ROW,
            if active { k.ink } else { k.ink_2 },
        ))
        .tooltip(move |window, cx| gpui_component::tooltip::Tooltip::new(tip).build(window, cx))
}

/// The 1 × 16 divider between title bar groups.
pub fn bar_divider(cx: &App) -> Div {
    let k = theme::colors(cx);
    div().flex_none().w(px(1.)).h(px(16.)).bg(k.line_2)
}

/// The side column of a page: `rail` fill, a hairline at its start and its
/// own 52 pt header with a title and the collapse button.
pub fn aside(
    id: &'static str,
    title: &str,
    width: Pixels,
    pad: Pixels,
    on_collapse: impl Fn(&mut gpui::Window, &mut App) + 'static,
    cx: &App,
) -> Stateful<Div> {
    let k = theme::colors(cx);
    div()
        .id(id)
        .flex_none()
        .w(width)
        .h_full()
        .overflow_y_scroll()
        .bg(k.rail)
        .border_l_1()
        .border_color(k.line)
        .px(pad)
        .pb(space::S24)
        .flex()
        .flex_col()
        .child(
            bar()
                .px(space::S4)
                .child(
                    div()
                        .t_small()
                        .semibold()
                        .text_color(k.ink)
                        .child(title.to_string()),
                )
                .child(div().flex_1())
                .child(
                    icon_button(
                        SharedString::from(format!("{id}-collapse")),
                        Lucide::PanelRight,
                        c::GROUP_PAGE_COLLAPSE,
                        true,
                        cx,
                    )
                    .on_click(move |_, window, cx| on_collapse(window, cx)),
                ),
        )
}

/// A card of a side column: radius 12, `line`, `--lift`, padding 12.
pub fn side_card(cx: &App) -> Div {
    ui::card(cx).p(space::S12).flex().flex_col()
}

/// The label row of a side card (12/18 500 `ink-3`), with an optional mono
/// count.
pub fn card_label(label: &str, count: Option<usize>, cx: &App) -> Div {
    let k = theme::colors(cx);
    div()
        .flex()
        .items_center()
        .gap(space::S6)
        .h(px(24.))
        .type_role(META)
        .medium()
        .text_color(k.ink_3)
        .child(label.to_string())
        .children(count.map(|n| {
            div()
                .mono()
                .font_weight(gpui::FontWeight::NORMAL)
                .child(n.to_string())
        }))
}

/// A quiet text link (`全部`, `修改`): 24 pt, 12 `ink-2`, hover fill.
pub fn text_link(id: impl Into<ElementId>, label: &str, cx: &App) -> Stateful<Div> {
    let k = theme::colors(cx);
    div()
        .id(id.into())
        .flex_none()
        .h(px(24.))
        .px(space::S6)
        .rounded(px(6.))
        .flex()
        .items_center()
        .cursor_pointer()
        .type_role(META)
        .text_color(k.ink_2)
        .hover(|s| s.bg(k.hover))
        .child(label.to_string())
}

/// A section heading of a page (13/600) with an optional mono count.
pub fn section_head(label: &str, count: Option<usize>, cx: &App) -> Div {
    let k = theme::colors(cx);
    div()
        .flex()
        .items_center()
        .gap(space::S8)
        .h(px(24.))
        .child(
            div()
                .t_small()
                .semibold()
                .text_color(k.ink)
                .child(label.to_string()),
        )
        .children(count.map(|n| {
            div()
                .mono()
                .type_role(META)
                .text_color(k.ink_3)
                .child(n.to_string())
        }))
}

/// A round dot.
pub fn dot(color: Hsla, size: f32) -> Div {
    div().flex_none().size(px(size)).rounded_full().bg(color)
}

/// A hollow ring (waits on someone else).
pub fn ring_dot(color: Hsla) -> Div {
    div()
        .flex_none()
        .size(px(7.))
        .rounded_full()
        .border(px(1.5))
        .border_color(color)
}

/// A live dot that breathes.
pub fn live_dot(id: impl Into<ElementId>, size: f32, cx: &App) -> AnyElement {
    let k = theme::colors(cx);
    ui::motion::breathe(id, dot(k.live, size), cx)
}

/// `+214 −63` in mono, green and red.
pub fn line_counts(additions: u32, deletions: u32, cx: &App) -> Div {
    let k = theme::colors(cx);
    div()
        .flex_none()
        .flex()
        .gap(px(4.))
        .mono()
        .child(div().text_color(k.ok_ink).child(format!("+{additions}")))
        .child(div().text_color(k.bad_ink).child(format!("−{deletions}")))
}

/// `已 3 分钟` / `已 2 小时` from `since` to `now`.
pub fn elapsed(now: Millis, since: Millis) -> String {
    let minutes = ((now - since).max(0) / 60_000) as u64;
    if minutes < 60 {
        fill(c::ELAPSED_MINUTES, &[("n", &minutes.max(1).to_string())])
    } else {
        fill(c::ELAPSED_HOURS, &[("n", &(minutes / 60).to_string())])
    }
}

/// A date as the week lists print it: `11-03`.
pub fn month_day(at: Millis) -> String {
    use chrono::{Local, TimeZone as _};
    Local
        .timestamp_millis_opt(at)
        .single()
        .map(|t| t.format("%m-%d").to_string())
        .unwrap_or_default()
}

/// `今天 09:55`, `昨天`, or `10-31` relative to `now`.
pub fn day_and_time(now: Millis, at: Millis, with_time_yesterday: bool) -> String {
    use chrono::{Local, TimeZone as _};
    let (Some(n), Some(t)) = (
        Local.timestamp_millis_opt(now).single(),
        Local.timestamp_millis_opt(at).single(),
    ) else {
        return String::new();
    };
    if t.date_naive() == n.date_naive() {
        format!("{} {}", crate::copy::timeline::TODAY, t.format("%H:%M"))
    } else if t.date_naive().succ_opt() == Some(n.date_naive()) {
        if with_time_yesterday {
            format!("{} {}", common::YESTERDAY, t.format("%H:%M"))
        } else {
            common::YESTERDAY.to_string()
        }
    } else {
        t.format("%m-%d").to_string()
    }
}

/// Overlapping member avatars (20 pt, ring of the page colour), at most
/// five.
pub fn avatar_stack(services: &Services, ids: &[String], cx: &App) -> Div {
    let k = theme::colors(cx);
    let members: Vec<_> = ids
        .iter()
        .filter_map(|id| services.provider.member(id))
        .filter(|m| !m.deactivated)
        .take(5)
        .collect();
    div()
        .flex()
        .items_center()
        .children(members.into_iter().enumerate().map(|(i, m)| {
            div()
                .flex_none()
                .when(i > 0, |el| el.ml(px(-4.)))
                .rounded(px(7.))
                .border_2()
                .border_color(k.bg)
                .child(ui::MemberAvatar::new(Some(&m), px(20.)))
        }))
}

/// A plan as 3 pt segments: done `ink-3`, current `live` (breathing),
/// pending `line-2`.
pub fn step_bar(id: &str, plan: &[PlanItem], gap: Pixels, cx: &App) -> Div {
    let k = theme::colors(cx);
    div()
        .flex()
        .gap(gap)
        .children(plan.iter().enumerate().map(|(i, p)| {
            let seg = div().flex_1().h(px(3.)).rounded(px(2.));
            match p.state {
                PlanState::Done => seg.bg(k.ink_3).into_any_element(),
                PlanState::Current => ui::motion::breathe(
                    SharedString::from(format!("{id}-step-{i}")),
                    seg.bg(k.live),
                    cx,
                ),
                _ => seg.bg(k.line_2).into_any_element(),
            }
        }))
}

/// `n/m` of a plan: steps done (the current one counts) over all.
pub fn plan_progress(plan: &[PlanItem]) -> Option<(usize, usize)> {
    if plan.is_empty() {
        return None;
    }
    let done = plan.iter().filter(|p| p.state == PlanState::Done).count();
    let current = plan.iter().any(|p| p.state == PlanState::Current) as usize;
    Some(((done + current).min(plan.len()), plan.len()))
}

/// The live terminal of a run as a small dark screen: `实时` with the
/// machine at the top, then the newest lines.
pub fn mini_screen(
    services: &Services,
    id: &str,
    run_id: &str,
    height: Pixels,
    traffic_lights: bool,
    cx: &App,
) -> Option<Div> {
    let k = theme::colors(cx);
    let screen = services.provider.machine_screen(run_id)?;
    // Rows of 12 + 4 pt under the 16 pt header; the last one may be cut.
    let rows = ((f32::from(height) - 26.) / 16.).ceil().max(1.) as usize;
    let lines: Vec<_> = screen
        .lines
        .iter()
        .rev()
        .take(rows)
        .rev()
        .cloned()
        .collect();
    let chip = div()
        .flex_none()
        .flex()
        .items_center()
        .gap(px(5.))
        .h(px(16.))
        .px(px(6.))
        .rounded(px(4.))
        .bg(k.screen_ink.opacity(0.1))
        .text_size(px(10.))
        .text_color(k.screen_ink)
        .child(live_dot(SharedString::from(format!("{id}-live")), 5., cx))
        .child(c::GROUP_PAGE_LIVE);
    let lights = traffic_lights.then(|| {
        div()
            .flex()
            .gap(px(5.))
            .child(dot(gpui::rgb(0xFF5F57).into(), 6.))
            .child(dot(gpui::rgb(0xFEBC2E).into(), 6.))
            .child(dot(gpui::rgb(0x28C840).into(), 6.))
    });
    Some(
        div()
            .flex_none()
            .h(height)
            .rounded(theme::radius::MD)
            .bg(k.screen)
            .overflow_hidden()
            .px(space::S10)
            .pt(space::S8)
            .flex()
            .flex_col()
            .gap(px(6.))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .children(lights)
                    .child(chip)
                    .children(screen.machine_name.map(|m| {
                        div()
                            .mono()
                            .text_size(px(10.))
                            .line_height(px(12.))
                            .text_color(k.screen_dim)
                            .child(m)
                    })),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(4.))
                    .mono()
                    .text_size(px(10.))
                    .line_height(px(12.))
                    .children(lines.into_iter().map(|l| {
                        let (color, text) = match l.kind {
                            ScreenLineKind::Command => (k.screen_dim, format!("$ {}", l.text)),
                            ScreenLineKind::Result => (k.screen_ink, l.text),
                            ScreenLineKind::Error => (k.bad, l.text),
                            ScreenLineKind::Output => (k.screen_ink.opacity(0.8), l.text),
                        };
                        div().one_line().text_color(color).child(text)
                    })),
            ),
    )
}

/// A 产出 thumbnail: a little page with its title and first rows, or the
/// kind's glyph when the provider has no picture.
pub fn artifact_thumb(
    services: &Services,
    a: &ArtifactSummary,
    w: Pixels,
    h: Pixels,
    cx: &App,
) -> Div {
    let k = theme::colors(cx);
    let frame = div()
        .flex_none()
        .w(w)
        .h(h)
        .rounded(px(6.))
        .border_1()
        .border_color(k.line)
        .bg(k.raised)
        .overflow_hidden();
    let Some(t) = services.provider.artifact_thumb(&a.id) else {
        return frame.flex().items_center().justify_center().child(ui::icon(
            Lucide::FileText,
            theme::icon::ROW,
            k.ink_3,
        ));
    };
    let scale = f32::from(w) / 64.;
    frame
        .px(px(6. * scale))
        .py(px(4. * scale))
        .flex()
        .flex_col()
        .gap(px(1.))
        .child(
            div()
                .text_size(px(8. * scale))
                .line_height(px(10. * scale))
                .semibold()
                .text_color(k.ink)
                .whitespace_nowrap()
                .child(t.title),
        )
        .children(t.rows.into_iter().map(|r| {
            div()
                .flex()
                .text_size(px(7. * scale))
                .line_height(px(9. * scale))
                .text_color(k.ink_2)
                .whitespace_nowrap()
                .child(r.label)
                .child(div().flex_1())
                .children(r.value.map(|v| div().mono().child(v)))
                .when(r.ok, |el| el.child(div().text_color(k.ok_ink).child("✓")))
        }))
}
