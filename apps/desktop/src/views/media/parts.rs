//! Pieces shared by the inline previews and the full views: the card and pane frames,
//! small chips, and the actions (copy, save, send to the 话题, open with the system).

use std::{path::Path, sync::Arc};

use gpui::{
    App, ClipboardEntry, ClipboardItem, Div, ElementId, Hsla, ImageFormat, IntoElement,
    ParentElement, SharedString, Styled, div, px,
};
use gpui_component::{button::Button, notification::Notification};

use super::{Screen, state::Target};
use crate::{
    copy::media as cm,
    data::{DraftKey, NewFile},
    theme::{self, Type as _, space},
    ui::{self, NamedButton as _, Sym},
};

/// The frame of an inline preview (COMPLETENESS.md §2.4): the card container, radius 14,
/// a 1 px border, shadow sm, 640 wide in a thread.
pub fn card(cx: &App) -> Div {
    crate::ui::native::cards::card_frame(cx).flex().flex_col()
}

/// The header of an inline card: glyph, name, meta, then trailing tools.
pub fn card_header(icon: Sym, title: impl Into<SharedString>, cx: &App) -> Div {
    let k = theme::colors(cx);
    div()
        .flex_none()
        .h(crate::ui::native::tokens::height::LG)
        .px(space::S16)
        .flex()
        .items_center()
        .gap(space::S8)
        .child(ui::icon(
            icon,
            px(16.),
            crate::ui::native::native_colors(cx).secondary,
        ))
        .child(
            div()
                .t_small()
                .medium()
                .text_color(k.text_1)
                .one_line()
                .flex_shrink(1.)
                .min_w(px(0.))
                .child(title.into()),
        )
}

/// The header of a full view: 48 pt, glyph, name 15/600, kind or meta, tools on the right.
pub fn pane_header(icon: Sym, title: impl Into<SharedString>, cx: &App) -> Div {
    let k = theme::colors(cx);
    div()
        .flex_none()
        .h(crate::ui::native::tokens::height::TOOLBAR)
        .px(space::S16)
        .flex()
        .items_center()
        .gap(space::S8)
        .child(ui::icon(
            icon,
            px(16.),
            crate::ui::native::native_colors(cx).secondary,
        ))
        .child(
            div()
                .t_card_title()
                .text_color(k.text_1)
                .one_line()
                .flex_shrink(1.)
                .min_w(px(0.))
                .child(title.into()),
        )
}

/// Meta figures: the system font with tabular figures (`text-3`, §3).
pub fn mono(text: impl Into<SharedString>, color: Hsla) -> Div {
    div()
        .text_size(crate::ui::native::tokens::text::META.0)
        .line_height(px(16.))
        .figures()
        .text_color(color)
        .whitespace_nowrap()
        .child(text.into())
}

/// A quiet `secondary` label (`表格 1`, `v3`): text, not a chip (NATIVE.md
/// rule 4).
pub fn chip(text: impl Into<SharedString>, cx: &App) -> Div {
    let k = theme::colors(cx);
    div()
        .flex_none()
        .h(px(22.))
        .flex()
        .items_center()
        .t_meta()
        .tnum()
        .text_color(k.text_2)
        .whitespace_nowrap()
        .child(text.into())
}

/// The quiet bottom strip of a card or pane.
pub fn footer(cx: &App) -> Div {
    let k = theme::colors(cx);
    div()
        .flex_none()
        .min_h(px(32.))
        .px(space::S16)
        .flex()
        .items_center()
        .gap(space::S8)
        .t_meta()
        .text_color(k.text_3)
}

/// A named 28 pt tool button.
pub fn tool(id: impl Into<ElementId>, icon: Sym, name: &'static str) -> Button {
    ui::native::icon_button_bare(id, icon, ui::native::IconSize::Small).named(name)
}

/// A tool button on the dark screen (lightbox, video): `screen-ink` glyph.
pub fn screen_tool(
    id: impl Into<ElementId>,
    icon: Sym,
    name: &'static str,
    s: &Screen,
    cx: &App,
) -> Button {
    crate::ui::native::screen_button(id, icon, name, false, s.ink, cx)
}

// --- actions ------------------------------------------------------------------------

fn toast(text: &str, cx: &mut App) {
    crate::views::files::notify(Notification::new().message(text.to_string()), cx);
}

/// Copies an image file to the clipboard.
pub fn copy_image_file(path: &Path, cx: &mut App) {
    let Ok(bytes) = std::fs::read(path) else {
        return;
    };
    let format = match path.extension().and_then(|e| e.to_str()) {
        Some("png") => ImageFormat::Png,
        Some("gif") => ImageFormat::Gif,
        Some("webp") => ImageFormat::Webp,
        _ => ImageFormat::Jpeg,
    };
    copy_image(format, bytes, cx);
}

pub fn copy_image(format: ImageFormat, bytes: Vec<u8>, cx: &mut App) {
    cx.write_to_clipboard(ClipboardItem {
        entries: vec![ClipboardEntry::Image(gpui::Image::from_bytes(
            format, bytes,
        ))],
    });
    toast(cm::COPIED, cx);
}

/// Copies files into ~/Downloads (keeping their names, adding ` 2` on a clash).
pub fn save_to_downloads(paths: &[&Path], cx: &mut App) {
    let Some(home) = std::env::var_os("HOME") else {
        return;
    };
    let dir = Path::new(&home).join("Downloads");
    for p in paths {
        let Some(name) = p.file_name() else { continue };
        let mut out = dir.join(name);
        let mut n = 2;
        while out.exists() {
            let stem = p.file_stem().and_then(|s| s.to_str()).unwrap_or("file");
            let ext = p.extension().and_then(|s| s.to_str()).unwrap_or("");
            out = dir.join(if ext.is_empty() {
                format!("{stem} {n}")
            } else {
                format!("{stem} {n}.{ext}")
            });
            n += 1;
        }
        if std::fs::copy(p, &out).is_err() {
            return;
        }
    }
    toast(cm::SAVED, cx);
}

/// Puts a picture into the 话题's composer as a pending image: the person adds words and
/// sends it (nothing is sent without them).
pub fn send_image(target: &Target, bytes: Vec<u8>, cx: &mut App) {
    let Some(provider) = &target.provider else {
        return;
    };
    if target.group_id.is_empty() || target.topic_id.is_empty() {
        return;
    }
    provider.attach_files(
        &DraftKey::topic(target.group_id.clone(), target.topic_id.clone()),
        vec![NewFile::Image {
            mime: "image/png".into(),
            extension: "png".into(),
            bytes: Arc::from(bytes),
        }],
    );
    toast(cm::VIDEO_FRAME_SENT, cx);
}

/// Opens a local file with its default app (the 产出 window path for pages is separate).
pub fn open_with_system(path: &Path, cx: &mut App) {
    cx.open_with_system(path);
}

/// An empty box of a given size with a centered note: loading or not available.
pub fn placeholder(h: gpui::Pixels, note: impl IntoElement, cx: &App) -> Div {
    let k = theme::colors(cx);
    div()
        .w_full()
        .h(h)
        .flex()
        .items_center()
        .justify_center()
        .gap(space::S8)
        .bg(k.code_bg)
        .t_small()
        .text_color(k.text_3)
        .child(note)
}

/// The play / pause control of the audio players: a borderless symbol
/// button (NATIVE.md §5.1): the `label` symbol, a `fillHover` circle on
/// hover, `quaternary` pressed. Labelled for VoiceOver.
pub fn play_button(
    id: impl Into<ElementId>,
    playing: bool,
    size: f32,
    cx: &App,
    on_click: impl Fn(&gpui::ClickEvent, &mut gpui::Window, &mut App) + 'static,
) -> gpui::Stateful<Div> {
    use gpui::{InteractiveElement as _, StatefulInteractiveElement as _};
    let n = crate::ui::native::native_colors(cx);
    let label = if playing {
        cm::AUDIO_PAUSE
    } else {
        cm::AUDIO_PLAY
    };
    let size = size.min(32.);
    div()
        .id(id.into())
        .flex_none()
        .size(px(size))
        .rounded_full()
        .hover(|s| s.bg(n.fill_hover))
        .active(|s| s.bg(n.quaternary))
        .flex()
        .items_center()
        .justify_center()
        .tooltip(move |window, cx| gpui_component::tooltip::Tooltip::new(label).build(window, cx))
        .child(ui::icon(
            if playing { Sym::Pause } else { Sym::Play },
            px(20.),
            n.label,
        ))
        .on_click(on_click)
}
