//! One renderer per card block type. Every string except `text` blocks is
//! plain text (docs/protocol.md). Long tables and lists show the first rows
//! and a button to show the rest, so a 200-row card stays cheap to draw.

use gpui::{
    AnyElement, App, Div, ElementId, InteractiveElement as _, IntoElement, ObjectFit,
    ParentElement, SharedString, StatefulInteractiveElement as _, Styled, StyledImage as _, Window,
    div, img, prelude::FluentBuilder as _, px, relative,
};
use gpui_component::{
    Disableable as _, Sizable as _,
    button::{Button, ButtonVariants as _},
    progress::Progress,
    tooltip::Tooltip,
};

use super::{is_expanded, part_id, toggle_expanded};
use crate::{
    copy::{cards as c, fill},
    data::{
        Message,
        card::{CardBlock, CardField, CardListItem, CardListStatus, LinkTarget},
    },
    markdown,
    theme::{self, Elevation, Type as _, space},
    ui::{self, Lucide},
    views::timeline::extras::ExtrasContext,
};

/// Rows of a table, and items of a list, shown before "查看全部" (§13.9).
pub const COLLAPSED_ROWS: usize = 8;
/// Fixed height of an image block, so the row height does not jump when the
/// file arrives. The width is the card's.
pub const IMAGE_WIDTH: f32 = 432.;
pub const IMAGE_HEIGHT: f32 = 200.;
/// Width of a field label (§13.9).
const FIELD_LABEL_W: f32 = 56.;
/// A value longer than this many columns gets a tooltip with the full text.
const TOOLTIP_AFTER: f32 = 14.;

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

/// The header glyph that a block type suggests, for a card without a tone.
pub fn type_glyph(block: &CardBlock) -> Option<Lucide> {
    match block {
        CardBlock::Table { .. } => Some(Lucide::Table),
        CardBlock::List { .. } => Some(Lucide::ListChecks),
        CardBlock::Progress { .. } => Some(Lucide::Loader),
        CardBlock::Code { .. } => Some(Lucide::Terminal),
        CardBlock::Image { .. } => Some(Lucide::Image),
        _ => None,
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
    let colors = theme::colors(cx);
    match block {
        // A summary under the header: 12 / 17 `text-2` (§13.9).
        CardBlock::Text { text } => div()
            .w_full()
            .text_color(colors.text_2)
            .child(
                markdown::message_body(ElementId::Name(key.into()), text.clone())
                    .text_size(theme::type_scale::CALLOUT.0)
                    .line_height(px(17.)),
            )
            .into_any_element(),
        CardBlock::Fields { fields } => render_fields(&key, fields, cx),
        CardBlock::Table { columns, rows } => render_table(&key, columns, rows, cx),
        CardBlock::List { items } => render_list(&key, items, cx),
        CardBlock::Progress { label, .. } => {
            render_progress(&key, block.progress().unwrap_or(0.0), label.as_deref(), cx)
        }
        CardBlock::Code { lang, text } => markdown::message_body(
            ElementId::Name(key.into()),
            code_fence(lang.as_deref(), text),
        )
        .w_full()
        .into_any_element(),
        CardBlock::Image { file_id, alt } => {
            render_image(&key, message, ctx, file_id, alt.as_deref(), cx)
        }
        CardBlock::Divider => ui::hairline(cx).into_any_element(),
        CardBlock::Link { label, target } => render_link(&key, message, label, target),
        CardBlock::Unknown => div().into_any_element(),
    }
}

/// Display columns of a text: Han and other wide characters count two.
fn columns_of(text: &str) -> f32 {
    text.chars()
        .map(|ch| if ch.is_ascii() { 1.0 } else { 2.0 })
        .sum()
}

/// One line of text with an ellipsis, and the full text in a tooltip when it
/// may not fit (§12).
fn one_line_text(id: String, text: &str, max_columns: f32) -> AnyElement {
    let base = div().min_w(px(0.)).one_line();
    if columns_of(text) > max_columns {
        let full: SharedString = text.to_string().into();
        base.id(ElementId::Name(id.into()))
            .tooltip(move |window, cx| Tooltip::new(full.clone()).build(window, cx))
            .child(text.to_string())
            .into_any_element()
    } else {
        base.child(text.to_string()).into_any_element()
    }
}

/// `DescriptionList` layout of §13.9: two pairs per row, a 56 pt label in
/// `text-2` and a 13 pt value on one line.
fn render_fields(key: &str, fields: &[CardField], cx: &App) -> AnyElement {
    let colors = theme::colors(cx);
    let pair = |ix: usize, f: &CardField| {
        div()
            .w(relative(0.5))
            .min_w(px(0.))
            .pr(space::S10)
            .flex()
            .items_center()
            .gap(space::S10)
            .child(
                div()
                    .w(px(FIELD_LABEL_W))
                    .flex_none()
                    .text_color(colors.text_2)
                    .one_line()
                    .child(f.label.clone()),
            )
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.))
                    .text_color(colors.text_1)
                    .tnum()
                    .child(one_line_text(
                        format!("{key}-f{ix}"),
                        &f.value,
                        TOOLTIP_AFTER,
                    )),
            )
    };
    div()
        .flex()
        .flex_col()
        .gap(space::S6)
        .t_ui()
        .children(fields.chunks(2).enumerate().map(|(row, chunk)| {
            div()
                .flex()
                .children(
                    chunk
                        .iter()
                        .enumerate()
                        .map(|(i, f)| pair(row * 2 + i, f)),
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
    (longest * 7.0 + 20.0).clamp(48.0, 240.0)
}

/// A column of numbers (counts, durations, sizes): every filled cell starts
/// with a digit or a dash. Numbers are right aligned (§13.9).
fn is_numeric_column(rows: &[Vec<String>], col: usize) -> bool {
    let mut any = false;
    for row in rows {
        let cell = row.get(col).map(|s| s.trim()).unwrap_or("");
        if cell.is_empty() {
            continue;
        }
        let first = cell.chars().next().unwrap_or(' ');
        if !(first.is_ascii_digit() || matches!(first, '—' | '-' | '+' | '−')) {
            return false;
        }
        any = true;
    }
    any
}

/// "查看全部 n 行" or "收起" under a long table or list.
fn more_button(key: &str, label: String) -> Button {
    let toggle_key = key.to_string();
    ui::text_button(ElementId::Name(format!("{key}-more").into()), label)
        .on_click(move |_, window, cx| toggle_expanded(toggle_key.clone(), window, cx))
}

fn render_table(key: &str, columns: &[String], rows: &[Vec<String>], cx: &App) -> AnyElement {
    let colors = theme::colors(cx);
    let widths: Vec<f32> = (0..columns.len())
        .map(|i| column_width(columns, rows, i))
        .collect();
    let numeric: Vec<bool> = (0..columns.len())
        .map(|i| is_numeric_column(rows, i))
        .collect();
    let expanded = is_expanded(key, cx);
    let shown = if expanded {
        rows.len()
    } else {
        rows.len().min(COLLAPSED_ROWS)
    };
    let last = columns.len().saturating_sub(1);

    // The first column takes the spare width; numbers align right.
    let cell = |col: usize, text: String| -> Div {
        // The first column shrinks to 120 pt (with an ellipsis) before the
        // table scrolls sideways.
        let el = div()
            .when(col > 0, |el| el.w(px(widths[col])).flex_none())
            .when(col == 0, |el| el.flex_1().min_w(px(widths[0].min(120.))))
            .when(col < last, |el| el.pr(space::S8))
            .one_line();
        if numeric[col] {
            el.flex().justify_end().tnum().child(text)
        } else {
            el.child(text)
        }
    };
    let header = div()
        .flex()
        .pb(space::S6)
        .t_section()
        .text_color(colors.text_2)
        .children(
            columns
                .iter()
                .enumerate()
                .map(|(i, name)| cell(i, name.clone())),
        );
    let body = rows[..shown].iter().map(|row| {
        div()
            .flex()
            .py(space::S6)
            .border_t(theme::HAIRLINE)
            .border_color(colors.sep)
            .text_size(px(12.5))
            .line_height(px(17.))
            .text_color(colors.text_1)
            .children(
                (0..columns.len()).map(|i| cell(i, row.get(i).cloned().unwrap_or_default())),
            )
    });
    div()
        .flex()
        .flex_col()
        .gap(space::S4)
        .child(
            div()
                .id(ElementId::Name(format!("{key}-scroll").into()))
                .w_full()
                .overflow_x_scroll()
                .child(
                    div()
                        .min_w_full()
                        .flex()
                        .flex_col()
                        .child(header)
                        .children(body),
                ),
        )
        .when(rows.len() > COLLAPSED_ROWS, |el| {
            let label = if expanded {
                c::TABLE_LESS.to_string()
            } else {
                fill(c::TABLE_ALL, &[("n", &thousands(rows.len()))])
            };
            el.child(div().flex().child(more_button(key, label)))
        })
        .into_any_element()
}

/// "1,284" (§12).
pub fn thousands(n: usize) -> String {
    let digits = n.to_string();
    let mut out = String::new();
    for (i, ch) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(ch);
    }
    out
}

/// The 15 pt glyph of a list item (§13.9).
fn list_icon(status: Option<CardListStatus>, cx: &App) -> AnyElement {
    let k = theme::colors(cx);
    let size = px(15.);
    let (glyph, color) = match status {
        None => {
            return div()
                .size(px(5.))
                .rounded_full()
                .bg(k.text_3)
                .into_any_element();
        }
        Some(CardListStatus::Todo) => (Lucide::Circle, k.text_3),
        Some(CardListStatus::InProgress) => (Lucide::Loader, k.accent),
        Some(CardListStatus::Done) => (Lucide::CircleCheck, k.success),
        Some(CardListStatus::Failed) => (Lucide::CircleAlert, k.danger),
        Some(CardListStatus::Skipped) => (Lucide::CircleDashed, k.text_3),
    };
    ui::icon(glyph, size, color).into_any_element()
}

fn render_list(key: &str, items: &[CardListItem], cx: &App) -> AnyElement {
    let colors = theme::colors(cx);
    let expanded = is_expanded(key, cx);
    let shown = if expanded {
        items.len()
    } else {
        items.len().min(COLLAPSED_ROWS)
    };
    div()
        .flex()
        .flex_col()
        .gap(space::S6)
        .t_ui()
        .children(items[..shown].iter().map(|item| {
            let quiet = matches!(
                item.status,
                Some(CardListStatus::Done | CardListStatus::Skipped)
            );
            div()
                .flex()
                .items_start()
                .gap(space::S8)
                .child(
                    div()
                        .h(theme::type_scale::UI.1)
                        .w(px(15.))
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
                        .text_color(if quiet { colors.text_2 } else { colors.text_1 })
                        .when(item.status == Some(CardListStatus::Skipped), |el| {
                            el.line_through()
                        })
                        .child(item.text.clone()),
                )
                // Failed and skipped also say it in words, not only by glyph.
                .children(
                    item.status
                        .filter(|s| matches!(s, CardListStatus::Failed | CardListStatus::Skipped))
                        .map(|s| {
                            div()
                                .flex_none()
                                .t_callout()
                                .text_color(if s == CardListStatus::Failed {
                                    colors.danger_text
                                } else {
                                    colors.text_3
                                })
                                .child(list_status_text(s))
                        }),
                )
        }))
        .when(items.len() > COLLAPSED_ROWS, |el| {
            let label = if expanded {
                c::TABLE_LESS.to_string()
            } else {
                fill(c::LIST_ALL, &[("n", &thousands(items.len()))])
            };
            el.child(div().flex().child(more_button(key, label)))
        })
        .into_any_element()
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

/// Label 13 pt and the value 12 pt `text-2` on one line, then a 6 pt
/// `Progress` in `accent` (§13.9).
fn render_progress(key: &str, value: f64, label: Option<&str>, cx: &App) -> AnyElement {
    let colors = theme::colors(cx);
    let percent = (value * 100.0).round();
    div()
        .flex()
        .flex_col()
        .gap(space::S8)
        .child(
            div()
                .flex()
                .items_baseline()
                .gap(space::S8)
                .child(
                    div()
                        .flex_1()
                        .min_w(px(0.))
                        .t_ui()
                        .text_color(colors.text_1)
                        .one_line()
                        .children(label.map(str::to_string)),
                )
                .child(
                    div()
                        .flex_none()
                        .t_callout()
                        .tnum()
                        .text_color(colors.text_2)
                        .child(format!("{percent}%")),
                ),
        )
        .child(
            Progress::new(ElementId::Name(format!("{key}-bar").into()))
                .small()
                .color(colors.accent)
                .value(percent as f32),
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
    key: &str,
    message: &Message,
    ctx: &ExtrasContext,
    file_id: &str,
    alt: Option<&str>,
    cx: &App,
) -> AnyElement {
    let colors = theme::colors(cx);
    // §13.5 images: radius 8 and an `image-border` ring.
    let frame = div()
        .w_full()
        .max_w(px(IMAGE_WIDTH))
        .h(px(IMAGE_HEIGHT))
        .rounded(theme::radius::ROW)
        .shadow(colors.shadow(Elevation::Image))
        .bg(colors.fill_field)
        .overflow_hidden()
        .flex()
        .items_center()
        .justify_center();
    let image = match ctx
        .provider
        .card_image(&message.group_id, &message.topic_id, file_id)
    {
        Some(path) => {
            let fallback = alt.unwrap_or(c::IMAGE).to_string();
            let text_3 = colors.text_3;
            frame.child(
                img(path)
                    .size_full()
                    .object_fit(ObjectFit::Contain)
                    .with_fallback(move || {
                        div()
                            .size_full()
                            .flex()
                            .items_center()
                            .justify_center()
                            .t_callout()
                            .text_color(text_3)
                            .child(fallback.clone())
                            .into_any_element()
                    }),
            )
        }
        None => frame.child(
            div()
                .flex()
                .flex_col()
                .items_center()
                .gap(space::S6)
                .t_callout()
                .text_color(colors.text_3)
                .child(ui::icon(Lucide::Image, theme::icon::EMPTY, colors.text_3))
                .child(c::IMAGE_LOADING),
        ),
    };
    div()
        .id(ElementId::Name(format!("{key}-image").into()))
        .flex()
        .flex_col()
        .gap(space::S6)
        .child(image)
        .children(alt.map(|alt| {
            div()
                .t_callout()
                .text_color(colors.text_2)
                .one_line()
                .child(alt.to_string())
        }))
        .into_any_element()
}

fn render_link(key: &str, message: &Message, label: &str, target: &LinkTarget) -> AnyElement {
    div()
        .flex()
        .child(link_button(key, message, label, target))
        .into_any_element()
}

/// A text button in `accent-text` with a 13 pt glyph.
fn link_button(key: &str, message: &Message, label: &str, target: &LinkTarget) -> AnyElement {
    let id = ElementId::Name(format!("{key}-link").into());
    let button = Button::new(id).small().link().label(label.to_string());
    match target {
        LinkTarget::Url { url } => match markdown::safe_link(url) {
            Some(safe) if safe.starts_with("http") || safe.starts_with("HTTP") => {
                let url = safe.to_string();
                button
                    .icon(Lucide::ExternalLink)
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
                .icon(Lucide::ChevronRight)
                .tooltip(c::OPEN_TOPIC)
                .on_click(move |_, _, cx| crate::views::nav::open_topic(g.clone(), t.clone(), cx))
                .into_any_element()
        }
        LinkTarget::Artifact { .. } => button
            .icon(Lucide::FileText)
            .tooltip(c::ARTIFACT_LATER)
            .disabled(true)
            .into_any_element(),
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
        assert_eq!(column_width(&cols, &rows, 0), 48.0);
        assert_eq!(column_width(&cols, &rows, 1), 240.0);
        assert_eq!(column_width(&cols, &rows, 5), 48.0);
    }

    #[test]
    fn counts_have_thousands_separators() {
        assert_eq!(thousands(8), "8");
        assert_eq!(thousands(1284), "1,284");
        assert_eq!(thousands(1_000_000), "1,000,000");
    }

    #[test]
    fn number_columns_are_found() {
        let rows = vec![
            vec!["main".to_string(), "2:41".to_string(), "—".to_string()],
            vec!["fix".to_string(), "3:05".to_string(), String::new()],
        ];
        assert!(!is_numeric_column(&rows, 0));
        assert!(is_numeric_column(&rows, 1));
        assert!(is_numeric_column(&rows, 2));
    }
}
