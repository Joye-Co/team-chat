//! ⌘K 交代或搜索 (Ask.dc.html): one 640 pt panel over the page. Typing a
//! sentence offers 交代 first: 交给 <the member it names> opens a 话题
//! addressed to that member in the 群 of 发到 (Enter), and 交给 Jojo 安排
//! hands it to the main helper (⌘Enter). Below, the 话题, 产出, 成员 and 群
//! that share words with the query, the shared words in weight 600. With no
//! query it lists 最近, then 群 and 私聊, like the quick switcher it
//! replaces.

pub mod model;

use std::{
    ops::Range,
    sync::atomic::{AtomicBool, Ordering},
};

use gpui::{
    AnyElement, App, AppContext as _, Context, ElementId, Entity, FocusHandle, Focusable,
    FontWeight, HighlightStyle, InteractiveElement as _, IntoElement, KeyBinding, ParentElement,
    Pixels, Point, Render, ScrollHandle, SharedString, StatefulInteractiveElement as _, Styled,
    StyledText, Subscription, Window, div, prelude::FluentBuilder as _, px,
};
use gpui_component::{
    WindowExt as _,
    button::{Button, ButtonVariants as _},
    input::{Input, InputEvent, InputState},
    menu::{DropdownMenu as _, PopupMenuItem},
};

use crate::{
    app::{Place, Services},
    copy::{artifacts as art_copy, fill, nav as nav_copy, rail as c},
    data::{
        ArtifactKind, ArtifactSummary, GroupId, GroupKind, Member, StartTopicRequest, Topic,
        TopicStatus,
    },
    theme::{self, Type as _, space},
    ui::{self, Lucide, MemberAvatar, Ring},
    views::{
        nav::switcher::{self, Entry, Target},
        rail::model::{self as rail_model, RowState},
    },
};

gpui::actions!(ask, [SelectPrevious, SelectNext, GiveToHelper]);

const CONTEXT: &str = "AskPalette";
/// The query row.
const QUERY_ROW: Pixels = px(52.);
/// A 交代 row: avatar 32, two lines.
const GIVE_ROW: Pixels = px(56.);
/// 话题, 成员 and 群 rows.
const ROW: Pixels = px(40.);
/// 产出 rows (a page thumbnail).
const ARTIFACT_ROW: Pixels = px(44.);
const LABEL_ROW: Pixels = px(28.);
const DIVIDER: Pixels = px(13.);
const FOOTER: Pixels = px(40.);
/// The leading slot of every row.
const SLOT: Pixels = px(32.);
/// Rows per section while a query is typed.
const TOPICS_MAX: usize = 5;
const OTHERS_MAX: usize = 3;
const RECENT: usize = 5;

/// One choosable row.
#[derive(Clone, Debug)]
enum Item {
    /// 交给 <member>: a new 话题 addressed to it. `helper` is the 交给 Jojo
    /// 安排 row.
    Give {
        member: Member,
        task: String,
        helper: bool,
    },
    Topic {
        topic: Topic,
        ranges: Vec<Range<usize>>,
    },
    Artifact {
        artifact: ArtifactSummary,
        ranges: Vec<Range<usize>>,
    },
    Member {
        member: Member,
        ranges: Vec<Range<usize>>,
    },
    /// A 群 or a 私聊.
    Group {
        entry: Entry,
        ranges: Vec<Range<usize>>,
    },
}

impl Item {
    fn height(&self) -> Pixels {
        match self {
            Item::Give { .. } => GIVE_ROW,
            Item::Artifact { .. } => ARTIFACT_ROW,
            _ => ROW,
        }
    }

    fn key(&self) -> String {
        match self {
            Item::Give { member, helper, .. } => format!("give-{}-{helper}", member.id),
            Item::Topic { topic, .. } => format!("topic-{}", topic.id),
            Item::Artifact { artifact, .. } => format!("artifact-{}", artifact.id),
            Item::Member { member, .. } => format!("member-{}", member.id),
            Item::Group { entry, .. } => format!("group-{}", entry.label),
        }
    }
}

#[derive(Clone)]
struct Section {
    label: &'static str,
    items: Vec<Item>,
    /// A line under the section (after 交代).
    divider: bool,
}

pub struct AskPalette {
    services: Services,
    query: Entity<InputState>,
    focus: FocusHandle,
    sections: Vec<Section>,
    active: usize,
    /// 发到: the 群 chosen by hand for a new 话题.
    target: Option<GroupId>,
    scroll: ScrollHandle,
    pointer: Option<Point<Pixels>>,
    reveal: bool,
    _subscriptions: Vec<Subscription>,
}

impl Focusable for AskPalette {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

/// The palette's keys (bound on first open).
fn bind_keys(cx: &mut App) {
    static BOUND: AtomicBool = AtomicBool::new(false);
    if BOUND.swap(true, Ordering::SeqCst) {
        return;
    }
    cx.bind_keys([
        KeyBinding::new("up", SelectPrevious, Some(CONTEXT)),
        KeyBinding::new("down", SelectNext, Some(CONTEXT)),
        KeyBinding::new("ctrl-p", SelectPrevious, Some(CONTEXT)),
        KeyBinding::new("ctrl-n", SelectNext, Some(CONTEXT)),
        KeyBinding::new("cmd-enter", GiveToHelper, Some(CONTEXT)),
    ]);
}

/// The main helper: Jojo, else the first bot (`side_chat_default_member`).
fn helper(services: &Services) -> Option<Member> {
    services
        .provider
        .side_chat_default_member()
        .and_then(|id| services.provider.member(&id))
}

impl AskPalette {
    fn new(services: Services, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let query = cx.new(|cx| InputState::new(window, cx).placeholder(c::ASK_PLACEHOLDER));
        let subscriptions = vec![cx.subscribe_in(
            &query,
            window,
            |this, _, event: &InputEvent, window, cx| match event {
                InputEvent::Change => this.requery(cx),
                InputEvent::PressEnter { secondary, .. } => {
                    if *secondary {
                        this.give_to_helper(window, cx)
                    } else {
                        this.confirm(window, cx)
                    }
                }
                _ => {}
            },
        )];
        let mut this = Self {
            services,
            query,
            focus: cx.focus_handle(),
            sections: Vec::new(),
            active: 0,
            target: None,
            scroll: ScrollHandle::new(),
            pointer: None,
            reveal: true,
            _subscriptions: subscriptions,
        };
        this.requery(cx);
        this
    }

    fn text(&self, cx: &App) -> String {
        self.query.read(cx).value().trim().to_string()
    }

    fn items(&self) -> impl Iterator<Item = &Item> {
        self.sections.iter().flat_map(|s| s.items.iter())
    }

    fn active_item(&self) -> Option<&Item> {
        self.items().nth(self.active)
    }

    /// The first 交代 row (the member the query names, else the helper).
    fn first_give(&self) -> Option<&Member> {
        self.items().find_map(|i| match i {
            Item::Give { member, .. } => Some(member),
            _ => None,
        })
    }

    fn requery(&mut self, cx: &mut Context<Self>) {
        let text = self.text(cx);
        self.sections = self.build(&text);
        self.active = 0;
        self.reveal = true;
        self.scroll.set_offset(gpui::point(px(0.), px(0.)));
        cx.notify();
    }

    fn build(&self, query: &str) -> Vec<Section> {
        let p = self.services.provider.as_ref();
        let me = self.services.me();
        let all = switcher::entries(p, me.as_deref());
        if query.is_empty() {
            // 最近, 群, 私聊: the quick switcher's order.
            return switcher::display(all)
                .into_iter()
                .map(|(label, entries)| Section {
                    label: if label == nav_copy::SECTION_RECENT {
                        c::ASK_SECTION_RECENT
                    } else if label == nav_copy::GROUPS {
                        c::ASK_SECTION_GROUPS
                    } else {
                        c::ASK_SECTION_DMS
                    },
                    items: entries
                        .into_iter()
                        .take(if label == nav_copy::SECTION_RECENT {
                            RECENT
                        } else {
                            usize::MAX
                        })
                        .map(|e| match (&e.target, &e.topic) {
                            (Target::Topic(..), Some(t)) => Item::Topic {
                                topic: t.clone(),
                                ranges: Vec::new(),
                            },
                            _ => Item::Group {
                                entry: e,
                                ranges: Vec::new(),
                            },
                        })
                        .collect(),
                    divider: false,
                })
                .collect();
        }
        let members = p.members();
        let named = model::named(query, &members, me.as_deref());
        let helper = helper(&self.services);
        let mut sections = Vec::new();

        // 交代: the named member, then the helper.
        let mut give = Vec::new();
        if let Some(m) = named.first() {
            give.push(Item::Give {
                member: (*m).clone(),
                task: model::task(query, Some(&m.name)),
                helper: helper.as_ref().is_some_and(|h| h.id == m.id),
            });
        }
        if let Some(h) = &helper
            && !give.iter().any(|g| matches!(g, Item::Give { member, .. } if member.id == h.id))
        {
            give.push(Item::Give {
                member: h.clone(),
                task: model::task(query, named.first().map(|m| m.name.as_str())),
                helper: true,
            });
        }
        if !give.is_empty() {
            sections.push(Section {
                label: c::ASK_SECTION_GIVE,
                items: give,
                divider: true,
            });
        }

        // 话题.
        let mut topics: Vec<(model::Weighed, Topic)> = all
            .iter()
            .flat_map(|(_, e)| e)
            .filter_map(|e| e.topic.clone())
            .filter_map(|t| model::weigh(&t.title, query).map(|w| (w, t)))
            .collect();
        topics.sort_by_key(|(w, t)| (std::cmp::Reverse(w.score), std::cmp::Reverse(t.last_activity_at)));
        let topics: Vec<Item> = topics
            .into_iter()
            .take(TOPICS_MAX)
            .map(|(w, topic)| Item::Topic {
                topic,
                ranges: w.ranges,
            })
            .collect();
        if !topics.is_empty() {
            sections.push(Section {
                label: c::ASK_SECTION_TOPICS,
                items: topics,
                divider: false,
            });
        }

        // 产出.
        let mut artifacts: Vec<(model::Weighed, ArtifactSummary)> = p
            .all_artifacts()
            .into_iter()
            .filter_map(|(_, a)| model::weigh(&a.title, query).map(|w| (w, a)))
            .collect();
        artifacts.sort_by_key(|(w, a)| (std::cmp::Reverse(w.score), std::cmp::Reverse(a.updated_at)));
        let artifacts: Vec<Item> = artifacts
            .into_iter()
            .take(OTHERS_MAX)
            .map(|(w, artifact)| Item::Artifact {
                artifact,
                ranges: w.ranges,
            })
            .collect();
        if !artifacts.is_empty() {
            sections.push(Section {
                label: c::ASK_SECTION_ARTIFACTS,
                items: artifacts,
                divider: false,
            });
        }

        // 成员: named ones first, then name matches.
        let mut people: Vec<Item> = Vec::new();
        for m in members
            .iter()
            .filter(|m| !m.deactivated && Some(m.id.as_str()) != me.as_deref())
        {
            let ranges = if named.iter().any(|n| n.id == m.id) {
                Some(vec![0..m.name.len()])
            } else {
                model::weigh(&m.name, query).map(|w| w.ranges)
            };
            if let Some(ranges) = ranges {
                people.push(Item::Member {
                    member: m.clone(),
                    ranges,
                });
            }
        }
        // The members the query names come first.
        people.sort_by_key(|i| match i {
            Item::Member { member, .. } => !named.iter().any(|n| n.id == member.id),
            _ => true,
        });
        people.truncate(OTHERS_MAX);
        if !people.is_empty() {
            sections.push(Section {
                label: c::ASK_SECTION_MEMBERS,
                items: people,
                divider: false,
            });
        }

        // 群 and 私聊.
        let groups: Vec<Item> = all
            .iter()
            .filter(|(s, _)| matches!(s, switcher::Section::Groups | switcher::Section::Dms))
            .flat_map(|(_, e)| e)
            .filter_map(|e| {
                model::weigh(&e.label, query).map(|w| Item::Group {
                    entry: e.clone(),
                    ranges: w.ranges,
                })
            })
            .take(OTHERS_MAX)
            .collect();
        if !groups.is_empty() {
            sections.push(Section {
                label: c::ASK_SECTION_GROUPS,
                items: groups,
                divider: false,
            });
        }
        sections
    }

    /// 发到: the 群 a new 话题 for `member` goes to. Chosen by hand, else
    /// the open 群 when the member is in it, else the first 群 with it.
    fn target_group(&self, member: Option<&Member>, cx: &App) -> Option<(GroupId, String)> {
        let groups: Vec<_> = self
            .services
            .provider
            .groups()
            .into_iter()
            .filter(|g| g.kind == GroupKind::Group)
            .collect();
        if let Some(t) = &self.target
            && let Some(g) = groups.iter().find(|g| &g.id == t)
        {
            return Some((g.id.clone(), g.name.clone()));
        }
        let has = |g: &crate::data::GroupSummary| {
            member.is_none_or(|m| g.member_ids.iter().any(|id| id == &m.id))
        };
        let nav = self.services.nav.read(cx);
        let current = match (&nav.place, &nav.topic) {
            (_, Some((g, _))) => Some(g.clone()),
            (Some(Place::Group(g)), None) => Some(g.clone()),
            _ => None,
        };
        current
            .and_then(|c| groups.iter().find(|g| g.id == c && has(g)))
            .or_else(|| groups.iter().find(|g| has(g)))
            .or_else(|| groups.first())
            .map(|g| (g.id.clone(), g.name.clone()))
    }

    fn select(&mut self, delta: isize, cx: &mut Context<Self>) {
        let n = self.items().count() as isize;
        if n == 0 {
            return;
        }
        self.active = ((self.active as isize + delta).rem_euclid(n)) as usize;
        self.reveal = true;
        cx.notify();
    }

    fn pointer_moved(&mut self, ix: usize, at: Point<Pixels>, cx: &mut Context<Self>) {
        let moved = self.pointer.is_some_and(|p| p != at);
        self.pointer = Some(at);
        if moved && self.active != ix {
            self.active = ix;
            cx.notify();
        }
    }

    /// Opens a new 话题 addressed to `member` with `task`, then shows it.
    fn give(&mut self, member: Member, task: String, window: &mut Window, cx: &mut Context<Self>) {
        let Some((group_id, _)) = self.target_group(Some(&member), cx) else {
            return;
        };
        let provider = self.services.provider.clone();
        provider.start_topic(StartTopicRequest {
            group_id: group_id.clone(),
            title: Some(task.clone()),
            body: format!("@{} {task}", member.name),
            mentions: vec![member.id.clone()],
        });
        window.close_dialog(cx);
        // The new 话题 is the newest one in the 群.
        if let Some(topic) = provider.topics(&group_id).into_iter().next() {
            self.services
                .nav
                .update(cx, |nav, cx| nav.open_topic(group_id, topic.id, cx));
        }
    }

    /// ⌘Enter: 交给 Jojo 安排.
    fn give_to_helper(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let item = self.items().find_map(|i| match i {
            Item::Give {
                member,
                task,
                helper: true,
            } => Some((member.clone(), task.clone())),
            _ => None,
        });
        if let Some((member, task)) = item {
            self.give(member, task, window, cx);
        }
    }

    fn confirm(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(item) = self.active_item().cloned() else {
            return;
        };
        let services = self.services.clone();
        match item {
            Item::Give { member, task, .. } => self.give(member, task, window, cx),
            Item::Topic { topic, .. } => {
                window.close_dialog(cx);
                switcher::go(&services, &Target::Topic(topic.group_id, topic.id), cx);
            }
            Item::Group { entry, .. } => {
                window.close_dialog(cx);
                switcher::go(&services, &entry.target, cx);
            }
            Item::Member { member, .. } => {
                window.close_dialog(cx);
                services
                    .nav
                    .update(cx, |nav, cx| nav.open_members(Some(member.id), cx));
            }
            Item::Artifact { artifact, .. } => {
                window.close_dialog(cx);
                services
                    .nav
                    .update(cx, |nav, cx| nav.open_artifact(artifact.id, None, cx));
            }
        }
    }

    fn open_row(&mut self, ix: usize, window: &mut Window, cx: &mut Context<Self>) {
        self.active = ix;
        self.confirm(window, cx);
    }
}

/// `text` with `ranges` in weight 600 `ink`, the rest in `base`.
fn weighed(text: &str, ranges: &[Range<usize>], cx: &App) -> StyledText {
    let k = theme::colors(cx);
    StyledText::new(text.to_string()).with_highlights(ranges.iter().cloned().map(|r| {
        (
            r,
            HighlightStyle {
                color: Some(k.ink),
                font_weight: Some(FontWeight::SEMIBOLD),
                ..Default::default()
            },
        )
    }))
}

/// A 岗位 tag: 18 pt, radius 5, `sunken`.
fn tag(text: String, cx: &App) -> AnyElement {
    let k = theme::colors(cx);
    div()
        .flex_none()
        .h(theme::TAG_HEIGHT)
        .px(space::S6)
        .rounded(theme::TAG_RADIUS)
        .bg(k.sunken)
        .flex()
        .items_center()
        .t_meta()
        .text_color(k.ink_2)
        .child(text)
        .into_any_element()
}

/// A key in the footer: 20 pt, `sunken`.
fn key_box(children: Vec<AnyElement>, cx: &App) -> AnyElement {
    let k = theme::colors(cx);
    div()
        .flex_none()
        .min_w(px(20.))
        .h(px(20.))
        .px(px(5.))
        .rounded(px(5.))
        .bg(k.sunken)
        .flex()
        .items_center()
        .justify_center()
        .gap(space::S2)
        .text_color(k.ink_2)
        .children(children)
        .into_any_element()
}

fn enter_glyph(cx: &App) -> AnyElement {
    ui::icon(Lucide::CornerDownLeft, px(12.), theme::colors(cx).ink_2).into_any_element()
}

impl AskPalette {
    fn row_shell(&self, ix: usize, item: &Item, cx: &mut Context<Self>) -> gpui::Stateful<gpui::Div> {
        let k = theme::colors(cx);
        let active = ix == self.active;
        div()
            .id(ElementId::NamedInteger("ask-row".into(), ix as u64))
            .flex_none()
            .w_full()
            .h(item.height())
            .px(space::S10)
            .rounded(px(10.))
            .flex()
            .items_center()
            .gap(space::S12)
            .cursor_pointer()
            .when(active, |el| el.bg(k.sunken))
            .active(|s| s.bg(k.pressed))
            .on_mouse_move(cx.listener(move |this, e: &gpui::MouseMoveEvent, _, cx| {
                this.pointer_moved(ix, e.position, cx)
            }))
            .on_click(cx.listener(move |this, _, window, cx| this.open_row(ix, window, cx)))
    }

    fn give_row(&self, ix: usize, item: &Item, cx: &mut Context<Self>) -> AnyElement {
        let k = theme::colors(cx);
        let Item::Give {
            member,
            task,
            helper,
        } = item
        else {
            return div().into_any_element();
        };
        let active = ix == self.active;
        let first = self.first_give().is_some_and(|m| m.id == member.id);
        let (title, detail, detail_color) = if *helper && !first {
            (
                fill(c::ASK_GIVE_TO_JOJO, &[("name", &member.name)]),
                fill(c::ASK_GIVE_JOJO_DETAIL, &[("name", &member.name)]),
                k.ink_3,
            )
        } else {
            (
                fill(c::ASK_GIVE_TO, &[("name", &member.name)]),
                fill(c::ASK_GIVE_DETAIL, &[("task", task)]),
                k.ink_2,
            )
        };
        // What the member does now.
        let run = self
            .services
            .provider
            .active_runs()
            .into_iter()
            .find(|r| r.bot_id == member.id);
        let status: Option<AnyElement> = match run {
            Some(r) => {
                let title = self
                    .services
                    .provider
                    .topic(&r.group_id, &r.topic_id)
                    .map(|t| t.title)
                    .unwrap_or_default();
                Some(
                    div()
                        .flex_none()
                        .flex()
                        .items_center()
                        .gap(space::S6)
                        .t_meta()
                        .text_color(k.ink_3)
                        .child(ui::motion::breathe(
                            SharedString::from(format!("ask-live-{}", member.id)),
                            div().size(px(6.)).rounded_full().bg(k.live),
                            cx,
                        ))
                        .child(fill(c::ASK_WORKING_ON, &[("title", &title)]))
                        .into_any_element(),
                )
            }
            None if member.is_bot() => Some(
                div()
                    .flex_none()
                    .flex()
                    .items_center()
                    .gap(space::S6)
                    .t_meta()
                    .text_color(k.ink_3)
                    .child(
                        div()
                            .size(px(6.))
                            .rounded_full()
                            .border(px(1.5))
                            .border_color(k.ink_3),
                    )
                    .child(c::ASK_IDLE)
                    .into_any_element(),
            ),
            None => None,
        };
        // Enter gives to the first row (shown when it is chosen); ⌘Enter to
        // the helper (always shown).
        let keys: Vec<AnyElement> = if first {
            vec![enter_glyph(cx)]
        } else {
            vec![
                div().t_meta().child("⌘").into_any_element(),
                enter_glyph(cx),
            ]
        };
        let show_keys = !first || active;
        self.row_shell(ix, item, cx)
            .child(MemberAvatar::new(Some(member), SLOT))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .gap(space::S2)
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(space::S8)
                            .child(div().t_ui().medium().text_color(k.ink).child(title))
                            .children(member.role.clone().map(|r| tag(r, cx))),
                    )
                    .child(
                        div()
                            .t_meta()
                            .line_height(px(18.))
                            .text_color(detail_color)
                            .one_line()
                            .child(detail),
                    ),
            )
            .children(status)
            .child(
                div()
                    .flex_none()
                    .min_w(px(26.))
                    .h(px(22.))
                    .px(px(5.))
                    .rounded(px(6.))
                    .border_1()
                    .border_color(k.line_2)
                    .bg(k.bg)
                    .shadow(k.sec_hi())
                    .flex()
                    .items_center()
                    .justify_center()
                    .gap(space::S2)
                    .text_color(k.ink_2)
                    .when(!show_keys, |el| el.opacity(0.))
                    .children(keys),
            )
            .into_any_element()
    }

    fn topic_row(&self, ix: usize, item: &Item, cx: &mut Context<Self>) -> AnyElement {
        let k = theme::colors(cx);
        let Item::Topic { topic, ranges } = item else {
            return div().into_any_element();
        };
        let p = self.services.provider.as_ref();
        let me = self.services.me();
        let (state, meta) = rail_model::row_state(p, me.as_deref(), topic);
        let group = p
            .groups()
            .into_iter()
            .find(|g| g.id == topic.group_id)
            .map(|g| g.name)
            .unwrap_or_default();
        let owner = topic
            .owner_id
            .as_deref()
            .and_then(|m| p.member(m))
            .map(|m| m.name)
            .unwrap_or_default();
        let word = match &state {
            RowState::Done => Some(fill(c::ASK_TOPIC_DONE, &[("name", &owner)])),
            RowState::Working(m) => p
                .member(m)
                .map(|m| fill(c::ASK_TOPIC_WORKING, &[("name", &m.name)])),
            RowState::NeedsYou => Some(c::ASK_TOPIC_WAITING_YOU.to_string()),
            RowState::Waiting => meta.as_ref().map(|m| m.text.clone()),
            RowState::Idle => None,
        };
        let detail = match word {
            Some(w) => format!("{group} · {w}"),
            None => group,
        };
        let time = (topic.status == TopicStatus::Done || ranges.is_empty())
            .then(|| crate::views::shell::clock(topic.last_activity_at));
        let slot: AnyElement = match &state {
            RowState::NeedsYou => div().size(px(7.)).rounded_full().bg(k.you).into_any_element(),
            RowState::Working(m) => {
                let m = p.member(m);
                MemberAvatar::new(m.as_ref(), px(18.))
                    .ring(Some(Ring::Live))
                    .into_any_element()
            }
            RowState::Done => ui::icon(Lucide::Check, px(16.), k.ok).into_any_element(),
            RowState::Waiting => div()
                .size(px(7.))
                .rounded_full()
                .border(px(1.5))
                .border_color(k.ink_3)
                .into_any_element(),
            RowState::Idle => ui::icon(Lucide::Hash, px(16.), k.ink_3).into_any_element(),
        };
        let title = crate::views::sidebar::topic_display_title(&topic.title).0;
        let ranges: Vec<Range<usize>> = if title == topic.title {
            ranges.clone()
        } else {
            Vec::new()
        };
        self.row_shell(ix, item, cx)
            .child(
                div()
                    .flex_none()
                    .w(SLOT)
                    .flex()
                    .justify_center()
                    .child(slot),
            )
            .child(
                div()
                    .flex_shrink(1.)
                    .min_w_0()
                    .one_line()
                    .t_ui()
                    .text_color(if ranges.is_empty() { k.ink } else { k.ink_2 })
                    .child(weighed(&title, &ranges, cx)),
            )
            .child(
                div()
                    .flex_shrink(1.)
                    .min_w(px(40.))
                    .one_line()
                    .t_meta()
                    .text_color(k.ink_3)
                    .child(detail),
            )
            .child(div().flex_1())
            .children(time.map(|t| div().flex_none().t_mono().text_color(k.ink_3).child(t)))
            .into_any_element()
    }

    fn artifact_row(&self, ix: usize, item: &Item, cx: &mut Context<Self>) -> AnyElement {
        let k = theme::colors(cx);
        let Item::Artifact { artifact, ranges } = item else {
            return div().into_any_element();
        };
        let kind = match artifact.kind {
            ArtifactKind::Page => art_copy::PANEL_KIND_PAGE,
            ArtifactKind::File => art_copy::KIND_FILE,
            ArtifactKind::Image => c::ASK_ARTIFACT_IMAGE,
            ArtifactKind::Decision => c::ASK_ARTIFACT_DECISION,
        };
        let by = self
            .services
            .provider
            .member(&artifact.created_by)
            .map(|m| fill(c::ASK_ARTIFACT_BY, &[("name", &m.name)]));
        let glyph = match artifact.kind {
            ArtifactKind::Page => Lucide::ChartLine,
            ArtifactKind::Image => Lucide::Image,
            ArtifactKind::Decision => Lucide::Check,
            ArtifactKind::File => Lucide::File,
        };
        self.row_shell(ix, item, cx)
            .child(
                div()
                    .flex_none()
                    .w(SLOT)
                    .h(px(26.))
                    .rounded(px(5.))
                    .border_1()
                    .border_color(k.line)
                    .bg(k.bg)
                    .shadow(k.lift())
                    .overflow_hidden()
                    .pt(px(3.))
                    .px(px(4.))
                    .flex()
                    .flex_col()
                    .gap(px(2.))
                    .child(div().w(px(14.)).h(px(2.)).rounded(px(1.)).bg(k.ink_2))
                    .child(ui::icon(glyph, px(14.), k.ink_2)),
            )
            .child(
                div()
                    .flex_shrink(1.)
                    .min_w_0()
                    .one_line()
                    .t_ui()
                    .text_color(k.ink_2)
                    .child(weighed(&artifact.title, ranges, cx)),
            )
            .child(
                div()
                    .flex_none()
                    .flex()
                    .items_center()
                    .t_meta()
                    .text_color(k.ink_3)
                    .child(
                        div()
                            .mono()
                            .child(fill(art_copy::VERSION_SHORT, &[("n", &artifact.version.to_string())])),
                    )
                    .child(format!(" · {kind}")),
            )
            .child(div().flex_1())
            .children(by.map(|b| div().flex_none().t_meta().text_color(k.ink_3).child(b)))
            .into_any_element()
    }

    fn member_row(&self, ix: usize, item: &Item, cx: &mut Context<Self>) -> AnyElement {
        let k = theme::colors(cx);
        let Item::Member { member, ranges } = item else {
            return div().into_any_element();
        };
        let week = self
            .services
            .provider
            .member_stats(&member.id)
            .map(|s| s.topics_this_week)
            .filter(|n| *n > 0);
        self.row_shell(ix, item, cx)
            .child(
                div()
                    .flex_none()
                    .w(SLOT)
                    .flex()
                    .justify_center()
                    .child(MemberAvatar::new(Some(member), px(22.))),
            )
            .child(
                div()
                    .flex_none()
                    .t_ui()
                    .text_color(k.ink_2)
                    .child(weighed(&member.name, ranges, cx)),
            )
            .children(member.role.clone().map(|r| tag(r, cx)))
            .child(div().flex_1())
            .children(week.map(|n| {
                // `本周 4 个话题` with the count in mono.
                let n = n.to_string();
                let text = fill(c::ASK_MEMBER_WEEK, &[("n", &n)]);
                let (pre, post) = text.split_once(n.as_str()).unwrap_or((text.as_str(), ""));
                div()
                    .flex_none()
                    .flex()
                    .items_center()
                    .t_meta()
                    .text_color(k.ink_3)
                    .child(pre.to_string())
                    .child(div().mono().child(n.clone()))
                    .child(post.to_string())
            }))
            .into_any_element()
    }

    fn group_row(&self, ix: usize, item: &Item, cx: &mut Context<Self>) -> AnyElement {
        let k = theme::colors(cx);
        let Item::Group { entry, ranges } = item else {
            return div().into_any_element();
        };
        let lead: AnyElement = match &entry.member {
            Some(m) => MemberAvatar::new(Some(m), px(22.)).into_any_element(),
            None => ui::icon(Lucide::Hash, px(16.), k.ink_3).into_any_element(),
        };
        let detail = match &entry.target {
            Target::Group(g) => self
                .services
                .provider
                .groups()
                .into_iter()
                .find(|x| &x.id == g && x.kind == GroupKind::Group)
                .map(|x| fill(c::ASK_GROUP_MEMBERS, &[("n", &x.member_ids.len().to_string())]))
                .or_else(|| entry.detail.clone()),
            _ => entry.detail.clone(),
        };
        self.row_shell(ix, item, cx)
            .child(div().flex_none().w(SLOT).flex().justify_center().child(lead))
            .child(
                div()
                    .flex_shrink(1.)
                    .min_w_0()
                    .one_line()
                    .t_ui()
                    .text_color(if ranges.is_empty() { k.ink } else { k.ink_2 })
                    .child(weighed(&entry.label, ranges, cx)),
            )
            .children(detail.map(|d| {
                div()
                    .flex_shrink(1.)
                    .min_w(px(40.))
                    .one_line()
                    .t_meta()
                    .text_color(k.ink_3)
                    .child(d)
            }))
            .into_any_element()
    }

    fn row(&self, ix: usize, item: &Item, cx: &mut Context<Self>) -> AnyElement {
        match item {
            Item::Give { .. } => self.give_row(ix, item, cx),
            Item::Topic { .. } => self.topic_row(ix, item, cx),
            Item::Artifact { .. } => self.artifact_row(ix, item, cx),
            Item::Member { .. } => self.member_row(ix, item, cx),
            Item::Group { .. } => self.group_row(ix, item, cx),
        }
    }

    /// 发到 <群> ⌄: where a new 话题 goes.
    fn target_button(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let member = self.first_give()?.clone();
        let (_, name) = self.target_group(Some(&member), cx)?;
        let k = theme::colors(cx);
        let groups: Vec<(GroupId, String)> = rail_model::groups(self.services.provider.as_ref());
        let this = cx.entity().downgrade();
        let current = self.target_group(Some(&member), cx).map(|(id, _)| id);
        Some(
            Button::new("ask-target")
                .ghost()
                .h(px(26.))
                .pl(space::S8)
                .pr(space::S6)
                .rounded(px(7.))
                .border_1()
                .border_color(k.line)
                .tooltip(c::ASK_SEND_TO_GROUP)
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(space::S4)
                        .t_meta()
                        .line_height(px(18.))
                        .text_color(k.ink_3)
                        .child(c::ASK_SEND_TO)
                        .child(div().text_color(k.ink).medium().child(name))
                        .child(ui::icon(Lucide::ChevronDown, px(12.), k.ink_3)),
                )
                .dropdown_menu_with_anchor(gpui::Anchor::TopRight, move |menu, _, _| {
                    groups.iter().fold(menu, |menu, (id, name)| {
                        let this = this.clone();
                        let id2 = id.clone();
                        menu.item(
                            PopupMenuItem::new(name.clone())
                                .checked(current.as_ref() == Some(id))
                                .on_click(move |_, _, cx| {
                                    let id = id2.clone();
                                    let _ = this.update(cx, |this, cx| {
                                        this.target = Some(id);
                                        cx.notify();
                                    });
                                }),
                        )
                    })
                })
                .into_any_element(),
        )
    }

    fn footer(&self, cx: &mut Context<Self>) -> AnyElement {
        let k = theme::colors(cx);
        let enter = match self.active_item() {
            Some(Item::Give { member, .. }) => fill(c::ASK_GIVE_TO, &[("name", &member.name)]),
            _ => c::ASK_HINT_OPEN.to_string(),
        };
        // ⌘Enter gives to the helper while another row is chosen.
        let helper = self.items().find_map(|i| match i {
            Item::Give {
                member,
                helper: true,
                ..
            } => Some(member.clone()),
            _ => None,
        });
        let helper_hint = helper.filter(|h| {
            !matches!(self.active_item(), Some(Item::Give { member, .. }) if member.id == h.id)
        });
        let hint = |keys: AnyElement, label: String| {
            div()
                .flex_none()
                .flex()
                .items_center()
                .gap(space::S6)
                .child(keys)
                .child(label)
        };
        div()
            .flex_none()
            .h(FOOTER)
            .px(space::S16)
            .flex()
            .items_center()
            .gap(space::S20)
            .border_t_1()
            .border_color(k.line)
            .bg(k.raised)
            .t_meta()
            .line_height(px(18.))
            .text_color(k.ink_3)
            .child(hint(key_box(vec![enter_glyph(cx)], cx), enter))
            .children(helper_hint.map(|h| {
                hint(
                    key_box(
                        vec![div().child("⌘").into_any_element(), enter_glyph(cx)],
                        cx,
                    ),
                    fill(c::ASK_GIVE_TO, &[("name", &h.name)]),
                )
            }))
            .child(hint(
                key_box(
                    vec![
                        ui::icon(Lucide::ArrowUp, px(12.), k.ink_2).into_any_element(),
                        ui::icon(Lucide::ArrowDown, px(12.), k.ink_2).into_any_element(),
                    ],
                    cx,
                ),
                c::ASK_HINT_SELECT.into(),
            ))
            .child(div().flex_1())
            .child(hint(
                key_box(
                    vec![div().t_mono().text_size(px(11.)).child(nav_copy::KEY_ESC).into_any_element()],
                    cx,
                ),
                c::ASK_HINT_CLOSE.into(),
            ))
            .into_any_element()
    }
}

impl Render for AskPalette {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let k = theme::colors(cx);
        // Children of the list with their heights (rows true, labels false).
        let mut children: Vec<AnyElement> = Vec::new();
        let mut heights: Vec<(Pixels, bool)> = Vec::new();
        let mut active_child = 0;
        let mut ix = 0;
        let sections = self.sections.clone();
        for (si, section) in sections.iter().enumerate() {
            children.push(
                div()
                    .flex_none()
                    .h(LABEL_ROW)
                    .px(space::S10)
                    .when(si > 0 && !sections[si - 1].divider, |el| el.mt(space::S2))
                    .flex()
                    .items_center()
                    .t_meta_medium()
                    .text_color(k.ink_3)
                    .child(section.label)
                    .into_any_element(),
            );
            heights.push((LABEL_ROW, false));
            for item in &section.items {
                if ix == self.active {
                    active_child = children.len();
                }
                let row = self.row(ix, item, cx);
                children.push(ui::motion::rise(
                    SharedString::from(format!("ask-{}", item.key())),
                    ix.min(6),
                    div().flex_none().child(row),
                    cx,
                ));
                heights.push((item.height(), true));
                ix += 1;
            }
            if section.divider {
                children.push(
                    div()
                        .flex_none()
                        .h(DIVIDER)
                        .px(space::S10)
                        .flex()
                        .items_center()
                        .child(div().w_full().h(px(1.)).bg(k.line))
                        .into_any_element(),
                );
                heights.push((DIVIDER, false));
            }
        }
        let empty = ix == 0;
        if !empty && std::mem::take(&mut self.reveal) {
            let target = if active_child > 0 && !heights[active_child - 1].1 {
                active_child - 1
            } else {
                active_child
            };
            self.scroll.scroll_to_item(target);
        }
        let max = (window.viewport_size().height * 0.7 - QUERY_ROW - FOOTER - px(14.)).max(ROW);
        let total: Pixels = heights.iter().map(|(h, _)| *h).sum();
        let list_h = total.min(max);
        let target = self.target_button(cx);
        div()
            .key_context(CONTEXT)
            .track_focus(&self.focus)
            .on_action(cx.listener(|this, _: &SelectPrevious, _, cx| this.select(-1, cx)))
            .on_action(cx.listener(|this, _: &SelectNext, _, cx| this.select(1, cx)))
            .on_action(
                cx.listener(|this, _: &GiveToHelper, window, cx| this.give_to_helper(window, cx)),
            )
            .w_full()
            .flex()
            .flex_col()
            .bg(k.bg)
            .text_color(k.ink)
            .child(
                div()
                    .flex_none()
                    .h(QUERY_ROW)
                    .px(space::S16)
                    .flex()
                    .items_center()
                    .gap(space::S12)
                    .border_b_1()
                    .border_color(k.line)
                    .child(ui::icon(Lucide::Search, px(18.), k.ink_3))
                    .child(
                        div().flex_1().min_w(px(0.)).child(ui::placeholder_input(
                            Input::new(&self.query)
                                .appearance(false)
                                .px_0()
                                .py_0()
                                .text_size(px(15.))
                                .line_height(px(22.)),
                            window,
                            cx,
                        )),
                    )
                    .children(target),
            )
            .child(
                div()
                    .id("ask-list")
                    .flex_none()
                    .when(!empty, |el| el.h(list_h + px(14.)))
                    .overflow_y_scroll()
                    .track_scroll(&self.scroll)
                    .pt(space::S6)
                    .px(space::S6)
                    .pb(space::S8)
                    .flex()
                    .flex_col()
                    .on_mouse_move(cx.listener(|this, e: &gpui::MouseMoveEvent, _, _| {
                        this.pointer.get_or_insert(e.position);
                    }))
                    .children(children)
                    .when(empty, |el| {
                        el.child(
                            div()
                                .h(px(120.))
                                .child(ui::EmptyState::new(Lucide::SearchX, c::ASK_EMPTY)),
                        )
                    }),
            )
            .child(self.footer(cx))
    }
}

/// Opens 交代或搜索 in `window` (⌘K).
pub fn open(services: Services, window: &mut Window, cx: &mut App) {
    open_with(services, "", None, window, cx);
}

/// Opens it with `query` typed and 发到 set to `group` (the self-test).
pub fn open_with(
    services: Services,
    query: &str,
    group: Option<GroupId>,
    window: &mut Window,
    cx: &mut App,
) -> Entity<AskPalette> {
    bind_keys(cx);
    let query = query.to_string();
    let view = cx.new(|cx| {
        let mut s = AskPalette::new(services, window, cx);
        s.target = group;
        if !query.is_empty() {
            s.query
                .update(cx, |q, cx| q.set_value(query.clone(), window, cx));
            s.requery(cx);
        }
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
        let k = theme::colors(cx);
        crate::ui::sheet::switcher(dialog, cx)
            .close_button(false)
            .p_0()
            .bg(k.bg)
            .border_color(k.line)
            .rounded(px(14.))
            .shadow(k.pop())
            .overflow_hidden()
            .child(view.clone())
    });
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

impl AskPalette {
    /// Whether the query field has the keyboard focus (the self-test).
    pub fn query_focused(&self, window: &Window, cx: &App) -> bool {
        self.query.read(cx).focus_handle(cx).is_focused(window)
    }
}
