//! One renderer per Card v2 block (DESIGN.md §7.10 "Blocks"). Runs of the
//! same kind draw together: metrics share a row, evidence shares a strip of
//! thumbnails and chips, refs share one list box, inputs form one grid.

use gpui::{
    AnyElement, App, Div, FontWeight, Hsla, InteractiveElement as _, IntoElement, ObjectFit,
    ParentElement, SharedString, StatefulInteractiveElement as _, Styled, StyledImage as _, Window,
    div, img, prelude::FluentBuilder as _, px,
};
use gpui_component::{
    Disableable as _, Sizable as _,
    button::Button,
    input::{Input, Textarea},
    menu::{DropdownMenu as _, PopupMenuItem},
    switch::Switch,
};
use serde_json::Value;

use super::{Ctx, choice_text, form::Field, meta, option_label, state::CardState};
use crate::copy::cards21 as c21;
use crate::ui::native::FocusRow as _;
use crate::views::cards::v21::interact;
use crate::{
    copy::{cards_v2 as c, fill},
    data::{
        card::{CardField, CardListItem, CardListStatus, LinkTarget},
        cards_v2::{
            CardIntent, CardV2, CardV2Block, DecisionOption, EvidenceState, InputKind, InputOption,
            MetricBetter, MetricTone, RefTarget, RunPlanItem, RunPlanState,
        },
    },
    theme::{self, Type as _, card_v2 as m, card_v2::mix, space},
    ui::{
        self, Sym,
        native::{BadgeState, NativeType as _},
    },
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
    el.text_size(crate::ui::native::tokens::text::UI.0)
        .line_height(px(20.))
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
    // A done card leads with what it delivered: its 产出 first, its passed checks folded
    // into one line at the end (the steps are how, not what).
    let done = ctx.card.intent == CardIntent::Done;
    let (blocks, folded) = if done {
        done_order(blocks)
    } else {
        (blocks, Vec::new())
    };
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
                // A row holds four metrics; a longer run goes on in the next row.
                let end = run_end(&|x| matches!(x, CardV2Block::Metric { .. }));
                let j = metric_row_end(i, end);
                let parts = CardParts::of(ctx, cx);
                // A run over one row keeps four columns in every row, so the numbers of
                // its last, shorter row stand under the ones above.
                let start = (0..i)
                    .rev()
                    .take_while(|p| matches!(blocks[*p], CardV2Block::Metric { .. }))
                    .last()
                    .unwrap_or(i);
                let columns = if end - start > m::METRICS_PER_ROW {
                    m::METRICS_PER_ROW
                } else {
                    j - i
                };
                (
                    Some(metrics_row(
                        &blocks[i..j],
                        columns,
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
            CardV2Block::Ref { .. } if done && is_output(b) => {
                let j = run_end(&is_output);
                (Some(outputs(ctx, &blocks[i..j], cx)), j, space::S12)
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
            let el = copyable_run(ctx, &blocks[i..next], el, cx);
            out.push(div().mt(gap).min_w(px(0.)).child(el).into_any_element());
        }
        prev = Some(b);
        i = next;
    }
    if !folded.is_empty() {
        out.push(
            div()
                .mt(space::S12)
                .min_w(px(0.))
                .child(checks(ctx, &folded, window, cx))
                .into_any_element(),
        );
    }
    if let Some(notice) = no_evidence(ctx, cx) {
        out.push(div().mt(space::S12).child(notice).into_any_element());
    }
    out
}

/// Where the row of metrics that starts at `start` ends, in a run that ends at `run_end`:
/// [`m::METRICS_PER_ROW`] metrics share a row, so no metric of a longer run is dropped.
fn metric_row_end(start: usize, run_end: usize) -> usize {
    run_end.min(start + m::METRICS_PER_ROW)
}

/// The part of an element id that names block `b`: its id, so two blocks never share one
/// (the 2.1 renderer draws every leaf with index 0), else its index in a 2.0 card whose
/// block has no id.
pub(crate) fn block_key(b: &CardV2Block, ix: usize) -> String {
    match b.id().filter(|id| !id.is_empty()) {
        Some(id) => id.to_string(),
        None => ix.to_string(),
    }
}

/// The text of a block as a person would paste it: the Markdown of a text block, a
/// table as tab-separated lines, `label value` lines for fields and metrics, the steps of
/// a list or a plan, a diff or code as it is, a proof with its result. `None` for blocks
/// that are controls or pictures.
pub(crate) fn block_text(b: &CardV2Block) -> Option<String> {
    let lines = |v: Vec<String>| (!v.is_empty()).then(|| v.join("\n"));
    match b {
        CardV2Block::Text { text, .. } | CardV2Block::Code { text, .. } => Some(text.clone()),
        CardV2Block::Diff { text, .. } => Some(text.clone()),
        CardV2Block::Fields { fields, .. } => lines(
            fields
                .iter()
                .map(|f| format!("{}\t{}", f.label, f.value))
                .collect(),
        ),
        CardV2Block::Table { columns, rows, .. } => lines(
            std::iter::once(columns.join("\t"))
                .chain(rows.iter().map(|r| r.join("\t")))
                .collect(),
        ),
        CardV2Block::List { items, .. } => lines(items.iter().map(|i| i.text.clone()).collect()),
        CardV2Block::Plan { items, .. } => {
            lines(items.iter().flatten().map(|i| i.text.clone()).collect())
        }
        CardV2Block::Metric {
            label, value, unit, ..
        } => {
            let v = fmt_number(value.as_f64().unwrap_or(0.));
            Some(match unit.as_deref() {
                Some("%") => format!("{label} {v}%"),
                Some(u) if !u.is_empty() => format!("{label} {v} {u}"),
                _ => format!("{label} {v}"),
            })
        }
        CardV2Block::Chart {
            label, x, series, ..
        } => {
            let xs = x.clone().unwrap_or_default();
            let mut out: Vec<String> = label.iter().cloned().collect();
            out.push(
                std::iter::once(String::new())
                    .chain(xs.iter().cloned())
                    .collect::<Vec<_>>()
                    .join("\t"),
            );
            out.extend(series.iter().map(|s| {
                std::iter::once(s.name.clone())
                    .chain(s.values.iter().map(|v| v.to_string()))
                    .collect::<Vec<_>>()
                    .join("\t")
            }));
            lines(out)
        }
        CardV2Block::Evidence {
            label,
            detail,
            tests,
            url,
            ..
        } => {
            let mut out = vec![match tests {
                Some(t) => tests_text(t),
                None => label.clone(),
            }];
            out.extend(detail.iter().cloned());
            out.extend(url.iter().cloned());
            lines(out)
        }
        _ => None,
    }
}

/// `el` with 复制 in its context menu, which copies `text`: the way to take a number, a
/// table or a diff out of a card where 引用提问 (whose part menus copy too) does not show.
pub(crate) fn copyable(id: SharedString, text: String, el: AnyElement) -> AnyElement {
    use gpui_component::menu::ContextMenuExt as _;
    div()
        .id(id)
        .min_w(px(0.))
        .child(el)
        .context_menu(move |menu, _, _| {
            let text = text.clone();
            menu.item(
                PopupMenuItem::new(crate::copy::refs::COPY).on_click(move |_, _, cx| {
                    cx.write_to_clipboard(gpui::ClipboardItem::new_string(text.clone()))
                }),
            )
        })
        .into_any_element()
}

/// [`copyable`] for the blocks `run` draws together, when the card shows no 引用提问 parts.
pub(crate) fn copyable_run(
    ctx: &Ctx,
    run: &[&CardV2Block],
    el: AnyElement,
    cx: &App,
) -> AnyElement {
    if CardParts::of(ctx, cx).is_some() {
        return el;
    }
    let text: Vec<String> = run.iter().filter_map(|b| block_text(b)).collect();
    let Some(first) = run.first().filter(|_| !text.is_empty()) else {
        return el;
    };
    copyable(
        ctx.id(format!("copy-{}", block_key(first, 0))),
        text.join("\n"),
        el,
    )
}

/// A reference to a 产出.
fn is_output(b: &CardV2Block) -> bool {
    matches!(
        b,
        CardV2Block::Ref {
            target: RefTarget::Artifact { .. },
            ..
        }
    )
}

/// A passed check with no screenshot: folds away in a done card.
fn is_passed_check(b: &CardV2Block) -> bool {
    matches!(
        b,
        CardV2Block::Evidence {
            state: Some(EvidenceState::Pass),
            file_id: None,
            recording: None,
            ..
        }
    )
}

/// The blocks of a done card in reading order: its 产出 first, then the rest as sent; the
/// passed checks come back apart, to fold into one line.
fn done_order<'a>(blocks: Vec<&'a CardV2Block>) -> (Vec<&'a CardV2Block>, Vec<&'a CardV2Block>) {
    let mut first = Vec::new();
    let mut rest = Vec::new();
    let mut folded = Vec::new();
    for b in blocks {
        if is_output(b) {
            first.push(b);
        } else if is_passed_check(b) {
            folded.push(b);
        } else {
            rest.push(b);
        }
    }
    // One passed check reads fine as it is: folding it saves nothing.
    if folded.len() == 1 {
        rest.append(&mut folded);
    }
    first.append(&mut rest);
    (first, folded)
}

/// A done card's passed checks: one quiet line `✓ 已检查 4 项` that opens them.
fn checks(ctx: &Ctx, blocks: &[&CardV2Block], window: &mut Window, cx: &mut App) -> AnyElement {
    let k = theme::colors(cx);
    let open = ctx.form.read(cx).checks_all;
    let form = ctx.form.clone();
    let label = if open {
        c::CHECKS_HIDE.to_string()
    } else {
        fill(c::CHECKS_PASSED, &[("n", &blocks.len().to_string())])
    };
    let toggle = div()
        .id(ctx.id("checks-toggle"))
        .focus_row(false, cx)
        .flex()
        .items_center()
        .gap(space::S6)
        .min_h(px(28.))
        .px(space::S4)
        .mx(-space::S4)
        .rounded(theme::radius::MD)
        .text_color(k.ink_3)
        .child(ui::icon(Sym::Check, px(14.), k.ink_3))
        .child(meta(div()).child(label))
        .child(ui::icon(
            if open {
                Sym::ChevronUp
            } else {
                Sym::ChevronDown
            },
            px(12.),
            k.ink_3,
        ))
        .on_click(move |_, _, cx| form.update(cx, |f, cx| f.toggle_checks_all(cx)));
    div()
        .flex()
        .flex_col()
        .gap(space::S8)
        .child(div().flex().child(toggle))
        .when(open, |el| el.child(evidence(ctx, blocks, window, cx)))
        .into_any_element()
}

/// A done card's 产出 (§2.17 file item, the thread's one recipe): a bordered row with the
/// kind symbol, the title over `网页产出 · v1 · Jojo`, and the open arrow. The whole row
/// opens the 产出 beside the 话题.
fn outputs(ctx: &Ctx, blocks: &[&CardV2Block], cx: &App) -> AnyElement {
    use crate::data::ArtifactKind;
    use crate::ui::native::NativeType as _;
    let n = crate::ui::native::native_colors(cx);
    let p = &ctx.env.provider;
    let rows = blocks.iter().enumerate().filter_map(|(i, b)| {
        let CardV2Block::Ref { label, target, .. } = b else {
            return None;
        };
        let RefTarget::Artifact {
            artifact_id,
            version,
        } = target
        else {
            return None;
        };
        let detail = p.artifact(artifact_id);
        let (icon, title, line) = match &detail {
            Some(a) => {
                let shown = version.unwrap_or(a.summary.version);
                let (kind, icon) = match a.summary.kind {
                    ArtifactKind::Page => (crate::copy::artifacts::CARD_KIND_PAGE, Sym::Document),
                    ArtifactKind::Image => (crate::copy::artifacts::CARD_KIND_FILE, Sym::Image),
                    _ => (crate::copy::artifacts::CARD_KIND_FILE, Sym::File),
                };
                let mut line = fill(
                    c::REF_ARTIFACT_KIND_LINE,
                    &[
                        ("kind", kind),
                        ("version", &shown.to_string()),
                        ("name", &ctx.name(&a.summary.created_by)),
                    ],
                );
                if a.summary.version > shown {
                    line.push_str(" · ");
                    line.push_str(&fill(
                        c::REF_ARTIFACT_NEWER,
                        &[("version", &a.summary.version.to_string())],
                    ));
                }
                (icon, a.summary.title.clone(), line)
            }
            None => (
                Sym::MissingFile,
                c::REF_ARTIFACT_GONE.to_string(),
                fill(c::REF_ARTIFACT_GONE_DETAIL, &[("title", label)]),
            ),
        };
        let gone = detail.is_none();
        let a = ctx.answerer();
        let t = target.clone();
        Some(
            div()
                .id(ctx.id(format!("output-{i}")))
                .w_full()
                .h(crate::views::files::FILE_ROW_H)
                .flex_none()
                .px(space::S12)
                .rounded(crate::ui::native::tokens::radius::CONTAINER)
                .border_1()
                .border_color(n.separator)
                // The row sits in the card: the card's fill, the border
                // is its only edge.
                .bg(n.sheet)
                .flex()
                .items_center()
                .gap(space::S12)
                .when(!gone, |el| {
                    el.focus_row(false, cx)
                        .cursor_pointer()
                        .on_click(move |_, window, cx| a.open(&t, window, cx))
                })
                .child(crate::ui::native::media_box(icon, cx))
                .child(
                    div()
        // The version the pane already shows says so; another version still opens.
        let in_pane = detail.as_ref().is_some_and(|a| {
            crate::views::artifacts::host::version_in_pane(
                artifact_id,
                *version,
                a.summary.version,
                cx,
            )
        });
                        .flex_1()
                        .min_w_0()
                        .flex()
                        .flex_col()
                        .child(
                            crate::ui::truncate::TruncText::new(title)
                                .id(ctx.id(format!("output-name-{i}")))
                                .n_headline()
                                .text_color(if gone { n.secondary } else { n.label }),
                        )
                        .child(
                            crate::ui::truncate::TruncText::new(line)
                                .id(ctx.id(format!("output-line-{i}")))
                                .n_subheadline()
                                .n_figures()
                                .text_color(n.secondary),
                        ),
                )
                .when(in_pane, |el| {
                    el.child(crate::views::artifacts::host::in_pane_label(cx))
                })
                .when(!gone && !in_pane, |el| {
                    el.child(ui::icon(
                        Sym::OpenExternal,
                        theme::icon::HEADER,
                        theme::colors(cx).text_2,
                    ))
                }),
        )
    });
    div()
        .flex()
        .flex_col()
        .gap(space::S8)
        .children(rows)
        .into_any_element()
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
    let key = block_key(b, ix);
    match b {
        // Bots write Markdown here, like in a message: draw it the same way.
        CardV2Block::Text { text, .. } => div()
            .w_full()
            .text_size(crate::ui::native::tokens::text::UI.0)
            .line_height(px(22.))
            .text_color(k.ink_2)
            .child(
                crate::markdown::message_body(ctx.id(format!("text-{key}")), text.clone(), &[])
                    .w_full(),
            )
            .into_any_element(),
        CardV2Block::Fields { fields, .. } => fields_grid(fields, cx),
        CardV2Block::Table { columns, rows, .. } => {
            // 引用提问: cells, rows and columns (REFS §A2.2, R4).
            let refs = block_parts(ctx, b, cx).and_then(|p| p.table_refs());
            let fold = ctx.id(format!("table-{key}"));
            table_with_refs(
                Some(fold.as_ref()),
                &|_| false,
                columns,
                rows,
                refs.as_ref(),
                Some(window),
                cx,
            )
        }
        CardV2Block::List { items, .. } => {
            list_with(items, block_parts(ctx, b, cx).as_ref(), Some(window), cx)
        }
        CardV2Block::Progress { value, label, .. } => {
            progress_bar(value.as_f64().unwrap_or(0.), label.as_deref(), None, cx)
        }
        // The message's own code box (the v1 card draws it the same way): the text can be
        // selected, the language is highlighted and named, and the box copies.
        CardV2Block::Code { text, lang, .. } => crate::markdown::message_body(
            ctx.id(format!("code-{key}")),
            crate::views::cards::blocks::code_fence(lang.as_deref(), text),
            &[],
        )
        .w_full()
        .into_any_element(),
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
        CardV2Block::Divider { .. } => div()
            .h(crate::ui::native::tokens::layout::BORDER)
            .bg(k.line)
            .into_any_element(),
        CardV2Block::Link { label, target, .. } => link(ctx, label, target, &key, cx),
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
                ctx.id(format!("chart-{key}")),
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
        } => diff_folded(
            ctx.id(format!("diff-{key}")),
            Some(ctx.id(format!("diff-fold-{key}")).as_ref()),
            path.as_deref(),
            text,
            *additions,
            *deletions,
            window,
            cx,
        ),
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
                // A DescriptionList value is one line, cut with "…" and a tooltip
                // (§2.5, C-09), never wrapped mid-phrase.
                .child(
                    ui::truncate::TruncText::new(f.value.clone())
                        .flex_1()
                        .min_w(px(0.))
                        .text_color(k.ink),
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
    let est = est_width;
    let head = columns.get(ci).map_or(0., |h| est(h, 12.));
    let cells = rows
        .iter()
        .filter_map(|r| r.get(ci))
        .map(|v| {
            // A share cell: the bar, the gap and the column's one figure slot.
            if percent(v).is_some() {
                f32::from(m::TABLE_PCT_BAR)
                    + f32::from(space::S8)
                    + pct_figure_width(rows, ci).map_or(0., f32::from)
            } else {
                est(v, 14.)
            }
        })
        .fold(0f32, f32::max);
    px(head.max(cells).ceil() + 2.).max(m::TABLE_NUM_COL)
}

/// The estimated width of `t` at `size`: one CJK character is about one em; a tabular
/// digit about 0.62 em, 0.66 em in the semibold total row, so it takes the wider one.
fn est_width(t: &str, size: f32) -> f32 {
    t.chars()
        .map(|c| if c.is_ascii() { 0.66 * size } else { size })
        .sum()
}

/// The slot of the figure beside a share bar in column `ci`: the widest percent of
/// the column, so every bar of the column starts at one x whatever its row's figure.
fn pct_figure_width(rows: &[Vec<String>], ci: usize) -> Option<gpui::Pixels> {
    rows.iter()
        .filter_map(|r| r.get(ci))
        .filter(|v| percent(v).is_some())
        // A little wider than the column estimate: the figure must never reach
        // into the gap after its bar, in the semibold total row too.
        .map(|v| (est_width(v, 14.) * 1.1).ceil())
        .reduce(f32::max)
        .map(px)
}

fn percent(s: &str) -> Option<f32> {
    s.trim()
        .strip_suffix('%')
        .and_then(|n| n.trim().parse::<f32>().ok())
}

pub(crate) fn table(columns: &[String], rows: &[Vec<String>], cx: &mut App) -> AnyElement {
    table_with_refs(None, &|_| false, columns, rows, None, None, cx)
}

/// True when `row` is the total row of a table (`合计` in its first cell).
fn is_total(row: &[String]) -> bool {
    row.first().is_some_and(|x| x.trim() == c::TABLE_TOTAL)
}

/// The rows a table shows, in `order` (indices into `rows`), and how many the fold hides.
/// Folded, it shows the first [`m::TABLE_FOLD_ROWS`], a `合计` row at the end, and every
/// row `keep` names (a picked row never hides). A table one row over the fold shows whole:
/// the 展开 row would take that row's place.
pub(crate) fn table_fold(
    rows: &[Vec<String>],
    order: &[usize],
    expanded: bool,
    keep: &dyn Fn(usize) -> bool,
) -> (Vec<usize>, usize) {
    if expanded || order.len() <= m::TABLE_FOLD_ROWS + 1 {
        return (order.to_vec(), 0);
    }
    let total = order
        .last()
        .copied()
        .filter(|ri| rows.get(*ri).is_some_and(|r| is_total(r)));
    let shown: Vec<usize> = order
        .iter()
        .enumerate()
        .filter(|(pos, ri)| *pos < m::TABLE_FOLD_ROWS || Some(**ri) == total || keep(**ri))
        .map(|(_, ri)| *ri)
        .collect();
    let hidden = order.len() - shown.len();
    (shown, hidden)
}

/// True when a table of `rows` rows folds at all (its 展开 / 收起 row shows).
pub(crate) fn table_folds(rows: usize) -> bool {
    rows > m::TABLE_FOLD_ROWS + 1
}

/// The least width the columns of a table need: each number column its numbers
/// (`widths`), each text column [`m::TABLE_COL_MIN`] ([`m::TABLE_COL_FIRST`] for the
/// first), the gaps between them and the row padding. A card narrower than this scrolls
/// the table sideways instead of cutting every cell to "…".
pub(crate) fn table_min_width(widths: &[Option<gpui::Pixels>]) -> gpui::Pixels {
    let cols: f32 = widths
        .iter()
        .enumerate()
        .map(|(ci, w)| match w {
            Some(w) => f32::from(*w),
            None if ci == 0 => f32::from(m::TABLE_COL_FIRST),
            None => f32::from(m::TABLE_COL_MIN),
        })
        .sum();
    let gaps = f32::from(space::S12) * (widths.len().saturating_sub(1) as f32 + 2.);
    px(cols + gaps)
}

/// The 展开其余 n 行 / 收起 row at the end of a folded table or diff: the fold of the v1
/// table, kept per card element in [`crate::views::cards::Expanded`].
pub(crate) fn fold_row(key: &str, hidden: usize, expanded: bool, cx: &App) -> Div {
    let k = theme::colors(cx);
    div()
        .flex()
        .px(space::S4)
        .py(space::S4)
        .border_t_1()
        .border_color(k.line)
        // 4 + 8: the label starts on the 12 pt inset of the frame's cells.
        .child(crate::views::cards::blocks::more_button(key, hidden, expanded).px(space::S8))
}

/// The 展开其余 n 行 / 收起 button of a list with no cell inset: the label starts on the
/// content edge and the button's hover reaches out past it, as the rows' hover does.
pub(crate) fn flush_more(key: &str, hidden: usize, expanded: bool) -> Div {
    div()
        .flex()
        .ml(-ui::native::metrics::BUTTON_SMALL_PAD)
        .child(crate::views::cards::blocks::more_button(
            key, hidden, expanded,
        ))
}

/// [`fold_row`] of a table whose cells start on the content edge.
pub(crate) fn flush_fold_row(key: &str, hidden: usize, expanded: bool, cx: &App) -> Div {
    let k = theme::colors(cx);
    div()
        .flex()
        .py(space::S4)
        .border_t_1()
        .border_color(k.line)
        .child(flush_more(key, hidden, expanded))
}

/// A wide table inside its frame: `content` keeps at least `min` and scrolls sideways when
/// the card is narrower, with the code box's bottom scrollbar and right fade (D13).
fn scroll_table(
    key: &str,
    min: gpui::Pixels,
    content: Div,
    fill: Hsla,
    window: &mut Window,
    cx: &mut App,
) -> Div {
    let id = gpui::ElementId::Name(format!("{key}-scroll").into());
    let scroll = window
        .use_keyed_state(id.clone(), cx, |_, _| gpui::ScrollHandle::new())
        .read(cx)
        .clone();
    let scrolls = scroll.max_offset().x > px(0.);
    div()
        .relative()
        .w_full()
        .min_w(px(0.))
        .child(
            div()
                .id(id)
                .w_full()
                .flex()
                .overflow_x_scroll()
                .restrict_scroll_to_axis()
                .track_scroll(&scroll)
                .child(
                    content
                        .flex_none()
                        .w_full()
                        .min_w(min)
                        // Room for the scrollbar under the last row.
                        .when(scrolls, |el| el.pb(space::S12)),
                ),
        )
        .when(scrolls, |el| {
            el.child(
                div()
                    .absolute()
                    .top_0()
                    .bottom_0()
                    .right_0()
                    .w(space::S16)
                    .bg(gpui::linear_gradient(
                        90.,
                        gpui::linear_color_stop(fill.opacity(0.), 0.),
                        gpui::linear_color_stop(fill, 1.),
                    )),
            )
            // Only the strip under the rows: the rows keep their hover and their parts.
            .child(
                div()
                    .absolute()
                    .left_0()
                    .right_0()
                    .bottom_0()
                    .h(space::S12)
                    .child(
                        gpui_component::scroll::Scrollbar::horizontal(&scroll)
                            .mode(gpui_component::scroll::ScrollbarMode::Always),
                    ),
            )
        })
}

/// [`table`] with the 引用提问 parts of its cells, rows and header (inc-integrate): the
/// same look, plus the 问这里 slot at the end of each row. `key` names the table's fold and
/// sideways scroll (`None`: every row, no scroll); `keep` names rows the fold never hides.
pub(crate) fn table_with_refs(
    key: Option<&str>,
    keep: &dyn Fn(usize) -> bool,
    columns: &[String],
    rows: &[Vec<String>],
    refs: Option<&crate::views::refs::markdown::TableRefs>,
    mut window: Option<&mut Window>,
    cx: &mut App,
) -> AnyElement {
    let order: Vec<usize> = (0..rows.len()).collect();
    let expanded = key.is_some_and(|key| crate::views::cards::is_expanded(key, cx));
    let (shown, hidden) = match key {
        Some(_) => table_fold(rows, &order, expanded, keep),
        None => (order, 0),
    };
    let more = key
        .filter(|_| table_folds(rows.len()))
        .map(|key| fold_row(key, hidden, expanded, cx));
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
    let min_width = table_min_width(&widths);
    let scroller = match (key, window.as_deref_mut()) {
        (Some(key), Some(w)) => Some((key, w)),
        _ => None,
    };
    // The fade ends in the card's own fill, in both themes.
    let fill = ui::native::native_colors(cx).sheet;
    // The sideways scroll takes the window first; the parts below only read it.
    let scroll_state = scroller.map(|(key, w)| {
        let id = gpui::ElementId::Name(format!("{key}-scroll").into());
        w.use_keyed_state(id, cx, |_, _| gpui::ScrollHandle::new());
        key.to_string()
    });
    let window_ref: Option<&Window> = window.as_deref();
    let refs = refs.zip(window_ref);
    let cx_ref: &App = cx;
    let content = table_content(
        columns,
        rows,
        &shown,
        &numeric_col,
        &widths,
        refs,
        more.is_some(),
        cx_ref,
    );
    let k = theme::colors(cx);
    let frame = |inner: Div| {
        // The compact table of §2.7 in a frame: radius 10 and a 1 px border.
        div()
            .relative()
            .rounded(crate::ui::native::tokens::radius::CONTAINER)
            .border_1()
            .border_color(k.line)
            .overflow_hidden()
            .flex()
            .flex_col()
            // The header's fill stays in the frame, inside its corners, while the rows
            // scroll sideways under it (the clip is square).
            .child(
                div()
                    .absolute()
                    .top_0()
                    .left_0()
                    .right_0()
                    .h(m::TABLE_ROW)
                    .rounded_t(
                        crate::ui::native::tokens::radius::CONTAINER
                            - crate::ui::native::tokens::metrics::HAIRLINE,
                    )
                    .bg(k.hover),
            )
            .child(inner)
    };
    let body = match (scroll_state, window) {
        (Some(key), Some(w)) => scroll_table(&key, min_width, content, fill, w, cx),
        _ => content,
    };
    frame(body).children(more).into_any_element()
}

/// The header and the shown rows of a table, without its frame.
#[allow(clippy::too_many_arguments)]
fn table_content(
    columns: &[String],
    rows: &[Vec<String>],
    shown: &[usize],
    numeric_col: &[bool],
    widths: &[Option<gpui::Pixels>],
    refs: Option<(&crate::views::refs::markdown::TableRefs, &Window)>,
    folded: bool,
    cx: &App,
) -> Div {
    let k = theme::colors(cx);
    let _ = folded;
    let cell =
        |ci: usize, text: &str, head: bool, bold: bool| {
            let num = numeric_col.get(ci).copied().unwrap_or(false);
            let pct = (!head).then(|| percent(text)).flatten();
            // Text columns share the width; number columns take what their numbers need.
            let width = widths.get(ci).copied().flatten();
            // A number column takes its widest value in every row (one x per column);
            // numbers never truncate (§2.7).
            div()
                .map(|el| match width {
                    Some(w) => el.flex_none().w(w),
                    // A text column keeps a readable least width: past it the table
                    // scrolls sideways (`scroll_table`).
                    None if ci == 0 => el.flex_1().min_w(m::TABLE_COL_FIRST),
                    None => el.flex_1().min_w(m::TABLE_COL_MIN),
                })
                .flex()
                .items_center()
                .gap(space::S8)
                .when(num, |el| el.justify_end())
                .when_some(pct, |el, p| {
                    // The bar neither moves nor shrinks with the figure beside it.
                    el.child(div().flex_none().w(m::TABLE_PCT_BAR).child(
                        ui::native::determinate_bar((p / 100.).clamp(0., 1.) as f32, None, cx),
                    ))
                })
                .child(if num {
                    div()
                        .flex_none()
                        .when_some(pct.and(pct_figure_width(rows, ci)), |el, w| {
                            el.w(w).flex().justify_end()
                        })
                        .whitespace_nowrap()
                        .when(!head, |el| el.n_figures())
                        .when(bold, |el| el.font_weight(FontWeight::SEMIBOLD))
                        .child(text.to_string())
                        .into_any_element()
                } else {
                    // A text cell cuts at the end with the full text in a tooltip (§2.21).
                    ui::truncate::TruncText::new(text.to_string())
                        .when(bold, |el| el.font_weight(FontWeight::SEMIBOLD))
                        .into_any_element()
                })
        };
    let last = rows.len().saturating_sub(1);
    let last_shown = shown.len().saturating_sub(1);
    // A muted 12 medium header, rows 40 apart with a line between them.
    small(div())
        .flex()
        .flex_col()
        .child(
            meta(div())
                .flex()
                .items_center()
                .gap(space::S12)
                .h(m::TABLE_ROW)
                .px(space::S12)
                .border_b_1()
                .border_color(k.line)
                .font_weight(theme::MEDIUM_WEIGHT)
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
        .children(shown.iter().enumerate().map(|(pos, ri)| {
            let ri = *ri;
            let r = &rows[ri];
            let total = ri == last && is_total(r);
            let line = div()
                .flex()
                .items_center()
                .gap(space::S12)
                .h(m::TABLE_ROW)
                .px(space::S12)
                .when(pos < last_shown, |el| el.border_b_1().border_color(k.line))
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
                // From the row's right end: its padding, then each column and its gap.
                let cells = (1..columns.len()).rev().map(|ci| {
                    let w = widths
                        .get(ci)
                        .copied()
                        .flatten()
                        .unwrap_or(m::TABLE_COL_MIN);
                    f32::from(w + space::S12)
                });
                let cover = crate::views::refs::markdown::cover_width(cells, 0.);
                let surface = ui::native::native_colors(cx).sheet;
                el.relative()
                    .child(rf.row_end_over(ri, cover, px(0.), surface, cx))
            })
        }))
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
            let row =
                div()
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
                                // Work in progress: the working loader (§2.12), not a dot.
                                (None, Some(CardListStatus::InProgress)) => {
                                    gpui_component::Sizable::with_size(
                                        ui::motion::spinner(),
                                        crate::ui::native::tokens::icon::SM,
                                    )
                                    .color(color)
                                    .into_any_element()
                                }
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
                            // The holder has no height: 问这里 never moves the step's text off
                            // the centre it shares with its mark.
                            .children(parts.and_then(|(p, w)| p.button(pick, w, cx)).map(|b| {
                                div().flex_none().h(px(0.)).flex().items_center().child(b)
                            })),
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

/// Progress (D12): the one Progress, h4, primary on primary 20%. A counted label
/// (`已重启 9/24`) stays text; no segments.
fn progress_bar(value: f64, label: Option<&str>, _count: Option<usize>, cx: &App) -> AnyElement {
    let k = theme::colors(cx);
    let v = value.clamp(0., 1.) as f32;
    div()
        .flex()
        .flex_col()
        .gap(space::S6)
        .when_some(label, |el, l| {
            el.child(meta(div()).text_color(k.ink_3).child(l.to_string()))
        })
        .child(ui::native::progress(v, cx))
        .into_any_element()
}

/// The size an image block draws at, in points: the picture's own ratio, never enlarged,
/// inside [`m::IMAGE_MAX_W`] by [`m::IMAGE_MAX_H`], and no side under the gallery's least
/// (a thin strip still reads as a picture). `None` (the size is not known yet): the
/// 16:10 box.
pub(crate) fn image_size(natural: Option<(u32, u32)>) -> (f32, f32) {
    let Some((w, h)) = natural.filter(|(w, h)| *w > 0 && *h > 0) else {
        return (f32::from(m::IMAGE_BOX.0), f32::from(m::IMAGE_BOX.1));
    };
    let (w, h) = (w as f32, h as f32);
    let (max_w, max_h) = (f32::from(m::IMAGE_MAX_W), f32::from(m::IMAGE_MAX_H));
    let least = f32::from(crate::ui::native::tokens::gallery::SINGLE_MIN);
    let scale = (max_w / w).min(max_h / h).min(1.);
    (
        (w * scale).round().clamp(least.min(max_w), max_w),
        (h * scale).round().clamp(least.min(max_h), max_h),
    )
}

/// Sizes of card pictures read from their local copies, by path: the file record of a
/// card image does not always carry them.
#[derive(Default)]
struct PictureSizes(std::collections::HashMap<std::path::PathBuf, Option<(u32, u32)>>);

impl gpui::Global for PictureSizes {}

/// The size in pixels of card picture `file_id`: from its file record, else read once from
/// the header of its local copy. `None` while it downloads or when it cannot be read.
fn picture_size(
    ctx: &Ctx,
    file_id: &str,
    path: Option<&std::path::Path>,
    cx: &mut App,
) -> Option<(u32, u32)> {
    if let Some(size) = ctx
        .env
        .provider
        .card_image_file(file_id)
        .and_then(|f| f.width.zip(f.height))
    {
        return Some(size);
    }
    let path = path?;
    let sizes = &mut cx.default_global::<PictureSizes>().0;
    if let Some(size) = sizes.get(path) {
        return *size;
    }
    let size = image::image_dimensions(path).ok();
    sizes.insert(path.to_path_buf(), size);
    size
}

fn image(ctx: &Ctx, file_id: &str, alt: Option<&str>, cx: &mut App) -> gpui::Stateful<Div> {
    let k = theme::colors(cx);
    let path = ctx
        .env
        .provider
        .card_image(&ctx.msg.group_id, &ctx.msg.topic_id, file_id);
    let natural = picture_size(ctx, file_id, path.as_deref(), cx);
    let (w, h) = image_size(natural);
    div()
        .id(ctx.id(format!("image-{file_id}")))
        // The picture keeps its ratio and gives way to a narrow column (a `row` of three):
        // the width is the most it takes, the height follows.
        .w(px(w))
        .max_w_full()
        .aspect_ratio(w / h)
        .flex_none()
        .rounded(theme::radius::MD)
        .overflow_hidden()
        // A superseded or expired card dims its pictures; its text keeps the muted
        // token's contrast instead (D1).
        .when(ctx.state.muted(), |el| el.opacity(m::MUTED))
        .when_some(open_picture(ctx, file_id), |el, open| {
            el.cursor_pointer().on_click(open)
        })
        .map(|el| match path {
            // A picture whose size is known shows whole on the sunken fill; one whose
            // size is not fills the 16:10 box as before.
            Some(p) if natural.is_some() => el.bg(k.sunken).child(
                img(p)
                    .size_full()
                    .rounded(theme::radius::MD)
                    .object_fit(ObjectFit::Contain),
            ),
            Some(p) => el.child(
                img(p)
                    .size_full()
                    .rounded(theme::radius::MD)
                    .object_fit(ObjectFit::Cover),
            ),
            None => missing_image(el, cx)
                .flex_col()
                .gap(space::S8)
                .when_some(alt, |el, a| {
                    el.child(meta(div()).text_color(k.ink_3).child(a.to_string()))
                }),
        })
}

/// The pictures of a card the image viewer goes through: its image blocks and the
/// screenshots of its 证据, in the card's order, once each is downloaded.
fn card_pictures(ctx: &Ctx) -> Vec<crate::data::FileAttachment> {
    let mut seen = std::collections::HashSet::new();
    ctx.card
        .all_blocks()
        .into_iter()
        .filter_map(|b| match b {
            CardV2Block::Image { file_id, .. } => Some(file_id.as_str()),
            CardV2Block::Evidence {
                file_id: Some(f), ..
            } => Some(f.as_str()),
            _ => None,
        })
        .filter(|f| seen.insert(*f))
        .filter_map(|f| ctx.env.provider.card_image_file(f))
        .collect()
}

/// A click that opens the card picture `file_id` full size in the image viewer (the
/// one message pictures use), going through the card's pictures. `None` while the
/// picture downloads: it is not clickable yet.
fn open_picture(
    ctx: &Ctx,
    file_id: &str,
) -> Option<impl Fn(&gpui::ClickEvent, &mut Window, &mut App) + 'static> {
    use crate::views::files::lightbox;
    let provider = ctx.env.provider.clone();
    let file = provider.card_image_file(file_id)?;
    let opening = lightbox::Opening {
        group_id: ctx.msg.group_id.clone(),
        topic_id: ctx.msg.topic_id.clone(),
        message_id: ctx.msg.message_id.clone(),
        file,
        provider,
        set: Some(lightbox::Set {
            files: card_pictures(ctx),
            author_id: ctx.msg.author_id.clone(),
            created_at: ctx.msg.created_at,
        }),
    };
    Some(
        move |_: &gpui::ClickEvent, window: &mut Window, cx: &mut App| {
            cx.stop_propagation();
            lightbox::open(opening.clone(), window, cx);
        },
    )
}

/// An image with no file: a 1 px border on the card fill and the 16 image icon in
/// the middle (§2.21, F5), never a grey placeholder box.
pub(crate) fn missing_image<E: Styled + ParentElement>(el: E, cx: &App) -> E {
    let k = theme::colors(cx);
    el.border_1()
        .border_color(k.line)
        .bg(k.bg)
        .flex()
        .items_center()
        .justify_center()
        .child(ui::icon(
            Sym::Image,
            crate::ui::native::tokens::icon::MD,
            k.ink_3,
        ))
}

fn link(ctx: &Ctx, label: &str, target: &LinkTarget, key: &str, cx: &App) -> AnyElement {
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
        .id(ctx.id(format!("link-block-{key}")))
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

pub(crate) fn decision(
    ctx: &Ctx,
    id: &str,
    options: &[DecisionOption],
    multiple: bool,
    recommended: &[String],
    cx: &App,
) -> AnyElement {
    let quick = ctx.quick();
    let live = ctx.live();
    let chosen = ctx.form.read(cx).choice(id).to_vec();
    let a = ctx.answerer();
    let sending = matches!(ctx.state, CardState::Sending { .. });
    let enabled = live && !sending;
    // Every choice is a RadioGroup of full-width rows (§2.17): h40, a muted
    // fill, radius 8, the letter in a 20 box (a checkbox when several may
    // be chosen). With two or three short options a press answers at once.
    let rows = options.iter().enumerate().map(|(i, o)| {
        let picked = chosen.contains(&o.id);
        let rec = recommended.contains(&o.id);
        let row = choice_row(
            ctx.id(format!("opt-{id}-{}", o.id)),
            i,
            &o.label,
            o.detail.as_deref(),
            rec,
            picked,
            multiple,
            enabled || (quick && picked),
            cx,
        );
        if !enabled {
            return row;
        }
        if quick {
            let (a, block, option) = (a.clone(), id.to_string(), o.id.clone());
            return row.on_click(move |_, _, cx| a.pick(&block, &option, cx));
        }
        let (block, option, form) = (id.to_string(), o.id.clone(), ctx.form.clone());
        row.on_click(move |_, _, cx| {
            form.update(cx, |f, cx| f.choose(&block, &option, multiple, cx))
        })
    });
    div()
        .flex()
        .flex_col()
        .gap(space::S8)
        .children(rows)
        .into_any_element()
}

/// One choice row (§2.17): h40, full width, a muted fill, radius 8; the
/// letter in a 20 box (a checkbox when several may be chosen), the label
/// with `推荐`, the detail at the trailing edge. Selected: a foreground
/// border on the muted fill. Hover: the pressed fill. A row that cannot be
/// chosen shows at 50% with no hover.
#[allow(clippy::too_many_arguments)]
pub(crate) fn choice_row(
    id: SharedString,
    index: usize,
    label: &str,
    detail: Option<&str>,
    recommended: bool,
    picked: bool,
    multiple: bool,
    enabled: bool,
    cx: &App,
) -> gpui::Stateful<Div> {
    use crate::ui::native::{self as nk, NativeType as _};
    let n = nk::native_colors(cx);
    let mark = if multiple {
        nk::NativeCheckbox::new(SharedString::from(format!("{id}-c")), picked)
            .disabled(!enabled)
            .into_any_element()
    } else {
        let letter = char::from_u32('A' as u32 + (index as u32 % 26)).unwrap_or('A');
        div()
            .flex_none()
            .size(m::TILE)
            .rounded(theme::radius::ITEM)
            .bg(if picked { n.primary } else { n.content })
            .flex()
            .items_center()
            .justify_center()
            .n_section()
            .text_color(if picked { n.on_primary } else { n.secondary })
            .child(letter.to_string())
            .into_any_element()
    };
    // No line at rest (owner decision, v18): the row is a muted fill, the
    // hover one step darker; only the picked row draws a foreground border,
    // so the choice stands out (E2, E3). The border is always 1 px wide so
    // picking does not move the text.
    let hover = n.pressed;
    div()
        .id(id)
        .w_full()
        .min_h(m::OPTION)
        .px(space::S12)
        .rounded(m::OPTION_RADIUS)
        .bg(n.fill)
        .border_1()
        .border_color(if picked {
            n.label
        } else {
            gpui::transparent_black()
        })
        // A tab stop with Enter / Space and ↑ ↓, and the focus ring apart
        // from the picked fill (E3, R1 FocusRow); the hover below keeps its
        // border step.
        // `selected: true` leaves the hover to this row (one hover style).
        .when(enabled, |el| {
            el.focus_row_with(true, ui::native::Arrows::Vertical, cx)
        })
        .when(enabled && !picked, |el| el.hover(move |s| s.bg(hover)))
        .when(!enabled, |el| el.opacity(0.5))
        .flex()
        .items_center()
        .gap(space::S12)
        .n_body()
        .text_color(n.label)
        .child(mark)
        .child(option_title(label, recommended, cx).flex_1())
        .when_some(detail, |el, d| {
            el.child(
                div()
                    .flex_none()
                    .max_w(gpui::relative(0.45))
                    .child(ui::truncate::TruncText::new(d.to_string()).text_color(n.secondary)),
            )
        })
}

/// `text` laid out left to right, as every line of this interface: a label that starts
/// with a right-to-left run (an Arabic name) keeps its run order inside, and a cut puts the
/// "…" at the line's end, never at its start (F3, D1-81).
pub(crate) fn ltr_line(text: &str) -> String {
    let rtl_first = text
        .chars()
        .find(|c| c.is_alphabetic())
        .is_some_and(|c| matches!(c as u32, 0x0590..=0x08FF | 0xFB1D..=0xFDFF | 0xFE70..=0xFEFF));
    if rtl_first {
        format!("\u{200E}{text}")
    } else {
        text.to_string()
    }
}

/// The title of an option row: the label, then `推荐` as `secondary` text
/// right after it, never a capsule (NATIVE.md rule 4).
pub(crate) fn option_title(label: &str, recommended: bool, cx: &App) -> Div {
    let n = ui::native::native_colors(cx);
    div()
        .min_w(px(0.))
        .flex()
        .items_baseline()
        .gap(space::S6)
        .n_body()
        .text_color(n.label)
        .child(ui::truncate::TruncText::new(ltr_line(label)))
        .when(recommended, |el| {
            el.child(
                div()
                    .flex_none()
                    .n_subheadline()
                    .text_color(n.secondary)
                    .child(c::RECOMMENDED),
            )
        })
}

/// The mark of a chosen picture or page (Photos): a 20 pt `accent` circle
/// with an `onAccent` checkmark.
pub(crate) fn chosen_badge(cx: &App) -> Div {
    let n = ui::native::native_colors(cx);
    div()
        .size(px(20.))
        .rounded_full()
        .bg(n.primary)
        .shadow(n.float())
        .flex()
        .items_center()
        .justify_center()
        .child(Sym::Check.inline_small().text_color(n.on_primary))
}

/// A short option as a Regular push button (NATIVE.md §5.2): no letter
/// badge; the recommended option in the Default style; the chosen one
/// leads with a 13 pt checkmark. Disabled while the card is not live.
pub(crate) fn option_button(
    id: SharedString,
    label: &str,
    recommended: bool,
    picked: bool,
    enabled: bool,
    cx: &App,
) -> Button {
    use crate::ui::native as nk;
    let n = nk::native_colors(cx);
    let style = if recommended {
        nk::PushStyle::Default
    } else {
        nk::PushStyle::Outline
    };
    let ink = if recommended { n.on_primary } else { n.label };
    nk::disabled_push(
        nk::push_button(id, "", style, nk::PushSize::Small, cx)
            .flex_none()
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(space::S4)
                    .when(picked, |el| el.child(Sym::Check.inline().text_color(ink)))
                    .child(label.to_string()),
            ),
        !enabled,
        cx,
    )
}

/// The answer record that replaces the controls once answered: a `sunken`
/// row with the drawn check, `你选了「…」` (or `Joye 选了「…」`), where it
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
    let provider = ctx.env.provider.clone();
    let events = provider.card_v2_events(&ctx.msg.group_id, &ctx.msg.message_id);
    // 撤回 of a stored answer (DESIGN §7.10 answered): while the card is still answered
    // (the member has set no status and sent no new rev) and the server knows the answer.
    // The server has the last word: a refusal says the member has started.
    let stored = mine && !sending;
    let retract = interact::retract_state(&events).filter(|_| stored);
    // The server takes an answer back only for a while (INTERACT §3.6): past that the
    // button would only ever be refused, so it goes.
    let in_window = at.is_some_and(|at| interact::retract_open(at, ctx.env.now()));
    let can_retract = stored
        && retract != Some(interact::Retract::Refused)
        && (in_window || retract == Some(interact::Retract::Sending))
        && provider.can_retract_card_v2_answer(&ctx.msg.group_id, &ctx.msg.message_id);
    // An answer nobody picks up says so (the record would read 已发给 for ever).
    let waiting =
        record_wait(ctx, &events, stored.then_some(at).flatten(), cx).filter(|_| retract.is_none());
    let retract_el = can_retract.then(|| {
        let (g, t, m) = (
            ctx.msg.group_id.clone(),
            ctx.msg.topic_id.clone(),
            ctx.msg.message_id.clone(),
        );
        let p = provider.clone();
        quiet_button(ctx.id("retract"), c::CARD_UNDO, cx)
            .loading(retract == Some(interact::Retract::Sending))
            .on_click(move |_, _, _| p.retract_card_v2_answer(&g, &t, &m))
            .into_any_element()
    });
    let remind_el = (waiting == Some(interact::AnswerStage::Silent)).then(|| {
        let (a, author, name) = (a.clone(), ctx.msg.author_id.clone(), ctx.author.clone());
        quiet_button(ctx.id("remind"), c21::RECORD_REMIND, cx)
            .on_click(move |_, _, _| a.mention(&author, &name, c21::RECORD_REMIND_MESSAGE))
            .into_any_element()
    });
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
        .text_size(crate::ui::native::tokens::text::UI.0)
        .line_height(px(18.))
        .child(if sending {
            ui::motion::spinner().color(k.ink_3).into_any_element()
        } else {
            ui::motion::check_mark(
                ctx.id("record-check"),
                ui::icon(Sym::Check, px(16.), k.ink_3),
                px(16.),
                fresh,
                cx,
            )
        })
        .child(div().min_w(px(0.)).one_line().text_color(k.ink).child(text))
        .when(mine, |el| {
            // Where the answer is: on its way, with the member, or with a member who has
            // not started (and, at the end, never took it).
            let tail = match (retract, waiting) {
                (Some(interact::Retract::Refused), _) => c::RECORD_UNDO_REFUSED,
                (Some(interact::Retract::NotSent), _) if can_retract => c::RECORD_UNDO_NOT_SENT,
                (_, Some(interact::AnswerStage::Silent)) => c21::RECORD_NEVER_STARTED,
                (_, Some(interact::AnswerStage::Slow)) => c21::RECORD_NOT_STARTED,
                _ if sending => c::RECORD_SENDING_TO,
                _ => c::RECORD_SENT_TO,
            };
            el.child(
                div()
                    .min_w(px(0.))
                    .flex_shrink(1.)
                    .one_line()
                    .text_color(k.ink_3)
                    .child(crate::ui::truncate::TruncText::new(fill(
                        tail,
                        &[("name", &ctx.author)],
                    ))),
            )
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
                .children(remind_el)
                .children(retract_el)
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

/// How long the member has had my stored answer of `at` without starting, or `None`: the
/// member picked it up (a run in the 话题 since, or the 话题 moved on), the card has a
/// status, or it is too soon. Schedules the redraw at the next stage.
fn record_wait(
    ctx: &Ctx,
    events: &[crate::data::cards_v2::MyCardEvent],
    at: Option<crate::data::Millis>,
    cx: &mut App,
) -> Option<interact::AnswerStage> {
    let (stage, next) = answer_wait(ctx, events, at?)?;
    let now = ctx.env.now();
    if let Some(next) = next {
        let wait = std::time::Duration::from_millis((next - now).max(0) as u64 + 20);
        let due = std::time::Instant::now() + wait;
        let form = ctx.form.clone();
        let scheduled = form.read(cx).wake_at;
        if scheduled.is_none_or(|w| {
            w > due + std::time::Duration::from_millis(50) || w < std::time::Instant::now()
        }) {
            form.update(cx, |f, _| f.wake_at = Some(due));
            cx.spawn(async move |cx: &mut gpui::AsyncApp| {
                cx.background_executor().timer(wait).await;
                cx.update(|cx| {
                    form.update(cx, |f, cx| {
                        f.wake_at = None;
                        cx.notify();
                    })
                });
            })
            .detach();
        }
    }
    stage
}

/// Whether my stored answer of `at` still waits for the member (还没开始, 一直没接): the
/// record line says so, and the header must not say the member has it.
pub(super) fn answer_unclaimed(ctx: &Ctx, actor: &str, at: crate::data::Millis) -> bool {
    if ctx.me.as_deref() != Some(actor) {
        return false;
    }
    let events = ctx
        .env
        .provider
        .card_v2_events(&ctx.msg.group_id, &ctx.msg.message_id);
    interact::retract_state(&events).is_none()
        && answer_wait(ctx, &events, at).is_some_and(|(stage, _)| stage.is_some())
}

/// The stage of my stored answer of `at` and when the next one starts; `None` when the
/// member picked it up, or the card has moved on.
fn answer_wait(
    ctx: &Ctx,
    events: &[crate::data::cards_v2::MyCardEvent],
    at: crate::data::Millis,
) -> Option<(Option<interact::AnswerStage>, Option<crate::data::Millis>)> {
    if ctx.card.status.is_some() || !matches!(ctx.state, CardState::Answered { .. }) {
        return None;
    }
    let p = &ctx.env.provider;
    let runs: Vec<crate::data::Millis> = p
        .runs(&ctx.msg.group_id, &ctx.msg.topic_id)
        .into_iter()
        .filter(|r| r.bot_id == ctx.msg.author_id)
        .map(|r| r.created_at)
        .collect();
    let activity = p
        .topic(&ctx.msg.group_id, &ctx.msg.topic_id)
        .map_or(0, |t| t.last_activity_at);
    if interact::picked_up(at, &runs, activity) {
        return None;
    }
    let now = ctx.env.now();
    // A 2.1 answer's ack says how the member takes it; a 2.0 answer waits like a run.
    // The ack of a 2.1 answer says how the member takes it, and that word stands (a
    // member that is offline, or one that takes answers without a run, is not late).
    // With no ack to go by (a 2.0 answer, or the app restarted since), only a member that
    // has run in this 话题 before is expected to start a run for the answer.
    Some(if interact::has_answer(events) {
        interact::answer_stage(events, now)
    } else if !runs.is_empty() {
        interact::answer_stage_since(at, crate::data::cards_v2::ExpectMode::Run, now)
    } else {
        (None, None)
    })
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
                    && let Some(o) = options.iter().find(|o| &o.id == s)
                {
                    rows.push(CardField {
                        label: c::RECORD_CHOICE.to_string(),
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
        ui::native::PushStyle::Outline,
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
    // A 2.1 card draws its inputs in the body with their labels; the footer
    // never repeats one as a field with no label (F51).
    if super::quick(card) || crate::views::cards::v21::is_21(card) {
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
            label,
            ..
        } if !required.unwrap_or(false) => Some((
            id.clone(),
            // Every input is labeled: the placeholder, else its label.
            placeholder.clone().or_else(|| Some(label.clone())),
        )),
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
    let (edge, shadows) = ui::native::field_edge(&n, focused, error);
    // In-card fields are sm h28 (§1.8, F51); a multi-line area keeps its height.
    let h = if height <= m::OPTION {
        crate::ui::native::tokens::height::SM
    } else {
        height
    };
    div()
        .h(h)
        .w_full()
        .min_w(px(0.))
        .px(ui::native::metrics::FIELD_PAD)
        .rounded(ui::native::metrics::FIELD_RADIUS)
        // The kit field recipe (`field_edge`, K-02): the same fill, edge and rings as the
        // card's textarea and every sheet field.
        .bg(n.control)
        .border_1()
        .border_color(edge)
        .shadow(shadows)
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
    field_input(field, false, disabled, window, cx)
}

/// The input of a field; `number`: tabular figures against the field's right edge.
fn field_input(
    field: &Field,
    number: bool,
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
                .when(number, |el| el.text_right().n_figures())
                .text_size(crate::ui::native::tokens::text::UI.0)
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
                .text_size(crate::ui::native::tokens::text::UI.0)
                .line_height(px(20.))
                .text_color(k.ink),
            window,
            cx,
        ),
    }
}

/// A multi-line card field: the kit textarea (K-02, C-09) over three lines, so its
/// edge, fill, ring and text inset match the single-line field; a single-line field
/// keeps the field box.
pub(crate) fn area_field(
    field: &Field,
    id: &str,
    focused: bool,
    error: bool,
    disabled: bool,
    window: &mut Window,
    cx: &mut App,
) -> AnyElement {
    match field {
        Field::Area(s) => {
            ui::native::NativeTextArea::new(SharedString::from(format!("area-{id}")), s)
                .rows(3)
                .error(error)
                .disabled(disabled)
                .show_focus(focused)
                .into_any_element()
        }
        Field::Line(_) => field_box(focused, error, m::OPTION, cx)
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.))
                    .child(line_input(field, disabled, window, cx)),
            )
            .into_any_element(),
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
            help,
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
        let row = div()
            .flex()
            .when(top, |el| el.items_start())
            .when(!top, |el| el.items_center())
            .gap(space::S12)
            // FormRow (§2.8, D8): the label left-aligned in a 112 column (gap
            // included, so the field starts 112 in as in the sheets), the
            // control fills the value column.
            .child(
                div()
                    .n_body()
                    .flex_none()
                    .w(m::FORM_LABEL - space::S12)
                    .one_line()
                    .when(kind == &InputKind::Textarea, |el| el.pt(space::S4))
                    .text_color(k.ink)
                    .child(label.clone()),
            )
            .child(div().flex_1().min_w(px(0.)).child(control));
        // What the member says about the field (`help`) goes under the field, so the
        // label stays level with the field and not with the two of them.
        let help = help.as_ref().filter(|h| !h.trim().is_empty()).map(|h| {
            meta(div())
                .pl(m::FORM_LABEL)
                .text_color(k.ink_3)
                .child(h.clone())
        });
        rows.push(
            div()
                .flex()
                .flex_col()
                .gap(space::S4)
                .child(row)
                .children(help)
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
                    ui::native::PushSize::Small,
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
            // One control per row (§2.8).
            div()
                .flex()
                .flex_col()
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
                            el.child(ui::icon(Sym::Lock, px(14.), k.ink_3))
                                .tooltip(|window, cx| {
                                    gpui_component::tooltip::Tooltip::new(c::FORM_LOCKED)
                                        .build(window, cx)
                                })
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
            area_field(&field, id, focused || lead, invalid, !live, window, cx)
        }
        InputKind::Date => {
            let field = ctx
                .form
                .update(cx, |f, cx| f.field(id, None, false, window, cx));
            let value = field.text(cx);
            let (form, block) = (ctx.form.clone(), id.to_string());
            let (picker, focused) = ui::native::fields::date_field(
                ctx.id(format!("date-{id}")),
                &value,
                placeholder.unwrap_or(c::FORM_DATE_PLACEHOLDER),
                !live,
                move |text, window, cx| {
                    form.update(cx, |f, cx| f.set_line(&block, text, window, cx));
                },
                window,
                cx,
            );
            field_box(focused || lead, invalid, m::OPTION, cx)
                .child(div().flex_1().min_w(px(0.)).child(picker))
                .into_any_element()
        }
        InputKind::Number | InputKind::Slider | InputKind::Text | InputKind::Unknown => {
            let field = ctx
                .form
                .update(cx, |f, cx| f.field(id, placeholder, false, window, cx));
            let focused = field.is_focused(window, cx);
            // The unit of a number stays in the field, after the number (`unit`).
            let unit = match ctx.card.block(id) {
                Some(CardV2Block::Input {
                    kind: InputKind::Number,
                    unit: Some(u),
                    ..
                }) if !u.trim().is_empty() => Some(u.clone()),
                _ => None,
            };
            // A number is short (§2.8): right-aligned figures with the unit right after.
            let number = matches!(
                ctx.card.block(id),
                Some(CardV2Block::Input {
                    kind: InputKind::Number,
                    ..
                })
            );
            field_box(focused || lead, invalid, m::OPTION, cx)
                .when(number, |el| {
                    el.w(ui::native::metrics::FIELD_NUMBER_WIDTH).max_w_full()
                })
                .child(
                    div()
                        .flex_1()
                        .min_w(px(0.))
                        .child(field_input(&field, number, !live, window, cx)),
                )
                .when_some(unit, |el, u| {
                    el.child(
                        small(div())
                            .flex_none()
                            .whitespace_nowrap()
                            .text_color(k.ink_3)
                            .child(u),
                    )
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
            // A title (the first of several drafts) or a field capped at 40
            // characters stays one line h28 whatever its text (C-04, E5).
            let short =
                (ix == 0 && drafts.len() > 1) || limit(ctx.card, id).is_some_and(|l| l <= 40);
            let multi = !short && (text.contains('\n') || text.chars().count() > 40);
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
                            // Over the limit: neutral text with a red icon
                            // (owner decision v15, field errors).
                            .child(
                                meta(div())
                                    .ml_auto()
                                    .flex()
                                    .items_center()
                                    .gap(space::S4)
                                    .mono()
                                    .text_color(if over { k.ink } else { k.ink_3 })
                                    .when(over, |el| {
                                        el.child(ui::icon(Sym::Failed, px(12.), k.bad))
                                    })
                                    .child(counter),
                            ),
                    )
                    .child(area_field(&field, id, focused, over, false, window, cx))
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
                .id(ctx.id(format!("draft-image-{i}")))
                .when_some(open_picture(ctx, file), |el, open| {
                    el.cursor_pointer().on_click(open)
                })
                .w(px(54.))
                .h(h)
                .rounded(theme::radius::SM)
                .overflow_hidden()
                .when(ctx.state.muted(), |el| el.opacity(m::MUTED))
                .map(|el| match path {
                    Some(p) => el.child(
                        img(p)
                            .size_full()
                            .rounded(theme::radius::SM)
                            .object_fit(ObjectFit::Cover),
                    ),
                    // No file yet: the image icon in a bordered box, never a blank fill.
                    None => missing_image(el, cx),
                })
        };
        let grid = if images.len() >= 3 {
            div()
                .flex()
                .gap(crate::ui::native::tokens::space::S4)
                .child(tile(0, px(74.)))
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap(crate::ui::native::tokens::space::S4)
                        .child(tile(1, px(35.)))
                        .child(tile(2, px(35.))),
                )
        } else {
            div()
                .flex()
                .gap(crate::ui::native::tokens::space::S4)
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
                .text_size(crate::ui::native::tokens::text::UI.0)
                .line_height(px(22.))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(if ctx.state.muted() { k.ink_2 } else { k.ink })
                .line_clamp(2)
                .text_ellipsis()
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
                // The well previews the text; 改一下 shows all of it.
                .child(
                    small(div())
                        .text_color(k.ink_2)
                        // The sixth line ends in "…" on a whole glyph (§2.4 draft
                        // well), never cut sideways at the well's edge.
                        .line_clamp(DRAFT_LINES)
                        .text_ellipsis()
                        .child(t),
                )
        });
    }
    div()
        .flex()
        .items_start()
        .gap(space::S14)
        .p(space::S12)
        .rounded(theme::radius::FIELD)
        .bg(ui::native::native_colors(cx).sheet)
        .border_1()
        .border_color(k.line)
        .children(thumbs)
        .child(texts)
        .into_any_element()
}

/// Lines of draft text the well previews.
const DRAFT_LINES: usize = 6;

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

/// 已 N 分钟 (· 预计 M 分钟) of the current plan item, from its `startedAt` and `estimateMs`.
pub(crate) fn plan_time(it: &RunPlanItem, now: crate::data::Millis) -> Option<String> {
    let mins = |ms: u64| (ms / 60_000).max(1).to_string();
    let elapsed = it
        .started_at
        .map(|t| mins((now.max(0) as u64).saturating_sub(t)));
    let estimate = it.estimate_ms.map(mins);
    match (elapsed, estimate) {
        (Some(n), Some(m)) => Some(fill(c::PLAN_ELAPSED_OF, &[("n", &n), ("m", &m)])),
        (Some(n), None) => Some(fill(c::PLAN_ELAPSED, &[("n", &n)])),
        (None, Some(m)) => Some(fill(c::PLAN_ESTIMATE, &[("n", &m)])),
        (None, None) => None,
    }
}

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
    let header_bar = super::strip_parts(ctx, cx).is_some_and(|p| p.bar.is_some());
    div()
        .flex()
        .flex_col()
        .children(items.iter().enumerate().map(|(i, it)| {
            let pick = Pick::Step(i);
            let ask = parts.as_ref().and_then(|(p, w)| p.button(pick, w, cx));
            let current = it.state == RunPlanState::Current && !current_seen;
            if current {
                current_seen = true;
            }
            let glyph: AnyElement = match it.state {
                RunPlanState::Done => ui::icon(Sym::Check, px(14.), k.ink_3).into_any_element(),
                // The current step: the working loader (§2.12), not a breathing dot.
                RunPlanState::Current => gpui_component::Sizable::with_size(
                    ui::motion::spinner(),
                    crate::ui::native::tokens::icon::SM,
                )
                .color(k.live)
                .into_any_element(),
                // Not started: the mark of the working card (`views/timeline/plan.rs`).
                _ => ui::sym(
                    Sym::Todo,
                    ui::SymSize::INLINE_SMALL,
                    if it.state == RunPlanState::Waiting {
                        k.ink_2
                    } else {
                        k.ink_3
                    },
                )
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
                RunPlanState::Current => progress
                    .as_ref()
                    .and_then(|(_, l)| l.clone())
                    .or_else(|| plan_time(it, ctx.env.now()))
                    .map(|l| {
                        meta(div())
                            .mono()
                            .text_color(k.ink_3)
                            .child(l)
                            .into_any_element()
                    }),
                _ => None,
            };
            // One Progress per card: when the header draws the work bar, the
            // current step keeps only its label (§2.4 working card).
            let bar = progress
                .as_ref()
                .filter(|_| current && !header_bar)
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
                        // The text keeps the centre of its mark and of 问这里 when the
                        // button makes the line taller.
                        .items_center()
                        // A long step ends in "…" with the full text in a
                        // tooltip; 问这里 stays after it (§2.21, F3).
                        .child(
                            ui::truncate::TruncText::new(it.text.clone())
                                .id(SharedString::from(format!("plan-step-text-{i}")))
                                .flex_shrink(1.)
                                .min_w(px(0.))
                                .text_color(text_color)
                                .font_weight(weight),
                        )
                        .when(it.state == RunPlanState::Waiting, |el| {
                            el.child(div().min_w(px(0.)).one_line().text_color(k.ink_2).child(
                                crate::ui::truncate::TruncText::new(fill(
                                    c::PLAN_WAITS_ON,
                                    &[("name", &waiting_on)],
                                )),
                            ))
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

/// How the change of a metric reads: good (`Some(true)`), bad, or neither. The author's
/// `tone` wins over what `better` and the sign of `delta` say (card2: "overrides the tone").
pub(crate) fn metric_good(
    delta: Option<f64>,
    better: Option<MetricBetter>,
    tone: Option<MetricTone>,
) -> Option<bool> {
    match tone {
        Some(MetricTone::Good) => return Some(true),
        Some(MetricTone::Bad) => return Some(false),
        Some(MetricTone::Neutral) => return None,
        Some(MetricTone::Unknown) | None => {}
    }
    match (delta, better) {
        (Some(d), Some(MetricBetter::Up)) => Some(d >= 0.),
        (Some(d), Some(MetricBetter::Down)) => Some(d <= 0.),
        _ => None,
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
    metrics_row(blocks, blocks.len(), parts, window, cx)
}

/// One row of metrics in `columns` equal columns: a row with fewer metrics leaves the
/// rest of its columns empty.
pub(crate) fn metrics_row(
    blocks: &[&CardV2Block],
    columns: usize,
    parts: Option<&CardParts>,
    window: Option<&Window>,
    cx: &App,
) -> AnyElement {
    let k = theme::colors(cx);
    let n = ui::native::native_colors(cx);
    let shown = blocks.len().min(m::METRICS_PER_ROW);
    let empty = columns.min(m::METRICS_PER_ROW).saturating_sub(shown);
    div()
        .flex()
        .gap(space::S16)
        .children(blocks.iter().take(m::METRICS_PER_ROW).map(|b| {
            let bp = parts.filter(|_| window.is_some()).and_then(|p| p.block(b));
            let CardV2Block::Metric {
                label,
                value,
                unit,
                delta,
                better,
                tone,
                note,
                ..
            } = b
            else {
                return div().into_any_element();
            };
            let v = value.as_f64().unwrap_or(0.);
            let d = delta.as_ref().and_then(|d| d.as_f64());
            let good = metric_good(d, *better, *tone);
            let delta_text = d.map(|d| {
                let sign = if d > 0. {
                    "+"
                } else if d < 0. {
                    "−"
                } else {
                    ""
                };
                let mut s = format!("{sign}{}", fmt_number(d.abs()));
                // The unit stays with its number (C-09): `+12%`, `−0.3 秒`.
                match unit.as_deref() {
                    Some("%") => s.push('%'),
                    Some(u) if !u.is_empty() => {
                        s.push(' ');
                        s.push_str(u);
                    }
                    _ => {}
                }
                s
            });
            let mut tail = Vec::new();
            // A note that only repeats the unit says nothing more.
            if let Some(n) = note
                .as_ref()
                .filter(|n| Some(n.as_str()) != unit.as_deref())
            {
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
                // KPI (§2.4): label 14 muted, the number 24/32 semibold
                // tabular, the trend as an outline Badge with a 12 icon; the
                // color sits on the icon only (§1.3).
                .child(
                    div()
                        .one_line()
                        .n_body()
                        .text_color(k.ink_3)
                        .child(label.clone()),
                )
                .child(
                    div()
                        .mt(space::S4)
                        .flex()
                        .items_baseline()
                        .gap(space::S2)
                        .child(
                            div()
                                .flex_none()
                                .text_size(crate::ui::native::tokens::text::KPI_COMPACT.0)
                                .line_height(crate::ui::native::tokens::text::KPI_COMPACT.1)
                                .font_weight(FontWeight::SEMIBOLD)
                                .n_figures()
                                .text_color(k.ink)
                                .child(fmt_number(v)),
                        )
                        .when_some(unit.clone().filter(|u| u != "%"), |el, u| {
                            el.child(div().n_body().text_color(k.ink_3).child(u))
                        })
                        .when(unit.as_deref() == Some("%") && d.is_none(), |el| {
                            el.child(div().n_body().text_color(k.ink_3).child("%"))
                        }),
                )
                .when(delta_text.is_some() || !tail.is_empty(), |el| {
                    // `% 比 9 月`: the % belongs to the delta (`+12%`).
                    let mut tail = tail.join("，");
                    let delta_text = match (delta_text, tail.strip_prefix('%')) {
                        (Some(t), Some(rest)) => {
                            let t = format!("{t}%");
                            tail = rest.trim().to_string();
                            Some(t)
                        }
                        (t, _) => t,
                    };
                    let up = d.is_some_and(|d| d >= 0.);
                    let icon_color = match good {
                        Some(true) => n.green,
                        Some(false) => n.red,
                        None => n.secondary,
                    };
                    el.child(
                        div()
                            .mt(space::S8)
                            // Badge-high with or without a badge, so sibling
                            // metrics share one caption line.
                            .h(crate::ui::native::tokens::height::BADGE)
                            .flex()
                            .items_center()
                            .gap(space::S8)
                            .min_w(px(0.))
                            .when_some(delta_text, |el, t| {
                                el.child(ui::native::Badge::new(t).icon(
                                    if up { Sym::ArrowUp } else { Sym::ArrowDown },
                                    icon_color,
                                ))
                            })
                            .when(!tail.is_empty(), |el| {
                                el.child(
                                    div()
                                        .min_w(px(0.))
                                        .one_line()
                                        .n_subheadline()
                                        .text_color(k.ink_3)
                                        .child(tail),
                                )
                            }),
                    )
                });
            ref_parts::wrap_opt(bp.as_ref(), Pick::Metric, el, Anchor::Corner, window, cx)
        }))
        .children((0..empty).map(|_| div().flex_1().min_w(px(0.))))
        .into_any_element()
}

// ---------------------------------------------------------------------------
// Diff

/// A unified diff in a `sunken` well with its path and counts (cards and the 改动 pane).
/// Line numbers and signs stay put; the code never wraps and scrolls sideways as one
/// column, with the code box's bottom scrollbar and right fade whenever a line is cut
/// (D13). `id` names the well's scroll state.
pub(crate) fn diff(
    id: impl Into<gpui::ElementId>,
    path: Option<&str>,
    text: &str,
    adds: Option<u64>,
    dels: Option<u64>,
    window: &mut Window,
    cx: &mut App,
) -> AnyElement {
    diff_folded(id, None, path, text, adds, dels, window, cx)
}

/// True for the lines of a unified diff that are neither code nor a hunk head.
fn diff_file_line(raw: &str) -> bool {
    raw.starts_with("+++")
        || raw.starts_with("---")
        || raw.starts_with("diff ")
        || raw.starts_with("index ")
}

/// How many code lines a folded diff shows, and how many it hides: the first hunk, up to
/// [`m::DIFF_FOLD_LINES`] lines (DESIGN §7.10: one hunk in the card, the rest in the PR). A
/// one-hunk diff a line over the fold shows whole.
pub(crate) fn diff_fold(text: &str) -> (usize, usize) {
    let (mut hunks, mut first, mut total) = (0usize, 0usize, 0usize);
    for raw in text.lines().filter(|l| !diff_file_line(l)) {
        if raw.starts_with("@@") {
            hunks += 1;
        } else {
            total += 1;
            if hunks <= 1 {
                first += 1;
            }
        }
    }
    if hunks <= 1 && total <= m::DIFF_FOLD_LINES + 1 {
        return (total, 0);
    }
    let shown = first.min(m::DIFF_FOLD_LINES);
    (shown, total - shown)
}

/// [`diff`] that folds: with `fold` (the key of its 展开 state) a long diff shows its first
/// hunk up to [`m::DIFF_FOLD_LINES`] lines and a row that opens the rest in place. The
/// well never scrolls up and down inside the timeline.
#[allow(clippy::too_many_arguments)]
pub(crate) fn diff_folded(
    id: impl Into<gpui::ElementId>,
    fold: Option<&str>,
    path: Option<&str>,
    text: &str,
    adds: Option<u64>,
    dels: Option<u64>,
    window: &mut Window,
    cx: &mut App,
) -> AnyElement {
    let k = theme::colors(cx);
    let expanded = fold.is_some_and(|key| crate::views::cards::is_expanded(key, cx));
    let (fold_shown, fold_hidden) = diff_fold(text);
    // The code lines drawn: all of them, or the first hunk's while folded.
    let limit = (fold.is_some() && !expanded && fold_hidden > 0).then_some(fold_shown);
    let (mut hunk_ix, mut code_ix) = (0usize, 0usize);
    let (mut old, mut new) = (0u32, 0u32);
    // Three columns of rows of one height each: the fixed gutter, the code
    // that scrolls, and the hunk lines drawn over both at the well's width.
    let mut gutter: Vec<AnyElement> = Vec::new();
    let mut lines: Vec<AnyElement> = Vec::new();
    let mut hunks: Vec<AnyElement> = Vec::new();
    let hunk_h = m::DIFF_LINE + px(4.);
    let (mut counted_a, mut counted_d) = (0u64, 0u64);
    for raw in text.lines() {
        if diff_file_line(raw) {
            continue;
        }
        if let Some(rest) = raw.strip_prefix("@@") {
            hunk_ix += 1;
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
            if limit.is_some() && hunk_ix > 1 {
                continue;
            }
            gutter.push(div().h(hunk_h).into_any_element());
            lines.push(div().h(hunk_h).into_any_element());
            hunks.push(
                div()
                    .h(hunk_h)
                    .px(space::S12)
                    .flex()
                    .items_center()
                    .text_color(k.ink_3)
                    .one_line()
                    .child(crate::ui::truncate::TruncText::new(raw.to_string()))
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
                    // The tint carries the state; the sign stays muted (§1.3, C-09).
                    k.ink_3,
                )
            }
            "+" => {
                counted_a += 1;
                new += 1;
                (
                    None,
                    Some(new - 1),
                    Some(mix(k.ok, k.bg, m::DIFF_FILL)),
                    k.ink_3,
                )
            }
            _ => {
                old += 1;
                new += 1;
                (Some(old - 1), Some(new - 1), None, k.ink_3)
            }
        };
        // The counts and the line numbers follow every line; a folded diff draws only
        // the first of them.
        let drawn = limit.is_none_or(|n| hunk_ix <= 1 && code_ix < n);
        code_ix += 1;
        if !drawn {
            continue;
        }
        let num = |n: Option<u32>| {
            div()
                .flex_none()
                .w(m::DIFF_NUMBER)
                .text_color(k.ink_3)
                .flex()
                .justify_end()
                .child(n.map(|n| n.to_string()).unwrap_or_default())
        };
        hunks.push(div().h(m::DIFF_LINE).into_any_element());
        gutter.push(
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
                .into_any_element(),
        );
        lines.push(
            div()
                .h(m::DIFF_LINE)
                // The room of the fade, so a line's end is readable at the
                // end of the scroll.
                .pr(space::S16)
                .flex()
                .items_center()
                .when_some(bg, |el, bg| el.bg(bg))
                .whitespace_nowrap()
                .text_color(k.ink)
                .child(body.replace('\t', "    "))
                .into_any_element(),
        );
    }
    let well = ui::native::native_colors(cx).sheet;
    let code = ui::native::blocks::scroll_x(
        id,
        well,
        |scrolls| {
            div()
                .flex()
                .flex_col()
                // Room for the scrollbar under the last line.
                .when(scrolls, |el| el.pb(space::S12))
                .children(lines)
        },
        window,
        cx,
    );
    let adds = adds.unwrap_or(counted_a);
    let dels = dels.unwrap_or(counted_d);
    // A `sunken` well with its own 1 px line (§1.7: a fill step alone is not an edge).
    div()
        .rounded(theme::radius::MD)
        .bg(well)
        .border_1()
        .border_color(k.line)
        .overflow_hidden()
        .flex()
        .flex_col()
        // No path: the caller names the file itself (the 改动 pane's file
        // row), so the well has no header.
        .children(path.map(|path| {
            div()
                .h(crate::ui::native::tokens::height::LG)
                .px(space::S12)
                .flex()
                .items_center()
                .gap(space::S8)
                .border_b_1()
                .border_color(k.line_2)
                // Mono has one size, 13 (§1.4).
                .text_size(crate::ui::native::tokens::text::MONO.0)
                .code_font()
                .child(
                    div()
                        .flex_1()
                        .min_w(px(0.))
                        .one_line()
                        .text_color(k.ink)
                        .child(crate::ui::truncate::TruncText::new(path.to_string()).middle()),
                )
                // Counts in muted text; the +/− glyph carries the meaning (§1.3).
                .child(div().text_color(k.ink_3).child(format!("+{adds}")))
                .child(div().text_color(k.ink_3).child(format!("−{dels}")))
        }))
        .child(
            div()
                .relative()
                .py(space::S4)
                .text_size(crate::ui::native::tokens::text::MONO.0)
                .line_height(m::DIFF_LINE)
                .code_font()
                .flex()
                .items_start()
                .child(div().flex_none().flex().flex_col().children(gutter))
                .child(code.flex_1())
                .child(
                    div()
                        .absolute()
                        .top(space::S4)
                        .left_0()
                        .right_0()
                        .flex()
                        .flex_col()
                        .children(hunks),
                ),
        )
        .children(
            fold.filter(|_| fold_hidden > 0)
                .map(|key| fold_row(key, fold_hidden, expanded, cx)),
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
    url: Option<ProofOpen>,
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
            el.child(ui::icon(g, crate::ui::native::tokens::icon::SM, color))
        })
        .child(div().whitespace_nowrap().child(label))
        .when_some(url, |el, url| {
            // A proof that opens says so: the ↗ after the label, underlined on hover
            // (the muted-link recipe of COMPLETENESS §2).
            el.child(ui::icon(
                Sym::OpenExternal,
                crate::ui::native::tokens::icon::SM,
                n.label,
            ))
            .cursor_pointer()
            .hover(|s| s.underline())
            .on_click(move |_, window, cx| url(window, cx))
        })
        .into_any_element()
}

/// What a click on a proof does: opens its link, or its 产出 at the version it names.
pub(crate) type ProofOpen = std::rc::Rc<dyn Fn(&mut Window, &mut App)>;

/// The click of a proof: its `url`, else its 产出 (the handler of a 产出 reference row).
fn proof_open(
    ctx: &Ctx,
    url: Option<&str>,
    artifact: Option<&crate::data::cards_v2::ArtifactRef>,
) -> Option<ProofOpen> {
    if let Some(url) = url {
        let url = url.to_string();
        return Some(std::rc::Rc::new(move |_, cx| cx.open_url(&url)));
    }
    let a = artifact?;
    let target = RefTarget::Artifact {
        artifact_id: a.artifact_id.clone(),
        version: Some(a.version),
    };
    let answerer = ctx.answerer();
    Some(std::rc::Rc::new(move |window, cx| {
        answerer.open(&target, window, cx)
    }))
}

/// The words of a test result, from its counts and never from the author's label:
/// `测试 148 项通过 · 0 失败`, with ` · 跳过 3` when some were skipped.
pub(crate) fn tests_text(t: &crate::data::cards_v2::EvidenceTests) -> String {
    let text = fill(
        c::EVIDENCE_TESTS,
        &[
            ("passed", &t.passed.to_string()),
            ("failed", &t.failed.to_string()),
        ],
    );
    match t.skipped.filter(|n| *n > 0) {
        Some(n) => fill(
            c::EVIDENCE_TESTS_SKIPPED,
            &[("tests", &text), ("n", &n.to_string())],
        ),
        None => text,
    }
}

/// The mark of a proof: a failed test run or a failed check keeps the `bad` cross, a
/// passed one the muted check (§2.12: no free colored mark). Counts win over `state`.
pub(crate) fn proof_failed(
    state: Option<EvidenceState>,
    tests: Option<&crate::data::cards_v2::EvidenceTests>,
) -> Option<bool> {
    match (tests, state) {
        (Some(t), _) => Some(t.failed > 0),
        (None, Some(EvidenceState::Fail)) => Some(true),
        (None, Some(EvidenceState::Pass)) => Some(false),
        _ => None,
    }
}

/// A tile of the proof strip: a screenshot, or a recording with its poster.
struct Thumb {
    /// The block's id (the element ids of the tile).
    key: String,
    /// The picture: the screenshot, or the poster frame of a recording.
    file: Option<String>,
    detail: Option<String>,
    label: String,
    part: Option<BlockParts>,
    /// The video file of a recording and its length in ms.
    recording: Option<(String, u64)>,
}

/// A click that plays the recording `file_id`, opened the way an attachment opens. A video
/// the 话题 does not list is looked up by its id first; a Toast says when it cannot be read.
fn open_recording(
    ctx: &Ctx,
    file_id: &str,
) -> impl Fn(&gpui::ClickEvent, &mut Window, &mut App) + 'static {
    let provider = ctx.env.provider.clone();
    let (group, topic) = (ctx.msg.group_id.clone(), ctx.msg.topic_id.clone());
    let listed = provider
        .topic_files(&group, &topic)
        .into_iter()
        .find(|f| f.file.file_id == file_id)
        .map(|f| f.file);
    let file_id = file_id.to_string();
    move |_: &gpui::ClickEvent, window: &mut Window, cx: &mut App| {
        use crate::views::files::{OpenAction, open_file};
        cx.stop_propagation();
        if let Some(file) = &listed {
            open_file(&provider, &group, &topic, file, OpenAction::Open, cx);
            return;
        }
        let rx = provider.card_recording(&file_id);
        let (provider, group, topic) = (provider.clone(), group.clone(), topic.clone());
        let handle = window.window_handle();
        cx.spawn(async move |cx: &mut gpui::AsyncApp| {
            let found = rx.await.unwrap_or(Err(crate::data::FileError::Other));
            cx.update(|cx| match found {
                Ok(file) => open_file(&provider, &group, &topic, &file, OpenAction::Open, cx),
                Err(_) => {
                    use gpui_component::WindowExt as _;
                    let note = crate::ui::overlay::Toast::new(ui::Sym::Failed, c::RECORDING_FAILED)
                        .notification();
                    let _ = handle.update(cx, |_, window, cx| window.push_notification(note, cx));
                }
            });
        })
        .detach();
    }
}

/// The play disc and the length of a recording, over its tile.
fn recording_marks(duration_ms: u64, cx: &App) -> [Div; 2] {
    use crate::ui::native::tokens::gallery as g;
    let n = ui::native::native_colors(cx);
    [
        div()
            .absolute()
            .inset_0()
            .flex()
            .items_center()
            .justify_center()
            .child(
                div()
                    .size(m::PLAY_DISC)
                    .rounded_full()
                    // The scrim of the duration chip: it reads on any frame, in both themes.
                    .bg(gpui::black().opacity(g::BADGE_SCRIM))
                    .shadow(n.float())
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(ui::icon(
                        Sym::Play,
                        crate::ui::native::tokens::icon::SM,
                        gpui::white(),
                    )),
            ),
        div().absolute().right(space::S6).bottom(space::S6).child(
            meta(div())
                .h(g::BADGE_H)
                .px(space::S6)
                .rounded(crate::ui::native::tokens::radius::ITEM)
                .bg(gpui::black().opacity(g::BADGE_SCRIM))
                .flex()
                .items_center()
                .mono()
                .text_color(gpui::white())
                .child(crate::views::media::time_text(duration_ms as f64 / 1000.)),
        ),
    ]
}

fn evidence(ctx: &Ctx, blocks: &[&CardV2Block], window: &mut Window, cx: &mut App) -> AnyElement {
    let k = theme::colors(cx);
    // 引用提问: each proof is a part (REFS §A2.2 evidence `image` / `item`).
    let parts = CardParts::of(ctx, cx);
    let part = |b: &CardV2Block| parts.as_ref().and_then(|p| p.block(b));
    let window: &Window = window;
    let mut thumbs: Vec<Thumb> = Vec::new();
    let mut lines: Vec<AnyElement> = Vec::new();
    let mut chips: Vec<AnyElement> = Vec::new();
    for (i, b) in blocks.iter().enumerate() {
        let CardV2Block::Evidence {
            id,
            label,
            state,
            detail,
            file_id,
            url,
            artifact,
            tests,
            recording,
            ..
        } = b
        else {
            continue;
        };
        // A recording is a tile with its poster, the play disc and its length; a
        // screenshot a tile of its own.
        if recording.is_some() || file_id.is_some() {
            thumbs.push(Thumb {
                key: block_key(b, i),
                file: match recording {
                    Some(r) => r.poster_file_id.clone(),
                    None => file_id.clone(),
                },
                detail: detail.clone(),
                label: label.clone(),
                part: part(b),
                recording: recording
                    .as_ref()
                    .map(|r| (r.file_id.clone(), r.duration_ms)),
            });
            let _ = id;
            continue;
        }
        let bp = part(b);
        let glyph = match proof_failed(*state, tests.as_ref()) {
            // A passed check is a muted mark; only a failure keeps color on
            // its icon (§2.12: no free colored mark).
            Some(false) => Some((Sym::Check, k.ink_3)),
            Some(true) => Some((Sym::Close, k.bad)),
            None => None,
        };
        // Test counts say the result themselves (card2: read these, not the label).
        let label = match tests {
            Some(t) => tests_text(t),
            None => label.clone(),
        };
        let open = proof_open(ctx, url.as_deref(), artifact.as_ref());
        // A result with a sentence of detail reads as a line; a bare count
        // as a chip.
        if let Some(d) = detail.as_ref().filter(|d| d.chars().count() > 8) {
            let head = small(div())
                .id(ctx.id(format!("proof-line-head-{i}")))
                .flex()
                .items_center()
                .gap(space::S8)
                .text_color(k.ink)
                .when_some(glyph, |el, (g, c)| el.child(ui::icon(g, px(14.), c)))
                .child(div().min_w(px(0.)).child(label))
                // A line that opens says so like a chip does: the ↗ after its label,
                // underlined on hover.
                .when_some(open, |el, open| {
                    el.child(ui::icon(
                        Sym::OpenExternal,
                        crate::ui::native::tokens::icon::SM,
                        k.ink,
                    ))
                    .cursor_pointer()
                    .hover(|s| s.underline())
                    .on_click(move |_, window, cx| open(window, cx))
                });
            let line = div()
                .id(ctx.id(format!("proof-line-{i}")))
                .flex()
                .flex_col()
                .gap(space::S2)
                .child(div().flex().child(head))
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
                None => label,
            };
            let chip = proof_chip(ctx.id(format!("proof-{i}")), glyph, text, open, cx);
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
            .children(thumbs.iter().take(shown).map(|t| {
                let (detail, label, bp, key) = (&t.detail, &t.label, &t.part, &t.key);
                let path = t.file.as_ref().and_then(|file| {
                    ctx.env
                        .provider
                        .card_image(&ctx.msg.group_id, &ctx.msg.topic_id, file)
                });
                let thumb = div()
                    .id(ctx.id(format!("proof-shot-{key}")))
                    .map(|el| match &t.recording {
                        // A recording plays; a screenshot opens in the image viewer.
                        Some((video, _)) => el
                            .relative()
                            .cursor_pointer()
                            .on_click(open_recording(ctx, video)),
                        None => el.when_some(
                            t.file.as_ref().and_then(|f| open_picture(ctx, f)),
                            |el, open| el.cursor_pointer().on_click(open),
                        ),
                    })
                    .flex_1()
                    .min_w(px(0.))
                    .h(m::THUMB)
                    .rounded(theme::radius::MD)
                    .border_1()
                    .border_color(k.line)
                    .bg(k.bg)
                    .overflow_hidden()
                    .when(ctx.state.muted(), |el| el.opacity(m::MUTED))
                    .map(|el| match path {
                        // The clip is square: the picture rounds itself inside the border.
                        Some(p) => el.child(
                            img(p)
                                .size_full()
                                .rounded(
                                    theme::radius::MD
                                        - crate::ui::native::tokens::metrics::HAIRLINE,
                                )
                                .object_fit(ObjectFit::Cover),
                        ),
                        None => el
                            .p(space::S8)
                            .flex()
                            .flex_col()
                            .gap(crate::ui::native::tokens::space::S4)
                            // The label and at most 2 log lines at 12/16 fit the 84
                            // tile inside its padding: nothing is cut mid-glyph (§2.21).
                            .child(
                                div()
                                    .text_size(crate::ui::native::tokens::text::META.0)
                                    .line_height(crate::ui::native::tokens::text::META.1)
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(k.ink)
                                    .pb(space::S4)
                                    .border_b_1()
                                    .border_color(k.line)
                                    .one_line()
                                    .child(crate::ui::truncate::TruncText::new(label.clone())),
                            )
                            .children(detail.iter().flat_map(|d| d.lines()).take(2).map(|l| {
                                div()
                                    .text_size(crate::ui::native::tokens::text::META.0)
                                    .line_height(crate::ui::native::tokens::text::META.1)
                                    .text_color(k.ink_3)
                                    .one_line()
                                    .child(crate::ui::truncate::TruncText::new(l.to_string()))
                            })),
                    })
                    .children(
                        t.recording
                            .as_ref()
                            .map(|(_, ms)| recording_marks(*ms, cx))
                            .into_iter()
                            .flatten(),
                    );
                // The thumbnail clips, so the part wraps it: 问这里 sits inside its corner.
                ref_parts::wrap_opt(
                    bp.as_ref(),
                    Pick::Evidence,
                    div()
                        .id(ctx.id(format!("proof-shot-part-{key}")))
                        .flex_1()
                        .min_w(px(0.))
                        .child(thumb),
                    Anchor::Corner,
                    Some(window),
                    cx,
                )
            }))
            .when(more > 0, |el| {
                // +N opens the first picture it stands for.
                let open = thumbs
                    .get(shown)
                    .and_then(|t| t.file.as_ref())
                    .and_then(|file| open_picture(ctx, file));
                // The wrapper keeps the border out of the flex base: four equal columns.
                el.child(
                    div().flex_1().min_w(px(0.)).child(
                        div()
                            .id(ctx.id("proof-shot-more"))
                            .when_some(open, |el, open| el.cursor_pointer().on_click(open))
                            .w_full()
                            .h(m::THUMB)
                            .rounded(theme::radius::MD)
                            .border_1()
                            .border_color(k.line)
                            .bg(k.raised)
                            .flex()
                            .items_center()
                            .justify_center()
                            .text_size(crate::ui::native::tokens::text::UI.0)
                            .text_color(k.ink_2)
                            .child(fill(c::EVIDENCE_MORE, &[("n", &more.to_string())])),
                    ),
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

/// `这次没有附证据` + `让 X 补上`: a change to review (an ask card with one
/// pull request) that carries no proof of testing. Neutral, never red. A done
/// card never shows it: most answers are not code changes, and the nag on
/// every answer read as noise.
fn no_evidence(ctx: &Ctx, cx: &App) -> Option<AnyElement> {
    let blocks = ctx.card.all_blocks();
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
                | CardV2Block::Evidence { tests: Some(_), .. }
                | CardV2Block::Evidence {
                    recording: Some(_),
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
    if !change || matches!(ctx.state, CardState::Superseded | CardState::Expired) {
        return None;
    }
    let a = ctx.answerer();
    let (author, name) = (ctx.msg.author_id.clone(), ctx.author.clone());
    // The §2.15 Alert (neutral, info icon, the action at the right), like 读不懂.
    Some(
        ui::native::Alert::new(Sym::Info, c::EVIDENCE_NONE)
            .on_card()
            .action(
                super::button(
                    ctx.id("ask-evidence"),
                    fill(c::EVIDENCE_ASK, &[("name", &ctx.author)]),
                    ui::native::PushStyle::Outline,
                    cx,
                )
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
    /// Closed without a merge.
    Closed,
    /// Open, a draft, or free text: not merged yet.
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
        "closed" => (rail::CHANGES_CLOSED, PrTone::Closed),
        "failed" => (c::REF_CHECK_FAILED, PrTone::Bad),
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
                    } else if part.ends_with(c::REF_CHECK_FAILED) {
                        PrTone::Bad
                    } else if part == rail::CHANGES_MERGING || is_count(last) {
                        PrTone::Running
                    } else if part == rail::CHANGES_CLOSED {
                        PrTone::Closed
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
                    PrTone::Closed => 4,
                    PrTone::Plain => 5,
                })
                .unwrap_or(PrTone::Plain);
            // The badge says the §2.12 word of its state (C-09): 等你审 for a review that
            // waits on me, 在做 n/m for checks that run; other free text shows as it is.
            let count = raw
                .split_whitespace()
                .last()
                .filter(|w| is_count(w))
                .and_then(|w| w.split_once('/'));
            let text = match (tone, count) {
                (PrTone::Waits, _) => common::STATUS_WAITING_REVIEW_ME.to_string(),
                (PrTone::Running, Some((n, total))) if !raw.contains(rail::CHANGES_MERGING) => {
                    fill(c::STRIP_WORKING_COUNT, &[("n", n), ("total", total)])
                }
                _ => raw.to_string(),
            };
            return PrState { text, tone };
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

/// The state of the pull request at `url` from its 话题's newest link (`topic.linked`), in the
/// words [`pr_state`] reads; `None` when the 话题 links another pull request or none.
fn live_pr_state(ctx: &Ctx, url: &str) -> Option<String> {
    use crate::data::v2::ChangeState;
    let changes = ctx
        .env
        .provider
        .topic_changes(&ctx.msg.group_id, &ctx.msg.topic_id)?;
    (changes.url.as_deref() == Some(url)).then(|| match changes.state {
        ChangeState::Open => "open".to_string(),
        ChangeState::Merging => crate::copy::topic_rail::CHANGES_MERGING.to_string(),
        ChangeState::Merged => "merged".to_string(),
        ChangeState::Closed => "closed".to_string(),
    })
}

/// The title of the 话题's linked pull request, when `url` is that pull request: the
/// card's own label is only `repo#number`.
fn live_pr_title(ctx: &Ctx, url: &str) -> Option<(u32, String)> {
    let changes = ctx
        .env
        .provider
        .topic_changes(&ctx.msg.group_id, &ctx.msg.topic_id)?;
    let title = changes.title.trim();
    (changes.url.as_deref() == Some(url) && !title.is_empty())
        .then(|| (changes.number, title.to_string()))
}

/// The two lines of a pull request row. With the title known: `#61 <title>` over the
/// repo. Without it the label stands alone, and the repo is not said a second time
/// under a label that already holds it.
fn pr_row_text(
    label: &str,
    number: Option<u64>,
    repo: Option<&str>,
    live: Option<(u32, String)>,
) -> (String, String) {
    let repo = repo.unwrap_or_default();
    match live {
        Some((n, title)) => (
            format!("#{} {title}", number.unwrap_or(u64::from(n))),
            repo.to_string(),
        ),
        None => (
            match number {
                Some(n) if !label.contains(&format!("#{n}")) => format!("#{n} {label}"),
                _ => label.to_string(),
            },
            if !repo.is_empty() && label.contains(repo) {
                String::new()
            } else {
                repo.to_string()
            },
        ),
    }
}

fn refs(ctx: &Ctx, blocks: &[&CardV2Block], cx: &App) -> AnyElement {
    let k = theme::colors(cx);
    let p = &ctx.env.provider;
    let rows = blocks.iter().enumerate().filter_map(|(i, b)| {
        let CardV2Block::Ref { label, target, .. } = b else {
            return None;
        };
        let (lead, name, line, right, right_state): (AnyElement, String, String, Option<String>, Option<BadgeState>) =
            match target {
                RefTarget::Pr { state, number, repo, url, .. } => {
                    // The 话题's newest link of this pull request knows its state now; the card
                    // keeps the state it was posted with, which a merge later does not change.
                    let now = live_pr_state(ctx, url);
                    let state = now.or_else(|| state.clone()).as_deref().map(pr_state);
                    let tone = state.as_ref().map_or(PrTone::Plain, |s| s.tone);
                    let (name, line) =
                        pr_row_text(label, *number, repo.as_deref(), live_pr_title(ctx, url));
                    (
                        ref_tile(
                            if tone == PrTone::Merged { Sym::Merge } else { Sym::PullRequest },
                            false,
                            cx,
                        ),
                        name,
                        line,
                        state.map(|s| s.text),
                        match tone {
                            PrTone::Waits => Some(BadgeState::NeedsYou),
                            PrTone::Bad => Some(BadgeState::Failed),
                            PrTone::Running => Some(BadgeState::Working),
                            PrTone::Merged => Some(BadgeState::Done),
                            // The same marks as the 改动 pane (topic_rail native_changes):
                            // the clock for a PR that is not merged, the ring for a closed one.
                            PrTone::Closed => Some(BadgeState::Offline),
                            PrTone::Plain => Some(BadgeState::Queued),
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
                            None,
                        )
                    }
                    None => (
                        ref_tile(Sym::MissingFile, true, cx),
                        c::REF_ARTIFACT_GONE.to_string(),
                        fill(c::REF_ARTIFACT_GONE_DETAIL, &[("title", label)]),
                        None,
                        None,
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
                    // The 话题's state in the §2.12 words of every other view
                    // (`topic_page::topic_state`: asks first, 在做 only with a run), once in
                    // the badge; the line under the title names the 群 only (D1-66).
                    // The badge holds the state words only (`在做 1/4`, never cut): the
                    // member's name that leads the sentence stays out of it (D1-79).
                    let state = topic
                        .as_ref()
                        .map(|t| crate::views::topic_page::topic_state(p.as_ref(), ctx.me.as_deref(), t));
                    let words = state.as_ref().map(|s| {
                        s.name
                            .as_deref()
                            .and_then(|n| s.text.strip_prefix(n))
                            .map(str::trim_start)
                            .filter(|w| !w.is_empty())
                            .unwrap_or(&s.text)
                            .to_string()
                    });
                    (
                        ref_tile(Sym::List, false, cx),
                        topic.map(|t| t.title).unwrap_or_else(|| label.clone()),
                        group,
                        words,
                        state.map(|s| s.badge),
                    )
                }
                RefTarget::Member { member_id } => {
                    let member = p.member(member_id);
                    (
                        ui::MemberAvatar::new(member.as_ref(), px(20.)).into_any_element(),
                        member.as_ref().map(|m| m.name.clone()).unwrap_or_else(|| label.clone()),
                        member.and_then(|m| m.role).unwrap_or_default(),
                        None,
                        None,
                    )
                }
                RefTarget::Url { url } => (
                    ref_tile(Sym::Link, false, cx),
                    label.clone(),
                    url.clone(),
                    None,
                    None,
                ),
                RefTarget::Unknown => (
                    ref_tile(Sym::Link, false, cx),
                    label.clone(),
                    String::new(),
                    None,
                    None,
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
                    el.focus_row(false, cx)
                        .on_click(move |_, window, cx| a.open(&t, window, cx))
                })
                // One 24 media column for every kind, so every name starts
                // at one x (F4).
                .child(
                    div()
                        .flex_none()
                        .size(px(24.))
                        .flex()
                        .items_center()
                        .justify_center()
                        .child(lead),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w(px(0.))
                        .flex()
                        .flex_col()
                        .child(
                            ui::truncate::TruncText::new(name)
                                .id(ctx.id(format!("ref-name-{i}")))
                                .text_size(crate::ui::native::tokens::text::UI.0)
                                .line_height(px(20.))
                                .font_weight(theme::MEDIUM_WEIGHT)
                                .text_color(if gone { k.ink_2 } else { k.ink }),
                        )
                        .when(!line.is_empty(), |el| {
                            el.child(
                                ui::truncate::TruncText::new(line)
                                    .id(ctx.id(format!("ref-line-{i}")))
                                    .text_size(crate::ui::native::tokens::text::META.0)
                                    .line_height(crate::ui::native::tokens::text::META.1)
                                    .text_color(k.ink_3),
                            )
                        }),
                )
                // The state in one right column as a StatusBadge (§2.12, F30).
                .when_some(right, |el, r| {
                    el.child(match right_state {
                        Some(state) => ui::native::StatusBadge::new(state, r)
                            .max_w(m::MAX_WIDTH / 4.)
                            .into_any_element(),
                        // Every badge has an icon (§2.12): a newer version
                        // reads with the refresh mark.
                        None => ui::native::Badge::new(r)
                            .secondary()
                            .icon(Sym::Refresh, ui::native::native_colors(cx).secondary)
                            .max_w(m::MAX_WIDTH / 4.)
                            .into_any_element(),
                    })
                }),
        )
    });
    // A long list shows its first rows and ends with a footer row that
    // opens the rest (§2.6).
    let total = blocks.len();
    let all = ctx.form.read(cx).refs_all;
    let capped = total > REFS_CAP + 1 && !all;
    let shown = if capped { REFS_CAP } else { total };
    let footer = (total > REFS_CAP + 1).then(|| {
        let form = ctx.form.clone();
        let label = if all {
            c::REFS_FEWER.to_string()
        } else {
            fill(c::REFS_ALL, &[("count", &total.to_string())])
        };
        quiet_button(ctx.id("refs-all"), &label, cx)
            .on_click(move |_, _, cx| form.update(cx, |f, cx| f.toggle_refs_all(cx)))
            .into_any_element()
    });
    // No box of its own inside the card: rows divided by a hairline, their
    // hover reaching past the text (one container level, §7.10).
    let list = ui::divided(
        rows.take(shown)
            .map(|r| ui::inset_row(r).into_any_element()),
        theme::LIST_ROW_BLEED,
        cx,
    )
    .mx(-theme::LIST_ROW_BLEED);
    match footer {
        Some(f) => div()
            .flex()
            .flex_col()
            .child(list)
            .child(
                div()
                    .flex()
                    .pt(space::S8)
                    .border_t_1()
                    .border_color(k.line)
                    .child(f),
            )
            .into_any_element(),
        None => list.into_any_element(),
    }
}

/// Rows a ref list shows before its 看全部 footer row.
const REFS_CAP: usize = 5;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::copy::{common, topic_rail as rail};

    #[test]
    fn a_current_plan_step_says_how_long_it_runs() {
        let mut it = RunPlanItem::new("读代码", RunPlanState::Current);
        assert_eq!(plan_time(&it, 0), None);
        it.started_at = Some(1_000);
        assert_eq!(
            plan_time(&it, 1_000 + 5 * 60_000).as_deref(),
            Some("已 5 分钟")
        );
        it.estimate_ms = Some(20 * 60_000);
        assert_eq!(
            plan_time(&it, 1_000 + 5 * 60_000).as_deref(),
            Some("已 5 分钟 · 预计 20 分钟")
        );
        it.started_at = None;
        assert_eq!(plan_time(&it, 0).as_deref(), Some("预计 20 分钟"));
    }

    fn blocks(v: serde_json::Value) -> Vec<CardV2Block> {
        serde_json::from_value(v).expect("blocks parse")
    }

    #[test]
    fn a_run_of_six_metrics_draws_all_six_in_two_rows() {
        // The body walks a run row by row: every metric lands in exactly one row.
        let (mut i, end, mut rows) = (0, 6, Vec::new());
        while i < end {
            let j = metric_row_end(i, end);
            rows.push((i, j));
            i = j;
        }
        assert_eq!(rows, [(0, 4), (4, 6)]);
        assert_eq!(rows.iter().map(|(a, b)| b - a).sum::<usize>(), 6);
        assert_eq!(metric_row_end(0, 3), 3, "a short run is one row");
        assert_eq!(metric_row_end(8, 9), 9);
    }

    #[test]
    fn element_ids_follow_the_block_id_not_the_index() {
        let b = blocks(serde_json::json!([
            { "type": "text", "id": "a", "text": "一" },
            { "type": "text", "id": "b", "text": "二" },
            { "type": "text", "id": "", "text": "三" }
        ]));
        // The 2.1 renderer draws every leaf with index 0.
        assert_ne!(block_key(&b[0], 0), block_key(&b[1], 0));
        assert_eq!(block_key(&b[0], 7), "a");
        assert_eq!(block_key(&b[2], 7), "7", "no id: the index");
    }

    #[test]
    fn a_long_table_folds_to_ten_rows_and_keeps_its_total_and_picked_rows() {
        let mut rows: Vec<Vec<String>> = (0..199).map(|i| vec![format!("r{i}")]).collect();
        rows.push(vec![c::TABLE_TOTAL.to_string()]);
        let order: Vec<usize> = (0..rows.len()).collect();
        let (shown, hidden) = table_fold(&rows, &order, false, &|_| false);
        assert_eq!(shown.len(), m::TABLE_FOLD_ROWS + 1);
        assert_eq!(shown.last(), Some(&199), "合计 stays in sight");
        assert_eq!(hidden, 189);
        // A picked row the fold would hide stays shown, in its place.
        let (shown, hidden) = table_fold(&rows, &order, false, &|ri| ri == 57);
        assert_eq!(&shown[10..], [57, 199]);
        assert_eq!(hidden, 188);
        // Expanded, or short: every row.
        assert_eq!(
            table_fold(&rows, &order, true, &|_| false),
            (order.clone(), 0)
        );
        let short: Vec<Vec<String>> = (0..11).map(|i| vec![format!("r{i}")]).collect();
        let order: Vec<usize> = (0..11).collect();
        assert_eq!(table_fold(&short, &order, false, &|_| false).1, 0);
        assert!(!table_folds(11) && table_folds(12));
        // A sorted table folds in its sorted order.
        let rows: Vec<Vec<String>> = (0..30).map(|i| vec![format!("r{i}")]).collect();
        let order: Vec<usize> = (0..30).rev().collect();
        assert_eq!(table_fold(&rows, &order, false, &|_| false).0[0], 29);
    }

    #[test]
    fn twenty_columns_need_more_than_a_card_is_wide() {
        // Four text columns and sixteen number columns.
        let mut widths: Vec<Option<gpui::Pixels>> = vec![None; 4];
        widths.extend(vec![Some(m::TABLE_NUM_COL); 16]);
        assert!(
            table_min_width(&widths) > m::MAX_WIDTH,
            "it scrolls sideways"
        );
        // Four columns fit: no scroll.
        let small = [None, Some(m::TABLE_NUM_COL), Some(m::TABLE_NUM_COL), None];
        assert!(table_min_width(&small) < m::MAX_WIDTH / 1.5);
    }

    #[test]
    fn a_long_diff_folds_to_its_first_hunk() {
        let text = crate::views::cards::v2::samples::long_diff();
        assert!(
            text.len() < 32_768,
            "inside the schema's limit: {}",
            text.len()
        );
        assert!(text.lines().any(|l| l.len() >= 300), "one very wide line");
        let (shown, hidden) = diff_fold(&text);
        assert_eq!(shown, m::DIFF_FOLD_LINES);
        assert_eq!(shown + hidden, 600);
        // Two short hunks: the first shows whole, the second folds.
        let two = "@@ -1,2 +1,2 @@\n-a\n+b\n@@ -9,2 +9,2 @@\n-c\n+d";
        assert_eq!(diff_fold(two), (2, 2));
        // One short hunk never folds; file lines are not code.
        let one = "--- a/x\n+++ b/x\n@@ -1,2 +1,2 @@\n-a\n+b\n c";
        assert_eq!(diff_fold(one), (3, 0));
        let edge: String = (0..=m::DIFF_FOLD_LINES)
            .map(|i| format!(" l{i}\n"))
            .collect();
        assert_eq!(diff_fold(&edge).1, 0, "one line over the fold shows whole");
    }

    #[test]
    fn an_image_keeps_its_ratio_inside_the_block() {
        let (bw, bh) = (f32::from(m::IMAGE_BOX.0), f32::from(m::IMAGE_BOX.1));
        assert_eq!(image_size(None), (bw, bh), "unknown size: the 16:10 box");
        assert_eq!(image_size(Some((0, 10))), (bw, bh));
        // A tall phone screenshot: as tall as the block allows, its width follows.
        let (w, h) = image_size(Some((1170, 2532)));
        assert_eq!(h, f32::from(m::IMAGE_MAX_H));
        assert!((w / h - 1170. / 2532.).abs() < 0.01);
        // A panorama: as wide as the block allows.
        let (w, h) = image_size(Some((4000, 1000)));
        assert_eq!((w, h), (f32::from(m::IMAGE_MAX_W), 120.));
        // A small picture is never enlarged.
        assert_eq!(image_size(Some((200, 150))), (200., 150.));
        // A hairline strip keeps a side one can see.
        let (w, _) = image_size(Some((10, 4000)));
        assert!(w >= 96.);
    }

    #[test]
    fn the_authors_tone_wins_over_better_and_delta() {
        let up = Some(MetricBetter::Up);
        assert_eq!(metric_good(Some(2.), up, None), Some(true));
        assert_eq!(metric_good(Some(-2.), up, None), Some(false));
        assert_eq!(metric_good(Some(-2.), up, Some(MetricTone::Neutral)), None);
        assert_eq!(
            metric_good(Some(0.6), None, Some(MetricTone::Bad)),
            Some(false)
        );
        assert_eq!(
            metric_good(Some(5.), None, Some(MetricTone::Good)),
            Some(true)
        );
        assert_eq!(metric_good(Some(5.), None, None), None);
        assert_eq!(
            metric_good(Some(2.), up, Some(MetricTone::Unknown)),
            Some(true),
            "an unknown tone falls back"
        );
    }

    #[test]
    fn a_block_copies_as_text_one_can_paste() {
        let b = blocks(serde_json::json!([
            { "type": "table", "id": "t", "columns": ["渠道", "新增群"], "rows": [["微博", "14"], ["合计", "38"]] },
            { "type": "metric", "id": "m", "label": "日活", "value": 2340 },
            { "type": "metric", "id": "p", "label": "留存", "value": 61, "unit": "%" },
            { "type": "fields", "id": "f", "fields": [{ "label": "影响", "value": "重启 2 分钟" }] },
            { "type": "diff", "id": "d", "text": "@@ -1 +1 @@\n-a\n+b" },
            { "type": "evidence", "id": "e", "label": "CI", "tests": { "passed": 3, "failed": 1 },
              "url": "https://ci.example.com/1" },
            { "type": "divider", "id": "x" }
        ]));
        assert_eq!(
            block_text(&b[0]).unwrap(),
            "渠道\t新增群\n微博\t14\n合计\t38"
        );
        assert_eq!(block_text(&b[1]).unwrap(), "日活 2,340");
        assert_eq!(block_text(&b[2]).unwrap(), "留存 61%");
        assert_eq!(block_text(&b[3]).unwrap(), "影响\t重启 2 分钟");
        assert_eq!(block_text(&b[4]).unwrap(), "@@ -1 +1 @@\n-a\n+b");
        assert_eq!(
            block_text(&b[5]).unwrap(),
            "测试 3 项通过 · 1 失败\nhttps://ci.example.com/1"
        );
        assert_eq!(block_text(&b[6]), None, "nothing to copy");
    }

    #[test]
    fn test_counts_say_the_result_not_the_label() {
        use crate::data::cards_v2::EvidenceTests;
        let ok = EvidenceTests {
            passed: 148,
            failed: 0,
            skipped: Some(3),
        };
        assert_eq!(tests_text(&ok), "测试 148 项通过 · 0 失败 · 跳过 3");
        let bad = EvidenceTests {
            passed: 31,
            failed: 2,
            skipped: None,
        };
        assert_eq!(tests_text(&bad), "测试 31 项通过 · 2 失败");
        let none = EvidenceTests {
            passed: 1,
            failed: 0,
            skipped: Some(0),
        };
        assert_eq!(tests_text(&none), "测试 1 项通过 · 0 失败");
        // The counts decide the mark, whatever `state` says.
        assert_eq!(
            proof_failed(Some(EvidenceState::Pass), Some(&bad)),
            Some(true)
        );
        assert_eq!(proof_failed(None, Some(&ok)), Some(false));
        assert_eq!(proof_failed(Some(EvidenceState::Fail), None), Some(true));
        assert_eq!(proof_failed(None, None), None);
    }

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
        assert_eq!(pr_state("closed").tone, PrTone::Closed);
    }

    #[test]
    fn pr_tones_match_exact_words_only() {
        assert_eq!(pr_state("未合并").tone, PrTone::Plain);
        assert_eq!(pr_state(rail::CHANGES_MERGING).tone, PrTone::Running);
        assert_eq!(pr_state("3 项检查通过 · 未合并").tone, PrTone::Plain);
        assert_eq!(pr_state("未合并 · 等你审").tone, PrTone::Waits);
        assert_eq!(pr_state("已合并").tone, PrTone::Merged);
        assert_eq!(pr_state("已合并 11-02").tone, PrTone::Merged);
        assert_eq!(pr_state("需要你审").tone, PrTone::Waits);
        assert_eq!(pr_state("1 项检查没过").tone, PrTone::Bad);
        assert_eq!(pr_state("检查 2/3").tone, PrTone::Running);
        assert_eq!(pr_state("你好").tone, PrTone::Plain);
        // §2.12 words on the badge.
        assert_eq!(pr_state("需要你审").text, common::STATUS_WAITING_REVIEW_ME);
        assert_eq!(pr_state("检查 2/3").text, "在做 2/3");
        // Free text shows unchanged.
        assert_eq!(pr_state("已合并 11-02").text, "已合并 11-02");
        assert_eq!(pr_state("reopened").text, "reopened");
    }
}
