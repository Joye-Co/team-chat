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
