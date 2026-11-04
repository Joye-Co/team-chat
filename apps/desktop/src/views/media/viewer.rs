//! The 产出 viewer: a 560 sheet in the window it was opened from (COMPLETENESS.md
//! F52, §2.9) that shows a version's full view ([`super::full`]): one fixed header
//! (icon, title, meta, then copy, download, open and close), the body filling the
//! sheet, a footer with user-facing text. Opened from an inline preview or from the
//! 产出 list for media the app can show.

use std::{rc::Rc, sync::Arc};

use gpui::{
    App, AppContext as _, Context, Entity, FocusHandle, Focusable, InteractiveElement as _,
    IntoElement, KeyDownEvent, ParentElement, Pixels, Render, SharedString, Styled, Window, div,
    px,
};
use gpui_component::WindowExt as _;

use super::{
    full::{self, FullSpec, VersionRow},
    state::{MediaState, Target},
    store::{self, Data, Load, MediaKey},
};
use crate::{
    theme::{self, space},
    ui::native::{SheetWidth, metrics, native_sheet},
};

/// Where the view starts.
#[derive(Clone, Debug, Default)]
pub struct Opening {
    pub index: Option<usize>,
    pub frame: Option<usize>,
    pub autoplay: bool,
}

impl Opening {
    pub fn at(index: usize) -> Self {
        Self {
            index: Some(index),
            ..Self::default()
        }
    }
    pub fn frame(i: usize) -> Self {
        Self {
            frame: Some(i),
            ..Self::default()
        }
    }
    pub fn play() -> Self {
        Self {
            autoplay: true,
            ..Self::default()
        }
    }
}

pub struct MediaViewer {
    key: MediaKey,
    title: String,
    target: Target,
    state: Entity<MediaState>,
    opening: Opening,
    focus: FocusHandle,
}

impl MediaViewer {
    fn new(
        key: MediaKey,
        title: String,
        target: Target,
        opening: Opening,
        cx: &mut Context<Self>,
    ) -> Self {
        let state = cx.new(|_| MediaState::new());
        apply(&state, &opening, cx);
        Self {
            key,
            title,
            target,
            state,
            opening,
            focus: cx.focus_handle(),
        }
    }

    fn versions(&self) -> Vec<VersionRow> {
        let Some(p) = &self.target.provider else {
            return Vec::new();
        };
        let Some(d) = p.artifact(&self.key.artifact_id) else {
            return Vec::new();
        };
        d.versions
            .iter()
            .map(|v| VersionRow {
                version: v.version,
                when: when(v.created_at),
                note: v.note.clone().unwrap_or_default(),
            })
            .collect()
    }

    fn on_key(&mut self, e: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let Some(media) = store::ready(&self.key, cx) else {
            return;
        };
        let n = match &media.data {
            Data::Images(p) => p.len(),
            Data::Pdf(p) => p.images.len(),
            _ => 0,
        };
        match e.keystroke.key.as_str() {
            "escape" => window.close_dialog(cx),
            "left" | "right" if n > 0 => {
                let d: isize = if e.keystroke.key == "left" { -1 } else { 1 };
                self.state.update(cx, |s, cx| {
                    s.index = (s.index as isize + d).rem_euclid(n as isize) as usize;
                    if matches!(media.data, Data::Pdf(_)) {
                        s.pages.scroll_to_top_of_item(s.index);
                    }
                    cx.notify();
                });
            }
            "space" => {
                if let Data::Audio(a) = &media.data {
                    let path = a.path.clone();
                    self.state.update(cx, |s, cx| s.toggle_audio(&path, cx));
                }
            }
            _ => {}
        }
    }
}

fn apply(state: &Entity<MediaState>, opening: &Opening, cx: &mut App) {
    state.update(cx, |s, cx| {
        if let Some(i) = opening.index {
            s.index = i;
        }
        if let Some(f) = opening.frame {
            s.frame = Some(f);
        }
        cx.notify();
    });
}

/// `09:40` today, `11-03` earlier, `未知` for a missing time: the app's one
/// zero-safe formatter.
fn when(at: i64) -> String {
    crate::views::shell::clock(at)
}

impl Focusable for MediaViewer {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for MediaViewer {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let k = theme::colors(cx);
        if store::get(&self.key, cx).is_none()
            && let Some(p) = self.target.provider.clone()
        {
            store::request_artifact(&p, &self.key, &self.title, cx);
        }
        let body = match store::get(&self.key, cx) {
            Some(Load::Ready(media)) => {
                let spec = FullSpec {
                    media: media.clone(),
                    target: self.target.clone(),
                    versions: self.versions(),
                    close: Some(close_sheet()),
                    id: SharedString::from(format!("viewer-{}", self.key.artifact_id)),
                    autoplay: self.opening.autoplay,
                };
                // The frame the view opened at: seek once the player is there.
                if let (Data::Video(v), Some(f)) = (&media.data, self.opening.frame.take())
                    && let Some(fr) = v.frames.get(f)
                {
                    self.state.update(cx, |s, _| s.position = fr.t);
                }
                full::render(&spec, &self.state, window, cx)
            }
            // Loading and failed keep the header, so the sheet keeps its anatomy and its
            // close button (§2.14, §2.15).
            Some(Load::Failed(_)) => full::failed(
                &self.key,
                &self.title,
                self.target.clone(),
                close_sheet(),
                cx,
            ),
            _ => full::loading(&self.title, close_sheet(), cx),
        };
        div()
            .track_focus(&self.focus)
            .on_key_down(cx.listener(Self::on_key))
            .size_full()
            .flex()
            .flex_col()
            .rounded(metrics::SHEET_RADIUS)
            .overflow_hidden()
            .bg(k.content)
            .text_color(k.text_1)
            .child(body)
    }
}

impl MediaViewer {
    /// The sheet height: the content's own height when it is shorter than the room
    /// (a short code file, a short table, the failed Alert), so the sheet ends at its
    /// content instead of a blank band (§2.4, §2.21).
    fn height(&self, window: &Window, cx: &App) -> Pixels {
        let room = sheet_height(window);
        let head = metrics::TOOLBAR_ROW;
        let natural = match store::get(&self.key, cx) {
            Some(Load::Failed(_)) => Some(head + space::S24 * 2. + px(72.)),
            Some(Load::Ready(m)) => match &m.data {
                Data::Code(c) => Some(head + space::S8 * 2. + px(20.) * c.lines.len() as f32),
                Data::Table(t) => {
                    Some(head + full::TABLE_ROW * (t.rows.len() as f32 + 1.) + full::FOOT_HEIGHT)
                }
                _ => None,
            },
            Some(Load::Loading) | None => Some(head + space::S24 * 2. + px(200. + 3. * 26.)),
        };
        natural.map_or(room, |n| n.min(room))
    }
}

fn close_sheet() -> full::CloseFn {
    Rc::new(|window: &mut Window, cx: &mut App| window.close_dialog(cx))
}

/// The height of the viewer sheet: from under the toolbar row to 48 above the window's
/// bottom edge, at most 760.
fn sheet_height(window: &Window) -> Pixels {
    (window.viewport_size().height - metrics::TOOLBAR_ROW - space::S48)
        .min(px(760.))
        .max(px(320.))
}

/// Opens the viewer of a 产出 version as a sheet on the active window (or the first
/// window when none is key), after the current update.
pub fn open(key: MediaKey, title: String, target: Target, opening: Opening, cx: &mut App) {
    let Some(handle) = cx
        .active_window()
        .or_else(|| cx.windows().into_iter().next())
    else {
        return;
    };
    cx.defer(move |cx| {
        let _ = handle.update(cx, |_, window, cx| {
            open_in(window, key, title, target, opening, cx);
        });
    });
}

/// Opens the viewer of a 产出 version as a 560 sheet on `window`.
pub fn open_in(
    window: &mut Window,
    key: MediaKey,
    title: String,
    target: Target,
    opening: Opening,
    cx: &mut App,
) -> Entity<MediaViewer> {
    let viewer = cx.new(|cx| MediaViewer::new(key, title, target, opening, cx));
    let view = viewer.clone();
    window.open_dialog(cx, move |dialog, window, cx| {
        let h = view.read(cx).height(window, cx);
        native_sheet(dialog, SheetWidth::List, cx)
            .px(px(0.))
            .h(h)
            .child(view.clone())
    });
    let focus = viewer.read(cx).focus.clone();
    window.focus(&focus, cx);
    viewer
}

/// The provider and 话题 of a 产出, for opening its viewer from anywhere.
pub fn target_for(provider: &Arc<dyn crate::data::ChatProvider>, artifact_id: &str) -> Target {
    let d = provider.artifact(artifact_id);
    Target {
        provider: Some(provider.clone()),
        group_id: d.as_ref().map(|d| d.group_id.clone()).unwrap_or_default(),
        topic_id: d.map(|d| d.summary.topic_id).unwrap_or_default(),
    }
}
