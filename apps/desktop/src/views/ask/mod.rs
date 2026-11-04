//! ⌘K 交代或搜索 in the Spotlight-style panel (NATIVE.md §9.7): 600 wide, a
//! 48 pt capsule field with 22 pt text, 32 pt rows with symbols. Typing a
//! sentence offers 交代 first: 交给 <the member it names> opens a 话题
//! addressed to that member in the 群 of 发到 (Enter), and 交给 Jojo 安排
//! hands it to the main helper (⌘Enter). Below, the 话题, 产出, 成员 and 群
//! that share words with the query, the shared words in weight 600. With no
//! query it lists 最近, then 群 and 私聊, like the quick switcher it
//! replaces.

pub mod model;

use std::ops::Range;

use gpui::{
    AnyElement, App, AppContext as _, Context, ElementId, Entity, FocusHandle, Focusable,
    InteractiveElement as _, IntoElement, KeyBinding, ParentElement, Pixels, Point, Render,
    ScrollHandle, StatefulInteractiveElement as _, Styled, Subscription, Window, div,
    prelude::FluentBuilder as _, px,
};
use gpui_component::{
    WindowExt as _,
    input::{InputEvent, InputState},
    menu::{DropdownMenu as _, PopupMenuItem},
};

use crate::{
    app::{Place, Services},
    copy::{artifacts as art_copy, fill, nav as nav_copy, rail as c, search as search_copy},
    data::{ArtifactKind, ArtifactSummary, GroupId, GroupKind, Member, StartTopicRequest, Topic},
    ui::{
        MemberAvatar, Sym,
        native::{
            BadgeState, PushSize, StatusBadge, native_colors,
            overlays::{
                SpotlightField, spot, spotlight, spotlight_divider, spotlight_ink, spotlight_row,
                spotlight_section,
            },
            popup_button,
        },
    },
    views::{
        nav::switcher::{self, Entry, Target, palette},
        rail::model as rail_model,
    },
};

gpui::actions!(
    ask,
    [SelectPrevious, SelectNext, OpenSelected, GiveToHelper]
);

const CONTEXT: &str = "AskPalette";
/// Rows per section while a query is typed.
const TOPICS_MAX: usize = 5;
const OTHERS_MAX: usize = 3;
const RECENT: usize = 5;
/// The widest 发到 <群> gets (F48); its tooltip names the whole 群.
const TARGET_NAME_MAX: Pixels = px(220.);

/// Byte ranges of `text` (what [`model::weigh`] gives) as the char indices
/// `weighted` takes.
fn char_ranges(text: &str, ranges: &[Range<usize>]) -> Vec<Range<usize>> {
    let at = |b: usize| text[..b.min(text.len())].chars().count();
    ranges.iter().map(|r| at(r.start)..at(r.end)).collect()
}

/// One choosable row.
#[derive(Clone, Debug)]
enum Item {
    /// 交给 <member>: a new 话题 addressed to it. `helper` is the 交给 Jojo
    /// 安排 row.
    Give {
        member: Member,
        task: String,
        helper: bool,
        /// The query names this member (`让 Ledger …`).
        named: bool,
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
    /// The name the query asks for when no member has it (`让 Ledger …`
    /// with no Ledger): said above the fallback to the helper.
    nobody: Option<String>,
    _subscriptions: Vec<Subscription>,
}

impl Focusable for AskPalette {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

/// The palette's keys (call once at start-up, beside the switcher's).
pub fn bind_keys(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("up", SelectPrevious, Some(CONTEXT)),
        KeyBinding::new("down", SelectNext, Some(CONTEXT)),
        KeyBinding::new("ctrl-p", SelectPrevious, Some(CONTEXT)),
        KeyBinding::new("ctrl-n", SelectNext, Some(CONTEXT)),
        KeyBinding::new("enter", OpenSelected, Some(CONTEXT)),
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
            // Enter and ⌘Enter are key actions of the palette (`bind_keys`).
            |this, _, event: &InputEvent, _, cx| {
                if let InputEvent::Change = event {
                    this.requery(cx)
                }
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
            nobody: None,
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
        let members = self.services.provider.members();
        let me = self.services.me();
        self.nobody = model::named(&text, &members, me.as_deref())
            .is_empty()
            .then(|| model::intended_name(&text))
            .flatten();
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
                named: true,
            });
        }
        if let Some(h) = &helper
            && !give
                .iter()
                .any(|g| matches!(g, Item::Give { member, .. } if member.id == h.id))
        {
            give.push(Item::Give {
                member: h.clone(),
                task: model::task(query, named.first().map(|m| m.name.as_str())),
                helper: true,
                named: false,
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
        topics.sort_by_key(|(w, t)| {
            (
                std::cmp::Reverse(w.score),
                std::cmp::Reverse(t.last_activity_at),
            )
        });
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
        artifacts
            .sort_by_key(|(w, a)| (std::cmp::Reverse(w.score), std::cmp::Reverse(a.updated_at)));
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
                Some(std::iter::once(0..m.name.len()).collect())
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
                ..
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

impl AskPalette {
    fn row_shell(&self, ix: usize, cx: &mut Context<Self>) -> gpui::Stateful<gpui::Div> {
        spotlight_row(
            ElementId::NamedInteger("ask-row".into(), ix as u64),
            ix == self.active,
            cx,
        )
        .on_mouse_move(cx.listener(move |this, e: &gpui::MouseMoveEvent, _, cx| {
            this.pointer_moved(ix, e.position, cx)
        }))
        .on_click(cx.listener(move |this, _, window, cx| this.open_row(ix, window, cx)))
    }

    /// The avatar slot of a row (16), on the row's fill.
    fn avatar(&self, ix: usize, member: &Member, cx: &App) -> gpui::Div {
        let k = native_colors(cx);
        let surface = if ix == self.active { k.accent } else { k.sheet };
        div()
            .flex_none()
            .size(spot::SLOT)
            .child(MemberAvatar::new(Some(member), spot::AVATAR).on(surface))
    }

    /// 交给 <member>: the avatar, the action, what it opens; the member's
    /// state as a StatusBadge; the ↩ column. ⌘↩ is in the footer.
    fn give_row(&self, ix: usize, item: &Item, cx: &mut Context<Self>) -> AnyElement {
        let Item::Give {
            member,
            task,
            helper,
            named,
        } = item
        else {
            return div().into_any_element();
        };
        let active = ix == self.active;
        let (_, ink2) = spotlight_ink(active, cx);
        // The helper reads 交给 <name> 安排 unless the query names it.
        let (title, detail) = if *helper && !*named {
            (
                fill(c::ASK_GIVE_TO_JOJO, &[("name", &member.name)]),
                fill(c::ASK_GIVE_JOJO_DETAIL, &[("name", &member.name)]),
            )
        } else {
            (
                fill(c::ASK_GIVE_TO, &[("name", &member.name)]),
                fill(c::ASK_GIVE_DETAIL, &[("work", task)]),
            )
        };
        // What the member does now: 在做 (the 话题 in its tooltip) or 空闲.
        let run = self
            .services
            .provider
            .active_runs()
            .into_iter()
            .find(|r| r.bot_id == member.id);
        let status = match run {
            Some(r) => {
                let topic = self
                    .services
                    .provider
                    .topic(&r.group_id, &r.topic_id)
                    .map(|t| t.title)
                    .unwrap_or_default();
                let tip: gpui::SharedString = fill(c::ASK_WORKING_ON, &[("title", &topic)]).into();
                Some(
                    div()
                        .id(ElementId::NamedInteger("ask-status".into(), ix as u64))
                        .tooltip(move |window, cx| {
                            gpui_component::tooltip::Tooltip::new(tip.clone()).build(window, cx)
                        })
                        .child(StatusBadge::new(
                            BadgeState::Working,
                            search_copy::STATUS_WORKING,
                        ))
                        .into_any_element(),
                )
            }
            None if member.is_bot() => Some(
                StatusBadge::new(BadgeState::Offline, search_copy::STATUS_IDLE).into_any_element(),
            ),
            None => None,
        };
        self.row_shell(ix, cx)
            .child(self.avatar(ix, member, cx))
            .child(palette::title(
                ElementId::NamedInteger("ask-title".into(), ix as u64),
                &title,
                &[],
                true,
            ))
            .child(palette::meta(detail, ink2))
            .child(div().flex_1())
            .child(palette::trailing(status, None, active, ink2))
            .into_any_element()
    }

    fn topic_row(&self, ix: usize, item: &Item, cx: &mut Context<Self>) -> AnyElement {
        let Item::Topic { topic, ranges } = item else {
            return div().into_any_element();
        };
        let active = ix == self.active;
        let (_, ink2) = spotlight_ink(active, cx);
        let p = self.services.provider.as_ref();
        let me = self.services.me();
        let group = p
            .groups()
            .into_iter()
            .find(|g| g.id == topic.group_id)
            .map(|g| g.name)
            .unwrap_or_default();
        let title = crate::views::sidebar::topic_display_title(&topic.title).0;
        let ranges: Vec<Range<usize>> = if title == topic.title {
            char_ranges(&title, ranges)
        } else {
            Vec::new()
        };
        self.row_shell(ix, cx)
            .child(palette::kind_icon(Sym::Topic, cx))
            .child(palette::title(
                ElementId::NamedInteger("ask-title".into(), ix as u64),
                &title,
                &ranges,
                active,
            ))
            .child(palette::meta(group, ink2))
            .child(div().flex_1())
            .child(palette::trailing(
                palette::topic_status(p, me.as_deref(), topic),
                Some(palette::time_text(topic.last_activity_at)),
                active,
                ink2,
            ))
            .into_any_element()
    }

    fn artifact_row(&self, ix: usize, item: &Item, cx: &mut Context<Self>) -> AnyElement {
        let Item::Artifact { artifact, ranges } = item else {
            return div().into_any_element();
        };
        let active = ix == self.active;
        let (_, ink2) = spotlight_ink(active, cx);
        let kind = match artifact.kind {
            ArtifactKind::Page => art_copy::PANEL_KIND_PAGE,
            ArtifactKind::File => art_copy::KIND_FILE,
            ArtifactKind::Image => c::ASK_ARTIFACT_IMAGE,
            ArtifactKind::Decision => c::ASK_ARTIFACT_DECISION,
        };
        let glyph = match artifact.kind {
            ArtifactKind::Page => Sym::Document,
            ArtifactKind::Image => Sym::Image,
            ArtifactKind::Decision => Sym::Check,
            ArtifactKind::File => Sym::File,
        };
        let version = fill(
            art_copy::VERSION_SHORT,
            &[("n", &artifact.version.to_string())],
        );
        // v3 · 网页 · Ledger 做的: one muted meta after the title.
        let mut meta = format!("{version} · {kind}");
        if let Some(m) = self.services.provider.member(&artifact.created_by) {
            meta.push_str(" · ");
            meta.push_str(&fill(c::ASK_ARTIFACT_BY, &[("name", &m.name)]));
        }
        self.row_shell(ix, cx)
            .child(palette::kind_icon(glyph, cx))
            .child(palette::title(
                ElementId::NamedInteger("ask-title".into(), ix as u64),
                &artifact.title,
                &char_ranges(&artifact.title, ranges),
                active,
            ))
            .child(palette::meta(meta, ink2))
            .child(div().flex_1())
            .child(palette::trailing(
                None,
                Some(palette::time_text(artifact.updated_at)),
                active,
                ink2,
            ))
            .into_any_element()
    }

    fn member_row(&self, ix: usize, item: &Item, cx: &mut Context<Self>) -> AnyElement {
        let Item::Member { member, ranges } = item else {
            return div().into_any_element();
        };
        let active = ix == self.active;
        let (_, ink2) = spotlight_ink(active, cx);
        let week = self
            .services
            .provider
            .member_stats(&member.id)
            .map(|s| s.topics_this_week)
            .filter(|n| *n > 0);
        // 岗位 · 本周 4 个话题: one muted meta after the name.
        let parts: Vec<String> = member
            .role
            .clone()
            .filter(|r| !r.trim().is_empty())
            .into_iter()
            .chain(week.map(|n| fill(c::ASK_MEMBER_WEEK, &[("n", &n.to_string())])))
            .collect();
        let meta = (!parts.is_empty()).then(|| parts.join(" · "));
        self.row_shell(ix, cx)
            .child(self.avatar(ix, member, cx))
            .child(palette::title(
                ElementId::NamedInteger("ask-title".into(), ix as u64),
                &member.name,
                &char_ranges(&member.name, ranges),
                active,
            ))
            .children(meta.map(|m| palette::meta(m, ink2)))
            .child(div().flex_1())
            .child(palette::trailing(None, None, active, ink2))
            .into_any_element()
    }

    fn group_row(&self, ix: usize, item: &Item, cx: &mut Context<Self>) -> AnyElement {
        let Item::Group { entry, ranges } = item else {
            return div().into_any_element();
        };
        let active = ix == self.active;
        let (_, ink2) = spotlight_ink(active, cx);
        let lead = match &entry.member {
            Some(m) => self.avatar(ix, m, cx),
            None => palette::kind_icon(Sym::Members, cx),
        };
        // A 群's member count never truncates (the title gives way); a
        // DM's 岗位 is the muted meta.
        let count = match &entry.target {
            Target::Group(g) => self
                .services
                .provider
                .groups()
                .into_iter()
                .find(|x| &x.id == g && x.kind == GroupKind::Group)
                .map(|x| {
                    fill(
                        c::ASK_GROUP_MEMBERS,
                        &[("n", &x.member_ids.len().to_string())],
                    )
                }),
            _ => None,
        };
        let meta = count.is_none().then(|| entry.detail.clone()).flatten();
        self.row_shell(ix, cx)
            .child(lead)
            .child(palette::title(
                ElementId::NamedInteger("ask-title".into(), ix as u64),
                &entry.label,
                &char_ranges(&entry.label, ranges),
                active,
            ))
            .children(meta.map(|d| palette::meta(d, ink2)))
            .children(count.map(|n| palette::count(n, ink2)))
            .child(div().flex_1())
            .child(palette::trailing(None, None, active, ink2))
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

    /// 发到 <群>: a pop-up button at the trailing edge of the field (where a
    /// new 话题 goes).
    fn target_button(&self, window: &Window, cx: &mut Context<Self>) -> Option<AnyElement> {
        let member = self.first_give()?.clone();
        let (_, name) = self.target_group(Some(&member), cx)?;
        let groups: Vec<(GroupId, String)> = rail_model::groups(self.services.provider.as_ref());
        let this = cx.entity().downgrade();
        let current = self.target_group(Some(&member), cx).map(|(id, _)| id);
        let label = format!("{} {name}", c::ASK_SEND_TO);
        Some(
            popup_button("ask-target", label.clone(), PushSize::Regular, window, cx)
                .max_w(TARGET_NAME_MAX)
                .tooltip(label)
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
}

impl Render for AskPalette {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let k = native_colors(cx);
        // Children of the list with their heights (rows true, labels false).
        let mut children: Vec<AnyElement> = Vec::new();
        let mut heights: Vec<(Pixels, bool)> = Vec::new();
        let mut active_child = 0;
        let mut ix = 0;
        let sections = self.sections.clone();
        for section in sections.iter() {
            children.push(spotlight_section(section.label, cx).into_any_element());
            heights.push((spot::SECTION, false));
            // 让 Ledger … with no Ledger: say so above the fallback.
            if section.label == c::ASK_SECTION_GIVE
                && let Some(name) = &self.nobody
            {
                children.push(
                    div()
                        .flex_none()
                        .h(spot::ROW)
                        .mx(spot::INSET)
                        .px(crate::ui::native::tokens::space::S8)
                        .flex()
                        .items_center()
                        .child(crate::ui::native::page::compact_empty(
                            Sym::Info,
                            fill(search_copy::NOBODY_NAMED, &[("name", name)]),
                            None,
                            cx,
                        ))
                        .into_any_element(),
                );
                heights.push((spot::ROW, false));
            }
            for item in &section.items {
                if ix == self.active {
                    active_child = children.len();
                }
                children.push(self.row(ix, item, cx));
                heights.push((spot::ROW, true));
                ix += 1;
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
        let room = window.viewport_size().height * 0.6
            - spot::FIELD
            - palette::FOOTER
            - palette::LIST_PAD * 2.;
        let list_h = palette::list_height(&heights, room);
        let text = self.text(cx);
        let has_query = !text.is_empty();
        let field = match self.target_button(window, cx) {
            Some(t) => SpotlightField::new(&self.query).trailing(t),
            None => SpotlightField::new(&self.query),
        };
        // The footer: ↑↓ 选择, ↩ 打开, ⌘↩ 交给 <helper> 安排, esc 关闭.
        let mut hints = palette::basic_hints(cx);
        if let Some(h) = self.items().find_map(|i| match i {
            Item::Give {
                member,
                helper: true,
                ..
            } => Some(member.name.clone()),
            _ => None,
        }) {
            hints.insert(
                2,
                palette::hint(
                    &[search_copy::KEY_COMMAND, search_copy::KEY_RETURN],
                    fill(search_copy::HINT_GIVE_HELPER, &[("name", &h)]),
                    cx,
                ),
            );
        }
        let clear = palette::state_button("ask-clear", search_copy::NO_MATCH_CLEAR, cx)
            .on_click(cx.listener(|this, _, window, cx| {
                this.query.update(cx, |q, cx| {
                    q.set_value("", window, cx);
                    q.focus(window, cx);
                });
            }))
            .into_any_element();
        div()
            .key_context(CONTEXT)
            .track_focus(&self.focus)
            .on_action(cx.listener(|this, _: &SelectPrevious, _, cx| this.select(-1, cx)))
            .on_action(cx.listener(|this, _: &SelectNext, _, cx| this.select(1, cx)))
            .on_action(cx.listener(|this, _: &OpenSelected, window, cx| this.confirm(window, cx)))
            .on_action(
                cx.listener(|this, _: &GiveToHelper, window, cx| this.give_to_helper(window, cx)),
            )
            .w_full()
            .flex()
            .flex_col()
            .text_color(k.label)
            .child(field)
            .when(!empty || has_query, |el| el.child(spotlight_divider(cx)))
            .when(!empty, |el| {
                el.child(
                    div()
                        .id("ask-list")
                        .flex_none()
                        .h(list_h + palette::LIST_PAD * 2.)
                        .overflow_y_scroll()
                        .track_scroll(&self.scroll)
                        .py(palette::LIST_PAD)
                        .flex()
                        .flex_col()
                        .on_mouse_move(cx.listener(|this, e: &gpui::MouseMoveEvent, _, _| {
                            this.pointer.get_or_insert(e.position);
                        }))
                        .children(children),
                )
            })
            .when(empty && has_query, |el| {
                el.child(palette::no_match(&text, clear, cx))
            })
            .when(!empty || has_query, |el| {
                el.child(palette::footer(hints, cx))
            })
    }
}

/// Opens 搜索或交代 in `window` (⌘K).
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
        spotlight(dialog, cx).child(view.clone())
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
