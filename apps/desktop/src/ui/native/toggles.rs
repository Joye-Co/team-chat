//! Switches, checkboxes and radio buttons (NATIVE.md §5.4) on
//! gpui-component's headless primitives (`gpui-base`): they keep the
//! component's role, keyboard (Space, Return) and focus behaviour; this file
//! only draws them at the native sizes.

use std::rc::Rc;

use gpui::{
    App, ElementId, IntoElement, ParentElement, RenderOnce, SharedString, Styled, Window, div,
    prelude::FluentBuilder as _,
};
use gpui_base::{
    Checkbox as BaseCheckbox, CheckboxState, Radio as BaseRadio, Switch as BaseSwitch,
};

use super::tokens::{NativeType as _, metrics, native_colors};
use crate::ui::sf_symbols::{Sf, Weight, symbol};

type Handler = Rc<dyn Fn(bool, &mut Window, &mut App)>;

/// The size of a switch (§5.4).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SwitchSize {
    /// 32 × 18 (knob 16): every switch in a grouped form row.
    Small,
    /// 40 × 24: only a section's master switch.
    Regular,
}

/// A switch: `accent` track when on, `quaternary` when off, a white knob
/// with a shadow.
#[derive(IntoElement)]
pub struct NativeSwitch {
    id: ElementId,
    on: bool,
    size: SwitchSize,
    disabled: bool,
    name: Option<SharedString>,
    on_change: Option<Handler>,
    /// Draw the focus ring (gallery: the focused state without a key event).
    focused: bool,
}

impl NativeSwitch {
    pub fn new(id: impl Into<ElementId>, on: bool) -> Self {
        Self {
            id: id.into(),
            on,
            size: SwitchSize::Small,
            disabled: false,
            name: None,
            on_change: None,
            focused: false,
        }
    }

    pub fn size(mut self, size: SwitchSize) -> Self {
        self.size = size;
        self
    }

    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    /// The accessible name (the row label it belongs to).
    pub fn name(mut self, name: impl Into<SharedString>) -> Self {
        self.name = Some(name.into());
        self
    }

    pub fn on_change(mut self, f: impl Fn(bool, &mut Window, &mut App) + 'static) -> Self {
        self.on_change = Some(Rc::new(f));
        self
    }

    /// Shows the focus ring as if it had keyboard focus (gallery only).
    pub fn show_focus(mut self, focused: bool) -> Self {
        self.focused = focused;
        self
    }
}

impl RenderOnce for NativeSwitch {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let k = native_colors(cx);
        let (w, h) = match self.size {
            SwitchSize::Small => metrics::SWITCH_SMALL,
            SwitchSize::Regular => metrics::SWITCH_REGULAR,
        };
        let knob = h - metrics::SWITCH_INSET * 2.;
        let handle = window
            .use_keyed_state(self.id.clone(), cx, |_, cx| cx.focus_handle())
            .read(cx)
            .clone();
        // The ring marks keyboard focus only (focus-visible): a click
        // focuses the control too, but must not leave a halo around it.
        let focused =
            self.focused || (handle.is_focused(window) && window.last_input_was_keyboard());
        // The knob slides and the track fades between off and on over FAST
        // on the out curve when `on` changes (not on the first draw);
        // Reduce Motion jumps.
        let (off_x, on_x) = (metrics::SWITCH_INSET, w - knob - metrics::SWITCH_INSET);
        let on = self.on;
        let motion = window.use_keyed_state(
            ElementId::Name(format!("{:?}-motion", self.id).into()),
            cx,
            move |_, _| (on, None::<std::time::Instant>),
        );
        let (last, since) = *motion.read(cx);
        let since = if last != self.on && !cx.reduce_motion() {
            Some(std::time::Instant::now())
        } else if last != self.on {
            None
        } else {
            since
        };
        motion.update(cx, |m, _| *m = (on, since));
        let dur = crate::theme::motion::FAST.as_secs_f32();
        let progress = since
            .map(|t| (t.elapsed().as_secs_f32() / dur).min(1.))
            .unwrap_or(1.);
        if progress < 1. {
            window.request_animation_frame();
        }
        let eased = 1. - (1. - progress).powi(3);
        let at = if self.on { eased } else { 1. - eased };
        let track = k.quaternary.blend(k.primary.opacity(at));
        let on_change = self.on_change.clone();
        BaseSwitch::new(self.id)
            .checked(self.on)
            .disabled(self.disabled)
            .track_focus(&handle)
            .when_some(self.name, |s, n| s.accessibility_label(n))
            .when_some(on_change, |s, f| {
                s.on_change(move |next, _, window, cx| f(next, window, cx))
            })
            .flex_none()
            .w(w)
            .h(h)
            .rounded(h / 2.)
            .bg(track)
            .relative()
            .when(self.disabled, |s| s.opacity(0.5))
            .when(focused && !self.disabled, |s| s.shadow(k.focus_ring()))
            .child(
                div()
                    .absolute()
                    .top(metrics::SWITCH_INSET)
                    .left(off_x + (on_x - off_x) * at)
                    .size(knob)
                    .rounded_full()
                    .bg(if self.on { k.knob_on } else { k.knob })
                    .shadow(k.knob_shadow()),
            )
    }
}

/// A 16 × 16 checkbox with an optional label after it (§5.4): radius 4;
/// on: `accent` fill with an `onAccent` `checkmark` 11 pt.
#[derive(IntoElement)]
pub struct NativeCheckbox {
    id: ElementId,
    on: bool,
    label: Option<SharedString>,
    disabled: bool,
    on_change: Option<Handler>,
    focused: bool,
}

impl NativeCheckbox {
    pub fn new(id: impl Into<ElementId>, on: bool) -> Self {
        Self {
            id: id.into(),
            on,
            label: None,
            disabled: false,
            on_change: None,
            focused: false,
        }
    }

    pub fn label(mut self, label: impl Into<SharedString>) -> Self {
        self.label = Some(label.into());
        self
    }

    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    pub fn on_change(mut self, f: impl Fn(bool, &mut Window, &mut App) + 'static) -> Self {
        self.on_change = Some(Rc::new(f));
        self
    }

    pub fn show_focus(mut self, focused: bool) -> Self {
        self.focused = focused;
        self
    }
}

impl RenderOnce for NativeCheckbox {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let k = native_colors(cx);
        let handle = window
            .use_keyed_state(self.id.clone(), cx, |_, cx| cx.focus_handle())
            .read(cx)
            .clone();
        // The ring marks keyboard focus only (focus-visible): a click
        // focuses the control too, but must not leave a halo around it.
        let focused =
            self.focused || (handle.is_focused(window) && window.last_input_was_keyboard());
        let on_change = self.on_change.clone();
        let indicator = div()
            .flex_none()
            .size(metrics::CHECKBOX)
            .rounded(metrics::CHECKBOX_RADIUS)
            .flex()
            .items_center()
            .justify_center()
            .map(|d| {
                if self.on {
                    d.bg(k.primary).child(symbol(
                        Sf::Check,
                        metrics::CHECKMARK,
                        Weight::Semibold,
                        k.on_primary,
                        window,
                    ))
                } else {
                    d.bg(k.control).shadow(vec![gpui::BoxShadow {
                        spread_radius: gpui::px(1.),
                        ..k.inner_edge(k.quaternary)
                    }])
                }
            })
            .when(focused && !self.disabled, |d| d.shadow(k.focus_ring()));
        BaseCheckbox::new(self.id)
            .checked(self.on)
            .disabled(self.disabled)
            .track_focus(&handle)
            .when_some(self.label.clone(), |s, n| s.accessibility_label(n))
            .when_some(on_change, |s, f| {
                s.on_change(move |state, _, window, cx| {
                    f(state == CheckboxState::Checked, window, cx)
                })
            })
            .flex()
            .items_start()
            .gap(gpui::px(6.))
            .when(self.disabled, |s| s.opacity(0.5))
            .child(div().pt(gpui::px(1.)).child(indicator))
            .when_some(self.label, |s, l| {
                s.child(
                    div()
                        .min_w(gpui::px(0.))
                        .n_body()
                        .text_color(k.label)
                        .child(l),
                )
            })
    }
}

/// A 16 × 16 radio button with an optional label (§5.4): on: `accent`
/// with an `onAccent` 6 pt dot.
#[derive(IntoElement)]
pub struct NativeRadio {
    id: ElementId,
    on: bool,
    label: Option<SharedString>,
    disabled: bool,
    on_change: Option<Handler>,
}

impl NativeRadio {
    pub fn new(id: impl Into<ElementId>, on: bool) -> Self {
        Self {
            id: id.into(),
            on,
            label: None,
            disabled: false,
            on_change: None,
        }
    }

    pub fn label(mut self, label: impl Into<SharedString>) -> Self {
        self.label = Some(label.into());
        self
    }

    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    pub fn on_change(mut self, f: impl Fn(bool, &mut Window, &mut App) + 'static) -> Self {
        self.on_change = Some(Rc::new(f));
        self
    }
}

impl RenderOnce for NativeRadio {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let k = native_colors(cx);
        let on_change = self.on_change.clone();
        let dot = div()
            .flex_none()
            .size(metrics::CHECKBOX)
            .rounded_full()
            .flex()
            .items_center()
            .justify_center()
            .map(|d| {
                if self.on {
                    d.bg(k.primary).child(
                        div()
                            .size(metrics::RADIO_DOT)
                            .rounded_full()
                            .bg(k.on_primary),
                    )
                } else {
                    d.bg(k.control).shadow(vec![gpui::BoxShadow {
                        spread_radius: gpui::px(1.),
                        ..k.inner_edge(k.quaternary)
                    }])
                }
            });
        BaseRadio::new(self.id)
            .checked(self.on)
            .disabled(self.disabled)
            .when_some(self.label.clone(), |s, n| s.accessibility_label(n))
            .when_some(on_change, |s, f| {
                s.on_change(move |next, _, window, cx| f(next, window, cx))
            })
            .flex()
            .items_center()
            .gap(gpui::px(6.))
            .when(self.disabled, |s| s.opacity(0.5))
            .child(dot)
            .when_some(self.label, |s, l| {
                s.child(div().n_body().text_color(k.label).child(l))
            })
    }
}
