//! Feedback and state components (COMPLETENESS.md §2.13–§2.16, D12, K08,
//! K10): the in-place Alert, the Empty state (full and compact), Skeleton
//! shapes for loading regions, the one Progress bar and the Toast.

use gpui::{
    AnyElement, App, Hsla, IntoElement, ParentElement, Pixels, RenderOnce, SharedString, Styled,
    Window, div, prelude::FluentBuilder as _, px, relative,
};

use super::tokens::{
    NativeColors, NativeType as _, avatar, height, icon, metrics, native_colors, radius, shadow,
    space,
};
use crate::ui::icons::{Sym, icon as sym_icon};

// ---------------------------------------------------------------------------
// Alert (§2.15)

/// The tone of an [`Alert`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum AlertTone {
    /// Foreground icon and title.
    #[default]
    Neutral,
    /// A warning icon (offline, needs attention).
    Warning,
    /// An info icon (a summary, a notice).
    Info,
    /// Title, icon and border tint destructive (a failure).
    Destructive,
}

/// An Alert in place of failed or pending content (§2.15): radius 10, a
/// 1 px border, px 16, py 12, a 16 icon, gap-x 12, title 14 medium,
/// description 14 muted, an action at the right (outline sm "重试").
#[derive(IntoElement)]
pub struct Alert {
    icon: Sym,
    title: SharedString,
    description: Option<SharedString>,
    tone: AlertTone,
    actions: Vec<AnyElement>,
    icon_color: Option<Hsla>,
}

impl Alert {
    pub fn new(icon: Sym, title: impl Into<SharedString>) -> Self {
        Self {
            icon,
            title: title.into(),
            description: None,
            tone: AlertTone::Neutral,
            actions: Vec::new(),
            icon_color: None,
        }
    }

    pub fn description(mut self, text: impl Into<SharedString>) -> Self {
        self.description = Some(text.into());
        self
    }

    pub fn tone(mut self, tone: AlertTone) -> Self {
        self.tone = tone;
        self
    }

    /// An action at the right (outline sm, or primary sm then outline sm).
    pub fn action(mut self, el: impl IntoElement) -> Self {
        self.actions.push(el.into_any_element());
        self
    }

    /// Overrides the icon color (a state color the caller already chose).
    pub fn icon_color(mut self, color: Hsla) -> Self {
        self.icon_color = Some(color);
        self
    }
}

/// The (icon, title, border) colors of an alert tone.
pub fn alert_colors(tone: AlertTone, k: &NativeColors) -> (Hsla, Hsla, Hsla) {
    match tone {
        AlertTone::Neutral => (k.label, k.label, k.separator),
        AlertTone::Warning => (k.amber, k.label, k.separator),
        AlertTone::Info => (k.blue, k.label, k.separator),
        AlertTone::Destructive => (k.red, k.red, k.red.opacity(0.5)),
    }
}

impl RenderOnce for Alert {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let k = native_colors(cx);
        let (ink, title_ink, edge) = alert_colors(self.tone, &k);
        div()
            .flex_none()
            .w_full()
            .px(space::S16)
            .py(space::S12)
            .rounded(radius::CONTAINER)
            .border_1()
            .border_color(edge)
            .bg(k.content)
            .flex()
            .items_start()
            .gap(space::S12)
            .child(
                // The icon centers on the title line (20 high).
                div()
                    .flex_none()
                    .h(px(20.))
                    .flex()
                    .items_center()
                    .child(sym_icon(self.icon, icon::MD, self.icon_color.unwrap_or(ink))),
            )
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.))
                    .flex()
                    .flex_col()
                    .gap(space::S2)
                    .child(
                        div()
                            .n_headline()
                            .text_color(title_ink)
                            .child(self.title),
                    )
                    .when_some(self.description, |d, t| {
                        d.child(div().n_body().text_color(k.secondary).child(t))
                    }),
            )
            .when(!self.actions.is_empty(), |d| {
                d.child(
                    div()
                        .flex_none()
                        .self_center()
                        .flex()
                        .items_center()
                        .gap(space::S8)
                        .children(self.actions),
                )
            })
    }
}

// ---------------------------------------------------------------------------
// Empty (§2.13)

/// The full Empty state (§2.13): centered, gap 24, p 24 (p 48 in a region
/// wider than 640). A 40 icon box (radius 10, `muted` fill, 24 icon), title
/// 18 medium, description 14 muted (max width 384), actions gap 8.
#[derive(IntoElement)]
pub struct Empty {
    icon: Sym,
    title: SharedString,
    description: Option<SharedString>,
    actions: Vec<AnyElement>,
    wide: bool,
    dashed: bool,
}

impl Empty {
    pub fn new(icon: Sym, title: impl Into<SharedString>) -> Self {
        Self {
            icon,
            title: title.into(),
            description: None,
            actions: Vec::new(),
            wide: false,
            dashed: false,
        }
    }

    pub fn description(mut self, text: impl Into<SharedString>) -> Self {
        self.description = Some(text.into());
        self
    }

    /// An action (primary sm, then outline sm).
    pub fn action(mut self, el: impl IntoElement) -> Self {
        self.actions.push(el.into_any_element());
        self
    }

    /// p 48: a region wider than 640.
    pub fn wide(mut self) -> Self {
        self.wide = true;
        self
    }

    /// A dashed border (radius 10): inside a page with other sections.
    pub fn dashed(mut self) -> Self {
        self.dashed = true;
        self
    }
}

/// The 40 icon box of an Empty state or a thread start.
pub fn icon_box(sym: Sym, cx: &App) -> gpui::Div {
    let k = native_colors(cx);
    div()
        .flex_none()
        .size(avatar::LG)
        .rounded(radius::CONTAINER)
        .bg(k.fill)
        .flex()
        .items_center()
        .justify_center()
        .child(sym_icon(sym, icon::LG, k.label))
}

impl RenderOnce for Empty {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let k = native_colors(cx);
        div()
            .size_full()
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .gap(space::S24)
            .p(if self.wide { space::S48 } else { space::S24 })
            .when(self.dashed, |d| {
                d.rounded(radius::CONTAINER)
                    .border_1()
                    .border_dashed()
                    .border_color(k.separator)
            })
            .text_center()
            .child(
                div()
                    .flex()
                    .flex_col()
                    .items_center()
                    .gap(space::S8)
                    .max_w(px(384.))
                    .child(icon_box(self.icon, cx))
                    .child(
                        div()
                            .pt(space::S8)
                            .n_title2()
                            .font_weight(crate::theme::MEDIUM_WEIGHT)
                            .text_color(k.label)
                            .child(self.title),
                    )
                    .when_some(self.description, |d, t| {
                        d.child(div().n_body().text_color(k.secondary).child(t))
                    }),
            )
            .when(!self.actions.is_empty(), |d| {
                d.child(
                    div()
                        .flex()
                        .items_center()
                        .gap(space::S8)
                        .children(self.actions),
                )
            })
    }
}

/// The compact Empty (§2.13): a 16 icon and a 14 muted line, left-aligned
/// in its section, with an optional ghost sm action.
#[derive(IntoElement)]
pub struct EmptyLine {
    icon: Option<Sym>,
    text: SharedString,
    action: Option<AnyElement>,
}

impl EmptyLine {
    pub fn new(text: impl Into<SharedString>) -> Self {
        Self {
            icon: None,
            text: text.into(),
            action: None,
        }
    }

    pub fn icon(mut self, sym: Sym) -> Self {
        self.icon = Some(sym);
        self
    }

    pub fn action(mut self, el: impl IntoElement) -> Self {
        self.action = Some(el.into_any_element());
        self
    }
}

impl RenderOnce for EmptyLine {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let k = native_colors(cx);
        div()
            .w_full()
            .min_h(height::SM)
            .flex()
            .items_center()
            .gap(space::S8)
            .children(self.icon.map(|s| sym_icon(s, icon::MD, k.secondary)))
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.))
                    .n_body()
                    .text_color(k.secondary)
                    .child(self.text),
            )
            .children(self.action)
    }
}

// ---------------------------------------------------------------------------
// Skeleton (§2.14)

/// A skeleton bar: `muted` fill, radius 8, pulsing (gpui-component
/// `Skeleton`).
pub fn skeleton(w: impl Into<gpui::Length>, h: Pixels) -> gpui_component::skeleton::Skeleton {
    gpui_component::skeleton::Skeleton::new()
        .w(w.into())
        .h(h)
        .rounded(radius::CONTROL)
}

/// The skeleton of a region, with the final shapes (§2.14).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SkeletonKind {
    /// Circle 32 and lines h14 at 40 % and 70 %.
    Message,
    /// A 16 square and a h14 bar, pitch 32.
    SidebarItem,
    /// A 32 box and lines h14 / h12, h56.
    ListRow,
    /// A row at the table row height.
    TableRow,
}

/// `rows` skeleton rows of `kind` (3–6 per region).
pub fn skeleton_rows(kind: SkeletonKind, rows: usize) -> gpui::Div {
    let row = |ix: usize| -> AnyElement {
        // Widths vary a little row to row, so the block reads as text.
        let vary = [0.9_f32, 0.7, 0.8, 0.6, 0.85, 0.75][ix % 6];
        match kind {
            SkeletonKind::Message => div()
                .w_full()
                .flex()
                .gap(space::S12)
                .child(skeleton(avatar::MD, avatar::MD).rounded_full())
                .child(
                    div()
                        .flex_1()
                        .flex()
                        .flex_col()
                        .gap(space::S8)
                        .pt(space::S2)
                        .child(skeleton(relative(0.4), px(14.)))
                        .child(skeleton(relative(0.7 * vary / 0.8), px(14.))),
                )
                .into_any_element(),
            SkeletonKind::SidebarItem => div()
                .w_full()
                .h(height::ROW)
                .px(space::S8)
                .flex()
                .items_center()
                .gap(space::S8)
                .child(skeleton(icon::MD, icon::MD).rounded(radius::CHECKBOX))
                .child(skeleton(relative(0.5 + 0.4 * vary), px(14.)))
                .into_any_element(),
            SkeletonKind::ListRow => div()
                .w_full()
                .h(height::LIST_ROW)
                .px(space::S12)
                .flex()
                .items_center()
                .gap(space::S12)
                .child(skeleton(avatar::MD, avatar::MD).rounded(radius::ITEM))
                .child(
                    div()
                        .flex_1()
                        .flex()
                        .flex_col()
                        .gap(space::S6)
                        .child(skeleton(relative(0.5 * vary + 0.2), px(14.)))
                        .child(skeleton(relative(0.35), px(12.))),
                )
                .into_any_element(),
            SkeletonKind::TableRow => div()
                .w_full()
                .h(height::TABLE_ROW)
                .px(space::S16)
                .flex()
                .items_center()
                .gap(space::S24)
                .child(skeleton(relative(0.3 * vary + 0.1), px(14.)))
                .child(skeleton(relative(0.2), px(14.)))
                .child(skeleton(relative(0.15), px(14.)))
                .into_any_element(),
        }
    };
    let gap = match kind {
        SkeletonKind::Message => space::S16,
        _ => px(0.),
    };
    div()
        .w_full()
        .flex()
        .flex_col()
        .gap(gap)
        .children((0..rows).map(row))
}

// ---------------------------------------------------------------------------
// Progress (D12)

/// The one Progress (D12): h4, radius full, track = primary at 20 %, value
/// = primary. `value` is 0..=1.
pub fn progress(value: f32, cx: &App) -> gpui::Div {
    let k = native_colors(cx);
    let v = value.clamp(0., 1.);
    div()
        .w_full()
        .h(metrics::BAR)
        .rounded_full()
        .bg(k.accent.opacity(0.2))
        .overflow_hidden()
        .child(div().h_full().w(relative(v)).rounded_full().bg(k.accent))
}

/// The steps variant of Progress (D12): one segment per step, 2 px gaps,
/// done segments in `primary`, the rest primary at 20 %.
pub fn progress_steps(done: usize, total: usize, cx: &App) -> gpui::Div {
    let k = native_colors(cx);
    let total = total.max(1);
    div()
        .w_full()
        .h(metrics::BAR)
        .flex()
        .gap(space::S2)
        .children((0..total).map(|ix| {
            div()
                .flex_1()
                .h_full()
                .rounded_full()
                .bg(if ix < done {
                    k.accent
                } else {
                    k.accent.opacity(0.2)
                })
        }))
}

// ---------------------------------------------------------------------------
// Toast (§2.16)

/// The toast surface (§2.16): float surface, radius 10, a 1 px border,
/// shadow md, p 16, w 360.
pub fn toast_surface(cx: &App) -> gpui::Div {
    let k = native_colors(cx);
    div()
        .w(px(360.))
        .p(space::S16)
        .rounded(radius::CONTAINER)
        .bg(k.menu)
        .border_1()
        .border_color(k.separator)
        .shadow(shadow::md())
        .text_color(k.label)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn alert_tones_tint_only_the_destructive_title() {
        let k = NativeColors::light();
        let (_, title, edge) = alert_colors(AlertTone::Warning, &k);
        assert_eq!(title, k.label);
        assert_eq!(edge, k.separator);
        let (icon, title, _) = alert_colors(AlertTone::Destructive, &k);
        assert_eq!(icon, k.red);
        assert_eq!(title, k.red);
    }

    #[test]
    fn the_progress_bar_is_h4() {
        assert_eq!(metrics::BAR, px(4.));
    }
}
