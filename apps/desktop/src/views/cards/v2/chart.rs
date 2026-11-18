//! The `chart` block: one line, area, bar or spark chart (DESIGN.md §7.10
//! "Chart"). The main series is `ink` (solid, with a dot on its last point),
//! each other series its own stroke of the mono palette ([`stroke`]); three quiet
//! grid lines; mono axis labels under the points, only as many as fit
//! ([`shown_labels`]); a legend under those labels, on the plot's left edge.
//! Lines draw in once ([`motion::LINE_DRAW`], each further series
//! [`motion::SERIES_STAGGER`] later) and bars grow from zero once
//! ([`motion::BAR_GROW`]), once per chart in the session; with Reduce motion
//! they are drawn whole.

use gpui::{
    AnyElement, App, Bounds, Hsla, IntoElement, ParentElement, PathBuilder, Pixels, SharedString,
    Styled, Window, canvas, div, point, prelude::FluentBuilder as _, px, quad,
};

use super::meta;
use crate::{
    data::cards_v2::{ChartKind, ChartSeries},
    theme::{self, Type as _, card_v2 as m, motion, space},
    ui,
};

/// Values of a series as floats.
fn values(s: &ChartSeries) -> Vec<f32> {
    s.values
        .iter()
        .map(|v| v.as_f64().unwrap_or(0.) as f32)
        .collect()
}

/// The value range with headroom, so lines do not touch the edges.
pub(crate) fn range(series: &[Vec<f32>], bars: bool) -> (f32, f32) {
    let all = series.iter().flatten().copied();
    let (lo, hi) = all.fold((f32::MAX, f32::MIN), |(lo, hi), v| (lo.min(v), hi.max(v)));
    if lo > hi {
        return (0., 1.);
    }
    if bars {
        return (lo.min(0.), if hi <= 0. { 1. } else { hi * 1.1 });
    }
    let span = (hi - lo).max(hi.abs() * 0.1).max(1e-6);
    (lo - span * 0.6, hi + span * 0.15)
}

/// Three round ticks (bottom, middle, top) that hold every value of a line or area chart
/// (Interact.dc.html 01: 2,000 / 2,200 / 2,400): the bottom tick and the step.
pub(crate) fn ticks(series: &[Vec<f32>]) -> Option<(f32, f32)> {
    let all = series.iter().flatten().copied().filter(|v| v.is_finite());
    let (mut lo, mut hi) = all.fold((f32::MAX, f32::MIN), |(lo, hi), v| (lo.min(v), hi.max(v)));
    if lo > hi {
        return None;
    }
    if (hi - lo).abs() < f32::EPSILON {
        let pad = (hi.abs() * 0.1).max(1.);
        lo -= pad;
        hi += pad;
    }
    for e in -4..=12 {
        for m in [1., 2., 2.5, 5.] {
            let step = m * 10f32.powi(e);
            let bottom = (lo / step).floor() * step;
            if bottom + 2. * step >= hi {
                return Some((bottom, step));
            }
        }
    }
    None
}

/// The value range a chart draws: the round ticks of a line or area chart, room for the
/// value labels above single-series bars.
fn scale(kind: ChartKind, series: &[Vec<f32>]) -> (f32, f32) {
    match kind {
        ChartKind::Bar if series.len() == 1 => {
            let (lo, hi) = range(series, true);
            (lo, lo + (hi - lo) / 1.1 * 1.3)
        }
        ChartKind::Bar => range(series, true),
        ChartKind::Spark => range(series, false),
        _ => ticks(series)
            .map(|(lo, step)| (lo, lo + 2. * step))
            .unwrap_or_else(|| range(series, false)),
    }
}

/// The y-axis labels of a chart: three for a line or area chart, none for bars and sparks.
pub(crate) fn y_labels(kind: ChartKind, series: &[ChartSeries]) -> Vec<String> {
    if matches!(kind, ChartKind::Bar | ChartKind::Spark) {
        return Vec::new();
    }
    let vals: Vec<Vec<f32>> = series.iter().map(values).collect();
    let Some((lo, step)) = ticks(&vals) else {
        return Vec::new();
    };
    (0..3)
        .rev()
        .map(|i| super::blocks::fmt_number(f64::from(lo + step * i as f32)))
        .collect()
}

/// Where the plot starts in a chart element: after the y-axis labels.
pub(crate) fn plot_left(kind: ChartKind, series: &[ChartSeries]) -> Pixels {
    let longest = y_labels(kind, series)
        .iter()
        .map(|l| l.chars().count())
        .max()
        .unwrap_or(0);
    if longest == 0 {
        px(0.)
    } else {
        px(longest as f32 * 7.5 + 10.)
    }
}

/// Where value `v` sits in the plot, from its top (0) to its bottom (1).
pub(crate) fn y_from_top(kind: ChartKind, series: &[ChartSeries], v: f64) -> f32 {
    let vals: Vec<Vec<f32>> = series.iter().map(values).collect();
    let (lo, hi) = scale(kind, &vals);
    1. - ((v as f32 - lo) / (hi - lo)).clamp(0., 1.)
}

/// x of point `i` of `n`: the center of its column.
fn px_at(b: &Bounds<Pixels>, i: usize, n: usize) -> Pixels {
    b.left() + b.size.width * ((i as f32 + 0.5) / n.max(1) as f32)
}

fn py_at(b: &Bounds<Pixels>, v: f32, (lo, hi): (f32, f32)) -> Pixels {
    let t = ((v - lo) / (hi - lo)).clamp(0., 1.);
    b.bottom() - b.size.height * t
}

struct Paint {
    kind: ChartKind,
    series: Vec<Vec<f32>>,
    grid: Hsla,
    /// The color of each stroke of [`stroke`], by [`Ink`].
    inks: [Hsla; 4],
    fill: Hsla,
    bg: Hsla,
    spark: bool,
    /// The point under the keyboard focus or the pointer ([`ChartPeek`]).
    peek: Option<usize>,
}

/// The inks a series draws in, all of the mono palette (COMPLETENESS: black and white).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Ink {
    Main = 0,
    Second = 1,
    Third = 2,
    Faint = 3,
}

/// How the line of a series is cut.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Dash {
    Solid,
    Dashed,
    Dotted,
}

/// What the legend of a chart with `series` series adds under its axis labels: one line
/// and the chart's gap. The columns of a chart to pick from stop above it.
pub(crate) fn legend_room(series: usize) -> Pixels {
    if series > 1 {
        m::META_LINE + space::S12
    } else {
        px(0.)
    }
}

/// The stroke of series `i` (a chart has at most five): the main series is solid `ink`;
/// the others differ in ink and in dash, so no two read the same in the plot or in the
/// legend.
pub(crate) fn stroke(i: usize) -> (Ink, Dash) {
    match i {
        0 => (Ink::Main, Dash::Solid),
        1 => (Ink::Second, Dash::Dashed),
        2 => (Ink::Third, Dash::Solid),
        3 => (Ink::Third, Dash::Dotted),
        _ => (Ink::Faint, Dash::Solid),
    }
}

/// The estimated width of an axis label: 12 pt mono, a CJK character one em.
pub(crate) fn label_width(label: &str) -> f32 {
    label
        .chars()
        .map(|c| if c.is_ascii() { 7.3 } else { 12. })
        .sum()
}

/// The x labels a plot `width` wide shows: every k-th, so each keeps its own width and
/// [`m::CHART_LABEL_GAP`]; always the first and the last. All of them when they fit.
pub(crate) fn shown_labels(labels: &[String], width: f32) -> Vec<usize> {
    let n = labels.len();
    if n == 0 || width <= 0. {
        return (0..n).collect();
    }
    let slot = width / n as f32;
    let need =
        labels.iter().map(|l| label_width(l)).fold(0f32, f32::max) + f32::from(m::CHART_LABEL_GAP);
    if need <= slot {
        return (0..n).collect();
    }
    // The end labels are pulled in to the plot's edges: their neighbours keep clear of that.
    let widest = labels.iter().map(|l| label_width(l)).fold(0f32, f32::max);
    let need = need + (widest / 2. - slot / 2.).max(0.);
    let k = (need / slot).ceil().max(1.) as usize;
    let mut out: Vec<usize> = (0..n).step_by(k).collect();
    if out.last() != Some(&(n - 1)) {
        // The last label always shows; the one before gives way when they would touch.
        if out.len() > 1
            && out
                .last()
                .is_some_and(|l| ((n - 1 - l) as f32) * slot < need)
        {
            out.pop();
        }
        out.push(n - 1);
    }
    out
}

/// The bars of a single-series bar chart that say their value: all of them while a bar's
/// column is at least [`m::CHART_BAR_LABEL`] wide, else only the highest and the last.
pub(crate) fn bar_labels(values: &[Option<f64>], width: f32) -> Vec<usize> {
    let n = values.len();
    if n == 0 {
        return Vec::new();
    }
    if width <= 0. || width / n as f32 >= f32::from(m::CHART_BAR_LABEL) {
        return (0..n).collect();
    }
    let max = values
        .iter()
        .enumerate()
        .filter_map(|(i, v)| v.map(|v| (i, v)))
        .max_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal))
        .map(|(i, _)| i);
    let mut out: Vec<usize> = max.into_iter().collect();
    if !out.contains(&(n - 1)) {
        out.push(n - 1);
    }
    out.sort_unstable();
    out
}

/// The plot widths of the charts drawn, by element id, from their last paint: the axis
/// labels that fit depend on it.
#[derive(Default)]
struct PlotWidths(std::collections::HashMap<SharedString, f32>);

impl gpui::Global for PlotWidths {}

/// Most widths kept; past it the map starts over.
const PLOT_WIDTHS_MAX: usize = 1024;

/// The plot width of chart `id`: measured at its last paint, else the in-thread card's.
fn plot_width(id: &SharedString, left: Pixels, cx: &App) -> f32 {
    cx.try_global::<PlotWidths>()
        .and_then(|w| w.0.get(id).copied())
        .unwrap_or_else(|| f32::from(m::MAX_WIDTH - space::S16 * 2. - left))
}

/// Measures the plot of chart `id`; a change of width redraws once.
fn measure_plot(id: SharedString) -> impl IntoElement {
    canvas(
        move |bounds, window, cx| {
            let w = f32::from(bounds.size.width);
            let widths = &mut cx.default_global::<PlotWidths>().0;
            if widths.get(&id).is_none_or(|old| (old - w).abs() > 1.) {
                if widths.len() >= PLOT_WIDTHS_MAX {
                    widths.clear();
                }
                widths.insert(id.clone(), w);
                window.refresh();
            }
        },
        |_, _, _, _| {},
    )
    .absolute()
    .inset_0()
}

/// How long a chart of `kind` with `series` series draws in.
fn draw_time(kind: ChartKind, series: usize) -> std::time::Duration {
    if kind == ChartKind::Bar {
        motion::BAR_GROW
    } else {
        motion::LINE_DRAW + motion::SERIES_STAGGER * series.saturating_sub(1) as u32
    }
}

/// The eased progress of series `si` at `t` (linear 0 → 1 over
/// [`draw_time`]): bars grow together, lines start one stagger apart.
fn series_progress(kind: ChartKind, series: usize, si: usize, t: f32) -> f32 {
    let total = draw_time(kind, series).as_secs_f32();
    let (delay, run) = if kind == ChartKind::Bar {
        (0., total)
    } else {
        (
            motion::SERIES_STAGGER.as_secs_f32() * si as f32,
            motion::LINE_DRAW.as_secs_f32(),
        )
    };
    motion::out_fn()(((t * total - delay) / run).clamp(0., 1.))
}

/// The hollow marker on a point of the main series.
fn point_marker(p: &Paint, at: gpui::Point<Pixels>, window: &mut Window) {
    let d = px(9.);
    window.paint_quad(quad(
        Bounds {
            origin: point(at.x - d / 2., at.y - d / 2.),
            size: gpui::size(d, d),
        },
        d / 2.,
        p.bg,
        px(2.),
        p.inks[Ink::Main as usize],
        Default::default(),
    ));
}

/// Paints the chart at `t`, linear 0 → 1 over [`draw_time`].
fn paint(p: &Paint, b: Bounds<Pixels>, t: f32, window: &mut Window) {
    let n = p.series.iter().map(Vec::len).max().unwrap_or(0);
    if n == 0 {
        return;
    }
    let bars = p.kind == ChartKind::Bar;
    let r = scale(p.kind, &p.series);
    if !p.spark {
        for i in 0..3 {
            let y = b.top() + b.size.height * (i as f32 / 2.);
            let mut path = PathBuilder::stroke(px(1.));
            path.move_to(point(b.left(), y));
            path.line_to(point(b.right(), y));
            if let Ok(path) = path.build() {
                window.paint_path(path, p.grid);
            }
        }
    }
    if bars {
        let groups = p.series.len().max(1);
        let col = b.size.width / n as f32;
        let bar_w = (col * 0.5 / groups as f32).min(px(18.));
        for (si, s) in p.series.iter().enumerate() {
            for (i, v) in s.iter().enumerate() {
                let x = px_at(&b, i, n) - bar_w * (groups as f32 / 2.) + bar_w * si as f32;
                let top = py_at(&b, *v, r);
                let h = (b.bottom() - top) * series_progress(p.kind, 1, 0, t);
                let rect = Bounds {
                    origin: point(x + px(1.), b.bottom() - h),
                    size: gpui::size(bar_w - px(2.), h),
                };
                let color = p.inks[stroke(si).0 as usize];
                window.paint_quad(quad(
                    rect,
                    px(2.),
                    color,
                    px(0.),
                    gpui::transparent_black(),
                    Default::default(),
                ));
            }
        }
        return;
    }
    // Lines draw in from the left.
    for (si, s) in p.series.iter().enumerate().rev() {
        if s.is_empty() {
            continue;
        }
        let pts: Vec<_> = s
            .iter()
            .enumerate()
            .map(|(i, v)| point(px_at(&b, i, n), py_at(&b, *v, r)))
            .collect();
        let st = series_progress(p.kind, p.series.len(), si, t);
        let shown = ((pts.len() - 1) as f32 * st).max(0.);
        let whole = shown.floor() as usize;
        let mut visible: Vec<_> = pts[..=whole.min(pts.len() - 1)].to_vec();
        if whole + 1 < pts.len() {
            let f = shown - whole as f32;
            let (a, c) = (pts[whole], pts[whole + 1]);
            visible.push(point(a.x + (c.x - a.x) * f, a.y + (c.y - a.y) * f));
        }
        let main = si == 0;
        if main && p.kind == ChartKind::Area && visible.len() > 1 {
            let mut area = PathBuilder::fill();
            area.move_to(point(visible[0].x, b.bottom()));
            for q in &visible {
                area.line_to(*q);
            }
            area.line_to(point(visible[visible.len() - 1].x, b.bottom()));
            area.close();
            if let Ok(path) = area.build() {
                window.paint_path(path, p.fill);
            }
        }
        let (ink, dash) = stroke(si);
        let mut line = match dash {
            Dash::Solid if main => PathBuilder::stroke(px(if p.spark { 1.5 } else { 2. })),
            Dash::Solid => PathBuilder::stroke(px(1.5)),
            Dash::Dashed => PathBuilder::stroke(px(1.5)).dash_array(&[px(4.), px(3.)]),
            Dash::Dotted => PathBuilder::stroke(px(1.5)).dash_array(&[px(1.5), px(3.)]),
        };
        line.move_to(visible[0]);
        for q in &visible[1..] {
            line.line_to(*q);
        }
        if let Ok(path) = line.build() {
            window.paint_path(path, p.inks[ink as usize]);
        }
        if main && !p.spark && st >= 0.999 {
            point_marker(p, pts[pts.len() - 1], window);
            if let Some(at) = p.peek.and_then(|i| pts.get(i)) {
                point_marker(p, *at, window);
            }
        }
    }
}

/// The point of a chart a person can pick from that is under the keyboard focus or the
/// pointer: its index and what it says, in the label row. The row is kept while there is
/// none, so the plot stays where it is.
pub(crate) struct ChartPeek(pub Option<(usize, String)>);

/// A chart block.
pub(crate) fn chart(
    id: SharedString,
    kind: ChartKind,
    label: Option<&str>,
    unit: Option<&str>,
    x: &[String],
    series: &[ChartSeries],
    cx: &App,
) -> AnyElement {
    chart_with_peek(id, kind, label, unit, x, series, None, cx)
}

/// [`chart`] of a chart whose points can be picked.
#[allow(clippy::too_many_arguments)]
pub(crate) fn chart_with_peek(
    id: SharedString,
    kind: ChartKind,
    label: Option<&str>,
    unit: Option<&str>,
    x: &[String],
    series: &[ChartSeries],
    peek: Option<ChartPeek>,
    cx: &App,
) -> AnyElement {
    let k = theme::colors(cx);
    let pickable = peek.is_some();
    let (peek_at, peek_text) = peek.and_then(|p| p.0).unzip();
    let spark = kind == ChartKind::Spark;
    let kind = if kind == ChartKind::Unknown {
        ChartKind::Line
    } else {
        kind
    };
    let p = std::rc::Rc::new(Paint {
        kind,
        series: series.iter().map(values).collect(),
        grid: k.line,
        inks: [k.ink, k.ink_2, k.ink_3, k.line_2],
        fill: m::alpha(k.ink, 0.08),
        bg: k.bg,
        spark,
        peek: peek_at,
    });
    let height = if spark { px(28.) } else { m::CHART };
    let still = cx.reduce_motion();
    let run = draw_time(kind, series.len());
    let canvas_box = div().size_full();
    let drawn: AnyElement = if still {
        let p = p.clone();
        canvas_box
            .child(
                canvas(
                    |_, _, _| (),
                    move |b, _, window, _| paint(&p, b, 1., window),
                )
                .size_full(),
            )
            .into_any_element()
    } else {
        // Drawn once per chart: a remount (scroll back, a return to the
        // page) shows it whole.
        ui::motion::once(id.clone(), run, canvas_box, move |el, t| {
            let p = p.clone();
            el.child(
                canvas(|_, _, _| (), move |b, _, window, _| paint(&p, b, t, window)).size_full(),
            )
        })
    };
    // The y-axis labels at the left of the three grid lines (line and area), and the
    // value above each bar of a single-series bar chart.
    let ys = if spark {
        Vec::new()
    } else {
        y_labels(kind, series)
    };
    let left = if spark {
        px(0.)
    } else {
        plot_left(kind, series)
    };
    let label_h = px(18.);
    let width = plot_width(&id, left, cx);
    let axis = (!ys.is_empty()).then(|| {
        div()
            .relative()
            .flex_none()
            .w(left)
            .h_full()
            .children(ys.iter().enumerate().map(|(i, l)| {
                meta(div())
                    .absolute()
                    .left_0()
                    .top(height * (i as f32 / 2.) - label_h / 2.)
                    .mono()
                    .text_color(k.ink_3)
                    .child(l.clone())
            }))
    });
    let bar_values = (kind == ChartKind::Bar && series.len() == 1 && !spark).then(|| {
        let s = &series[0];
        let values: Vec<Option<f64>> = s.values.iter().map(|v| v.as_f64()).collect();
        // Narrow bars say only the highest and the last value: a label over every one of
        // 200 bars is a smear.
        let labelled = bar_labels(&values, width);
        div()
            .absolute()
            .inset_0()
            .flex()
            .children(s.values.iter().enumerate().map(move |(i, v)| {
                let col = div().relative().flex_1().h_full();
                let Some(n) = v.as_f64().filter(|_| labelled.contains(&i)) else {
                    return col;
                };
                let top = height * y_from_top(kind, series, n) - label_h - px(2.);
                col.child(
                    meta(div())
                        .absolute()
                        .left_0()
                        .right_0()
                        .top(top)
                        .flex()
                        .justify_center()
                        .mono()
                        .whitespace_nowrap()
                        .text_color(k.ink_2)
                        .child(super::blocks::fmt_number(n)),
                )
            }))
    });
    let plot: AnyElement = div()
        .w_full()
        .h(height)
        .flex()
        .children(axis)
        .child(
            div()
                .relative()
                .flex_1()
                .min_w(px(0.))
                .h_full()
                .child(drawn)
                .children(bar_values)
                .when(!spark, |el| el.child(measure_plot(id.clone()))),
        )
        .into_any_element();
    if spark {
        return plot;
    }
    let inks = [k.ink, k.ink_2, k.ink_3, k.line_2];
    let legend = (series.len() > 1).then(|| {
        div()
            .flex()
            .flex_wrap()
            .items_center()
            .gap_x(space::S16)
            .gap_y(crate::ui::native::tokens::space::S4)
            .min_w(px(0.))
            .children(series.iter().enumerate().map(|(i, s)| {
                // The swatch is the series' own stroke: its ink and its dash. Bars have
                // no dash, so their swatch is the bar's fill.
                let (ink, dash) = stroke(i);
                let color = inks[ink as usize];
                let (pieces, piece, h) = match dash {
                    _ if kind == ChartKind::Bar => (1, m::CHART_SWATCH / 2., px(8.)),
                    Dash::Solid => (1, m::CHART_SWATCH, px(2.)),
                    Dash::Dashed => (3, px(4.), px(1.5)),
                    Dash::Dotted => (4, px(1.5), px(1.5)),
                };
                let swatch = div()
                    .flex_none()
                    .w(m::CHART_SWATCH)
                    .flex()
                    .items_center()
                    .justify_between()
                    .children((0..pieces).map(|_| {
                        div()
                            .w(piece)
                            .h(h)
                            .rounded(crate::ui::native::tokens::radius::CHECKBOX / 2.)
                            .bg(color)
                    }));
                div()
                    .flex_none()
                    .max_w_full()
                    .flex()
                    .items_center()
                    .gap(space::S6)
                    .child(swatch)
                    .child(crate::ui::truncate::TruncText::new(s.name.clone()))
            }))
    });
    let title = match (label, unit) {
        (Some(l), Some(u)) => Some(format!("{l}（{u}）")),
        (Some(l), None) => Some(l.to_string()),
        _ => None,
    };
    div()
        .flex()
        .flex_col()
        .gap(space::S12)
        .child(
            // The title cuts with "…". The row stays with no title on a chart with a
            // legend: a focused point's tooltip sits here, over neither line nor legend.
            meta(div())
                .flex()
                .items_center()
                .when(legend.is_some() || pickable, |el| el.min_h(m::META_LINE))
                .text_color(k.ink_2)
                .when_some(title, |el, t| {
                    el.child(
                        div().flex_1().min_w(m::TABLE_COL_FIRST * 2.).child(
                            crate::ui::truncate::TruncText::new(t)
                                .font_weight(theme::MEDIUM_WEIGHT)
                                .text_color(k.ink),
                        ),
                    )
                })
                .when_some(peek_text, |el, t| {
                    el.child(
                        div()
                            .flex_none()
                            .ml_auto()
                            .pl(space::S8)
                            .whitespace_nowrap()
                            .font_weight(theme::MEDIUM_WEIGHT)
                            .text_color(k.ink)
                            .child(t),
                    )
                }),
        )
        .child(plot)
        .when(!x.is_empty(), |el| {
            // Only the labels that fit, each whole and centered under its point; the first
            // and the last stay inside the plot's edges.
            let n = x.len() as f32;
            let last = x.len() - 1;
            let slot = width / n;
            el.child(meta(div()).pl(left).text_color(k.ink_3).mono().child(
                div().relative().w_full().h(label_h).children(
                    shown_labels(x, width).into_iter().map(|i| {
                        let wide = label_width(&x[i]) > slot;
                        div()
                            .absolute()
                            .top_0()
                            .flex()
                            .map(|el| match i {
                                0 if wide => el.left_0().justify_start(),
                                i if i == last && wide => el.right_0().justify_end(),
                                i => el
                                    .left(gpui::relative((i as f32 + 0.5) / n))
                                    .w(px(0.))
                                    .justify_center(),
                            })
                            .child(div().flex_none().whitespace_nowrap().child(x[i].clone()))
                    }),
                ),
            ))
        })
        // Under the labels, on the plot's left edge.
        .when_some(legend, |el, l| {
            el.child(meta(div()).pl(left).text_color(k.ink_2).child(l))
        })
        .into_any_element()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn x_labels_thin_to_what_fits_and_keep_both_ends() {
        let days: Vec<String> = (1..=30).map(|d| format!("10-{d:02}")).collect();
        // 30 dates in 600 pt: 20 pt each, a date needs 36.5 + 8.
        let shown = shown_labels(&days, 600.);
        assert_eq!(shown.first(), Some(&0));
        assert_eq!(shown.last(), Some(&29));
        assert!(shown.len() < 30 && shown.len() >= 8, "{shown:?}");
        let slot = 600. / 30.;
        let need = label_width("10-01") + f32::from(m::CHART_LABEL_GAP);
        for w in shown.windows(2) {
            assert!(
                (w[1] - w[0]) as f32 * slot >= need,
                "labels never touch: {shown:?}"
            );
        }
        // 200 points: still the first and the last, and room between all of them.
        let many: Vec<String> = (1..=200).map(|i| format!("#{i}")).collect();
        let shown = shown_labels(&many, 600.);
        assert_eq!((shown[0], *shown.last().unwrap()), (0, 199));
        assert!(shown.len() <= 20, "{shown:?}");
        // Four labels fit whole.
        let four: Vec<String> = ["10-06", "10-13", "10-20", "10-27"]
            .map(String::from)
            .to_vec();
        assert_eq!(shown_labels(&four, 600.), [0, 1, 2, 3]);
        assert!(shown_labels(&[], 600.).is_empty());
        // One CJK character is one em.
        assert!(label_width("周一") > label_width("10"));
    }

    #[test]
    fn narrow_bars_say_only_the_highest_and_the_last_value() {
        let week: Vec<Option<f64>> = [4., 7., 5., 9., 6., 3., 4.].map(Some).to_vec();
        assert_eq!(bar_labels(&week, 600.), [0, 1, 2, 3, 4, 5, 6]);
        let many: Vec<Option<f64>> = (0..200).map(|i| Some(((i * 71) % 180) as f64)).collect();
        let shown = bar_labels(&many, 600.);
        assert_eq!(shown.len(), 2, "{shown:?}");
        assert_eq!(shown[1], 199);
        assert!(bar_labels(&[], 600.).is_empty());
        assert_eq!(bar_labels(&[None, Some(1.)], 10.), [1]);
    }

    #[test]
    fn the_five_series_of_a_chart_all_draw_differently() {
        let strokes: std::collections::HashSet<(u8, u8)> = (0..5)
            .map(|i| {
                let (ink, dash) = stroke(i);
                (ink as u8, dash as u8)
            })
            .collect();
        assert_eq!(strokes.len(), 5);
        assert_eq!(stroke(0), (Ink::Main, Dash::Solid));
    }

    #[test]
    fn ranges_leave_headroom_and_bars_start_at_zero() {
        let (lo, hi) = range(&[vec![10., 20.]], false);
        assert!(lo < 10. && hi > 20.);
        assert_eq!(range(&[vec![3., 5.]], true).0, 0.);
        assert_eq!(range(&[], false), (0., 1.));
        let (lo, hi) = range(&[vec![7., 7.]], false);
        assert!(lo < 7. && hi > 7., "a flat series still has a span");
    }

    #[test]
    fn line_charts_get_three_round_ticks_that_hold_every_value() {
        // Interact.dc.html 01: 10 月 and 9 月 between 2,150 and 2,390 → 2,000 / 2,200 / 2,400.
        let vals = vec![
            vec![2210., 2268., 2301., 2390.],
            vec![2150., 2190., 2236., 2250.],
        ];
        assert_eq!(ticks(&vals), Some((2000., 200.)));
        let (lo, step) = ticks(&[vec![0.12, 0.31, 0.2]]).unwrap();
        assert!(lo <= 0.12 && lo + 2. * step >= 0.31);
        assert!(
            ticks(&[vec![5., 5.]]).is_some(),
            "a flat series still has a scale"
        );
        assert_eq!(ticks(&[]), None);
        let series = vec![ChartSeries {
            id: None,
            name: "10 月".into(),
            values: vals[0]
                .iter()
                .filter_map(|v| serde_json::Number::from_f64(f64::from(*v)))
                .collect(),
        }];
        // The 10 月 series alone (2,210 to 2,390) fits a tighter scale.
        assert_eq!(
            y_labels(ChartKind::Line, &series),
            ["2,400", "2,300", "2,200"]
        );
        assert!(
            y_labels(ChartKind::Bar, &series).is_empty(),
            "bars say their values"
        );
        assert!(plot_left(ChartKind::Line, &series) > px(0.));
    }

    #[test]
    fn lines_draw_in_line_draw_and_later_series_follow() {
        assert_eq!(draw_time(ChartKind::Line, 1), motion::LINE_DRAW);
        assert_eq!(
            draw_time(ChartKind::Line, 3),
            motion::LINE_DRAW + motion::SERIES_STAGGER * 2
        );
        assert_eq!(draw_time(ChartKind::Bar, 3), motion::BAR_GROW);
        // At the end every series is whole; early on the second lags.
        assert!((series_progress(ChartKind::Line, 2, 1, 1.) - 1.).abs() < 1e-4);
        assert!(
            series_progress(ChartKind::Line, 2, 1, 0.05)
                < series_progress(ChartKind::Line, 2, 0, 0.05)
        );
        assert_eq!(series_progress(ChartKind::Line, 2, 1, 0.), 0.);
    }
}
