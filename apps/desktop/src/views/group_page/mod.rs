//! 群 page (NATIVE.md §1, §9.4 lists, §9.3 inspector). 1.1.1: a click on a
//! 群 opens its 群聊 (the 话题 page of the 群's chat); this page is the
//! secondary view of its 话题, opened from the 群聊's `话题`, with a
//! `回到群里聊` item back. The toolbar row says
//! the 群's name and purpose; the content is a Mail-like list of 进行中 and
//! 最近完成 话题 (a 话题 starts from 新话题 in the toolbar or the sidebar, or
//! ⌘N, which open the 新话题 sheet preset to this 群; there is no inline
//! post box). The page shows only 话题: the 群's info (成员, 通知, 群里的决定 …)
//! is 群资料, the 群聊's 详情 pane ([`info`]).
//!
//! The page draws its own toolbar row ([`PageView::draws_own_header`]).

pub mod info;
pub mod kit;
pub mod rows;
pub mod scenes;
mod sheets;

use gpui::{
    AnyElement, App, Context, Entity, InteractiveElement as _, IntoElement, ParentElement, Render,
    SharedString, StatefulInteractiveElement as _, Styled, Subscription, Window, div,
    prelude::FluentBuilder as _, px,
};
use gpui_component::menu::{ContextMenuExt as _, DropdownMenu as _, PopupMenuItem};

use crate::ui::native::tokens::space as tsp;
use crate::{
    app::{Place, RightPane, Services},
    copy::{fill, group_page as c, nav as nav_copy},
    data::{ArtifactKind, BotRunSummary, PlanState, Topic, TopicStatus},
    ui::{
        native::{
            PushSize, PushStyle, ToolbarGroup, ToolbarItemKind, ToolbarRow, emphasized,
            native_colors,
            page::{self, PageLoad, Status},
            push_button, toolbar_item,
        },
        sf_symbols::{Sf, Sym},
    },
    views::{
        composer::ComposerView,
        shell::{PageView, TitleBarSpec, clock, member_name, panes::watch},
    },
};

use rows::line;

pub struct GroupPage {
    services: Services,
    /// 进行中 shows every 话题 (`看全部`), not the first [`ACTIVE_ROWS`].
    show_all: bool,
    _subscriptions: Vec<Subscription>,
}

impl GroupPage {
    /// `_composer`: the shared composer, which this page no longer shows
    /// (topics start in the 新话题 sheet); the shell still passes it.
    pub fn new(
        services: Services,
        _composer: Entity<ComposerView>,
        cx: &mut Context<Self>,
    ) -> Self {
        Self {
            _subscriptions: watch(&services, cx),
            services,
            show_all: false,
        }
    }

    fn group_id(&self, cx: &App) -> Option<String> {
        match &self.services.nav.read(cx).place {
            Some(Place::Group(g)) => Some(g.clone()),
            _ => None,
        }
    }
}

pub use sheets::{open_purpose_sheet, open_record_sheet};

impl PageView for GroupPage {
    fn title_bar(&self, cx: &App) -> TitleBarSpec {
        let Some(g) = self.group_id(cx) else {
            return TitleBarSpec::default();
        };
        let p = &self.services.provider;
        let group = p.groups().into_iter().find(|x| x.id == g);
        TitleBarSpec {
            subtitle: p.group_purpose(&g),
            ..TitleBarSpec::page(group.map(|x| x.name).unwrap_or_default())
        }
    }

    fn draws_own_header(&self, _cx: &App) -> bool {
        true
    }
}

/// The section a 话题 sits in on the 群 page, in order (owner decision
/// v16: 等你 and 在做 are separate sections; a waiting row is never under
/// 在做). It comes from the one state mapping (`topic_state`, §2.12).
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Standing {
    /// 等你: an ask or a review waits on me.
    OnMe,
    /// 在做: a member's run works (or stops, is paused, or merges).
    Running,
    /// 卡住了: the run failed or the 话题 is blocked.
    Stuck,
    /// 等别人: it waits on other people.
    OnOthers,
    /// 排队: nobody works on it yet.
    Other,
}

impl Standing {
    const ALL: [Standing; 5] = [
        Standing::OnMe,
        Standing::Running,
        Standing::Stuck,
        Standing::OnOthers,
        Standing::Other,
    ];

    fn title(self) -> &'static str {
        match self {
            Standing::OnMe => c::GROUP_PAGE_SECTION_YOU,
            Standing::Running => c::GROUP_PAGE_ACTIVE,
            Standing::Stuck => c::GROUP_PAGE_SECTION_STUCK,
            Standing::OnOthers => c::GROUP_PAGE_SECTION_OTHERS,
            Standing::Other => c::GROUP_PAGE_SECTION_QUEUED,
        }
    }

    fn slug(self) -> &'static str {
        match self {
            Standing::OnMe => "you",
            Standing::Running => "working",
            Standing::Stuck => "stuck",
            Standing::OnOthers => "others",
            Standing::Other => "queued",
        }
    }
}

fn standing(t: &Topic, state: &crate::views::topic_page::StateLine) -> Standing {
    use crate::ui::native::BadgeState as B;
    match state.badge {
        B::NeedsYou => Standing::OnMe,
        B::Working | B::Stopping | B::Paused | B::Merging => Standing::Running,
        B::Failed | B::Abnormal => Standing::Stuck,
        _ if !t.waiting_on.is_empty() => Standing::OnOthers,
        _ => Standing::Other,
    }
}

pub fn kind_label(kind: ArtifactKind) -> &'static str {
    match kind {
        ArtifactKind::Page => c::GROUP_PAGE_KIND_PAGE,
        ArtifactKind::File => c::GROUP_PAGE_KIND_FILE,
        ArtifactKind::Image => c::GROUP_PAGE_KIND_IMAGE,
        ArtifactKind::Decision => c::GROUP_PAGE_KIND_DECISION,
    }
}

/// `09:55` today, else `11-02`.
pub fn short_time(now: crate::data::Millis, at: crate::data::Millis) -> String {
    use chrono::{Local, TimeZone as _};
    let same_day = match (
        Local.timestamp_millis_opt(now).single(),
        Local.timestamp_millis_opt(at).single(),
    ) {
        (Some(a), Some(b)) => a.date_naive() == b.date_naive(),
        _ => false,
    };
    if same_day {
        clock(at)
    } else {
        kit::month_day(at)
    }
}

/// The 进行中 rows shown before `看全部`.
const ACTIVE_ROWS: usize = 8;
/// The columns of the 群's lists: StatusBadge and time.
/// The StatusBadge column of a 话题 row.
const STATUS_COLUMN: gpui::Pixels = px(112.);

const COLUMNS: page::Columns = page::Columns {
    status: Some(STATUS_COLUMN),
    time: Some(page::m::TIME_COLUMN),
    actions: None,
};

impl GroupPage {
    /// The toolbar row: the 群's name over its purpose; 产出 and 群设置, then
    /// `新话题`.
    fn toolbar(
        &self,
        g: &str,
        load: PageLoad,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let p = &self.services.provider;
        let group = p.groups().into_iter().find(|x| x.id == g);
        // The same members 群资料 lists, so the count matches it.
        // §2.14, A5: the count shows once the 群 is synced (the cache has
        // only me before that); offline the subtitle says 离线.
        let n = self.listed_members(g).len();
        let ready = load == PageLoad::Ready;
        let count = match load {
            PageLoad::Ready => Some(fill(c::GROUP_PAGE_MEMBERS, &[("n", &n.to_string())])),
            PageLoad::Failed => Some(crate::copy::desk::DESK_SUBTITLE_OFFLINE.to_string()),
            PageLoad::Loading => Some(crate::copy::desk::DESK_SUBTITLE_SYNCING.to_string()),
        };
        let subtitle = [count, p.group_purpose(g)]
            .into_iter()
            .flatten()
            .collect::<Vec<_>>()
            .join(" · ");
        let active = emphasized(window);
        let nav = self.services.nav.clone();
        let chat_nav = self.services.nav.clone();
        let chat_group = g.to_string();
        let services = self.services.clone();
        let group_id = g.to_string();
        let more = toolbar_item(
            "group-more",
            Sf::More,
            c::GROUP_PAGE_SETTINGS,
            ToolbarItemKind::Plain,
            active,
            window,
            cx,
        )
        .dropdown_menu_with_anchor(gpui::Anchor::TopRight, move |menu, window, cx| {
            group_menu(menu, &services, &group_id, window, cx)
        });
        // §2.2: every toolbar control is h32 on one center line.
        // §2.14: no create button while the first sync runs or offline;
        // it stays in place, disabled at 50 %.
        let new_topic = crate::ui::native::disabled_push(
            // A quiet muted-fill button with a plus, not the black primary:
            // the page is for reading what runs, creating is one action of
            // several (owner decision, v18).
            push_button(
                "group-new-topic",
                c::GROUP_PAGE_NEW_TOPIC,
                PushStyle::Secondary,
                PushSize::Regular,
                cx,
            )
            .icon(crate::ui::button_glyph(
                Sym::Plus,
                crate::theme::icon::HEADER,
                crate::theme::size::ICON_BUTTON,
            )),
            !ready,
            cx,
        )
        .on_click(|_, window, cx| window.dispatch_action(Box::new(crate::app::NewTopic), cx));
        // ToolbarRow draws the one 1 px line at y 52 (§2.2).
        div()
            .flex_none()
            .child(
                ToolbarRow::new(group.map(|x| x.name).unwrap_or_default())
                    .subtitle(subtitle)
                    .leading_inset(rows::toolbar_leading(&self.services, cx))
                    .group(ToolbarGroup::new([toolbar_item(
                        "group-chat",
                        Sf::ChevronLeft,
                        c::GROUP_PAGE_BACK_TO_CHAT,
                        ToolbarItemKind::Plain,
                        active,
                        window,
                        cx,
                    )
                    .on_click(move |_, _, cx| {
                        let g = chat_group.clone();
                        chat_nav.update(cx, |n, cx| n.open_group_chat(g, cx))
                    })]))
                    .group(ToolbarGroup::new([toolbar_item(
                        "group-outputs",
                        Sf::Outputs,
                        c::GROUP_PAGE_OUTPUTS,
                        ToolbarItemKind::Plain,
                        active,
                        window,
                        cx,
                    )
                    .on_click(move |_, _, cx| {
                        nav.update(cx, |n, cx| n.open_artifacts(cx))
                    })]))
                    .group(rows::toolbar_capsule(more, cx))
                    .group(new_topic),
            )
            .into_any_element()
    }

    /// One open 话题 row (§2.6): the member's avatar (owner decision v16),
    /// the title over the owner and the step or note, then the StatusBadge
    /// and the time at fixed columns.
    fn topic_row(
        &self,
        t: &Topic,
        st: Standing,
        run: Option<&BotRunSummary>,
        cx: &App,
    ) -> AnyElement {
        let p = &self.services.provider;
        let owner_id = run
            .map(|r| r.bot_id.clone())
            .or_else(|| t.owner_id.clone())
            .unwrap_or_else(|| t.created_by.clone());
        let glance = p.topic_glance(&t.group_id, &t.id);
        // X-01: the name beside the member's avatar drops a leading emoji.
        let owner_name =
            crate::ui::avatar::name_beside_avatar(&member_name(&self.services, &owner_id))
                .to_string();
        let with_note = |first: String| match &glance.state_note {
            Some(n) => format!("{first} · {n}"),
            None => first,
        };
        // The one state mapping of a 话题 (§2.12, `topic_state`): asks to
        // me first, 在做 n/m only with a run, 排队 without one.
        let state =
            crate::views::topic_page::topic_state(p.as_ref(), self.services.me().as_deref(), t);
        let label = if state.short.is_empty() {
            state.text.clone()
        } else {
            state.short.clone()
        };
        let unified = crate::ui::native::StatusBadge::new(state.badge, label).into_any_element();
        let asker = || {
            with_note(fill(
                c::GROUP_PAGE_ASKED_BY,
                &[(
                    "name",
                    crate::ui::avatar::name_beside_avatar(&member_name(
                        &self.services,
                        &t.created_by,
                    )),
                )],
            ))
        };
        let (second, badge): (String, AnyElement) = match st {
            // The same words as the 话题 header and the sidebar.
            Standing::OnMe => (
                with_note(owner_name.clone()),
                crate::ui::native::StatusBadge::new(state.badge, state.text.clone())
                    .into_any_element(),
            ),
            Standing::Running => {
                let mut parts = vec![owner_name.clone()];
                if let Some(r) = run {
                    parts.extend(
                        r.plan
                            .iter()
                            .find(|s| s.state == PlanState::Current)
                            .map(|s| s.text.clone()),
                    );
                    // One clock (A5, `run_elapsed`): as on 首页, 成员 and the
                    // live pane.
                    parts.extend(crate::views::topic_page::run_elapsed_text(p.as_ref(), r));
                }
                (parts.join(" · "), unified)
            }
            // The same words as the sidebar (`topic_state`): 等小王回答, the
            // clock, muted. A long name cuts with "…" (whole in a tooltip),
            // so data never widens the status column. The meta line names the
            // owner, as on every row.
            Standing::OnOthers => (
                with_note(owner_name.clone()),
                crate::ui::native::StatusBadge::new(state.badge, state.text.clone())
                    .max_w(STATUS_COLUMN)
                    .into_any_element(),
            ),
            Standing::Stuck | Standing::Other => (asker(), unified),
        };
        // Owner decision v16: the leading media is the member (the status
        // shows only in the StatusBadge); the kind icon when no member.
        let media = match p.member(&owner_id) {
            Some(m) => rows::avatar(Some(&m), rows::AVATAR, cx),
            None => page::icon_box(Sym::Topic, None, cx),
        };
        // The time column: a run's start while it works, else the last
        // activity (one meaning per row kind on every page).
        let time = match (st, run) {
            (Standing::Running, Some(r)) => r.started_at.unwrap_or(r.created_at),
            _ => t.last_activity_at,
        };
        let nav = self.services.nav.clone();
        let (gid, tid) = (t.group_id.clone(), t.id.clone());
        let live = run
            .filter(|r| p.machine_screen(&r.id).is_some())
            .map(|r| r.id.clone());
        let menu_nav = self.services.nav.clone();
        let (mg, mt) = (gid.clone(), tid.clone());
        page::PageRow {
            media: Some(media),
            second: Some(line(second).into_any_element()),
            status: Some(badge),
            time: Some(clock(time).into()),
            ..page::PageRow::new(t.title.clone())
        }
        .render(
            SharedString::from(format!("group-topic-{}", t.id)),
            COLUMNS,
            cx,
        )
        // The title (TruncText) shows the whole title only when it is cut.
        .on_click(move |_, _, cx| {
            let (gid, tid, live) = (gid.clone(), tid.clone(), live.clone());
            nav.update(cx, |nav, cx| {
                nav.open_topic(gid, tid, cx);
                if let Some(run) = live {
                    nav.set_right_pane(RightPane::Live(run), cx);
                }
            })
        })
        .context_menu(move |menu, _, _| {
            let (nav, g, t) = (menu_nav.clone(), mg.clone(), mt.clone());
            menu.item(
                PopupMenuItem::new(c::GROUP_PAGE_OPEN_TOPIC).on_click(move |_, _, cx| {
                    let (g, t) = (g.clone(), t.clone());
                    nav.update(cx, |n, cx| n.open_topic(g, t, cx))
                }),
            )
        })
        .into_any_element()
    }

    /// The 最近完成 rows (at most 5) and how many there are. 完成 is a done
    /// 话题 (O-2), as in the sidebar: a 话题 that is open again (in
    /// `active`) is not listed (§2.21: no duplicates), and a 话题 marked
    /// done this week with no run result is listed too.
    fn done_rows(&self, g: &str, active: &[String], cx: &App) -> (Vec<AnyElement>, usize) {
        let p = &self.services.provider;
        let mut all: Vec<_> = p
            .group_recent_done(g)
            .into_iter()
            .filter(|d| !active.contains(&d.topic_id))
            .collect();
        let week_ago = p.clock_now() - 7 * 24 * 3_600_000;
        for t in p.topics(g) {
            if t.status == TopicStatus::Done
                && t.last_activity_at >= week_ago
                && !all.iter().any(|d| d.topic_id == t.id)
            {
                all.push(crate::data::pages::RecentDone {
                    group_id: t.group_id.clone(),
                    member_id: t.owner_id.clone().unwrap_or_else(|| t.created_by.clone()),
                    title: t.title.clone(),
                    note: None,
                    at: t.last_activity_at,
                    topic_id: t.id,
                });
            }
        }
        all.sort_by_key(|d| std::cmp::Reverse(d.at));
        let total = all.len();
        let done: Vec<_> = all.into_iter().take(5).collect();
        let rows = done
            .into_iter()
            .enumerate()
            .map(|(i, d)| {
                let nav = self.services.nav.clone();
                let (gid, tid) = (d.group_id.clone(), d.topic_id.clone());
                let mut rest = vec![
                    crate::ui::avatar::name_beside_avatar(&member_name(
                        &self.services,
                        &d.member_id,
                    ))
                    .to_string(),
                ];
                rest.extend(d.note.clone());
                page::PageRow {
                    // Owner decision v16: the member, not a second status.
                    media: Some(match p.member(&d.member_id) {
                        Some(m) => rows::avatar(Some(&m), rows::AVATAR, cx),
                        None => page::icon_box(Sym::Topic, None, cx),
                    }),
                    second: Some(line(rest.join(" · ")).into_any_element()),
                    status: Some(page::status_badge(
                        Status::Done,
                        c::GROUP_PAGE_STATUS_DONE,
                        cx,
                    )),
                    time: Some(kit::month_day(d.at).into()),
                    ..page::PageRow::new(d.title.clone())
                }
                .render(SharedString::from(format!("group-done-{i}")), COLUMNS, cx)
                .on_click(move |_, _, cx| {
                    let (gid, tid) = (gid.clone(), tid.clone());
                    nav.update(cx, |nav, cx| nav.open_topic(gid, tid, cx))
                })
                .into_any_element()
            })
            .collect();
        (rows, total)
    }

    /// The members 群资料 lists (and the subtitle counts).
    fn listed_members(&self, g: &str) -> Vec<crate::data::Member> {
        info::listed_members(&self.services, g)
    }
}

impl Render for GroupPage {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let k = native_colors(cx);
        let Some(g) = self.group_id(cx) else {
            return div().into_any_element();
        };
        let p = self.services.provider.clone();
        let me = self.services.me();
        let mut active: Vec<(Standing, Topic, Option<BotRunSummary>)> = p
            .topics(&g)
            .into_iter()
            .filter(|t| t.status != TopicStatus::Done)
            .map(|t| {
                let run = p.runs(&g, &t.id).into_iter().find(|r| r.status.is_active());
                let state = crate::views::topic_page::topic_state(p.as_ref(), me.as_deref(), &t);
                (standing(&t, &state), t, run)
            })
            .collect();
        active.sort_by(|a, b| {
            a.0.cmp(&b.0)
                .then(b.1.last_activity_at.cmp(&a.1.last_activity_at))
        });
        let n = active.len();
        let on_me = active.iter().filter(|(s, ..)| *s == Standing::OnMe).count();
        crate::selftest::drawn::mark("group-page", format!("{g} active={n} on_me={on_me}"));
        let active_ids: Vec<String> = active.iter().map(|(_, t, _)| t.id.clone()).collect();
        let (done, done_total) = self.done_rows(&g, &active_ids, cx);
        let load = PageLoad::of(
            &self.services,
            n > 0 || !done.is_empty() || !p.topics(&g).is_empty(),
        );
        let toolbar = self.toolbar(&g, load, window, cx);
        // Frames hug their rows and footer row; space below the last frame
        // is allowed (A1).
        let mut content = div()
            .min_h_full()
            .px(page::m::EDGE)
            .pt(page::m::GUTTER)
            .pb(page::m::GUTTER)
            .flex()
            .flex_col()
            .gap(page::m::GUTTER);
        let section = |header: gpui::Div, body: gpui::Div, grow: bool| {
            div()
                .flex()
                .flex_col()
                .when(grow, |d| d.flex_1())
                .gap(page::m::HEADER_GAP)
                .child(header)
                .child(body)
        };
        match load {
            PageLoad::Failed => {
                content = content.child(rows::failed_region("group", &self.services, cx));
            }
            PageLoad::Loading => {
                content = content.child(section(
                    page::section_header(c::GROUP_PAGE_ACTIVE, None, None, None, cx),
                    div().child(kit::skeleton_list("group-sk", COLUMNS, false, 4, cx)),
                    false,
                ));
                content = content.child(section(
                    page::section_header(c::GROUP_PAGE_RECENT_DONE, None, None, None, cx),
                    div().child(kit::skeleton_list("group-sk-done", COLUMNS, false, 3, cx)),
                    false,
                ));
            }
            PageLoad::Ready if n == 0 && done.is_empty() => {
                content = content.child(
                    div()
                        .flex_none()
                        .flex()
                        .rounded(page::m::FRAME_RADIUS)
                        .child(page::page_empty(
                            Sym::Topic,
                            c::GROUP_PAGE_EMPTY_TITLE,
                            c::GROUP_PAGE_EMPTY_BODY,
                            vec![
                                push_button(
                                    "group-empty-new",
                                    c::GROUP_PAGE_NEW_TOPIC,
                                    PushStyle::Default,
                                    PushSize::Small,
                                    cx,
                                )
                                .on_click(|_, window, cx| {
                                    window.dispatch_action(Box::new(crate::app::NewTopic), cx)
                                })
                                .into_any_element(),
                            ],
                            cx,
                        )),
                );
            }
            PageLoad::Ready => {
                // One section per standing (owner decision v16): 等你, 在做,
                // then 卡住了, 等别人 and 排队 when they have rows. 在做
                // always shows (a compact empty when nothing works). Each
                // frame ends with a footer row (§2.6): `看全部 N 个话题`
                // when the list is cut, else the count.
                for st in Standing::ALL {
                    let rows: Vec<_> = active.iter().filter(|(s, ..)| *s == st).collect();
                    let total = rows.len();
                    if total == 0 && st != Standing::Running {
                        continue;
                    }
                    let shown = if self.show_all {
                        total
                    } else {
                        total.min(ACTIVE_ROWS)
                    };
                    let topic_rows: Vec<AnyElement> = rows
                        .iter()
                        .take(shown)
                        .map(|(st, t, run)| self.topic_row(t, *st, run.as_ref(), cx))
                        .collect();
                    let slug = st.slug();
                    let footer = if total > ACTIVE_ROWS {
                        let text = if self.show_all {
                            c::GROUP_PAGE_SHOW_LESS.to_string()
                        } else {
                            fill(c::GROUP_PAGE_SHOW_ALL, &[("n", &total.to_string())])
                        };
                        page::list_footer(
                            SharedString::from(format!("group-more-{slug}")),
                            text,
                            None,
                            cx,
                        )
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.show_all = !this.show_all;
                            cx.notify();
                        }))
                    } else {
                        page::list_footer(
                            SharedString::from(format!("group-count-{slug}")),
                            fill(c::GROUP_PAGE_COUNT, &[("n", &total.to_string())]),
                            None,
                            cx,
                        )
                    };
                    let body = if total > 0 {
                        page::frame(cx).children(topic_rows).child(footer)
                    } else {
                        page::frame(cx).child(div().px(tsp::S16).child(page::compact_empty(
                            Sym::Topic,
                            c::GROUP_PAGE_ACTIVE_EMPTY,
                            None,
                            cx,
                        )))
                    };
                    content = content.child(section(
                        page::section_header(st.title(), Some(total), None, None, cx),
                        body,
                        false,
                    ));
                }
                // 最近完成 always shows (an Empty when nothing is done); its
                // frame hugs its rows and footer.
                let done_body = if done.is_empty() {
                    page::frame(cx).child(page::page_empty(
                        Sym::Done,
                        c::GROUP_PAGE_DONE_EMPTY,
                        c::GROUP_PAGE_DONE_EMPTY_BODY,
                        Vec::new(),
                        cx,
                    ))
                } else {
                    page::frame(cx).children(done).child(page::list_footer(
                        "group-count-done",
                        fill(c::GROUP_PAGE_COUNT, &[("n", &done_total.to_string())]),
                        None,
                        cx,
                    ))
                };
                content = content.child(section(
                    page::section_header(
                        c::GROUP_PAGE_RECENT_DONE,
                        Some(done_total),
                        None,
                        None,
                        cx,
                    ),
                    done_body,
                    false,
                ));
            }
        }
        let list = div()
            .id("group-main")
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .child(content);
        div()
            .size_full()
            .flex()
            .bg(k.content)
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .h_full()
                    .flex()
                    .flex_col()
                    .child(toolbar)
                    .child(list),
            )
            .into_any_element()
    }
}

/// The 群 commands of the toolbar's `ellipsis` pull-down: the same items as
/// the 群's context menu in the sidebar (§7.3: every context-menu item also
/// exists in the main UI). 静音 or 取消静音 and the 提醒 submenu; for a 群
/// also 重命名群…, 添加成员… and, apart, 离开群… (it asks first).
fn group_menu(
    menu: gpui_component::menu::PopupMenu,
    services: &Services,
    group_id: &str,
    window: &mut Window,
    cx: &mut gpui::Context<gpui_component::menu::PopupMenu>,
) -> gpui_component::menu::PopupMenu {
    use crate::views::nav as nav_views;
    let p = &services.provider;
    let Some(g) = p.groups().into_iter().find(|g| g.id == group_id) else {
        return menu;
    };
    let menu = nav_views::notify::items(menu, services, &g.id, window, cx);
    if g.kind != crate::data::GroupKind::Group {
        return menu;
    }
    let menu = menu.separator();
    let (rename, add, leave) = (services.clone(), services.clone(), services.clone());
    let purpose = services.clone();
    let (g1, g2, g3) = (g.clone(), g.clone(), g);
    let purpose_group = g1.id.clone();
    menu.item(
        PopupMenuItem::new(nav_copy::RENAME_GROUP).on_click(move |_, window, cx| {
            nav_views::rename::open_rename_group(
                rename.clone(),
                g1.id.clone(),
                g1.name.clone(),
                window,
                cx,
            )
        }),
    )
    .item(
        PopupMenuItem::new(c::GROUP_PAGE_PURPOSE_MORE).on_click(move |_, window, cx| {
            open_purpose_sheet(purpose.clone(), purpose_group.clone(), window, cx)
        }),
    )
    .item(
        PopupMenuItem::new(nav_copy::ADD_MEMBERS).on_click(move |_, window, cx| {
            nav_views::dialogs::open_add_members(
                add.clone(),
                g2.id.clone(),
                g2.name.clone(),
                window,
                cx,
            )
        }),
    )
    .separator()
    // A destructive item reads in the destructive color (§2.11, T4-14).
    .item(
        PopupMenuItem::element(|_, cx: &mut App| {
            div()
                .text_color(crate::ui::native::native_colors(cx).red)
                .child(nav_copy::LEAVE_GROUP)
        })
        .on_click(move |_, window, cx| {
            nav_views::dialogs::confirm_leave(
                leave.clone(),
                g3.id.clone(),
                g3.name.clone(),
                window,
                cx,
            )
        }),
    )
}
