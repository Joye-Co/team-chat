//! Captures only the app's own window, never the full screen. On macOS it
//! finds the NSWindow number of a GPUI window and runs `screencapture -l`; on
//! Windows it asks the compositor for the window's pixels (`PrintWindow` with
//! `PW_RENDERFULLCONTENT`, which includes a DirectX swap chain) and writes
//! the client area as a PNG.

use std::path::Path;
#[cfg(target_os = "macos")]
use std::process::Command;

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

/// Shows the window above the others without activating it: the compositor
/// keeps no pixels of a hidden window.
#[cfg(target_os = "windows")]
pub fn order_front(window: &Window) -> Result<(), String> {
    use windows::Win32::UI::WindowsAndMessaging::{
        HWND_TOP, SW_SHOWNOACTIVATE, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SetWindowPos,
        ShowWindow,
    };

    let hwnd = hwnd(window)?;
    // SAFETY: a live HWND of this process, on its own thread.
    unsafe {
        let _ = ShowWindow(hwnd, SW_SHOWNOACTIVATE);
        SetWindowPos(
            hwnd,
            Some(HWND_TOP),
            0,
            0,
            0,
            0,
            SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
        )
        .map_err(|e| format!("SetWindowPos failed: {e}"))
    }
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
pub fn order_front(_window: &Window) -> Result<(), String> {
    Ok(())
}

#[cfg(target_os = "windows")]
fn hwnd(window: &Window) -> Result<windows::Win32::Foundation::HWND, String> {
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};

    let handle = HasWindowHandle::window_handle(window).map_err(|e| e.to_string())?;
    let RawWindowHandle::Win32(win32) = handle.as_raw() else {
        return Err("not a Win32 window".into());
    };
    Ok(windows::Win32::Foundation::HWND(
        win32.hwnd.get() as *mut core::ffi::c_void
    ))
}

/// Gives a window that opened at the requested `width` exactly `height`, with
/// its top edge kept (a new window can come 1 pt taller than asked). AppKit shrinks a window
/// to the screen's visible frame, which the Dock shortens (887 pt on a 982 pt
/// display), in `constrainFrameRect:toScreen:`. The screenshot process adds
/// an override to the window's class that keeps the frame as asked: the Dock
/// does not show in a capture of one window. With `own_size`, the size is the
/// window's own (settings, 960 x 720) and only the extra point goes.
#[cfg(target_os = "macos")]
pub fn undo_clamp(window: &Window, width: f64, height: f64, own_size: bool) -> Result<(), String> {
    use objc2::runtime::AnyClass;
    use objc2_foundation::{NSPoint, NSRect, NSSize};

    let ns_window = ns_window(window)?;
    // SAFETY: a live NSWindow on the main thread. `class` and `frame` are
    // getters and `setFrame:display:` a setter.
    unsafe {
        let class: *const AnyClass = objc2::msg_send![ns_window, class];
        keep_frames_of(class);
        let frame: NSRect = objc2::msg_send![ns_window, frame];
        if frame.size.width != width || frame.size.height == height {
            return Ok(());
        }
        // A window with its own size (settings) only loses the extra point.
        if own_size && (frame.size.height - height).abs() > 1. {
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

/// Adds the frame-keeping `constrainFrameRect:toScreen:` to GPUI's window
/// class before the first window opens, so every capture window opens at
/// its requested size. A window that AppKit shrank and the self-test then
/// grew could be captured before its Metal layer drew at the new size: a
/// black band at the bottom, over the rounded corners.
#[cfg(target_os = "macos")]
pub fn keep_frames() {
    if let Some(class) = objc2::runtime::AnyClass::get(c"GPUIWindow") {
        // SAFETY: GPUI's own NSWindow subclass, on the main thread.
        unsafe { keep_frames_of(class) }
    }
}

#[cfg(not(target_os = "macos"))]
pub fn keep_frames() {}

/// SAFETY: `class` is a live NSWindow subclass; call on the main thread.
/// `class_addMethod` only adds the method to that class (it fails,
/// harmlessly, when the method is already there) with the signature of
/// `-[NSWindow constrainFrameRect:toScreen:]`.
#[cfg(target_os = "macos")]
unsafe fn keep_frames_of(class: *const objc2::runtime::AnyClass) {
    use objc2::runtime::{AnyObject, Imp, Sel};
    use objc2_foundation::NSRect;

    extern "C-unwind" fn keep_frame(
        _: *mut AnyObject,
        _: Sel,
        rect: NSRect,
        _: *mut AnyObject,
    ) -> NSRect {
        rect
    }

    unsafe {
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
    }
}

#[cfg(not(target_os = "macos"))]
pub fn undo_clamp(
    _window: &Window,
    _width: f64,
    _height: f64,
    _own_size: bool,
) -> Result<(), String> {
    Ok(())
}

/// Draws the window as the key window without making it key: the app stays
/// inactive and the focus stays with the app the person uses. Fields hide
/// their caret and selection in an inactive window (gpui-component checks
/// `isKeyWindow` through GPUI), so a capture of a focused field would lose
/// both. In the screenshot process only, GPUI's window class answers
/// `isKeyWindow` with YES, and its delegate hears `windowDidBecomeKey:`.
/// The window server's key window does not change.
#[cfg(target_os = "macos")]
pub fn draw_as_key(window: &Window) -> Result<(), String> {
    use objc2::runtime::{AnyClass, AnyObject, Bool, Imp, Sel};

    extern "C-unwind" fn yes(_: *mut AnyObject, _: Sel) -> Bool {
        Bool::YES
    }

    let ns_window = ns_window(window)?;
    // SAFETY: a live NSWindow on the main thread. `class_addMethod` only adds
    // the method to GPUI's own window subclass (it fails, harmlessly, when it
    // is already there) with the signature of `-[NSWindow isKeyWindow]`;
    // `delegate` is a getter and GPUI's delegate implements
    // `windowDidBecomeKey:`, which takes a notification it does not read.
    unsafe {
        let class: *const AnyClass = objc2::msg_send![ns_window, class];
        let imp: Imp =
            std::mem::transmute::<extern "C-unwind" fn(*mut AnyObject, Sel) -> Bool, Imp>(yes);
        objc2::ffi::class_addMethod(
            class.cast_mut(),
            objc2::sel!(isKeyWindow),
            imp,
            c"B@:".as_ptr(),
        );
        let delegate: *mut AnyObject = objc2::msg_send![ns_window, delegate];
        if !delegate.is_null() {
            let none: *mut AnyObject = std::ptr::null_mut();
            let _: () = objc2::msg_send![delegate, windowDidBecomeKey: none];
        }
    }
    Ok(())
}

#[cfg(not(target_os = "macos"))]
pub fn draw_as_key(_window: &Window) -> Result<(), String> {
    Ok(())
}

/// The window's frame size in points.
#[cfg(target_os = "macos")]
pub fn frame_size(window: &Window) -> Result<(f64, f64), String> {
    let ns_window = ns_window(window)?;
    // SAFETY: `frame` is a plain getter on a live NSWindow.
    let frame: objc2_foundation::NSRect = unsafe { objc2::msg_send![ns_window, frame] };
    Ok((frame.size.width, frame.size.height))
}

#[cfg(not(target_os = "macos"))]
pub fn frame_size(_window: &Window) -> Result<(f64, f64), String> {
    Ok((0., 0.))
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

/// The window's HWND, as the number [`capture`] takes.
#[cfg(target_os = "windows")]
pub fn window_number(window: &Window) -> Result<isize, String> {
    Ok(hwnd(window)?.0 as isize)
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
pub fn window_number(_window: &Window) -> Result<isize, String> {
    Err("the screenshot self-test needs macOS or Windows".into())
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
pub fn capture(_number: isize, _png: &Path) -> Result<(), String> {
    Err("the screenshot self-test needs macOS or Windows".into())
}

/// Writes the client area of the window `number` (an HWND) to `png`.
///
/// `PrintWindow` with `PW_RENDERFULLCONTENT` copies what the compositor holds
/// for the window, so it works for GPUI's DirectX swap chain, for a window
/// that other windows cover and for one that is larger than the screen. Plain
/// GDI (`BitBlt`, `PrintWindow` without the flag) returns black for it.
#[cfg(target_os = "windows")]
pub fn capture(number: isize, png: &Path) -> Result<(), String> {
    use windows::Win32::{
        Foundation::{HWND, POINT, RECT},
        Graphics::Gdi::{
            BI_RGB, BITMAPINFO, BITMAPINFOHEADER, ClientToScreen, CreateCompatibleDC,
            CreateDIBSection, DIB_RGB_COLORS, DeleteDC, DeleteObject, GdiFlush, GetDC, ReleaseDC,
            SelectObject,
        },
        Storage::Xps::{PRINT_WINDOW_FLAGS, PrintWindow},
        UI::WindowsAndMessaging::{GetClientRect, GetWindowRect},
    };

    const PW_RENDERFULLCONTENT: PRINT_WINDOW_FLAGS = PRINT_WINDOW_FLAGS(2);

    let hwnd = HWND(number as *mut core::ffi::c_void);
    // SAFETY: plain Win32 calls on a live HWND of this process. The DIB
    // section's memory (`bits`, `width * height * 4` bytes) lives until
    // `DeleteObject`, after the copy.
    let (frame, client, pixels) = unsafe {
        let mut frame = RECT::default();
        GetWindowRect(hwnd, &mut frame).map_err(|e| format!("GetWindowRect failed: {e}"))?;
        let mut client = RECT::default();
        GetClientRect(hwnd, &mut client).map_err(|e| format!("GetClientRect failed: {e}"))?;
        let mut origin = POINT::default();
        let _ = ClientToScreen(hwnd, &mut origin);
        let (width, height) = (frame.right - frame.left, frame.bottom - frame.top);
        if width <= 0 || height <= 0 {
            return Err(format!("the window is {width} x {height} px"));
        }
        let info = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: width,
                // Negative: rows from the top.
                biHeight: -height,
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };
        let screen = GetDC(None);
        let memory = CreateCompatibleDC(Some(screen));
        let mut bits: *mut core::ffi::c_void = std::ptr::null_mut();
        let bitmap = CreateDIBSection(Some(memory), &info, DIB_RGB_COLORS, &mut bits, None, 0);
        let result = match bitmap {
            Ok(bitmap) if !bits.is_null() => {
                let old = SelectObject(memory, bitmap.into());
                let printed = PrintWindow(hwnd, memory, PW_RENDERFULLCONTENT).as_bool();
                let _ = GdiFlush();
                let pixels = std::slice::from_raw_parts(
                    bits.cast::<u8>(),
                    width as usize * height as usize * 4,
                )
                .to_vec();
                SelectObject(memory, old);
                let _ = DeleteObject(bitmap.into());
                if printed {
                    Ok(pixels)
                } else {
                    Err("PrintWindow failed".to_string())
                }
            }
            Ok(bitmap) => {
                let _ = DeleteObject(bitmap.into());
                Err("CreateDIBSection gave no memory".to_string())
            }
            Err(e) => Err(format!("CreateDIBSection failed: {e}")),
        };
        let _ = DeleteDC(memory);
        ReleaseDC(None, screen);
        (
            (width as u32, height as u32),
            (
                (origin.x - frame.left).max(0) as u32,
                (origin.y - frame.top).max(0) as u32,
                (client.right - client.left).max(0) as u32,
                (client.bottom - client.top).max(0) as u32,
            ),
            result?,
        )
    };
    let mut image = image::RgbaImage::from_raw(frame.0, frame.1, pixels)
        .ok_or("the capture has the wrong size")?;
    // BGRA with an undefined alpha to opaque RGBA.
    for pixel in image.pixels_mut() {
        pixel.0 = [pixel.0[2], pixel.0[1], pixel.0[0], 255];
    }
    // The window rectangle includes the invisible resize border; keep the
    // client area, which is all GPUI draws.
    let (x, y, w, h) = client;
    let (w, h) = (
        w.min(frame.0.saturating_sub(x)),
        h.min(frame.1.saturating_sub(y)),
    );
    if w == 0 || h == 0 {
        return Err("the window has no client area".into());
    }
    let image = image::imageops::crop_imm(&image, x, y, w, h).to_image();
    let first = *image.get_pixel(0, 0);
    if image.pixels().all(|p| *p == first) {
        return Err(format!(
            "the capture is one flat colour ({:?}): the compositor has no frame of the window",
            &first.0[..3]
        ));
    }
    image
        .save(png)
        .map_err(|e| format!("cannot write {}: {e}", png.display()))
}

/// Runs `screencapture -l <number> -o -x <png>`: one window, no shadow, no sound.
#[cfg(target_os = "macos")]
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
    to_srgb(png)?;
    match std::fs::metadata(png) {
        Ok(meta) if meta.len() > 0 => Ok(()),
        _ => Err(format!(
            "screencapture wrote no image to {} (is Screen Recording allowed for the terminal?)",
            png.display()
        )),
    }
}

/// Converts a capture from the display's profile (Display P3 on a Mac
/// screen) to sRGB, the space of the theme tokens and the mockups, so pixel
/// values read from the PNG are the token values (`#2F6FEB` stays `#2F6FEB`).
#[cfg(target_os = "macos")]
fn to_srgb(png: &Path) -> Result<(), String> {
    const SRGB: &str = "/System/Library/ColorSync/Profiles/sRGB Profile.icc";
    let output = Command::new("/usr/bin/sips")
        .args(["--matchTo", SRGB])
        .arg(png)
        .output()
        .map_err(|e| format!("cannot run sips: {e}"))?;
    if !output.status.success() {
        return Err(format!(
            "sips could not convert {} to sRGB: {}",
            png.display(),
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    Ok(())
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

/// Whether two captures hold the same pixels (the state-scene check).
pub fn same_pixels(a: &Path, b: &Path) -> Result<bool, String> {
    let read = |p: &Path| {
        image::open(p)
            .map(|i| i.to_rgba8())
            .map_err(|e| format!("{}: {e}", p.display()))
    };
    let (a, b) = (read(a)?, read(b)?);
    Ok(a.dimensions() == b.dimensions() && a.as_raw() == b.as_raw())
}
