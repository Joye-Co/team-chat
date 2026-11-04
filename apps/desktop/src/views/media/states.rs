//! Upload and preview states of a 产出 file (Media.dc.html §08 状态): each state says what is
//! happening in one line with a glyph and words; only 发不了 is red. Never color only.

use std::path::PathBuf;

use gpui::{
    AnyElement, App, Div, ElementId, InteractiveElement as _, IntoElement, ObjectFit,
    ParentElement, SharedString, Styled, StyledImage as _, div, img, prelude::FluentBuilder as _,
    px,
};

use super::{
    MediaKind,
    inline::InlineSpec,
    parts::{self, mono},
    size_text,
    store::Media,
    viewer::{self, Opening},
};
use crate::{
    copy::{fill, media as cm},
    theme::{self, Type as _, radius, space},
    ui::{self, PushStyle, PushSize, Sym},
};

/// What the row shows on its left.
pub enum Lead {
    Icon(Sym),
    Thumb(PathBuf),
    /// A dark thumbnail with nothing in it yet (the poster is being made).
    Blank,
}

pub enum Status {
    Uploading { done: u64, total: u64 },
    Generating { duration: Option<f64> },
    Ready { meta: String },
    Failed { size: u64 },
    TooLarge { limit: u64, size: u64 },
}

fn lead_el(lead: &Lead, cx: &App) -> Div {
    let k = theme::colors(cx);
    match lead {
        // A plain `secondary` symbol, no tile (NATIVE.md §6).
        Lead::Icon(icon) => div()
            .flex_none()
            .size(px(28.))
            .flex()
            .items_center()
            .justify_center()
            .child(ui::icon(*icon, px(20.), k.text_3)),
        Lead::Thumb(path) => div()
            .flex_none()
            .w(px(64.))
            .h(crate::ui::native::tokens::height::LG)
            .rounded(radius::MARK)
            .overflow_hidden()
            .bg(super::screen(cx).bg)
            .child(img(path.clone()).size_full().object_fit(ObjectFit::Cover)),
        Lead::Blank => div()
            .flex_none()
            .w(px(64.))
            .h(crate::ui::native::tokens::height::LG)
            .rounded(radius::MARK)
            .bg(super::screen(cx).bg),
    }
}

/// One state row. `trailing` is a button or a mark on the right.
pub fn row(
    id: impl Into<ElementId>,
    lead: Lead,
    name: impl Into<SharedString>,
    status: &Status,
    trailing: Option<AnyElement>,
    cx: &App,
) -> gpui::Stateful<Div> {
    let k = theme::colors(cx);
    let line: AnyElement = match status {
        Status::Uploading { done, total } => {
            let p = if *total > 0 { done * 100 / total } else { 0 };
            mono(
                fill(
                    cm::STATE_UPLOADING,
                    &[
                        ("done", &size_text(*done)),
                        ("total", &size_text(*total)),
                        ("p", &p.to_string()),
                    ],
                ),
                k.text_3,
            )
            .into_any_element()
        }
        Status::Generating { .. } => div()
            .t_meta()
            .text_color(k.text_3)
            .child(cm::STATE_GENERATING)
            .into_any_element(),
        Status::Ready { meta } => mono(meta.clone(), k.text_3).into_any_element(),
        Status::Failed { size } => div()
            .flex()
            .items_center()
            .gap(space::S4)
            .t_meta()
            .text_color(k.text_3)
            .child(ui::icon(Sym::Alert, px(12.), k.text_3))
            .child(format!(
                "{} · {}",
                cm::STATE_FAILED_DETAIL,
                size_text(*size)
            ))
            .into_any_element(),
        Status::TooLarge { limit, size } => div()
            .flex()
            .items_center()
            .gap(space::S4)
            .t_meta()
            .child(
                div()
                    .text_color(k.danger_text)
                    .child(fill(cm::STATE_TOO_LARGE, &[("limit", &size_text(*limit))])),
            )
            .child(div().text_color(k.text_3).child(format!(
                "· {} · {}",
                size_text(*size),
                cm::STATE_TOO_LARGE_HINT
            )))
            .into_any_element(),
    };
    let progress = match status {
        Status::Uploading { done, total } if *total > 0 => Some(*done as f32 / *total as f32),
        _ => None,
    };
    let trailing = trailing.or_else(|| match status {
        Status::Generating { duration: Some(d) } => {
            Some(mono(super::time_text(*d), k.text_2).into_any_element())
        }
        _ => None,
    });
    div()
        .id(id.into())
        .w_full()
        .rounded(crate::ui::native::cards::card::RADIUS)
        .bg(crate::ui::native::native_colors(cx).fill)
        .flex()
        .flex_col()
        .child(
            div()
                .min_h(px(56.))
                .px(space::S10)
                .py(space::S8)
                .flex()
                .items_center()
                .gap(space::S12)
                .child(lead_el(&lead, cx))
                .child(
                    div()
                        .flex_1()
                        .min_w(px(0.))
                        .flex()
                        .flex_col()
                        .gap(space::S2)
                        .child(
                            div()
                                .t_small()
                                .medium()
                                .text_color(k.text_1)
                                .one_line()
                                .child(name.into()),
                        )
                        .child(line),
                )
                .children(trailing),
        )
        .when_some(progress, |el, p| {
            el.child(
                div()
                    .px(space::S10)
                    .pb(space::S8)
                    .child(crate::ui::native::determinate_bar(p, None, cx)),
            )
        })
}

/// The ✓ 可以看了 mark.
pub fn ready_mark(cx: &App) -> AnyElement {
    let k = theme::colors(cx);
    div()
        .flex()
        .items_center()
        .gap(space::S4)
        .t_meta()
        .text_color(k.text_2)
        .child(ui::icon(Sym::Check, px(13.), k.success_text))
        .child(cm::STATE_READY)
        .into_any_element()
}

fn small_button(
    id: impl Into<ElementId>,
    label: &'static str,
    icon: Option<Sym>,
    cx: &App,
) -> gpui_component::button::Button {
    let b = ui::native::push_button(id, label, PushStyle::Outline, PushSize::Small, cx);
    match icon {
        Some(i) => b.icon(ui::icon(i, px(13.), theme::colors(cx).text_2)),
        None => b,
    }
}

/// The preview could not load (download failed): file row with 用系统应用打开.
pub fn failed_row(spec: &InlineSpec, cx: &App) -> gpui::Stateful<Div> {
    let s = spec.clone();
    row(
        ElementId::Name(format!("{}-failed", spec.id).into()),
        Lead::Icon(spec.kind.icon()),
        spec.title.clone(),
        &Status::Failed { size: spec.bytes },
        Some(
            small_button(
                format!("{}-sys", spec.id),
                cm::OPEN,
                Some(Sym::OpenExternal),
                cx,
            )
            .on_click(move |_, _, cx| {
                viewer::open(
                    s.key.clone(),
                    s.title.clone(),
                    s.target.clone(),
                    Opening::default(),
                    cx,
                )
            })
            .into_any_element(),
        ),
        cx,
    )
    .max_w(super::INLINE_W)
}

/// The version is on disk but this machine cannot make a preview of it.
pub fn failed_media_row(spec: &InlineSpec, media: &Media, cx: &App) -> gpui::Stateful<Div> {
    let path = media.entry_path();
    let icon = if media.kind == MediaKind::File {
        spec.kind.icon()
    } else {
        media.kind.icon()
    };
    row(
        ElementId::Name(format!("{}-failed", spec.id).into()),
        Lead::Icon(icon),
        media.file_name().to_string(),
        &Status::Failed { size: media.bytes },
        Some(
            small_button(
                format!("{}-sys", spec.id),
                cm::OPEN_WITH_SYSTEM,
                Some(Sym::OpenExternal),
                cx,
            )
            .on_click(move |_, _, cx| parts::open_with_system(&path, cx))
            .into_any_element(),
        ),
        cx,
    )
    .max_w(super::INLINE_W)
}

/// A large version that the timeline does not load by itself.
pub fn big_row(spec: &InlineSpec, cx: &App) -> gpui::Stateful<Div> {
    let s = spec.clone();
    row(
        ElementId::Name(format!("{}-big", spec.id).into()),
        Lead::Icon(spec.kind.icon()),
        spec.title.clone(),
        &Status::Ready {
            meta: fill(cm::STATE_BIG, &[("size", &size_text(spec.bytes))]),
        },
        Some(
            small_button(format!("{}-open", spec.id), cm::OPEN, None, cx)
                .on_click(move |_, _, cx| {
                    viewer::open(
                        s.key.clone(),
                        s.title.clone(),
                        s.target.clone(),
                        Opening::default(),
                        cx,
                    )
                })
                .into_any_element(),
        ),
        cx,
    )
    .max_w(super::INLINE_W)
}

/// Buttons used by the state scene (取消, 移除, 用系统应用打开).
pub fn action(
    id: impl Into<ElementId>,
    label: &'static str,
    icon: Option<Sym>,
    cx: &App,
) -> AnyElement {
    small_button(id, label, icon, cx).into_any_element()
}
