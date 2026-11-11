//! Dialogs, alerts and the switcher on gpui-component's `Dialog` (DESIGN.md
//! §7.12, §12): one overlay component with the `scrim`, the `overlay`
//! surface, radius 12 and the level-2 shadow.

use gpui::{App, Styled};
use gpui_component::dialog::{AlertDialog, Dialog, DialogEntrance};

use crate::theme::{self, size};

/// The `overlay` surface and its edge (§2.1, §5) on a gpui-component dialog,
/// which paints the page color (`content`) by default: in dark mode that
/// blends into the dimmed page.
fn overlay_fill<T: Styled>(dialog: T, cx: &App) -> T {
    // v12/kit: the `sheet` fill with the 0.5 pt glass edge (NATIVE.md §7).
    let n = crate::ui::native::native_colors(cx);
    dialog.bg(n.sheet).border_color(n.glass_edge)
}

/// Width of an alert.
pub const ALERT_WIDTH: gpui::Pixels = crate::ui::native::metrics::ALERT_WIDTH;
/// Width of the quick switcher.
pub const SWITCHER_WIDTH: gpui::Pixels = size::SWITCHER_WIDTH;
/// The switcher sits this far below the window top.
pub const SWITCHER_TOP: gpui::Pixels = size::PALETTE_TOP;

/// How a sheet enters (§12): it fades in place, on the theme's out curve
/// and its `normal` duration, enter and exit alike; gpui-component's dialog
/// has no scale and no separate exit time, and a fade without travel is the
/// closest it draws to `opacity + scale 0.98 → 1`. A sheet opened from a key
/// binding appears at once: a keyboard action never animates.
fn entrance(keyed: bool) -> DialogEntrance {
    if keyed {
        DialogEntrance::None
    } else {
        // v12/kit: a sheet slides down from under the toolbar while it fades
        // in (NATIVE.md §7.1).
        crate::ui::native::sheet::SHEET_ENTRANCE
    }
}

pub trait SheetExt: Sized {
    /// A centered dialog: 440 wide, padding 20, fading in over the scrim,
    /// on the `overlay` surface.
    fn sheet(self, cx: &App) -> Self;

    /// The sheet was opened from the keyboard (the last input when the
    /// opening action ran): no entrance and no exit animation.
    fn keyed(self, keyed: bool) -> Self;
}

impl SheetExt for Dialog {
    fn sheet(self, cx: &App) -> Self {
        // v12/kit (NATIVE.md §7.1): attached under the toolbar row, 460
        // wide, radius 22; the window behind dims (theme `overlay`).
        overlay_fill(self, cx)
            .entrance(entrance(false))
            .margin_top(crate::ui::native::window_chrome::sheet_top())
            .width(crate::ui::native::metrics::SHEET_WIDTH)
            .rounded(crate::ui::native::metrics::SHEET_RADIUS)
    }

    fn keyed(self, keyed: bool) -> Self {
        self.entrance(entrance(keyed))
    }
}

impl SheetExt for AlertDialog {
    fn sheet(self, cx: &App) -> Self {
        // v12/kit: the Tahoe alert (NATIVE.md §7.2): 260 wide, radius 22.
        overlay_fill(self, cx)
            .entrance(entrance(false))
            .centered()
            .width(ALERT_WIDTH)
            .rounded(crate::ui::native::metrics::SHEET_RADIUS)
    }

    fn keyed(self, keyed: bool) -> Self {
        self.entrance(entrance(keyed))
    }
}

/// The quick switcher and other keyboard palettes: no entrance (they open
/// tens of times a day), over the scrim, 96 pt from the window top (§7.12).
pub fn switcher(dialog: Dialog, cx: &App) -> Dialog {
    overlay_fill(dialog, cx)
        .entrance(DialogEntrance::None)
        .w(SWITCHER_WIDTH)
        .margin_top(SWITCHER_TOP)
        .rounded(theme::radius::LG)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sheets_slide_down_and_keyboard_sheets_do_not_animate() {
        assert_eq!(entrance(false), DialogEntrance::FadeSlide);
        assert_eq!(entrance(true), DialogEntrance::None);
    }
}
