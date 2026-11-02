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
