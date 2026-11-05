//! The topic inspector in the native kit (NATIVE.md §9.3): sections 负责,
//! 计划, 改动, 产出, 用到的决定, 在哪干活 and 用时和成本, each an
//! [`InspectorSection`] with a disclosure header; Body rows with `secondary`
//! labels and `label` values; actions as a trailing `arrow.up.forward`
//! borderless button; plan progress as the 4 pt determinate bar. No pills,
//! no text links, no hover fills on rows.

use gpui::{
    AnyElement, App, ClickEvent, Context, ElementId, Hsla, InteractiveElement as _, IntoElement,
    ParentElement, SharedString, StatefulInteractiveElement as _, Styled, div,
    prelude::FluentBuilder as _, px,
};

use super::{Data, TopicRail, media_time, money_text, preview_thumbnail, step_minutes};
use crate::{
    app::RightPane,
    copy::{fill, topic_rail as c},
    data::{BotRunSummary, PlanState, v2::ChangeState},
    theme::{Type as _, space},
    ui::{
        self, Sym, SymSize,
        native::{
            BadgeState, EmptyLine, FocusRow as _, InspectorSection, NativeType as _, StatusBadge,
            badge::state_mark, borderless_button, label_value, native_colors, progress,
            tokens::radius,
        },
        sf_symbols::Sf,
    },
    views::shell::clock,
};

/// The sections, for their collapsed state.
pub(super) const STATUS: &str = "status";
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

/// The room of a person value in the inspector: the 320 pane less its
/// px 16, the 112 label column, its gap, and the avatar 16 with its gap 8.
const VALUE_ROOM: f32 = 320. - 32. - 112. - 8. - 24.;
/// A time after the person (`提出 … 09:30`) and its gap.
const TAIL_ROOM: f32 = 48.;

/// About how wide `text` sets at 14: CJK 14, other characters 7.5.
fn text_width(text: &str) -> f32 {
    text.chars()
        .map(|ch| if ch.is_ascii() { 7.5 } else { 14. })
        .sum()
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

    /// 状态: the one state of the 话题 (`topic_page::topic_state`, the same
    /// words as the title bar and the sidebar) as a StatusBadge, and the
    /// step a member is on under it. Always shown, first.
    pub(super) fn native_status(&self, d: &Data, cx: &mut Context<Self>) -> AnyElement {
        let k = native_colors(cx);
        let (badge, text, detail) = match d.state.as_ref() {
            Some(s) => (s.badge, s.text.clone(), s.detail.clone()),
            None => (
                BadgeState::Queued,
                crate::copy::common::STATUS_QUEUED.to_string(),
                None,
            ),
        };
        let mut section = self
            .section(STATUS, c::INSPECTOR_STATE, cx)
            .row(row(cx).child(StatusBadge::new(badge, text).max_w(px(288.))));
        if let Some(step) = detail {
            section = section.row(
                div()
                    .w_full()
                    .pt(space::S4)
                    .n_body()
                    .text_color(k.secondary)
                    .child(crate::ui::truncate::TruncText::new(step).id("rail-state-detail")),
            );
        }
        section.into_any_element()
    }

    /// A section with no data (§2.13 compact Empty): its header and one
    /// 14 muted line with a 16 icon, so every 话题 shows the same sections
    /// in the same order.
    pub(super) fn native_empty(
        &self,
        key: &'static str,
        title: &'static str,
        line: &'static str,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let sym = match key {
            OWNER => Sym::Person,
            PLAN => Sym::Checklist,
            CHANGES => Sym::PullRequest,
            OUTPUTS => Sym::Document,
            DECISIONS => Sym::Check,
            WHERE => Sym::Computer,
            _ => Sym::Waiting,
        };
        self.section(key, title, cx)
            .row(EmptyLine::new(line).icon(sym))
            .into_any_element()
    }

    /// A person value (§2.5): avatar 16 and the name; the role truncates
    /// first, then the name, each with a tooltip when cut. The name and the
    /// role are separate runs, so a right-to-left name never reorders the
    /// line around it.
    fn person(
        &self,
        id: &str,
        member: Option<&crate::data::Member>,
        tail: Option<AnyElement>,
        cx: &App,
    ) -> AnyElement {
        let k = native_colors(cx);
        // X-01, C5: beside its avatar a name drops its leading emoji.
        let name = member
            .map(|m| crate::ui::avatar::name_beside_avatar(&m.name).to_string())
            .unwrap_or_default();
        // R2-14, §2.21: the role truncates first, and goes when fewer than
        // 2 of its characters would fit (never a lone `…`).
        let room = VALUE_ROOM - if tail.is_some() { TAIL_ROOM } else { 0. };
        let role = member
            .and_then(|m| m.role.clone())
            .filter(|r| !r.trim().is_empty())
            .filter(|r| {
                let two: String = r.chars().take(2).collect();
                text_width(&name) + 8. + text_width(&two) + 14. <= room
            });
        div()
            .w_full()
            .min_w(px(0.))
            .flex()
            .items_center()
            .gap(space::S8)
            .child(ui::MemberAvatar::new(member, px(16.)))
            .child(
                // The name gives way only once the role is gone.
                div()
                    .min_w(px(24.))
                    .flex_shrink(1.)
                    .text_color(k.label)
                    .child(
                        crate::ui::truncate::TruncText::new(name)
                            .id(SharedString::from(format!("rail-person-{id}"))),
                    ),
            )
            .children(role.map(|r| {
                div()
                    .min_w(px(0.))
                    .flex_shrink(1000.)
                    .text_color(k.secondary)
                    .child(
                        crate::ui::truncate::TruncText::new(r)
                            .id(SharedString::from(format!("rail-role-{id}"))),
                    )
            }))
            .children(tail)
            .into_any_element()
    }

    /// 成员 (§2.5): 负责 and 提出 as DescriptionList rows of people. The
    /// state is in 状态 only; this section never repeats it.
    pub(super) fn native_owner(
        &self,
        d: &Data,
        owner: Option<&str>,
        by: Option<(String, i64)>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let k = native_colors(cx);
        // R2-17, §2.13: no owner is a compact empty (icon and muted line),
        // and the section header offers the ghost 指派 (§2.5).
        let owner_value: AnyElement = match owner {
            Some(id) => self.person(id, d.member(id).as_ref(), None, cx),
            None => EmptyLine::new(c::RAIL_EMPTY_OWNER)
                .icon(Sym::Person)
                .into_any_element(),
        };
        let mut section = self.section(OWNER, c::TOPIC_RAIL_PEOPLE, cx);
        if owner.is_none()
            && let Some(t) = d.topic.as_ref()
        {
            section = section.action(self.assign_button(t, cx));
        }
        let mut section = section.row(label_value(c::INSPECTOR_OWNER, owner_value, cx));
        if let Some((id, at)) = by {
            // The time keeps a fixed tabular slot; the name truncates first.
            let time = div()
                .flex_none()
                .n_subheadline()
                .n_figures()
                .text_color(k.secondary)
                .child(clock(at))
                .into_any_element();
            section = section.row(label_value(
                c::INSPECTOR_ASKED,
                self.person(&format!("by-{id}"), d.member(&id).as_ref(), Some(time), cx),
                cx,
            ));
        }
        section.into_any_element()
    }

    /// 计划: the plan of the run the rail shows and, while more members
    /// work (A2-11), one block per member's run, each led by its member.
    pub(super) fn native_plan(
        &self,
        d: &Data,
        run: &BotRunSummary,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let mut section = self.section(PLAN, c::TOPIC_RAIL_PLAN, cx);
        let many = !d.others.is_empty();
        let blocks = std::iter::once((run, d.step_times.as_slice()))
            .chain(d.others.iter().map(|(r, t)| (r, t.as_slice())));
        for (i, (run, times)) in blocks.enumerate() {
            if many {
                let who = self.person(
                    &format!("plan-{}", run.id),
                    d.member(&run.bot_id).as_ref(),
                    None,
                    cx,
                );
                section = section.row(
                    div()
                        .w_full()
                        .when(i > 0, |el| el.pt(space::S12))
                        .pb(space::S4)
                        .n_body()
                        .child(who),
                );
            }
            section = self.plan_rows(section, run, times, cx);
        }
        section.into_any_element()
    }

    fn plan_rows(
        &self,
        mut section: InspectorSection,
        run: &BotRunSummary,
        times: &[crate::data::Millis],
        cx: &mut Context<Self>,
    ) -> InspectorSection {
        let k = native_colors(cx);
        let minutes = step_minutes(run, times);
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
                fill(c::PLAN_ALL_DONE, &[("n", &total.to_string())])
            } else {
                // §2.12, T-02: one count, done/total, as the bar and the card.
                fill(
                    c::OWNER_WORKING,
                    &[("n", &done.to_string()), ("total", &total.to_string())],
                )
            })
            .child(div().flex_1())
            .children(took.map(|ms| {
                div()
                    .n_subheadline()
                    .n_figures()
                    .text_color(k.secondary)
                    .child(crate::views::timeline::plan::span_text(
                        crate::views::timeline::plan::TOOK,
                        ms as i64,
                    ))
            }));
        let value = if total == 0 {
            0.
        } else {
            done as f32 / total as f32
        };
        let _ = live;
        // D12: the one Progress (h4, primary on primary 20 %).
        let bar = div().py(space::S6).child(progress(value, cx));
        section = section.row(summary).row(bar);
        for (i, item) in run.plan.iter().enumerate() {
            // §2.12: the step at work spins (info), a step that waits on a
            // person has the needs-you icon; never a free colored dot.
            let glyph = match item.state {
                PlanState::Done => mark(Sym::Check, k.secondary),
                PlanState::Current => state_mark(BadgeState::Working, &k)
                    .unwrap_or_else(|| mark(Sym::Todo, k.secondary)),
                PlanState::Waiting => state_mark(BadgeState::NeedsYou, &k)
                    .unwrap_or_else(|| mark(Sym::NeedsYou, k.amber)),
                PlanState::Pending => mark(Sym::Todo, k.tertiary),
            };
            let ink = match item.state {
                PlanState::Done | PlanState::Pending => k.secondary,
                _ => k.label,
            };
            section = section.row(
                row(cx)
                    .child(
                        div()
                            .flex_none()
                            .w(px(16.))
                            .flex()
                            .justify_center()
                            .child(glyph),
                    )
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
                            .child(crate::views::timeline::plan::span_text(
                                crate::views::timeline::plan::TOOK,
                                (m * 60_000) as i64,
                            ))
                    })),
            );
        }
        section
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
        // §2.12: every state value is a StatusBadge, never plain or colored
        // text.
        let (state_kind, state_text) = match ch.state {
            ChangeState::Merging => (BadgeState::Merging, c::CHANGES_MERGING),
            ChangeState::Merged => (BadgeState::Done, c::CHANGES_MERGED),
            ChangeState::Closed => (BadgeState::Offline, c::CHANGES_CLOSED),
            _ => (BadgeState::Queued, c::CHANGES_OPEN),
        };
        let state = StatusBadge::new(state_kind, state_text);
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
                    .child(format!("+{}", ch.additions))
                    .child(format!("−{}", ch.deletions))
                    .child(div().flex_1())
                    .child(fill(c::INSPECTOR_FILES, &[("n", &ch.files.to_string())])),
            );
        if ch.checks_total > 0 {
            section = section.row(div().pt(space::S6).child(label_value(
                c::INSPECTOR_CHECKS,
                StatusBadge::new(
                    if checks_ok {
                        BadgeState::Done
                    } else {
                        BadgeState::Failed
                    },
                    fill(
                        c::INSPECTOR_CHECKS_VALUE,
                        &[
                            ("passed", &ch.checks_passed.to_string()),
                            ("total", &ch.checks_total.to_string()),
                        ],
                    ),
                ),
                cx,
            )));
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
                    .rounded(radius::CONTROL)
                    .border_1()
                    .border_color(k.separator)
                    .bg(k.fill)
                    .overflow_hidden()
                    // T-16, E3: a tab stop that Enter / Space opens.
                    .focus_row(false, cx)
                    .child(preview_thumbnail(&preview, cx))
                    .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                        this.open_pane(pane.clone(), cx)
                    })),
            );
        }
        for a in &d.artifacts {
            // The version the page host knows when it is newer than the
            // provider's (a new version arrived): the embed and this row agree.
            let version = crate::views::artifacts::host::latest(&a.id, cx)
                .map_or(a.version, |v| v.max(a.version));
            // A2-21: the kind's own symbol (an image is not a document).
            let sym = match a.kind {
                crate::data::ArtifactKind::Image => Sym::Image,
                _ => Sym::Document,
            };
            // T-16, E3, F3: a row opens its 产出 (a tab stop), and a cut
            // title shows whole in a tooltip.
            let open = RightPane::Artifact {
                id: a.id.clone(),
                version: None,
            };
            section = section.row(
                row(cx)
                    .id(SharedString::from(format!("rail-output-{}", a.id)))
                    .rounded(radius::CONTROL)
                    .focus_row(false, cx)
                    .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                        this.open_pane(open.clone(), cx)
                    }))
                    .child(mark(sym, k.secondary))
                    .child(
                        crate::ui::truncate::TruncText::new(a.title.clone())
                            .id(SharedString::from(format!("rail-output-title-{}", a.id)))
                            .flex_shrink(1.),
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
                                &[("v", &version.to_string()), ("time", &clock(a.updated_at))],
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
        // R2-14: a right-to-left name keeps its place in the line (FSI…PDI).
        let who = crate::copy::isolate(&who);
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
                                .child(div().flex_none().child(StatusBadge::new(
                                    if busy {
                                        BadgeState::Working
                                    } else {
                                        BadgeState::Offline
                                    },
                                    // §2.12, A2-13: the machine at work
                                    // says the working word, 在做.
                                    if busy {
                                        c::STATE_WORKING_SHORT
                                    } else {
                                        c::MACHINE_IDLE
                                    },
                                ))),
                        )
                        .child(detail(line, cx)),
                ),
        );
        Some(section.into_any_element())
    }

    pub(super) fn native_cost(&self, d: &Data, cx: &mut Context<Self>) -> Option<AnyElement> {
        // One time source (T2-19, R2-04): while a member works, 用时 and 时间
        // are its run's, the numbers its working card shows, also before the
        // server reports a cost.
        let run = d.run.as_ref().filter(|r| r.status.is_active());
        let active = run.and_then(|r| r.started_at);
        let cost = match (d.cost, active) {
            (Some(cost), _) => cost,
            (None, Some(_)) => crate::data::v2::TopicCost {
                duration_ms: 0,
                cost_cents: None,
                currency: crate::data::v2::Currency::Usd,
                started_at: None,
                ended_at: None,
            },
            (None, None) => return None,
        };
        if active.is_none() && cost.duration_ms == 0 && cost.cost_cents.is_none() {
            return None;
        }
        // P-01: a run at work reads the one clock of every page.
        let ms = run
            .and_then(|r| crate::views::topic_page::run_elapsed(self.services.provider.as_ref(), r))
            .unwrap_or(cost.duration_ms as i64);
        let used = crate::views::timeline::plan::span_text(crate::views::timeline::plan::TOOK, ms);
        let mut section = self.section(COST, c::TOPIC_RAIL_COST, cx).row(label_value(
            c::INSPECTOR_TIME,
            used,
            cx,
        ));
        if let Some(cents) = cost.cost_cents {
            section = section.row(label_value(
                c::INSPECTOR_COST,
                money_text(cents, cost.currency),
                cx,
            ));
        }
        let when = match active {
            Some(at) => Some(run_since(at)),
            None => cost
                .started_at
                .zip(cost.ended_at)
                .map(|(a, b)| when_range(a, b)),
        };
        if let Some(when) = when {
            section = section.row(label_value(c::INSPECTOR_WHEN, when, cx));
        }
        Some(section.into_any_element())
    }
}

/// When a run at work started, in the one form of every place (T-20, §2.12
/// one clock): `今天 07:23 起`, `11-01 07:23 起`.
pub fn run_since(at: crate::data::Millis) -> String {
    fill(c::INSPECTOR_WHEN_SINCE, &[("when", &when_start(at))])
}

/// When a run at work started: `今天 07:23`, `11-01 07:23`.
fn when_start(at: crate::data::Millis) -> String {
    use chrono::{Local, TimeZone as _};
    let Some(start) =
        crate::views::shell::known(at).and_then(|at| Local.timestamp_millis_opt(at).single())
    else {
        return crate::copy::common::TIME_UNKNOWN.to_string();
    };
    let time = start.format("%H:%M").to_string();
    if start.date_naive() == crate::selftest::clock::local_now().date_naive() {
        fill(c::INSPECTOR_WHEN_VALUE, &[("range", &time)])
    } else {
        fill(
            c::INSPECTOR_WHEN_DAY,
            &[
                ("day", &start.format("%m-%d").to_string()),
                ("range", &time),
            ],
        )
    }
}

/// When the work ran: `今天 09:14–09:55`, `11-01 10:21–14:21` on one other
/// day, or `10-31 22:10 – 11-01 01:05` across days (never `今天 11-01–11-01`).
fn when_range(a: crate::data::Millis, b: crate::data::Millis) -> String {
    use chrono::{Local, TimeZone as _};
    let local =
        |at| crate::views::shell::known(at).and_then(|at| Local.timestamp_millis_opt(at).single());
    let (Some(start), Some(end)) = (local(a), local(b)) else {
        return crate::copy::common::TIME_UNKNOWN.to_string();
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
