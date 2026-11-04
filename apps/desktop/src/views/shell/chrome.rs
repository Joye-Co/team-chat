//! The window chrome of the main window (NATIVE.md §2, §4, §7.3): the live
//! native colors (with the system accent), the floating sidebar frame, the
//! inspector frame, the scroll edge under the toolbar and the scope that
//! gives gpui-component's menus the accent highlight.
//!
//! `ui::native` draws the ink accent; [`colors`] is the same set with the
//! accent the person picked in System Settings (`theme::system_accent`) and
//! the soft focus halo. The kit's `native_colors` should return it too: the
//! shell track does not edit `ui/native`, so the kit parts keep the ink
//! accent until it does.

use gpui::{
    AnyElement, App, Bounds, Element, ElementId, GlobalElementId, InspectorElementId, IntoElement,
    LayoutId, Pixels, Styled, Window, div, linear_color_stop, linear_gradient, px,
};
use gpui_component::Theme;

use crate::ui::native::{NativeColors, metrics, native_colors};

/// The native colors of the current mode with the app accent: the system
/// accent when the person picked one, and the focus halo at 35 %.
pub fn colors(cx: &App) -> NativeColors {
    native_colors(cx)
}

/// How far content fades out under the toolbar row (§2 scroll edge).
pub const SCROLL_EDGE: Pixels = px(24.);

/// The scroll edge effect: the content background at the top fading to
/// clear over [`SCROLL_EDGE`], laid over the top of a page body so content
/// that scrolls under the toolbar fades out. No hairline.
pub fn scroll_edge(cx: &App) -> gpui::Div {
    let k = colors(cx);
    div()
        .absolute()
        .top_0()
        .left_0()
        .right_0()
        .h(SCROLL_EDGE)
        .bg(linear_gradient(
            180.,
            linear_color_stop(k.content, 0.),
            linear_color_stop(k.content.opacity(0.), 1.),
        ))
}

/// The inspector column (§2): edge to edge, the `sidebar` fill and a 1 px
/// `separator` on its leading edge.
pub fn inspector_frame(width: Pixels, cx: &App) -> gpui::Div {
    let k = colors(cx);
    div()
        .w(width)
        .h_full()
        .flex_none()
        .relative()
        .overflow_hidden()
        .bg(k.sidebar)
        .border_l_1()
        .border_color(k.separator)
}

/// Gives the gpui-component menus opened inside `child` the menu highlight
/// of §7.3 (`accent` fill, `onAccent` text). gpui-component paints menu
/// rows and ghost buttons from one token, and markdown inline code from its
/// color, so the app theme keeps them gray; this swaps the token only while
/// `child` (and the menus it opens, which render in its prepaint) is laid
/// out. Wrap menu triggers that hold no ghost buttons.
pub fn menu_accent(child: impl IntoElement) -> MenuAccent {
    MenuAccent {
        child: child.into_any_element(),
    }
}

pub struct MenuAccent {
    child: AnyElement,
}

impl IntoElement for MenuAccent {
    type Element = Self;

    fn into_element(self) -> Self {
        self
    }
}

/// Runs `f` with the menu highlight in the theme, then puts it back.
fn with_highlight<R>(cx: &mut App, f: impl FnOnce(&mut App) -> R) -> R {
    let k = colors(cx);
    let theme = Theme::global_mut(cx);
    let saved = (theme.tokens.accent, theme.colors.accent_foreground);
    theme.tokens.accent = k.accent.into();
    theme.colors.accent_foreground = k.on_accent;
    let out = f(cx);
    let theme = Theme::global_mut(cx);
    theme.tokens.accent = saved.0;
    theme.colors.accent_foreground = saved.1;
    out
}

impl Element for MenuAccent {
    type RequestLayoutState = ();
    type PrepaintState = ();

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static std::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, ()) {
        let child = &mut self.child;
        (
            with_highlight(cx, |cx| child.request_layout(window, cx)),
            (),
        )
    }

    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        _: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) {
        let child = &mut self.child;
        with_highlight(cx, |cx| child.prepaint(window, cx));
    }

    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        _: &mut (),
        _: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) {
        self.child.paint(window, cx);
    }
}

/// The width the floating sidebar takes from the window: the panel and
/// its inset on both sides.
pub fn sidebar_room() -> Pixels {
    metrics::SIDEBAR_WIDTH + metrics::SIDEBAR_INSET * 2.
}
