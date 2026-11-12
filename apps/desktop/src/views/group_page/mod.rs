//! 群 page (NATIVE.md §1, §9.4 lists, §9.3 inspector). 1.1.1: a click on a
//! 群 opens its 群聊 (the 话题 page of the 群's chat); this page is the
//! secondary view of its 话题, opened from the 群聊's `话题`, with a
//! `回到群里聊` item back. The toolbar row says
//! the 群's name and purpose; the content is a Mail-like list of 进行中 and
//! 最近完成 话题 (a 话题 starts from 新话题 in the toolbar or the sidebar, or
//! ⌘N, which open the 新话题 sheet preset to this 群; there is no inline
//! post box). The page shows its 话题, then 群里的约定 (what members keep for this
//! 群, `views::memory`); the 群's info (成员, 通知, 群里的决定 …) is 群资料, the
//! 群聊's 详情 pane ([`info`]).
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
    /// 群里的约定 of the 群 on show, read again when another 群 opens.
    memory: Option<Entity<crate::views::memory::MemoryBoard>>,
    /// The page's scroll of each 群 (each keeps its own place) and where 群里的约定
    /// starts in it (a scene scrolls to it).
    scrolls: std::cell::RefCell<std::collections::HashMap<String, gpui::ScrollHandle>>,
    memory_top: std::rc::Rc<std::cell::Cell<gpui::Pixels>>,
    /// What the page shows, read from the provider once per change: a
    /// frame (a spinner elsewhere in the window redraws this page too) reads
    /// no store.
    snap: Option<Snapshot>,
    _subscriptions: Vec<Subscription>,
}

/// Set by a screenshot scene: the page scrolls to 群里的约定 once it has its answer.
static SCENE_SCROLL_MEMORY: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

/// Set by a screenshot scene before the page opens.
pub fn scroll_to_memory_for_scene() {
    SCENE_SCROLL_MEMORY.store(true, std::sync::atomic::Ordering::Relaxed);
}

impl GroupPage {
    /// `_composer`: the shared composer, which this page no longer shows
    /// (topics start in the 新话题 sheet); the shell still passes it.
    pub fn new(
        services: Services,
        _composer: Entity<ComposerView>,
        cx: &mut Context<Self>,
    ) -> Self {
        let mut subscriptions = watch(&services, cx);
        subscriptions.push(cx.subscribe(
            &services.hub,
            |this: &mut Self, _, _: &crate::app::Changes, _| this.snap = None,
        ));
        Self {
            _subscriptions: subscriptions,
            services,
            show_all: false,
            memory: None,
            scrolls: Default::default(),
            memory_top: std::rc::Rc::new(std::cell::Cell::new(gpui::Pixels::ZERO)),
            snap: None,
        }
    }

    /// The snapshot of `g`, read again after a provider change or when
    /// another 群 opens. Render puts it back when it is done.
    fn take_snapshot(&mut self, g: &str) -> Snapshot {
        match self.snap.take() {
            Some(snap) if snap.group == g => snap,
            _ => Snapshot::read(&self.services, g),
        }
    }

    fn scroll_of(&self, g: &str) -> gpui::ScrollHandle {
        self.scrolls
            .borrow_mut()
            .entry(g.to_string())
            .or_insert_with(gpui::ScrollHandle::new)
            .clone()
    }

    /// 群里的约定 (沉淀, the 群's scope) of `g`: one board per 群 on show.
    fn memory_board(
        &mut self,
        g: &str,
        cx: &mut Context<Self>,
    ) -> Entity<crate::views::memory::MemoryBoard> {
        use crate::{data::memory::MemoryScope, views::memory};
        let scope = MemoryScope::Group(g.to_string());
        if let Some(b) = self
            .memory
            .as_ref()
            .filter(|b| *b.read(cx).scope() == scope)
        {
            return b.clone();
        }
        let services = self.services.clone();
        let board = gpui::AppContext::new(cx, |cx| {
            memory::MemoryBoard::new(services, scope, memory::Host::Page, String::new(), cx)
        });
        // The page draws again when the board's answers arrive (it may hide itself).
        cx.observe(&board, |_, _, cx| cx.notify()).detach();
        self.memory = Some(board.clone());
        board
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
        if let Some(snap) = self.snap.as_ref().filter(|x| x.group == g) {
            return TitleBarSpec {
                subtitle: snap.purpose.clone(),
                ..TitleBarSpec::page(snap.name.clone())
            };
        }
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
        snap: &Snapshot,
        load: PageLoad,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let g = snap.group.as_str();
        // The same members 群资料 lists, so the count matches it.
        // §2.14, A5: the count shows once the 群 is synced (the cache has
        // only me before that); offline the subtitle says 离线.
        let n = snap.members;
        let ready = load == PageLoad::Ready;
        let count = match load {
            PageLoad::Ready => Some(fill(c::GROUP_PAGE_MEMBERS, &[("n", &n.to_string())])),
            PageLoad::Failed => Some(crate::copy::desk::DESK_SUBTITLE_OFFLINE.to_string()),
            PageLoad::Loading => Some(crate::copy::desk::DESK_SUBTITLE_SYNCING.to_string()),
        };
        let subtitle = [count, snap.purpose.clone()]
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
                ToolbarRow::new(snap.name.clone())
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
    fn topic_row(&self, row: &ActiveRow, cx: &App) -> AnyElement {
        let p = &self.services.provider;
        let (t, st, run, state) = (&row.topic, row.standing, row.run.as_ref(), &row.state);
        // X-01: the name beside the member's avatar drops a leading emoji.
        let owner_name = crate::ui::avatar::name_beside_avatar(&row.owner_name).to_string();
        let with_note = |first: String| match &row.note {
            Some(n) => format!("{first} · {n}"),
            None => first,
        };
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
                    crate::ui::avatar::name_beside_avatar(&row.asker_name),
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
        let media = match &row.owner {
            Some(m) => rows::avatar(Some(m), rows::AVATAR, cx),
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
        // Whether its machine screen shows is asked on the click, not in a
        // frame.
        let run_id = run.map(|r| r.id.clone());
        let screens = self.services.provider.clone();
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
            let (gid, tid) = (gid.clone(), tid.clone());
            let live = run_id
                .clone()
                .filter(|r| screens.machine_screen(r).is_some());
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
    fn done_rows(&self, snap: &Snapshot, cx: &App) -> (Vec<AnyElement>, usize) {
        let rows = snap
            .done
            .iter()
            .enumerate()
            .map(|(i, row)| {
                let d = &row.done;
                let nav = self.services.nav.clone();
                let (gid, tid) = (d.group_id.clone(), d.topic_id.clone());
                let mut rest = vec![crate::ui::avatar::name_beside_avatar(&row.name).to_string()];
                rest.extend(d.note.clone());
                page::PageRow {
                    // Owner decision v16: the member, not a second status.
                    media: Some(match &row.member {
                        Some(m) => rows::avatar(Some(m), rows::AVATAR, cx),
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
        (rows, snap.done_total)
    }
}

/// One open 话题 row of the page, with what its row shows.
struct ActiveRow {
    standing: Standing,
    topic: Topic,
    run: Option<BotRunSummary>,
    /// The one state mapping of a 话题 (§2.12, `topic_state`): asks to me
    /// first, 在做 n/m only with a run, 排队 without one.
    state: crate::views::topic_page::StateLine,
    /// The glance's state note.
    note: Option<String>,
    owner: Option<crate::data::Member>,
    owner_name: String,
    asker_name: String,
}

/// One 最近完成 row.
struct DoneRow {
    done: crate::data::pages::RecentDone,
    member: Option<crate::data::Member>,
    name: String,
}

/// What the 群 page shows of one 群, read from the provider once per change.
struct Snapshot {
    group: String,
    name: String,
    purpose: Option<String>,
    /// The members 群资料 lists (and the subtitle counts).
    members: usize,
    /// The open 话题 by standing, then newest first.
    active: Vec<ActiveRow>,
    /// The first 5 of 最近完成.
    done: Vec<DoneRow>,
    done_total: usize,
    /// The cache has 话题 of this 群.
    has_topics: bool,
    /// A 群, not a 私聊.
    is_group: bool,
}

impl Snapshot {
    fn read(services: &Services, g: &str) -> Self {
        let p = &services.provider;
        let me = services.me();
        let topics = p.topics(g);
        let group = p.groups().into_iter().find(|x| x.id == g);
        let mut active: Vec<ActiveRow> = topics
            .iter()
            .filter(|t| t.status != TopicStatus::Done)
            .map(|t| {
                let run = p.runs(g, &t.id).into_iter().find(|r| r.status.is_active());
                let state = crate::views::topic_page::topic_state(p.as_ref(), me.as_deref(), t);
                let owner_id = run
                    .as_ref()
                    .map(|r| r.bot_id.clone())
                    .or_else(|| t.owner_id.clone())
                    .unwrap_or_else(|| t.created_by.clone());
                let owner = p.member(&owner_id);
                ActiveRow {
                    standing: standing(t, &state),
                    note: p.topic_glance(&t.group_id, &t.id).state_note,
                    owner_name: owner.as_ref().map(|m| m.name.clone()).unwrap_or_default(),
                    asker_name: member_name(services, &t.created_by),
                    owner,
                    state,
                    run,
                    topic: t.clone(),
                }
            })
            .collect();
        active.sort_by(|a, b| {
            a.standing
                .cmp(&b.standing)
                .then(b.topic.last_activity_at.cmp(&a.topic.last_activity_at))
        });
        // 完成 is a done 话题 (O-2), as in the sidebar: a 话题 that is open
        // again is not listed (§2.21: no duplicates), and a 话题 marked done
        // this week with no run result is listed too.
        let mut all: Vec<_> = p
            .group_recent_done(g)
            .into_iter()
            .filter(|d| !active.iter().any(|a| a.topic.id == d.topic_id))
            .collect();
        let week_ago = p.clock_now() - 7 * 24 * 3_600_000;
        for t in &topics {
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
                    topic_id: t.id.clone(),
                });
            }
        }
        all.sort_by_key(|d| std::cmp::Reverse(d.at));
        let done_total = all.len();
        let done = all
            .into_iter()
            .take(5)
            .map(|d| {
                let member = p.member(&d.member_id);
                DoneRow {
                    name: member.as_ref().map(|m| m.name.clone()).unwrap_or_default(),
                    member,
                    done: d,
                }
            })
            .collect();
        Self {
            group: g.to_string(),
            name: group.as_ref().map(|x| x.name.clone()).unwrap_or_default(),
            purpose: p.group_purpose(g),
            members: info::listed_members(services, g).len(),
            active,
            done,
            done_total,
            has_topics: !topics.is_empty(),
            is_group: group.is_some_and(|x| x.kind == crate::data::GroupKind::Group),
        }
    }
}

impl Render for GroupPage {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let k = native_colors(cx);
        let Some(g) = self.group_id(cx) else {
            return div().into_any_element();
        };
        let snap = self.take_snapshot(&g);
        let n = snap.active.len();
        let on_me = snap
            .active
            .iter()
            .filter(|a| a.standing == Standing::OnMe)
            .count();
        crate::selftest::drawn::mark("group-page", format!("{g} active={n} on_me={on_me}"));
        let (done, done_total) = self.done_rows(&snap, cx);
        let load = PageLoad::of(&self.services, n > 0 || !done.is_empty() || snap.has_topics);
        let toolbar = self.toolbar(&snap, load, window, cx);
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
                    let rows: Vec<_> = snap.active.iter().filter(|a| a.standing == st).collect();
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
                        .map(|row| self.topic_row(row, cx))
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
        // 群里的约定 under the 话题 of a 群 (not a 私聊: nothing is kept for one there).
        let is_group = snap.is_group;
        // Kept for the next frame.
        self.snap = Some(snap);
        if load == PageLoad::Ready && is_group {
            let board = self.memory_board(&g, cx);
            if !board.read(cx).hidden() {
                let top = self.memory_top.clone();
                content = content.child(
                    div().relative().child(board.clone()).child(
                        // Measures where the card starts (a scene scrolls to it).
                        gpui::canvas(
                            move |bounds, _, _| top.set(bounds.origin.y),
                            |_, _, _, _| {},
                        )
                        .absolute()
                        .top_0()
                        .left_0()
                        .size_full(),
                    ),
                );
            }
            // A scene's scroll: kept until the board has its answer, since it grows then.
            if SCENE_SCROLL_MEMORY.load(std::sync::atomic::Ordering::Relaxed) {
                if board.read(cx).loaded() {
                    SCENE_SCROLL_MEMORY.store(false, std::sync::atomic::Ordering::Relaxed);
                }
                let (scroll, top) = (self.scroll_of(&g), self.memory_top.clone());
                window.on_next_frame(move |window, _| {
                    let y = top.get() - scroll.bounds().origin.y - scroll.offset().y;
                    scroll.set_offset(gpui::point(gpui::Pixels::ZERO, -(y - page::m::GUTTER)));
                    window.refresh();
                });
            }
        }
        // One scroll place per 群: GPUI keeps it by element id, so a shared
        // id opened every 群 at the place the last one was left.
        let list = div()
            .id(gpui::ElementId::Name(format!("group-main-{g}").into()))
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .track_scroll(&self.scroll_of(&g))
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
        let menu = if services.provider.can_hide_delete() {
            menu.separator()
        } else {
            menu
        };
        return nav_views::hide_delete::hide_group_item(menu, services, &g);
    }
    let menu = menu.separator();
    let hide = g.clone();
    let (rename, add, leave) = (services.clone(), services.clone(), services.clone());
    let purpose = services.clone();
    let (g1, g2, g3) = (g.clone(), g.clone(), g);
    let purpose_group = g1.id.clone();
    let menu = menu
        .item(
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
        .separator();
    let menu = nav_views::hide_delete::hide_group_item(menu, services, &hide);
    // A destructive item reads in the destructive color (§2.11, T4-14).
    let menu = menu.item(
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
    );
    nav_views::hide_delete::delete_group_item(menu, services, &hide)
}
