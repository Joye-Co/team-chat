//! 群资料: the 详情 of a 群聊 (its right pane, ⌘⌥I), as the inspector is a
//! 话题's: 成员 first (who is in this 群, 添加成员…), then 通知, 群里的决定
//! (记一条… opens a sheet), 最近的产出 and 在哪干活. The shell draws its
//! header band (`群资料` and close).

use gpui::{
    AnyElement, App, Context, InteractiveElement as _, IntoElement, ParentElement, Render,
    SharedString, StatefulInteractiveElement as _, Styled, Subscription, Window, div,
    prelude::FluentBuilder as _,
};

use super::{kind_label, kit, open_record_sheet, rows, rows::line, short_time};
use crate::{
    app::{RightPane, Services},
    copy::{fill, group_page as c},
    data::{ArtifactKind, GroupKind, Member, PlanState},
    ui::{
        native::{
            FocusRow as _, InspectorSection, NativeType as _, borderless_button, native_colors,
            page::{self, PageLoad, Status},
            tokens::space as tsp,
        },
        sf_symbols::{Sf, Sym},
    },
    views::shell::{PaneView, member_name, panes::watch},
};

/// The members 群资料 lists (and the 群 page's subtitle counts): known, not
/// deactivated; people first, then bots, each by name.
pub fn listed_members(services: &Services, g: &str) -> Vec<Member> {
    let p = &services.provider;
    let mut members: Vec<Member> = p
        .groups()
        .into_iter()
        .find(|x| x.id == g)
        .map(|x| x.member_ids)
        .unwrap_or_default()
        .iter()
        .filter_map(|id| p.member(id))
        .filter(|m| !m.deactivated)
        .collect();
    members.sort_by(|a, b| (a.is_bot(), &a.name).cmp(&(b.is_bot(), &b.name)));
    members
}

/// The 群资料 pane beside a 群聊.
pub struct GroupInfoPane {
    services: Services,
    _subscriptions: Vec<Subscription>,
}

impl GroupInfoPane {
    pub fn new(services: Services, cx: &mut Context<Self>) -> Self {
        Self {
            _subscriptions: watch(&services, cx),
            services,
        }
    }
}

impl PaneView for GroupInfoPane {
    fn width(&self, _window_w: gpui::Pixels) -> gpui::Pixels {
        crate::ui::native::metrics::INSPECTOR_WIDTH
    }
}

impl Render for GroupInfoPane {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let k = native_colors(cx);
        let g = self.services.nav.read(cx).chat_open().cloned();
        let body = g.map(|g| {
            // The cache has only me before the first sync.
            let has = listed_members(&self.services, &g).len() > 1;
            let load = PageLoad::of(&self.services, has);
            sections(&self.services, &g, load, window, cx)
        });
        div().size_full().bg(k.content).children(body)
    }
}

/// 群资料 (§2.5): 成员 first, then 通知, 群里的决定, 最近的产出 and 在哪干活,
/// each with a compact empty when it has nothing. `loading` draws skeleton
/// rows instead.
pub fn sections(
    services: &Services,
    g: &str,
    load: PageLoad,
    window: &mut Window,
    cx: &mut App,
) -> AnyElement {
    let k = native_colors(cx);
    let p = services.provider.clone();
    // Skeletons only while the page loads (§2.14); a failed load shows
    // one compact line per section, never pulsing bones (§2.15).
    let loading = load == PageLoad::Loading;
    let failed = load == PageLoad::Failed;
    let offline = |sym: Sym, cx: &App| page::compact_empty(sym, c::GROUP_PAGE_OFFLINE, None, cx);
    let mut col = div()
        .id("group-info")
        .size_full()
        .overflow_y_scroll()
        .pb(tsp::S24)
        .flex()
        .flex_col()
        // Pull the first GroupLabel up: the section's own 16 top sits under
        // the pane's header band.
        .child(div().flex_none().mt(-tsp::S12));

    // 成员: the name keeps its room; the 岗位 truncates first, and both
    // show in full in the tooltip.
    let members = listed_members(services, g);
    let mut sec = InspectorSection::new("group-ins-members", c::GROUP_PAGE_MEMBERS_TITLE);
    let group = p.groups().into_iter().find(|x| x.id == g);
    if let Some(group) = group.filter(|x| x.kind == GroupKind::Group && !loading && !failed) {
        let services = services.clone();
        sec = sec.action(
            borderless_button(
                "group-add-members",
                Sf::Plus,
                crate::copy::nav::ADD_MEMBERS,
                window,
                cx,
            )
            .on_click(move |_, window, cx| {
                cx.stop_propagation();
                crate::views::nav::dialogs::open_add_members(
                    services.clone(),
                    group.id.clone(),
                    group.name.clone(),
                    window,
                    cx,
                )
            }),
        );
    }
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
        let nav = services.nav.clone();
        let id = m.id.clone();
        let full = match &m.role {
            Some(r) if !r.is_empty() => format!("{} · {r}", m.name),
            _ => m.name.clone(),
        };
        sec = sec.row(
            div()
                .id(SharedString::from(format!("group-member-{}", m.id)))
                .focus_row(false, cx)
                .h(tsp::S32)
                .flex()
                .items_center()
                .gap(tsp::S8)
                .n_body()
                .child(rows::avatar(Some(&m), tsp::S16, cx))
                // §2.21: the role truncates first; it shows only when
                // a few glyphs of it fit beside the whole name (never a
                // lone "…"), else the name takes the row. Both are in
                // the tip.
                .map(|d| {
                    let name = crate::ui::avatar::name_beside_avatar(&m.name).to_string();
                    if crate::views::artifact_page::role_fits(&name) {
                        d.child(
                            line(name)
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
                    } else {
                        d.child(line(name).flex_1().min_w_0().text_color(k.label))
                    }
                })
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

    // 通知: how this 群 reminds me (静音, 提醒); it is this 群's own
    // setting, so it sits here and not in the settings window.
    if !loading && !failed {
        col = col.child(crate::views::nav::notify::group_section(services, g, cx));
    }

    // 群里的决定.
    let decisions = if loading || failed {
        Vec::new()
    } else {
        p.group_decisions(g)
    };
    let now = p.clock_now();
    let mut sec = InspectorSection::new("group-ins-decisions", c::GROUP_PAGE_DECISIONS);
    if p.can_record_decision(g) && !loading && !failed {
        let services = services.clone();
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
            let services = services.clone();
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
                            who.map(|m| crate::ui::avatar::name_beside_avatar(&m.name).to_string())
                                .unwrap_or_default()
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
        let nav = services.nav.clone();
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
                .focus_row(false, cx)
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
                                    (
                                        "name",
                                        crate::ui::avatar::name_beside_avatar(&member_name(
                                            services,
                                            &a.created_by,
                                        )),
                                    ),
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
            .filter(|r| r.status.is_active())
            // A run that names its computer first; a run with no known
            // computer still shows (未知电脑), never "no computer" while
            // a member works (A5).
            .max_by_key(|r| (r.machine_name.is_some(), r.updated_at))
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
            let who = crate::ui::avatar::name_beside_avatar(&member_name(services, &run.bot_id))
                .to_string();
            let users = fill(c::GROUP_PAGE_MACHINE_USERS, &[("names", &who)]);
            let known = run.machine_name.is_some();
            let step = run
                .plan
                .iter()
                .find(|s| s.state == PlanState::Current)
                .map(|s| format!("{who} {}", s.text))
                .unwrap_or_else(|| who.clone());
            let nav = services.nav.clone();
            let (gid, tid, rid) = (run.group_id.clone(), run.topic_id.clone(), run.id.clone());
            let name = run
                .machine_name
                .clone()
                .unwrap_or_else(|| c::GROUP_PAGE_UNKNOWN_MACHINE.to_string());
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
                        .focus_row(false, cx)
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
                                .child(line(users).n_subheadline().text_color(k.secondary)),
                        )
                        // §2.12: a known computer is 在线 (the run's own
                        // state is on its 话题 row); 未知电脑 has no
                        // presence badge (B1-44, one recipe with 首页).
                        .when(known, |el| {
                            el.child(page::status_badge(Status::Online, c::GROUP_PAGE_IN_USE, cx))
                        })
                        .tooltip(move |window, cx| {
                            gpui_component::tooltip::Tooltip::new(tip.clone()).build(window, cx)
                        }),
                );
        }
    }
    col = col.child(sec);
    col.into_any_element()
}
