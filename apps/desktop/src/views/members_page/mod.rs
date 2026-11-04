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
            FormRow, GroupedForm, ListRow, NativeType as _, PushSize, PushStyle, Segment,
            Segmented, ToolbarGroup, ToolbarItemKind, ToolbarRow, borderless_button,
            determinate_bar, disabled_push, emphasized, metrics, native_colors, push_button,
            toolbar_item,
        },
        sf_symbols::{Sf, Sym, SymSize},
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
        let toolbar = ToolbarRow::new(c::MEMBERS_PAGE_TITLE)
            .subtitle(fill(c::MEMBERS_COUNT, &[("n", &entries.len().to_string())]))
            .leading_inset(rows::toolbar_leading(&self.services, cx))
            .group(filter)
            .group(rows::toolbar_capsule(add, cx));

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
            .pb(px(12.))
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
                    .mt(px(6.))
                    .px(rows::LEADING)
                    .flex()
                    .items_end()
                    .pb(px(4.))
                    .n_section()
                    .text_color(k.secondary)
                    .child(label),
            );
            let n = group.len();
            for (i, e) in group.into_iter().enumerate() {
                list = list.child(self.row(e, selected, me.as_deref(), focused, active, i + 1 == n, cx));
            }
        }
        if !any {
            list = list.child(
                div()
                    .pt(px(40.))
                    .flex()
                    .justify_center()
                    .n_body()
                    .text_color(k.secondary)
                    .child(c::MEMBERS_FILTER_EMPTY),
            );
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
        let ink = RowInk::new(ListRow::selection_ink(is_selected, focused, active, cx), cx);
        let on_accent = ink.fill == Some(k.accent);
        let mut parts: Vec<String> = m.role.clone().into_iter().collect();
        if me == Some(m.id.as_str()) {
            parts.push(c::MEMBERS_YOU_WORD.to_string());
        }
        let state: Option<(Sym, gpui::Hsla, String)>;
        match e.bucket {
            Bucket::Waiting => {
                state = Some((
                    Sym::NeedsYou,
                    if on_accent { ink.title } else { k.amber },
                    c::MEMBERS_WAITS_ON_YOU.to_string(),
                ));
                parts.extend(e.extras.line.clone().or(e.ask.clone()));
            }
            Bucket::Working => {
                state = None;
                let run = e.run.as_ref();
                let topic = run
                    .and_then(|r| p.topic(&r.group_id, &r.topic_id))
                    .map(|t| t.title);
                parts.extend(topic);
                if let Some((n, mm)) = run.and_then(|r| kit::plan_progress(&r.plan)) {
                    parts.push(format!("{n}/{mm}"));
                }
            }
            Bucket::Idle => {
                state = None;
                parts.push(
                    e.extras
                        .line
                        .clone()
                        .unwrap_or_else(|| c::MEMBERS_IDLE.to_string()),
                );
            }
            Bucket::People => {
                state = None;
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
        // The state as a symbol at the trailing edge (§1 rule 4: no pills).
        let mark = match e.bucket {
            Bucket::Working => Some(rows::sym(
                Sym::Live,
                SymSize::INLINE_SMALL,
                if on_accent { ink.title } else { k.blue },
            )),
            _ => None,
        };
        let second = if let Some(st) = state {
            let rest = parts.join(" · ");
            rows::state_line(Some(st), Some(rest).filter(|r| !r.is_empty()), ink.secondary)
        } else {
            rows::line(parts.join(" · ")).into_any_element()
        };
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

    /// The Contacts-like card head: avatar 64, the name (Large title), 岗位
    /// and who they report to, then 交代… (default), 单独问 and 暂停.
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
        let buttons = m.is_bot().then(|| {
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
            div()
                .mt(px(14.))
                .flex()
                .items_center()
                .gap(px(8.))
                .child(
                    push_button(
                        "member-hand-over",
                        c::MEMBERS_HAND_OVER_MORE,
                        PushStyle::Default,
                        PushSize::Regular,
                        cx,
                    )
                    .on_click(|_, window, cx| {
                        window.dispatch_action(Box::new(crate::views::nav::QuickSwitch), cx)
                    }),
                )
                .child(disabled_push(
                    push_button(
                        "member-side-chat",
                        c::MEMBERS_SIDE_CHAT,
                        PushStyle::Standard,
                        PushSize::Regular,
                        cx,
                    )
                    .on_click(side),
                    e.run.is_none(),
                    cx,
                ))
                .child(disabled_push(
                    push_button(
                        "member-pause",
                        c::MEMBERS_PAUSE,
                        PushStyle::Standard,
                        PushSize::Regular,
                        cx,
                    )
                    .on_click(move |_, _, _| {
                        if let Some(r) = &run {
                            let _ = services
                                .provider
                                .take_over_run(&r.group_id, &r.topic_id, &r.id);
                        }
                    }),
                    e.run.is_none(),
                    cx,
                ))
        });
        div()
            .flex()
            .items_center()
            .gap(px(16.))
            .child(rows::avatar(Some(m), px(64.), cx))
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.))
                    .flex()
                    .flex_col()
                    .child(rows::line(m.name.clone()).n_large_title().text_color(k.label))
                    .when(!parts.is_empty(), |el| {
                        el.child(
                            rows::line(parts.join(" · "))
                                .n_body()
                                .text_color(k.secondary),
                        )
                    })
                    .children(buttons),
            )
            .into_any_element()
    }

    /// 现在在做: the active run, its step, and what waits in the queue.
    fn now_section(&self, e: &Entry, cx: &App) -> Option<AnyElement> {
        if e.run.is_none() && e.extras.queued.is_empty() {
            return None;
        }
        let k = native_colors(cx);
        let p = &self.services.provider;
        let mut form = GroupedForm::new().header(c::MEMBERS_NOW_DOING);
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
            let progress = kit::plan_progress(&r.plan)
                .map(|(n, m)| format!("{n}/{m}"))
                .unwrap_or_default();
            form = form.row(
                FormRow::new(topic.map(|t| t.title).unwrap_or_default())
                    .detail(fill(
                        c::MEMBERS_TOPIC_META,
                        &[("group", &group), ("name", &by)],
                    ))
                    .value(progress, cx),
            );
            if let Some(i) = current {
                form = form.row(
                    FormRow::new(r.plan[i].text.clone())
                        .leading(rows::sym(Sym::Live, SymSize::INLINE, k.blue))
                        .value(kit::elapsed(p.clock_now(), since), cx),
                );
            }
        }
        for q in &e.extras.queued {
            let mut row = FormRow::new(q.title.clone())
                .leading(rows::sym(Sym::Waiting, SymSize::INLINE, k.secondary));
            row = match &q.note {
                Some(n) => row.detail(n.clone()).value(c::MEMBERS_QUEUED, cx),
                None => row.value(c::MEMBERS_QUEUED, cx),
            };
            form = form.row(row);
        }
        Some(form.into_any_element())
    }

    /// 这周: label/value rows; 成本 against 上限 with a 4 pt bar.
    fn week_section(&self, e: &Entry, cx: &App) -> AnyElement {
        let k = native_colors(cx);
        let done = match e.extras.avg_topic_ms {
            Some(ms) => fill(
                c::MEMBERS_DONE_VALUE,
                &[
                    ("n", &e.stats.done_this_week.to_string()),
                    ("m", &(ms / 60_000).max(1).to_string()),
                ],
            ),
            None => e.stats.done_this_week.to_string(),
        };
        let outputs = e
            .stats
            .artifacts_this_week
            .max(e.extras.prs_this_week + e.extras.pages_this_week);
        let outputs = fill(
            c::MEMBERS_OUTPUTS_VALUE,
            &[
                ("n", &outputs.to_string()),
                (
                    "kinds",
                    &fill(
                        c::MEMBERS_OUTPUT_KINDS,
                        &[
                            ("prs", &e.extras.prs_this_week.to_string()),
                            ("pages", &e.extras.pages_this_week.to_string()),
                        ],
                    ),
                ),
            ],
        );
        let currency = e.stats.cost_currency;
        let cost = e.stats.cost_cents_this_week.unwrap_or(0);
        let budget = e.extras.budget_cents.filter(|b| *b > 0);
        let cost_value = match budget {
            Some(b) => fill(
                c::MEMBERS_COST_OF,
                &[("cost", &money(cost, currency)), ("limit", &money(b, currency))],
            ),
            None => money(cost, currency),
        };
        // No row when no run reported a cost: ¥0 or $0 would be a wrong number.
        let cost_row = e.stats.cost_cents_this_week.map(|_| FormRow::new(c::MEMBERS_COST).control(
            div()
                .flex()
                .items_center()
                .gap(px(10.))
                .children(budget.map(|b| {
                    let pct = (cost as f32 / b as f32).clamp(0., 1.);
                    // Over 80 % of the limit the bar shows the problem color.
                    let color = (pct >= 0.8).then_some(k.amber);
                    div().w(COST_BAR).child(determinate_bar(pct, color, cx))
                }))
                .child(
                    div()
                        .n_body()
                        .n_figures()
                        .text_color(k.secondary)
                        .child(cost_value),
                ),
        ));
        GroupedForm::new()
            .header(c::MEMBERS_THIS_WEEK)
            .row(FormRow::new(c::MEMBERS_DONE_TOPICS).value(done, cx))
            .row(FormRow::new(c::MEMBERS_OUTPUTS).value(outputs, cx))
            .rows(cost_row)
            .into_any_element()
    }

    /// 最近交付: rows without chevrons; the state is text, never a pill.
    fn deliveries(&self, e: &Entry, cx: &App) -> Option<AnyElement> {
        if e.extras.deliveries.is_empty() {
            return None;
        }
        let k = native_colors(cx);
        let p = &self.services.provider;
        let now = p.clock_now();
        let rows_el: Vec<AnyElement> = e
            .extras
            .deliveries
            .iter()
            .enumerate()
            .map(|(i, d)| {
                let group = d
                    .topic
                    .as_ref()
                    .and_then(|(g, _)| p.groups().into_iter().find(|x| &x.id == g))
                    .map(|g| g.name);
                let (glyph, title, meta) = match &d.kind {
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
                        {
                            let mut line = kit::day_and_time(now, d.at, false);
                            if let Some(g) = &group {
                                line = format!("{line} · {g}");
                            }
                            line
                        },
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
                            format!("{kind} · {}", kit::day_and_time(now, d.at, false)),
                        )
                    }
                };
                let state = d.state.map(|s| match s {
                    DeliveryState::WaitsOnMe => (c::MEMBERS_WAITS_REVIEW_WORD, k.amber),
                    DeliveryState::Open => (c::MEMBERS_UNMERGED, k.secondary),
                    DeliveryState::Merged => (c::MEMBERS_MERGED, k.secondary),
                });
                let nav = self.services.nav.clone();
                let kind = d.kind.clone();
                let topic = d.topic.clone();
                div()
                    .id(SharedString::from(format!("member-delivery-{i}")))
                    .child(
                        FormRow::new(title)
                            .detail(meta)
                            .leading(rows::sym(glyph, SymSize::INLINE, k.secondary))
                            .when_some(state, |r, (text, color)| {
                                r.control(div().n_body().text_color(color).child(text))
                            }),
                    )
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
                    })
                    .into_any_element()
            })
            .collect();
        Some(
            GroupedForm::new()
                .header(c::MEMBERS_RECENT_DELIVERED)
                .rows(rows_el)
                .into_any_element(),
        )
    }

    /// 最近的问题: a red symbol, the problem and `重试` as a small button.
    fn problems(&self, e: &Entry, cx: &mut Context<Self>) -> Option<AnyElement> {
        if e.stats.problems.is_empty() {
            return None;
        }
        let k = native_colors(cx);
        let now = self.services.provider.clock_now();
        let name = e.member.name.clone();
        let mut form = GroupedForm::new().header(c::MEMBERS_RECENT_PROBLEMS);
        for (i, pr) in e.stats.problems.iter().enumerate() {
            let retried = pr.run_id.as_ref().is_some_and(|r| self.retried.contains(r));
            if retried {
                form = form.row(
                    FormRow::new(fill(c::MEMBERS_RETRIED, &[("name", &name)]))
                        .detail(c::MEMBERS_RETRIED_NOTE)
                        .leading(rows::sym(Sym::Done, SymSize::INLINE, k.green))
                        .value(c::MEMBERS_JUST_NOW, cx),
                );
                continue;
            }
            let member = e.member.id.clone();
            let mut row = FormRow::new(pr.text.clone())
                .detail(format!(
                    "{} · {}",
                    c::MEMBERS_STUCK,
                    kit::day_and_time(now, pr.at, true)
                ))
                .leading(rows::sym(Sym::Problem, SymSize::INLINE, k.red));
            if let Some(run) = pr.run_id.clone() {
                row = row.control(
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
                    })),
                );
            }
            form = form.row(row);
        }
        Some(form.into_any_element())
    }

    /// 在哪干活: the team computer (or cloud runner) the member works on.
    fn workplace(&self, e: &Entry, window: &Window, cx: &App) -> Option<AnyElement> {
        let mc = e.stats.machine.as_ref()?;
        let k = native_colors(cx);
        let (state, dot) = if mc.busy {
            (c::MEMBERS_IN_USE, Some(k.blue))
        } else if mc.online {
            (c::MEMBERS_MACHINE_IDLE, Some(k.green))
        } else {
            (c::MEMBERS_MACHINE_OFFLINE, None)
        };
        let mut detail = state.to_string();
        if let Some(pct) = mc.disk_used_pct {
            detail = format!("{detail} · {} {pct}%", c::MEMBERS_DISK);
        }
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
        });
        let mut row = FormRow::new(mc.name.clone())
            .detail(detail)
            .leading(
                div()
                    .relative()
                    .child(rows::sym(machine_sym(&mc.name), SymSize::SIDEBAR, k.secondary))
                    .children(dot.map(|d| {
                        div()
                            .absolute()
                            .right(px(-2.))
                            .bottom(px(-1.))
                            .size(px(7.))
                            .rounded_full()
                            .bg(d)
                    })),
            );
        if let Some(b) = open {
            row = row.control(b);
        }
        Some(
            GroupedForm::new()
                .header(c::MEMBERS_WHERE)
                .row(row)
                .into_any_element(),
        )
    }

    fn approvals(&self, e: &Entry, cx: &App) -> Option<AnyElement> {
        if e.stats.needs_approval.is_empty() {
            return None;
        }
        let k = native_colors(cx);
        let services = self.services.clone();
        Some(
            GroupedForm::new()
                .header(c::MEMBERS_NEEDS_APPROVAL)
                .rows(e.stats.needs_approval.iter().map(|a| {
                    FormRow::new(a.clone()).leading(rows::sym(Sym::Lock, SymSize::INLINE, k.secondary))
                }))
                .footer(fill(c::MEMBERS_APPROVAL_NOTE, &[("name", &e.member.name)]))
                .below(
                    push_button(
                        "member-approvals-edit",
                        c::MEMBERS_EDIT_MORE,
                        PushStyle::Standard,
                        PushSize::Regular,
                        cx,
                    )
                    .on_click(move |_, _, cx| {
                        crate::app::open_settings_window(services.clone(), cx)
                    }),
                )
                .into_any_element(),
        )
    }

    fn tools(&self, e: &Entry, cx: &App) -> Option<AnyElement> {
        if e.extras.tools.is_empty() {
            return None;
        }
        Some(
            GroupedForm::new()
                .header(c::MEMBERS_TOOLS)
                .rows(e.extras.tools.iter().map(|t| {
                    let mut row = FormRow::new(t.name.clone());
                    if let Some(d) = &t.detail {
                        row = row.detail(d.clone());
                    }
                    if let Some(a) = &t.access {
                        row = row.value(a.clone(), cx);
                    }
                    row
                }))
                .into_any_element(),
        )
    }

    fn decisions(&self, e: &Entry, cx: &App) -> Option<AnyElement> {
        if e.extras.decisions.is_empty() {
            return None;
        }
        let p = &self.services.provider;
        let now = p.clock_now();
        let rows_el: Vec<AnyElement> = e
            .extras
            .decisions
            .iter()
            .enumerate()
            .map(|(i, d)| {
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
                    .child(
                        FormRow::new(d.text.clone())
                            .detail(format!(
                                "{} · {place}",
                                who.as_ref().map(|w| w.name.clone()).unwrap_or_default()
                            ))
                            .leading(rows::avatar(who.as_ref(), px(20.), cx))
                            .value(when, cx),
                    )
                    .on_click(move |_, _, cx| {
                        let g = g.clone();
                        match t.clone() {
                            Some(t) => nav.update(cx, |n, cx| n.open_topic(g, t, cx)),
                            None => nav.update(cx, |n, cx| n.open_group(g, cx)),
                        }
                    })
                    .into_any_element()
            })
            .collect();
        Some(
            GroupedForm::new()
                .header(c::MEMBERS_DECISIONS)
                .rows(rows_el)
                .into_any_element(),
        )
    }
}

impl Render for MembersPage {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let k = native_colors(cx);
        let entries = self.entries();
        let selected = self.selected(cx);
        let list = self.list(&entries, selected.as_deref(), window, cx);
        let entry = selected
            .as_deref()
            .and_then(|id| entries.iter().find(|e| e.member.id == id));
        crate::selftest::drawn::mark(
            "members-page",
            format!(
                "entries={} detail={}",
                entries.len(),
                entry.map_or("", |e| e.member.name.as_str())
            ),
        );
        let detail = match entry {
            None => div()
                .flex_1()
                .h_full()
                .flex()
                .items_center()
                .justify_center()
                .n_body()
                .text_color(k.secondary)
                .child(c::MEMBERS_PICK)
                .into_any_element(),
            Some(e) => {
                let toolbar = self.detail_toolbar(e, window, cx);
                let mut sections: Vec<AnyElement> = vec![self.identity(e, cx)];
                sections.extend(self.now_section(e, cx));
                if e.member.is_bot() {
                    sections.push(self.week_section(e, cx));
                }
                sections.extend(self.deliveries(e, cx));
                sections.extend(self.problems(e, cx));
                sections.extend(self.workplace(e, window, cx));
                sections.extend(self.approvals(e, cx));
                sections.extend(self.tools(e, cx));
                sections.extend(self.decisions(e, cx));
                div()
                    .flex_1()
                    .min_w(px(0.))
                    .h_full()
                    .flex()
                    .flex_col()
                    .child(toolbar)
                    .child(
                        div()
                            .id("member-detail")
                            .flex_1()
                            .min_h(px(0.))
                            .overflow_y_scroll()
                            // The shared leading inset (§2): the detail
                            // starts on the same edge as the list's rows.
                            .px(rows::LEADING)
                            .pt(px(4.))
                            .pb(px(32.))
                            .child(
                                div()
                                    .w_full()
                                    .max_w(DETAIL_MEASURE)
                                    .flex()
                                    .flex_col()
                                    .gap(metrics::GROUP_GAP)
                                    .children(sections),
                            ),
                    )
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
