//! The topic inspector in the native kit (NATIVE.md §9.3): sections 负责,
//! 计划, 改动, 产出, 用到的决定, 在哪干活 and 用时和成本, each an
//! [`InspectorSection`] with a disclosure header; Body rows with `secondary`
//! labels and `label` values; actions as a trailing `arrow.up.forward`
//! borderless button; plan progress as the 4 pt determinate bar. No pills,
//! no text links, no hover fills on rows.

use gpui::{
    AnyElement, App, ClickEvent, Context, ElementId, Hsla, InteractiveElement as _, IntoElement,
    ParentElement, SharedString, StatefulInteractiveElement as _, Styled, div, px,
};

use super::{
    Data, TopicRail, duration_parts, media_time, money_text, preview_thumbnail, step_minutes,
};
use crate::{
    app::RightPane,
    copy::{fill, topic_rail as c},
    data::{BotRunSummary, PlanState, RunStatus, v2::ChangeState},
    theme::{Type as _, space},
    ui::{
        self, Sym, SymSize,
        native::{
            InspectorSection, NativeType as _, borderless_button, determinate_bar, label_value,
            native_colors,
        },
        sf_symbols::Sf,
    },
    views::shell::clock,
};

/// The sections, for their collapsed state.
pub(super) const OWNER: &str = "owner";
pub(super) const PLAN: &str = "plan";
pub(super) const CHANGES: &str = "changes";
pub(super) const OUTPUTS: &str = "outputs";
pub(super) const DECISIONS: &str = "decisions";
pub(super) const WHERE: &str = "where";
pub(super) const COST: &str = "cost";

/// A small symbol in a 14 pt slot, beside 11–13 pt text.
fn mark(sym: Sym, color: Hsla) -> AnyElement {
    ui::sym(sym, SymSize::INLINE_SMALL, color).into_any_element()
}

/// A 6 pt state dot.
fn dot(color: Hsla) -> AnyElement {
    div()
        .flex_none()
        .size(px(6.))
        .rounded(px(3.))
        .bg(color)
        .into_any_element()
}

/// A Body row: leading content, a flexible middle, trailing content.
fn row(cx: &App) -> gpui::Div {
    let k = native_colors(cx);
    div()
        .w_full()
        .min_h(px(24.))
        .flex()
        .items_center()
        .gap(space::S6)
        .n_body()
        .text_color(k.label)
}

/// A `secondary` Subheadline line under a row.
fn detail(text: impl Into<SharedString>, cx: &App) -> gpui::Div {
    let k = native_colors(cx);
    div()
        .w_full()
        .n_subheadline()
        .n_figures()
        .text_color(k.secondary)
        .one_line()
        .child(text.into())
}

impl TopicRail {
    fn section(
        &self,
        key: &'static str,
        title: &'static str,
        cx: &mut Context<Self>,
    ) -> InspectorSection {
        InspectorSection::new(ElementId::Name(format!("rail-{key}").into()), title)
            .collapsed(self.collapsed.contains(key))
            .on_toggle(cx.listener(move |this, _: &ClickEvent, _, cx| {
                if !this.collapsed.remove(key) {
                    this.collapsed.insert(key);
                }
                cx.notify();
            }))
    }

    /// The trailing `arrow.up.forward` that opens `pane`.
    fn open_button(
        &self,
        id: &'static str,
        name: &'static str,
        pane: RightPane,
        window: &mut gpui::Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        borderless_button(id, Sf::Open, name, window, cx)
            .on_click(
                cx.listener(move |this, _: &ClickEvent, _, cx| this.open_pane(pane.clone(), cx)),
            )
            .into_any_element()
    }

    pub(super) fn native_owner(
        &self,
        d: &Data,
        owner: &str,
        run: Option<&BotRunSummary>,
        by: Option<(String, i64)>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let k = native_colors(cx);
        let member = d.member(owner);
        let state: Option<AnyElement> = match run {
            Some(r) if r.status.is_active() => {
                let done = r.plan.iter().filter(|i| i.state == PlanState::Done).count();
                Some(
                    div()
                        .flex_none()
                        .flex()
                        .items_center()
                        .gap(space::S4)
                        .n_subheadline()
                        .text_color(k.secondary)
                        .child(dot(k.blue))
                        .child(if r.plan.is_empty() {
                            fill(c::STATE_WORKING_NO_PLAN, &[("name", "")])
                                .trim()
                                .to_string()
                        } else {
                            fill(
                                c::OWNER_WORKING,
                                &[
                                    ("n", &(done + 1).min(r.plan.len()).to_string()),
                                    ("total", &r.plan.len().to_string()),
                                ],
                            )
                        })
                        .into_any_element(),
                )
            }
            // A run that failed or was stopped says so (never 做完了).
            Some(r) if matches!(r.status, RunStatus::Failed | RunStatus::Cancelled) => {
                Some(crate::views::process::status_badge(r.status, cx).into_any_element())
            }
            Some(r) if r.ended_at.is_some() => Some(
                div()
                    .flex_none()
                    .flex()
                    .items_center()
                    .gap(space::S4)
                    .n_subheadline()
                    .n_figures()
                    .text_color(k.secondary)
                    .child(mark(Sym::Check, k.secondary))
                    .child(fill(
                        c::DONE_AT,
                        &[("time", &r.ended_at.map(clock).unwrap_or_default())],
                    ))
                    .into_any_element(),
            ),
            _ => None,
        };
        let role = member
            .as_ref()
            .and_then(|m| m.role.clone())
            .filter(|r| !r.trim().is_empty());
        let mut section = self.section(OWNER, c::TOPIC_RAIL_OWNER, cx).row(
            row(cx)
                .gap(space::S8)
                .child(ui::MemberAvatar::new(member.as_ref(), px(20.)))
                .child(
                    div()
                        .min_w_0()
                        .flex_shrink(1.)
                        .one_line()
                        .child(member.as_ref().map(|m| m.name.clone()).unwrap_or_default()),
                )
                .children(role.map(|r| {
                    div()
                        .flex_shrink(1.)
                        .min_w(px(24.))
                        .one_line()
                        .n_subheadline()
                        .text_color(k.secondary)
                        .child(r)
                }))
                .child(div().flex_1())
                .children(state),
        );
        if let Some((id, at)) = by {
            // A long name truncates at the tail; it never runs under the label.
            section = section.row(label_value(
                c::INSPECTOR_ASKED,
                div().min_w(px(0.)).truncate().child(fill(
                    c::DECISION_BY,
                    &[("name", &d.name(&id)), ("time", &clock(at))],
                )),
                cx,
            ));
        }
        section.into_any_element()
    }

    pub(super) fn native_plan(
        &self,
        d: &Data,
        run: &BotRunSummary,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let k = native_colors(cx);
        let minutes = step_minutes(run, &d.step_times);
        let total = run.plan.len();
        let done = run
            .plan
            .iter()
            .filter(|i| i.state == PlanState::Done)
            .count();
        let all_done = done == total;
        let live = run.status.is_active();
        let took = match (run.started_at, run.ended_at) {
            (Some(a), Some(b)) => Some(((b - a).max(0)) as u64),
            _ => None,
        };
        let summary = row(cx)
            .child(if all_done {
                mark(Sym::Done, k.green)
            } else if live {
                div()
                    .w(px(14.))
                    .flex()
                    .justify_center()
                    .child(dot(k.blue))
                    .into_any_element()
            } else {
                mark(Sym::Todo, k.secondary)
            })
            .child(if all_done {
                fill(c::PLAN_ALL_DONE, &[("n", &total.to_string())])
            } else {
                fill(
                    c::OWNER_WORKING,
                    &[
                        ("n", &(done + 1).min(total).to_string()),
                        ("total", &total.to_string()),
                    ],
                )
            })
            .child(div().flex_1())
            .children(took.map(|ms| {
                let (n, unit) = duration_parts(ms);
                div()
                    .n_subheadline()
                    .n_figures()
                    .text_color(k.secondary)
                    .child(format!("{n} {unit}"))
            }));
        let value = if total == 0 {
            0.
        } else {
            done as f32 / total as f32
        };
        let bar = div().py(space::S6).child(determinate_bar(
            value,
            (live && !all_done).then_some(k.blue),
            cx,
        ));
        let mut section = self
            .section(PLAN, c::TOPIC_RAIL_PLAN, cx)
            .row(summary)
            .row(bar);
        for (i, item) in run.plan.iter().enumerate() {
            let glyph = match item.state {
                PlanState::Done => mark(Sym::Check, k.secondary),
                PlanState::Current => div()
                    .w(px(14.))
                    .flex()
                    .justify_center()
                    .child(super::live_dot(
                        ElementId::Name(format!("rail-step-{i}").into()),
                        k.blue,
                        cx,
                    ))
                    .into_any_element(),
                PlanState::Waiting => mark(Sym::NeedsYou, k.amber),
                PlanState::Pending => mark(Sym::Todo, k.tertiary),
            };
            let ink = match item.state {
                PlanState::Done | PlanState::Pending => k.secondary,
                _ => k.label,
            };
            section = section.row(
                row(cx)
                    .child(div().flex_none().w(px(14.)).child(glyph))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .one_line()
                            .text_color(ink)
                            .child(item.text.clone()),
                    )
                    .children(minutes.get(i).copied().flatten().map(|m| {
                        div()
                            .flex_none()
                            .n_subheadline()
                            .n_figures()
                            .text_color(k.secondary)
                            .child(fill(c::STEP_MINUTES, &[("n", &m.to_string())]))
                    })),
            );
        }
        section.into_any_element()
    }

    pub(super) fn native_changes(
        &self,
        d: &Data,
        window: &mut gpui::Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let k = native_colors(cx);
        let ch = d.changes.clone()?;
        let action = self.open_button(
            "rail-see-changes",
            c::OPEN_CHANGES,
            RightPane::Changes,
            window,
            cx,
        );
        let state: AnyElement = match ch.state {
            ChangeState::Merging => div()
                .flex()
                .items_center()
                .gap(space::S4)
                .child(dot(k.blue))
                .child(c::CHANGES_MERGING)
                .into_any_element(),
            ChangeState::Merged => div()
                .flex()
                .items_center()
                .gap(space::S4)
                .child(mark(Sym::Merge, k.secondary))
                .child(c::CHANGES_MERGED)
                .into_any_element(),
            ChangeState::Closed => div().child(c::CHANGES_CLOSED).into_any_element(),
            _ => div().child(c::CHANGES_OPEN).into_any_element(),
        };
        let checks_ok = ch.checks_total > 0 && ch.checks_passed == ch.checks_total;
        let mut section = self
            .section(CHANGES, c::TOPIC_RAIL_CHANGES, cx)
            .action(action)
            .row(row(cx).child(div().min_w_0().one_line().child(ch.title.clone())))
            .row(
                div()
                    .w_full()
                    .flex()
                    .items_center()
                    .gap(space::S6)
                    .n_subheadline()
                    .n_figures()
                    .text_color(k.secondary)
                    .child(format!("#{}", ch.number))
                    .child(
                        div()
                            .text_color(k.green)
                            .child(format!("+{}", ch.additions)),
                    )
                    .child(div().text_color(k.red).child(format!("−{}", ch.deletions)))
                    .child(div().flex_1())
                    .child(fill(c::INSPECTOR_FILES, &[("n", &ch.files.to_string())])),
            );
        if ch.checks_total > 0 {
            section = section.row(
                div().pt(space::S6).child(label_value(
                    c::INSPECTOR_CHECKS,
                    div()
                        .flex()
                        .items_center()
                        .gap(space::S4)
                        .child(if checks_ok {
                            mark(Sym::Done, k.green)
                        } else {
                            mark(Sym::Problem, k.red)
                        })
                        .child(fill(
                            c::INSPECTOR_CHECKS_VALUE,
                            &[
                                ("passed", &ch.checks_passed.to_string()),
                                ("total", &ch.checks_total.to_string()),
                            ],
                        )),
                    cx,
                )),
            );
        }
        section = section.row(label_value(c::INSPECTOR_STATE, state, cx));
        Some(section.into_any_element())
    }

    pub(super) fn native_outputs(
        &self,
        d: &Data,
        window: &mut gpui::Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let k = native_colors(cx);
        let first = d.artifacts.first()?.clone();
        let pane = RightPane::Artifact {
            id: first.id.clone(),
            version: None,
        };
        let action = self.open_button("rail-open-output", c::OPEN_OUTPUT, pane.clone(), window, cx);
        let mut section = self
            .section(OUTPUTS, c::TOPIC_RAIL_OUTPUTS, cx)
            .action(action);
        if let Some(preview) = d.preview.clone() {
            section = section.row(
                div()
                    .id("rail-output-thumb")
                    .mt(space::S4)
                    .mb(space::S6)
                    .w_full()
                    .h(px(80.))
                    .pt(space::S8)
                    .px(space::S12)
                    .rounded(px(8.))
                    .bg(k.fill)
                    .overflow_hidden()
                    .child(preview_thumbnail(&preview, cx))
                    .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                        this.open_pane(pane.clone(), cx)
                    })),
            );
        }
        for a in &d.artifacts {
            section = section.row(
                row(cx)
                    .child(mark(Sym::Document, k.secondary))
                    .child(
                        div()
                            .min_w_0()
                            .flex_shrink(1.)
                            .one_line()
                            .child(a.title.clone()),
                    )
                    .child(div().flex_1())
                    .child(
                        div()
                            .flex_none()
                            .n_subheadline()
                            .n_figures()
                            .text_color(k.secondary)
                            .child(fill(
                                c::OUTPUT_VERSION,
                                &[
                                    ("v", &a.version.to_string()),
                                    ("time", &clock(a.updated_at)),
                                ],
                            )),
                    ),
            );
        }
        if let Some(ms) = d.recording {
            section = section.row(
                row(cx)
                    .child(mark(Sym::Video, k.secondary))
                    .child(c::RECORDING_ROW)
                    .child(div().flex_1())
                    .child(
                        div()
                            .n_subheadline()
                            .n_figures()
                            .text_color(k.secondary)
                            .child(media_time(ms)),
                    ),
            );
        }
        Some(section.into_any_element())
    }

    pub(super) fn native_decisions(&self, d: &Data, cx: &mut Context<Self>) -> Option<AnyElement> {
        let k = native_colors(cx);
        if d.decisions.is_empty() {
            return None;
        }
        let mut section = self.section(DECISIONS, c::TOPIC_RAIL_DECISIONS, cx);
        for dec in &d.decisions {
            section = section.row(
                div()
                    .w_full()
                    .py(space::S2)
                    .flex()
                    .items_start()
                    .gap(space::S6)
                    .child(
                        div()
                            .flex_none()
                            .pt(px(2.))
                            .child(mark(Sym::Check, k.secondary)),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .child(div().n_body().text_color(k.label).child(dec.text.clone()))
                            .child(detail(
                                fill(
                                    c::DECISION_BY,
                                    &[("name", &d.name(&dec.by)), ("time", &clock(dec.at))],
                                ),
                                cx,
                            )),
                    ),
            );
        }
        Some(section.into_any_element())
    }

    /// 在哪干活: the team computer (or the cloud runner) the member uses.
    pub(super) fn native_where(
        &self,
        d: &Data,
        run: &BotRunSummary,
        window: &mut gpui::Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let k = native_colors(cx);
        let name = run.machine_name.clone()?;
        let busy = run.status.is_active();
        let who = d.name(&run.bot_id);
        let line = if busy {
            fill(c::MACHINE_USING, &[("name", &who)])
        } else {
            fill(
                c::MACHINE_DONE,
                &[
                    ("name", &who),
                    ("time", &run.ended_at.map(clock).unwrap_or_default()),
                ],
            )
        };
        let lower = name.to_lowercase();
        let sym = if run
            .machine_id
            .as_deref()
            .is_some_and(|id| id.starts_with("cloud"))
        {
            Sym::Cloud
        } else if lower.contains("mini") {
            Sym::MacMini
        } else if lower.contains("book") || lower.contains("laptop") {
            Sym::Laptop
        } else {
            Sym::Computer
        };
        let mut section = self.section(WHERE, c::WHERE_WORK, cx);
        if busy {
            section = section.action(self.open_button(
                "rail-see-machine",
                c::OPEN_MACHINE,
                RightPane::Live(run.id.clone()),
                window,
                cx,
            ));
        }
        section = section.row(
            div()
                .w_full()
                .py(space::S2)
                .flex()
                .items_start()
                .gap(space::S8)
                .child(div().flex_none().pt(px(1.)).child(ui::sym(
                    sym,
                    SymSize::INLINE,
                    k.secondary,
                )))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .flex()
                        .flex_col()
                        .child(
                            row(cx)
                                .min_h(px(18.))
                                .child(div().min_w_0().flex_shrink(1.).one_line().child(name))
                                .child(div().flex_1())
                                .child(
                                    div()
                                        .flex_none()
                                        .flex()
                                        .items_center()
                                        .gap(space::S4)
                                        .n_subheadline()
                                        .text_color(k.secondary)
                                        .child(dot(if busy { k.blue } else { k.tertiary }))
                                        .child(if busy {
                                            c::MACHINE_BUSY
                                        } else {
                                            c::MACHINE_IDLE
                                        }),
                                ),
                        )
                        .child(detail(line, cx)),
                ),
        );
        Some(section.into_any_element())
    }

    pub(super) fn native_cost(&self, d: &Data, cx: &mut Context<Self>) -> Option<AnyElement> {
        let cost = d.cost.clone()?;
        if cost.duration_ms == 0 && cost.cost_cents.is_none() {
            return None;
        }
        let (n, unit) = duration_parts(cost.duration_ms);
        let mut section = self.section(COST, c::TOPIC_RAIL_COST, cx).row(label_value(
            c::INSPECTOR_TIME,
            format!("{n} {unit}"),
            cx,
        ));
        if let Some(cents) = cost.cost_cents {
            section = section.row(label_value(
                c::INSPECTOR_COST,
                money_text(cents, cost.currency),
                cx,
            ));
        }
        if let (Some(a), Some(b)) = (cost.started_at, cost.ended_at) {
            section = section.row(label_value(c::INSPECTOR_WHEN, when_range(a, b), cx));
        }
        Some(section.into_any_element())
    }
}

/// When the work ran: `今天 09:14–09:55`, `11-01 10:21–14:21` on one other
/// day, or `10-31 22:10 – 11-01 01:05` across days (never `今天 11-01–11-01`).
fn when_range(a: crate::data::Millis, b: crate::data::Millis) -> String {
    use chrono::{Local, TimeZone as _};
    let (Some(start), Some(end)) = (
        Local.timestamp_millis_opt(a).single(),
        Local.timestamp_millis_opt(b).single(),
    ) else {
        return String::new();
    };
    let today = crate::selftest::clock::local_now().date_naive();
    let times = format!("{}–{}", start.format("%H:%M"), end.format("%H:%M"));
    if start.date_naive() != end.date_naive() {
        fill(
            c::INSPECTOR_WHEN_SPAN,
            &[
                ("start", &start.format("%m-%d %H:%M").to_string()),
                ("end", &end.format("%m-%d %H:%M").to_string()),
            ],
        )
    } else if start.date_naive() == today {
        fill(c::INSPECTOR_WHEN_VALUE, &[("range", &times)])
    } else {
        fill(
            c::INSPECTOR_WHEN_DAY,
            &[
                ("day", &start.format("%m-%d").to_string()),
                ("range", &times),
            ],
        )
    }
}

#[cfg(test)]
mod tests {
    use chrono::{Local, TimeZone as _};

    use super::when_range;

    fn at(y: i32, m: u32, d: u32, h: u32, min: u32) -> crate::data::Millis {
        Local
            .with_ymd_and_hms(y, m, d, h, min, 0)
            .single()
            .unwrap()
            .timestamp_millis()
    }

    #[test]
    fn a_range_on_another_day_names_the_day_once() {
        assert_eq!(
            when_range(at(2020, 11, 1, 10, 21), at(2020, 11, 1, 14, 21)),
            "11-01 10:21–14:21"
        );
    }

    #[test]
    fn a_range_across_days_names_both_days() {
        assert_eq!(
            when_range(at(2020, 10, 31, 22, 10), at(2020, 11, 1, 1, 5)),
            "10-31 22:10 – 11-01 01:05"
        );
    }
}
