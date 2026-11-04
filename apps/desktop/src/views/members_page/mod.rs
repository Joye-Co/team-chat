//! 成员 (NATIVE.md §9.5): two panes. The list has its own toolbar row with
//! the segmented filter 全部 / 在做事 / 等人 and 52 pt rows (avatar 32, name,
//! 岗位 · state) in sections (等你, 在做事, 空闲, 同事). The detail reads like
//! a Contacts card: avatar 64, the name as the Large title, 岗位 and who
//! they report to, push buttons (交代… default, 单独问, 暂停), then grouped
//! forms with label/value rows: 现在在做, 这周, 最近交付, 最近的问题, 在哪干活,
//! 需要你批准的事, 能用的工具, 用到的决定. Stats come from the runs cache
//! and `BotRun.usage`; 添加 and 修改 reuse the settings forms.

pub mod scenes;

use std::collections::HashSet;

use gpui::{
    AnyElement, App, Context, InteractiveElement as _, IntoElement, ParentElement, Render,
    SharedString, StatefulInteractiveElement as _, Styled, Subscription, Window, div,
    prelude::FluentBuilder as _, px,
};
use gpui_component::{
    Disableable as _,
    menu::{ContextMenuExt as _, DropdownMenu as _, PopupMenu, PopupMenuItem},
};

use crate::ui::native::tokens::space as tsp;
use crate::{
    app::{Place, RightPane, Services},
    copy::{fill, members_page as c},
    data::{
        BotRunSummary, GroupKind, Member, PlanState,
        pages::{DeliveryKind, DeliveryState, MemberExtras},
        v2::{Currency, MemberStats},
    },
    ui::{
        native::{
            ListRow, NativeType as _, PushSize, PushStyle, Segment, Segmented, ToolbarGroup,
            ToolbarItemKind, ToolbarRow, borderless_button, disabled_push, emphasized,
            native_colors,
            page::{self, PageLoad, Status},
            push_button, toolbar_item,
        },
        sf_symbols::{Sf, Sym},
    },
    views::{
        group_page::{
            kit,
            rows::{self, AVATAR, RowInk, mail_row},
        },
        shell::{PageView, TitleBarSpec, member_name, panes::watch},
    },
};

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
    // --- v12/behavior ---
    /// The list's key focus: ↑/↓ move the selection, Return opens it.
    focus: gpui::FocusHandle,
    // --- end v12/behavior ---
    filter: Filter,
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
            focus: cx.focus_handle(),
            filter: Filter::All,
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

/// The list pane (§9.5): its own toolbar row with the segmented filter.
const LIST_WIDTH: gpui::Pixels = px(320.);
/// The measure of the member detail.
const DETAIL_MEASURE: gpui::Pixels = px(880.);
/// The width of the 成本 bar.
const COST_BAR: gpui::Pixels = px(120.);

fn machine_sym(name: &str) -> Sym {
    let n = name.to_lowercase();
    if n.contains("mini") {
        Sym::MacMini
    } else if n.contains("book") || n.contains("笔记本") {
        Sym::Laptop
    } else {
        Sym::Computer
    }
}

impl MembersPage {
    fn list(
        &self,
        entries: &[Entry],
        selected: Option<&str>,
        load: PageLoad,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let k = native_colors(cx);
        let me = self.services.me();
        let owner = me
            .as_deref()
            .and_then(|m| self.services.provider.member(m))
            .is_some_and(|m| m.level == crate::data::MemberLevel::Owner);
        let services = self.services.clone();
        let add = toolbar_item(
            "members-add",
            Sf::Invite,
            c::MEMBERS_ADD,
            ToolbarItemKind::Plain,
            emphasized(window),
            window,
            cx,
        )
        .disabled(!owner)
        .dropdown_menu_with_anchor(gpui::Anchor::TopRight, move |menu, _, _| {
            let invite = services.clone();
            let bot = services.clone();
            menu.item(
                PopupMenuItem::new(c::MEMBERS_INVITE_MORE)
                    .icon(gpui_component::Icon::new(Sym::Invite))
                    .on_click(move |_, window, cx| {
                        crate::views::settings::open_invite(invite.clone(), window, cx);
                    }),
            )
            .item(
                PopupMenuItem::new(c::MEMBERS_ADD_BOT_MORE)
                    .icon(gpui_component::Icon::new(Sym::AiMember))
                    .on_click(move |_, window, cx| {
                        crate::views::settings::open_add_bot(bot.clone(), window, cx);
                    }),
            )
        });
        let filter = Segmented::new(
            "members-filter",
            [
                Segment::Label(c::MEMBERS_FILTER_ALL.into()),
                Segment::Label(c::MEMBERS_FILTER_WORKING.into()),
                Segment::Label(c::MEMBERS_FILTER_WAITING.into()),
            ],
        )
        .toolbar()
        .selected(match self.filter {
            Filter::All => 0,
            Filter::Working => 1,
            Filter::Waiting => 2,
        })
        .on_change({
            let this = cx.entity().downgrade();
            move |ix, _, cx| {
                let _ = this.update(cx, |this, cx| {
                    this.filter = match ix {
                        1 => Filter::Working,
                        2 => Filter::Waiting,
                        _ => Filter::All,
                    };
                    cx.notify();
                });
            }
        });
        let mut row = ToolbarRow::new(c::MEMBERS_PAGE_TITLE)
            .leading_inset(rows::toolbar_leading(&self.services, cx))
            .group(filter)
            .group(rows::toolbar_capsule(add, cx));
        if load == PageLoad::Ready {
            row = row.subtitle(fill(c::MEMBERS_COUNT, &[("n", &entries.len().to_string())]));
        }
        let toolbar = div()
            .flex_none()
            .border_b_1()
            .border_color(k.separator)
            .child(row);

        let shown = |b: Bucket| match self.filter {
            Filter::All => true,
            Filter::Working => b == Bucket::Working,
            Filter::Waiting => b == Bucket::Waiting,
        };
        let focused = self.focus.is_focused(window);
        let active = emphasized(window);
        let mut list = div()
            .id("members-list")
            .flex_1()
            .min_h(px(0.))
            .overflow_y_scroll()
            .pb(tsp::S12)
            .flex()
            .flex_col();
        let mut any = false;
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
            any = true;
            let label = match bucket {
                Bucket::Waiting => c::MEMBERS_SECTION_WAITING,
                Bucket::Working => c::MEMBERS_SECTION_WORKING,
                Bucket::Idle => c::MEMBERS_SECTION_IDLE,
                Bucket::People => c::MEMBERS_SECTION_PEOPLE,
            };
            // Section style (§3): 11 pt semibold `secondary`, like a sidebar.
            list = list.child(
                div()
                    .flex_none()
                    .h(px(28.))
                    .mt(tsp::S6)
                    .px(rows::LEADING)
                    .flex()
                    .items_end()
                    .pb(tsp::S4)
                    .n_section()
                    .text_color(k.secondary)
                    .child(label),
            );
            let n = group.len();
            for (i, e) in group.into_iter().enumerate() {
                list = list.child(self.row(
                    e,
                    selected,
                    me.as_deref(),
                    focused,
                    active,
                    i + 1 == n,
                    cx,
                ));
            }
        }
        if load != PageLoad::Ready {
            list = div()
                .id("members-list")
                .flex_1()
                .min_h(px(0.))
                .p(tsp::S8)
                .child(page::skeleton_rows(
                    "members-sk",
                    page::SkeletonKind::Row,
                    6,
                    cx,
                ));
        } else if !any {
            let this = cx.entity().downgrade();
            list = list.child(page::page_empty(
                Sym::Filter,
                c::MEMBERS_FILTER_EMPTY,
                c::MEMBERS_FILTER_EMPTY_BODY,
                vec![
                    push_button(
                        "members-filter-all",
                        c::MEMBERS_FILTER_ALL,
                        PushStyle::Standard,
                        PushSize::Small,
                        cx,
                    )
                    .on_click(move |_, _, cx| {
                        let _ = this.update(cx, |this, cx| {
                            this.filter = Filter::All;
                            cx.notify();
                        });
                    })
                    .into_any_element(),
                ],
                cx,
            ));
        }
        div()
            .flex_none()
            .w(LIST_WIDTH)
            .h_full()
            .flex()
            .flex_col()
            .border_r_1()
            .border_color(k.separator)
            .child(toolbar)
            .child(list)
            .into_any_element()
    }

    #[allow(clippy::too_many_arguments)]
    fn row(
        &self,
        e: &Entry,
        selected: Option<&str>,
        me: Option<&str>,
        focused: bool,
        active: bool,
        last: bool,
        cx: &App,
    ) -> AnyElement {
        let k = native_colors(cx);
        let p = &self.services.provider;
        let m = &e.member;
        let is_selected = selected == Some(m.id.as_str());
        // D10: the selected row is the muted fill, focused or not; never an
        // inverted row.
        let _ = (focused, active);
        let ink = RowInk::new(ListRow::selection_ink(is_selected, false, false, cx), cx);
        let on_accent = ink.fill == Some(k.accent);
        let mut parts: Vec<String> = m.role.clone().into_iter().collect();
        if me == Some(m.id.as_str()) {
            parts.push(c::MEMBERS_YOU_WORD.to_string());
        }
        let _ = on_accent;
        // The state is a StatusBadge on the title line (§2.12), never
        // colored text.
        let mut badge: Option<AnyElement> = None;
        match e.bucket {
            Bucket::Waiting => {
                badge = Some(page::status_badge(
                    Status::NeedsYou,
                    c::MEMBERS_WAITS_ON_YOU,
                    cx,
                ));
                parts.extend(e.extras.line.clone().or(e.ask.clone()));
            }
            Bucket::Working => {
                let run = e.run.as_ref();
                let topic = run
                    .and_then(|r| p.topic(&r.group_id, &r.topic_id))
                    .map(|t| t.title);
                parts.extend(topic);
                let text = match run.and_then(|r| kit::plan_progress(&r.plan)) {
                    Some((n, mm)) => fill(
                        c::MEMBERS_WORKING_N,
                        &[("n", &n.to_string()), ("m", &mm.to_string())],
                    ),
                    None => c::MEMBERS_WORKING.to_string(),
                };
                badge = Some(page::status_badge(Status::Working, text, cx));
            }
            Bucket::Idle => {
                parts.push(
                    e.extras
                        .line
                        .clone()
                        .unwrap_or_else(|| c::MEMBERS_IDLE.to_string()),
                );
            }
            Bucket::People => {
                parts.push(e.extras.line.clone().unwrap_or_else(|| {
                    if m.online {
                        c::MEMBERS_ONLINE.to_string()
                    } else {
                        c::MEMBERS_OFFLINE.to_string()
                    }
                }));
            }
        }
        parts.retain(|p| !p.trim().is_empty());
        let mark = badge;
        let second = rows::line(parts.join(" · ")).into_any_element();
        let nav = self.services.nav.clone();
        let id = m.id.clone();
        mail_row(
            SharedString::from(format!("member-{}", m.id)),
            rows::MEMBER_ROW,
            rows::avatar(Some(m), AVATAR, cx),
            m.name.clone(),
            mark,
            second,
            None,
            !last && !is_selected,
            ink,
            cx,
        )
        .on_click(move |_, _, cx| {
            let id = id.clone();
            nav.update(cx, |nav, cx| nav.open_members(Some(id), cx))
        })
        .context_menu({
            let (services, member, run) = (self.services.clone(), m.clone(), e.run.clone());
            move |menu, _, _| member_menu(menu, &services, &member, run.as_ref())
        })
        .into_any_element()
    }

    /// The detail toolbar: no title (the name is the detail's Large title,
    /// §3); 产出 and the member's computer as toolbar items.
    fn detail_toolbar(&self, e: &Entry, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let active = emphasized(window);
        let nav = self.services.nav.clone();
        let mut items = vec![
            toolbar_item(
                "member-outputs",
                Sf::Outputs,
                c::MEMBERS_OUTPUTS_BUTTON,
                ToolbarItemKind::Plain,
                active,
                window,
                cx,
            )
            .on_click(move |_, _, cx| nav.update(cx, |n, cx| n.open_artifacts(cx))),
        ];
        if e.stats.machine.is_some() {
            let run = e.run.clone();
            let nav = self.services.nav.clone();
            items.push(
                toolbar_item(
                    "member-computer",
                    Sf::Computer,
                    c::MEMBERS_COMPUTER_BUTTON,
                    ToolbarItemKind::Plain,
                    active,
                    window,
                    cx,
                )
                .disabled(run.is_none())
                .on_click(move |_, _, cx| {
                    let Some(r) = run.clone() else { return };
                    nav.update(cx, |nav, cx| {
                        nav.open_topic(r.group_id, r.topic_id, cx);
                        nav.set_right_pane(RightPane::Live(r.id), cx);
                    })
                }),
            );
        }
        ToolbarRow::new("")
            .group(ToolbarGroup::new(items))
            .into_any_element()
    }

    /// The header block (§2.18): avatar 64, the name (24), one muted line
    /// (岗位, who they report to, their 群), and the actions at the right:
    /// 交代 (primary), 单独问 and 暂停 for an AI member, 私聊 for a person.
    fn identity(&self, e: &Entry, cx: &App) -> AnyElement {
        let k = native_colors(cx);
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
        let mut parts: Vec<String> = m.role.clone().into_iter().collect();
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
        let mut actions: Vec<AnyElement> = Vec::new();
        if m.is_bot() {
            let run = e.run.clone();
            let services = self.services.clone();
            let side = {
                let nav = self.services.nav.clone();
                let run = e.run.clone();
                let member = m.id.clone();
                move |_: &gpui::ClickEvent, _: &mut Window, cx: &mut App| {
                    let Some(r) = run.clone() else { return };
                    let member = member.clone();
                    nav.update(cx, |nav, cx| {
                        nav.open_topic(r.group_id, r.topic_id, cx);
                        nav.set_right_pane(RightPane::SideChat(member), cx);
                    })
                }
            };
            actions.push(
                disabled_push(
                    push_button(
                        "member-pause",
                        c::MEMBERS_PAUSE,
                        PushStyle::Standard,
                        PushSize::Small,
                        cx,
                    )
                    .on_click(move |_, _, _| {
                        if let Some(r) = &run {
                            let _ =
                                services
                                    .provider
                                    .take_over_run(&r.group_id, &r.topic_id, &r.id);
                        }
                    }),
                    e.run.is_none(),
                    cx,
                )
                .into_any_element(),
            );
            actions.push(
                disabled_push(
                    push_button(
                        "member-side-chat",
                        c::MEMBERS_SIDE_CHAT,
                        PushStyle::Standard,
                        PushSize::Small,
                        cx,
                    )
                    .on_click(side),
                    e.run.is_none(),
                    cx,
                )
                .into_any_element(),
            );
            actions.push(
                push_button(
                    "member-hand-over",
                    c::MEMBERS_HAND_OVER_SHORT,
                    PushStyle::Default,
                    PushSize::Small,
                    cx,
                )
                .on_click(|_, window, cx| {
                    window.dispatch_action(Box::new(crate::views::nav::QuickSwitch), cx)
                })
                .into_any_element(),
            );
        } else if self.services.me().as_deref() != Some(m.id.as_str()) {
            let id = m.id.clone();
            actions.push(
                push_button(
                    "member-message",
                    c::MEMBERS_MESSAGE,
                    PushStyle::Default,
                    PushSize::Small,
                    cx,
                )
                .on_click(move |_, _, cx| crate::views::nav::open_dm_with(id.clone(), cx))
                .into_any_element(),
            );
        }
        div()
            .flex()
            .items_center()
            .gap(tsp::S16)
            .child(rows::avatar(Some(m), px(64.), cx))
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.))
                    .flex()
                    .flex_col()
                    .gap(tsp::S4)
                    .child(
                        rows::line(m.name.clone())
                            .n_large_title()
                            .text_color(k.label),
                    )
                    .when(!parts.is_empty(), |el| {
                        el.child(
                            rows::line(parts.join(" · "))
                                .n_body()
                                .text_color(k.secondary),
                        )
                    }),
            )
            .child(
                div()
                    .flex_none()
                    .flex()
                    .items_center()
                    .gap(tsp::S8)
                    .children(actions),
            )
            .into_any_element()
    }

    /// 现在在做 (card): the run's 话题 with its StatusBadge and Progress,
    /// the step in progress, then what waits in the queue. A compact empty
    /// when nothing runs.
    fn now_section(&self, e: &Entry, cx: &App) -> AnyElement {
        let k = native_colors(cx);
        let p = &self.services.provider;
        let mut body = div().flex().flex_col();
        let mut rows_n = 0;
        if let Some(r) = &e.run {
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
            let (badge, value) = match kit::plan_progress(&r.plan) {
                Some((n, m)) => (
                    fill(
                        c::MEMBERS_WORKING_N,
                        &[("n", &n.to_string()), ("m", &m.to_string())],
                    ),
                    if m == 0 { 0. } else { n as f32 / m as f32 },
                ),
                None => (c::MEMBERS_WORKING.to_string(), 0.),
            };
            let nav = self.services.nav.clone();
            let (g, t) = (r.group_id.clone(), r.topic_id.clone());
            body = body.child(
                page::card_row(
                    "member-now-topic",
                    None,
                    topic.map(|t| t.title).unwrap_or_default(),
                    Some(fill(c::MEMBERS_TOPIC_META, &[("group", &group), ("name", &by)]).into()),
                    Some(page::status_badge(Status::Working, badge, cx)),
                    true,
                    cx,
                )
                .cursor_pointer()
                .on_click(move |_, _, cx| {
                    let (g, t) = (g.clone(), t.clone());
                    nav.update(cx, |n, cx| n.open_topic(g, t, cx))
                }),
            );
            body = body.child(div().pb(tsp::S8).child(page::progress(
                "member-now-progress",
                value,
                cx,
            )));
            rows_n += 1;
            if let Some(i) = current {
                body = body.child(page::card_row(
                    "member-now-step",
                    Some(page::row_icon(Sym::Spinner, k.blue)),
                    r.plan[i].text.clone(),
                    None,
                    Some(
                        div()
                            .n_callout()
                            .n_figures()
                            .text_color(k.secondary)
                            .child(kit::elapsed(p.clock_now(), since))
                            .into_any_element(),
                    ),
                    e.extras.queued.is_empty(),
                    cx,
                ));
            }
        }
        let nq = e.extras.queued.len();
        for (i, q) in e.extras.queued.iter().enumerate() {
            rows_n += 1;
            body = body.child(page::card_row(
                SharedString::from(format!("member-queued-{i}")),
                Some(page::row_icon(Sym::Waiting, k.secondary)),
                q.title.clone(),
                q.note.clone().map(Into::into),
                Some(page::status_badge(Status::Queued, c::MEMBERS_QUEUED, cx)),
                i + 1 == nq,
                cx,
            ));
        }
        if rows_n == 0 {
            body = body.child(page::compact_empty(
                Sym::Circle,
                c::MEMBERS_NOW_EMPTY,
                None,
                cx,
            ));
        }
        page::titled_card(
            "member-now",
            c::MEMBERS_NOW_DOING,
            None,
            None,
            body,
            None,
            cx,
        )
        .into_any_element()
    }

    /// 这周 (card): three stats (完成的话题, 产出, 成本) and the 成本 Progress
    /// against the limit.
    fn week_section(&self, e: &Entry, cx: &App) -> AnyElement {
        let k = native_colors(cx);
        let avg = e.extras.avg_topic_ms.map(|ms| {
            SharedString::from(fill(
                c::MEMBERS_AVG_MINUTES,
                &[("m", &(ms / 60_000).max(1).to_string())],
            ))
        });
        let outputs = e
            .stats
            .artifacts_this_week
            .max(e.extras.prs_this_week + e.extras.pages_this_week);
        let kinds = fill(
            c::MEMBERS_OUTPUT_KINDS,
            &[
                ("prs", &e.extras.prs_this_week.to_string()),
                ("pages", &e.extras.pages_this_week.to_string()),
            ],
        );
        // No run reported a cost: `–`, never a wrong ¥0.
        let currency = e.stats.cost_currency;
        let cost = e.stats.cost_cents_this_week;
        let budget = e.extras.budget_cents.filter(|b| *b > 0);
        let pct = cost
            .zip(budget)
            .map(|(c, b)| (c as f32 / b as f32).clamp(0., 1.));
        let limit_note = match budget {
            Some(b) => fill(c::MEMBERS_LIMIT, &[("v", &money(b, currency))]),
            None => c::MEMBERS_NO_LIMIT.to_string(),
        };
        let cost_text = cost
            .map(|c| money(c, currency))
            .unwrap_or_else(|| c::MEMBERS_NO_COST.to_string());
        let strip = div()
            .flex()
            .gap(tsp::S16)
            .child(page::stat(
                c::MEMBERS_DONE_TOPICS,
                e.stats.done_this_week.to_string(),
                avg,
                cx,
            ))
            .child(div().flex_none().w(px(1.)).bg(k.separator))
            .child(page::stat(
                c::MEMBERS_OUTPUTS,
                outputs.to_string(),
                Some(kinds.into()),
                cx,
            ))
            .child(div().flex_none().w(px(1.)).bg(k.separator))
            .child(page::stat(
                c::MEMBERS_COST,
                cost_text,
                Some(limit_note.into()),
                cx,
            ));
        let body = div()
            .flex()
            .flex_col()
            .gap(tsp::S16)
            .child(strip)
            .children(pct.map(|p| page::progress("member-cost", p, cx)));
        page::titled_card(
            "member-week",
            c::MEMBERS_THIS_WEEK,
            None,
            None,
            body,
            None,
            cx,
        )
        .into_any_element()
    }

    /// 最近交付 (card with a table): 名称, 在哪, 时间, 状态.
    fn deliveries(&self, e: &Entry, cx: &App) -> AnyElement {
        let k = native_colors(cx);
        let p = &self.services.provider;
        let now = p.clock_now();
        let body: AnyElement = if e.extras.deliveries.is_empty() {
            page::compact_empty(Sym::Document, c::MEMBERS_DELIVERIES_EMPTY, None, cx)
                .into_any_element()
        } else {
            let cols = [
                page::Col {
                    header: c::MEMBERS_COL_NAME.into(),
                    width: None,
                    right: false,
                },
                page::Col {
                    header: c::MEMBERS_COL_WHERE.into(),
                    width: Some(px(120.)),
                    right: false,
                },
                page::Col {
                    header: c::MEMBERS_COL_TIME.into(),
                    width: Some(px(96.)),
                    right: true,
                },
                page::Col {
                    header: c::MEMBERS_COL_STATE.into(),
                    width: Some(px(104.)),
                    right: true,
                },
            ];
            let rows = e
                .extras
                .deliveries
                .iter()
                .map(|d| {
                    let group = d
                        .topic
                        .as_ref()
                        .and_then(|(g, _)| p.groups().into_iter().find(|x| &x.id == g))
                        .map(|g| g.name);
                    let (glyph, title, place) = match &d.kind {
                        DeliveryKind::Pr { number, .. } => (
                            if d.state == Some(DeliveryState::Merged) {
                                Sym::Merge
                            } else {
                                Sym::PullRequest
                            },
                            fill(
                                c::MEMBERS_PR,
                                &[("n", &number.to_string()), ("title", &d.title)],
                            ),
                            group.unwrap_or_default(),
                        ),
                        DeliveryKind::Artifact {
                            artifact_id,
                            version,
                        } => {
                            let kind = p
                                .artifact(artifact_id)
                                .map(|a| crate::views::group_page::kind_label(a.summary.kind))
                                .unwrap_or(crate::copy::group_page::GROUP_PAGE_KIND_PAGE);
                            (
                                Sym::Document,
                                fill(
                                    c::MEMBERS_ARTIFACT,
                                    &[("title", &d.title), ("v", &version.to_string())],
                                ),
                                group
                                    .map(|g| format!("{g} · {kind}"))
                                    .unwrap_or(kind.to_string()),
                            )
                        }
                    };
                    let state = match d.state {
                        Some(DeliveryState::WaitsOnMe) => {
                            page::status_badge(Status::NeedsYou, c::MEMBERS_WAITS_REVIEW_WORD, cx)
                        }
                        Some(DeliveryState::Open) => {
                            page::status_badge(Status::Queued, c::MEMBERS_UNMERGED, cx)
                        }
                        Some(DeliveryState::Merged) => {
                            page::status_badge(Status::Done, c::MEMBERS_MERGED, cx)
                        }
                        None => page::status_badge(Status::Done, c::MEMBERS_DELIVERED, cx),
                    };
                    let name = div()
                        .min_w(px(0.))
                        .flex()
                        .items_center()
                        .gap(tsp::S8)
                        .child(div().flex_none().child(page::row_icon(glyph, k.secondary)))
                        .child(page::cell_text(title, false, cx))
                        .into_any_element();
                    let nav = self.services.nav.clone();
                    let kind = d.kind.clone();
                    let topic = d.topic.clone();
                    let click: Box<dyn Fn(&gpui::ClickEvent, &mut Window, &mut App)> =
                        Box::new(move |_, _, cx| match (&kind, &topic) {
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
                    (
                        vec![
                            name,
                            page::cell_text(place, true, cx),
                            div()
                                .n_figures()
                                .text_color(k.secondary)
                                .child(kit::day_and_time(now, d.at, true))
                                .into_any_element(),
                            state,
                        ],
                        Some(click),
                    )
                })
                .collect();
            page::frame(cx)
                .child(page::table("member-deliveries", &cols, rows, cx))
                .into_any_element()
        };
        page::titled_card(
            "member-deliveries",
            c::MEMBERS_RECENT_DELIVERED,
            None,
            None,
            body,
            None,
            cx,
        )
        .into_any_element()
    }

    /// 最近的问题 (card): each problem as an Alert with `重试`.
    fn problems(&self, e: &Entry, cx: &mut Context<Self>) -> AnyElement {
        let now = self.services.provider.clock_now();
        let name = e.member.name.clone();
        let mut body = div().flex().flex_col().gap(tsp::S8);
        if e.stats.problems.is_empty() {
            body = body.child(page::compact_empty(
                Sym::Done,
                c::MEMBERS_PROBLEMS_EMPTY,
                None,
                cx,
            ));
        }
        for (i, pr) in e.stats.problems.iter().enumerate() {
            let retried = pr.run_id.as_ref().is_some_and(|r| self.retried.contains(r));
            if retried {
                body = body.child(page::page_alert(
                    SharedString::from(format!("member-problem-{i}")),
                    page::AlertKind::Info,
                    fill(c::MEMBERS_RETRIED, &[("name", &name)]),
                    Some(c::MEMBERS_RETRIED_NOTE.into()),
                    None,
                    cx,
                ));
                continue;
            }
            let member = e.member.id.clone();
            let retry = pr.run_id.clone().map(|run| {
                push_button(
                    SharedString::from(format!("member-retry-{i}")),
                    c::MEMBERS_RETRY,
                    PushStyle::Standard,
                    PushSize::Small,
                    cx,
                )
                .on_click(cx.listener(move |this, _, _, cx| {
                    let rx = this.services.provider.retry_member_problem(&member, &run);
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
                }))
                .into_any_element()
            });
            body = body.child(page::page_alert(
                SharedString::from(format!("member-problem-{i}")),
                page::AlertKind::Error,
                pr.text.clone(),
                Some(
                    format!(
                        "{} · {}",
                        c::MEMBERS_STUCK,
                        kit::day_and_time(now, pr.at, true)
                    )
                    .into(),
                ),
                retry,
                cx,
            ));
        }
        page::titled_card(
            "member-problems",
            c::MEMBERS_RECENT_PROBLEMS,
            None,
            None,
            body,
            None,
            cx,
        )
        .into_any_element()
    }

    /// 在哪干活 (card): the team computer (or cloud runner) with its
    /// StatusBadge and a way to watch it.
    fn workplace(&self, e: &Entry, window: &Window, cx: &App) -> AnyElement {
        let body: AnyElement = match e.stats.machine.as_ref() {
            None => page::compact_empty(Sym::Computer, c::MEMBERS_WHERE_EMPTY, None, cx)
                .into_any_element(),
            Some(mc) => {
                let badge = if mc.busy {
                    page::status_badge(Status::Working, c::MEMBERS_IN_USE, cx)
                } else if mc.online {
                    page::status_badge(Status::Online, c::MEMBERS_MACHINE_IDLE, cx)
                } else {
                    page::status_badge(Status::Offline, c::MEMBERS_MACHINE_OFFLINE, cx)
                };
                let detail = mc
                    .disk_used_pct
                    .map(|pct| SharedString::from(format!("{} {pct}%", c::MEMBERS_DISK)));
                let open = e.run.clone().map(|r| {
                    let nav = self.services.nav.clone();
                    borderless_button(
                        "member-see-computer",
                        Sf::Open,
                        fill(c::MEMBERS_SEE_COMPUTER, &[("name", &e.member.name)]),
                        window,
                        cx,
                    )
                    .on_click(move |_, _, cx| {
                        let r = r.clone();
                        nav.update(cx, |nav, cx| {
                            nav.open_topic(r.group_id, r.topic_id, cx);
                            nav.set_right_pane(RightPane::Live(r.id), cx);
                        })
                    })
                    .into_any_element()
                });
                page::card_row(
                    "member-machine",
                    Some(page::icon_box(machine_sym(&mc.name), None, cx)),
                    mc.name.clone(),
                    detail,
                    Some(
                        div()
                            .flex()
                            .items_center()
                            .gap(tsp::S8)
                            .child(badge)
                            .children(open)
                            .into_any_element(),
                    ),
                    true,
                    cx,
                )
                .into_any_element()
            }
        };
        page::titled_card("member-where", c::MEMBERS_WHERE, None, None, body, None, cx)
            .into_any_element()
    }

    /// 需要你批准的事 (card): the list, the note as the description and
    /// `修改…` in the footer.
    fn approvals(&self, e: &Entry, cx: &App) -> AnyElement {
        let k = native_colors(cx);
        let services = self.services.clone();
        let n = e.stats.needs_approval.len();
        let body: AnyElement = if n == 0 {
            page::compact_empty(Sym::Lock, c::MEMBERS_APPROVALS_EMPTY, None, cx).into_any_element()
        } else {
            div()
                .flex()
                .flex_col()
                .children(e.stats.needs_approval.iter().enumerate().map(|(i, a)| {
                    page::card_row(
                        SharedString::from(format!("member-approval-{i}")),
                        Some(page::row_icon(Sym::Lock, k.secondary)),
                        a.clone(),
                        None,
                        None,
                        i + 1 == n,
                        cx,
                    )
                }))
                .into_any_element()
        };
        let edit = push_button(
            "member-approvals-edit",
            c::MEMBERS_EDIT_MORE,
            PushStyle::Standard,
            PushSize::Small,
            cx,
        )
        .on_click(move |_, _, cx| crate::app::open_settings_window(services.clone(), cx));
        page::titled_card(
            "member-approvals",
            c::MEMBERS_NEEDS_APPROVAL,
            Some(fill(c::MEMBERS_APPROVAL_NOTE, &[("name", &e.member.name)]).into()),
            Some(edit.into_any_element()),
            body,
            None,
            cx,
        )
        .into_any_element()
    }

    /// 能用的工具 (card).
    fn tools(&self, e: &Entry, cx: &App) -> AnyElement {
        let k = native_colors(cx);
        let n = e.extras.tools.len();
        let body: AnyElement = if n == 0 {
            page::compact_empty(Sym::General, c::MEMBERS_TOOLS_EMPTY, None, cx).into_any_element()
        } else {
            div()
                .flex()
                .flex_col()
                .children(e.extras.tools.iter().enumerate().map(|(i, t)| {
                    page::card_row(
                        SharedString::from(format!("member-tool-{i}")),
                        None,
                        t.name.clone(),
                        t.detail.clone().map(Into::into),
                        t.access.clone().map(|a| {
                            div()
                                .n_callout()
                                .text_color(k.secondary)
                                .child(a)
                                .into_any_element()
                        }),
                        i + 1 == n,
                        cx,
                    )
                }))
                .into_any_element()
        };
        page::titled_card("member-tools", c::MEMBERS_TOOLS, None, None, body, None, cx)
            .into_any_element()
    }

    /// 用到的决定 (card).
    fn decisions(&self, e: &Entry, cx: &App) -> AnyElement {
        let k = native_colors(cx);
        let p = &self.services.provider;
        let now = p.clock_now();
        let n = e.extras.decisions.len();
        let body: AnyElement = if n == 0 {
            page::compact_empty(Sym::Check, c::MEMBERS_DECISIONS_EMPTY, None, cx).into_any_element()
        } else {
            div()
                .flex()
                .flex_col()
                .children(e.extras.decisions.iter().enumerate().map(|(i, d)| {
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
                    page::card_row(
                        SharedString::from(format!("member-decision-{i}")),
                        Some(rows::avatar(who.as_ref(), tsp::S24, cx)),
                        d.text.clone(),
                        Some(
                            format!(
                                "{} · {place}",
                                who.as_ref().map(|w| w.name.clone()).unwrap_or_default()
                            )
                            .into(),
                        ),
                        Some(
                            div()
                                .n_callout()
                                .n_figures()
                                .text_color(k.secondary)
                                .child(when)
                                .into_any_element(),
                        ),
                        i + 1 == n,
                        cx,
                    )
                    .cursor_pointer()
                    .on_click(move |_, _, cx| {
                        let g = g.clone();
                        match t.clone() {
                            Some(t) => nav.update(cx, |n, cx| n.open_topic(g, t, cx)),
                            None => nav.update(cx, |n, cx| n.open_group(g, cx)),
                        }
                    })
                }))
                .into_any_element()
        };
        page::titled_card(
            "member-decisions",
            c::MEMBERS_DECISIONS,
            None,
            None,
            body,
            None,
            cx,
        )
        .into_any_element()
    }

    /// The detail body: the header block, a line, then the cards in pairs
    /// that sit side by side when the pane is wide enough (§2.18) and
    /// stack when it is not.
    fn detail_body(&self, e: &Entry, window: &mut Window, cx: &mut Context<Self>) -> gpui::Div {
        let k = native_colors(cx);
        let pair = |a: AnyElement, b: Option<AnyElement>| {
            div()
                .flex()
                .flex_wrap()
                .gap(page::m::GRID_GAP)
                .child(div().flex_1().min_w(CARD_MIN).flex().flex_col().child(a))
                .children(b.map(|b| div().flex_1().min_w(CARD_MIN).flex().flex_col().child(b)))
        };
        let now = self.now_section(e, cx);
        let week = e.member.is_bot().then(|| self.week_section(e, cx));
        let problems = self.problems(e, cx);
        let workplace = self.workplace(e, window, cx);
        let approvals = self.approvals(e, cx);
        let tools = self.tools(e, cx);
        let deliveries = self.deliveries(e, cx);
        let decisions = self.decisions(e, cx);
        div()
            .w_full()
            .flex()
            .flex_col()
            .gap(page::m::GUTTER)
            .child(self.identity(e, cx))
            .child(div().h(px(1.)).bg(k.separator))
            .child(pair(now, week))
            .child(deliveries)
            .child(pair(problems, Some(workplace)))
            .child(pair(approvals, Some(tools)))
            .child(decisions)
    }

    /// The detail while the first sync runs: the header block and two
    /// cards as skeletons.
    fn detail_skeleton(&self, cx: &App) -> gpui::Div {
        use gpui_component::skeleton::Skeleton;
        div()
            .w_full()
            .flex()
            .flex_col()
            .gap(page::m::GUTTER)
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(tsp::S16)
                    .child(Skeleton::new().size(px(64.)).rounded_full())
                    .child(
                        div()
                            .flex_1()
                            .flex()
                            .flex_col()
                            .gap(tsp::S8)
                            .child(Skeleton::new().h(tsp::S24).w(px(160.)).rounded(tsp::S8))
                            .child(Skeleton::new().h(px(14.)).w(px(280.)).rounded(tsp::S8)),
                    ),
            )
            .child(page::skeleton_cards(2, cx))
            .child(page::skeleton_rows(
                "member-sk",
                page::SkeletonKind::Row,
                3,
                cx,
            ))
    }
}

/// The narrowest card of a detail pair before the pair stacks.
const CARD_MIN: gpui::Pixels = px(320.);

impl Render for MembersPage {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let k = native_colors(cx);
        let entries = self.entries();
        // Only me in the cache: the first sync has not brought the team yet.
        let load = PageLoad::of(&self.services, entries.len() > 1);
        let selected = self.selected(cx);
        let list = self.list(&entries, selected.as_deref(), load, window, cx);
        let entry = selected
            .as_deref()
            .and_then(|id| entries.iter().find(|e| e.member.id == id))
            .filter(|_| load == PageLoad::Ready);
        crate::selftest::drawn::mark(
            "members-page",
            format!(
                "entries={} detail={}",
                entries.len(),
                entry.map_or("", |e| e.member.name.as_str())
            ),
        );
        let scroll = |body: gpui::Div| {
            div()
                .id("member-detail")
                .flex_1()
                .min_h(px(0.))
                .overflow_y_scroll()
                .px(page::m::EDGE)
                .pt(page::m::GUTTER)
                .pb(page::m::GUTTER)
                .child(body)
        };
        let toolbar_band = |toolbar: AnyElement| {
            div()
                .flex_none()
                .border_b_1()
                .border_color(k.separator)
                .child(toolbar)
        };
        let detail = match (load, entry) {
            (PageLoad::Ready, None) => div()
                .flex_1()
                .min_w(px(0.))
                .h_full()
                .flex()
                .flex_col()
                .child(toolbar_band(ToolbarRow::new("").into_any_element()))
                .child(page::page_empty(
                    Sym::Members,
                    c::MEMBERS_PICK,
                    c::MEMBERS_PICK_BODY,
                    Vec::new(),
                    cx,
                ))
                .into_any_element(),
            (PageLoad::Ready, Some(e)) => {
                let toolbar = self.detail_toolbar(e, window, cx);
                let body = self.detail_body(e, window, cx);
                div()
                    .flex_1()
                    .min_w(px(0.))
                    .h_full()
                    .flex()
                    .flex_col()
                    .child(toolbar_band(toolbar))
                    .child(scroll(body))
                    .into_any_element()
            }
            (load, _) => {
                let mut body = div().flex().flex_col().gap(page::m::GUTTER);
                if load == PageLoad::Failed {
                    body = body.child(page::load_failed("members", &self.services, cx));
                }
                body = body.child(self.detail_skeleton(cx));
                div()
                    .flex_1()
                    .min_w(px(0.))
                    .h_full()
                    .flex()
                    .flex_col()
                    .child(toolbar_band(ToolbarRow::new("").into_any_element()))
                    .child(scroll(body))
                    .into_any_element()
            }
        };
        div()
            .size_full()
            .flex()
            .bg(k.content)
            .track_focus(&self.focus)
            .key_context(KEY_CONTEXT)
            .on_action(
                cx.listener(|this, _: &SelectPrevious, _, cx| this.move_selection(false, cx)),
            )
            .on_action(cx.listener(|this, _: &SelectNext, _, cx| this.move_selection(true, cx)))
            .on_action(cx.listener(|this, _: &OpenSelected, _, cx| this.move_selection_by(0, cx)))
            // A click in the list makes it take the arrow keys (§10).
            .child(
                div()
                    .h_full()
                    .on_mouse_down(
                        gpui::MouseButton::Left,
                        cx.listener(|this, _, window, cx| window.focus(&this.focus, cx)),
                    )
                    .child(list),
            )
            .child(detail)
    }
}

// --- v12/behavior ---
gpui::actions!(members_page, [SelectPrevious, SelectNext, OpenSelected]);

/// The key context of the 成员 page.
const KEY_CONTEXT: &str = "MembersPage";

/// ↑/↓ move the selection in the member list, Return opens the selected
/// member (call once at start-up).
pub fn bind_keys(cx: &mut App) {
    cx.bind_keys([
        gpui::KeyBinding::new("up", SelectPrevious, Some(KEY_CONTEXT)),
        gpui::KeyBinding::new("down", SelectNext, Some(KEY_CONTEXT)),
        gpui::KeyBinding::new("enter", OpenSelected, Some(KEY_CONTEXT)),
    ]);
}

impl MembersPage {
    /// The members the list shows, in its order.
    fn visible_ids(&self) -> Vec<String> {
        let filter = self.filter;
        self.entries()
            .into_iter()
            .filter(|e| match filter {
                Filter::All => true,
                Filter::Working => e.bucket == Bucket::Working,
                Filter::Waiting => e.bucket == Bucket::Waiting,
            })
            .map(|e| e.member.id)
            .collect()
    }

    fn move_selection(&mut self, down: bool, cx: &mut Context<Self>) {
        self.move_selection_by(if down { 1 } else { -1 }, cx)
    }

    /// Selects the member `step` rows from the selected one (the first or
    /// last row when none is selected), as a click on the row does.
    fn move_selection_by(&mut self, step: isize, cx: &mut Context<Self>) {
        let ids = self.visible_ids();
        let Some(last) = ids.len().checked_sub(1) else {
            return;
        };
        let current = self
            .selected(cx)
            .and_then(|id| ids.iter().position(|x| *x == id));
        let next = match current {
            Some(ix) => ix.saturating_add_signed(step).min(last),
            None if step < 0 => last,
            None => 0,
        };
        let id = ids[next].clone();
        self.services
            .nav
            .update(cx, |nav, cx| nav.open_members(Some(id), cx));
    }
}

/// The context menu of a member row: the commands of the member's detail
/// (交代给…, 单独问, 看电脑, 产出, 暂停). No shortcuts show here (§7.3).
fn member_menu(
    menu: PopupMenu,
    services: &Services,
    m: &Member,
    run: Option<&BotRunSummary>,
) -> PopupMenu {
    if !m.is_bot() {
        let nav = services.nav.clone();
        return menu.item(
            PopupMenuItem::new(c::MEMBERS_OUTPUTS_BUTTON)
                .on_click(move |_, _, cx| nav.update(cx, |n, cx| n.open_artifacts(cx))),
        );
    }
    let nav = services.nav.clone();
    let mut menu = menu.item(
        PopupMenuItem::new(fill(c::MEMBERS_HAND_OVER, &[("name", &m.name)])).on_click(
            |_, window, cx| window.dispatch_action(Box::new(crate::views::nav::QuickSwitch), cx),
        ),
    );
    if let Some(r) = run.cloned() {
        let (side, live) = (nav.clone(), nav.clone());
        let (r1, r2, member) = (r.clone(), r.clone(), m.id.clone());
        menu = menu
            .item(
                PopupMenuItem::new(c::MEMBERS_SIDE_CHAT).on_click(move |_, _, cx| {
                    let (r, member) = (r1.clone(), member.clone());
                    side.update(cx, |nav, cx| {
                        nav.open_topic(r.group_id, r.topic_id, cx);
                        nav.set_right_pane(RightPane::SideChat(member), cx);
                    })
                }),
            )
            .item(
                PopupMenuItem::new(fill(c::MEMBERS_SEE_COMPUTER, &[("name", &m.name)])).on_click(
                    move |_, _, cx| {
                        let r = r2.clone();
                        live.update(cx, |nav, cx| {
                            nav.open_topic(r.group_id, r.topic_id, cx);
                            nav.set_right_pane(RightPane::Live(r.id), cx);
                        })
                    },
                ),
            );
    }
    let outputs = nav.clone();
    menu = menu.item(
        PopupMenuItem::new(c::MEMBERS_OUTPUTS_BUTTON)
            .on_click(move |_, _, cx| outputs.update(cx, |n, cx| n.open_artifacts(cx))),
    );
    match run.cloned() {
        Some(r) => {
            let services = services.clone();
            menu.separator().item(
                PopupMenuItem::new(c::MEMBERS_PAUSE).on_click(move |_, _, _| {
                    let _ = services
                        .provider
                        .take_over_run(&r.group_id, &r.topic_id, &r.id);
                }),
            )
        }
        None => menu,
    }
}
// --- end v12/behavior ---

// --- v12/behavior ---
#[cfg(test)]
mod behavior_tests {
    use std::sync::Arc;

    use super::*;
    use crate::{data::ChatProvider, fake::v2::V2Samples};

    /// With the list focused, ↓ selects the next member in the list's
    /// order and shows it, ↑ goes back, Return keeps it (NATIVE.md §10).
    #[gpui::test]
    fn arrow_keys_move_the_member_selection(cx: &mut gpui::TestAppContext) {
        cx.update(|cx| {
            gpui_component::init(cx);
            bind_keys(cx);
        });
        let provider: Arc<dyn ChatProvider> = V2Samples::new();
        let (view, cx) = cx.add_window_view(move |_, cx| {
            let services = Services::new(provider, cx);
            MembersPage::new(services, cx)
        });
        cx.run_until_parked();
        let ids = view.update(cx, |this, _| this.visible_ids());
        assert!(ids.len() >= 2, "the samples have members");
        let selected = |view: &gpui::Entity<MembersPage>, cx: &mut gpui::VisualTestContext| {
            view.update(cx, |this, cx| this.selected(cx))
        };
        view.update_in(cx, |this, window, cx| window.focus(&this.focus, cx));
        cx.simulate_keystrokes("down");
        cx.run_until_parked();
        assert_eq!(selected(&view, cx), Some(ids[0].clone()));
        cx.simulate_keystrokes("down");
        cx.run_until_parked();
        assert_eq!(selected(&view, cx), Some(ids[1].clone()));
        cx.simulate_keystrokes("enter");
        cx.run_until_parked();
        assert_eq!(selected(&view, cx), Some(ids[1].clone()));
        cx.simulate_keystrokes("up");
        cx.run_until_parked();
        assert_eq!(selected(&view, cx), Some(ids[0].clone()));
    }
}
// --- end v12/behavior ---
