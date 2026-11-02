//! The slot for app-level banners (DESIGN.md §8.6): the update banner and
//! the connection banner sit above the sidebar footer as 48 pt cards, never
//! in the topic column, so the header lines of the columns stay aligned
//! (U17). The main window registers its banners; the sidebar draws the slot.

use gpui::{
    AnyElement, App, Entity, Global, InteractiveElement as _, IntoElement, ParentElement,
    SharedString, StatefulInteractiveElement as _, Styled, div, prelude::FluentBuilder as _, px,
};
use gpui_component::tooltip::Tooltip;

use crate::{
    theme::{self, Type as _, space},
    updater::banner::UpdateBanner,
    views::connection_banner::ConnectionBanner,
};

/// The banners of the open main window.
pub struct AppBanners {
    pub update: Entity<UpdateBanner>,
    pub connection: Entity<ConnectionBanner>,
}

impl Global for AppBanners {}

/// Registers the banners the slot draws.
pub fn register(update: Entity<UpdateBanner>, connection: Entity<ConnectionBanner>, cx: &mut App) {
    cx.set_global(AppBanners { update, connection });
}

/// The slot: each banner draws itself with the sidebar's 12 pt margins and
/// 8 pt under it while it has something to show, and nothing otherwise, so
/// the slot follows the banners without the sidebar observing them.
pub fn slot(cx: &App) -> Option<gpui::Div> {
    let banners = cx.try_global::<AppBanners>()?;
    Some(
        div()
            .flex_none()
            .flex()
            .flex_col()
            .child(banners.connection.clone())
            .child(banners.update.clone()),
    )
}

/// The margins of one banner in the slot.
pub fn place(card: impl IntoElement) -> gpui::Div {
    div().flex_none().px(space::S12).pb(space::S8).child(card)
}

/// One app-level banner card (§8.6): a 52 pt card, radius 8, `raised` with a
/// 1 pt `border`, padding 8 8 8 12. A 16 pt glyph, then the title (13/20 500
/// `text-1`) over the detail (12/16 `text-3`), each one line with `…`, then
/// the `sm` actions. The tooltip carries the whole sentence when a line can be
/// cut. Both banners use it, so they share one size and rhythm.
pub struct Card {
    pub id: &'static str,
    pub glyph: AnyElement,
    pub title: SharedString,
    /// The second line: text, optionally followed by a bar.
    pub detail: Option<AnyElement>,
    pub actions: Vec<AnyElement>,
    pub tooltip: Option<SharedString>,
}

impl Card {
    pub fn render(self, cx: &App) -> gpui::Div {
        let k = theme::colors(cx);
        let tooltip = self.tooltip;
        place(
            div()
                .id(self.id)
                .flex_none()
                .w_full()
                .min_h(theme::size::BANNER)
                .pl(space::S12)
                .pr(space::S8)
                .py(space::S8)
                .rounded(theme::radius::MD)
                .bg(k.raised)
                .border_1()
                .border_color(k.border)
                .flex()
                .items_center()
                .gap(space::S10)
                .when_some(tooltip, |el, text| {
                    el.tooltip(move |window, cx| Tooltip::new(text.clone()).build(window, cx))
                })
                .child(div().flex_none().flex().items_center().child(self.glyph))
                .child(
                    div()
                        .flex_1()
                        .min_w(px(0.))
                        .flex()
                        .flex_col()
                        .child(
                            div()
                                .t_small()
                                .medium()
                                .tnum()
                                .text_color(k.text_1)
                                .one_line()
                                .child(self.title),
                        )
                        .children(self.detail.map(|d| {
                            div()
                                .t_meta()
                                .tnum()
                                .text_color(k.text_3)
                                .min_w(px(0.))
                                .child(d)
                        })),
                )
                .when(!self.actions.is_empty(), |el| {
                    el.child(
                        div()
                            .flex_none()
                            .flex()
                            .items_center()
                            .gap(space::S4)
                            .children(self.actions),
                    )
                }),
        )
    }
}

/// A one-line detail text with `…`.
pub fn detail_text(text: impl Into<SharedString>) -> AnyElement {
    div()
        .min_w(px(0.))
        .one_line()
        .child(text.into())
        .into_any_element()
}
