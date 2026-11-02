//! Widths of the sidebar and the topic list (DESIGN.md §4.4): a drag on the
//! hairline between two columns resizes the column on its left 1:1 with the
//! pointer, with rubber-band resistance past the bounds and a settle back on
//! release; a double-click resets the default. The widths are kept in
//! `layout.json` under the data root, for the app only (never in self-tests).

use std::{
    path::PathBuf,
    time::{Duration, Instant},
};

use gpui::{Pixels, px};
use serde::{Deserialize, Serialize};

use crate::theme;

/// How far past a bound the rubber band can stretch, at most.
const RUBBER_LIMIT: f32 = 40.;
/// Resistance of the rubber band (UIScrollView's constant).
const RUBBER_K: f32 = 0.55;
/// The settle back to the bound after a release past it.
pub const SETTLE: Duration = theme::motion::SLOW;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Column {
    Sidebar,
    TopicList,
}

impl Column {
    pub fn bounds(self) -> (Pixels, Pixels) {
        match self {
            Column::Sidebar => (theme::SIDEBAR_MIN_WIDTH, theme::SIDEBAR_MAX_WIDTH),
            Column::TopicList => (theme::TOPIC_LIST_MIN_WIDTH, theme::TOPIC_LIST_MAX_WIDTH),
        }
    }

    pub fn default_width(self) -> Pixels {
        match self {
            Column::Sidebar => theme::SIDEBAR_WIDTH,
            Column::TopicList => theme::TOPIC_LIST_WIDTH,
        }
    }
}

/// A drag in progress: the pointer's x and the column's width at the grab.
#[derive(Clone, Copy, Debug)]
pub struct Drag {
    pub column: Column,
    pub grab_x: Pixels,
    pub start: Pixels,
}

/// A column easing back to its bound after a release past it.
#[derive(Clone, Copy, Debug)]
pub struct Settle {
    pub column: Column,
    pub from: Pixels,
    pub to: Pixels,
    pub at: Instant,
}

#[derive(Clone, Copy, Debug)]
pub struct Columns {
    pub sidebar: Pixels,
    pub topic_list: Pixels,
    pub drag: Option<Drag>,
    pub settle: Option<Settle>,
    /// Save changes to `layout.json` (the app, not a self-test).
    pub persist: bool,
}

#[derive(Serialize, Deserialize, Default)]
struct Saved {
    sidebar: Option<f32>,
    topic_list: Option<f32>,
}

impl Default for Columns {
    fn default() -> Self {
        Self {
            sidebar: theme::SIDEBAR_WIDTH,
            topic_list: theme::TOPIC_LIST_WIDTH,
            drag: None,
            settle: None,
            persist: false,
        }
    }
}

/// The width a drag gives: 1:1 inside the bounds, then a rubber band that
/// never stretches more than [`RUBBER_LIMIT`] past them.
pub fn rubber_width(raw: Pixels, (min, max): (Pixels, Pixels)) -> Pixels {
    let band = |over: f32| (1. - 1. / (over * RUBBER_K / RUBBER_LIMIT + 1.)) * RUBBER_LIMIT;
    if raw < min {
        min - px(band(f32::from(min - raw)))
    } else if raw > max {
        max + px(band(f32::from(raw - max)))
    } else {
        raw
    }
}

fn layout_file() -> PathBuf {
    crate::provider::session::default_data_root().join("layout.json")
}

impl Columns {
    /// The saved widths of the app (clamped to the bounds), saving changes.
    pub fn load() -> Self {
        let saved: Saved = std::fs::read(layout_file())
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default();
        let width = |v: Option<f32>, c: Column| {
            let (min, max) = c.bounds();
            v.filter(|v| v.is_finite())
                .map_or(c.default_width(), |v| px(v).clamp(min, max))
        };
        Self {
            sidebar: width(saved.sidebar, Column::Sidebar),
            topic_list: width(saved.topic_list, Column::TopicList),
            persist: true,
            ..Self::default()
        }
    }

    fn save(&self) {
        if !self.persist {
            return;
        }
        let saved = Saved {
            sidebar: Some(f32::from(self.sidebar)),
            topic_list: Some(f32::from(self.topic_list)),
        };
        let path = layout_file();
        if let (Some(dir), Ok(json)) = (path.parent(), serde_json::to_vec(&saved)) {
            let _ = std::fs::create_dir_all(dir);
            if let Err(error) = std::fs::write(&path, json) {
                tracing::warn!("layout: could not save the column widths: {error}");
            }
        }
    }

    fn width_mut(&mut self, column: Column) -> &mut Pixels {
        match column {
            Column::Sidebar => &mut self.sidebar,
            Column::TopicList => &mut self.topic_list,
        }
    }

    /// The width of `column` as drawn now (with a settle in progress).
    pub fn width(&self, column: Column) -> Pixels {
        if let Some(s) = self.settle.filter(|s| s.column == column) {
            let t = (s.at.elapsed().as_secs_f32() / SETTLE.as_secs_f32()).min(1.);
            let e = theme::motion::out().sample(t);
            return s.from + (s.to - s.from) * e;
        }
        match column {
            Column::Sidebar => self.sidebar,
            Column::TopicList => self.topic_list,
        }
    }

    /// A settle is running (the view asks for another frame).
    pub fn settling(&mut self) -> bool {
        match self.settle {
            Some(s) if s.at.elapsed() < SETTLE => true,
            Some(_) => {
                self.settle = None;
                false
            }
            None => false,
        }
    }

    pub fn start_drag(&mut self, column: Column, x: Pixels) {
        self.settle = None;
        self.drag = Some(Drag {
            column,
            grab_x: x,
            start: self.width(column),
        });
    }

    /// Follows the pointer; `true` when the width changed.
    pub fn drag_to(&mut self, x: Pixels) -> bool {
        let Some(d) = self.drag else {
            return false;
        };
        let w = rubber_width(d.start + (x - d.grab_x), d.column.bounds());
        let slot = self.width_mut(d.column);
        let changed = *slot != w;
        *slot = w;
        changed
    }

    /// Ends a drag: a width past a bound eases back to it; the result is saved.
    pub fn end_drag(&mut self, reduce_motion: bool) -> bool {
        let Some(d) = self.drag.take() else {
            return false;
        };
        let (min, max) = d.column.bounds();
        let w = self.width(d.column);
        let to = w.clamp(min, max);
        *self.width_mut(d.column) = to;
        if to != w && !reduce_motion {
            self.settle = Some(Settle {
                column: d.column,
                from: w,
                to,
                at: Instant::now(),
            });
        }
        self.save();
        true
    }

    /// Double-click on a divider: back to the default width.
    pub fn reset(&mut self, column: Column) {
        self.drag = None;
        self.settle = None;
        *self.width_mut(column) = column.default_width();
        self.save();
    }

    /// The widths that fit a window `window_w` wide: the topic view keeps its
    /// 480 pt minimum; the topic list gives way first, then the sidebar.
    pub fn fitted(&self, window_w: Pixels, sidebar_shown: bool) -> (Pixels, Pixels) {
        let (s, l, _) = self.layout(window_w, sidebar_shown, false);
        (s, l)
    }

    /// The column widths for a window `window_w` wide and whether the open
    /// side panel floats over the topic view (§4.4): the panel's 300 pt and
    /// the topic view's 480 pt come first; the topic list shrinks to 280,
    /// then the sidebar to 200; with no room left the panel floats and the
    /// columns keep their widths.
    pub fn layout(
        &self,
        window_w: Pixels,
        sidebar_shown: bool,
        panel_open: bool,
    ) -> (Pixels, Pixels, bool) {
        let fit = |reserve: Pixels| {
            let mut sidebar = if sidebar_shown {
                self.width(Column::Sidebar)
            } else {
                px(0.)
            };
            let mut list = self.width(Column::TopicList);
            let room = window_w - theme::TOPIC_VIEW_MIN_WIDTH - reserve;
            let over = sidebar + list - room;
            if over > px(0.) {
                let give = over.min((list - theme::TOPIC_LIST_MIN_WIDTH).max(px(0.)));
                list -= give;
                let rest = over - give;
                if sidebar_shown && rest > px(0.) {
                    sidebar = (sidebar - rest).max(theme::SIDEBAR_MIN_WIDTH);
                }
            }
            (sidebar, list, sidebar + list <= room)
        };
        if panel_open {
            let (s, l, fits) = fit(theme::SIDE_PANEL_WIDTH);
            if fits {
                return (s, l, false);
            }
            let (s, l, _) = fit(px(0.));
            return (s, l, true);
        }
        let (s, l, _) = fit(px(0.));
        (s, l, false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn drag_tracks_the_pointer_and_resists_past_the_bounds() {
        let b = Column::Sidebar.bounds();
        assert_eq!(rubber_width(px(260.), b), px(260.));
        let past = rubber_width(px(420.), b);
        assert!(past > px(320.) && past < px(360.), "{past:?}");
        let far = rubber_width(px(5000.), b);
        assert!(far < px(320. + RUBBER_LIMIT), "{far:?}");
        let under = rubber_width(px(100.), b);
        assert!(under < px(200.) && under > px(160.), "{under:?}");
    }

    #[test]
    fn a_release_past_a_bound_settles_on_it_and_double_click_resets() {
        let mut c = Columns::default();
        c.start_drag(Column::TopicList, px(600.));
        assert!(c.drag_to(px(650.)));
        assert_eq!(c.topic_list, px(370.));
        c.drag_to(px(900.));
        assert!(c.topic_list > px(400.));
        c.end_drag(true);
        assert_eq!(c.topic_list, theme::TOPIC_LIST_MAX_WIDTH);
        assert!(c.settle.is_none());
        c.reset(Column::TopicList);
        assert_eq!(c.topic_list, theme::TOPIC_LIST_WIDTH);
    }

    #[test]
    fn narrow_windows_keep_the_topic_view_minimum() {
        let mut c = Columns::default();
        c.sidebar = px(320.);
        c.topic_list = px(400.);
        let (s, l) = c.fitted(px(1100.), true);
        assert_eq!((s, l), (px(248.), px(320.)));
        assert!(l >= theme::TOPIC_LIST_MIN_WIDTH);
        assert_eq!(c.fitted(px(1440.), true), (px(320.), px(400.)));
        assert_eq!(c.fitted(px(1100.), false).0, px(0.));
    }

    #[test]
    fn the_side_panel_takes_room_then_floats() {
        let c = Columns::default();
        assert_eq!(c.layout(px(1440.), true, true), (px(248.), px(320.), false));
        // 1300 pt: the topic list gives way first, then the sidebar.
        let (s, l, floats) = c.layout(px(1300.), true, true);
        assert!(!floats);
        assert_eq!(s + l, px(1300. - 480. - 300.));
        assert_eq!(l, theme::TOPIC_LIST_MIN_WIDTH);
        // 1100 pt: no room for 200 + 280 + 300 + 480, so the panel floats
        // and the columns keep what fits without it.
        let (s, l, floats) = c.layout(px(1100.), true, true);
        assert!(floats);
        assert_eq!((s, l), (px(248.), px(320.)));
    }
}
