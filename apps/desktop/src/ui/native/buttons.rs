//! The one Button (COMPLETENESS.md §1.8, §1.9, K03): a gpui-component
//! `Button` at the three control heights (sm 28, default 32, lg 36), radius
//! 8, 14 medium text, a 16 icon and gap 8, in the six variants of §1.8.
//! Icon buttons are the same Button, square (32, sm 28). Toolbar items are
//! 32 ghost icon buttons (D4). Views build every button from here.

use crate::ui::truncate::TruncText;
use gpui::{
    AnyElement, App, ElementId, Hsla, IntoElement, ParentElement, Pixels, SharedString, Styled,
    Window, div, prelude::FluentBuilder as _, px,
};
use gpui_component::{
    Disableable as _, Sizable as _,
    button::{Button, ButtonCustomVariant, ButtonVariants as _},
};

use super::tokens::{NativeColors, NativeType as _, icon, metrics, native_colors, shadow, space};
use crate::ui::{
    icons::{Sym, SymSize, button_sym},
    sf_symbols::{Sf, Weight, symbol},
};

/// The variant of a button (§1.8).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PushStyle {
    /// The primary action: `primary` fill, `primary-foreground` text. One per
    /// view; Return activates it.
    Default,
    /// `background` fill, a 1 px `input` border, shadow xs. Cancel is always
    /// this variant (D11).
    Outline,
    /// The `muted` fill, no border.
    Secondary,
    /// No fill at rest, `accent` on hover: toolbars and rows.
    Ghost,
    /// `destructive` fill, white text: the destructive confirm of an alert.
    Destructive,
    /// Foreground text, underlined on hover.
    Link,
}

/// The size of a button (§1.8, D3).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PushSize {
    /// h32, px 16.
    Regular,
    /// h28, px 12: form rows, table cells, card footers.
    Small,
    /// h36, px 24: login, a full-page empty state, composer Send.
    Large,
}

impl PushSize {
    pub fn height(self) -> Pixels {
        match self {
            PushSize::Regular => metrics::BUTTON_REGULAR,
            PushSize::Small => metrics::BUTTON_SMALL,
            PushSize::Large => metrics::BUTTON_LARGE,
        }
    }

    fn radius(self) -> Pixels {
        // §1.6: every button is radius 8.
        metrics::BUTTON_REGULAR_RADIUS
    }

    fn pad(self) -> Pixels {
        match self {
            PushSize::Regular => metrics::BUTTON_REGULAR_PAD,
            PushSize::Small => metrics::BUTTON_SMALL_PAD,
            PushSize::Large => metrics::BUTTON_LARGE_PAD,
        }
    }
}

/// The colors of a variant: rest fill, text, hover fill, pressed fill and
/// the border (if any).
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Look {
    pub fill: Hsla,
    pub text: Hsla,
    pub hover: Hsla,
    pub pressed: Hsla,
    pub border: Option<Hsla>,
}

pub(super) fn look(style: PushStyle, k: &NativeColors) -> Look {
    let clear = gpui::transparent_black();
    match style {
        PushStyle::Default => Look {
            fill: k.primary,
            text: k.on_primary,
            hover: k.primary.opacity(0.9),
            pressed: k.primary.opacity(0.8),
            border: None,
        },
        // A Mac push button: a soft tint, no frame (owner review: a border
        // on every 编辑… / 取消 is noise).
        PushStyle::Outline => Look {
            fill: k.push,
            text: k.label,
            hover: k.push_hover,
            pressed: k.push_pressed,
            border: None,
        },
        PushStyle::Secondary => Look {
            fill: k.fill,
            text: k.label,
            hover: k.fill.opacity(0.8),
            pressed: k.pressed,
            border: None,
        },
        PushStyle::Ghost => Look {
            fill: clear,
            text: k.label,
            hover: k.fill_hover,
            pressed: k.pressed,
            border: None,
        },
        // shadcn: `bg-destructive text-white`. One fill in both modes
        // (T1-15, COMPLETENESS.md §1.3): #dc2626, white text at 4.8:1. The
        // dark `destructive` token (#f87171) stays for icons and text.
        PushStyle::Destructive => {
            let fill = k.destructive_fill;
            Look {
                fill,
                text: gpui::white(),
                hover: fill.opacity(0.9),
                pressed: fill.opacity(0.8),
                border: None,
            }
        }
        PushStyle::Link => Look {
            fill: clear,
            text: k.label,
            hover: clear,
            pressed: clear,
            border: None,
        },
    }
}

fn variant(l: Look, cx: &App) -> ButtonCustomVariant {
    ButtonCustomVariant::new(cx)
        .color(l.fill)
        .foreground(l.text)
        .hover(l.hover)
        .active(l.pressed)
}

/// The look of a field-like control (a select): the input's fill, its 1 px
/// border and shadow xs, so a select lines up with the text fields beside
/// it.
fn field_look(k: &NativeColors) -> Look {
    Look {
        fill: k.control,
        text: k.label,
        hover: k.control_hover,
        pressed: k.pressed,
        border: Some(k.quaternary),
    }
}

/// Gives a button the frame of a text field ([`field_look`]).
fn styled_field(b: Button, cx: &App) -> Button {
    let k = native_colors(cx);
    let l = field_look(&k);
    b.custom(variant(l, cx))
        .bg(l.fill)
        .text_color(l.text)
        .border_1()
        .border_color(k.quaternary)
        .shadow(shadow::xs())
}

/// Gives a gpui-component button the look of `style`: fill, text, border and
/// shadow xs for the outline variant. The rest colors are set on the button
/// itself, so a disabled button keeps them and only fades to 50 %.
fn styled(b: Button, style: PushStyle, cx: &App) -> Button {
    let k = native_colors(cx);
    let l = look(style, &k);
    b.custom(variant(l, cx))
        .bg(l.fill)
        .text_color(l.text)
        .map(|b| match l.border {
            Some(edge) => b.border_1().border_color(edge).shadow(shadow::xs()),
            None => b.border_0(),
        })
}

/// A button (§1.8): `label` in 14 medium at `size`, radius 8. Chain
/// `.icon(..)`, `.on_click(..)`, `.loading(..)` as on any gpui-component
/// button; disable it with [`disabled_push`]. A title that opens more UI
/// ends with `…`.
pub fn push_button(
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    style: PushStyle,
    size: PushSize,
    cx: &App,
) -> Button {
    // An empty label is no label: an empty label slot plus the gap would
    // push a caller's own content row off its left edge (a select built on
    // a push button, review 2 R1-10).
    let label: SharedString = label.into();
    let b = Button::new(id)
        .when(!label.is_empty(), |b| b.label(label))
        .with_size(gpui_component::Size::Medium)
        .h(size.height())
        .px(size.pad())
        .rounded(size.radius())
        .n_headline();
    styled(b, style, cx)
}

/// A ghost button built without an `App` at hand (message extras): the
/// gpui-component ghost variant, which the theme maps to the `accent` hover,
/// at `size`, 14 medium.
pub fn ghost_button(
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    size: PushSize,
) -> Button {
    Button::new(id)
        .ghost()
        .label(label)
        .with_size(gpui_component::Size::Medium)
        .h(size.height())
        .px(size.pad())
        .rounded(size.radius())
        .n_headline()
}

/// A disabled button: the same variant at 50 % (§1.8, D11), so a disabled
/// primary never looks like Cancel.
pub fn disabled_push(b: Button, disabled: bool, _cx: &App) -> Button {
    b.disabled(disabled).when(disabled, |b| b.opacity(0.5))
}

/// The primary of a short form that is not complete yet (the login, §2.20):
/// inert, primary at 30 %, so it reads light and turns solid once the form
/// is filled. Use [`disabled_push`] for any other disabled button.
pub fn waiting_push(b: Button, waiting: bool) -> Button {
    b.disabled(waiting)
        .when(waiting, |b| b.opacity(metrics::PRIMARY_WAITING_OPACITY))
}

/// A loading button (§1.8): a 16 spinner in the icon slot, the label and
/// the width kept, inert while loading. Use it for any button that waits.
pub fn loading_push(b: Button, loading: bool, size: PushSize) -> Button {
    if !loading {
        return b;
    }
    b.icon(button_sym(
        Sym::Spinner,
        SymSize::for_slot(icon::MD),
        size.height(),
    ))
    .loading(true)
}

/// What an icon button draws: an app concept or an SF Symbol.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Glyph {
    Sym(Sym),
    Sf(Sf),
}

impl From<Sym> for Glyph {
    fn from(s: Sym) -> Self {
        Glyph::Sym(s)
    }
}

impl From<Sf> for Glyph {
    fn from(s: Sf) -> Self {
        Glyph::Sf(s)
    }
}

impl From<crate::ui::icons::Lucide> for Glyph {
    fn from(l: crate::ui::icons::Lucide) -> Self {
        Glyph::Sym(l.into())
    }
}

/// The size of an icon button (§1.8): 32, or sm 28. The icon is 16.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IconSize {
    Regular,
    Small,
}

impl IconSize {
    pub fn side(self) -> Pixels {
        match self {
            IconSize::Regular => metrics::BUTTON_REGULAR,
            IconSize::Small => metrics::BUTTON_SMALL,
        }
    }
}

fn glyph_el(glyph: Glyph, ink: Hsla, window: &Window) -> AnyElement {
    match glyph {
        Glyph::Sf(sf) => symbol(sf, icon::MD, Weight::Regular, ink, window),
        Glyph::Sym(s) => {
            crate::ui::icons::sym(s, SymSize::for_slot(icon::MD), ink).into_any_element()
        }
    }
}

/// A square icon button of `style` (ghost in toolbars and rows): 32 or 28,
/// radius 8, a 16 icon. `name` is its tooltip and what VoiceOver reads.
pub fn icon_button_styled(
    id: impl Into<ElementId>,
    glyph: impl Into<Glyph>,
    name: impl Into<SharedString>,
    style: PushStyle,
    size: IconSize,
    window: &Window,
    cx: &App,
) -> Button {
    let k = native_colors(cx);
    let name = name.into();
    let ink = look(style, &k).text;
    let side = size.side();
    let b = Button::new(id)
        .with_size(side)
        .size(side)
        .p_0()
        .rounded(metrics::BUTTON_REGULAR_RADIUS)
        .accessibility_label(name.clone())
        .tooltip(name);
    styled(b, style, cx).child(glyph_el(glyph.into(), ink, window))
}

/// A ghost icon button (§1.8): 32 or 28 square, radius 8, a 16 icon in
/// `muted-foreground`, `accent` on hover. Named: the name is its tooltip and
/// what VoiceOver reads.
pub fn icon_button(
    id: impl Into<ElementId>,
    glyph: impl Into<Glyph>,
    name: impl Into<SharedString>,
    size: IconSize,
    window: &Window,
    cx: &App,
) -> Button {
    let k = native_colors(cx);
    let name = name.into();
    let side = size.side();
    let b = Button::new(id)
        .with_size(side)
        .size(side)
        .p_0()
        .rounded(metrics::BUTTON_REGULAR_RADIUS)
        .accessibility_label(name.clone())
        .tooltip(name);
    styled(b, PushStyle::Ghost, cx).child(glyph_el(glyph.into(), k.secondary, window))
}

/// An icon button whose icon a gpui-component `Button` draws itself (no
/// window or `App` at hand): the same 32 / 28 ghost square on the
/// gpui-component ghost variant, which the theme maps to the `accent` hover.
/// Name it with [`crate::ui::NamedButton::named`].
pub fn icon_button_bare(id: impl Into<ElementId>, sym: impl Into<Sym>, size: IconSize) -> Button {
    let side = size.side();
    Button::new(id)
        .ghost()
        .icon(button_sym(sym.into(), SymSize::for_slot(icon::MD), side))
        .with_size(side)
        .size(side)
        .p_0()
        .rounded(metrics::BUTTON_REGULAR_RADIUS)
}

/// A ghost button variant for a gpui-component control that is not built
/// here (a dropdown trigger): no fill at rest, `accent` on hover.
pub fn borderless_variant(cx: &App) -> ButtonCustomVariant {
    variant(look(PushStyle::Ghost, &native_colors(cx)), cx)
}

/// A select / pop-up button (§1.9) for a form field: an outline button
/// whose title is the chosen value, a quiet 16 `chevron.down` at the end
/// (shadcn Select: the chevron in `tertiary`, so the value leads); the title
/// truncates with an ellipsis. Chain `.dropdown_menu(..)` to open the menu.
/// A choice inside a header, toolbar or sentence is an [`inline_popup`].
pub fn popup_button(
    id: impl Into<ElementId>,
    title: impl Into<SharedString>,
    size: PushSize,
    window: &Window,
    cx: &App,
) -> Button {
    chevron_button(id, title, Sf::ChevronDown, size, window, cx)
}

/// A pop-up that reads as part of the line it sits in (a pane header's
/// member, a toolbar's version, a palette's target): no frame and no fill
/// at rest, the title in `label`, a 12 `chevron.down` in `secondary` 4
/// after it, the ghost hover. h28, px 8, radius 8; the title truncates and
/// the chevron stays. Chain a text style (`.n_title3()`) to match the line,
/// then `.dropdown_menu(..)`.
pub fn inline_popup(
    id: impl Into<ElementId>,
    title: impl Into<SharedString>,
    window: &Window,
    cx: &App,
) -> Button {
    let k = native_colors(cx);
    let b = Button::new(id)
        .with_size(gpui_component::Size::Medium)
        .h(metrics::BUTTON_SMALL)
        .px(space::S8)
        .rounded(metrics::BUTTON_SMALL_RADIUS)
        .n_headline();
    styled(b, PushStyle::Ghost, cx).child(
        div()
            .min_w(px(0.))
            .flex()
            .items_center()
            .gap(space::S4)
            .child(TruncText::new(title).min_w(px(0.)))
            .child(div().flex_none().child(symbol(
                Sf::ChevronDown,
                icon::SM,
                Weight::Semibold,
                k.secondary,
                window,
            ))),
    )
}

/// A pull-down: an outline button with a trailing `chevron.down`; its title
/// is the action, not a value.
pub fn pull_down(
    id: impl Into<ElementId>,
    title: impl Into<SharedString>,
    size: PushSize,
    window: &Window,
    cx: &App,
) -> Button {
    chevron_button(id, title, Sf::ChevronDown, size, window, cx)
}

fn chevron_button(
    id: impl Into<ElementId>,
    title: impl Into<SharedString>,
    chevron: Sf,
    size: PushSize,
    window: &Window,
    cx: &App,
) -> Button {
    let k = native_colors(cx);
    let b = Button::new(id)
        .with_size(gpui_component::Size::Medium)
        .h(size.height())
        .pl(metrics::FIELD_PAD)
        .pr(space::S8)
        .rounded(size.radius())
        .n_body();
    // The row grows to fill the button (flex 1: the button's own label row
    // centers its children), so the title starts at x + 12 and the chevron
    // ends 8 before the right edge in every select of a column (§1.8, F4;
    // review 2 R1-10).
    styled_field(b, cx).child(
        div()
            .flex_1()
            .w_full()
            .min_w(px(0.))
            .flex()
            .items_center()
            .justify_between()
            .gap(space::S8)
            .child(TruncText::new(title).flex_1())
            .child(div().flex_none().child(symbol(
                chevron,
                metrics::POPUP_CHEVRON_SLOT,
                Weight::Regular,
                k.tertiary,
                window,
            ))),
    )
}

/// The joined `+` / `−` pair under a list: two 28 ghost icon buttons in one
/// outline frame.
pub fn square_pair(add: Button, remove: Button, cx: &App) -> gpui::Div {
    let k = native_colors(cx);
    div()
        .flex_none()
        .h(metrics::SQUARE_BUTTON)
        .flex()
        .items_center()
        .rounded(metrics::BUTTON_REGULAR_RADIUS)
        .border_1()
        .border_color(k.quaternary)
        .bg(k.control)
        .overflow_hidden()
        .child(add)
        .child(div().w(px(1.)).h(icon::MD).bg(k.separator))
        .child(remove)
}

/// One half of a [`square_pair`]: `plus` or `minus`.
pub fn square_button(
    id: impl Into<ElementId>,
    add: bool,
    name: impl Into<SharedString>,
    window: &Window,
    cx: &App,
) -> Button {
    let k = native_colors(cx);
    let name = name.into();
    let glyph: AnyElement = if add {
        symbol(Sf::Plus, icon::MD, Weight::Regular, k.label, window)
    } else {
        // The shared symbol table has no `minus` yet: the system font's minus
        // sign draws the same stroke.
        div()
            .n_body()
            .text_color(k.label)
            .child(crate::copy::native_kit::MINUS_SIGN)
            .into_any_element()
    };
    let b = Button::new(id)
        .with_size(metrics::SQUARE_BUTTON)
        .size(metrics::SQUARE_BUTTON)
        .p_0()
        .rounded(px(0.))
        .accessibility_label(name.clone())
        .tooltip(name);
    styled(b, PushStyle::Ghost, cx).child(glyph)
}

/// A ghost icon button in a row (the inspector's open arrow, row tools):
/// [`icon_button`] at 32.
pub fn borderless_button(
    id: impl Into<ElementId>,
    sf: Sf,
    name: impl Into<SharedString>,
    window: &Window,
    cx: &App,
) -> Button {
    icon_button(id, sf, name, IconSize::Regular, window, cx)
}

/// A ghost text button in a row (`编辑…`, `使用其他服务器`): sm, 14 medium.
pub fn borderless_text(
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    cx: &App,
) -> Button {
    push_button(id, label, PushStyle::Ghost, PushSize::Small, cx)
}

/// A select (§1.9) built without a window: an outline button whose title is
/// the chosen value, a quiet 16 `chevron.down` at the end, the title
/// truncating at the end; 160 wide unless the caller sets a width. Chain
/// `.dropdown_menu(..)` to open the menu.
pub fn select_button(
    id: impl Into<ElementId>,
    title: impl Into<SharedString>,
    size: PushSize,
    cx: &App,
) -> Button {
    let k = native_colors(cx);
    let b = Button::new(id)
        .with_size(gpui_component::Size::Medium)
        .h(size.height())
        .w(crate::ui::native::tokens::size::SELECT_WIDTH)
        .pl(metrics::FIELD_PAD)
        .pr(space::S8)
        .rounded(size.radius())
        .n_body();
    // The same row as [`popup_button`]: title at x + 12, chevron pinned 8
    // before the right edge.
    styled_field(b, cx).child(
        div()
            .flex_1()
            .w_full()
            .min_w(px(0.))
            .flex()
            .items_center()
            .justify_between()
            .gap(space::S8)
            .child(TruncText::new(title).flex_1())
            .child(div().flex_none().child(crate::ui::icons::icon(
                Sym::ChevronDown,
                icon::MD,
                k.tertiary,
            ))),
    )
}

/// A toggle chip (reactions, scope chips): h28, radius full, px 8. At rest
/// an outline chip; selected ("mine") the `muted` fill with a foreground
/// border (COMPLETENESS.md F30), never a state color.
pub fn chip_button(id: impl Into<ElementId>, selected: bool, cx: &App) -> Button {
    let k = native_colors(cx);
    let (fill, edge) = if selected {
        (k.fill, k.label)
    } else {
        (k.control, k.quaternary)
    };
    Button::new(id)
        .custom(
            ButtonCustomVariant::new(cx)
                .color(fill)
                .foreground(k.label)
                .hover(if selected {
                    k.fill_hover
                } else {
                    k.control_hover
                })
                .active(k.pressed),
        )
        .with_size(gpui_component::Size::Medium)
        .h(metrics::BUTTON_SMALL)
        .min_w(metrics::BUTTON_SMALL)
        .px(space::S8)
        .rounded_full()
        .bg(fill)
        .border_1()
        .border_color(edge)
        .text_color(k.label)
        .n_section()
}

/// A ghost button around custom content (an "add member" row, a Settings
/// navigation row): full width, radius 8, and the row hover of
/// [`super::FocusRow`], muted at 50 % (§2.6 v15, review 3 C1-07).
pub fn row_button(id: impl Into<ElementId>, cx: &App) -> Button {
    let k = native_colors(cx);
    row_button_with(id, k.fill.opacity(0.5), cx)
}

/// [`row_button`] inside a GroupBox (a Settings navigation row, a theme
/// tile). The group fill is `sidebar`, one level from `muted`, so muted at
/// 50 % does not show there (250 → 247 in light, review 4 K-13). The hover
/// is `pressed` at 50 %: 250 → 240 in light, 23 → 44 in dark, a step of
/// more than 8 levels; pressed stays the full `pressed`.
pub fn group_row_button(id: impl Into<ElementId>, cx: &App) -> Button {
    let k = native_colors(cx);
    row_button_with(id, k.row_hover, cx)
}

fn row_button_with(id: impl Into<ElementId>, hover: Hsla, cx: &App) -> Button {
    let k = native_colors(cx);
    let l = Look {
        hover,
        ..look(PushStyle::Ghost, &k)
    };
    Button::new(id)
        .with_size(gpui_component::Size::Medium)
        .w_full()
        .rounded(metrics::BUTTON_REGULAR_RADIUS)
        .n_body()
        .custom(variant(l, cx))
        .bg(l.fill)
        .text_color(l.text)
        .border_0()
}

/// A square tile in a picker grid (avatar grid): ghost at rest; selected
/// the `muted` fill with a foreground ring.
pub fn tile_button(id: impl Into<ElementId>, side: Pixels, selected: bool, cx: &App) -> Button {
    let k = native_colors(cx);
    let b = Button::new(id)
        .with_size(side)
        .size(side)
        .p_0()
        .rounded(metrics::BUTTON_REGULAR_RADIUS);
    styled(b, PushStyle::Ghost, cx)
        .when(selected, |b| b.bg(k.fill).border_1().border_color(k.label))
}

/// A tool on the dark media screen (lightbox, video): `ink` icon, white
/// 10 % hover; a 32 square, or a `round` 36 circle for the pager arrows.
pub fn screen_button(
    id: impl Into<ElementId>,
    sym: Sym,
    name: impl Into<SharedString>,
    round: bool,
    ink: Hsla,
    cx: &App,
) -> Button {
    let name = name.into();
    let side = if round {
        metrics::BUTTON_LARGE
    } else {
        metrics::BUTTON_REGULAR
    };
    let rest = if round {
        gpui::hsla(0., 0., 1., 0.10)
    } else {
        gpui::transparent_black()
    };
    Button::new(id)
        .custom(
            ButtonCustomVariant::new(cx)
                .color(rest)
                .foreground(ink)
                .hover(gpui::hsla(0., 0., 1., 0.18))
                .active(gpui::hsla(0., 0., 1., 0.24)),
        )
        .icon(button_sym(sym, SymSize::for_slot(icon::MD), side))
        .with_size(side)
        .size(side)
        .map(|b| {
            if round {
                b.rounded_full()
            } else {
                b.rounded(metrics::BUTTON_REGULAR_RADIUS)
            }
        })
        .accessibility_label(name.clone())
        .tooltip(name)
}

/// A tab of the Settings window's toolbar tab strip (Mac chrome, kept by
/// COMPLETENESS.md §2.19): an 18 symbol over a 12 label; the selected tab
/// has the `muted` fill. `active` is the key-window state.
pub fn toolbar_tab(
    id: impl Into<ElementId>,
    sym: Sym,
    label: impl Into<SharedString>,
    on: bool,
    active: bool,
    cx: &App,
) -> Button {
    let k = native_colors(cx);
    let label = label.into();
    let ink = match (on, active) {
        (true, true) => k.accent_text,
        (true, false) => k.label,
        (false, _) => k.secondary,
    };
    let rest = if on {
        k.selection
    } else {
        gpui::transparent_black()
    };
    Button::new(id)
        .custom(
            ButtonCustomVariant::new(cx)
                .color(rest)
                .foreground(ink)
                .hover(if on { rest } else { k.fill_hover })
                .active(if on { rest } else { k.fill }),
        )
        .h(px(56.))
        .min_w(px(60.))
        .px(space::S8)
        .rounded(metrics::BUTTON_REGULAR_RADIUS)
        .bg(rest)
        .accessibility_label(label.clone())
        .child(
            div()
                .flex()
                .flex_col()
                .items_center()
                .gap(space::S4)
                .child(crate::ui::icons::sym(sym, SymSize::TAB, ink))
                .child(
                    div()
                        .n_subheadline()
                        .text_color(ink)
                        .whitespace_nowrap()
                        .child(label),
                ),
        )
}

/// How a toolbar item draws (§2.2, D4).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ToolbarItemKind {
    /// A ghost icon button.
    Plain,
    /// A toggled item (its pane is open): the `muted` fill, foreground icon.
    Toggled,
    /// The one primary action of a window: the primary fill.
    Prominent,
}

/// A toolbar item (D4): a 32 square ghost icon button, radius 8, a 16 icon.
/// No glass circle and no drop shadow. Named: the name is its tooltip and
/// what VoiceOver reads.
pub fn toolbar_item(
    id: impl Into<ElementId>,
    sf: Sf,
    name: impl Into<SharedString>,
    kind: ToolbarItemKind,
    emphasized: bool,
    window: &Window,
    cx: &App,
) -> Button {
    let k = native_colors(cx);
    let name = name.into();
    let (style, ink) = match kind {
        ToolbarItemKind::Prominent => (PushStyle::Default, k.on_primary),
        ToolbarItemKind::Toggled => (PushStyle::Secondary, k.label),
        ToolbarItemKind::Plain if emphasized => (PushStyle::Ghost, k.label),
        ToolbarItemKind::Plain => (PushStyle::Ghost, k.secondary),
    };
    let b = Button::new(id)
        .with_size(metrics::TOOLBAR_ITEM)
        .size(metrics::TOOLBAR_ITEM)
        .p_0()
        .rounded(metrics::BUTTON_REGULAR_RADIUS)
        .accessibility_label(name.clone())
        .tooltip(name);
    styled(b, style, cx).child(symbol(sf, icon::MD, Weight::Regular, ink, window))
}

/// A toolbar item with its word (owner decision, 2026-11-08): h32, radius
/// 8, px 10, a 15 symbol and the label in 13 medium, gap 6. The item of the
/// open pane takes the `muted` fill, the rest are ghost. `name` is the
/// tooltip and the VoiceOver label.
#[allow(clippy::too_many_arguments)]
pub fn toolbar_labeled_item(
    id: impl Into<ElementId>,
    sf: Sf,
    label: impl Into<SharedString>,
    name: impl Into<SharedString>,
    toggled: bool,
    emphasized: bool,
    window: &Window,
    cx: &App,
) -> Button {
    let k = native_colors(cx);
    let name = name.into();
    let (style, ink) = match (toggled, emphasized) {
        (true, _) => (PushStyle::Secondary, k.label),
        (false, true) => (PushStyle::Ghost, k.secondary),
        (false, false) => (PushStyle::Ghost, k.secondary),
    };
    let b = Button::new(id)
        .with_size(metrics::TOOLBAR_ITEM)
        .h(metrics::TOOLBAR_ITEM)
        .px(px(10.))
        .rounded(metrics::BUTTON_REGULAR_RADIUS)
        .accessibility_label(name.clone())
        .tooltip(name);
    styled(b, style, cx).child(
        div()
            .flex()
            .items_center()
            .gap(px(6.))
            .child(symbol(sf, px(15.), Weight::Regular, ink, window))
            .child(
                div()
                    .text_size(px(13.))
                    .font_weight(gpui::FontWeight::MEDIUM)
                    .text_color(ink)
                    .whitespace_nowrap()
                    .child(label.into()),
            ),
    )
}

/// A toolbar item drawn with an app concept (no window at hand): the same
/// 32 ghost square as [`toolbar_item`]; `toggled` takes the `muted` fill.
pub fn toolbar_item_sym(
    id: impl Into<ElementId>,
    sym: Sym,
    name: impl Into<SharedString>,
    toggled: bool,
    emphasized: bool,
    cx: &App,
) -> Button {
    let k = native_colors(cx);
    let name = name.into();
    let (style, ink) = match (toggled, emphasized) {
        (true, _) => (PushStyle::Secondary, k.label),
        (false, true) => (PushStyle::Ghost, k.label),
        (false, false) => (PushStyle::Ghost, k.secondary),
    };
    let b = Button::new(id)
        .with_size(metrics::TOOLBAR_ITEM)
        .size(metrics::TOOLBAR_ITEM)
        .p_0()
        .rounded(metrics::BUTTON_REGULAR_RADIUS)
        .accessibility_label(name.clone())
        .tooltip(name);
    styled(b, style, cx).child(crate::ui::icons::sym(sym, SymSize::for_slot(icon::MD), ink))
}

/// A ghost icon button drawn with an app concept (no window at hand): 32 or
/// 28, radius 8, a 16 icon in `muted-foreground`. Named.
pub fn icon_button_sym(
    id: impl Into<ElementId>,
    sym: Sym,
    name: impl Into<SharedString>,
    size: IconSize,
    cx: &App,
) -> Button {
    let k = native_colors(cx);
    let name = name.into();
    let side = size.side();
    let b = Button::new(id)
        .with_size(side)
        .size(side)
        .p_0()
        .rounded(metrics::BUTTON_REGULAR_RADIUS)
        .accessibility_label(name.clone())
        .tooltip(name);
    styled(b, PushStyle::Ghost, cx).child(crate::ui::icons::sym(
        sym,
        SymSize::for_slot(icon::MD),
        k.secondary,
    ))
}

/// An outline icon button (§1.8, §2.7 table footer): `size` square, radius
/// 8, a 16 symbol in `label`, the outline border and fill.
pub fn icon_button_outline(
    id: impl Into<ElementId>,
    sym: Sym,
    name: impl Into<SharedString>,
    size: IconSize,
    cx: &App,
) -> Button {
    let k = native_colors(cx);
    let name = name.into();
    let side = size.side();
    let b = Button::new(id)
        .with_size(side)
        .size(side)
        .p_0()
        .rounded(metrics::BUTTON_REGULAR_RADIUS)
        .accessibility_label(name.clone())
        .tooltip(name);
    styled(b, PushStyle::Outline, cx).child(crate::ui::icons::sym(
        sym,
        SymSize::for_slot(icon::MD),
        k.label,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_variants_follow_the_table() {
        for k in [NativeColors::light(), NativeColors::dark()] {
            // Cancel (outline) is a tinted push button with no border; each
            // state is a deeper tint of the text color.
            let outline = look(PushStyle::Outline, &k);
            assert_eq!(outline.fill, k.push);
            assert!(outline.border.is_none());
            assert!(outline.hover.a > outline.fill.a);
            assert!(outline.pressed.a > outline.hover.a);
            // A select keeps the field frame; K-04: its hover is one visible
            // step over the rest fill (at least 8 levels of 255), below
            // pressed.
            let field = field_look(&k);
            assert_eq!(field.fill, k.control);
            assert_eq!(field.border, Some(k.quaternary));
            let lum = |c: gpui::Hsla| gpui::Rgba::from(c).r * 255.;
            let step = |c: gpui::Hsla| (lum(c) - lum(field.fill)).abs();
            assert!(step(field.hover) >= 8., "hover step {}", step(field.hover));
            assert!(step(field.pressed) > step(field.hover));
            let primary = look(PushStyle::Default, &k);
            assert_eq!(primary.fill, k.primary);
            assert!(primary.border.is_none());
            assert_ne!(primary.fill, outline.fill);
            // Destructive: red fill, white text.
            let red = look(PushStyle::Destructive, &k);
            assert_eq!(red.fill.h, k.red.h);
            assert_eq!(red.text, gpui::white());
            // Ghost: no fill at rest, accent on hover.
            let ghost = look(PushStyle::Ghost, &k);
            assert_eq!(ghost.fill.a, 0.);
            assert_eq!(ghost.hover, k.fill_hover);
            // Secondary: the muted fill.
            assert_eq!(look(PushStyle::Secondary, &k).fill, k.fill);
        }
    }
}

#[cfg(test)]
mod naming {
    /// Every icon-only button has an accessible name (§11): each
    /// `icon_button_bare(..)` chain calls `.named(..)`, never `.tooltip(..)`
    /// alone. (`icon_button` and `toolbar_item` take the name as an argument.)
    #[test]
    fn bare_icon_buttons_are_named() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let mut stack = vec![root];
        let mut unnamed = Vec::new();
        while let Some(dir) = stack.pop() {
            for entry in std::fs::read_dir(&dir).unwrap() {
                let path = entry.unwrap().path();
                if path.is_dir() {
                    stack.push(path);
                    continue;
                }
                if path.extension().is_none_or(|e| e != "rs") || path.ends_with("buttons.rs") {
                    continue;
                }
                let text = std::fs::read_to_string(&path).unwrap();
                for (ix, _) in text.match_indices("icon_button_bare(") {
                    let rest = &text[ix..];
                    // 500 bytes, cut back to a char boundary (CJK copy nearby).
                    let end = (0..=rest.len().min(500))
                        .rev()
                        .find(|&i| rest.is_char_boundary(i))
                        .unwrap_or(0);
                    let chain = &rest[..end];
                    let named = chain.find(".named(");
                    let tooltip = chain.find(".tooltip(");
                    // The hover toolbar builds its buttons in a closure and
                    // names them where they are used.
                    if named.is_none()
                        && !chain.contains("let tool")
                        && !chain.contains("{id}-{key}")
                    {
                        unnamed.push(format!("{}:{ix}", path.display()));
                    } else if let (Some(n), Some(t)) = (named, tooltip)
                        && t < n
                    {
                        unnamed.push(format!("{}:{ix}", path.display()));
                    }
                }
            }
        }
        assert!(
            unnamed.is_empty(),
            "icon buttons without a name: {unnamed:?}"
        );
    }
}
