//! Small pieces the settings pages share: section headers, rows, the error
//! text of a request, the one-time secret block, and helpers for replies.

use gpui::{
    AnyElement, App, Div, ElementId, IntoElement, ParentElement, Pixels, SharedString, Styled,
    Window, div, prelude::FluentBuilder as _, px,
};
use gpui_component::{
    Disableable as _, Sizable as _,
    button::{Button, ButtonCustomVariant, ButtonVariants as _},
    clipboard::Clipboard,
    spinner::Spinner,
};

use crate::{
    copy::{fill, settings as s},
    data::{AdminError, AdminReply, MemberLevel, Millis},
    theme::{self, Elevation, Type as _, space},
    ui::Lucide,
};

/// Waits for a reply off the UI thread. A dropped sender is [`AdminError::Other`].
pub async fn answer<T>(reply: AdminReply<T>) -> Result<T, AdminError> {
    reply.await.unwrap_or(Err(AdminError::Other))
}

/// The copy text of a failed request.
pub fn error_text(e: AdminError) -> &'static str {
    match e {
        AdminError::Unsupported => s::ERROR_UNSUPPORTED,
        AdminError::Offline => s::ERROR_OFFLINE,
        AdminError::Forbidden => s::ERROR_FORBIDDEN,
        AdminError::NotFound => s::ERROR_NOT_FOUND,
        AdminError::LastOwner => s::ERROR_LAST_OWNER,
        AdminError::Invalid => s::ERROR_INVALID,
        AdminError::TooLarge => s::ERROR_TOO_LARGE,
        AdminError::RateLimited => s::ERROR_RATE_LIMITED,
        AdminError::Other => s::ERROR_OTHER,
    }
}

pub fn level_text(level: MemberLevel) -> &'static str {
    match level {
        MemberLevel::Owner => s::LEVEL_OWNER,
        MemberLevel::Member => s::LEVEL_MEMBER,
    }
}

/// Splits labels typed as `a, b，c d` into trimmed, unique labels.
pub fn split_labels(text: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for label in text.split([',', '，', ' ', '、']) {
        let label = label.trim();
        if !label.is_empty() && !out.iter().any(|l| l == label) {
            out.push(label.to_string());
        }
    }
    out
}

/// `12 GB`, or `512 MB` under 1 GB.
pub fn size_text(bytes: u64) -> String {
    const GB: u64 = 1024 * 1024 * 1024;
    const MB: u64 = 1024 * 1024;
    if bytes >= GB {
        let gb = bytes as f64 / GB as f64;
        let n = if gb >= 10.0 {
            format!("{gb:.0}")
        } else {
            format!("{gb:.1}")
        };
        fill(s::SIZE_GB, &[("n", &n)])
    } else {
        fill(s::SIZE_MB, &[("n", &(bytes / MB).to_string())])
    }
}

/// `2026年11月10日 14:05` in local time.
pub fn date_text(at: Millis) -> String {
    use chrono::TimeZone as _;
    match chrono::Local.timestamp_millis_opt(at) {
        chrono::LocalResult::Single(t) => t.format("%Y年%-m月%-d日 %H:%M").to_string(),
        _ => String::new(),
    }
}

/// The text of `n` characters at most, or the error.
pub fn check_len(text: &str, max: usize) -> Result<(), String> {
    if text.chars().count() > max {
        Err(fill(s::TOO_LONG, &[("n", &max.to_string())]))
    } else {
        Ok(())
    }
}

/// The column every page sits in (DESIGN.md §8.4): at most 704 pt including
/// 32 pt side padding, centered in the content area; the page title
/// (`display` 22/30) 36 pt from the top with the page actions at its right,
/// then the page body 20 pt under it.
pub fn page(
    title: &'static str,
    actions: Option<AnyElement>,
    body: impl IntoElement,
    cx: &App,
) -> Div {
    let k = theme::colors(cx);
    div().w_full().flex().justify_center().child(
        div()
            .w_full()
            .max_w(COLUMN_MAX)
            .px(space::S32)
            .pt(px(36.))
            .pb(space::S44)
            .flex()
            .flex_col()
            .child(
                div()
                    .h(theme::size::BUTTON_PAGE)
                    .flex()
                    .items_center()
                    .gap(space::S8)
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .t_display()
                            .text_color(k.text_1)
                            .one_line()
                            .child(title),
                    )
                    .children(actions.map(|a| {
                        div()
                            .flex_none()
                            .flex()
                            .items_center()
                            .gap(space::S8)
                            .child(a)
                    })),
            )
            .child(div().pt(space::S20).flex().flex_col().child(body)),
    )
}

/// The content column of the settings window: 640 pt of content plus 32 pt
/// on each side.
pub const COLUMN_MAX: Pixels = px(704.);

/// Column widths of the settings tables (§8.4).
pub const STATUS_COL: Pixels = px(152.);
pub const LEVEL_COL: Pixels = px(132.);
pub const TRAILING_COL: Pixels = px(40.);

/// A grouped table: radius 12, 1 pt `border`, `content` fill. Its header row
/// sits inside it (U41).
pub fn table(cx: &App) -> Div {
    let k = theme::colors(cx);
    div()
        .w_full()
        .flex()
        .flex_col()
        .rounded(theme::radius::LG)
        .border_1()
        .border_color(k.border)
        .bg(k.content)
        .overflow_hidden()
}

/// The header row inside a table: 36 pt, no fill, 12/16 500 `text-3`, a
/// 0.5 pt `sep` under it. Give it the same columns as the rows.
pub fn table_header(cx: &App) -> Div {
    let k = theme::colors(cx);
    div()
        .flex_none()
        .w_full()
        .h(theme::size::BUTTON_PAGE)
        .pl(space::S16)
        .pr(space::S12)
        .flex()
        .items_center()
        .t_meta()
        .medium()
        .text_color(k.text_3)
        .border_b(theme::HAIRLINE)
        .border_color(k.sep)
}

/// One table row: at least 60 pt, padding 0 12 0 16, gap 12.
pub fn table_row() -> Div {
    div()
        .flex_none()
        .w_full()
        .min_h(theme::size::SETTINGS_ROW)
        .pl(space::S16)
        .pr(space::S12)
        .py(space::S10)
        .flex()
        .items_center()
        .gap(space::S12)
}

/// A table of `rows` under an optional header, with 0.5 pt `sep` lines
/// between the rows.
pub fn table_of(header: Option<Div>, rows: Vec<Div>, cx: &App) -> Div {
    let k = theme::colors(cx);
    let n = rows.len();
    table(cx)
        .children(header)
        .children(rows.into_iter().enumerate().map(|(ix, row)| {
            row.when(ix + 1 < n, |r| {
                r.border_b(theme::HAIRLINE).border_color(k.sep)
            })
        }))
}

/// A table with one quiet line: 13/20 `text-3`, centered, 60 pt.
pub fn table_empty(header: Option<Div>, text: impl Into<SharedString>, cx: &App) -> Div {
    let k = theme::colors(cx);
    table(cx).children(header).child(
        table_row()
            .justify_center()
            .t_small()
            .text_color(k.text_3)
            .child(text.into()),
    )
}

/// A label above a table or a group of fields: 13/20 500 `text-2`, 8 pt
/// under it; 24 pt above it unless it opens the page.
pub fn section_label(text: impl Into<SharedString>, first: bool, cx: &App) -> Div {
    let k = theme::colors(cx);
    div()
        .when(!first, |el| el.pt(space::S24))
        .pb(space::S8)
        .t_small()
        .medium()
        .text_color(k.text_2)
        .one_line()
        .child(text.into())
}

/// A footnote under a table: `small` 13/20 `text-3`, 12 pt under it.
pub fn footnote(text: impl Into<SharedString>, cx: &App) -> Div {
    let k = theme::colors(cx);
    div()
        .pt(space::S12)
        .t_small()
        .text_color(k.text_3)
        .child(text.into())
}

/// A table cell with a title (`ui` 14/20 500) and an optional second line
/// (`small` 13/20 `text-2`). Fills the free width.
pub fn title_cell(title: impl Into<SharedString>, detail: Option<SharedString>, cx: &App) -> Div {
    let k = theme::colors(cx);
    div()
        .flex_1()
        .min_w_0()
        .flex()
        .flex_col()
        .child(
            div()
                .t_ui()
                .medium()
                .text_color(k.text_1)
                .one_line()
                .child(title.into()),
        )
        .children(detail.map(|d| div().t_small().text_color(k.text_2).one_line().child(d)))
}

/// A form field with its label above (§7.4): label `small` 13/20 500
/// `text-2` 8 pt above, help or error 12/16 6 pt under.
pub fn field(label: &'static str, control: impl IntoElement, cx: &App) -> Div {
    div()
        .w_full()
        .flex()
        .flex_col()
        .child(crate::ui::field_label(label, cx))
        .child(control)
}

/// A form field built from an input state: 40 pt, label above.
pub fn form_input(
    id: impl Into<ElementId>,
    label: &'static str,
    state: &gpui::Entity<gpui_component::input::InputState>,
    cx: &App,
) -> Div {
    field(
        label,
        crate::ui::TextField::new(id, state, crate::ui::FieldKind::Form),
        cx,
    )
}

/// A help line: 12/16 `text-3`.
pub fn hint(text: impl Into<SharedString>, cx: &App) -> Div {
    let k = theme::colors(cx);
    div().t_meta().text_color(k.text_3).child(text.into())
}

/// The result of a request: 12/16, `danger-text` for an error, `text-3`
/// otherwise; with a 14 pt spinner while busy.
pub fn status_line(status: &Status, cx: &App) -> Option<Div> {
    let k = theme::colors(cx);
    let line = |text: SharedString, color| {
        div()
            .flex()
            .items_center()
            .gap(space::S6)
            .t_meta()
            .text_color(color)
            .child(text)
    };
    match status {
        Status::Idle => None,
        Status::Busy(text) => Some(
            div()
                .flex()
                .items_center()
                .gap(space::S6)
                .child(Spinner::new().with_size(theme::icon::CHIP).color(k.text_3))
                .child(line(text.clone(), k.text_3)),
        ),
        Status::Done(text) => Some(line(text.clone(), k.text_3)),
        Status::Failed(text) => Some(line(text.clone(), k.danger_text).child(crate::ui::icon(
            Lucide::CircleAlert,
            theme::icon::CHIP,
            k.danger,
        ))),
    }
}

/// An inline load error (§8.5): `circle-alert` in `danger`, the message in
/// `ui` `text-1`, and a `sm` 重试 at the trailing edge, as a table row.
pub fn error_row(text: impl Into<SharedString>, retry: Button, cx: &App) -> Div {
    let k = theme::colors(cx);
    table(cx).child(
        table_row()
            .child(crate::ui::icon(
                Lucide::CircleAlert,
                theme::icon::ROW,
                k.danger,
            ))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .t_ui()
                    .text_color(k.text_1)
                    .child(text.into()),
            )
            .child(retry),
    )
}

/// Skeleton rows while a table loads (§7.1 Loading): `field` bars in the
/// shape of a row, shown after the 400 ms busy delay by the caller.
pub fn skeleton_table(rows: usize, cx: &App) -> Div {
    let k = theme::colors(cx);
    let bar = |w: Pixels, h: Pixels| div().w(w).h(h).rounded(theme::radius::SM).bg(k.field);
    table_of(
        None,
        (0..rows)
            .map(|ix| {
                table_row()
                    .child(
                        div()
                            .size(theme::size::AVATAR_MESSAGE)
                            .rounded(theme::radius::MD)
                            .bg(k.field),
                    )
                    .child(
                        div()
                            .flex_1()
                            .flex()
                            .flex_col()
                            .gap(space::S6)
                            .child(bar(px(140. - 24. * ix as f32), px(12.)))
                            .child(bar(px(220. - 32. * ix as f32), px(10.))),
                    )
            })
            .collect(),
        cx,
    )
}

/// A select in a table row (§7.4): a gpui-component button drawn as the
/// 36 x 120 select, `content` + 1 pt `border`, radius 8, `ui` 14 text and a
/// trailing `chevrons-up-down` 14 in `text-3`. Open its menu with
/// `.dropdown_menu`.
pub fn select_button(id: impl Into<ElementId>, label: impl Into<SharedString>, cx: &App) -> Button {
    let k = theme::colors(cx);
    Button::new(id)
        .font_weight(gpui::FontWeight::NORMAL)
        .custom(
            ButtonCustomVariant::new(cx)
                .color(k.content)
                .foreground(k.text_1)
                .hover(k.hover)
                .active(k.pressed),
        )
        .h(theme::size::SELECT)
        .w(theme::size::SELECT_WIDTH)
        .pl(space::S12)
        .pr(space::S8)
        .rounded(theme::radius::MD)
        .border_1()
        .border_color(k.border)
        .child(
            div()
                .flex_1()
                .min_w_0()
                .t_ui()
                .font_weight(gpui::FontWeight::NORMAL)
                .one_line()
                .child(label.into()),
        )
        .child(crate::ui::icon(
            Lucide::ChevronsUpDown,
            theme::icon::CHIP,
            k.text_3,
        ))
}

/// A presence dot of the settings tables (§8.4): 8 pt; filled for a live
/// state, a 1.5 pt `text-3` ring when offline.
pub fn state_dot(color: Option<gpui::Hsla>, cx: &App) -> Div {
    let k = theme::colors(cx);
    let dot = div().flex_none().size(space::S8).rounded_full();
    match color {
        Some(c) => dot.bg(c),
        None => dot.border(px(1.5)).border_color(k.text_3),
    }
}

/// The status cell (152 pt): an 8 pt dot and `ui` `text-2`, and an optional
/// second line 12/16 `text-3` (a machine name), the dot on line 1.
pub fn status_cell(
    dot: Div,
    text: impl Into<SharedString>,
    second: Option<SharedString>,
    cx: &App,
) -> Div {
    let k = theme::colors(cx);
    div()
        .w(STATUS_COL)
        .flex_none()
        .flex()
        .items_start()
        .gap(space::S8)
        .child(div().h(px(20.)).flex().items_center().child(dot))
        .child(
            div()
                .min_w_0()
                .flex()
                .flex_col()
                .child(
                    div()
                        .t_ui()
                        .text_color(k.text_2)
                        .one_line()
                        .child(text.into()),
                )
                .children(second.map(|s| div().t_meta().text_color(k.text_3).one_line().child(s))),
        )
}

/// The state of one request on a page.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum Status {
    #[default]
    Idle,
    Busy(SharedString),
    Done(SharedString),
    Failed(SharedString),
}

impl Status {
    pub fn busy(&self) -> bool {
        matches!(self, Status::Busy(_))
    }

    pub fn from_error(e: AdminError) -> Self {
        Status::Failed(error_text(e).into())
    }
}

/// A one-time secret: its label above, the value in a `code-bg` box (radius
/// 10, padding 12, Menlo 13/20) with a copy button. The value is never logged.
pub fn secret_block(id: impl Into<ElementId>, label: &'static str, value: &str, cx: &App) -> Div {
    let k = theme::colors(cx);
    let value: SharedString = value.to_string().into();
    field(
        label,
        div()
            .w_full()
            .flex()
            .items_start()
            .gap(space::S8)
            .pl(space::S12)
            .pr(space::S6)
            .py(space::S6)
            .rounded(theme::radius::FIELD)
            .bg(k.code_bg)
            .border_1()
            .border_color(k.border)
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .py(space::S4)
                    .t_code()
                    .text_color(k.code_text)
                    .child(value.clone()),
            )
            .child(Clipboard::new(id).tooltip(s::COPY).value(value)),
        cx,
    )
}

/// A segmented control (§8.4, 通知): 32 pt, `field` track with 2 pt inset,
/// 28 pt segments; the selected one has a `content` fill with the level-1
/// shadow. Each segment is a gpui-component button; `on_pick` gets the index.
pub fn choice(
    id: impl Into<ElementId>,
    options: &[&'static str],
    selected: usize,
    disabled: bool,
    on_pick: impl Fn(usize, &mut Window, &mut App) + 'static,
    cx: &App,
) -> Div {
    let k = theme::colors(cx);
    let id: ElementId = id.into();
    let on_pick = std::rc::Rc::new(on_pick);
    // The wrapper keeps the track at its own width in a stretching column.
    let track = div()
        .flex_none()
        .h(theme::size::BUTTON_MD)
        .p(space::S2)
        .rounded(theme::radius::MD)
        .bg(k.field)
        .flex()
        .items_center()
        .gap(space::S2)
        .when(disabled, |el| el.opacity(0.4))
        .children(options.iter().enumerate().map(|(ix, label)| {
            let on = ix == selected;
            let pick = on_pick.clone();
            Button::new(ElementId::NamedInteger(
                SharedString::from(format!("{id}-opt")),
                ix as u64,
            ))
            .custom(
                ButtonCustomVariant::new(cx)
                    .color(if on {
                        k.overlay
                    } else {
                        gpui::transparent_black()
                    })
                    .foreground(if on { k.text_1 } else { k.text_2 })
                    .hover(if on { k.overlay } else { k.hover })
                    .active(if on { k.overlay } else { k.pressed }),
            )
            .label(*label)
            .h(theme::size::BUTTON_SM)
            .px(space::S12)
            .rounded(theme::radius::SM)
            .text_size(theme::type_scale::SMALL.0)
            .line_height(theme::type_scale::SMALL.1)
            .when(on, |b| {
                b.bg(k.overlay)
                    .font_weight(gpui::FontWeight::MEDIUM)
                    .shadow(k.shadow(Elevation::Float))
            })
            .toggled(on)
            .disabled(disabled)
            .when(!disabled && !on, |b| {
                b.on_click(move |_, window, cx| pick(ix, window, cx))
            })
        }));
    div().flex().child(track)
}

/// A dialog footer (§7.12): right-aligned `md` buttons, secondary then
/// primary, 20 pt above.
pub fn dialog_footer(buttons: Vec<Button>) -> Div {
    div()
        .pt(space::S4)
        .flex()
        .justify_end()
        .gap(space::S8)
        .children(buttons)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn labels_split_on_any_separator() {
        assert_eq!(
            split_labels(" mac, office，gpu  mac 、x"),
            vec!["mac", "office", "gpu", "x"]
        );
        assert!(split_labels(" , ").is_empty());
    }

    #[test]
    fn sizes_read_well() {
        assert_eq!(size_text(212 * 1024 * 1024 * 1024), "212 GB");
        assert_eq!(size_text(3 * 1024 * 1024 * 1024 / 2), "1.5 GB");
        assert_eq!(size_text(512 * 1024 * 1024), "512 MB");
    }

    #[test]
    fn every_error_has_text() {
        for e in [
            AdminError::Unsupported,
            AdminError::Offline,
            AdminError::Forbidden,
            AdminError::NotFound,
            AdminError::LastOwner,
            AdminError::Invalid,
            AdminError::TooLarge,
            AdminError::RateLimited,
            AdminError::Other,
        ] {
            assert!(!error_text(e).is_empty());
        }
    }

    #[test]
    fn length_check() {
        assert!(check_len("一二三", 3).is_ok());
        assert_eq!(check_len("一二三四", 3), Err("不能超过 3 个字".to_string()));
    }
}
