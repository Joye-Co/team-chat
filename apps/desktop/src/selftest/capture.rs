//! Finds the NSWindow number of a GPUI window and captures only that window
//! with `screencapture -l`. Never the full screen.

use std::{path::Path, process::Command};

use gpui::Window;

/// The window server number of this window (`-[NSWindow windowNumber]`).
#[cfg(target_os = "macos")]
pub fn window_number(window: &Window) -> Result<isize, String> {
    use objc2::{msg_send, runtime::AnyObject};
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};

    let handle = HasWindowHandle::window_handle(window).map_err(|e| e.to_string())?;
    let RawWindowHandle::AppKit(appkit) = handle.as_raw() else {
        return Err("not an AppKit window".into());
    };
    let view = appkit.ns_view.as_ptr().cast::<AnyObject>();
    // SAFETY: GPUI's AppKit handle points at the window's live content NSView;
    // `window` and `windowNumber` are plain getters on the main thread.
    unsafe {
        let ns_window: *mut AnyObject = msg_send![view, window];
        if ns_window.is_null() {
            return Err("the view has no window".into());
        }
        let number: isize = msg_send![ns_window, windowNumber];
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
