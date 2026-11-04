//! v12/overlays: the overlay parts the kit does not have yet (NATIVE.md
//! §7.1, §7.2, §9.7).
//!
//! - [`native_alert`]: a gpui-component dialog configured as a Tahoe alert
//!   (the kit has [`super::AlertLayout`] for its inside, not the dialog).
//! - The Spotlight-style panel of ⌘K 搜索或交代 and the quick switcher:
//!   [`spotlight`] (600 wide), [`SpotlightField`] (a 48 pt capsule with
//!   22 pt text), [`spotlight_row`] (32 pt rows with a leading symbol or
//!   avatar), [`spotlight_section`] and [`spotlight_ink`].
//! - [`sheet_list`]: the bordered list inside a sheet (添加成员, 发起私聊),
//!   with [`sheet_list_row`] rows.
//! - [`app_icon`]: the app icon, as alerts and the login window show it.

use std::sync::{Arc, OnceLock};

use gpui::{
    AnyElement, App, ElementId, Entity, Hsla, Image, ImageFormat, InteractiveElement as _,
    IntoElement, ParentElement, Pixels, RenderOnce, Styled, Window, div, img,
    prelude::FluentBuilder as _, px,
};
use gpui_component::{
    Sizable as _,
    dialog::{Dialog, DialogEntrance},
    input::{Input, InputState},
};

use super::{
    sheet::SHEET_ENTRANCE,
    tokens::{NativeType as _, metrics, native_colors},
};
use crate::ui::{
    controls::placeholder_input,
    sf_symbols::{Sym, SymSize, Weight},
};

/// Sizes of the overlays (NATIVE.md §7.2, §9.7).
pub mod spot {
    use gpui::{Pixels, px};

    /// The Spotlight panel.
    pub const WIDTH: Pixels = px(600.);
    /// The capsule field: 48 pt with 22 pt text.
    pub const FIELD: Pixels = px(48.);
    pub const FIELD_TEXT: Pixels = px(22.);
    pub const FIELD_LINE: Pixels = px(28.);
    /// The search symbol in the field.
    pub const FIELD_SYMBOL: Pixels = px(20.);
    /// The panel's corner: half the field, so a panel with no results is a
    /// capsule.
    pub const RADIUS: Pixels = px(24.);
    /// A result row.
    pub const ROW: Pixels = px(32.);
    pub const ROW_RADIUS: Pixels = px(8.);
    /// The leading slot of a row (an inline symbol or a 20 pt avatar).
    pub const SLOT: Pixels = px(20.);
    pub const AVATAR: Pixels = px(20.);
    /// A section header above its rows.
    pub const SECTION: Pixels = px(26.);
    /// The inset of the rows from the panel edge.
    pub const INSET: Pixels = px(8.);
    /// The panel's distance from the window top (Spotlight sits in the
    /// upper third).
    pub const TOP: Pixels = px(120.);
    /// The bordered list in a sheet.
    pub const LIST_ROW: Pixels = px(32.);
    pub const LIST_RADIUS: Pixels = px(8.);
    /// The app icon of the login window.
    pub const LOGIN_ICON: Pixels = px(96.);
}

/// Configures a gpui-component dialog as a Tahoe alert (§7.2): 260 wide,
/// attached at the top center under the toolbar row like a sheet (a
/// window-modal alert), `sheet` fill, radius 22, the glass edge, no close
/// button; Escape cancels. Lay out its inside with [`super::AlertLayout`].
pub fn native_alert(dialog: Dialog, cx: &App) -> Dialog {
    let k = native_colors(cx);
    dialog
        .entrance(SHEET_ENTRANCE)
        .margin_top(metrics::TOOLBAR_ROW)
        .width(metrics::ALERT_WIDTH)
        .close_button(false)
        .keyboard(true)
        .overlay_closable(false)
        .bg(k.sheet)
        .border_color(k.glass_edge)
        .rounded(metrics::SHEET_RADIUS)
        .p(px(0.))
        .gap(px(0.))
}

/// Configures a gpui-component dialog as the Spotlight-style panel (§9.7):
/// 600 wide, in the upper third of the window, radius 24, `sheet` fill with
/// the glass edge and `float`. It opens at once (a palette opens many times
/// a day); a click outside or Escape closes it.
pub fn spotlight(dialog: Dialog, cx: &App) -> Dialog {
    let k = native_colors(cx);
    dialog
        .entrance(DialogEntrance::None)
        .margin_top(spot::TOP)
        .width(spot::WIDTH)
        .close_button(false)
        .keyboard(true)
        .overlay_closable(true)
        .bg(k.sheet)
        .border_color(k.glass_edge)
        .rounded(spot::RADIUS)
        .shadow(k.float())
        .p(px(0.))
        .gap(px(0.))
        .overflow_hidden()
}

/// The Spotlight field (§9.7): a 48 pt capsule row, a 20 pt
/// `magnifyingglass` in `secondary`, 22 pt text; an optional token before
/// the text (a scope) and an element at the trailing edge (a pop-up).
#[derive(IntoElement)]
pub struct SpotlightField {
    state: Entity<InputState>,
    leading: Option<AnyElement>,
    trailing: Option<AnyElement>,
}

impl SpotlightField {
    pub fn new(state: &Entity<InputState>) -> Self {
        Self {
            state: state.clone(),
            leading: None,
            trailing: None,
        }
    }

    /// A token right before the text (the 群 a search is scoped to).
    pub fn token(mut self, el: impl IntoElement) -> Self {
        self.leading = Some(el.into_any_element());
        self
    }

    pub fn trailing(mut self, el: impl IntoElement) -> Self {
        self.trailing = Some(el.into_any_element());
        self
    }
}

impl RenderOnce for SpotlightField {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let k = native_colors(cx);
        div()
            .flex_none()
            .w_full()
            .h(spot::FIELD)
            .pl(px(16.))
            .pr(px(12.))
            .flex()
            .items_center()
            .gap(px(10.))
            .cursor_text()
            .child(
                Sym::Search
                    .icon(SymSize::new(20., 24., Weight::Regular))
                    .text_color(k.secondary),
            )
            .children(self.leading)
            .child(
                div().flex_1().min_w(px(0.)).child(placeholder_input(
                    Input::new(&self.state)
                        .appearance(false)
                        .with_size(gpui_component::Size::Large)
                        .px_0()
                        .py_0()
                        .text_size(spot::FIELD_TEXT)
                        .line_height(spot::FIELD_LINE)
                        .text_color(k.label),
                    window,
                    cx,
                )),
            )
            .children(self.trailing)
    }
}

/// The hairline between the field and the results.
pub fn spotlight_divider(cx: &App) -> gpui::Div {
    div()
        .flex_none()
        .mx(spot::INSET)
        .h(px(1.))
        .bg(native_colors(cx).separator)
}

/// A section header in the results: Section style (11 pt semibold),
/// `secondary`, aligned with the row text.
pub fn spotlight_section(label: impl Into<gpui::SharedString>, cx: &App) -> gpui::Div {
    let k = native_colors(cx);
    div()
        .flex_none()
        .h(spot::SECTION)
        .px(spot::INSET + px(8.))
        .pt(px(8.))
        .flex()
        .items_center()
        .n_section()
        .text_color(k.secondary)
        .child(label.into())
}

/// The (text, secondary text) colors of a row: `onAccent` on the chosen
/// row, `label` and `secondary` otherwise.
pub fn spotlight_ink(active: bool, cx: &App) -> (Hsla, Hsla) {
    let k = native_colors(cx);
    if active {
        (k.on_accent, k.on_accent.opacity(0.72))
    } else {
        (k.label, k.secondary)
    }
}

/// A 32 pt result row (§9.7): inset 8 pt, radius 8; the chosen row has
/// the `accent` fill (a focused list). No hover fill: the pointer moves the
/// choice, as in Spotlight.
pub fn spotlight_row(
    id: impl Into<ElementId>,
    active: bool,
    cx: &App,
) -> gpui::Stateful<gpui::Div> {
    let k = native_colors(cx);
    let (ink, _) = spotlight_ink(active, cx);
    div()
        .id(id)
        .flex_none()
        .h(spot::ROW)
        .mx(spot::INSET)
        .px(px(8.))
        .rounded(spot::ROW_RADIUS)
        .flex()
        .items_center()
        .gap(px(8.))
        .n_body()
        .text_color(ink)
        .when(active, |d| d.bg(k.accent))
}

/// The leading slot of a row: an inline symbol (13 pt in a 20 pt slot).
pub fn spotlight_symbol(sym: Sym, color: Hsla) -> gpui::Div {
    div()
        .flex_none()
        .size(spot::SLOT)
        .flex()
        .items_center()
        .justify_center()
        .child(sym.icon(SymSize::INLINE).text_color(color))
}

/// The `return` mark at the trailing edge of the chosen row, after an
/// optional `⌘`.
pub fn return_mark(command: bool, color: Hsla) -> gpui::Div {
    div()
        .flex_none()
        .flex()
        .items_center()
        .gap(px(1.))
        .n_body()
        .text_color(color)
        .when(command, |d| {
            d.child(Sym::Shortcuts.icon(SymSize::INLINE_SMALL).text_color(color))
        })
        .child(Sym::Return.icon(SymSize::INLINE_SMALL).text_color(color))
}

/// The bordered list inside a sheet (a Mac table): `content` fill, 1 px
/// `separator` border, radius 8, `rows` tall; it scrolls inside.
pub fn sheet_list(rows: usize, cx: &App) -> gpui::Div {
    let k = native_colors(cx);
    div()
        .relative()
        .flex_none()
        .w_full()
        .h(spot::LIST_ROW * rows as f32 + px(10.))
        .rounded(spot::LIST_RADIUS)
        .bg(k.content)
        .border_1()
        .border_color(k.separator)
        .overflow_hidden()
}

/// A row of a [`sheet_list`]: 32 pt, inset 5 pt, radius 6; a chosen row
/// has the `accent` fill (`selection` when the list is not focused). No
/// hover fill.
pub fn sheet_list_row(
    id: impl Into<ElementId>,
    chosen: bool,
    focused: bool,
    cx: &App,
) -> gpui::Stateful<gpui::Div> {
    let k = native_colors(cx);
    let (fill, ink) = if focused {
        (k.accent, k.on_accent)
    } else {
        (k.selection, k.label)
    };
    div()
        .id(id)
        .flex_none()
        .h(spot::LIST_ROW)
        .mx(px(5.))
        .px(px(8.))
        .rounded(metrics::LIST_ROW_RADIUS)
        .flex()
        .items_center()
        .gap(px(8.))
        .n_body()
        .text_color(if chosen { ink } else { k.label })
        .when(chosen, |d| d.bg(fill))
}

/// The red error line under a sheet field (Subheadline, `red`).
pub fn field_error(text: impl Into<gpui::SharedString>, cx: &App) -> gpui::Div {
    div()
        .px(px(2.))
        .pt(px(6.))
        .n_subheadline()
        .text_color(native_colors(cx).red)
        .child(text.into())
}

fn app_icon_image() -> Arc<Image> {
    static ICON: OnceLock<Arc<Image>> = OnceLock::new();
    ICON.get_or_init(|| {
        Arc::new(Image::from_bytes(
            ImageFormat::Png,
            include_bytes!("../../../../../assets/brand/AppIcon.iconset/icon_128x128@2x.png")
                .to_vec(),
        ))
    })
    .clone()
}

/// The app icon (the real icon with its margin, as the system draws it in
/// alerts and the login window) at `size`.
pub fn app_icon(size: Pixels) -> gpui::Img {
    img(app_icon_image()).size(size).flex_none()
}

/// `text` with the characters in `ranges` (char indices) in Semibold: how
/// Spotlight shows what matched. No highlight fill.
pub fn weighted(text: &str, ranges: &[std::ops::Range<usize>]) -> gpui::StyledText {
    let starts: Vec<usize> = text
        .char_indices()
        .map(|(b, _)| b)
        .chain(std::iter::once(text.len()))
        .collect();
    let n = starts.len() - 1;
    gpui::StyledText::new(text.to_string()).with_highlights(ranges.iter().filter_map(|r| {
        let (a, b) = (r.start.min(n), r.end.min(n));
        (a < b).then(|| {
            (
                starts[a]..starts[b],
                gpui::HighlightStyle {
                    font_weight: Some(gpui::FontWeight::SEMIBOLD),
                    ..Default::default()
                },
            )
        })
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn weighted_maps_char_ranges_to_bytes() {
        // Must not panic on multi-byte text or ranges past the end.
        let _ = weighted("登录页支持", &[0..2, 4..9]);
        let _ = weighted("", &[0..1]);
    }
}

/// A 36 × 36 toolbar item for a concept the [`super::toolbar_item`] table
/// does not list (expand, collapse, close in a pane's toolbar row): a 16 pt
/// Medium symbol in `label`, `fillHover` on hover. Named: the name is its
/// tooltip and what VoiceOver reads.
pub fn sym_toolbar_item(
    id: impl Into<ElementId>,
    sym: Sym,
    name: impl Into<gpui::SharedString>,
    cx: &App,
) -> gpui_component::button::Button {
    use gpui_component::button::{Button, ButtonVariants as _};
    let k = native_colors(cx);
    let name = name.into();
    Button::new(id)
        .custom(super::borderless_variant(cx))
        .with_size(metrics::TOOLBAR_ITEM)
        .size(metrics::TOOLBAR_ITEM)
        .p_0()
        .rounded(metrics::TOOLBAR_ITEM / 2.)
        .accessibility_label(name.clone())
        .tooltip(name)
        .child(sym.icon(SymSize::TOOLBAR).text_color(k.label))
}

/// A toolbar item that turns a view on and off (开发者视图): while on, the
/// symbol sits in the 28 pt `selection` pill, like the inspector toggle
/// (§5.1 Toggled). The name is the tooltip and what VoiceOver reads.
pub fn sym_toolbar_toggle(
    id: impl Into<ElementId>,
    sym: Sym,
    name: impl Into<gpui::SharedString>,
    on: bool,
    cx: &App,
) -> gpui_component::button::Button {
    use gpui_component::button::{Button, ButtonVariants as _};
    let k = native_colors(cx);
    let name = name.into();
    let glyph = sym
        .icon(SymSize::TOOLBAR)
        .text_color(if on { k.accent_text } else { k.label });
    Button::new(id)
        .custom(super::borderless_variant(cx))
        .with_size(metrics::TOOLBAR_ITEM)
        .size(metrics::TOOLBAR_ITEM)
        .p_0()
        .rounded(metrics::TOOLBAR_ITEM / 2.)
        .accessibility_label(name.clone())
        .tooltip(name)
        .child(if on {
            div()
                .size(metrics::TOOLBAR_PILL)
                .rounded_full()
                .bg(k.selection)
                .flex()
                .items_center()
                .justify_center()
                .child(glyph)
                .into_any_element()
        } else {
            glyph.into_any_element()
        })
}

/// A bar on `fill`, radius 12 (§9.2 decision bar): the state symbol, a
/// Headline title and a `secondary` line, push buttons at the trailing edge.
pub fn decision_bar(cx: &App) -> gpui::Div {
    let k = native_colors(cx);
    div()
        .flex_none()
        .min_h(px(48.))
        .pl(px(14.))
        .pr(px(10.))
        .py(px(8.))
        .rounded(px(12.))
        .bg(k.fill)
        .flex()
        .items_center()
        .gap(px(10.))
}
