//! Dialogs, alerts and the switcher on gpui-component's `Dialog` (DESIGN.md
//! §7.12, §12): one overlay component with the `scrim`, the `overlay`
//! surface, radius 12 and the level-2 shadow.

use gpui::Styled as _;
use gpui_component::dialog::{AlertDialog, Dialog, DialogEntrance};

use crate::theme::{self, size};

/// Width of an alert.
pub const ALERT_WIDTH: gpui::Pixels = size::ALERT_WIDTH;
/// Width of the quick switcher.
pub const SWITCHER_WIDTH: gpui::Pixels = size::SWITCHER_WIDTH;
/// The switcher sits this far below the window top.
pub const SWITCHER_TOP: gpui::Pixels = size::PALETTE_TOP;

pub trait SheetExt: Sized {
    /// A centered dialog: 440 wide, padding 20, fade with a short settle
    /// (200 ms, out curve) over the scrim.
    fn sheet(self) -> Self;
}

impl SheetExt for Dialog {
    fn sheet(self) -> Self {
        self.entrance(DialogEntrance::FadeSlide)
            .centered()
            .width(size::DIALOG_WIDTH)
            .rounded(theme::radius::LG)
    }
}

impl SheetExt for AlertDialog {
    fn sheet(self) -> Self {
        self.entrance(DialogEntrance::FadeSlide)
            .centered()
            .width(ALERT_WIDTH)
    }
}

/// The quick switcher and other keyboard palettes: no entrance (they open
/// tens of times a day), over the scrim, 96 pt from the window top (§7.12).
pub fn switcher(dialog: Dialog) -> Dialog {
    dialog
        .entrance(DialogEntrance::None)
        .w(SWITCHER_WIDTH)
        .margin_top(SWITCHER_TOP)
        .rounded(theme::radius::LG)
}
