//! The images of a message in the timeline.
//!
//! One picture keeps its own ratio inside [`gallery::WIDTH`] x [`gallery::SINGLE_MAX_H`].
//! Two or more sit in a mosaic with hairline gaps whose outline is rounded as one shape
//! (Photos, Messages): two side by side, three as one large and two stacked, four as
//! 2 x 2, then rows of two and three; past [`gallery::MAX_TILES`] the last tile says
//! `+N`. Every box has its size from the message data, so a thumbnail that arrives never
//! moves the row. A click opens the picture in the viewer ([`super::lightbox`]).
//!
//! [`gallery::WIDTH`]: crate::ui::native::tokens::gallery::WIDTH
//! [`gallery::SINGLE_MAX_H`]: crate::ui::native::tokens::gallery::SINGLE_MAX_H
//! [`gallery::MAX_TILES`]: crate::ui::native::tokens::gallery::MAX_TILES

use std::sync::Arc;

use gpui::{
    AnyElement, App, ElementId, FontWeight, InteractiveElement as _, IntoElement, ObjectFit,
    ParentElement, SharedString, StatefulInteractiveElement as _, Styled, StyledImage as _, div,
    img, prelude::FluentBuilder as _, px,
};
use gpui_component::menu::{ContextMenuExt as _, PopupMenuItem};

use super::{
    AUTO_DOWNLOAD_MAX_BYTES, OpenAction, error_text,
    export::{self, FileItem as _, FileRef},
    open_file,
};
use crate::{
    copy::{files as f, fill},
    data::{ChatProvider, FileAttachment, Message, TransferState, download_key},
    theme::{self, Type as _, space},
    ui::{
        self, Sym,
        native::{
            Arrows, FocusRow as _, native_colors,
            tokens::{gallery as g, radius, text},
        },
    },
};

/// Which outer corners of the mosaic a tile owns (they are rounded).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Corners {
    pub tl: bool,
    pub tr: bool,
    pub br: bool,
    pub bl: bool,
}

/// One tile of a mosaic, in points from the mosaic's top-left corner.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Tile {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
    pub corners: Corners,
}

/// The tiles of `n` pictures (at most [`g::MAX_TILES`] of them) and the mosaic's size.
#[derive(Clone, Debug, PartialEq)]
pub struct Mosaic {
    pub w: f32,
    pub h: f32,
    pub tiles: Vec<Tile>,
}

/// The box of a single picture of `width` x `height` pixels: its own ratio, never larger
/// than the picture itself, inside the gallery bounds, each side at least
/// [`g::SINGLE_MIN`] (the picture covers the box, so a long strip is cropped, not thin).
pub fn single(width: Option<u32>, height: Option<u32>) -> (f32, f32) {
    let max_w = f32::from(g::WIDTH);
    let max_h = f32::from(g::SINGLE_MAX_H);
    let min = f32::from(g::SINGLE_MIN);
    let (w, h) = match (width, height) {
        (Some(w), Some(h)) if w > 0 && h > 0 => (w as f32, h as f32),
        _ => return (max_w, (max_w * 0.75).min(max_h)),
    };
    let s = (max_w / w).min(max_h / h).min(1.0);
    (
        (w * s).round().clamp(min, max_w),
        (h * s).round().clamp(min, max_h),
    )
}

/// The cells of each row for `n` tiles (2..=9): 3 is special (one large, two stacked),
/// otherwise rows of three with the rest in rows of two on top.
fn rows(n: usize) -> Vec<usize> {
    match n {
        0 | 1 => vec![n],
        2 => vec![2],
        4 => vec![2, 2],
        _ => match n % 3 {
            0 => vec![3; n / 3],
            2 => [vec![2], vec![3; n / 3]].concat(),
            _ => [vec![2, 2], vec![3; (n - 4) / 3]].concat(),
        },
    }
}

/// Splits `total` into `k` whole widths with `gap` between them; the last takes the rest.
fn split(total: f32, k: usize, gap: f32) -> Vec<f32> {
    let room = total - gap * (k as f32 - 1.);
    let each = (room / k as f32).floor();
    let mut out = vec![each; k];
    out[k - 1] = room - each * (k as f32 - 1.);
    out
}

/// The mosaic of `n` pictures (`n` >= 2).
pub fn mosaic(n: usize) -> Mosaic {
    let w = f32::from(g::WIDTH);
    let gap = f32::from(g::GAP);
    let n = n.clamp(2, g::MAX_TILES);
    let mut tiles = Vec::with_capacity(n);
    let h;
    if n == 3 {
        // One large on the left (two thirds), two squares stacked on the right.
        let small = ((w - gap * 2.) / 3.).floor();
        let right = w - small;
        let left = right - gap;
        h = small * 2. + gap;
        tiles.push(Tile {
            x: 0.,
            y: 0.,
            w: left,
            h,
            corners: Corners::default(),
        });
        tiles.push(Tile {
            x: right,
            y: 0.,
            w: small,
            h: small,
            corners: Corners::default(),
        });
        tiles.push(Tile {
            x: right,
            y: small + gap,
            w: small,
            h: small,
            corners: Corners::default(),
        });
    } else {
        let rows = rows(n);
        let only_row = rows.len() == 1;
        let mut y = 0.;
        for k in rows {
            let widths = split(w, k, gap);
            let row_h = match k {
                2 if only_row => widths[0],
                2 => (widths[0] * g::PAIR_RATIO).round(),
                _ => widths[0],
            };
            let mut x = 0.;
            for cw in widths {
                tiles.push(Tile {
                    x,
                    y,
                    w: cw,
                    h: row_h,
                    corners: Corners::default(),
                });
                x += cw + gap;
            }
            y += row_h + gap;
        }
        h = y - gap;
    }
    let near = |a: f32, b: f32| (a - b).abs() < 0.5;
    for t in &mut tiles {
        t.corners = Corners {
            tl: near(t.x, 0.) && near(t.y, 0.),
            tr: near(t.x + t.w, w) && near(t.y, 0.),
            br: near(t.x + t.w, w) && near(t.y + t.h, h),
            bl: near(t.x, 0.) && near(t.y + t.h, h),
        };
    }
    Mosaic { w, h, tiles }
}

/// Whether the timeline shows `file` as a picture (uploaded, an image, small enough to
/// make a thumbnail of).
pub fn shows_inline(file: &FileAttachment) -> bool {
    file.is_uploaded()
        && file.is_image()
        && file.size <= crate::provider::files::THUMB_SOURCE_MAX_BYTES
}

/// The pictures of `message` (`images` are its attachments [`shows_inline`] keeps, with
/// their index in the message).
pub fn render(
    message: &Message,
    images: &[(usize, FileAttachment)],
    provider: &Arc<dyn ChatProvider>,
    row_key: &str,
    cx: &mut App,
) -> Option<AnyElement> {
    let id = |index: usize| -> ElementId {
        ElementId::Name(SharedString::from(format!("att-{row_key}-{index}-image")))
    };
    match images {
        [] => None,
        [(index, file)] => {
            let (w, h) = single(file.width, file.height);
            let all = Corners {
                tl: true,
                tr: true,
                br: true,
                bl: true,
            };
            Some(tile(
                message,
                file,
                provider,
                id(*index),
                w,
                h,
                all,
                None,
                cx,
            ))
        }
        _ => {
            let m = mosaic(images.len());
            let shown = m.tiles.len();
            let more = images.len().saturating_sub(shown);
            let mut frame = div().relative().w(px(m.w)).h(px(m.h)).flex_none();
            for (i, (t, (index, file))) in m.tiles.iter().zip(images).enumerate() {
                let more = (i + 1 == shown && more > 0).then_some(more);
                frame = frame.child(div().absolute().left(px(t.x)).top(px(t.y)).child(tile(
                    message,
                    file,
                    provider,
                    id(*index),
                    t.w,
                    t.h,
                    t.corners,
                    more,
                    cx,
                )));
            }
            Some(frame.into_any_element())
        }
    }
}

/// Rounds the corners `c` owns with `r`.
fn round_corners<E: Styled + gpui::prelude::FluentBuilder>(
    el: E,
    c: Corners,
    r: gpui::Pixels,
) -> E {
    el.when(c.tl, |e| e.rounded_tl(r))
        .when(c.tr, |e| e.rounded_tr(r))
        .when(c.br, |e| e.rounded_br(r))
        .when(c.bl, |e| e.rounded_bl(r))
}

#[allow(clippy::too_many_arguments)]
fn tile(
    message: &Message,
    file: &FileAttachment,
    provider: &Arc<dyn ChatProvider>,
    id: ElementId,
    w: f32,
    h: f32,
    corners: Corners,
    more: Option<usize>,
    cx: &mut App,
) -> AnyElement {
    let k = theme::colors(cx);
    let n = native_colors(cx);
    let r = radius::CONTAINER;
    let (gid, tid) = (message.group_id.clone(), message.topic_id.clone());
    let thumb = provider.thumbnail(&gid, &tid, file);
    let state = provider.file_transfer(&download_key(file));
    if thumb.is_none()
        && state.is_none()
        && file.size <= AUTO_DOWNLOAD_MAX_BYTES
        && provider.cached_file(file).is_none()
    {
        // Fetch it once for the preview; the provider shares the transfer.
        drop(provider.fetch_file(&gid, &tid, file));
    }
    let small = w < 200. || h < 140.;
    // While it loads: the tile's own size in `fill`, the image glyph and (when there is
    // room) the name; a failed download says why.
    let skeleton = |line: Option<AnyElement>| {
        div()
            .size_full()
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .gap(space::S6)
            .px(space::S12)
            .child(ui::icon(Sym::Image, theme::icon::EMPTY, k.text_3))
            .children(line)
            .into_any_element()
    };
    let inner = match (&thumb, state) {
        // GPUI clips to the box, not to its corner radius: round the picture itself.
        (Some(path), _) => round_corners(
            img(path.clone()).size_full().object_fit(ObjectFit::Cover),
            corners,
            r,
        )
        .into_any_element(),
        (None, Some(TransferState::Failed(e))) => skeleton(Some(
            div()
                .t_meta()
                .text_color(n.red)
                .text_center()
                .child(fill(f::DOWNLOAD_FAILED, &[("reason", error_text(e))]))
                .into_any_element(),
        )),
        _ => skeleton((!small).then(|| {
            crate::ui::truncate::file_name(file.name.clone())
                .id(SharedString::from(format!("{id}-name")))
                .max_w_full()
                .t_meta()
                .text_color(k.text_3)
                .into_any_element()
        })),
    };
    let progress = match (&thumb, state) {
        (None, Some(s @ TransferState::Running { .. })) => Some(s.percent()),
        _ => None,
    };
    let gif = thumb.is_some() && file.mime == "image/gif" && more.is_none();
    let target = FileRef::attachment(provider, &gid, &tid, file);
    let menu_target = target.clone();
    let (p_menu, file_menu, g_menu, t_menu) =
        (provider.clone(), file.clone(), gid.clone(), tid.clone());
    let (message_id, file_open, p_open) = (message.id.clone(), file.clone(), provider.clone());
    let group = SharedString::from(format!("{id}-tile"));
    round_corners(
        div()
            .id(id.clone())
            .group(group.clone())
            .relative()
            .w(px(w))
            .h(px(h))
            .flex_none()
            // A covering picture is drawn past its box: clip it (the corners are rounded on
            // the picture itself).
            .overflow_hidden()
            .bg(n.fill)
            .cursor_pointer()
            .focus_row_with(false, Arrows::Grid, cx)
            .child(inner)
            // The tile under the pointer darkens a little (a picture has no hover fill).
            .child(round_corners(
                div().absolute().inset_0().group_hover(group.clone(), |s| {
                    s.bg(gpui::hsla(0., 0., 0., g::HOVER_DIM))
                }),
                corners,
                r,
            ))
            .when(gif, |el| {
                el.child(
                    div()
                        .absolute()
                        .left(space::S6)
                        .bottom(space::S6)
                        .h(g::BADGE_H)
                        .px(space::S4)
                        .rounded(radius::CHECKBOX)
                        .bg(gpui::hsla(0., 0., 0., g::BADGE_SCRIM))
                        .flex()
                        .items_center()
                        .text_size(crate::ui::native::tokens::type_scale::CAPTION.0)
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(gpui::white())
                        .child("GIF"),
                )
            })
            .when_some(more, |el, more| {
                el.child(round_corners(
                    div()
                        .absolute()
                        .inset_0()
                        .bg(gpui::hsla(0., 0., 0., g::MORE_SCRIM))
                        .flex()
                        .items_center()
                        .justify_center()
                        .text_size(text::HERO.0)
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(gpui::white())
                        .child(format!("+{more}")),
                    corners,
                    r,
                ))
            })
            .when_some(progress, |el, p| {
                el.child(
                    div()
                        .absolute()
                        .left(space::S12)
                        .right(space::S12)
                        .bottom(space::S8)
                        .child(crate::ui::native::progress(p / 100., cx)),
                )
            })
            .on_click(move |_, window, cx| {
                super::lightbox::open(
                    super::lightbox::Opening {
                        group_id: gid.clone(),
                        topic_id: tid.clone(),
                        message_id: message_id.clone(),
                        file: file_open.clone(),
                        provider: p_open.clone(),
                    },
                    window,
                    cx,
                )
            })
            .file_item(target, Sym::Image)
            .context_menu(move |menu, window, _| {
                let at = window.mouse_position();
                let item = |label: &'static str, what: OpenAction| {
                    let (provider, file) = (p_menu.clone(), file_menu.clone());
                    let (g, t) = (g_menu.clone(), t_menu.clone());
                    PopupMenuItem::new(label)
                        .on_click(move |_, _, cx| open_file(&provider, &g, &t, &file, what, cx))
                };
                let menu = menu
                    .item(item(f::OPEN_IMAGE, OpenAction::Open))
                    .item(item(f::REVEAL, OpenAction::RevealInFinder));
                export::menu_items(menu, &menu_target, at)
            }),
        corners,
        r,
    )
    .into_any_element()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_single_picture_keeps_its_ratio_inside_the_bounds() {
        assert_eq!(single(Some(4000), Some(3000)), (480., 360.));
        assert_eq!(single(Some(1000), Some(3000)), (120., 360.));
        // Never larger than the picture itself.
        assert_eq!(single(Some(300), Some(200)), (300., 200.));
        // A long strip keeps the minimum height (the picture covers the box).
        assert_eq!(single(Some(2000), Some(10)), (480., 96.));
        assert_eq!(single(None, None), (480., 360.));
    }

    #[test]
    fn rows_put_the_twos_on_top() {
        assert_eq!(rows(2), vec![2]);
        assert_eq!(rows(4), vec![2, 2]);
        assert_eq!(rows(5), vec![2, 3]);
        assert_eq!(rows(6), vec![3, 3]);
        assert_eq!(rows(7), vec![2, 2, 3]);
        assert_eq!(rows(8), vec![2, 3, 3]);
        assert_eq!(rows(9), vec![3, 3, 3]);
    }

    /// Every mosaic is the full width, its tiles never overlap and keep the hairline
    /// between them, and exactly the four outer corners are rounded.
    #[test]
    fn mosaics_tile_the_frame() {
        let w = f32::from(g::WIDTH);
        let gap = f32::from(g::GAP);
        for n in 2..=12 {
            let m = mosaic(n);
            assert_eq!(m.tiles.len(), n.min(g::MAX_TILES), "{n}");
            assert_eq!(m.w, w);
            for t in &m.tiles {
                assert!(t.x >= 0. && t.x + t.w <= w + 0.01, "{n}: {t:?}");
                assert!(t.y >= 0. && t.y + t.h <= m.h + 0.01, "{n}: {t:?}");
                assert_eq!(t.x.fract(), 0., "{n}: whole points");
                assert_eq!(t.w.fract(), 0., "{n}: whole points");
            }
            for (i, a) in m.tiles.iter().enumerate() {
                for b in &m.tiles[i + 1..] {
                    let apart = a.x + a.w + gap <= b.x + 0.01
                        || b.x + b.w + gap <= a.x + 0.01
                        || a.y + a.h + gap <= b.y + 0.01
                        || b.y + b.h + gap <= a.y + 0.01;
                    assert!(apart, "{n}: {a:?} {b:?}");
                }
            }
            let count = |f: fn(&Corners) -> bool| m.tiles.iter().filter(|t| f(&t.corners)).count();
            assert_eq!(count(|c| c.tl), 1, "{n}");
            assert_eq!(count(|c| c.tr), 1, "{n}");
            assert_eq!(count(|c| c.br), 1, "{n}");
            assert_eq!(count(|c| c.bl), 1, "{n}");
            // Each row reaches the right edge.
            let right = m
                .tiles
                .iter()
                .filter(|t| (t.x + t.w - w).abs() < 0.5)
                .count();
            assert!(right >= 1);
        }
    }

    #[test]
    fn three_is_one_large_and_two_stacked() {
        let m = mosaic(3);
        let [big, a, b] = m.tiles[..] else { panic!() };
        assert_eq!(big.h, m.h);
        assert_eq!(a.w, a.h);
        assert_eq!(a.x, b.x);
        assert_eq!(b.y, a.h + f32::from(g::GAP));
        assert!(big.corners.tl && big.corners.bl && a.corners.tr && b.corners.br);
    }
}
