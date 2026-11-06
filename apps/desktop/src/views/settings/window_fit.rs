//! The settings window's height follows its pane (NATIVE.md §8): the pane is
//! measured as it is laid out and the window frame changes with its top edge
//! fixed, animated by the app frame by frame over 0.2 s on the out curve,
//! at once in self-tests, on the first frame and under Reduce Motion. While a sheet is open the
//! window is at least as tall as the sheet needs.

use std::cell::Cell;

use gpui::{AnyWindowHandle, App, Pixels, Window, px};
use gpui_component::WindowExt as _;

use super::{MAX_HEIGHT, MIN_HEIGHT};

/// How long the window takes to reach a pane's height.
const RESIZE: std::time::Duration = crate::theme::motion::SLOW;
/// One step of that resize (120 Hz).
const FRAME: std::time::Duration = std::time::Duration::from_micros(8_333);

thread_local! {
    /// The window height the open sheet needs (0 when none was asked for).
    static SHEET_FLOOR: Cell<f32> = const { Cell::new(0.) };
}

/// Asks the window to be at least `height` tall while the sheet opened next
/// is up.
pub fn sheet_floor(height: Pixels) {
    SHEET_FLOOR.with(|f| f.set(f32::from(height)));
}

#[derive(Default)]
pub struct Fit {
    /// The height last asked of the window.
    applied: Cell<f32>,
    /// Whether the window has had its first height (no animation for it).
    fitted: Cell<bool>,
    /// Bumped per new target, so a running resize stops when a newer one
    /// starts.
    generation: std::rc::Rc<Cell<u64>>,
}

impl Fit {
    /// The pane laid out at `wanted` (toolbar plus pane). Changes the window
    /// height after this frame when it differs from the window's.
    pub fn measured(
        &self,
        wanted: Pixels,
        handle: AnyWindowHandle,
        window: &mut Window,
        cx: &mut App,
    ) {
        let floor = if window.has_active_dialog(cx) {
            SHEET_FLOOR.with(|f| f.get())
        } else {
            0.
        };
        let target = f32::from(wanted)
            .max(floor)
            .clamp(f32::from(MIN_HEIGHT), f32::from(MAX_HEIGHT))
            .round();
        let now = f32::from(window.viewport_size().height);
        let first = !self.fitted.get();
        if !first && ((target - now).abs() < 1. || (target - self.applied.get()).abs() < 1.) {
            return;
        }
        self.applied.set(target);
        // Reduce Motion: the frame changes at once (review-animations).
        let animate = self.fitted.replace(true) && super::prefs::saving() && !cx.reduce_motion();
        // After this frame and outside the window update: AppKit calls back
        // into the window while it resizes.
        let generation = self.generation.clone();
        let run = generation.get() + 1;
        generation.set(run);
        cx.spawn(async move |cx| {
            let Ok(Some((ns_window, now))) = handle.update(cx, |_, window, _| {
                ns_window(window).map(|w| (w, window.viewport_size().height))
            }) else {
                return;
            };
            if animate {
                // One smooth resize driven here, frame by frame, on the out
                // curve: AppKit's own animated `setFrame` runs ahead of
                // GPUI's redraws, so the pane jumped and settled in steps.
                let from = f32::from(now);
                let started = std::time::Instant::now();
                let dur = RESIZE.as_secs_f32();
                loop {
                    if generation.get() != run {
                        return;
                    }
                    let t = (started.elapsed().as_secs_f32() / dur).min(1.);
                    let eased = 1. - (1. - t).powi(3);
                    let h = (from + (target - from) * eased).round();
                    let Ok(cur) = handle.update(cx, |_, window, _| window.viewport_size().height)
                    else {
                        return;
                    };
                    set_height(ns_window, cur, px(h), false, false);
                    if t >= 1. {
                        return;
                    }
                    cx.background_executor().timer(FRAME).await;
                }
            }
            set_height(ns_window, now, px(target), false, first);
            // The window was transparent until it had its own height, so it
            // never shows the opening size and then jumps (the flicker).
            if first {
                reveal(ns_window);
            }
        })
        .detach();
    }
}

/// Changes the content height of a window from `now` to `height`, keeping
/// its top edge where it is, and always inside the screen's visible frame
/// (above the Dock, under the menu bar): a window that would run past the
/// bottom moves up, one taller than the screen gets the screen's height. The
/// `first` height centers the window on the screen instead.
#[cfg(target_os = "macos")]
fn set_height(ns_window: NsWindow, now: Pixels, height: Pixels, animate: bool, first: bool) {
    use objc2::{msg_send, runtime::AnyObject};
    use objc2_foundation::{NSPoint, NSRect, NSSize};

    let delta = f64::from(f32::from(height)) - f64::from(f32::from(now));
    // SAFETY: a live NSWindow on the main thread; `frame`, `screen` and
    // `visibleFrame` are getters and `setFrame:display:animate:` a setter.
    unsafe {
        let frame: NSRect = msg_send![ns_window, frame];
        let mut next = NSRect::new(
            NSPoint::new(frame.origin.x, frame.origin.y - delta),
            NSSize::new(frame.size.width, frame.size.height + delta),
        );
        let screen: *mut AnyObject = msg_send![ns_window, screen];
        if !screen.is_null() {
            let visible: NSRect = msg_send![screen, visibleFrame];
            next.size.height = next.size.height.min(visible.size.height);
            let top = visible.origin.y + visible.size.height;
            if first {
                next.origin.y = visible.origin.y + (visible.size.height - next.size.height) / 2.;
            }
            if next.origin.y < visible.origin.y {
                next.origin.y = visible.origin.y;
            }
            if next.origin.y + next.size.height > top {
                next.origin.y = top - next.size.height;
            }
        }
        if (next.size.height - frame.size.height).abs() < 1.
            && (next.origin.y - frame.origin.y).abs() < 1.
        {
            return;
        }
        let _: () = msg_send![ns_window, setFrame: next, display: true, animate: animate];
    }
}

#[cfg(not(target_os = "macos"))]
fn set_height(_: NsWindow, _: Pixels, _: Pixels, _: bool, _: bool) {}

/// Keeps a new window transparent until its first height is set.
#[cfg(target_os = "macos")]
pub fn hide_until_fitted(window: &Window) {
    let Some(ns_window) = ns_window(window) else {
        return;
    };
    // SAFETY: a live NSWindow on the main thread; `setAlphaValue:` a setter.
    unsafe {
        let _: () = objc2::msg_send![ns_window, setAlphaValue: 0.0f64];
    }
}

#[cfg(not(target_os = "macos"))]
pub fn hide_until_fitted(_window: &Window) {}

#[cfg(target_os = "macos")]
fn reveal(ns_window: NsWindow) {
    // SAFETY: a live NSWindow on the main thread; `setAlphaValue:` a setter.
    unsafe {
        let _: () = objc2::msg_send![ns_window, setAlphaValue: 1.0f64];
    }
}

#[cfg(not(target_os = "macos"))]
fn reveal(_: NsWindow) {}

/// Dims the minimize and zoom buttons (§8): the window has one size.
#[cfg(target_os = "macos")]
pub fn dim_window_buttons(window: &Window) {
    use objc2::{msg_send, runtime::AnyObject};
    let Some(ns_window) = ns_window(window) else {
        return;
    };
    // NSWindowMiniaturizeButton = 1, NSWindowZoomButton = 2.
    for kind in [1usize, 2] {
        // SAFETY: a live NSWindow on the main thread; the button may be nil.
        unsafe {
            let button: *mut AnyObject = msg_send![ns_window, standardWindowButton: kind];
            if !button.is_null() {
                let _: () = msg_send![button, setEnabled: false];
            }
        }
    }
}

#[cfg(not(target_os = "macos"))]
pub fn dim_window_buttons(_window: &Window) {}

type NsWindow = *mut objc2::runtime::AnyObject;

#[cfg(not(target_os = "macos"))]
fn ns_window(_: &Window) -> Option<NsWindow> {
    None
}

#[cfg(target_os = "macos")]
fn ns_window(window: &Window) -> Option<NsWindow> {
    use objc2::{msg_send, runtime::AnyObject};
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};

    let handle = HasWindowHandle::window_handle(window).ok()?;
    let RawWindowHandle::AppKit(appkit) = handle.as_raw() else {
        return None;
    };
    let view = appkit.ns_view.as_ptr().cast::<AnyObject>();
    // SAFETY: GPUI's AppKit handle points at the window's live content view;
    // `window` is a getter on the main thread.
    let ns_window: *mut AnyObject = unsafe { msg_send![view, window] };
    (!ns_window.is_null()).then_some(ns_window)
}
