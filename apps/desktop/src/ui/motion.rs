//! Motion helpers (DESIGN.md §10): the spec's durations and curves, with the
//! macOS "Reduce motion" setting honored.

use std::time::Duration;

use gpui::{Animation, AnimationExt as _, App, ElementId, IntoElement, Styled};

use crate::theme::motion;

/// Reads "Reduce motion" from the system and passes it to GPUI, which then
/// stops decorative animations (`cx.reduce_motion()`). Called at start and on
/// every appearance change.
pub fn sync_reduce_motion(cx: &mut App) {
    cx.set_reduce_motion(system_reduce_motion());
}

#[cfg(target_os = "macos")]
fn system_reduce_motion() -> bool {
    objc2_app_kit::NSWorkspace::sharedWorkspace().accessibilityDisplayShouldReduceMotion()
}

#[cfg(not(target_os = "macos"))]
fn system_reduce_motion() -> bool {
    false
}

/// Fades an element in once, on the out curve. Opacity is not travel, so it
/// stays with reduce motion (§10).
pub fn fade_in<E: IntoElement + Styled + 'static>(
    id: impl Into<ElementId>,
    duration: Duration,
    element: E,
) -> impl IntoElement {
    element.with_animation(
        id,
        Animation::new(duration).with_easing(motion::out_fn()),
        |el, delta| el.opacity(delta),
    )
}

/// The busy spinner (DESIGN.md §7.1, §12): gpui-component's `Spinner`, one
/// turn per [`motion::SPINNER_TURN`] at a constant speed (a loop is linear;
/// the library default eases each turn in and out, which reads as a
/// stutter). With Reduce motion GPUI stops its own loops, which would leave
/// a still icon that looks stuck, so the spinner then turns by the clock, as
/// a macOS progress indicator keeps turning.
#[derive(gpui::IntoElement)]
pub struct BusySpinner {
    size: gpui_component::Size,
    color: Option<gpui::Hsla>,
}

/// A busy spinner at the medium size in the inherited text color.
pub fn spinner() -> BusySpinner {
    BusySpinner {
        size: gpui_component::Size::Medium,
        color: None,
    }
}

impl BusySpinner {
    pub fn color(mut self, color: gpui::Hsla) -> Self {
        self.color = Some(color);
        self
    }
}

impl gpui_component::Sizable for BusySpinner {
    fn with_size(mut self, size: impl Into<gpui_component::Size>) -> Self {
        self.size = size.into();
        self
    }
}

impl gpui::RenderOnce for BusySpinner {
    fn render(self, window: &mut gpui::Window, cx: &mut App) -> impl IntoElement {
        use gpui::{ParentElement as _, prelude::FluentBuilder as _};
        use gpui_component::Sizable as _;
        if !cx.reduce_motion() {
            return gpui_component::spinner::Spinner::new()
                .ease(gpui::linear)
                .with_size(self.size)
                .when_some(self.color, |s, c| s.color(c))
                .into_any_element();
        }
        window.request_animation_frame();
        gpui::div()
            .child(
                gpui_component::Icon::new(gpui_component::IconName::Loader)
                    .with_size(self.size)
                    .when_some(self.color, |i, c| i.text_color(c))
                    .transform(gpui::Transformation::rotate(gpui::percentage(turn_at(
                        spinner_epoch().elapsed(),
                    )))),
            )
            .into_any_element()
    }
}

/// The clock all reduce-motion spinners turn by.
fn spinner_epoch() -> std::time::Instant {
    static EPOCH: std::sync::OnceLock<std::time::Instant> = std::sync::OnceLock::new();
    *EPOCH.get_or_init(std::time::Instant::now)
}

/// The part of a turn done `elapsed` after the epoch, 0 to 1, at a constant
/// speed.
fn turn_at(elapsed: Duration) -> f32 {
    let turn = motion::SPINNER_TURN.as_secs_f32();
    (elapsed.as_secs_f32() % turn) / turn
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_spinner_turns_at_a_constant_speed() {
        let quarter = motion::SPINNER_TURN / 4;
        assert!((turn_at(Duration::ZERO) - 0.0).abs() < 1e-4);
        assert!((turn_at(quarter) - 0.25).abs() < 1e-3);
        assert!((turn_at(quarter * 2) - 0.5).abs() < 1e-3);
        assert!((turn_at(motion::SPINNER_TURN + quarter) - 0.25).abs() < 1e-3);
    }

    struct Host;

    impl gpui::Render for Host {
        fn render(
            &mut self,
            _: &mut gpui::Window,
            _: &mut gpui::Context<Self>,
        ) -> impl IntoElement {
            spinner()
        }
    }

    /// With Reduce motion GPUI's own loops stop; ours still asks for frames,
    /// so the spinner keeps showing that work goes on.
    #[gpui::test]
    fn reduce_motion_keeps_the_spinner_turning(cx: &mut gpui::TestAppContext) {
        cx.update(|cx| cx.set_reduce_motion(true));
        let window = cx.open_window(gpui::size(gpui::px(100.), gpui::px(100.)), |_, _| Host);
        cx.run_until_parked();
        let frames = window
            .update(cx, |_, window, cx| window.simulate_next_frame(cx))
            .unwrap();
        assert!(frames > 0, "the spinner asks for the next frame");
    }
}
