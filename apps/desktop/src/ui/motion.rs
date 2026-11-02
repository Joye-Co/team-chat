//! Motion helpers (DESIGN.md §10, §12): the spec's durations and curves, with
//! the macOS "Reduce motion" setting honored, and the busy-indicator rule.

use std::time::{Duration, Instant};

use gpui::{
    AnyElement, App, Element, ElementId, GlobalElementId, InspectorElementId, IntoElement,
    LayoutId, Styled, Window,
};

use crate::theme::motion;

/// Reads "Reduce motion" from the system and passes it to GPUI, which then
/// stops decorative animations (`cx.reduce_motion()`). Called at start, on
/// every appearance change and when the accessibility display options change.
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

/// Follows a change of "Reduce motion" in System Settings while the app
/// runs. Call once on the main thread.
#[cfg(target_os = "macos")]
pub fn watch_reduce_motion(cx: &mut App) {
    use std::ptr::NonNull;

    use block2::RcBlock;
    use futures::StreamExt as _;
    use objc2_app_kit::{NSWorkspace, NSWorkspaceAccessibilityDisplayOptionsDidChangeNotification};
    use objc2_foundation::NSNotification;

    let (tx, mut rx) = futures::channel::mpsc::unbounded::<()>();
    let block = RcBlock::new(move |_n: NonNull<NSNotification>| {
        let _ = tx.unbounded_send(());
    });
    let center = NSWorkspace::sharedWorkspace().notificationCenter();
    // SAFETY: the name is a valid notification name; the block only sends on
    // a channel.
    let observer = unsafe {
        center.addObserverForName_object_queue_usingBlock(
            Some(NSWorkspaceAccessibilityDisplayOptionsDidChangeNotification),
            None,
            None,
            &block,
        )
    };
    // Observe for the life of the process.
    std::mem::forget(observer);
    std::mem::forget(block);
    cx.spawn(async move |cx| {
        while rx.next().await.is_some() {
            cx.update(|cx| {
                sync_reduce_motion(cx);
                cx.refresh_windows();
            });
        }
    })
    .detach();
}

#[cfg(not(target_os = "macos"))]
pub fn watch_reduce_motion(_cx: &mut App) {}

/// An enter animation: opacity plus the travel that `travel` adds (0 → 1).
/// With reduce motion the travel goes and the opacity fade stays (§12):
/// GPUI's own animations would snap to the end state instead.
pub fn enter<E: IntoElement + Styled + 'static>(
    id: impl Into<ElementId>,
    duration: Duration,
    element: E,
    travel: impl Fn(E, f32) -> E + 'static,
    cx: &App,
) -> AnyElement {
    use gpui::{Animation, AnimationExt as _};
    let id = id.into();
    if cx.reduce_motion() {
        return Fade {
            id,
            duration,
            element: Some(element),
        }
        .into_any_element();
    }
    element
        .with_animation(
            id,
            Animation::new(duration).with_easing(motion::out_fn()),
            move |el, t| travel(el.opacity(t), t),
        )
        .into_any_element()
}

/// Fades an element in once, on the out curve. Opacity is not travel, so it
/// stays with reduce motion (§12).
pub fn fade_in<E: IntoElement + Styled + 'static>(
    id: impl Into<ElementId>,
    duration: Duration,
    element: E,
    cx: &App,
) -> AnyElement {
    enter(id, duration, element, |el, _| el, cx)
}

/// A one-shot opacity fade that runs whatever `cx.reduce_motion()` says.
struct Fade<E> {
    id: ElementId,
    duration: Duration,
    element: Option<E>,
}

impl<E: IntoElement + Styled + 'static> IntoElement for Fade<E> {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

impl<E: IntoElement + Styled + 'static> Element for Fade<E> {
    type RequestLayoutState = AnyElement;
    type PrepaintState = ();

    fn id(&self) -> Option<ElementId> {
        Some(self.id.clone())
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        let now = cx.background_executor().now();
        window.with_element_state(
            id.expect("Fade has an id"),
            |start: Option<Instant>, window| {
                let start = start.unwrap_or(now);
                let t = fade_progress(now.saturating_duration_since(start), self.duration);
                let element = self.element.take().expect("laid out once");
                let mut element = element.opacity(motion::out_fn()(t)).into_any_element();
                if t < 1.0 {
                    window.request_animation_frame();
                }
                ((element.request_layout(window, cx), element), start)
            },
        )
    }

    fn prepaint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        _bounds: gpui::Bounds<gpui::Pixels>,
        element: &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) {
        element.prepaint(window, cx);
    }

    fn paint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        _bounds: gpui::Bounds<gpui::Pixels>,
        element: &mut Self::RequestLayoutState,
        _: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        element.paint(window, cx);
    }
}

/// Linear progress 0 → 1 of a fade `elapsed` into `duration`.
fn fade_progress(elapsed: Duration, duration: Duration) -> f32 {
    if duration.is_zero() {
        return 1.0;
    }
    (elapsed.as_secs_f32() / duration.as_secs_f32()).clamp(0.0, 1.0)
}

/// What a busy indicator shows at `now` (§12: wait [`motion::BUSY_DELAY`]
/// before showing; once shown, stay at least [`motion::BUSY_MIN`]).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct BusyTimes {
    /// When the work started (while it runs).
    since: Option<Instant>,
    /// When the indicator appeared.
    shown_at: Option<Instant>,
}

impl BusyTimes {
    /// Whether the indicator shows, and when to look again.
    pub fn step(&mut self, busy: bool, now: Instant) -> (bool, Option<Instant>) {
        if busy {
            let since = *self.since.get_or_insert(now);
            if self.shown_at.is_some() {
                return (true, None);
            }
            let at = since + motion::BUSY_DELAY;
            if now >= at {
                self.shown_at = Some(now);
                return (true, None);
            }
            return (false, Some(at));
        }
        self.since = None;
        match self.shown_at {
            Some(shown) if now < shown + motion::BUSY_MIN => (true, Some(shown + motion::BUSY_MIN)),
            _ => {
                self.shown_at = None;
                (false, None)
            }
        }
    }
}

#[derive(Default)]
struct GateState {
    times: BusyTimes,
    /// The wake-up already scheduled.
    wake: Option<Instant>,
}

/// Draws `render(true)` (the busy look) only after the work has run for
/// [`motion::BUSY_DELAY`], and keeps it at least [`motion::BUSY_MIN`];
/// `render(false)` otherwise. The id must be stable for one piece of work (a
/// message, a button, a status line).
pub fn busy_gate(
    id: impl Into<ElementId>,
    working: bool,
    render: impl FnOnce(bool) -> AnyElement + 'static,
) -> BusyGate {
    BusyGate {
        id: id.into(),
        working,
        render: Some(Box::new(render)),
    }
}

pub struct BusyGate {
    id: ElementId,
    working: bool,
    render: Option<Box<dyn FnOnce(bool) -> AnyElement>>,
}

impl IntoElement for BusyGate {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

impl Element for BusyGate {
    type RequestLayoutState = AnyElement;
    type PrepaintState = ();

    fn id(&self) -> Option<ElementId> {
        Some(self.id.clone())
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        let now = cx.background_executor().now();
        let working = self.working;
        window.with_element_state(
            id.expect("BusyGate has an id"),
            |state: Option<GateState>, window| {
                let mut state = state.unwrap_or_default();
                let (show, wake) = state.times.step(working, now);
                if let Some(at) = wake
                    && state.wake != Some(at)
                {
                    state.wake = Some(at);
                    let view = window.current_view();
                    let wait = at.saturating_duration_since(now);
                    window
                        .spawn(cx, async move |cx| {
                            cx.background_executor().timer(wait).await;
                            cx.update(move |_, cx| cx.notify(view)).ok();
                        })
                        .detach();
                }
                let render = self.render.take().expect("laid out once");
                let mut element = render(show);
                ((element.request_layout(window, cx), element), state)
            },
        )
    }

    fn prepaint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        _bounds: gpui::Bounds<gpui::Pixels>,
        element: &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) {
        element.prepaint(window, cx);
    }

    fn paint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        _bounds: gpui::Bounds<gpui::Pixels>,
        element: &mut Self::RequestLayoutState,
        _: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        element.paint(window, cx);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn busy_indicators_wait_then_stay() {
        let t0 = Instant::now();
        let ms = |n| t0 + Duration::from_millis(n);
        let mut b = BusyTimes::default();
        // A fast ack never shows the indicator.
        assert_eq!(b.step(true, t0), (false, Some(ms(400))));
        assert_eq!(b.step(true, ms(50)), (false, Some(ms(400))));
        assert_eq!(b.step(false, ms(60)), (false, None));
        // Slow work shows it at 400 ms and keeps it 500 ms.
        assert_eq!(b.step(true, ms(100)), (false, Some(ms(500))));
        assert_eq!(b.step(true, ms(500)), (true, None));
        assert_eq!(b.step(false, ms(600)), (true, Some(ms(1000))));
        assert_eq!(b.step(false, ms(1000)), (false, None));
    }

    #[test]
    fn fades_run_to_the_end() {
        let d = Duration::from_millis(200);
        assert_eq!(fade_progress(Duration::ZERO, d), 0.0);
        assert_eq!(fade_progress(Duration::from_millis(100), d), 0.5);
        assert_eq!(fade_progress(Duration::from_secs(1), d), 1.0);
        assert_eq!(fade_progress(Duration::ZERO, Duration::ZERO), 1.0);
    }
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
