//! Small helpers around gpui-component controls: the accessible name of an
//! icon button, the placeholder color of every input, and the label and help
//! text of a stacked field. Buttons and fields live in `ui::native`
//! (COMPLETENESS.md §1.9).

use gpui::{
    AnyElement, App, ElementId, Hsla, IntoElement, ParentElement, Pixels, SharedString, Styled,
    Window, div,
};
use gpui_component::button::Button;

use crate::theme::{self, Type as _, space};

/// The name of an icon-only button: its tooltip and the accessible name
/// that VoiceOver reads (§11). gpui-component never reads the tooltip.
pub trait NamedButton {
    fn named(self, name: impl Into<SharedString>) -> Self;
}

impl NamedButton for Button {
    fn named(self, name: impl Into<SharedString>) -> Self {
        let name = name.into();
        self.accessibility_label(name.clone()).tooltip(name)
    }
}

/// Renders a gpui-component input (`Input`, `Textarea`) with its
/// placeholder in the `placeholder` token, light and dark (§2.2, §7.4).
///
/// gpui-component paints a placeholder in the theme's `muted_foreground`,
/// which this app sets to `text-2` for secondary text, so placeholders read
/// as typed text. The input copies the theme colors into its state while it
/// renders, so the theme carries `placeholder` while the input lays out.
/// A `Textarea` renders into an `Input` that renders only at layout, so the
/// swap wraps the layout, not the builder call. Every input in the app
/// renders through here.
pub fn placeholder_input(input: impl IntoElement, _: &mut Window, cx: &mut App) -> AnyElement {
    PlaceholderScope {
        child: input.into_any_element(),
        placeholder: theme::colors(cx).placeholder,
    }
    .into_any_element()
}

/// Lays out its child with the theme's `muted_foreground` set to the
/// `placeholder` token (see [`placeholder_input`]).
struct PlaceholderScope {
    child: AnyElement,
    placeholder: Hsla,
}

impl IntoElement for PlaceholderScope {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

impl gpui::Element for PlaceholderScope {
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
        _: Option<&gpui::GlobalElementId>,
        _: Option<&gpui::InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (gpui::LayoutId, ()) {
        let theme = gpui_component::Theme::global_mut(cx);
        let saved = std::mem::replace(&mut theme.muted_foreground, self.placeholder);
        let layout = self.child.request_layout(window, cx);
        gpui_component::Theme::global_mut(cx).muted_foreground = saved;
        (layout, ())
    }

    fn prepaint(
        &mut self,
        _: Option<&gpui::GlobalElementId>,
        _: Option<&gpui::InspectorElementId>,
        _: gpui::Bounds<Pixels>,
        _: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) {
        self.child.prepaint(window, cx);
    }

    fn paint(
        &mut self,
        _: Option<&gpui::GlobalElementId>,
        _: Option<&gpui::InspectorElementId>,
        _: gpui::Bounds<Pixels>,
        _: &mut (),
        _: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) {
        self.child.paint(window, cx);
    }
}

/// The label above a form field: `small` 13/20 500 `text-2`, 8 pt above the
/// field (§7.4).
pub fn field_label(text: impl Into<SharedString>, cx: &App) -> gpui::Div {
    let k = theme::colors(cx);
    div()
        .pb(space::S8)
        .t_small()
        .medium()
        .text_color(k.text_2)
        .child(text.into())
}

/// Help under a form field: 12/16 `text-3`, 6 pt below. With `error`, the
/// message is `danger-text`.
pub fn field_help(text: impl Into<SharedString>, error: bool, cx: &App) -> gpui::Div {
    let k = theme::colors(cx);
    div()
        .pt(space::S6)
        .t_meta()
        .text_color(if error { k.danger_text } else { k.text_3 })
        .child(text.into())
}
