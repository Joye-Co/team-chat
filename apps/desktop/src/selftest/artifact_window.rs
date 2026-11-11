//! Screenshots of the real 产出 window on Windows, in the real app:
//!
//! ```text
//! desktop --artifact-window <dir>
//! ```
//!
//! The 产出 window is not a GPUI window (crates/artifact-view), so the scene loop of
//! the screenshot self-test cannot capture it. This mode opens it the way the app does
//! (`views::artifacts::window`), in the light and in the dark theme, and writes what is
//! on screen: `artifact-window-<theme>.png` (title bar in the theme's colors, the app's
//! icon, the version picker, the page) and `artifact-window-<theme>-versions.png` (the
//! picker's list open). Exit code 0 when all four were written.

use std::{path::PathBuf, time::Duration};

use gpui::{App, AsyncApp};
use gpui_component::ThemeMode;
use windows::{
    Win32::{
        Foundation::{HWND, LPARAM, RECT, WPARAM},
        Graphics::Dwm::{DWMWA_EXTENDED_FRAME_BOUNDS, DwmGetWindowAttribute},
        UI::WindowsAndMessaging::{
            CB_SHOWDROPDOWN, FindWindowExW, FindWindowW, SendMessageW, SetForegroundWindow,
        },
    },
    core::{HSTRING, PCWSTR, w},
};

use crate::theme::{self, ThemeChoice};

/// The sample window's title, which is how this mode finds it.
pub const TITLE: &str = "10 月数据月报";

/// Runs the mode when the arguments ask for it and returns its exit code.
pub fn main_if_requested(args: &[String]) -> Option<i32> {
    let at = args.iter().position(|a| a == "--artifact-window")?;
    let Some(dir) = args.get(at + 1).map(PathBuf::from) else {
        eprintln!("usage: desktop --artifact-window <dir>");
        return Some(2);
    };
    if let Err(error) = std::fs::create_dir_all(&dir) {
        eprintln!("cannot create {}: {error}", dir.display());
        return Some(1);
    }
    gpui_kit::application()
        .with_assets(crate::ui::AppAssets)
        .run(move |cx: &mut App| {
            gpui_kit::init(cx);
            cx.set_quit_mode(gpui::QuitMode::Explicit);
            cx.spawn(async move |cx| {
                let mut ok = true;
                for (choice, mode, name) in [
                    (ThemeChoice::Light, ThemeMode::Light, "light"),
                    (ThemeChoice::Dark, ThemeMode::Dark, "dark"),
                ] {
                    if let Err(error) = shoot(choice, mode, name, &dir, cx).await {
                        eprintln!("artifact-window-{name}: FAILED: {error}");
                        ok = false;
                    }
                }
                std::process::exit(if ok { 0 } else { 1 });
            })
            .detach();
        });
    Some(0)
}

async fn wait(cx: &mut AsyncApp, ms: u64) {
    cx.background_executor()
        .timer(Duration::from_millis(ms))
        .await;
}

async fn shoot(
    choice: ThemeChoice,
    mode: ThemeMode,
    name: &str,
    dir: &std::path::Path,
    cx: &mut AsyncApp,
) -> Result<(), String> {
    cx.update(|cx| {
        theme::set_theme_choice(choice);
        theme::set_mode(mode, cx);
        crate::views::artifacts::window::open_sample(TITLE, cx);
    });
    // The window, then its page (the web view starts its own processes the first time).
    let mut hwnd = HWND::default();
    for _ in 0..600 {
        wait(cx, 100).await;
        // SAFETY: a lookup by title.
        if let Ok(found) = unsafe { FindWindowW(PCWSTR::null(), &HSTRING::from(TITLE)) } {
            hwnd = found;
            if cx.update(|cx| crate::views::artifacts::window::sample_loaded(cx)) {
                break;
            }
        }
    }
    if hwnd.is_invalid() {
        return Err("the 产出 window did not open".into());
    }
    if !cx.update(|cx| crate::views::artifacts::window::sample_loaded(cx)) {
        return Err("the page did not load within 60 s".into());
    }
    // SAFETY: brings this process's own window to the front for the capture.
    unsafe {
        let _ = SetForegroundWindow(hwnd);
    }
    wait(cx, 1500).await;
    capture(hwnd, &dir.join(format!("artifact-window-{name}.png")))?;
    // SAFETY: opens and closes the list of this window's own combo box.
    let picker = unsafe { FindWindowExW(Some(hwnd), None, w!("ComboBox"), PCWSTR::null()) }
        .map_err(|e| format!("no version picker: {e}"))?;
    unsafe {
        SendMessageW(picker, CB_SHOWDROPDOWN, Some(WPARAM(1)), Some(LPARAM(0)));
    }
    wait(cx, 800).await;
    let shot = capture_below(
        hwnd,
        &dir.join(format!("artifact-window-{name}-versions.png")),
    );
    unsafe {
        SendMessageW(picker, CB_SHOWDROPDOWN, Some(WPARAM(0)), Some(LPARAM(0)));
    }
    shot?;
    cx.update(|cx| crate::views::artifacts::window::close_sample(cx));
    wait(cx, 500).await;
    Ok(())
}

/// The window's frame on screen as DWM draws it (without the invisible resize border).
fn frame(hwnd: HWND) -> Result<RECT, String> {
    let mut rect = RECT::default();
    // SAFETY: reads one RECT attribute of a live window.
    unsafe {
        DwmGetWindowAttribute(
            hwnd,
            DWMWA_EXTENDED_FRAME_BOUNDS,
            (&mut rect as *mut RECT).cast(),
            std::mem::size_of::<RECT>() as u32,
        )
    }
    .map_err(|e| format!("window frame: {e}"))?;
    Ok(rect)
}

fn capture(hwnd: HWND, png: &std::path::Path) -> Result<(), String> {
    let r = frame(hwnd)?;
    write(r.left, r.top, r.right - r.left, r.bottom - r.top, png)
}

/// The window with what hangs below its picker (the open list can end under the frame).
fn capture_below(hwnd: HWND, png: &std::path::Path) -> Result<(), String> {
    let r = frame(hwnd)?;
    write(r.left, r.top, r.right - r.left, r.bottom - r.top + 40, png)
}

fn write(x: i32, y: i32, width: i32, height: i32, png: &std::path::Path) -> Result<(), String> {
    let bytes = artifact_view::win_support::capture_screen_png(x, y, width, height)
        .ok_or("the screen could not be read")?;
    std::fs::write(png, bytes).map_err(|e| e.to_string())?;
    eprintln!(
        "{}: {width}x{height} px",
        png.file_name().unwrap_or_default().to_string_lossy()
    );
    Ok(())
}
