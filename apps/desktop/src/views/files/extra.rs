//! The attachments of a message row: image previews and file rows.
//!
//! Every box has a fixed size from the message data (image pixel size, file
//! row height), so a preview that finishes loading or a progress bar that
//! ends never changes the row height.

use std::sync::Arc;

use gpui::{
    AnyElement, App, ElementId, InteractiveElement as _, IntoElement, ObjectFit, ParentElement,
    Pixels, SharedString, StatefulInteractiveElement as _, Styled, StyledImage as _, Window, div,
    img, prelude::FluentBuilder as _, px,
};
use gpui_component::{
    Disableable as _, Sizable as _,
    button::{Button, ButtonVariants as _},
    progress::Progress,
    tooltip::Tooltip,
};

use super::{
    AUTO_DOWNLOAD_MAX_BYTES, FILE_ROW_H, FILE_ROW_W, OpenAction, PANEL_ROW_H, error_text, open_file,
    preview_size, size_text,
};
use crate::{
    copy::{files as f, fill},
    data::{ChatProvider, FileAttachment, Message, TransferState, download_key, upload_key},
    provider::files::THUMB_SOURCE_MAX_BYTES,
    theme::{self, Elevation, Type as _, space},
    ui::{self, Lucide},
    views::timeline::extras::{ExtrasContext, MessageExtra},
};

/// Images and files attached to a message.
pub struct AttachmentsExtra;

impl MessageExtra for AttachmentsExtra {
    fn render(
        &self,
        message: &Message,
        ctx: &ExtrasContext,
        _window: &mut Window,
        cx: &mut App,
    ) -> Option<AnyElement> {
        if message.attachments.is_empty() {
            return None;
        }
        let row_key = if message.id.is_empty() {
            message.client_msg_id.as_str()
        } else {
            message.id.as_str()
        };
        let mut images = div().flex().flex_wrap().gap(space::S6);
        let mut files = div().flex().flex_col().gap(space::S6);
        let (mut n_images, mut n_files) = (0, 0);
        for (index, file) in message.attachments.iter().enumerate() {
            let id = |part: &str| -> ElementId {
                ElementId::Name(SharedString::from(format!("att-{row_key}-{index}-{part}")))
            };
            if !file.is_uploaded() {
                let state = ctx
                    .provider
                    .file_transfer(&upload_key(&message.client_msg_id, index))
                    .unwrap_or(TransferState::Queued);
                files = files.child(upload_row(file, state, id, cx));
                n_files += 1;
            } else if file.is_image() && file.size <= THUMB_SOURCE_MAX_BYTES {
                images = images.child(image_preview(message, file, ctx.provider, id, cx));
                n_images += 1;
            } else {
                files = files.child(file_row(message, file, ctx.provider, id, cx));
                n_files += 1;
            }
        }
        Some(
            div()
                .mt(space::S6)
                .flex()
                .flex_col()
                .gap(space::S6)
                .when(n_images > 0, |el| el.child(images))
                .when(n_files > 0, |el| el.child(files))
                .into_any_element(),
        )
    }
}

fn image_preview(
    message: &Message,
    file: &FileAttachment,
    provider: &Arc<dyn ChatProvider>,
    id: impl Fn(&str) -> ElementId,
    cx: &mut App,
) -> AnyElement {
    let colors = theme::colors(cx);
    let (w, h) = preview_size(file.width, file.height);
    let (g, t) = (message.group_id.clone(), message.topic_id.clone());
    let thumb = provider.thumbnail(&g, &t, file);
    let state = provider.file_transfer(&download_key(file));
    if thumb.is_none()
        && state.is_none()
        && file.size <= AUTO_DOWNLOAD_MAX_BYTES
        && provider.cached_file(file).is_none()
    {
        // Fetch it once for the preview; the provider shares the transfer.
        drop(provider.fetch_file(&g, &t, file));
    }
    let inner = match (&thumb, state) {
        (Some(path), _) => img(path.clone())
            .size_full()
            .rounded(theme::radius::ROW)
            .object_fit(ObjectFit::Cover)
            .into_any_element(),
        (None, Some(TransferState::Running { .. })) => Progress::new(id("progress"))
            .xsmall()
            .color(colors.accent)
            .value(state.map(|s| s.percent()).unwrap_or(0.))
            .w(w * 0.6)
            .into_any_element(),
        (None, Some(TransferState::Failed(e))) => div()
            .px(space::S12)
            .flex()
            .flex_col()
            .items_center()
            .gap(space::S6)
            .text_center()
            .t_callout()
            .text_color(colors.danger_text)
            .child(ui::icon(Lucide::CircleAlert, theme::icon::SMALL, colors.danger))
            .child(fill(f::DOWNLOAD_FAILED, &[("reason", error_text(e))]))
            .into_any_element(),
        _ => ui::icon(Lucide::Image, theme::icon::EMPTY, colors.text_3).into_any_element(),
    };
    let provider = provider.clone();
    let file2 = file.clone();
    // §13.5: radius 8 and an `image-border` ring; `fill-field` until it loads.
    div()
        .id(id("image"))
        .w(w)
        .h(h)
        .flex_none()
        .rounded(theme::radius::ROW)
        .overflow_hidden()
        .shadow(colors.shadow(Elevation::Image))
        .bg(colors.fill_field)
        .flex()
        .items_center()
        .justify_center()
        .cursor_pointer()
        .child(inner)
        .tooltip(move |window, cx| Tooltip::new(f::OPEN_IMAGE).build(window, cx))
        .on_click(move |_, _, cx| open_file(&provider, &g, &t, &file2, OpenAction::Open, cx))
        .into_any_element()
}

/// What a tile shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TileKind {
    /// An HTML 产出 or file: `accent-soft` with `file-code` in `accent`.
    Page,
    Image,
    Text,
    File,
}

impl TileKind {
    pub fn of(file: &FileAttachment) -> Self {
        let lower = file.name.to_ascii_lowercase();
        if super::is_html(&file.name) {
            Self::Page
        } else if file.is_image() || file.mime.starts_with("image/") {
            Self::Image
        } else if file.mime.starts_with("text/")
            || [".md", ".txt", ".csv", ".json", ".pdf", ".doc", ".docx"]
                .iter()
                .any(|e| lower.ends_with(e))
        {
            Self::Text
        } else {
            Self::File
        }
    }
}

/// A 40 pt (timeline) or 32 pt (side panel) file tile (§13.5, §13.7).
pub fn tile(kind: TileKind, size: Pixels, cx: &App) -> gpui::Div {
    let k = theme::colors(cx);
    let (bg, fg, glyph) = match kind {
        TileKind::Page => (k.accent_soft, k.accent, Lucide::FileCode),
        TileKind::Image => (k.fill_field, k.text_2, Lucide::Image),
        TileKind::Text => (k.fill_field, k.text_2, Lucide::FileText),
        TileKind::File => (k.fill_field, k.text_2, Lucide::File),
    };
    let radius = if size >= px(40.) {
        theme::radius::ROW
    } else {
        theme::radius::MD
    };
    let glyph_size = if size >= px(40.) {
        theme::icon::EMPTY
    } else {
        theme::icon::TOOLBAR
    };
    div()
        .size(size)
        .flex_none()
        .rounded(radius)
        .bg(bg)
        .flex()
        .items_center()
        .justify_center()
        .child(ui::icon(glyph, glyph_size, fg))
}

/// A file name on one line with a middle ellipsis that keeps the extension
/// (§12), with the full name in a tooltip.
pub fn file_name(id: ElementId, name: &str) -> gpui::Stateful<gpui::Div> {
    let full: SharedString = name.to_string().into();
    let tip = full.clone();
    div()
        .id(id)
        .min_w(px(0.))
        .overflow_hidden()
        .whitespace_nowrap()
        .text_ellipsis_middle()
        .tooltip(move |window, cx| Tooltip::new(tip.clone()).build(window, cx))
        .child(full)
}

/// The frame of a file in the timeline: a level-1 card with a 40 pt tile,
/// the name 13 / 600 and the line under it, then the buttons (§13.5).
fn row_frame(
    file: &FileAttachment,
    detail: AnyElement,
    actions: Vec<AnyElement>,
    id: &dyn Fn(&str) -> ElementId,
    cx: &App,
) -> gpui::Div {
    let colors = theme::colors(cx);
    ui::card(cx)
        .w(FILE_ROW_W)
        .max_w_full()
        .h(FILE_ROW_H)
        .flex_none()
        .px(space::S12)
        .flex()
        .items_center()
        .gap(space::S12)
        .child(tile(TileKind::of(file), px(40.), cx))
        .child(
            div()
                .flex_1()
                .min_w(px(0.))
                .flex()
                .flex_col()
                .child(
                    file_name(id("name"), &file.name)
                        .t_ui()
                        .semibold()
                        .text_color(colors.text_1),
                )
                .child(detail),
        )
        .child(
            div()
                .flex_none()
                .flex()
                .items_center()
                .gap(space::S6)
                .children(actions),
        )
}

/// The frame of a file row in the side panel: 46 pt, a 32 pt tile, the name
/// 13 / 500 and an 11 pt line (§13.7).
fn panel_frame(
    file: &FileAttachment,
    detail: AnyElement,
    actions: Vec<AnyElement>,
    id: &dyn Fn(&str) -> ElementId,
    cx: &App,
) -> gpui::Div {
    let colors = theme::colors(cx);
    div()
        .w_full()
        .h(PANEL_ROW_H)
        .flex()
        .items_center()
        .gap(space::S10)
        .child(tile(TileKind::of(file), px(32.), cx))
        .child(
            div()
                .flex_1()
                .min_w(px(0.))
                .flex()
                .flex_col()
                .gap(px(1.))
                .child(
                    file_name(id("name"), &file.name)
                        .t_ui()
                        .medium()
                        .text_color(colors.text_1),
                )
                .child(detail),
        )
        .child(div().flex_none().flex().items_center().children(actions))
}

/// The meta line under a name: 12 pt in the timeline, 11 pt in the panel.
fn meta(text: String, color: gpui::Hsla, compact: bool) -> gpui::Div {
    let el = div().min_w(px(0.)).one_line().tnum().text_color(color);
    let el = if compact { el.t_caption() } else { el.t_callout() };
    el.child(text)
}

/// A progress bar and its words, on the meta line.
fn progress_line(
    id: ElementId,
    percent: f32,
    text: String,
    compact: bool,
    cx: &App,
) -> AnyElement {
    let colors = theme::colors(cx);
    div()
        .flex()
        .items_center()
        .gap(space::S8)
        .child(
            Progress::new(id)
                .with_size(gpui_component::Size::Size(px(3.)))
                .color(colors.accent)
                .value(percent)
                .w(px(if compact { 96. } else { 140. })),
        )
        .child(meta(text, colors.text_2, compact).flex_none())
        .into_any_element()
}

fn upload_row(
    file: &FileAttachment,
    state: TransferState,
    id: impl Fn(&str) -> ElementId,
    cx: &App,
) -> AnyElement {
    let colors = theme::colors(cx);
    let detail = match state {
        TransferState::Failed(e) => meta(
            fill(f::UPLOAD_FAILED, &[("reason", error_text(e))]),
            colors.danger_text,
            false,
        )
        .into_any_element(),
        TransferState::Queued => meta(f::QUEUED.to_string(), colors.text_2, false).into_any_element(),
        s => progress_line(
            id("up"),
            s.percent(),
            fill(f::UPLOADING, &[("p", &format!("{:.0}", s.percent()))]),
            false,
            cx,
        ),
    };
    row_frame(file, detail, Vec::new(), &id, cx).into_any_element()
}

/// A row for a file in the timeline or the side panel.
pub fn file_row(
    message: &Message,
    file: &FileAttachment,
    provider: &Arc<dyn ChatProvider>,
    id: impl Fn(&str) -> ElementId,
    cx: &App,
) -> AnyElement {
    file_row_in(&message.group_id, &message.topic_id, file, provider, false, id, cx)
}

/// A file row; `compact` (the side panel) shows icon buttons with tooltips.
pub fn file_row_in(
    group_id: &str,
    topic_id: &str,
    file: &FileAttachment,
    provider: &Arc<dyn ChatProvider>,
    compact: bool,
    id: impl Fn(&str) -> ElementId,
    cx: &App,
) -> AnyElement {
    let colors = theme::colors(cx);
    let state = provider.file_transfer(&download_key(file));
    let kind = match TileKind::of(file) {
        TileKind::Image => crate::copy::cards::IMAGE,
        _ => crate::copy::artifacts::KIND_FILE,
    };
    let detail = match state {
        Some(s @ TransferState::Running { .. }) => progress_line(
            id("down"),
            s.percent(),
            fill(f::DOWNLOADING, &[("p", &format!("{:.0}", s.percent()))]),
            compact,
            cx,
        ),
        Some(TransferState::Failed(e)) => meta(
            fill(f::DOWNLOAD_FAILED, &[("reason", error_text(e))]),
            colors.danger_text,
            compact,
        )
        .into_any_element(),
        _ => meta(
            format!("{kind} · {}", size_text(file.size)),
            colors.text_2,
            compact,
        )
        .into_any_element(),
    };
    let running = matches!(state, Some(TransferState::Running { .. }));
    let failed = matches!(state, Some(TransferState::Failed(_)));
    let k = colors;
    let action = |part: &str, label: &'static str, what: OpenAction| {
        let provider = provider.clone();
        let (g, t, file) = (group_id.to_string(), topic_id.to_string(), file.clone());
        let icon = match (part, what) {
            ("retry", _) => Lucide::RotateCcw,
            (_, OpenAction::RevealInFinder) => Lucide::FolderOpen,
            _ => Lucide::ExternalLink,
        };
        let button = Button::new(id(part));
        let button = if compact {
            button.xsmall().ghost().icon(icon).tooltip(label)
        } else if part == "retry" {
            button.small().ghost().icon(icon).label(label)
        } else {
            // §13.5: a small secondary button with the control ring.
            button
                .small()
                .label(label)
                .shadow(k.shadow(Elevation::Control))
        };
        button
            .disabled(running)
            .on_click(move |_, _, cx| open_file(&provider, &g, &t, &file, what, cx))
            .into_any_element()
    };
    let actions = if failed {
        vec![action("retry", f::RETRY, OpenAction::Open)]
    } else if compact {
        vec![
            action("open", f::OPEN, OpenAction::Open),
            action("reveal", f::REVEAL, OpenAction::RevealInFinder),
        ]
    } else if TileKind::of(file) == TileKind::Page || !super::opens_unsafely(&file.name) {
        // One button, as in the mockup: 打开 for what opens safely.
        vec![action("open", f::OPEN, OpenAction::Open)]
    } else {
        vec![action("reveal", f::REVEAL, OpenAction::RevealInFinder)]
    };
    if compact {
        panel_frame(file, detail, actions, &id, cx).into_any_element()
    } else {
        row_frame(file, detail, actions, &id, cx).into_any_element()
    }
}
