//! Dragging a file out of the window on Windows, to Explorer or another app.
//!
//! On macOS GPUI hands a file drag to the system when the pointer leaves the window
//! (`external_drag_payload` in [`super::export`]); its Windows platform has no such call.
//! So the app does it: while a file drags inside the window a task watches the pointer,
//! and once it is outside the window (with the button still down, and the file on disk)
//! the task ends GPUI's drag and starts the system's (`DoDragDrop`) with the shell's own
//! data object for the file, which carries `CF_HDROP` and everything else Explorer and
//! other apps read. Only a copy is offered: the file is the app's cached copy.
//!
//! `DoDragDrop` runs the system's drag loop and returns when the drag ends. It is called
//! from the watching task between two app updates, so GPUI is not borrowed while the loop
//! dispatches messages (as the notification-area menu does).

use std::{path::Path, time::Duration};

use gpui::{App, AsyncApp, Window};
use windows::{
    Win32::{
        Foundation::{
            DRAGDROP_S_CANCEL, DRAGDROP_S_DROP, DRAGDROP_S_USEDEFAULTCURSORS, HWND, S_OK,
        },
        System::{
            Com::IDataObject,
            Ole::{DROPEFFECT, DROPEFFECT_COPY, DoDragDrop, IDropSource, IDropSource_Impl},
            SystemServices::{MK_LBUTTON, MODIFIERKEYS_FLAGS},
        },
        UI::{
            Input::KeyboardAndMouse::{GetAsyncKeyState, VK_LBUTTON},
            Shell::{BHID_DataObject, IShellItem, SHCreateItemFromParsingName},
            WindowsAndMessaging::{GetCursorPos, GetWindowRect},
        },
    },
    core::{BOOL, HRESULT, HSTRING, implement},
};

use super::export::FileRef;

/// How often the pointer is looked at while a file drags inside the window.
const TICK: Duration = Duration::from_millis(16);

/// Whether the point is outside the rectangle (left, top, right, bottom), in one
/// coordinate space.
pub fn outside(point: (i32, i32), rect: (i32, i32, i32, i32)) -> bool {
    let ((x, y), (left, top, right, bottom)) = (point, rect);
    x < left || x >= right || y < top || y >= bottom
}

/// What the system's drag loop does next, from Escape and the mouse buttons.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Step {
    Continue,
    Drop,
    Cancel,
}

pub fn next_step(escape: bool, left_button_down: bool) -> Step {
    if escape {
        Step::Cancel
    } else if left_button_down {
        Step::Continue
    } else {
        Step::Drop
    }
}

/// The drag's source: it only says when the drag drops or is cancelled; the system draws
/// the cursors.
#[implement(IDropSource)]
struct DropSource;

impl IDropSource_Impl for DropSource_Impl {
    fn QueryContinueDrag(&self, escape: BOOL, keys: MODIFIERKEYS_FLAGS) -> HRESULT {
        match next_step(escape.as_bool(), keys.contains(MK_LBUTTON)) {
            Step::Continue => S_OK,
            Step::Drop => DRAGDROP_S_DROP,
            Step::Cancel => DRAGDROP_S_CANCEL,
        }
    }

    fn GiveFeedback(&self, _effect: DROPEFFECT) -> HRESULT {
        DRAGDROP_S_USEDEFAULTCURSORS
    }
}

/// The shell's data object for the file or folder at `path`: what a drag out of Explorer
/// itself carries.
pub fn data_object(path: &Path) -> windows::core::Result<IDataObject> {
    // SAFETY: the path string outlives the calls; the handler id names the interface
    // asked for.
    unsafe {
        let item: IShellItem = SHCreateItemFromParsingName(&HSTRING::from(path.as_os_str()), None)?;
        item.BindToHandler(None, &BHID_DataObject)
    }
}

/// Runs the system's drag of `path`, copy only, until it drops or is cancelled. True when
/// a target took the file. Call on the main thread, outside any app update.
fn drag_out(path: &Path) -> bool {
    let Ok(data) = data_object(path) else {
        return false;
    };
    let source: IDropSource = DropSource.into();
    let mut effect = DROPEFFECT(0);
    // SAFETY: both objects live across the call; `effect` is a valid out pointer.
    let result = unsafe { DoDragDrop(&data, &source, DROPEFFECT_COPY, &mut effect) };
    result == DRAGDROP_S_DROP && effect != DROPEFFECT(0)
}

fn left_button_down() -> bool {
    // SAFETY: reads the key state.
    unsafe { GetAsyncKeyState(VK_LBUTTON.0 as i32) < 0 }
}

/// True when the pointer is outside the window `hwnd` (false when either cannot be read).
fn pointer_outside(hwnd: HWND) -> bool {
    let (mut at, mut rect) = Default::default();
    // SAFETY: both out pointers are valid; `hwnd` is a window of this thread.
    unsafe {
        if GetCursorPos(&mut at).is_err() || GetWindowRect(hwnd, &mut rect).is_err() {
            return false;
        }
    }
    outside((at.x, at.y), (rect.left, rect.top, rect.right, rect.bottom))
}

/// A file started to drag in `window`: hand the drag to the system once the pointer leaves
/// the window. The watch ends with the drag.
pub fn watch(target: FileRef, window: &mut Window, cx: &mut App) {
    let Some(hwnd) = crate::win32::hwnd(window) else {
        return;
    };
    let (hwnd, handle) = (hwnd.0 as isize, window.window_handle());
    cx.spawn(async move |cx: &mut AsyncApp| {
        loop {
            cx.background_executor().timer(TICK).await;
            // The drag starts on a later turn than this watch: wait for the button, not
            // for the drag, and stop when the button is up.
            if !left_button_down() {
                return;
            }
            if !cx.update(|cx| cx.has_active_drag()) || !pointer_outside(HWND(hwnd as *mut _)) {
                continue;
            }
            // Not on disk yet (the drag started its download): keep the drag in the
            // window until it is.
            let Some(local) = cx.update(|_| target.local_now()) else {
                continue;
            };
            let stopped = handle
                .update(cx, |_, window, cx| cx.stop_active_drag(window))
                .unwrap_or(false);
            if stopped {
                let taken = drag_out(&local.item);
                tracing::debug!(taken, "file dragged out of the window");
            }
            return;
        }
    })
    .detach();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_pointer_leaves_the_window_past_any_edge() {
        let rect = (100, 100, 500, 400);
        assert!(!outside((100, 100), rect));
        assert!(!outside((499, 399), rect));
        for point in [(99, 200), (500, 200), (200, 99), (200, 400), (-5, -5)] {
            assert!(outside(point, rect), "{point:?}");
        }
    }

    #[test]
    fn the_drag_drops_on_release_and_cancels_on_escape() {
        assert_eq!(next_step(false, true), Step::Continue);
        assert_eq!(next_step(false, false), Step::Drop);
        assert_eq!(next_step(true, true), Step::Cancel);
        assert_eq!(next_step(true, false), Step::Cancel);
        // The same through the COM object the system calls.
        let source: IDropSource = DropSource.into();
        // SAFETY: calls on a live object.
        unsafe {
            assert_eq!(source.QueryContinueDrag(false, MK_LBUTTON), S_OK);
            assert_eq!(
                source.QueryContinueDrag(false, MODIFIERKEYS_FLAGS(0)),
                DRAGDROP_S_DROP
            );
            assert_eq!(
                source.QueryContinueDrag(true, MK_LBUTTON),
                DRAGDROP_S_CANCEL
            );
            assert_eq!(
                source.GiveFeedback(DROPEFFECT_COPY),
                DRAGDROP_S_USEDEFAULTCURSORS
            );
        }
    }
}
