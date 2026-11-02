//! One renderer per card block type. Every string except `text` blocks is
//! plain text (docs/protocol.md). Long tables and lists show the first rows
//! and a button to show the rest, so a 200-row card stays cheap to draw.

use gpui::{
    AnyElement, App, Div, ElementId, FontWeight, InteractiveElement as _, IntoElement, ObjectFit,
    ParentElement, StatefulInteractiveElement as _, Styled, StyledImage as _, Window, div, img,
    prelude::FluentBuilder as _, px, relative,
};
use gpui_component::{
    ActiveTheme as _, Disableable as _, Icon, IconName, Sizable as _,
    button::{Button, ButtonVariants as _},
    progress::Progress,
    separator::Separator,
};

use super::{is_expanded, part_id, toggle_expanded};
use crate::{
    copy::{cards as c, fill},
    data::{
        Message,
        card::{CardBlock, CardField, CardListItem, CardListStatus, LinkTarget},
    },
    markdown, theme,
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
    match block {
        CardBlock::Text { text } => {
            markdown::message_body(ElementId::Name(key.into()), text.clone())
                .w_full()
                .into_any_element()
        }
        CardBlock::Fields { fields } => render_fields(fields, cx),
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
            render_image(message, ctx, file_id, alt.as_deref(), cx)
        }
        CardBlock::Divider => Separator::horizontal().into_any_element(),
        CardBlock::Link { label, target } => render_link(&key, message, label, target),
        CardBlock::Unknown => div().into_any_element(),
    }
}

fn render_fields(fields: &[CardField], cx: &App) -> AnyElement {
    let colors = theme::colors(cx);
    div()
        .flex()
        .flex_wrap()
        .gap_y(px(8.))
        .children(fields.iter().map(|f| {
            div()
                .w(relative(0.5))
                .min_w(px(160.))
                .pr(px(12.))
                .flex()
                .flex_col()
                .gap(px(2.))
                .child(
                    div()
                        .text_xs()
                        .text_color(colors.muted_foreground)
                        .child(f.label.clone()),
                )
                .child(
                    div()
                        .text_sm()
                        .text_color(colors.foreground)
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
    Button::new(ElementId::Name(format!("{key}-more").into()))
        .xsmall()
        .ghost()
        .label(label)
        .on_click(move |_, window, cx| toggle_expanded(toggle_key.clone(), window, cx))
}

fn render_table(key: &str, columns: &[String], rows: &[Vec<String>], cx: &App) -> AnyElement {
    let colors = theme::colors(cx);
    let widths: Vec<f32> = (0..columns.len())
        .map(|i| column_width(columns, rows, i))
        .collect();
    let expanded = is_expanded(key, cx);
    let shown = if expanded {
        rows.len()
    } else {
        rows.len().min(COLLAPSED_ROWS)
    };
    let hidden = rows.len() - rows.len().min(COLLAPSED_ROWS);

    let cell = |text: String, width: f32| -> Div {
        div()
            .w(px(width))
            .flex_none()
            .px(px(8.))
            .py(px(4.))
            .child(text)
    };
    let header = div()
        .flex()
        .bg(colors.code_background)
        .text_xs()
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(colors.foreground)
        .children(
            columns
                .iter()
                .zip(&widths)
                .map(|(name, w)| cell(name.clone(), *w)),
        );
    let body = rows[..shown].iter().map(|row| {
        div()
            .flex()
            .border_t_1()
            .border_color(colors.border)
            .text_xs()
            .text_color(colors.foreground)
            .children(
                widths
                    .iter()
                    .enumerate()
                    .map(|(i, w)| cell(row.get(i).cloned().unwrap_or_default(), *w)),
            )
    });
    div()
        .flex()
        .flex_col()
        .gap(px(4.))
        .child(
            div()
                .id(ElementId::Name(format!("{key}-scroll").into()))
                .w_full()
                .overflow_x_scroll()
                .rounded(cx.theme().radius)
                .border_1()
                .border_color(colors.border)
                .child(div().flex().flex_col().child(header).children(body)),
        )
        .when(hidden > 0, |el| {
            el.child(div().flex().child(more_button(key, hidden, expanded)))
        })
        .into_any_element()
}

fn list_icon(status: Option<CardListStatus>, cx: &App) -> AnyElement {
    let t = cx.theme();
    let (icon, color) = match status {
        None => {
            return div()
                .size(px(6.))
                .mx(px(4.))
                .rounded_full()
                .bg(t.muted_foreground)
                .into_any_element();
        }
        Some(CardListStatus::Todo) => {
            return div()
                .size(px(12.))
                .mx(px(1.))
                .rounded_full()
                .border_1()
                .border_color(t.muted_foreground)
                .into_any_element();
        }
        Some(CardListStatus::InProgress) => (IconName::LoaderCircle, t.blue),
        Some(CardListStatus::Done) => (IconName::CircleCheck, t.green),
        Some(CardListStatus::Failed) => (IconName::CircleX, t.red),
        Some(CardListStatus::Skipped) => (IconName::Minus, t.muted_foreground),
    };
    Icon::new(icon).small().text_color(color).into_any_element()
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

fn render_list(key: &str, items: &[CardListItem], cx: &App) -> AnyElement {
    let colors = theme::colors(cx);
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
        .gap(px(4.))
        .children(items[..shown].iter().map(|item| {
            let skipped = item.status == Some(CardListStatus::Skipped);
            div()
                .flex()
                .items_start()
                .gap(px(6.))
                .text_sm()
                .child(
                    div()
                        .h(px(20.))
                        .w(px(14.))
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
                        .text_color(if skipped {
                            colors.muted_foreground
                        } else {
                            colors.foreground
                        })
                        .when(skipped, |el| el.line_through())
                        .child(item.text.clone()),
                )
                .children(item.status.map(|s| {
                    div()
                        .flex_none()
                        .text_xs()
                        .text_color(colors.muted_foreground)
                        .child(list_status_text(s))
                }))
        }))
        .when(hidden > 0, |el| {
            el.child(div().flex().child(more_button(key, hidden, expanded)))
        })
        .into_any_element()
}

fn render_progress(key: &str, value: f64, label: Option<&str>, cx: &App) -> AnyElement {
    let colors = theme::colors(cx);
    let percent = (value * 100.0).round();
    div()
        .flex()
        .flex_col()
        .gap(px(4.))
        .child(
            div()
                .flex()
                .text_xs()
                .text_color(colors.muted_foreground)
                .child(
                    div()
                        .flex_1()
                        .min_w(px(0.))
                        .children(label.map(str::to_string)),
                )
                .child(div().flex_none().child(format!("{percent}%"))),
        )
        .child(Progress::new(ElementId::Name(format!("{key}-bar").into())).value(percent as f32))
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
        .rounded(cx.theme().radius)
        .border_1()
        .border_color(colors.border)
        .bg(colors.code_background)
        .overflow_hidden()
        .flex()
        .items_center()
        .justify_center();
    let placeholder = |text: String| {
        div()
            .flex()
            .flex_col()
            .items_center()
            .gap(px(6.))
            .px(px(12.))
            .text_xs()
            .text_color(colors.muted_foreground)
            .child(Icon::new(IconName::Frame).text_color(colors.muted_foreground))
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
                            div().text_xs().child(fallback.clone()).into_any_element()
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

fn render_link(key: &str, message: &Message, label: &str, target: &LinkTarget) -> AnyElement {
    div()
        .flex()
        .child(link_button(key, message, label, target))
        .into_any_element()
}

fn link_button(key: &str, message: &Message, label: &str, target: &LinkTarget) -> AnyElement {
    let id = ElementId::Name(format!("{key}-link").into());
    let button = Button::new(id).small().link().label(label.to_string());
    match target {
        LinkTarget::Url { url } => match markdown::safe_link(url) {
            Some(safe) if safe.starts_with("http") || safe.starts_with("HTTP") => {
                let url = safe.to_string();
                button
                    .icon(IconName::ExternalLink)
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
                .icon(IconName::ArrowRight)
                .tooltip(c::OPEN_TOPIC)
                .on_click(move |_, _, cx| crate::views::nav::open_topic(g.clone(), t.clone(), cx))
                .into_any_element()
        }
        LinkTarget::Artifact { .. } => button
            .icon(IconName::FileText)
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
        assert_eq!(column_width(&cols, &rows, 0), 72.0);
        assert_eq!(column_width(&cols, &rows, 1), 240.0);
        assert_eq!(column_width(&cols, &rows, 5), 72.0);
    }
}
