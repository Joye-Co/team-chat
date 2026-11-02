//! The attachments of a message row: image previews and file rows.
//!
//! Every box has a fixed size from the message data (image pixel size, file
//! row height), so a preview that finishes loading or a progress bar that
//! ends never changes the row height.

use std::sync::Arc;

use gpui::{
    AnyElement, App, ElementId, InteractiveElement as _, IntoElement, ObjectFit, ParentElement,
    SharedString, StatefulInteractiveElement as _, Styled, StyledImage as _, Window, div, img,
    prelude::FluentBuilder as _, px, relative,
};
use gpui_component::{Disableable as _, tooltip::Tooltip};

use super::{
    AUTO_DOWNLOAD_MAX_BYTES, FILE_ROW_H, FILE_ROW_W, OpenAction, error_text, open_file,
    preview_size, size_text,
};
use crate::{
    copy::{files as f, fill},
    data::{ChatProvider, FileAttachment, Message, TransferState, download_key, upload_key},
    provider::files::THUMB_SOURCE_MAX_BYTES,
    theme::{self, Type as _, space},
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
        let mut images = div().flex().flex_wrap().gap(space::S8);
        let mut files = div().flex().flex_col().gap(space::S8);
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
                .mt(space::S8)
                .flex()
                .flex_col()
                .gap(space::S8)
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
    let k = theme::colors(cx);
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
    // While it loads: a `field` skeleton of the final size with the image
    // glyph and the name (§7.11, U30).
    let skeleton = |line: AnyElement| {
        div()
            .flex()
            .flex_col()
            .items_center()
            .gap(space::S6)
            .px(space::S12)
            .max_w_full()
            .child(ui::icon(Lucide::FileImage, theme::icon::EMPTY, k.text_3))
            .child(line)
            .into_any_element()
    };
    let name_line = div()
        .max_w_full()
        .t_meta()
        .text_color(k.text_3)
        .one_line()
        .child(file.name.clone())
        .into_any_element();
    let inner = match (&thumb, state) {
        (Some(path), _) => img(path.clone())
            .size_full()
            .object_fit(ObjectFit::Cover)
            .into_any_element(),
        (None, Some(TransferState::Failed(e))) => skeleton(
            div()
                .t_meta()
                .text_color(k.danger_text)
                .child(fill(f::DOWNLOAD_FAILED, &[("reason", error_text(e))]))
                .into_any_element(),
        ),
        _ => skeleton(name_line),
    };
    let progress = match (&thumb, state) {
        (None, Some(s @ TransferState::Running { .. })) => Some(s.percent()),
        _ => None,
    };
    let provider = provider.clone();
    let file2 = file.clone();
    div()
        .id(id("image"))
        .relative()
        .w(w)
        .h(h)
        .flex_none()
        .rounded(theme::radius::MD)
        .overflow_hidden()
        .border_1()
        .border_color(k.border)
        .bg(k.field)
        .flex()
        .items_center()
        .justify_center()
        .cursor_pointer()
        .child(inner)
        .when_some(progress, |el, p| el.child(track(p, cx)))
        .tooltip(move |window, cx| Tooltip::new(f::OPEN_IMAGE).build(window, cx))
        .on_click(move |_, _, cx| open_file(&provider, &g, &t, &file2, OpenAction::Open, cx))
        .into_any_element()
}

/// The type glyph of a file tile.
fn file_icon(file: &FileAttachment) -> Lucide {
    if file.is_image() {
        Lucide::FileImage
    } else if super::is_html(&file.name) {
        Lucide::FileCode
    } else {
        Lucide::FileText
    }
}

/// The type word of a file in its meta line (`网页 · 18 KB`).
fn kind_word(file: &FileAttachment) -> &'static str {
    if file.is_image() {
        f::KIND_IMAGE
    } else if super::is_html(&file.name) {
        f::KIND_PAGE
    } else {
        f::KIND_FILE
    }
}

/// The inset progress track of a file row (§7.11): left 12, right 12,
/// bottom 6, 3 pt tall, radius 1.5, `field` track and `accent` fill, so the
/// bar never crosses the row's rounded corners or border.
fn track(percent: f32, cx: &App) -> gpui::Div {
    let k = theme::colors(cx);
    div()
        .absolute()
        .left(space::S12)
        .right(space::S12)
        .bottom(space::S6)
        .h(px(3.))
        .rounded(px(1.5))
        .bg(k.field)
        .overflow_hidden()
        .child(
            div()
                .h_full()
                .w(relative((percent / 100.).clamp(0., 1.)))
                .rounded(px(1.5))
                .bg(k.accent),
        )
}

/// The frame of a file row (§7.11). In the timeline: 12 pt padding (8 at the
/// right), radius 12, 1 pt `border`, a 40 pt `accent-soft` tile with the type
/// glyph, the name 14/500 cut in the middle, the meta line 12/16 `text-2`, and
/// 32 pt icon buttons that show on hover (always while `pin_actions`). In the
/// side panel (`compact`): a 48 pt row with a 32 pt tile, no border, a hover
/// fill.
#[allow(clippy::too_many_arguments)]
fn row_frame(
    group: SharedString,
    file: &FileAttachment,
    compact: bool,
    meta: AnyElement,
    progress: Option<f32>,
    actions: Vec<AnyElement>,
    pin_actions: bool,
    cx: &App,
) -> gpui::Div {
    let k = theme::colors(cx);
    let tile = if compact { px(32.) } else { px(40.) };
    let glyph = if compact {
        theme::icon::ROW
    } else {
        theme::icon::EMPTY
    };
    let row = div()
        .group(group.clone())
        .relative()
        .flex_none()
        .flex()
        .items_center()
        .gap(space::S12)
        .overflow_hidden();
    let row = if compact {
        row.h(super::PANEL_ROW_H)
            .mx(-space::S8)
            .px(space::S8)
            .rounded(theme::radius::MD)
            .hover(|s| s.bg(k.hover))
    } else {
        row.w_full()
            .max_w(FILE_ROW_W)
            .h(FILE_ROW_H)
            .pl(space::S12)
            .pr(space::S8)
            .rounded(theme::radius::LG)
            .border_1()
            .border_color(k.border)
            .bg(k.content)
    };
    row.child(
        div()
            .flex_none()
            .size(tile)
            .rounded(theme::radius::MD)
            .bg(k.accent_soft)
            .flex()
            .items_center()
            .justify_center()
            .child(ui::icon(file_icon(file), glyph, k.accent_text)),
    )
    .child(
        div()
            .flex_1()
            .min_w(px(0.))
            .flex()
            .flex_col()
            .when(!compact, |el| el.gap(space::S2))
            .child(
                // Middle cut on the measured width: the last 12 characters
                // and the extension stay (§15).
                div()
                    .t_ui()
                    .medium()
                    .text_color(k.text_1)
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .text_ellipsis_middle()
                    .child(file.name.clone()),
            )
            .child(div().t_meta().text_color(k.text_2).one_line().child(meta)),
    )
    .child(
        div()
            .flex_none()
            .flex()
            .gap(space::S2)
            .when(!pin_actions, |el| {
                el.invisible().group_hover(group, |s| s.visible())
            })
            .children(actions),
    )
    .when_some(progress, |el, p| {
        // In the side panel the track runs under the text column, 2 pt
        // from the row's bottom, so it never covers the meta line.
        el.child(if compact {
            track(p, cx).left(px(52.)).right(space::S8).bottom(px(1.))
        } else {
            track(p, cx)
        })
    })
}

fn upload_row(
    file: &FileAttachment,
    state: TransferState,
    id: impl Fn(&str) -> ElementId,
    cx: &App,
) -> AnyElement {
    let k = theme::colors(cx);
    let (meta, progress) = match state {
        TransferState::Failed(e) => (
            div()
                .text_color(k.danger_text)
                .child(fill(f::UPLOAD_FAILED, &[("reason", error_text(e))]))
                .into_any_element(),
            None,
        ),
        TransferState::Queued => (div().child(f::QUEUED).into_any_element(), Some(0.)),
        s => (
            div()
                .tnum()
                .child(fill(f::UPLOADING, &[("p", &format!("{:.0}", s.percent()))]))
                .into_any_element(),
            Some(s.percent()),
        ),
    };
    let group = group_name(&id);
    row_frame(group, file, false, meta, progress, Vec::new(), false, cx).into_any_element()
}

fn group_name(id: &impl Fn(&str) -> ElementId) -> SharedString {
    SharedString::from(format!("{}", id("row")))
}

/// A row for a file in the timeline or the side panel.
pub fn file_row(
    message: &Message,
    file: &FileAttachment,
    provider: &Arc<dyn ChatProvider>,
    id: impl Fn(&str) -> ElementId,
    cx: &App,
) -> AnyElement {
    file_row_in(
        &message.group_id,
        &message.topic_id,
        file,
        provider,
        false,
        id,
        cx,
    )
}

/// A file row; `compact` is the 48 pt side-panel row (§7.11).
pub fn file_row_in(
    group_id: &str,
    topic_id: &str,
    file: &FileAttachment,
    provider: &Arc<dyn ChatProvider>,
    compact: bool,
    id: impl Fn(&str) -> ElementId,
    cx: &App,
) -> AnyElement {
    let k = theme::colors(cx);
    let state = provider.file_transfer(&download_key(file));
    let plain = fill(
        f::META,
        &[("kind", kind_word(file)), ("size", &size_text(file.size))],
    );
    let (meta, progress) = match state {
        Some(s @ TransferState::Running { .. }) => (
            // `文件 · 112.4 MB · 正在下载 64%`
            div()
                .tnum()
                .child(format!(
                    "{plain} · {}",
                    fill(f::DOWNLOADING, &[("p", &format!("{:.0}", s.percent()))])
                ))
                .into_any_element(),
            Some(s.percent()),
        ),
        Some(TransferState::Failed(e)) => (
            div()
                .text_color(k.danger_text)
                .child(fill(f::DOWNLOAD_FAILED, &[("reason", error_text(e))]))
                .into_any_element(),
            None,
        ),
        _ => (div().child(plain).into_any_element(), None),
    };
    let running = progress.is_some();
    let failed = matches!(state, Some(TransferState::Failed(_)));
    let action = |part: &str, label: &'static str, what: OpenAction, glyph: Lucide| {
        let provider = provider.clone();
        let (g, t, file) = (group_id.to_string(), topic_id.to_string(), file.clone());
        ui::icon_button(id(part), glyph)
            .tooltip(label)
            .disabled(running)
            .on_click(move |_, _, cx| {
                cx.stop_propagation();
                open_file(&provider, &g, &t, &file, what, cx)
            })
            .into_any_element()
    };
    let actions = if failed {
        vec![action(
            "retry",
            f::RETRY,
            OpenAction::Open,
            Lucide::RefreshCw,
        )]
    } else if running {
        // Downloading: cancel `x` (§7.11).
        let provider = provider.clone();
        let file = file.clone();
        vec![
            ui::icon_button(id("cancel"), Lucide::X)
                .tooltip(f::CANCEL_DOWNLOAD)
                .on_click(move |_, _, cx| {
                    cx.stop_propagation();
                    provider.cancel_download(&file);
                })
                .into_any_element(),
        ]
    } else {
        vec![
            action("open", f::OPEN, OpenAction::Open, Lucide::ExternalLink),
            action(
                "reveal",
                f::REVEAL,
                OpenAction::RevealInFinder,
                Lucide::FolderOpen,
            ),
        ]
    };
    let group = group_name(&id);
    let provider2 = provider.clone();
    let (g, t, file2) = (group_id.to_string(), topic_id.to_string(), file.clone());
    // Actions stay on while the file downloads or after it failed.
    row_frame(
        group,
        file,
        compact,
        meta,
        progress,
        actions,
        failed || running,
        cx,
    )
    .id(id("frame"))
    .when(!running, |el| {
        el.cursor_pointer()
            .on_click(move |_, _, cx| open_file(&provider2, &g, &t, &file2, OpenAction::Open, cx))
    })
    .into_any_element()
}
