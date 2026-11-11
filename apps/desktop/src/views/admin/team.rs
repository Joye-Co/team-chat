//! 团队管理 › 成员: every member in a table (成员 and 岗位, 级别, this week's 话题, 本周花费 with a
//! budget bar for AI members, 最近) with tabs 全部 / 人 / AI 成员 / 邀请中 / 已停用 and headers that
//! sort, and an inspector for the selected member: this week's numbers, recent 话题, the AI members
//! they use most, 级别, when they joined and who invited them, and their devices.

use gpui::{
    AnyElement, AppContext as _, Context, Entity, InteractiveElement as _, IntoElement,
    ParentElement as _, SharedString, StatefulInteractiveElement as _, Styled, Window, div,
    prelude::FluentBuilder as _, relative,
};
use gpui_component::input::InputState;
use gpui_component::menu::{DropdownMenu as _, PopupMenuItem};

use chat_core::proto::{TeamRoster, TeamUsage};

use super::{AdminView, model};
use crate::{
    copy::{admin as m, fill},
    data::{Member, MemberKind, MemberLevel},
    ui::{
        MemberAvatar, Sym,
        icons::sym as sym_icon,
        native::{
            Alert, AlertTone, Badge, NativeType as _, PushSize, PushStyle, SearchField, Segment,
            Segmented, StatusBadge, TruncText, card_surface, native_colors, progress, push_button,
            tokens::{avatar, height, monitor as t, radius, space},
        },
        sf_symbols::SymSize,
    },
    views::settings::widgets::answer,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum TeamTab {
    #[default]
    All,
    People,
    Ai,
    Invited,
    Off,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TeamSortKey {
    Name,
    Level,
    Topics,
    Spend,
    Recent,
}

pub struct Team {
    pub roster: Option<TeamRoster>,
    pub week: Option<TeamUsage>,
    pub error: Option<String>,
    pub tab: TeamTab,
    pub sort: (TeamSortKey, bool),
    pub selected: Option<String>,
    search: Entity<InputState>,
}

impl Team {
    pub fn new(window: &mut Window, cx: &mut Context<AdminView>) -> Team {
        Team {
            roster: None,
            week: None,
            error: None,
            tab: TeamTab::default(),
            sort: (TeamSortKey::Spend, true),
            selected: None,
            search: cx.new(|cx| InputState::new(window, cx).placeholder(m::TEAM_SEARCH)),
        }
    }
}

/// One row of the table.
#[derive(Clone, Debug)]
pub struct Row {
    pub member: Member,
    pub topics: u32,
    pub spend: f64,
    pub budget_cents: Option<u64>,
    /// When last seen (people) or 0; AI members use `ai_state`.
    pub seen: Option<u64>,
    pub ai_state: Option<&'static str>,
}

/// The rows in sort order; ties keep the name order.
pub fn sort_rows(mut rows: Vec<Row>, key: TeamSortKey, desc: bool) -> Vec<Row> {
    rows.sort_by(|a, b| {
        a.member
            .name
            .to_lowercase()
            .cmp(&b.member.name.to_lowercase())
    });
    let level = |r: &Row| {
        if r.member.level == MemberLevel::Owner {
            1.
        } else {
            0.
        }
    };
    let val = |r: &Row| -> f64 {
        match key {
            TeamSortKey::Name => 0.,
            TeamSortKey::Level => level(r),
            TeamSortKey::Topics => r.topics as f64,
            TeamSortKey::Spend => r.spend,
            TeamSortKey::Recent => {
                r.seen.unwrap_or(0) as f64 + if r.ai_state.is_some() { 1e15 } else { 0. }
            }
        }
    };
    if key == TeamSortKey::Name {
        if desc {
            rows.reverse();
        }
    } else {
        rows.sort_by(|a, b| {
            let o = val(a).total_cmp(&val(b));
            if desc { o.reverse() } else { o }
        });
    }
    rows
}

impl AdminView {
    pub(super) fn fetch_team(&mut self, cx: &mut Context<Self>) {
        let roster = self.services.provider.team_roster();
        let (from, to) = super::usage::UsageRange::Week.span(crate::selftest::clock::local_now());
        let week = self.services.provider.team_usage(from, to);
        if self.fleet.monitors.is_empty() {
            self.fetch_fleet(cx);
        }
        cx.spawn(async move |this, cx| {
            let (r, w) = (answer(roster).await, answer(week).await);
            let _ = this.update(cx, |this, cx| {
                match r {
                    Ok(r) => {
                        this.team.roster = Some(r);
                        this.team.error = None;
                    }
                    Err(e) => {
                        this.team.error =
                            Some(crate::views::settings::widgets::error_text(e).to_string())
                    }
                }
                if let Ok(w) = w {
                    this.team.week = Some(w);
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// 在干活 / 等你 / 空闲 of an AI member, from the machines' runs.
    fn ai_state(&self, id: &str) -> &'static str {
        let (mut running, mut waiting) = (0, 0);
        for d in self.fleet.monitors.values() {
            for mm in d.members.iter().filter(|x| x.member_id == id) {
                running += mm.running;
                waiting += mm.waiting;
            }
        }
        if waiting > 0 {
            m::TEAM_WAITING
        } else if running > 0 {
            m::TEAM_WORKING
        } else {
            m::TEAM_IDLE
        }
    }

    fn team_rows(&self, cx: &Context<Self>) -> Vec<Row> {
        let week = self.team.week.as_ref();
        let query = self.team.search.read(cx).value().trim().to_lowercase();
        self.services
            .provider
            .members()
            .into_iter()
            .filter(|x| match self.team.tab {
                TeamTab::All => !x.deactivated,
                TeamTab::People => !x.deactivated && x.kind == MemberKind::Human,
                TeamTab::Ai => !x.deactivated && x.kind == MemberKind::Bot,
                TeamTab::Off => x.deactivated,
                TeamTab::Invited => false,
            })
            .filter(|x| {
                query.is_empty()
                    || x.name.to_lowercase().contains(&query)
                    || x.role
                        .as_deref()
                        .is_some_and(|r| r.to_lowercase().contains(&query))
            })
            .map(|x| {
                let id = x.id.to_string();
                let bot = x.kind == MemberKind::Bot;
                let (topics, spend, budget) = if bot {
                    week.and_then(|w| w.bots.iter().find(|b| b.member_id == id))
                        .map(|b| (b.spend.topics, b.spend.cost_usd, b.budget_cents))
                        .unwrap_or((0, 0., None))
                } else {
                    week.and_then(|w| w.initiators.iter().find(|b| b.member_id == id))
                        .map(|b| (b.spend.topics, b.spend.cost_usd, None))
                        .unwrap_or((0, 0., None))
                };
                let seen = if x.online {
                    Some(now_ms())
                } else {
                    self.team
                        .roster
                        .as_ref()
                        .and_then(|r| r.members.iter().find(|r| r.member_id == id))
                        .and_then(|r| r.last_seen_at)
                };
                Row {
                    ai_state: bot.then(|| self.ai_state(&id)),
                    member: x,
                    topics,
                    spend,
                    budget_cents: budget,
                    seen,
                }
            })
            .collect()
    }

    pub(super) fn team_actions(&self, cx: &mut Context<Self>) -> AnyElement {
        let (s1, s2) = (self.services.clone(), self.services.clone());
        div()
            .flex()
            .gap(space::S8)
            .child(
                push_button(
                    "team-add-ai",
                    m::TEAM_ADD_AI,
                    PushStyle::Outline,
                    PushSize::Small,
                    cx,
                )
                .on_click(move |_, window, cx| {
                    crate::views::settings::open_add_bot(s1.clone(), window, cx);
                }),
            )
            .child(
                push_button(
                    "team-invite",
                    m::TEAM_INVITE,
                    PushStyle::Default,
                    PushSize::Small,
                    cx,
                )
                .on_click(move |_, window, cx| {
                    crate::views::settings::open_invite(s2.clone(), window, cx);
                }),
            )
            .into_any_element()
    }

    pub(super) fn team_subtitle(&self) -> String {
        let n = self
            .services
            .provider
            .members()
            .iter()
            .filter(|x| !x.deactivated)
            .count();
        match self
            .team
            .roster
            .as_ref()
            .map(|r| r.invites.len())
            .filter(|i| *i > 0)
        {
            Some(i) => fill(m::TEAM_SUB, &[("n", &n.to_string()), ("i", &i.to_string())]),
            None => fill(m::TEAM_SUB_NO_INVITES, &[("n", &n.to_string())]),
        }
    }

    pub(super) fn team_page(&self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let members = self.services.provider.members();
        let count = |f: &dyn Fn(&Member) -> bool| members.iter().filter(|x| f(x)).count() as u32;
        let invited = self
            .team
            .roster
            .as_ref()
            .map(|r| r.invites.len() as u32)
            .unwrap_or(0);
        let tabs = [
            (TeamTab::All, m::TEAM_TAB_ALL, count(&|x| !x.deactivated)),
            (
                TeamTab::People,
                m::TEAM_TAB_PEOPLE,
                count(&|x| !x.deactivated && x.kind == MemberKind::Human),
            ),
            (
                TeamTab::Ai,
                m::TEAM_TAB_AI,
                count(&|x| !x.deactivated && x.kind == MemberKind::Bot),
            ),
            (TeamTab::Invited, m::TEAM_TAB_INVITED, invited),
            (TeamTab::Off, m::TEAM_TAB_OFF, count(&|x| x.deactivated)),
        ];
        let view = cx.entity();
        let seg = Segmented::new(
            "team-tabs",
            tabs.iter()
                .map(|(_, l, n)| Segment::Counted((*l).into(), *n)),
        )
        .selected(tabs.iter().position(|t| t.0 == self.team.tab).unwrap_or(0))
        .on_change(move |ix, _, cx| {
            view.update(cx, |v, cx| {
                v.team.tab = tabs[ix.min(4)].0;
                cx.notify();
            })
        });
        let table = if self.team.tab == TeamTab::Invited {
            self.invites_table(cx)
        } else {
            self.members_table(window, cx)
        };
        div()
            .w_full()
            .flex()
            .flex_col()
            .gap(space::S16)
            .children(self.team.error.clone().map(|e| {
                Alert::new(Sym::Failed, m::TEAM_FAILED)
                    .tone(AlertTone::Destructive)
                    .description(e)
            }))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(space::S12)
                    .child(seg)
                    .child(div().flex_1())
                    .child(
                        SearchField::new("team-search", &self.team.search).width(t::FLEET_ADD_TEXT),
                    ),
            )
            .child(
                div()
                    .flex()
                    .items_start()
                    .gap(space::S16)
                    .child(div().flex_1().min_w(gpui::Pixels::ZERO).child(table))
                    .child(
                        div()
                            .w(t::MEMBER_INSPECTOR)
                            .flex_none()
                            .child(self.member_inspector(window, cx)),
                    ),
            )
            .into_any_element()
    }

    fn members_table(&self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let k = native_colors(cx);
        let (key, desc) = self.team.sort;
        let rows = sort_rows(self.team_rows(cx), key, desc);
        let max_spend = rows.iter().map(|r| r.spend).fold(0., f64::max);
        let cols: [(TeamSortKey, &'static str, Option<gpui::Pixels>, bool); 5] = [
            (TeamSortKey::Name, m::TEAM_COL_MEMBER, None, false),
            (
                TeamSortKey::Level,
                m::TEAM_COL_LEVEL,
                Some(t::TEAM_COL_LEVEL),
                false,
            ),
            (
                TeamSortKey::Topics,
                m::TEAM_COL_TOPICS,
                Some(t::TEAM_COL_TOPICS),
                true,
            ),
            (
                TeamSortKey::Spend,
                m::TEAM_COL_SPEND,
                Some(t::TEAM_COL_SPEND),
                false,
            ),
            (
                TeamSortKey::Recent,
                m::TEAM_COL_RECENT,
                Some(t::TEAM_COL_RECENT),
                false,
            ),
        ];
        let cell = |w: Option<gpui::Pixels>, right: bool| {
            let c = div()
                .px(t::TEAM_CELL_PAD)
                .flex()
                .items_center()
                .overflow_hidden();
            let c = match w {
                Some(w) => c.w(w).flex_none(),
                None => c.flex_1().min_w(gpui::Pixels::ZERO),
            };
            if right { c.justify_end() } else { c }
        };
        let head = div()
            .h(height::TABLE_HEAD)
            .px(space::S4)
            .flex()
            .items_center()
            .bg(k.group)
            .border_b_1()
            .border_color(k.separator)
            .children(cols.iter().map(|(sk, label, w, right)| {
                let on = key == *sk;
                let sk = *sk;
                let view = cx.entity();
                let arrow = if on {
                    if desc { Sym::ArrowDown } else { Sym::ArrowUp }
                } else {
                    Sym::Sort
                };
                cell(*w, *right).child(
                    div()
                        .id(SharedString::from(format!("team-sort-{sk:?}")))
                        .flex()
                        .items_center()
                        .gap(space::S4)
                        .px(space::S4)
                        .rounded(radius::ITEM)
                        .hover(|s| s.bg(k.fill_hover))
                        .n_section()
                        .text_color(if on { k.label } else { k.secondary })
                        .on_click(move |_, _, cx| {
                            view.update(cx, |v, cx| {
                                let (cur, d) = v.team.sort;
                                v.team.sort = if cur == sk {
                                    (sk, !d)
                                } else {
                                    (sk, sk != TeamSortKey::Name)
                                };
                                cx.notify();
                            })
                        })
                        .child(*label)
                        .child(sym_icon(
                            arrow,
                            SymSize::INLINE_SMALL,
                            if on { k.label } else { k.tertiary },
                        )),
                )
            }))
            .child(div().w(height::LG).flex_none());
        let n = rows.len();
        let now = now_ms();
        let body: Vec<AnyElement> = rows
            .into_iter()
            .enumerate()
            .map(|(i, r)| {
                let id = r.member.id.to_string();
                let selected = self.team.selected.as_deref() == Some(id.as_str());
                let bot = r.member.kind == MemberKind::Bot;
                let sub = match (bot, r.member.role.as_deref()) {
                    (true, Some(role)) => fill(m::TEAM_AI_ROLE, &[("role", role)]),
                    (true, None) => m::TEAM_AI.to_string(),
                    (false, role) => role.unwrap_or_default().to_string(),
                };
                let level = if r.member.level == MemberLevel::Owner {
                    m::LEVEL_OWNER
                } else {
                    m::LEVEL_MEMBER
                };
                let recent = if r.member.deactivated {
                    (m::TEAM_OFF.to_string(), k.secondary)
                } else if let Some(s) = r.ai_state {
                    (
                        s.to_string(),
                        if s == m::TEAM_WAITING {
                            k.amber
                        } else if s == m::TEAM_WORKING {
                            k.green
                        } else {
                            k.secondary
                        },
                    )
                } else {
                    match r.seen {
                        Some(t) if now.saturating_sub(t) < 60_000 => {
                            (m::TEAM_ONLINE_NOW.to_string(), k.secondary)
                        }
                        Some(t) => (model::ago(now.saturating_sub(t)), k.secondary),
                        None => (m::TEAM_NEVER.to_string(), k.secondary),
                    }
                };
                let spend = match r.budget_cents.filter(|b| *b > 0) {
                    Some(b) => fill(
                        m::TEAM_SPEND_OF,
                        &[
                            ("spend", &usd(r.spend)),
                            ("budget", &format!("${}", b / 100)),
                        ],
                    ),
                    None => usd(r.spend),
                };
                let share = match r.budget_cents.filter(|b| *b > 0) {
                    Some(b) => Some((r.spend / (b as f64 / 100.)) as f32),
                    None if bot && max_spend > 0. => Some((r.spend / max_spend) as f32),
                    None => None,
                };
                let more = self.member_menu(&r.member, window, cx);
                let view = cx.entity();
                let sel_id = id.clone();
                div()
                    .id(SharedString::from(format!("team-row-{id}")))
                    .min_h(height::TABLE_ROW)
                    .px(space::S4)
                    .flex()
                    .items_center()
                    .n_body()
                    .n_figures()
                    .when(selected, |d| d.bg(k.selection))
                    .when(i + 1 < n, |d| d.border_b_1().border_color(k.separator))
                    .on_click(move |_, _, cx| {
                        let id = sel_id.clone();
                        view.update(cx, |v, cx| {
                            v.team.selected = Some(id);
                            cx.notify();
                        })
                    })
                    .child(
                        cell(None, false)
                            .gap(space::S12)
                            .child(MemberAvatar::new(Some(&r.member), avatar::SM))
                            .child(
                                div()
                                    .flex_1()
                                    .min_w(gpui::Pixels::ZERO)
                                    .flex()
                                    .flex_col()
                                    .child(TruncText::new(r.member.name.clone()).n_headline())
                                    .when(!sub.is_empty(), |d| {
                                        d.child(
                                            TruncText::new(sub)
                                                .n_footnote()
                                                .text_color(k.secondary),
                                        )
                                    }),
                            ),
                    )
                    .child(cell(Some(t::TEAM_COL_LEVEL), false).child(Badge::new(level)))
                    .child(cell(Some(t::TEAM_COL_TOPICS), true).child(r.topics.to_string()))
                    .child(
                        cell(Some(t::TEAM_COL_SPEND), false).child(
                            div()
                                .w_full()
                                .flex()
                                .flex_col()
                                .gap(space::S4)
                                .child(div().n_headline().child(spend))
                                .children(share.map(|s| progress(s, cx))),
                        ),
                    )
                    .child(
                        cell(Some(t::TEAM_COL_RECENT), false)
                            .text_color(recent.1)
                            .child(recent.0),
                    )
                    .child(
                        div()
                            .w(height::LG)
                            .flex_none()
                            .flex()
                            .justify_center()
                            .child(more),
                    )
                    .into_any_element()
            })
            .collect();
        div()
            .rounded(radius::CONTAINER)
            .border_1()
            .border_color(k.separator)
            .bg(k.admin_panel())
            .overflow_hidden()
            .flex()
            .flex_col()
            .child(head)
            .when(body.is_empty(), |d| {
                d.child(
                    div()
                        .h(height::TABLE_ROW * 2.)
                        .flex()
                        .items_center()
                        .justify_center()
                        .n_body()
                        .text_color(k.secondary)
                        .child(m::TEAM_EMPTY),
                )
            })
            .children(body)
            .into_any_element()
    }

    fn member_menu(&self, x: &Member, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let services = self.services.clone();
        let id = x.id.to_string();
        let off = x.deactivated;
        let view = cx.entity();
        // The roster's member, for the AI member sheets.
        let member = x.is_bot().then(|| x.clone());
        crate::views::settings::widgets::more_button(
            SharedString::from(format!("team-more-{id}")),
            window,
            cx,
        )
        .dropdown_menu(move |menu, _, _| {
            let (services, id, view) = (services.clone(), id.clone(), view.clone());
            // An AI member: 更换电脑… (常用设置) and 编辑… (everything). Without them 团队管理
            // had no way to change an AI member's computer.
            let menu = match member.clone().filter(|_| !off) {
                Some(bot) => {
                    let (s1, b1) = (services.clone(), bot.clone());
                    let (s2, b2) = (services.clone(), bot);
                    menu.item(
                        PopupMenuItem::new(crate::copy::settings::QUICK_MENU).on_click(
                            move |_, window, cx| {
                                crate::views::settings::bot_quick::open_bot_quick(
                                    s1.clone(),
                                    &b1,
                                    window,
                                    cx,
                                )
                            },
                        ),
                    )
                    .item(PopupMenuItem::new(crate::copy::settings::EDIT).on_click(
                        move |_, window, cx| {
                            crate::views::settings::members::open_edit_bot(
                                s2.clone(),
                                &b2,
                                window,
                                cx,
                            );
                        },
                    ))
                    .separator()
                }
                None => menu,
            };
            menu.item(
                PopupMenuItem::new(if off {
                    m::TEAM_REACTIVATE
                } else {
                    m::TEAM_DEACTIVATE
                })
                .on_click(move |_, _, cx| {
                    let reply = services.provider.set_member_deactivated(&id, !off);
                    let view = view.clone();
                    cx.spawn(async move |cx| {
                        let _ = answer(reply).await;
                        let _ = view.update(cx, |v, cx| v.fetch_team(cx));
                    })
                    .detach();
                }),
            )
        })
        .into_any_element()
    }

    fn invites_table(&self, cx: &mut Context<Self>) -> AnyElement {
        let k = native_colors(cx);
        let invites = self
            .team
            .roster
            .as_ref()
            .map(|r| r.invites.clone())
            .unwrap_or_default();
        let now = now_ms();
        let n = invites.len();
        div()
            .rounded(radius::CONTAINER)
            .border_1()
            .border_color(k.separator)
            .bg(k.admin_panel())
            .overflow_hidden()
            .flex()
            .flex_col()
            .when(n == 0, |d| {
                d.child(
                    div()
                        .h(height::TABLE_ROW * 2.)
                        .flex()
                        .items_center()
                        .justify_center()
                        .n_body()
                        .text_color(k.secondary)
                        .child(m::TEAM_NO_INVITES),
                )
            })
            .children(invites.into_iter().enumerate().map(|(i, inv)| {
                let level = if inv.level == "owner" {
                    m::LEVEL_OWNER
                } else {
                    m::LEVEL_MEMBER
                };
                div()
                    .min_h(height::TABLE_ROW)
                    .px(space::S16)
                    .flex()
                    .items_center()
                    .gap(space::S12)
                    .n_body()
                    .when(i + 1 < n, |d| d.border_b_1().border_color(k.separator))
                    .child(
                        div()
                            .flex_1()
                            .min_w(gpui::Pixels::ZERO)
                            .flex()
                            .flex_col()
                            .child(TruncText::new(inv.name.clone()).n_headline())
                            .children(
                                inv.role.clone().map(|r| {
                                    TruncText::new(r).n_footnote().text_color(k.secondary)
                                }),
                            ),
                    )
                    .child(Badge::new(level))
                    .child(
                        div()
                            .w(t::COL_TOKENS)
                            .text_color(k.secondary)
                            .child(self.member_name(&inv.created_by)),
                    )
                    .child(
                        div()
                            .w(t::COL_CPU)
                            .text_right()
                            .n_figures()
                            .text_color(k.secondary)
                            .child(fill(
                                m::TEAM_EXPIRES_IN,
                                &[("d", &model::duration(inv.expires_at.saturating_sub(now)))],
                            )),
                    )
            }))
            .into_any_element()
    }

    fn member_inspector(&self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let _ = window;
        let k = native_colors(cx);
        let Some(id) = self.team.selected.clone() else {
            return card_surface(cx)
                .p(space::S24)
                .n_body()
                .text_color(k.secondary)
                .child(m::TEAM_PICK)
                .into_any_element();
        };
        let Some(x) = self.services.provider.member(&id) else {
            return div().into_any_element();
        };
        let bot = x.kind == MemberKind::Bot;
        let row = self
            .team_rows(cx)
            .into_iter()
            .find(|r| r.member.id.as_str() == id);
        let (topics, spend) = row.as_ref().map(|r| (r.topics, r.spend)).unwrap_or((0, 0.));
        let week_ago = now_ms().saturating_sub(7 * 86_400_000);
        let artifacts = self
            .services
            .provider
            .all_artifacts()
            .iter()
            .filter(|(_, a)| a.created_by.as_str() == id && a.updated_at as u64 >= week_ago)
            .count();
        // Recent 话题: the ones they asked for or worked on, newest first.
        let mut recent: Vec<(String, String, crate::ui::native::BadgeState, String, u64)> = vec![];
        let mut uses: Vec<(String, u32)> = vec![];
        let me = match self.services.provider.session() {
            crate::data::SessionState::SignedIn { me, .. } => Some(me.id.to_string()),
            _ => None,
        };
        for g in self.services.provider.groups() {
            for tp in self.services.provider.topics(&g.id) {
                let runs = self.services.provider.runs(&g.id, &tp.id);
                let mine = tp.created_by.as_str() == id
                    || runs.iter().any(|r| {
                        r.bot_id.as_str() == id || r.requested_by.as_deref() == Some(id.as_str())
                    });
                for r in runs
                    .iter()
                    .filter(|r| r.requested_by.as_deref() == Some(id.as_str()))
                {
                    match uses.iter_mut().find(|u| u.0 == r.bot_id.as_str()) {
                        Some(u) => u.1 += 1,
                        None => uses.push((r.bot_id.to_string(), 1)),
                    }
                }
                if mine {
                    let state = crate::views::topic_page::topic_state(
                        self.services.provider.as_ref(),
                        me.as_deref(),
                        &tp,
                    );
                    recent.push((
                        tp.title.clone(),
                        state.short.clone(),
                        state.badge,
                        state.text.clone(),
                        tp.last_activity_at as u64,
                    ));
                }
            }
        }
        recent.sort_by(|a, b| b.4.cmp(&a.4));
        uses.sort_by(|a, b| b.1.cmp(&a.1));
        // With the server's split of 用量 by person and AI member, count this week's 话题 there:
        // whom a person asks most, or who asks an AI member most.
        let pairs = self
            .team
            .week
            .as_ref()
            .map(|w| w.pairs.as_slice())
            .filter(|p| !p.is_empty());
        let by_topics = pairs.is_some();
        if let Some(pairs) = pairs {
            uses = pairs
                .iter()
                .filter_map(|x| {
                    let other = if bot {
                        (x.member_id == id).then(|| x.initiator_id.clone())
                    } else {
                        (x.initiator_id == id).then(|| x.member_id.clone())
                    };
                    other.map(|o| (o, x.spend.topics))
                })
                .filter(|u| u.1 > 0)
                .collect();
            uses.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
        }
        let max_use = uses.first().map(|u| u.1).unwrap_or(1).max(1);
        let roster = self
            .team
            .roster
            .as_ref()
            .and_then(|r| r.members.iter().find(|r| r.member_id == id));
        let joined = roster.map(|r| {
            let date = super::clock_day(r.joined_at);
            let date = date.split(' ').take(4).collect::<Vec<_>>().join(" ");
            match r.added_by.as_deref() {
                Some(by) => fill(
                    m::TEAM_JOINED_BY,
                    &[("date", &date), ("who", &self.member_name(by))],
                ),
                None => date,
            }
        });
        let level_word = if x.level == MemberLevel::Owner {
            m::LEVEL_OWNER
        } else {
            m::LEVEL_MEMBER
        };
        let services = self.services.clone();
        let view = cx.entity();
        let mid = id.clone();
        let level =
            crate::ui::native::pull_down("team-level", level_word, PushSize::Small, window, cx)
                .dropdown_menu(move |menu, _, _| {
                    let mut menu = menu;
                    for (lv, word) in [
                        (MemberLevel::Owner, m::LEVEL_OWNER),
                        (MemberLevel::Member, m::LEVEL_MEMBER),
                    ] {
                        let (services, id, view) = (services.clone(), mid.clone(), view.clone());
                        menu = menu.item(PopupMenuItem::new(word).on_click(move |_, _, cx| {
                            let reply = services.provider.set_member_level(&id, lv);
                            let view = view.clone();
                            cx.spawn(async move |cx| {
                                let _ = answer(reply).await;
                                let _ = view.update(cx, |v, cx| v.fetch_team(cx));
                            })
                            .detach();
                        }));
                    }
                    menu
                });
        let stat = |v: String, l: &'static str| {
            div()
                .flex_1()
                .min_w(gpui::Pixels::ZERO)
                .p(space::S12)
                .rounded(radius::CONTAINER)
                .bg(k.group)
                .border_1()
                .border_color(k.separator)
                .flex()
                .flex_col()
                .child(
                    div()
                        .n_title3()
                        .font_weight(gpui::FontWeight::SEMIBOLD)
                        .n_figures()
                        .child(v),
                )
                .child(div().n_footnote().text_color(k.secondary).child(l))
        };
        let fact = |l: &'static str, v: AnyElement| {
            div()
                .flex()
                .items_center()
                .justify_between()
                .gap(space::S12)
                .n_body()
                .child(div().text_color(k.secondary).child(l))
                .child(v)
        };
        let devices: Vec<String> = roster
            .map(|r| {
                r.devices
                    .iter()
                    .take(3)
                    .map(|d| d.label.clone())
                    .filter(|l| !l.is_empty())
                    .collect()
            })
            .unwrap_or_default();
        card_surface(cx)
            .p(space::S24)
            .flex()
            .flex_col()
            .gap(space::S16)
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(space::S12)
                    .child(MemberAvatar::new(Some(&x), avatar::LG))
                    .child(
                        div()
                            .flex_1()
                            .min_w(gpui::Pixels::ZERO)
                            .flex()
                            .flex_col()
                            .child(
                                TruncText::new(x.name.clone())
                                    .n_title3()
                                    .font_weight(gpui::FontWeight::SEMIBOLD),
                            )
                            .child(
                                TruncText::new(
                                    [x.role.clone().unwrap_or_default(), level_word.to_string()]
                                        .into_iter()
                                        .filter(|s| !s.is_empty())
                                        .collect::<Vec<_>>()
                                        .join(" · "),
                                )
                                .n_footnote()
                                .text_color(k.secondary),
                            ),
                    ),
            )
            .child(
                div()
                    .flex()
                    .gap(space::S8)
                    .child(stat(topics.to_string(), m::TEAM_STAT_TOPICS))
                    .child(stat(artifacts.to_string(), m::TEAM_STAT_ARTIFACTS))
                    .child(stat(usd(spend), m::TEAM_STAT_SPEND)),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(space::S8)
                    .child(
                        div()
                            .n_section()
                            .text_color(k.secondary)
                            .child(m::TEAM_RECENT_TOPICS),
                    )
                    .when(recent.is_empty(), |d| {
                        d.child(
                            div()
                                .n_body()
                                .text_color(k.secondary)
                                .child(m::TEAM_NO_TOPICS),
                        )
                    })
                    .children(
                        recent
                            .into_iter()
                            .take(4)
                            .map(|(title, short, badge, text, _)| {
                                div()
                                    .flex()
                                    .items_center()
                                    .gap(space::S8)
                                    .child(sym_icon(Sym::Topic, SymSize::INLINE_SMALL, k.secondary))
                                    .child(TruncText::new(title).flex_1().n_body())
                                    .when(!short.is_empty(), |d| {
                                        d.child(
                                            StatusBadge::new(
                                                badge,
                                                if short.is_empty() { text } else { short },
                                            )
                                            .dense(),
                                        )
                                    })
                            }),
                    ),
            )
            .when(!uses.is_empty(), |d| {
                d.child(
                    div()
                        .flex()
                        .flex_col()
                        .gap(space::S8)
                        .child(div().n_section().text_color(k.secondary).child(if bot {
                            m::TEAM_ASKED_BY
                        } else {
                            m::TEAM_USES_MOST
                        }))
                        .children(uses.iter().take(3).map(|(bid, n)| {
                            let member = self.services.provider.member(bid);
                            div()
                                .flex()
                                .items_center()
                                .gap(space::S8)
                                .child(MemberAvatar::new(member.as_ref(), avatar::XS))
                                .child(div().w(relative(0.3)).n_body().child(self.member_name(bid)))
                                .child(
                                    div()
                                        .flex_1()
                                        .child(progress(*n as f32 / max_use as f32, cx)),
                                )
                                .child(div().n_footnote().text_color(k.secondary).child(fill(
                                    if by_topics {
                                        m::HOME_WHO_TOPICS
                                    } else {
                                        m::TEAM_TIMES
                                    },
                                    &[("n", &n.to_string())],
                                )))
                        })),
                )
            })
            .child(
                div()
                    .pt(space::S16)
                    .border_t_1()
                    .border_color(k.separator)
                    .flex()
                    .flex_col()
                    .gap(space::S12)
                    .child(fact(m::TEAM_LEVEL, level.into_any_element()))
                    .children(
                        joined.map(|j| fact(m::TEAM_JOINED, div().child(j).into_any_element())),
                    )
                    .when(bot, |d| {
                        let models = self
                            .team
                            .week
                            .as_ref()
                            .map(|w| super::usage::model_names(w, &id))
                            .unwrap_or_default();
                        d.child(fact(
                            m::TEAM_MODELS,
                            if models.is_empty() {
                                div().child(m::TEAM_NO_MODELS).into_any_element()
                            } else {
                                // One a line: names are long ("Claude Sonnet 5.5").
                                div()
                                    .flex()
                                    .flex_col()
                                    .items_end()
                                    .children(models.into_iter().map(TruncText::new))
                                    .into_any_element()
                            },
                        ))
                    })
                    .when(!bot, |d| {
                        d.child(fact(
                            m::TEAM_DEVICES,
                            TruncText::new(if devices.is_empty() {
                                m::TEAM_NO_DEVICES.to_string()
                            } else {
                                devices.join("、")
                            })
                            .into_any_element(),
                        ))
                    }),
            )
            .into_any_element()
    }
}

fn usd(x: f64) -> String {
    format!("${x:.2}")
}

fn now_ms() -> u64 {
    crate::selftest::clock::local_now()
        .timestamp_millis()
        .max(0) as u64
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(name: &str, spend: f64, topics: u32, owner: bool) -> Row {
        Row {
            member: Member {
                name: name.into(),
                level: if owner {
                    MemberLevel::Owner
                } else {
                    MemberLevel::Member
                },
                id: name.into(),
                kind: MemberKind::Bot,
                role: None,
                avatar_url: None,
                deactivated: false,
                online: false,
                character: None,
            },
            topics,
            spend,
            budget_cents: None,
            seen: None,
            ai_state: None,
        }
    }

    #[test]
    fn headers_sort_and_flip() {
        let rows = vec![
            row("Scout", 61., 38, false),
            row("Forge", 86., 56, true),
            row("Ledger", 32., 21, false),
        ];
        let names = |r: Vec<Row>| r.into_iter().map(|r| r.member.name).collect::<Vec<_>>();
        assert_eq!(
            names(sort_rows(rows.clone(), TeamSortKey::Spend, true)),
            ["Forge", "Scout", "Ledger"]
        );
        assert_eq!(
            names(sort_rows(rows.clone(), TeamSortKey::Topics, false)),
            ["Ledger", "Scout", "Forge"]
        );
        assert_eq!(
            names(sort_rows(rows.clone(), TeamSortKey::Name, false)),
            ["Forge", "Ledger", "Scout"]
        );
        assert_eq!(names(sort_rows(rows, TeamSortKey::Level, true))[0], "Forge");
    }
}
