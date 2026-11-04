//! The settings window's height follows its pane (NATIVE.md §8): the pane is
//! measured as it is laid out and the window frame changes with its top edge
//! fixed, animated in the app (AppKit's own resize animation, about 0.2 s),
//! at once in self-tests and on the first frame. While a sheet is open the
//! window is at least as tall as the sheet needs.

use std::cell::Cell;

use gpui::{AnyWindowHandle, App, Pixels, Window, px};
use gpui_component::WindowExt as _;

use super::{MAX_HEIGHT, MIN_HEIGHT};

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
}

impl Fit {
    /// The pane laid out at `wanted` (toolbar plus pane). Changes the window
    /// height after this frame when it differs from the window's.
    pub fn measured(&self, wanted: Pixels, handle: AnyWindowHandle, window: &mut Window, cx: &mut App) {
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
        if (target - now).abs() < 1. || (target - self.applied.get()).abs() < 1. && self.fitted.get() {
            return;
        }
        self.applied.set(target);
        let animate = self.fitted.replace(true) && super::prefs::saving();
        // After this frame and outside the window update: AppKit calls back
        // into the window while it resizes.
        cx.spawn(async move |cx| {
            let Ok(Some((ns_window, now))) =
                handle.update(cx, |_, window, _| ns_window(window).map(|w| (w, window.viewport_size().height)))
            else {
                return;
            };
            set_height(ns_window, now, px(target), animate);
        })
        .detach();
    }
}

/// Changes the content height of a window from `now` to `height`, keeping
/// its top edge where it is.
#[cfg(target_os = "macos")]
fn set_height(ns_window: NsWindow, now: Pixels, height: Pixels, animate: bool) {
    use objc2::msg_send;
    use objc2_foundation::{NSPoint, NSRect, NSSize};

    let delta = f64::from(f32::from(height)) - f64::from(f32::from(now));
    if delta.abs() < 1. {
        return;
    }
    // SAFETY: a live NSWindow on the main thread; `frame` is a getter and
    // `setFrame:display:animate:` a setter.
    unsafe {
        let frame: NSRect = msg_send![ns_window, frame];
        let next = NSRect::new(
            NSPoint::new(frame.origin.x, frame.origin.y - delta),
            NSSize::new(frame.size.width, frame.size.height + delta),
        );
        let _: () = msg_send![ns_window, setFrame: next, display: true, animate: animate];
    }
}

#[cfg(not(target_os = "macos"))]
fn set_height(_: NsWindow, _: Pixels, _: Pixels, _: bool) {}

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
