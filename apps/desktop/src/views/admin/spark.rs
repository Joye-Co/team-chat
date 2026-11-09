//! The KPI sparkline: one series as a line over a soft fill, in the foreground color (pure black
//! and white, COMPLETENESS.md §1.2). A bucket with no sample breaks the line; the y axis runs
//! from 0 to `max` so cards of percentages compare at a glance.

use gpui::{
    App, Bounds, Hsla, IntoElement, PathBuilder, Pixels, Styled as _, Window, canvas, point,
};

use crate::ui::native::{native_colors, tokens::monitor as t};

/// `values` oldest first; `max` is the top of the y axis (100 for percentages).
pub fn spark(values: Vec<Option<f64>>, max: f64, cx: &App) -> impl IntoElement {
    let k = native_colors(cx);
    let (line, fill) = (k.label, k.label.opacity(0.08));
    canvas(
        |_, _, _| (),
        move |b, _, window, _| paint(&values, max, b, line, fill, window),
    )
    .w_full()
    .h(t::SPARK_HEIGHT)
}

fn paint(
    values: &[Option<f64>],
    max: f64,
    b: Bounds<Pixels>,
    line: Hsla,
    fill: Hsla,
    window: &mut Window,
) {
    let n = values.len();
    if n < 2 {
        return;
    }
    let max = if max > 0. { max } else { 1. };
    let inset = t::SPARK_STROKE * 2.;
    let x = |i: usize| b.left() + b.size.width * (i as f32 / (n - 1) as f32);
    let y = |v: f64| {
        let r = (v / max).clamp(0., 1.) as f32;
        b.bottom() - inset - (b.size.height - inset * 2.) * r
    };
    // Runs of consecutive samples.
    let mut runs: Vec<Vec<gpui::Point<Pixels>>> = vec![];
    let mut cur = vec![];
    for (i, v) in values.iter().enumerate() {
        match v {
            Some(v) if v.is_finite() => cur.push(point(x(i), y(*v))),
            _ => {
                if !cur.is_empty() {
                    runs.push(std::mem::take(&mut cur));
                }
            }
        }
    }
    if !cur.is_empty() {
        runs.push(cur);
    }
    for pts in runs {
        if pts.len() == 1 {
            // A lone sample: a short tick, so it still shows.
            let p = pts[0];
            let mut s = PathBuilder::stroke(t::SPARK_STROKE);
            s.move_to(point(p.x - t::SPARK_STROKE, p.y));
            s.line_to(point(p.x + t::SPARK_STROKE, p.y));
            if let Ok(path) = s.build() {
                window.paint_path(path, line);
            }
            continue;
        }
        let mut area = PathBuilder::fill();
        area.move_to(point(pts[0].x, b.bottom()));
        for p in &pts {
            area.line_to(*p);
        }
        area.line_to(point(pts[pts.len() - 1].x, b.bottom()));
        area.close();
        if let Ok(path) = area.build() {
            window.paint_path(path, fill);
        }
        let mut s = PathBuilder::stroke(t::SPARK_STROKE);
        s.move_to(pts[0]);
        for p in &pts[1..] {
            s.line_to(*p);
        }
        if let Ok(path) = s.build() {
            window.paint_path(path, line);
        }
    }
}

/// The top of a rate's y axis: the largest value with headroom, at least 1 KiB/s.
pub fn rate_max(values: &[Option<f64>]) -> f64 {
    let m = values.iter().flatten().fold(0f64, |a, b| a.max(*b));
    (m * 1.25).max(1024.)
}
