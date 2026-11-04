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
    Window, div, prelude::FluentBuilder as _, px,
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
            InspectorSection, NativeTextField, NativeType as _, SheetLayout, SheetWidth,
            ToolbarGroup, ToolbarItemKind, ToolbarRow, borderless_button, emphasized,
            inspector_column, metrics, native_colors, native_sheet, sheet_buttons, toolbar_item,
        },
        sf_symbols::{Sf, Sym, SymSize},
    },
    views::{
        composer::ComposerView,
        shell::{PageView, TitleBarSpec, clock, member_name, panes::watch},
    },
};

use rows::{AVATAR, ROW, RowInk, line, mail_row, state_line};

pub struct GroupPage {
    services: Services,
    aside_open: bool,
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


impl GroupPage {
    /// The toolbar row: the 群's name over its purpose; 产出 and 群设置, then
    /// the inspector toggle.
    fn toolbar(&self, g: &str, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
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
        ToolbarRow::new(group.map(|x| x.name).unwrap_or_default())
            .subtitle(subtitle)
            .leading_inset(rows::toolbar_leading(&self.services, cx))
            .group(ToolbarGroup::new([
                toolbar_item(
                    "group-new-topic",
                    Sf::NewTopic,
                    nav_copy::NEW_TOPIC,
                    ToolbarItemKind::Plain,
                    active,
                    window,
                    cx,
                )
                .on_click(|_, window, cx| {
                    window.dispatch_action(Box::new(crate::app::NewTopic), cx)
                }),
                toolbar_item(
                    "group-outputs",
                    Sf::Outputs,
                    c::GROUP_PAGE_OUTPUTS,
                    ToolbarItemKind::Plain,
                    active,
                    window,
                    cx,
                )
                .on_click(move |_, _, cx| nav.update(cx, |n, cx| n.open_artifacts(cx))),
            ]))
            .group(rows::toolbar_capsule(more, cx))
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
            }))]))
            .into_any_element()
    }

    /// One 进行中 row: the owner, the title, the state as a symbol and text
    /// in its state color, the time; a run shows its step at the end.
    fn topic_row(&self, t: &Topic, st: Standing, run: Option<&BotRunSummary>, last: bool, cx: &App) -> AnyElement {
        let k = native_colors(cx);
        let p = &self.services.provider;
        let owner_id = run
            .map(|r| r.bot_id.clone())
            .or_else(|| t.owner_id.clone())
            .unwrap_or_else(|| t.created_by.clone());
        let owner = p.member(&owner_id);
        let glance = p.topic_glance(&t.group_id, &t.id);
        let owner_name = owner.as_ref().map(|m| m.name.clone()).unwrap_or_default();
        let with_note = |first: String| match &glance.state_note {
            Some(n) => format!("{first} · {n}"),
            None => first,
        };
        let (second, slot): (AnyElement, Option<AnyElement>) = match st {
            Standing::OnMe => {
                let label = match t.status {
                    TopicStatus::WaitingReview => p
                        .topic_changes(&t.group_id, &t.id)
                        .map(|ch| {
                            fill(
                                c::GROUP_PAGE_NEEDS_REVIEW_PR,
                                &[("n", &ch.number.to_string())],
                            )
                        })
                        .unwrap_or_else(|| c::GROUP_PAGE_NEEDS_REVIEW.to_string()),
                    TopicStatus::WaitingAnswer => c::GROUP_PAGE_NEEDS_DECISION.to_string(),
                    _ => c::GROUP_PAGE_NEEDS_ANSWER.to_string(),
                };
                (
                    state_line(
                        Some((Sym::NeedsYou, k.amber, label)),
                        Some(with_note(owner_name)),
                        k.secondary,
                    ),
                    None,
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
                let progress = kit::plan_progress(&r.plan)
                    .map(|(n, m)| rows::progress_slot(n, m, k.blue, k.secondary));
                (state_line(None, Some(parts.join(" · ")), k.secondary), progress)
            }
            Standing::OnOthers => {
                let who = t
                    .waiting_on
                    .iter()
                    .map(|w| member_name(&self.services, w))
                    .collect::<Vec<_>>()
                    .join("、");
                (
                    state_line(
                        Some((
                            Sym::Waiting,
                            k.secondary,
                            fill(c::GROUP_PAGE_WAITING_ON, &[("name", &who)]),
                        )),
                        glance.state_note.clone(),
                        k.secondary,
                    ),
                    None,
                )
            }
            Standing::Other => (
                state_line(
                    None,
                    Some(with_note(fill(
                        c::GROUP_PAGE_ASKED_BY,
                        &[("name", &member_name(&self.services, &t.created_by))],
                    ))),
                    k.secondary,
                ),
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
        mail_row(
            SharedString::from(format!("group-topic-{}", t.id)),
            ROW,
            rows::avatar(owner.as_ref(), AVATAR, cx),
            t.title.clone(),
            // One trailing slot per line: the time on the title line, the
            // progress of a run on the second line.
            Some(rows::time_slot(clock(t.last_activity_at), k.secondary)),
            second,
            slot,
            !last,
            RowInk::new(None, cx),
            cx,
        )
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
                let member = p.member(&d.member_id);
                let nav = self.services.nav.clone();
                let (gid, tid) = (d.group_id.clone(), d.topic_id.clone());
                let mut rest = vec![member.as_ref().map(|m| m.name.clone()).unwrap_or_default()];
                rest.extend(d.note.clone());
                mail_row(
                    SharedString::from(format!("group-done-{i}")),
                    ROW,
                    rows::avatar(member.as_ref(), AVATAR, cx),
                    d.title.clone(),
                    Some(rows::time_slot(kit::month_day(d.at), k.secondary)),
                    state_line(None, Some(rest.join(" · ")), k.secondary),
                    None,
                    i + 1 < n,
                    RowInk::new(None, cx),
                    cx,
                )
                .on_click(move |_, _, cx| {
                    let (gid, tid) = (gid.clone(), tid.clone());
                    nav.update(cx, |nav, cx| nav.open_topic(gid, tid, cx))
                })
                .into_any_element()
            })
            .collect()
    }

    /// The inspector: the 群's info, Contacts-style.
    fn inspector(&self, g: &str, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let k = native_colors(cx);
        let p = self.services.provider.clone();
        let mut col = inspector_column(cx)
            .id("group-inspector")
            .overflow_y_scroll()
            .pt(metrics::TOOLBAR_ROW - px(12.))
            .pb(px(16.));

        // 成员.
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
        for m in members {
            let nav = self.services.nav.clone();
            let id = m.id.clone();
            sec = sec.row(
                div()
                    .id(SharedString::from(format!("group-member-{}", m.id)))
                    .h(metrics::INSPECTOR_ROW)
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .n_body()
                    .child(rows::avatar(Some(&m), px(16.), cx))
                    .child(div().flex_none().text_color(k.label).child(m.name.clone()))
                    .child(
                        line(m.role.clone().unwrap_or_default())
                            .flex_1()
                            .text_color(k.secondary),
                    )
                    .on_click(move |_, _, cx| {
                        let id = id.clone();
                        nav.update(cx, |n, cx| n.open_members(Some(id), cx))
                    }),
            );
        }
        col = col.child(sec);

        // 群里的决定.
        let decisions = p.group_decisions(g);
        let now = p.clock_now();
        let mut sec = InspectorSection::new("group-ins-decisions", c::GROUP_PAGE_DECISIONS);
        if p.can_record_decision(g) {
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
        if decisions.is_empty() {
            sec = sec.row(
                div()
                    .py(px(4.))
                    .n_subheadline()
                    .text_color(k.secondary)
                    .child(c::GROUP_PAGE_DECISIONS_NOTE),
            );
        }
        for d in decisions {
            let who = p.member(&d.by);
            let date = if now - d.at < 60_000 {
                c::GROUP_PAGE_JUST_NOW.to_string()
            } else {
                kit::month_day(d.at)
            };
            sec = sec.row(
                div()
                    .py(px(4.))
                    .flex()
                    .flex_col()
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
        let outputs: Vec<_> = p.group_artifacts(g).into_iter().take(3).collect();
        if !outputs.is_empty() {
            let mut sec = InspectorSection::new("group-ins-outputs", c::GROUP_PAGE_RECENT_OUTPUTS);
            for a in outputs {
                let nav = self.services.nav.clone();
                let id = a.id.clone();
                let glyph = match a.kind {
                    ArtifactKind::Image => Sym::Image,
                    ArtifactKind::File => Sym::File,
                    _ => Sym::Document,
                };
                sec = sec.row(
                    div()
                        .id(SharedString::from(format!("group-output-{}", a.id)))
                        .py(px(4.))
                        .flex()
                        .items_start()
                        .gap(px(8.))
                        .child(div().pt(px(1.)).child(rows::sym(glyph, SymSize::INLINE, k.secondary)))
                        .child(
                            div()
                                .flex_1()
                                .min_w(px(0.))
                                .flex()
                                .flex_col()
                                .child(line(a.title.clone()).n_body().text_color(k.label))
                                .child(line(fill(
                                    c::GROUP_PAGE_ARTIFACT_META,
                                    &[
                                        ("v", &a.version.to_string()),
                                        ("kind", kind_label(a.kind)),
                                        ("name", &member_name(&self.services, &a.created_by)),
                                    ],
                                ))
                                .n_subheadline()
                                .text_color(k.secondary)),
                        )
                        .child(
                            div()
                                .flex_none()
                                .n_subheadline()
                                .n_figures()
                                .text_color(k.secondary)
                                .child(short_time(p.clock_now(), a.updated_at)),
                        )
                        .on_click(move |_, _, cx| {
                            let id = id.clone();
                            nav.update(cx, |nav, cx| nav.open_artifact(id, None, cx))
                        }),
                );
            }
            col = col.child(sec);
        }

        // 在哪干活: the team computer a member of this 群 works on now.
        let run = p
            .topics(g)
            .into_iter()
            .flat_map(|t| p.runs(g, &t.id))
            .filter(|r| r.status.is_active() && r.machine_name.is_some())
            .max_by_key(|r| r.updated_at);
        if let Some(run) = run {
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
            let sec = InspectorSection::new("group-ins-where", c::GROUP_PAGE_WHERE)
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
                        .py(px(4.))
                        .flex()
                        .items_start()
                        .gap(px(8.))
                        .child(div().pt(px(1.)).child(rows::sym(glyph, SymSize::INLINE, k.secondary)))
                        .child(
                            div()
                                .flex_1()
                                .min_w(px(0.))
                                .flex()
                                .flex_col()
                                .child(
                                    div()
                                        .flex()
                                        .items_center()
                                        .gap(px(6.))
                                        .n_body()
                                        .child(line(name).text_color(k.label))
                                        .child(div().flex_none().size(px(6.)).rounded_full().bg(k.blue))
                                        .child(
                                            div()
                                                .flex_none()
                                                .n_subheadline()
                                                .text_color(k.secondary)
                                                .child(c::GROUP_PAGE_IN_USE),
                                        ),
                                )
                                .child(line(step).n_subheadline().text_color(k.secondary)),
                        ),
                );
            col = col.child(sec);
        }
        col.into_any_element()
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
        let topic_rows: Vec<AnyElement> = active
            .iter()
            .enumerate()
            .map(|(i, (st, t, run))| self.topic_row(t, *st, run.as_ref(), i + 1 == n, cx))
            .collect();
        let done = self.done_rows(&g, cx);
        let mut list = div()
            .id("group-main")
            .flex_1()
            .min_h(px(0.))
            .overflow_y_scroll()
            .pt(px(4.))
            .pb(px(20.))
            .flex()
            .flex_col();
        if !topic_rows.is_empty() {
            list = list
                .child(
                    rows::section_header(c::GROUP_PAGE_ACTIVE, Some(n.to_string()), cx).child(
                        div()
                            .when(on_me == 0, |d| d.invisible())
                            .flex()
                            .items_center()
                            .gap(px(3.))
                            .n_body()
                            .text_color(k.amber)
                            .child(rows::sym(Sym::NeedsYou, SymSize::INLINE_SMALL, k.amber))
                            .child(fill(c::GROUP_PAGE_WAITING_YOU, &[("n", &on_me.to_string())])),
                    ),
                )
                .children(topic_rows);
        }
        if !done.is_empty() {
            list = list
                .child(rows::section_header(
                    c::GROUP_PAGE_RECENT_DONE,
                    Some(c::GROUP_PAGE_THIS_WEEK.to_string()),
                    cx,
                ))
                .children(done);
        }
        let inspector = self.aside_open.then(|| self.inspector(&g, window, cx));
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
