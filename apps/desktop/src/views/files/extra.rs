//! The attachments of a message row: image previews and file rows.
//!
//! Every box has a fixed size from the message data (image pixel size, file
//! row height), so a preview that finishes loading or a progress bar that
//! ends never changes the row height.

use std::sync::Arc;

use gpui::{
    AnyElement, App, ElementId, InteractiveElement as _, IntoElement, ObjectFit, ParentElement,
    SharedString, StatefulInteractiveElement as _, Styled, StyledImage as _, Window, div, img,
    prelude::FluentBuilder as _, px,
};
use gpui_component::{
    Disableable as _, Icon, IconName, Sizable as _,
    button::{Button, ButtonVariants as _},
    progress::Progress,
    tooltip::Tooltip,
};

use super::{
    AUTO_DOWNLOAD_MAX_BYTES, FILE_ROW_H, FILE_ROW_W, OpenAction, error_text, open_file,
    preview_size, size_text,
};
use crate::{
    copy::{files as f, fill},
    data::{ChatProvider, FileAttachment, Message, TransferState, download_key, upload_key},
    provider::files::THUMB_SOURCE_MAX_BYTES,
    theme,
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
        let mut images = div().flex().flex_wrap().gap(px(6.));
        let mut files = div().flex().flex_col().gap(px(4.));
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
                .mt(px(4.))
                .flex()
                .flex_col()
                .gap(px(6.))
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
            .object_fit(ObjectFit::Cover)
            .into_any_element(),
        (None, Some(TransferState::Running { .. })) => Progress::new(id("progress"))
            .value(state.map(|s| s.percent()).unwrap_or(0.))
            .w(w * 0.6)
            .into_any_element(),
        (None, Some(TransferState::Failed(e))) => div()
            .px(px(8.))
            .text_xs()
            .text_color(colors.danger)
            .child(fill(f::DOWNLOAD_FAILED, &[("reason", error_text(e))]))
            .into_any_element(),
        _ => Icon::new(IconName::File)
            .text_color(colors.muted_foreground)
            .into_any_element(),
    };
    let provider = provider.clone();
    let file2 = file.clone();
    div()
        .id(id("image"))
        .w(w)
        .h(h)
        .flex_none()
        .rounded(px(6.))
        .overflow_hidden()
        .border_1()
        .border_color(colors.border)
        .bg(colors.code_background)
        .flex()
        .items_center()
        .justify_center()
        .cursor_pointer()
        .child(inner)
        .tooltip(move |window, cx| Tooltip::new(f::OPEN_IMAGE).build(window, cx))
        .on_click(move |_, _, cx| open_file(&provider, &g, &t, &file2, OpenAction::Open, cx))
        .into_any_element()
}

fn file_icon(file: &FileAttachment) -> IconName {
    if file.is_image() {
        IconName::File
    } else if super::is_html(&file.name) {
        IconName::Globe
    } else {
        IconName::FileText
    }
}

/// The frame of a file row: icon, name and the line under it, then actions.
fn row_frame(
    file: &FileAttachment,
    detail: AnyElement,
    actions: Vec<AnyElement>,
    cx: &App,
) -> gpui::Div {
    let colors = theme::colors(cx);
    div()
        .w(FILE_ROW_W)
        .max_w_full()
        .h(FILE_ROW_H)
        .flex_none()
        .px(px(10.))
        .rounded(px(6.))
        .border_1()
        .border_color(colors.border)
        .bg(colors.background)
        .flex()
        .items_center()
        .gap(px(10.))
        .child(
            Icon::new(file_icon(file))
                .text_color(colors.muted_foreground)
                .flex_none(),
        )
        .child(
            div()
                .flex_1()
                .min_w(px(0.))
                .flex()
                .flex_col()
                .gap(px(2.))
                .child(
                    div()
                        .text_sm()
                        .overflow_hidden()
                        .whitespace_nowrap()
                        .text_ellipsis()
                        .child(file.name.clone()),
                )
                .child(detail),
        )
        .child(div().flex_none().flex().gap(px(2.)).children(actions))
}

fn upload_row(
    file: &FileAttachment,
    state: TransferState,
    id: impl Fn(&str) -> ElementId,
    cx: &App,
) -> AnyElement {
    let colors = theme::colors(cx);
    let detail = match state {
        TransferState::Failed(e) => div()
            .text_xs()
            .text_color(colors.danger)
            .child(fill(f::UPLOAD_FAILED, &[("reason", error_text(e))]))
            .into_any_element(),
        TransferState::Queued => div()
            .text_xs()
            .text_color(colors.muted_foreground)
            .child(f::QUEUED)
            .into_any_element(),
        s => div()
            .flex()
            .items_center()
            .gap(px(8.))
            .child(Progress::new(id("up")).value(s.percent()).w(px(160.)))
            .child(
                div()
                    .text_xs()
                    .text_color(colors.muted_foreground)
                    .child(fill(f::UPLOADING, &[("p", &format!("{:.0}", s.percent()))])),
            )
            .into_any_element(),
    };
    row_frame(file, detail, Vec::new(), cx).into_any_element()
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
    let size = size_text(file.size);
    let detail = match state {
        Some(s @ TransferState::Running { .. }) => div()
            .flex()
            .items_center()
            .gap(px(8.))
            .child(Progress::new(id("down")).value(s.percent()).w(px(140.)))
            .child(
                div()
                    .text_xs()
                    .text_color(colors.muted_foreground)
                    .child(fill(f::DOWNLOADING, &[("p", &format!("{:.0}", s.percent()))])),
            )
            .into_any_element(),
        Some(TransferState::Failed(e)) => div()
            .text_xs()
            .text_color(colors.danger)
            .overflow_hidden()
            .whitespace_nowrap()
            .text_ellipsis()
            .child(fill(f::DOWNLOAD_FAILED, &[("reason", error_text(e))]))
            .into_any_element(),
        _ => div()
            .text_xs()
            .text_color(colors.muted_foreground)
            .child(size)
            .into_any_element(),
    };
    let running = matches!(state, Some(TransferState::Running { .. }));
    let failed = matches!(state, Some(TransferState::Failed(_)));
    let action = |part: &str, label: &'static str, what: OpenAction| {
        let provider = provider.clone();
        let (g, t, file) = (group_id.to_string(), topic_id.to_string(), file.clone());
        let icon = match (part, what) {
            ("retry", _) => IconName::RefreshCw,
            (_, OpenAction::RevealInFinder) => IconName::FolderOpen,
            _ => IconName::ExternalLink,
        };
        let button = Button::new(id(part)).xsmall().ghost();
        let button = if compact {
            button.icon(icon).tooltip(label)
        } else {
            button.label(label)
        };
        button
            .disabled(running)
            .on_click(move |_, _, cx| open_file(&provider, &g, &t, &file, what, cx))
            .into_any_element()
    };
    let actions = if failed {
        vec![action("retry", f::RETRY, OpenAction::Open)]
    } else {
        vec![
            action("open", f::OPEN, OpenAction::Open),
            action("reveal", f::REVEAL, OpenAction::RevealInFinder),
        ]
    };
    row_frame(file, detail, actions, cx).into_any_element()
}
