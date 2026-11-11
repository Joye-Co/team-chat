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
    Disableable as _,
    menu::{ContextMenuExt as _, PopupMenuItem},
};

use super::{
    FILE_ROW_H, FILE_ROW_W, OpenAction, error_text,
    export::{self, FileItem as _, FileRef},
    open_file, size_text,
};
use crate::{
    copy::{files as f, fill},
    data::{ChatProvider, FileAttachment, Message, TransferState, download_key, upload_key},
    theme::{self, Type as _, space},
    ui::{
        self, Sym,
        native::{NativeType as _, native_colors},
    },
    views::timeline::extras::{ExtrasContext, MessageExtra},
};

/// Images and files attached to a message.
pub struct AttachmentsExtra;

impl MessageExtra for AttachmentsExtra {
    fn draws_files(&self) -> bool {
        true
    }

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
        let images: Vec<(usize, FileAttachment)> = message
            .attachments
            .iter()
            .enumerate()
            .filter(|(_, f)| super::gallery::shows_inline(f))
            .map(|(i, f)| (i, f.clone()))
            .collect();
        let mut files = div().flex().flex_col().gap(space::S8);
        let mut n_files = 0;
        for (index, file) in message.attachments.iter().enumerate() {
            let id = |part: &str| -> ElementId {
                ElementId::Name(SharedString::from(format!("att-{row_key}-{index}-{part}")))
            };
            if !file.is_uploaded() {
                let key = upload_key(&message.client_msg_id, index);
                let state = ctx
                    .provider
                    .file_transfer(&key)
                    .unwrap_or(TransferState::Queued);
                // inc-host: media files show the MEDIA §08 states with the real progress:
                // 正在上传 n%, then 正在生成预览 while the sender makes the preview.
                match media_state(file, &state, ctx.provider.upload_generating(&key)) {
                    Some((lead, status)) => {
                        files = files.child(crate::views::media::states::row(
                            id("state"),
                            lead,
                            file.name.clone(),
                            &status,
                            None,
                            cx,
                        ));
                    }
                    None => {
                        // A1-14: an upload can be cancelled like a download:
                        // the message's send is dropped.
                        let (provider, g, c) = (
                            ctx.provider.clone(),
                            message.group_id.clone(),
                            message.client_msg_id.clone(),
                        );
                        let cancel = move || provider.discard_send(&g, &c);
                        files = files.child(upload_row(file, state, id, cancel, cx))
                    }
                }
                n_files += 1;
            } else if super::gallery::shows_inline(file) {
                // Drawn by the gallery below.
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
                .children(super::gallery::render(
                    message,
                    &images,
                    ctx.provider,
                    row_key,
                    cx,
                ))
                .when(n_files > 0, |el| el.child(files))
                .into_any_element(),
        )
    }
}

/// The type glyph of a file tile.
fn file_icon(file: &FileAttachment) -> Sym {
    if file.is_image() {
        Sym::Image
    } else if super::is_html(&file.name) {
        Sym::CodeFile
    } else {
        Sym::Document
    }
}

/// The type word of a file in its meta line (`网页 · 18 KB`).
pub fn kind_word(file: &FileAttachment) -> &'static str {
    if file.is_image() {
        f::KIND_IMAGE
    } else if super::is_html(&file.name) {
        f::KIND_PAGE
    } else {
        f::KIND_FILE
    }
}

/// The progress of a file row (NATIVE.md §5.7): a 4 pt determinate bar,
/// radius 2, `quaternary` track and `label` fill, under the text column
/// (as Messages draws a transfer), 6 pt from the row's bottom.
fn track(percent: f32, cx: &App) -> gpui::Div {
    // D12: the one Progress (h4, primary on primary 20 %).
    div()
        .absolute()
        .left(space::S12)
        .right(space::S12)
        .bottom(space::S6)
        .child(crate::ui::native::progress(percent / 100., cx))
}

/// The frame of a file row (NATIVE.md §9.2 attachments, like a Card v2
/// surface): in the timeline a 52 pt row on `fill`, radius 12, no border
/// and no shadow, the type symbol in `secondary` (no colored tile), the name
/// in Body cut in the middle, the meta line in Subheadline `secondary`, and
/// borderless buttons that show on hover (always while `pin_actions`). In
/// the side panel (`compact`): a 48 pt row without a fill.
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
    let n = native_colors(cx);
    let row = div()
        .group(group.clone())
        .relative()
        .flex_none()
        .flex()
        .items_center()
        .gap(space::S10)
        .overflow_hidden();
    // R2-10, §2.4: one file item everywhere: a bordered container (radius
    // 10, 1 px border), the 32 media box, the name 14 medium cut in the
    // middle, 12 muted meta, and the Progress inside the item.
    let row = if compact {
        row.h(super::PANEL_ROW_H)
            .mx(-space::S8)
            .px(space::S8)
            .rounded(theme::radius::MD)
    } else {
        row.w_full()
            .max_w(FILE_ROW_W)
            .h(FILE_ROW_H)
            .pl(space::S12)
            .pr(space::S8)
            .gap(space::S12)
            .rounded(crate::ui::native::tokens::radius::CONTAINER)
            .border_1()
            .border_color(n.separator)
            .bg(n.content)
    };
    row.child(file_media(file_icon(file), cx))
        .child(
            div()
                .flex_1()
                .min_w(px(0.))
                .flex()
                .flex_col()
                .child(
                    // R1-06: the kit's file name: a middle cut that keeps the
                    // extension, the whole name in a tooltip when cut.
                    crate::ui::truncate::file_name(file.name.clone())
                        .id(SharedString::from(format!("{group}-name")))
                        .n_headline()
                        .text_color(n.label),
                )
                .child(
                    div()
                        .n_subheadline()
                        .n_figures()
                        .text_color(n.secondary)
                        .one_line()
                        .child(meta),
                ),
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
                track(p, cx).left(px(46.)).right(space::S8).bottom(px(1.))
            } else {
                // Under the name and meta: the media box, then the gap.
                track(p, cx)
                    .left(space::S12 + px(32.) + space::S12)
                    .right(px(44.))
                    .bottom(px(7.))
            })
        })
}

/// The 32 media box of a file item (§2.6): radius 6, a 1 px border, the
/// muted fill and a 16 symbol.
pub fn file_media(sym: Sym, cx: &App) -> gpui::Div {
    let n = native_colors(cx);
    div()
        .flex_none()
        .size(px(32.))
        .rounded(crate::ui::native::tokens::radius::ITEM)
        .border_1()
        .border_color(n.separator)
        .bg(n.fill)
        .flex()
        .items_center()
        .justify_center()
        .child(ui::sym(sym, ui::SymSize::INLINE, n.secondary))
}

/// [`file_media`] for a video or a PDF with a poster: the poster cut to
/// the same 32 box (radius 6, 1 px border), else the box with `sym`.
pub fn file_media_poster(sym: Sym, poster: Option<std::path::PathBuf>, cx: &App) -> gpui::Div {
    let Some(path) = poster else {
        return file_media(sym, cx);
    };
    let n = native_colors(cx);
    div()
        .flex_none()
        .size(px(32.))
        .rounded(crate::ui::native::tokens::radius::ITEM)
        .border_1()
        .border_color(n.separator)
        .bg(n.fill)
        .overflow_hidden()
        .child(img(path).size_full().object_fit(ObjectFit::Cover))
}

/// A borderless symbol button inside a file row (§5.2, §6): 24 × 24,
/// radius 6, a 13 pt Regular symbol (inline size, not the 16 pt toolbar
/// size), `fillHover` on hover.
fn row_button(id: ElementId, glyph: Sym, name: &'static str) -> gpui_component::button::Button {
    use crate::ui::NamedButton as _;
    crate::ui::native::icon_button_bare(id, glyph, crate::ui::native::IconSize::Small).named(name)
}

fn upload_row(
    file: &FileAttachment,
    state: TransferState,
    id: impl Fn(&str) -> ElementId,
    cancel: impl Fn() + 'static,
    cx: &App,
) -> AnyElement {
    // A1-14: one meta for every file row: `<kind> · <size> · <state>`.
    let plain = fill(
        f::META,
        &[("kind", kind_word(file)), ("size", &size_text(file.size))],
    );
    let (meta, progress) = match state {
        TransferState::Failed(e) => (
            div()
                .text_color(native_colors(cx).red)
                .child(fill(f::UPLOAD_FAILED, &[("reason", error_text(e))]))
                .into_any_element(),
            None,
        ),
        TransferState::Queued => (
            div()
                .child(format!("{plain} · {}", f::QUEUED))
                .into_any_element(),
            Some(0.),
        ),
        s => (
            div()
                .tnum()
                .child(format!(
                    "{plain} · {}",
                    fill(f::UPLOADING, &[("p", &format!("{:.0}", s.percent()))])
                ))
                .into_any_element(),
            Some(s.percent()),
        ),
    };
    let actions = if progress.is_some() {
        vec![
            row_button(id("cancel"), Sym::Close, f::CANCEL_UPLOAD)
                .on_click(move |_, _, cx| {
                    cx.stop_propagation();
                    cancel();
                })
                .into_any_element(),
        ]
    } else {
        Vec::new()
    };
    let pinned = !actions.is_empty();
    let group = group_name(&id);
    row_frame(group, file, false, meta, progress, actions, pinned, cx).into_any_element()
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
                .text_color(native_colors(cx).red)
                .child(fill(f::DOWNLOAD_FAILED, &[("reason", error_text(e))]))
                .into_any_element(),
            None,
        ),
        _ => (div().child(plain).into_any_element(), None),
    };
    let running = progress.is_some();
    let failed = matches!(state, Some(TransferState::Failed(_)));
    let action = |part: &str, label: &'static str, what: OpenAction, glyph: Sym| {
        let provider = provider.clone();
        let (g, t, file) = (group_id.to_string(), topic_id.to_string(), file.clone());
        row_button(id(part), glyph, label)
            .disabled(running)
            .on_click(move |_, _, cx| {
                cx.stop_propagation();
                open_file(&provider, &g, &t, &file, what, cx)
            })
            .into_any_element()
    };
    let actions = if failed {
        vec![action("retry", f::RETRY, OpenAction::Open, Sym::Refresh)]
    } else if running {
        // Downloading: cancel `x` (§7.11).
        let provider = provider.clone();
        let file = file.clone();
        vec![
            row_button(id("cancel"), Sym::Close, f::CANCEL_DOWNLOAD)
                .on_click(move |_, _, cx| {
                    cx.stop_propagation();
                    provider.cancel_download(&file);
                })
                .into_any_element(),
        ]
    } else {
        vec![
            action("open", f::OPEN, OpenAction::Open, Sym::OpenExternal),
            action("reveal", f::REVEAL, OpenAction::RevealInFinder, Sym::Folder),
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
        el.on_click(move |_, _, cx| open_file(&provider2, &g, &t, &file2, OpenAction::Open, cx))
    })
    // Space previews it, and it drags out to Finder as the file.
    .file_item(
        FileRef::attachment(provider, group_id, topic_id, file),
        file_icon(file),
    )
    // v12/behavior: the row's buttons as a context menu (§7.3).
    .context_menu({
        let provider = provider.clone();
        let (g, t, file) = (group_id.to_string(), topic_id.to_string(), file.clone());
        let target = FileRef::attachment(&provider, &g, &t, &file);
        move |menu, window, _| {
            let at = window.mouse_position();
            let item = |label: &'static str, what: OpenAction| {
                let (provider, g, t, file) = (provider.clone(), g.clone(), t.clone(), file.clone());
                PopupMenuItem::new(label)
                    .on_click(move |_, _, cx| open_file(&provider, &g, &t, &file, what, cx))
            };
            if running {
                let (provider, file) = (provider.clone(), file.clone());
                menu.item(
                    PopupMenuItem::new(f::CANCEL_DOWNLOAD)
                        .on_click(move |_, _, _| provider.cancel_download(&file)),
                )
            } else if failed {
                menu.item(item(f::RETRY, OpenAction::Open))
            } else {
                let menu = menu
                    .item(item(f::OPEN, OpenAction::Open))
                    .item(item(f::REVEAL, OpenAction::RevealInFinder));
                export::menu_items(menu, &target, at)
            }
        }
    })
    .into_any_element()
}

// --- inc-host track ---
/// The MEDIA §08 state of an uploading video, audio or PDF file; `None` for other files
/// and for a failed upload (the file row says why).
fn media_state(
    file: &FileAttachment,
    state: &TransferState,
    generating: bool,
) -> Option<(
    crate::views::media::states::Lead,
    crate::views::media::states::Status,
)> {
    use crate::views::media::{
        MediaKind, file_kind,
        states::{Lead, Status},
    };
    let kind = file_kind(&file.name, &file.mime);
    if !matches!(kind, MediaKind::Video | MediaKind::Audio | MediaKind::Pdf)
        || matches!(state, TransferState::Failed(_))
    {
        return None;
    }
    let lead = if kind == MediaKind::Video {
        Lead::Blank
    } else {
        Lead::Icon(kind.icon())
    };
    let status = match state {
        _ if generating => Status::Generating { duration: None },
        TransferState::Running { done, total } => Status::Uploading {
            done: *done,
            total: *total,
        },
        _ => Status::Uploading {
            done: 0,
            total: file.size,
        },
    };
    Some((lead, status))
}

#[cfg(test)]
mod media_state_tests {
    use super::*;
    use crate::views::media::states::Status;

    fn f(name: &str, mime: &str) -> FileAttachment {
        FileAttachment {
            name: name.into(),
            mime: mime.into(),
            size: 100,
            ..Default::default()
        }
    }

    #[test]
    fn uploads_of_media_follow_the_real_progress() {
        let v = f("录屏.mov", "video/quicktime");
        let running = TransferState::Running {
            done: 62,
            total: 100,
        };
        assert!(matches!(
            media_state(&v, &running, false),
            Some((
                _,
                Status::Uploading {
                    done: 62,
                    total: 100
                }
            ))
        ));
        assert!(matches!(
            media_state(&v, &running, true),
            Some((_, Status::Generating { .. }))
        ));
        assert!(
            media_state(
                &v,
                &TransferState::Failed(crate::data::FileError::Other),
                false
            )
            .is_none()
        );
        assert!(media_state(&f("a.txt", "text/plain"), &running, false).is_none());
    }
}
// --- end inc-host track ---
