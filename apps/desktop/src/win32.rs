//! Win32 helpers shared by the Windows system integration: wide strings, the HWND of a
//! GPUI window, and one hidden top-level window on the main thread that receives what
//! Windows sends to an app rather than to a visible window (the notification-area icon's
//! clicks, `WM_SETTINGCHANGE`, the taskbar being created again).
//!
//! GPUI's message loop dispatches every message of the main thread, so this window's
//! procedure runs inside it. Handlers must not touch GPUI state: they send on a channel
//! that a GPUI task drains.

use std::{cell::RefCell, rc::Rc};

use windows::{
    Win32::{
        Foundation::{HWND, LPARAM, LRESULT, WPARAM},
        System::LibraryLoader::GetModuleHandleW,
        UI::WindowsAndMessaging::{
            CreateWindowExW, DefWindowProcW, RegisterClassW, WINDOW_EX_STYLE, WNDCLASSW,
            WS_OVERLAPPED,
        },
    },
    core::{PCWSTR, w},
};

/// A NUL-terminated UTF-16 copy of `s`.
pub fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

/// The HWND of a GPUI window.
pub fn hwnd(window: &gpui::Window) -> Option<HWND> {
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    let handle = HasWindowHandle::window_handle(window).ok()?;
    match handle.as_raw() {
        RawWindowHandle::Win32(win32) => Some(HWND(win32.hwnd.get() as *mut _)),
        _ => None,
    }
}

type Handler = Rc<dyn Fn(HWND, u32, WPARAM, LPARAM)>;

thread_local! {
    static HANDLERS: RefCell<Vec<Handler>> = const { RefCell::new(Vec::new()) };
    static WINDOW: RefCell<Option<isize>> = const { RefCell::new(None) };
}

/// Calls `handler` for every message the events window gets. Main thread only (handlers
/// registered on another thread are never called).
pub fn on_message(handler: impl Fn(HWND, u32, WPARAM, LPARAM) + 'static) {
    HANDLERS.with(|h| h.borrow_mut().push(Rc::new(handler)));
}

unsafe extern "system" fn events_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    // A handler may pump messages (a menu's loop), which re-enters this procedure: call
    // them from a copy so the list is not borrowed meanwhile.
    let handlers: Vec<Handler> = HANDLERS.with(|h| h.borrow().clone());
    for handler in handlers {
        handler(hwnd, msg, wparam, lparam);
    }
    // SAFETY: the arguments are the ones this procedure was called with.
    unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
}

/// The hidden events window of this thread, created on first use. Call on the main
/// thread. It is a top-level window (never shown), not a message-only one: only
/// top-level windows get broadcasts such as `WM_SETTINGCHANGE`.
pub fn events_window() -> Option<HWND> {
    if let Some(raw) = WINDOW.with(|w| *w.borrow()) {
        return Some(HWND(raw as *mut _));
    }
    let class: PCWSTR = w!("DesktopSystemEvents");
    // SAFETY: the class and its procedure live for the whole process; the window is
    // created and used on this thread only.
    let hwnd = unsafe {
        let instance = GetModuleHandleW(None).ok()?;
        let wc = WNDCLASSW {
            lpfnWndProc: Some(events_proc),
            hInstance: instance.into(),
            lpszClassName: class,
            ..Default::default()
        };
        // Zero when the class exists already; creating the window then still works.
        RegisterClassW(&wc);
        CreateWindowExW(
            WINDOW_EX_STYLE(0),
            class,
            PCWSTR::null(),
            WS_OVERLAPPED,
            0,
            0,
            0,
            0,
            None,
            None,
            Some(instance.into()),
            None,
        )
        .ok()?
    };
    WINDOW.with(|w| *w.borrow_mut() = Some(hwnd.0 as isize));
    Some(hwnd)
}
