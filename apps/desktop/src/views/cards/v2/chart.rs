//! The `chart` block: one line, area, bar or spark chart (DESIGN.md §7.10
//! "Chart"). The main series is `ink` (solid, with a dot on its last point),
//! the others `ink-3` dashed or `line-2` bars; three quiet grid lines; mono
//! axis labels under the points; a legend at the right of the label row.
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
    main: Hsla,
    other: Hsla,
    bar_other: Hsla,
    fill: Hsla,
    bg: Hsla,
    spark: bool,
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

/// Paints the chart at `t`, linear 0 → 1 over [`draw_time`].
fn paint(p: &Paint, b: Bounds<Pixels>, t: f32, window: &mut Window) {
    let n = p.series.iter().map(Vec::len).max().unwrap_or(0);
    if n == 0 {
        return;
    }
    let bars = p.kind == ChartKind::Bar;
    let r = range(&p.series, bars);
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
                let color = if si == 0 { p.main } else { p.bar_other };
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
        let mut line = if main {
            PathBuilder::stroke(px(if p.spark { 1.5 } else { 2. }))
        } else {
            PathBuilder::stroke(px(1.5)).dash_array(&[px(4.), px(3.)])
        };
        line.move_to(visible[0]);
        for q in &visible[1..] {
            line.line_to(*q);
        }
        if let Ok(path) = line.build() {
            window.paint_path(path, if main { p.main } else { p.other });
        }
        if main && !p.spark && st >= 0.999 {
            let last = pts[pts.len() - 1];
            let d = px(9.);
            window.paint_quad(quad(
                Bounds {
                    origin: point(last.x - d / 2., last.y - d / 2.),
                    size: gpui::size(d, d),
                },
                d / 2.,
                p.bg,
                px(2.),
                p.main,
                Default::default(),
            ));
        }
    }
}

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
    let k = theme::colors(cx);
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
        main: k.ink,
        other: k.ink_3,
        bar_other: k.line_2,
        fill: m::alpha(k.ink, 0.08),
        bg: k.bg,
        spark,
    });
    let height = if spark { px(28.) } else { m::CHART };
    let still = cx.reduce_motion();
    let run = draw_time(kind, series.len());
    let plot = div().w_full().h(height);
    let plot: AnyElement = if still {
        let p = p.clone();
        plot.child(
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
        ui::motion::once(id.clone(), run, plot, move |el, t| {
            let p = p.clone();
            el.child(
                canvas(|_, _, _| (), move |b, _, window, _| paint(&p, b, t, window)).size_full(),
            )
        })
    };
    if spark {
        return plot;
    }
    let legend = (series.len() > 1).then(|| {
        div()
            .flex()
            .items_center()
            .gap(space::S16)
            .children(series.iter().enumerate().map(|(i, s)| {
                let swatch = if i == 0 {
                    div().w(px(14.)).h(px(2.)).bg(k.ink).into_any_element()
                } else {
                    div()
                        .flex()
                        .gap(px(2.))
                        .children((0..3).map(|_| div().w(px(3.)).h(px(1.5)).bg(k.ink_2)))
                        .into_any_element()
                };
                div()
                    .flex()
                    .items_center()
                    .gap(space::S6)
                    .child(swatch)
                    .child(s.name.clone())
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
            meta(div())
                .flex()
                .items_center()
                .text_color(k.ink_2)
                .when_some(title, |el, t| {
                    el.child(
                        div()
                            .font_weight(theme::MEDIUM_WEIGHT)
                            .text_color(k.ink)
                            .child(t),
                    )
                })
                .when_some(legend, |el, l| el.child(div().ml_auto().child(l))),
        )
        .child(plot)
        .when(!x.is_empty(), |el| {
            el.child(
                meta(div())
                    .flex()
                    .text_color(k.ink_3)
                    .mono()
                    .children(x.iter().map(|l| {
                        div()
                            .flex_1()
                            .min_w(px(0.))
                            .flex()
                            .justify_center()
                            .one_line()
                            .child(l.clone())
                    })),
            )
        })
        .into_any_element()
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn lines_draw_in_650_ms_and_later_series_follow() {
        assert_eq!(draw_time(ChartKind::Line, 1).as_millis(), 650);
        assert_eq!(draw_time(ChartKind::Line, 3).as_millis(), 750);
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
