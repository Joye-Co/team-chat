//! Finds the NSWindow number of a GPUI window and captures only that window
//! with `screencapture -l`. Never the full screen.

use std::{path::Path, process::Command};

use gpui::Window;

#[cfg(target_os = "macos")]
fn ns_window(window: &Window) -> Result<*mut objc2::runtime::AnyObject, String> {
    use objc2::{msg_send, runtime::AnyObject};
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};

    let handle = HasWindowHandle::window_handle(window).map_err(|e| e.to_string())?;
    let RawWindowHandle::AppKit(appkit) = handle.as_raw() else {
        return Err("not an AppKit window".into());
    };
    let view = appkit.ns_view.as_ptr().cast::<AnyObject>();
    // SAFETY: GPUI's AppKit handle points at the window's live content NSView;
    // `window` is a plain getter on the main thread.
    let ns_window: *mut AnyObject = unsafe { msg_send![view, window] };
    if ns_window.is_null() {
        return Err("the view has no window".into());
    }
    Ok(ns_window)
}

/// Puts the window above other apps' windows without activating this app or
/// taking focus. A fully covered window gets no frames (GPUI stops its display
/// link while AppKit reports it occluded), and the capture would be stale.
#[cfg(target_os = "macos")]
pub fn order_front(window: &Window) -> Result<(), String> {
    let ns_window = ns_window(window)?;
    // SAFETY: a live NSWindow on the main thread.
    unsafe {
        let _: () = objc2::msg_send![ns_window, orderFrontRegardless];
    }
    Ok(())
}

#[cfg(not(target_os = "macos"))]
pub fn order_front(_window: &Window) -> Result<(), String> {
    Ok(())
}

/// Gives a window that opened at the requested `width` exactly `height`, with
/// its top edge kept (a new window can come 1 pt taller than asked). AppKit shrinks a window
/// to the screen's visible frame, which the Dock shortens (887 pt on a 982 pt
/// display), in `constrainFrameRect:toScreen:`. The screenshot process adds
/// an override to the window's class that keeps the frame as asked: the Dock
/// does not show in a capture of one window. Windows with their own size
/// (settings) keep it.
#[cfg(target_os = "macos")]
pub fn undo_clamp(window: &Window, width: f64, height: f64) -> Result<(), String> {
    use objc2::runtime::{AnyClass, AnyObject, Imp, Sel};
    use objc2_foundation::{NSPoint, NSRect, NSSize};

    extern "C-unwind" fn keep_frame(
        _: *mut AnyObject,
        _: Sel,
        rect: NSRect,
        _: *mut AnyObject,
    ) -> NSRect {
        rect
    }

    let ns_window = ns_window(window)?;
    // SAFETY: a live NSWindow on the main thread. `class_addMethod` only adds
    // the method to GPUI's own window subclass (it fails, harmlessly, when the
    // method is already there) with the signature of
    // `-[NSWindow constrainFrameRect:toScreen:]`; `frame` is a getter and
    // `setFrame:display:` a setter.
    unsafe {
        let class: *const AnyClass = objc2::msg_send![ns_window, class];
        let imp: Imp = std::mem::transmute::<
            extern "C-unwind" fn(*mut AnyObject, Sel, NSRect, *mut AnyObject) -> NSRect,
            Imp,
        >(keep_frame);
        objc2::ffi::class_addMethod(
            class.cast_mut(),
            objc2::sel!(constrainFrameRect:toScreen:),
            imp,
            c"{CGRect={CGPoint=dd}{CGSize=dd}}@:{CGRect={CGPoint=dd}{CGSize=dd}}@".as_ptr(),
        );
        let frame: NSRect = objc2::msg_send![ns_window, frame];
        if frame.size.width != width || frame.size.height == height {
            return Ok(());
        }
        let top = frame.origin.y + frame.size.height;
        let next = NSRect::new(
            NSPoint::new(frame.origin.x, (top - height).max(0.)),
            NSSize::new(width, height),
        );
        let _: () = objc2::msg_send![ns_window, setFrame: next, display: true];
    }
    Ok(())
}

#[cfg(not(target_os = "macos"))]
pub fn undo_clamp(_window: &Window, _width: f64, _height: f64) -> Result<(), String> {
    Ok(())
}

/// The app that had the focus before the self-test activated itself.
pub struct Frontmost(
    #[cfg(target_os = "macos")] Option<objc2::rc::Retained<objc2_app_kit::NSRunningApplication>>,
);

impl Frontmost {
    #[cfg(target_os = "macos")]
    pub fn now() -> Self {
        Self(objc2_app_kit::NSWorkspace::sharedWorkspace().frontmostApplication())
    }

    #[cfg(not(target_os = "macos"))]
    pub fn now() -> Self {
        Self()
    }

    /// Gives the focus back to that app.
    #[cfg(target_os = "macos")]
    pub fn restore(&self) {
        if let Some(app) = &self.0 {
            #[allow(deprecated)] // `activate` needs macOS 14
            app.activateWithOptions(objc2_app_kit::NSApplicationActivationOptions::empty());
        }
    }

    #[cfg(not(target_os = "macos"))]
    pub fn restore(&self) {}
}

/// The window server number of this window (`-[NSWindow windowNumber]`).
#[cfg(target_os = "macos")]
pub fn window_number(window: &Window) -> Result<isize, String> {
    let ns_window = ns_window(window)?;
    // SAFETY: `windowNumber` is a plain getter on a live NSWindow.
    unsafe {
        let number: isize = objc2::msg_send![ns_window, windowNumber];
        if number <= 0 {
            return Err(format!("window number {number} is not on screen"));
        }
        Ok(number)
    }
}

#[cfg(not(target_os = "macos"))]
pub fn window_number(_window: &Window) -> Result<isize, String> {
    Err("the screenshot self-test needs macOS".into())
}

/// Runs `screencapture -l <number> -o -x <png>`: one window, no shadow, no sound.
pub fn capture(number: isize, png: &Path) -> Result<(), String> {
    let output = Command::new("/usr/sbin/screencapture")
        .arg(format!("-l{number}"))
        .args(["-o", "-x"])
        .arg(png)
        .output()
        .map_err(|e| format!("cannot run screencapture: {e}"))?;
    if !output.status.success() {
        return Err(format!(
            "screencapture failed ({}): {}",
            output.status,
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    match std::fs::metadata(png) {
        Ok(meta) if meta.len() > 0 => Ok(()),
        _ => Err(format!(
            "screencapture wrote no image to {} (is Screen Recording allowed for the terminal?)",
            png.display()
        )),
    }
}

/// Pixel width and height from a PNG header.
pub fn png_size(png: &Path) -> Option<(u32, u32)> {
    let bytes = std::fs::read(png).ok()?;
    if bytes.len() < 24 || &bytes[1..4] != b"PNG" {
        return None;
    }
    let w = u32::from_be_bytes(bytes[16..20].try_into().ok()?);
    let h = u32::from_be_bytes(bytes[20..24].try_into().ok()?);
    Some((w, h))
}
