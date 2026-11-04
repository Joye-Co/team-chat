//! Badges (COMPLETENESS.md §1.8, §2.12, K11): the one Badge, the
//! StatusBadge that carries every state of the app, the count pill and the
//! key cap. A state color only ever colors the 12 icon of a StatusBadge;
//! the text stays foreground.

use gpui::{
    AnyElement, App, Hsla, IntoElement, ParentElement, RenderOnce, SharedString, Styled, Window,
    div, prelude::FluentBuilder as _, px,
};

use super::tokens::{NativeColors, NativeType as _, height, icon, native_colors, radius, space};
use crate::{
    theme::StateTone,
    ui::icons::{Sym, icon as sym_icon},
};

/// The variant of a [`Badge`] (§1.8).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum BadgeVariant {
    /// `primary` fill, `primary-foreground` text.
    Default,
    /// `muted` fill, foreground text.
    Secondary,
    /// No fill, a 1 px border, foreground text.
    #[default]
    Outline,
    /// `destructive` fill, white text.
    Destructive,
}

/// A badge (§1.8): h22, px 8 (px 6 in tables), radius full, 12 medium,
/// 1 px border, an optional 12 icon, gap 4.
#[derive(IntoElement)]
pub struct Badge {
    label: SharedString,
    variant: BadgeVariant,
    lead: Option<AnyElement>,
    dense: bool,
    max_w: Option<gpui::Pixels>,
}

impl Badge {
    pub fn new(label: impl Into<SharedString>) -> Self {
        Self {
            label: label.into(),
            variant: BadgeVariant::Outline,
            lead: None,
            dense: false,
            max_w: None,
        }
    }

    pub fn variant(mut self, variant: BadgeVariant) -> Self {
        self.variant = variant;
        self
    }

    pub fn secondary(self) -> Self {
        self.variant(BadgeVariant::Secondary)
    }

    /// A 12 icon before the label.
    pub fn icon(mut self, sym: Sym, color: Hsla) -> Self {
        self.lead = Some(sym_icon(sym, icon::SM, color).into_any_element());
        self
    }

    /// Any 12 element before the label (a spinner, a dot).
    pub fn lead(mut self, el: impl IntoElement) -> Self {
        self.lead = Some(el.into_any_element());
        self
    }

    /// px 6: a badge in a table cell.
    pub fn dense(mut self) -> Self {
        self.dense = true;
        self
    }

    /// Truncates the label at the end beyond `w`.
    pub fn max_w(mut self, w: gpui::Pixels) -> Self {
        self.max_w = Some(w);
        self
    }
}

/// The (fill, border, text) of a badge variant.
pub fn badge_colors(variant: BadgeVariant, k: &NativeColors) -> (Hsla, Hsla, Hsla) {
    let clear = gpui::transparent_black();
    match variant {
        BadgeVariant::Default => (k.accent, clear, k.on_accent),
        BadgeVariant::Secondary => (k.fill, clear, k.label),
        BadgeVariant::Outline => (clear, k.separator, k.label),
        BadgeVariant::Destructive => (k.red, clear, gpui::white()),
    }
}

impl RenderOnce for Badge {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let k = native_colors(cx);
        let (fill, edge, text) = badge_colors(self.variant, &k);
        div()
            .flex_none()
            .h(height::BADGE)
            .px(if self.dense { space::S6 } else { space::S8 })
            .rounded_full()
            .bg(fill)
            .border_1()
            .border_color(edge)
            .flex()
            .items_center()
            .gap(space::S4)
            .n_section()
            .text_color(text)
            .when_some(self.max_w, |d, w| d.max_w(w))
            .children(self.lead.map(|l| div().flex_none().child(l)))
            .child(
                div()
                    .min_w(px(0.))
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .text_ellipsis()
                    .child(self.label),
            )
    }
}

/// The states of §2.12, each with one icon and one color everywhere.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BadgeState {
    /// 等你 / 等你审 / 等你回答: alert-circle, warning.
    NeedsYou,
    /// 在做 3/5: a spinning loader, info.
    Working,
    /// 停止中: a loader, muted.
    Stopping,
    /// 排队: clock, muted.
    Queued,
    /// 完成 / 已合并 / 已回答: check-circle (filled), success.
    Done,
    /// 失败 / 卡住了: x-circle, destructive.
    Failed,
    /// 异常: alert-triangle, destructive.
    Abnormal,
    /// 在线: a filled 8 dot, success.
    Online,
    /// 离线 / 空闲 / 不在: an 8 ring, muted.
    Offline,
    /// 已停用: ban, muted.
    Deactivated,
    /// No state icon (a quiet word such as 等李雪).
    Quiet,
}

impl BadgeState {
    /// The icon color of the state.
    pub fn color(self, k: &NativeColors) -> Hsla {
        match self {
            BadgeState::NeedsYou => k.amber,
            BadgeState::Working => k.blue,
            BadgeState::Done | BadgeState::Online => k.green,
            BadgeState::Failed | BadgeState::Abnormal => k.red,
            BadgeState::Stopping
            | BadgeState::Queued
            | BadgeState::Offline
            | BadgeState::Deactivated
            | BadgeState::Quiet => k.secondary,
        }
    }

    pub fn symbol(self) -> Option<Sym> {
        Some(match self {
            BadgeState::NeedsYou => Sym::NeedsYou,
            BadgeState::Done => Sym::Done,
            BadgeState::Failed => Sym::Failed,
            BadgeState::Abnormal => Sym::Problem,
            BadgeState::Queued => Sym::Waiting,
            BadgeState::Deactivated => Sym::Stopped,
            _ => return None,
        })
    }

    /// The state of a subtitle tone (toolbar, cards).
    pub fn from_tone(tone: StateTone) -> Self {
        match tone {
            StateTone::You => BadgeState::NeedsYou,
            StateTone::Live => BadgeState::Working,
            StateTone::Problem => BadgeState::Failed,
            StateTone::Done => BadgeState::Done,
            StateTone::Quiet => BadgeState::Quiet,
        }
    }
}

/// The 12 mark of a state: its icon, a spinner or a presence dot.
pub fn state_mark(state: BadgeState, k: &NativeColors) -> Option<AnyElement> {
    let color = state.color(k);
    match state {
        BadgeState::Working | BadgeState::Stopping => Some(
            div()
                .size(icon::SM)
                .flex()
                .items_center()
                .justify_center()
                .child(
                    gpui_component::Sizable::with_size(crate::ui::motion::spinner(), icon::SM)
                        .color(color),
                )
                .into_any_element(),
        ),
        BadgeState::Online => Some(
            div()
                .size(icon::SM)
                .flex()
                .items_center()
                .justify_center()
                .child(div().size(px(8.)).rounded_full().bg(color))
                .into_any_element(),
        ),
        BadgeState::Offline => Some(
            div()
                .size(icon::SM)
                .flex()
                .items_center()
                .justify_center()
                .child(
                    div()
                        .size(px(8.))
                        .rounded_full()
                        .border_1()
                        .border_color(color),
                )
                .into_any_element(),
        ),
        BadgeState::Quiet => None,
        s => s
            .symbol()
            .map(|sym| sym_icon(sym, icon::SM, color).into_any_element()),
    }
}

/// The StatusBadge (§2.12): an outline [`Badge`] with the state's colored
/// 12 icon and the state words in foreground. The one status carrier of the
/// app: never a free colored dot or colored text.
#[derive(IntoElement)]
pub struct StatusBadge {
    state: BadgeState,
    label: SharedString,
    dense: bool,
    max_w: Option<gpui::Pixels>,
}

impl StatusBadge {
    pub fn new(state: BadgeState, label: impl Into<SharedString>) -> Self {
        Self {
            state,
            label: label.into(),
            dense: false,
            max_w: None,
        }
    }

    pub fn dense(mut self) -> Self {
        self.dense = true;
        self
    }

    pub fn max_w(mut self, w: gpui::Pixels) -> Self {
        self.max_w = Some(w);
        self
    }
}

impl RenderOnce for StatusBadge {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let k = native_colors(cx);
        let mut b = Badge::new(self.label);
        if let Some(mark) = state_mark(self.state, &k) {
            b = b.lead(mark);
        }
        if self.dense {
            b = b.dense();
        }
        if let Some(w) = self.max_w {
            b = b.max_w(w);
        }
        b
    }
}

/// The count pill (§1.8): a 20 circle (min width 20), 12 medium tabular,
/// `muted` fill. Nothing for 0; 99+ above 99.
#[derive(IntoElement)]
pub struct CountPill {
    text: SharedString,
    show: bool,
    tone: Option<Hsla>,
}

impl CountPill {
    pub fn new(count: u32) -> Self {
        Self {
            text: cap(count).into(),
            show: count > 0,
            tone: None,
        }
    }

    /// A pill with its own text (`@3`).
    pub fn text(text: impl Into<SharedString>) -> Self {
        Self {
            text: text.into(),
            show: true,
            tone: None,
        }
    }

    /// Text color (a mention count keeps foreground; a quiet one muted).
    pub fn tone(mut self, color: Hsla) -> Self {
        self.tone = Some(color);
        self
    }
}

/// 99+ above 99.
pub fn cap(count: u32) -> String {
    if count > 99 {
        crate::copy::common::COUNT_CAP.to_string()
    } else {
        count.to_string()
    }
}

impl RenderOnce for CountPill {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        if !self.show {
            return div().into_any_element();
        }
        let k = native_colors(cx);
        div()
            .flex_none()
            .h(height::COUNT)
            .min_w(height::COUNT)
            .px(space::S6)
            .rounded_full()
            .bg(k.fill)
            .flex()
            .items_center()
            .justify_center()
            .n_section()
            .n_figures()
            .text_color(self.tone.unwrap_or(k.label))
            .child(self.text)
            .into_any_element()
    }
}

/// A key cap (§1.8): h20, min width 20, px 4, radius 4, 12 medium, a 1 px
/// border. One cap per key.
#[derive(IntoElement)]
pub struct KeyCap {
    label: SharedString,
}

impl KeyCap {
    pub fn new(label: impl Into<SharedString>) -> Self {
        Self {
            label: label.into(),
        }
    }
}

impl RenderOnce for KeyCap {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let k = native_colors(cx);
        div()
            .flex_none()
            .h(height::KBD)
            .min_w(height::KBD)
            .px(space::S4)
            .rounded(radius::CHECKBOX)
            .border_1()
            .border_color(k.separator)
            .bg(k.content)
            .flex()
            .items_center()
            .justify_center()
            .n_section()
            .text_color(k.secondary)
            .child(self.label)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn badge_and_pill_sizes() {
        assert_eq!(height::BADGE, px(22.));
        assert_eq!(height::COUNT, px(20.));
        assert_eq!(height::KBD, px(20.));
        assert_eq!(cap(1000), "99+");
        assert_eq!(cap(7), "7");
    }

    #[test]
    fn every_state_has_one_mark_and_color() {
        let k = NativeColors::light();
        assert_eq!(BadgeState::NeedsYou.color(&k), k.amber);
        assert_eq!(BadgeState::Working.color(&k), k.blue);
        assert_eq!(BadgeState::Done.color(&k), k.green);
        assert_eq!(BadgeState::Failed.color(&k), k.red);
        assert_eq!(BadgeState::Queued.color(&k), k.secondary);
        for s in [
            BadgeState::NeedsYou,
            BadgeState::Working,
            BadgeState::Stopping,
            BadgeState::Queued,
            BadgeState::Done,
            BadgeState::Failed,
            BadgeState::Abnormal,
            BadgeState::Online,
            BadgeState::Offline,
            BadgeState::Deactivated,
        ] {
            assert!(state_mark(s, &k).is_some(), "{s:?} has no mark");
        }
        assert!(state_mark(BadgeState::Quiet, &k).is_none());
        // The outline badge text is foreground, never the state color.
        let (_, edge, text) = badge_colors(BadgeVariant::Outline, &k);
        assert_eq!(text, k.label);
        assert_eq!(edge, k.separator);
    }
}
