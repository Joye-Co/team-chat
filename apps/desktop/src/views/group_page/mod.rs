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

use gpui::{
    AnyElement, App, AppContext as _, Context, Entity, InteractiveElement as _, IntoElement,
    ParentElement, Render, SharedString, StatefulInteractiveElement as _, Styled, Subscription,
    Window, div, px,
};
use gpui_component::{
    WindowExt as _,
    input::{InputEvent, InputState},
    menu::{ContextMenuExt as _, DropdownMenu as _, PopupMenuItem},
};

use crate::{
    app::{Place, RightPane, Services},
    copy::{common, fill, group_page as c, nav as nav_copy},
    data::{ArtifactKind, BotRunSummary, PlanState, Topic, TopicStatus},
    ui::{
        native::{
            InspectorSection, NativeTextField, NativeType as _, PushSize, PushStyle, SheetLayout,
            SheetWidth, ToolbarGroup, ToolbarItemKind, ToolbarRow, borderless_button, emphasized,
            inspector_column, metrics, native_colors, native_sheet,
            page::{self, PageLoad, Status},
            push_button, sheet_buttons, toolbar_item,
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
    pub fn new(services: Services, _composer: Entity<ComposerView>, cx: &mut Context<Self>) -> Self {
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

/// 记一条…: a sheet (§7.1) with one field; Return or 记下 records the
/// decision, Escape or 取消 closes it.
pub fn open_record_sheet(services: Services, group: String, window: &mut Window, cx: &mut App) {
    if window.has_active_dialog(cx) {
        return;
    }
    let input = cx.new(|cx| InputState::new(window, cx).placeholder(c::GROUP_PAGE_RECORD_PLACEHOLDER));
    let save = {
        let (services, group, input) = (services.clone(), group.clone(), input.clone());
        move |window: &mut Window, cx: &mut App| {
            let text = input.read(cx).value().trim().to_string();
            if text.is_empty() {
                return;
            }
            let _ = services.provider.record_decision(&group, text);
            window.close_dialog(cx);
        }
    };
    let on_enter = save.clone();
    let sub = window.subscribe(&input, cx, move |_, e: &InputEvent, window, cx| {
        if let InputEvent::PressEnter { .. } = e {
            on_enter(window, cx);
        }
    });
    let first = std::cell::Cell::new(true);
    window.open_dialog(cx, move |dialog, window, cx| {
        let _keep = &sub;
        if first.replace(false) {
            input.update(cx, |s, cx| s.focus(window, cx));
        }
        let [cancel, ok] = sheet_buttons(
            "group-record",
            common::CANCEL,
            c::GROUP_PAGE_SAVE,
            cx,
        );
        let save = save.clone();
        native_sheet(dialog, SheetWidth::Form, cx).child(
            SheetLayout::new(c::GROUP_PAGE_RECORD_TITLE)
                .explanation(c::GROUP_PAGE_DECISIONS_NOTE)
                .body(
                    NativeTextField::new("group-record-field", &input)
                        .width(metrics::SHEET_WIDTH - metrics::SHEET_PAD * 2.),
                )
                .buttons([
                    cancel.on_click(|_, window, cx| window.close_dialog(cx)),
                    ok.on_click(move |_, window, cx| save(window, cx)),
                ]),
        )
    });
}

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
        let k = native_colors(cx);
        let p = &self.services.provider;
        let group = p.groups().into_iter().find(|x| x.id == g);
        let n = group.as_ref().map(|x| x.member_ids.len()).unwrap_or(0);
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
        let new_topic = push_button(
            "group-new-topic",
            c::GROUP_PAGE_NEW_TOPIC,
            PushStyle::Default,
            PushSize::Small,
            cx,
        )
        .on_click(|_, window, cx| window.dispatch_action(Box::new(crate::app::NewTopic), cx));
        div()
            .flex_none()
            .border_b_1()
            .border_color(k.separator)
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
                    .on_click(move |_, _, cx| nav.update(cx, |n, cx| n.open_artifacts(cx)))]))
                    .group(rows::toolbar_capsule(more, cx))
                    .group(new_topic)
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
    fn topic_row(&self, t: &Topic, st: Standing, run: Option<&BotRunSummary>, last: bool, cx: &App) -> AnyElement {
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
        let (glyph, second, badge): (Sym, String, Option<AnyElement>) = match st {
            Standing::OnMe => {
                let (label, pr) = match t.status {
                    TopicStatus::WaitingReview => (
                        c::GROUP_PAGE_STATUS_REVIEW,
                        p.topic_changes(&t.group_id, &t.id).map(|ch| {
                            fill(crate::copy::desk::DESK_PROOF_PR, &[("n", &ch.number.to_string())])
                        }),
                    ),
                    TopicStatus::WaitingAnswer => (c::GROUP_PAGE_STATUS_DECIDE, None),
                    _ => (c::GROUP_PAGE_STATUS_ANSWER, None),
                };
                let mut parts: Vec<String> = pr.into_iter().collect();
                parts.push(with_note(owner_name));
                (
                    Sym::NeedsYou,
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
                    parts.join(" · "),
                    Some(page::status_badge(Status::Working, text, cx)),
                )
            }
            Standing::OnOthers => {
                let text = match t.waiting_on.as_slice() {
                    [one] => fill(
                        c::GROUP_PAGE_STATUS_WAITING,
                        &[("name", &member_name(&self.services, one))],
                    ),
                    many => fill(
                        c::GROUP_PAGE_STATUS_WAITING_MANY,
                        &[("n", &many.len().to_string())],
                    ),
                };
                (
                    Sym::Waiting,
                    glance.state_note.clone().unwrap_or(owner_name),
                    Some(page::status_badge(Status::Queued, text, cx)),
                )
            }
            Standing::Other => (
                Sym::Topic,
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
            media: Some(page::icon_box(glyph, Some(k.label), cx)),
            second: Some(line(second).into_any_element()),
            status: badge,
            time: Some(clock(t.last_activity_at).into()),
            separator: !last,
            ..page::PageRow::new(t.title.clone())
        }
        .render(SharedString::from(format!("group-topic-{}", t.id)), COLUMNS, cx)
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
            menu.item(PopupMenuItem::new(c::GROUP_PAGE_OPEN_TOPIC).on_click(move |_, _, cx| {
                let (g, t) = (g.clone(), t.clone());
                nav.update(cx, |n, cx| n.open_topic(g, t, cx))
            }))
        })
        .into_any_element()
    }

    fn done_rows(&self, g: &str, cx: &App) -> Vec<AnyElement> {
        let k = native_colors(cx);
        let p = &self.services.provider;
        let done: Vec<_> = p.group_recent_done(g).into_iter().take(5).collect();
        let n = done.len();
        done.into_iter()
            .enumerate()
            .map(|(i, d)| {
                let nav = self.services.nav.clone();
                let (gid, tid) = (d.group_id.clone(), d.topic_id.clone());
                let mut rest = vec![member_name(&self.services, &d.member_id)];
                rest.extend(d.note.clone());
                page::PageRow {
                    media: Some(page::icon_box(Sym::Done, Some(k.label), cx)),
                    second: Some(line(rest.join(" · ")).into_any_element()),
                    time: Some(kit::month_day(d.at).into()),
                    separator: i + 1 < n,
                    ..page::PageRow::new(d.title.clone())
                }
                .render(SharedString::from(format!("group-done-{i}")), COLUMNS, cx)
                .on_click(move |_, _, cx| {
                    let (gid, tid) = (gid.clone(), tid.clone());
                    nav.update(cx, |nav, cx| nav.open_topic(gid, tid, cx))
                })
                .into_any_element()
            })
            .collect()
    }

    /// The right pane (§2.5): the header band `群资料`, then 成员, 群里的决定,
    /// 最近的产出 and 在哪干活, each with a compact empty when it has nothing.
    /// `loading` draws skeleton rows instead.
    fn inspector(&self, g: &str, loading: bool, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let k = native_colors(cx);
        let p = self.services.provider.clone();
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
            .min_h(px(0.))
            .overflow_y_scroll()
            .pt(px(4.))
            .pb(px(24.))
            .flex()
            .flex_col();

        // 成员: the name keeps its room; the 岗位 truncates first, and both
        // show in full in the tooltip.
        let ids = p
            .groups()
            .into_iter()
            .find(|x| x.id == g)
            .map(|x| x.member_ids)
            .unwrap_or_default();
        let members: Vec<_> = ids
            .iter()
            .filter_map(|id| p.member(id))
            .filter(|m| !m.deactivated)
            .collect();
        let mut sec = InspectorSection::new("group-ins-members", c::GROUP_PAGE_MEMBERS_TITLE);
        if loading {
            sec = sec.row(page::skeleton_rows("group-sk-m", page::SkeletonKind::Compact, 4, cx));
        } else if members.is_empty() {
            sec = sec.row(page::compact_empty(Sym::Members, c::GROUP_PAGE_MEMBERS_EMPTY, None, cx));
        }
        for m in members.into_iter().filter(|_| !loading) {
            let nav = self.services.nav.clone();
            let id = m.id.clone();
            let full = match &m.role {
                Some(r) if !r.is_empty() => format!("{} · {r}", m.name),
                _ => m.name.clone(),
            };
            sec = sec.row(
                div()
                    .id(SharedString::from(format!("group-member-{}", m.id)))
                    .h(px(32.))
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .n_body()
                    .child(rows::avatar(Some(&m), px(16.), cx))
                    .child(
                        line(m.name.clone())
                            .flex_none()
                            .max_w(px(160.))
                            .text_color(k.label),
                    )
                    .child(
                        line(m.role.clone().unwrap_or_default())
                            .flex_1()
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
        let decisions = if loading { Vec::new() } else { p.group_decisions(g) };
        let now = p.clock_now();
        let mut sec = InspectorSection::new("group-ins-decisions", c::GROUP_PAGE_DECISIONS);
        if p.can_record_decision(g) && !loading {
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
            sec = sec.row(page::skeleton_rows("group-sk-d", page::SkeletonKind::Compact, 2, cx));
        } else if decisions.is_empty() {
            sec = sec.row(page::compact_empty(Sym::Check, c::GROUP_PAGE_DECISIONS_NOTE, None, cx));
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
                    .py(px(6.))
                    .flex()
                    .flex_col()
                    .gap(px(2.))
                    .child(div().n_body().text_color(k.label).child(d.text.clone()))
                    .child(
                        div()
                            .n_subheadline()
                            .n_figures()
                            .text_color(k.secondary)
                            .child(format!(
                                "{} · {date}",
                                who.map(|m| m.name).unwrap_or_default()
                            )),
                    ),
            );
        }
        col = col.child(sec);

        // 最近的产出.
        let outputs: Vec<_> = if loading {
            Vec::new()
        } else {
            p.group_artifacts(g).into_iter().take(3).collect()
        };
        let mut sec = InspectorSection::new("group-ins-outputs", c::GROUP_PAGE_RECENT_OUTPUTS);
        if loading {
            sec = sec.row(page::skeleton_rows("group-sk-o", page::SkeletonKind::Compact, 2, cx));
        } else if outputs.is_empty() {
            sec = sec.row(page::compact_empty(Sym::Document, c::GROUP_PAGE_OUTPUTS_EMPTY, None, cx));
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
                    .py(px(6.))
                    .flex()
                    .items_start()
                    .gap(px(8.))
                    .child(div().pt(px(2.)).child(page::row_icon(glyph, k.secondary)))
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(0.))
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
        let run = if loading {
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
                sec = sec.row(page::skeleton_rows("group-sk-w", page::SkeletonKind::Compact, 1, cx));
            }
            None => {
                sec = sec.row(page::compact_empty(Sym::Computer, c::GROUP_PAGE_WHERE_EMPTY, None, cx));
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
                let glyph = if name.to_lowercase().contains("mini") {
                    Sym::MacMini
                } else {
                    Sym::Computer
                };
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
                            .py(px(6.))
                            .flex()
                            .items_center()
                            .gap(px(8.))
                            .child(page::row_icon(glyph, k.secondary))
                            .child(
                                div()
                                    .flex_1()
                                    .min_w(px(0.))
                                    .flex()
                                    .flex_col()
                                    .child(line(name).n_body().text_color(k.label))
                                    .child(line(step).n_subheadline().text_color(k.secondary)),
                            )
                            .child(page::status_badge(Status::Working, c::GROUP_PAGE_IN_USE, cx))
                            .tooltip(move |window, cx| {
                                gpui_component::tooltip::Tooltip::new(tip.clone()).build(window, cx)
                            }),
                    );
            }
        }
        col = col.child(sec);
        inspector_column(cx).child(header).child(col).into_any_element()
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
        let toolbar = self.toolbar(&g, window, cx);
        let done = self.done_rows(&g, cx);
        let load = PageLoad::of(&self.services, n > 0 || !done.is_empty() || !p.topics(&g).is_empty());
        let shown = if self.show_all { n } else { n.min(ACTIVE_ROWS) };
        let topic_rows: Vec<AnyElement> = active
            .iter()
            .take(shown)
            .enumerate()
            .map(|(i, (st, t, run))| self.topic_row(t, *st, run.as_ref(), i + 1 == shown, cx))
            .collect();
        let mut content = div()
            .px(page::m::EDGE)
            .pt(page::m::GUTTER)
            .pb(page::m::GUTTER)
            .flex()
            .flex_col()
            .gap(page::m::GUTTER);
        let section = |header: gpui::Div, body: gpui::Div| {
            div()
                .flex()
                .flex_col()
                .gap(page::m::HEADER_GAP)
                .child(header)
                .child(body)
        };
        match load {
            PageLoad::Loading | PageLoad::Failed => {
                if load == PageLoad::Failed {
                    content = content.child(page::load_failed("group", &self.services, cx));
                }
                content = content.child(section(
                    page::section_header(c::GROUP_PAGE_ACTIVE, None, None, None, cx),
                    div().child(page::skeleton_rows("group-sk", page::SkeletonKind::Row, 4, cx)),
                ));
            }
            PageLoad::Ready if n == 0 && done.is_empty() => {
                content = content.child(
                    div()
                        .min_h(px(360.))
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
                if n > 0 {
                    let footer = (n > ACTIVE_ROWS).then(|| {
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
                    });
                    let waiting = (on_me > 0).then(|| {
                        page::status_badge(
                            Status::NeedsYou,
                            fill(c::GROUP_PAGE_WAITING_YOU, &[("n", &on_me.to_string())]),
                            cx,
                        )
                    });
                    content = content.child(section(
                        page::section_header(c::GROUP_PAGE_ACTIVE, Some(n), None, waiting, cx),
                        page::frame(cx).children(topic_rows).children(footer),
                    ));
                }
                if !done.is_empty() {
                    content = content.child(section(
                        page::section_header(
                            c::GROUP_PAGE_RECENT_DONE,
                            Some(done.len()),
                            None,
                            None,
                            cx,
                        ),
                        page::frame(cx).children(done),
                    ));
                }
            }
        }
        let list = div()
            .id("group-main")
            .flex_1()
            .min_h(px(0.))
            .overflow_y_scroll()
            .child(content);
        let inspector = self
            .aside_open
            .then(|| self.inspector(&g, load != PageLoad::Ready, window, cx));
        div()
            .size_full()
            .flex()
            .bg(k.content)
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.))
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
        PopupMenuItem::new(if muted { nav_copy::UNMUTE } else { nav_copy::MUTE })
            .on_click(move |_, _, _| s.provider.set_group_muted(&id, !muted)),
    );
    if g.kind != crate::data::GroupKind::Group {
        return menu;
    }
    let (rename, add, leave) = (services.clone(), services.clone(), services.clone());
    let (g1, g2, g3) = (g.clone(), g.clone(), g);
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
    .item(
        PopupMenuItem::new(nav_copy::LEAVE_GROUP).on_click(move |_, window, cx| {
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
