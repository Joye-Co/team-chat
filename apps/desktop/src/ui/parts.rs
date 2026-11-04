//! Small shared pieces on top of gpui-component (DESIGN.md §4.4, §7, §8.5):
//! toolbar and header bands, the toolbar capsule, keyboard hints, empty
//! states, bands, cards and hairlines.

use gpui::{
    AnyElement, App, ElementId, Hsla, InteractiveElement as _, IntoElement, Keystroke, MouseButton,
    ParentElement, Pixels, RenderOnce, SharedString, Styled, Window, div,
    prelude::FluentBuilder as _, px,
};
use gpui_component::kbd::Kbd;

use crate::{
    theme::{self, Type as _, space},
    ui::icons::{Sym, icon},
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

/// A grouped toolbar: the level-1 float surface (hover toolbar, §7.5).
pub fn capsule(cx: &App) -> gpui::Div {
    crate::ui::overlay::float_surface(cx).flex_none()
}

/// A header band of 92 pt (§4.4): a 52 pt title row and a 40 pt second row,
/// with the 0.5 pt `sep` bottom rule on the same y in every column.
pub fn header_band(cx: &App) -> gpui::Div {
    div()
        .flex_none()
        .w_full()
        .h(theme::HEADER_BAND)
        .flex()
        .flex_col()
        .border_b(theme::HAIRLINE)
        .border_color(theme::colors(cx).sep)
}

/// A keyboard hint: 20 pt kbd keys followed by a 12 pt label (`↵ 发送`).
/// Hints live in tooltips, menus and the switcher footer, never as permanent
/// text in the composer (§7.2).
#[derive(IntoElement)]
pub struct KbdHint {
    keys: Vec<Keystroke>,
    label: Option<SharedString>,
}

impl KbdHint {
    /// `keys` are GPUI keystroke strings: `"cmd-k"`, `"enter"`, `"up"`.
    pub fn new(keys: &[&'static str]) -> Self {
        Self {
            keys: keys
                .iter()
                .filter_map(|k| Keystroke::parse(k).ok())
                .collect(),
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
            .t_meta_medium()
            .children(self.keys.into_iter().map(|stroke| {
                Kbd::new(stroke)
                    .h(theme::size::KBD)
                    .min_w(theme::size::KBD)
                    .px(px(5.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded(theme::radius::SM)
                    .bg(k.field)
                    .border_0()
                    .text_color(k.text_2)
                    .text_size(theme::type_scale::META.0)
            }))
            .when_some(self.label, |el, label| {
                el.child(div().text_color(k.text_2).child(label))
            })
    }
}

/// An empty state (§8.5): a 20 pt icon in `text-3`, a `ui` 14/20 600 title,
/// a `small` 13/20 `text-2` description and an optional action. Centered, at
/// most 320 pt. Never more than one on a screen.
#[derive(IntoElement)]
pub struct EmptyState {
    icon: Sym,
    title: SharedString,
    description: Option<SharedString>,
    action: Option<AnyElement>,
}

impl EmptyState {
    pub fn new(icon: Sym, title: impl Into<SharedString>) -> Self {
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
    fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
        // COMPLETENESS.md §2.13: the one Empty state.
        let mut e = crate::ui::native::feedback::Empty::new(self.icon, self.title);
        if let Some(d) = self.description {
            e = e.description(d);
        }
        if let Some(a) = self.action {
            e = e.action(a);
        }
        e
    }
}

/// A neutral band (offline, reconnecting, reconnected; §8.6): `raised`,
/// 1 pt `border`, radius 8, a 16 pt icon and 13/20 `text-2`. Never red.
/// App-level banners with a detail line use `ui::Banner`.
#[derive(IntoElement)]
pub struct Band {
    icon: Sym,
    icon_color: Option<Hsla>,
    text: SharedString,
    height: Pixels,
    trailing: Option<AnyElement>,
}

impl Band {
    pub fn new(icon: Sym, text: impl Into<SharedString>) -> Self {
        Self {
            icon,
            icon_color: None,
            text: text.into(),
            height: px(40.),
            trailing: None,
        }
    }

    /// The 36 pt sidebar variant.
    pub fn compact(mut self) -> Self {
        self.height = px(36.);
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
            .px(space::S12)
            .rounded(theme::radius::MD)
            .bg(k.raised)
            .border_1()
            .border_color(k.border)
            .flex()
            .items_center()
            .gap(space::S8)
            .child(icon(
                self.icon,
                theme::icon::ROW,
                self.icon_color.unwrap_or(k.text_2),
            ))
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.))
                    .t_small()
                    .text_color(k.text_2)
                    .one_line()
                    .child(self.text),
            )
            .children(self.trailing)
    }
}

/// A card (v3): `bg` fill, 1 pt `line`, radius 12 and `--lift` (no shadow
/// in dark). Nothing inside a card has its own outline. Padding 14 16 16 is
/// the caller's (`card_padding`).
pub fn card(cx: &App) -> gpui::Div {
    // COMPLETENESS.md §2.4: the one card surface.
    crate::ui::native::card_surface(cx)
}

/// A rail section (topic rail, 正在做, 群资料, 来龙去脉, 工作环境): no
/// border, fill or shadow, so a right pane has no box in a box. Sections
/// stack in a [`rail_stack`], which divides them with a 1 pt `line`.
pub fn rail_section(_cx: &App) -> gpui::Div {
    div()
        .py(theme::RAIL_SECTION_PAD_Y)
        .px(theme::RAIL_SECTION_PAD_X)
        .flex()
        .flex_col()
        .gap(space::S8)
}

/// The older name of [`rail_section`].
pub fn rail_card(cx: &App) -> gpui::Div {
    rail_section(cx)
}

/// A column of rail sections with a 1 pt `line` between each two.
pub fn rail_stack(sections: impl IntoIterator<Item = AnyElement>, cx: &App) -> gpui::Div {
    divided(sections, theme::RAIL_SECTION_PAD_X, cx)
}

/// A column of rows with a 1 pt `line` between each two, inset `inset` on
/// both sides: a list inside a card or rail section without a box of its
/// own (§7.10 Refs). Rows that hover pull out with [`inset_row`].
pub fn divided(rows: impl IntoIterator<Item = AnyElement>, inset: Pixels, cx: &App) -> gpui::Div {
    let k = theme::colors(cx);
    let mut out: Vec<AnyElement> = Vec::new();
    for (i, el) in rows.into_iter().enumerate() {
        if i > 0 {
            out.push(
                div()
                    .flex_none()
                    .mx(inset)
                    .h(theme::BORDER)
                    .bg(k.line)
                    .into_any_element(),
            );
        }
        out.push(el);
    }
    div().flex().flex_col().children(out)
}

/// The row of a boxless list: its hover fill reaches `theme::LIST_ROW_BLEED`
/// past the text on each side, so the text stays on the card's column. Put
/// the list in a container with `.mx(-LIST_ROW_BLEED)` and divide it with
/// [`divided`] at the same inset.
pub fn inset_row<E: Styled>(el: E) -> E {
    el.px(theme::LIST_ROW_BLEED).rounded(theme::radius::MD)
}

/// The action row of a card: in the card under its blocks, with no band,
/// rule or fill of its own (§7.10). Answers at the start, a note at the end.
pub fn card_footer(_cx: &App) -> gpui::Div {
    div()
        .flex_none()
        .flex()
        .items_center()
        .gap(space::S8)
        .px(space::S16)
        .pb(theme::card_v2::FOOTER_BOTTOM)
}

/// The inner padding of a card: 14 top, 16 sides and bottom.
pub fn card_padding(el: gpui::Div) -> gpui::Div {
    el.pt(space::S14).px(space::S16).pb(space::S16)
}

/// A popover-level surface (`overlay`, level 2, radius 12).
pub fn floating(cx: &App) -> gpui::Div {
    crate::ui::overlay::overlay_surface(cx)
}

/// A horizontal hairline in `sep`.
pub fn hairline(cx: &App) -> gpui::Div {
    div()
        .flex_none()
        .h(theme::HAIRLINE)
        .w_full()
        .bg(theme::colors(cx).sep)
}

/// The 0.5 pt `sep` edge at the end of a column (§4.4), drawn inside that
/// column as the mockups' `border-right` is: the element takes no width, so
/// the next column starts exactly at the column's width.
pub fn column_edge(cx: &App) -> gpui::Div {
    div().relative().flex_none().w(px(0.)).h_full().child(
        div()
            .absolute()
            .top_0()
            .bottom_0()
            .left(-theme::HAIRLINE)
            .w(theme::HAIRLINE)
            .bg(theme::colors(cx).sep),
    )
}

/// The 0.5 pt `sep` edge at the start of a column (the side panel's
/// `border-left`): add it as the last child of a `relative` column.
pub fn column_start_edge(cx: &App) -> gpui::Div {
    div()
        .absolute()
        .top_0()
        .bottom_0()
        .left_0()
        .w(theme::HAIRLINE)
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
