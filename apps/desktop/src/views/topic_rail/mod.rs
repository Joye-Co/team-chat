//! The topic inspector beside a 话题 page (NATIVE.md §9.3, `native.rs`):
//! sections 负责, 计划, 改动, 产出, 用到的决定, 在哪干活 and 用时和成本 on
//! `sidebar`, each hidden when it has no data. Also the 改动 pane.
//!
//! The rail shows status only: the merge decision is the pinned card above
//! the composer, never a second button here.

use std::{collections::HashMap, rc::Rc};

use gpui::{
    AnyElement, App, Context, ElementId, Hsla, InteractiveElement as _, IntoElement, ParentElement,
    Pixels, Render, StatefulInteractiveElement as _, Styled, Subscription, Window, div,
    prelude::FluentBuilder as _, px,
};

use crate::{
    app::{Changes, RightPane, Services},
    copy::{fill, topic_rail as c},
    data::{
        ArtifactKind, ArtifactSummary, BotRunSummary, Member, MemberId, Millis, PlanState, Topic,
        v2::{ArtifactPreview, Currency, Decision, Proof, TopicChanges, TopicCost},
    },
    theme::{self, Type as _, space},
    ui::{self, Sym},
    views::shell::{PaneView, panes::watch, stub_body},
};

/// What the rail of one 话题 shows, read from the provider once per change
/// or 话题 switch. A breathing ring renders the rail on every frame it asks
/// for, so render reads only this.
struct Data {
    group_id: String,
    topic_id: String,
    topic: Option<Topic>,
    /// The newest run.
    run: Option<BotRunSummary>,
    step_times: Vec<Millis>,
    changes: Option<TopicChanges>,
    artifacts: Vec<ArtifactSummary>,
    /// The preview of the newest 产出.
    preview: Option<ArtifactPreview>,
    decisions: Vec<Decision>,
    cost: Option<TopicCost>,
    recording: Option<u64>,
    /// The members the cards name.
    members: HashMap<MemberId, Member>,
}

impl Data {
    fn read(services: &Services, g: &str, t: &str) -> Self {
        let p = &services.provider;
        let topic = p.topic(g, t);
        let run = p.runs(g, t).into_iter().next();
        let artifacts = p.artifacts(g, t);
        let preview = artifacts
            .first()
            .and_then(|a| p.artifact_preview(&a.id, a.version));
        let decisions = p.topic_decisions(g, t);
        let mut ids: Vec<&str> = Vec::new();
        if let Some(t) = &topic {
            ids.extend(t.owner_id.as_deref());
            ids.push(&t.created_by);
        }
        if let Some(r) = &run {
            ids.push(&r.bot_id);
        }
        ids.extend(decisions.iter().map(|d| d.by.as_str()));
        let members = ids
            .into_iter()
            .filter_map(|id| p.member(id).map(|m| (id.to_string(), m)))
            .collect();
        Self {
            group_id: g.to_string(),
            topic_id: t.to_string(),
            step_times: run
                .as_ref()
                .map(|r| p.run_step_times(&r.id))
                .unwrap_or_default(),
            changes: p.topic_changes(g, t),
            cost: p.topic_cost(g, t),
            recording: topic_recording(services, g, t),
            topic,
            run,
            artifacts,
            preview,
            decisions,
            members,
        }
    }

    fn member(&self, id: &str) -> Option<Member> {
        self.members.get(id).cloned()
    }

    fn name(&self, id: &str) -> String {
        self.members
            .get(id)
            .map(|m| m.name.clone())
            .unwrap_or_default()
    }
}

mod native;

// --- v12/fx-topic: one trailing pane width (coordinator decision): the
// inspector is 280 pt, the wider panes (单独问, 产出, 改动) are 400 pt. Use the
// shell's token for the wide pane once it lands.
/// The width of the wider trailing panes: 单独问, the topic's 产出 and 改动.
pub const PANE_WIDE: Pixels = gpui::px(400.);
// --- end v12/fx-topic ---

/// The empty inspector (NATIVE.md §9.4 empty states): centered `secondary`
/// Body text on the inspector fill, no illustration.
fn rail_empty(cx: &App) -> AnyElement {
    use crate::ui::native::{NativeType as _, native_colors};
    let n = native_colors(cx);
    div()
        .size_full()
        .bg(n.sidebar)
        .flex()
        .items_center()
        .justify_center()
        .px(space::S16)
        .n_body()
        .text_color(n.secondary)
        .child(c::TOPIC_RAIL_EMPTY)
        .into_any_element()
}

pub struct TopicRail {
    services: Services,
    /// The inspector sections closed with their disclosure chevron.
    collapsed: std::collections::HashSet<&'static str>,
    data: Option<Rc<Data>>,
    _subscriptions: Vec<Subscription>,
}

impl TopicRail {
    pub fn new(services: Services, cx: &mut Context<Self>) -> Self {
        let subscriptions = vec![
            cx.subscribe(&services.hub, |this, _, _: &Changes, cx| {
                this.data = None;
                cx.notify();
            }),
            cx.observe(&services.nav, |_, _, cx| cx.notify()),
        ];
        Self {
            _subscriptions: subscriptions,
            services,
            collapsed: Default::default(),
            data: None,
        }
    }

    /// The rail of 话题 `t`, read again only after a change.
    fn data(&mut self, g: &str, t: &str) -> Rc<Data> {
        match &self.data {
            Some(d) if d.group_id == g && d.topic_id == t => d.clone(),
            _ => {
                let d = Rc::new(Data::read(&self.services, g, t));
                self.data = Some(d.clone());
                d
            }
        }
    }
}

impl PaneView for TopicRail {
    fn width(&self, _window_w: Pixels) -> Pixels {
        crate::ui::native::metrics::INSPECTOR_WIDTH
    }
}

/// Minutes as `41 分钟`, hours past 90 minutes as `1.5 小时`.
pub fn duration_text(ms: u64) -> String {
    let (n, unit) = duration_parts(ms);
    format!("{n} {unit}")
}

/// A duration as its mono number and its unit (`41`, `分钟`).
pub fn duration_parts(ms: u64) -> (String, &'static str) {
    let min = ms / 60_000;
    if min >= 90 {
        (format!("{:.1}", min as f32 / 60.), c::HOURS_UNIT)
    } else {
        (min.to_string(), c::MINUTES_UNIT)
    }
}

/// Cents in the currency the data has: `$0.85` for US dollars, `¥6.20`
/// for yuan. Never converted.
pub fn money_text(cents: u64, currency: Currency) -> String {
    let v = format!("{}.{:02}", cents / 100, cents % 100);
    let template = match currency {
        Currency::Usd => c::MONEY_USD,
        Currency::Cny => c::MONEY_CNY,
    };
    fill(template, &[("v", &v)])
}

/// A recording length as `0:42`.
pub fn media_time(ms: u64) -> String {
    let s = ms / 1000;
    format!("{}:{:02}", s / 60, s % 60)
}

/// How long each plan step took, from the step start times and the run's
/// end (or now while it runs). Steps with no start time get `None`.
pub fn step_minutes(run: &BotRunSummary, starts: &[Millis]) -> Vec<Option<u64>> {
    let end = run
        .ended_at
        .unwrap_or_else(|| crate::selftest::clock::utc_now().timestamp_millis());
    (0..run.plan.len())
        .map(|i| {
            let from = *starts.get(i)?;
            let to = match run.plan[i].state {
                PlanState::Done => starts.get(i + 1).copied().unwrap_or(end),
                _ => return None,
            };
            Some(((to - from).max(0) / 60_000) as u64)
        })
        .collect()
}

/// The kind of a 产出 in a meta line (`网页`).
pub fn kind_text(kind: ArtifactKind) -> &'static str {
    match kind {
        ArtifactKind::Page => c::KIND_PAGE,
        ArtifactKind::File => c::KIND_FILE,
        ArtifactKind::Image => c::KIND_IMAGE,
        ArtifactKind::Decision => c::KIND_DECISION,
    }
}

/// The diffstat squares of a change: five, green for added and red for
/// removed lines in proportion, the last one neutral.
pub fn diff_squares(additions: u32, deletions: u32, cx: &App) -> gpui::Div {
    let k = theme::colors(cx);
    let total = (additions + deletions).max(1) as f32;
    let green = if additions + deletions == 0 {
        0
    } else {
        ((additions as f32 / total) * 4.).round() as usize
    };
    let red = if additions + deletions == 0 {
        0
    } else {
        4 - green
    };
    div().flex().gap(px(2.)).children((0..5).map(move |i| {
        let color = if i < green {
            k.ok
        } else if i < green + red {
            k.bad
        } else {
            k.line_2
        };
        div().size(px(7.)).rounded(px(2.)).bg(color)
    }))
}

/// A breathing 6 pt dot in `color` (live work).
pub fn live_dot(id: impl Into<ElementId>, color: Hsla, cx: &App) -> AnyElement {
    ui::motion::breathe(
        id,
        div().flex_none().size(px(6.)).rounded(px(3.)).bg(color),
        cx,
    )
}

impl TopicRail {
    fn open_pane(&self, pane: RightPane, cx: &mut Context<Self>) {
        self.services
            .nav
            .update(cx, |nav, cx| nav.set_right_pane(pane, cx));
    }
}

/// The small real-content thumbnail of a 产出 page: its eyebrow, title, the
/// first metrics and the first rows of its table, on a lifted sheet.
pub fn preview_thumbnail(preview: &ArtifactPreview, cx: &App) -> AnyElement {
    let k = theme::colors(cx);
    let tiny = |px_size: f32, line: f32| div().text_size(px(px_size)).line_height(px(line));
    div()
        .size_full()
        .pt(space::S8)
        .px(space::S10)
        .rounded_t(px(6.))
        .border_1()
        .border_b_0()
        .border_color(k.line)
        .bg(k.bg)
        .shadow(k.lift())
        .overflow_hidden()
        .children(preview.eyebrow.clone().map(|e| {
            tiny(8., 10.)
                .figures()
                .text_color(k.ink_3)
                .one_line()
                .child(e)
        }))
        .child(
            tiny(11., 14.)
                .mt(px(2.))
                .font_weight(gpui::FontWeight::SEMIBOLD)
                .text_color(k.ink)
                .one_line()
                .child(preview.title.clone()),
        )
        .when(!preview.metrics.is_empty(), |el| {
            el.child(
                tiny(8., 10.)
                    .mt(px(4.))
                    .flex()
                    .gap(space::S10)
                    .text_color(k.ink_2)
                    .children(preview.metrics.iter().take(2).map(|m| {
                        // `6/6 场景` for a count, `最长重连 1.8 秒` for a value.
                        let count = m.unit.as_deref().is_some_and(|u| u.starts_with('/'));
                        let value = div()
                            .figures()
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .text_color(k.ink);
                        if count {
                            div()
                                .flex()
                                .gap(px(2.))
                                .child(value.child(format!(
                                    "{}{}",
                                    m.value,
                                    m.unit.as_deref().unwrap_or_default().replace(' ', "")
                                )))
                                .child(m.label.clone())
                        } else {
                            div()
                                .flex()
                                .gap(px(2.))
                                .child(m.label.clone())
                                .child(value.child(m.value.clone()))
                                .children(m.unit.clone())
                        }
                    })),
            )
        })
        .child(
            tiny(8., 12.)
                .mt(px(5.))
                .flex()
                .flex_col()
                .text_color(k.ink_2)
                .children(preview.rows.iter().take(3).map(|r| {
                    div()
                        .flex()
                        .items_center()
                        .gap(space::S4)
                        .border_t_1()
                        .border_color(k.line)
                        .child(div().flex_1().one_line().child(r.label.clone()))
                        .children(r.ok.map(|ok| {
                            div()
                                .text_color(if ok { k.ok } else { k.bad })
                                .child(if ok { "✓" } else { "✕" })
                        }))
                        .child(
                            div()
                                .w(px(16.))
                                .flex()
                                .justify_end()
                                .figures()
                                .child(r.value.clone()),
                        )
                })),
        )
        .into_any_element()
}

/// The recording proof of the 话题's result, when one reported it.
fn topic_recording(services: &Services, g: &str, t: &str) -> Option<u64> {
    let p = &services.provider;
    p.messages(g, t, 50)
        .messages
        .iter()
        .rev()
        .filter(|m| !m.id.is_empty())
        .find_map(|m| {
            p.message_proof(g, &m.id)
                .into_iter()
                .find_map(|proof| match proof {
                    Proof::Recording { duration_ms } => Some(duration_ms),
                    _ => None,
                })
        })
}

impl Render for TopicRail {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let k = ui::native::native_colors(cx);
        let Some((g, t)) = self.services.nav.read(cx).topic.clone() else {
            return rail_empty(cx);
        };
        let d = self.data(&g, &t);
        let topic = d.topic.as_ref();
        let run = d.run.as_ref();
        // NATIVE.md §9.3: 负责, 计划, 改动, 产出, 用到的决定, 在哪干活,
        // 用时和成本; a section without data is left out.
        let mut sections: Vec<AnyElement> = Vec::new();
        if let Some(owner) = topic.and_then(|t| t.owner_id.clone()) {
            let by = topic
                .filter(|x| x.created_by != owner)
                .map(|x| (x.created_by.clone(), x.created_at));
            sections.push(self.native_owner(&d, &owner, run, by, cx));
        }
        if let Some(r) = run.filter(|r| !r.plan.is_empty()) {
            sections.push(self.native_plan(&d, r, cx));
        }
        sections.extend(self.native_changes(&d, window, cx));
        sections.extend(self.native_outputs(&d, window, cx));
        sections.extend(self.native_decisions(&d, cx));
        if let Some(r) = run {
            sections.extend(self.native_where(&d, r, window, cx));
        }
        sections.extend(self.native_cost(&d, cx));
        if sections.is_empty() {
            return rail_empty(cx);
        }
        div()
            .id("topic-rail")
            .size_full()
            .overflow_y_scroll()
            .bg(k.sidebar)
            // Content starts below the 52 pt toolbar row, as on 首页 and 群.
            .pt(crate::ui::native::metrics::TOOLBAR_ROW - gpui::px(12.))
            .pb(space::S16)
            .flex()
            .flex_col()
            .children(sections)
            .into_any_element()
    }
}

/// The 改动 pane (the PR of the 话题). Foundation stub.
pub struct ChangesPane {
    services: Services,
    _subscriptions: Vec<Subscription>,
}

impl ChangesPane {
    pub fn new(services: Services, cx: &mut Context<Self>) -> Self {
        Self {
            _subscriptions: watch(&services, cx),
            services,
        }
    }
}

impl PaneView for ChangesPane {
    fn width(&self, _window_w: Pixels) -> Pixels {
        PANE_WIDE
    }
}

impl Render for ChangesPane {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let changes = self
            .services
            .nav
            .read(cx)
            .topic
            .clone()
            .and_then(|(g, t)| self.services.provider.topic_changes(&g, &t));
        let line = changes.map(|ch| {
            format!(
                "#{} {} · +{} −{}",
                ch.number, ch.title, ch.additions, ch.deletions
            )
        });
        stub_body(Sym::PullRequest, c::TITLE_CHANGES, line, cx)
    }
}

#[cfg(test)]
mod money_tests {
    use super::*;

    /// A cost shows in the currency the data has, never converted.
    #[test]
    fn money_shows_the_reported_currency() {
        assert_eq!(money_text(85, Currency::Usd), "$0.85");
        assert_eq!(money_text(1_234, Currency::Usd), "$12.34");
        assert_eq!(money_text(620, Currency::Cny), "¥6.20");
        assert_eq!(money_text(0, Currency::Usd), "$0.00");
    }
}
