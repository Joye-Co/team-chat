//! The topic view: header (title, status chip, panel toggle) and the message
//! timeline.
//!
//! The timeline is a GPUI `list` (variable row heights, virtualized, anchored
//! at the bottom, following the tail). GPUI's list keeps the measured height
//! of every row in a sum tree; rows that were not measured yet carry an
//! estimate ([`EST_ROW_HEIGHT`]), and a changed row is remeasured with its old
//! height as the estimate, so edits, acks and streaming keep the scroll
//! position. Message windows are turned into list updates by
//! [`model::diff`].
//!
//! Memory: only a window of the newest messages is held ([`WINDOW`] rows, more
//! while the user reads older ones). Near the top the window grows by
//! [`PAGE`] (from the provider's cache, or with `load_older`); back at the
//! bottom it shrinks to [`WINDOW`] again.
//!
//! Extension points: [`extras`] (content attached to a row) and the row
//! context menu in `timeline/menu.rs`. Quote replies are emitted as
//! [`TimelineEvent::Quote`] for the composer.

pub mod extras;
mod focus;
mod menu;
mod model;
mod row;
pub mod selftest;

use std::{
    cell::Cell,
    rc::Rc,
    time::{Duration, Instant},
};

use gpui::{
    AnyElement, Context, Entity, EventEmitter, FollowMode, InteractiveElement, IntoElement,
    ListAlignment, ListOffset, ListState, ParentElement, Pixels, Render, Styled, Subscription,
    Window, div, list, prelude::FluentBuilder as _, px,
};
use gpui_component::{
    Sizable as _,
    button::{Button, ButtonVariants as _},
    input::TextareaState,
    menu::DropdownMenu as _,
};

use crate::{
    app::{Changes, Services},
    copy::{fill, realtime as r, timeline as t},
    data::{
        Change, GroupId, Member, MemberLevel, Message, MessageId, SessionState, Topic, TopicId,
    },
    theme,
    views::{status_chip, topic_menu},
};

use model::ListOp;

/// Messages kept in memory while the user is at the bottom.
pub const WINDOW: usize = 100;
/// Messages added to the window when the user nears its top.
pub const PAGE: usize = 100;
/// Grow the window when the first visible row is closer than this to its top.
const LOAD_AHEAD: usize = 30;
/// Height estimate of a row that was not measured yet.
pub const EST_ROW_HEIGHT: Pixels = px(56.);
/// While a message streams, the timeline refreshes at most this often.
const STREAM_REFRESH: Duration = Duration::from_millis(100);

/// Events for other views.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TimelineEvent {
    /// The user chose 引用回复 on a message: the composer should quote it
    /// (`SendRequest::quote_of`).
    Quote {
        group_id: GroupId,
        topic_id: TopicId,
        message_id: MessageId,
    },
}

/// A message being edited in place.
struct Editing {
    key: String,
    message_id: MessageId,
    input: Entity<TextareaState>,
    _subscription: Subscription,
}

/// Paint times, for the timeline self-test.
#[derive(Default)]
pub struct FrameProbe {
    /// When the timeline finished painting its last frame.
    pub painted: Cell<Option<Instant>>,
}

pub struct TimelineView {
    services: Services,
    open: Option<(GroupId, TopicId)>,
    topic: Option<Topic>,
    members: Vec<Member>,
    me: Option<String>,
    i_am_owner: bool,
    typing: Vec<String>,
    /// The window in memory, oldest first.
    messages: Vec<Message>,
    /// Older messages exist (in the provider, or trimmed from the window).
    has_older: bool,
    /// What the provider returned last time (before trimming).
    provider_len: usize,
    provider_has_older: bool,
    provider_loading: bool,
    /// How many of the newest messages the window holds.
    wanted: usize,
    /// A grow of the window is running; cleared when it lands.
    older_requested: bool,
    /// The first unread message when the topic opened.
    divider_key: Option<String>,
    /// Unread messages when the topic opened (the jump button's count).
    unread_at_open: u32,
    /// The divider was on screen once, so the jump button is gone.
    unread_seen: bool,
    /// Messages that arrived while the user was scrolled up.
    new_below: usize,
    list: ListState,
    /// End of the visible range from the last user scroll.
    visible_end: Rc<Cell<usize>>,
    /// Highest seq this view already marked read.
    marked_seq: u64,
    last_refresh: Option<Instant>,
    refresh_scheduled: bool,
    editing: Option<Editing>,
    probe: Option<Rc<FrameProbe>>,
    activation: Option<Subscription>,
    /// Search: scroll to a message and highlight it (`timeline/focus.rs`).
    focus: focus::FocusState,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<TimelineEvent> for TimelineView {}

impl TimelineView {
    pub fn new(services: Services, cx: &mut Context<Self>) -> Self {
        let subscriptions = vec![
            cx.subscribe(&services.hub, |this, _, changes: &Changes, cx| {
                let Some((g, tid)) = this.open.clone() else {
                    return;
                };
                let touches = changes.touches_topic(&g, &tid)
                    || changes.any(|c| matches!(c, Change::Topics { group_id } if *group_id == g))
                    || changes.any(|c| matches!(c, Change::Members | Change::Session));
                if touches {
                    this.on_changes(cx);
                }
            }),
            cx.observe(&services.nav, |this, nav, cx| {
                let open = nav.read(cx).topic.clone();
                if open != this.open {
                    this.open_topic(open, cx);
                }
                let request = nav.read(cx).focus.clone();
                this.on_focus_request(request, cx);
                cx.notify();
            }),
        ];
        let list = ListState::new(0, ListAlignment::Bottom, px(800.));
        list.set_follow_mode(FollowMode::Tail);
        let visible_end: Rc<Cell<usize>> = Rc::default();
        let end = visible_end.clone();
        list.set_scroll_handler(move |event, _, _| end.set(event.visible_range.end));
        let mut this = Self {
            services,
            open: None,
            topic: None,
            members: Vec::new(),
            me: None,
            i_am_owner: false,
            typing: Vec::new(),
            messages: Vec::new(),
            has_older: false,
            provider_len: 0,
            provider_has_older: false,
            provider_loading: false,
            wanted: WINDOW,
            older_requested: false,
            divider_key: None,
            unread_at_open: 0,
            unread_seen: false,
            new_below: 0,
            list,
            visible_end,
            marked_seq: 0,
            last_refresh: None,
            refresh_scheduled: false,
            editing: None,
            probe: None,
            activation: None,
            focus: focus::FocusState::default(),
            _subscriptions: subscriptions,
        };
        let open = this.services.nav.read(cx).topic.clone();
        this.open_topic(open, cx);
        let request = this.services.nav.read(cx).focus.clone();
        this.on_focus_request(request, cx);
        this
    }

    /// Number of messages in the window (for self-tests).
    pub fn message_count(&self) -> usize {
        self.messages.len()
    }

    fn open_topic(&mut self, open: Option<(GroupId, TopicId)>, cx: &mut Context<Self>) {
        // Hold enough messages that the first unread one is in the window
        // (up to the most the window keeps while following the tail).
        let unread = open
            .as_ref()
            .and_then(|(g, t)| self.services.provider.topic(g, t))
            .map_or(0, |t| t.unread);
        self.open = open;
        self.messages.clear();
        self.list.reset(0);
        self.list.set_follow_mode(FollowMode::Tail);
        self.visible_end.set(0);
        self.wanted = (unread as usize + 10).clamp(WINDOW, WINDOW + PAGE);
        self.has_older = false;
        self.provider_len = 0;
        self.provider_has_older = false;
        self.provider_loading = false;
        self.older_requested = false;
        self.new_below = 0;
        self.marked_seq = 0;
        self.editing = None;
        self.focus.clear();
        self.refresh(cx);
        let me = self.me.clone();
        let read_seq = self
            .open
            .as_ref()
            .and_then(|(g, t)| self.services.provider.topic_read_seq(g, t));
        self.divider_key = self.topic.as_ref().and_then(|topic| match read_seq {
            Some(seq) if topic.unread > 0 => {
                model::first_unread_after(&self.messages, seq, self.has_older, me.as_deref())
            }
            Some(_) => None,
            None => model::first_unread_key(&self.messages, topic.unread, me.as_deref()),
        });
        self.unread_at_open = self.topic.as_ref().map_or(0, |t| t.unread);
        self.unread_seen = self.divider_key.is_none();
    }

    /// Scrolls so the first unread message (the divider) is at the top.
    fn jump_to_first_unread(&mut self) {
        if let Some(ix) = self.divider_key.as_deref().and_then(|k| self.row_index(k)) {
            self.list.scroll_to(ListOffset {
                item_ix: ix,
                offset_in_item: px(0.),
            });
        }
        self.unread_seen = true;
    }

    /// The divider is above the visible rows (the jump button shows).
    fn first_unread_above(&mut self) -> bool {
        if self.unread_seen {
            return false;
        }
        let Some(ix) = self.divider_key.as_deref().and_then(|k| self.row_index(k)) else {
            self.unread_seen = true;
            return false;
        };
        match self.list.item_is_above_viewport(ix) {
            Some(true) => true,
            Some(false) => {
                self.unread_seen = true;
                false
            }
            None => false,
        }
    }

    /// A provider change for this topic: refresh now, or within
    /// [`STREAM_REFRESH`] while a message streams.
    fn on_changes(&mut self, cx: &mut Context<Self>) {
        let streaming = self.messages.iter().rev().any(|m| m.streaming);
        if streaming && let Some(last) = self.last_refresh {
            let since = last.elapsed();
            if since < STREAM_REFRESH {
                if !self.refresh_scheduled {
                    self.refresh_scheduled = true;
                    let wait = STREAM_REFRESH - since;
                    cx.spawn(async move |this, cx| {
                        cx.background_executor().timer(wait).await;
                        let _ = this.update(cx, |this, cx| {
                            this.refresh_scheduled = false;
                            this.refresh(cx);
                            cx.notify();
                        });
                    })
                    .detach();
                }
                return;
            }
        }
        self.refresh(cx);
        cx.notify();
    }

    /// Re-reads the snapshot of the open topic and updates the list in place.
    fn refresh(&mut self, _cx: &mut Context<Self>) {
        self.last_refresh = Some(Instant::now());
        let p = self.services.provider.clone();
        let Some((g, tid)) = self.open.clone() else {
            self.topic = None;
            self.messages.clear();
            self.list.reset(0);
            return;
        };
        match p.session() {
            SessionState::SignedIn { me, .. } => {
                self.i_am_owner = me.level == MemberLevel::Owner;
                self.me = Some(me.id);
            }
            _ => {
                self.me = None;
                self.i_am_owner = false;
            }
        }
        self.members = p.members();
        self.topic = p.topic(&g, &tid);
        self.typing = p.typing(&g, &tid);

        let following = self.list.is_following_tail();
        let window = p.messages(&g, &tid, self.wanted);
        let was_loading = self.provider_loading;
        self.provider_len = window.messages.len();
        self.provider_has_older = window.has_older;
        self.provider_loading = window.loading_older;
        let mut messages = window.messages;
        let mut has_older = window.has_older;
        if messages.len() > self.wanted {
            messages.drain(..messages.len() - self.wanted);
            has_older = true;
        }

        let old_first = self.messages.first().map(|m| model::key(m).to_string());
        let appended = model::appended(&self.messages, &messages);
        let me = self.me.as_deref();
        let mine_appended = messages[messages.len() - appended..]
            .iter()
            .any(|m| Some(m.author_id.as_str()) == me);
        let others_appended = messages[messages.len() - appended..]
            .iter()
            .filter(|m| Some(m.author_id.as_str()) != me)
            .count();
        let ops = model::diff(&self.messages, &messages);
        self.apply(&ops);
        let grew_at_top =
            old_first.is_some() && messages.first().map(model::key) != old_first.as_deref();
        self.messages = messages;
        self.has_older = has_older;
        if grew_at_top || !has_older || (was_loading && !self.provider_loading) {
            self.older_requested = false;
        }

        if mine_appended {
            // My own new message: show it.
            self.jump_to_latest();
        } else if !following && appended > 0 {
            self.new_below += others_appended;
            // Keep what the user reads in the window.
            self.wanted += appended;
        }
        if let Some(editing) = &self.editing
            && !self
                .messages
                .iter()
                .any(|m| model::key(m) == editing.key && m.deleted_at.is_none())
        {
            self.editing = None;
        }
        self.continue_focus(_cx);
    }

    fn apply(&self, ops: &[ListOp]) {
        let mut estimate = false;
        for op in ops {
            match op {
                ListOp::Reset(count) => {
                    self.list.reset(*count);
                    estimate = true;
                }
                ListOp::Splice { range, count } => {
                    self.list.splice(range.clone(), *count);
                    estimate |= *count > 1;
                }
                ListOp::Remeasure(ix) => self.list.remeasure_items(*ix..*ix + 1),
            }
        }
        if estimate {
            // Rows not measured yet get a non-zero height estimate.
            let _ = self.list.clone().with_uniform_item_height(EST_ROW_HEIGHT);
        }
    }

    fn jump_to_latest(&mut self) {
        self.new_below = 0;
        self.list.set_follow_mode(FollowMode::Tail);
        self.list.scroll_to_end();
    }

    /// Grows the window by one page (from the cache, or by asking the
    /// provider for the page before it).
    fn grow_window(&mut self, cx: &mut Context<Self>) {
        self.wanted += PAGE;
        if self.provider_len < self.wanted
            && self.provider_has_older
            && !self.provider_loading
            && let Some((g, t)) = &self.open
        {
            self.services.provider.load_older(g, t);
        }
        self.refresh(cx);
        cx.notify();
    }

    /// Back at the bottom: drop the old rows the user scrolled through.
    fn shrink_window(&mut self, cx: &mut Context<Self>) {
        if self.wanted > WINDOW && self.list.is_following_tail() {
            self.wanted = WINDOW;
            self.refresh(cx);
            cx.notify();
        }
    }

    /// Work that depends on where the list is scrolled: grow or shrink the
    /// window, reset the new-message count, mark read.
    fn after_scroll(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.messages.is_empty() {
            return;
        }
        let following = self.list.is_following_tail();
        let top = self.list.logical_scroll_top().item_ix;
        if self.has_older && !self.older_requested && top < LOAD_AHEAD && !following
            || self.has_older && !self.older_requested && self.messages.len() < LOAD_AHEAD
        {
            self.older_requested = true;
            cx.defer_in(window, |this, _, cx| this.grow_window(cx));
        }
        if following {
            self.new_below = 0;
            if self.wanted > WINDOW && self.messages.len() > WINDOW + PAGE {
                cx.defer_in(window, |this, _, cx| this.shrink_window(cx));
            }
        }
        self.mark_read(following, window);
    }

    /// Marks read up to the newest visible message while the window is active.
    fn mark_read(&mut self, following: bool, window: &Window) {
        let (Some((g, tid)), Some(topic)) = (&self.open, &self.topic) else {
            return;
        };
        if topic.unread == 0 || !window.is_window_active() {
            return;
        }
        let seq = if following {
            topic.last_seq
        } else {
            let end = self.visible_end.get().min(self.messages.len());
            self.messages[..end]
                .iter()
                .map(|m| m.created_seq)
                .max()
                .unwrap_or(0)
        };
        if seq > self.marked_seq {
            self.marked_seq = seq;
            self.services.provider.mark_read(g, tid, seq);
        }
    }

    fn quote(&mut self, message: &Message, cx: &mut Context<Self>) {
        cx.emit(TimelineEvent::Quote {
            group_id: message.group_id.clone(),
            topic_id: message.topic_id.clone(),
            message_id: message.id.clone(),
        });
    }

    fn row_index(&self, key: &str) -> Option<usize> {
        self.messages.iter().rposition(|m| model::key(m) == key)
    }

    fn remeasure_key(&self, key: &str) {
        if let Some(ix) = self.row_index(key) {
            self.list.remeasure_items(ix..ix + 1);
        }
    }

    fn render_header(&self, topic: &Topic, cx: &mut Context<Self>) -> impl IntoElement {
        let colors = theme::colors(cx);
        let following = self
            .services
            .provider
            .topic_following(&topic.group_id, &topic.id);
        div()
            .h(px(48.))
            .flex_none()
            .px(px(16.))
            .flex()
            .items_center()
            .gap(px(8.))
            .border_b_1()
            .border_color(colors.border)
            .child(
                div()
                    .text_base()
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .text_ellipsis()
                    .child(topic.title.clone()),
            )
            .when_some(
                status_chip(topic, self.me.as_deref(), &self.members, cx),
                |el, chip| el.child(chip),
            )
            .when_some(topic_menu::owner_text(topic, &self.members), |el, text| {
                el.child(topic_menu::header_label(text, cx))
            })
            .child(
                div()
                    .text_xs()
                    .text_color(colors.muted_foreground)
                    .child(fill(
                        t::MESSAGE_COUNT,
                        &[("n", &topic.message_count.to_string())],
                    )),
            )
            .child(div().flex_1())
            .when(following, |el| {
                el.child(topic_menu::header_label(r::FOLLOWING, cx))
            })
            .child({
                let services = self.services.clone();
                let (g, tid) = (topic.group_id.clone(), topic.id.clone());
                Button::new("topic-status")
                    .small()
                    .ghost()
                    .label(r::STATUS_MENU)
                    .dropdown_menu(move |menu, window, cx| {
                        // Read the topic again: the menu shows the newest state.
                        match services.provider.topic(&g, &tid) {
                            Some(topic) => {
                                topic_menu::build(menu, &services, &g, &topic, window, cx)
                            }
                            None => menu,
                        }
                    })
            })
            .child(
                Button::new("toggle-panel")
                    .small()
                    .ghost()
                    .label(t::TOGGLE_PANEL)
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.services
                            .nav
                            .update(cx, |nav, cx| nav.toggle_side_panel(cx))
                    })),
            )
    }

    fn typing_text(&self) -> Option<String> {
        match self.typing.as_slice() {
            [] => None,
            [one] => {
                let name = self
                    .members
                    .iter()
                    .find(|m| &m.id == one)
                    .map(|m| m.name.clone())
                    .unwrap_or_default();
                Some(fill(t::TYPING_ONE, &[("name", &name)]))
            }
            many => Some(fill(t::TYPING_MANY, &[("n", &many.len().to_string())])),
        }
    }

    fn probe_element(&self) -> Option<AnyElement> {
        self.probe
            .clone()
            .map(|p| selftest::PaintProbe(p).into_any_element())
    }
}

impl Render for TimelineView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self.activation.is_none() {
            self.activation = Some(cx.observe_window_activation(window, |_, _, cx| cx.notify()));
        }
        let colors = theme::colors(cx);
        let Some(topic) = self.topic.clone() else {
            let hint = if self.services.nav.read(cx).current_group().is_some() {
                t::NEW_TOPIC_HINT
            } else {
                t::SELECT_TOPIC
            };
            return div()
                .size_full()
                .flex()
                .items_center()
                .justify_center()
                .text_color(colors.muted_foreground)
                .child(hint)
                .children(self.probe_element())
                .into_any_element();
        };
        self.after_scroll(window, cx);

        let view: Entity<Self> = cx.entity();
        let timeline = list(self.list.clone(), move |ix, window, cx| {
            view.update(cx, |this, cx| this.render_row(ix, window, cx))
        })
        .size_full();

        let following = self.list.is_following_tail();
        let jump = (!following && !self.messages.is_empty()).then(|| {
            let label = if self.new_below > 0 {
                fill(t::JUMP_NEW, &[("n", &self.new_below.to_string())])
            } else {
                t::JUMP_LATEST.to_string()
            };
            div().absolute().bottom(px(12.)).right(px(16.)).child(
                Button::new("jump-latest")
                    .small()
                    .primary()
                    .label(label)
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.jump_to_latest();
                        cx.notify();
                    })),
            )
        });
        let jump_unread = self.first_unread_above().then(|| {
            div()
                .absolute()
                .top(px(8.))
                .left_0()
                .right_0()
                .flex()
                .justify_center()
                .child(
                    Button::new("jump-unread")
                        .small()
                        .primary()
                        .label(fill(
                            r::JUMP_UNREAD,
                            &[("n", &self.unread_at_open.to_string())],
                        ))
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.jump_to_first_unread();
                            cx.notify();
                        })),
                )
        });
        let loading = (self.provider_loading && self.has_older).then(|| {
            div()
                .absolute()
                .top(px(4.))
                .left_0()
                .right_0()
                .flex()
                .justify_center()
                .text_xs()
                .text_color(colors.muted_foreground)
                .child(t::LOADING_OLDER)
        });

        div()
            .id("timeline")
            .size_full()
            .flex()
            .flex_col()
            .child(self.render_header(&topic, cx))
            .child(
                div()
                    .relative()
                    .flex_1()
                    .min_h(px(0.))
                    .child(timeline)
                    .children(loading)
                    .children(jump_unread)
                    .children(jump),
            )
            .child(
                div()
                    .h(px(18.))
                    .flex_none()
                    .px(px(16.))
                    .text_xs()
                    .text_color(colors.muted_foreground)
                    .when_some(self.typing_text(), |el, text| el.child(text)),
            )
            .children(self.probe_element())
            .into_any_element()
    }
}
