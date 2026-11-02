//! Opening a topic at one message (from search): grow the window until the
//! message is in it, scroll to it and highlight it for a moment.

use std::time::{Duration, Instant};

use gpui::{Context, FollowMode, ListOffset, px};

use super::{PAGE, TimelineView, model};
use crate::{app::FocusRequest, theme::motion};

/// How long the opened message stays fully highlighted (§7.5: a 2 s
/// `me-bg` fill that fades out).
const HIGHLIGHT: Duration = Duration::from_millis(2000);
/// The fade out after it, on the out curve. Opacity, so it also runs with
/// reduce motion (§12).
const HIGHLIGHT_FADE: Duration = motion::SLOW;
/// One fade frame.
const FADE_FRAME: Duration = Duration::from_millis(16);
/// Window grows to look for the message before giving up (x PAGE messages).
const MAX_GROWS: u32 = 200;

#[derive(Default)]
pub(super) struct FocusState {
    /// The last request handled.
    serial: u64,
    /// The message still to find: (id, seq).
    target: Option<(String, u64)>,
    grows: u32,
    /// Guards against re-entry from `refresh`.
    busy: bool,
    /// Key of the highlighted row, and the serial that set it.
    highlight: Option<(String, u64)>,
    /// Strength of the highlight fill, 1 until it fades to 0.
    highlight_alpha: f32,
}

impl FocusState {
    pub(super) fn clear(&mut self) {
        self.target = None;
        self.grows = 0;
        self.highlight = None;
    }
}

impl TimelineView {
    /// A new request from navigation, if it is for the open topic.
    pub(super) fn on_focus_request(
        &mut self,
        request: Option<FocusRequest>,
        cx: &mut Context<Self>,
    ) {
        let Some(request) = request else {
            return;
        };
        if request.serial == self.focus.serial {
            return;
        }
        let Some((g, t)) = &self.open else {
            return;
        };
        if *g != request.group_id || *t != request.topic_id {
            return;
        }
        self.focus.serial = request.serial;
        self.focus.target = Some((request.message_id, request.seq));
        self.focus.grows = 0;
        self.continue_focus(cx);
        cx.notify();
    }

    /// The strength of the search highlight on row `key` (0 when none).
    pub(super) fn highlight_alpha(&self, key: &str) -> f32 {
        match &self.focus.highlight {
            Some((k, _)) if k == key => self.focus.highlight_alpha,
            _ => 0.,
        }
    }

    /// Looks for the target in the window, growing it page by page (from the
    /// cache, or with `load_older`) until the message shows up.
    pub(super) fn continue_focus(&mut self, cx: &mut Context<Self>) {
        if self.focus.busy || self.focus.target.is_none() {
            return;
        }
        self.focus.busy = true;
        while let Some((id, seq)) = self.focus.target.clone() {
            if let Some(ix) = self.messages.iter().position(|m| m.id == id) {
                self.reveal(ix, cx);
                break;
            }
            let oldest = self.messages.first().map(|m| m.created_seq).unwrap_or(0);
            let went_past = seq > 0 && oldest > 0 && oldest < seq;
            let no_more = !self.has_older && !self.provider_has_older;
            if went_past || no_more || self.focus.grows >= MAX_GROWS {
                // Deleted, or not in this topic: leave the view where it is.
                self.focus.target = None;
                break;
            }
            if self.provider_loading {
                // `refresh` calls back when the page arrives.
                break;
            }
            self.focus.grows += 1;
            self.wanted += PAGE;
            let ask_server = self.provider_len < self.wanted && self.provider_has_older;
            if ask_server && let Some((g, t)) = self.open.clone() {
                self.services.provider.load_older(&g, &t);
            }
            let before = self.messages.len();
            self.refresh(cx);
            if self.messages.len() == before && !self.provider_loading {
                // Nothing new from the cache and no request running.
                if !ask_server {
                    self.focus.target = None;
                }
                break;
            }
        }
        self.focus.busy = false;
    }

    fn reveal(&mut self, ix: usize, cx: &mut Context<Self>) {
        self.focus.target = None;
        let key = model::key(&self.messages[ix]).to_string();
        let serial = self.focus.serial;
        self.focus.highlight = Some((key, serial));
        self.focus.highlight_alpha = 1.;
        self.list.set_follow_mode(FollowMode::Normal);
        self.list.scroll_to(ListOffset {
            item_ix: ix.saturating_sub(1),
            offset_in_item: px(0.),
        });
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(HIGHLIGHT).await;
            let ease = motion::out_fn();
            let start = Instant::now();
            loop {
                let t = (start.elapsed().as_secs_f32() / HIGHLIGHT_FADE.as_secs_f32()).min(1.);
                let alpha = highlight_alpha_at(t, &ease);
                let current = this.update(cx, |this, cx| {
                    let current = this
                        .focus
                        .highlight
                        .as_ref()
                        .is_some_and(|(_, s)| *s == serial);
                    if current {
                        if alpha <= 0. {
                            this.focus.highlight = None;
                        }
                        this.focus.highlight_alpha = alpha;
                        cx.notify();
                    }
                    current
                });
                // A newer highlight took over, the view is gone, or done.
                if !matches!(current, Ok(true)) || alpha <= 0. {
                    break;
                }
                cx.background_executor().timer(FADE_FRAME).await;
            }
        })
        .detach();
    }
}

/// The highlight's strength at `t` (0 to 1) of the fade: 1 - out(t), and
/// exactly 0 at the end.
fn highlight_alpha_at(t: f32, ease: &impl Fn(f32) -> f32) -> f32 {
    if t >= 1. {
        0.
    } else {
        (1. - ease(t)).clamp(0., 1.)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn highlight_holds_two_seconds_then_fades_out_on_the_out_curve() {
        assert_eq!(HIGHLIGHT, Duration::from_millis(2000));
        assert_eq!(HIGHLIGHT_FADE, motion::SLOW);
        let ease = motion::out_fn();
        assert_eq!(highlight_alpha_at(0., &ease), 1.);
        assert_eq!(highlight_alpha_at(1., &ease), 0.);
        // The out curve drops fast first: past half way after a quarter.
        assert!(highlight_alpha_at(0.25, &ease) < 0.5);
        let mut last = 1.;
        for i in 1..=20 {
            let a = highlight_alpha_at(i as f32 / 20., &ease);
            assert!(a <= last, "the fade never comes back");
            last = a;
        }
    }
}
