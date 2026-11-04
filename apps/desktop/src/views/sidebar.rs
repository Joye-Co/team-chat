//! Helpers of the v1 sidebar that other views still use: menu rows, scroll
//! edge fades and 话题 titles. The v1 sidebar view itself is gone (v12/shell):
//! the main window's sidebar is `views/rail` in the shell's floating panel.

use gpui::{
    Context, Div, Hsla, ParentElement, ScrollHandle, Styled, Window, div,
    prelude::FluentBuilder as _, px,
};
use gpui_component::menu::PopupMenuItem;

use crate::{
    copy::nav,
    theme::{self, Type as _, size, space},
    ui::{self, Sym},
};

/// At most this many 待办 rows show in the sidebar (§7.7).
const TODO_ROWS: usize = 5;

/// Width of a context menu.
pub const MENU_WIDTH: gpui::Pixels = px(220.);

/// The sections that collapse.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum Section {
    Todo,
    Groups,
    Dms,
}

/// A menu row (§7.12): 32 pt, a 16 pt glyph in `text-2` (or `danger-text`
/// for a destructive item), 8 pt gap, `ui` 14.
pub fn menu_item(glyph: Sym, label: &'static str, danger: bool) -> PopupMenuItem {
    PopupMenuItem::element(move |_, cx| {
        let k = theme::colors(cx);
        let (glyph_color, text) = if danger {
            (k.danger_text, k.danger_text)
        } else {
            (k.text_2, k.text_1)
        };
        div()
            .h(size::MENU_ROW)
            .flex()
            .items_center()
            .gap(space::S8)
            .t_ui()
            .text_color(text)
            .child(ui::icon(glyph, theme::icon::ROW, glyph_color))
            .child(label)
    })
}

/// Which edges of a scroll area have content beyond them: (top, bottom).
pub fn scroll_edges(handle: &ScrollHandle) -> (bool, bool) {
    let y = handle.offset().y;
    let max = handle.max_offset().y;
    (y < px(-0.5), max > px(0.5) && y > -max + px(0.5))
}

/// A 24 pt fade from `color` at the top or bottom edge of a scroll area
/// (§7.7 scroll edges), so a row cut by the edge never shows as a hard
/// sliver. Place it in a relative parent over the scroll area.
pub fn edge_fade(top: bool, color: Hsla) -> Div {
    // A 24 pt fade (§7.7): 8 pt of solid `color` at the edge, so a row cut
    // by the edge never peeks out (badges included), then an eased 16 pt
    // ramp 1.0, 0.85, 0.5, 0.15, 0 in 4 pt linear steps (a gradient has two
    // stops).
    const STOPS: [f32; 5] = [1.0, 0.85, 0.5, 0.15, 0.0];
    let el = div()
        .absolute()
        .left_0()
        .right_0()
        .h(space::S24)
        .flex()
        .when(top, |el| el.top_0().flex_col())
        .when(!top, |el| el.bottom_0().flex_col_reverse())
        .child(div().flex_none().h(space::S8).bg(color));
    el.children(STOPS.windows(2).map(|w| {
        div().flex_none().h(space::S4).bg(gpui::linear_gradient(
            if top { 180. } else { 0. },
            gpui::linear_color_stop(color.opacity(w[0]), 0.),
            gpui::linear_color_stop(color.opacity(w[1]), 1.),
        ))
    }))
}

/// Re-renders `view` on the next frame when the scroll edges in `seen` are
/// out of date (the handle learns its sizes only at layout).
pub fn watch_scroll_edges<V: 'static>(
    handle: &ScrollHandle,
    seen: (bool, bool),
    window: &mut Window,
    cx: &mut Context<V>,
) {
    let handle = handle.clone();
    let view = cx.entity().downgrade();
    window.on_next_frame(move |_, cx| {
        if scroll_edges(&handle) != seen {
            view.update(cx, |_, cx| cx.notify()).ok();
        }
    });
}

/// The first line of a title; newlines become spaces.
pub fn topic_line(title: &str) -> String {
    title.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// The title as every surface shows it (§15): its first line, or
/// `（无标题）` when that is empty. The flag is `true` for the fallback,
/// which renders in `text-2`.
pub fn topic_display_title(title: &str) -> (String, bool) {
    let line = topic_line(title);
    if line.is_empty() {
        (nav::UNTITLED.to_string(), true)
    } else {
        (line, false)
    }
}
