//! 概览: the status strip, four KPI cards with sparklines, 容量 and 要注意 side by side, and the
//! 按成员 table with click-to-sort headers (the approved design's Main board).

use gpui::{
    AnyElement, App, Context, Entity, Hsla, InteractiveElement as _, IntoElement,
    ParentElement as _, SharedString, StatefulInteractiveElement as _, Styled, Window, div,
    prelude::FluentBuilder as _, relative,
};

use chat_core::proto::MachineMonitor;

use super::{
    MonitorView, dot,
    model::{self, AttentionKind, Range, Sort, SortKey, Tone},
    spark::{rate_max, spark},
};
use crate::{
    copy::{fill, monitor as m},
    ui::{
        MemberAvatar, Sym,
        icons::sym as sym_icon,
        native::{
            NativeColors, NativeType as _, TruncText, card_surface, native_colors, skeleton,
            tokens::{avatar, height, monitor as t, radius, space, text},
        },
        sf_symbols::SymSize,
    },
};

fn tone_color(tone: Tone, k: &NativeColors) -> Hsla {
    match tone {
        Tone::Ok => k.green,
        Tone::Warn => k.amber,
        Tone::Bad => k.red,
        Tone::Quiet => k.secondary,
    }
}

/// A card surface with the design's inner padding.
fn panel(cx: &App) -> gpui::Div {
    card_surface(cx).flex().flex_col()
}

#[allow(clippy::too_many_arguments)]
pub fn page(
    d: &MachineMonitor,
    range: Range,
    sort: Sort,
    now: u64,
    name: &dyn Fn(&str) -> String,
    member: &dyn Fn(&str) -> Option<crate::data::Member>,
    time: &dyn Fn(u64) -> String,
    view: Entity<MonitorView>,
    window: &mut Window,
    cx: &mut Context<MonitorView>,
) -> impl IntoElement {
    let _ = window;
    div()
        .w_full()
        .flex()
        .flex_col()
        .gap(space::S16)
        .child(strip(d, now, cx))
        .child(kpis(d, range, cx))
        .child(
            div()
                .w_full()
                .flex()
                .gap(space::S16)
                .child(div().flex_1().min_w(gpui::Pixels::ZERO).flex().child(capacity(d, now, cx)))
                .child(
                    div()
                        .w(relative(t::ATTENTION_SHARE))
                        .flex_none()
                        .flex()
                        .child(attention(d, now, name, time, cx)),
                ),
        )
        .child(members(d, sort, name, member, view, cx))
}

// ---------------------------------------------------------------------------
// Status strip

fn strip(d: &MachineMonitor, now: u64, cx: &App) -> impl IntoElement {
    let k = native_colors(cx);
    let cells = model::strip(d, now);
    let n = cells.len();
    panel(cx).flex_row().overflow_hidden().children(cells.into_iter().enumerate().map(|(i, c)| {
        div()
            .flex_1()
            .min_w(gpui::Pixels::ZERO)
            .px(space::S16)
            .py(space::S12)
            .flex()
            .flex_col()
            .gap(space::S4)
            .when(i > 0, |d| d.border_l_1().border_color(k.separator))
            .when(i + 1 == n, |d| d)
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(space::S6)
                    .n_footnote()
                    .text_color(k.secondary)
                    .child(sym_icon(
                        if c.cli { Sym::Terminal } else { Sym::Activity },
                        SymSize::INLINE_SMALL,
                        k.secondary,
                    ))
                    .child(TruncText::new(c.label)),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(space::S8)
                    .n_headline()
                    .child(dot(tone_color(c.tone, &k)))
                    .child(TruncText::new(c.value)),
            )
    }))
}

// ---------------------------------------------------------------------------
// KPI cards

fn kpis(d: &MachineMonitor, range: Range, cx: &App) -> impl IntoElement {
    let h = d.host.as_ref();
    let pct = |x: Option<f64>| x.map(|v| format!("{}", v.round() as i64));
    let range_label = range.label();
    let mem_pct = h.and_then(|h| {
        let total = h.mem_total_bytes?;
        let avail = h.mem_available_bytes?;
        Some(model::share(total.saturating_sub(avail), total) as f64 * 100.)
    });
    let disk_pct = h
        .filter(|h| h.disk_total_bytes > 0)
        .map(|h| model::share(h.disk_total_bytes - h.disk_free_bytes, h.disk_total_bytes) as f64 * 100.);
    let (net_v, net_u) = match h.and_then(|h| h.net_rx_bps) {
        Some(r) => {
            let (v, u) = model::rate_parts(r);
            (Some(v), u)
        }
        None => (None, String::new()),
    };
    let cards = [
        Kpi {
            id: "kpi-cpu",
            icon: Sym::Cpu,
            label: m::KPI_CPU,
            side: h
                .and_then(|h| h.cpu_threads)
                .map(|n| fill(m::THREADS, &[("n", &n.to_string())])),
            value: pct(h.and_then(|h| h.cpu_pct)),
            unit: "%".into(),
            foot: h.map(|h| fill(m::LOAD, &[("n", &format!("{:.1}", h.load1))])),
            series: d.series.cpu_pct.clone(),
            max: 100.,
        },
        Kpi {
            id: "kpi-mem",
            icon: Sym::Memory,
            label: m::KPI_MEM,
            side: h.and_then(|h| h.mem_total_bytes).map(model::bytes),
            value: pct(mem_pct),
            unit: "%".into(),
            foot: h
                .and_then(|h| h.mem_available_bytes)
                .map(|a| fill(m::AVAILABLE, &[("size", &model::bytes(a))])),
            series: d.series.mem_pct.clone(),
            max: 100.,
        },
        Kpi {
            id: "kpi-disk",
            icon: Sym::Disk,
            label: m::KPI_DISK,
            side: h.map(|h| model::bytes(h.disk_total_bytes)),
            value: pct(disk_pct),
            unit: "%".into(),
            foot: h.map(|h| fill(m::AVAILABLE, &[("size", &model::bytes(h.disk_free_bytes))])),
            series: d.series.disk_pct.clone(),
            max: 100.,
        },
        Kpi {
            id: "kpi-net",
            icon: Sym::Network,
            label: m::KPI_NET,
            side: h
                .and_then(|h| h.net_tx_bps)
                .map(|r| fill(m::UPLOAD, &[("rate", &model::rate(r))])),
            value: net_v,
            unit: net_u,
            foot: h.and_then(|h| h.net_rx_bps).map(|_| m::DOWNLOAD.to_string()),
            max: rate_max(&d.series.net_rx_bps),
            series: d.series.net_rx_bps.clone(),
        },
    ];
    div()
        .w_full()
        .flex()
        .gap(space::S16)
        .children(cards.into_iter().map(|c| kpi(c, range_label, cx)))
}

struct Kpi {
    id: &'static str,
    icon: Sym,
    label: &'static str,
    side: Option<String>,
    value: Option<String>,
    unit: String,
    foot: Option<String>,
    series: Vec<Option<f64>>,
    max: f64,
}

fn kpi(c: Kpi, range: &str, cx: &App) -> impl IntoElement {
    let k = native_colors(cx);
    let has_value = c.value.is_some();
    let aria = fill(m::SPARK_LABEL, &[("name", c.label), ("range", range)]);
    panel(cx)
        .id(c.id)
        .flex_1()
        .min_w(gpui::Pixels::ZERO)
        .pt(space::S16)
        .overflow_hidden()
        .child(
            div()
                .px(space::S16)
                .flex()
                .items_center()
                .gap(space::S8)
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(space::S6)
                        .n_headline()
                        .text_color(k.secondary)
                        .child(sym_icon(c.icon, SymSize::INLINE, k.secondary))
                        .child(c.label),
                )
                .child(div().flex_1())
                .children(c.side.map(|s| {
                    TruncText::new(s).n_footnote().n_figures().text_color(k.secondary)
                })),
        )
        .child(
            div()
                .px(space::S16)
                .mt(space::S4)
                .flex()
                .items_baseline()
                .gap(space::S2)
                .child(
                    div()
                        .n_style(text::KPI)
                        .font_weight(gpui::FontWeight::SEMIBOLD)
                        .n_figures()
                        .text_color(if has_value { k.label } else { k.secondary })
                        .child(c.value.unwrap_or_else(|| "—".into())),
                )
                .when(has_value, |d| {
                    d.child(div().n_headline().text_color(k.secondary).child(c.unit))
                })
                .child(div().flex_1())
                .child(
                    TruncText::new(c.foot.unwrap_or_else(|| {
                        if has_value { String::new() } else { m::KPI_NO_DATA.into() }
                    }))
                    .n_footnote()
                    .n_figures()
                    .text_color(k.secondary),
                ),
        )
        .child(
            div()
                .id(SharedString::from(format!("{}-spark", c.id)))
                .mt(space::S8)
                .tooltip(move |window, cx| {
                    gpui_component::tooltip::Tooltip::new(aria.clone()).build(window, cx)
                })
                .child(spark(c.series, c.max, cx)),
        )
}

// ---------------------------------------------------------------------------
// 容量

fn capacity(d: &MachineMonitor, now: u64, cx: &App) -> impl IntoElement {
    let k = native_colors(cx);
    let cap = &d.capacity;
    let split = model::memory_split(d);
    let bar = split.map(|(vm, host, free, ..)| {
        div()
            .w_full()
            .h(t::CAPACITY_BAR)
            .flex()
            .gap(space::S2)
            .child(seg(vm, k.label, cx))
            .child(seg(host, k.secondary, cx))
            .child(seg(free, k.fill, cx))
    });
    let legend = split.map(|(_, _, _, vb, hb, fb)| {
        div()
            .flex()
            .flex_wrap()
            .gap(space::S16)
            .child(swatch(m::LEGEND_VM, &model::bytes(vb), k.label, cx))
            .child(swatch(m::LEGEND_HOST, &model::bytes(hb), k.secondary, cx))
            .child(swatch(m::LEGEND_FREE, &model::bytes(fb), k.fill, cx))
    });
    let hint = match (cap.avg_vm_bytes, cap.reserve_bytes) {
        (Some(a), Some(r)) => fill(
            m::CAPACITY_HINT,
            &[("avg", &model::bytes(a)), ("reserve", &model::bytes(r))],
        ),
        _ => fill(m::CAPACITY_HINT_SLOTS, &[("slots", &cap.slots.to_string())]),
    };
    let t = &d.today;
    let tiles = [
        (
            d.queue.count.to_string(),
            m::TILE_QUEUE,
            d.queue
                .oldest_at
                .map(|at| fill(m::TILE_QUEUE_OLDEST, &[("wait", &model::duration(now.saturating_sub(at)))]))
                .unwrap_or_else(|| m::TILE_QUEUE_NONE.into()),
        ),
        (
            d.boot
                .as_ref()
                .map(|b| model::seconds(b.avg_ms))
                .unwrap_or_else(|| "—".into()),
            m::TILE_BOOT,
            d.boot
                .as_ref()
                .map(|b| fill(m::TILE_BOOT_MAX, &[("time", &model::seconds(b.max_ms))]))
                .unwrap_or_else(|| m::TILE_BOOT_NONE.into()),
        ),
        (
            t.finished.to_string(),
            m::TILE_DONE,
            if t.finished > t.finished_yesterday {
                fill(m::TILE_DONE_MORE, &[("n", &(t.finished - t.finished_yesterday).to_string())])
            } else if t.finished < t.finished_yesterday {
                fill(m::TILE_DONE_LESS, &[("n", &(t.finished_yesterday - t.finished).to_string())])
            } else {
                m::TILE_DONE_SAME.into()
            },
        ),
        {
            let judged = t.succeeded + t.failed;
            (
                if judged == 0 {
                    "—".into()
                } else {
                    format!("{}%", (t.succeeded as f64 * 100. / judged as f64).round() as u32)
                },
                m::TILE_OK,
                if judged == 0 {
                    m::TILE_OK_NONE.into()
                } else if t.failed == 0 {
                    m::TILE_OK_ALL.into()
                } else {
                    fill(m::TILE_OK_FAILED, &[("n", &t.failed.to_string())])
                },
            )
        },
    ];
    panel(cx)
        .flex_1()
        .p(space::S24)
        .gap(space::S16)
        .child(
            div()
                .flex()
                .items_center()
                .child(div().flex_1().n_title3().font_weight(gpui::FontWeight::SEMIBOLD).child(m::CAPACITY))
                .child(
                    div()
                        .id("capacity-all")
                        .opacity(0.5)
                        .flex()
                        .items_center()
                        .gap(space::S4)
                        .n_body()
                        .text_color(k.secondary)
                        .tooltip(|window, cx| {
                            gpui_component::tooltip::Tooltip::new(m::NAV_SOON).build(window, cx)
                        })
                        .child(m::SEE_ALL_VMS)
                        .child(sym_icon(Sym::ChevronRight, SymSize::INLINE_SMALL, k.secondary)),
                ),
        )
        .child(
            div()
                .flex()
                .items_baseline()
                .gap(space::S8)
                .child(
                    div()
                        .n_style(text::KPI)
                        .font_weight(gpui::FontWeight::SEMIBOLD)
                        .n_figures()
                        .child(cap.running.to_string()),
                )
                .child(div().n_body().text_color(k.secondary).child(m::RUNNING_SUFFIX))
                .child(div().flex_1())
                .child(
                    div()
                        .n_headline()
                        .n_figures()
                        .child(fill(m::CAN_TAKE, &[("n", &cap.can_take.to_string())])),
                ),
        )
        .child(
            div()
                .flex()
                .flex_col()
                .gap(space::S8)
                .children(bar)
                .children(legend)
                .child(TruncText::new(hint).n_footnote().text_color(k.secondary)),
        )
        .child(
            div()
                .mt_auto()
                .flex()
                .gap(space::S12)
                .children(tiles.into_iter().map(|(v, l, s)| {
                    div()
                        .flex_1()
                        .min_w(gpui::Pixels::ZERO)
                        .px(space::S12)
                        .py(space::S12)
                        .rounded(radius::CONTAINER)
                        .bg(k.group)
                        .border_1()
                        .border_color(k.separator)
                        .flex()
                        .flex_col()
                        .child(div().n_title3().font_weight(gpui::FontWeight::SEMIBOLD).n_figures().child(v))
                        .child(TruncText::new(l).n_section())
                        .child(TruncText::new(s).n_footnote().n_figures().text_color(k.secondary))
                })),
        )
}

fn seg(share: f32, color: Hsla, cx: &App) -> gpui::Div {
    let _ = cx;
    div()
        .h_full()
        .rounded(radius::CHECKBOX)
        .bg(color)
        .when(share > 0., |d| d.w(relative(share)))
        .when(share <= 0., |d| d.w(gpui::Pixels::ZERO))
}

fn swatch(label: &'static str, value: &str, color: Hsla, cx: &App) -> impl IntoElement {
    let k = native_colors(cx);
    div()
        .flex()
        .items_center()
        .gap(space::S6)
        .n_footnote()
        .text_color(k.secondary)
        .child(
            div()
                .flex_none()
                .size(t::SWATCH)
                .rounded(radius::CHECKBOX)
                .bg(color)
                .border_1()
                .border_color(k.separator),
        )
        .child(label)
        .child(div().n_figures().font_weight(gpui::FontWeight::MEDIUM).text_color(k.label).child(value.to_string()))
}

// ---------------------------------------------------------------------------
// 要注意

fn attention(
    d: &MachineMonitor,
    now: u64,
    name: &dyn Fn(&str) -> String,
    time: &dyn Fn(u64) -> String,
    cx: &App,
) -> impl IntoElement {
    let k = native_colors(cx);
    let items = model::attention(d, now, name, time);
    let n = items.len();
    panel(cx)
        .flex_1()
        .p(space::S24)
        .gap(space::S8)
        .child(
            div()
                .flex()
                .items_center()
                .gap(space::S8)
                .child(div().n_title3().font_weight(gpui::FontWeight::SEMIBOLD).child(m::ATTENTION))
                .when(n > 0, |d| {
                    d.child(crate::ui::native::CountPill::new(n as u32).tone(k.amber))
                }),
        )
        .when(n == 0, |d| {
            d.child(
                crate::ui::native::EmptyLine::new(m::ATT_NONE).icon(Sym::Done),
            )
        })
        .children(items.into_iter().enumerate().map(|(i, a)| {
            let icon = match a.kind {
                AttentionKind::Waiting => Sym::Privacy,
                AttentionKind::CliLoggedOut => Sym::Terminal,
                AttentionKind::Queue => Sym::Hourglass,
                AttentionKind::Failed => Sym::Failed,
                AttentionKind::Disk => Sym::Disk,
                AttentionKind::Offline => Sym::Offline,
            };
            let c = tone_color(a.tone, &k);
            div()
                .flex()
                .items_center()
                .gap(space::S12)
                .py(space::S8)
                .when(i > 0, |d| d.border_t_1().border_color(k.separator))
                .child(
                    div()
                        .flex_none()
                        .size(height::DEFAULT)
                        .rounded(radius::CONTROL)
                        .bg(k.fill)
                        .flex()
                        .items_center()
                        .justify_center()
                        .child(sym_icon(icon, SymSize::INLINE, c)),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w(gpui::Pixels::ZERO)
                        .flex()
                        .flex_col()
                        .child(TruncText::new(a.title).n_headline())
                        .when(!a.meta.is_empty(), |d| {
                            d.child(TruncText::new(a.meta).n_footnote().n_figures().text_color(k.secondary))
                        }),
                )
        }))
}

// ---------------------------------------------------------------------------
// 按成员

fn members(
    d: &MachineMonitor,
    sort: Sort,
    name: &dyn Fn(&str) -> String,
    member_of: &dyn Fn(&str) -> Option<crate::data::Member>,
    view: Entity<MonitorView>,
    cx: &mut Context<MonitorView>,
) -> impl IntoElement {
    let k = native_colors(cx);
    let rows = model::member_rows(d, sort, name);
    let max_mem = rows.iter().map(|r| r.mem_bytes).max().unwrap_or(0);
    let cols: [(SortKey, Option<gpui::Pixels>); 7] = [
        (SortKey::Name, None),
        (SortKey::Doing, Some(t::COL_COUNT)),
        (SortKey::Waiting, Some(t::COL_COUNT)),
        (SortKey::Cpu, Some(t::COL_CPU)),
        (SortKey::Memory, Some(t::COL_MEM)),
        (SortKey::Tokens, Some(t::COL_TOKENS)),
        (SortKey::Done, Some(t::COL_DONE)),
    ];
    let cell = |w: Option<gpui::Pixels>, right: bool| {
        let c = div().px(space::S12).flex().items_center().overflow_hidden();
        let c = match w {
            Some(w) => c.w(w).flex_none(),
            None => c.flex_1().min_w(gpui::Pixels::ZERO),
        };
        if right { c.justify_end() } else { c }
    };
    let head = div()
        .h(height::TABLE_HEAD)
        .px(space::S4)
        .flex()
        .items_center()
        .bg(k.group)
        .border_b_1()
        .border_color(k.separator)
        .children(cols.iter().map(|(key, w)| {
            let on = sort.key == *key;
            let right = *key != SortKey::Name && *key != SortKey::Memory;
            let key = *key;
            let view = view.clone();
            let arrow = if on {
                if sort.descending { Sym::ArrowDown } else { Sym::ArrowUp }
            } else {
                Sym::Sort
            };
            cell(*w, right).child(
                div()
                    .id(SharedString::from(format!("sort-{key:?}")))
                    .flex()
                    .items_center()
                    .gap(space::S4)
                    .px(space::S4)
                    .rounded(radius::ITEM)
                    .hover(|s| s.bg(k.fill_hover))
                    .n_section()
                    .text_color(if on { k.label } else { k.secondary })
                    .tooltip(move |window, cx| {
                        gpui_component::tooltip::Tooltip::new(fill(m::SORT_BY, &[("col", key.label())]))
                            .build(window, cx)
                    })
                    .on_click(move |_, _, cx| view.update(cx, |v, cx| v.sort_by(key, cx)))
                    .child(key.label())
                    .child(sym_icon(arrow, SymSize::INLINE_SMALL, if on { k.label } else { k.tertiary })),
            )
        }));
    let n = rows.len();
    let body: Vec<AnyElement> = if rows.is_empty() {
        vec![
            div()
                .h(height::TABLE_ROW * 2.)
                .flex()
                .items_center()
                .justify_center()
                .n_body()
                .text_color(k.secondary)
                .child(m::MEMBERS_EMPTY)
                .into_any_element(),
        ]
    } else {
        rows.into_iter()
            .enumerate()
            .map(|(i, r)| {
                let member = member_of(&r.member_id);
                div()
                    .h(height::TABLE_ROW_COMPACT)
                    .px(space::S4)
                    .flex()
                    .items_center()
                    .n_body()
                    .n_figures()
                    .when(i + 1 < n, |d| d.border_b_1().border_color(k.separator))
                    .child(
                        cell(None, false).gap(space::S12).child(MemberAvatar::new(member.as_ref(), avatar::XS)).child(
                            TruncText::new(r.name.clone()).n_headline(),
                        ),
                    )
                    .child(cell(Some(t::COL_COUNT), true).child(r.doing.to_string()))
                    .child(
                        cell(Some(t::COL_COUNT), true)
                            .text_color(if r.waiting > 0 { k.amber } else { k.secondary })
                            .when(r.waiting > 0, |d| d.font_weight(gpui::FontWeight::SEMIBOLD))
                            .child(r.waiting.to_string()),
                    )
                    .child(cell(Some(t::COL_CPU), true).text_color(k.secondary).child(model::cores(r.cpu_cores)))
                    .child(
                        cell(Some(t::COL_MEM), false)
                            .gap(space::S8)
                            .child(
                                div()
                                    .flex_none()
                                    .w(t::MEMBER_BAR_WIDTH)
                                    .h(t::MEMBER_BAR)
                                    .rounded_full()
                                    .bg(k.fill)
                                    .overflow_hidden()
                                    .child(
                                        div()
                                            .h_full()
                                            .rounded_full()
                                            .bg(k.label)
                                            .w(relative(model::share(r.mem_bytes, max_mem))),
                                    ),
                            )
                            .child(div().flex_1().flex().justify_end().child(model::bytes(r.mem_bytes))),
                    )
                    .child(cell(Some(t::COL_TOKENS), true).child(model::count_short(r.tokens)))
                    .child(cell(Some(t::COL_DONE), true).text_color(k.secondary).child(r.done.to_string()))
                    .into_any_element()
            })
            .collect()
    };
    div()
        .w_full()
        .flex()
        .flex_col()
        .gap(space::S12)
        .mt(space::S8)
        .child(
            div()
                .flex()
                .items_baseline()
                .gap(space::S8)
                .child(div().n_title3().font_weight(gpui::FontWeight::SEMIBOLD).child(m::MEMBERS))
                .child(
                    div()
                        .n_body()
                        .text_color(k.secondary)
                        .child(fill(m::MEMBERS_COUNT, &[("n", &d.members.len().to_string())])),
                ),
        )
        .child(
            div()
                .w_full()
                .rounded(radius::CONTAINER)
                .border_1()
                .border_color(k.separator)
                .bg(k.content)
                .overflow_hidden()
                .flex()
                .flex_col()
                .child(head)
                .children(body),
        )
}

// ---------------------------------------------------------------------------
// Loading

/// The page's shapes while the first answer is on its way (§2.14).
pub fn page_skeleton(cx: &App) -> impl IntoElement {
    let bone = |h: gpui::Pixels| skeleton(relative(1.), h);
    let card = |h: gpui::Pixels| panel(cx).flex_1().p(space::S16).gap(space::S8).child(bone(h));
    div()
        .w_full()
        .flex()
        .flex_col()
        .gap(space::S16)
        .child(panel(cx).p(space::S16).child(bone(height::DEFAULT)))
        .child(
            div()
                .flex()
                .gap(space::S16)
                .children((0..4).map(|_| card(height::LIST_ROW * 2.))),
        )
        .child(
            div()
                .flex()
                .gap(space::S16)
                .child(card(height::LIST_ROW * 4.))
                .child(card(height::LIST_ROW * 4.)),
        )
        .child(
            panel(cx)
                .p(space::S16)
                .gap(space::S12)
                .children((0..4).map(|_| bone(height::SM))),
        )
}
