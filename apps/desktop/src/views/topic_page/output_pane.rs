//! The 产出 pane beside a 话题 (Topic.dc.html right pane): the header with
//! the 产出's title, version, 在窗口打开 and close, and the page itself on a
//! lifted sheet, drawn from the 产出's real preview (metrics, the result
//! table and the chart, whose bars grow once).
//!
//! `ArtifactPane` (artifact track) calls [`render`] for a 产出 of the open
//! 话题; without a preview the body says so and offers 在窗口打开.

use std::time::Duration;

use gpui::{
    AnyElement, App, ClickEvent, ElementId, InteractiveElement as _, IntoElement, ParentElement,
    StatefulInteractiveElement as _, Styled, div, prelude::FluentBuilder as _, px,
};

use crate::{
    app::{RightPane, Services},
    copy::{common, fill, topic_rail as c},
    data::v2::ArtifactPreview,
    theme::{self, Type as _, space},
    ui::{
        self, Sym,
        native::{NativeType as _, PushSize, PushStyle, metrics, native_colors, push_button},
    },
    views::shell::clock,
};

/// A bar grows from zero to `share` of its track once, after `delay`. With
/// Reduce motion it is drawn at its width.
fn bar_width(id: ElementId, bar: gpui::Div, share: f32, delay: Duration, cx: &App) -> AnyElement {
    use gpui::{Animation, AnimationExt as _};
    if cx.reduce_motion() {
        return bar.w(gpui::relative(share)).into_any_element();
    }
    let run = theme::motion::BAR_GROW;
    bar.with_animation(
        id,
        Animation::new(delay + run)
            .with_easing(move |t| ui::motion::delayed_progress(t, delay, run)),
        move |el, p| el.w(gpui::relative(share * p)),
    )
    .into_any_element()
}

fn page(preview: &ArtifactPreview, id: &str, cx: &App) -> AnyElement {
    let k = theme::colors(cx);
    let metrics = (!preview.metrics.is_empty()).then(|| {
        div()
            .mt(space::S20)
            .flex()
            .rounded(theme::radius::FIELD)
            .border_1()
            .border_color(k.line)
            .children(preview.metrics.iter().enumerate().map(|(i, m)| {
                div()
                    .flex_1()
                    .flex_basis(px(0.))
                    .py(space::S12)
                    .px(space::S14)
                    .when(i > 0, |el| el.border_l_1().border_color(k.line))
                    .child(
                        div()
                            .t_meta()
                            .line_height(px(18.))
                            .text_color(k.ink_3)
                            .child(m.label.clone()),
                    )
                    .child(
                        div()
                            .mt(space::S4)
                            .flex()
                            .items_baseline()
                            .gap(space::S4)
                            .child(
                                div()
                                    .mono()
                                    .text_size(crate::ui::native::tokens::text::HERO.0)
                                    .line_height(px(32.))
                                    .font_weight(gpui::FontWeight::SEMIBOLD)
                                    .text_color(k.ink)
                                    .child(m.value.clone()),
                            )
                            .children(m.unit.clone().map(|u| {
                                div()
                                    .text_size(crate::ui::native::tokens::text::UI.0)
                                    .medium()
                                    .text_color(k.ink_3)
                                    .child(u)
                            })),
                    )
                    .children(m.note.clone().map(|n| {
                        div()
                            .t_meta()
                            .line_height(px(18.))
                            .text_color(k.ink_2)
                            .child(n)
                    }))
            }))
    });
    let table = (!preview.rows.is_empty()).then(|| {
        let col = |i: usize| preview.columns.get(i).cloned().unwrap_or_default();
        div()
            .children(preview.table_title.clone().map(|t| {
                div()
                    .mt(space::S24)
                    .t_small()
                    .semibold()
                    .text_color(k.ink)
                    .child(t)
            }))
            .child(
                div()
                    .mt(space::S6)
                    .h(crate::ui::native::tokens::height::DEFAULT)
                    .flex()
                    .items_center()
                    .t_meta()
                    .medium()
                    .text_color(k.ink_3)
                    .child(div().flex_1().child(col(0)))
                    .child(div().w(px(84.)).child(col(1)))
                    .child(div().w(px(72.)).flex().justify_end().child(col(2))),
            )
            .children(preview.rows.iter().enumerate().map(|(i, r)| {
                let last = i + 1 == preview.rows.len();
                div()
                    .h(crate::ui::native::tokens::height::LG)
                    .flex()
                    .items_center()
                    .border_t_1()
                    .when(last, |el| el.border_b_1())
                    .border_color(k.line)
                    .t_small()
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .one_line()
                            .text_color(k.ink)
                            .child(r.label.clone()),
                    )
                    .child(
                        div()
                            .w(px(84.))
                            .flex()
                            .items_center()
                            .gap(space::S6)
                            .text_color(k.ink_2)
                            .children(r.ok.map(|ok| {
                                if ok {
                                    ui::icon(Sym::Check, px(13.), k.ok).into_any_element()
                                } else {
                                    ui::icon(Sym::Failed, px(13.), k.bad).into_any_element()
                                }
                            }))
                            .children(
                                r.ok.map(|ok| if ok { c::RESULT_PASS } else { c::RESULT_FAIL }),
                            ),
                    )
                    .child(
                        div()
                            .w(px(72.))
                            .flex()
                            .justify_end()
                            .t_mono()
                            .text_color(k.ink_2)
                            .child(r.value.clone()),
                    )
            }))
    });
    let chart = (!preview.bars.is_empty()).then(|| {
        div()
            .child(
                div()
                    .mt(space::S24)
                    .flex()
                    .items_baseline()
                    .gap(space::S8)
                    .children(
                        preview
                            .chart_title
                            .clone()
                            .map(|t| div().t_small().semibold().text_color(k.ink).child(t)),
                    )
                    .children(preview.chart_note.clone().map(|n| {
                        div()
                            .t_meta()
                            .line_height(px(18.))
                            .text_color(k.ink_3)
                            .child(n)
                    })),
            )
            .child(
                div()
                    .mt(space::S12)
                    .flex()
                    .flex_col()
                    .gap(space::S8)
                    .children(preview.bars.iter().enumerate().map(|(i, b)| {
                        let (ink, weight) = if b.highlight {
                            (k.ink, true)
                        } else {
                            (k.ink_2, false)
                        };
                        let fill_color = if b.highlight { k.ink } else { k.ink_3 };
                        div()
                            .h(px(18.))
                            .flex()
                            .items_center()
                            .gap(space::S12)
                            .t_meta()
                            .line_height(px(18.))
                            .child(
                                div()
                                    .flex_none()
                                    .w(px(112.))
                                    .one_line()
                                    .text_color(ink)
                                    .when(weight, |el| el.medium())
                                    .child(b.label.clone()),
                            )
                            .child(div().flex_1().h(px(8.)).rounded(px(2.)).bg(k.sunken).child(
                                bar_width(
                                    ElementId::Name(format!("out-bar-{id}-{i}").into()),
                                    div().h_full().rounded(px(2.)).bg(fill_color),
                                    b.share.clamp(0., 1.),
                                    Duration::from_millis(120 + 40 * i as u64),
                                    cx,
                                ),
                            ))
                            .child(
                                div()
                                    .flex_none()
                                    .w(px(32.))
                                    .flex()
                                    .justify_end()
                                    .mono()
                                    .text_color(ink)
                                    .when(weight, |el| el.medium())
                                    .child(b.value.clone()),
                            )
                    }))
                    .when(!preview.axis.is_empty(), |el| {
                        el.child(
                            div()
                                .flex()
                                .gap(space::S12)
                                .t_mono()
                                .text_color(k.ink_3)
                                .child(div().flex_none().w(px(112.)))
                                .child(
                                    div()
                                        .flex_1()
                                        .flex()
                                        .justify_between()
                                        .children(preview.axis.iter().cloned()),
                                )
                                .child(div().flex_none().w(px(32.))),
                        )
                    }),
            )
    });
    div()
        .children(preview.eyebrow.clone().map(|e| {
            div()
                .t_mono()
                .line_height(px(18.))
                .text_color(k.ink_3)
                .child(e)
        }))
        .child(
            div()
                .mt(space::S4)
                .text_size(px(20.))
                .line_height(px(28.))
                .font_weight(gpui::FontWeight::SEMIBOLD)
                .text_color(k.ink)
                .child(preview.title.clone()),
        )
        .children(
            preview
                .summary
                .clone()
                .map(|s| div().mt(space::S6).t_small().text_color(k.ink_2).child(s)),
        )
        .children(metrics)
        .children(table)
        .children(chart)
        .into_any_element()
}

/// A borderless 24 pt symbol button of the pane header (NATIVE.md §5.2).
fn header_button(
    id: &'static str,
    sym: Sym,
    name: &'static str,
    cx: &App,
) -> gpui_component::button::Button {
    crate::ui::native::icon_button_sym(id, sym, name, crate::ui::native::IconSize::Regular, cx)
}

/// The 产出 pane for one 产出 (`version` or its latest): a 52 pt header in
/// the toolbar style (Title 3 title over a `secondary` subtitle, borderless
/// 在窗口打开 and close), then the page edge to edge on `content`.
pub fn render(services: &Services, id: &str, version: Option<u32>, cx: &App) -> Option<AnyElement> {
    let n = native_colors(cx);
    let p = &services.provider;
    let detail = p.artifact(id)?;
    let v = version.unwrap_or(detail.summary.version);
    let at = detail
        .versions
        .iter()
        .find(|x| x.version == v)
        .map(|x| x.created_at)
        .unwrap_or(detail.summary.updated_at);
    let preview = p.artifact_preview(id, v);
    let nav = services.nav.clone();
    let open_id = id.to_string();
    let header = div()
        .flex_none()
        .h(metrics::TOOLBAR_ROW)
        .pl(space::S16)
        .pr(space::S10)
        .flex()
        .items_center()
        .gap(space::S8)
        .child(
            div()
                .flex_1()
                .min_w_0()
                .flex()
                .flex_col()
                .child(
                    div()
                        .min_w_0()
                        .one_line()
                        .n_title3()
                        .text_color(n.label)
                        .child(detail.summary.title.clone()),
                )
                .child(
                    div()
                        .one_line()
                        .n_subheadline()
                        .n_figures()
                        .text_color(n.secondary)
                        .child(fill(
                            c::OUTPUT_VERSION,
                            &[("v", &v.to_string()), ("time", &clock(at))],
                        )),
                ),
        )
        .child(
            header_button("out-open-window", Sym::OpenExternal, c::OPEN_WINDOW, cx).on_click({
                let nav = nav.clone();
                let id = open_id.clone();
                move |_: &ClickEvent, _, cx| {
                    nav.update(cx, |nav, cx| nav.open_artifact(id.clone(), Some(v), cx));
                }
            }),
        )
        .child(
            header_button("out-close", Sym::Close, common::CLOSE, cx).on_click({
                let nav = nav.clone();
                move |_: &ClickEvent, _, cx| {
                    nav.update(cx, |nav, cx| nav.set_right_pane(RightPane::TopicRail, cx));
                }
            }),
        );
    let body: AnyElement = match preview {
        Some(preview) => div()
            .id("out-sheet")
            .size_full()
            .overflow_y_scroll()
            .pt(space::S8)
            .px(space::S16)
            .pb(px(28.))
            .child(page(&preview, id, cx))
            .into_any_element(),
        None => div()
            .size_full()
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .gap(space::S12)
            .child(div().n_body().text_color(n.secondary).child(c::PANE_EMPTY))
            .child(
                push_button(
                    "out-open-window-empty",
                    c::OPEN_WINDOW,
                    PushStyle::Outline,
                    PushSize::Regular,
                    cx,
                )
                .on_click(move |_: &ClickEvent, _, cx| {
                    nav.update(cx, |nav, cx| {
                        nav.open_artifact(open_id.clone(), Some(v), cx)
                    });
                }),
            )
            .into_any_element(),
    };
    Some(
        div()
            .size_full()
            .flex()
            .flex_col()
            .bg(n.content)
            .child(header)
            .child(div().flex_1().min_h(px(0.)).child(body))
            .into_any_element(),
    )
}
