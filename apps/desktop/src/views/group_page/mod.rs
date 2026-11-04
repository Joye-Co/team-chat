//! 群 page (Group.dc.html): the title with the purpose and member stack,
//! the composer that starts a 话题, 进行中 cards with real previews (the
//! change, the options of an ask, the live terminal, a search), 最近完成,
//! and the 群资料 side column: 群里的决定 (with 记一条), 最近的产出 and 电脑.
//!
//! The page draws its own title bars ([`PageView::draws_own_header`]): the
//! side column has its own header from the top of the window.

pub mod kit;
pub mod rows;
pub mod scenes;

use gpui::{
    AnyElement, App, AppContext as _, Context, Entity, InteractiveElement as _, IntoElement,
    ParentElement, Render, SharedString, StatefulInteractiveElement as _, Styled, Subscription,
    Window, div, prelude::FluentBuilder as _, px,
};
use gpui_component::{
    Disableable as _,
    input::{Input, InputEvent, InputState},
};

use crate::{
    app::{Place, RightPane, Services},
    copy::{fill, group_page as c},
    data::{
        ArtifactKind, ArtifactSummary, BotRunSummary, PlanState, Topic, TopicStatus,
        pages::{ChoiceSketch, TopicGlance, TopicPreview},
    },
    theme::{self, Type as _, space},
    ui::{self, Sym, MemberAvatar, Ring},
    views::{
        composer::ComposerView,
        shell::{PageView, TitleBarSpec, clock, member_name, panes::watch},
    },
};

use kit::{META, SMALL, TINY};

pub struct GroupPage {
    services: Services,
    composer: Entity<ComposerView>,
    aside_open: bool,
    /// 记一条: the open input, when writing.
    note: Option<Entity<InputState>>,
    _note_sub: Option<Subscription>,
    _subscriptions: Vec<Subscription>,
}

impl GroupPage {
    pub fn new(services: Services, composer: Entity<ComposerView>, cx: &mut Context<Self>) -> Self {
        Self {
            _subscriptions: watch(&services, cx),
            services,
            composer,
            aside_open: true,
            note: None,
            _note_sub: None,
        }
    }

    fn group_id(&self, cx: &App) -> Option<String> {
        match &self.services.nav.read(cx).place {
            Some(Place::Group(g)) => Some(g.clone()),
            _ => None,
        }
    }

    fn open_note(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let input =
            cx.new(|cx| InputState::new(window, cx).placeholder(c::GROUP_PAGE_RECORD_PLACEHOLDER));
        let sub = cx.subscribe_in(&input, window, |this, _, e: &InputEvent, _, cx| match e {
            InputEvent::PressEnter { .. } => this.save_note(cx),
            InputEvent::Change => cx.notify(),
            _ => {}
        });
        input.update(cx, |s, cx| s.focus(window, cx));
        self.note = Some(input);
        self._note_sub = Some(sub);
        cx.notify();
    }

    fn close_note(&mut self, cx: &mut Context<Self>) {
        self.note = None;
        self._note_sub = None;
        cx.notify();
    }

    fn save_note(&mut self, cx: &mut Context<Self>) {
        let (Some(g), Some(input)) = (self.group_id(cx), self.note.as_ref()) else {
            return;
        };
        let text = input.read(cx).value().trim().to_string();
        if text.is_empty() {
            return;
        }
        let _ = self.services.provider.record_decision(&g, text);
        self.close_note(cx);
    }
}

impl PageView for GroupPage {
    fn title_bar(&self, cx: &App) -> TitleBarSpec {
        let Some(g) = self.group_id(cx) else {
            return TitleBarSpec::default();
        };
        let p = &self.services.provider;
        let group = p.groups().into_iter().find(|x| x.id == g);
        TitleBarSpec {
            subtitle: p.group_purpose(&g),
            ..TitleBarSpec::page(group.map(|x| x.name).unwrap_or_default())
        }
    }

    fn draws_own_header(&self, _cx: &App) -> bool {
        true
    }
}

/// How a 话题 stands on its card, in sort order.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Standing {
    OnMe,
    Running,
    OnOthers,
    Other,
}

fn standing(t: &Topic, me: Option<&str>, run: Option<&BotRunSummary>) -> Standing {
    if me.is_some_and(|me| t.waiting_on.iter().any(|w| w == me)) {
        Standing::OnMe
    } else if run.is_some() {
        Standing::Running
    } else if !t.waiting_on.is_empty() {
        Standing::OnOthers
    } else {
        Standing::Other
    }
}

pub fn kind_label(kind: ArtifactKind) -> &'static str {
    match kind {
        ArtifactKind::Page => c::GROUP_PAGE_KIND_PAGE,
        ArtifactKind::File => c::GROUP_PAGE_KIND_FILE,
        ArtifactKind::Image => c::GROUP_PAGE_KIND_IMAGE,
        ArtifactKind::Decision => c::GROUP_PAGE_KIND_DECISION,
    }
}

impl GroupPage {
    fn header(&self, g: &str, cx: &App) -> AnyElement {
        let k = theme::colors(cx);
        let p = &self.services.provider;
        let group = p.groups().into_iter().find(|x| x.id == g);
        let member_ids = group
            .as_ref()
            .map(|x| x.member_ids.clone())
            .unwrap_or_default();
        let nav = self.services.nav.clone();
        let members = div()
            .id("group-members")
            .flex_none()
            .flex()
            .items_center()
            .gap(space::S8)
            .h(px(28.))
            .pl(space::S4)
            .pr(space::S8)
            .rounded(px(7.))
            .child(kit::avatar_stack(&self.services, &member_ids, cx))
            .child(div().type_role(META).text_color(k.ink_2).child(fill(
                c::GROUP_PAGE_MEMBERS,
                &[("n", &member_ids.len().to_string())],
            )))
            .on_click(move |_, _, cx| nav.update(cx, |n, cx| n.open_members(None, cx)));
        let nav = self.services.nav.clone();
        let services = self.services.clone();
        kit::bar()
            .gap(px(10.))
            .child(kit::page_title(
                group.as_ref().map(|x| x.name.clone()).unwrap_or_default(),
                cx,
            ))
            .children(p.group_purpose(g).map(|purpose| {
                div()
                    .flex_shrink(1.)
                    .min_w_0()
                    .one_line()
                    .t_small()
                    .text_color(k.ink_3)
                    .child(purpose)
            }))
            .child(div().flex_1())
            .child(members)
            .child(kit::bar_divider(cx))
            .child(
                div()
                    .flex_none()
                    .flex()
                    .items_center()
                    .gap(px(2.))
                    .child(
                        kit::bar_icon(
                            "group-side-chat",
                            Sym::AskAside,
                            c::GROUP_PAGE_SIDE_CHAT,
                            false,
                            cx,
                        )
                        .on_click(move |_, window, cx| {
                            window.dispatch_action(Box::new(crate::views::nav::QuickSwitch), cx)
                        }),
                    )
                    .child(
                        kit::bar_icon(
                            "group-outputs",
                            Sym::Outputs,
                            c::GROUP_PAGE_OUTPUTS,
                            false,
                            cx,
                        )
                        .on_click(move |_, _, cx| nav.update(cx, |n, cx| n.open_artifacts(cx))),
                    )
                    .child(
                        kit::bar_icon(
                            "group-settings",
                            Sym::More,
                            c::GROUP_PAGE_SETTINGS,
                            false,
                            cx,
                        )
                        .on_click(move |_, _, cx| {
                            crate::app::open_settings_window(services.clone(), cx)
                        }),
                    ),
            )
            .into_any_element()
    }

    /// One 进行中 card.
    fn topic_card(&self, i: usize, t: &Topic, cx: &App) -> AnyElement {
        let k = theme::colors(cx);
        let p = &self.services.provider;
        let me = self.services.me();
        let run = p
            .runs(&t.group_id, &t.id)
            .into_iter()
            .find(|r| r.status.is_active());
        let glance = p.topic_glance(&t.group_id, &t.id);
        let st = standing(t, me.as_deref(), run.as_ref());
        let owner_id = run
            .as_ref()
            .map(|r| r.bot_id.clone())
            .or_else(|| t.owner_id.clone())
            .unwrap_or_else(|| t.created_by.clone());
        let owner = p.member(&owner_id);
        let ring = run.as_ref().map(|_| Ring::Live);
        let head = div()
            .flex()
            .items_center()
            .gap(space::S8)
            .type_role(META)
            .text_color(k.ink_3)
            .child(MemberAvatar::new(owner.as_ref(), px(20.)).ring(ring))
            .child(
                div()
                    .text_size(px(13.))
                    .medium()
                    .text_color(k.ink)
                    .child(owner.as_ref().map(|m| m.name.clone()).unwrap_or_default()),
            )
            .children(owner.and_then(|m| m.role).map(ui::JobTag::new))
            .child(div().flex_1())
            .child(div().flex_none().mono().child(clock(t.last_activity_at)));
        let title = div()
            .mt(space::S10)
            .min_w_0()
            .one_line()
            .t_card_title()
            .text_color(k.ink)
            .child(t.title.clone());
        let state = self.state_line(t, st, run.as_ref(), &glance, cx);
        let preview = glance
            .preview
            .as_ref()
            .and_then(|pv| self.preview(&t.id, pv, cx));
        let footer = self.footer(t, run.as_ref(), &glance, cx);
        let nav = self.services.nav.clone();
        let (gid, tid) = (t.group_id.clone(), t.id.clone());
        let live = run
            .as_ref()
            .filter(|r| p.machine_screen(&r.id).is_some())
            .map(|r| r.id.clone());
        let card = ui::card(cx)
            .id(SharedString::from(format!("group-topic-{}", t.id)))
            .px(space::S16)
            .py(space::S14)
            .flex()
            .flex_col()
            .min_w_0()
            .hover(|s| s.border_color(k.line_2))
            .child(head)
            .child(title)
            .children(state)
            .children(preview.map(|el| div().mt(space::S12).child(el)))
            .child(footer)
            .on_click(move |_, _, cx| {
                let (gid, tid, live) = (gid.clone(), tid.clone(), live.clone());
                nav.update(cx, |nav, cx| {
                    nav.open_topic(gid, tid, cx);
                    if let Some(run) = live {
                        nav.set_right_pane(RightPane::Live(run), cx);
                    }
                })
            });
        ui::motion::rise(
            SharedString::from(format!("group-card-{}", t.id)),
            i + 1,
            card,
            cx,
        )
    }

    fn state_line(
        &self,
        t: &Topic,
        st: Standing,
        run: Option<&BotRunSummary>,
        glance: &TopicGlance,
        cx: &App,
    ) -> Option<AnyElement> {
        let k = theme::colors(cx);
        let p = &self.services.provider;
        let row = || {
            div()
                .mt(space::S2)
                .flex()
                .items_center()
                .gap(space::S6)
                .min_w_0()
                .t_small()
        };
        let note = |el: gpui::Div| {
            el.children(glance.state_note.clone().map(|n| {
                div()
                    .min_w_0()
                    .one_line()
                    .text_color(k.ink_3)
                    .child(format!("· {n}"))
            }))
        };
        Some(match st {
            Standing::OnMe => {
                let label = match t.status {
                    TopicStatus::WaitingReview => p
                        .topic_changes(&t.group_id, &t.id)
                        .map(|ch| {
                            fill(
                                c::GROUP_PAGE_NEEDS_REVIEW_PR,
                                &[("n", &ch.number.to_string())],
                            )
                        })
                        .unwrap_or_else(|| c::GROUP_PAGE_NEEDS_REVIEW.to_string()),
                    TopicStatus::WaitingAnswer => c::GROUP_PAGE_NEEDS_DECISION.to_string(),
                    _ => c::GROUP_PAGE_NEEDS_ANSWER.to_string(),
                };
                note(
                    row().child(kit::dot(k.you, 6.)).child(
                        div()
                            .flex_none()
                            .medium()
                            .text_color(k.you_ink)
                            .child(label),
                    ),
                )
                .into_any_element()
            }
            Standing::Running => {
                let run = run?;
                let progress = kit::plan_progress(&run.plan);
                let step = run.plan.iter().position(|s| s.state == PlanState::Current);
                let since = step
                    .and_then(|i| p.run_step_times(&run.id).get(i).copied())
                    .or(run.started_at)
                    .unwrap_or(run.created_at);
                let text = match step.map(|i| run.plan[i].text.clone()) {
                    Some(s) => fill(
                        c::GROUP_PAGE_ELAPSED,
                        &[("step", &s), ("time", &kit::elapsed(p.clock_now(), since))],
                    ),
                    None => kit::elapsed(p.clock_now(), since),
                };
                row()
                    .children(progress.map(|(n, m)| {
                        div()
                            .flex_none()
                            .mono()
                            .type_role(META)
                            .text_color(k.ink_3)
                            .child(format!("{n}/{m}"))
                    }))
                    .child(div().min_w_0().one_line().text_color(k.ink_2).child(text))
                    .into_any_element()
            }
            Standing::OnOthers => {
                let who = t
                    .waiting_on
                    .iter()
                    .map(|w| member_name(&self.services, w))
                    .collect::<Vec<_>>()
                    .join("、");
                note(
                    row().child(kit::ring_dot(k.ink_3)).child(
                        div()
                            .flex_none()
                            .text_color(k.ink_2)
                            .child(fill(c::GROUP_PAGE_WAITING_ON, &[("name", &who)])),
                    ),
                )
                .into_any_element()
            }
            Standing::Other => return None,
        })
    }

    fn preview(&self, id: &str, pv: &TopicPreview, cx: &App) -> Option<AnyElement> {
        let k = theme::colors(cx);
        let sunken = || {
            div()
                .h(kit::PREVIEW_HEIGHT)
                .rounded(theme::radius::MD)
                .bg(k.sunken)
                .px(space::S10)
                .py(space::S8)
                .overflow_hidden()
        };
        Some(match pv {
            TopicPreview::Diff {
                path,
                additions,
                deletions,
                lines,
            } => sunken()
                .flex()
                .flex_col()
                .mono()
                .text_size(px(11.))
                .line_height(px(18.))
                .text_color(k.ink_2)
                .child(
                    div()
                        .flex()
                        .text_color(k.ink_3)
                        .child(div().flex_1().min_w_0().one_line().child(path.clone()))
                        .child(kit::line_counts(*additions, *deletions, cx)),
                )
                .children(lines.iter().map(|l| {
                    div()
                        .flex()
                        .whitespace_nowrap()
                        .child(
                            div()
                                .flex_none()
                                .w(px(14.))
                                .text_color(if l.added { k.ok_ink } else { k.bad_ink })
                                .child(if l.added { "+" } else { "−" }),
                        )
                        .child(l.text.clone())
                }))
                .into_any_element(),
            TopicPreview::Choice { options } => div()
                .h(kit::PREVIEW_HEIGHT)
                .grid()
                .grid_cols(options.len().clamp(1, 3) as u16)
                .gap(space::S8)
                .children(options.iter().map(|o| {
                    let sketch = o.sketch.as_ref().map(|s| match s {
                        ChoiceSketch::Field(text) => div()
                            .h(px(26.))
                            .flex()
                            .items_center()
                            .px(space::S8)
                            .rounded(px(6.))
                            .border_1()
                            .border_color(k.line_2)
                            .bg(k.bg)
                            .mono()
                            .text_size(px(11.))
                            .text_color(k.ink)
                            .child(text.clone())
                            .into_any_element(),
                        ChoiceSketch::Cells { total, filled } => {
                            let chars: Vec<char> = filled.chars().collect();
                            div()
                                .h(px(26.))
                                .flex()
                                .gap(px(3.))
                                .children((0..*total as usize).map(|i| {
                                    div()
                                        .flex_1()
                                        .min_w_0()
                                        .flex()
                                        .items_center()
                                        .justify_center()
                                        .rounded(px(4.))
                                        .border_1()
                                        .border_color(if i == chars.len() {
                                            k.ink_3
                                        } else {
                                            k.line_2
                                        })
                                        .bg(k.bg)
                                        .mono()
                                        .text_size(px(10.))
                                        .text_color(k.ink)
                                        .children(chars.get(i).map(|c| c.to_string()))
                                }))
                                .into_any_element()
                        }
                        ChoiceSketch::Text(text) => div()
                            .type_role(TINY)
                            .text_color(k.ink_2)
                            .line_clamp(2)
                            .child(text.clone())
                            .into_any_element(),
                    });
                    div()
                        .min_w_0()
                        .flex()
                        .flex_col()
                        .gap(space::S6)
                        .px(space::S10)
                        .py(space::S8)
                        .rounded(theme::radius::MD)
                        .bg(k.sunken)
                        .overflow_hidden()
                        .child(
                            div()
                                .flex()
                                .gap(space::S4)
                                .type_role(TINY)
                                .text_color(k.ink_3)
                                .child(div().mono().text_color(k.ink_2).child(o.key.clone()))
                                .child(div().min_w_0().one_line().child(o.label.clone())),
                        )
                        .children(sketch)
                }))
                .into_any_element(),
            TopicPreview::Screen { run_id } => {
                kit::mini_screen(&self.services, id, run_id, kit::PREVIEW_HEIGHT, false, cx)?
                    .into_any_element()
            }
            TopicPreview::Matches {
                query,
                count,
                lines,
            } => sunken()
                .flex()
                .flex_col()
                .gap(space::S4)
                .type_role(META)
                .text_color(k.ink_2)
                .child(
                    div()
                        .h(px(20.))
                        .flex()
                        .items_center()
                        .gap(space::S6)
                        .px(space::S6)
                        .rounded(px(5.))
                        .border_1()
                        .border_color(k.line_2)
                        .bg(k.bg)
                        .text_color(k.ink)
                        .child(ui::icon(Sym::Search, px(11.), k.ink_3))
                        .child(query.clone())
                        .child(div().flex_1())
                        .child(
                            div()
                                .mono()
                                .text_size(px(10.))
                                .text_color(k.ink_3)
                                .child(fill(c::GROUP_PAGE_MATCHES, &[("n", &count.to_string())])),
                        ),
                )
                .children(lines.iter().map(|l| marked(l, query, cx)))
                .into_any_element(),
        })
    }

    fn footer(
        &self,
        t: &Topic,
        run: Option<&BotRunSummary>,
        glance: &TopicGlance,
        cx: &App,
    ) -> AnyElement {
        let k = theme::colors(cx);
        let p = &self.services.provider;
        let changes = p.topic_changes(&t.group_id, &t.id);
        let outputs = p
            .group_artifacts(&t.group_id)
            .into_iter()
            .filter(|a| a.topic_id == t.id)
            .count();
        let trailing: Option<AnyElement> = if let Some(ch) = &changes {
            Some(kit::line_counts(ch.additions, ch.deletions, cx).into_any_element())
        } else if let Some(run) = run.filter(|r| !r.plan.is_empty()) {
            Some(
                div()
                    .w(px(92.))
                    .child(kit::step_bar(&t.id, &run.plan, px(3.), cx))
                    .into_any_element(),
            )
        } else if outputs > 0 {
            Some(
                div()
                    .flex()
                    .items_center()
                    .gap(space::S4)
                    .child(ui::icon(Sym::Outputs, px(12.), k.ink_3))
                    .child(fill(
                        c::GROUP_PAGE_OUTPUTS_N,
                        &[("n", &outputs.to_string())],
                    ))
                    .into_any_element(),
            )
        } else {
            None
        };
        div()
            .mt_auto()
            .pt(space::S12)
            .flex()
            .items_center()
            .gap(space::S12)
            .type_role(META)
            .text_color(k.ink_3)
            .child(div().flex_none().child(fill(
                c::GROUP_PAGE_ASKED_BY,
                &[("name", &member_name(&self.services, &t.created_by))],
            )))
            .children(glance.tests_passed.map(|n| {
                div()
                    .flex_none()
                    .flex()
                    .items_center()
                    .gap(space::S4)
                    .child(ui::icon(Sym::Check, px(12.), k.ok))
                    .child(fill(c::GROUP_PAGE_TESTS_PASSED, &[("n", &n.to_string())]))
            }))
            .children(
                glance
                    .footer_note
                    .clone()
                    .map(|n| div().min_w_0().one_line().child(n)),
            )
            .child(div().flex_1())
            .children(trailing.map(|el| div().flex_none().child(el)))
            .into_any_element()
    }

    fn done_rows(&self, g: &str, cx: &App) -> Vec<AnyElement> {
        let k = theme::colors(cx);
        let p = &self.services.provider;
        p.group_recent_done(g)
            .into_iter()
            .take(5)
            .enumerate()
            .map(|(i, d)| {
                let member = p.member(&d.member_id);
                let nav = self.services.nav.clone();
                let (gid, tid) = (d.group_id.clone(), d.topic_id.clone());
                let row = div()
                    .id(SharedString::from(format!("group-done-{i}")))
                    .flex()
                    .items_center()
                    .gap(space::S12)
                    .h(px(44.))
                    .px(space::S12)
                    .mx(-space::S12)
                    .rounded(px(10.))
                    .child(
                        div()
                            .flex_none()
                            .w(px(16.))
                            .flex()
                            .justify_center()
                            .child(ui::icon(Sym::Check, px(14.), k.ok)),
                    )
                    .child(
                        div()
                            .flex_none()
                            .max_w(px(420.))
                            .one_line()
                            .t_ui()
                            .medium()
                            .text_color(k.ink)
                            .child(d.title.clone()),
                    )
                    .children(d.note.clone().map(|n| {
                        div()
                            .flex_1()
                            .min_w_0()
                            .one_line()
                            .t_small()
                            .text_color(k.ink_3)
                            .child(n)
                    }))
                    .when(d.note.is_none(), |el| el.child(div().flex_1()))
                    .child(MemberAvatar::new(member.as_ref(), px(18.)))
                    .child(
                        div()
                            .flex_none()
                            .w(px(40.))
                            .flex()
                            .justify_end()
                            .mono()
                            .type_role(META)
                            .text_color(k.ink_3)
                            .child(kit::month_day(d.at)),
                    )
                    .on_click(move |_, _, cx| {
                        let (gid, tid) = (gid.clone(), tid.clone());
                        nav.update(cx, |nav, cx| nav.open_topic(gid, tid, cx))
                    });
                ui::motion::rise(
                    SharedString::from(format!("group-done-rise-{i}")),
                    i + 5,
                    row,
                    cx,
                )
            })
            .collect()
    }

    fn decisions_card(&self, g: &str, cx: &mut Context<Self>) -> AnyElement {
        let k = theme::colors(cx);
        let p = self.services.provider.clone();
        let decisions = p.group_decisions(g);
        let writing = self.note.clone();
        let header = kit::card_label(c::GROUP_PAGE_DECISIONS, Some(decisions.len()), cx)
            .child(div().flex_1())
            .when(writing.is_none() && p.can_record_decision(g), |el| {
                el.child(
                    ui::button(
                        "group-record",
                        c::GROUP_PAGE_RECORD,
                        ui::ButtonKind::Secondary,
                        ui::ButtonSize::Sm,
                        cx,
                    )
                    .h(px(24.))
                    .px(space::S8)
                    .icon(gpui_component::Icon::new(Sym::Plus))
                    .on_click(cx.listener(|this, _, window, cx| this.open_note(window, cx))),
                )
            });
        let input = writing.map(|input| {
            let empty = input.read(cx).value().trim().is_empty();
            div()
                .mt(space::S10)
                .p(space::S8)
                .rounded(px(9.))
                .border_1()
                .border_color(k.ink_3)
                .bg(k.bg)
                .flex()
                .flex_col()
                .gap(space::S6)
                .child(Input::new(&input).appearance(false).h(px(24.)))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(space::S6)
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .one_line()
                                .type_role(META)
                                .text_color(k.ink_3)
                                .child(c::GROUP_PAGE_RECORD_HINT),
                        )
                        .child(
                            ui::button(
                                "group-record-cancel",
                                c::GROUP_PAGE_CANCEL,
                                ui::ButtonKind::Ghost,
                                ui::ButtonSize::Sm,
                                cx,
                            )
                            .h(px(26.))
                            .on_click(cx.listener(|this, _, _, cx| this.close_note(cx))),
                        )
                        .child(
                            ui::button(
                                "group-record-save",
                                c::GROUP_PAGE_SAVE,
                                ui::ButtonKind::Primary,
                                ui::ButtonSize::Sm,
                                cx,
                            )
                            .h(px(26.))
                            .disabled(empty)
                            .on_click(cx.listener(|this, _, _, cx| this.save_note(cx))),
                        ),
                )
        });
        let now = p.clock_now();
        let rows = decisions.into_iter().enumerate().map(|(i, d)| {
            let member = p.member(&d.by);
            let date = if now - d.at < 60_000 {
                c::GROUP_PAGE_JUST_NOW.to_string()
            } else {
                kit::month_day(d.at)
            };
            div()
                .py(space::S10)
                .border_t_1()
                .border_color(k.line)
                .when(i == 0, |el| el.mt(space::S10))
                .child(div().t_small().text_color(k.ink).child(d.text.clone()))
                .child(
                    div()
                        .mt(space::S4)
                        .flex()
                        .items_center()
                        .gap(space::S6)
                        .type_role(META)
                        .text_color(k.ink_3)
                        .child(MemberAvatar::new(member.as_ref(), px(16.)))
                        .child(
                            div()
                                .text_color(k.ink_2)
                                .child(member.map(|m| m.name).unwrap_or_default()),
                        )
                        .child(div().flex_1())
                        .child(div().mono().child(date)),
                )
        });
        kit::side_card(cx)
            .child(header)
            .child(
                div()
                    .mt(space::S2)
                    .type_role(META)
                    .text_color(k.ink_3)
                    .child(c::GROUP_PAGE_DECISIONS_NOTE),
            )
            .children(input)
            .children(rows)
            .into_any_element()
    }

    fn outputs_card(&self, g: &str, cx: &App) -> Option<AnyElement> {
        let k = theme::colors(cx);
        let p = &self.services.provider;
        let outputs: Vec<ArtifactSummary> = p.group_artifacts(g).into_iter().take(3).collect();
        if outputs.is_empty() {
            return None;
        }
        let nav = self.services.nav.clone();
        let rows =
            outputs.into_iter().map(|a| {
                let nav = nav.clone();
                let id = a.id.clone();
                div()
                    .id(SharedString::from(format!("group-output-{}", a.id)))
                    .flex()
                    .items_center()
                    .gap(space::S10)
                    .p(space::S6)
                    .mx(-space::S6)
                    .rounded(px(9.))
                    .child(kit::artifact_thumb(
                        &self.services,
                        &a,
                        px(64.),
                        px(48.),
                        cx,
                    ))
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
                                    .child(a.title.clone()),
                            )
                            .child(div().one_line().type_role(META).text_color(k.ink_3).child(
                                fill(
                                    c::GROUP_PAGE_ARTIFACT_META,
                                    &[
                                        ("v", &a.version.to_string()),
                                        ("kind", kind_label(a.kind)),
                                        ("name", &member_name(&self.services, &a.created_by)),
                                    ],
                                ),
                            )),
                    )
                    .child(
                        div()
                            .flex_none()
                            .self_start()
                            .mono()
                            .type_role(SMALL)
                            .text_size(px(12.))
                            .text_color(k.ink_3)
                            .child(short_time(p.clock_now(), a.updated_at)),
                    )
                    .on_click(move |_, _, cx| {
                        let id = id.clone();
                        nav.update(cx, |nav, cx| nav.open_artifact(id, None, cx))
                    })
            });
        let nav = self.services.nav.clone();
        Some(
            kit::side_card(cx)
                .child(
                    kit::card_label(c::GROUP_PAGE_RECENT_OUTPUTS, None, cx)
                        .child(div().flex_1())
                        .child(
                            kit::text_link("group-outputs-all", c::GROUP_PAGE_ALL, cx).on_click(
                                move |_, _, cx| nav.update(cx, |n, cx| n.open_artifacts(cx)),
                            ),
                        ),
                )
                .child(
                    div()
                        .mt(space::S6)
                        .flex()
                        .flex_col()
                        .gap(space::S2)
                        .children(rows),
                )
                .into_any_element(),
        )
    }

    /// 电脑: the machine a member of this 群 uses now.
    fn machine_card(&self, g: &str, cx: &App) -> Option<AnyElement> {
        let k = theme::colors(cx);
        let p = &self.services.provider;
        let run = p
            .topics(g)
            .into_iter()
            .flat_map(|t| p.runs(g, &t.id))
            .filter(|r| r.status.is_active() && r.machine_name.is_some())
            .max_by_key(|r| r.updated_at)?;
        let who = member_name(&self.services, &run.bot_id);
        let step = run
            .plan
            .iter()
            .find(|s| s.state == PlanState::Current)
            .map(|s| format!("{who} {}", s.text))
            .unwrap_or_else(|| who.clone());
        let nav = self.services.nav.clone();
        let (gid, tid, rid) = (run.group_id.clone(), run.topic_id.clone(), run.id.clone());
        Some(
            kit::side_card(cx)
                .child(kit::card_label(c::GROUP_PAGE_COMPUTER, None, cx))
                .child(
                    div()
                        .mt(space::S6)
                        .flex()
                        .items_center()
                        .gap(space::S10)
                        .child(
                            div()
                                .flex_none()
                                .size(px(32.))
                                .rounded(theme::radius::MD)
                                .bg(k.sunken)
                                .flex()
                                .items_center()
                                .justify_center()
                                .child(ui::icon(Sym::Computer, theme::icon::ROW, k.live)),
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
                                        .items_center()
                                        .gap(space::S6)
                                        .min_w_0()
                                        .t_small()
                                        .child(
                                            div()
                                                .min_w_0()
                                                .one_line()
                                                .medium()
                                                .text_color(k.ink)
                                                .child(
                                                    run.machine_name.clone().unwrap_or_default(),
                                                ),
                                        )
                                        .child(
                                            div()
                                                .flex()
                                                .items_center()
                                                .gap(px(5.))
                                                .type_role(META)
                                                .text_color(k.live_ink)
                                                .child(kit::live_dot("group-machine-live", 6., cx))
                                                .child(c::GROUP_PAGE_IN_USE),
                                        ),
                                )
                                .child(
                                    div()
                                        .one_line()
                                        .type_role(META)
                                        .text_color(k.ink_3)
                                        .child(step),
                                ),
                        ),
                )
                .child(
                    ui::button(
                        "group-machine-open",
                        fill(c::GROUP_PAGE_SEE_COMPUTER, &[("name", &who)]),
                        ui::ButtonKind::Secondary,
                        ui::ButtonSize::Sm,
                        cx,
                    )
                    .mt(space::S10)
                    .w_full()
                    .text_size(px(12.))
                    .child(ui::icon(Sym::ArrowRight, px(13.), k.ink))
                    .on_click(move |_, _, cx| {
                        let (gid, tid, rid) = (gid.clone(), tid.clone(), rid.clone());
                        nav.update(cx, |nav, cx| {
                            nav.open_topic(gid, tid, cx);
                            nav.set_right_pane(RightPane::Live(rid), cx);
                        })
                    }),
                )
                .into_any_element(),
        )
    }
}

/// `09:55` today, else `11-02`.
pub fn short_time(now: crate::data::Millis, at: crate::data::Millis) -> String {
    use chrono::{Local, TimeZone as _};
    let same_day = match (
        Local.timestamp_millis_opt(now).single(),
        Local.timestamp_millis_opt(at).single(),
    ) {
        (Some(a), Some(b)) => a.date_naive() == b.date_naive(),
        _ => false,
    };
    if same_day {
        clock(at)
    } else {
        kit::month_day(at)
    }
}

/// A line with every `query` in it marked (`sel` fill, 600).
fn marked(line: &str, query: &str, cx: &App) -> AnyElement {
    let k = theme::colors(cx);
    let mut row = div().flex().one_line().whitespace_nowrap();
    if query.is_empty() {
        return row.child(line.to_string()).into_any_element();
    }
    for (i, part) in line.split(query).enumerate() {
        if i > 0 {
            row = row.child(
                div()
                    .rounded(px(2.))
                    .bg(k.sel)
                    .semibold()
                    .text_color(k.ink)
                    .child(query.to_string()),
            );
        }
        if !part.is_empty() {
            row = row.child(part.to_string());
        }
    }
    row.into_any_element()
}

impl Render for GroupPage {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let k = theme::colors(cx);
        let Some(g) = self.group_id(cx) else {
            return div().into_any_element();
        };
        let p = self.services.provider.clone();
        let me = self.services.me();
        let mut active: Vec<(Standing, Topic)> = p
            .topics(&g)
            .into_iter()
            .filter(|t| t.status != TopicStatus::Done)
            .map(|t| {
                let run = p.runs(&g, &t.id).into_iter().find(|r| r.status.is_active());
                (standing(&t, me.as_deref(), run.as_ref()), t)
            })
            .collect();
        active.sort_by(|a, b| {
            a.0.cmp(&b.0)
                .then(b.1.last_activity_at.cmp(&a.1.last_activity_at))
        });
        let on_me = active.iter().filter(|(s, _)| *s == Standing::OnMe).count();
        let leading = kit::leading(&self.services, cx);
        let window_w = window.viewport_size().width;
        let rail_w = if self.services.nav.read(cx).sidebar_hidden {
            px(0.)
        } else {
            theme::RAIL_WIDTH
        };
        let aside_w = if self.aside_open {
            kit::ASIDE_WIDTH
        } else {
            px(0.)
        };
        let main_w = (window_w - rail_w - aside_w - px(80.)).min(kit::MEASURE);
        let cols = ((f32::from(main_w) + 12.) / 312.).floor().clamp(1., 3.) as u16;
        let cards: Vec<AnyElement> = active
            .iter()
            .enumerate()
            .map(|(i, (_, t))| self.topic_card(i, t, cx))
            .collect();
        let done = self.done_rows(&g, cx);
        let main = div()
            .id("group-main")
            .flex_1()
            .min_w(px(0.))
            .h_full()
            .overflow_y_scroll()
            .pl(px(40.).max(leading))
            .pr(px(40.))
            .pb(space::S24)
            .child(
                div()
                    .max_w(kit::MEASURE)
                    .mx_auto()
                    .flex()
                    .flex_col()
                    .child(self.header(&g, cx))
                    .child(ui::motion::rise(
                        "group-composer",
                        0,
                        // The shared composer in its v2 style has no gutter of
                        // its own: it lines up with the cards (Group.dc.html).
                        div().mt(space::S8).child(self.composer.clone()),
                        cx,
                    ))
                    .when(!cards.is_empty(), |el| {
                        el.child(
                            kit::section_head(c::GROUP_PAGE_ACTIVE, Some(active.len()), cx)
                                .mt(space::S24)
                                .when(on_me > 0, |el| {
                                    el.child(
                                        div()
                                            .ml(space::S4)
                                            .flex()
                                            .items_center()
                                            .gap(space::S6)
                                            .type_role(META)
                                            .text_color(k.you_ink)
                                            .child(kit::dot(k.you, 6.))
                                            .child(fill(
                                                c::GROUP_PAGE_WAITING_YOU,
                                                &[("n", &on_me.to_string())],
                                            )),
                                    )
                                }),
                        )
                        .child(
                            div()
                                .mt(space::S10)
                                .grid()
                                .grid_cols(cols)
                                .gap(space::S12)
                                .children(cards),
                        )
                    })
                    .when(!done.is_empty(), |el| {
                        let nav = self.services.nav.clone();
                        el.child(
                            kit::section_head(c::GROUP_PAGE_RECENT_DONE, None, cx)
                                .mt(px(26.))
                                .child(
                                    div()
                                        .type_role(META)
                                        .text_color(k.ink_3)
                                        .child(c::GROUP_PAGE_THIS_WEEK),
                                )
                                .child(div().flex_1())
                                .child(
                                    kit::text_link(
                                        "group-all-topics",
                                        c::GROUP_PAGE_ALL_TOPICS,
                                        cx,
                                    )
                                    .on_click(
                                        move |_, window, cx| {
                                            let _ = &nav;
                                            window.dispatch_action(
                                                Box::new(crate::views::nav::QuickSwitch),
                                                cx,
                                            )
                                        },
                                    ),
                                ),
                        )
                        .child(div().mt(space::S6).flex().flex_col().children(done))
                    }),
            );
        let aside = if self.aside_open {
            let this = cx.entity().downgrade();
            let decisions = self.decisions_card(&g, cx);
            let outputs = self.outputs_card(&g, cx);
            let machine = self.machine_card(&g, cx);
            let cards: Vec<AnyElement> = std::iter::once(decisions)
                .chain(outputs)
                .chain(machine)
                .enumerate()
                .map(|(i, el)| {
                    ui::motion::rise(
                        SharedString::from(format!("group-side-{i}")),
                        i + 1,
                        div().child(el),
                        cx,
                    )
                })
                .collect();
            Some(
                kit::aside(
                    "group-aside",
                    c::GROUP_PAGE_INFO,
                    kit::ASIDE_WIDTH,
                    space::S16,
                    Some(Box::new(move |_, cx| {
                        let _ = this.update(cx, |this, cx| {
                            this.aside_open = false;
                            cx.notify();
                        });
                    })),
                    cx,
                )
                .child(ui::rail_stack(cards, cx))
                .into_any_element(),
            )
        } else {
            None
        };
        let reopen = (!self.aside_open).then(|| {
            div().absolute().top(space::S12).right(space::S12).child(
                kit::bar_icon(
                    "group-aside-open",
                    Sym::Inspector,
                    c::GROUP_PAGE_EXPAND,
                    false,
                    cx,
                )
                .on_click(cx.listener(|this, _, _, cx| {
                    this.aside_open = true;
                    cx.notify();
                })),
            )
        });
        div()
            .size_full()
            .relative()
            .flex()
            .bg(k.bg)
            .child(main)
            .children(aside)
            .children(reopen)
            .into_any_element()
    }
}
