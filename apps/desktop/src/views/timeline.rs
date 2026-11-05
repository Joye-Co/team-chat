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
mod header;
mod live;
mod menu;
pub(crate) mod model;
pub mod plan;
pub mod preview;
mod row;
// --- v12/fx-topic: Messages-style tapbacks ---
mod tapback;
// --- end v12/fx-topic ---
pub mod selftest;
pub mod v2;

use std::{
    cell::Cell,
    rc::Rc,
    time::{Duration, Instant},
};

use gpui::{
    AnyElement, App, Context, Entity, EventEmitter, FollowMode, InteractiveElement, IntoElement,
    ListAlignment, ListOffset, ListState, ParentElement, Pixels, Render,
    StatefulInteractiveElement as _, Styled, Subscription, Window, div, list, px,
};
use gpui_component::input::TextareaState;

use crate::{
    app::{Changes, Services},
    copy::{fill, timeline as t},
    data::{
        Change, GroupId, Member, MemberLevel, Message, MessageId, SessionState, Topic, TopicId,
    },
    theme::{self, Type as _, space},
    ui::{self, Sym},
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
    /// Draw the v1 header band (the v2 shell draws its own title bar).
    show_header: bool,
    /// The v2 style of the 话题 page (`timeline/v2.rs`).
    v2: bool,
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
    /// Height of the start block's title last frame (one or two lines).
    start_title_h: Rc<Cell<Pixels>>,
    /// How much of row 0 is above the viewport, read before the list lays
    /// out (the list's state is borrowed while it renders rows). Only used
    /// while row 0 is the first row of the topic (the start block).
    start_cut: Pixels,
    /// Highest seq this view already marked read.
    marked_seq: u64,
    last_refresh: Option<Instant>,
    refresh_scheduled: bool,
    editing: Option<Editing>,
    probe: Option<Rc<FrameProbe>>,
    activation: Option<Subscription>,
    /// Search: scroll to a message and highlight it (`timeline/focus.rs`).
    focus: focus::FocusState,
    /// The message row under the pointer (its hover toolbar shows).
    hovered: Option<String>,
    /// R2-12, E3: the thread is a Tab stop; ↑↓ move the row whose toolbar
    /// shows (the same toolbar as under the pointer).
    key_focus: gpui::FocusHandle,
    /// The arrow keys set `hovered` (the pointer clears this), and the
    /// thread has the keyboard focus this frame: the row draws the ring.
    keyboard_row: bool,
    key_focused: bool,
    /// The emoji picker is open for this message row: from the hover
    /// toolbar (`false`) or from the add pill under the reactions (`true`).
    reacting: Option<(String, bool)>,
    /// Bot runs of the open topic, newest first.
    runs: Vec<crate::data::BotRunSummary>,
    /// Working runs at the last refresh, rows of runs that just ended (with
    /// the time they ended) and when each run's row first showed
    /// (`timeline/live.rs`).
    live_shown: Vec<crate::data::BotRunSummary>,
    live_leaving: Vec<(crate::data::BotRunSummary, Instant)>,
    live_entered: std::collections::HashMap<String, Instant>,
    /// Opening a topic: its pending rows show without an animation.
    live_quiet: bool,
    /// Messages from others that arrived while the list followed the tail
    /// in the focused window, with the time they arrived: they fade in once
    /// (§12, 120 ms opacity, no rise).
    arrived: std::collections::HashMap<String, Instant>,
    /// The window was the key window at the last render.
    window_active: bool,
    /// Runs whose log this view asked for once (steps and live labels).
    followed_runs: std::collections::HashSet<String>,
    /// Steps rows the user opened.
    open_steps: std::collections::HashSet<String>,
    /// Plan cards the user folded (run ids; `timeline/plan.rs`).
    plan_collapsed: std::collections::HashSet<String>,
    /// A one-second tick runs while a bot works (the elapsed time).
    ticking: bool,
    /// A card was in the rows painted last frame (the header hides the
    /// 等你… label then).
    card_visible: bool,
    /// A card row was drawn since the last render (set by `render_row`).
    card_seen: bool,
    /// The message ids of the card rows painted last frame, and those drawn
    /// since the last render: the pinned banner gives way while the card it
    /// points at is on screen (R2-09).
    cards_shown: std::collections::BTreeSet<String>,
    cards_drawn: std::collections::BTreeSet<String>,
    /// The new-messages divider row was drawn since the last render: it is
    /// on screen, so the jump pill must not show (§8.1, U14).
    divider_painted: bool,
    /// T-01: where each drawn row's own date label sits (`row.rs`), so the
    /// sticky date pill shows only once that label has gone under it.
    date_marks: Rc<std::cell::RefCell<DateMarks>>,
    /// T-15: the row whose own date label the sticky pill stands for this
    /// frame; its divider is not drawn, so no slice of it shows above the
    /// pill.
    sticky_row: Option<String>,
    _subscriptions: Vec<Subscription>,
}

/// The painted places of the rows and of their own date labels (window
/// space, from the last frame).
#[derive(Default)]
pub(super) struct DateMarks {
    /// The row's top in this frame (its mark paints before its label's).
    row_top: std::collections::HashMap<String, Pixels>,
    /// The middle of the row's own date label, from the row's top.
    label_mid: std::collections::HashMap<String, Pixels>,
}

impl EventEmitter<TimelineEvent> for TimelineView {}

impl TimelineView {
    /// Shows or hides the v1 header band (the v2 shell has its own title bar).
    pub fn set_show_header(&mut self, show: bool, cx: &mut Context<Self>) {
        if self.show_header != show {
            self.show_header = show;
            cx.notify();
        }
    }

    /// Switches to the v2 style of the 话题 page (`timeline/v2.rs`).
    pub fn set_v2(&mut self, v2: bool, cx: &mut Context<Self>) {
        if self.v2 != v2 {
            self.v2 = v2;
            // Rows change height: measure again, but keep the place (a
            // search hit opened before the page switched style).
            let following = self.list.is_following_tail();
            let top = self.list.logical_scroll_top();
            self.list.reset(self.messages.len());
            if following {
                self.list.set_follow_mode(FollowMode::Tail);
            } else {
                self.list.scroll_to(top);
            }
            cx.notify();
        }
    }

    pub fn new(services: Services, cx: &mut Context<Self>) -> Self {
        let subscriptions = vec![
            cx.subscribe(&services.hub, |this, _, changes: &Changes, cx| {
                let Some((g, tid)) = this.open.clone() else {
                    return;
                };
                let touches = changes.touches_topic(&g, &tid)
                    || changes.any(|c| match c {
                        Change::Topics { group_id } => *group_id == g,
                        Change::Runs { group_id, topic_id } => *group_id == g && *topic_id == tid,
                        Change::RunLog { .. }
                        | Change::Members
                        | Change::Session
                        | Change::Groups => true,
                        _ => false,
                    });
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
        // Top-aligned and following the tail: a long 话题 opens at its
        // newest message, a short one reads from its start block at the top
        // (no blank band above it).
        let list = ListState::new(0, ListAlignment::Top, px(800.));
        list.set_follow_mode(FollowMode::Tail);
        let visible_end: Rc<Cell<usize>> = Rc::default();
        let end = visible_end.clone();
        list.set_scroll_handler(move |event, _, _| end.set(event.visible_range.end));
        let mut this = Self {
            services,
            show_header: true,
            v2: false,
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
            start_title_h: Rc::new(Cell::new(theme::type_scale::DISPLAY.1)),
            start_cut: px(0.),
            marked_seq: 0,
            last_refresh: None,
            refresh_scheduled: false,
            editing: None,
            probe: None,
            activation: None,
            focus: focus::FocusState::default(),
            hovered: None,
            key_focus: cx.focus_handle().tab_index(0).tab_stop(true),
            keyboard_row: false,
            key_focused: false,
            reacting: None,
            runs: Vec::new(),
            live_shown: Vec::new(),
            live_leaving: Vec::new(),
            live_entered: Default::default(),
            live_quiet: false,
            arrived: Default::default(),
            window_active: false,
            followed_runs: Default::default(),
            open_steps: Default::default(),
            plan_collapsed: Default::default(),
            ticking: false,
            card_visible: false,
            card_seen: false,
            cards_shown: Default::default(),
            cards_drawn: Default::default(),
            divider_painted: false,
            date_marks: Default::default(),
            sticky_row: None,
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
        self.hovered = None;
        self.open_steps.clear();
        *self.date_marks.borrow_mut() = DateMarks::default();
        self.focus.clear();
        self.reset_live();
        self.live_quiet = true;
        self.refresh(cx);
        self.live_quiet = false;
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
        self.topic = crate::data::topic_or_chat(p.as_ref(), &g, &tid);
        self.typing = p.typing(&g, &tid);
        self.runs = p.runs(&g, &tid);
        for run in &self.runs {
            if self.followed_runs.insert(run.id.clone()) {
                p.follow_run(&run.id);
            }
        }

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
        // Row 0 draws the start block, or the older-messages skeleton while
        // they load: measure it again when either changes.
        if (self.has_older != has_older || was_loading != self.provider_loading)
            && !self.messages.is_empty()
        {
            self.list.remeasure_items(0..1);
        }
        self.has_older = has_older;
        if grew_at_top || !has_older || (was_loading && !self.provider_loading) {
            self.older_requested = false;
        }

        // New messages from others fade in while the person looks at the
        // end of the topic; not a batch of more than 3 and not in a window
        // in the background (§12).
        self.arrived
            .retain(|_, at| at.elapsed() < theme::motion::FAST * 4);
        if following && self.window_active && (1..=3).contains(&others_appended) {
            let now = Instant::now();
            for m in &self.messages[self.messages.len() - appended..] {
                if Some(m.author_id.as_str()) != me {
                    self.arrived.insert(model::key(m).to_string(), now);
                }
            }
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
        self.track_live(self.live_quiet, _cx);
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

    /// In the v2 style, `el` (drawn 24 pt from the edges) sits in the
    /// centered 720 pt column of the 话题 page.
    fn v2_column(&self, el: AnyElement) -> AnyElement {
        if !self.v2 {
            return el;
        }
        // `el` (the typing line) starts its text 38 pt in; move it so the
        // text lines up with message text: the shared leading inset, the
        // 28 pt avatar and the 12 pt gap.
        div()
            .w_full()
            .relative()
            .left(v2::LEADING + v2::GUTTER + space::S12 - theme::CONTENT_GUTTER - space::S14)
            .pr(v2::SIDE - theme::CONTENT_GUTTER)
            .child(
                div()
                    .w_full()
                    .max_w(v2::COLUMN + theme::CONTENT_GUTTER * 2.)
                    .child(el),
            )
            .into_any_element()
    }

    fn probe_element(&self) -> Option<AnyElement> {
        self.probe
            .clone()
            .map(|p| selftest::PaintProbe(p).into_any_element())
    }
}

/// The key context of the thread.
const KEY_CONTEXT: &str = "Timeline";

/// ↑↓ in a focused thread (call once at start-up).
pub fn bind_keys(cx: &mut App) {
    cx.bind_keys([
        gpui::KeyBinding::new("up", crate::views::rail::SelectPrevious, Some(KEY_CONTEXT)),
        gpui::KeyBinding::new("down", crate::views::rail::SelectNext, Some(KEY_CONTEXT)),
    ]);
}

impl TimelineView {
    /// Moves the keyboard row `step` rows (from the newest when none is
    /// set): it gets the hover band and the message toolbar, whose buttons
    /// Tab then reaches.
    fn move_keyboard_row(&mut self, step: isize, cx: &mut Context<Self>) {
        let Some(last) = self.messages.len().checked_sub(1) else {
            return;
        };
        let current = self
            .hovered
            .as_deref()
            .and_then(|k| self.messages.iter().position(|m| model::key(m) == k));
        let ix = match current {
            Some(i) => i.saturating_add_signed(step).min(last),
            None => last,
        };
        let key = model::key(&self.messages[ix]).to_string();
        if self.hovered.as_deref() != Some(key.as_str()) || !self.keyboard_row {
            // The row's look changes: measure it again (the list caches it).
            if let Some(old) = current {
                self.list.remeasure_items(old..old + 1);
            }
            self.list.remeasure_items(ix..ix + 1);
        }
        self.hovered = Some(key);
        self.keyboard_row = true;
        self.list.scroll_to_reveal_item(ix);
        cx.notify();
    }

    /// The card of message `id` was on screen in the last frame.
    pub fn shows_card(&self, id: &str) -> bool {
        self.cards_shown.contains(id)
    }

    /// The message ids of the card rows painted last frame.
    pub fn cards_on_screen(&self) -> impl Iterator<Item = &str> {
        self.cards_shown.iter().map(String::as_str)
    }
}

impl Render for TimelineView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self.activation.is_none() {
            self.activation = Some(cx.observe_window_activation(window, |_, _, cx| cx.notify()));
        }
        self.window_active = window.is_window_active();
        self.key_focused = self.key_focus.is_focused(window);
        let colors = theme::colors(cx);
        let Some(topic) = self.topic.clone() else {
            return div()
                .size_full()
                .flex()
                .flex_col()
                .bg(colors.content)
                .child(ui::header_band(cx).child(ui::toolbar_band("topic-toolbar-empty")))
                .child(
                    div()
                        .flex_1()
                        .flex()
                        .items_center()
                        .justify_center()
                        .px(theme::CONTENT_GUTTER)
                        .t_body()
                        .text_color(colors.text_2)
                        .child(t::SELECT_TOPIC),
                )
                .children(self.probe_element())
                .into_any_element();
        };
        self.after_scroll(window, cx);
        self.place_revealed(cx);
        self.keep_ticking(cx);
        // The list lays out after the header: when what it painted changes
        // whether a card is on screen, draw the header again.
        // The divider row drawn last frame is on screen: the unread
        // messages are seen, and the jump pill goes (or never shows).
        if std::mem::take(&mut self.divider_painted) && !self.unread_seen {
            self.unread_seen = true;
            cx.defer_in(window, |_, _, cx| cx.notify());
        }
        let shown = std::mem::take(&mut self.cards_drawn);
        if shown != self.cards_shown {
            self.cards_shown = shown;
            cx.defer_in(window, |_, _, cx| cx.notify());
        }
        let card_visible = std::mem::take(&mut self.card_seen);
        if card_visible != self.card_visible {
            self.card_visible = card_visible;
            cx.defer_in(window, |_, _, cx| cx.notify());
        }

        // The content's scroll offset in pixels (also right while the list
        // follows the tail, when its logical top is past the last row).
        self.start_cut = -self.list.scroll_px_offset_for_scrollbar().y;
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
            // Its own strip under the list, so it never covers a row.
            div()
                .flex_none()
                .pt(space::S8)
                .pb(space::S4)
                .flex()
                .justify_center()
                .child(
                    jump_pill("jump-latest", label, Sym::ArrowDown, cx).on_click(cx.listener(
                        |this, _, _, cx| {
                            this.jump_to_latest();
                            cx.notify();
                        },
                    )),
                )
        });
        let search_bar = self.render_search_bar(cx);
        let jump_unread = self.first_unread_above().then(|| {
            div()
                .absolute()
                .top(space::S12)
                .left_0()
                .right_0()
                .flex()
                .justify_center()
                .child(
                    jump_pill(
                        "jump-unread",
                        fill(t::JUMP_UNREAD, &[("n", &self.unread_at_open.to_string())]),
                        Sym::ArrowUp,
                        cx,
                    )
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.jump_to_first_unread();
                        cx.notify();
                    })),
                )
        });
        // §2.14: older messages loading show 2 message skeleton rows in the
        // flow above the first message: row 0 draws them, so they scroll
        // with the messages and never clip the first one (`row.rs`).
        // §2.17, R2-01: a 话题 with no messages still opens with its start
        // block and a compact "no messages" line, never a blank column.
        let empty = (self.v2 && self.messages.is_empty() && !self.has_older)
            .then(|| self.render_empty_thread(&topic, cx));
        // §2.17, T-01: once the day label of the first row on screen has
        // gone under the header line, that day floats as a pill (h24, float
        // surface) 2 under it. It is the label's own place, so the same date
        // never shows twice, and nothing behind the pill but the label hides.
        let sticky_date = (self.v2 && jump_unread.is_none())
            .then(|| self.list.logical_scroll_top())
            .filter(|top| self.date_label_gone(top.item_ix, top.offset_in_item))
            .and_then(|top| self.messages.get(top.item_ix));
        self.sticky_row = sticky_date.map(|m| model::key(m).to_string());
        let sticky_date = sticky_date.map(|m| {
            use crate::ui::native::{NativeType as _, native_colors};
            let n = native_colors(cx);
            div()
                .absolute()
                .top(STICKY_TOP)
                .left_0()
                .right_0()
                .pl(v2::LEADING)
                .pr(v2::SIDE)
                .flex()
                .justify_center()
                // P-02: a native page under the pill is cut below it, so
                // the pill stays on top (`web_embed::clip_band`).
                .child(crate::ui::web_embed::clip_band())
                .child(
                    ui::float_surface(cx)
                        .h(STICKY_H)
                        .px(space::S12)
                        .rounded_full()
                        .flex()
                        .items_center()
                        .n_section()
                        .n_figures()
                        .text_color(n.secondary)
                        .child(v2::date_label(m.created_at)),
                )
        });
        // §2.2: content scrolls under the header band with no fade. When
        // there is more below (the jump pill shows), the list's bottom edge
        // fades over 24 so its last row is never a hard mid-glyph cut.
        let bottom_edge = (!following && !self.messages.is_empty()).then(|| {
            div()
                .absolute()
                .left_0()
                .right_0()
                .bottom_0()
                .h(space::S24)
                .bg(gpui::linear_gradient(
                    0.,
                    gpui::linear_color_stop(colors.content, 0.),
                    gpui::linear_color_stop(colors.content.opacity(0.), 1.),
                ))
        });

        div()
            .id("timeline")
            .track_focus(&self.key_focus)
            .key_context(KEY_CONTEXT)
            .on_action(
                cx.listener(|this, _: &crate::views::rail::SelectPrevious, _, cx| {
                    this.move_keyboard_row(-1, cx)
                }),
            )
            .on_action(
                cx.listener(|this, _: &crate::views::rail::SelectNext, _, cx| {
                    this.move_keyboard_row(1, cx)
                }),
            )
            .size_full()
            .flex()
            .flex_col()
            .bg(colors.content)
            .children(
                self.show_header
                    .then(|| self.render_header(&topic, window, cx)),
            )
            .children(search_bar)
            .child(match empty {
                Some(empty) => div().flex_1().min_h(px(0.)).child(empty),
                None => div()
                    .relative()
                    .flex_1()
                    .min_h(px(0.))
                    .child(timeline)
                    .children(bottom_edge)
                    .children(sticky_date)
                    .children(jump_unread),
            })
            .children(jump)
            // The 话题 page shows a member at work as the plan card at the
            // foot of the list (`timeline/plan.rs`), not the pending row.
            .children((!self.v2).then(|| self.render_live(window, cx)).flatten())
            .children(
                self.render_typing(cx)
                    .map(|el| self.v2_column(el.into_any_element())),
            )
            .children(self.probe_element())
            .into_any_element()
    }
}

/// The sticky date pill: h24, 2 under the header line (§2.17).
const STICKY_TOP: Pixels = space::S2;
const STICKY_H: Pixels = space::S24;

impl TimelineView {
    /// The thread's key focus (a region of the window, F6).
    pub fn focus_handle(&self) -> gpui::FocusHandle {
        self.key_focus.clone()
    }

    /// The first row on screen starts `offset` into row `ix`: its day label
    /// is gone under the pill's place (or the row has none, so the label of
    /// its day is above it).
    fn date_label_gone(&self, ix: usize, offset: Pixels) -> bool {
        let Some(message) = self.messages.get(ix) else {
            return false;
        };
        let start = ix == 0 && !self.has_older;
        let prev = ix.checked_sub(1).and_then(|i| self.messages.get(i));
        let own_label = start || model::starts_day(prev, message);
        if !own_label {
            return true;
        }
        let key = model::key(message);
        // Not painted yet: a plain day row has its label 24 + 8 down; the
        // start block's height is not known, so the pill waits a frame.
        let mid = self
            .date_marks
            .borrow()
            .label_mid
            .get(key)
            .copied()
            .or((!start).then_some(space::S32));
        // The label's middle has passed the pill's middle: the pill covers
        // the rest of it, in its place.
        mid.is_some_and(|mid| mid - offset < STICKY_TOP + STICKY_H / 2.)
    }

    /// Marks where row `key` and its own date label are painted.
    pub(super) fn date_mark(&self, key: &str, label: bool) -> impl IntoElement {
        let marks = self.date_marks.clone();
        let key = key.to_string();
        gpui::canvas(
            move |bounds, window, _| {
                let mut marks = marks.borrow_mut();
                if !label {
                    marks.row_top.insert(key, bounds.top());
                    return;
                }
                let Some(top) = marks.row_top.get(&key).copied() else {
                    return;
                };
                // A first or moved label: draw once more so the pill reads
                // the new place.
                let mid = bounds.center().y - top;
                let old = marks.label_mid.insert(key, mid);
                if old.is_none_or(|old| (old - mid).abs() > px(0.5)) {
                    window.refresh();
                }
            },
            |_, _, _, _| {},
        )
        .absolute()
        .top_0()
        .left_0()
        .size_full()
    }

    /// The search bar of a 话题 opened at a search hit (COMPLETENESS §2.17):
    /// 40 pt under the toolbar, the query, `2 / 7`, the previous and next
    /// hit, and close (back to the newest message).
    fn render_search_bar(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        use crate::{
            copy::search as sc,
            ui::native::{NativeType as _, native_colors},
        };
        let search = self.focus.search.as_ref()?;
        let n = native_colors(cx);
        let total = search.hits.len();
        let button = |id: &'static str, sym: Sym, name: &'static str, enabled: bool| {
            ui::native::disabled_push(
                ui::native::icon_button_sym(id, sym, name, ui::native::IconSize::Small, cx),
                !enabled,
                cx,
            )
        };
        let leading = if self.v2 {
            v2::LEADING
        } else {
            theme::CONTENT_GUTTER
        };
        Some(
            div()
                .flex_none()
                .h(crate::ui::native::tokens::height::FOOTER_ROW)
                .pl(leading)
                .pr(v2::SIDE)
                .border_b_1()
                .border_color(n.separator)
                .flex()
                .items_center()
                .gap(space::S8)
                .child(ui::sym(Sym::Search, ui::SymSize::INLINE, n.secondary))
                .child(
                    div()
                        .min_w_0()
                        .max_w(px(240.))
                        .h(theme::size::CHIP)
                        .px(space::S8)
                        .rounded(theme::radius::SM)
                        .bg(n.fill)
                        .flex()
                        .items_center()
                        .n_body()
                        .text_color(n.label)
                        .child(div().min_w_0().one_line().child(search.query.clone())),
                )
                .child(div().flex_1())
                .child(
                    div()
                        .flex_none()
                        .t_meta()
                        .tnum()
                        .text_color(n.secondary)
                        .child(fill(
                            sc::BAR_POSITION,
                            &[
                                ("n", &(search.index + 1).to_string()),
                                ("total", &total.to_string()),
                            ],
                        )),
                )
                .child(
                    button(
                        "search-older",
                        Sym::ChevronUp,
                        sc::BAR_OLDER,
                        search.index + 1 < total,
                    )
                    .on_click(cx.listener(|this, _, _, cx| this.step_search(true, cx))),
                )
                .child(
                    button(
                        "search-newer",
                        Sym::ChevronDown,
                        sc::BAR_NEWER,
                        search.index > 0,
                    )
                    .on_click(cx.listener(|this, _, _, cx| this.step_search(false, cx))),
                )
                .child(
                    button("search-close", Sym::Close, sc::BAR_CLOSE, true)
                        .on_click(cx.listener(|this, _, _, cx| this.close_search(cx))),
                )
                .into_any_element(),
        )
    }
}

/// A 28 pt level-1 pill: the jump to the first unread or to the latest.
fn jump_pill(id: &'static str, label: String, glyph: Sym, cx: &App) -> gpui::Stateful<gpui::Div> {
    let colors = theme::colors(cx);
    ui::float_surface(cx)
        .id(id)
        .h(px(28.))
        .pl(space::S12)
        .pr(space::S10)
        .rounded_full()
        .gap(space::S6)
        .t_ui()
        .medium()
        .tnum()
        .text_color(colors.text_1)
        .hover(|el| el.bg(colors.raised))
        .active(|el| el.bg(colors.pressed))
        .child(label)
        .child(ui::icon(glyph, theme::icon::CHIP, colors.text_2))
}

#[cfg(test)]
mod keyboard_tests {
    use std::sync::Arc;

    use super::*;
    use crate::{
        data::ChatProvider,
        fake::v2::{DEV, T_RECONNECT, V2Samples},
    };

    /// R2-12: with the thread focused, ↑ picks the newest message (its
    /// toolbar shows, as under the pointer) and walks back; ↓ comes forward.
    #[gpui::test]
    fn arrow_keys_move_the_keyboard_row(cx: &mut gpui::TestAppContext) {
        cx.update(|cx| {
            gpui_component::init(cx);
            bind_keys(cx);
        });
        let provider: Arc<dyn ChatProvider> = V2Samples::new();
        let (view, cx) = cx.add_window_view(move |_, cx| {
            let services = Services::new(provider, cx);
            services.nav.update(cx, |nav, cx| {
                nav.open_topic(DEV.into(), T_RECONNECT.into(), cx)
            });
            TimelineView::new(services, cx)
        });
        cx.run_until_parked();
        let keys = |view: &Entity<TimelineView>, cx: &mut gpui::VisualTestContext| {
            view.update(cx, |this, _| {
                let all: Vec<String> = this
                    .messages
                    .iter()
                    .map(|m| model::key(m).to_string())
                    .collect();
                (all, this.hovered.clone())
            })
        };
        let (all, none) = keys(&view, cx);
        assert!(all.len() >= 2, "the sample 话题 has messages");
        assert!(none.is_none());
        view.update_in(cx, |this, window, cx| window.focus(&this.key_focus, cx));
        cx.simulate_keystrokes("up");
        cx.run_until_parked();
        assert_eq!(
            keys(&view, cx).1.as_ref(),
            all.last(),
            "↑ starts at the newest"
        );
        cx.simulate_keystrokes("up");
        cx.run_until_parked();
        assert_eq!(
            keys(&view, cx).1.as_ref(),
            all.get(all.len() - 2),
            "↑ walks back"
        );
        cx.simulate_keystrokes("down");
        cx.run_until_parked();
        assert_eq!(keys(&view, cx).1.as_ref(), all.last(), "↓ comes forward");
    }
}
