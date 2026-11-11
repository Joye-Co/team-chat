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

/// A window's top edge and height on its screen, in pixels counted down from the top
/// (Windows).
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Span {
    top: i32,
    height: i32,
}

/// Where a window at `frame` goes when its height changes by `delta`, on a screen whose
/// usable area is `work`: the top edge stays, the height stops at the screen's, the first
/// height centers the window, and the window never leaves the usable area.
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
fn fit_span(frame: Span, work: Span, delta: i32, first: bool) -> Span {
    let height = (frame.height + delta).min(work.height).max(0);
    let mut top = frame.top;
    if first {
        top = work.top + (work.height - height) / 2;
    }
    let bottom = work.top + work.height;
    if top + height > bottom {
        top = bottom - height;
    }
    if top < work.top {
        top = work.top;
    }
    Span { top, height }
}

/// Windows: the window's rectangle changes with its top edge fixed (`animate` is unused:
/// the app steps the height itself).
#[cfg(target_os = "windows")]
fn set_height(hwnd: NsWindow, now: Pixels, height: Pixels, _animate: bool, first: bool) {
    use windows::Win32::{
        Foundation::{HWND, RECT},
        Graphics::Gdi::{
            GetMonitorInfoW, MONITOR_DEFAULTTONEAREST, MONITORINFO, MonitorFromWindow,
        },
        UI::{
            HiDpi::GetDpiForWindow,
            WindowsAndMessaging::{GetWindowRect, SWP_NOACTIVATE, SWP_NOZORDER, SetWindowPos},
        },
    };

    let hwnd = HWND(hwnd);
    // SAFETY: `hwnd` is a live window of this thread (from GPUI's handle); every out
    // pointer is valid for its call.
    unsafe {
        let scale = match GetDpiForWindow(hwnd) {
            0 => 1.,
            dpi => dpi as f32 / 96.,
        };
        let delta = ((f32::from(height) - f32::from(now)) * scale).round() as i32;
        let mut rect = RECT::default();
        if GetWindowRect(hwnd, &mut rect).is_err() {
            return;
        }
        let frame = Span {
            top: rect.top,
            height: rect.bottom - rect.top,
        };
        let mut info = MONITORINFO {
            cbSize: std::mem::size_of::<MONITORINFO>() as u32,
            ..Default::default()
        };
        let monitor = MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST);
        let next = if GetMonitorInfoW(monitor, &mut info).as_bool() {
            let work = Span {
                top: info.rcWork.top,
                height: info.rcWork.bottom - info.rcWork.top,
            };
            fit_span(frame, work, delta, first)
        } else {
            Span {
                top: frame.top,
                height: frame.height + delta,
            }
        };
        if next == frame {
            return;
        }
        let _ = SetWindowPos(
            hwnd,
            None,
            rect.left,
            next.top,
            rect.right - rect.left,
            next.height,
            SWP_NOZORDER | SWP_NOACTIVATE,
        );
    }
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
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

/// Windows: DWM cloaks the window (it is laid out and painted, but not shown) until
/// [`reveal`].
#[cfg(target_os = "windows")]
pub fn hide_until_fitted(window: &Window) {
    if let Some(hwnd) = ns_window(window) {
        cloak(hwnd, true);
    }
}

#[cfg(target_os = "windows")]
fn cloak(hwnd: NsWindow, on: bool) {
    use windows::{
        Win32::{
            Foundation::HWND,
            Graphics::Dwm::{DWMWA_CLOAK, DwmSetWindowAttribute},
        },
        core::BOOL,
    };
    let value = BOOL::from(on);
    // SAFETY: the attribute takes a BOOL; the pointer and its size match.
    unsafe {
        let _ = DwmSetWindowAttribute(
            HWND(hwnd),
            DWMWA_CLOAK,
            (&raw const value).cast(),
            std::mem::size_of::<BOOL>() as u32,
        );
    }
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
pub fn hide_until_fitted(_window: &Window) {}

#[cfg(target_os = "macos")]
fn reveal(ns_window: NsWindow) {
    // SAFETY: a live NSWindow on the main thread; `setAlphaValue:` a setter.
    unsafe {
        let _: () = objc2::msg_send![ns_window, setAlphaValue: 1.0f64];
    }
}

#[cfg(target_os = "windows")]
fn reveal(hwnd: NsWindow) {
    cloak(hwnd, false);
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
fn reveal(_: NsWindow) {}

/// Pins the drawn frame to the window's top edge, unscaled. The layer's
/// default gravity stretches the last frame over the new size until the app
/// draws again, so while the height animates a stale frame squeezed or
/// stretched the whole pane and it jumped by a few points. The top edge stays
/// where it is during a resize, so a stale frame pinned there only shows
/// less or more of its bottom.
#[cfg(target_os = "macos")]
pub fn pin_content_to_top(window: &Window) {
    use objc2::{msg_send, runtime::AnyObject};
    use objc2_foundation::NSString;
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};

    let Ok(handle) = HasWindowHandle::window_handle(window) else {
        return;
    };
    let RawWindowHandle::AppKit(appkit) = handle.as_raw() else {
        return;
    };
    let view = appkit.ns_view.as_ptr().cast::<AnyObject>();
    // SAFETY: GPUI's AppKit handle points at the window's live content view;
    // `layer` and `contentsAreFlipped` are getters and `setContentsGravity:`
    // a setter, on the main thread.
    unsafe {
        let layer: *mut AnyObject = msg_send![view, layer];
        if layer.is_null() {
            return;
        }
        // The gravity names follow the layer's y axis: in a flipped layer
        // "bottom" is the top of the screen.
        let flipped: bool = msg_send![layer, contentsAreFlipped];
        let gravity = NSString::from_str(if flipped { "bottom" } else { "top" });
        let _: () = msg_send![layer, setContentsGravity: &*gravity];
    }
}

#[cfg(not(target_os = "macos"))]
pub fn pin_content_to_top(_window: &Window) {}

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

/// Windows: the window loses its minimize and maximize boxes (and with them maximize on
/// a double click of the title bar and Snap's maximize).
#[cfg(target_os = "windows")]
pub fn dim_window_buttons(window: &Window) {
    let Some(hwnd) = ns_window(window) else {
        return;
    };
    remove_size_boxes(hwnd);
}

/// Takes the minimize and maximize boxes off the window `hwnd`.
#[cfg(target_os = "windows")]
fn remove_size_boxes(hwnd: NsWindow) {
    use windows::Win32::{
        Foundation::HWND,
        UI::WindowsAndMessaging::{
            GWL_STYLE, GetWindowLongPtrW, SWP_FRAMECHANGED, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE,
            SWP_NOZORDER, SetWindowLongPtrW, SetWindowPos, WS_MAXIMIZEBOX, WS_MINIMIZEBOX,
        },
    };
    let hwnd = HWND(hwnd);
    // SAFETY: `hwnd` is a live window of this thread; the style is read, two bits are
    // cleared and the frame is told to redraw.
    unsafe {
        let style = GetWindowLongPtrW(hwnd, GWL_STYLE);
        let next = style & !((WS_MINIMIZEBOX.0 | WS_MAXIMIZEBOX.0) as isize);
        if next != style {
            SetWindowLongPtrW(hwnd, GWL_STYLE, next);
            let _ = SetWindowPos(
                hwnd,
                None,
                0,
                0,
                0,
                0,
                SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE | SWP_FRAMECHANGED,
            );
        }
    }
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
pub fn dim_window_buttons(_window: &Window) {}

#[cfg(target_os = "macos")]
type NsWindow = *mut objc2::runtime::AnyObject;
#[cfg(not(target_os = "macos"))]
type NsWindow = *mut std::ffi::c_void;

/// Windows: the HWND.
#[cfg(target_os = "windows")]
fn ns_window(window: &Window) -> Option<NsWindow> {
    crate::win32::hwnd(window).map(|hwnd| hwnd.0)
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
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

#[cfg(test)]
mod tests {
    use super::{Span, fit_span};

    const WORK: Span = Span {
        top: 0,
        height: 1000,
    };

    #[test]
    fn the_top_edge_stays_while_the_height_changes() {
        let frame = Span {
            top: 200,
            height: 400,
        };
        assert_eq!(
            fit_span(frame, WORK, 150, false),
            Span {
                top: 200,
                height: 550
            }
        );
        assert_eq!(
            fit_span(frame, WORK, -100, false),
            Span {
                top: 200,
                height: 300
            }
        );
    }

    #[test]
    fn the_first_height_centers_the_window() {
        let frame = Span {
            top: 30,
            height: 400,
        };
        assert_eq!(
            fit_span(frame, WORK, 200, true),
            Span {
                top: 200,
                height: 600
            }
        );
    }

    #[test]
    fn the_window_stays_inside_the_usable_area() {
        let work = Span {
            top: 40,
            height: 1000,
        };
        // Growing past the bottom moves the window up.
        let low = Span {
            top: 700,
            height: 300,
        };
        assert_eq!(
            fit_span(low, work, 200, false),
            Span {
                top: 540,
                height: 500
            }
        );
        // Taller than the screen: the screen's height, from its top.
        assert_eq!(
            fit_span(low, work, 5000, false),
            Span {
                top: 40,
                height: 1000
            }
        );
        // Above the usable area: back inside.
        let high = Span {
            top: -50,
            height: 300,
        };
        assert_eq!(fit_span(high, work, 0, false).top, 40);
    }
}

/// The settings window's Windows calls on a real window: its height follows the pane with
/// the top edge fixed, it is cloaked until it is fitted and shown after, and it loses its
/// minimize and maximize boxes (what the dimmed caption button stands for).
#[cfg(all(test, target_os = "windows"))]
mod windows_tests {
    use gpui::px;
    use windows::{
        Win32::{
            Foundation::{HWND, RECT},
            Graphics::Dwm::{DWMWA_CLOAKED, DwmGetWindowAttribute},
            UI::{
                HiDpi::GetDpiForWindow,
                WindowsAndMessaging::{
                    CreateWindowExW, DestroyWindow, GWL_STYLE, GetWindowLongPtrW, GetWindowRect,
                    IsZoomed, SW_MAXIMIZE, ShowWindow, WINDOW_EX_STYLE, WS_MAXIMIZEBOX,
                    WS_MINIMIZEBOX, WS_OVERLAPPEDWINDOW, WS_VISIBLE,
                },
            },
        },
        core::w,
    };

    use super::{cloak, remove_size_boxes, reveal, set_height};

    fn rect(hwnd: HWND) -> RECT {
        let mut rect = RECT::default();
        // SAFETY: a live window and a valid out pointer.
        unsafe { GetWindowRect(hwnd, &mut rect).unwrap() };
        rect
    }

    fn cloaked(hwnd: HWND) -> bool {
        let mut value = 0u32;
        // SAFETY: the attribute is a DWORD; the pointer and size match.
        unsafe {
            DwmGetWindowAttribute(
                hwnd,
                DWMWA_CLOAKED,
                (&raw mut value).cast(),
                std::mem::size_of::<u32>() as u32,
            )
            .unwrap();
        }
        value != 0
    }

    #[test]
    fn the_settings_window_fits_its_pane_and_has_one_size() {
        // SAFETY: a plain window of this thread, destroyed below.
        unsafe {
            let hwnd = CreateWindowExW(
                WINDOW_EX_STYLE(0),
                w!("STATIC"),
                w!("settings fit test"),
                WS_OVERLAPPEDWINDOW | WS_VISIBLE,
                120,
                90,
                640,
                480,
                None,
                None,
                None,
                None,
            )
            .expect("a window");
            let scale = GetDpiForWindow(hwnd).max(96) as f32 / 96.;
            // A taller pane: the window grows by the difference, its top edge stays.
            let before = rect(hwnd);
            set_height(hwnd.0, px(480.), px(633.), false, false);
            let taller = rect(hwnd);
            assert_eq!(taller.top, before.top, "the top edge stays");
            assert_eq!((taller.left, taller.right), (before.left, before.right));
            assert_eq!(
                (taller.bottom - taller.top) - (before.bottom - before.top),
                (153. * scale).round() as i32
            );
            // A shorter pane: back down.
            set_height(hwnd.0, px(633.), px(480.), false, false);
            assert_eq!(
                rect(hwnd).bottom - rect(hwnd).top,
                before.bottom - before.top
            );
            // Hidden until fitted, then shown.
            cloak(hwnd.0, true);
            assert!(cloaked(hwnd), "cloaked before the first fit");
            reveal(hwnd.0);
            assert!(!cloaked(hwnd), "shown after it");
            // One size: no minimize or maximize box, so the window cannot be maximized.
            remove_size_boxes(hwnd.0);
            let style = GetWindowLongPtrW(hwnd, GWL_STYLE) as u32;
            assert_eq!(style & (WS_MINIMIZEBOX.0 | WS_MAXIMIZEBOX.0), 0);
            let _ = ShowWindow(hwnd, SW_MAXIMIZE);
            println!(
                "maximized without a maximize box: {}",
                IsZoomed(hwnd).as_bool()
            );
            let _ = DestroyWindow(hwnd);
        }
    }
}
