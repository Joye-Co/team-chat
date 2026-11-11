//! The 关于 window off the Mac. macOS has a standard about panel
//! (`orderFrontStandardAboutPanel`: the icon, the name and the version from the bundle);
//! Windows has none, so the app draws the same three things in a small window of its own.
//! One window: 关于 again brings it to the front. Esc closes it.
#![cfg_attr(target_os = "macos", allow(dead_code))]

use gpui::{
    AnyWindowHandle, App, AppContext as _, Bounds, Context, FocusHandle, Global,
    InteractiveElement as _, IntoElement, KeyDownEvent, ParentElement as _, Pixels, Render, Size,
    Styled as _, TitlebarOptions, Window, WindowBounds, WindowOptions, div, px, size,
};

use crate::{
    copy::{fill, nav},
    theme::Type as _,
    ui::native::{
        NativeType as _, native_colors, overlays::app_icon, tokens::space, window_chrome,
    },
};

/// The content size: the standard about panel's width, and room for the icon and two lines.
pub const WINDOW_SIZE: Size<Pixels> = Size {
    width: px(284.),
    height: px(204.),
};

/// The icon's side, as in the standard about panel.
const ICON: Pixels = px(64.);

/// The window's title and the menu item: 关于 and the product's name.
pub fn title() -> String {
    fill(nav::MENU_ABOUT, &[("product", env!("PRODUCT_NAME"))])
}

/// The version line, as the about panel words it.
pub fn version_line() -> String {
    fill(nav::ABOUT_VERSION, &[("version", env!("CARGO_PKG_VERSION"))])
}

pub struct AboutView {
    focus: FocusHandle,
}

impl AboutView {
    fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let focus = cx.focus_handle();
        window.focus(&focus, cx);
        Self { focus }
    }

    fn on_key(&mut self, event: &KeyDownEvent, window: &mut Window, _: &mut Context<Self>) {
        if event.keystroke.key == "escape" {
            window.remove_window();
        }
    }
}

impl Render for AboutView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let k = native_colors(cx);
        let body = div()
            .id("about")
            .track_focus(&self.focus)
            .on_key_down(cx.listener(Self::on_key))
            .size_full()
            .bg(k.content)
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .gap(space::S4)
            .pb(space::S16)
            .child(app_icon(ICON))
            .child(
                div()
                    .pt(space::S12)
                    .n_headline()
                    .text_color(k.label)
                    .child(env!("PRODUCT_NAME")),
            )
            .child(
                div()
                    .n_footnote()
                    .tnum()
                    .text_color(k.secondary)
                    .child(version_line()),
            );
        window_chrome::framed_fixed(title(), body, window, cx)
    }
}

/// The open 关于 window, so the command does not open a second one.
#[derive(Default)]
struct AboutWindow(Option<AnyWindowHandle>);

impl Global for AboutWindow {}

/// Opens the 关于 window, or brings the open one to the front.
pub fn open(cx: &mut App) {
    if let Some(handle) = cx.try_global::<AboutWindow>().and_then(|w| w.0)
        && handle
            .update(cx, |_, window, _| window.activate_window())
            .is_ok()
    {
        return;
    }
    if let Ok(handle) = open_window(true, cx) {
        cx.set_global(AboutWindow(Some(handle)));
    }
}

/// Opens a 关于 window. Self-tests pass `focus: false`.
pub fn open_window(focus: bool, cx: &mut App) -> Result<AnyWindowHandle, String> {
    let content = size(
        WINDOW_SIZE.width,
        WINDOW_SIZE.height
            + if window_chrome::drawn() {
                crate::ui::native::metrics::CAPTION
            } else {
                px(0.)
            },
    );
    let options = WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(Bounds::centered(None, content, cx))),
        titlebar: Some(TitlebarOptions {
            title: Some(title().into()),
            appears_transparent: true,
            traffic_light_position: None,
        }),
        window_background: gpui::WindowBackgroundAppearance::Opaque,
        is_resizable: false,
        is_minimizable: false,
        focus,
        show: true,
        ..Default::default()
    };
    gpui_kit::open_window(options, cx, move |window, cx| {
        cx.new(|cx| AboutView::new(window, cx))
    })
    .map(|(handle, _)| handle)
    .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_window_names_the_product_and_its_version() {
        assert!(title().contains(env!("PRODUCT_NAME")));
        assert!(version_line().contains(env!("CARGO_PKG_VERSION")));
        assert!(!version_line().contains('{'));
    }

    /// 关于 twice shows one window; Esc closes it.
    #[gpui::test]
    fn about_opens_one_window_and_escape_closes_it(cx: &mut gpui::TestAppContext) {
        cx.update(|cx| {
            gpui_component::init(cx);
            open(cx);
            open(cx);
        });
        cx.run_until_parked();
        assert_eq!(cx.update(|cx| cx.windows().len()), 1);
        let window = cx.update(|cx| cx.windows()[0]);
        let mut vcx = gpui::VisualTestContext::from_window(window, cx);
        vcx.update(|window, _| window.activate_window());
        vcx.run_until_parked();
        vcx.simulate_keystrokes("escape");
        cx.run_until_parked();
        assert!(cx.update(|cx| cx.windows().is_empty()), "Esc closed it");
        // And it opens again afterwards.
        cx.update(open);
        cx.run_until_parked();
        assert_eq!(cx.update(|cx| cx.windows().len()), 1);
    }
}
