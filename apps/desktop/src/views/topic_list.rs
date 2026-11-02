//! The middle column (DESIGN.md §7.6, §8.1): the topics of the selected 群
//! (or the 待办 list) under a 92 pt header band (the 52 pt title row with
//! the 新话题 button, then the 40 pt tabs 全部 · 未读 · 等我). Each topic is a
//! 68 pt two-line row: the responsible member's avatar, title and time, the
//! preview `作者：内容` and one trailing element. Rows have one height, so the
//! list is a `uniform_list` and scales to many topics.

use std::{collections::HashMap, time::Duration};

use chrono::{DateTime, Datelike, Local, TimeZone};
use gpui::{
    Context, ElementId, InteractiveElement as _, IntoElement, ParentElement, Render, SharedString,
    StatefulInteractiveElement, Styled, Subscription, Window, div, prelude::FluentBuilder as _, px,
    uniform_list,
};
use gpui_component::{menu::ContextMenuExt as _, tooltip::Tooltip};

use crate::ui::NamedButton as _;
use crate::{
    app::{Changes, Place, Services},
    copy::{common, fill, nav, realtime as r},
    data::{Change, GroupId, Member, MemberId, Millis, Topic, TopicId, TopicStatus},
    theme::{self, Type as _, size, space},
    ui::{self, BadgeKind, CountBadge, Lucide, RowSurface, StatusChip},
    views::{
        sidebar::{edge_fade, scroll_edges, topic_line, watch_scroll_edges},
        topic_menu,
    },
};

/// The tabs above the topics of a group (DESIGN.md §8.1): 全部, 未读, 等我.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum TopicFilter {
    #[default]
    All,
    /// Topics with unread messages for me.
    Unread,
    /// The 待办 of this group: `waitingOn` includes me and the topic is not
    /// done, or it has unread mentions of me.
    Todo,
}

impl TopicFilter {
    pub const ALL: [TopicFilter; 3] = [TopicFilter::All, TopicFilter::Unread, TopicFilter::Todo];

    fn label(self) -> &'static str {
        match self {
            TopicFilter::All => nav::FILTER_ALL,
            TopicFilter::Unread => nav::FILTER_UNREAD,
            TopicFilter::Todo => nav::FILTER_TODO,
        }
    }

    pub fn keeps(self, topic: &Topic, me: Option<&str>) -> bool {
        match self {
            TopicFilter::All => true,
            TopicFilter::Unread => topic.unread > 0 || topic.mentions > 0,
            TopicFilter::Todo => {
                topic.mentions > 0
                    || (topic.status != TopicStatus::Done
                        && me.is_some_and(|me| topic.waiting_on.iter().any(|m| m == me)))
            }
        }
    }
}

/// The time of a topic row (DESIGN.md §8.2): today `14:32`, yesterday `昨天`,
/// within 7 days `周一`, this year `10/28`, older `2024/12/31` (a 4-digit
/// year, so `23/11/04` cannot read as day/month).
pub fn list_time(at: Millis, now: DateTime<Local>) -> String {
    // A missing (zero) timestamp shows nothing, never 1970年1月1日.
    if at <= 0 {
        return String::new();
    }
    let Some(then) = Local.timestamp_millis_opt(at).single() else {
        return String::new();
    };
    let days = (now.date_naive() - then.date_naive()).num_days();
    if days <= 0 {
        then.format("%H:%M").to_string()
    } else if days == 1 {
        common::YESTERDAY.to_string()
    } else if days < 7 {
        let names = [
            nav::WEEKDAY_MON,
            nav::WEEKDAY_TUE,
            nav::WEEKDAY_WED,
            nav::WEEKDAY_THU,
            nav::WEEKDAY_FRI,
            nav::WEEKDAY_SAT,
            nav::WEEKDAY_SUN,
        ];
        names[then.weekday().num_days_from_monday() as usize].to_string()
    } else if then.year() == now.year() {
        format!("{}/{}", then.month(), then.day())
    } else {
        format!("{}/{}/{}", then.year(), then.month(), then.day())
    }
}

/// The full time for a tooltip on any time (§8.2): `2025年11月4日 23:59`.
pub fn full_time(at: Millis) -> String {
    if at <= 0 {
        return String::new();
    }
    let Some(then) = Local.timestamp_millis_opt(at).single() else {
        return String::new();
    };
    let date = fill(
        nav::YEAR_MONTH_DAY,
        &[
            ("y", &then.year().to_string()),
            ("m", &then.month().to_string()),
            ("d", &then.day().to_string()),
        ],
    );
    format!("{date} {}", then.format("%H:%M"))
}

/// Relative time for the topic list: 刚刚, N 分钟前, N 小时前, 昨天, N 天前,
/// M月D日, and Y年M月D日 for another year.
pub fn relative_time(at: Millis, now: DateTime<Local>) -> String {
    let Some(then) = Local.timestamp_millis_opt(at).single() else {
        return String::new();
    };
    let ago = now.signed_duration_since(then);
    let days = (now.date_naive() - then.date_naive()).num_days();
    if ago.num_minutes() < 1 {
        common::JUST_NOW.to_string()
    } else if ago.num_minutes() < 60 {
        fill(
            common::MINUTES_AGO,
            &[("n", &ago.num_minutes().to_string())],
        )
    } else if days == 0 {
        fill(nav::HOURS_AGO, &[("n", &ago.num_hours().to_string())])
    } else if days == 1 {
        common::YESTERDAY.to_string()
    } else if days < 7 {
        fill(nav::DAYS_AGO, &[("n", &days.to_string())])
    } else if then.year() == now.year() {
        fill(
            common::MONTH_DAY,
            &[
                ("m", &then.month().to_string()),
                ("d", &then.day().to_string()),
            ],
        )
    } else {
        fill(
            nav::YEAR_MONTH_DAY,
            &[
                ("y", &then.year().to_string()),
                ("m", &then.month().to_string()),
                ("d", &then.day().to_string()),
            ],
        )
    }
}

/// One row: a topic and the group it belongs to (the 待办 list mixes groups).
#[derive(Clone)]
struct Row {
    group_id: GroupId,
    group_name: Option<String>,
    topic: Topic,
}

pub struct TopicListView {
    services: Services,
    place: Option<Place>,
    title: String,
    /// Every topic of the place; `rows` is the filtered view.
    all: Vec<Row>,
    rows: Vec<Row>,
    members: Vec<Member>,
    /// Who is typing, per topic of the shown rows.
    typing: HashMap<TopicId, Vec<MemberId>>,
    filter: TopicFilter,
    /// The filter of each 群, remembered while the app runs.
    filters: HashMap<GroupId, TopicFilter>,
    /// Row previews by topic, with the `last_seq` they were made at.
    previews: HashMap<TopicId, (u64, String)>,
    scroll: gpui::UniformListScrollHandle,
    _subscriptions: Vec<Subscription>,
    /// Re-renders when the window becomes key or stops being key.
    activation: Option<Subscription>,
    /// The selection uses the accent tint (the window is key).
    emphasized: bool,
}

impl TopicListView {
    pub fn new(services: Services, cx: &mut Context<Self>) -> Self {
        let subscriptions = vec![
            cx.subscribe(&services.hub, |this, _, changes: &Changes, cx| {
                let relevant = changes.any(|c| match (c, &this.place) {
                    (Change::Topics { group_id }, Some(Place::Group(g))) => g == group_id,
                    (Change::Todo | Change::Topics { .. }, Some(Place::Todo)) => true,
                    (Change::Members | Change::Groups | Change::Session, _) => true,
                    _ => false,
                });
                if relevant {
                    this.refresh(cx);
                    cx.notify();
                } else if changes.any(|c| match c {
                    Change::Typing { group_id, .. } => this.shows_group(group_id),
                    _ => false,
                }) {
                    this.refresh_typing();
                    cx.notify();
                }
            }),
            cx.observe(&services.nav, |this, _, cx| {
                this.refresh(cx);
                cx.notify();
            }),
        ];
        // Relative times change as time passes.
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(Duration::from_secs(30))
                    .await;
                if this.update(cx, |_, cx| cx.notify()).is_err() {
                    break;
                }
            }
        })
        .detach();
        let mut this = Self {
            services,
            place: None,
            title: String::new(),
            all: Vec::new(),
            rows: Vec::new(),
            members: Vec::new(),
            typing: HashMap::new(),
            filter: TopicFilter::All,
            filters: HashMap::new(),
            previews: HashMap::new(),
            scroll: gpui::UniformListScrollHandle::new(),
            _subscriptions: subscriptions,
            activation: None,
            emphasized: true,
        };
        this.refresh(cx);
        this
    }

    fn refresh(&mut self, cx: &mut Context<Self>) {
        let p = self.services.provider.clone();
        let place = self.services.nav.read(cx).place.clone();
        if place != self.place {
            self.filter = match &place {
                Some(Place::Group(g)) => self.filters.get(g).copied().unwrap_or_default(),
                _ => TopicFilter::All,
            };
        }
        self.place = place;
        self.members = p.members();
        match &self.place {
            Some(Place::Todo) => {
                self.title = nav::TODO.to_string();
                self.all = p
                    .todo()
                    .into_iter()
                    .map(|t| Row {
                        group_id: t.group_id,
                        group_name: Some(t.group_name),
                        topic: t.topic,
                    })
                    .collect();
            }
            Some(Place::Group(group_id)) => {
                self.title = p
                    .groups()
                    .into_iter()
                    .find(|g| &g.id == group_id)
                    .map(|g| g.name)
                    .unwrap_or_default();
                self.all = p
                    .topics(group_id)
                    .into_iter()
                    .map(|topic| Row {
                        group_id: group_id.clone(),
                        group_name: None,
                        topic,
                    })
                    .collect();
            }
            None => {
                self.title.clear();
                self.all.clear();
            }
        }
        self.apply_filter();
        self.refresh_typing();
    }

    /// The rows show topics of this group.
    fn shows_group(&self, group_id: &str) -> bool {
        match &self.place {
            Some(Place::Group(g)) => g == group_id,
            Some(Place::Todo) => self.all.iter().any(|r| r.group_id == group_id),
            None => false,
        }
    }

    fn refresh_typing(&mut self) {
        let p = &self.services.provider;
        self.typing = self
            .all
            .iter()
            .filter_map(|r| {
                let who = p.typing(&r.group_id, &r.topic.id);
                (!who.is_empty()).then(|| (r.topic.id.clone(), who))
            })
            .collect();
    }

    /// "{name} 正在输入…" for a topic, when someone types in it.
    fn typing_text(&self, topic_id: &str) -> Option<String> {
        match self.typing.get(topic_id)?.as_slice() {
            [] => None,
            [one] => {
                let name = self
                    .members
                    .iter()
                    .find(|m| &m.id == one)
                    .map(|m| m.name.clone())
                    .unwrap_or_default();
                let name = crate::copy::isolate(&name);
                Some(crate::copy::ltr(fill(r::TOPIC_TYPING, &[("name", &name)])))
            }
            many => Some(fill(
                r::TOPIC_TYPING_MANY,
                &[("n", &many.len().to_string())],
            )),
        }
    }

    fn apply_filter(&mut self) {
        let me = self.services.me();
        let filter = if matches!(self.place, Some(Place::Group(_))) {
            self.filter
        } else {
            TopicFilter::All
        };
        self.rows = self
            .all
            .iter()
            .filter(|r| filter.keeps(&r.topic, me.as_deref()))
            .cloned()
            .collect();
    }

    fn set_filter(&mut self, filter: TopicFilter, cx: &mut Context<Self>) {
        self.filter = filter;
        if let Some(Place::Group(g)) = &self.place {
            self.filters.insert(g.clone(), filter);
        }
        self.apply_filter();
        cx.notify();
    }

    pub fn row_count(&self) -> usize {
        self.rows.len()
    }

    /// The preview of a topic (§7.6): `作者：内容` of its newest message,
    /// Markdown stripped. Cached by the topic's `last_seq`.
    fn preview(&mut self, group_id: &str, topic: &Topic) -> String {
        if let Some((seq, text)) = self.previews.get(&topic.id)
            && *seq == topic.last_seq
        {
            return text.clone();
        }
        let window = self.services.provider.messages(group_id, &topic.id, 1);
        let text = window
            .messages
            .iter()
            .rev()
            .find(|m| !m.body.trim().is_empty())
            .map(|m| {
                let who = self
                    .members
                    .iter()
                    .find(|x| x.id == m.author_id)
                    .map(|x| topic_line(&x.name))
                    .unwrap_or_default();
                let body = plain_preview(&m.body);
                if who.is_empty() {
                    crate::copy::ltr(body)
                } else {
                    let who = crate::copy::isolate(&who);
                    crate::copy::ltr(format!("{who}{}{body}", nav::PREVIEW_SEP))
                }
            })
            .unwrap_or_default();
        self.previews
            .insert(topic.id.clone(), (topic.last_seq, text.clone()));
        text
    }

    /// The 40 pt tab band (§8.1): 全部 · 未读 n · 等我 n, gap 24, 16 pt left,
    /// the active tab underlined on the bottom hairline.
    fn tabs(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let k = theme::colors(cx);
        let me = self.services.me();
        let is_group = matches!(self.place, Some(Place::Group(_)));
        let filters: &[TopicFilter] = if is_group {
            &TopicFilter::ALL
        } else {
            &[TopicFilter::All]
        };
        div()
            .flex_none()
            .h(theme::HEADER_SECOND_ROW)
            .px(space::S16)
            .flex()
            .gap(space::S24)
            .border_b(theme::HAIRLINE)
            .border_color(k.sep)
            .children(filters.iter().map(|&f| {
                let n = if f == TopicFilter::All {
                    0
                } else {
                    self.all
                        .iter()
                        .filter(|r| f.keeps(&r.topic, me.as_deref()))
                        .count() as u32
                };
                let active = f == self.filter || !is_group;
                ui::tab(
                    ElementId::Name(SharedString::from(format!("topic-tab-{}", f.label()))),
                    f.label(),
                    n,
                    active,
                    cx,
                )
                .on_click(cx.listener(move |this, _, _, cx| this.set_filter(f, cx)))
            }))
    }

    /// One topic row (§7.6).
    fn topic_row(
        &self,
        ix: usize,
        row: &Row,
        selected: bool,
        now: DateTime<Local>,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let k = theme::colors(cx);
        let me = self.services.me();
        let topic = &row.topic;
        let unread = topic.unread > 0 || topic.mentions > 0;
        let lead_id = topic.owner_id.as_ref().unwrap_or(&topic.created_by);
        let lead = self.members.iter().find(|m| &m.id == lead_id);
        let waits_on_me = me
            .as_deref()
            .is_some_and(|me| topic.waiting_on.iter().any(|m| m == me))
            && matches!(
                topic.status,
                TopicStatus::WaitingAnswer | TopicStatus::WaitingReview
            );
        let chip = StatusChip::for_topic(topic, me.as_deref(), &self.members);
        // Exactly one trailing element, by priority (§7.6, §9.1).
        let trailing = if waits_on_me {
            chip.map(|c| c.into_any_element())
        } else if topic.mentions > 0 {
            Some(CountBadge::new(topic.mentions, BadgeKind::Mention).into_any_element())
        } else if let Some(c) = chip {
            Some(c.into_any_element())
        } else if topic.unread > 0 {
            Some(CountBadge::new(topic.unread, BadgeKind::Unread).into_any_element())
        } else {
            None
        };
        let title = topic_line(&topic.title);
        let (title, untitled) = if title.is_empty() {
            (nav::UNTITLED.to_string(), true)
        } else {
            (title, false)
        };
        let tip_title = SharedString::from(title.clone());
        let preview = match self.typing_text(&topic.id) {
            Some(t) => t,
            None => {
                let text = self
                    .previews
                    .get(&topic.id)
                    .map(|(_, t)| t.clone())
                    .unwrap_or_default();
                match (&row.group_name, text.is_empty()) {
                    (Some(g), true) => g.clone(),
                    _ => text,
                }
            }
        };
        // Title and preview are both cut to one line; the tooltip has both.
        let tip_detail = SharedString::from(preview.to_string());
        let (g, t) = (row.group_id.clone(), topic.id.clone());
        let menu_services = self.services.clone();
        let menu_topic = topic.clone();
        let menu_group = row.group_id.clone();
        div()
            .w_full()
            .h(size::TOPIC_ROW)
            .px(space::S8)
            .child(
                ui::list_row(
                    ElementId::NamedInteger(SharedString::from("topic-row"), ix as u64),
                    RowSurface::Content,
                    selected,
                    cx,
                )
                .when(selected && !self.emphasized, |el| el.bg(k.pressed))
                .relative()
                .size_full()
                .items_start()
                .pt(space::S12)
                .pb(space::S12)
                .pl(px(18.))
                .pr(space::S10)
                .gap(space::S12)
                .when(unread && !selected, |el| {
                    el.child(ui::unread_dot(cx).absolute().left(px(5.)).top(px(16.)))
                })
                .child(
                    ui::MemberAvatar::new(lead, size::AVATAR_MESSAGE)
                        .on(if selected { k.selected } else { k.content })
                        .selected(selected),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w(px(0.))
                        .mt(px(-2.))
                        .flex()
                        .flex_col()
                        .gap(space::S4)
                        .child(
                            div()
                                .h(px(20.))
                                .flex()
                                .items_center()
                                .gap(space::S8)
                                .child(
                                    div()
                                        .flex_1()
                                        .min_w(px(0.))
                                        .t_ui()
                                        .medium()
                                        .one_line()
                                        .when(unread || selected, |el| el.semibold())
                                        .text_color(if untitled { k.text_2 } else { k.text_1 })
                                        .child(title),
                                )
                                .child({
                                    let full =
                                        SharedString::from(full_time(topic.last_activity_at));
                                    div()
                                        .id("time")
                                        .flex_none()
                                        .t_meta()
                                        .tnum()
                                        .text_color(k.text_3)
                                        .when(!full.is_empty(), |el| {
                                            el.tooltip(move |window, cx| {
                                                Tooltip::new(full.clone()).build(window, cx)
                                            })
                                        })
                                        .child(list_time(topic.last_activity_at, now))
                                }),
                        )
                        .child(
                            div()
                                .h(px(20.))
                                .flex()
                                .items_center()
                                .gap(space::S8)
                                .child(
                                    div()
                                        .flex_1()
                                        .min_w(px(0.))
                                        .t_small()
                                        .text_color(k.text_2)
                                        .one_line()
                                        .child(preview),
                                )
                                .children(trailing),
                        ),
                )
                .tooltip(move |window, cx| {
                    ui::row_tip(tip_title.clone(), tip_detail.clone()).build(window, cx)
                })
                .on_click(cx.listener(move |this, _, _, cx| {
                    let (g, t) = (g.clone(), t.clone());
                    this.services
                        .nav
                        .update(cx, |nav, cx| nav.open_topic(g, t, cx));
                }))
                .context_menu(move |menu, window, cx| {
                    let topic = menu_services
                        .provider
                        .topic(&menu_group, &menu_topic.id)
                        .unwrap_or_else(|| menu_topic.clone());
                    topic_menu::build(menu, &menu_services, &menu_group, &topic, window, cx)
                }),
            )
            .into_any_element()
    }
}

/// A one-line preview of a message body: Markdown marks and line breaks
/// removed, and a table row `| a | b |` read as `a · b` (its `| --- |`
/// separator row dropped).
pub fn plain_preview(body: &str) -> String {
    let head: String = body.chars().take(400).collect();
    let lines: Vec<String> = head.lines().filter_map(table_row).collect();
    crate::views::search::plain::plain(&lines.join("\n"))
}

/// A body line with Markdown table syntax replaced; `None` for a separator.
fn table_row(line: &str) -> Option<String> {
    let t = line.trim();
    if !t.starts_with('|') {
        return Some(line.to_string());
    }
    if t.chars().all(|c| matches!(c, '|' | '-' | ':' | ' ')) {
        return None;
    }
    let cells: Vec<&str> = t
        .split('|')
        .map(str::trim)
        .filter(|c| !c.is_empty())
        .collect();
    Some(cells.join(" · "))
}

impl Render for TopicListView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self.activation.is_none() {
            self.activation = Some(cx.observe_window_activation(window, |_, _, cx| cx.notify()));
        }
        self.emphasized = ui::rows::selection_emphasized(window);
        let k = theme::colors(cx);
        let base = self.scroll.0.borrow().base_handle.clone();
        let edges = scroll_edges(&base);
        watch_scroll_edges(&base, edges, window, cx);
        let is_group = matches!(self.place, Some(Place::Group(_)));
        let empty_text = match &self.place {
            None => Some((nav::SELECT_GROUP, None)),
            Some(Place::Todo) if self.rows.is_empty() => {
                Some((nav::TODO_EMPTY, Some(nav::TODO_EMPTY_HINT)))
            }
            Some(Place::Group(_)) if self.all.is_empty() => {
                Some((nav::TOPICS_EMPTY, Some(nav::TOPICS_EMPTY_HINT)))
            }
            Some(Place::Group(_)) if self.rows.is_empty() => Some((nav::FILTER_EMPTY, None)),
            _ => None,
        };
        // Title row (§8.1): 52 pt, padding 0 10 0 16, the 群 name `title`
        // 16/24, then the 32 pt 新话题 icon button.
        let header = ui::toolbar_band("topic-list-toolbar")
            .pl(space::S16)
            .pr(space::S10)
            .gap(space::S8)
            .child(
                ui::truncating("topic-list-title", self.title.clone())
                    .flex_1()
                    .t_title(),
            )
            .when(is_group, |el| {
                el.child(
                    ui::icon_button("new-topic", Lucide::SquarePen)
                        .named(nav::NEW_TOPIC_TIP)
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.services.nav.update(cx, |nav, cx| nav.new_topic(cx))
                        })),
                )
            });

        let selected = self.services.nav.read(cx).topic.clone();
        let now = Local::now();
        let empty_icon = match &self.place {
            Some(Place::Todo) => Lucide::CircleCheck,
            Some(Place::Group(_)) if !self.all.is_empty() => Lucide::SearchX,
            Some(Place::Group(_)) => Lucide::MessageSquarePlus,
            None => Lucide::UsersRound,
        };
        let body = match empty_text {
            Some((text, hint)) => div()
                .flex_1()
                .child(
                    ui::EmptyState::new(empty_icon, text)
                        .when_some(hint, |el, hint| el.description(hint)),
                )
                .into_any_element(),
            None => {
                // Fill the previews of the rows before they draw.
                let rows = self.rows.clone();
                for r in &rows {
                    self.preview(&r.group_id, &r.topic);
                }
                // Rows cut by an edge fade from `content` (§7.7 scroll
                // edges), as in the sidebar.
                div()
                    .flex_1()
                    .min_h(px(0.))
                    .pt(space::S8)
                    .flex()
                    .flex_col()
                    .child(
                        div()
                            .relative()
                            .flex_1()
                            .min_h(px(0.))
                            .flex()
                            .flex_col()
                            .child(
                                uniform_list(
                                    "topic-rows",
                                    self.rows.len(),
                                    cx.processor(
                                        move |this, range: std::ops::Range<usize>, _, cx| {
                                            range
                                                .filter_map(|ix| {
                                                    this.rows.get(ix).cloned().map(|row| (ix, row))
                                                })
                                                .map(|(ix, row)| {
                                                    let sel = selected.as_ref()
                                                        == Some(&(
                                                            row.group_id.clone(),
                                                            row.topic.id.clone(),
                                                        ));
                                                    this.topic_row(ix, &row, sel, now, cx)
                                                })
                                                .collect::<Vec<_>>()
                                        },
                                    ),
                                )
                                .track_scroll(&self.scroll)
                                .flex_1(),
                            )
                            .when(edges.0, |el| el.child(edge_fade(true, k.content)))
                            .when(edges.1, |el| el.child(edge_fade(false, k.content))),
                    )
                    .into_any_element()
            }
        };

        div()
            .size_full()
            .flex()
            .flex_col()
            .bg(k.content)
            .child(header)
            .when(self.place.is_some(), |el| el.child(self.tabs(cx)))
            .child(body)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::ChatProvider;
    use crate::fake::{FakeProvider, ME};
    use chrono::Duration as D;

    #[test]
    fn relative_time_in_chinese() {
        let now = Local.with_ymd_and_hms(2026, 6, 15, 12, 0, 0).unwrap();
        let at = |d: D| (now - d).timestamp_millis();
        assert_eq!(relative_time(at(D::seconds(20)), now), "刚刚");
        assert_eq!(relative_time(at(D::minutes(5)), now), "5 分钟前");
        assert_eq!(relative_time(at(D::hours(3)), now), "3 小时前");
        assert_eq!(relative_time(at(D::hours(13)), now), "昨天");
        assert_eq!(relative_time(at(D::days(3)), now), "3 天前");
        assert_eq!(relative_time(at(D::days(30)), now), "5月16日");
        assert_eq!(relative_time(at(D::days(400)), now), "2025年5月11日");
    }

    #[test]
    fn list_time_follows_design_dates() {
        // A Wednesday.
        let now = Local.with_ymd_and_hms(2026, 6, 17, 12, 0, 0).unwrap();
        let at = |d: D| (now - d).timestamp_millis();
        assert_eq!(list_time(at(D::minutes(5)), now), "11:55");
        assert_eq!(list_time(at(D::hours(13)), now), "昨天");
        assert_eq!(list_time(at(D::days(2)), now), "周一");
        assert_eq!(list_time(at(D::days(6)), now), "周四");
        assert_eq!(list_time(at(D::days(30)), now), "5/18");
        assert_eq!(list_time(at(D::days(400)), now), "2025/5/13");
        assert!(full_time(at(D::days(400))).starts_with("2025年5月13日 "));
        assert_eq!(full_time(0), "");
        assert_eq!(list_time(0, now), "");
    }

    #[test]
    fn previews_read_tables_as_text() {
        let body = "| 名称 | 状态 |\n| --- | :-: |\n| 登录 | 卡住了 |";
        assert_eq!(plain_preview(body), "名称 · 状态 登录 · 卡住了");
        assert_eq!(plain_preview("a | b"), "a | b");
    }

    #[test]
    fn filters_follow_unread_and_todo() {
        let p = FakeProvider::new();
        let topics: Vec<Topic> = p.groups().iter().flat_map(|g| p.topics(&g.id)).collect();
        let me = Some(ME);
        assert!(topics.iter().all(|t| TopicFilter::All.keeps(t, me)));
        let unread: Vec<_> = topics
            .iter()
            .filter(|t| TopicFilter::Unread.keeps(t, me))
            .collect();
        assert!(!unread.is_empty());
        assert!(unread.iter().all(|t| t.unread > 0 || t.mentions > 0));
        let todo: Vec<_> = topics
            .iter()
            .filter(|t| TopicFilter::Todo.keeps(t, me))
            .collect();
        assert!(!todo.is_empty());
        assert!(todo.iter().all(|t| t.mentions > 0
            || (t.waiting_on.iter().any(|m| m == ME) && t.status != TopicStatus::Done)));
        // Every 待办 entry of the sidebar is in the 待办 filter of its 群.
        for item in p.todo() {
            assert!(
                TopicFilter::Todo.keeps(&item.topic, me),
                "{}",
                item.topic.title
            );
        }
        // Signed out: nothing waits on me, mentions still count.
        assert!(
            topics
                .iter()
                .filter(|t| TopicFilter::Todo.keeps(t, None))
                .all(|t| t.mentions > 0)
        );
    }
}
