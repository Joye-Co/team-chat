//! Small pieces the settings pages share: section headers, rows, the error
//! text of a request, the one-time secret block, and helpers for replies.

use gpui::{
    AnyElement, App, Div, ElementId, IntoElement, ParentElement, Pixels, SharedString, Styled,
    Window, div, prelude::FluentBuilder as _, px,
};
use gpui_component::{Sizable as _, button::Button, clipboard::Clipboard};

use crate::{
    copy::{fill, settings as s},
    data::{AdminError, AdminReply, MemberLevel, Millis},
    theme::{self, Type as _, space},
    ui::Sym,
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

/// The column every page sits in (DESIGN.md §8.10): at most 664 pt including
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

/// The content column of the settings window: 600 pt of content plus 32 pt
/// on each side.
pub const COLUMN_MAX: Pixels = theme::size::SETTINGS_COLUMN;

/// Column widths of the settings tables (§8.4).
pub const STATUS_COL: Pixels = px(152.);
/// 成员 has one status line, `等人回答` at most (8 pt dot, 8 pt gap, four
/// characters): 96 pt fits it, and the name column takes the rest.
pub const MEMBER_STATUS_COL: Pixels = px(96.);
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

/// A settings list row (§8.10, macOS System Settings): the label at the
/// left (`ui` 14/20 `text-1`) with an optional second line under it, the
/// control at the right. At least 52 pt, padding 8 12 8 16. Put the rows in
/// one [`table_of`] group, which draws the hairlines between them.
pub fn list_row(
    label: impl Into<SharedString>,
    detail: Option<AnyElement>,
    control: Option<AnyElement>,
    cx: &App,
) -> Div {
    let k = theme::colors(cx);
    div()
        .flex_none()
        .w_full()
        .min_h(theme::size::SETTINGS_LIST_ROW)
        .pl(space::S16)
        .pr(space::S12)
        .py(space::S8)
        .flex()
        .items_center()
        .gap(space::S16)
        .child(
            div()
                .flex_1()
                .min_w_0()
                .flex()
                .flex_col()
                .gap(space::S2)
                .child(div().t_ui().text_color(k.text_1).child(label.into()))
                .children(detail),
        )
        .children(control.map(|c| div().flex_none().flex().items_center().child(c)))
}

/// The second line of a [`list_row`]: `small` 13/20 `text-2`.
pub fn row_detail(text: impl Into<SharedString>, cx: &App) -> AnyElement {
    let k = theme::colors(cx);
    div()
        .t_small()
        .text_color(k.text_2)
        .child(text.into())
        .into_any_element()
}

/// A read-only value at the right of a [`list_row`]: `ui` `text-2`, one
/// line, at most 320 pt.
pub fn row_value(text: impl Into<SharedString>, cx: &App) -> AnyElement {
    let k = theme::colors(cx);
    div()
        .max_w(theme::size::SETTINGS_VALUE_MAX)
        .t_ui()
        .text_color(k.text_2)
        .one_line()
        .child(text.into())
        .into_any_element()
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
        crate::ui::native::NativeTextField::new(id, state).full_width(),
        cx,
    )
}

/// A help line: 12/16 `text-3`.
pub fn hint(text: impl Into<SharedString>, cx: &App) -> Div {
    let k = theme::colors(cx);
    div().t_meta().text_color(k.text_3).child(text.into())
}

/// The result of a request: 12/16, `danger-text` for an error, `text-3`
/// otherwise; with a 14 pt spinner while busy. The spinner waits 400 ms and
/// then stays at least 500 ms (§12), so a fast save shows only its result.
/// `id` is stable for the line.
pub fn status_line(id: impl Into<ElementId>, status: &Status, cx: &App) -> Option<Div> {
    let k = theme::colors(cx);
    let line = move |text: SharedString, color| {
        div()
            .flex()
            .items_center()
            .gap(space::S6)
            .t_meta()
            .text_color(color)
            .child(text)
    };
    let (text, color, failed) = match status {
        Status::Idle => return None,
        Status::Busy(text) => (text.clone(), k.text_3, false),
        Status::Done(text) => (text.clone(), k.text_3, false),
        Status::Failed(text) => (text.clone(), k.danger_text, true),
    };
    let busy = status.busy();
    let gate = crate::ui::motion::busy_gate(id, busy, move |shown| {
        if shown {
            div()
                .flex()
                .items_center()
                .gap(space::S6)
                .child(
                    crate::ui::spinner()
                        .with_size(theme::icon::CHIP)
                        .color(k.text_3),
                )
                .child(line(text, color))
                .into_any_element()
        } else if busy {
            div().into_any_element()
        } else if failed {
            line(text, color)
                .child(crate::ui::icon(Sym::Alert, theme::icon::CHIP, k.danger))
                .into_any_element()
        } else {
            line(text, color).into_any_element()
        }
    });
    Some(div().child(gate))
}

/// An inline load error (§8.5): `circle-alert` in `danger`, the message in
/// `ui` `text-1`, and a `sm` 重试 at the trailing edge, as a table row.
pub fn error_row(text: impl Into<SharedString>, retry: Button, cx: &App) -> Div {
    let k = theme::colors(cx);
    table(cx).child(
        table_row()
            .child(crate::ui::icon(Sym::Alert, theme::icon::ROW, k.danger))
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
    crate::ui::native::select_button(id, label, crate::ui::native::PushSize::Regular, cx)
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

/// A segmented control (§8.4, 通知): the one Segmented (K09).
pub fn choice(
    id: impl Into<ElementId>,
    options: &[&'static str],
    selected: usize,
    disabled: bool,
    on_pick: impl Fn(usize, &mut Window, &mut App) + 'static,
    _cx: &App,
) -> Div {
    use crate::ui::native::{Segment, Segmented};
    div().flex().child(
        Segmented::new(
            id,
            options
                .iter()
                .map(|l| Segment::Label(SharedString::from(*l))),
        )
        .selected(selected)
        .disabled(disabled)
        .on_change(on_pick),
    )
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

// --- v12/settings track: native pane parts (NATIVE.md §5, §7, §8) ---

/// The body of a pane (§8): 20 pt side margins, groups 20 pt apart.
pub fn pane() -> Div {
    use crate::ui::native::metrics;
    div()
        .w_full()
        .px(metrics::GROUP_MARGIN)
        .pt(px(20.))
        .pb(px(20.))
        .flex()
        .flex_col()
        .gap(metrics::GROUP_GAP)
}

/// A quiet line under a group or in a sheet (Subheadline): `secondary`, or
/// `red` when the request failed. `None` when idle.
pub fn native_status(status: &Status, cx: &App) -> Option<Div> {
    use crate::ui::native::{NativeType as _, native_colors};
    let k = native_colors(cx);
    let (text, color) = match status {
        Status::Idle => return None,
        Status::Busy(t) | Status::Done(t) => (t.clone(), k.secondary),
        Status::Failed(t) => (t.clone(), k.red),
    };
    Some(
        div()
            .px(px(2.))
            .n_subheadline()
            .text_color(color)
            .child(text),
    )
}

/// A `secondary` Subheadline line (a footer of my own, a field's help).
pub fn quiet(text: impl Into<SharedString>, cx: &App) -> Div {
    use crate::ui::native::{NativeType as _, native_colors};
    div()
        .px(px(2.))
        .n_subheadline()
        .text_color(native_colors(cx).secondary)
        .child(text.into())
}

/// A field error under a standalone field: `red` Subheadline.
pub fn field_error(text: impl Into<SharedString>, cx: &App) -> Div {
    use crate::ui::native::{NativeType as _, native_colors};
    div()
        .n_subheadline()
        .text_color(native_colors(cx).red)
        .child(text.into())
}

/// A state dot and its text (§1 rule 4: a dot, never a pill).
pub fn dot_text(color: Option<gpui::Hsla>, text: impl Into<SharedString>, cx: &App) -> Div {
    use crate::ui::native::{NativeType as _, native_colors};
    let k = native_colors(cx);
    div()
        .flex()
        .items_center()
        .gap(crate::ui::native::tokens::space::S6)
        .child(
            div()
                .flex_none()
                .size(px(7.))
                .rounded_full()
                .bg(color.unwrap_or(k.tertiary)),
        )
        .child(
            div()
                .n_body()
                .text_color(k.secondary)
                .whitespace_nowrap()
                .child(text.into()),
        )
}

/// The 48 pt app icon of an alert (§7.2).
pub fn app_icon(cx: &App) -> AnyElement {
    let icon = if theme::is_dark(cx) {
        crate::ui::icons::APP_ICON_DARK_72
    } else {
        crate::ui::icons::APP_ICON_72
    };
    gpui::img(icon)
        .flex_none()
        .size(crate::ui::native::metrics::ALERT_ICON)
        .into_any_element()
}

/// Opens a sheet attached under the settings toolbar (§7.1): Escape is
/// 取消, the window grows to `floor` while it is up. `build` lays out the
/// inside with `SheetLayout` on the configured dialog.
pub fn open_sheet(
    window: &mut Window,
    cx: &mut App,
    width: crate::ui::native::SheetWidth,
    floor: Pixels,
    build: impl Fn(
        gpui_component::dialog::Dialog,
        &mut Window,
        &mut App,
    ) -> gpui_component::dialog::Dialog
    + 'static,
) {
    use gpui_component::WindowExt as _;
    super::window_fit::sheet_floor(floor);
    window.open_dialog(cx, move |dialog, window, cx| {
        let dialog = crate::ui::native::native_sheet(dialog, width, cx)
            .margin_top(super::TOOLBAR_HEIGHT)
            .px(px(0.));
        build(dialog, window, cx)
    });
}

/// Opens a Tahoe alert (§7.2) on the settings window: 260 wide, attached
/// under the toolbar like a sheet. `build` returns the `AlertLayout`.
pub fn open_alert(
    window: &mut Window,
    cx: &mut App,
    build: impl Fn(&mut Window, &mut App) -> AnyElement + 'static,
) {
    use crate::ui::native::metrics;
    use gpui_component::WindowExt as _;
    super::window_fit::sheet_floor(px(420.));
    window.open_dialog(cx, move |dialog, window, cx| {
        crate::ui::native::native_sheet(dialog, crate::ui::native::SheetWidth::Form, cx)
            .width(metrics::ALERT_WIDTH)
            .margin_top(super::TOOLBAR_HEIGHT)
            .px(px(0.))
            .child(build(window, cx))
    });
}

/// A pop-up button (§5.3) of a fixed width, for a column of pop-ups: the
/// title at the leading edge, the `chevron.up.chevron.down` at the trailing
/// edge, so the marks line up row under row.
pub fn popup_fixed(
    id: impl Into<ElementId>,
    title: impl Into<SharedString>,
    width: Pixels,
    window: &Window,
    cx: &App,
) -> Button {
    use crate::ui::{
        native::{PushSize, PushStyle, metrics, native_colors, push_button},
        sf_symbols::{Sf, Weight, symbol},
    };
    let k = native_colors(cx);
    push_button(id, "", PushStyle::Outline, PushSize::Regular, cx)
        .w(width)
        .pl(metrics::BUTTON_REGULAR_PAD)
        .pr(metrics::FIELD_PAD)
        .child(
            div()
                .w_full()
                .flex()
                .items_center()
                .justify_between()
                .gap(crate::ui::native::tokens::space::S6)
                .child(
                    div()
                        .min_w(px(0.))
                        .overflow_hidden()
                        .whitespace_nowrap()
                        .child(title.into()),
                )
                .child(symbol(
                    Sf::Popup,
                    metrics::POPUP_CHEVRON_SLOT,
                    Weight::Semibold,
                    k.secondary,
                    window,
                )),
        )
}

/// A row under a list group: the `+`/`−` buttons, then a quiet line.
pub fn list_controls(buttons: impl IntoElement, note: impl Into<SharedString>, cx: &App) -> Div {
    div()
        .flex()
        .items_center()
        .gap(crate::ui::native::tokens::space::S12)
        .child(buttons)
        .child(quiet(note, cx))
}

/// A pop-up of the hours of the day (and `current` when it is not on the
/// hour), checked at `current`; `pick` gets the chosen time.
pub fn time_popup(
    id: impl Into<ElementId>,
    current: crate::data::ClockTime,
    disabled: bool,
    pick: impl Fn(crate::data::ClockTime, &mut Window, &mut App) + 'static,
    window: &Window,
    cx: &App,
) -> impl IntoElement {
    use crate::data::ClockTime;
    use crate::ui::native::{PushSize, disabled_push, popup_button};
    use gpui_component::menu::{DropdownMenu as _, PopupMenuItem};
    let pick = std::rc::Rc::new(pick);
    let mut times: Vec<ClockTime> = (0..24).map(|h| ClockTime::new(h, 0)).collect();
    if !times.contains(&current) {
        times.push(current);
        times.sort();
    }
    disabled_push(
        popup_button(id, current.wire(), PushSize::Regular, window, cx),
        disabled,
        cx,
    )
    .dropdown_menu(move |menu, _, _| {
        times
            .iter()
            .fold(menu.scrollable(true).max_h(px(264.)), |menu, t| {
                let (t, pick) = (*t, pick.clone());
                menu.item(
                    PopupMenuItem::new(t.wire())
                        .checked(t == current)
                        .on_click(move |_, window, cx| pick(t, window, cx)),
                )
            })
    })
}

// --- end v12/settings track ---

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
