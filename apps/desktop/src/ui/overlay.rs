//! Floating surfaces (DESIGN.md §5, §7.12, §8.6): popovers, menus and their
//! rows, the @mention popup, the hover toolbar, toasts and banners.

use gpui::{
    AnyElement, App, ElementId, IntoElement, ParentElement, RenderOnce, SharedString, Styled,
    Window, div, prelude::FluentBuilder as _, px,
};

use crate::{
    theme::{self, Elevation, Type as _, size, space},
    ui::{
        icons::{Sym, icon},
        rows::{RowSurface, list_row},
    },
};

/// A level-2 overlay surface: `overlay` fill, radius 12, the overlay shadow
/// (a visible 1 pt edge in dark mode). Popovers, menus, the switcher,
/// dialogs and toasts use it.
pub fn overlay_surface(cx: &App) -> gpui::Div {
    // v12/kit (NATIVE.md §7.3): `sheet` fill, radius 12, the glass edge and
    // the `float` shadow.
    let n = crate::ui::native::native_colors(cx);
    div()
        .bg(n.sheet)
        .text_color(n.label)
        .rounded(crate::ui::native::metrics::MENU_RADIUS)
        .shadow(n.glass())
}

/// A menu or popover: the overlay surface with padding 4.
pub fn menu_surface(cx: &App) -> gpui::Div {
    // v12/kit: menus at 92 % with 5 pt inner padding (NATIVE.md §7.3).
    let n = crate::ui::native::native_colors(cx);
    overlay_surface(cx)
        .bg(n.menu)
        .p(crate::ui::native::metrics::MENU_PAD)
        .flex()
        .flex_col()
}

/// The @mention popup (§7.2): 320 wide, radius 10, padding 4, max 240 high.
pub fn mention_surface(cx: &App) -> gpui::Div {
    overlay_surface(cx)
        .w(size::MENTION_POPUP_WIDTH)
        .max_h(px(240.))
        .rounded(theme::radius::FIELD)
        .p(space::S4)
        .flex()
        .flex_col()
}

/// A level-1 float (hover toolbar, jump pill, selected segment): `overlay`
/// fill, radius 8, the float shadow, padding 2.
pub fn float_surface(cx: &App) -> gpui::Div {
    let k = theme::colors(cx);
    div()
        .bg(k.overlay)
        .rounded(theme::radius::MD)
        .shadow(k.shadow(Elevation::Float))
        .p(space::S2)
        .flex()
        .items_center()
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
    let k = theme::colors(cx);
    // v12/kit: 24 pt rows, radius 6, a 13 pt leading symbol (NATIVE.md
    // §7.3); the active row is the inset highlight.
    let _ = k;
    let n = crate::ui::native::native_colors(cx);
    list_row(id, RowSurface::Overlay, active, cx)
        .h(size::MENU_ROW)
        .px(crate::ui::native::metrics::MENU_ROW_PAD)
        .gap(space::S6)
        .rounded(crate::ui::native::metrics::MENU_ROW_RADIUS)
        .t_ui()
        .text_color(n.label)
        .when_some(glyph, |el, g| el.child(icon(g, px(13.), n.secondary)))
        .child(div().flex_1().min_w(px(0.)).one_line().child(label.into()))
}

/// A 0.5 pt separator between menu groups, with 4 pt margins.
pub fn menu_separator(cx: &App) -> gpui::Div {
    // v12/kit: a 1 px `separator` inset to the text column.
    div()
        .my(space::S4)
        .mx(crate::ui::native::metrics::MENU_ROW_PAD)
        .h(px(1.))
        .bg(crate::ui::native::native_colors(cx).separator)
}

/// A popup row (@mention popup, switcher rows use their own height): 40 pt,
/// radius 6, padding 0 10, gap 10; the active row shows a trailing
/// `corner-down-left` 14.
pub fn popup_row(id: impl Into<ElementId>, active: bool, cx: &App) -> gpui::Stateful<gpui::Div> {
    let k = theme::colors(cx);
    list_row(id, RowSurface::Overlay, active, cx)
        .h(size::POPUP_ROW)
        .px(space::S10)
        .gap(space::S10)
        .t_ui()
        .text_color(k.text_1)
}

/// The return glyph at the end of the active popup row.
pub fn return_hint(cx: &App) -> impl IntoElement {
    icon(
        Sym::Return,
        theme::icon::CHIP,
        theme::colors(cx).text_3,
    )
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
        let k = theme::colors(cx);
        let glyph_color = match self.tone {
            Tone::Neutral => k.text_2,
            Tone::Success => k.success,
            Tone::Danger => k.danger,
        };
        overlay_surface(cx)
            .h(size::TOAST)
            .max_w(px(420.))
            .pl(space::S14)
            .pr(space::S12)
            .flex()
            .items_center()
            .gap(space::S10)
            .child(icon(self.glyph, theme::icon::ROW, glyph_color))
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.))
                    .t_ui()
                    .text_color(k.text_1)
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
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let k = theme::colors(cx);
        div()
            .flex_none()
            .w_full()
            .min_h(size::BANNER)
            .px(space::S12)
            .py(space::S6)
            // v12/kit: a quiet `fill` row, no border, no card.
            .rounded(theme::radius::MD)
            .bg(k.field)
            .flex()
            .items_center()
            .gap(space::S10)
            .child(icon(
                self.glyph,
                theme::icon::ROW,
                self.glyph_color.unwrap_or(k.text_2),
            ))
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.))
                    .flex()
                    .flex_col()
                    .child(
                        div()
                            .t_small()
                            .text_color(k.text_1)
                            .one_line()
                            .child(self.text),
                    )
                    .when_some(self.detail, |el, d| {
                        el.child(div().t_meta().text_color(k.text_3).one_line().child(d))
                    }),
            )
            .children(self.action)
    }
}
