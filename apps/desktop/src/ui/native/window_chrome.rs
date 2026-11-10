//! The window's caption off the Mac (docs/plans/windows.zh.md, C1).
//!
//! On macOS the system draws the traffic lights inside the window and the
//! toolbar row is the title bar, so [`framed`] returns the content as it is.
//! Windows draws nothing in a window with a hidden title bar: [`framed`] puts
//! a caption strip above the content, built from gpui-component's `TitleBar`.
//! The strip is the drag region (`WindowControlArea::Drag`: the system moves
//! the window, maximizes it on a double click and snaps it), and its
//! minimize, maximize and close buttons are window-control areas too, so
//! Windows 11 shows Snap Layouts over the maximize button. GPUI insets a
//! maximized window by the frame thickness itself.

use gpui::{
    AnyElement, App, IntoElement, ParentElement as _, SharedString, Styled as _, Window, div,
};
use gpui_component::TitleBar;

use super::tokens::{NativeType as _, emphasized, metrics, native_colors, space};

/// Whether this platform needs the app to draw the caption.
pub const fn drawn() -> bool {
    cfg!(not(target_os = "macos"))
}

/// `content` under the window's caption strip; `content` alone on macOS and
/// in full screen.
pub fn framed(
    title: impl Into<SharedString>,
    content: impl IntoElement,
    window: &mut Window,
    cx: &mut App,
) -> AnyElement {
    frame(title.into(), true, content, window, cx)
}

/// [`framed`] for a window that cannot be resized (login, 设置): its maximize
/// button is drawn dimmed and does nothing, as the Mac dims the zoom button.
pub fn framed_fixed(
    title: impl Into<SharedString>,
    content: impl IntoElement,
    window: &mut Window,
    cx: &mut App,
) -> AnyElement {
    frame(title.into(), false, content, window, cx)
}

fn frame(
    title: SharedString,
    resizable: bool,
    content: impl IntoElement,
    window: &mut Window,
    cx: &mut App,
) -> AnyElement {
    if !drawn() || window.is_fullscreen() {
        return content.into_any_element();
    }
    let k = native_colors(cx);
    div()
        .size_full()
        .flex()
        .flex_col()
        .bg(k.content)
        .child(caption(title, resizable, window, cx))
        .child(div().flex_1().min_h_0().w_full().relative().child(content))
        .into_any_element()
}

/// The caption strip: the window's title at the leading edge, the window
/// buttons at the trailing edge.
fn caption(
    title: SharedString,
    resizable: bool,
    window: &mut Window,
    cx: &mut App,
) -> impl IntoElement {
    let k = native_colors(cx);
    // Like every title in the app, the title dims in an inactive window.
    let ink = if emphasized(window) {
        k.secondary
    } else {
        k.tertiary
    };
    TitleBar::new()
        .maximizable(resizable)
        .h(metrics::CAPTION)
        .pl(space::S12)
        .bg(k.sidebar)
        .border_color(k.separator)
        // The app's menus go here once they exist off the Mac
        // (`views::nav::menus`), before the title.
        .child(
            div()
                .n_footnote()
                .text_color(ink)
                .whitespace_nowrap()
                .child(title),
        )
}
