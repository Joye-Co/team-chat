//! The slot at the bottom of the sidebar for the app's quiet status lines
//! (NATIVE.md §7.4: no banners): the connection state and an update that
//! runs while its sheet is hidden. The main window registers them; the
//! sidebar draws the slot.

use gpui::{App, Entity, Global, ParentElement, Styled, div};

use crate::{updater::banner::UpdateBanner, views::connection_banner::ConnectionBanner};

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

/// The slot: each line draws itself while it has something to show, and
/// nothing otherwise, so the slot follows them without the sidebar
/// observing them.
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
