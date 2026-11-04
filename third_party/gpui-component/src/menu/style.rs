//! A menu style the app can set (team-chat patch). Without it, menus keep
//! the gpui-component look. With it, every `PopupMenu` (dropdown and
//! context menus too) draws its surface, rows, separators and highlight
//! from these values.

use gpui::{App, BoxShadow, Global, Hsla, Pixels};

/// How every popup menu draws.
#[derive(Clone, Debug)]
pub struct MenuStyle {
    /// The surface fill (opaque, or a real blur behind it).
    pub fill: Hsla,
    /// The surface corner radius.
    pub radius: Pixels,
    /// The surface shadows (float shadow plus the glass edge).
    pub shadow: Vec<BoxShadow>,
    /// The inset of the rows from the surface edge.
    pub padding: Pixels,
    /// The height of one row; rows sit with no gap between them.
    pub row_height: Pixels,
    /// The corner radius of the highlighted row.
    pub row_radius: Pixels,
    /// The side padding inside a row.
    pub row_padding: Pixels,
    /// The row text size and line height.
    pub text_size: Pixels,
    pub line_height: Pixels,
    /// The highlighted row: fill and text.
    pub highlight: Hsla,
    pub highlight_text: Hsla,
    /// The separator line, its height and its space above and below.
    pub separator: Hsla,
    pub separator_height: Pixels,
    pub separator_margin: Pixels,
}

impl Global for MenuStyle {}

impl MenuStyle {
    /// Sets the style of every menu drawn from now on.
    pub fn set(self, cx: &mut App) {
        cx.set_global(self);
    }

    pub(crate) fn get(cx: &App) -> Option<&MenuStyle> {
        cx.try_global::<MenuStyle>()
    }
}
