//! Data blocks with 2.1 events (INTERACT §2.3, §3.2): a table with row keys, selection and
//! a local sort; a chart whose points can be selected (`chart.select` / `chart.clear`,
//! R3 `part {kind: point|bar}`); a metric that can be pressed (`metric.press`).
//! Without events they draw exactly like 2.0.

use crate::ui::native::FocusRow as _;
use gpui::{
    AnyElement, App, InteractiveElement as _, IntoElement, ParentElement,
    StatefulInteractiveElement as _, Styled, div, prelude::FluentBuilder as _, px,
};
use serde_json::json;

use super::{Node, eid, interact::Emitter};
use crate::{
    copy::{cards21 as c, fill},
    data::cards_v2::{CardV2Block, ChartKind, PartKey, PartRef, TableSelect},
    theme::{self, Type as _, card_v2 as m, card_v2::mix, card_v21 as t, space},
    ui::{self, Sym},
    views::cards::v2::{blocks as b20, meta},
};

/// Row keys of a table: `rowKeys`, else the 0-based row index (INC R4).
pub fn row_keys(rows: &[Vec<String>], keys: Option<&[String]>) -> Vec<String> {
    match keys {
        Some(k) if k.len() == rows.len() => k.to_vec(),
        _ => (0..rows.len()).map(|i| i.to_string()).collect(),
    }
}

/// Sorts row indices by column `col` (numbers as numbers, the rest as text).
pub fn sorted(rows: &[Vec<String>], col: usize, asc: bool) -> Vec<usize> {
    let num = |s: &str| {
        s.trim()
            .trim_end_matches('%')
            .replace([',', '+'], "")
            .replace('−', "-")
            .parse::<f64>()
            .ok()
    };
    let mut ix: Vec<usize> = (0..rows.len()).collect();
    ix.sort_by(|a, b| {
        let (x, y) = (
            rows[*a].get(col).map(String::as_str).unwrap_or(""),
            rows[*b].get(col).map(String::as_str).unwrap_or(""),
        );
        let o = match (num(x), num(y)) {
            (Some(p), Some(q)) => p.partial_cmp(&q).unwrap_or(std::cmp::Ordering::Equal),
            _ => x.cmp(y),
        };
        if asc { o } else { o.reverse() }
    });
    ix
}

pub fn table(node: &Node, b: &CardV2Block, window: &gpui::Window, cx: &mut App) -> AnyElement {
    let CardV2Block::Table {
        id,
        columns,
        rows,
        title,
        row_keys: keys,
        select,
        sortable,
        ..
    } = b
    else {
        return div().into_any_element();
    };
    let ctx = node.ctx;
    let k = theme::colors(cx);
    let plain = select.is_none() && !sortable.unwrap_or(false);
    if plain {
        // inc-integrate: cell, row and column parts for 引用提问 (REFS §A2.2, R4).
        let refs = super::parts::table_refs(node, b, cx);
        let t = b20::table_with_refs(columns, rows, refs.as_ref(), Some(window), cx);
        return match title {
            Some(tt) => div()
                .flex()
                .flex_col()
                .gap(space::S8)
                .child(super::question(tt, cx))
                .child(t)
                .into_any_element(),
            None => t,
        };
    }
    let keys = row_keys(rows, keys.as_deref());
    // 引用提问 on the cells of a table I pick rows in: ⌥-click and the context menu only
    // (REFS §A3: controls), so a plain click still picks the row.
    let cell_refs = super::parts::control_table_refs(node, b, cx);
    let ui = node.ui.block(id);
    let order = match ui.sort {
        Some((col, asc)) => sorted(rows, col, asc),
        None => (0..rows.len()).collect(),
    };
    let picked = ctx.form.read(cx).choice(id).to_vec();
    let can_pick = select.is_some()
        && (if ctx.card.ask.is_some() && ctx.card.surface_ids().contains(id.as_str()) {
            ctx.live()
        } else {
            super::interact::may_emit(ctx, b)
        });
    let multiple = *select == Some(TableSelect::Multiple);
    let numeric: Vec<bool> = (0..columns.len())
        .map(|ci| {
            ci > 0
                && rows
                    .iter()
                    .all(|r| r.get(ci).is_none_or(|v| v.is_empty() || b20::numeric(v)))
        })
        .collect();
    // Number columns take what their numbers need, right-aligned at the row end.
    let widths: Vec<Option<gpui::Pixels>> = numeric
        .iter()
        .enumerate()
        .map(|(ci, n)| n.then(|| b20::num_col_width(columns, rows, ci)))
        .collect();
    let sized = |el: gpui::Stateful<gpui::Div>, ci: usize| match widths[ci] {
        Some(w) => el.flex_none().w(w),
        None => el.flex_1(),
    };
    let head = meta(div())
        .flex()
        .items_center()
        .gap(space::S12)
        .h(crate::ui::native::tokens::height::SM)
        .border_b_1()
        .border_color(k.line)
        .text_color(k.ink_3)
        .when(can_pick, |el| el.child(div().flex_none().w(px(16.))))
        .children(columns.iter().enumerate().map(|(ci, label)| {
            let sorted_here = ui.sort.filter(|(c, _)| *c == ci);
            let emit = Emitter::of(ctx);
            let (uie, block, col_label) = (ctx.ui.clone(), id.clone(), label.clone());
            sized(div().id(eid(node, format!("th-{id}-{ci}"))), ci)
                .when(ci == 0, |el| el.min_w(px(96.)))
                .min_w(px(0.))
                .flex()
                .items_center()
                .gap(space::S4)
                .when(numeric[ci], |el| el.justify_end())
                .when(sortable.unwrap_or(false), |el| {
                    el.hover(|s| s.text_color(k.ink)).on_click(move |_, _, cx| {
                        let asc = !matches!(sorted_here, Some((_, true)));
                        uie.update(cx, |u, cx| {
                            u.block_mut(&block).sort = Some((ci, asc));
                            cx.notify();
                        });
                        // The bot gets `table.sort` only when it is declared.
                        emit.send(
                            &block,
                            "table.sort",
                            json!({ "column": col_label, "dir": if asc { "asc" } else { "desc" } }),
                            None,
                            cx,
                        );
                    })
                })
                .child(div().one_line().child(label.clone()))
                .when_some(sorted_here, |el, (_, asc)| {
                    el.child(ui::icon(
                        if asc { Sym::ArrowUp } else { Sym::ArrowDown },
                        px(12.),
                        k.ink_2,
                    ))
                })
        }));
    let last = order.len().saturating_sub(1);
    let body = order.iter().enumerate().map(|(pos, ri)| {
        let r = &rows[*ri];
        let key = keys[*ri].clone();
        let on = picked.contains(&key);
        let (form, block, target) = (
            ctx.form.clone(),
            id.clone(),
            super::leaves::PickTarget::of(node),
        );
        let mark_id = eid(node, format!("tr-{id}-{key}-mark"));
        div()
            .id(eid(node, format!("tr-{id}-{key}")))
            .flex()
            .items_center()
            .gap(space::S12)
            .h(m::TABLE_ROW)
            .mx(px(-6.))
            .px(px(6.))
            .rounded(crate::ui::native::tokens::radius::ITEM)
            .when(pos < last, |el| el.border_b_1().border_color(k.line))
            .when(on, |el| el.bg(k.sunken))
            .text_color(k.ink)
            .when(can_pick, |el| {
                el.focus_row(on, cx).on_click(move |_, _, cx| {
                    form.update(cx, |f, cx| {
                        let mut cur = f.choice(&block).to_vec();
                        if multiple {
                            if let Some(i) = cur.iter().position(|x| *x == key) {
                                cur.remove(i);
                            } else {
                                cur.push(key.clone());
                            }
                        } else {
                            cur = vec![key.clone()];
                        }
                        f.set_choice(&block, cur, cx);
                    });
                    target.after(&block, cx);
                })
            })
            .when(can_pick, |el| {
                // The native check box or radio button (NATIVE.md §5.4).
                let mark_id = mark_id.clone();
                el.child(if multiple {
                    crate::ui::native::NativeCheckbox::new(mark_id, on).into_any_element()
                } else {
                    crate::ui::native::NativeRadio::new(mark_id, on).into_any_element()
                })
            })
            .children((0..columns.len()).map(|ci| {
                let text = r.get(ci).cloned().unwrap_or_default();
                let cell = div()
                    .map(|el| match widths[ci] {
                        Some(w) => el.flex_none().w(w),
                        None => el.flex_1(),
                    })
                    .when(ci == 0, |el| el.min_w(px(96.)))
                    .min_w(px(0.))
                    .flex()
                    .when(numeric[ci], |el| el.justify_end())
                    .child(
                        div()
                            .one_line()
                            .when(numeric[ci], |el| el.mono())
                            .when(on && ci == 0, |el| el.font_weight(theme::MEDIUM_WEIGHT))
                            .child(text),
                    );
                // The source row index, so a sorted table references the same row.
                match &cell_refs {
                    Some(r) => r.cell(cell, *ri, ci, window, cx),
                    None => cell.into_any_element(),
                }
            }))
    });
    let mut col = b20::small(div()).flex().flex_col();
    if let Some(tt) = title {
        col = col.child(div().mb(space::S8).child(super::question(tt, cx)));
    }
    col = col.child(head).children(body);
    if can_pick && multiple && !picked.is_empty() {
        col = col.child(
            meta(div())
                .mt(space::S8)
                .text_color(k.ink_3)
                .child(fill(c::TABLE_SELECTED, &[("n", &picked.len().to_string())])),
        );
    }
    col.into_any_element()
}

pub fn chart(node: &Node, b: &CardV2Block, window: &gpui::Window, cx: &mut App) -> AnyElement {
    let CardV2Block::Chart {
        id,
        kind,
        label,
        unit,
        x,
        series,
        summary,
        ..
    } = b
    else {
        return div().into_any_element();
    };
    let ctx = node.ctx;
    let k = theme::colors(cx);
    // 引用提问: the points (bars) of the main series are parts (REFS §A2.2).
    let parts = b20::block_parts(ctx, b, cx);
    let xs = x.clone().unwrap_or_default();
    let drawn = super::super::v2::chart_el(
        ctx.id(format!("chart-{id}")),
        *kind,
        label.as_deref(),
        unit.as_deref(),
        &xs,
        series,
        cx,
    );
    let selectable = super::interact::sends(ctx.card, b, "chart.select")
        && super::interact::may_emit(ctx, b)
        && !xs.is_empty()
        && !series.is_empty()
        && *kind != ChartKind::Spark;
    // The chosen point: mine now, else the one my waiting event names (optimistic).
    let point = node.ui.block(id).point.clone().or_else(|| {
        let events = ctx
            .env
            .provider
            .card_v2_events(&ctx.msg.group_id, &ctx.msg.message_id);
        let now = ctx.env.now();
        events.iter().rev().find_map(|e| {
            (e.request.block_id.as_deref() == Some(id.as_str())
                && e.request.event == "chart.select"
                && super::interact::stage_of(e, now).is_some()
                && !super::interact::undone(e, now))
            .then(|| {
                let v = e.request.value.as_ref()?;
                Some((
                    v.get("series")?.as_str()?.to_string(),
                    v.get("index")?.as_u64()? as usize,
                ))
            })
            .flatten()
        })
    });
    // The update band already names a pick that caused this rev; the chart does not say it
    // twice.
    let from_cause = point.is_none();
    let point = point.or_else(|| {
        // My pick that caused this rev stays marked in it (Interact.dc.html 02 step 3), until
        // I clear it.
        let cause = ctx.msg.cause.as_ref()?;
        if node.ui.block(id).cleared
            || Some(cause.actor_id.as_str()) != ctx.me.as_deref()
            || cause.block_id.as_deref() != Some(id.as_str())
            || cause.event != "chart.select"
        {
            return None;
        }
        let events = ctx
            .env
            .provider
            .card_v2_events(&ctx.msg.group_id, &ctx.msg.message_id);
        let e = events.iter().rev().find(|e| {
            e.event_id() == Some(cause.event_id.as_str())
                || (e.request.block_id.as_deref() == Some(id.as_str())
                    && e.request.event == "chart.select")
        })?;
        let v = e.request.value.as_ref()?;
        Some((
            v.get("series")?.as_str()?.to_string(),
            v.get("index")?.as_u64()? as usize,
        ))
    });
    // A single-series bar chart the viewer can pick from reads as a list of bars, one row
    // per x (Interact.dc.html 02: 各渠道新增群，点一个看明细).
    if selectable && *kind == ChartKind::Bar && series.len() == 1 {
        let tip = point.is_some() && !from_cause;
        return bar_rows(node, b, point, tip, parts, window, cx);
    }
    let mut out = div()
        .relative()
        .flex()
        .flex_col()
        .gap(space::S6)
        .min_w(px(0.));
    let main = series.first();
    // The chosen point: its x and value, in the label row.
    let tip = point.as_ref().and_then(|(skey, i)| {
        let s = series.iter().find(|s| s.key() == skey)?;
        let v = s.values.get(*i)?.as_f64()?;
        let shown = match unit {
            Some(u) => format!("{} {u}", b20::fmt_number(v)),
            None => b20::fmt_number(v),
        };
        Some(fill(
            c::CHART_POINT,
            &[
                ("x", xs.get(*i).map(String::as_str).unwrap_or("")),
                ("value", &shown),
            ],
        ))
    });
    let overlay = parts.as_ref().filter(|_| !selectable).and_then(|p| {
        crate::views::refs::cards::chart_overlay(
            p,
            crate::views::refs::cards::chart_plot_top(b),
            m::CHART,
            window,
            cx,
        )
    });
    // The chosen and the hovered column get a quiet tint drawn under the series, so the
    // bar or point stays in sight; a clear hit layer on top takes the clicks.
    let hot = node.ui.block(id).hot;
    let n = xs.len();
    // The columns start where the plot does (after the y-axis labels).
    let left = super::super::v2::chart_plot_left(*kind, series);
    let tint = div()
        .absolute()
        .top_0()
        .bottom_0()
        .right_0()
        .left(left)
        .flex()
        .children((0..n).map(|i| {
            let on = point.as_ref().is_some_and(|(_, p)| *p == i);
            let fill = if on {
                Some(mix(k.ink, k.bg, t::CHART_PICK_FILL))
            } else if hot == Some(i) {
                Some(mix(k.ink, k.bg, t::CHART_HOVER_FILL))
            } else {
                None
            };
            div()
                .flex_1()
                .h_full()
                .rounded(theme::radius::SM)
                .when_some(fill.filter(|_| selectable), |el, f| el.bg(f))
        }));
    out = out.child(
        div()
            .relative()
            .child(tint)
            .child(drawn)
            .children(overlay)
            .when(selectable, |el| {
                let emit = Emitter::of(ctx);
                let uie = ctx.ui.clone();
                let block = id.clone();
                let skey = main.map(|s| s.key().to_string()).unwrap_or_default();
                let values: Vec<Option<f64>> = main
                    .map(|s| s.values.iter().map(|v| v.as_f64()).collect())
                    .unwrap_or_default();
                let bar = *kind == ChartKind::Bar;
                let clear = super::interact::sends(ctx.card, b, "chart.clear");
                el.child(
                    div()
                        .absolute()
                        .top_0()
                        .bottom_0()
                        .right_0()
                        .left(left)
                        .flex()
                        .children((0..n).map(|i| {
                            let on = point.as_ref().is_some_and(|(_, p)| *p == i);
                            let (emit, uie, block, skey, xv) = (
                                emit.clone(),
                                uie.clone(),
                                block.clone(),
                                skey.clone(),
                                xs[i].clone(),
                            );
                            let (uih, blockh) = (uie.clone(), block.clone());
                            let y = values.get(i).copied().flatten();
                            // The hover tint follows a layer inside the column: the column's own
                            // hover belongs to the 引用提问 affordance when the point is a part
                            // (an element takes one `on_hover`).
                            let hot = div()
                                .id(eid(node, format!("pt-hot-{block}-{i}")))
                                .absolute()
                                .inset_0()
                                .on_hover(move |over, _, cx| {
                                    uih.update(cx, |u, cx| {
                                        let b = u.block_mut(&blockh);
                                        let next = if *over {
                                            Some(i)
                                        } else if b.hot == Some(i) {
                                            None
                                        } else {
                                            b.hot
                                        };
                                        if b.hot != next {
                                            b.hot = next;
                                            cx.notify();
                                        }
                                    })
                                });
                            let col = div()
                                .id(eid(node, format!("pt-{block}-{i}")))
                                .relative()
                                .flex_1()
                                .h_full()
                                .child(hot)
                                .on_click(move |_, _, cx| {
                                    if on {
                                        // A second click clears (chart.clear when declared).
                                        uie.update(cx, |u, cx| {
                                            let b = u.block_mut(&block);
                                            b.point = None;
                                            b.cleared = true;
                                            cx.notify();
                                        });
                                        if clear {
                                            emit.send(&block, "chart.clear", json!({}), None, cx);
                                        }
                                        return;
                                    }
                                    uie.update(cx, |u, cx| {
                                        let b = u.block_mut(&block);
                                        b.point = Some((skey.clone(), i));
                                        b.cleared = false;
                                        cx.notify();
                                    });
                                    let part = PartRef {
                                        kind: if bar { "bar" } else { "point" }.into(),
                                        keys: Some(
                                            [
                                                ("series".to_string(), PartKey::Key(skey.clone())),
                                                ("x".to_string(), PartKey::Key(xv.clone())),
                                            ]
                                            .into(),
                                        ),
                                    };
                                    emit.send(
                                        &block,
                                        "chart.select",
                                        json!({ "series": skey, "x": xv, "y": y, "index": i }),
                                        Some(part),
                                        cx,
                                    );
                                });
                            // A point that answers is a control: ⌥-click and the menu reference
                            // it, a plain click still picks it.
                            match &parts {
                                Some(p) => crate::views::refs::affordance::part(
                                    col,
                                    p.spec(crate::views::refs::cards::Pick::Point {
                                        series: 0,
                                        x: i,
                                    })
                                    .control(true),
                                    window,
                                    cx,
                                ),
                                None => col.into_any_element(),
                            }
                        })),
                )
            }),
    );
    if let Some(t) = tip {
        out = out.child(
            meta(div())
                .flex()
                .items_center()
                .gap(space::S6)
                .text_color(k.ink)
                .font_weight(theme::MEDIUM_WEIGHT)
                .child(t),
        );
    }
    if let Some(s) = summary {
        out = out.child(meta(div()).text_color(k.ink_3).child(s.clone()));
    }
    out.into_any_element()
}

/// The bars of a pickable single-series bar chart as rows (Interact.dc.html 02): the x at
/// the left, the bar, the value at the right. Before a pick every bar is `ink-3`; after one
/// the chosen bar is `ink` with its name in 600 and the others fall back to `line-2`.
fn bar_rows(
    node: &Node,
    b: &CardV2Block,
    point: Option<(String, usize)>,
    tip: bool,
    parts: Option<crate::views::refs::cards::BlockParts>,
    window: &gpui::Window,
    cx: &mut App,
) -> AnyElement {
    let CardV2Block::Chart {
        id,
        label,
        unit,
        x,
        series,
        summary,
        ..
    } = b
    else {
        return div().into_any_element();
    };
    let ctx = node.ctx;
    let k = theme::colors(cx);
    let xs = x.clone().unwrap_or_default();
    let Some(main) = series.first() else {
        return div().into_any_element();
    };
    let values: Vec<Option<f64>> = main.values.iter().map(|v| v.as_f64()).collect();
    let max = values
        .iter()
        .flatten()
        .copied()
        .fold(0f64, f64::max)
        .max(f64::EPSILON);
    let picked = point.as_ref().map(|(_, i)| *i);
    let hot = node.ui.block(id).hot;
    let emit = Emitter::of(ctx);
    let clear = super::interact::sends(ctx.card, b, "chart.clear");
    let skey = main.key().to_string();
    let mut list = div()
        .flex()
        .flex_col()
        .gap(crate::ui::native::tokens::space::S2)
        .min_w(px(0.));
    for (i, xv) in xs.iter().enumerate() {
        let on = picked == Some(i);
        // Every value is primary on a primary 20% track (D12, §2.22); the picked
        // bar says so by its weight, never by fading the others under their track.
        let (name_ink, weight, bar) = match picked {
            Some(_) if on => (k.ink, gpui::FontWeight::SEMIBOLD, k.ink),
            _ => (k.ink_2, gpui::FontWeight::NORMAL, k.ink),
        };
        let y = values.get(i).copied().flatten();
        let shown = y.map(|v| match unit {
            Some(u) => format!("{} {u}", b20::fmt_number(v)),
            None => b20::fmt_number(v),
        });
        let (emit, uie, block, skey, xv2) = (
            emit.clone(),
            ctx.ui.clone(),
            id.clone(),
            skey.clone(),
            xv.clone(),
        );
        let (uih, blockh) = (ctx.ui.clone(), id.clone());
        let row = div()
            .id(eid(node, format!("bar-{id}-{i}")))
            .flex()
            .items_center()
            .gap(space::S10)
            .h(t::BAR_ROW)
            .px(space::S6)
            .mx(-space::S6)
            .rounded(t::BAR_ROW_RADIUS)
            .when(hot == Some(i), |el| el.bg(k.hover))
            .text_size(crate::ui::native::tokens::text::UI.0)
            .relative()
            .child(
                div()
                    .id(eid(node, format!("bar-hot-{id}-{i}")))
                    .absolute()
                    .inset_0()
                    .on_hover(move |over, _, cx| {
                        uih.update(cx, |u, cx| {
                            let b = u.block_mut(&blockh);
                            let next = if *over {
                                Some(i)
                            } else if b.hot == Some(i) {
                                None
                            } else {
                                b.hot
                            };
                            if b.hot != next {
                                b.hot = next;
                                cx.notify();
                            }
                        })
                    }),
            )
            .on_click(move |_, _, cx| {
                if on {
                    // A second click clears (chart.clear when declared).
                    uie.update(cx, |u, cx| {
                        let b = u.block_mut(&block);
                        b.point = None;
                        b.cleared = true;
                        cx.notify();
                    });
                    if clear {
                        emit.send(&block, "chart.clear", json!({}), None, cx);
                    }
                    return;
                }
                uie.update(cx, |u, cx| {
                    let b = u.block_mut(&block);
                    b.point = Some((skey.clone(), i));
                    b.cleared = false;
                    cx.notify();
                });
                let part = PartRef {
                    kind: "bar".into(),
                    keys: Some(
                        [
                            ("series".to_string(), PartKey::Key(skey.clone())),
                            ("x".to_string(), PartKey::Key(xv2.clone())),
                        ]
                        .into(),
                    ),
                };
                emit.send(
                    &block,
                    "chart.select",
                    json!({ "series": skey, "x": xv2, "y": y, "index": i }),
                    Some(part),
                    cx,
                );
            })
            .child(
                div()
                    .flex_none()
                    .w(t::BAR_NAME)
                    .one_line()
                    .text_color(name_ink)
                    .font_weight(weight)
                    .child(xv.clone()),
            )
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.))
                    .h(t::BAR_TRACK)
                    .rounded(t::BAR_TRACK_RADIUS)
                    .bg(theme::card_v2::alpha(k.ink, 0.2))
                    .overflow_hidden()
                    .child(
                        div()
                            .h_full()
                            .rounded(t::BAR_TRACK_RADIUS)
                            .bg(bar)
                            .w(gpui::relative((y.unwrap_or(0.) / max).clamp(0., 1.) as f32)),
                    ),
            )
            .child(
                div()
                    .flex_none()
                    .min_w(t::BAR_VALUE)
                    .flex()
                    .justify_end()
                    .mono()
                    .text_size(crate::ui::native::tokens::text::META.0)
                    .text_color(k.ink_2)
                    .children(shown),
            );
        // A bar that answers is a control: ⌥-click and the menu reference it, a plain
        // click still picks it.
        list = list.child(match &parts {
            Some(p) => crate::views::refs::affordance::part(
                row,
                p.spec(crate::views::refs::cards::Pick::Point { series: 0, x: i })
                    .control(true),
                window,
                cx,
            ),
            None => row.into_any_element(),
        });
    }
    div()
        .flex()
        .flex_col()
        .gap(space::S6)
        .min_w(px(0.))
        .when_some(label.clone(), |el, l| {
            el.child(
                meta(div())
                    .font_weight(theme::MEDIUM_WEIGHT)
                    .text_color(k.ink)
                    .child(l),
            )
        })
        .child(list)
        // The picked bar, as the column chart says it (`● B 站 · 9`).
        .when_some(
            picked.filter(|_| tip).and_then(|i| {
                let v = values.get(i).copied().flatten()?;
                let shown = match unit {
                    Some(u) => format!("{} {u}", b20::fmt_number(v)),
                    None => b20::fmt_number(v),
                };
                Some(fill(
                    c::CHART_POINT,
                    &[
                        ("x", xs.get(i).map(String::as_str).unwrap_or("")),
                        ("value", &shown),
                    ],
                ))
            }),
            |el, t| {
                el.child(
                    meta(div())
                        .flex()
                        .items_center()
                        .gap(space::S6)
                        .text_color(k.ink)
                        .font_weight(theme::MEDIUM_WEIGHT)
                        .child(t),
                )
            },
        )
        // The text alternative of the chart.
        .when_some(summary.clone(), |el, s| {
            el.child(meta(div()).text_color(k.ink_3).child(s))
        })
        .into_any_element()
}

pub fn metric(node: &Node, b: &CardV2Block, window: &gpui::Window, cx: &mut App) -> AnyElement {
    let ctx = node.ctx;
    let parts = crate::views::refs::cards::CardParts::of(ctx, cx);
    let el = b20::metrics_with(&[b], parts.as_ref(), Some(window), cx);
    let Some(id) = b.id() else { return el };
    if !(super::interact::sends(ctx.card, b, "metric.press") && super::interact::may_emit(ctx, b)) {
        return el;
    }
    let emit = Emitter::of(ctx);
    let block = id.to_string();
    div()
        .id(eid(node, format!("metric-{id}")))
        .mx(px(-8.))
        .px(px(8.))
        .py(px(4.))
        .my(px(-4.))
        .rounded(theme::radius::MD)
        .on_click(move |_, _, cx| {
            emit.send(
                &block,
                "metric.press",
                json!({}),
                Some(PartRef {
                    kind: "metric".into(),
                    keys: None,
                }),
                cx,
            );
        })
        .child(el)
        .into_any_element()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn row_keys_fall_back_to_the_index() {
        let rows = vec![vec!["a".to_string()], vec!["b".to_string()]];
        assert_eq!(row_keys(&rows, None), ["0", "1"]);
        let keys = vec!["xhs".to_string(), "bili".to_string()];
        assert_eq!(row_keys(&rows, Some(&keys)), ["xhs", "bili"]);
    }

    #[test]
    fn numbers_sort_as_numbers() {
        let rows: Vec<Vec<String>> = [["小红书", "14"], ["抖音", "5"], ["B 站", "9"]]
            .iter()
            .map(|r| r.iter().map(|s| s.to_string()).collect())
            .collect();
        assert_eq!(sorted(&rows, 1, true), [1, 2, 0]);
        assert_eq!(sorted(&rows, 1, false), [0, 2, 1]);
    }
}

#[cfg(test)]
mod chart_tests {
    use gpui::{
        Context, IntoElement, Modifiers, ParentElement as _, Render, Styled as _, Window, div,
        point, px,
    };

    use crate::{
        app::Services,
        data::ChatProvider as _,
        fake::v2::{ME, V2Samples},
        views::cards::{
            v2::{CardEnv, CardMode, render},
            v21::samples::REPORT,
        },
    };

    /// The 7.1 report as a 话题 draws it, where 引用提问 affordances show.
    struct Host {
        services: Services,
    }

    impl Render for Host {
        fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            let env = CardEnv {
                viewer: Some(ME.into()),
                ..CardEnv::from_services(&self.services)
            };
            let group = self
                .services
                .provider
                .card_v2("", REPORT)
                .map(|m| m.group_id)
                .unwrap_or_default();
            div()
                .w(px(720.))
                .children(render(&env, &group, REPORT, CardMode::Topic, window, cx))
        }
    }

    /// Regression (live check of PR B): a point of a chart I may select that is also a
    /// 引用提问 part took a second `on_hover` on one element, a debug assert that crashed the
    /// app on the first draw. The chart draws, and the pointer moves over its points.
    #[gpui::test]
    fn a_selectable_chart_with_part_refs_draws_and_takes_the_pointer(
        cx: &mut gpui::TestAppContext,
    ) {
        cx.update(gpui_component::init);
        let provider = V2Samples::new();
        assert!(provider.refs_supported(), "the parts show");
        let (_view, vcx) = cx.add_window_view(move |_, cx| Host {
            services: Services::new(provider, cx),
        });
        vcx.run_until_parked();
        for y in (200..700).step_by(25) {
            for x in (40..700).step_by(60) {
                vcx.simulate_mouse_move(point(px(x as f32), px(y as f32)), None, Modifiers::none());
            }
        }
        vcx.run_until_parked();
    }
}
