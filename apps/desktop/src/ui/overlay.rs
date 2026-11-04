//! Floating surfaces (DESIGN.md §5, §7.12, §8.6): popovers, menus and their
//! rows, the @mention popup, the hover toolbar, toasts and banners.

use gpui::{
    AnyElement, App, ElementId, InteractiveElement as _, IntoElement, ParentElement, RenderOnce,
    SharedString, Styled, Window, div, prelude::FluentBuilder as _, px,
};

use crate::{
    theme::{self, Type as _, size, space},
    ui::icons::{Sym, icon},
    ui::native::NativeType as _,
};

/// A level-2 overlay surface: `overlay` fill, radius 12, the overlay shadow
/// (a visible 1 pt edge in dark mode). Popovers, menus, the switcher,
/// dialogs and toasts use it.
pub fn overlay_surface(cx: &App) -> gpui::Div {
    // COMPLETENESS.md §1.9 float surface: opaque `card`, radius 8, a 1 px
    // border and shadow md.
    let n = crate::ui::native::native_colors(cx);
    div()
        .bg(n.menu)
        .text_color(n.label)
        .rounded(crate::ui::native::metrics::MENU_RADIUS)
        .border_1()
        .border_color(n.separator)
        .shadow(crate::ui::native::tokens::shadow::md())
}

/// A menu surface: the float surface with p 4 (§2.11).
pub fn menu_surface(cx: &App) -> gpui::Div {
    overlay_surface(cx)
        .min_w(px(128.))
        .p(crate::ui::native::metrics::MENU_PAD)
        .flex()
        .flex_col()
}

/// The @mention popup (§2.11): a menu, w 320, max 8 rows.
pub fn mention_surface(cx: &App) -> gpui::Div {
    overlay_surface(cx)
        .w(size::MENTION_POPUP_WIDTH)
        .max_h(size::MENU_ROW * 8. + crate::ui::native::metrics::MENU_PAD * 2.)
        .p(crate::ui::native::metrics::MENU_PAD)
        .flex()
        .flex_col()
}

/// A small floating capsule (the hover toolbar): the float surface, p 2.
pub fn float_surface(cx: &App) -> gpui::Div {
    overlay_surface(cx).p(space::S2).flex().items_center()
}

/// A menu row (§7.12): 32 pt, radius 6, padding 0 8, gap 8, a 16 pt icon in
/// `text-2` and `ui` 14 text. `active` is the keyboard-active row.
pub fn menu_row(
    id: impl Into<ElementId>,
    glyph: Option<Sym>,
    label: impl Into<SharedString>,
    active: bool,
    cx: &App,
) -> gpui::Stateful<gpui::Div> {
    // §2.11: h32, px 8, radius 6, 14 text, a 16 icon, gap 8; the active row
    // is the `accent` fill (D10).
    let n = crate::ui::native::native_colors(cx);
    popup_row(id, active, cx)
        .when_some(glyph, |el, g| {
            el.child(icon(g, crate::ui::native::tokens::icon::MD, n.secondary))
        })
        .child(div().flex_1().min_w(px(0.)).one_line().child(label.into()))
}

/// A menu separator (§2.11): 1 px, edge to edge, my 4.
pub fn menu_separator(cx: &App) -> gpui::Div {
    crate::ui::native::menu_separator(false, cx)
}

/// A row of a pop-up list (mention popup, pickers): the menu item of §2.11.
pub fn popup_row(id: impl Into<ElementId>, active: bool, cx: &App) -> gpui::Stateful<gpui::Div> {
    let n = crate::ui::native::native_colors(cx);
    div()
        .id(id)
        .flex_none()
        .w_full()
        .h(size::MENU_ROW)
        .px(crate::ui::native::metrics::MENU_ROW_PAD)
        .gap(space::S8)
        .rounded(crate::ui::native::metrics::MENU_ROW_RADIUS)
        .flex()
        .items_center()
        .t_ui()
        .text_color(n.label)
        .when(active, |el| el.bg(n.fill_hover))
        .when(!active, |el| el.hover(|s| s.bg(n.fill_hover)))
}

/// The return glyph at the end of the active popup row.
pub fn return_hint(cx: &App) -> impl IntoElement {
    icon(Sym::Return, theme::icon::CHIP, theme::colors(cx).text_3)
}

/// The tone of a toast or banner.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tone {
    Neutral,
    Success,
    Danger,
}

/// A toast: the overlay surface, 44 pt, padding 0 12 0 14, a 16 pt glyph,
/// `ui` 14 text and an optional `sm` action.
#[derive(IntoElement)]
pub struct Toast {
    glyph: Sym,
    tone: Tone,
    text: SharedString,
    action: Option<AnyElement>,
}

impl Toast {
    pub fn new(glyph: Sym, text: impl Into<SharedString>) -> Self {
        Self {
            glyph,
            tone: Tone::Neutral,
            text: text.into(),
            action: None,
        }
    }

    pub fn tone(mut self, tone: Tone) -> Self {
        self.tone = tone;
        self
    }

    pub fn action(mut self, el: impl IntoElement) -> Self {
        self.action = Some(el.into_any_element());
        self
    }
}

impl RenderOnce for Toast {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        // COMPLETENESS.md §2.16: the toast surface, a 16 icon, 14 medium
        // text, an outline sm action.
        let n = crate::ui::native::native_colors(cx);
        let glyph_color = match self.tone {
            Tone::Neutral => n.label,
            Tone::Success => n.green,
            Tone::Danger => n.red,
        };
        crate::ui::native::feedback::toast_surface(cx)
            .flex()
            .items_center()
            .gap(space::S12)
            .child(icon(
                self.glyph,
                crate::ui::native::tokens::icon::MD,
                glyph_color,
            ))
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.))
                    .n_headline()
                    .text_color(n.label)
                    .one_line()
                    .child(self.text),
            )
            .children(self.action)
    }
}

/// An app-level banner (update ready, offline; §8.6): a 48 pt card above the
/// sidebar footer, radius 8, `raised`, 1 pt `border`, a 16 pt glyph, 13/20
/// text and an optional `sm` action. Never in the topic column.
#[derive(IntoElement)]
pub struct Banner {
    glyph: Sym,
    glyph_color: Option<gpui::Hsla>,
    text: SharedString,
    detail: Option<SharedString>,
    action: Option<AnyElement>,
}

impl Banner {
    pub fn new(glyph: Sym, text: impl Into<SharedString>) -> Self {
        Self {
            glyph,
            glyph_color: None,
            text: text.into(),
            detail: None,
            action: None,
        }
    }

    pub fn glyph_color(mut self, color: gpui::Hsla) -> Self {
        self.glyph_color = Some(color);
        self
    }

    /// A second line in 12/16 `text-3`.
    pub fn detail(mut self, text: impl Into<SharedString>) -> Self {
        self.detail = Some(text.into());
        self
    }

    pub fn action(mut self, el: impl IntoElement) -> Self {
        self.action = Some(el.into_any_element());
        self
    }
}

impl RenderOnce for Banner {
    fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
        // COMPLETENESS.md §2.15, §2.16: an in-page banner is the Alert.
        let mut alert = crate::ui::native::feedback::Alert::new(self.glyph, self.text);
        if let Some(c) = self.glyph_color {
            alert = alert.icon_color(c);
        }
        if let Some(d) = self.detail {
            alert = alert.description(d);
        }
        if let Some(a) = self.action {
            alert = alert.action(a);
        }
        alert
    }
}
