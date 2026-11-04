//! 成员 in the main window (Members.dc.html): the list with its filters and
//! sections (等你, 在做事, 空闲, 同事), one member's detail (现在, 这周,
//! 最近交付, 最近的问题) and the 工作环境 side column (电脑, 需要你批准的事,
//! 能用的工具, 用到的决定). Stats come from the runs cache and
//! `BotRun.usage`; 添加 and 修改 reuse the settings forms.

pub mod scenes;

use std::collections::HashSet;

use gpui::{
    AnyElement, App, Context, InteractiveElement as _, IntoElement, ParentElement, Render,
    SharedString, StatefulInteractiveElement as _, Styled, Subscription, Window, div,
    prelude::FluentBuilder as _, px,
};
use gpui_component::{
    Disableable as _,
    menu::{DropdownMenu as _, PopupMenuItem},
};

use crate::{
    app::{Place, RightPane, Services},
    copy::{fill, members_page as c},
    data::{
        BotRunSummary, GroupKind, Member, PlanState,
        pages::{DeliveryKind, DeliveryState, MemberExtras},
        v2::{Currency, MemberStats},
    },
    theme::{self, Type as _, space},
    ui::{self, Lucide, MemberAvatar, Ring},
    views::{
        group_page::kit::{self, META, SMALL, TINY},
        shell::{PageView, TitleBarSpec, clock, member_name, panes::watch, stub_body},
    },
};

/// A member row's name never gets narrower than this (its first characters).
const NAME_MIN: gpui::Pixels = px(40.);
/// The 岗位 shrinks this much faster than the name: it gives way first.
const ROLE_SHRINK: f32 = 1000.;

/// The list filter.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Filter {
    All,
    Working,
    Waiting,
}

/// The list section a member falls in.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Bucket {
    Waiting,
    Working,
    Idle,
    People,
}

pub struct MembersPage {
    services: Services,
    filter: Filter,
    aside_open: bool,
    retried: HashSet<String>,
    _subscriptions: Vec<Subscription>,
}

/// What the list knows of one member.
struct Entry {
    member: Member,
    bucket: Bucket,
    run: Option<BotRunSummary>,
    ask: Option<String>,
    stats: MemberStats,
    extras: MemberExtras,
}

impl MembersPage {
    pub fn new(services: Services, cx: &mut Context<Self>) -> Self {
        Self {
            _subscriptions: watch(&services, cx),
            services,
            filter: Filter::All,
            aside_open: true,
            retried: HashSet::new(),
        }
    }

    fn selected(&self, cx: &App) -> Option<String> {
        match &self.services.nav.read(cx).place {
            Some(Place::Members(m)) => m.clone(),
            _ => None,
        }
    }

    fn entries(&self) -> Vec<Entry> {
        let p = &self.services.provider;
        let me = self.services.me();
        let runs = p.active_runs();
        let asks = p.asks_for_me();
        let mut out: Vec<Entry> = p
            .members()
            .into_iter()
            .filter(|m| !m.deactivated)
            .map(|m| {
                let run = runs.iter().find(|r| r.bot_id == m.id).cloned();
                let ask = asks
                    .iter()
                    .filter(|a| a.from == m.id)
                    .max_by_key(|a| a.at)
                    .map(|a| a.title.clone());
                let bucket = if !m.is_bot() {
                    Bucket::People
                } else if run.is_some() {
                    Bucket::Working
                } else if ask.is_some() {
                    Bucket::Waiting
                } else {
                    Bucket::Idle
                };
                Entry {
                    stats: p.member_stats(&m.id).unwrap_or_default(),
                    extras: p.member_extras(&m.id),
                    member: m,
                    bucket,
                    run,
                    ask,
                }
            })
            .collect();
        // Me last among the people (Members.dc.html).
        out.sort_by_key(|e| (e.bucket, Some(&e.member.id) == me.as_ref()));
        out
    }
}

impl PageView for MembersPage {
    fn title_bar(&self, _cx: &App) -> TitleBarSpec {
        TitleBarSpec::page(c::MEMBERS_PAGE_TITLE)
    }

    fn draws_own_header(&self, _cx: &App) -> bool {
        true
    }
}

fn count_word(n: usize) -> String {
    c::MEMBERS_COUNT_WORDS
        .split(' ')
        .nth(n)
        .map(str::to_string)
        .unwrap_or_else(|| n.to_string())
}

/// Cents in the currency the data has, never converted (`$0.85`, `¥6.20`).
fn money(cents: u64, currency: Currency) -> String {
    crate::views::topic_rail::money_text(cents, currency)
}

impl MembersPage {
    fn list(
        &self,
        entries: &[Entry],
        selected: Option<&str>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let k = theme::colors(cx);
        let me = self.services.me();
        let working = entries
            .iter()
            .filter(|e| e.bucket == Bucket::Working)
            .count();
        let waiting = entries
            .iter()
            .filter(|e| e.bucket == Bucket::Waiting)
            .count();
        let tab = |id: &'static str, label: &str, n: usize, f: Filter, cx: &mut Context<Self>| {
            let on = self.filter == f;
            div()
                .id(id)
                .flex()
                .items_center()
                .gap(space::S6)
                .h(px(28.))
                .px(space::S10)
                .rounded(px(7.))
                .cursor_pointer()
                .t_small()
                .when(on, |el| el.bg(k.sel).medium().text_color(k.ink))
                .when(!on, |el| el.text_color(k.ink_2).hover(|s| s.bg(k.hover)))
                .child(label.to_string())
                .child(
                    div()
                        .mono()
                        .type_role(META)
                        .font_weight(gpui::FontWeight::NORMAL)
                        .text_color(k.ink_3)
                        .child(n.to_string()),
                )
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.filter = f;
                    cx.notify();
                }))
        };
        let services = self.services.clone();
        let owner = me
            .as_deref()
            .and_then(|m| self.services.provider.member(m))
            .is_some_and(|m| m.level == crate::data::MemberLevel::Owner);
        let add = ui::button(
            "members-add",
            c::MEMBERS_ADD,
            ui::ButtonKind::Secondary,
            ui::ButtonSize::Sm,
            cx,
        )
        .icon(gpui_component::Icon::new(Lucide::Plus))
        .disabled(!owner)
        .dropdown_menu_with_anchor(gpui::Anchor::TopRight, move |menu, _, _| {
            let invite = services.clone();
            let bot = services.clone();
            menu.item(
                PopupMenuItem::new(c::MEMBERS_INVITE)
                    .icon(gpui_component::Icon::new(Lucide::UserPlus))
                    .on_click(move |_, window, cx| {
                        crate::views::settings::open_invite(invite.clone(), window, cx);
                    }),
            )
            .item(
                PopupMenuItem::new(c::MEMBERS_ADD_BOT)
                    .icon(gpui_component::Icon::new(Lucide::Bot))
                    .on_click(move |_, window, cx| {
                        crate::views::settings::open_add_bot(bot.clone(), window, cx);
                    }),
            )
        });
        let shown = |b: Bucket| match self.filter {
            Filter::All => true,
            Filter::Working => b == Bucket::Working,
            Filter::Waiting => b == Bucket::Waiting,
        };
        let mut rows: Vec<AnyElement> = Vec::new();
        let mut ix = 0;
        for bucket in [
            Bucket::Waiting,
            Bucket::Working,
            Bucket::Idle,
            Bucket::People,
        ] {
            if !shown(bucket) {
                continue;
            }
            let group: Vec<&Entry> = entries.iter().filter(|e| e.bucket == bucket).collect();
            if group.is_empty() {
                continue;
            }
            let label = match bucket {
                Bucket::Waiting => c::MEMBERS_SECTION_WAITING,
                Bucket::Working => c::MEMBERS_SECTION_WORKING,
                Bucket::Idle => c::MEMBERS_SECTION_IDLE,
                Bucket::People => c::MEMBERS_SECTION_PEOPLE,
            };
            rows.push(
                div()
                    .flex()
                    .items_center()
                    .gap(space::S8)
                    .h(px(28.))
                    .px(space::S10)
                    .mt(space::S4)
                    .type_role(META)
                    .medium()
                    .text_color(k.ink_3)
                    .when(bucket == Bucket::Waiting, |el| {
                        el.child(kit::dot(k.you, 6.))
                    })
                    .child(label)
                    .child(
                        div()
                            .mono()
                            .font_weight(gpui::FontWeight::NORMAL)
                            .child(group.len().to_string()),
                    )
                    .into_any_element(),
            );
            for e in group {
                ix += 1;
                rows.push(self.row(ix, e, selected, me.as_deref(), cx));
            }
        }
        let hidden = match self.filter {
            Filter::All => 0,
            _ => entries.iter().filter(|e| !shown(e.bucket)).count(),
        };
        div()
            .id("members-list")
            .flex_none()
            .w(kit::MEMBER_LIST_WIDTH)
            .h_full()
            .overflow_y_scroll()
            .border_r_1()
            .border_color(k.line)
            .px(space::S10)
            .pb(space::S12)
            .child(
                kit::bar()
                    .pl(space::S10.max(kit::leading(&self.services, cx)))
                    .pr(space::S6)
                    .child(kit::page_title(c::MEMBERS_PAGE_TITLE, cx))
                    .child(
                        div()
                            .mono()
                            .type_role(META)
                            .text_color(k.ink_3)
                            .child(entries.len().to_string()),
                    )
                    .child(div().flex_1())
                    .child(add),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(2.))
                    .px(space::S10)
                    .pb(space::S4)
                    .child(tab(
                        "members-all",
                        c::MEMBERS_FILTER_ALL,
                        entries.len(),
                        Filter::All,
                        cx,
                    ))
                    .child(tab(
                        "members-working",
                        c::MEMBERS_FILTER_WORKING,
                        working,
                        Filter::Working,
                        cx,
                    ))
                    .child(tab(
                        "members-waiting",
                        c::MEMBERS_FILTER_WAITING,
                        waiting,
                        Filter::Waiting,
                        cx,
                    )),
            )
            .children(rows)
            .when(hidden > 0, |el| {
                el.child(
                    div()
                        .mt(space::S12)
                        .mx(space::S10)
                        .pt(space::S12)
                        .border_t_1()
                        .border_color(k.line)
                        .flex()
                        .items_center()
                        .gap(space::S8)
                        .type_role(META)
                        .text_color(k.ink_3)
                        .child(fill(c::MEMBERS_HIDDEN, &[("n", &hidden.to_string())]))
                        .child(div().flex_1())
                        .child(
                            kit::text_link("members-see-all", c::MEMBERS_SEE_ALL, cx).on_click(
                                cx.listener(|this, _, _, cx| {
                                    this.filter = Filter::All;
                                    cx.notify();
                                }),
                            ),
                        ),
                )
            })
            .into_any_element()
    }

    fn row(
        &self,
        ix: usize,
        e: &Entry,
        selected: Option<&str>,
        me: Option<&str>,
        cx: &App,
    ) -> AnyElement {
        let k = theme::colors(cx);
        let p = &self.services.provider;
        let m = &e.member;
        let is_selected = selected == Some(m.id.as_str());
        let away = !m.is_bot() && !m.online;
        let ring = match e.bucket {
            Bucket::Waiting => Some(Ring::You),
            Bucket::Working => Some(Ring::Live),
            _ => None,
        };
        let week = if e.stats.topics_this_week == 0 {
            c::MEMBERS_WEEK_NONE.to_string()
        } else {
            fill(
                c::MEMBERS_WEEK_TOPICS,
                &[("n", &e.stats.topics_this_week.to_string())],
            )
        };
        let line: AnyElement = match e.bucket {
            Bucket::Waiting => div()
                .flex()
                .min_w_0()
                .child(
                    div()
                        .flex_none()
                        .text_color(k.you_ink)
                        .child(c::MEMBERS_WAITS_ON_YOU),
                )
                .child(div().min_w_0().one_line().child(format!(
                    " · {}",
                    e.extras.line.clone().or(e.ask.clone()).unwrap_or_default()
                )))
                .into_any_element(),
            Bucket::Working => {
                let run = e.run.as_ref();
                let topic = run
                    .and_then(|r| p.topic(&r.group_id, &r.topic_id))
                    .map(|t| t.title)
                    .unwrap_or_default();
                let step = e.extras.line.clone().or_else(|| {
                    run.and_then(|r| {
                        r.plan
                            .iter()
                            .find(|s| s.state == PlanState::Current)
                            .map(|s| s.text.clone())
                    })
                });
                let progress = run.and_then(|r| kit::plan_progress(&r.plan));
                div()
                    .flex()
                    .min_w_0()
                    .child(div().min_w_0().one_line().child(match step {
                        Some(s) => format!("{topic} · {s} · "),
                        None => format!("{topic} · "),
                    }))
                    .children(
                        progress.map(|(n, m)| div().flex_none().mono().child(format!("{n}/{m}"))),
                    )
                    .into_any_element()
            }
            Bucket::Idle => div()
                .min_w_0()
                .one_line()
                .child(
                    e.extras
                        .line
                        .clone()
                        .unwrap_or_else(|| c::MEMBERS_IDLE.to_string()),
                )
                .into_any_element(),
            Bucket::People => div()
                .min_w_0()
                .one_line()
                .when(away, |el| el.text_color(k.ink_3))
                .child(e.extras.line.clone().unwrap_or_else(|| {
                    if m.online {
                        c::MEMBERS_ONLINE.to_string()
                    } else {
                        c::MEMBERS_OFFLINE.to_string()
                    }
                }))
                .into_any_element(),
        };
        let nav = self.services.nav.clone();
        let id = m.id.clone();
        let row = div()
            .id(SharedString::from(format!("member-{}", m.id)))
            .flex()
            .items_center()
            .gap(space::S12)
            .h(px(48.))
            .px(space::S10)
            .rounded(px(9.))
            .cursor_pointer()
            .when(is_selected, |el| el.bg(k.sel))
            .when(!is_selected, |el| el.hover(|s| s.bg(k.hover)))
            .child(
                MemberAvatar::new(Some(m), px(28.))
                    .ring(ring)
                    .on(if is_selected { k.sel } else { k.bg }),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .child(
                        div()
                            .flex()
                            .items_baseline()
                            .gap(space::S8)
                            .min_w_0()
                            // The 岗位 gives way first, down to nothing; then the
                            // name ends in an ellipsis and keeps its first
                            // characters.
                            .child(
                                div()
                                    .flex_shrink(1.)
                                    .min_w(NAME_MIN)
                                    .one_line()
                                    .t_small()
                                    .medium()
                                    .text_color(if away { k.ink_2 } else { k.ink })
                                    .child(m.name.clone()),
                            )
                            .children(m.role.clone().map(|r| {
                                div()
                                    .flex_shrink(ROLE_SHRINK)
                                    .min_w_0()
                                    .one_line()
                                    .type_role(META)
                                    .text_color(k.ink_3)
                                    .child(r)
                            }))
                            .when(me == Some(m.id.as_str()), |el| {
                                el.child(
                                    div()
                                        .flex_none()
                                        .type_role(META)
                                        .text_color(k.ink_3)
                                        .child(c::MEMBERS_YOU),
                                )
                            })
                            .child(div().flex_1())
                            .child(
                                div()
                                    .flex_none()
                                    .type_role(META)
                                    .text_color(k.ink_3)
                                    .child(week),
                            ),
                    )
                    .child(div().type_role(META).text_color(k.ink_2).child(line)),
            )
            .on_click(move |_, _, cx| {
                let id = id.clone();
                nav.update(cx, |nav, cx| nav.open_members(Some(id), cx))
            });
        ui::motion::rise(
            SharedString::from(format!("member-row-{}", m.id)),
            ix,
            row,
            cx,
        )
    }

    fn detail_header(&self, e: &Entry, cx: &mut Context<Self>) -> AnyElement {
        let k = theme::colors(cx);
        let nav = self.services.nav.clone();
        let run = e.run.clone();
        let busy = e.stats.machine.as_ref().is_some_and(|m| m.busy);
        let member = e.member.id.clone();
        let side_chat = {
            let nav = nav.clone();
            let run = run.clone();
            move |_: &gpui::ClickEvent, _: &mut Window, cx: &mut App| {
                let Some(r) = run.clone() else { return };
                let member = member.clone();
                nav.update(cx, |nav, cx| {
                    nav.open_topic(r.group_id, r.topic_id, cx);
                    nav.set_right_pane(RightPane::SideChat(member), cx);
                })
            }
        };
        let live = {
            let nav = nav.clone();
            let run = run.clone();
            move |_: &gpui::ClickEvent, _: &mut Window, cx: &mut App| {
                let Some(r) = run.clone() else { return };
                nav.update(cx, |nav, cx| {
                    nav.open_topic(r.group_id, r.topic_id, cx);
                    nav.set_right_pane(RightPane::Live(r.id), cx);
                })
            }
        };
        let back = nav.clone();
        kit::bar()
            .gap(space::S6)
            .child(
                div()
                    .id("members-crumb")
                    .cursor_pointer()
                    .t_bar_title()
                    .font_weight(gpui::FontWeight::NORMAL)
                    .text_color(k.ink_3)
                    .child(c::MEMBERS_PAGE_TITLE)
                    .on_click(move |_, _, cx| back.update(cx, |n, cx| n.open_members(None, cx))),
            )
            .child(
                div()
                    .t_bar_title()
                    .font_weight(gpui::FontWeight::NORMAL)
                    .text_color(k.ink_3)
                    .child("/"),
            )
            .child(
                div()
                    .min_w_0()
                    .one_line()
                    .t_bar_title()
                    .text_color(k.ink)
                    .child(e.member.name.clone()),
            )
            .child(div().flex_1())
            .child(
                div()
                    .flex_none()
                    .flex()
                    .items_center()
                    .gap(px(2.))
                    .child(
                        kit::bar_icon(
                            "member-side-chat",
                            Lucide::MessageCircle,
                            c::MEMBERS_SIDE_CHAT,
                            false,
                            cx,
                        )
                        .on_click(side_chat),
                    )
                    .child(
                        kit::bar_icon(
                            "member-outputs",
                            Lucide::Layers,
                            c::MEMBERS_OUTPUTS_BUTTON,
                            false,
                            cx,
                        )
                        .on_click(move |_, _, cx| nav.update(cx, |n, cx| n.open_artifacts(cx))),
                    )
                    .when(e.stats.machine.is_some(), |el| {
                        el.child(
                            kit::bar_icon(
                                "member-computer",
                                Lucide::Monitor,
                                c::MEMBERS_COMPUTER_BUTTON,
                                false,
                                cx,
                            )
                            .when(busy, |el| {
                                el.child(
                                    div()
                                        .absolute()
                                        .top(px(5.))
                                        .right(px(5.))
                                        .rounded_full()
                                        .border(px(1.5))
                                        .border_color(k.bg)
                                        .child(kit::live_dot("member-computer-dot", 6., cx)),
                                )
                            })
                            .on_click(live),
                        )
                    })
                    .child(div().mx(space::S4).child(kit::bar_divider(cx)))
                    .child(
                        kit::bar_icon(
                            "member-aside",
                            Lucide::PanelRight,
                            c::MEMBERS_TOGGLE_ASIDE,
                            self.aside_open,
                            cx,
                        )
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.aside_open = !this.aside_open;
                            cx.notify();
                        })),
                    ),
            )
            .into_any_element()
    }

    fn identity(&self, e: &Entry, cx: &App) -> AnyElement {
        let k = theme::colors(cx);
        let p = &self.services.provider;
        let m = &e.member;
        // A 群 that holds the whole team (全员) says nothing about the work.
        let team = p.members().iter().filter(|x| !x.deactivated).count();
        let groups: Vec<String> = p
            .groups()
            .into_iter()
            .filter(|g| g.kind == GroupKind::Group && g.member_ids.contains(&m.id))
            .filter(|g| g.member_ids.len() < team)
            .map(|g| g.name)
            .collect();
        let mut parts = Vec::new();
        if let Some(r) = &e.stats.reports_to {
            parts.push(fill(
                c::MEMBERS_REPORTS_TO,
                &[("name", &member_name(&self.services, r))],
            ));
        }
        if !groups.is_empty() {
            parts.push(fill(
                c::MEMBERS_IN_GROUPS,
                &[
                    ("groups", &groups.join(c::MEMBERS_LIST_SEP)),
                    ("n", &count_word(groups.len())),
                ],
            ));
        }
        let ring = e.run.as_ref().map(|_| Ring::Live);
        let run = e.run.clone();
        let services = self.services.clone();
        ui::motion::rise(
            "member-identity",
            1,
            div()
                .flex()
                .items_center()
                .gap(space::S16)
                .pt(space::S8)
                .pb(space::S4)
                .child(MemberAvatar::new(Some(m), px(48.)).ring(ring))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .flex()
                        .flex_col()
                        .gap(space::S2)
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap(space::S8)
                                .child(kit::page_title(m.name.clone(), cx))
                                .children(m.role.clone().map(ui::JobTag::new)),
                        )
                        .when(!parts.is_empty(), |el| {
                            el.child(
                                div()
                                    .one_line()
                                    .t_small()
                                    .text_color(k.ink_3)
                                    .child(parts.join(" · ")),
                            )
                        }),
                )
                .when(m.is_bot(), |el| {
                    el.child(
                        div()
                            .flex_none()
                            .flex()
                            .gap(space::S8)
                            .child(
                                ui::button(
                                    "member-pause",
                                    c::MEMBERS_PAUSE,
                                    ui::ButtonKind::Secondary,
                                    ui::ButtonSize::Md,
                                    cx,
                                )
                                .text_size(px(13.))
                                .disabled(run.is_none())
                                .on_click(move |_, _, _| {
                                    if let Some(r) = &run {
                                        // The 实时查看 state follows the run's change event.
                                        drop(services.provider.take_over_run(
                                            &r.group_id,
                                            &r.topic_id,
                                            &r.id,
                                        ));
                                    }
                                }),
                            )
                            .child(
                                ui::button(
                                    "member-hand-over",
                                    fill(c::MEMBERS_HAND_OVER, &[("name", &m.name)]),
                                    ui::ButtonKind::Primary,
                                    ui::ButtonSize::Md,
                                    cx,
                                )
                                .text_size(px(13.))
                                .on_click(|_, window, cx| {
                                    window.dispatch_action(
                                        Box::new(crate::views::nav::QuickSwitch),
                                        cx,
                                    )
                                }),
                            ),
                    )
                }),
            cx,
        )
    }

    /// 现在: the active run with its plan, and what waits in the queue.
    fn now_section(&self, e: &Entry, cx: &App) -> Option<AnyElement> {
        let k = theme::colors(cx);
        let p = &self.services.provider;
        if e.run.is_none() && e.extras.queued.is_empty() {
            return None;
        }
        let card = e.run.as_ref().map(|r| {
            let topic = p.topic(&r.group_id, &r.topic_id);
            let group = p
                .groups()
                .into_iter()
                .find(|g| g.id == r.group_id)
                .map(|g| g.name)
                .unwrap_or_default();
            let by = r
                .requested_by
                .as_deref()
                .map(|b| member_name(&self.services, b))
                .unwrap_or_default();
            let current = r.plan.iter().position(|s| s.state == PlanState::Current);
            let since = current
                .and_then(|i| p.run_step_times(&r.id).get(i).copied())
                .or(r.started_at)
                .unwrap_or(r.created_at);
            let nav = self.services.nav.clone();
            let (g, t, id) = (r.group_id.clone(), r.topic_id.clone(), r.id.clone());
            let cols = r.plan.len().max(1) as u16;
            div()
                .id("member-now")
                .block()
                .px(space::S16)
                .py(space::S14)
                .cursor_pointer()
                .hover(|s| s.bg(k.hover))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(space::S8)
                        .type_role(META)
                        .text_color(k.ink_3)
                        .child(
                            div()
                                .flex_shrink(0.3)
                                .min_w(px(80.))
                                .one_line()
                                .t_ui()
                                .line_height(px(22.))
                                .medium()
                                .text_color(k.ink)
                                .child(topic.map(|t| t.title).unwrap_or_default()),
                        )
                        .child(div().flex_1().min_w_0().one_line().child(fill(
                            c::MEMBERS_TOPIC_META,
                            &[("group", &group), ("name", &by)],
                        )))
                        .children(
                            kit::plan_progress(&r.plan)
                                .map(|(n, m)| div().flex_none().mono().child(format!("{n}/{m}"))),
                        ),
                )
                .children(current.map(|i| {
                    div()
                        .mt(space::S4)
                        .flex()
                        .items_center()
                        .gap(space::S8)
                        .t_small()
                        .text_color(k.ink_2)
                        .child(kit::live_dot("member-now-dot", 6., cx))
                        .child(r.plan[i].text.clone())
                        .child(
                            div()
                                .type_role(META)
                                .text_color(k.ink_3)
                                .child(kit::elapsed(p.clock_now(), since)),
                        )
                }))
                .child(div().mt(space::S12).child(kit::step_bar(
                    "member-now",
                    &r.plan,
                    space::S4,
                    cx,
                )))
                .child(
                    div()
                        .mt(space::S6)
                        .grid()
                        .grid_cols(cols)
                        .gap(space::S4)
                        .type_role(TINY)
                        .text_color(k.ink_3)
                        .children(r.plan.iter().map(|s| {
                            div()
                                .min_w_0()
                                .one_line()
                                .when(s.state == PlanState::Current, |el| {
                                    el.text_color(k.live_ink)
                                })
                                .child(s.text.clone())
                        })),
                )
                .on_click(move |_, _, cx| {
                    let (g, t, id) = (g.clone(), t.clone(), id.clone());
                    nav.update(cx, |nav, cx| {
                        nav.open_topic(g, t, cx);
                        nav.set_right_pane(RightPane::Live(id), cx);
                    })
                })
        });
        let has_card = card.is_some();
        let queued = e.extras.queued.iter().enumerate().map(|(i, q)| {
            let nav = self.services.nav.clone();
            let topic = q.topic.clone();
            div()
                .id(SharedString::from(format!("member-queued-{i}")))
                .flex()
                .items_center()
                .gap(space::S8)
                .px(space::S16)
                .py(space::S10)
                .when(has_card || i > 0, |el| el.border_t_1())
                .border_color(k.line)
                .bg(k.raised)
                .t_small()
                .cursor_pointer()
                .child(ui::JobTag::new(c::MEMBERS_QUEUED))
                .child(
                    div()
                        .min_w_0()
                        .one_line()
                        .medium()
                        .text_color(k.ink)
                        .child(q.title.clone()),
                )
                .child(div().flex_1())
                .children(q.note.clone().map(|n| {
                    div()
                        .flex_none()
                        .type_role(META)
                        .text_color(k.ink_3)
                        .child(n)
                }))
                .on_click(move |_, _, cx| {
                    if let Some((g, t)) = topic.clone() {
                        nav.update(cx, |nav, cx| nav.open_topic(g, t, cx));
                    }
                })
        });
        let n_active = e.run.is_some() as usize;
        Some(
            div()
                .mt(space::S20)
                .child(
                    kit::section_head(c::MEMBERS_NOW, None, cx).child(
                        div().type_role(META).text_color(k.ink_3).child(fill(
                            c::MEMBERS_NOW_COUNTS,
                            &[
                                ("a", &n_active.to_string()),
                                (
                                    "b",
                                    &e.extras
                                        .queued
                                        .len()
                                        .max(e.stats.queued as usize)
                                        .to_string(),
                                ),
                            ],
                        )),
                    ),
                )
                .child(ui::motion::rise(
                    "member-now-card",
                    2,
                    ui::card(cx)
                        .mt(space::S8)
                        .overflow_hidden()
                        .children(card)
                        .children(queued),
                    cx,
                ))
                .into_any_element(),
        )
    }

    /// 这周: finished 话题, 产出 and cost against the limit.
    fn week_section(&self, e: &Entry, cx: &App) -> AnyElement {
        let k = theme::colors(cx);
        let tile = |label: AnyElement, value: String, sub: AnyElement, first: bool| {
            div()
                .min_w_0()
                .px(space::S16)
                .py(space::S12)
                .flex()
                .flex_col()
                .when(!first, |el| el.border_l_1().border_color(k.line))
                .child(label)
                .child(
                    div()
                        .mono()
                        .text_size(px(20.))
                        .line_height(px(28.))
                        .medium()
                        .text_color(k.ink)
                        .child(value),
                )
                .child(sub)
        };
        let meta = |text: String| {
            div()
                .type_role(META)
                .text_color(k.ink_3)
                .child(text)
                .into_any_element()
        };
        let done = tile(
            meta(c::MEMBERS_DONE_TOPICS.to_string()),
            e.stats.done_this_week.to_string(),
            div()
                .mt(space::S2)
                .child(meta(
                    e.extras
                        .avg_topic_ms
                        .map(|ms| {
                            fill(
                                c::MEMBERS_AVERAGE,
                                &[("n", &(ms / 60_000).max(1).to_string())],
                            )
                        })
                        .unwrap_or_default(),
                ))
                .into_any_element(),
            true,
        );
        let outputs = tile(
            meta(c::MEMBERS_OUTPUTS.to_string()),
            e.stats
                .artifacts_this_week
                .max(e.extras.prs_this_week + e.extras.pages_this_week)
                .to_string(),
            div()
                .mt(space::S2)
                .child(meta(fill(
                    c::MEMBERS_OUTPUT_KINDS,
                    &[
                        ("prs", &e.extras.prs_this_week.to_string()),
                        ("pages", &e.extras.pages_this_week.to_string()),
                    ],
                )))
                .into_any_element(),
            false,
        );
        let currency = e.stats.cost_currency;
        let cost = e.stats.cost_cents_this_week.unwrap_or(0);
        let pct = e
            .extras
            .budget_cents
            .filter(|b| *b > 0)
            .map(|b| (cost as f32 / b as f32).clamp(0., 1.));
        let cost_label = div()
            .flex()
            .justify_between()
            .gap(space::S8)
            .type_role(META)
            .text_color(k.ink_3)
            .child(c::MEMBERS_COST)
            .children(
                e.extras
                    .budget_cents
                    .map(|b| fill(c::MEMBERS_LIMIT, &[("v", &money(b, currency))])),
            )
            .into_any_element();
        let bar = pct.map(|pct| {
            div()
                .mt(px(9.))
                .mb(px(6.))
                .h(px(3.))
                .rounded(px(2.))
                .bg(k.sunken)
                .overflow_hidden()
                .child(grow_width(
                    "member-cost-bar",
                    div().h_full().rounded(px(2.)).bg(k.ink_3),
                    pct,
                    cx,
                ))
                .into_any_element()
        });
        // No tile when no run reported a cost: ¥0 or $0 would be a wrong number.
        let cost_tile = e.stats.cost_cents_this_week.map(|_| {
            tile(
                cost_label,
                money(cost, currency),
                bar.unwrap_or_else(|| div().into_any_element()),
                false,
            )
        });
        let cols = if cost_tile.is_some() { 3 } else { 2 };
        div()
            .mt(space::S20)
            .child(kit::section_head(c::MEMBERS_THIS_WEEK, None, cx))
            .child(ui::motion::rise(
                "member-week",
                3,
                ui::card(cx)
                    .mt(space::S8)
                    .grid()
                    .grid_cols(cols)
                    .child(done)
                    .child(outputs)
                    .children(cost_tile),
                cx,
            ))
            .into_any_element()
    }

    fn deliveries(&self, e: &Entry, cx: &App) -> Option<AnyElement> {
        if e.extras.deliveries.is_empty() {
            return None;
        }
        let k = theme::colors(cx);
        let p = &self.services.provider;
        let now = p.clock_now();
        let rows = e.extras.deliveries.iter().enumerate().map(|(i, d)| {
            let group = d
                .topic
                .as_ref()
                .and_then(|(g, _)| p.groups().into_iter().find(|x| &x.id == g))
                .map(|g| g.name);
            let (lead, title, meta): (AnyElement, String, gpui::Div) = match &d.kind {
                DeliveryKind::Pr {
                    number,
                    additions,
                    deletions,
                } => (
                    div()
                        .flex_none()
                        .size(px(28.))
                        .rounded(theme::radius::MD)
                        .border_1()
                        .border_color(k.line)
                        .bg(k.bg)
                        .flex()
                        .items_center()
                        .justify_center()
                        .child(ui::icon(
                            if d.state == Some(DeliveryState::Merged) {
                                Lucide::GitBranch
                            } else {
                                Lucide::GitPullRequest
                            },
                            px(14.),
                            k.ink_2,
                        ))
                        .into_any_element(),
                    fill(
                        c::MEMBERS_PR,
                        &[("n", &number.to_string()), ("title", &d.title)],
                    ),
                    {
                        let mut line = kit::day_and_time(now, d.at, false);
                        if let Some(g) = &group {
                            line = format!("{line} · {g}");
                        }
                        let today = kit::day_and_time(now, d.at, false)
                            .starts_with(crate::copy::timeline::TODAY);
                        div().flex().gap(space::S4).child(line).when(today, |el| {
                            el.child("·")
                                .child(kit::line_counts(*additions, *deletions, cx))
                        })
                    },
                ),
                DeliveryKind::Artifact {
                    artifact_id,
                    version,
                } => {
                    let summary = p.artifact(artifact_id).map(|a| a.summary);
                    let thumb = match &summary {
                        Some(a) => kit::artifact_thumb(&self.services, a, px(28.), px(36.), cx)
                            .rounded(px(4.))
                            .shadow(k.lift())
                            .into_any_element(),
                        None => div().size(px(28.)).into_any_element(),
                    };
                    let kind = summary
                        .as_ref()
                        .map(|a| crate::views::group_page::kind_label(a.kind))
                        .unwrap_or(crate::copy::group_page::GROUP_PAGE_KIND_PAGE);
                    (
                        thumb,
                        fill(
                            c::MEMBERS_ARTIFACT,
                            &[("title", &d.title), ("v", &version.to_string())],
                        ),
                        div().child(format!("{kind} · {}", kit::day_and_time(now, d.at, false))),
                    )
                }
            };
            let tag = d.state.map(|s| match s {
                DeliveryState::WaitsOnMe => div()
                    .flex_none()
                    .h(theme::TAG_HEIGHT)
                    .px(space::S6)
                    .rounded(theme::TAG_RADIUS)
                    .bg(k.you_bg)
                    .type_role(META)
                    .text_color(k.you_ink)
                    .child(c::MEMBERS_WAITS_REVIEW)
                    .into_any_element(),
                DeliveryState::Open => ui::JobTag::new(c::MEMBERS_UNMERGED).into_any_element(),
                DeliveryState::Merged => ui::JobTag::new(c::MEMBERS_MERGED).into_any_element(),
            });
            let nav = self.services.nav.clone();
            let kind = d.kind.clone();
            let topic = d.topic.clone();
            let row = div()
                .id(SharedString::from(format!("member-delivery-{i}")))
                .flex()
                .items_center()
                .gap(space::S12)
                .h(px(48.))
                .px(space::S10)
                .rounded(px(9.))
                .cursor_pointer()
                .hover(|s| s.bg(k.hover))
                .child(lead)
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .flex()
                        .flex_col()
                        .child(
                            div()
                                .one_line()
                                .t_small()
                                .medium()
                                .text_color(k.ink)
                                .child(title),
                        )
                        .child(meta.type_role(META).text_color(k.ink_3)),
                )
                .children(tag)
                .child(ui::icon(Lucide::ChevronRight, px(14.), k.ink_3))
                .on_click(move |_, _, cx| match (&kind, &topic) {
                    (DeliveryKind::Artifact { artifact_id, .. }, _) => {
                        let id = artifact_id.clone();
                        nav.update(cx, |n, cx| n.open_artifact(id, None, cx))
                    }
                    (_, Some((g, t))) => {
                        let (g, t) = (g.clone(), t.clone());
                        nav.update(cx, |n, cx| n.open_topic(g, t, cx))
                    }
                    _ => {}
                });
            ui::motion::rise(
                SharedString::from(format!("member-delivery-rise-{i}")),
                i + 4,
                row,
                cx,
            )
        });
        Some(
            div()
                .mt(space::S20)
                .child(kit::section_head(
                    c::MEMBERS_RECENT_DELIVERED,
                    Some(e.extras.deliveries.len()),
                    cx,
                ))
                .child(
                    div()
                        .mt(space::S4)
                        .mx(-space::S10)
                        .flex()
                        .flex_col()
                        .children(rows),
                )
                .into_any_element(),
        )
    }

    fn problems(&self, e: &Entry, cx: &mut Context<Self>) -> Option<AnyElement> {
        if e.stats.problems.is_empty() {
            return None;
        }
        let k = theme::colors(cx);
        let p = self.services.provider.clone();
        let now = p.clock_now();
        let name = e.member.name.clone();
        let rows: Vec<AnyElement> = e
            .stats
            .problems
            .iter()
            .enumerate()
            .map(|(i, pr)| {
                let retried = pr.run_id.as_ref().is_some_and(|r| self.retried.contains(r));
                let row = div()
                    .flex()
                    .items_center()
                    .gap(space::S12)
                    .pl(space::S16)
                    .pr(space::S12)
                    .py(space::S12)
                    .when(i > 0, |el| el.border_t_1().border_color(k.line));
                if retried {
                    return row
                        .child(ui::icon(Lucide::CircleCheck, px(16.), k.ok))
                        .child(
                            div()
                                .min_w_0()
                                .flex()
                                .flex_col()
                                .child(
                                    div()
                                        .t_small()
                                        .text_color(k.ink)
                                        .child(fill(c::MEMBERS_RETRIED, &[("name", &name)])),
                                )
                                .child(div().type_role(META).text_color(k.ink_3).child(format!(
                                    "{} · {}",
                                    c::MEMBERS_RETRIED_NOTE,
                                    c::MEMBERS_JUST_NOW
                                ))),
                        )
                        .into_any_element();
                }
                let member = e.member.id.clone();
                let run = pr.run_id.clone();
                row.child(ui::icon(Lucide::CircleAlert, px(16.), k.bad))
                    .child(
                        div()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .child(div().t_small().text_color(k.ink).child(pr.text.clone()))
                            .child(
                                div()
                                    .flex()
                                    .type_role(META)
                                    .text_color(k.ink_3)
                                    .child(div().text_color(k.bad_ink).child(c::MEMBERS_STUCK))
                                    .child(format!(" · {}", kit::day_and_time(now, pr.at, true))),
                            ),
                    )
                    .child(div().flex_1())
                    .when_some(run, |el, run| {
                        el.child(
                            ui::button(
                                SharedString::from(format!("member-retry-{i}")),
                                c::MEMBERS_RETRY,
                                ui::ButtonKind::Secondary,
                                ui::ButtonSize::Md,
                                cx,
                            )
                            .text_size(px(13.))
                            .on_click(cx.listener(
                                move |this, _, _, cx| {
                                    let rx =
                                        this.services.provider.retry_member_problem(&member, &run);
                                    let run = run.clone();
                                    cx.spawn(async move |this, cx| {
                                        if let Ok(Ok(())) = rx.await {
                                            let _ = this.update(cx, |this, cx| {
                                                this.retried.insert(run);
                                                cx.notify();
                                            });
                                        }
                                    })
                                    .detach();
                                },
                            )),
                        )
                    })
                    .into_any_element()
            })
            .collect();
        Some(
            div()
                .mt(space::S16)
                .child(kit::section_head(
                    c::MEMBERS_RECENT_PROBLEMS,
                    Some(e.stats.problems.len()),
                    cx,
                ))
                .child(ui::motion::rise(
                    "member-problems",
                    7,
                    ui::card(cx).mt(space::S8).overflow_hidden().children(rows),
                    cx,
                ))
                .into_any_element(),
        )
    }

    fn aside(&self, e: &Entry, cx: &mut Context<Self>) -> AnyElement {
        let k = theme::colors(cx);
        let p = self.services.provider.clone();
        let name = e.member.name.clone();
        let mut cards: Vec<AnyElement> = Vec::new();
        if let Some(mc) = &e.stats.machine {
            let run = e.run.clone();
            let nav = self.services.nav.clone();
            let open_live = move |_: &gpui::ClickEvent, _: &mut Window, cx: &mut App| {
                let Some(r) = run.clone() else { return };
                nav.update(cx, |nav, cx| {
                    nav.open_topic(r.group_id, r.topic_id, cx);
                    nav.set_right_pane(RightPane::Live(r.id), cx);
                })
            };
            let screen = e.run.as_ref().and_then(|r| {
                kit::mini_screen(&self.services, "member-screen", &r.id, px(104.), true, cx)
            });
            let state = if mc.busy {
                c::MEMBERS_IN_USE
            } else if mc.online {
                c::MEMBERS_MACHINE_IDLE
            } else {
                c::MEMBERS_MACHINE_OFFLINE
            };
            cards.push(
                kit::side_card(cx)
                    .child(
                        kit::card_label(c::MEMBERS_COMPUTER, None, cx)
                            .h(px(18.))
                            .when(e.run.is_some(), |el| {
                                el.child(div().flex_1()).child(
                                    div()
                                        .id("member-computer-link")
                                        .cursor_pointer()
                                        .font_weight(gpui::FontWeight::NORMAL)
                                        .text_color(k.ink_2)
                                        .child(fill(c::MEMBERS_SEE_COMPUTER, &[("name", &name)]))
                                        .on_click(open_live.clone()),
                                )
                            }),
                    )
                    .children(screen.map(|s| div().mt(space::S10).child(s)))
                    .child(
                        div()
                            .mt(space::S10)
                            .flex()
                            .items_center()
                            .gap(space::S8)
                            .t_small()
                            .child(ui::icon(Lucide::Monitor, px(14.), k.live))
                            .child(div().medium().text_color(k.ink).child(mc.name.clone()))
                            .child(div().text_color(k.ink_2).child(state))
                            .child(div().flex_1())
                            .children(mc.disk_used_pct.map(|pct| {
                                div()
                                    .flex()
                                    .items_center()
                                    .gap(space::S6)
                                    .type_role(META)
                                    .text_color(k.ink_3)
                                    .child(c::MEMBERS_DISK)
                                    .child(
                                        div()
                                            .w(px(40.))
                                            .h(px(3.))
                                            .rounded(px(2.))
                                            .bg(k.sunken)
                                            .overflow_hidden()
                                            .child(grow_width(
                                                "member-disk",
                                                div().h_full().rounded(px(2.)).bg(k.ink_3),
                                                pct as f32 / 100.,
                                                cx,
                                            )),
                                    )
                                    .child(div().mono().child(format!("{pct}%")))
                            })),
                    )
                    .into_any_element(),
            );
        }
        if !e.stats.needs_approval.is_empty() {
            let services = self.services.clone();
            cards.push(
                kit::side_card(cx)
                    .child(
                        kit::card_label(c::MEMBERS_NEEDS_APPROVAL, None, cx)
                            .h(px(18.))
                            .child(div().flex_1())
                            .child(
                                kit::text_link("member-approvals-edit", c::MEMBERS_EDIT, cx)
                                    .h(px(22.))
                                    .mr(-space::S6)
                                    .font_weight(gpui::FontWeight::NORMAL)
                                    .on_click(move |_, _, cx| {
                                        crate::app::open_settings_window(services.clone(), cx)
                                    }),
                            ),
                    )
                    .child(div().mt(space::S8).flex().flex_col().children(
                        e.stats.needs_approval.iter().map(|a| {
                            div()
                                .flex()
                                .items_center()
                                .gap(space::S8)
                                .h(px(28.))
                                .t_small()
                                .text_color(k.ink)
                                .child(ui::icon(Lucide::Lock, px(13.), k.ink_3))
                                .child(a.clone())
                        }),
                    ))
                    .child(
                        div()
                            .mt(space::S8)
                            .pt(space::S8)
                            .border_t_1()
                            .border_color(k.line)
                            .type_role(META)
                            .text_color(k.ink_3)
                            .child(fill(c::MEMBERS_APPROVAL_NOTE, &[("name", &name)])),
                    )
                    .into_any_element(),
            );
        }
        if !e.extras.tools.is_empty() {
            cards.push(
                kit::side_card(cx)
                    .child(
                        kit::card_label(c::MEMBERS_TOOLS, Some(e.extras.tools.len()), cx)
                            .h(px(18.)),
                    )
                    .child(
                        div()
                            .mt(space::S8)
                            .flex()
                            .flex_col()
                            .gap(space::S10)
                            .children(e.extras.tools.iter().map(|t| {
                                div()
                                    .flex()
                                    .flex_col()
                                    .child(
                                        div()
                                            .flex()
                                            .items_center()
                                            .gap(space::S8)
                                            .child(
                                                div()
                                                    .t_small()
                                                    .medium()
                                                    .text_color(k.ink)
                                                    .child(t.name.clone()),
                                            )
                                            .child(div().flex_1())
                                            .children(t.access.clone().map(ui::JobTag::new)),
                                    )
                                    .children(t.detail.clone().map(|d| {
                                        div().type_role(META).text_color(k.ink_3).child(d)
                                    }))
                            })),
                    )
                    .into_any_element(),
            );
        }
        if !e.extras.decisions.is_empty() {
            let now = p.clock_now();
            let rows = e.extras.decisions.iter().enumerate().map(|(i, d)| {
                let who = p.member(&d.by);
                let place = match &d.topic_id {
                    Some(t) => p.topic(&d.group_id, t).map(|t| t.title).unwrap_or_default(),
                    None => p
                        .groups()
                        .into_iter()
                        .find(|g| g.id == d.group_id)
                        .map(|g| fill(c::MEMBERS_GROUP_NAME, &[("name", &g.name)]))
                        .unwrap_or_default(),
                };
                let when = crate::views::group_page::short_time(now, d.at);
                let nav = self.services.nav.clone();
                let (g, t) = (d.group_id.clone(), d.topic_id.clone());
                div()
                    .id(SharedString::from(format!("member-decision-{i}")))
                    .flex()
                    .gap(space::S8)
                    .p(space::S6)
                    .m(-space::S6)
                    .rounded(theme::radius::MD)
                    .cursor_pointer()
                    .hover(|s| s.bg(k.hover))
                    .child(
                        div()
                            .mt(px(1.))
                            .child(MemberAvatar::new(who.as_ref(), px(18.))),
                    )
                    .child(
                        div()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .child(div().t_small().text_color(k.ink).child(d.text.clone()))
                            .child(div().one_line().type_role(META).text_color(k.ink_3).child(
                                format!(
                                    "{} · {place} · {when}",
                                    who.map(|w| w.name).unwrap_or_default()
                                ),
                            )),
                    )
                    .on_click(move |_, _, cx| {
                        let g = g.clone();
                        match t.clone() {
                            Some(t) => nav.update(cx, |n, cx| n.open_topic(g, t, cx)),
                            None => nav.update(cx, |n, cx| n.open_group(g, cx)),
                        }
                    })
            });
            cards.push(
                kit::side_card(cx)
                    .child(
                        kit::card_label(c::MEMBERS_DECISIONS, Some(e.extras.decisions.len()), cx)
                            .h(px(18.)),
                    )
                    .child(
                        div()
                            .mt(space::S8)
                            .flex()
                            .flex_col()
                            .gap(space::S10)
                            .children(rows),
                    )
                    .into_any_element(),
            );
        }
        kit::aside(
            "member-aside",
            c::MEMBERS_WORKPLACE,
            kit::MEMBER_ASIDE_WIDTH,
            space::S12,
            None,
            cx,
        )
        // The 成员 detail's own title bar holds the toggle (Members.dc.html).
        // With nothing to show, one quiet line says what will appear here.
        .when(cards.is_empty(), |el| {
            el.child(
                div().flex_1().min_h(px(160.)).child(
                    ui::EmptyState::new(Lucide::Monitor, c::MEMBERS_WORKPLACE_EMPTY)
                        .description(c::MEMBERS_WORKPLACE_EMPTY_LINE),
                ),
            )
        })
        .child(ui::rail_stack(
            cards.into_iter().enumerate().map(|(i, el)| {
                ui::motion::rise(
                    SharedString::from(format!("member-side-{i}")),
                    i + 2,
                    div().child(el),
                    cx,
                )
            }),
            cx,
        ))
        .into_any_element()
    }
}

/// A bar fill that grows to `pct` of its track once (CONCEPT2 §5 bar grow).
fn grow_width(id: &'static str, el: gpui::Div, pct: f32, cx: &App) -> AnyElement {
    use gpui::{Animation, AnimationExt as _};
    if cx.reduce_motion() {
        return el.w(gpui::relative(pct)).into_any_element();
    }
    el.with_animation(
        id,
        Animation::new(theme::motion::BAR_GROW).with_easing(theme::motion::out_fn()),
        move |el, t| el.w(gpui::relative(pct * t)),
    )
    .into_any_element()
}

impl Render for MembersPage {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let k = theme::colors(cx);
        let entries = self.entries();
        let selected = self.selected(cx);
        let list = self.list(&entries, selected.as_deref(), cx);
        let entry = selected
            .as_deref()
            .and_then(|id| entries.iter().find(|e| e.member.id == id));
        let (detail, aside) = match entry {
            None => (
                stub_body(Lucide::Users, c::MEMBERS_PICK, None, cx).into_any_element(),
                None,
            ),
            Some(e) => {
                let header = self.detail_header(e, cx);
                let identity = self.identity(e, cx);
                let now = self.now_section(e, cx);
                let week = e.member.is_bot().then(|| self.week_section(e, cx));
                let deliveries = self.deliveries(e, cx);
                let problems = self.problems(e, cx);
                let detail = div()
                    .id("member-detail")
                    .flex_1()
                    .min_w(px(0.))
                    .h_full()
                    .overflow_y_scroll()
                    .px(space::S32)
                    .pb(space::S32)
                    .child(header)
                    .child(
                        div()
                            .max_w(px(600.))
                            .mx_auto()
                            .child(identity)
                            .children(now)
                            .children(week)
                            .children(deliveries)
                            .children(problems),
                    )
                    .into_any_element();
                let aside = self.aside_open.then(|| self.aside(e, cx));
                (detail, aside)
            }
        };
        let _ = (SMALL, clock);
        div()
            .size_full()
            .flex()
            .bg(k.bg)
            .child(list)
            .child(detail)
            .children(aside)
    }
}
