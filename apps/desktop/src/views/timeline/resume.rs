//! Continuity across 话题: each 话题 reopens where the person left it, and a
//! 话题 with more unread messages than fit on screen opens at the first one.
//!
//! One [`TimelineView`] shows every 话题 in turn, so leaving one used to drop
//! its place. The place is kept as an anchor (the message at the top of the
//! viewport and how far into it the viewport starts) plus the window size
//! that held it; a 话题 left at its newest message keeps no anchor and opens
//! at the tail again.

use std::time::Duration;

use gpui::{
    AnyElement, Context, FollowMode, IntoElement, ListOffset, ParentElement as _, Pixels,
    StatefulInteractiveElement as _, Styled as _, Window, div, px,
};

use super::{TimelineView, model};
use crate::{
    app::{Location, Place, Services},
    copy::{fill, rail as rc, timeline as t},
    theme::space,
};

/// How long the 返回 pill stays after a jump to a message.
const CAME_FROM_SHOWN: Duration = Duration::from_secs(8);
/// Most characters of a 话题 title in the 返回 pill.
const TITLE_CHARS: usize = 14;

/// Where a 话题 was left.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct Anchor {
    /// Key of the message at the top of the viewport.
    key: String,
    /// How far the viewport starts into that message.
    offset: Pixels,
    /// Messages the window held then, so the anchor is in it again.
    wanted: usize,
}

/// Renders to wait after a 话题 opens before deciding where unread
/// messages land: the first one still shows the previous layout.
const LAND_FRAMES: u8 = 1;

impl TimelineView {
    /// Remembers where the open 话题 is scrolled (call before it closes).
    pub(super) fn remember_place(&mut self) {
        let Some(open) = self.open.clone() else {
            return;
        };
        match anchor_of(
            self.list.is_following_tail(),
            self.list.logical_scroll_top(),
            &self.messages,
            self.wanted,
        ) {
            Some(anchor) => {
                self.resume.insert(open, anchor);
            }
            None => {
                self.resume.remove(&open);
            }
        }
    }

    /// The window size the remembered place of the open 话题 needs.
    pub(super) fn resume_wanted(&self) -> Option<usize> {
        let open = self.open.as_ref()?;
        self.resume.get(open).map(|a| a.wanted)
    }

    /// Scrolls back to the remembered place of the open 话题. `true` when
    /// it did; a place whose message is gone is forgotten.
    pub(super) fn restore_place(&mut self) -> bool {
        let Some(open) = self.open.clone() else {
            return false;
        };
        let Some(anchor) = self.resume.get(&open).cloned() else {
            return false;
        };
        let Some(ix) = self.row_index(&anchor.key) else {
            self.resume.remove(&open);
            return false;
        };
        self.list.set_follow_mode(FollowMode::Normal);
        self.list.scroll_to(ListOffset {
            item_ix: ix,
            offset_in_item: anchor.offset,
        });
        true
    }

    /// Asks the next renders to land on the first unread message when it
    /// is above what the tail shows.
    pub(super) fn plan_unread_landing(&mut self) {
        self.land_unread = self.divider_key.is_some().then_some(LAND_FRAMES);
    }

    /// Call from `render` with whether the divider row was drawn last
    /// frame: once the list has laid out the 话题 at its tail, a divider
    /// that did not show is more than a screen up, so the list moves to it.
    pub(super) fn land_on_unread(&mut self, divider_drawn: bool, window: &mut Window) {
        let Some(frames) = self.land_unread else {
            return;
        };
        if frames > 0 {
            self.land_unread = Some(frames - 1);
            window.request_animation_frame();
            return;
        }
        self.land_unread = None;
        if !divider_drawn && !self.unread_seen && self.list.is_following_tail() {
            self.jump_to_first_unread();
            window.request_animation_frame();
        }
    }
}

impl TimelineView {
    /// A jump to one message (a reference, a card, a notification) landed
    /// here from another place: offer the way back for a moment. A search
    /// has its own bar, so it gets none.
    pub(super) fn note_came_from(
        &mut self,
        serial: u64,
        from_search: bool,
        cx: &mut Context<Self>,
    ) {
        self.came_from = None;
        if from_search {
            return;
        }
        let nav = self.services.nav.read(cx);
        let Some(previous) = nav.previous().cloned() else {
            return;
        };
        if previous.topic.is_some() && previous.topic == self.open {
            return;
        }
        let Some(name) = place_name(&self.services, &previous) else {
            return;
        };
        self.came_from = Some((name, serial));
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(CAME_FROM_SHOWN).await;
            let _ = this.update(cx, |this, cx| {
                if this.came_from.as_ref().is_some_and(|(_, s)| *s == serial) {
                    this.came_from = None;
                    cx.notify();
                }
            });
        })
        .detach();
    }

    /// The 返回 pill at the top of the thread, while it shows.
    pub(super) fn render_came_from(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let (name, _) = self.came_from.as_ref()?;
        let label = fill(t::JUMP_BACK, &[("place", name)]);
        Some(
            div()
                .absolute()
                .top(space::S12)
                .left_0()
                .right_0()
                .flex()
                .justify_center()
                .child(
                    super::back_pill("came-from", label, cx).on_click(cx.listener(
                        |this, _, _, cx| {
                            this.came_from = None;
                            this.services.nav.update(cx, |nav, cx| nav.go_back(cx));
                        },
                    )),
                )
                .into_any_element(),
        )
    }
}

/// For the navigation self-test (`views/nav/selftest.rs`).
impl TimelineView {
    /// Scrolls the thread up by `distance`, as a wheel does.
    pub fn selftest_scroll_up(&mut self, distance: Pixels, cx: &mut Context<Self>) {
        self.list.scroll_by(-distance);
        cx.notify();
    }

    /// Whether the thread follows its newest message, and the key of the
    /// message at the top of the viewport.
    pub fn selftest_place(&self) -> (bool, Option<String>) {
        let top = self.list.logical_scroll_top();
        (
            self.list.is_following_tail(),
            self.messages
                .get(top.item_ix)
                .map(|m| model::key(m).to_string()),
        )
    }

    /// Whether the 返回 pill shows.
    pub fn selftest_came_from(&self) -> bool {
        self.came_from.is_some()
    }
}

/// What the 返回 pill calls a place: the 话题's title, the 群's name or the
/// page's name.
fn place_name(services: &Services, place: &Location) -> Option<String> {
    let p = services.provider.as_ref();
    if let Some((g, topic)) = &place.topic {
        if crate::data::is_main_topic(topic) {
            return group_name(services, g);
        }
        let title = crate::data::topic_or_chat(p, g, topic)?.title;
        return Some(fill(t::JUMP_BACK_TOPIC, &[("title", &clip(&title))]));
    }
    match place.place.as_ref()? {
        Place::Desk | Place::Todo => Some(rc::RAIL_DESK.to_string()),
        Place::Members(_) => Some(rc::RAIL_MEMBERS.to_string()),
        Place::Artifacts | Place::Artifact { .. } => Some(rc::RAIL_ARTIFACTS.to_string()),
        Place::Group(g) => group_name(services, g),
    }
}

fn group_name(services: &Services, group: &str) -> Option<String> {
    services
        .provider
        .groups()
        .into_iter()
        .find(|g| g.id == group)
        .map(|g| clip(&g.name))
}

/// `text` cut to [`TITLE_CHARS`] characters with an ellipsis.
fn clip(text: &str) -> String {
    if text.chars().count() <= TITLE_CHARS {
        return text.to_string();
    }
    let mut cut: String = text.chars().take(TITLE_CHARS - 1).collect();
    cut.push('…');
    cut
}

/// The anchor to keep for a list scrolled to `top`; none while it follows
/// the tail (the 话题 opens at its newest message again).
fn anchor_of(
    following: bool,
    top: ListOffset,
    messages: &[crate::data::Message],
    wanted: usize,
) -> Option<Anchor> {
    if following {
        return None;
    }
    let message = messages.get(top.item_ix)?;
    Some(Anchor {
        key: model::key(message).to_string(),
        offset: top.offset_in_item.max(px(0.)),
        wanted,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn message(id: &str) -> crate::data::Message {
        model::tests::msg(id, "a", 1)
    }

    #[test]
    fn long_titles_are_clipped_with_an_ellipsis() {
        assert_eq!(clip("短标题"), "短标题");
        let long = "一二三四五六七八九十一二三四五六";
        let cut = clip(long);
        assert_eq!(cut.chars().count(), TITLE_CHARS);
        assert!(cut.ends_with('…'));
    }

    #[test]
    fn a_topic_left_at_its_tail_keeps_no_place() {
        let messages = [message("a"), message("b")];
        let top = ListOffset {
            item_ix: 1,
            offset_in_item: px(10.),
        };
        assert_eq!(anchor_of(true, top, &messages, 100), None);
    }

    #[test]
    fn a_topic_left_scrolled_up_keeps_the_top_message_and_window() {
        let messages = [message("a"), message("b"), message("c")];
        let top = ListOffset {
            item_ix: 1,
            offset_in_item: px(12.),
        };
        let anchor = anchor_of(false, top, &messages, 200).expect("an anchor");
        assert_eq!(anchor.key, model::key(&messages[1]));
        assert_eq!(anchor.offset, px(12.));
        assert_eq!(anchor.wanted, 200);
    }

    #[test]
    fn a_top_past_the_rows_keeps_no_place() {
        let messages = [message("a")];
        let top = ListOffset {
            item_ix: 3,
            offset_in_item: px(0.),
        };
        assert_eq!(anchor_of(false, top, &messages, 100), None);
    }
}
