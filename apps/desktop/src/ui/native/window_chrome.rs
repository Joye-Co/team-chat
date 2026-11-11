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

#[cfg(not(target_os = "macos"))]
use gpui::prelude::FluentBuilder as _;
use gpui::{
    AnyElement, App, IntoElement, ParentElement as _, SharedString, Styled as _, Window, div,
};
use gpui_component::TitleBar;

use super::tokens::{NativeType as _, emphasized, metrics, native_colors, space};

/// Whether this platform needs the app to draw the caption.
pub const fn drawn() -> bool {
    cfg!(not(target_os = "macos"))
}

thread_local! {
    /// The windows whose content sits under a caption strip ([`frame`]).
    static FRAMED: std::cell::RefCell<std::collections::HashSet<gpui::WindowId>> =
        std::cell::RefCell::new(std::collections::HashSet::new());
}

/// The height the caption strip takes above `window`'s content: none on the
/// Mac, in full screen, and in a window that draws no strip (the self-tests'
/// own sample windows).
pub fn height(window: &Window) -> gpui::Pixels {
    let framed = FRAMED.with(|f| f.borrow().contains(&window.window_handle().window_id()));
    if drawn() && framed && !window.is_fullscreen() {
        metrics::CAPTION
    } else {
        gpui::px(0.)
    }
}

/// Where a sheet or an alert hangs, from the window's top edge: under the
/// toolbar row (NATIVE.md §7.1), which sits under the caption strip off the
/// Mac. A dialog is configured without its window, so a full-screen window
/// (no strip) keeps the strip's height as a gap above its sheets.
pub fn sheet_top() -> gpui::Pixels {
    if drawn() {
        metrics::TOOLBAR_ROW + metrics::CAPTION
    } else {
        metrics::TOOLBAR_ROW
    }
}

/// `content` under the window's caption strip; `content` alone on macOS and
/// in full screen.
pub fn framed(
    title: impl Into<SharedString>,
    content: impl IntoElement,
    window: &mut Window,
    cx: &mut App,
) -> AnyElement {
    frame(title.into(), Kind::Plain, content, window, cx)
}

/// [`framed`] for the main window: the app's menus sit in the caption strip
/// in place of the title (Windows has no menu bar outside the window).
pub fn framed_main(
    title: impl Into<SharedString>,
    content: impl IntoElement,
    window: &mut Window,
    cx: &mut App,
) -> AnyElement {
    frame(title.into(), Kind::Main, content, window, cx)
}

/// [`framed`] for a window that cannot be resized (login, 设置): its maximize
/// button is drawn dimmed and does nothing, as the Mac dims the zoom button.
pub fn framed_fixed(
    title: impl Into<SharedString>,
    content: impl IntoElement,
    window: &mut Window,
    cx: &mut App,
) -> AnyElement {
    frame(title.into(), Kind::Fixed, content, window, cx)
}

/// How much of the menus' ink an inactive window keeps (`label` to `tertiary`).
#[cfg(not(target_os = "macos"))]
const INACTIVE_MENUS: f32 = 0.5;

/// What a window's caption strip holds.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    /// The title and the three window buttons.
    Plain,
    /// The app's menus too.
    Main,
    /// A window that cannot be resized: the maximize button is dimmed.
    Fixed,
}

fn frame(
    title: SharedString,
    kind: Kind,
    content: impl IntoElement,
    window: &mut Window,
    cx: &mut App,
) -> AnyElement {
    if !drawn() || window.is_fullscreen() {
        return content.into_any_element();
    }
    FRAMED.with(|f| f.borrow_mut().insert(window.window_handle().window_id()));
    let k = native_colors(cx);
    div()
        .size_full()
        .flex()
        .flex_col()
        .bg(k.content)
        .child(caption(title, kind, window, cx))
        .child(div().flex_1().min_h_0().w_full().relative().child(content))
        .into_any_element()
}

/// The caption strip: the window's title at the leading edge, the window
/// buttons at the trailing edge.
fn caption(title: SharedString, kind: Kind, window: &mut Window, cx: &mut App) -> impl IntoElement {
    let k = native_colors(cx);
    // Like every title in the app, the title dims in an inactive window.
    let ink = if emphasized(window) {
        k.secondary
    } else {
        k.tertiary
    };
    // gpui-component's menu bar over the app's menus (`views::nav::menus`).
    #[cfg(not(target_os = "macos"))]
    let menus = (kind == Kind::Main).then(|| {
        div()
            .flex_none()
            .h_full()
            // The menus dim with the rest of an inactive window's title.
            .when(!emphasized(window), |d| d.opacity(INACTIVE_MENUS))
            .child(crate::views::nav::menus::window_menu_bar(cx))
    });
    #[cfg(target_os = "macos")]
    let menus: Option<gpui::Div> = None;
    TitleBar::new()
        .maximizable(kind != Kind::Fixed)
        .h(metrics::CAPTION)
        .pl(space::S12)
        .bg(k.sidebar)
        .border_color(k.separator)
        .child(
            div()
                .h_full()
                .flex()
                .items_center()
                .gap(space::S12)
                .children(menus)
                // The main window's first menu is the app's name already.
                .children((kind != Kind::Main).then(|| {
                    div()
                        .n_footnote()
                        .text_color(ink)
                        .whitespace_nowrap()
                        .child(title)
                })),
        )
}
