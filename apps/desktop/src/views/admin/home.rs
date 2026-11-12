//! 团队管理 › 总览: the team at a glance. 成员, 话题, 花费 and 产出 of this week or month, who asks
//! the AI members for work (谁在用), what needs the owner across every machine, budget and invite
//! (要注意), and a card per machine. It reads what the other pages load: 用量, the roster and each
//! machine's monitor.

use gpui::{
    AnyElement, Context, IntoElement, ParentElement as _, Styled, Window, div,
    prelude::FluentBuilder as _, relative,
};

use super::{
    AdminPage, AdminView, model,
    model::{Attention, AttentionKind, Tone},
    usage::{UsageRange, kpi_card},
};
use crate::{
    copy::{admin as m, fill},
    data::MemberKind,
    ui::{
        MemberAvatar, Sym,
        native::{
            Alert, AlertTone, EmptyLine, NativeType as _, PushSize, PushStyle, TruncText,
            card_surface, native_colors, progress, push_button,
            tokens::{avatar, height, monitor as t, space},
        },
    },
};

/// An AI member at or over this share of its weekly budget shows in 要注意.
const BUDGET_WARN: f64 = 0.8;
/// An invite that ends within this many ms shows in 要注意.
const INVITE_SOON_MS: u64 = 2 * 86_400_000;
/// 谁在用 lists this many people at most.
const WHO_ROWS: usize = 6;
/// 谁在用 shows this many of the AI members each person asks most.
const WHO_AVATARS: usize = 3;

fn usd(x: f64) -> String {
    if x >= 100. {
        format!("${}", x.round() as u64)
    } else {
        format!("${x:.2}")
    }
}

fn now_ms() -> u64 {
    crate::selftest::clock::local_now()
        .timestamp_millis()
        .max(0) as u64
}

/// 比上一段多 / 少 / 一样, for a count.
fn compare(now: u64, before: u64) -> String {
    if now == before {
        m::HOME_SAME.to_string()
    } else if now > before {
        fill(m::HOME_MORE, &[("n", &(now - before).to_string())])
    } else {
        fill(m::HOME_LESS, &[("n", &(before - now).to_string())])
    }
}

impl AdminView {
    /// Loads what 总览 shows: 用量 of the range, the roster and every machine's monitor.
    pub(super) fn fetch_home(&mut self, cx: &mut Context<Self>) {
        self.fetch_usage(cx);
        self.fetch_team(cx);
        self.fetch_fleet(cx);
    }

    pub(super) fn home_subtitle(&self) -> String {
        match self.usage.range {
            UsageRange::Week => m::HOME_SUB_WEEK.to_string(),
            UsageRange::Month => m::HOME_SUB_MONTH.to_string(),
            UsageRange::LastMonth => m::HOME_SUB_LAST_MONTH.to_string(),
        }
    }

    /// 要注意 of the whole team: each machine's items (named), AI members near their budget and
    /// invites about to end.
    fn home_attention(&self, now: u64, ai_configured: Option<bool>) -> Vec<Attention> {
        let mut out: Vec<Attention> = model::ai_service_attention(ai_configured)
            .into_iter()
            .collect();
        let names = |id: &str| self.member_name(id);
        let time = |t: u64| super::clock_day(t);
        for x in self.machines.iter().flatten() {
            if let Some(d) = self.fleet.monitors.get(&x.id) {
                for a in model::attention(d, now, &names, &time) {
                    out.push(Attention {
                        meta: if a.meta.is_empty() {
                            x.name.clone()
                        } else {
                            format!("{} · {}", x.name, a.meta)
                        },
                        ..a
                    });
                }
            } else if !x.connected {
                out.push(Attention {
                    kind: AttentionKind::Offline,
                    title: m::ATT_OFFLINE.into(),
                    meta: x.name.clone(),
                    tone: Tone::Bad,
                });
            }
        }
        if let Some(w) = self.team.week.as_ref() {
            for b in &w.bots {
                let Some(cents) = b.budget_cents.filter(|c| *c > 0) else {
                    continue;
                };
                // 到了预算先停下来问: runs wait for an owner.
                if let Some(n) = b.held_runs.filter(|n| *n > 0) {
                    out.push(Attention {
                        kind: AttentionKind::Budget,
                        title: fill(
                            m::ATT_BUDGET_PAUSED,
                            &[
                                ("name", &self.member_name(&b.member_id)),
                                ("n", &n.to_string()),
                            ],
                        ),
                        meta: fill(
                            m::ATT_BUDGET_PAUSED_META,
                            &[
                                ("spent", &usd(b.spend.cost_usd)),
                                ("budget", &usd(cents as f64 / 100.)),
                            ],
                        ),
                        tone: Tone::Bad,
                    });
                    continue;
                }
                let share = b.spend.cost_usd / (cents as f64 / 100.);
                if share >= BUDGET_WARN {
                    out.push(Attention {
                        kind: AttentionKind::Budget,
                        title: fill(
                            m::ATT_BUDGET,
                            &[
                                ("name", &self.member_name(&b.member_id)),
                                ("pct", &((share * 100.).round() as u64).to_string()),
                            ],
                        ),
                        meta: fill(m::ATT_BUDGET_META, &[("budget", &usd(cents as f64 / 100.))]),
                        tone: if share >= 1. { Tone::Bad } else { Tone::Warn },
                    });
                }
            }
        }
        if let Some(r) = self.team.roster.as_ref() {
            for i in &r.invites {
                if i.expires_at > now && i.expires_at - now <= INVITE_SOON_MS {
                    out.push(Attention {
                        kind: AttentionKind::Invite,
                        title: fill(m::ATT_INVITE, &[("name", &i.name)]),
                        meta: fill(
                            m::ATT_INVITE_META,
                            &[("when", &model::duration(i.expires_at - now))],
                        ),
                        tone: Tone::Info,
                    });
                }
            }
        }
        out
    }

    pub(super) fn home_page(&self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let now = now_ms();
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
        // What the AI 服务 page saved last wins over what 总览 read when it opened.
        let ai_configured = self
            .ai
            .read(cx)
            .saved()
            .map(|s| s.provider.is_some())
            .or(self.team.ai_configured);
        let members = self.services.provider.members();
        let active: Vec<_> = members.iter().filter(|x| !x.deactivated).collect();
        let ai = active.iter().filter(|x| x.kind == MemberKind::Bot).count();
        let people = active.len() - ai;
        let invites = self
            .team
            .roster
            .as_ref()
            .map(|r| r.invites.len())
            .unwrap_or(0);
        let artifacts: Vec<String> = self
            .services
            .provider
            .all_artifacts()
            .iter()
            .filter(|(_, a)| (u.from..u.to).contains(&(a.updated_at.max(0) as u64)))
            .map(|(_, a)| a.created_by.to_string())
            .collect();
        let makers = {
            let mut v = artifacts.clone();
            v.sort();
            v.dedup();
            v.len()
        };
        let budget_cents = u.team_budget_cents.map(|c| {
            if week {
                c
            } else {
                c * ((u.to - u.from) / 86_400_000) / 7
            }
        });
        let total = u.total.cost_usd;
        let kpis = div()
            .flex()
            .gap(space::S16)
            .child(kpi_card(
                m::HOME_KPI_MEMBERS,
                Sym::Members,
                active.len().to_string(),
                if invites > 0 {
                    fill(
                        m::HOME_MEMBERS_FOOT_INVITES,
                        &[
                            ("people", &people.to_string()),
                            ("ai", &ai.to_string()),
                            ("n", &invites.to_string()),
                        ],
                    )
                } else {
                    fill(
                        m::HOME_MEMBERS_FOOT,
                        &[("people", &people.to_string()), ("ai", &ai.to_string())],
                    )
                },
                None,
                cx,
            ))
            .child(kpi_card(
                if week {
                    m::HOME_KPI_TOPICS_WEEK
                } else {
                    m::HOME_KPI_TOPICS
                },
                Sym::Topic,
                u.total.topics.to_string(),
                compare(u.total.topics as u64, u.previous.topics as u64),
                None,
                cx,
            ))
            .child(kpi_card(
                if week {
                    m::USAGE_SPENT_WEEK
                } else {
                    m::USAGE_SPENT
                },
                Sym::BarChart,
                usd(total),
                match budget_cents {
                    Some(b) if b > 0 => fill(
                        m::USAGE_BUDGET_USED,
                        &[
                            ("budget", &usd(b as f64 / 100.)),
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
                if week {
                    m::HOME_KPI_ARTIFACTS_WEEK
                } else {
                    m::HOME_KPI_ARTIFACTS
                },
                Sym::Document,
                artifacts.len().to_string(),
                if artifacts.is_empty() {
                    m::HOME_ARTIFACTS_NONE.to_string()
                } else {
                    fill(m::HOME_ARTIFACTS_FOOT, &[("n", &makers.to_string())])
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
                            .child(self.who_uses(&u, cx)),
                    )
                    .child(
                        div()
                            .w(relative(t::ATTENTION_SHARE))
                            .flex_none()
                            .flex()
                            .child(super::overview::attention_panel(
                                self.home_attention(now, ai_configured),
                                cx,
                            )),
                    ),
            )
            .child(self.home_machines(now, window, cx))
            .into_any_element()
    }

    /// 谁在用: the people who asked AI members for the most work in the range.
    fn who_uses(&self, u: &chat_core::proto::TeamUsage, cx: &mut Context<Self>) -> AnyElement {
        let k = native_colors(cx);
        let mut rows: Vec<_> = u
            .initiators
            .iter()
            .filter(|x| x.spend.topics > 0 || x.spend.cost_usd > 0.)
            .collect();
        rows.sort_by(|a, b| {
            b.spend
                .topics
                .cmp(&a.spend.topics)
                .then(b.spend.cost_usd.total_cmp(&a.spend.cost_usd))
        });
        let max = rows.iter().map(|x| x.spend.cost_usd).fold(0., f64::max);
        let view = cx.entity();
        let all = push_button(
            "home-all-members",
            m::HOME_ALL_MEMBERS,
            PushStyle::Ghost,
            PushSize::Small,
            cx,
        )
        .on_click(move |_, window, cx| {
            view.update(cx, |v, cx| v.open_page(AdminPage::Members, window, cx))
        });
        let n = rows.len().min(WHO_ROWS);
        card_surface(cx)
            .flex_1()
            .p(space::S24)
            .flex()
            .flex_col()
            .gap(space::S8)
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(space::S8)
                    .child(
                        div()
                            .flex_1()
                            .n_title3()
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .child(m::HOME_WHO),
                    )
                    .child(all),
            )
            .when(rows.is_empty(), |d| {
                d.child(EmptyLine::new(m::HOME_WHO_EMPTY).icon(Sym::Members))
            })
            .children(rows.into_iter().take(WHO_ROWS).enumerate().map(|(i, x)| {
                let member = self.services.provider.member(&x.member_id);
                let name = member
                    .as_ref()
                    .map(|mm| mm.name.clone())
                    .unwrap_or_else(|| m::UNKNOWN_MEMBER.to_string());
                let role = member.as_ref().and_then(|mm| mm.role.clone());
                // 常找: the AI members this person asked most, from the split by person.
                let mut most: Vec<_> = u
                    .pairs
                    .iter()
                    .filter(|p| p.initiator_id == x.member_id && p.spend.topics > 0)
                    .collect();
                most.sort_by(|a, b| {
                    b.spend
                        .topics
                        .cmp(&a.spend.topics)
                        .then(b.spend.cost_usd.total_cmp(&a.spend.cost_usd))
                });
                let most: Vec<AnyElement> = most
                    .iter()
                    .take(WHO_AVATARS)
                    .filter_map(|p| self.services.provider.member(&p.member_id))
                    .map(|mm| MemberAvatar::new(Some(&mm), avatar::XS).into_any_element())
                    .collect();
                div()
                    .flex()
                    .items_center()
                    .gap(space::S12)
                    .py(space::S8)
                    .when(i > 0 && i < n, |d| d.border_t_1().border_color(k.separator))
                    .child(MemberAvatar::new(member.as_ref(), avatar::SM))
                    .child(
                        div()
                            .flex_1()
                            .min_w(gpui::Pixels::ZERO)
                            .flex()
                            .flex_col()
                            .child(TruncText::new(name).n_headline())
                            .children(
                                role.map(|r| {
                                    TruncText::new(r).n_footnote().text_color(k.secondary)
                                }),
                            ),
                    )
                    .child(
                        div()
                            .w(t::COL_CPU)
                            .flex_none()
                            .flex()
                            .gap(space::S2)
                            .children(most),
                    )
                    .child(
                        div()
                            .w(t::COL_COUNT)
                            .flex_none()
                            .flex()
                            .justify_end()
                            .n_body()
                            .n_figures()
                            .text_color(k.secondary)
                            .child(fill(
                                m::HOME_WHO_TOPICS,
                                &[("n", &x.spend.topics.to_string())],
                            )),
                    )
                    .child(
                        div()
                            .w(t::COL_SPEND)
                            .flex_none()
                            .flex()
                            .flex_col()
                            .gap(space::S4)
                            .child(
                                div()
                                    .flex()
                                    .justify_end()
                                    .n_headline()
                                    .n_figures()
                                    .child(usd(x.spend.cost_usd)),
                            )
                            .child(progress(
                                if max > 0. {
                                    (x.spend.cost_usd / max) as f32
                                } else {
                                    0.
                                },
                                cx,
                            )),
                    )
            }))
            .into_any_element()
    }

    /// The machine cards, two a row, under a 机器 heading with 管理机器.
    fn home_machines(&self, now: u64, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let k = native_colors(cx);
        let cards: Vec<AnyElement> = self
            .machines
            .iter()
            .flatten()
            .map(|x| self.machine_card(x, now, window, cx))
            .collect();
        let view = cx.entity();
        let manage = push_button(
            "home-machines",
            m::HOME_MANAGE_MACHINES,
            PushStyle::Ghost,
            PushSize::Small,
            cx,
        )
        .on_click(move |_, window, cx| {
            view.update(cx, |v, cx| v.open_page(AdminPage::Machines, window, cx))
        });
        let mut rows: Vec<AnyElement> = vec![];
        let mut it = cards.into_iter();
        while let Some(a) = it.next() {
            let b = it.next();
            rows.push(
                div()
                    .w_full()
                    .flex()
                    .gap(space::S16)
                    .child(div().flex_1().min_w(gpui::Pixels::ZERO).flex().child(a))
                    .child(div().flex_1().min_w(gpui::Pixels::ZERO).flex().children(b))
                    .into_any_element(),
            );
        }
        div()
            .flex()
            .flex_col()
            .gap(space::S12)
            .child(
                div()
                    .h(height::DEFAULT)
                    .flex()
                    .items_center()
                    .gap(space::S8)
                    .child(
                        div()
                            .flex_1()
                            .n_title3()
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .child(m::NAV_MACHINES),
                    )
                    .child(manage),
            )
            .when(rows.is_empty(), |d| {
                d.child(div().n_body().text_color(k.secondary).child(m::NO_MACHINES))
            })
            .children(rows)
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compare_says_more_less_or_same() {
        assert_eq!(compare(5, 5), m::HOME_SAME);
        assert_eq!(compare(7, 5), fill(m::HOME_MORE, &[("n", "2")]));
        assert_eq!(compare(3, 5), fill(m::HOME_LESS, &[("n", "2")]));
    }
}
