//! One renderer per Card v2 block (DESIGN.md §7.10 "Blocks"). Runs of the
//! same kind draw together: metrics share a row, evidence shares a strip of
//! thumbnails and chips, refs share one list box, inputs form one grid.

use gpui::{
    AnyElement, App, Div, FontWeight, Hsla, InteractiveElement as _, IntoElement, ObjectFit,
    ParentElement, SharedString, StatefulInteractiveElement as _, Styled, StyledImage as _, Window,
    div, img, prelude::FluentBuilder as _, px, relative,
};
use gpui_component::{
    Disableable as _, Sizable as _,
    button::Button,
    input::{Input, Textarea},
    menu::{DropdownMenu as _, PopupMenuItem},
    switch::Switch,
};
use serde_json::Value;

use super::{Ctx, choice_text, form::Field, letter, meta, option_label, state::CardState};
use crate::{
    copy::{cards_v2 as c, fill},
    data::{
        TopicStatus,
        card::{CardField, CardListItem, CardListStatus, LinkTarget},
        cards_v2::{
            CardIntent, CardV2, CardV2Block, DecisionOption, EvidenceState, InputKind, InputOption,
            MetricBetter, RefTarget, RunPlanItem, RunPlanState,
        },
    },
    theme::{self, Type as _, card_v2 as m, card_v2::mix, space},
    ui::{self, Sym, native::NativeType as _},
    views::refs::{
        affordance::Anchor,
        cards::{self as ref_parts, BlockParts, CardParts, Pick},
    },
};

/// The 引用提问 parts of block `b` (REFS §A2.2), where they show.
pub(crate) fn block_parts(ctx: &Ctx, b: &CardV2Block, cx: &App) -> Option<BlockParts> {
    CardParts::of(ctx, cx)?.block(b)
}

/// 13/20.
pub(crate) fn small(el: Div) -> Div {
    el.text_size(px(13.)).line_height(px(20.))
}

/// The blocks of the card body, with their spacing.
pub(crate) fn body(ctx: &Ctx, window: &mut Window, cx: &mut App) -> Vec<AnyElement> {
    let mut out: Vec<AnyElement> = Vec::new();
    let blocks: Vec<&CardV2Block> = ctx
        .card
        .blocks
        .iter()
        .filter(|b| !matches!(b, CardV2Block::Unknown))
        .collect();
    let note_input = footer_note_input(ctx.card).map(|(id, _)| id);
    let collapsed = ctx.state.collapsed();
    let mut record_done = false;
    let mut i = 0;
    let mut prev: Option<&CardV2Block> = None;
    while i < blocks.len() {
        let b = blocks[i];
        // Runs of one kind.
        let run_end = |pred: &dyn Fn(&CardV2Block) -> bool| {
            let mut j = i;
            while j < blocks.len() && pred(blocks[j]) {
                j += 1;
            }
            j
        };
        let (el, next, gap) = match b {
            CardV2Block::Input { id, .. } if note_input.as_deref() == Some(id.as_str()) => {
                (None, i + 1, space::S12)
            }
            CardV2Block::Decision { .. } | CardV2Block::Input { .. } if collapsed => {
                let j = run_end(&|x| {
                    matches!(x, CardV2Block::Decision { .. } | CardV2Block::Input { .. })
                });
                let el = (!record_done).then(|| record(ctx, window, cx));
                record_done = true;
                (el, j, space::S12)
            }
            CardV2Block::Input { .. } => {
                let j = run_end(&|x| matches!(x, CardV2Block::Input { .. }))
                    .min(note_input_ix(&blocks, note_input.as_deref()).unwrap_or(usize::MAX));
                let j = j.max(i + 1);
                (
                    Some(form_grid(ctx, &blocks[i..j], window, cx)),
                    j,
                    space::S12,
                )
            }
            CardV2Block::Metric { .. } => {
                let j = run_end(&|x| matches!(x, CardV2Block::Metric { .. }));
                let parts = CardParts::of(ctx, cx);
                (
                    Some(metrics_with(
                        &blocks[i..j],
                        parts.as_ref(),
                        Some(window),
                        cx,
                    )),
                    j,
                    space::S12,
                )
            }
            CardV2Block::Evidence { .. } => {
                let j = run_end(&|x| matches!(x, CardV2Block::Evidence { .. }));
                (
                    Some(evidence(ctx, &blocks[i..j], window, cx)),
                    j,
                    space::S12,
                )
            }
            CardV2Block::Ref { .. } => {
                let j = run_end(&|x| matches!(x, CardV2Block::Ref { .. }));
                (Some(refs(ctx, &blocks[i..j], cx)), j, space::S12)
            }
            CardV2Block::Text { .. }
                if collapsed && matches!(prev, Some(CardV2Block::Decision { .. })) =>
            {
                (None, i + 1, px(0.))
            }
            CardV2Block::Text { text, .. }
                if matches!(prev, Some(CardV2Block::Decision { .. })) =>
            {
                // The recommendation line under the options.
                let k = theme::colors(cx);
                (
                    Some(
                        meta(div())
                            .text_color(k.ink_3)
                            .child(text.clone())
                            .into_any_element(),
                    ),
                    i + 1,
                    space::S10,
                )
            }
            CardV2Block::Chart { .. } if matches!(prev, Some(CardV2Block::Metric { .. })) => {
                let k = theme::colors(cx);
                (
                    Some(
                        div()
                            .border_t_1()
                            .border_color(k.line)
                            .pt(space::S14)
                            .child(block(ctx, b, i, window, cx))
                            .into_any_element(),
                    ),
                    i + 1,
                    space::S16,
                )
            }
            CardV2Block::Progress { .. } if matches!(prev, Some(CardV2Block::Plan { .. })) => {
                // Drawn inside the plan, under its current step.
                (None, i + 1, px(0.))
            }
            CardV2Block::Plan { .. } => {
                let progress = match blocks.get(i + 1) {
                    Some(CardV2Block::Progress { value, label, .. }) => {
                        Some((value.as_f64().unwrap_or(0.), label.clone()))
                    }
                    _ => None,
                };
                (
                    Some(plan(ctx, b, progress, Some(window), cx)),
                    i + 1,
                    space::S12,
                )
            }
            _ => (Some(block(ctx, b, i, window, cx)), i + 1, space::S12),
        };
        if let Some(el) = el {
            // The keyboard part focus of the block (or the run drawn together), REFS §A3.
            let el = match CardParts::of(ctx, cx) {
                Some(cp) => {
                    let run: Vec<BlockParts> =
                        blocks[i..next].iter().filter_map(|b| cp.block(b)).collect();
                    ref_parts::focus_run(&run, el, window, cx)
                }
                None => el,
            };
            out.push(div().mt(gap).min_w(px(0.)).child(el).into_any_element());
        }
        prev = Some(b);
        i = next;
    }
    if let Some(notice) = no_evidence(ctx, cx) {
        out.push(div().mt(space::S12).child(notice).into_any_element());
    }
    out
}

fn note_input_ix(blocks: &[&CardV2Block], id: Option<&str>) -> Option<usize> {
    let id = id?;
    blocks.iter().position(|b| b.id() == Some(id))
}

/// One block on its own.
pub(crate) fn block(
    ctx: &Ctx,
    b: &CardV2Block,
    ix: usize,
    window: &mut Window,
    cx: &mut App,
) -> AnyElement {
    let k = theme::colors(cx);
    match b {
        CardV2Block::Text { text, .. } => div()
            .text_size(px(14.))
            .line_height(px(22.))
            .text_color(k.ink_2)
            .child(text.clone())
            .into_any_element(),
        CardV2Block::Fields { fields, .. } => fields_grid(fields, cx),
        CardV2Block::Table { columns, rows, .. } => {
            // 引用提问: cells, rows and columns (REFS §A2.2, R4).
            let refs = block_parts(ctx, b, cx).and_then(|p| p.table_refs());
            table_with_refs(columns, rows, refs.as_ref(), Some(window), cx)
        }
        CardV2Block::List { items, .. } => {
            list_with(items, block_parts(ctx, b, cx).as_ref(), Some(window), cx)
        }
        CardV2Block::Progress { value, label, .. } => {
            progress_bar(value.as_f64().unwrap_or(0.), label.as_deref(), None, cx)
        }
        CardV2Block::Code { text, .. } => code(text, cx),
        CardV2Block::Image { file_id, alt, .. } => {
            let el = image(ctx, file_id, alt.as_deref(), cx);
            ref_parts::wrap_opt(
                block_parts(ctx, b, cx).as_ref(),
                Pick::Image,
                el,
                Anchor::Corner,
                Some(window),
                cx,
            )
        }
        CardV2Block::Divider { .. } => div().h(px(1.)).bg(k.line).into_any_element(),
        CardV2Block::Link { label, target, .. } => link(ctx, label, target, ix, cx),
        CardV2Block::Decision {
            id,
            options,
            multiple,
            default,
            ..
        } => decision(
            ctx,
            id,
            options,
            multiple.unwrap_or(false),
            default.as_deref().unwrap_or(&[]),
            cx,
        ),
        CardV2Block::Input { .. } => form_grid(ctx, &[b], window, cx),
        CardV2Block::Draft { .. } => draft_well(ctx, &[b], window, cx),
        CardV2Block::Plan { .. } => plan(ctx, b, None, Some(window), cx),
        CardV2Block::Metric { .. } => {
            metrics_with(&[b], CardParts::of(ctx, cx).as_ref(), Some(window), cx)
        }
        CardV2Block::Chart {
            kind,
            label,
            unit,
            x,
            series,
            ..
        } => {
            let el = super::chart::chart(
                ctx.id(format!("chart-{ix}")),
                *kind,
                label.as_deref(),
                unit.as_deref(),
                x.as_deref().unwrap_or(&[]),
                series,
                cx,
            );
            // 引用提问: the points (bars) of the main series.
            match block_parts(ctx, b, cx).and_then(|p| {
                ref_parts::chart_overlay(&p, ref_parts::chart_plot_top(b), m::CHART, window, cx)
            }) {
                Some(overlay) => div().relative().child(el).child(overlay).into_any_element(),
                None => el,
            }
        }
        CardV2Block::Diff {
            path,
            text,
            additions,
            deletions,
            ..
        } => diff(path.as_deref(), text, *additions, *deletions, cx),
        CardV2Block::Evidence { .. } => evidence(ctx, &[b], window, cx),
        CardV2Block::Ref { .. } => refs(ctx, &[b], cx),
        CardV2Block::Group { label, blocks, .. } => group(
            ctx,
            label.as_deref(),
            blocks.as_deref().unwrap_or(&[]),
            ix,
            window,
            cx,
        ),
        // Unknown blocks and the 2.1 blocks (the 2.1 renderer draws those).
        _ => div().into_any_element(),
    }
}

// ---------------------------------------------------------------------------
// v1 blocks

fn fields_grid(fields: &[CardField], cx: &App) -> AnyElement {
    let k = theme::colors(cx);
    small(div())
        .flex()
        .flex_col()
        .gap(space::S6)
        .children(fields.iter().map(|f| {
            div()
                .flex()
                .items_start()
                .gap(space::S12)
                .child(
                    div()
                        .flex_none()
                        .w(m::FIELD_KEY)
                        .text_color(k.ink_3)
                        .child(f.label.clone()),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w(px(0.))
                        .text_color(k.ink)
                        .child(f.value.clone()),
                )
        }))
        .into_any_element()
}

/// A number-like cell: digits with an optional sign, `%`, `,` or `.`.
pub(crate) fn numeric(s: &str) -> bool {
    let t = s.trim();
    !t.is_empty()
        && t.chars().any(|c| c.is_ascii_digit())
        && t.chars()
            .all(|c| c.is_ascii_digit() || "+-−.,%  ".contains(c))
}

/// The width of number column `ci`: its widest header (12 pt) or mono cell (13 pt), at
/// least [`m::TABLE_NUM_COL`], so the header and every row line up at the right.
pub(crate) fn num_col_width(columns: &[String], rows: &[Vec<String>], ci: usize) -> gpui::Pixels {
    // One CJK character is about one em; a mono or Latin character about 0.62 em.
    let est = |t: &str, size: f32| -> f32 {
        t.chars()
            .map(|c| if c.is_ascii() { 0.62 * size } else { size })
            .sum()
    };
    let head = columns.get(ci).map_or(0., |h| est(h, 12.));
    let cells = rows
        .iter()
        .filter_map(|r| r.get(ci))
        .map(|v| {
            let bar = if percent(v).is_some() {
                f32::from(m::TABLE_PCT_BAR) + f32::from(space::S8)
            } else {
                0.
            };
            est(v, 13.) + bar
        })
        .fold(0f32, f32::max);
    px(head.max(cells).ceil() + 2.).max(m::TABLE_NUM_COL)
}

fn percent(s: &str) -> Option<f32> {
    s.trim()
        .strip_suffix('%')
        .and_then(|n| n.trim().parse::<f32>().ok())
}

pub(crate) fn table(columns: &[String], rows: &[Vec<String>], cx: &App) -> AnyElement {
    table_with_refs(columns, rows, None, None, cx)
}

/// [`table`] with the 引用提问 parts of its cells, rows and header (inc-integrate): the
/// same look, plus the 问这里 slot at the end of each row.
pub(crate) fn table_with_refs(
    columns: &[String],
    rows: &[Vec<String>],
    refs: Option<&crate::views::refs::markdown::TableRefs>,
    window: Option<&gpui::Window>,
    cx: &App,
) -> AnyElement {
    let refs = refs.zip(window);
    let k = theme::colors(cx);
    let numeric_col: Vec<bool> = (0..columns.len())
        .map(|ci| {
            ci > 0
                && rows
                    .iter()
                    .all(|r| r.get(ci).is_none_or(|v| v.is_empty() || numeric(v)))
        })
        .collect();
    let widths: Vec<Option<gpui::Pixels>> = numeric_col
        .iter()
        .enumerate()
        .map(|(ci, n)| n.then(|| num_col_width(columns, rows, ci)))
        .collect();
    let cell = |ci: usize, text: &str, head: bool, bold: bool| {
        let num = numeric_col.get(ci).copied().unwrap_or(false);
        let pct = (!head).then(|| percent(text)).flatten();
        // Text columns share the width; number columns take what their numbers need.
        let width = widths.get(ci).copied().flatten();
        div()
            .map(|el| match width {
                Some(w) => el.flex_none().w(w),
                None => el.flex_1(),
            })
            .when(ci == 0, |el| el.flex_grow(1.).min_w(px(96.)))
            .min_w(px(0.))
            .flex()
            .items_center()
            .gap(space::S8)
            .when(num, |el| el.justify_end())
            .when_some(pct, |el, p| {
                el.child(
                    div()
                        .w(m::TABLE_PCT_BAR)
                        .h(px(3.))
                        .rounded(px(2.))
                        .bg(k.line_2)
                        .child(
                            div()
                                .h_full()
                                .rounded(px(2.))
                                .bg(k.ink_2)
                                .w(relative((p / 100.).clamp(0., 1.))),
                        ),
                )
            })
            .child(
                div()
                    .one_line()
                    .when(num && !head, |el| el.mono())
                    .when(bold, |el| el.font_weight(FontWeight::SEMIBOLD))
                    .child(text.to_string()),
            )
    };
    let last = rows.len().saturating_sub(1);
    small(div())
        .flex()
        .flex_col()
        .child(
            meta(div())
                .flex()
                .gap(space::S12)
                .pb(space::S8)
                .border_b_1()
                .border_color(k.line)
                .text_color(k.ink_3)
                .children(columns.iter().enumerate().map(|(ci, col)| {
                    let mut el = cell(ci, col, true, false);
                    if ci == 0
                        && let Some((r, _)) = refs
                    {
                        // The first column grows: its width for 问这里 after a row's name.
                        el = el.relative().child(r.measure(0.));
                    }
                    match refs {
                        Some((r, w)) => r.header(el, ci, w, cx),
                        None => el.into_any_element(),
                    }
                })),
        )
        .children(rows.iter().enumerate().map(|(ri, r)| {
            let total = ri == last && r.first().is_some_and(|x| x.trim() == c::TABLE_TOTAL);
            let line = div()
                .flex()
                .items_center()
                .gap(space::S12)
                .h(m::TABLE_ROW)
                .when(ri < last, |el| el.border_b_1().border_color(k.line))
                .when(total, |el| el.border_t_1().border_color(k.line_2))
                .text_color(k.ink);
            let line = match refs {
                Some((rf, w)) => rf.row(line, ri, w, cx),
                None => line,
            };
            // 问这里 after the row's name when the first column holds both (Refs.dc.html).
            let beside = refs.is_some_and(|(rf, w)| {
                !numeric_col.first().copied().unwrap_or(false)
                    && rf.beside(r.first().map(String::as_str).unwrap_or(""), None, w)
            });
            line.children((0..columns.len()).map(|ci| {
                let mut el = cell(
                    ci,
                    r.get(ci).map(String::as_str).unwrap_or(""),
                    false,
                    total,
                );
                if ci == 0 && beside {
                    el = el.children(
                        refs.and_then(|(rf, _)| rf.name_button(ri, f32::from(space::S8), cx)),
                    );
                }
                match refs {
                    Some((rf, w)) => rf.cell(el, ri, ci, w, cx),
                    None => el.into_any_element(),
                }
            }))
            // A name too long for 问这里 beside it gets it at the row end, over the row, so
            // the number columns stay at the right edge (Interact.dc.html 02).
            .when_some(refs.filter(|_| !beside), |el, (rf, _)| {
                el.relative().child(
                    div()
                        .absolute()
                        .top_0()
                        .bottom_0()
                        .right_0()
                        .flex()
                        .child(rf.row_end(ri, cx)),
                )
            })
        }))
        .into_any_element()
}

pub(crate) fn list(items: &[CardListItem], cx: &App) -> AnyElement {
    list_with(items, None, None, cx)
}

/// [`list`] with the 引用提问 part of each item (REFS §A2.2 `item`).
pub(crate) fn list_with(
    items: &[CardListItem],
    parts: Option<&BlockParts>,
    window: Option<&Window>,
    cx: &App,
) -> AnyElement {
    let k = theme::colors(cx);
    let parts = parts.zip(window);
    small(div())
        .flex()
        .flex_col()
        .gap(space::S4)
        .children(items.iter().enumerate().map(|(i, it)| {
            let pick = Pick::Item(i);
            let (glyph, color, text) = match it.status {
                Some(CardListStatus::Done) => (Some(Sym::Check), k.ink_3, k.ink_2),
                Some(CardListStatus::InProgress) => (None, k.live, k.ink),
                Some(CardListStatus::Failed) => (Some(Sym::Close), k.bad, k.ink),
                Some(CardListStatus::Skipped) => (Some(Sym::Minus), k.ink_3, k.ink_3),
                Some(CardListStatus::Todo) | None => (None, k.ink_3, k.ink_2),
            };
            let row = div()
                .id(SharedString::from(format!("list-item-{i}")))
                .flex()
                .items_center()
                .gap(space::S10)
                .min_h(px(24.))
                .when(parts.is_some(), |el| el.mx(-space::S4).px(space::S4))
                .child(
                    div().flex_none().w(px(16.)).flex().justify_center().child(
                        match (glyph, it.status) {
                            (Some(g), _) => ui::icon(g, px(14.), color).into_any_element(),
                            (None, Some(CardListStatus::InProgress)) => div()
                                .size(px(8.))
                                .rounded_full()
                                .bg(color)
                                .into_any_element(),
                            _ => div()
                                .size(px(5.))
                                .rounded_full()
                                .bg(k.ink_3)
                                .into_any_element(),
                        },
                    ),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w(px(0.))
                        .flex()
                        .items_center()
                        .gap(space::S8)
                        .child(div().min_w(px(0.)).text_color(text).child(it.text.clone()))
                        .children(parts.and_then(|(p, _)| p.button(pick, cx))),
                );
            match parts {
                Some((p, w)) => p.wrap_own(pick, row, w, cx),
                None => row.into_any_element(),
            }
        }))
        .into_any_element()
}

/// Segments of a counted progress (`已重启 9/24`), else a plain bar.
fn segments(label: Option<&str>) -> Option<usize> {
    let l = label?;
    let tail = l.rsplit(['/', '／']).next()?;
    let n: usize = tail
        .trim()
        .chars()
        .take_while(char::is_ascii_digit)
        .collect::<String>()
        .parse()
        .ok()?;
    (2..=40).contains(&n).then_some(n)
}

fn progress_bar(value: f64, label: Option<&str>, count: Option<usize>, cx: &App) -> AnyElement {
    let k = theme::colors(cx);
    let v = value.clamp(0., 1.) as f32;
    let bar = match count.or_else(|| segments(label)) {
        Some(n) => {
            let lit = (v * n as f32).round() as usize;
            div()
                .flex()
                .gap(px(3.))
                .children((0..n).map(|i| {
                    div().flex_1().h(px(4.)).rounded(px(2.)).bg(
                        if i + 1 < lit || (i < lit && lit == n) {
                            k.ink_2
                        } else if i + 1 == lit {
                            k.live
                        } else {
                            k.line_2
                        },
                    )
                }))
                .into_any_element()
        }
        None => div()
            .h(px(4.))
            .rounded(px(2.))
            .bg(k.line_2)
            .child(div().h_full().rounded(px(2.)).bg(k.live).w(relative(v)))
            .into_any_element(),
    };
    div()
        .flex()
        .flex_col()
        .gap(space::S6)
        .when_some(label, |el, l| {
            el.child(meta(div()).text_color(k.ink_3).child(l.to_string()))
        })
        .child(bar)
        .into_any_element()
}

fn code(text: &str, cx: &App) -> AnyElement {
    let k = theme::colors(cx);
    div()
        .rounded(theme::radius::MD)
        .bg(k.sunken)
        .px(space::S12)
        .py(space::S8)
        .text_size(px(12.))
        .line_height(px(20.))
        .mono()
        .text_color(k.ink)
        .child(text.to_string())
        .into_any_element()
}

fn image(ctx: &Ctx, file_id: &str, alt: Option<&str>, cx: &App) -> gpui::Stateful<Div> {
    let k = theme::colors(cx);
    let path = ctx
        .env
        .provider
        .card_image(&ctx.msg.group_id, &ctx.msg.topic_id, file_id);
    div()
        .id(ctx.id(format!("image-{file_id}")))
        .w(px(320.))
        .h(px(200.))
        .rounded(theme::radius::MD)
        .overflow_hidden()
        .bg(k.ph[0])
        .map(|el| match path {
            Some(p) => el.child(img(p).size_full().object_fit(ObjectFit::Cover)),
            None => el.when_some(alt, |el, a| {
                el.flex()
                    .items_center()
                    .justify_center()
                    .child(meta(div()).text_color(k.ink_3).child(a.to_string()))
            }),
        })
}

fn link(ctx: &Ctx, label: &str, target: &LinkTarget, ix: usize, cx: &App) -> AnyElement {
    let k = theme::colors(cx);
    let t = match target {
        LinkTarget::Url { url } => RefTarget::Url { url: url.clone() },
        LinkTarget::Topic { group_id, topic_id } => RefTarget::Topic {
            group_id: group_id.clone(),
            topic_id: topic_id.clone(),
        },
        LinkTarget::Artifact {
            artifact_id,
            version,
        } => RefTarget::Artifact {
            artifact_id: artifact_id.clone(),
            version: *version,
        },
    };
    let a = ctx.answerer();
    small(div())
        .id(ctx.id(format!("link-block-{ix}")))
        .flex()
        .items_center()
        .gap(space::S4)
        .text_color(k.ink)
        .font_weight(theme::MEDIUM_WEIGHT)
        .hover(|s| s.text_color(k.ink_2))
        .child(label.to_string())
        .child(ui::icon(Sym::ChevronRight, px(14.), k.ink_3))
        .on_click(move |_, window, cx| a.open(&t, window, cx))
        .into_any_element()
}

// ---------------------------------------------------------------------------
// Decisions and the answer record

pub(crate) fn key_tile(i: usize, cx: &App) -> Div {
    let k = theme::colors(cx);
    div()
        .flex_none()
        .size(m::TILE)
        .rounded(theme::radius::SM)
        .bg(k.sunken)
        .flex()
        .items_center()
        .justify_center()
        .text_size(px(11.))
        .mono()
        .text_color(k.ink_2)
        .child(letter(i).to_string())
}

pub(crate) fn decision(
    ctx: &Ctx,
    id: &str,
    options: &[DecisionOption],
    multiple: bool,
    recommended: &[String],
    cx: &App,
) -> AnyElement {
    use crate::ui::native::{self as nk, NativeType as _};
    let n = nk::native_colors(cx);
    let quick = ctx.quick();
    let live = ctx.live();
    let chosen = ctx.form.read(cx).choice(id).to_vec();
    let a = ctx.answerer();
    let sending = matches!(ctx.state, CardState::Sending { .. });
    let enabled = live && !sending;
    // Two short options: a row of push buttons, a press answers
    // (NATIVE.md §5.2). The option letter leads in `secondary` so
    // `推荐 A` still reads.
    if quick {
        let buttons = options.iter().enumerate().map(|(i, o)| {
            let picked = chosen.contains(&o.id);
            let (a, block, option) = (a.clone(), id.to_string(), o.id.clone());
            let style = if picked {
                nk::PushStyle::Default
            } else {
                nk::PushStyle::Standard
            };
            let ink = if picked { n.on_accent } else { n.secondary };
            nk::disabled_push(
                nk::push_button(
                    ctx.id(format!("opt-{id}-{}", o.id)),
                    "",
                    style,
                    nk::PushSize::Regular,
                    cx,
                )
                .flex_none()
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(space::S6)
                        .child(div().text_color(ink).child(letter(i).to_string()))
                        .child(o.label.clone()),
                ),
                !enabled && !picked,
                cx,
            )
            .on_click(move |_, _, cx| a.pick(&block, &option, cx))
        });
        return div()
            .flex()
            .flex_wrap()
            .gap(space::S8)
            .children(buttons)
            .into_any_element();
    }
    // Three or more: radio buttons (checkboxes when several may be
    // chosen), one 24 pt row each; the letter, the label, a `secondary`
    // detail and `推荐` at the trailing edge (§5.4).
    let rows = options.iter().enumerate().map(|(i, o)| {
        let picked = chosen.contains(&o.id);
        let rec = recommended.contains(&o.id);
        let choose = {
            let (block, option, form) = (id.to_string(), o.id.clone(), ctx.form.clone());
            move |_: bool, _: &mut Window, cx: &mut App| {
                form.update(cx, |f, cx| f.choose(&block, &option, multiple, cx))
            }
        };
        let control = if multiple {
            nk::NativeCheckbox::new(ctx.id(format!("opt-{id}-{}-c", o.id)), picked)
                .disabled(!enabled)
                .on_change(choose.clone())
                .into_any_element()
        } else {
            nk::NativeRadio::new(ctx.id(format!("opt-{id}-{}-c", o.id)), picked)
                .disabled(!enabled)
                .on_change(choose.clone())
                .into_any_element()
        };
        div()
            .id(ctx.id(format!("opt-{id}-{}", o.id)))
            .min_h(px(24.))
            .flex()
            .items_center()
            .gap(space::S8)
            .n_body()
            .text_color(n.label)
            .when(enabled, |el| {
                el.on_click(move |_, window, cx| choose(true, window, cx))
            })
            .child(control)
            .child(
                div()
                    .flex_none()
                    .w(px(12.))
                    .text_color(n.secondary)
                    .child(letter(i).to_string()),
            )
            .child(div().min_w(px(0.)).one_line().child(o.label.clone()))
            .when_some(o.detail.clone(), |el, d| {
                el.child(div().min_w(px(0.)).one_line().text_color(n.secondary).child(d))
            })
            .when(rec, |el| {
                el.child(
                    div()
                        .ml_auto()
                        .flex_none()
                        .n_subheadline()
                        .text_color(n.secondary)
                        .child(c::RECOMMENDED),
                )
            })
    });
    div()
        .flex()
        .flex_col()
        .gap(space::S4)
        .children(rows)
        .into_any_element()
}

/// The answer record that replaces the controls once answered: a `sunken`
/// row with the drawn check, `你选了 B：…` (or `Joye 选了 B：…`), where it
/// went, the time, and `撤回` while my answer is still on its way or
/// `展开` for others.
pub(crate) fn record(ctx: &Ctx, window: &mut Window, cx: &mut App) -> AnyElement {
    let _ = window;
    let k = theme::colors(cx);
    let (actor, action, inputs, at, sending) = match ctx.state {
        CardState::Answered { answer } => (
            answer.actor_id.clone(),
            answer.action_id.clone(),
            answer.inputs.clone(),
            Some(answer.at),
            false,
        ),
        CardState::Sending { action_id } => {
            let pending = ctx
                .env
                .provider
                .card_v2_pending(&ctx.msg.group_id, &ctx.msg.message_id);
            (
                ctx.me.clone().unwrap_or_default(),
                action_id.clone(),
                pending.as_ref().and_then(|p| p.inputs.clone()),
                pending.map(|p| p.at),
                true,
            )
        }
        _ => return div().into_any_element(),
    };
    let mine = ctx.me.as_deref() == Some(actor.as_str());
    let fresh = !sending && at.is_some_and(ui::motion::just_answered);
    let choice = choice_text(ctx.card, &action, inputs.as_ref());
    let text = if mine {
        fill(c::RECORD_MINE, &[("choice", &choice)])
    } else {
        fill(
            c::RECORD_OTHER,
            &[("name", &ctx.name(&actor)), ("choice", &choice)],
        )
    };
    let expanded = ctx.form.read(cx).expanded;
    let a = ctx.answerer();
    let form = ctx.form.clone();
    let action_el = if sending && mine {
        Some(
            quiet_button(ctx.id("undo"), c::CARD_UNDO, cx)
                .on_click(move |_, _, cx| a.withdraw(cx))
                .into_any_element(),
        )
    } else if !mine || inputs.as_ref().is_some_and(|i| i.len() > 1) {
        Some(
            quiet_button(
                ctx.id("expand"),
                if expanded {
                    c::RECORD_COLLAPSE
                } else {
                    c::RECORD_EXPAND
                },
                cx,
            )
            .on_click(move |_, _, cx| form.update(cx, |f, cx| f.toggle_expanded(cx)))
            .into_any_element(),
        )
    } else {
        None
    };
    let line = div()
        .flex()
        .items_center()
        .gap(space::S8)
        .min_h(px(24.))
        .text_size(px(13.))
        .line_height(px(18.))
        .child(if sending {
            ui::motion::spinner().color(k.ink_3).into_any_element()
        } else {
            ui::motion::check_mark(
                ctx.id("record-check"),
                ui::icon(Sym::Check, px(16.), k.ok),
                px(16.),
                fresh,
                cx,
            )
        })
        .child(div().min_w(px(0.)).one_line().text_color(k.ink).child(text))
        .when(mine, |el| {
            el.child(div().flex_none().text_color(k.ink_3).child(fill(
                if sending {
                    c::RECORD_SENDING_TO
                } else {
                    c::RECORD_SENT_TO
                },
                &[("name", &ctx.author)],
            )))
        })
        .child(
            div()
                .ml_auto()
                .flex_none()
                .flex()
                .items_center()
                .gap(space::S8)
                .when(!mine, |el| {
                    el.when_some(at, |el, t| {
                        el.child(
                            meta(div())
                                .mono()
                                .text_color(k.ink_3)
                                .child(crate::views::shell::clock(t)),
                        )
                    })
                })
                .children(action_el),
        );
    let details = (expanded).then(|| answered_fields(ctx, inputs.as_ref(), cx));
    let body = div()
        .flex()
        .flex_col()
        .gap(space::S8)
        .child(line)
        .children(details);
    // Only an answer made just now rises in; older records are at rest.
    if fresh {
        ui::motion::rise(ctx.id("record-rise"), 0, body, cx)
    } else {
        body.into_any_element()
    }
}

/// The submitted values as key-value rows.
fn answered_fields(
    ctx: &Ctx,
    inputs: Option<&crate::data::cards_v2::AnswerInputs>,
    cx: &App,
) -> AnyElement {
    let mut rows = Vec::new();
    for b in ctx.card.all_blocks() {
        match b {
            CardV2Block::Input { id, label, .. } => {
                let Some(v) = inputs.and_then(|i| i.get(id)) else {
                    continue;
                };
                let text = match v {
                    Value::String(s) => option_label(ctx.card, id, s).unwrap_or_else(|| s.clone()),
                    Value::Array(a) => a
                        .iter()
                        .filter_map(|x| x.as_str())
                        .map(|s| option_label(ctx.card, id, s).unwrap_or_else(|| s.to_string()))
                        .collect::<Vec<_>>()
                        .join(c::FORM_LIST_SEPARATOR),
                    Value::Bool(b) => if *b { "✓" } else { "—" }.to_string(),
                    other => other.to_string(),
                };
                rows.push(CardField {
                    label: label.clone(),
                    value: text,
                });
            }
            CardV2Block::Decision { id, options, .. } => {
                if let Some(Value::String(s)) = inputs.and_then(|i| i.get(id))
                    && let Some((i, o)) = options.iter().enumerate().find(|(_, o)| &o.id == s)
                {
                    rows.push(CardField {
                        label: letter(i).to_string(),
                        value: o.label.clone(),
                    });
                }
            }
            _ => {}
        }
    }
    fields_grid(&rows, cx)
}

/// A small standard push button for an action inside a block (`撤回`,
/// `展开`): NATIVE.md §5.2.
pub(crate) fn quiet_button(id: SharedString, label: &str, cx: &App) -> Button {
    ui::native::push_button(
        id,
        label.to_string(),
        ui::native::PushStyle::Standard,
        ui::native::PushSize::Small,
        cx,
    )
    .flex_none()
}

// ---------------------------------------------------------------------------
// Forms

/// The optional one-line field after a decision of three or more options
/// that the footer draws inline (`补充一句（可选）`): its id and placeholder.
pub(crate) fn footer_note_input(card: &CardV2) -> Option<(String, Option<String>)> {
    if super::quick(card) {
        return None;
    }
    let blocks = card.all_blocks();
    let d = blocks
        .iter()
        .position(|b| matches!(b, CardV2Block::Decision { .. }))?;
    let inputs: Vec<_> = blocks
        .iter()
        .filter(|b| matches!(b, CardV2Block::Input { .. }))
        .collect();
    if inputs.len() != 1 {
        return None;
    }
    blocks[d..].iter().find_map(|b| match b {
        CardV2Block::Input {
            id,
            kind: InputKind::Text,
            required,
            placeholder,
            ..
        } if !required.unwrap_or(false) => Some((id.clone(), placeholder.clone())),
        _ => None,
    })
}

/// A field box: 36 pt (32 in the footer), radius 8, 1 pt `line-2`; `ink-3`
/// while focused or when it is the first empty required field; `bad` when
/// invalid.
/// A standalone text field (NATIVE.md §5.6): 24 pt (taller only for a
/// multi-line area), radius 6, `content` fill, a 1 pt `separator` edge, 7 pt
/// sides, the 3 pt focus ring; `red` edge on an error.
pub(crate) fn field_box(focused: bool, error: bool, height: gpui::Pixels, cx: &App) -> Div {
    let n = ui::native::native_colors(cx);
    let h = if height <= m::OPTION {
        ui::native::metrics::FIELD
    } else {
        height
    };
    div()
        .h(h)
        .w_full()
        .min_w(px(0.))
        .px(ui::native::metrics::FIELD_PAD)
        .rounded(ui::native::metrics::FIELD_RADIUS)
        .bg(n.content)
        .border_1()
        .border_color(if error { n.red } else { n.separator })
        .when(focused && !error, |el| el.shadow(n.focus_ring()))
        .flex()
        .items_center()
        .gap(space::S6)
}

pub(crate) fn line_input(
    field: &Field,
    disabled: bool,
    window: &mut Window,
    cx: &mut App,
) -> AnyElement {
    let k = theme::colors(cx);
    match field {
        Field::Line(s) => ui::placeholder_input(
            Input::new(s)
                .appearance(false)
                .disabled(disabled)
                .with_size(gpui_component::Size::Small)
                .px_0()
                .py_0()
                .text_size(px(13.))
                .line_height(px(20.))
                .text_color(k.ink),
            window,
            cx,
        ),
        Field::Area(s) => ui::placeholder_input(
            Textarea::new(s)
                .appearance(false)
                .disabled(disabled)
                .px_0()
                .py_0()
                .h(px(70.))
                .text_size(px(13.))
                .line_height(px(20.))
                .text_color(k.ink),
            window,
            cx,
        ),
    }
}

pub(crate) fn note_field(
    ctx: &Ctx,
    id: &str,
    placeholder: Option<&str>,
    live: bool,
    window: &mut Window,
    cx: &mut App,
) -> AnyElement {
    let field = ctx
        .form
        .update(cx, |f, cx| f.field(id, placeholder, false, window, cx));
    let focused = field.is_focused(window, cx);
    div()
        .flex_1()
        .min_w(px(0.))
        .child(
            field_box(focused, false, m::BUTTON, cx).child(
                div()
                    .flex_1()
                    .min_w(px(0.))
                    .child(line_input(&field, !live, window, cx)),
            ),
        )
        .into_any_element()
}

pub(crate) fn form_grid(
    ctx: &Ctx,
    blocks: &[&CardV2Block],
    window: &mut Window,
    cx: &mut App,
) -> AnyElement {
    let k = theme::colors(cx);
    let problems = super::form::problems(ctx.card, &ctx.values);
    let first_missing = problems.iter().find_map(|p| match p {
        super::form::Problem::Missing(l) if !l.is_empty() => Some(l.clone()),
        _ => None,
    });
    let mut rows: Vec<AnyElement> = Vec::new();
    for b in blocks {
        let CardV2Block::Input {
            id,
            kind,
            label,
            placeholder,
            options,
            ..
        } = b
        else {
            continue;
        };
        let invalid = problems
            .iter()
            .any(|p| matches!(p, super::form::Problem::Invalid(l) if l == label));
        let lead = first_missing.as_deref() == Some(label.as_str());
        let control = input_control(
            ctx,
            id,
            *kind,
            label,
            placeholder.as_deref(),
            options.as_deref().unwrap_or(&[]),
            invalid,
            lead,
            None,
            window,
            cx,
        );
        let top = matches!(kind, InputKind::Chips | InputKind::Textarea);
        rows.push(
            div()
                .flex()
                .when(top, |el| el.items_start())
                .when(!top, |el| el.items_center())
                .gap(space::S8)
                // A Mac form: the label right-aligned before its control,
                // the control as wide as its text (NATIVE.md §5.6).
                .child(
                    div()
                        .n_body()
                        .flex_none()
                        .w(m::FORM_LABEL)
                        .flex()
                        .justify_end()
                        .when(kind == &InputKind::Textarea, |el| el.pt(space::S4))
                        .text_color(k.ink)
                        .child(label.clone()),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w(px(0.))
                        .when(!top, |el| el.max_w(px(280.)))
                        .child(control),
                )
                .into_any_element(),
        );
    }
    div()
        .flex()
        .flex_col()
        .gap(space::S12)
        .children(rows)
        .into_any_element()
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn input_control(
    ctx: &Ctx,
    id: &str,
    kind: InputKind,
    label: &str,
    placeholder: Option<&str>,
    options: &[InputOption],
    invalid: bool,
    lead: bool,
    // v11/pb-cards: a 2.1 block's own say (an input outside the ask takes live events).
    live: Option<bool>,
    window: &mut Window,
    cx: &mut App,
) -> AnyElement {
    let k = theme::colors(cx);
    let live = live.unwrap_or_else(|| ctx.live());
    match kind {
        InputKind::Select => {
            let chosen = ctx.form.read(cx).choice(id).first().cloned();
            let text = chosen
                .as_ref()
                .and_then(|c| options.iter().find(|o| &o.id == c))
                .map(|o| o.label.clone());
            let shown = text.unwrap_or_else(|| {
                placeholder
                    .map(str::to_string)
                    .unwrap_or_else(|| fill(c::FORM_SELECT_PLACEHOLDER, &[("label", label)]))
            });
            // A pop-up button (NATIVE.md §5.3): the chosen option and the
            // `chevron.up.chevron.down` mark; the placeholder in `tertiary`.
            let _ = lead;
            let trigger = ui::native::disabled_push(
                ui::native::popup_button(
                    ctx.id(format!("select-{id}")),
                    shown,
                    ui::native::PushSize::Regular,
                    window,
                    cx,
                ),
                !live,
                cx,
            )
            .w_full();
            if !live {
                return trigger.into_any_element();
            }
            let form = ctx.form.clone();
            let opts: Vec<InputOption> = options.to_vec();
            let block = id.to_string();
            trigger
                .dropdown_menu(move |menu, _, _| {
                    opts.iter().fold(menu, |menu, o| {
                        let (form, block, oid) = (form.clone(), block.clone(), o.id.clone());
                        let checked = chosen.as_deref() == Some(o.id.as_str());
                        menu.item(
                            PopupMenuItem::new(o.label.clone())
                                .checked(checked)
                                .on_click(move |_, _, cx| {
                                    form.update(cx, |f, cx| f.choose(&block, &oid, false, cx))
                                }),
                        )
                    })
                })
                .into_any_element()
        }
        InputKind::Chips => {
            let chosen = ctx.form.read(cx).choice(id).to_vec();
            let locked: Vec<String> = ctx
                .card
                .all_blocks()
                .iter()
                .find_map(|b| match b {
                    CardV2Block::Input {
                        id: bid,
                        default: Some(Value::Array(a)),
                        ..
                    } if bid == id => Some(
                        a.iter()
                            .filter_map(|x| x.as_str().map(str::to_string))
                            .collect(),
                    ),
                    _ => None,
                })
                .unwrap_or_default();
            div()
                .flex()
                .flex_wrap()
                .gap(space::S8)
                .children(options.iter().map(|o| {
                    let lock = locked.contains(&o.id);
                    let on = chosen.contains(&o.id) && !lock;
                    let (form, block, oid) = (ctx.form.clone(), id.to_string(), o.id.clone());
                    // A checkbox per option (NATIVE.md §5.4); a locked
                    // option shows `lock` and cannot change.
                    div()
                        .id(ctx.id(format!("chip-{id}-{}", o.id)))
                        .flex()
                        .items_center()
                        .gap(space::S4)
                        .child(
                            ui::native::NativeCheckbox::new(
                                ctx.id(format!("chip-{id}-{}-c", o.id)),
                                on || lock,
                            )
                            .label(o.label.clone())
                            .disabled(!live || lock)
                            .on_change(move |_, _, cx| {
                                form.update(cx, |f, cx| f.choose(&block, &oid, true, cx))
                            }),
                        )
                        .when(lock, |el| {
                            el.child(ui::icon(Sym::Lock, px(14.), k.ink_3)).tooltip(
                                |window, cx| {
                                    gpui_component::tooltip::Tooltip::new(c::FORM_LOCKED)
                                        .build(window, cx)
                                },
                            )
                        })
                }))
                .into_any_element()
        }
        InputKind::Switch => {
            let on = ctx.form.read(cx).switch(id);
            let (form, block) = (ctx.form.clone(), id.to_string());
            Switch::new(ctx.id(format!("switch-{id}")))
                .checked(on)
                .disabled(!live)
                .on_click(move |_, _, cx| form.update(cx, |f, cx| f.toggle_switch(&block, cx)))
                .into_any_element()
        }
        InputKind::Textarea => {
            let field = ctx
                .form
                .update(cx, |f, cx| f.field(id, placeholder, true, window, cx));
            let focused = field.is_focused(window, cx);
            field_box(focused || lead, invalid, px(88.), cx)
                .items_start()
                .py(space::S8)
                .child(
                    div()
                        .flex_1()
                        .min_w(px(0.))
                        .child(line_input(&field, !live, window, cx)),
                )
                .into_any_element()
        }
        InputKind::Date
        | InputKind::Number
        | InputKind::Slider
        | InputKind::Text
        | InputKind::Unknown => {
            let ph = match kind {
                InputKind::Date => Some(placeholder.unwrap_or(c::FORM_DATE_PLACEHOLDER)),
                _ => placeholder,
            };
            let field = ctx
                .form
                .update(cx, |f, cx| f.field(id, ph, false, window, cx));
            let focused = field.is_focused(window, cx);
            field_box(focused || lead, invalid, m::OPTION, cx)
                .child(
                    div()
                        .flex_1()
                        .min_w(px(0.))
                        .child(line_input(&field, !live, window, cx)),
                )
                .when(kind == InputKind::Date, |el| {
                    el.child(ui::icon(Sym::Calendar, px(14.), k.ink_3))
                })
                .into_any_element()
        }
    }
}

// ---------------------------------------------------------------------------
// Drafts

/// A limit for a draft's counter from the card's `data.limits` hint.
fn limit(card: &CardV2, id: &str) -> Option<u64> {
    card.data.as_ref()?.get("limits")?.get(id)?.as_u64()
}

/// The draft well (the real object, as it will go out) or, in the edit
/// mode, its fields with counters.
fn draft_well(ctx: &Ctx, blocks: &[&CardV2Block], window: &mut Window, cx: &mut App) -> AnyElement {
    let k = theme::colors(cx);
    let images: Vec<(&str, Option<&str>)> = blocks
        .iter()
        .filter_map(|b| match b {
            CardV2Block::Image { file_id, alt, .. } => Some((file_id.as_str(), alt.as_deref())),
            _ => None,
        })
        .collect();
    let drafts: Vec<(&str, Option<&str>, &str, bool)> = blocks
        .iter()
        .filter_map(|b| match b {
            CardV2Block::Draft {
                id,
                label,
                text,
                editable,
                ..
            } => Some((
                id.as_str(),
                label.as_deref(),
                text.as_str(),
                editable.unwrap_or(false),
            )),
            _ => None,
        })
        .collect();
    let editing = ctx.form.read(cx).editing.is_some() && ctx.live();
    if editing {
        let mut rows: Vec<AnyElement> = Vec::new();
        for (ix, (id, label, text, editable)) in drafts.iter().enumerate() {
            if !editable {
                continue;
            }
            let multi = text.contains('\n') || text.chars().count() > 40;
            let field = ctx
                .form
                .update(cx, |f, cx| f.field(id, None, multi, window, cx));
            let now = field.text(cx).chars().count();
            let counter = match limit(ctx.card, id) {
                Some(l) => format!("{now}/{l}"),
                None => now.to_string(),
            };
            let over = limit(ctx.card, id).is_some_and(|l| now as u64 > l);
            let focused = field.is_focused(window, cx) || ix == 0;
            rows.push(
                div()
                    .flex()
                    .flex_col()
                    .gap(space::S6)
                    .child(
                        div()
                            .flex()
                            .items_baseline()
                            .child(
                                small(div())
                                    .font_weight(theme::MEDIUM_WEIGHT)
                                    .text_color(k.ink)
                                    .child(label.unwrap_or("").to_string()),
                            )
                            .child(
                                meta(div())
                                    .ml_auto()
                                    .mono()
                                    .text_color(if over { k.bad_ink } else { k.ink_3 })
                                    .child(counter),
                            ),
                    )
                    .child(
                        field_box(focused, over, if multi { px(88.) } else { m::OPTION }, cx)
                            .when(multi, |el| el.items_start().py(space::S8))
                            .child(
                                div()
                                    .flex_1()
                                    .min_w(px(0.))
                                    .child(line_input(&field, false, window, cx)),
                            ),
                    )
                    .into_any_element(),
            );
        }
        if !images.is_empty() {
            rows.push(
                meta(div())
                    .flex()
                    .items_center()
                    .gap(space::S8)
                    .text_color(k.ink_3)
                    .child(ui::icon(Sym::Image, px(14.), k.ink_3))
                    .child(fill(c::DRAFT_IMAGES, &[("n", &images.len().to_string())]))
                    .into_any_element(),
            );
        }
        return div()
            .flex()
            .flex_col()
            .gap(space::S12)
            .children(rows)
            .into_any_element();
    }
    // The text that goes out: the confirmed edits once answered.
    let edits = match ctx.state {
        CardState::Answered { answer } => answer.edited_values.clone(),
        _ => None,
    };
    let text_of = |id: &str, text: &str| {
        edits
            .as_ref()
            .and_then(|e| e.get(id).cloned())
            .unwrap_or_else(|| text.to_string())
    };
    let thumbs = (!images.is_empty()).then(|| {
        let tile = |i: usize, h: gpui::Pixels| {
            let (file, _) = images[i.min(images.len() - 1)];
            let path = ctx
                .env
                .provider
                .card_image(&ctx.msg.group_id, &ctx.msg.topic_id, file);
            div()
                .w(px(54.))
                .h(h)
                .rounded(theme::radius::SM)
                .overflow_hidden()
                .bg(k.ph[i % 3])
                .when_some(path, |el, p| {
                    el.child(img(p).size_full().object_fit(ObjectFit::Cover))
                })
        };
        let grid = if images.len() >= 3 {
            div().flex().gap(px(4.)).child(tile(0, px(74.))).child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(4.))
                    .child(tile(1, px(35.)))
                    .child(tile(2, px(35.))),
            )
        } else {
            div()
                .flex()
                .gap(px(4.))
                .children((0..images.len()).map(|i| tile(i, px(74.))))
        };
        grid.flex_none()
    });
    let mut texts = div()
        .flex_1()
        .min_w(px(0.))
        .flex()
        .flex_col()
        .gap(space::S2);
    for (ix, (id, label, text, _)) in drafts.iter().enumerate() {
        let t = text_of(id, text);
        let first = ix == 0 && drafts.len() > 1;
        texts = texts.child(if first {
            div()
                .text_size(px(14.))
                .line_height(px(22.))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(k.ink)
                .child(t)
        } else {
            div()
                .when(drafts.len() == 1, |el| {
                    el.when_some(*label, |el, l| {
                        el.child(
                            meta(div())
                                .font_weight(theme::MEDIUM_WEIGHT)
                                .text_color(k.ink_3)
                                .child(l.to_string()),
                        )
                    })
                })
                .child(small(div()).text_color(k.ink_2).child(t))
        });
    }
    div()
        .flex()
        .items_start()
        .gap(space::S14)
        .p(space::S12)
        .rounded(theme::radius::FIELD)
        .bg(k.sunken)
        .children(thumbs)
        .child(texts)
        .into_any_element()
}

/// A group: a label and its blocks; a group of drafts and images is the
/// draft well.
fn group(
    ctx: &Ctx,
    label: Option<&str>,
    blocks: &[CardV2Block],
    ix: usize,
    window: &mut Window,
    cx: &mut App,
) -> AnyElement {
    let k = theme::colors(cx);
    let refs: Vec<&CardV2Block> = blocks.iter().collect();
    let draft = refs.iter().any(|b| matches!(b, CardV2Block::Draft { .. }))
        && refs
            .iter()
            .all(|b| matches!(b, CardV2Block::Draft { .. } | CardV2Block::Image { .. }));
    if draft {
        return draft_well(ctx, &refs, window, cx);
    }
    div()
        .flex()
        .flex_col()
        .gap(space::S8)
        .when_some(label, |el, l| {
            el.child(
                meta(div())
                    .font_weight(theme::MEDIUM_WEIGHT)
                    .text_color(k.ink_2)
                    .child(l.to_string()),
            )
        })
        .children(
            refs.iter()
                .enumerate()
                .map(|(j, b)| block(ctx, b, ix * 100 + j, window, cx)),
        )
        .into_any_element()
}

// ---------------------------------------------------------------------------
// Plans

fn plan(
    ctx: &Ctx,
    b: &CardV2Block,
    progress: Option<(f64, Option<String>)>,
    window: Option<&Window>,
    cx: &App,
) -> AnyElement {
    let k = theme::colors(cx);
    // 引用提问: each step is a part (REFS §A2.2 `step`); 问这里 follows its text.
    let parts = window.and_then(|w| block_parts(ctx, b, cx).map(|p| (p, w)));
    let CardV2Block::Plan { items, .. } = b else {
        return div().into_any_element();
    };
    let items: &[RunPlanItem] = items.as_deref().unwrap_or(&[]);
    let waiting_on = ctx
        .card
        .ask
        .as_ref()
        .and_then(|a| a.from.first().cloned())
        .map(|id| ctx.name(&id))
        .or_else(|| ctx.me.as_ref().map(|me| ctx.name(me)))
        .unwrap_or_default();
    let mut current_seen = false;
    div()
        .flex()
        .flex_col()
        .children(items.iter().enumerate().map(|(i, it)| {
            let pick = Pick::Step(i);
            let ask = parts.as_ref().and_then(|(p, _)| p.button(pick, cx));
            let current = it.state == RunPlanState::Current && !current_seen;
            if current {
                current_seen = true;
            }
            let glyph: AnyElement = match it.state {
                RunPlanState::Done => ui::icon(Sym::Check, px(14.), k.ink_3).into_any_element(),
                RunPlanState::Current => ui::motion::breathe(
                    ctx.id(format!("plan-dot-{i}")),
                    div().size(px(8.)).rounded_full().bg(k.live),
                    cx,
                ),
                _ => div()
                    .size(px(9.))
                    .rounded_full()
                    .border(px(1.5))
                    .border_color(if it.state == RunPlanState::Waiting {
                        k.ink_2
                    } else {
                        k.ink_3
                    })
                    .into_any_element(),
            };
            let (text_color, weight) = match it.state {
                RunPlanState::Done => (k.ink_2, FontWeight::NORMAL),
                RunPlanState::Current => (k.ink, theme::MEDIUM_WEIGHT),
                RunPlanState::Waiting => (k.ink, FontWeight::NORMAL),
                RunPlanState::Pending => (k.ink_3, FontWeight::NORMAL),
            };
            let right: Option<AnyElement> = match it.state {
                RunPlanState::Waiting => Some(
                    small(div())
                        .font_weight(theme::MEDIUM_WEIGHT)
                        .text_color(k.ink)
                        .child(c::PLAN_SEE_QUESTION)
                        .into_any_element(),
                ),
                RunPlanState::Current => progress.as_ref().and_then(|(_, l)| {
                    l.clone().map(|l| {
                        meta(div())
                            .mono()
                            .text_color(k.ink_3)
                            .child(l)
                            .into_any_element()
                    })
                }),
                _ => None,
            };
            let bar = progress
                .as_ref()
                .filter(|_| current)
                .map(|(v, l)| progress_bar(*v, None, segments(l.as_deref()), cx));
            let row = small(div())
                .flex()
                .items_center()
                .gap(space::S10)
                .min_h(m::PLAN_ROW)
                .child(
                    div()
                        .flex_none()
                        .w(px(16.))
                        .flex()
                        .justify_center()
                        .child(glyph),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w(px(0.))
                        .flex()
                        .overflow_hidden()
                        .child(
                            div()
                                .flex_none()
                                .text_color(text_color)
                                .font_weight(weight)
                                .child(it.text.clone()),
                        )
                        .when(it.state == RunPlanState::Waiting, |el| {
                            el.child(
                                div()
                                    .min_w(px(0.))
                                    .one_line()
                                    .text_color(k.ink_2)
                                    .child(fill(c::PLAN_WAITS_ON, &[("name", &waiting_on)])),
                            )
                        })
                        .children(ask.map(|a| div().flex_none().ml(space::S8).child(a))),
                )
                .children(right);
            let step = div()
                .id(SharedString::from(format!("plan-step-{i}")))
                .when(parts.is_some(), |el| el.mx(-space::S8).px(space::S8))
                .when(current && bar.is_none(), |el| {
                    el.mx(-space::S8)
                        .px(space::S8)
                        .rounded(theme::radius::SM)
                        .bg(k.sunken)
                })
                .child(row)
                .when_some(bar, |el, bar| {
                    el.child(div().pl(px(26.)).pb(space::S6).child(bar))
                });
            match &parts {
                Some((p, w)) => p.wrap_own(pick, step, w, cx),
                None => step.into_any_element(),
            }
        }))
        .into_any_element()
}

// ---------------------------------------------------------------------------
// Data

pub(crate) fn fmt_number(n: f64) -> String {
    if n.fract() == 0. && n.abs() < 1e15 {
        let s = format!("{}", n.abs() as i64);
        let mut out = String::new();
        for (i, ch) in s.chars().enumerate() {
            if i > 0 && (s.len() - i) % 3 == 0 {
                out.push(',');
            }
            out.push(ch);
        }
        if n < 0. { format!("−{out}") } else { out }
    } else {
        let s = format!("{n:.2}");
        let s = s.trim_end_matches('0').trim_end_matches('.').to_string();
        s.replace('-', "−")
    }
}

pub(crate) fn metrics(blocks: &[&CardV2Block], cx: &App) -> AnyElement {
    metrics_with(blocks, None, None, cx)
}

/// [`metrics`] with the 引用提问 part of each metric (REFS §A2.2 `metric`).
pub(crate) fn metrics_with(
    blocks: &[&CardV2Block],
    parts: Option<&CardParts>,
    window: Option<&Window>,
    cx: &App,
) -> AnyElement {
    let k = theme::colors(cx);
    div()
        .flex()
        .gap(space::S16)
        .children(blocks.iter().take(4).map(|b| {
            let bp = parts.filter(|_| window.is_some()).and_then(|p| p.block(b));
            let CardV2Block::Metric {
                label,
                value,
                unit,
                delta,
                better,
                note,
                ..
            } = b
            else {
                return div().into_any_element();
            };
            let v = value.as_f64().unwrap_or(0.);
            let d = delta.as_ref().and_then(|d| d.as_f64());
            let good = match (d, better) {
                (Some(d), Some(MetricBetter::Up)) => Some(d >= 0.),
                (Some(d), Some(MetricBetter::Down)) => Some(d <= 0.),
                _ => None,
            };
            let delta_text = d.map(|d| {
                let sign = if d > 0. {
                    "+"
                } else if d < 0. {
                    "−"
                } else {
                    ""
                };
                let mut s = format!("{sign}{}", fmt_number(d.abs()));
                if let Some(u) = unit.as_deref().filter(|u| *u == "%") {
                    s.push_str(u);
                }
                s
            });
            let mut tail = Vec::new();
            if let Some(n) = note {
                tail.push(n.clone());
            }
            if matches!(better, Some(MetricBetter::Down)) && d.is_some() {
                tail.push(c::METRIC_LOWER_BETTER.to_string());
            }
            let el = div()
                .id(SharedString::from(format!(
                    "metric-{}",
                    b.id().unwrap_or_default()
                )))
                .flex_1()
                .min_w(px(0.))
                .flex()
                .flex_col()
                .when(bp.is_some(), |el| {
                    el.mx(-space::S8).px(space::S8).my(-space::S4).py(space::S4)
                })
                .child(
                    meta(div())
                        .one_line()
                        .text_color(k.ink_3)
                        .child(label.clone()),
                )
                .child(
                    div()
                        .mt(space::S2)
                        .flex()
                        .items_baseline()
                        .gap(space::S2)
                        .child(
                            div()
                                .text_size(px(16.))
                                .line_height(px(24.))
                                .font_weight(FontWeight::SEMIBOLD)
                                .mono()
                                .text_color(k.ink)
                                .child(fmt_number(v)),
                        )
                        .when_some(unit.clone().filter(|u| u != "%"), |el, u| {
                            el.child(small(div()).text_color(k.ink).child(u))
                        })
                        .when(unit.as_deref() == Some("%") && d.is_none(), |el| {
                            el.child(small(div()).text_color(k.ink).child("%"))
                        }),
                )
                .when(delta_text.is_some() || !tail.is_empty(), |el| {
                    el.child(
                        meta(div())
                            .mt(space::S2)
                            .one_line()
                            .text_color(match good {
                                Some(true) => k.ok_ink,
                                Some(false) => k.bad_ink,
                                None => k.ink_3,
                            })
                            .flex()
                            .when_some(delta_text, |el, t| el.child(div().mono().child(t)))
                            .when(!tail.is_empty(), |el| {
                                let t = tail.join("，");
                                // `% 比 9 月` sticks to the number.
                                let glued = t.starts_with('%');
                                el.child(
                                    div()
                                        .when(!glued, |el| el.ml(space::S4))
                                        .one_line()
                                        .child(t),
                                )
                            }),
                    )
                });
            ref_parts::wrap_opt(bp.as_ref(), Pick::Metric, el, Anchor::Corner, window, cx)
        }))
        .into_any_element()
}

// ---------------------------------------------------------------------------
// Diff

fn diff(
    path: Option<&str>,
    text: &str,
    adds: Option<u64>,
    dels: Option<u64>,
    cx: &App,
) -> AnyElement {
    let k = theme::colors(cx);
    let (mut old, mut new) = (0u32, 0u32);
    let mut lines: Vec<AnyElement> = Vec::new();
    let (mut counted_a, mut counted_d) = (0u64, 0u64);
    for raw in text.lines() {
        if raw.starts_with("+++")
            || raw.starts_with("---")
            || raw.starts_with("diff ")
            || raw.starts_with("index ")
        {
            continue;
        }
        if let Some(rest) = raw.strip_prefix("@@") {
            // @@ -42,7 +42,10 @@ context
            let mut it = rest.split_whitespace();
            if let Some(o) = it.next() {
                old = o
                    .trim_start_matches('-')
                    .split(',')
                    .next()
                    .and_then(|n| n.parse().ok())
                    .unwrap_or(1);
            }
            if let Some(n) = it.next() {
                new = n
                    .trim_start_matches('+')
                    .split(',')
                    .next()
                    .and_then(|n| n.parse().ok())
                    .unwrap_or(1);
            }
            lines.push(
                div()
                    .h(m::DIFF_LINE + px(4.))
                    .px(space::S12)
                    .flex()
                    .items_center()
                    .text_color(k.ink_3)
                    .one_line()
                    .child(raw.to_string())
                    .into_any_element(),
            );
            continue;
        }
        let (sign, body) = raw.split_at(raw.chars().next().map(char::len_utf8).unwrap_or(0));
        let (o, n, bg, sc) = match sign {
            "-" => {
                counted_d += 1;
                old += 1;
                (
                    Some(old - 1),
                    None,
                    Some(mix(k.bad, k.bg, m::DIFF_FILL)),
                    k.bad_ink,
                )
            }
            "+" => {
                counted_a += 1;
                new += 1;
                (
                    None,
                    Some(new - 1),
                    Some(mix(k.ok, k.bg, m::DIFF_FILL)),
                    k.ok_ink,
                )
            }
            _ => {
                old += 1;
                new += 1;
                (Some(old - 1), Some(new - 1), None, k.ink_3)
            }
        };
        let num = |n: Option<u32>| {
            div()
                .flex_none()
                .w(m::DIFF_NUMBER)
                .text_color(k.ink_3)
                .flex()
                .justify_end()
                .child(n.map(|n| n.to_string()).unwrap_or_default())
        };
        lines.push(
            div()
                .h(m::DIFF_LINE)
                .flex()
                .items_center()
                .when_some(bg, |el, bg| el.bg(bg))
                .child(num(o))
                .child(num(n))
                .child(
                    div()
                        .flex_none()
                        .w(px(28.))
                        .flex()
                        .justify_center()
                        .text_color(sc)
                        .child(if sign == " " || sign.is_empty() {
                            String::new()
                        } else {
                            sign.replace('-', "−")
                        }),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w(px(0.))
                        .whitespace_nowrap()
                        .overflow_hidden()
                        .text_color(k.ink)
                        .child(body.replace('\t', "    ")),
                )
                .into_any_element(),
        );
    }
    let adds = adds.unwrap_or(counted_a);
    let dels = dels.unwrap_or(counted_d);
    // A soft `sunken` well, no outline: the card keeps the only edge.
    div()
        .rounded(theme::radius::MD)
        .bg(k.sunken)
        .overflow_hidden()
        .flex()
        .flex_col()
        .child(
            div()
                .h(px(36.))
                .px(space::S12)
                .flex()
                .items_center()
                .gap(space::S8)
                .border_b_1()
                .border_color(k.line_2)
                .text_size(px(13.))
                .code_font()
                .child(
                    div()
                        .flex_1()
                        .min_w(px(0.))
                        .one_line()
                        .text_color(k.ink)
                        .child(path.unwrap_or("").to_string()),
                )
                .child(div().text_color(k.ok_ink).child(format!("+{adds}")))
                .child(div().text_color(k.bad_ink).child(format!("−{dels}"))),
        )
        .child(
            div()
                .py(space::S4)
                .text_size(px(12.))
                .line_height(m::DIFF_LINE)
                .code_font()
                .flex()
                .flex_col()
                .children(lines),
        )
        .into_any_element()
}

// ---------------------------------------------------------------------------
// Evidence

/// A proof fact (NATIVE.md rule 4: no chips): the state symbol and the
/// label as a `secondary` Callout run; a proof with a link opens it.
pub(crate) fn proof_chip(
    id: SharedString,
    glyph: Option<(Sym, Hsla)>,
    label: String,
    url: Option<String>,
    cx: &App,
) -> AnyElement {
    let n = ui::native::native_colors(cx);
    let has_url = url.is_some();
    div()
        .id(id)
        .flex_none()
        .flex()
        .items_center()
        .gap(space::S4)
        .n_callout()
        .n_figures()
        .text_color(if has_url { n.label } else { n.secondary })
        .when_some(glyph, |el, (g, color)| {
            // Done reads quietly in a row: `checkmark` in `green`.
            el.child(ui::icon(g, px(14.), color))
        })
        .child(div().whitespace_nowrap().child(label))
        .when_some(url, |el, url| {
            el.cursor_pointer()
                .on_click(move |_, _, cx| cx.open_url(&url))
        })
        .into_any_element()
}

fn evidence(ctx: &Ctx, blocks: &[&CardV2Block], window: &mut Window, cx: &mut App) -> AnyElement {
    let k = theme::colors(cx);
    // 引用提问: each proof is a part (REFS §A2.2 evidence `image` / `item`).
    let parts = CardParts::of(ctx, cx);
    let part = |b: &CardV2Block| parts.as_ref().and_then(|p| p.block(b));
    let window: &Window = window;
    let mut thumbs: Vec<(String, Option<String>, String, Option<BlockParts>)> = Vec::new();
    let mut lines: Vec<AnyElement> = Vec::new();
    let mut chips: Vec<AnyElement> = Vec::new();
    for (i, b) in blocks.iter().enumerate() {
        let CardV2Block::Evidence {
            label,
            state,
            detail,
            file_id,
            url,
            ..
        } = b
        else {
            continue;
        };
        if let Some(f) = file_id {
            thumbs.push((f.clone(), detail.clone(), label.clone(), part(b)));
            continue;
        }
        let bp = part(b);
        let glyph = match state {
            Some(EvidenceState::Pass) => Some((Sym::Check, k.ok)),
            Some(EvidenceState::Fail) => Some((Sym::Close, k.bad)),
            _ => None,
        };
        // A result with a sentence of detail reads as a line; a bare count
        // as a chip.
        if let Some(d) = detail.as_ref().filter(|d| d.chars().count() > 8) {
            let line = div()
                .id(ctx.id(format!("proof-line-{i}")))
                .flex()
                .flex_col()
                .gap(space::S2)
                .child(
                    small(div())
                        .flex()
                        .items_center()
                        .gap(space::S8)
                        .text_color(k.ink)
                        .when_some(glyph, |el, (g, c)| el.child(ui::icon(g, px(14.), c)))
                        .child(label.clone()),
                )
                .child(meta(div()).text_color(k.ink_3).child(d.clone()));
            lines.push(ref_parts::wrap_opt(
                bp.as_ref(),
                Pick::Evidence,
                line,
                Anchor::Corner,
                Some(window),
                cx,
            ));
        } else {
            let text = match detail {
                Some(d) => format!("{label} {d}"),
                None => label.clone(),
            };
            let chip = proof_chip(ctx.id(format!("proof-{i}")), glyph, text, url.clone(), cx);
            chips.push(ref_parts::wrap_opt(
                bp.as_ref(),
                Pick::Evidence,
                div().id(ctx.id(format!("proof-part-{i}"))).child(chip),
                Anchor::TopRight,
                Some(window),
                cx,
            ));
        }
    }
    let shown = thumbs.len().min(3);
    let more = thumbs.len().saturating_sub(3);
    let thumb_row = (!thumbs.is_empty()).then(|| {
        div()
            .flex()
            .gap(space::S8)
            .children(thumbs.iter().take(shown).map(|(file, detail, label, bp)| {
                let path = ctx
                    .env
                    .provider
                    .card_image(&ctx.msg.group_id, &ctx.msg.topic_id, file);
                let thumb = div()
                    .id(ctx.id(format!("proof-shot-{file}")))
                    .flex_1()
                    .min_w(px(0.))
                    .h(m::THUMB)
                    .rounded(theme::radius::MD)
                    .border_1()
                    .border_color(k.line)
                    .bg(k.bg)
                    .overflow_hidden()
                    .map(|el| match path {
                        Some(p) => el.child(img(p).size_full().object_fit(ObjectFit::Cover)),
                        None => el
                            .p(space::S8)
                            .flex()
                            .flex_col()
                            .gap(px(3.))
                            .child(
                                div()
                                    .text_size(px(10.))
                                    .line_height(px(14.))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(k.ink)
                                    .pb(px(3.))
                                    .border_b_1()
                                    .border_color(k.line)
                                    .child(label.clone()),
                            )
                            .children(detail.iter().flat_map(|d| d.lines()).take(3).map(|l| {
                                div()
                                    .text_size(px(10.))
                                    .line_height(px(15.))
                                    .text_color(k.ink_3)
                                    .one_line()
                                    .child(l.to_string())
                            })),
                    });
                // The thumbnail clips, so the part wraps it: 问这里 sits inside its corner.
                ref_parts::wrap_opt(
                    bp.as_ref(),
                    Pick::Evidence,
                    div()
                        .id(ctx.id(format!("proof-shot-part-{file}")))
                        .flex_1()
                        .min_w(px(0.))
                        .child(thumb),
                    Anchor::Corner,
                    Some(window),
                    cx,
                )
            }))
            .when(more > 0, |el| {
                el.child(
                    div()
                        .flex_1()
                        .h(m::THUMB)
                        .rounded(theme::radius::MD)
                        .border_1()
                        .border_color(k.line)
                        .bg(k.raised)
                        .flex()
                        .items_center()
                        .justify_center()
                        .text_size(px(13.))
                        .text_color(k.ink_2)
                        .child(fill(c::EVIDENCE_MORE, &[("n", &more.to_string())])),
                )
            })
    });
    div()
        .flex()
        .flex_col()
        .gap(space::S10)
        .children(thumb_row)
        .when(!lines.is_empty(), |el| {
            el.child(div().flex().flex_col().gap(space::S8).children(lines))
        })
        .when(!chips.is_empty(), |el| {
            el.child(div().flex().flex_wrap().gap(space::S6).children(chips))
        })
        .into_any_element()
}

/// `这次没有附证据` + `让 X 补上`: a result of work (a done card with no data
/// blocks, or a change to review) that carries no evidence. Neutral, never
/// red.
fn no_evidence(ctx: &Ctx, cx: &App) -> Option<AnyElement> {
    let blocks = ctx.card.all_blocks();
    let any_evidence = blocks
        .iter()
        .any(|b| matches!(b, CardV2Block::Evidence { .. }));
    // Proof of testing: a screenshot, a recording, a link or a result line.
    let proof = blocks.iter().any(|b| {
        matches!(
            b,
            CardV2Block::Evidence {
                file_id: Some(_),
                ..
            } | CardV2Block::Evidence { url: Some(_), .. }
                | CardV2Block::Evidence {
                    artifact: Some(_),
                    ..
                }
                | CardV2Block::Evidence {
                    detail: Some(_),
                    ..
                }
        )
    });
    let prs = blocks
        .iter()
        .filter(|b| {
            matches!(
                b,
                CardV2Block::Ref {
                    target: RefTarget::Pr { .. },
                    ..
                }
            )
        })
        .count();
    // A change to review that carries no proof.
    let change = ctx.card.intent == CardIntent::Ask
        && prs == 1
        && !proof
        && !blocks.iter().any(|b| matches!(b, CardV2Block::Diff { .. }));
    let data = blocks.iter().any(|b| {
        matches!(
            b,
            CardV2Block::Metric { .. }
                | CardV2Block::Chart { .. }
                | CardV2Block::Table { .. }
                | CardV2Block::Plan { .. }
                | CardV2Block::Ref { .. }
                | CardV2Block::Diff { .. }
                | CardV2Block::Draft { .. }
                | CardV2Block::Image { .. }
                | CardV2Block::Fields { .. }
        )
    });
    let done = ctx.card.intent == CardIntent::Done && !data && !any_evidence;
    if !(done || change) || matches!(ctx.state, CardState::Superseded | CardState::Expired) {
        return None;
    }
    let k = theme::colors(cx);
    let a = ctx.answerer();
    let (author, name) = (ctx.msg.author_id.clone(), ctx.author.clone());
    Some(
        small(div())
            .flex()
            .items_center()
            .gap(space::S8)
            .h(px(40.))
            .pl(space::S12)
            .pr(space::S4)
            .rounded(theme::radius::MD)
            .bg(k.sunken)
            .text_color(k.ink_2)
            .child(ui::icon(Sym::Info, px(14.), k.ink_3))
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.))
                    .one_line()
                    .child(c::EVIDENCE_NONE),
            )
            .child(
                super::button(
                    ctx.id("ask-evidence"),
                    fill(c::EVIDENCE_ASK, &[("name", &ctx.author)]),
                    ui::ButtonKind::Secondary,
                    cx,
                )
                .h(px(30.))
                .on_click(move |_, _, _| a.mention(&author, &name, c::EVIDENCE_ASK_MESSAGE)),
            )
            .into_any_element(),
    )
}

// ---------------------------------------------------------------------------
// Refs

fn ref_tile(glyph: Sym, dashed: bool, cx: &App) -> AnyElement {
    let k = theme::colors(cx);
    div()
        .flex_none()
        .size(px(20.))
        .flex()
        .items_center()
        .justify_center()
        // A plain `secondary` symbol, no tile (NATIVE.md §6); a missing
        // target reads `tertiary`.
        .child(ui::icon(
            glyph,
            px(18.),
            if dashed { k.ink_3 } else { k.ink_2 },
        ))
        .into_any_element()
}

/// How a PR ref's state reads: its icon and color.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PrTone {
    Merged,
    /// Waits for the viewer's review.
    Waits,
    /// A check failed.
    Bad,
    /// Checks or a merge are running.
    Running,
    Plain,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct PrState {
    text: String,
    tone: PrTone,
}

/// The shown text and tone of a PR ref state. The protocol values (`open`,
/// `draft`, `merged`, `closed`) show in Chinese; other text from the bot
/// shows unchanged. Tones come from exact words, never from a substring, so
/// 未合并 and 合并中 do not read as merged.
fn pr_state(raw: &str) -> PrState {
    use crate::copy::{common, group_page, topic_rail as rail};
    let raw = raw.trim();
    let (text, tone) = match raw {
        "open" => (rail::CHANGES_OPEN, PrTone::Plain),
        "draft" => (c::REF_PR_DRAFT, PrTone::Plain),
        "merged" => (rail::CHANGES_MERGED, PrTone::Merged),
        "closed" => (rail::CHANGES_CLOSED, PrTone::Plain),
        "failed" => (c::STRIP_FAILED, PrTone::Bad),
        _ => {
            // A free-text state such as "已合并 11-02" or
            // "3 项检查通过 · 未合并": look at each part and its first word.
            let tone = raw
                .split('·')
                .map(str::trim)
                .map(|part| {
                    let head = part.split_whitespace().next().unwrap_or("");
                    let last = part.split_whitespace().last().unwrap_or("");
                    if head == rail::CHANGES_MERGED {
                        PrTone::Merged
                    } else if part == group_page::GROUP_PAGE_NEEDS_REVIEW
                        || part == common::STATUS_WAITING_REVIEW_ME
                    {
                        PrTone::Waits
                    } else if part.ends_with(c::STRIP_FAILED) {
                        PrTone::Bad
                    } else if part == rail::CHANGES_MERGING || is_count(last) {
                        PrTone::Running
                    } else {
                        PrTone::Plain
                    }
                })
                // The strongest tone of the parts wins.
                .min_by_key(|t| match t {
                    PrTone::Bad => 0,
                    PrTone::Waits => 1,
                    PrTone::Running => 2,
                    PrTone::Merged => 3,
                    PrTone::Plain => 4,
                })
                .unwrap_or(PrTone::Plain);
            return PrState {
                text: raw.to_string(),
                tone,
            };
        }
    };
    PrState {
        text: text.to_string(),
        tone,
    }
}

/// `2/3`: checks done of all.
fn is_count(word: &str) -> bool {
    word.split_once('/').is_some_and(|(a, b)| {
        !a.is_empty()
            && !b.is_empty()
            && a.bytes().all(|x| x.is_ascii_digit())
            && b.bytes().all(|x| x.is_ascii_digit())
    })
}

fn refs(ctx: &Ctx, blocks: &[&CardV2Block], cx: &App) -> AnyElement {
    let k = theme::colors(cx);
    let p = &ctx.env.provider;
    let rows = blocks.iter().enumerate().filter_map(|(i, b)| {
        let CardV2Block::Ref { label, target, .. } = b else {
            return None;
        };
        let (lead, name, line, right, right_color): (AnyElement, String, String, Option<String>, Hsla) =
            match target {
                RefTarget::Pr { state, number, repo, .. } => {
                    let state = state.as_deref().map(pr_state);
                    let tone = state.as_ref().map_or(PrTone::Plain, |s| s.tone);
                    (
                        ref_tile(
                            if tone == PrTone::Merged { Sym::Merge } else { Sym::PullRequest },
                            false,
                            cx,
                        ),
                        match number {
                            Some(n) if !label.contains(&format!("#{n}")) => format!("#{n} {label}"),
                            _ => label.clone(),
                        },
                        repo.clone().unwrap_or_default(),
                        state.map(|s| s.text),
                        match tone {
                            PrTone::Waits => k.you_ink,
                            PrTone::Bad => k.bad_ink,
                            PrTone::Running => k.live_ink,
                            PrTone::Merged | PrTone::Plain => k.ink_2,
                        },
                    )
                }
                RefTarget::Artifact { artifact_id, version } => match p.artifact(artifact_id) {
                    Some(a) => {
                        let author = ctx.name(&a.summary.created_by);
                        let shown = version.unwrap_or(a.summary.version);
                        let newer = a.summary.version > shown;
                        (
                            ref_tile(Sym::Document, false, cx),
                            a.summary.title.clone(),
                            fill(
                                c::REF_ARTIFACT_LINE,
                                &[("version", &shown.to_string()), ("name", &author)],
                            ),
                            newer.then(|| {
                                fill(c::REF_ARTIFACT_NEWER, &[("version", &a.summary.version.to_string())])
                            }),
                            k.ink,
                        )
                    }
                    None => (
                        ref_tile(Sym::MissingFile, true, cx),
                        c::REF_ARTIFACT_GONE.to_string(),
                        fill(c::REF_ARTIFACT_GONE_DETAIL, &[("title", label)]),
                        None,
                        k.ink_3,
                    ),
                },
                RefTarget::Topic { group_id, topic_id } => {
                    let topic = p.topic(group_id, topic_id);
                    let group = p
                        .groups()
                        .into_iter()
                        .find(|g| &g.id == group_id)
                        .map(|g| g.name)
                        .unwrap_or_default();
                    let status = topic
                        .as_ref()
                        .map(|t| crate::views::topic_menu::status_label(t.status).to_string())
                        .unwrap_or_default();
                    let run = p
                        .runs(group_id, topic_id)
                        .into_iter()
                        .max_by_key(|r| r.updated_at)
                        .filter(|r| !r.plan.is_empty() && topic.as_ref().is_some_and(|t| t.status != TopicStatus::Done))
                        .map(|r| {
                            let done = r.plan.iter().filter(|x| x.state == crate::data::PlanState::Done).count();
                            format!("{}/{}", (done + 1).min(r.plan.len()), r.plan.len())
                        });
                    (
                        ref_tile(Sym::List, false, cx),
                        topic.map(|t| t.title).unwrap_or_else(|| label.clone()),
                        fill(c::REF_TOPIC_LINE, &[("group", &group), ("status", &status)]),
                        run,
                        k.ink_2,
                    )
                }
                RefTarget::Member { member_id } => {
                    let member = p.member(member_id);
                    (
                        ui::MemberAvatar::new(member.as_ref(), px(20.)).into_any_element(),
                        member.as_ref().map(|m| m.name.clone()).unwrap_or_else(|| label.clone()),
                        member.and_then(|m| m.role).unwrap_or_default(),
                        None,
                        k.ink_2,
                    )
                }
                RefTarget::Url { url } => (
                    ref_tile(Sym::Link, false, cx),
                    label.clone(),
                    url.clone(),
                    None,
                    k.ink_2,
                ),
                RefTarget::Unknown => (
                    ref_tile(Sym::Link, false, cx),
                    label.clone(),
                    String::new(),
                    None,
                    k.ink_2,
                ),
            };
        let a = ctx.answerer();
        let t = target.clone();
        let gone = matches!(target, RefTarget::Artifact { artifact_id, .. } if p.artifact(artifact_id).is_none());
        Some(
            div()
                .id(ctx.id(format!("ref-{i}")))
                .flex()
                .items_center()
                .gap(space::S12)
                .min_h(px(44.))
                .py(space::S8)
                .when(!gone, |el| {
                    el
                        .on_click(move |_, window, cx| a.open(&t, window, cx))
                })
                .child(lead)
                .child(
                    div()
                        .flex_1()
                        .min_w(px(0.))
                        .flex()
                        .flex_col()
                        .child(
                            small(div())
                                .one_line()
                                .font_weight(theme::MEDIUM_WEIGHT)
                                .text_color(if gone { k.ink_2 } else { k.ink })
                                .child(name),
                        )
                        .when(!line.is_empty(), |el| {
                            el.child(meta(div()).one_line().text_color(k.ink_3).child(line))
                        }),
                )
                .when_some(right, |el, r| {
                    el.child(
                        meta(div())
                            .flex_none()
                            .font_weight(theme::MEDIUM_WEIGHT)
                            .text_color(right_color)
                            .child(r),
                    )
                }),
        )
    });
    // No box of its own inside the card: rows divided by a hairline, their
    // hover reaching past the text (one container level, §7.10).
    ui::divided(
        rows.map(|r| ui::inset_row(r).into_any_element()),
        theme::LIST_ROW_BLEED,
        cx,
    )
    .mx(-theme::LIST_ROW_BLEED)
    .into_any_element()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::copy::topic_rail as rail;

    #[test]
    fn protocol_pr_states_show_in_chinese() {
        assert_eq!(pr_state("open").text, rail::CHANGES_OPEN);
        assert_eq!(pr_state("draft").text, c::REF_PR_DRAFT);
        assert_eq!(pr_state("closed").text, rail::CHANGES_CLOSED);
        let merged = pr_state("merged");
        assert_eq!(
            (merged.text.as_str(), merged.tone),
            (rail::CHANGES_MERGED, PrTone::Merged)
        );
        assert_eq!(pr_state("open").tone, PrTone::Plain);
    }

    #[test]
    fn pr_tones_match_exact_words_only() {
        assert_eq!(pr_state("未合并").tone, PrTone::Plain);
        assert_eq!(pr_state("合并中").tone, PrTone::Running);
        assert_eq!(pr_state("3 项检查通过 · 未合并").tone, PrTone::Plain);
        assert_eq!(pr_state("未合并 · 等你审").tone, PrTone::Waits);
        assert_eq!(pr_state("已合并").tone, PrTone::Merged);
        assert_eq!(pr_state("已合并 11-02").tone, PrTone::Merged);
        assert_eq!(pr_state("需要你审").tone, PrTone::Waits);
        assert_eq!(pr_state("1 项检查没过").tone, PrTone::Bad);
        assert_eq!(pr_state("检查 2/3").tone, PrTone::Running);
        assert_eq!(pr_state("你好").tone, PrTone::Plain);
        // Free text shows unchanged.
        assert_eq!(pr_state("已合并 11-02").text, "已合并 11-02");
        assert_eq!(pr_state("reopened").text, "reopened");
    }
}
