//! Data blocks with 2.1 events (INTERACT §2.3, §3.2): a table with row keys, selection and
//! a local sort; a chart whose points can be selected (`chart.select` / `chart.clear`,
//! R3 `part {kind: point|bar}`); a metric that can be pressed (`metric.press`).
//! Without events they draw exactly like 2.0.

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
    ui::{self, Lucide},
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
    let head = meta(div())
        .flex()
        .items_center()
        .gap(space::S12)
        .h(px(28.))
        .border_b_1()
        .border_color(k.line)
        .text_color(k.ink_3)
        .when(can_pick, |el| el.child(div().flex_none().w(px(16.))))
        .children(columns.iter().enumerate().map(|(ci, label)| {
            let sorted_here = ui.sort.filter(|(c, _)| *c == ci);
            let emit = Emitter::of(ctx);
            let (uie, block, col_label) = (ctx.ui.clone(), id.clone(), label.clone());
            div()
                .id(eid(node, format!("th-{id}-{ci}")))
                .flex_1()
                .when(ci == 0, |el| el.min_w(px(96.)))
                .min_w(px(0.))
                .flex()
                .items_center()
                .gap(space::S4)
                .when(numeric[ci], |el| el.justify_end())
                .when(sortable.unwrap_or(false), |el| {
                    el.cursor_pointer()
                        .hover(|s| s.text_color(k.ink))
                        .on_click(move |_, _, cx| {
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
                        if asc {
                            Lucide::ArrowUp
                        } else {
                            Lucide::ArrowDown
                        },
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
        div()
            .id(eid(node, format!("tr-{id}-{key}")))
            .flex()
            .items_center()
            .gap(space::S12)
            .h(m::TABLE_ROW)
            .mx(px(-6.))
            .px(px(6.))
            .rounded(px(6.))
            .when(pos < last, |el| el.border_b_1().border_color(k.line))
            .when(on, |el| el.bg(k.sunken))
            .text_color(k.ink)
            .when(can_pick, |el| {
                el.cursor_pointer()
                    .hover(|s| s.bg(k.hover))
                    .on_click(move |_, _, cx| {
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
                el.child(
                    div()
                        .flex_none()
                        .size(px(16.))
                        .when(multiple, |el| el.rounded(px(4.)))
                        .when(!multiple, |el| el.rounded_full())
                        .border_1()
                        .border_color(if on { k.ink } else { k.line_2 })
                        .when(on && multiple, |el| {
                            el.bg(k.ink)
                                .flex()
                                .items_center()
                                .justify_center()
                                .child(ui::icon(Lucide::Check, px(12.), k.bg))
                        })
                        .when(on && !multiple, |el| el.border(px(5.))),
                )
            })
            .children((0..columns.len()).map(|ci| {
                let text = r.get(ci).cloned().unwrap_or_default();
                div()
                    .flex_1()
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
                    )
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

pub fn chart(node: &Node, b: &CardV2Block, cx: &mut App) -> AnyElement {
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
    // The chosen and the hovered column get a quiet tint drawn under the series, so the
    // bar or point stays in sight; a clear hit layer on top takes the clicks.
    let hot = node.ui.block(id).hot;
    let n = xs.len();
    let tint = div().absolute().inset_0().flex().children((0..n).map(|i| {
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
                el.child(div().absolute().inset_0().flex().children((0..n).map(|i| {
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
                    div()
                        .id(eid(node, format!("pt-{block}-{i}")))
                        .flex_1()
                        .h_full()
                        .cursor_pointer()
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
                        })
                        .on_click(move |_, _, cx| {
                            if on {
                                // A second click clears (chart.clear when declared).
                                uie.update(cx, |u, cx| {
                                    u.block_mut(&block).point = None;
                                    cx.notify();
                                });
                                if clear {
                                    emit.send(&block, "chart.clear", json!({}), None, cx);
                                }
                                return;
                            }
                            uie.update(cx, |u, cx| {
                                u.block_mut(&block).point = Some((skey.clone(), i));
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
                        })
                })))
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
                .child(div().size(px(6.)).rounded_full().bg(k.ink))
                .child(t),
        );
    }
    if let Some(s) = summary {
        out = out.child(meta(div()).text_color(k.ink_3).child(s.clone()));
    }
    out.into_any_element()
}

pub fn metric(node: &Node, b: &CardV2Block, cx: &mut App) -> AnyElement {
    let ctx = node.ctx;
    let el = b20::metrics(&[b], cx);
    let Some(id) = b.id() else { return el };
    if !(super::interact::sends(ctx.card, b, "metric.press") && super::interact::may_emit(ctx, b)) {
        return el;
    }
    let k = theme::colors(cx);
    let emit = Emitter::of(ctx);
    let block = id.to_string();
    div()
        .id(eid(node, format!("metric-{id}")))
        .mx(px(-8.))
        .px(px(8.))
        .py(px(4.))
        .my(px(-4.))
        .rounded(theme::radius::MD)
        .cursor_pointer()
        .hover(|s| s.bg(k.hover))
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
