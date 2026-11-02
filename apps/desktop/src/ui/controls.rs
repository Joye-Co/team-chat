//! Buttons, icon buttons and fields at the spec sizes (DESIGN.md §4.2, §7.1,
//! §7.3, §7.4). Every control is a gpui-component control (`Button`,
//! `Input`); this module only gives it the v2.1 metrics and states, so a view
//! never sets a height, radius or padding on a control itself.

use gpui::{
    AnyElement, App, ElementId, Entity, Focusable as _, Hsla, InteractiveElement as _, IntoElement,
    ParentElement, Pixels, RenderOnce, SharedString, StatefulInteractiveElement as _, Styled,
    Window, div, prelude::FluentBuilder as _, px,
};
use gpui_component::{
    Disableable as _, Icon, Sizable as _,
    button::{Button, ButtonCustomVariant, ButtonVariants as _},
    input::{Input, InputState},
};

use crate::{
    theme::{self, Type as _, size, space, type_scale},
    ui::{
        chip::Kbd,
        icons::{Lucide, icon},
    },
};

/// Button sizes (§4.2).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ButtonSize {
    /// 28 pt, 13/500, padding 10, radius 6: inline in a message or card row.
    Sm,
    /// 32 pt, 14/500, padding 14, radius 8: the default.
    Md,
    /// 36 pt, 14/500, padding 14, radius 8: the page primary in settings.
    Page,
    /// 40 pt, 15/500, padding 18, radius 10: login, empty states, submit.
    Lg,
}

/// Button variants (§7.3).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ButtonKind {
    /// One per view or card: the main action.
    Primary,
    /// Alternatives: `content` fill with a 1 pt `border-strong`.
    Secondary,
    /// Low-emphasis actions and toolbars.
    Ghost,
    /// Destructive, in its own row or dialog.
    Danger,
}

/// Applies the v2.1 metrics to a gpui-component `Button`.
pub trait ButtonSpec: Sized {
    /// Height, padding, radius, text style and icon size of `size`.
    fn spec(self, size: ButtonSize) -> Self;
    /// Disables a filled button with the neutral disabled fill (§7.1): never
    /// `accent` at reduced opacity.
    fn disabled_fill(self, disabled: bool, cx: &App) -> Self;
}

impl ButtonSpec for Button {
    fn spec(self, button: ButtonSize) -> Self {
        // Icons stay gpui-component's 16 pt at `Size::Medium` (§4.2: 16 in
        // buttons; pass `Icon::new(..).size(..)` for the 14 pt of `sm`).
        let (h, pad, radius, text) = match button {
            ButtonSize::Sm => (
                size::BUTTON_SM,
                space::S10,
                theme::radius::SM,
                type_scale::SMALL,
            ),
            ButtonSize::Md => (
                size::BUTTON_MD,
                space::S14,
                theme::radius::MD,
                type_scale::UI,
            ),
            ButtonSize::Page => (
                size::BUTTON_PAGE,
                space::S14,
                theme::radius::MD,
                type_scale::UI,
            ),
            ButtonSize::Lg => (
                size::BUTTON_LG,
                px(18.),
                theme::radius::FIELD,
                type_scale::BUTTON_LG,
            ),
        };
        self.with_size(gpui_component::Size::Medium)
            .h(h)
            .px(pad)
            .rounded(radius)
            .text_size(text.0)
            .line_height(text.1)
            .medium()
    }

    fn disabled_fill(self, disabled: bool, cx: &App) -> Self {
        let k = theme::colors(cx);
        self.disabled(disabled).when(disabled, |b| {
            b.bg(k.disabled_bg)
                .text_color(k.disabled_text)
                .border_color(gpui::transparent_black())
        })
    }
}

/// A text button of `kind` at `size` (§7.3). Chain `.icon`, `.on_click`,
/// `.tooltip` as on any gpui-component button.
pub fn button(
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    kind: ButtonKind,
    size: ButtonSize,
    cx: &App,
) -> Button {
    let k = theme::colors(cx);
    let b = Button::new(id).label(label);
    let b = match kind {
        ButtonKind::Primary => b.primary(),
        ButtonKind::Secondary => b.border_1().border_color(k.border_strong),
        ButtonKind::Ghost => b.ghost(),
        ButtonKind::Danger => b.danger(),
    };
    b.spec(size)
}

/// The primary `md` button.
pub fn primary_button(
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    cx: &App,
) -> Button {
    button(id, label, ButtonKind::Primary, ButtonSize::Md, cx)
}

/// The secondary `md` button.
pub fn secondary_button(
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    cx: &App,
) -> Button {
    button(id, label, ButtonKind::Secondary, ButtonSize::Md, cx)
}

/// A ghost icon button: 32 x 32, radius 8, an 18 pt glyph in `text-2` that
/// turns `text-1` on hover (§4.2). Give it a tooltip: it is its name.
pub fn icon_button(id: impl Into<ElementId>, name: Lucide) -> Button {
    Button::new(id)
        .ghost()
        .icon(Icon::new(name).size(theme::icon::HEADER))
        .with_size(size::ICON_BUTTON)
        .rounded(theme::radius::MD)
}

/// The `sm` icon button: 28 x 28, radius 6, a 16 pt glyph (section `+`,
/// buttons inside rows).
pub fn icon_button_sm(id: impl Into<ElementId>, name: Lucide) -> Button {
    Button::new(id)
        .ghost()
        .icon(Icon::new(name).size(theme::icon::ROW))
        .with_size(size::ICON_BUTTON_SM)
        .rounded(theme::radius::SM)
}

/// A text button in `accent-text` (重试 after a failed send, 查看过程,
/// 更改): 22 pt, padding 0 6, radius 6, 13/20 500, no underline, hover
/// `hover`, pressed `pressed` (§7.5).
pub fn text_button(id: impl Into<ElementId>, label: impl Into<SharedString>, cx: &App) -> Button {
    let k = theme::colors(cx);
    Button::new(id)
        .custom(
            ButtonCustomVariant::new(cx)
                .foreground(k.accent_text)
                .hover(k.hover)
                .active(k.pressed),
        )
        .label(label)
        .with_size(gpui_component::Size::Small)
        .h(size::TEXT_BUTTON)
        .px(space::S6)
        .rounded(theme::radius::SM)
        .text_size(type_scale::SMALL.0)
        .line_height(type_scale::SMALL.1)
        .medium()
}

/// Field kinds (§7.4).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FieldKind {
    /// 36 pt, `field` fill, no border, radius 8, a leading `search` glyph.
    Search,
    /// 36 pt, `content` + 1 pt `border`, radius 8, a leading `search` glyph:
    /// on a page that also has selects or form fields.
    SearchBordered,
    /// 40 pt, `content` + 1 pt `border`, radius 10: login, settings, dialogs.
    Form,
}

/// A single-line text field around a gpui-component `Input`, with the spec
/// height, fill, edge and states: hover `border-strong`; focus a 1 pt
/// `focus-border` with a 3 pt `focus-soft` ring; error a `danger` border with
/// a `danger-bg` ring; disabled a `field` fill with no border.
#[derive(IntoElement)]
pub struct TextField {
    id: ElementId,
    state: Entity<InputState>,
    kind: FieldKind,
    error: bool,
    disabled: bool,
    trailing: Option<AnyElement>,
    shortcut: Option<SharedString>,
    width: Option<Pixels>,
}

impl TextField {
    pub fn new(id: impl Into<ElementId>, state: &Entity<InputState>, kind: FieldKind) -> Self {
        Self {
            id: id.into(),
            state: state.clone(),
            kind,
            error: false,
            disabled: false,
            trailing: None,
            shortcut: None,
            width: None,
        }
    }

    pub fn error(mut self, error: bool) -> Self {
        self.error = error;
        self
    }

    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    /// A trailing element (a clear `x`, a spinner).
    pub fn trailing(mut self, el: impl IntoElement) -> Self {
        self.trailing = Some(el.into_any_element());
        self
    }

    /// A trailing kbd hint (`⌘K` in the sidebar search), shown while empty.
    pub fn shortcut(mut self, label: impl Into<SharedString>) -> Self {
        self.shortcut = Some(label.into());
        self
    }

    pub fn width(mut self, width: Pixels) -> Self {
        self.width = Some(width);
        self
    }
}

impl RenderOnce for TextField {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let k = theme::colors(cx);
        let focused = self.state.read(cx).focus_handle(cx).is_focused(window);
        let empty = self.state.read(cx).value().is_empty();
        let (height, radius) = match self.kind {
            FieldKind::Search | FieldKind::SearchBordered => {
                (size::SEARCH_FIELD, theme::radius::MD)
            }
            FieldKind::Form => (size::FORM_FIELD, theme::radius::FIELD),
        };
        let search = self.kind != FieldKind::Form;
        let bordered = self.kind != FieldKind::Search;
        let (fill, edge): (Hsla, Option<Hsla>) = if self.disabled {
            (k.field, None)
        } else if self.error {
            (k.content, Some(k.danger))
        } else if focused {
            (
                if bordered { k.content } else { k.field },
                Some(k.focus_border),
            )
        } else if bordered {
            (k.content, Some(k.border))
        } else {
            (k.field, None)
        };
        let halo = if self.disabled {
            vec![]
        } else if self.error {
            k.error_halo()
        } else if focused {
            k.focus_halo()
        } else {
            vec![]
        };
        let hover_edge = bordered && !focused && !self.error && !self.disabled;
        div()
            .id(self.id)
            .flex_none()
            .map(|el| match self.width {
                Some(w) => el.w(w),
                None => el.w_full(),
            })
            .h(height)
            .rounded(radius)
            .bg(fill)
            .border_1()
            .border_color(edge.unwrap_or(gpui::transparent_black()))
            .shadow(halo)
            .when(hover_edge, |el| {
                el.hover(|s| s.border_color(k.border_strong))
            })
            .pl(space::S12)
            .pr(if search { space::S8 } else { space::S12 })
            .flex()
            .items_center()
            .gap(space::S8)
            .cursor_text()
            .when(search, |el| {
                el.child(icon(Lucide::Search, theme::icon::ROW, k.text_3))
            })
            .child(
                div().flex_1().min_w(px(0.)).child(
                    Input::new(&self.state)
                        .appearance(false)
                        .disabled(self.disabled)
                        .with_size(gpui_component::Size::Small)
                        .px_0()
                        .py_0()
                        .text_size(type_scale::UI.0)
                        .line_height(type_scale::UI.1)
                        .text_color(if self.disabled { k.text_3 } else { k.text_1 }),
                ),
            )
            .children(self.trailing)
            .when_some(self.shortcut.filter(|_| empty), |el, s| {
                el.child(Kbd::new(s))
            })
    }
}

/// The label above a form field: `small` 13/20 500 `text-2`, 8 pt above the
/// field (§7.4).
pub fn field_label(text: impl Into<SharedString>, cx: &App) -> gpui::Div {
    let k = theme::colors(cx);
    div()
        .pb(space::S8)
        .t_small()
        .medium()
        .text_color(k.text_2)
        .child(text.into())
}

/// Help under a form field: 12/16 `text-3`, 6 pt below. With `error`, the
/// message is `danger-text`.
pub fn field_help(text: impl Into<SharedString>, error: bool, cx: &App) -> gpui::Div {
    let k = theme::colors(cx);
    div()
        .pt(space::S6)
        .t_meta()
        .text_color(if error { k.danger_text } else { k.text_3 })
        .child(text.into())
}

/// A select trigger in a table row (§7.4): 36 x 120, `content` + 1 pt
/// `border`, radius 8, `ui` 14 text, a trailing `chevrons-up-down` 14.
/// Open the menu from its click handler.
pub fn select_trigger(
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    cx: &App,
) -> gpui::Stateful<gpui::Div> {
    let k = theme::colors(cx);
    div()
        .id(id)
        .flex_none()
        .h(size::SELECT)
        .w(size::SELECT_WIDTH)
        .pl(space::S12)
        .pr(space::S8)
        .rounded(theme::radius::MD)
        .bg(k.content)
        .border_1()
        .border_color(k.border)
        .hover(|s| s.border_color(k.border_strong))
        .active(|s| s.bg(k.hover))
        .cursor_pointer()
        .flex()
        .items_center()
        .gap(space::S6)
        .t_ui()
        .text_color(k.text_1)
        .child(div().flex_1().min_w(px(0.)).one_line().child(label.into()))
        .child(icon(Lucide::ChevronsUpDown, theme::icon::CHIP, k.text_3))
}
