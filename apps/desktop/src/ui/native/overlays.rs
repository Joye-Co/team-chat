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
    tokens::{NativeType as _, metrics, native_colors, space},
};
use crate::theme::MEDIUM_WEIGHT;
use crate::ui::{
    controls::placeholder_input,
    sf_symbols::{Sym, SymSize, Weight},
};

/// Sizes of the overlays: the command palette of COMPLETENESS.md §2.11, on
/// the scales of ui/native/tokens.rs.
pub mod spot {
    use crate::ui::native::tokens::{avatar, height, icon, radius, space, text};
    use gpui::{Pixels, px};

    /// The command palette.
    pub const WIDTH: Pixels = px(600.);
    /// The input row: h48 with 16 text.
    pub const FIELD: Pixels = height::PALETTE_INPUT;
    pub const FIELD_TEXT: Pixels = text::HEADING.0;
    pub const FIELD_LINE: Pixels = text::HEADING.1;
    /// The search symbol in the field.
    pub const FIELD_SYMBOL: Pixels = icon::MD;
    /// The panel's corner.
    pub const RADIUS: Pixels = radius::CONTAINER;
    /// A result row.
    pub const ROW: Pixels = height::ROW;
    pub const ROW_RADIUS: Pixels = radius::ITEM;
    /// The leading slot of a row (an inline symbol or a 16 avatar).
    pub const SLOT: Pixels = icon::MD;
    pub const AVATAR: Pixels = avatar::XS;
    /// A group label above its rows.
    pub const SECTION: Pixels = height::ROW;
    /// The inset of the rows from the panel edge (list p 4).
    pub const INSET: Pixels = space::S4;
    /// The panel's distance from the window top (Spotlight sits in the
    /// upper third).
    pub const TOP: Pixels = px(120.);
    /// The bordered list in a sheet (list box radius 10).
    pub const LIST_ROW: Pixels = height::ROW;
    pub const LIST_RADIUS: Pixels = radius::CONTAINER;
    /// The logo of the login card (§2.20).
    pub const LOGIN_ICON: Pixels = px(48.);
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
        .border_1()
        .border_color(k.separator)
        .shadow(super::tokens::shadow::lg())
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

/// The palette field (COMPLETENESS.md §2.11): an h48 row, a 16 search icon
/// in `secondary`, 16 text, gap 8; an optional token before the text (a
/// scope) and an element at the trailing edge (a pop-up).
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
            .gap(space::S8)
            .cursor_text()
            .child(
                Sym::Search
                    .icon(SymSize::new(16., 16., Weight::Regular))
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

/// The (text, secondary text) colors of a row. The chosen row is the
/// `accent` (muted) fill, so its text stays `label` and `secondary` (D10):
/// never an inverted row.
pub fn spotlight_ink(active: bool, cx: &App) -> (Hsla, Hsla) {
    let _ = active;
    let k = native_colors(cx);
    (k.label, k.secondary)
}

/// A 32 result row (§2.11): inset 4, px 8, radius 6; the chosen row has the
/// `accent` fill (muted, D10) and 500 text. No hover fill: the pointer moves
/// the choice, as in Spotlight.
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
        .when(active, |d| d.bg(k.accent).font_weight(MEDIUM_WEIGHT))
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

/// A row of a [`sheet_list`]: 32, inset 5, radius 6; a chosen row has the
/// muted `selection` fill and 500 text, focused or not (D10). No hover
/// fill.
pub fn sheet_list_row(
    id: impl Into<ElementId>,
    chosen: bool,
    focused: bool,
    cx: &App,
) -> gpui::Stateful<gpui::Div> {
    let _ = focused;
    let k = native_colors(cx);
    let (fill, ink) = (k.selection, k.label);
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
        .when(chosen, |d| d.bg(fill).font_weight(MEDIUM_WEIGHT))
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
    // D4: the 32 ghost square of `toolbar_item`.
    super::toolbar_item_sym(id, sym, name, false, true, cx)
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
    // D4: a 32 ghost square; while on, the `muted` fill.
    super::toolbar_item_sym(id, sym, name, on, true, cx)
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
