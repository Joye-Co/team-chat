//! The slot for app-level banners (DESIGN.md §8.6): the update banner and
//! the connection banner sit above the sidebar footer as 48 pt cards, never
//! in the topic column, so the header lines of the columns stay aligned
//! (U17). The main window registers its banners; the sidebar draws the slot.

use gpui::{App, Entity, Global, IntoElement, ParentElement, Styled, div};

use crate::{
    theme::space, updater::banner::UpdateBanner, views::connection_banner::ConnectionBanner,
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
