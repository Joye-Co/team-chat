//! The left rail of the v2 shell (RAIL2.html, RailOptions.dc.html): title
//! row with 新话题, 搜索或交代 ⌘K, destinations 首页 / 产出 / 成员, 话题 in the
//! user's rail mode, the team strip and me.
//!
//! Foundation stub: the structure, navigation and data are wired; the rail
//! track restyles it to the artboards and adds the 话题 filter menu and the
//! header menu that switches the mode.

pub mod scenes;

use gpui::{
    AnyElement, App, Context, InteractiveElement as _, IntoElement, ParentElement, Render,
    SharedString, StatefulInteractiveElement as _, Styled, Subscription, Window, div,
    prelude::FluentBuilder as _, px,
};

use crate::{
    app::{NewTopic, Page, Place, RailMode, Services},
    copy::rail as c,
    data::{GroupKind, MemberId, PlanState, Topic, TopicStatus},
    theme::{self, Type as _, space},
    ui::{self, Lucide, MemberAvatar, NamedButton as _, Ring, RowSurface},
    views::{nav::QuickSwitch, shell::panes::watch},
};

pub struct RailView {
    services: Services,
    _subscriptions: Vec<Subscription>,
}

impl RailView {
    pub fn new(services: Services, cx: &mut Context<Self>) -> Self {
        Self {
            _subscriptions: watch(&services, cx),
            services,
        }
    }
}

/// What the slot at the start of a 话题 row shows.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RowState {
    /// Amber dot: needs me.
    NeedsYou,
    /// The doing member's tile with a breathing ring.
    Working(MemberId),
    /// A check: done.
    Done,
    /// A hollow ring: waits on someone else.
    Waiting,
    Idle,
}

/// The state of a 话题 row and its mono meta (`3/5`, `09:12`, `等小王`).
pub fn row_state(services: &Services, topic: &Topic) -> (RowState, Option<String>) {
    let p = &services.provider;
    let me = services.me();
    if me
        .as_deref()
        .is_some_and(|me| topic.waiting_on.iter().any(|m| m == me))
    {
        return (RowState::NeedsYou, None);
    }
    let runs = p.runs(&topic.group_id, &topic.id);
    if let Some(run) = runs.iter().find(|r| r.status.is_active()) {
        let done = run
            .plan
            .iter()
            .filter(|i| i.state == PlanState::Done)
            .count();
        let meta = (!run.plan.is_empty()).then(|| format!("{}/{}", done + 1, run.plan.len()));
        return (RowState::Working(run.bot_id.clone()), meta);
    }
    match topic.status {
        TopicStatus::Done => (
            RowState::Done,
            Some(crate::views::shell::clock(topic.last_activity_at)),
        ),
        TopicStatus::WaitingAnswer | TopicStatus::WaitingReview => {
            let who = topic
                .waiting_on
                .first()
                .and_then(|m| p.member(m))
                .map(|m| crate::copy::fill(c::RAIL_WAITING_ON, &[("name", &m.name)]));
            (RowState::Waiting, who)
        }
        _ => (RowState::Idle, None),
    }
}

impl RailView {
    fn destination(
        &self,
        id: &'static str,
        icon: Lucide,
        label: &'static str,
        selected: bool,
        count: Option<usize>,
        go: fn(&mut crate::app::Navigation, &mut Context<crate::app::Navigation>),
        cx: &App,
    ) -> AnyElement {
        let k = theme::colors(cx);
        let nav = self.services.nav.clone();
        ui::list_row(id, RowSurface::Rail, selected, cx)
            .h(theme::RAIL_ROW)
            .px(space::S10)
            .gap(space::S10)
            .child(ui::icon(
                icon,
                theme::icon::ROW,
                if selected { k.ink } else { k.ink_2 },
            ))
            .child(
                div()
                    .flex_1()
                    .t_small()
                    .text_color(if selected { k.ink } else { k.ink_2 })
                    .when(selected, |el| el.medium())
                    .child(label),
            )
            .children(
                count
                    .filter(|n| *n > 0)
                    .map(|n| div().t_mono().text_color(k.you_ink).child(n.to_string())),
            )
            .on_click(move |_, _, cx| nav.update(cx, |nav, cx| go(nav, cx)))
            .into_any_element()
    }

    fn slot(&self, state: &RowState, cx: &App) -> AnyElement {
        let k = theme::colors(cx);
        let center = |el: AnyElement| {
            div()
                .flex_none()
                .size(theme::RAIL_SLOT)
                .flex()
                .items_center()
                .justify_center()
                .child(el)
                .into_any_element()
        };
        match state {
            RowState::NeedsYou => center(
                div()
                    .size(px(7.))
                    .rounded_full()
                    .bg(k.you)
                    .into_any_element(),
            ),
            RowState::Working(m) => {
                let member = self.services.provider.member(m);
                center(
                    MemberAvatar::new(member.as_ref(), theme::RAIL_SLOT)
                        .ring(Some(Ring::Live))
                        .into_any_element(),
                )
            }
            RowState::Done => {
                center(ui::icon(Lucide::Check, theme::icon::TAG, k.ok).into_any_element())
            }
            RowState::Waiting => center(
                div()
                    .size(px(7.))
                    .rounded_full()
                    .border(px(1.5))
                    .border_color(k.ink_3)
                    .into_any_element(),
            ),
            RowState::Idle => center(div().into_any_element()),
        }
    }

    fn topic_row(&self, topic: &Topic, selected: bool, cx: &App) -> AnyElement {
        let k = theme::colors(cx);
        let (state, meta) = row_state(&self.services, topic);
        let nav = self.services.nav.clone();
        let (g, t) = (topic.group_id.clone(), topic.id.clone());
        let title = crate::views::sidebar::topic_display_title(&topic.title).0;
        ui::list_row(
            SharedString::from(format!("rail-topic-{}", topic.id)),
            RowSurface::Rail,
            selected,
            cx,
        )
        .h(theme::RAIL_ROW)
        .px(space::S10)
        .gap(space::S8)
        .child(self.slot(&state, cx))
        .child(
            div()
                .flex_1()
                .min_w_0()
                .one_line()
                .t_small()
                .text_color(k.ink)
                .when(selected, |el| el.medium())
                .child(title),
        )
        .children(meta.map(|m| div().flex_none().t_mono().text_color(k.ink_3).child(m)))
        .on_click(move |_, _, cx| {
            let (g, t) = (g.clone(), t.clone());
            nav.update(cx, |nav, cx| nav.open_topic(g, t, cx))
        })
        .into_any_element()
    }

    fn header(
        &self,
        id: SharedString,
        label: String,
        selected: bool,
        group: Option<String>,
        cx: &App,
    ) -> AnyElement {
        let k = theme::colors(cx);
        let nav = self.services.nav.clone();
        ui::list_row(id, RowSurface::Rail, selected, cx)
            .h(theme::RAIL_GROUP_ROW)
            .pl(space::S8)
            .pr(space::S10)
            .gap(space::S6)
            .t_meta_medium()
            .text_color(if selected { k.ink } else { k.ink_3 })
            .child(ui::icon(Lucide::ChevronDown, theme::icon::TAG, k.ink_3))
            .child(label)
            .when_some(group, |el, g| {
                el.on_click(move |_, _, cx| {
                    let g = g.clone();
                    nav.update(cx, |nav, cx| nav.open_group(g, cx))
                })
            })
            .into_any_element()
    }

    /// A: 话题 grouped by 群, then 私聊.
    fn work(
        &self,
        open: Option<&(String, String)>,
        group_page: Option<&str>,
        cx: &App,
    ) -> Vec<AnyElement> {
        let p = &self.services.provider;
        let mut out = Vec::new();
        let groups = p.groups();
        for g in groups.iter().filter(|g| g.kind == GroupKind::Group) {
            out.push(self.header(
                format!("rail-group-{}", g.id).into(),
                g.name.clone(),
                group_page == Some(g.id.as_str()),
                Some(g.id.clone()),
                cx,
            ));
            for t in p
                .topics(&g.id)
                .iter()
                .filter(|t| t.status != TopicStatus::Open || t.message_count > 0)
            {
                let selected = open.is_some_and(|(og, ot)| og == &g.id && ot == &t.id);
                out.push(self.topic_row(t, selected, cx));
            }
        }
        let dms: Vec<_> = groups.iter().filter(|g| g.kind == GroupKind::Dm).collect();
        if !dms.is_empty() {
            out.push(self.header("rail-dms".into(), c::RAIL_DMS.into(), false, None, cx));
            let k = theme::colors(cx);
            for g in dms {
                let peer = g.dm_peer.as_deref().and_then(|m| p.member(m));
                let nav = self.services.nav.clone();
                let id = g.id.clone();
                out.push(
                    ui::list_row(
                        SharedString::from(format!("rail-dm-{}", g.id)),
                        RowSurface::Rail,
                        group_page == Some(g.id.as_str()),
                        cx,
                    )
                    .h(theme::RAIL_ROW)
                    .px(space::S10)
                    .gap(space::S8)
                    .child(MemberAvatar::new(peer.as_ref(), theme::RAIL_SLOT))
                    .child(div().t_small().text_color(k.ink).child(g.name.clone()))
                    .on_click(move |_, _, cx| {
                        let id = id.clone();
                        nav.update(cx, |nav, cx| nav.open_group(id, cx))
                    })
                    .into_any_element(),
                );
            }
        }
        out
    }

    /// B: 需要你 / 在做 / 做完了 · 今天.
    fn attention(&self, open: Option<&(String, String)>, cx: &App) -> Vec<AnyElement> {
        let p = &self.services.provider;
        let all: Vec<Topic> = p.groups().iter().flat_map(|g| p.topics(&g.id)).collect();
        let mut buckets: [(String, Vec<&Topic>); 3] = [
            (c::RAIL_NEEDS_YOU.into(), Vec::new()),
            (c::RAIL_WORKING.into(), Vec::new()),
            (c::RAIL_DONE_TODAY.into(), Vec::new()),
        ];
        for t in &all {
            match row_state(&self.services, t).0 {
                RowState::NeedsYou => buckets[0].1.push(t),
                RowState::Working(_) => buckets[1].1.push(t),
                RowState::Done => buckets[2].1.push(t),
                _ => {}
            }
        }
        let mut out = Vec::new();
        for (i, (label, topics)) in buckets.iter().enumerate() {
            out.push(self.header(
                format!("rail-bucket-{i}").into(),
                format!("{label} {}", topics.len()),
                false,
                None,
                cx,
            ));
            for t in topics {
                let selected = open.is_some_and(|(og, ot)| og == &t.group_id && ot == &t.id);
                out.push(self.topic_row(t, selected, cx));
            }
        }
        out
    }

    /// C: members, each with the 话题 it works on.
    fn by_member(&self, open: Option<&(String, String)>, cx: &App) -> Vec<AnyElement> {
        let p = &self.services.provider;
        let k = theme::colors(cx);
        let runs = p.active_runs();
        let mut out = Vec::new();
        for m in p.members().iter().filter(|m| !m.deactivated) {
            let mine: Vec<_> = runs.iter().filter(|r| r.bot_id == m.id).collect();
            let ring = (!mine.is_empty()).then_some(Ring::Live);
            let nav = self.services.nav.clone();
            let id = m.id.clone();
            out.push(
                ui::list_row(
                    SharedString::from(format!("rail-member-{}", m.id)),
                    RowSurface::Rail,
                    false,
                    cx,
                )
                .h(theme::RAIL_ROW)
                .px(space::S10)
                .gap(space::S8)
                .child(MemberAvatar::new(Some(m), theme::RAIL_SLOT).ring(ring))
                .child(
                    div()
                        .flex_1()
                        .t_small()
                        .text_color(k.ink)
                        .child(m.name.clone()),
                )
                .on_click(move |_, _, cx| {
                    let id = id.clone();
                    nav.update(cx, |nav, cx| nav.open_members(Some(id), cx))
                })
                .into_any_element(),
            );
            for r in mine {
                if let Some(t) = p.topic(&r.group_id, &r.topic_id) {
                    let selected = open.is_some_and(|(og, ot)| og == &t.group_id && ot == &t.id);
                    out.push(
                        div()
                            .pl(space::S16)
                            .child(self.topic_row(&t, selected, cx))
                            .into_any_element(),
                    );
                }
            }
        }
        out
    }
}

impl Render for RailView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let _ = window;
        let k = theme::colors(cx);
        let nav = self.services.nav.read(cx);
        let page = nav.page();
        let mode = nav.rail_mode;
        let open = nav.topic.clone();
        let group_page = match (&nav.place, page) {
            (Some(Place::Group(g)), Page::Group) => Some(g.clone()),
            _ => None,
        };
        let asks = self.services.provider.asks_for_me().len();
        let topics = match mode {
            RailMode::Work => self.work(open.as_ref(), group_page.as_deref(), cx),
            RailMode::Attention => self.attention(open.as_ref(), cx),
            RailMode::Member => self.by_member(open.as_ref(), cx),
        };
        let me = self
            .services
            .me()
            .and_then(|m| self.services.provider.member(&m));
        let working: Vec<_> = {
            let runs = self.services.provider.active_runs();
            let mut ids: Vec<String> = runs.iter().map(|r| r.bot_id.clone()).collect();
            ids.dedup();
            ids
        };
        let bots = self
            .services
            .provider
            .members()
            .into_iter()
            .filter(|m| m.is_bot())
            .count();
        div()
            .id("rail")
            .size_full()
            .flex()
            .flex_col()
            .bg(k.rail)
            .px(space::S10)
            .pb(space::S10)
            // Title row: the traffic lights, then 新话题.
            .child(
                div()
                    .flex_none()
                    .h(theme::TITLE_BAR)
                    .flex()
                    .items_center()
                    .justify_end()
                    .child(
                        ui::icon_button_sm("rail-new-topic", Lucide::SquarePen)
                            .named(c::RAIL_NEW_TOPIC)
                            .on_click(|_, window, cx| {
                                window.dispatch_action(Box::new(NewTopic), cx)
                            }),
                    ),
            )
            .child(
                div()
                    .id("rail-search")
                    .flex_none()
                    .h(px(34.))
                    .pl(space::S10)
                    .pr(space::S8)
                    .rounded(px(9.))
                    .bg(k.bg)
                    .border_1()
                    .border_color(k.line)
                    .shadow(k.sec_hi())
                    .flex()
                    .items_center()
                    .gap(space::S8)
                    .cursor_pointer()
                    .child(ui::icon(Lucide::Search, theme::icon::CHIP, k.ink_3))
                    .child(
                        div()
                            .flex_1()
                            .t_small()
                            .text_color(k.ink_3)
                            .child(c::RAIL_SEARCH),
                    )
                    .child(ui::Kbd::new("⌘K"))
                    .on_click(|_, window, cx| window.dispatch_action(Box::new(QuickSwitch), cx)),
            )
            .child(
                div()
                    .flex_none()
                    .mt(space::S10)
                    .flex()
                    .flex_col()
                    .gap(px(1.))
                    .child(self.destination(
                        "rail-desk",
                        Lucide::House,
                        c::RAIL_DESK,
                        page == Page::Desk,
                        Some(asks),
                        |n, cx| n.open_desk(cx),
                        cx,
                    ))
                    .child(self.destination(
                        "rail-artifacts",
                        Lucide::Layers,
                        c::RAIL_ARTIFACTS,
                        matches!(page, Page::Artifacts | Page::Artifact),
                        None,
                        |n, cx| n.open_artifacts(cx),
                        cx,
                    ))
                    .child(self.destination(
                        "rail-members",
                        Lucide::Users,
                        c::RAIL_MEMBERS,
                        page == Page::Members,
                        None,
                        |n, cx| n.open_members(None, cx),
                        cx,
                    )),
            )
            .child(
                div()
                    .flex_none()
                    .mt(space::S16)
                    .h(px(28.))
                    .pl(space::S10)
                    .pr(space::S6)
                    .flex()
                    .items_center()
                    .t_meta_medium()
                    .text_color(k.ink_3)
                    .child(div().flex_1().child(c::RAIL_TOPICS))
                    .child(c::RAIL_FILTER_MINE),
            )
            .child(
                div()
                    .id("rail-topics")
                    .flex_1()
                    .min_h(px(0.))
                    .overflow_y_scroll()
                    .flex()
                    .flex_col()
                    .gap(px(1.))
                    .children(topics),
            )
            // App banners (connection, update) sit above the footer.
            .children(crate::views::banners::slot(cx))
            // The team strip: members at work, then +n.
            .child(
                div()
                    .flex_none()
                    .h(px(36.))
                    .px(space::S8)
                    .flex()
                    .items_center()
                    .gap(space::S8)
                    .children(working.iter().take(5).map(|m| {
                        let member = self.services.provider.member(m);
                        MemberAvatar::new(member.as_ref(), px(20.)).ring(Some(Ring::Live))
                    }))
                    .when(bots > working.len().min(5), |el| {
                        el.child(div().t_mono().text_color(k.ink_3).child(crate::copy::fill(
                            c::RAIL_MORE_MEMBERS,
                            &[("n", &(bots - working.len().min(5)).to_string())],
                        )))
                    }),
            )
            .child(
                div()
                    .flex_none()
                    .h(px(40.))
                    .px(space::S8)
                    .flex()
                    .items_center()
                    .gap(space::S10)
                    .child(MemberAvatar::new(me.as_ref(), px(22.)))
                    .child(
                        div()
                            .flex_1()
                            .t_small()
                            .medium()
                            .text_color(k.ink)
                            .children(me.map(|m| m.name)),
                    )
                    .child(
                        ui::icon_button_sm("rail-settings", Lucide::Settings)
                            .named(c::RAIL_SETTINGS)
                            .on_click(|_, window, cx| {
                                window.dispatch_action(Box::new(crate::app::OpenSettings), cx)
                            }),
                    ),
            )
    }
}
