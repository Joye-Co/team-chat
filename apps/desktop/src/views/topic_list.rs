//! The middle column: the topics of the selected 群 (or the 待办 list), with
//! title, status chip, last activity (relative, in Chinese), unread count and
//! my mention marker; a status filter; and the 新话题 button.
//! Rows have one height, so the list is a `uniform_list` and scales to many topics.

use std::{collections::HashMap, time::Duration};

use chrono::{DateTime, Datelike, Local, TimeZone};
use gpui::{
    Context, ElementId, InteractiveElement, IntoElement, ParentElement, Render, SharedString,
    StatefulInteractiveElement, Styled, Subscription, Window, div, prelude::FluentBuilder as _, px,
    uniform_list,
};
use gpui_component::{
    Sizable as _,
    menu::ContextMenuExt as _,
    tab::{Tab, TabBar},
};

use crate::{
    app::{Changes, Place, Services},
    copy::{common, fill, nav, realtime as r},
    data::{Change, GroupId, Member, MemberId, Millis, Topic, TopicId, TopicStatus},
    theme::{self, Type as _, space},
    ui::{self, BadgeKind, CountBadge, Lucide, StatusChip},
    views::{sidebar::topic_line, topic_menu},
};

/// Height of one topic row: 9 + 18 + 4 + 18 + 10 (§5.3) plus the 2 pt inset.
const TOPIC_ROW_HEIGHT: gpui::Pixels = px(61.);

/// The status filter above the topics of a group.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum TopicFilter {
    #[default]
    All,
    /// `waitingOn` includes me and the topic is not done.
    WaitingOnMe,
    InProgress,
    Done,
}

impl TopicFilter {
    pub const ALL: [TopicFilter; 4] = [
        TopicFilter::All,
        TopicFilter::WaitingOnMe,
        TopicFilter::InProgress,
        TopicFilter::Done,
    ];

    fn label(self) -> &'static str {
        match self {
            TopicFilter::All => nav::FILTER_ALL,
            TopicFilter::WaitingOnMe => nav::FILTER_WAITING_ME,
            TopicFilter::InProgress => nav::FILTER_IN_PROGRESS,
            TopicFilter::Done => nav::FILTER_DONE,
        }
    }

    pub fn keeps(self, topic: &Topic, me: Option<&str>) -> bool {
        match self {
            TopicFilter::All => true,
            TopicFilter::WaitingOnMe => {
                topic.status != TopicStatus::Done
                    && me.is_some_and(|me| topic.waiting_on.iter().any(|m| m == me))
            }
            TopicFilter::InProgress => topic.status == TopicStatus::InProgress,
            TopicFilter::Done => topic.status == TopicStatus::Done,
        }
    }
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
    _subscriptions: Vec<Subscription>,
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
            _subscriptions: subscriptions,
        };
        this.refresh(cx);
        this
    }

    fn refresh(&mut self, cx: &mut Context<Self>) {
        let p = self.services.provider.clone();
        let place = self.services.nav.read(cx).place.clone();
        if place != self.place {
            self.filter = TopicFilter::All;
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
                Some(fill(r::TOPIC_TYPING, &[("name", &name)]))
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
        self.apply_filter();
        cx.notify();
    }

    pub fn row_count(&self) -> usize {
        self.rows.len()
    }

    fn filter_bar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let colors = theme::colors(cx);
        let me = self.services.me();
        let selected = TopicFilter::ALL
            .iter()
            .position(|f| *f == self.filter)
            .unwrap_or(0);
        let view = cx.entity().downgrade();
        div().flex_none().px(space::S12).pb(space::S8).child(
            TabBar::new("topic-filter")
                .segmented()
                .small()
                .w_full()
                .selected_index(selected)
                .children(TopicFilter::ALL.into_iter().map(|f| {
                    let n = self
                        .all
                        .iter()
                        .filter(|r| f.keeps(&r.topic, me.as_deref()))
                        .count() as u32;
                    Tab::new().flex_1().child(
                        div()
                            .flex()
                            .items_center()
                            .gap(space::S4)
                            .child(f.label())
                            .child(
                                div()
                                    .t_caption()
                                    .font_weight(gpui::FontWeight::NORMAL)
                                    .tnum()
                                    .text_color(colors.text_3)
                                    .child(CountBadge::text(n, BadgeKind::Unread)),
                            ),
                    )
                }))
                .on_click(move |ix, _, cx| {
                    if let Some(f) = TopicFilter::ALL.get(*ix).copied() {
                        let _ = view.update(cx, |this, cx| this.set_filter(f, cx));
                    }
                }),
        )
    }
}

impl Render for TopicListView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let colors = theme::colors(cx);
        let is_group = matches!(self.place, Some(Place::Group(_)));
        let empty_text = match &self.place {
            None => Some(nav::SELECT_GROUP),
            Some(Place::Todo) if self.rows.is_empty() => Some(nav::TODO_EMPTY),
            Some(Place::Group(_)) if self.all.is_empty() => Some(nav::TOPICS_EMPTY),
            Some(Place::Group(_)) if self.rows.is_empty() => Some(nav::FILTER_EMPTY),
            _ => None,
        };
        let sub = if is_group {
            let members = match &self.place {
                Some(Place::Group(g)) => self
                    .services
                    .provider
                    .groups()
                    .into_iter()
                    .find(|x| &x.id == g)
                    .map(|x| x.member_ids.len())
                    .unwrap_or(0),
                _ => 0,
            };
            Some(format!(
                "{} · {}",
                fill(nav::MEMBER_COUNT, &[("n", &members.to_string())]),
                fill(nav::TOPIC_COUNT, &[("n", &self.all.len().to_string())])
            ))
        } else {
            None
        };
        let header = ui::toolbar_band("topic-list-toolbar")
            .px(space::S16)
            .gap(space::S8)
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.))
                    .flex()
                    .flex_col()
                    .child(div().t_title().one_line().child(self.title.clone()))
                    .when_some(sub, |el, sub| {
                        el.child(
                            div()
                                .type_role(theme::type_scale::CAPTION)
                                .tnum()
                                .text_color(colors.text_2)
                                .one_line()
                                .child(sub),
                        )
                    }),
            )
            .when(is_group, |el| {
                el.child(
                    ui::icon_button("new-topic", Lucide::SquarePen)
                        .tooltip(nav::NEW_TOPIC)
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.services.nav.update(cx, |nav, cx| nav.new_topic(cx))
                        })),
                )
            });

        let selected = self.services.nav.read(cx).topic.clone();
        let me = self.services.me();
        let now = Local::now();
        let empty_icon = match &self.place {
            Some(Place::Todo) => Lucide::CircleCheck,
            Some(Place::Group(_)) if !self.all.is_empty() => Lucide::SearchX,
            Some(Place::Group(_)) => Lucide::MessageSquarePlus,
            None => Lucide::UsersRound,
        };
        let body = match empty_text {
            Some(text) => div()
                .flex_1()
                .child(ui::EmptyState::new(empty_icon, text))
                .into_any_element(),
            None => uniform_list(
                "topic-rows",
                self.rows.len(),
                cx.processor(move |this, range: std::ops::Range<usize>, _window, cx| {
                    let colors = theme::colors(cx);
                    let is_sel = |ix: usize| {
                        this.rows.get(ix).is_some_and(|row| {
                            selected.as_ref() == Some(&(row.group_id.clone(), row.topic.id.clone()))
                        })
                    };
                    range
                        .filter_map(|ix| this.rows.get(ix).cloned().map(|row| (ix, row)))
                        .map(|(ix, row)| {
                            let is_selected = is_sel(ix);
                            // The hairline under a row hides next to the selected row.
                            let separator =
                                !is_selected && !is_sel(ix + 1) && ix + 1 < this.rows.len();
                            let chip =
                                StatusChip::for_topic(&row.topic, me.as_deref(), &this.members);
                            let (g, t) = (row.group_id.clone(), row.topic.id.clone());
                            let unread = row.topic.unread > 0;
                            let badge = if row.topic.mentions > 0 {
                                Some(CountBadge::new(row.topic.mentions, BadgeKind::Mention))
                            } else if unread {
                                Some(CountBadge::new(row.topic.unread, BadgeKind::Unread))
                            } else {
                                None
                            };
                            let title = topic_line(&row.topic.title);
                            let (title, untitled) = if title.is_empty() {
                                (nav::UNTITLED.to_string(), true)
                            } else {
                                (title, false)
                            };
                            let typing = this.typing_text(&row.topic.id);
                            let menu_services = this.services.clone();
                            let menu_topic = row.topic.clone();
                            let menu_group = row.group_id.clone();
                            div()
                                .id(ElementId::NamedInteger(
                                    SharedString::from("topic-row"),
                                    ix as u64,
                                ))
                                .w_full()
                                .h(TOPIC_ROW_HEIGHT)
                                .px(space::S8)
                                .pt(px(2.))
                                .child(
                                    div()
                                        .relative()
                                        .size_full()
                                        .rounded(theme::radius::ROW)
                                        .pt(px(9.))
                                        .pb(space::S10)
                                        .pl(space::S20)
                                        .pr(space::S10)
                                        .flex()
                                        .flex_col()
                                        .gap(space::S4)
                                        .when(is_selected, |el| el.bg(colors.fill_selected))
                                        .when(unread, |el| {
                                            el.child(
                                                div()
                                                    .absolute()
                                                    .left(space::S6)
                                                    .top(px(14.))
                                                    .size(space::S8)
                                                    .rounded_full()
                                                    .bg(colors.accent),
                                            )
                                        })
                                        .child(
                                            div()
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
                                                        .when(unread, |el| el.semibold())
                                                        .when(untitled, |el| {
                                                            el.text_color(colors.text_2)
                                                        })
                                                        .child(title),
                                                )
                                                .child(
                                                    div()
                                                        .flex_none()
                                                        .t_callout()
                                                        .tnum()
                                                        .text_color(colors.text_2)
                                                        .child(relative_time(
                                                            row.topic.last_activity_at,
                                                            now,
                                                        )),
                                                ),
                                        )
                                        .child(
                                            div()
                                                .h(px(18.))
                                                .flex()
                                                .items_center()
                                                .gap(space::S6)
                                                .t_callout()
                                                .text_color(colors.text_2)
                                                .children(chip)
                                                .child(div().flex_1().min_w(px(0.)).one_line().map(
                                                    |el| {
                                                        match typing {
                                                            Some(text) => el
                                                                .text_color(colors.accent_text)
                                                                .child(text),
                                                            None => {
                                                                el.children(row.group_name.clone())
                                                            }
                                                        }
                                                    },
                                                ))
                                                .children(badge),
                                        )
                                        .when(separator, |el| {
                                            el.child(
                                                div()
                                                    .absolute()
                                                    .bottom(px(0.))
                                                    .left(space::S20)
                                                    .right(space::S10)
                                                    .h(theme::HAIRLINE)
                                                    .bg(colors.sep),
                                            )
                                        }),
                                )
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
                                    topic_menu::build(
                                        menu,
                                        &menu_services,
                                        &menu_group,
                                        &topic,
                                        window,
                                        cx,
                                    )
                                })
                        })
                        .collect::<Vec<_>>()
                }),
            )
            .flex_1()
            .into_any_element(),
        };

        div()
            .size_full()
            .flex()
            .flex_col()
            .bg(colors.content)
            .child(header)
            .when(is_group && !self.all.is_empty(), |el| {
                el.child(self.filter_bar(cx))
            })
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
    fn filters_follow_status_and_waiting_on() {
        let p = FakeProvider::new();
        let topics: Vec<Topic> = p.groups().iter().flat_map(|g| p.topics(&g.id)).collect();
        let me = Some(ME);
        assert!(topics.iter().all(|t| TopicFilter::All.keeps(t, me)));
        let waiting: Vec<_> = topics
            .iter()
            .filter(|t| TopicFilter::WaitingOnMe.keeps(t, me))
            .collect();
        assert!(!waiting.is_empty());
        assert!(
            waiting
                .iter()
                .all(|t| t.waiting_on.iter().any(|m| m == ME) && t.status != TopicStatus::Done)
        );
        assert!(
            topics
                .iter()
                .filter(|t| TopicFilter::Done.keeps(t, me))
                .all(|t| t.status == TopicStatus::Done)
        );
        assert!(
            topics
                .iter()
                .filter(|t| TopicFilter::InProgress.keeps(t, me))
                .all(|t| t.status == TopicStatus::InProgress)
        );
        // Signed out: nothing waits on me.
        assert!(
            !topics
                .iter()
                .any(|t| TopicFilter::WaitingOnMe.keeps(t, None))
        );
    }
}
