//! The quick switcher (NATIVE.md §9.7): the Spotlight-style panel, 600
//! wide, a 48 pt capsule field with 22 pt text, 32 pt result rows with a
//! symbol or avatar (话题, 群, 私聊, 成员, 消息), matches in Semibold. With no
//! query it shows 最近 (five topics), then 群 and 私聊. Picking a member opens
//! (or starts) the DM with them; picking a message opens its topic at that
//! message. ⌘F scopes it to one 群 (a removable `在 qqbot 研发` token): its
//! topics and messages only. ↑/↓ move the choice, Return opens it; the
//! pointer moves the choice only when it really moves.

use std::{rc::Rc, time::Duration};

use gpui::{
    AnyElement, App, AppContext as _, Context, ElementId, Entity, FocusHandle, Focusable,
    InteractiveElement as _, IntoElement, KeyBinding, ParentElement, Pixels, Point, Render,
    ScrollHandle, StatefulInteractiveElement as _, Styled, Subscription, Task, Window, div,
    prelude::FluentBuilder as _, px,
};
use gpui_component::{
    WindowExt as _,
    button::{Button, ButtonCustomVariant, ButtonVariants as _},
    input::{InputEvent, InputState},
};

use crate::{
    app::Services,
    copy::{fill, nav, search as search_copy},
    data::{
        ChatProvider, GroupId, GroupKind, Member, MemberId, SearchHit, SearchQuery, Topic, TopicId,
    },
    theme::Type as _,
    ui::{
        MemberAvatar, Sym,
        native::{
            NativeType as _, native_colors,
            overlays::{
                SpotlightField, return_mark, spot, spotlight, spotlight_divider, spotlight_ink,
                spotlight_row, spotlight_section, spotlight_symbol, weighted,
            },
        },
        sf_symbols::SymSize,
    },
    views::{search::plain::plain_with_ranges, sidebar::topic_line, topic_list::list_time},
};

gpui::actions!(
    switcher,
    [SelectPrevious, SelectNext, OpenSelected, ScopeToGroup]
);

pub(super) const CONTEXT: &str = "TeamSwitcher";

/// Rows per group while a query is typed.
const MAX_PER_GROUP: usize = 5;
/// Message hits shown under 消息.
const MAX_HITS: usize = 3;
/// Recent topics with no query.
const RECENT: usize = 5;
/// Space above the first and below the last child of the list.
const LIST_PAD_TOP: Pixels = px(2.);
const LIST_PAD_BOTTOM: Pixels = px(8.);
/// How much of the next row shows above the bottom padding when more rows
/// follow: with the padding, the 24 pt of the bottom fade.
const PEEK: Pixels = px(16.);

/// The switcher's keys (call once at start-up).
pub fn bind_keys(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("up", SelectPrevious, Some(CONTEXT)),
        KeyBinding::new("down", SelectNext, Some(CONTEXT)),
        KeyBinding::new("ctrl-p", SelectPrevious, Some(CONTEXT)),
        KeyBinding::new("ctrl-n", SelectNext, Some(CONTEXT)),
        KeyBinding::new("enter", OpenSelected, Some(CONTEXT)),
        // Inside the switcher Cmd+F scopes it to the current 群.
        KeyBinding::new("cmd-f", ScopeToGroup, Some(CONTEXT)),
    ]);
}

/// What the switcher shows with no query (§7.12): 最近 (five topics), then 群
/// and 私聊. Indexes of the keyboard selection refer to this list.
pub fn display(sections: Vec<(Section, Vec<Entry>)>) -> Vec<(&'static str, Vec<Entry>)> {
    grouped(&sections, "", None)
}

/// The groups for `query`: with no query 最近, 群, 私聊; with one, matches in
/// 话题, 群, 私聊 and 成员 (five each). Scoped to a 群: only its topics.
pub fn grouped(
    sections: &[(Section, Vec<Entry>)],
    query: &str,
    scope: Option<&str>,
) -> Vec<(&'static str, Vec<Entry>)> {
    let get = |section: Section| -> Vec<Entry> {
        sections
            .iter()
            .filter(|(s, _)| *s == section)
            .flat_map(|(_, e)| e.iter().filter(|e| matches(e, query)).cloned())
            .collect()
    };
    if let Some(scope) = scope {
        let topics: Vec<Entry> = get(Section::Topics)
            .into_iter()
            .filter(|e| matches!(&e.target, Target::Topic(g, _) if g == scope))
            .collect();
        let (label, take) = if query.trim().is_empty() {
            (nav::SECTION_RECENT, RECENT)
        } else {
            (nav::SECTION_TOPICS, MAX_PER_GROUP)
        };
        let topics: Vec<Entry> = topics.into_iter().take(take).collect();
        return if topics.is_empty() {
            Vec::new()
        } else {
            vec![(label, topics)]
        };
    }
    let groups: Vec<(&'static str, Vec<Entry>)> = if query.trim().is_empty() {
        vec![
            (
                nav::SECTION_RECENT,
                get(Section::Topics).into_iter().take(RECENT).collect(),
            ),
            (nav::GROUPS, get(Section::Groups)),
            (nav::DMS, get(Section::Dms)),
        ]
    } else {
        [
            (nav::SECTION_TOPICS, Section::Topics),
            (nav::GROUPS, Section::Groups),
            (nav::DMS, Section::Dms),
            (nav::SECTION_MEMBERS, Section::Members),
        ]
        .into_iter()
        .map(|(label, s)| (label, get(s).into_iter().take(MAX_PER_GROUP).collect()))
        .collect()
    };
    groups.into_iter().filter(|(_, e)| !e.is_empty()).collect()
}

/// What an entry opens.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Target {
    Group(GroupId),
    Topic(GroupId, TopicId),
    /// Opens the DM with this member, creating it when needed.
    Member(MemberId),
}

#[derive(Clone, Debug)]
pub struct Entry {
    pub target: Target,
    /// The searched text: a group name, a DM peer's name, a topic title or a member name.
    pub label: String,
    /// Shown after the label: the topic's group, or the member's 岗位.
    pub detail: Option<String>,
    /// For DMs and members: the avatar.
    pub member: Option<Member>,
    /// For topics: the topic, for its status chip and time.
    pub topic: Option<Topic>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Section {
    Groups,
    Dms,
    Topics,
    Members,
}

/// The switcher's entries in display order; empty sections are left out.
pub fn entries(provider: &dyn ChatProvider, me: Option<&str>) -> Vec<(Section, Vec<Entry>)> {
    let members = provider.members();
    let groups = provider.groups();
    let find = |id: &str| members.iter().find(|m| m.id == id).cloned();
    let mut group_entries = Vec::new();
    let mut dm_entries = Vec::new();
    let mut topic_entries = Vec::new();
    for g in &groups {
        let entry = Entry {
            target: Target::Group(g.id.clone()),
            label: g.name.clone(),
            detail: match g.kind {
                GroupKind::Group => Some(fill(
                    nav::MEMBER_COUNT,
                    &[("n", &g.member_ids.len().to_string())],
                )),
                GroupKind::Dm => g.dm_peer.as_deref().and_then(find).and_then(|m| m.role),
            },
            member: g.dm_peer.as_deref().and_then(find),
            topic: None,
        };
        match g.kind {
            GroupKind::Group => group_entries.push(entry),
            GroupKind::Dm => dm_entries.push(entry),
        }
        for t in provider.topics(&g.id) {
            topic_entries.push(Entry {
                target: Target::Topic(g.id.clone(), t.id.clone()),
                label: t.title.clone(),
                detail: Some(g.name.clone()),
                member: None,
                topic: Some(t),
            });
        }
    }
    // Recent topics first (DESIGN.md §13.12).
    topic_entries.sort_by_key(|e: &Entry| {
        std::cmp::Reverse(e.topic.as_ref().map(|t| t.last_activity_at).unwrap_or(0))
    });
    let has_dm = |id: &str| {
        groups
            .iter()
            .any(|g| g.kind == GroupKind::Dm && g.dm_peer.as_deref() == Some(id))
    };
    let member_entries: Vec<Entry> = members
        .iter()
        .filter(|m| Some(m.id.as_str()) != me && !m.deactivated && !has_dm(&m.id))
        .map(|m| Entry {
            target: Target::Member(m.id.clone()),
            label: m.name.clone(),
            detail: m.role.clone(),
            member: Some(m.clone()),
            topic: None,
        })
        .collect();
    [
        (Section::Groups, group_entries),
        (Section::Dms, dm_entries),
        (Section::Topics, topic_entries),
        (Section::Members, member_entries),
    ]
    .into_iter()
    .filter(|(_, e)| !e.is_empty())
    .collect()
}

/// The palette's own matching rule, for tests and other callers: a
/// case-insensitive substring of the label.
pub fn matches(entry: &Entry, query: &str) -> bool {
    let query = query.trim().to_lowercase();
    query.is_empty() || entry.label.to_lowercase().contains(&query)
}

/// Opens what an entry points at.
pub fn go(services: &Services, target: &Target, cx: &mut App) {
    match target {
        Target::Group(g) => {
            let g = g.clone();
            services.nav.update(cx, |nav, cx| nav.open_group(g, cx));
        }
        Target::Topic(g, t) => {
            let (g, t) = (g.clone(), t.clone());
            services.nav.update(cx, |nav, cx| {
                nav.open_group(g.clone(), cx);
                nav.open_topic(g, t, cx);
            });
        }
        Target::Member(m) => super::open_dm_with(m.clone(), cx),
    }
}

/// `label` split around the first case-insensitive match of `query`.
fn split_match(label: &str, query: &str) -> Option<(String, String, String)> {
    let q: Vec<char> = query
        .trim()
        .chars()
        .map(|c| c.to_ascii_lowercase())
        .collect();
    if q.is_empty() {
        return None;
    }
    let chars: Vec<char> = label.chars().collect();
    let lower: Vec<char> = chars.iter().map(|c| c.to_ascii_lowercase()).collect();
    let at = lower.windows(q.len()).position(|w| w == q.as_slice())?;
    let s = |r: std::ops::Range<usize>| chars[r].iter().collect::<String>();
    Some((s(0..at), s(at..at + q.len()), s(at + q.len()..chars.len())))
}

/// `text` with the characters in `ranges` in Semibold (no highlight fill).
fn marked(text: &str, ranges: &[std::ops::Range<usize>]) -> gpui::Div {
    div()
        .min_w(px(0.))
        .overflow_hidden()
        .whitespace_nowrap()
        .text_ellipsis()
        .child(weighted(text, ranges))
}

/// A label with the query's first match in Semibold.
fn title_with_mark(label: &str, query: &str) -> gpui::Div {
    match split_match(label, query) {
        Some((pre, hit, _)) => {
            let start = pre.chars().count();
            let len = hit.chars().count();
            marked(label, std::slice::from_ref(&(start..start + len)))
        }
        None => marked(label, &[]),
    }
}

/// The leading symbol of a 话题 row (§6): needs you `amber`, live work a
/// `blue` dot, done a `secondary` check, waiting a clock, otherwise `number`.
pub fn topic_symbol(
    provider: &dyn ChatProvider,
    me: Option<&str>,
    topic: &Topic,
    ink: gpui::Hsla,
    cx: &App,
) -> gpui::Div {
    let k = native_colors(cx);
    let (state, _) = crate::views::rail::model::row_state(provider, me, topic);
    use crate::views::rail::model::RowState;
    let (sym, color) = match state {
        RowState::NeedsYou => (Sym::NeedsYou, k.amber),
        RowState::Working(_) => (Sym::Live, k.blue),
        RowState::Done => (Sym::Check, ink),
        RowState::Waiting => (Sym::Waiting, ink),
        RowState::Idle => (Sym::Topic, ink),
    };
    spotlight_symbol(sym, color)
}

fn plain_hit(mut hit: SearchHit) -> SearchHit {
    let (snippet, highlights) = plain_with_ranges(&hit.snippet, &hit.highlights);
    hit.snippet = snippet;
    hit.highlights = highlights;
    hit
}

/// The list height that shows whole rows only: the children's heights
/// (`label`, `row`) summed until `max`, never ending on a section label.
pub fn whole_rows_height(heights: &[(Pixels, bool)], max: Pixels) -> Pixels {
    let mut total = px(0.);
    let mut fit = px(0.);
    for &(h, is_row) in heights {
        if total + h > max {
            break;
        }
        total += h;
        if is_row {
            fit = total;
        }
    }
    fit
}

/// One row of the switcher.
enum Row {
    Entry(Box<Entry>),
    Hit(SearchHit),
}

pub struct Switcher {
    services: Services,
    query: Entity<InputState>,
    focus: FocusHandle,
    all: Vec<(Section, Vec<Entry>)>,
    groups: Vec<(&'static str, Vec<Entry>)>,
    hits: Vec<SearchHit>,
    /// The flat keyboard-selectable rows.
    rows: Vec<Row>,
    active: usize,
    members: Rc<Vec<Member>>,
    scroll: ScrollHandle,
    search: Option<Task<()>>,
    /// Cmd+F: the 群 searched, with its name for the token.
    scope: Option<(GroupId, String)>,
    /// The last pointer position seen over the list. The pointer picks a
    /// row only when it moves, so it never fights the keyboard selection.
    pointer: Option<Point<Pixels>>,
    /// Scroll to the active row on the next frame (set by the keyboard).
    reveal: bool,
    _subscriptions: Vec<Subscription>,
}

impl Focusable for Switcher {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Switcher {
    pub fn new(services: Services, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let query =
            cx.new(|cx| InputState::new(window, cx).placeholder(nav::QUICK_SWITCH_PLACEHOLDER));
        let me = services.me();
        let all = entries(services.provider.as_ref(), me.as_deref());
        let subscriptions =
            vec![
                cx.subscribe_in(&query, window, |this, _, event: &InputEvent, window, cx| {
                    match event {
                        InputEvent::Change => this.requery(cx),
                        InputEvent::PressEnter { .. } => this.confirm(window, cx),
                        _ => {}
                    }
                }),
            ];
        let members = Rc::new(services.provider.members());
        let mut this = Self {
            services,
            query,
            focus: cx.focus_handle(),
            all,
            groups: Vec::new(),
            hits: Vec::new(),
            rows: Vec::new(),
            active: 0,
            members,
            scroll: ScrollHandle::new(),
            search: None,
            scope: None,
            pointer: None,
            reveal: true,
            _subscriptions: subscriptions,
        };
        this.rebuild();
        this
    }

    fn text(&self, cx: &App) -> String {
        self.query.read(cx).value().trim().to_string()
    }

    fn rebuild(&mut self) {
        self.rows = self
            .groups
            .iter()
            .flat_map(|(_, e)| e.iter().cloned().map(|e| Row::Entry(Box::new(e))))
            .chain(self.hits.iter().cloned().map(Row::Hit))
            .collect();
        if self.active >= self.rows.len() {
            self.active = self.rows.len().saturating_sub(1);
        }
    }

    /// Scopes the switcher to `group` (Cmd+F) or back to everything.
    fn set_scope(&mut self, group: Option<GroupId>, window: &mut Window, cx: &mut Context<Self>) {
        let scope = group.and_then(|g| {
            self.all
                .iter()
                .flat_map(|(_, e)| e)
                .find(|e| e.target == Target::Group(g.clone()))
                .map(|e| (g, e.label.clone()))
        });
        if scope == self.scope {
            return;
        }
        self.scope = scope;
        let placeholder = if self.scope.is_some() {
            search_copy::PLACEHOLDER_IN_GROUP
        } else {
            nav::QUICK_SWITCH_PLACEHOLDER
        };
        self.query
            .update(cx, |q, cx| q.set_placeholder(placeholder, window, cx));
        self.requery(cx);
    }

    fn requery(&mut self, cx: &mut Context<Self>) {
        let text = self.text(cx);
        let scope = self.scope.as_ref().map(|(g, _)| g.clone());
        self.groups = grouped(&self.all, &text, scope.as_deref());
        self.hits.clear();
        self.active = 0;
        self.reveal = true;
        self.rebuild();
        self.scroll.set_offset(gpui::point(px(0.), px(0.)));
        cx.notify();
        // Messages need the server: after a short pause, two or more characters.
        self.search = (text.chars().count() >= 2).then(|| {
            let provider = self.services.provider.clone();
            cx.spawn(async move |this, cx| {
                cx.background_executor()
                    .timer(Duration::from_millis(150))
                    .await;
                let reply = provider.search(SearchQuery {
                    text: text.clone(),
                    group_id: scope.clone(),
                    limit: MAX_HITS as u32,
                });
                if let Ok(Ok(hits)) = reply.await {
                    let _ = this.update(cx, |this, cx| {
                        let same_scope = this.scope.as_ref().map(|(g, _)| g) == scope.as_ref();
                        if this.text(cx) == text && same_scope {
                            this.hits = hits.into_iter().take(MAX_HITS).map(plain_hit).collect();
                            this.rebuild();
                            cx.notify();
                        }
                    });
                }
            })
        });
    }

    fn select(&mut self, delta: isize, cx: &mut Context<Self>) {
        if self.rows.is_empty() {
            return;
        }
        let n = self.rows.len() as isize;
        self.active = ((self.active as isize + delta).rem_euclid(n)) as usize;
        self.reveal = true;
        cx.notify();
    }

    /// The pointer moved over row `ix`: it becomes the selection, but only
    /// after a real move (not when the list scrolls under a still pointer
    /// or the switcher opens under it).
    fn pointer_moved(&mut self, ix: usize, at: Point<Pixels>, cx: &mut Context<Self>) {
        let moved = self.pointer.is_some_and(|p| p != at);
        self.pointer = Some(at);
        if moved && self.active != ix {
            self.active = ix;
            cx.notify();
        }
    }

    fn confirm(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(row) = self.rows.get(self.active) else {
            return;
        };
        let services = self.services.clone();
        match row {
            Row::Entry(e) => {
                let target = e.target.clone();
                window.close_dialog(cx);
                go(&services, &target, cx);
            }
            Row::Hit(h) => {
                let h = h.clone();
                window.close_dialog(cx);
                services.nav.update(cx, |nav, cx| {
                    nav.open_message(h.group_id, h.topic_id, h.message_id, h.seq, cx)
                });
            }
        }
    }

    fn open_row(&mut self, ix: usize, window: &mut Window, cx: &mut Context<Self>) {
        self.active = ix;
        self.confirm(window, cx);
    }

    fn group_label(label: &'static str, cx: &App) -> gpui::Div {
        spotlight_section(label, cx)
    }

    fn entry_row(&self, ix: usize, e: &Entry, query: &str, cx: &mut Context<Self>) -> AnyElement {
        let k = native_colors(cx);
        let active = ix == self.active;
        let (_, ink2) = spotlight_ink(active, cx);
        let me = self.services.me();
        let surface = if active { k.accent } else { k.sheet };
        let lead = match (&e.target, &e.member, &e.topic) {
            (Target::Topic(..), _, Some(t)) => {
                topic_symbol(self.services.provider.as_ref(), me.as_deref(), t, ink2, cx)
            }
            (_, Some(m), _) => div().flex_none().size(spot::SLOT).child(
                MemberAvatar::new(Some(m), spot::AVATAR)
                    .presence(Some(m))
                    .on(surface),
            ),
            _ => spotlight_symbol(Sym::Members, ink2),
        };
        let (label, untitled) = match &e.topic {
            Some(_) => {
                let line = topic_line(&e.label);
                if line.is_empty() {
                    (nav::UNTITLED.to_string(), true)
                } else {
                    (line, false)
                }
            }
            None => (e.label.clone(), false),
        };
        let detail = match (&self.scope, &e.target) {
            (Some(_), Target::Topic(..)) => None,
            _ => e.detail.clone(),
        };
        let time = e
            .topic
            .as_ref()
            .map(|t| list_time(t.last_activity_at, crate::selftest::clock::local_now()))
            .filter(|t| !t.is_empty());
        spotlight_row(
            ElementId::NamedInteger("switcher-row".into(), ix as u64),
            active,
            cx,
        )
        .child(lead)
        .child(
            title_with_mark(&label, query)
                .flex_shrink(1.)
                .when(untitled, |el| el.text_color(ink2)),
        )
        .children(detail.map(|d| {
            div()
                .flex_shrink(2.)
                .min_w(px(40.))
                .text_color(ink2)
                .one_line()
                .child(d)
        }))
        .child(div().flex_1())
        .children(time.map(|t| div().flex_none().n_figures().text_color(ink2).child(t)))
        .when(active, |el| el.child(return_mark(false, ink2)))
        .on_mouse_move(cx.listener(move |this, e: &gpui::MouseMoveEvent, _, cx| {
            this.pointer_moved(ix, e.position, cx)
        }))
        .on_click(cx.listener(move |this, _, window, cx| this.open_row(ix, window, cx)))
        .into_any_element()
    }

    /// A message hit: one 32 pt row, the author's avatar and name, the
    /// snippet with its matches in Semibold, then the time.
    fn hit_row(&self, ix: usize, h: &SearchHit, cx: &mut Context<Self>) -> AnyElement {
        let k = native_colors(cx);
        let active = ix == self.active;
        let (ink, ink2) = spotlight_ink(active, cx);
        let author = self.members.iter().find(|m| m.id == h.author_id);
        let time = list_time(h.created_at, crate::selftest::clock::local_now());
        spotlight_row(
            ElementId::NamedInteger("switcher-row".into(), ix as u64),
            active,
            cx,
        )
        .child(
            div().flex_none().size(spot::SLOT).child(
                MemberAvatar::new(author, spot::AVATAR).on(if active { k.accent } else { k.sheet }),
            ),
        )
        .child(
            div()
                .flex_none()
                .max_w(px(140.))
                .n_headline()
                .text_color(ink)
                .one_line()
                .child(author.map(|a| a.name.clone()).unwrap_or_default()),
        )
        .child(
            marked(&h.snippet, &h.highlights)
                .flex_1()
                .text_color(ink2),
        )
        .when(!time.is_empty(), |el| {
            el.child(div().flex_none().n_figures().text_color(ink2).child(time))
        })
        .when(active, |el| el.child(return_mark(false, ink2)))
        .on_mouse_move(cx.listener(move |this, e: &gpui::MouseMoveEvent, _, cx| {
            this.pointer_moved(ix, e.position, cx)
        }))
        .on_click(cx.listener(move |this, _, window, cx| this.open_row(ix, window, cx)))
        .into_any_element()
    }
}

impl Render for Switcher {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let k = native_colors(cx);
        let query = self.text(cx);
        let mut children: Vec<AnyElement> = Vec::new();
        let mut heights: Vec<(Pixels, bool)> = Vec::new();
        let mut active_child = 0;
        let mut ix = 0;
        let groups = self.groups.clone();
        for (label, items) in &groups {
            children.push(Self::group_label(label, cx).into_any_element());
            heights.push((spot::SECTION, false));
            for e in items {
                if ix == self.active {
                    active_child = children.len();
                }
                children.push(self.entry_row(ix, e, &query, cx));
                heights.push((spot::ROW, true));
                ix += 1;
            }
        }
        if !self.hits.is_empty() {
            children.push(Self::group_label(nav::SECTION_MESSAGES, cx).into_any_element());
            heights.push((spot::SECTION, false));
            let hits = self.hits.clone();
            for h in &hits {
                if ix == self.active {
                    active_child = children.len();
                }
                children.push(self.hit_row(ix, h, cx));
                heights.push((spot::ROW, true));
                ix += 1;
            }
        }
        if !self.rows.is_empty() && std::mem::take(&mut self.reveal) {
            let target = if active_child > 0 && !heights[active_child - 1].1 {
                active_child - 1
            } else {
                active_child
            };
            self.scroll.scroll_to_item(target);
        }
        let empty = self.rows.is_empty();
        let max_rows = (window.viewport_size().height * 0.6
            - spot::FIELD
            - LIST_PAD_TOP
            - LIST_PAD_BOTTOM)
            .max(spot::ROW * 3.);
        let total: Pixels = heights.iter().map(|(h, _)| *h).sum();
        let list_h = if total > max_rows {
            whole_rows_height(&heights, max_rows - PEEK) + PEEK
        } else {
            whole_rows_height(&heights, max_rows)
        };
        // The 群 a search is scoped to (⌘F): a token before the text; click
        // or ⌫ in an empty field removes it.
        let scope_token = self.scope.as_ref().map(|(_, name)| {
            Button::new("switcher-scope")
                .custom(
                    ButtonCustomVariant::new(cx)
                        .color(k.fill)
                        .foreground(k.label)
                        .hover(k.fill)
                        .active(k.fill_hover),
                )
                .h(px(26.))
                .pl(px(8.))
                .pr(px(6.))
                .gap(px(4.))
                .rounded(px(6.))
                .n_body()
                .child(fill(search_copy::SCOPE_IN_GROUP, &[("name", name)]))
                .child(Sym::Close.icon(SymSize::INLINE_SMALL).text_color(k.secondary))
                .tooltip(search_copy::SCOPE_REMOVE)
                .on_click(cx.listener(|this, _, window, cx| {
                    this.set_scope(None, window, cx);
                    this.query.update(cx, |q, cx| q.focus(window, cx));
                }))
        });
        let has_query = !query.is_empty();
        let field = match scope_token {
            Some(t) => SpotlightField::new(&self.query).token(t),
            None => SpotlightField::new(&self.query),
        };
        div()
            .key_context(CONTEXT)
            .track_focus(&self.focus)
            .on_action(cx.listener(|this, _: &SelectPrevious, _, cx| this.select(-1, cx)))
            .on_action(cx.listener(|this, _: &SelectNext, _, cx| this.select(1, cx)))
            .on_action(cx.listener(|this, _: &OpenSelected, window, cx| this.confirm(window, cx)))
            .on_action(cx.listener(|this, _: &ScopeToGroup, window, cx| {
                let group = if this.scope.is_some() {
                    None
                } else {
                    crate::views::search::current_group(&this.services, cx)
                };
                this.set_scope(group, window, cx);
            }))
            .capture_key_down(cx.listener(|this, e: &gpui::KeyDownEvent, window, cx| {
                if e.keystroke.key == "backspace"
                    && !e.keystroke.modifiers.modified()
                    && this.scope.is_some()
                    && this.query.read(cx).value().is_empty()
                {
                    this.set_scope(None, window, cx);
                    cx.stop_propagation();
                }
            }))
            .w_full()
            .flex()
            .flex_col()
            .text_color(k.label)
            .child(field)
            .when(!empty || has_query, |el| el.child(spotlight_divider(cx)))
            .when(!empty, |el| {
                el.child(
                    div()
                        .id("switcher-list")
                        .flex_none()
                        .h(list_h + LIST_PAD_TOP + LIST_PAD_BOTTOM)
                        .overflow_y_scroll()
                        .track_scroll(&self.scroll)
                        .pt(LIST_PAD_TOP)
                        .pb(LIST_PAD_BOTTOM)
                        .flex()
                        .flex_col()
                        .on_mouse_move(cx.listener(|this, e: &gpui::MouseMoveEvent, _, _| {
                            // Over a label or the padding: remember where the
                            // pointer is, so the next move counts as a move.
                            this.pointer.get_or_insert(e.position);
                        }))
                        .children(children),
                )
            })
            .when(empty && has_query, |el| {
                el.child(
                    div()
                        .h(px(56.))
                        .flex()
                        .items_center()
                        .justify_center()
                        .n_body()
                        .text_color(k.secondary)
                        .child(nav::QUICK_SWITCH_EMPTY),
                )
            })
    }
}

/// Opens the switcher in `window`.
pub fn open(services: Services, window: &mut Window, cx: &mut App) {
    open_scoped(services, None, "", window, cx);
}

/// Opens the switcher with `query` typed (the self-test uses it).
pub fn open_with(services: Services, query: &str, window: &mut Window, cx: &mut App) {
    open_scoped(services, None, query, window, cx);
}

/// Opens the switcher scoped to `scope` (Cmd+F) with `query` typed. The
/// query field has the focus, so the caret shows at once.
pub fn open_scoped(
    services: Services,
    scope: Option<GroupId>,
    query: &str,
    window: &mut Window,
    cx: &mut App,
) -> Entity<Switcher> {
    let query = query.to_string();
    let view = cx.new(|cx| {
        let mut s = Switcher::new(services, window, cx);
        if !query.is_empty() {
            s.query
                .update(cx, |q, cx| q.set_value(query.clone(), window, cx));
        }
        s.set_scope(scope, window, cx);
        s.requery(cx);
        s
    });
    let focus = view.read(cx).query.read(cx).focus_handle(cx);
    let first = std::cell::Cell::new(true);
    let dialog_view = view.clone();
    window.open_dialog(cx, move |dialog, window, cx| {
        let view = &dialog_view;
        if first.replace(false) {
            let input = view.read(cx).query.clone();
            input.update(cx, |q, cx| q.focus(window, cx));
        }
        spotlight(dialog, cx).child(view.clone())
    });
    // The dialog moves the focus to itself when it opens; put it back in
    // the query field once that has happened.
    let view2 = view.clone();
    window.defer(cx, move |window, cx| {
        if !focus.is_focused(window) {
            view2
                .read(cx)
                .query
                .clone()
                .update(cx, |q, cx| q.focus(window, cx));
        }
    });
    view
}

impl Switcher {
    /// Whether the query field has the keyboard focus (the self-test).
    pub fn query_focused(&self, window: &Window, cx: &App) -> bool {
        self.query.read(cx).focus_handle(cx).is_focused(window)
    }

    /// The 群 this switcher is scoped to (the self-test).
    pub fn scope(&self) -> Option<&GroupId> {
        self.scope.as_ref().map(|(g, _)| g)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fake::{FakeProvider, ME};

    fn search(p: &FakeProvider, q: &str) -> Vec<Target> {
        entries(p, Some(ME))
            .into_iter()
            .flat_map(|(_, e)| e)
            .filter(|e| matches(e, q))
            .map(|e| e.target)
            .collect()
    }

    #[test]
    fn covers_groups_dms_topics_and_members() {
        let p = FakeProvider::new();
        let sections: Vec<Section> = entries(p.as_ref(), Some(ME))
            .iter()
            .map(|(s, _)| *s)
            .collect();
        assert_eq!(sections[0], Section::Groups);
        assert!(sections.contains(&Section::Dms));
        assert!(sections.contains(&Section::Topics));
        let all = search(&p, "");
        // I am never in the member list.
        assert!(!all.contains(&Target::Member(ME.into())));
        // Every group and every topic is there.
        for g in p.groups() {
            assert!(all.contains(&Target::Group(g.id.clone())));
            for t in p.topics(&g.id) {
                assert!(all.contains(&Target::Topic(g.id.clone(), t.id)));
            }
        }
    }

    #[test]
    fn substring_ignores_case_and_finds_each_kind() {
        let p = FakeProvider::new();
        let group = p
            .groups()
            .into_iter()
            .find(|g| g.kind == GroupKind::Group)
            .unwrap();
        let middle: String = group.name.chars().skip(1).take(2).collect();
        let hits = search(&p, &middle.to_uppercase());
        assert!(hits.contains(&Target::Group(group.id.clone())), "{middle}");
        let topic = p.topics(&group.id).into_iter().next().unwrap();
        let part: String = topic.title.chars().skip(1).take(3).collect();
        assert!(search(&p, &part).contains(&Target::Topic(group.id.clone(), topic.id)));
        let dm = p
            .groups()
            .into_iter()
            .find(|g| g.kind == GroupKind::Dm)
            .unwrap();
        assert!(search(&p, &dm.name).contains(&Target::Group(dm.id)));
        assert!(search(&p, "zzzz-no-such-thing").is_empty());
    }

    #[test]
    fn members_without_a_dm_are_offered() {
        let p = FakeProvider::new();
        let members: Vec<Target> = entries(p.as_ref(), Some(ME))
            .into_iter()
            .filter(|(s, _)| *s == Section::Members)
            .flat_map(|(_, e)| e)
            .map(|e| e.target)
            .collect();
        let groups = p.groups();
        for t in &members {
            let Target::Member(id) = t else { panic!() };
            assert!(
                !groups
                    .iter()
                    .any(|g| g.dm_peer.as_deref() == Some(id.as_str()))
            );
            assert!(!p.member(id).unwrap().deactivated);
        }
    }
}
