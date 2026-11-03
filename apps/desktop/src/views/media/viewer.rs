//! The 产出 viewer: one window per 产出 that shows a version's full view ([`super::full`]).
//! Opened from an inline preview or from the 产出 list for media the app can show.

use std::{collections::HashMap, rc::Rc, sync::Arc};

use gpui::{
    App, AppContext as _, Context, Entity, FocusHandle, Focusable, InteractiveElement as _,
    IntoElement, KeyDownEvent, ParentElement, Render, SharedString, Styled, Window, WindowBounds,
    WindowHandle, div, px, size,
};

use super::{
    full::{self, FullSpec, VersionRow},
    state::{MediaState, Target},
    store::{self, Data, Load, MediaKey},
};
use crate::{
    app::main_window_options,
    copy::media as cm,
    theme::{self, Type as _, space},
    ui::{self, parts::toolbar_band},
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

#[derive(Default)]
struct Viewers {
    open: HashMap<String, WindowHandle<MediaViewer>>,
}

impl gpui::Global for Viewers {}

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
            "escape" => window.remove_window(),
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

/// `11-03 09:40` in local time.
fn when(at: i64) -> String {
    use chrono::TimeZone as _;
    chrono::Local
        .timestamp_millis_opt(at)
        .single()
        .map(|t| t.format("%m-%d %H:%M").to_string())
        .unwrap_or_default()
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
                // The frame the view opened at: seek once the player is there.
                let spec = FullSpec {
                    media: media.clone(),
                    target: self.target.clone(),
                    versions: self.versions(),
                    close: Some(Rc::new(|window: &mut Window, _: &mut App| {
                        window.remove_window()
                    })),
                    id: SharedString::from(format!("viewer-{}", self.key.artifact_id)),
                    autoplay: self.opening.autoplay,
                };
                if let (Data::Video(v), Some(f)) = (&media.data, self.opening.frame.take())
                    && let Some(fr) = v.frames.get(f)
                {
                    self.state.update(cx, |s, _| s.position = fr.t);
                }
                full::render(&spec, &self.state, window, cx)
            }
            Some(Load::Failed(_)) => div()
                .size_full()
                .flex()
                .items_center()
                .justify_center()
                .t_small()
                .text_color(k.text_3)
                .child(cm::STATE_FAILED)
                .into_any_element(),
            _ => div()
                .size_full()
                .flex()
                .items_center()
                .justify_center()
                .gap(space::S8)
                .t_small()
                .text_color(k.text_3)
                .child(ui::spinner())
                .child(cm::STATE_LOADING)
                .into_any_element(),
        };
        div()
            .track_focus(&self.focus)
            .on_key_down(cx.listener(Self::on_key))
            .size_full()
            .flex()
            .flex_col()
            .bg(k.content)
            .text_color(k.text_1)
            .child(
                toolbar_band("media-viewer-bar")
                    .pl(theme::TRAFFIC_LIGHTS_INSET)
                    .pr(space::S16)
                    .child(
                        div()
                            .t_ui()
                            .medium()
                            .text_color(k.text_2)
                            .one_line()
                            .child(self.title.clone()),
                    ),
            )
            .child(ui::hairline(cx))
            .child(div().flex_1().min_h(px(0.)).child(body))
    }
}

/// Opens (or brings to the front) the viewer of a 产出 version.
pub fn open(key: MediaKey, title: String, target: Target, opening: Opening, cx: &mut App) {
    let id = key.artifact_id.clone();
    if let Some(handle) = cx.default_global::<Viewers>().open.get(&id).copied() {
        let ok = handle
            .update(cx, |v, window, cx| {
                v.key = key.clone();
                v.title = title.clone();
                v.opening = opening.clone();
                apply(&v.state, &opening, cx);
                window.activate_window();
                cx.notify();
            })
            .is_ok();
        if ok {
            return;
        }
        cx.default_global::<Viewers>().open.remove(&id);
    }
    let mut options = main_window_options(true, cx);
    options.window_bounds = Some(WindowBounds::Windowed(gpui::Bounds::centered(
        None,
        size(px(960.), px(760.)),
        cx,
    )));
    options.window_min_size = Some(size(px(560.), px(420.)));
    let result = gpui_kit::open_window(options, cx, move |window, cx| {
        let v = cx.new(|cx| MediaViewer::new(key, title, target, opening, cx));
        let focus = v.read(cx).focus.clone();
        window.focus(&focus, cx);
        v
    });
    match result {
        Ok((handle, _)) => {
            if let Some(h) = handle.downcast::<MediaViewer>() {
                cx.default_global::<Viewers>().open.insert(id, h);
            }
        }
        Err(e) => tracing::warn!(error = %e, "cannot open the 产出 viewer"),
    }
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
