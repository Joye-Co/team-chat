//! One renderer per card block type. Every string except `text` blocks is
//! plain text (docs/protocol.md). Long tables and lists show the first rows
//! and a button to show the rest, so a 200-row card stays cheap to draw.

use gpui::{
    AnyElement, App, Div, ElementId, InteractiveElement as _, IntoElement, ObjectFit,
    ParentElement, StatefulInteractiveElement as _, Styled, StyledImage as _, Window, div, img,
    prelude::FluentBuilder as _, px, relative,
};
use gpui_component::{Disableable as _, button::Button};

use super::{is_expanded, part_id, toggle_expanded};
use crate::{
    copy::{cards as c, fill},
    data::{
        Message,
        card::{CardBlock, CardField, CardListItem, CardListStatus, LinkTarget},
    },
    markdown,
    theme::{self, Type as _, space},
    ui::{self, Sym},
    views::timeline::extras::ExtrasContext,
};

/// Rows of a table, and items of a list, shown before "show the rest".
pub const COLLAPSED_ROWS: usize = 10;
/// Fixed box of an image block, so the row height does not jump when the
/// file arrives.
pub const IMAGE_WIDTH: f32 = 320.;
pub const IMAGE_HEIGHT: f32 = 200.;

/// The wire name of a block type.
pub fn type_name(block: &CardBlock) -> &'static str {
    match block {
        CardBlock::Text { .. } => "text",
        CardBlock::Fields { .. } => "fields",
        CardBlock::Table { .. } => "table",
        CardBlock::List { .. } => "list",
        CardBlock::Progress { .. } => "progress",
        CardBlock::Code { .. } => "code",
        CardBlock::Image { .. } => "image",
        CardBlock::Divider => "divider",
        CardBlock::Link { .. } => "link",
        CardBlock::Unknown => "unknown",
    }
}

pub fn render(
    block: &CardBlock,
    ix: usize,
    message: &Message,
    ctx: &ExtrasContext,
    _window: &mut Window,
    cx: &mut App,
) -> AnyElement {
    let key = part_id(message, ix);
    // --- inc-refs: the parts of a v1 card block can be referenced (REFS §A2.2 `v1:<i>`).
    let refs = v1_refs(message, ix, ctx, cx);
    if let Some(r) = &refs
        && !matches!(block, CardBlock::Table { .. } | CardBlock::List { .. })
        && let Some(part) = crate::views::refs::parts::v1::whole_block(&r.card, ix)
    {
        let el = render_plain(block, ix, &key, message, ctx, _window, cx);
        let spec = r.spec(format!("{}-whole", r.prefix), part);
        return crate::views::refs::affordance::part(
            div()
                .id(ElementId::Name(format!("{key}-ref").into()))
                .child(el),
            spec,
            _window,
            cx,
        );
    }
    match block {
        CardBlock::Table { columns, rows } => {
            return render_table(&key, columns, rows, refs.as_ref(), _window, cx);
        }
        CardBlock::List { items } if refs.is_some() => {
            return render_list(&key, items, refs.as_ref(), _window, cx);
        }
        _ => {}
    }
    render_plain(block, ix, &key, message, ctx, _window, cx)
}

// --- inc-refs ---
/// The references of one v1 card block.
struct V1Refs {
    card: std::sync::Arc<crate::data::card::Card>,
    ix: usize,
    prefix: String,
    source: crate::views::refs::RefSource,
}

impl V1Refs {
    fn target(&self, part: crate::views::refs::parts::Part) -> crate::views::refs::PartTarget {
        crate::views::refs::parts::v1::target(&self.card, self.ix, part, self.source.clone())
    }

    fn spec(
        &self,
        id: String,
        part: crate::views::refs::parts::Part,
    ) -> crate::views::refs::affordance::PartSpec {
        let target = self.target(part);
        crate::views::refs::affordance::PartSpec::new(id, move |_| target.clone())
    }

    fn table(
        &self,
        columns: &[String],
        rows: &[Vec<String>],
    ) -> crate::views::refs::markdown::TableRefs {
        use crate::views::refs::parts;
        let (columns, rows) = (columns.to_vec(), rows.to_vec());
        let (card, ix, source) = (self.card.clone(), self.ix, self.source.clone());
        crate::views::refs::markdown::TableRefs {
            prefix: self.prefix.clone(),
            target: std::rc::Rc::new(move |pick| {
                let data = parts::TableData {
                    columns: &columns,
                    rows: &rows,
                    row_keys: None,
                };
                parts::table(&data, pick).map(|p| parts::v1::target(&card, ix, p, source.clone()))
            }),
        }
    }
}

fn v1_refs(message: &Message, ix: usize, ctx: &ExtrasContext, cx: &App) -> Option<V1Refs> {
    if message.id.is_empty() || !crate::views::refs::enabled(cx) {
        return None;
    }
    let card = ctx.provider.card(&message.group_id, &message.id)?;
    Some(V1Refs {
        card,
        ix,
        prefix: format!("{}-v1:{ix}", message.id),
        source: crate::views::refs::RefSource::Message {
            group_id: message.group_id.clone(),
            topic_id: message.topic_id.clone(),
            message_id: message.id.clone(),
        },
    })
}
// --- end inc-refs ---

fn render_plain(
    block: &CardBlock,
    _ix: usize,
    key: &str,
    message: &Message,
    ctx: &ExtrasContext,
    _window: &mut Window,
    cx: &mut App,
) -> AnyElement {
    let key = key.to_string();
    match block {
        CardBlock::Text { text } => markdown::message_body(
            ElementId::Name(key.into()),
            text.clone(),
            &markdown::mention_targets(ctx.members, ctx.me),
        )
        .w_full()
        .into_any_element(),
        CardBlock::Fields { fields } => render_fields(fields, cx),
        CardBlock::Table { columns, rows } => render_table(&key, columns, rows, None, _window, cx),
        CardBlock::List { items } => render_list(&key, items, None, _window, cx),
        CardBlock::Progress { label, .. } => {
            render_progress(&key, block.progress().unwrap_or(0.0), label.as_deref(), cx)
        }
        CardBlock::Code { lang, text } => markdown::message_body(
            ElementId::Name(key.into()),
            code_fence(lang.as_deref(), text),
            &[],
        )
        .w_full()
        .into_any_element(),
        CardBlock::Image { file_id, alt } => {
            render_image(message, ctx, file_id, alt.as_deref(), cx)
        }
        CardBlock::Divider => ui::hairline(cx).into_any_element(),
        CardBlock::Link { label, target } => render_link(&key, message, ctx, label, target),
        CardBlock::Unknown => div().into_any_element(),
    }
}

/// Fields as key-value rows (§7.10 draft card, Feishu): `ui` 14/20, the key
/// column in `text-3` (48 pt, wider for long keys), a 0.5 pt `sep` under each
/// row, padding 10 0. No outer border: the card has the only one.
fn render_fields(fields: &[CardField], cx: &App) -> AnyElement {
    let k = theme::colors(cx);
    let longest = fields
        .iter()
        .map(|f| {
            f.label
                .chars()
                .map(|ch| if ch.is_ascii() { 7.5 } else { 14.0 })
                .sum::<f32>()
        })
        .fold(0.0_f32, f32::max);
    let key_w = px(longest.clamp(48.0, 120.0).ceil());
    div()
        .flex()
        .flex_col()
        .children(fields.iter().map(|f| {
            div()
                .py(space::S10)
                .flex()
                .items_start()
                .gap(space::S12)
                .t_ui()
                .border_b(theme::HAIRLINE)
                .border_color(k.sep)
                .child(
                    div()
                        .w(key_w)
                        .flex_none()
                        .text_color(k.text_3)
                        .child(f.label.clone()),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w(px(0.))
                        .text_color(k.text_1)
                        .child(f.value.clone()),
                )
        }))
        .into_any_element()
}

/// Width of a table column from its longest cell, in pixels.
fn column_width(columns: &[String], rows: &[Vec<String>], col: usize) -> f32 {
    let longest = std::iter::once(columns.get(col).map(String::as_str).unwrap_or(""))
        .chain(
            rows.iter()
                .map(|r| r.get(col).map(String::as_str).unwrap_or("")),
        )
        .map(|s| {
            s.chars()
                .map(|ch| if ch.is_ascii() { 1.0 } else { 1.8 })
                .sum::<f32>()
        })
        .fold(0.0_f32, f32::max);
    (longest * 7.0 + 20.0).clamp(72.0, 240.0)
}

fn more_button(key: &str, hidden: usize, expanded: bool) -> Button {
    let label = if expanded {
        c::TABLE_LESS.to_string()
    } else {
        fill(c::TABLE_MORE, &[("n", &hidden.to_string())])
    };
    let toggle_key = key.to_string();
    ui::native::ghost_button(
        ElementId::Name(format!("{key}-more").into()),
        label,
        ui::native::PushSize::Small,
    )
    .on_click(move |_, window, cx| toggle_expanded(toggle_key.clone(), window, cx))
}

fn render_table(
    key: &str,
    columns: &[String],
    rows: &[Vec<String>],
    refs: Option<&V1Refs>,
    window: &Window,
    cx: &App,
) -> AnyElement {
    let colors = theme::colors(cx);
    // inc-refs: cells, columns and the 问这里 slot at the end of each row.
    let table_refs = refs.map(|r| r.table(columns, rows));
    let mut widths: Vec<f32> = (0..columns.len())
        .map(|i| column_width(columns, rows, i))
        .collect();
    // Room for 问这里 after a row's name (Refs.dc.html).
    if table_refs.is_some()
        && let Some(w) = widths.first_mut()
    {
        *w += crate::views::refs::markdown::NAME_ROOM;
    }
    let expanded = is_expanded(key, cx);
    let shown = if expanded {
        rows.len()
    } else {
        rows.len().min(COLLAPSED_ROWS)
    };
    let hidden = rows.len() - rows.len().min(COLLAPSED_ROWS);

    // Number columns are right-aligned, their numbers in mono (DESIGN §7.10 Table).
    let numeric: Vec<bool> = (0..columns.len())
        .map(|ci| {
            ci > 0
                && rows.iter().all(|r| {
                    r.get(ci)
                        .is_none_or(|v| v.is_empty() || super::v2::blocks::numeric(v))
                })
        })
        .collect();
    let cell_in = |ci: usize, text: String, width: f32, head: bool| -> Div {
        let num = numeric.get(ci).copied().unwrap_or(false);
        div()
            .w(px(width))
            .flex_none()
            .px(space::S10)
            .py(space::S6)
            .flex()
            .items_center()
            .when(num, |el| el.justify_end())
            .child(div().when(num && !head, |el| el.mono()).child(text))
    };
    let header = div()
        .flex()
        .t_meta_medium()
        .text_color(colors.text_3)
        .children(
            columns
                .iter()
                .zip(&widths)
                .enumerate()
                .map(|(i, (name, w))| {
                    let el = cell_in(i, name.clone(), *w, true);
                    match &table_refs {
                        Some(r) => r.header(el, i, window, cx),
                        None => el.into_any_element(),
                    }
                }),
        )
        .when(table_refs.is_some(), |el| {
            el.child(
                div()
                    .w(px(crate::views::refs::markdown::ROW_BUTTON_SLOT))
                    .flex_none(),
            )
        });
    let body = rows[..shown].iter().enumerate().map(|(ri, row)| {
        let line = div()
            .flex()
            .border_t(theme::HAIRLINE)
            .border_color(colors.sep)
            .t_small()
            .tnum()
            .text_color(colors.text_1);
        let line = match &table_refs {
            Some(r) => r.row(line, ri, window, cx),
            None => line,
        };
        // 问这里 after the row's name when the first column holds both (Refs.dc.html).
        let beside = table_refs.as_ref().is_some_and(|r| {
            let name = row.first().map(String::as_str).unwrap_or("");
            let inner = widths.first().map(|w| w - 2. * f32::from(space::S10));
            r.beside(name, inner, window)
        });
        line.children(widths.iter().enumerate().map(|(i, w)| {
            let mut el = cell_in(i, row.get(i).cloned().unwrap_or_default(), *w, false);
            if i == 0 && beside {
                el = el.children(table_refs.as_ref().and_then(|r| r.name_button(ri, 0., cx)));
            }
            match &table_refs {
                Some(r) => r.cell(el, ri, i, window, cx),
                None => el.into_any_element(),
            }
        }))
        .when_some(table_refs.as_ref(), |el, r| {
            el.child(r.row_end_unless(ri, beside, cx))
        })
    });
    div()
        .flex()
        .flex_col()
        .gap(crate::ui::native::tokens::space::S4)
        .child(
            div()
                .id(ElementId::Name(format!("{key}-scroll").into()))
                .w_full()
                .overflow_x_scroll()
                .child(div().flex().flex_col().child(header).children(body)),
        )
        .when(hidden > 0, |el| {
            el.child(div().flex().child(more_button(key, hidden, expanded)))
        })
        .into_any_element()
}

/// The 16 pt glyph of a plan step (§8.1 progress): done in `success`, the
/// current step in `text-1`, later steps `circle-dashed` in `text-3`.
fn list_icon(status: Option<CardListStatus>, cx: &App) -> AnyElement {
    let k = theme::colors(cx);
    let (glyph, color) = match status {
        None => {
            return div()
                .size(px(5.))
                .rounded_full()
                .bg(k.text_3)
                .into_any_element();
        }
        Some(CardListStatus::Todo) => (Sym::Todo, k.text_3),
        Some(CardListStatus::InProgress) => (Sym::Live, k.text_1),
        Some(CardListStatus::Done) => (Sym::Done, k.success),
        Some(CardListStatus::Failed) => (Sym::Failed, k.danger),
        Some(CardListStatus::Skipped) => (Sym::Minus, k.text_3),
    };
    ui::icon(glyph, theme::icon::ROW, color).into_any_element()
}

fn list_status_text(status: CardListStatus) -> &'static str {
    match status {
        CardListStatus::Todo => c::LIST_TODO,
        CardListStatus::InProgress => c::LIST_IN_PROGRESS,
        CardListStatus::Done => c::LIST_DONE,
        CardListStatus::Failed => c::LIST_FAILED,
        CardListStatus::Skipped => c::LIST_SKIPPED,
    }
}

/// A plan list (Slack Block Kit, §8.1): 28 pt rows, a 16 pt glyph, `small`
/// 13/20 text; the current step `text-1` 500, the rest `text-2`; the state
/// word at the right in `text-3` for failed and skipped steps.
fn render_list(
    key: &str,
    items: &[CardListItem],
    refs: Option<&V1Refs>,
    window: &Window,
    cx: &App,
) -> AnyElement {
    let k = theme::colors(cx);
    let expanded = is_expanded(key, cx);
    let shown = if expanded {
        items.len()
    } else {
        items.len().min(COLLAPSED_ROWS)
    };
    let hidden = items.len() - items.len().min(COLLAPSED_ROWS);
    div()
        .flex()
        .flex_col()
        .children(items[..shown].iter().enumerate().map(|(i, item)| {
            let skipped = item.status == Some(CardListStatus::Skipped);
            let now = item.status == Some(CardListStatus::InProgress);
            let row = div()
                .id(ElementId::Name(format!("{key}-item-{i}").into()))
                .min_h(px(28.))
                .py(space::S4)
                .flex()
                .items_start()
                .gap(space::S10)
                .t_small()
                .child(
                    div()
                        .h(px(20.))
                        .w(px(16.))
                        .flex_none()
                        .flex()
                        .items_center()
                        .justify_center()
                        .child(list_icon(item.status, cx)),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w(px(0.))
                        .text_color(if now { k.text_1 } else { k.text_2 })
                        .when(now, |el| el.medium())
                        .when(skipped, |el| el.line_through())
                        .child(item.text.clone()),
                )
                .children(
                    item.status
                        .filter(|s| matches!(s, CardListStatus::Failed | CardListStatus::Skipped))
                        .map(|s| {
                            div()
                                .flex_none()
                                .t_meta()
                                .h(px(20.))
                                .flex()
                                .items_center()
                                .text_color(if s == CardListStatus::Failed {
                                    k.danger_text
                                } else {
                                    k.text_3
                                })
                                .child(list_status_text(s))
                        }),
                );
            // inc-refs: an item of the list.
            match refs {
                Some(r) => {
                    let status = item.status.map(|s| match s {
                        CardListStatus::Todo => "todo",
                        CardListStatus::InProgress => "inProgress",
                        CardListStatus::Done => "done",
                        CardListStatus::Failed => "failed",
                        CardListStatus::Skipped => "skipped",
                    });
                    let part = crate::views::refs::parts::list_item(i, None, &item.text, status);
                    crate::views::refs::affordance::part(
                        row,
                        r.spec(format!("{}-i{i}", r.prefix), part),
                        window,
                        cx,
                    )
                }
                None => row.into_any_element(),
            }
        }))
        .when(hidden > 0, |el| {
            el.child(div().flex().child(more_button(key, hidden, expanded)))
        })
        .into_any_element()
}

/// A progress block: the label `small` `text-2` with the percent at the
/// right (12 `text-3`, tabular), then a 4 pt `accent` bar on a `field` track.
fn render_progress(_key: &str, value: f64, label: Option<&str>, cx: &App) -> AnyElement {
    let k = theme::colors(cx);
    let percent = (value * 100.0).round();
    div()
        .flex()
        .flex_col()
        .gap(space::S6)
        .child(
            div()
                .flex()
                .items_center()
                .gap(space::S8)
                .child(
                    div()
                        .flex_1()
                        .min_w(px(0.))
                        .t_small()
                        .text_color(k.text_2)
                        .one_line()
                        .children(label.map(str::to_string)),
                )
                .child(
                    div()
                        .flex_none()
                        .t_meta()
                        .tnum()
                        .text_color(k.text_3)
                        .child(format!("{percent}%")),
                ),
        )
        .child(
            div().h(px(4.)).w_full().rounded(px(2.)).bg(k.field).child(
                div()
                    .h_full()
                    .w(relative(value.clamp(0.0, 1.0) as f32))
                    .rounded(px(2.))
                    .bg(k.accent),
            ),
        )
        .into_any_element()
}

/// A Markdown code fence around `text` that `text` cannot close: one
/// backtick more than its longest run, at least three.
pub fn code_fence(lang: Option<&str>, text: &str) -> String {
    let mut longest = 0;
    let mut run = 0;
    for ch in text.chars() {
        if ch == '`' {
            run += 1;
            longest = longest.max(run);
        } else {
            run = 0;
        }
    }
    let fence = "`".repeat((longest + 1).max(3));
    let lang: String = lang
        .unwrap_or("")
        .chars()
        .filter(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '+' | '-' | '_' | '#' | '.'))
        .take(32)
        .collect();
    let newline = if text.ends_with('\n') { "" } else { "\n" };
    format!("{fence}{lang}\n{text}{newline}{fence}")
}

fn render_image(
    message: &Message,
    ctx: &ExtrasContext,
    file_id: &str,
    alt: Option<&str>,
    cx: &App,
) -> AnyElement {
    let colors = theme::colors(cx);
    let frame = div()
        .w(px(IMAGE_WIDTH))
        .max_w_full()
        .h(px(IMAGE_HEIGHT))
        .rounded(theme::radius::MD)
        .border_1()
        .border_color(colors.border)
        .bg(colors.field)
        .overflow_hidden()
        .flex()
        .items_center()
        .justify_center();
    let placeholder = |text: String| {
        div()
            .flex()
            .flex_col()
            .items_center()
            .gap(space::S6)
            .px(space::S12)
            .t_meta()
            .text_color(colors.text_3)
            .child(ui::icon(Sym::Image, theme::icon::EMPTY, colors.text_3))
            .child(text)
    };
    match ctx
        .provider
        .card_image(&message.group_id, &message.topic_id, file_id)
    {
        Some(path) => {
            let fallback = alt.unwrap_or(c::IMAGE).to_string();
            frame
                .child(
                    img(path)
                        .size_full()
                        .object_fit(ObjectFit::Contain)
                        .with_fallback(move || {
                            div().t_meta().child(fallback.clone()).into_any_element()
                        }),
                )
                .into_any_element()
        }
        None => frame
            .child(placeholder(
                alt.map(str::to_string)
                    .unwrap_or_else(|| c::IMAGE_LOADING.to_string()),
            ))
            .into_any_element(),
    }
}

fn render_link(
    key: &str,
    message: &Message,
    ctx: &ExtrasContext,
    label: &str,
    target: &LinkTarget,
) -> AnyElement {
    div()
        .flex()
        .child(link_button(key, message, ctx, label, target))
        .into_any_element()
}

fn link_button(
    key: &str,
    message: &Message,
    ctx: &ExtrasContext,
    label: &str,
    target: &LinkTarget,
) -> AnyElement {
    let id = ElementId::Name(format!("{key}-link").into());
    let button = ui::native::ghost_button(id, label.to_string(), ui::native::PushSize::Small)
        .ml(-ui::native::metrics::BUTTON_SMALL_PAD);
    match target {
        LinkTarget::Url { url } => match markdown::safe_link(url) {
            Some(safe) if safe.starts_with("http") || safe.starts_with("HTTP") => {
                let url = safe.to_string();
                button
                    .icon(Sym::OpenExternal)
                    .tooltip(url.clone())
                    .on_click(move |_, _, cx| cx.open_url(&url))
                    .into_any_element()
            }
            _ => button.disabled(true).into_any_element(),
        },
        LinkTarget::Topic { group_id, topic_id } => {
            let (g, t) = (group_id.clone(), topic_id.clone());
            let _ = message;
            button
                .icon(Sym::ArrowRight)
                .tooltip(c::OPEN_TOPIC)
                .on_click(move |_, _, cx| crate::views::nav::open_topic(g.clone(), t.clone(), cx))
                .into_any_element()
        }
        LinkTarget::Artifact {
            artifact_id,
            version,
        } => {
            let provider = ctx.provider.clone();
            let (id, version) = (artifact_id.clone(), *version);
            button
                .icon(Sym::Document)
                .tooltip(c::OPEN_ARTIFACT)
                .on_click(move |_, _, cx| {
                    crate::views::artifacts::window::open_artifact(&provider, &id, version, cx)
                })
                .into_any_element()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn code_fence_is_longer_than_any_backtick_run() {
        assert_eq!(
            code_fence(Some("rust"), "fn a() {}"),
            "```rust\nfn a() {}\n```"
        );
        assert_eq!(code_fence(None, "a ```` b\n"), "`````\na ```` b\n`````");
        assert_eq!(code_fence(Some("x\n```js"), "1"), "```xjs\n1\n```");
    }

    #[test]
    fn column_width_is_clamped() {
        let cols = vec!["名称".to_string(), "x".repeat(500)];
        let rows = vec![vec!["a".to_string()]];
        assert_eq!(column_width(&cols, &rows, 0), 72.0);
        assert_eq!(column_width(&cols, &rows, 1), 240.0);
        assert_eq!(column_width(&cols, &rows, 5), 72.0);
    }
}
