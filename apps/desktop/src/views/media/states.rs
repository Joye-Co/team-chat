//! Upload and preview states of a 产出 file (Media.dc.html §08 状态): each state is a framed
//! list row whose StatusBadge says what is happening; color only on the badge icon.

use std::path::PathBuf;

use gpui::{
    AnyElement, App, Div, ElementId, InteractiveElement as _, IntoElement, ObjectFit,
    ParentElement, SharedString, Styled, StyledImage as _, div, img, prelude::FluentBuilder as _,
    px,
};

use super::{
    MediaKind,
    inline::InlineSpec,
    parts, size_text,
    store::Media,
    viewer::{self, Opening},
};
use crate::{
    copy::{fill, media as cm},
    theme::{self, Type as _, space},
    ui::{self, PushSize, PushStyle, Sym},
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

/// The 32 media box of a row (§2.6): a symbol in the kit box, or the poster cut to the
/// same box.
fn lead_el(lead: &Lead, cx: &App) -> Div {
    let n = crate::ui::native::native_colors(cx);
    let boxed = || {
        div()
            .flex_none()
            .size(crate::ui::native::tokens::avatar::MD)
            .rounded(crate::ui::native::tokens::radius::ITEM)
            .border_1()
            .border_color(n.separator)
            .bg(n.fill)
            .overflow_hidden()
    };
    match lead {
        Lead::Icon(icon) => crate::ui::native::media_box(*icon, cx),
        Lead::Thumb(path) => {
            boxed().child(img(path.clone()).size_full().object_fit(ObjectFit::Cover))
        }
        // The poster is being made: the box with the video symbol.
        Lead::Blank => crate::ui::native::media_box(Sym::Video, cx),
    }
}

/// One state row (§2.6 ListRow, F30): a 1 px frame, the 32 media box, the name 14 medium
/// over a 12 muted line, then the state as a StatusBadge and the action. The state's
/// color sits only on the badge icon. `trailing` is a button on the right.
pub fn row(
    id: impl Into<ElementId>,
    lead: Lead,
    name: impl Into<SharedString>,
    status: &Status,
    trailing: Option<AnyElement>,
    cx: &App,
) -> gpui::Stateful<Div> {
    use crate::ui::native::{BadgeState, StatusBadge};
    let n = crate::ui::native::native_colors(cx);
    let meta: String = match status {
        Status::Uploading { done, total } => {
            let p = if *total > 0 { done * 100 / total } else { 0 };
            fill(
                cm::STATE_UPLOADING,
                &[
                    ("done", &size_text(*done)),
                    ("total", &size_text(*total)),
                    ("p", &p.to_string()),
                ],
            )
        }
        Status::Generating { duration } => match duration {
            Some(d) => format!("{} · {}", cm::STATE_GENERATING, super::time_text(*d)),
            None => cm::STATE_GENERATING.to_string(),
        },
        Status::Ready { meta } => meta.clone(),
        // A size of 0 is unknown, never "0 B" (F5).
        Status::Failed { size: 0 } => cm::STATE_FAILED_HINT.to_string(),
        Status::Failed { size } => format!("{} · {}", cm::STATE_FAILED_HINT, size_text(*size)),
        Status::TooLarge { limit, size } => format!(
            "{} · {}",
            fill(
                cm::STATE_TOO_LARGE_DETAIL,
                &[("size", &size_text(*size)), ("limit", &size_text(*limit))]
            ),
            cm::STATE_TOO_LARGE_HINT
        ),
    };
    let badge = match status {
        Status::Uploading { .. } => {
            Some(StatusBadge::new(BadgeState::Working, cm::BADGE_UPLOADING))
        }
        Status::Generating { .. } => {
            Some(StatusBadge::new(BadgeState::Working, cm::BADGE_GENERATING))
        }
        Status::Ready { .. } => None,
        Status::Failed { .. } => Some(StatusBadge::new(BadgeState::Failed, cm::STATE_FAILED)),
        Status::TooLarge { .. } => Some(StatusBadge::new(BadgeState::Failed, cm::BADGE_TOO_LARGE)),
    };
    let progress = match status {
        Status::Uploading { done, total } if *total > 0 => Some(*done as f32 / *total as f32),
        _ => None,
    };
    div()
        .id(id.into())
        .w_full()
        .rounded(crate::ui::native::tokens::radius::CONTAINER)
        .border_1()
        .border_color(n.separator)
        .bg(n.content)
        .flex()
        .flex_col()
        .child(
            div()
                .min_h(px(56.))
                .px(space::S12)
                .py(space::S12)
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
                                .text_color(n.label)
                                .one_line()
                                .child(name.into()),
                        )
                        .child(
                            div()
                                .t_meta()
                                .tnum()
                                .text_color(n.secondary)
                                .one_line()
                                .child(meta),
                        ),
                )
                .children(badge.map(|b| div().flex_none().child(b)))
                .children(trailing.map(|t| div().flex_none().child(t))),
        )
        .when_some(progress, |el, p| {
            el.child(
                div()
                    .px(space::S12)
                    .pb(space::S12)
                    .child(crate::ui::native::determinate_bar(p, None, cx)),
            )
        })
}

/// The 可以看了 mark: a StatusBadge, color on its icon only.
pub fn ready_mark(_cx: &App) -> AnyElement {
    crate::ui::native::StatusBadge::new(crate::ui::native::BadgeState::Done, cm::STATE_READY)
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
