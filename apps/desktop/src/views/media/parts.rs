//! Pieces shared by the inline previews and the full views: the card and pane frames,
//! small chips, and the actions (copy, save, send to the 话题, open with the system).

use std::{path::Path, sync::Arc};

use gpui::{
    App, ClipboardEntry, ClipboardItem, Div, ElementId, Hsla, ImageFormat, IntoElement,
    ParentElement, SharedString, Styled, div, px,
};
use gpui_component::{
    Sizable as _,
    button::{Button, ButtonCustomVariant, ButtonVariants as _},
    notification::Notification,
};

use super::{Screen, state::Target};
use crate::{
    copy::media as cm,
    data::{DraftKey, NewFile},
    theme::{self, Type as _, radius, space},
    ui::{self, Sym, NamedButton as _},
};

/// The frame of an inline preview: 1 pt border, radius 10, `content` fill.
pub fn card(cx: &App) -> Div {
    let k = theme::colors(cx);
    div()
        .flex()
        .flex_col()
        .rounded(px(10.))
        .border_1()
        .border_color(k.border)
        .bg(k.content)
        .overflow_hidden()
}

/// The header of an inline card: glyph, name, meta, then trailing tools.
pub fn card_header(icon: Sym, title: impl Into<SharedString>, cx: &App) -> Div {
    let k = theme::colors(cx);
    div()
        .flex_none()
        .h(px(38.))
        .px(space::S12)
        .flex()
        .items_center()
        .gap(space::S8)
        .bg(k.raised)
        .border_b_1()
        .border_color(k.sep)
        .child(ui::icon(icon, theme::icon::ROW, k.text_2))
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
        .h(px(48.))
        .px(space::S16)
        .flex()
        .items_center()
        .gap(space::S8)
        .border_b_1()
        .border_color(k.sep)
        .child(ui::icon(icon, theme::icon::ROW, k.text_2))
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
        .text_size(px(12.))
        .line_height(px(16.))
        .figures()
        .text_color(color)
        .whitespace_nowrap()
        .child(text.into())
}

/// A small outlined chip (`表格 1`, `v3`).
pub fn chip(text: impl Into<SharedString>, cx: &App) -> Div {
    let k = theme::colors(cx);
    div()
        .flex_none()
        .h(px(22.))
        .px(space::S8)
        .rounded(radius::SM)
        .border_1()
        .border_color(k.border)
        .bg(k.content)
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
        .min_h(px(34.))
        .px(space::S12)
        .flex()
        .items_center()
        .gap(space::S8)
        .bg(k.raised)
        .border_t_1()
        .border_color(k.sep)
        .t_meta()
        .text_color(k.text_3)
}

/// A named 28 pt tool button.
pub fn tool(id: impl Into<ElementId>, icon: Sym, name: &'static str) -> Button {
    ui::icon_button_sm(id, icon).named(name)
}

/// A tool button on the dark screen (lightbox, video): `screen-ink` glyph.
pub fn screen_tool(
    id: impl Into<ElementId>,
    icon: Sym,
    name: &'static str,
    s: &Screen,
    cx: &App,
) -> Button {
    Button::new(id)
        .custom(
            ButtonCustomVariant::new(cx)
                .color(gpui::transparent_black())
                .foreground(s.ink)
                .hover(gpui::hsla(0., 0., 1., 0.10))
                .active(gpui::hsla(0., 0., 1., 0.16)),
        )
        .icon(ui::button_glyph(icon, theme::icon::ROW, px(28.)))
        .with_size(px(28.))
        .rounded(radius::SM)
        .named(name)
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

/// The round ink play / pause control of the audio players (Media.dc.html): `text-1`
/// fill, `content` glyph. Labelled for VoiceOver.
pub fn play_button(
    id: impl Into<ElementId>,
    playing: bool,
    size: f32,
    cx: &App,
    on_click: impl Fn(&gpui::ClickEvent, &mut gpui::Window, &mut App) + 'static,
) -> gpui::Stateful<Div> {
    use gpui::{InteractiveElement as _, StatefulInteractiveElement as _};
    let k = theme::colors(cx);
    let label = if playing {
        cm::AUDIO_PAUSE
    } else {
        cm::AUDIO_PLAY
    };
    div()
        .id(id.into())
        .flex_none()
        .size(px(size))
        .rounded_full()
        .bg(k.text_1)
        .hover(|s| s.opacity(0.88))
        .active(|s| s.opacity(0.76))
        .cursor_pointer()
        .flex()
        .items_center()
        .justify_center()
        .tooltip(move |window, cx| gpui_component::tooltip::Tooltip::new(label).build(window, cx))
        .child(ui::icon(
            if playing { Sym::Pause } else { Sym::Play },
            px(size * 0.42),
            k.content,
        ))
        .on_click(on_click)
}
