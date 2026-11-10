//! 团队管理 › 用量 (docs/protocol.md "Team usage"): what AI members' runs cost this week or month
//! against the team's budget, the share a subscription covers, the cost per day stacked by member,
//! where it went (each CLI, by subscription or API key), and the table by AI member, by the person
//! who asked or by machine. 改预算 sits in each AI member's ⋯.

use chrono::{Datelike as _, TimeZone as _};
use gpui::{
    AnyElement, AppContext as _, Context, Entity, Hsla, IntoElement, ParentElement as _,
    SharedString, Styled, Window, div, prelude::FluentBuilder as _, relative,
};
use gpui_component::input::InputState;
use gpui_component::menu::{DropdownMenu as _, PopupMenuItem};

use chat_core::proto::usage::{Spend, TeamUsage};

use super::{AdminView, model};
use crate::{
    copy::{admin as m, fill},
    ui::{
        MemberAvatar, Sym,
        native::{
            Alert, AlertTone, Empty, NativeTextField, NativeType as _, PushSize, PushStyle,
            Segment, Segmented, TruncText, card_surface, native_colors, progress, push_button,
            tokens::{avatar, height, monitor as t, radius, space, text},
        },
    },
    views::settings::widgets::answer,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum UsageRange {
    #[default]
    Week,
    Month,
    LastMonth,
}

impl UsageRange {
    pub const ALL: [UsageRange; 3] = [UsageRange::Week, UsageRange::Month, UsageRange::LastMonth];

    fn label(self) -> &'static str {
        match self {
            UsageRange::Week => m::USAGE_RANGE_WEEK,
            UsageRange::Month => m::USAGE_RANGE_MONTH,
            UsageRange::LastMonth => m::USAGE_RANGE_LAST_MONTH,
        }
    }

    /// [from, to) in local time: this week from Monday, this month, or last month.
    pub fn span(self, now: chrono::DateTime<chrono::Local>) -> (u64, u64) {
        let day = |d: chrono::NaiveDate| {
            chrono::Local
                .from_local_datetime(&d.and_hms_opt(0, 0, 0).unwrap_or_default())
                .earliest()
                .map(|t| t.timestamp_millis().max(0) as u64)
                .unwrap_or(0)
        };
        let today = now.date_naive();
        match self {
            UsageRange::Week => {
                let monday =
                    today - chrono::Days::new(today.weekday().num_days_from_monday() as u64);
                (day(monday), day(monday + chrono::Days::new(7)))
            }
            UsageRange::Month | UsageRange::LastMonth => {
                let first = today.with_day(1).unwrap_or(today);
                let (start, end) = if self == UsageRange::Month {
                    (first, first + chrono::Months::new(1))
                } else {
                    (first - chrono::Months::new(1), first)
                };
                (day(start), day(end))
            }
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum UsageTab {
    #[default]
    Bots,
    People,
    Machines,
}

pub struct Usage {
    pub data: Option<TeamUsage>,
    pub range: UsageRange,
    pub tab: UsageTab,
    pub loading: bool,
    pub error: Option<String>,
    /// 改预算 for this AI member.
    pub budget_for: Option<String>,
    budget: Entity<InputState>,
    budget_error: Option<String>,
    saving: bool,
}

impl Usage {
    pub fn new(window: &mut Window, cx: &mut Context<AdminView>) -> Usage {
        Usage {
            data: None,
            range: UsageRange::default(),
            tab: UsageTab::default(),
            loading: false,
            error: None,
            budget_for: None,
            budget: cx
                .new(|cx| InputState::new(window, cx).placeholder(m::USAGE_BUDGET_PLACEHOLDER)),
            budget_error: None,
            saving: false,
        }
    }
}

fn usd(x: f64) -> String {
    if x >= 100. {
        format!("${}", x.round() as u64)
    } else {
        format!("${x:.2}")
    }
}

/// A KPI card of 用量 and 总览: label with icon, the figure, a foot line and an optional bar.
pub(super) fn kpi_card(
    label: &'static str,
    icon: Sym,
    value: String,
    foot: String,
    bar: Option<f32>,
    cx: &gpui::App,
) -> gpui::Div {
    let k = native_colors(cx);
    card_surface(cx)
        .flex_1()
        .min_w(gpui::Pixels::ZERO)
        .p(space::S16)
        .flex()
        .flex_col()
        .gap(space::S4)
        .child(
            div()
                .flex()
                .items_center()
                .gap(space::S6)
                .n_headline()
                .text_color(k.secondary)
                .child(crate::ui::icons::sym(
                    icon,
                    crate::ui::sf_symbols::SymSize::INLINE,
                    k.secondary,
                ))
                .child(label),
        )
        .child(
            div()
                .n_style(text::KPI)
                .font_weight(gpui::FontWeight::SEMIBOLD)
                .n_figures()
                .child(value),
        )
        .child(
            TruncText::new(foot)
                .n_footnote()
                .n_figures()
                .text_color(k.secondary),
        )
        .children(bar.map(|b| div().mt(space::S8).child(progress(b, cx))))
}

fn cents(c: u64) -> String {
    format!("${}", (c as f64 / 100.).round() as u64)
}

/// The share of the spend that ran on a subscription login: a runtime's spend on a machine whose
/// CLI is signed in with a subscription.
pub fn included(u: &TeamUsage, method: &dyn Fn(&str, Option<&str>) -> Option<String>) -> f64 {
    u.runtimes
        .iter()
        .filter(|r| method(&r.runtime, r.machine_id.as_deref()).as_deref() == Some("subscription"))
        .map(|r| r.spend.cost_usd)
        // A float sum of nothing is -0.0, which prints as "$-0.00".
        .fold(0., |a, b| a + b)
}

impl AdminView {
    pub(super) fn fetch_usage(&mut self, cx: &mut Context<Self>) {
        if self.usage.loading {
            return;
        }
        let (from, to) = self.usage.range.span(crate::selftest::clock::local_now());
        self.usage.loading = true;
        let range = self.usage.range;
        let reply = self.services.provider.team_usage(from, to);
        if self.fleet.monitors.is_empty() {
            self.fetch_fleet(cx);
        }
        cx.spawn(async move |this, cx| {
            let result = answer(reply).await;
            let _ = this.update(cx, |this, cx| {
                this.usage.loading = false;
                if this.usage.range != range {
                    return this.fetch_usage(cx);
                }
                match result {
                    Ok(u) => {
                        this.usage.data = Some(u);
                        this.usage.error = None;
                    }
                    Err(e) => {
                        this.usage.error =
                            Some(crate::views::settings::widgets::error_text(e).to_string())
                    }
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    fn cli_method(&self, runtime: &str, machine: Option<&str>) -> Option<String> {
        let d = self.fleet.monitors.get(machine?)?;
        d.clis.iter().find(|c| c.runtime == runtime)?.method.clone()
    }

    fn save_budget(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(id) = self.usage.budget_for.clone() else {
            return;
        };
        let text = self
            .usage
            .budget
            .read(cx)
            .value()
            .trim()
            .trim_start_matches('$')
            .to_string();
        let cents = if text.is_empty() {
            None
        } else {
            match text.parse::<u64>() {
                Ok(d) if d <= 1_000_000 => Some(d * 100),
                _ => {
                    self.usage.budget_error = Some(m::USAGE_BUDGET_INVALID.into());
                    cx.notify();
                    return;
                }
            }
        };
        self.usage.saving = true;
        let reply = self.services.provider.set_member_budget(&id, cents);
        self.usage
            .budget
            .update(cx, |s, cx| s.set_value("", window, cx));
        cx.spawn(async move |this, cx| {
            let result = answer(reply).await;
            let _ = this.update(cx, |this, cx| {
                this.usage.saving = false;
                match result {
                    Ok(_) => {
                        this.usage.budget_for = None;
                        this.usage.budget_error = None;
                        this.fetch_usage(cx);
                    }
                    Err(e) => {
                        this.usage.budget_error =
                            Some(crate::views::settings::widgets::error_text(e).to_string())
                    }
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    pub(super) fn usage_range_control(&self, cx: &mut Context<Self>) -> AnyElement {
        let view = cx.entity();
        Segmented::new(
            "usage-range",
            UsageRange::ALL.map(|r| Segment::Label(r.label().into())),
        )
        .selected(
            UsageRange::ALL
                .iter()
                .position(|r| *r == self.usage.range)
                .unwrap_or(0),
        )
        .on_change(move |ix, _, cx| {
            view.update(cx, |v, cx| {
                v.usage.range = UsageRange::ALL[ix.min(2)];
                v.usage.data = None;
                v.fetch_usage(cx);
            })
        })
        .into_any_element()
    }

    pub(super) fn usage_page(&self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let Some(u) = self.usage.data.clone() else {
            return match &self.usage.error {
                Some(e) => Alert::new(Sym::Failed, m::USAGE_FAILED)
                    .tone(AlertTone::Destructive)
                    .description(e.clone())
                    .into_any_element(),
                None => super::overview::page_skeleton(cx).into_any_element(),
            };
        };
        let week = self.usage.range == UsageRange::Week;
        // Budgets are weekly; a month compares with four and a bit of them.
        let budget_cents = u.team_budget_cents.map(|c| {
            if week {
                c
            } else {
                c * ((u.to - u.from) / 86_400_000) / 7
            }
        });
        let total = u.total.cost_usd;
        let inc = included(&u, &|r, mm| self.cli_method(r, mm));
        let per_topic = |s: &Spend| {
            if s.topics > 0 {
                s.cost_usd / s.topics as f64
            } else {
                0.
            }
        };
        let prev_per = if u.previous.topics > 0 {
            u.previous.cost_usd / u.previous.topics as f64
        } else {
            0.
        };
        let now_per = per_topic(&u.total);
        let kpis = div()
            .flex()
            .gap(space::S16)
            .child(kpi_card(
                match self.usage.range {
                    UsageRange::Week => m::USAGE_SPENT_WEEK,
                    UsageRange::Month => m::USAGE_SPENT_MONTH,
                    UsageRange::LastMonth => m::USAGE_SPENT_LAST_MONTH,
                },
                Sym::BarChart,
                usd(total),
                match budget_cents {
                    Some(b) if b > 0 => fill(
                        m::USAGE_BUDGET_USED,
                        &[
                            ("budget", &cents(b)),
                            (
                                "pct",
                                &((total * 100. / (b as f64 / 100.)).round() as u64).to_string(),
                            ),
                        ],
                    ),
                    _ => m::USAGE_NO_BUDGET.to_string(),
                },
                budget_cents
                    .filter(|b| *b > 0)
                    .map(|b| (total / (b as f64 / 100.)) as f32),
                cx,
            ))
            .child(kpi_card(
                m::USAGE_INCLUDED,
                Sym::Account,
                usd(inc),
                fill(
                    m::USAGE_INCLUDED_FOOT,
                    &[(
                        "pct",
                        &(if total > 0. {
                            (inc * 100. / total).round()
                        } else {
                            0.
                        } as u64)
                            .to_string(),
                    )],
                ),
                None,
                cx,
            ))
            .child(kpi_card(
                m::USAGE_EXTRA,
                Sym::Key,
                usd((total - inc).max(0.)),
                m::USAGE_EXTRA_FOOT.into(),
                None,
                cx,
            ))
            .child(kpi_card(
                m::USAGE_PER_TOPIC,
                Sym::Topic,
                usd(now_per),
                if (now_per - prev_per).abs() < 0.005 {
                    m::USAGE_PER_TOPIC_SAME.to_string()
                } else if now_per < prev_per {
                    fill(m::USAGE_PER_TOPIC_LESS, &[("d", &usd(prev_per - now_per))])
                } else {
                    fill(m::USAGE_PER_TOPIC_MORE, &[("d", &usd(now_per - prev_per))])
                },
                None,
                cx,
            ));
        div()
            .w_full()
            .flex()
            .flex_col()
            .gap(space::S16)
            .child(kpis)
            .child(
                div()
                    .flex()
                    .gap(space::S16)
                    .child(
                        div()
                            .flex_1()
                            .min_w(gpui::Pixels::ZERO)
                            .flex()
                            .child(self.daily_chart(&u, budget_cents, cx)),
                    )
                    .child(
                        div()
                            .w(relative(t::ATTENTION_SHARE))
                            .flex_none()
                            .flex()
                            .child(self.where_spent(&u, cx)),
                    ),
            )
            .child(self.usage_table(&u, window, cx))
            .into_any_element()
    }

    /// The colors of the stack: the three that cost most, then the rest.
    fn shades(&self, cx: &Context<Self>) -> [Hsla; 4] {
        let k = native_colors(cx);
        [
            k.label,
            k.label.opacity(0.62),
            k.label.opacity(0.36),
            k.label.opacity(0.16),
        ]
    }

    fn daily_chart(
        &self,
        u: &TeamUsage,
        budget_cents: Option<u64>,
        cx: &Context<Self>,
    ) -> AnyElement {
        let k = native_colors(cx);
        let shades = self.shades(cx);
        let top: Vec<String> = u.bots.iter().take(3).map(|b| b.member_id.clone()).collect();
        let max = u
            .daily
            .iter()
            .map(|d| d.cost_usd)
            .fold(0., f64::max)
            .max(0.01)
            * 1.15;
        let days = u.daily.len().max(1);
        let names: Vec<&str> = m::WEEKDAYS.split(',').collect();
        let today = crate::selftest::clock::local_now()
            .timestamp_millis()
            .max(0) as u64;
        let bars = u.daily.iter().map(|d| {
            let mut segs: Vec<(Hsla, f64)> = top
                .iter()
                .enumerate()
                .map(|(i, id)| {
                    (
                        shades[i],
                        d.bots
                            .iter()
                            .filter(|b| &b.member_id == id)
                            .map(|b| b.cost_usd)
                            .sum(),
                    )
                })
                .collect();
            let rest: f64 = d
                .bots
                .iter()
                .filter(|b| !top.contains(&b.member_id))
                .map(|b| b.cost_usd)
                .sum();
            segs.push((shades[3], rest));
            let label = {
                let t = chrono::Local.timestamp_millis_opt(d.start as i64).single();
                if today >= d.start && today < d.start + 86_400_000 {
                    m::ACTIVITY_TODAY.to_string()
                } else if days <= 7 {
                    t.map(|t| {
                        names
                            .get(t.weekday().num_days_from_monday() as usize)
                            .copied()
                            .unwrap_or("")
                            .to_string()
                    })
                    .unwrap_or_default()
                } else {
                    // A month's bars are too narrow for every date: the 1st and every 5th.
                    t.filter(|t| t.day() == 1 || t.day() % 5 == 0)
                        .map(|t| t.day().to_string())
                        .unwrap_or_default()
                }
            };
            div()
                .flex_1()
                .min_w(gpui::Pixels::ZERO)
                .h_full()
                .flex()
                .flex_col()
                .items_center()
                .gap(space::S4)
                .child(
                    div()
                        .flex_1()
                        .w_full()
                        .flex()
                        .flex_col()
                        .justify_end()
                        .items_center()
                        .when(days <= 7 && d.cost_usd > 0., |c| {
                            c.child(
                                div()
                                    .n_footnote()
                                    .n_figures()
                                    .text_color(k.secondary)
                                    .child(usd(d.cost_usd)),
                            )
                        })
                        .child(
                            div()
                                .w(relative(0.56))
                                .h(relative((d.cost_usd / max) as f32))
                                .flex()
                                .flex_col_reverse()
                                .rounded(radius::CHECKBOX)
                                .overflow_hidden()
                                .children(segs.into_iter().filter(|(_, v)| *v > 0.).map(
                                    |(c, v)| {
                                        div()
                                            .w_full()
                                            .h(relative((v / d.cost_usd.max(0.0001)) as f32))
                                            .bg(c)
                                    },
                                )),
                        ),
                )
                .child(
                    div()
                        .whitespace_nowrap()
                        .n_footnote()
                        .n_figures()
                        .text_color(k.secondary)
                        .child(label),
                )
        });
        // The budget per day, as a line across the bars.
        let line = budget_cents.filter(|b| *b > 0).map(|b| {
            let per_day = b as f64 / 100. / 7.;
            let frac = (1. - per_day / max).clamp(0., 1.) as f32;
            div()
                .absolute()
                .left_0()
                .right_0()
                .top(relative(frac * 0.86))
                .border_t_1()
                .border_dashed()
                .border_color(k.amber)
        });
        let legend = top
            .iter()
            .enumerate()
            .map(|(i, id)| (shades[i], self.member_name(id)))
            .chain((u.bots.len() > 3).then(|| (shades[3], m::USAGE_OTHERS.to_string())))
            .map(|(c, n)| {
                div()
                    .flex()
                    .items_center()
                    .gap(space::S6)
                    .n_footnote()
                    .text_color(k.secondary)
                    .child(div().size(t::SWATCH).rounded(radius::CHECKBOX).bg(c))
                    .child(n)
            });
        card_surface(cx)
            .flex_1()
            .p(space::S24)
            .flex()
            .flex_col()
            .gap(space::S16)
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(space::S12)
                    .child(
                        div()
                            .flex_1()
                            .n_title3()
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .child(m::USAGE_DAILY),
                    )
                    .children(legend),
            )
            .child(
                div()
                    .relative()
                    .h(height::LIST_ROW * 3.)
                    .flex()
                    .gap(space::S4)
                    .children(bars)
                    .children(line),
            )
            .into_any_element()
    }

    fn where_spent(&self, u: &TeamUsage, cx: &Context<Self>) -> AnyElement {
        let k = native_colors(cx);
        // One row per CLI and way of paying.
        let mut rows: Vec<(String, String, f64, u64)> = vec![];
        for r in &u.runtimes {
            let kind = self
                .fleet
                .monitors
                .get(r.machine_id.as_deref().unwrap_or_default())
                .and_then(|d| {
                    d.clis
                        .iter()
                        .find(|c| c.runtime == r.runtime)
                        .map(|c| c.kind.clone())
                })
                .unwrap_or_else(|| r.runtime.clone());
            let name = model::cli_name(&kind, &r.runtime);
            let via = match self
                .cli_method(&r.runtime, r.machine_id.as_deref())
                .as_deref()
            {
                Some("subscription") => m::USAGE_VIA_SUBSCRIPTION,
                Some("api_key") => m::USAGE_VIA_KEY,
                _ => "",
            }
            .to_string();
            let tokens = r.spend.input_tokens + r.spend.output_tokens;
            match rows.iter_mut().find(|x| x.0 == name && x.1 == via) {
                Some(x) => {
                    x.2 += r.spend.cost_usd;
                    x.3 += tokens;
                }
                None => rows.push((name, via, r.spend.cost_usd, tokens)),
            }
        }
        rows.sort_by(|a, b| b.2.total_cmp(&a.2));
        let max = rows.iter().map(|r| r.2).fold(0., f64::max).max(0.01);
        card_surface(cx)
            .flex_1()
            .p(space::S24)
            .flex()
            .flex_col()
            .gap(space::S12)
            .child(
                div()
                    .n_title3()
                    .font_weight(gpui::FontWeight::SEMIBOLD)
                    .child(m::USAGE_WHERE),
            )
            .when(rows.is_empty(), |d| {
                d.child(div().n_body().text_color(k.secondary).child(m::USAGE_EMPTY))
            })
            .children(rows.into_iter().map(|(name, via, cost, tokens)| {
                div()
                    .flex()
                    .flex_col()
                    .gap(space::S6)
                    .child(
                        div()
                            .flex()
                            .items_baseline()
                            .gap(space::S8)
                            .child(div().n_headline().child(name))
                            .child(div().n_footnote().text_color(k.secondary).child(via))
                            .child(div().flex_1())
                            .child(div().n_headline().n_figures().child(if cost > 0. {
                                usd(cost)
                            } else {
                                fill(
                                    m::USAGE_TOKENS_ONLY,
                                    &[("tokens", &model::count_short(tokens))],
                                )
                            })),
                    )
                    .child(progress((cost / max) as f32, cx))
            }))
            .into_any_element()
    }

    fn usage_table(
        &self,
        u: &TeamUsage,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let k = native_colors(cx);
        let tab = self.usage.tab;
        let view = cx.entity();
        let tabs = Segmented::new(
            "usage-tab",
            [m::USAGE_BY_BOT, m::USAGE_BY_PERSON, m::USAGE_BY_MACHINE]
                .map(|l| Segment::Label(l.into())),
        )
        .selected(match tab {
            UsageTab::Bots => 0,
            UsageTab::People => 1,
            UsageTab::Machines => 2,
        })
        .on_change(move |ix, _, cx| {
            view.update(cx, |v, cx| {
                v.usage.tab = [UsageTab::Bots, UsageTab::People, UsageTab::Machines][ix.min(2)];
                cx.notify();
            })
        });
        let rows: Vec<(String, String, Spend, Option<u64>)> = match tab {
            UsageTab::Bots => u
                .bots
                .iter()
                .map(|b| {
                    (
                        b.member_id.clone(),
                        self.member_name(&b.member_id),
                        b.spend.clone(),
                        b.budget_cents,
                    )
                })
                .collect(),
            UsageTab::People => u
                .initiators
                .iter()
                .map(|b| {
                    (
                        b.member_id.clone(),
                        self.member_name(&b.member_id),
                        b.spend.clone(),
                        None,
                    )
                })
                .collect(),
            UsageTab::Machines => u
                .machines
                .iter()
                .map(|b| {
                    (
                        b.machine_id.clone(),
                        self.machine_name(&b.machine_id),
                        b.spend.clone(),
                        None,
                    )
                })
                .collect(),
        };
        let n = rows.len();
        let head_cell = |label: &'static str, w: Option<gpui::Pixels>, right: bool| {
            let c = div()
                .px(space::S12)
                .n_section()
                .text_color(k.secondary)
                .child(label);
            let c = match w {
                Some(w) => c.w(w).flex_none(),
                None => c.flex_1().min_w(gpui::Pixels::ZERO),
            };
            if right { c.text_right() } else { c }
        };
        let head = div()
            .h(height::TABLE_HEAD)
            .px(space::S4)
            .flex()
            .items_center()
            .bg(k.group)
            .border_b_1()
            .border_color(k.separator)
            .child(head_cell(
                if tab == UsageTab::Machines {
                    m::USAGE_COL_MACHINE
                } else {
                    m::USAGE_COL_WHO
                },
                None,
                false,
            ))
            .child(head_cell(m::USAGE_COL_COST, Some(t::COL_TOKENS), true))
            .when(tab == UsageTab::Bots, |d| {
                d.child(head_cell(m::USAGE_COL_BUDGET, Some(t::COL_MEM), false))
            })
            .child(head_cell(m::USAGE_COL_TOPICS, Some(t::COL_COUNT), true))
            .child(head_cell(m::USAGE_COL_PER_TOPIC, Some(t::COL_CPU), true))
            .when(tab == UsageTab::Bots, |d| {
                d.child(div().w(height::LG).flex_none())
            });
        let body: Vec<AnyElement> = rows
            .into_iter()
            .enumerate()
            .map(|(i, (id, name, s, budget))| {
                let member = (tab != UsageTab::Machines)
                    .then(|| self.services.provider.member(&id))
                    .flatten();
                let per = if s.topics > 0 {
                    s.cost_usd / s.topics as f64
                } else {
                    0.
                };
                let pct = budget
                    .filter(|b| *b > 0)
                    .map(|b| s.cost_usd / (b as f64 / 100.));
                let view = cx.entity();
                let (bid, bname) = (id.clone(), name.clone());
                let more = (tab == UsageTab::Bots).then(|| {
                    crate::views::settings::widgets::more_button(
                        SharedString::from(format!("usage-more-{id}")),
                        window,
                        cx,
                    )
                    .dropdown_menu(move |menu, _, _| {
                        let (v, id) = (view.clone(), bid.clone());
                        menu.item(PopupMenuItem::new(m::USAGE_EDIT_BUDGET).on_click(
                            move |_, _, cx| {
                                let id = id.clone();
                                v.update(cx, |v, cx| {
                                    v.usage.budget_for = Some(id);
                                    v.usage.budget_error = None;
                                    cx.notify();
                                })
                            },
                        ))
                    })
                });
                let _ = bname;
                div()
                    .min_h(height::TABLE_ROW_COMPACT)
                    .px(space::S4)
                    .flex()
                    .items_center()
                    .n_body()
                    .n_figures()
                    .when(i + 1 < n, |d| d.border_b_1().border_color(k.separator))
                    .child(
                        div()
                            .flex_1()
                            .min_w(gpui::Pixels::ZERO)
                            .px(space::S12)
                            .flex()
                            .items_center()
                            .gap(space::S12)
                            .children(
                                member
                                    .as_ref()
                                    .map(|mm| MemberAvatar::new(Some(mm), avatar::SM)),
                            )
                            .child(TruncText::new(name).n_headline()),
                    )
                    .child(
                        div()
                            .w(t::COL_TOKENS)
                            .flex_none()
                            .px(space::S12)
                            .text_right()
                            .n_headline()
                            .child(usd(s.cost_usd)),
                    )
                    .when(tab == UsageTab::Bots, |d| {
                        d.child(
                            div()
                                .w(t::COL_MEM)
                                .flex_none()
                                .px(space::S12)
                                .flex()
                                .items_center()
                                .gap(space::S8)
                                .child(div().flex_1().child(progress(pct.unwrap_or(0.) as f32, cx)))
                                .child(
                                    div()
                                        .n_footnote()
                                        .text_color(if pct.is_some_and(|p| p >= 0.8) {
                                            k.amber
                                        } else {
                                            k.secondary
                                        })
                                        .child(match (pct, budget) {
                                            (Some(p), Some(b)) => fill(
                                                m::USAGE_BUDGET_OF,
                                                &[
                                                    (
                                                        "pct",
                                                        &((p * 100.).round() as u64).to_string(),
                                                    ),
                                                    ("budget", &cents(b)),
                                                ],
                                            ),
                                            _ => m::NO_LIMIT.to_string(),
                                        }),
                                ),
                        )
                    })
                    .child(
                        div()
                            .w(t::COL_COUNT)
                            .flex_none()
                            .px(space::S12)
                            .text_right()
                            .child(s.topics.to_string()),
                    )
                    .child(
                        div()
                            .w(t::COL_CPU)
                            .flex_none()
                            .px(space::S12)
                            .text_right()
                            .text_color(k.secondary)
                            .child(usd(per)),
                    )
                    .children(more.map(|b| {
                        div()
                            .w(height::LG)
                            .flex_none()
                            .flex()
                            .justify_center()
                            .child(b)
                    }))
                    .into_any_element()
            })
            .collect();
        let edit = self.usage.budget_for.clone().map(|id| {
            let (save, cancel) = (cx.entity(), cx.entity());
            card_surface(cx)
                .p(space::S16)
                .flex()
                .flex_col()
                .gap(space::S8)
                .child(div().n_headline().child(fill(
                    m::USAGE_BUDGET_TITLE,
                    &[("name", &self.member_name(&id))],
                )))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(space::S8)
                        .child(div().flex_1().child(
                            NativeTextField::new("usage-budget", &self.usage.budget).full_width(),
                        ))
                        .child(
                            push_button(
                                "usage-budget-cancel",
                                m::CANCEL,
                                PushStyle::Outline,
                                PushSize::Small,
                                cx,
                            )
                            .on_click(move |_, _, cx| {
                                cancel.update(cx, |v, cx| {
                                    v.usage.budget_for = None;
                                    cx.notify();
                                })
                            }),
                        )
                        .child(
                            crate::ui::native::loading_push(
                                push_button(
                                    "usage-budget-save",
                                    m::USAGE_BUDGET_SAVE,
                                    PushStyle::Default,
                                    PushSize::Small,
                                    cx,
                                ),
                                self.usage.saving,
                                PushSize::Small,
                            )
                            .on_click(move |_, window, cx| {
                                save.update(cx, |v, cx| v.save_budget(window, cx))
                            }),
                        ),
                )
                .children(
                    self.usage
                        .budget_error
                        .clone()
                        .map(|e| Alert::new(Sym::Failed, e).tone(AlertTone::Destructive)),
                )
        });
        div()
            .flex()
            .flex_col()
            .gap(space::S12)
            .child(
                div()
                    .flex()
                    .items_center()
                    .child(tabs)
                    .child(div().flex_1())
                    .when(tab == UsageTab::Bots, |d| {
                        d.child(
                            div()
                                .n_footnote()
                                .text_color(k.secondary)
                                .child(m::USAGE_BUDGET_HINT),
                        )
                    }),
            )
            .children(edit)
            .child(if body.is_empty() {
                Empty::new(Sym::BarChart, m::USAGE_EMPTY)
                    .dashed()
                    .into_any_element()
            } else {
                div()
                    .rounded(radius::CONTAINER)
                    .border_1()
                    .border_color(k.separator)
                    .bg(k.admin_panel())
                    .overflow_hidden()
                    .flex()
                    .flex_col()
                    .child(head)
                    .children(body)
                    .into_any_element()
            })
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spans_start_on_monday_and_the_first_of_the_month() {
        let now = chrono::Local
            .with_ymd_and_hms(2026, 11, 12, 15, 0, 0)
            .unwrap();
        let (from, to) = UsageRange::Week.span(now);
        let f = chrono::Local.timestamp_millis_opt(from as i64).unwrap();
        assert_eq!(
            (f.month(), f.day(), f.weekday()),
            (11, 9, chrono::Weekday::Mon)
        );
        assert_eq!(to - from, 7 * 86_400_000);
        let (from, _) = UsageRange::LastMonth.span(now);
        let f = chrono::Local.timestamp_millis_opt(from as i64).unwrap();
        assert_eq!((f.month(), f.day()), (10, 1));
    }

    #[test]
    fn subscription_share_follows_each_machines_login() {
        use chat_core::proto::usage::RuntimeSpend;
        let spend = |c: f64| Spend {
            cost_usd: c,
            ..Default::default()
        };
        let u = TeamUsage {
            runtimes: vec![
                RuntimeSpend {
                    runtime: "claude".into(),
                    machine_id: Some("m1".into()),
                    spend: spend(10.),
                },
                RuntimeSpend {
                    runtime: "claude".into(),
                    machine_id: Some("m2".into()),
                    spend: spend(4.),
                },
            ],
            ..Default::default()
        };
        let method =
            |_: &str, m: Option<&str>| (m == Some("m1")).then(|| "subscription".to_string());
        assert_eq!(included(&u, &method), 10.);
    }
}
