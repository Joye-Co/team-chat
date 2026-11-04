//! 群 page (NATIVE.md §1, §9.4 lists, §9.3 inspector): the toolbar row says
//! the 群's name and purpose; the content is a Mail-like list of 进行中 and
//! 最近完成 话题 (a 话题 starts from `square.and.pencil` in the toolbar or
//! the sidebar, or ⌘N, which open the 新话题 sheet preset to this 群; there
//! is no inline post box); the inspector is
//! the 群's info like a Contacts group: 成员, 群里的决定 (记一条… opens a
//! sheet), 最近的产出 and 在哪干活.
//!
//! The page draws its own toolbar row ([`PageView::draws_own_header`]): the
//! inspector runs from the top of the window.

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
            InspectorSection, NativeType as _, PushSize, PushStyle, ToolbarGroup, ToolbarItemKind,
            ToolbarRow, borderless_button, emphasized, inspector_column, metrics, native_colors,
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
    aside_open: bool,
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
            aside_open: true,
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

/// How a 话题 stands on its card, in sort order.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Standing {
    OnMe,
    Running,
    OnOthers,
    Other,
}

fn standing(t: &Topic, me: Option<&str>, run: Option<&BotRunSummary>) -> Standing {
    if me.is_some_and(|me| t.waiting_on.iter().any(|w| w == me)) {
        Standing::OnMe
    } else if run.is_some() {
        Standing::Running
    } else if !t.waiting_on.is_empty() {
        Standing::OnOthers
    } else {
        Standing::Other
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
const COLUMNS: page::Columns = page::Columns {
    status: Some(px(112.)),
    time: Some(page::m::TIME_COLUMN),
    actions: None,
};

impl GroupPage {
    /// The toolbar row: the 群's name over its purpose; 产出 and 群设置, the
    /// primary `新话题`, then the right-pane toggle.
    fn toolbar(&self, g: &str, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let p = &self.services.provider;
        let group = p.groups().into_iter().find(|x| x.id == g);
        // The same members the inspector lists, so the count matches it.
        let n = self.listed_members(g).len();
        let subtitle = match p.group_purpose(g) {
            Some(purpose) => format!(
                "{} · {purpose}",
                fill(c::GROUP_PAGE_MEMBERS, &[("n", &n.to_string())])
            ),
            None => fill(c::GROUP_PAGE_MEMBERS, &[("n", &n.to_string())]),
        };
        let active = emphasized(window);
        let nav = self.services.nav.clone();
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
        .dropdown_menu_with_anchor(gpui::Anchor::TopRight, move |menu, _, _| {
            group_menu(menu, &services, &group_id)
        });
        // §2.2: every toolbar control is h32 on one center line.
        let new_topic = push_button(
            "group-new-topic",
            c::GROUP_PAGE_NEW_TOPIC,
            PushStyle::Default,
            PushSize::Regular,
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
                    .group(new_topic)
                    .group(rows::toolbar_separator(cx))
                    .group(ToolbarGroup::new([toolbar_item(
                        "group-inspector",
                        Sf::PaneRight,
                        if self.aside_open {
                            c::GROUP_PAGE_COLLAPSE
                        } else {
                            c::GROUP_PAGE_EXPAND
                        },
                        if self.aside_open {
                            ToolbarItemKind::Toggled
                        } else {
                            ToolbarItemKind::Plain
                        },
                        active,
                        window,
                        cx,
                    )
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.aside_open = !this.aside_open;
                        cx.notify();
                    }))])),
            )
            .into_any_element()
    }

    /// One 进行中 row (§2.6): a topic-status icon box (not the owner's
    /// avatar, which repeats), the title over the owner and the step or
    /// note, then the StatusBadge and the time at fixed columns.
    fn topic_row(
        &self,
        t: &Topic,
        st: Standing,
        run: Option<&BotRunSummary>,
        last: bool,
        cx: &App,
    ) -> AnyElement {
        let k = native_colors(cx);
        let p = &self.services.provider;
        let owner_id = run
            .map(|r| r.bot_id.clone())
            .or_else(|| t.owner_id.clone())
            .unwrap_or_else(|| t.created_by.clone());
        let glance = p.topic_glance(&t.group_id, &t.id);
        let owner_name = member_name(&self.services, &owner_id);
        let with_note = |first: String| match &glance.state_note {
            Some(n) => format!("{first} · {n}"),
            None => first,
        };
        let (glyph, tint, second, badge): (Sym, gpui::Hsla, String, Option<AnyElement>) = match st {
            Standing::OnMe => {
                // The same words as the 话题 header: `等你审 PR #61`.
                let (label, pr) = match t.status {
                    TopicStatus::WaitingReview => (
                        match p.topic_changes(&t.group_id, &t.id) {
                            Some(ch) => fill(
                                crate::copy::topic_rail::STATE_REVIEW,
                                &[("n", &ch.number.to_string())],
                            ),
                            None => c::GROUP_PAGE_STATUS_REVIEW.to_string(),
                        },
                        None::<String>,
                    ),
                    TopicStatus::WaitingAnswer => (c::GROUP_PAGE_STATUS_DECIDE.to_string(), None),
                    _ => (c::GROUP_PAGE_STATUS_ANSWER.to_string(), None),
                };
                let mut parts: Vec<String> = pr.into_iter().collect();
                parts.push(with_note(owner_name));
                (
                    Sym::NeedsYou,
                    k.amber,
                    parts.join(" · "),
                    Some(page::status_badge(Status::NeedsYou, label, cx)),
                )
            }
            Standing::Running => {
                let r = run.expect("a running 话题 has a run");
                let step = r.plan.iter().position(|s| s.state == PlanState::Current);
                let since = step
                    .and_then(|i| p.run_step_times(&r.id).get(i).copied())
                    .or(r.started_at)
                    .unwrap_or(r.created_at);
                let mut parts = vec![owner_name];
                parts.extend(step.map(|i| r.plan[i].text.clone()));
                parts.push(kit::elapsed(p.clock_now(), since));
                let text = match kit::plan_progress(&r.plan) {
                    Some((n, m)) => fill(
                        c::GROUP_PAGE_STATUS_WORKING,
                        &[("n", &n.to_string()), ("m", &m.to_string())],
                    ),
                    None => c::GROUP_PAGE_STATUS_STARTED.to_string(),
                };
                (
                    Sym::Spinner,
                    k.blue,
                    parts.join(" · "),
                    Some(page::status_badge(Status::Working, text, cx)),
                )
            }
            Standing::OnOthers => {
                // The badge keeps a short state word; who it waits on goes
                // to the meta line, so data never widens the status column.
                let (text, who) = match t.waiting_on.as_slice() {
                    [one] => (
                        c::GROUP_PAGE_STATUS_WAITING_OTHER.to_string(),
                        fill(
                            c::GROUP_PAGE_STATUS_WAITING,
                            &[("name", &member_name(&self.services, one))],
                        ),
                    ),
                    many => {
                        let n = fill(
                            c::GROUP_PAGE_STATUS_WAITING_MANY,
                            &[("n", &many.len().to_string())],
                        );
                        (n.clone(), n)
                    }
                };
                let mut parts = vec![who];
                parts.extend(glance.state_note.clone());
                (
                    Sym::Waiting,
                    k.secondary,
                    parts.join(" · "),
                    Some(page::status_badge(Status::Queued, text, cx)),
                )
            }
            Standing::Other => (
                Sym::Topic,
                k.label,
                with_note(fill(
                    c::GROUP_PAGE_ASKED_BY,
                    &[("name", &member_name(&self.services, &t.created_by))],
                )),
                None,
            ),
        };
        let nav = self.services.nav.clone();
        let (gid, tid) = (t.group_id.clone(), t.id.clone());
        let live = run
            .filter(|r| p.machine_screen(&r.id).is_some())
            .map(|r| r.id.clone());
        let menu_nav = self.services.nav.clone();
        let (mg, mt) = (gid.clone(), tid.clone());
        let title = t.title.clone();
        page::PageRow {
            // The media box shows the badge's own icon and color (§2.12: one
            // look per state).
            media: Some(page::icon_box(glyph, Some(tint), cx)),
            second: Some(line(second).into_any_element()),
            status: badge,
            time: Some(clock(t.last_activity_at).into()),
            separator: !last,
            ..page::PageRow::new(t.title.clone())
        }
        .render(
            SharedString::from(format!("group-topic-{}", t.id)),
            COLUMNS,
            cx,
        )
        .tooltip(move |window, cx| {
            gpui_component::tooltip::Tooltip::new(title.clone()).build(window, cx)
        })
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

    /// The 最近完成 rows (at most 5) and how many there are. A 话题 that is
    /// open again (in `active`) is not listed twice (§2.21: no duplicates).
    fn done_rows(&self, g: &str, active: &[String], cx: &App) -> (Vec<AnyElement>, usize) {
        let k = native_colors(cx);
        let p = &self.services.provider;
        let all: Vec<_> = p
            .group_recent_done(g)
            .into_iter()
            .filter(|d| !active.contains(&d.topic_id))
            .collect();
        let total = all.len();
        let done: Vec<_> = all.into_iter().take(5).collect();
        let n = done.len();
        let rows = done
            .into_iter()
            .enumerate()
            .map(|(i, d)| {
                let nav = self.services.nav.clone();
                let (gid, tid) = (d.group_id.clone(), d.topic_id.clone());
                let mut rest = vec![member_name(&self.services, &d.member_id)];
                rest.extend(d.note.clone());
                let title = d.title.clone();
                page::PageRow {
                    media: Some(page::icon_box(Sym::Done, Some(k.green), cx)),
                    second: Some(line(rest.join(" · ")).into_any_element()),
                    status: Some(page::status_badge(
                        Status::Done,
                        c::GROUP_PAGE_STATUS_DONE,
                        cx,
                    )),
                    time: Some(kit::month_day(d.at).into()),
                    separator: i + 1 < n,
                    ..page::PageRow::new(d.title.clone())
                }
                .render(SharedString::from(format!("group-done-{i}")), COLUMNS, cx)
                .tooltip(move |window, cx| {
                    gpui_component::tooltip::Tooltip::new(title.clone()).build(window, cx)
                })
                .on_click(move |_, _, cx| {
                    let (gid, tid) = (gid.clone(), tid.clone());
                    nav.update(cx, |nav, cx| nav.open_topic(gid, tid, cx))
                })
                .into_any_element()
            })
            .collect();
        (rows, total)
    }

    /// The members the inspector lists (and the subtitle counts): known,
    /// not deactivated.
    fn listed_members(&self, g: &str) -> Vec<crate::data::Member> {
        let p = &self.services.provider;
        p.groups()
            .into_iter()
            .find(|x| x.id == g)
            .map(|x| x.member_ids)
            .unwrap_or_default()
            .iter()
            .filter_map(|id| p.member(id))
            .filter(|m| !m.deactivated)
            .collect()
    }

    /// The right pane (§2.5): the header band `群资料`, then 成员, 群里的决定,
    /// 最近的产出 and 在哪干活, each with a compact empty when it has nothing.
    /// `loading` draws skeleton rows instead.
    fn inspector(
        &self,
        g: &str,
        load: PageLoad,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let k = native_colors(cx);
        let p = self.services.provider.clone();
        // Skeletons only while the page loads (§2.14); a failed load shows
        // one compact line per section, never pulsing bones (§2.15).
        let loading = load == PageLoad::Loading;
        let failed = load == PageLoad::Failed;
        let offline =
            |sym: Sym, cx: &App| page::compact_empty(sym, c::GROUP_PAGE_OFFLINE, None, cx);
        let header = div()
            .flex_none()
            .h(metrics::TOOLBAR_ROW)
            .px(metrics::INSPECTOR_PAD)
            .border_b_1()
            .border_color(k.separator)
            .flex()
            .flex_col()
            .justify_center()
            .child(
                div()
                    .n_title3()
                    .font_weight(gpui::FontWeight::MEDIUM)
                    .text_color(k.label)
                    .child(c::GROUP_PAGE_INFO),
            )
            .child(
                div()
                    .n_subheadline()
                    .text_color(k.secondary)
                    .child(c::GROUP_PAGE_INFO_LINE),
            );
        let mut col = div()
            .id("group-inspector")
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .pb(tsp::S24)
            .flex()
            .flex_col()
            // Pull the first GroupLabel up so its ink starts at y 68 (B2):
            // the section's own 16 top sits under the header band.
            .child(div().flex_none().mt(-tsp::S12));

        // 成员: the name keeps its room; the 岗位 truncates first, and both
        // show in full in the tooltip.
        let members = self.listed_members(g);
        let mut sec = InspectorSection::new("group-ins-members", c::GROUP_PAGE_MEMBERS_TITLE);
        if loading {
            sec = sec.row(page::skeleton_rows(
                "group-sk-m",
                page::SkeletonKind::Compact,
                4,
                cx,
            ));
        } else if failed {
            sec = sec.row(offline(Sym::Members, cx));
        } else if members.is_empty() {
            sec = sec.row(page::compact_empty(
                Sym::Members,
                c::GROUP_PAGE_MEMBERS_EMPTY,
                None,
                cx,
            ));
        }
        for m in members.into_iter().filter(|_| !loading && !failed) {
            let nav = self.services.nav.clone();
            let id = m.id.clone();
            let full = match &m.role {
                Some(r) if !r.is_empty() => format!("{} · {r}", m.name),
                _ => m.name.clone(),
            };
            sec = sec.row(
                div()
                    .id(SharedString::from(format!("group-member-{}", m.id)))
                    .h(tsp::S32)
                    .flex()
                    .items_center()
                    .gap(tsp::S8)
                    .n_body()
                    .child(rows::avatar(Some(&m), tsp::S16, cx))
                    // §2.21: the role truncates first; the name keeps its
                    // room (up to most of the row) and both are in the tip.
                    .child(
                        line(m.name.clone())
                            .flex_none()
                            .max_w(gpui::relative(0.72))
                            .text_color(k.label),
                    )
                    .child(
                        line(m.role.clone().unwrap_or_default())
                            .flex_1()
                            .min_w_0()
                            .text_color(k.secondary),
                    )
                    .tooltip(move |window, cx| {
                        gpui_component::tooltip::Tooltip::new(full.clone()).build(window, cx)
                    })
                    .on_click(move |_, _, cx| {
                        let id = id.clone();
                        nav.update(cx, |n, cx| n.open_members(Some(id), cx))
                    }),
            );
        }
        col = col.child(sec);

        // 群里的决定.
        let decisions = if loading || failed {
            Vec::new()
        } else {
            p.group_decisions(g)
        };
        let now = p.clock_now();
        let mut sec = InspectorSection::new("group-ins-decisions", c::GROUP_PAGE_DECISIONS);
        if p.can_record_decision(g) && !loading && !failed {
            let services = self.services.clone();
            let group = g.to_string();
            sec = sec.action(
                borderless_button(
                    "group-record",
                    Sf::Plus,
                    c::GROUP_PAGE_RECORD_MORE,
                    window,
                    cx,
                )
                .on_click(move |_, window, cx| {
                    cx.stop_propagation();
                    open_record_sheet(services.clone(), group.clone(), window, cx)
                }),
            );
        }
        if loading {
            sec = sec.row(page::skeleton_rows(
                "group-sk-d",
                page::SkeletonKind::Compact,
                2,
                cx,
            ));
        } else if failed {
            sec = sec.row(offline(Sym::List, cx));
        } else if decisions.is_empty() {
            // A neutral icon: a check would read as a done item.
            let record = p.can_record_decision(g).then(|| {
                let services = self.services.clone();
                let group = g.to_string();
                crate::ui::native::borderless_text("group-record-empty", c::GROUP_PAGE_RECORD, cx)
                    .on_click(move |_, window, cx| {
                        open_record_sheet(services.clone(), group.clone(), window, cx)
                    })
                    .into_any_element()
            });
            sec = sec.row(page::compact_empty(
                Sym::List,
                c::GROUP_PAGE_DECISIONS_EMPTY,
                record,
                cx,
            ));
        }
        for (i, d) in decisions.into_iter().enumerate() {
            let who = p.member(&d.by);
            let date = if now - d.at < 60_000 {
                c::GROUP_PAGE_JUST_NOW.to_string()
            } else {
                kit::month_day(d.at)
            };
            sec = sec.row(
                div()
                    .id(SharedString::from(format!("group-decision-{i}")))
                    .py(tsp::S6)
                    .flex()
                    .flex_col()
                    .gap(tsp::S2)
                    .child(kit::decision_text(&d.text, k.label))
                    .child(
                        div()
                            .n_subheadline()
                            .n_figures()
                            .text_color(k.secondary)
                            .child(format!(
                                "{} · {date}",
                                who.map(|m| m.name).unwrap_or_default()
                            )),
                    )
                    .tooltip({
                        let text = d.text.clone();
                        move |window, cx| {
                            gpui_component::tooltip::Tooltip::new(text.clone()).build(window, cx)
                        }
                    }),
            );
        }
        col = col.child(sec);

        // 最近的产出.
        let outputs: Vec<_> = if loading || failed {
            Vec::new()
        } else {
            p.group_artifacts(g).into_iter().take(3).collect()
        };
        let mut sec = InspectorSection::new("group-ins-outputs", c::GROUP_PAGE_RECENT_OUTPUTS);
        if loading {
            sec = sec.row(page::skeleton_rows(
                "group-sk-o",
                page::SkeletonKind::Compact,
                2,
                cx,
            ));
        } else if failed {
            sec = sec.row(offline(Sym::Document, cx));
        } else if outputs.is_empty() {
            sec = sec.row(page::compact_empty(
                Sym::Document,
                c::GROUP_PAGE_OUTPUTS_EMPTY,
                None,
                cx,
            ));
        }
        for a in outputs {
            let nav = self.services.nav.clone();
            let id = a.id.clone();
            let glyph = match a.kind {
                ArtifactKind::Image => Sym::Image,
                ArtifactKind::File => Sym::File,
                _ => Sym::Document,
            };
            let title = a.title.clone();
            sec = sec.row(
                div()
                    .id(SharedString::from(format!("group-output-{}", a.id)))
                    .py(tsp::S6)
                    .flex()
                    .items_start()
                    .gap(tsp::S8)
                    .child(div().pt(tsp::S2).child(page::row_icon(glyph, k.secondary)))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .child(line(a.title.clone()).n_body().text_color(k.label))
                            .child(
                                line(fill(
                                    c::GROUP_PAGE_ARTIFACT_META,
                                    &[
                                        ("v", &a.version.to_string()),
                                        ("kind", kind_label(a.kind)),
                                        ("name", &member_name(&self.services, &a.created_by)),
                                    ],
                                ))
                                .n_subheadline()
                                .text_color(k.secondary),
                            ),
                    )
                    .child(
                        div()
                            .flex_none()
                            .n_subheadline()
                            .n_figures()
                            .text_color(k.secondary)
                            .child(short_time(p.clock_now(), a.updated_at)),
                    )
                    .tooltip(move |window, cx| {
                        gpui_component::tooltip::Tooltip::new(title.clone()).build(window, cx)
                    })
                    .on_click(move |_, _, cx| {
                        let id = id.clone();
                        nav.update(cx, |nav, cx| nav.open_artifact(id, None, cx))
                    }),
            );
        }
        col = col.child(sec);

        // 在哪干活: the team computer a member of this 群 works on now.
        let run = if loading || failed {
            None
        } else {
            p.topics(g)
                .into_iter()
                .flat_map(|t| p.runs(g, &t.id))
                .filter(|r| r.status.is_active() && r.machine_name.is_some())
                .max_by_key(|r| r.updated_at)
        };
        let mut sec = InspectorSection::new("group-ins-where", c::GROUP_PAGE_WHERE);
        match run {
            None if loading => {
                sec = sec.row(page::skeleton_rows(
                    "group-sk-w",
                    page::SkeletonKind::Compact,
                    1,
                    cx,
                ));
            }
            None if failed => {
                sec = sec.row(offline(Sym::Computer, cx));
            }
            None => {
                sec = sec.row(page::compact_empty(
                    Sym::Computer,
                    c::GROUP_PAGE_WHERE_EMPTY,
                    None,
                    cx,
                ));
            }
            Some(run) => {
                let who = member_name(&self.services, &run.bot_id);
                let step = run
                    .plan
                    .iter()
                    .find(|s| s.state == PlanState::Current)
                    .map(|s| format!("{who} {}", s.text))
                    .unwrap_or_else(|| who.clone());
                let nav = self.services.nav.clone();
                let (gid, tid, rid) = (run.group_id.clone(), run.topic_id.clone(), run.id.clone());
                let name = run.machine_name.clone().unwrap_or_default();
                let glyph = kit::machine_sym(&name, run.machine_id.as_deref());
                let tip = format!("{name} · {step}");
                sec = sec
                    .action(
                        borderless_button(
                            "group-machine-open",
                            Sf::Open,
                            fill(c::GROUP_PAGE_SEE_COMPUTER, &[("name", &who)]),
                            window,
                            cx,
                        )
                        .on_click(move |_, _, cx| {
                            cx.stop_propagation();
                            let (gid, tid, rid) = (gid.clone(), tid.clone(), rid.clone());
                            nav.update(cx, |nav, cx| {
                                nav.open_topic(gid, tid, cx);
                                nav.set_right_pane(RightPane::Live(rid), cx);
                            })
                        }),
                    )
                    .row(
                        div()
                            .id("group-machine")
                            .py(tsp::S6)
                            .flex()
                            .items_center()
                            .gap(tsp::S8)
                            .child(page::row_icon(glyph, k.secondary))
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .flex()
                                    .flex_col()
                                    .child(line(name).n_body().text_color(k.label))
                                    .child(line(step).n_subheadline().text_color(k.secondary)),
                            )
                            .child(page::status_badge(
                                Status::Working,
                                c::GROUP_PAGE_IN_USE,
                                cx,
                            ))
                            .tooltip(move |window, cx| {
                                gpui_component::tooltip::Tooltip::new(tip.clone()).build(window, cx)
                            }),
                    );
            }
        }
        col = col.child(sec);
        inspector_column(cx)
            .child(header)
            .child(col)
            .into_any_element()
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
                (standing(&t, me.as_deref(), run.as_ref()), t, run)
            })
            .collect();
        active.sort_by(|a, b| {
            a.0.cmp(&b.0)
                .then(b.1.last_activity_at.cmp(&a.1.last_activity_at))
        });
        let n = active.len();
        let on_me = active.iter().filter(|(s, ..)| *s == Standing::OnMe).count();
        crate::selftest::drawn::mark("group-page", format!("{g} active={n} on_me={on_me}"));
        let toolbar = self.toolbar(&g, window, cx);
        let active_ids: Vec<String> = active.iter().map(|(_, t, _)| t.id.clone()).collect();
        let (done, done_total) = self.done_rows(&g, &active_ids, cx);
        let load = PageLoad::of(
            &self.services,
            n > 0 || !done.is_empty() || !p.topics(&g).is_empty(),
        );
        let shown = if self.show_all { n } else { n.min(ACTIVE_ROWS) };
        let topic_rows: Vec<AnyElement> = active
            .iter()
            .take(shown)
            .enumerate()
            .map(|(i, (st, t, run))| self.topic_row(t, *st, run.as_ref(), i + 1 == shown, cx))
            .collect();
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
                    div().child(page::skeleton_rows(
                        "group-sk",
                        page::SkeletonKind::Row,
                        4,
                        cx,
                    )),
                    false,
                ));
                content = content.child(section(
                    page::section_header(c::GROUP_PAGE_RECENT_DONE, None, None, None, cx),
                    div().child(page::skeleton_rows(
                        "group-sk-done",
                        page::SkeletonKind::Row,
                        4,
                        cx,
                    )),
                    false,
                ));
            }
            PageLoad::Ready if n == 0 && done.is_empty() => {
                content = content.child(
                    div()
                        .flex_none()
                        .flex()
                        .rounded(page::m::FRAME_RADIUS)
                        .border_1()
                        .border_color(k.separator)
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
                // 进行中: a footer row in every case (§2.6): `看全部 N 个话题`
                // when the list is cut, else the count.
                let footer = if n > ACTIVE_ROWS {
                    let text = if self.show_all {
                        c::GROUP_PAGE_SHOW_LESS.to_string()
                    } else {
                        fill(c::GROUP_PAGE_SHOW_ALL, &[("n", &n.to_string())])
                    };
                    page::list_footer("group-more-topics", text, None, cx)
                        .cursor_pointer()
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.show_all = !this.show_all;
                            cx.notify();
                        }))
                } else {
                    page::list_footer(
                        "group-count-topics",
                        fill(c::GROUP_PAGE_COUNT, &[("n", &n.to_string())]),
                        None,
                        cx,
                    )
                };
                let waiting = (on_me > 0).then(|| {
                    page::status_badge(
                        Status::NeedsYou,
                        fill(c::GROUP_PAGE_WAITING_YOU, &[("n", &on_me.to_string())]),
                        cx,
                    )
                });
                let active_body = if n > 0 {
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
                    page::section_header(c::GROUP_PAGE_ACTIVE, Some(n), None, waiting, cx),
                    active_body,
                    false,
                ));
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
        let inspector = self
            .aside_open
            .then(|| self.inspector(&g, load, window, cx));
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
            .children(inspector)
            .into_any_element()
    }
}

/// The 群 commands of the toolbar's `ellipsis` pull-down: the same items as
/// the 群's context menu in the sidebar (§7.3: every context-menu item also
/// exists in the main UI). 静音 or 取消静音; for a 群 also 重命名群…,
/// 添加成员… and, apart, 离开群… (it asks first).
fn group_menu(
    menu: gpui_component::menu::PopupMenu,
    services: &Services,
    group_id: &str,
) -> gpui_component::menu::PopupMenu {
    use crate::views::nav as nav_views;
    let p = &services.provider;
    let Some(g) = p.groups().into_iter().find(|g| g.id == group_id) else {
        return menu;
    };
    let muted = p.group_muted(&g.id);
    let (s, id) = (services.clone(), g.id.clone());
    let menu = menu.item(
        PopupMenuItem::new(if muted {
            nav_copy::UNMUTE
        } else {
            nav_copy::MUTE
        })
        .on_click(move |_, _, _| s.provider.set_group_muted(&id, !muted)),
    );
    if g.kind != crate::data::GroupKind::Group {
        return menu;
    }
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
