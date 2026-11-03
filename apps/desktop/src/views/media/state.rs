//! The interaction state behind one media view: what is selected, the audio player, the
//! table sort, the PDF zoom. Inline previews and full views keep one each, as keyed
//! element state ([`MediaState::for_view`]).

use std::{path::Path, sync::Arc, time::Duration};

use gpui::{App, Context, ElementId, Entity, ScrollHandle, Task, UniformListScrollHandle, Window};

use super::native::AudioPlayer;
use crate::data::ChatProvider;

/// Speeds of the audio player.
pub const SPEEDS: &[(f32, &str)] = &[
    (0.75, "0.75×"),
    (1.0, "1×"),
    (1.25, "1.25×"),
    (1.5, "1.5×"),
    (2.0, "2×"),
];

pub struct MediaState {
    /// Selected image, PDF page or video frame.
    pub index: usize,
    /// A video frame was picked in the strip.
    pub frame: Option<usize>,
    /// Audio position or the video's current time, in seconds.
    pub position: f64,
    pub audio: Option<AudioPlayer>,
    pub speed: usize,
    pub sort: Option<(usize, bool)>,
    pub toc: usize,
    pub zoom: f32,
    pub sidebar: bool,
    pub wrap: bool,
    pub expanded: bool,
    /// Events the HTML page sent, newest last.
    pub events: Vec<String>,
    /// The version shown in the HTML pane (its version list switches it).
    pub version: Option<u32>,
    pub scroll: ScrollHandle,
    pub list: UniformListScrollHandle,
    pub pages: ScrollHandle,
    _poll: Option<Task<()>>,
}

impl MediaState {
    pub fn new() -> Self {
        Self {
            index: 0,
            frame: None,
            position: 0.,
            audio: None,
            speed: 1,
            sort: None,
            toc: 0,
            zoom: 1.,
            sidebar: true,
            wrap: false,
            expanded: false,
            events: Vec::new(),
            version: None,
            scroll: ScrollHandle::new(),
            list: UniformListScrollHandle::new(),
            pages: ScrollHandle::new(),
            _poll: None,
        }
    }

    /// The state of the view `id`, kept while the view is rendered.
    pub fn for_view(id: impl Into<ElementId>, window: &mut Window, cx: &mut App) -> Entity<Self> {
        window.use_keyed_state(id.into(), cx, |_, _| MediaState::new())
    }

    pub fn playing(&self) -> bool {
        self.audio.as_ref().is_some_and(|a| a.is_playing())
    }

    /// Plays or pauses `path`, and follows the position while it plays.
    pub fn toggle_audio(&mut self, path: &Path, cx: &mut Context<Self>) {
        if self.audio.is_none() {
            self.audio = AudioPlayer::new(path);
            if let Some(a) = &mut self.audio {
                a.set_rate(SPEEDS[self.speed].0);
                if self.position > 0. {
                    a.seek(self.position);
                }
            }
        }
        let Some(a) = &mut self.audio else { return };
        if a.is_playing() {
            a.pause();
            self._poll = None;
        } else {
            a.play();
            self.follow(cx);
        }
        cx.notify();
    }

    pub fn seek_audio(&mut self, path: &Path, seconds: f64, cx: &mut Context<Self>) {
        self.position = seconds.max(0.);
        if self.audio.is_none() {
            self.audio = AudioPlayer::new(path);
        }
        if let Some(a) = &mut self.audio {
            a.seek(self.position);
        }
        cx.notify();
    }

    pub fn set_speed(&mut self, ix: usize, cx: &mut Context<Self>) {
        self.speed = ix.min(SPEEDS.len() - 1);
        if let Some(a) = &mut self.audio {
            a.set_rate(SPEEDS[self.speed].0);
        }
        cx.notify();
    }

    fn follow(&mut self, cx: &mut Context<Self>) {
        self._poll = Some(cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(200))
                    .await;
                let go = this
                    .update(cx, |s, cx| {
                        if let Some(a) = &s.audio {
                            s.position = a.position();
                        }
                        cx.notify();
                        s.playing()
                    })
                    .unwrap_or(false);
                if !go {
                    break;
                }
            }
        }));
    }
}

impl Default for MediaState {
    fn default() -> Self {
        Self::new()
    }
}

/// Where a view's 发到话题 actions go: the 话题 of the 产出.
#[derive(Clone)]
pub struct Target {
    pub provider: Option<Arc<dyn ChatProvider>>,
    pub group_id: String,
    pub topic_id: String,
}

impl Target {
    pub fn none() -> Self {
        Self {
            provider: None,
            group_id: String::new(),
            topic_id: String::new(),
        }
    }
}
