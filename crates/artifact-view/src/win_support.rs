//! Helpers for the Windows examples and self-tests: a message pump with a deadline, a plain
//! host window, and a PNG of a part of the screen. Not part of the crate's API.

use std::sync::atomic::{AtomicI32, Ordering};
use std::time::{Duration, Instant};

use windows::Win32::Foundation::{COLORREF, HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::{
    BI_RGB, BITMAPINFO, BITMAPINFOHEADER, BitBlt, CAPTUREBLT, ClientToScreen,
    CreateCompatibleBitmap, CreateCompatibleDC, CreateSolidBrush, DIB_RGB_COLORS, DeleteDC,
    DeleteObject, GetDC, GetDIBits, ReleaseDC, SRCCOPY, SelectObject,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::HiDpi::{
    DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, GetDpiForWindow, SetProcessDpiAwarenessContext,
};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    INPUT, INPUT_0, INPUT_KEYBOARD, INPUT_MOUSE, KEYBD_EVENT_FLAGS, KEYBDINPUT, KEYEVENTF_KEYUP,
    MOUSE_EVENT_FLAGS, MOUSEEVENTF_ABSOLUTE, MOUSEEVENTF_LEFTDOWN, MOUSEEVENTF_LEFTUP,
    MOUSEEVENTF_MOVE, MOUSEEVENTF_VIRTUALDESK, MOUSEEVENTF_WHEEL, MOUSEINPUT, SendInput,
    VIRTUAL_KEY,
};
use windows::Win32::UI::WindowsAndMessaging::{
    AdjustWindowRectEx, CreateWindowExW, DefWindowProcW, DispatchMessageW, GetClientRect,
    IDC_ARROW, LoadCursorW, MSG, MsgWaitForMultipleObjects, PM_REMOVE, PeekMessageW, QS_ALLINPUT,
    RegisterClassW, SW_SHOWNORMAL, SetForegroundWindow, ShowWindow, TranslateMessage,
    WINDOW_EX_STYLE, WM_QUIT, WNDCLASSW, WS_CLIPCHILDREN, WS_OVERLAPPEDWINDOW,
};
use windows::Win32::UI::WindowsAndMessaging::{
    FindWindowW, GetSystemMetrics, SM_CXVIRTUALSCREEN, SM_CYVIRTUALSCREEN, SM_XVIRTUALSCREEN,
    SM_YVIRTUALSCREEN, WM_MOUSEWHEEL,
};
use windows::core::{HSTRING, PCWSTR};

/// Makes the process per-monitor DPI aware, as the desktop app is. Call it before any
/// window exists.
pub fn set_dpi_aware() {
    // SAFETY: a plain process setting; it fails harmlessly when it was set before.
    unsafe {
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
    }
}

/// Runs the thread's message loop until `done` returns true or `timeout` passes. Returns
/// whether `done` became true.
pub fn pump(timeout: Duration, mut done: impl FnMut() -> bool) -> bool {
    let deadline = Instant::now() + timeout;
    loop {
        let mut msg = MSG::default();
        // SAFETY: the standard message loop on the calling thread.
        unsafe {
            while PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE).as_bool() {
                if msg.message == WM_QUIT {
                    return done();
                }
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        }
        if done() {
            return true;
        }
        let now = Instant::now();
        if now >= deadline {
            return false;
        }
        let wait = (deadline - now).min(Duration::from_millis(20));
        // SAFETY: waits for a message or the timeout; no handles are passed.
        unsafe {
            MsgWaitForMultipleObjects(None, false, wait.as_millis() as u32, QS_ALLINPUT);
        }
    }
}

/// Wheel messages the host windows of [`create_host_window`] got: how many, and the sum of
/// their deltas.
static WHEEL_COUNT: AtomicI32 = AtomicI32::new(0);
static WHEEL_DELTA: AtomicI32 = AtomicI32::new(0);

unsafe extern "system" fn host_proc(hwnd: HWND, msg: u32, w: WPARAM, l: LPARAM) -> LRESULT {
    if msg == WM_MOUSEWHEEL {
        WHEEL_COUNT.fetch_add(1, Ordering::Relaxed);
        WHEEL_DELTA.fetch_add(
            ((w.0 >> 16) & 0xffff) as u16 as i16 as i32,
            Ordering::Relaxed,
        );
        return LRESULT(0);
    }
    // SAFETY: forwards to the default procedure with the arguments Windows gave.
    unsafe { DefWindowProcW(hwnd, msg, w, l) }
}

/// `(count, sum of deltas)` of the wheel messages the test host windows got so far.
pub fn host_wheel() -> (i32, i32) {
    (
        WHEEL_COUNT.load(Ordering::Relaxed),
        WHEEL_DELTA.load(Ordering::Relaxed),
    )
}

fn mouse_input(x: i32, y: i32, flags: MOUSE_EVENT_FLAGS, data: i32) -> INPUT {
    // SAFETY: plain system metrics.
    let (left, top, width, height) = unsafe {
        (
            GetSystemMetrics(SM_XVIRTUALSCREEN),
            GetSystemMetrics(SM_YVIRTUALSCREEN),
            GetSystemMetrics(SM_CXVIRTUALSCREEN).max(1),
            GetSystemMetrics(SM_CYVIRTUALSCREEN).max(1),
        )
    };
    // Absolute coordinates run from 0 to 65535 over the virtual screen.
    let normalize = |v: i32, origin: i32, size: i32| {
        (((v - origin) as f64 + 0.5) * 65535.0 / size as f64).round() as i32
    };
    INPUT {
        r#type: INPUT_MOUSE,
        Anonymous: INPUT_0 {
            mi: MOUSEINPUT {
                dx: normalize(x, left, width),
                dy: normalize(y, top, height),
                mouseData: data as u32,
                dwFlags: flags | MOUSEEVENTF_MOVE | MOUSEEVENTF_ABSOLUTE | MOUSEEVENTF_VIRTUALDESK,
                time: 0,
                dwExtraInfo: 0,
            },
        },
    }
}

fn send(inputs: &[INPUT]) -> bool {
    // SAFETY: synthetic input on the calling desktop.
    unsafe { SendInput(inputs, std::mem::size_of::<INPUT>() as i32) == inputs.len() as u32 }
}

/// Clicks the left mouse button at a screen position through the system's input queue
/// (the pointer moves there), as the viewer's own click.
pub fn click_at_screen(x: i32, y: i32) -> bool {
    send(&[
        mouse_input(x, y, MOUSE_EVENT_FLAGS(0), 0),
        mouse_input(x, y, MOUSEEVENTF_LEFTDOWN, 0),
        mouse_input(x, y, MOUSEEVENTF_LEFTUP, 0),
    ])
}

/// Turns the wheel by `delta` (120 is one notch away from the user) with the pointer at a
/// screen position, through the system's input queue.
pub fn wheel_at_screen(x: i32, y: i32, delta: i32) -> bool {
    send(&[
        mouse_input(x, y, MOUSE_EVENT_FLAGS(0), 0),
        mouse_input(x, y, MOUSEEVENTF_WHEEL, delta),
    ])
}

/// Presses and releases one key (a virtual-key code) through the system's input queue.
pub fn press_key(virtual_key: u16) -> bool {
    let key = |flags| INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: VIRTUAL_KEY(virtual_key),
                wScan: 0,
                dwFlags: flags,
                time: 0,
                dwExtraInfo: 0,
            },
        },
    };
    send(&[key(KEYBD_EVENT_FLAGS(0)), key(KEYEVENTF_KEYUP)])
}

/// Presses `virtual_key` while `modifier` (a virtual-key code such as Ctrl's 0x11) is held.
pub fn press_chord(modifier: u16, virtual_key: u16) -> bool {
    let key = |vk, flags| INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: VIRTUAL_KEY(vk),
                wScan: 0,
                dwFlags: flags,
                time: 0,
                dwExtraInfo: 0,
            },
        },
    };
    send(&[
        key(modifier, KEYBD_EVENT_FLAGS(0)),
        key(virtual_key, KEYBD_EVENT_FLAGS(0)),
        key(virtual_key, KEYEVENTF_KEYUP),
        key(modifier, KEYEVENTF_KEYUP),
    ])
}

/// The top-level window of this thread with exactly this title (0 when there is none).
pub fn find_window(title: &str) -> isize {
    // SAFETY: a lookup by title.
    unsafe { FindWindowW(PCWSTR::null(), &HSTRING::from(title)) }
        .map(|h| h.0 as isize)
        .unwrap_or(0)
}

/// Opens a plain top-level window with a solid background (`0x00BBGGRR`) and a client area
/// of `width` × `height` pixels at the top left of the screen, and brings it to the front.
/// Returns its handle (0 on failure).
pub fn create_host_window(title: &str, width: i32, height: i32, color: u32) -> isize {
    // SAFETY: registers a window class and creates a window on the calling thread.
    unsafe {
        let Ok(module) = GetModuleHandleW(None) else {
            return 0;
        };
        let class = HSTRING::from(format!("ArtifactViewTestHost{color:06x}"));
        let wc = WNDCLASSW {
            lpfnWndProc: Some(host_proc),
            hInstance: module.into(),
            lpszClassName: PCWSTR(class.as_ptr()),
            hCursor: LoadCursorW(None, IDC_ARROW).unwrap_or_default(),
            hbrBackground: CreateSolidBrush(COLORREF(color)),
            ..Default::default()
        };
        RegisterClassW(&wc);
        let style = WS_OVERLAPPEDWINDOW | WS_CLIPCHILDREN;
        let mut frame = RECT {
            left: 0,
            top: 0,
            right: width,
            bottom: height,
        };
        let _ = AdjustWindowRectEx(&mut frame, style, false, WINDOW_EX_STYLE(0));
        let Ok(hwnd) = CreateWindowExW(
            WINDOW_EX_STYLE(0),
            PCWSTR(class.as_ptr()),
            &HSTRING::from(title),
            style,
            40,
            40,
            frame.right - frame.left,
            frame.bottom - frame.top,
            None,
            None,
            Some(module.into()),
            None,
        ) else {
            return 0;
        };
        let _ = ShowWindow(hwnd, SW_SHOWNORMAL);
        let _ = SetForegroundWindow(hwnd);
        hwnd.0 as isize
    }
}

/// Pixels per point of the window (1.0 at 96 dpi).
pub fn scale_of(hwnd: isize) -> f64 {
    // SAFETY: a query on a window handle; 0 for a dead window.
    let dpi = unsafe { GetDpiForWindow(HWND(hwnd as *mut _)) };
    if dpi == 0 { 1.0 } else { dpi as f64 / 96.0 }
}

/// The client area of the window in screen pixels: `(x, y, width, height)`.
pub fn client_rect_on_screen(hwnd: isize) -> (i32, i32, i32, i32) {
    let hwnd = HWND(hwnd as *mut _);
    let mut rect = RECT::default();
    let mut origin = POINT::default();
    // SAFETY: queries on a window handle with valid out-pointers.
    unsafe {
        let _ = GetClientRect(hwnd, &mut rect);
        let _ = ClientToScreen(hwnd, &mut origin);
    }
    (origin.x, origin.y, rect.right, rect.bottom)
}

/// The size of the primary screen in pixels.
pub fn screen_size() -> (i32, i32) {
    // SAFETY: plain system queries.
    unsafe {
        use windows::Win32::UI::WindowsAndMessaging::{SM_CXSCREEN, SM_CYSCREEN};
        (GetSystemMetrics(SM_CXSCREEN), GetSystemMetrics(SM_CYSCREEN))
    }
}

/// A PNG of a rectangle of the screen (what the user sees, with every window composed).
pub fn capture_screen_png(x: i32, y: i32, width: i32, height: i32) -> Option<Vec<u8>> {
    if width <= 0 || height <= 0 {
        return None;
    }
    let mut bgra = vec![0u8; width as usize * height as usize * 4];
    // SAFETY: GDI calls with handles created here and released before returning; the
    // buffer is as large as the 32-bit bitmap asked for.
    let lines = unsafe {
        let screen = GetDC(None);
        let mem = CreateCompatibleDC(Some(screen));
        let bitmap = CreateCompatibleBitmap(screen, width, height);
        let old = SelectObject(mem, bitmap.into());
        let _ = BitBlt(
            mem,
            0,
            0,
            width,
            height,
            Some(screen),
            x,
            y,
            SRCCOPY | CAPTUREBLT,
        );
        SelectObject(mem, old);
        let mut info = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: width,
                biHeight: -height,
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };
        let lines = GetDIBits(
            mem,
            bitmap,
            0,
            height as u32,
            Some(bgra.as_mut_ptr().cast()),
            &mut info,
            DIB_RGB_COLORS,
        );
        let _ = DeleteObject(bitmap.into());
        let _ = DeleteDC(mem);
        ReleaseDC(None, screen);
        lines
    };
    if lines == 0 {
        return None;
    }
    for px in bgra.chunks_exact_mut(4) {
        px.swap(0, 2);
        px[3] = 255;
    }
    Some(encode_png(width as u32, height as u32, &bgra))
}

/// The color of one pixel of an RGBA PNG made by [`encode_png`]: `(r, g, b)`.
pub fn png_pixel(png: &[u8], x: u32, y: u32) -> Option<(u8, u8, u8)> {
    let width = u32::from_be_bytes(png.get(16..20)?.try_into().ok()?);
    let height = u32::from_be_bytes(png.get(20..24)?.try_into().ok()?);
    if x >= width || y >= height {
        return None;
    }
    // One IDAT with stored blocks: 8 (signature) + 25 (IHDR) + 8 (IDAT header) + 2 (zlib).
    let row = width as usize * 4 + 1;
    let raw_at = y as usize * row + 1 + x as usize * 4;
    let data = png.get(8 + 25 + 8 + 2..)?;
    let at = raw_at + (raw_at / 65535 + 1) * 5;
    let p = data.get(at..at + 3)?;
    Some((p[0], p[1], p[2]))
}

/// Encodes RGBA pixels as a PNG without compression (stored deflate blocks).
pub fn encode_png(width: u32, height: u32, rgba: &[u8]) -> Vec<u8> {
    let row = width as usize * 4;
    let mut raw = Vec::with_capacity((row + 1) * height as usize);
    for line in rgba.chunks_exact(row).take(height as usize) {
        raw.push(0);
        raw.extend_from_slice(line);
    }
    let mut zlib = vec![0x78, 0x01];
    let mut blocks = raw.chunks(65535).peekable();
    if blocks.peek().is_none() {
        zlib.extend_from_slice(&[1, 0, 0, 0xff, 0xff]);
    }
    while let Some(block) = blocks.next() {
        let len = block.len() as u16;
        zlib.push(u8::from(blocks.peek().is_none()));
        zlib.extend_from_slice(&len.to_le_bytes());
        zlib.extend_from_slice(&(!len).to_le_bytes());
        zlib.extend_from_slice(block);
    }
    let (mut a, mut b) = (1u32, 0u32);
    for &byte in &raw {
        a = (a + byte as u32) % 65521;
        b = (b + a) % 65521;
    }
    zlib.extend_from_slice(&((b << 16) | a).to_be_bytes());

    let mut out = vec![0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a];
    let mut header = Vec::with_capacity(13);
    header.extend_from_slice(&width.to_be_bytes());
    header.extend_from_slice(&height.to_be_bytes());
    header.extend_from_slice(&[8, 6, 0, 0, 0]);
    chunk(&mut out, b"IHDR", &header);
    chunk(&mut out, b"IDAT", &zlib);
    chunk(&mut out, b"IEND", &[]);
    out
}

fn chunk(out: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]) {
    out.extend_from_slice(&(data.len() as u32).to_be_bytes());
    out.extend_from_slice(kind);
    out.extend_from_slice(data);
    let mut crc = 0xffff_ffffu32;
    for &byte in kind.iter().chain(data) {
        crc ^= byte as u32;
        for _ in 0..8 {
            crc = if crc & 1 == 1 {
                (crc >> 1) ^ 0xedb8_8320
            } else {
                crc >> 1
            };
        }
    }
    out.extend_from_slice(&(!crc).to_be_bytes());
}

/// Decodes standard base64 (with or without padding). `None` on any other character.
pub fn decode_base64(text: &str) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(text.len() / 4 * 3);
    let (mut acc, mut bits) = (0u32, 0u32);
    for c in text.bytes() {
        let v = match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            b'=' | b'\r' | b'\n' => continue,
            _ => return None,
        };
        acc = (acc << 6) | v as u32;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
            acc &= (1 << bits) - 1;
        }
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_round_trip() {
        assert_eq!(decode_base64("aGVsbG8=").unwrap(), b"hello");
        assert_eq!(decode_base64("aGVsbG8h").unwrap(), b"hello!");
        assert_eq!(decode_base64("").unwrap(), b"");
        assert_eq!(decode_base64("a b"), None);
    }

    #[test]
    fn png_keeps_pixels() {
        let (w, h) = (300u32, 200u32);
        let mut rgba = vec![0u8; (w * h * 4) as usize];
        for y in 0..h {
            for x in 0..w {
                let i = ((y * w + x) * 4) as usize;
                rgba[i] = x as u8;
                rgba[i + 1] = y as u8;
                rgba[i + 2] = (x + y) as u8;
                rgba[i + 3] = 255;
            }
        }
        let png = encode_png(w, h, &rgba);
        assert_eq!(&png[1..4], b"PNG");
        for (x, y) in [(0, 0), (299, 199), (17, 150), (255, 54), (100, 55)] {
            assert_eq!(
                png_pixel(&png, x, y),
                Some((x as u8, y as u8, (x + y) as u8)),
                "{x},{y}"
            );
        }
        assert_eq!(png_pixel(&png, 300, 0), None);
    }
}
