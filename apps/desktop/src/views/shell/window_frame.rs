//! The main window's place and size, kept under `window` in `layout.json`
//! like any Mac app: the window opens where it was closed, zoomed if it was
//! zoomed. A frame that no longer fits any screen (a display was unplugged)
//! is dropped and the window opens centred. Self-tests never read or write it.

use std::{cell::Cell, time::Duration};

use gpui::{App, Bounds, Pixels, Window, WindowBounds, point, px, size};
use serde::{Deserialize, Serialize};

const KEY: &str = "window";

/// How long a resize or move must rest before the frame is written.
const SAVE_DELAY: Duration = Duration::from_millis(500);

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Frame {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
    /// Zoomed (green button with ⌥, or 窗口 › 缩放).
    #[serde(default)]
    pub zoomed: bool,
}

impl Frame {
    pub fn from_window_bounds(bounds: WindowBounds) -> Self {
        let (b, zoomed) = match bounds {
            WindowBounds::Windowed(b) => (b, false),
            WindowBounds::Maximized(b) => (b, true),
            // Full screen comes back as the window it left.
            WindowBounds::Fullscreen(b) => (b, false),
        };
        Self {
            x: b.origin.x.as_f32(),
            y: b.origin.y.as_f32(),
            width: b.size.width.as_f32(),
            height: b.size.height.as_f32(),
            zoomed,
        }
    }

    fn bounds(&self) -> Bounds<Pixels> {
        Bounds::new(
            point(px(self.x), px(self.y)),
            size(px(self.width), px(self.height)),
        )
    }

    pub fn window_bounds(&self) -> WindowBounds {
        if self.zoomed {
            WindowBounds::Maximized(self.bounds())
        } else {
            WindowBounds::Windowed(self.bounds())
        }
    }

    /// True when the frame is at least the minimum size and its title row
    /// shows on one of `screens`, so the person can grab it.
    pub fn fits(&self, min: (f32, f32), screens: &[Bounds<Pixels>]) -> bool {
        if !(self.width >= min.0 && self.height >= min.1)
            || !self.x.is_finite()
            || !self.y.is_finite()
        {
            return false;
        }
        // A point 40 pt into the title row, away from the corners.
        let grab = point(px(self.x + self.width.min(200.) / 2.), px(self.y + 20.));
        screens.iter().any(|s| s.contains(&grab))
    }
}

/// The saved frame when it still fits a screen.
pub fn saved(cx: &App) -> Option<WindowBounds> {
    let value = std::fs::read(crate::views::columns::layout_file())
        .ok()
        .and_then(|b| serde_json::from_slice::<serde_json::Value>(&b).ok())?
        .get(KEY)
        .cloned()?;
    let frame: Frame = serde_json::from_value(value).ok()?;
    let screens: Vec<_> = cx.displays().iter().map(|d| d.bounds()).collect();
    let min = (
        crate::theme::WINDOW_MIN_WIDTH.as_f32(),
        crate::theme::WINDOW_MIN_HEIGHT.as_f32(),
    );
    frame.fits(min, &screens).then(|| frame.window_bounds())
}

thread_local! {
    /// Bumped on every bounds change; a pending save only writes when it is
    /// still the latest.
    static GENERATION: Cell<u64> = const { Cell::new(0) };
}

/// Saves the window's frame a moment after it stops moving.
pub fn note_change(window: &Window, cx: &mut App) {
    let frame = Frame::from_window_bounds(window.window_bounds());
    let generation = GENERATION.with(|g| {
        g.set(g.get() + 1);
        g.get()
    });
    cx.spawn(async move |cx| {
        cx.background_executor().timer(SAVE_DELAY).await;
        if GENERATION.with(|g| g.get()) == generation
            && let Ok(value) = serde_json::to_value(frame)
        {
            crate::views::shell::layout::merge(&[(KEY, value)]);
        }
    })
    .detach();
}

#[cfg(test)]
mod tests {
    use super::*;

    fn screen() -> Bounds<Pixels> {
        Bounds::new(point(px(0.), px(0.)), size(px(1512.), px(982.)))
    }

    fn frame(x: f32, y: f32) -> Frame {
        Frame {
            x,
            y,
            width: 1280.,
            height: 820.,
            zoomed: false,
        }
    }

    #[test]
    fn a_frame_on_screen_fits() {
        assert!(frame(100., 60.).fits((960., 620.), &[screen()]));
    }

    #[test]
    fn a_frame_on_an_unplugged_display_does_not_fit() {
        assert!(!frame(2000., 60.).fits((960., 620.), &[screen()]));
    }

    #[test]
    fn a_frame_below_the_minimum_does_not_fit() {
        let mut f = frame(100., 60.);
        f.width = 300.;
        assert!(!f.fits((960., 620.), &[screen()]));
    }

    #[test]
    fn zoom_survives_the_round_trip() {
        let mut f = frame(10., 20.);
        f.zoomed = true;
        assert_eq!(Frame::from_window_bounds(f.window_bounds()), f);
        let json = serde_json::to_value(f).unwrap();
        assert_eq!(serde_json::from_value::<Frame>(json).unwrap(), f);
    }
}
