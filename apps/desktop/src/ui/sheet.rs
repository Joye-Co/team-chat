//! Sheet, alert and switcher presentation for gpui-component dialogs
//! (DESIGN.md §5.5, §10, §13.12, §13.13).

use gpui::{Styled as _, px};
use gpui_component::dialog::{AlertDialog, Dialog, DialogEntrance};

use crate::theme;

/// Width of an alert.
pub const ALERT_WIDTH: gpui::Pixels = px(360.);
/// Width of the quick switcher.
pub const SWITCHER_WIDTH: gpui::Pixels = px(640.);
/// The switcher sits this far below the window top.
pub const SWITCHER_TOP: gpui::Pixels = px(132.);

pub trait SheetExt: Sized {
    /// A sheet from under the toolbar band: fade with a short settle (180 ms,
    /// out curve), 52 pt from the window top.
    fn sheet(self) -> Self;
}

impl SheetExt for Dialog {
    fn sheet(self) -> Self {
        self.entrance(DialogEntrance::FadeSlide)
            .margin_top(theme::TOOLBAR_HEIGHT)
            .rounded(theme::radius::XL)
    }
}

impl SheetExt for AlertDialog {
    fn sheet(self) -> Self {
        self.entrance(DialogEntrance::FadeSlide)
            .margin_top(theme::TOOLBAR_HEIGHT)
            .width(ALERT_WIDTH)
    }
}

/// The quick switcher and other keyboard palettes: no entrance, no scrim
/// (they open tens of times a day; §10).
pub fn switcher(dialog: Dialog) -> Dialog {
    dialog
        .entrance(DialogEntrance::None)
        .overlay(false)
        .w(SWITCHER_WIDTH)
        .margin_top(SWITCHER_TOP)
        .rounded(theme::radius::XL)
}
