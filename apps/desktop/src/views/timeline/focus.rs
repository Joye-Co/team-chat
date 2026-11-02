//! Opening a topic at one message (from search): grow the window until the
//! message is in it, scroll to it and highlight it for a moment.

use std::time::Duration;

use gpui::{Context, FollowMode, ListOffset, px};

use super::{PAGE, TimelineView, model};
use crate::app::FocusRequest;

/// How long the opened message stays highlighted.
const HIGHLIGHT: Duration = Duration::from_millis(2400);
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

    pub(super) fn is_highlighted(&self, key: &str) -> bool {
        self.focus.highlight.as_ref().is_some_and(|(k, _)| k == key)
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
        self.list.set_follow_mode(FollowMode::Normal);
        self.list.scroll_to(ListOffset {
            item_ix: ix.saturating_sub(1),
            offset_in_item: px(0.),
        });
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(HIGHLIGHT).await;
            let _ = this.update(cx, |this, cx| {
                if this
                    .focus
                    .highlight
                    .as_ref()
                    .is_some_and(|(_, s)| *s == serial)
                {
                    this.focus.highlight = None;
                    cx.notify();
                }
            });
        })
        .detach();
    }
}
