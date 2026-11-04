//! The status badge (COMPLETENESS §2.12): an outline badge, 22 pt, the state
//! word in 12/500 `label` ink and a 12 pt state symbol in the state color.
//! One mapping for every place that shows the state of a run or a check, so
//! a state never reads as free colored text or a bare colored dot.

use gpui::{App, ParentElement as _, SharedString, Styled as _, div, px};
use gpui_component::tag::Tag;

use super::native_colors;
use crate::{
    theme::{self, Type as _},
    ui::{Sym, sf_symbols::SymSize},
};

/// What a [`status_badge`] says.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StatusKind {
    /// 等你 / 等你审 / 等你回答: amber.
    NeedsYou,
    /// 在做, 检查中: blue.
    Working,
    /// 停止中: quiet.
    Stopping,
    /// 排队: quiet.
    Queued,
    /// 完成, 达标: green.
    Done,
    /// 失败, 没达标, 卡住了: red.
    Failed,
    /// 已取消: quiet.
    Idle,
    /// 没检查, 还没开始: quiet, a dashed circle.
    Pending,
}

impl StatusKind {
    fn symbol(self) -> Sym {
        match self {
            StatusKind::NeedsYou => Sym::NeedsYou,
            StatusKind::Working => Sym::Live,
            StatusKind::Stopping => Sym::Spinner,
            StatusKind::Queued => Sym::Waiting,
            StatusKind::Done => Sym::Done,
            StatusKind::Failed => Sym::Failed,
            StatusKind::Idle => Sym::Stopped,
            StatusKind::Pending => Sym::Todo,
        }
    }
}

/// An outline status badge: the symbol in the state color, the word in
/// `label`.
pub fn status_badge(kind: StatusKind, text: impl Into<SharedString>, cx: &App) -> Tag {
    let n = native_colors(cx);
    let color = match kind {
        StatusKind::NeedsYou => n.amber,
        StatusKind::Working => n.blue,
        StatusKind::Done => n.green,
        StatusKind::Failed => n.red,
        StatusKind::Stopping | StatusKind::Queued | StatusKind::Idle | StatusKind::Pending => {
            n.secondary
        }
    };
    Tag::custom(gpui::transparent_black(), n.label, n.separator)
        .outline()
        .rounded(theme::radius::SM)
        .flex_none()
        .h(theme::size::CHIP)
        .px(px(6.))
        .py_0()
        .gap(px(4.))
        .t_meta_medium()
        .whitespace_nowrap()
        .child(kind.symbol().icon(SymSize::INLINE_SMALL).text_color(color))
        .child(div().child(text.into()))
}
