//! Platform-independent parts of an embedded 产出 view: options and the frame math that
//! places a native web view inside a host window from the host's element bounds.
//!
//! The host (a GPUI element) reports two rectangles each frame, both in points with a
//! top-left origin in the host view's coordinates:
//!
//! - `full`: the element's bounds (where the whole page would be);
//! - `visible`: the part of `full` that the host does not clip (scroll containers and
//!   content masks).
//!
//! [`place`] turns them into the frame of a clipping container (the visible part, in the
//! parent's own coordinate system) and the frame of the web view inside that container
//! (always the full size, offset so that the page does not move while it is clipped).

use crate::{Capabilities, VersionSpec};

/// A rectangle in points.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

impl Rect {
    pub const fn new(x: f64, y: f64, w: f64, h: f64) -> Self {
        Self { x, y, w, h }
    }

    pub fn is_empty(&self) -> bool {
        !(self.w > 0.0 && self.h > 0.0)
    }

    pub fn right(&self) -> f64 {
        self.x + self.w
    }

    pub fn bottom(&self) -> f64 {
        self.y + self.h
    }

    /// The overlap of two rectangles (empty when they do not overlap).
    pub fn intersect(&self, other: &Rect) -> Rect {
        let x = self.x.max(other.x);
        let y = self.y.max(other.y);
        let r = self.right().min(other.right());
        let b = self.bottom().min(other.bottom());
        Rect::new(x, y, (r - x).max(0.0), (b - y).max(0.0))
    }

    /// Rounds the edges to whole device pixels at `scale` (2.0 on Retina), so the web view
    /// never lands on a half pixel and blurs.
    pub fn snap(&self, scale: f64) -> Rect {
        let s = if scale > 0.0 { scale } else { 1.0 };
        let x = (self.x * s).round() / s;
        let y = (self.y * s).round() / s;
        let r = (self.right() * s).round() / s;
        let b = (self.bottom() * s).round() / s;
        Rect::new(x, y, (r - x).max(0.0), (b - y).max(0.0))
    }
}

/// Where the embedded view goes in one frame.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Placement {
    /// The clipping container, in the parent view's coordinates (bottom-left origin when the
    /// parent is not flipped, as AppKit content views are).
    pub container: Rect,
    /// The web view inside the container, in the container's flipped (top-left) coordinates.
    pub web_view: Rect,
}

/// Places the view, or returns `None` when nothing of it is visible (the host hides it).
///
/// `parent_height` is the parent view's height in points; `parent_flipped` says whether its
/// origin is at the top left.
pub fn place(
    full: Rect,
    visible: Rect,
    parent_height: f64,
    parent_flipped: bool,
    scale: f64,
) -> Option<Placement> {
    let full = full.snap(scale);
    if full.is_empty() {
        return None;
    }
    let vis = visible.snap(scale).intersect(&full);
    if vis.is_empty() {
        return None;
    }
    let container = if parent_flipped {
        vis
    } else {
        Rect::new(vis.x, parent_height - vis.bottom(), vis.w, vis.h)
    };
    let web_view = Rect::new(full.x - vis.x, full.y - vis.y, full.w, full.h);
    Some(Placement {
        container,
        web_view,
    })
}

/// Everything needed to embed one 产出 in a host view.
#[derive(Clone, Debug)]
pub struct EmbedOptions {
    pub artifact_id: String,
    pub version: VersionSpec,
    pub capabilities: Capabilities,
    /// Draw the page with the dark appearance (`prefers-color-scheme: dark`).
    pub dark: bool,
    /// Corner radius of the clipping container, in points.
    pub corner_radius: f64,
    /// Give wheel and trackpad scrolling to the host view instead of the page. Use it for a
    /// page shown at its full height inside a scrolling list (a 话题), so the list scrolls
    /// under the pointer.
    pub pass_scroll: bool,
    /// Let `<video autoplay>` and `<audio autoplay>` start without a click.
    pub autoplay: bool,
    /// Allow Safari Web Inspector (developer view only).
    pub inspectable: bool,
}

impl EmbedOptions {
    pub fn new(artifact_id: impl Into<String>, version: VersionSpec) -> Self {
        Self {
            artifact_id: artifact_id.into(),
            version,
            capabilities: Capabilities::ALL,
            dark: false,
            corner_radius: 0.0,
            pass_scroll: false,
            autoplay: false,
            inspectable: false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn places_unclipped_in_a_bottom_left_parent() {
        let full = Rect::new(100.0, 50.0, 400.0, 300.0);
        let p = place(full, Rect::new(0.0, 0.0, 1440.0, 900.0), 900.0, false, 2.0).unwrap();
        assert_eq!(p.container, Rect::new(100.0, 550.0, 400.0, 300.0));
        assert_eq!(p.web_view, Rect::new(0.0, 0.0, 400.0, 300.0));
    }

    #[test]
    fn clips_when_scrolled_out_at_the_top() {
        // A list clips at y = 100; the element starts 40 pt above it.
        let full = Rect::new(20.0, 60.0, 300.0, 200.0);
        let mask = Rect::new(0.0, 100.0, 1000.0, 600.0);
        let p = place(full, mask, 800.0, true, 1.0).unwrap();
        assert_eq!(p.container, Rect::new(20.0, 100.0, 300.0, 160.0));
        // The page keeps its place: it starts 40 pt above the container.
        assert_eq!(p.web_view, Rect::new(0.0, -40.0, 300.0, 200.0));
        let p = place(full, mask, 800.0, false, 1.0).unwrap();
        assert_eq!(p.container, Rect::new(20.0, 800.0 - 260.0, 300.0, 160.0));
    }

    #[test]
    fn hides_when_fully_out_or_empty() {
        let full = Rect::new(0.0, 0.0, 100.0, 100.0);
        assert_eq!(place(full, Rect::new(0.0, 200.0, 100.0, 100.0), 900.0, false, 2.0), None);
        assert_eq!(place(Rect::new(0.0, 0.0, 0.0, 10.0), full, 900.0, false, 2.0), None);
        assert_eq!(place(full, Rect::default(), 900.0, false, 2.0), None);
    }

    #[test]
    fn snaps_to_device_pixels() {
        let r = Rect::new(10.3, 10.26, 100.1, 50.0).snap(2.0);
        assert_eq!(r, Rect::new(10.5, 10.5, 100.0, 50.0));
        assert_eq!(Rect::new(1.2, 0.0, 1.0, 1.0).snap(1.0), Rect::new(1.0, 0.0, 1.0, 1.0));
    }
}
