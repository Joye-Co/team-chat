//! Small shared pieces on top of gpui-component (DESIGN.md §5, §11, §13):
//! toolbar bands, toolbar capsules and icon buttons, keyboard hints, section
//! headers, empty states, bands, cards and hairlines.

use gpui::{
    AnyElement, App, ElementId, Hsla, InteractiveElement as _, IntoElement, Keystroke,
    MouseButton, ParentElement, Pixels, RenderOnce, SharedString, Styled, Window, div,
    prelude::FluentBuilder as _, px,
};
use gpui_component::{
    Sizable as _, Size,
    button::{Button, ButtonVariants as _},
    kbd::Kbd,
};

use crate::{
    theme::{self, Elevation, Type as _, space},
    ui::icons::{Lucide, icon},
};

/// The 52 pt band at the top of a column. Dragging it moves the window and a
/// double click zooms it, as a native title bar does; buttons inside stop the
/// mouse-down, so they still work.
pub fn toolbar_band(id: impl Into<ElementId>) -> gpui::Stateful<gpui::Div> {
    div()
        .id(id)
        .flex_none()
        .h(theme::TOOLBAR_HEIGHT)
        .w_full()
        .flex()
        .items_center()
        .on_mouse_down(MouseButton::Left, |event, window, _| {
            if event.click_count >= 2 {
                window.titlebar_double_click();
            } else {
                window.start_window_move();
            }
        })
}

/// A ghost icon button (32 × 28 in toolbars). Tooltips carry the shortcut.
pub fn icon_button(id: impl Into<ElementId>, name: Lucide) -> Button {
    Button::new(id)
        .ghost()
        .icon(gpui_component::Icon::new(name))
        .with_size(Size::Small)
}

/// The grouped toolbar capsule: 32 pt high, radius 16, capsule shadow.
pub fn capsule(cx: &App) -> gpui::Div {
    let k = theme::colors(cx);
    div()
        .flex_none()
        .h(px(32.))
        .px(space::S2)
        .rounded_full()
        .bg(k.toolbar_capsule)
        .shadow(k.shadow(Elevation::Capsule))
        .flex()
        .items_center()
}

/// A keyboard hint: kbd keys followed by an 11 pt label (`↵ 发送`).
#[derive(IntoElement)]
pub struct KbdHint {
    keys: Vec<Keystroke>,
    label: Option<SharedString>,
}

impl KbdHint {
    /// `keys` are GPUI keystroke strings: `"cmd-k"`, `"enter"`, `"up"`.
    pub fn new(keys: &[&'static str]) -> Self {
        Self {
            keys: keys.iter().filter_map(|k| Keystroke::parse(k).ok()).collect(),
            label: None,
        }
    }

    pub fn label(mut self, label: impl Into<SharedString>) -> Self {
        self.label = Some(label.into());
        self
    }
}

impl RenderOnce for KbdHint {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let k = theme::colors(cx);
        div()
            .flex_none()
            .flex()
            .items_center()
            .gap(space::S4)
            .t_caption()
            .children(self.keys.into_iter().map(|stroke| {
                Kbd::new(stroke)
                    .h(px(16.))
                    .min_w(px(16.))
                    .px(space::S4)
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded(theme::radius::XS)
                    .bg(k.fill_field)
                    .text_color(k.text_3)
                    .text_size(theme::type_scale::CAPTION.0)
            }))
            .when_some(self.label, |el, label| {
                el.child(div().text_color(k.text_2).child(label))
            })
    }
}

/// A section header (11 / 14, 600, `text-3`) with an optional trailing element.
#[derive(IntoElement)]
pub struct SectionHeader {
    label: SharedString,
    trailing: Option<AnyElement>,
}

impl SectionHeader {
    pub fn new(label: impl Into<SharedString>) -> Self {
        Self {
            label: label.into(),
            trailing: None,
        }
    }

    pub fn trailing(mut self, el: impl IntoElement) -> Self {
        self.trailing = Some(el.into_any_element());
        self
    }
}

impl RenderOnce for SectionHeader {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let k = theme::colors(cx);
        div()
            .h(px(20.))
            .flex()
            .items_center()
            .gap(space::S6)
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.))
                    .t_section()
                    .text_color(k.text_3)
                    .one_line()
                    .child(self.label),
            )
            .children(self.trailing)
    }
}

/// An empty state (§13.15): a 20 pt icon in `text-3`, a 13 / 600 title, a
/// 12 / 17 description and an optional action. Centered, at most 280 pt.
#[derive(IntoElement)]
pub struct EmptyState {
    icon: Lucide,
    title: SharedString,
    description: Option<SharedString>,
    action: Option<AnyElement>,
}

impl EmptyState {
    pub fn new(icon: Lucide, title: impl Into<SharedString>) -> Self {
        Self {
            icon,
            title: title.into(),
            description: None,
            action: None,
        }
    }

    pub fn description(mut self, text: impl Into<SharedString>) -> Self {
        self.description = Some(text.into());
        self
    }

    pub fn action(mut self, el: impl IntoElement) -> Self {
        self.action = Some(el.into_any_element());
        self
    }
}

impl RenderOnce for EmptyState {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let k = theme::colors(cx);
        div()
            .size_full()
            .flex()
            .items_center()
            .justify_center()
            .p(space::S20)
            .child(
                div()
                    .max_w(px(280.))
                    .flex()
                    .flex_col()
                    .items_center()
                    .gap(space::S6)
                    .text_center()
                    .child(icon(self.icon, theme::icon::EMPTY, k.text_3))
                    .child(
                        div()
                            .pt(space::S4)
                            .t_ui()
                            .semibold()
                            .text_color(k.text_1)
                            .child(self.title),
                    )
                    .when_some(self.description, |el, d| {
                        el.child(
                            div()
                                .text_size(theme::type_scale::CALLOUT.0)
                                .line_height(px(17.))
                                .text_color(k.text_2)
                                .child(d),
                        )
                    })
                    .when_some(self.action, |el, a| el.child(div().pt(space::S6).child(a))),
            )
    }
}

/// A neutral band (offline, reconnecting, reconnected; §11, §13.14):
/// `fill-field`, radius 8, an icon and 12 pt `text-2`. Never red.
#[derive(IntoElement)]
pub struct Band {
    icon: Lucide,
    icon_color: Option<Hsla>,
    text: SharedString,
    height: Pixels,
    trailing: Option<AnyElement>,
}

impl Band {
    pub fn new(icon: Lucide, text: impl Into<SharedString>) -> Self {
        Self {
            icon,
            icon_color: None,
            text: text.into(),
            height: px(30.),
            trailing: None,
        }
    }

    /// The 28 pt sidebar variant.
    pub fn compact(mut self) -> Self {
        self.height = px(28.);
        self
    }

    pub fn icon_color(mut self, color: Hsla) -> Self {
        self.icon_color = Some(color);
        self
    }

    pub fn trailing(mut self, el: impl IntoElement) -> Self {
        self.trailing = Some(el.into_any_element());
        self
    }
}

impl RenderOnce for Band {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let k = theme::colors(cx);
        div()
            .flex_none()
            .h(self.height)
            .px(space::S10)
            .rounded(theme::radius::ROW)
            .bg(k.fill_field)
            .flex()
            .items_center()
            .gap(space::S8)
            .child(icon(
                self.icon,
                theme::icon::SMALL,
                self.icon_color.unwrap_or(k.text_2),
            ))
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.))
                    .t_callout()
                    .text_color(k.text_2)
                    .one_line()
                    .child(self.text),
            )
            .children(self.trailing)
    }
}

/// A level-1 card on `content`: radius 10, a hairline ring, no shadow.
pub fn card(cx: &App) -> gpui::Div {
    let k = theme::colors(cx);
    div()
        .rounded(theme::radius::LG)
        .bg(k.content)
        .shadow(k.shadow(Elevation::Card))
}

/// A popover-level surface (`elevated`, level 2).
pub fn floating(cx: &App) -> gpui::Div {
    let k = theme::colors(cx);
    div()
        .bg(k.elevated)
        .shadow(k.shadow(Elevation::Popover))
}

/// A horizontal hairline in `sep`.
pub fn hairline(cx: &App) -> gpui::Div {
    div()
        .flex_none()
        .h(theme::HAIRLINE)
        .w_full()
        .bg(theme::colors(cx).sep)
}

/// A vertical hairline in `sep` for column edges.
pub fn vhairline(cx: &App) -> gpui::Div {
    div()
        .flex_none()
        .w(theme::HAIRLINE)
        .h_full()
        .bg(theme::colors(cx).sep)
}

/// Small text button in `accent-text` (重试, 查看全部).
pub fn text_button(id: impl Into<ElementId>, label: impl Into<SharedString>) -> Button {
    Button::new(id).link().label(label).xsmall()
}
