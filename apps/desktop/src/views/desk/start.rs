//! 从这里开始 (usable.zh.md paths 1 and 2): the 首页 checklist of a member no 话题 involves
//! yet. It walks a person from an empty app to the first thing an AI 成员 does for them:
//! 填好资料, a computer to work on, an AI 成员, then 交代第一件事. Each step ticks itself
//! from what the app already knows, the first open step carries the primary button, and the
//! whole list leaves once a 话题 involves me (the old first-run state, so no one who has
//! started ever sees it again).
//!
//! A person who joined a team with an invite code finds the computer and AI 成员 steps
//! already ticked (the team's computer and members are there) and goes straight to the
//! first 话题; one person on their own server is walked through all four.

use gpui::{
    AnyElement, Context, InteractiveElement as _, IntoElement, ParentElement, SharedString, Styled,
    div, prelude::FluentBuilder as _,
};

use super::DeskPage;
use crate::{
    app::NewTopic,
    copy::{desk as c, fill},
    data::{MachineInfo, Member, ThisMac},
    ui::{
        icons,
        native::{
            NativeType as _, PushSize, PushStyle, native_colors, page::m, push_button,
            tokens::space as tsp,
        },
        sf_symbols::{Sym, SymSize, Weight},
    },
};

/// The check glyph of a step.
const STEP_ICON: SymSize = SymSize::new(18., 18., Weight::Regular);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StepKind {
    Profile,
    Computer,
    AiMember,
    FirstTask,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Step {
    pub kind: StepKind,
    pub done: bool,
}

/// What the checklist is decided from, read from the provider (and, for the computers, the
/// last answer of `machines()`).
#[derive(Clone, Debug, Default)]
pub struct Facts {
    /// A 岗位, an avatar or a character is set on me.
    pub profile: bool,
    /// A computer is online for AI 成员 to work on; `None` until the list came back (the step
    /// waits rather than flashing undone).
    pub computer: Option<bool>,
    /// An active AI 成员 is in the team.
    pub ai_member: bool,
    /// Some 话题 involves me: the first thing was handed over.
    pub involved: bool,
}

impl Facts {
    pub fn profile_of(me: &Member) -> bool {
        me.role.as_deref().is_some_and(|r| !r.trim().is_empty())
            || me.avatar_url.is_some()
            || me.character.is_some()
    }

    /// Some computer is online, or this Mac runs the app's own service.
    pub fn computer_of(machines: &[MachineInfo], this_mac: ThisMac) -> bool {
        this_mac == ThisMac::Working || machines.iter().any(|m| m.connected)
    }
}

/// The steps to draw, in order; `None` when the checklist is not shown (a 话题 involves me).
pub fn steps(f: &Facts) -> Option<Vec<Step>> {
    if f.involved {
        return None;
    }
    let mut steps = vec![Step {
        kind: StepKind::Profile,
        done: f.profile,
    }];
    if let Some(done) = f.computer {
        steps.push(Step {
            kind: StepKind::Computer,
            done,
        });
    }
    steps.push(Step {
        kind: StepKind::AiMember,
        done: f.ai_member,
    });
    steps.push(Step {
        kind: StepKind::FirstTask,
        done: false,
    });
    Some(steps)
}

/// The step that carries the primary button: the first open one.
pub fn current(steps: &[Step]) -> Option<StepKind> {
    steps.iter().find(|s| !s.done).map(|s| s.kind)
}

impl DeskPage {
    pub(super) fn start_facts(&self) -> Facts {
        let p = &self.services.provider;
        let me_id = self.services.me();
        let members = p.members();
        let me = me_id
            .as_deref()
            .and_then(|id| members.iter().find(|m| m.id == id));
        let involved = p.groups().iter().any(|g| {
            p.topics(&g.id)
                .iter()
                .any(|t| crate::views::rail::model::involves_me(p.as_ref(), me_id.as_deref(), t))
        });
        Facts {
            profile: me.is_some_and(Facts::profile_of),
            computer: self.computer_ready,
            ai_member: members
                .iter()
                .any(|m| m.is_bot() && !m.deactivated && Some(&m.id) != me_id.as_ref()),
            involved,
        }
    }

    /// 从这里开始: the checklist, or `None` once a 话题 involves me.
    pub(super) fn first_run(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let facts = self.start_facts();
        let steps = steps(&facts)?;
        let now = current(&steps);
        let k = native_colors(cx);
        let done = steps.iter().filter(|s| s.done).count();
        let header = div()
            .flex()
            .items_start()
            .justify_between()
            .gap(tsp::S16)
            .p(m::GUTTER)
            .pb(tsp::S16)
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(tsp::S4)
                    .child(
                        div()
                            .n_title3()
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .text_color(k.label)
                            .child(c::DESK_START_TITLE),
                    )
                    .child(
                        div()
                            .n_body()
                            .text_color(k.secondary)
                            .child(c::DESK_START_BODY),
                    ),
            )
            .child(
                div()
                    .flex_none()
                    .n_footnote()
                    .text_color(k.secondary)
                    .child(fill(
                        c::DESK_START_PROGRESS,
                        &[
                            ("done", &done.to_string()),
                            ("total", &steps.len().to_string()),
                        ],
                    )),
            );
        let rows = steps
            .iter()
            .enumerate()
            .map(|(i, s)| self.step_row(i, *s, now == Some(s.kind), cx));
        Some(
            div()
                .id("desk-start")
                .flex_none()
                .flex()
                .flex_col()
                .rounded(m::FRAME_RADIUS)
                .border_1()
                .border_color(k.separator)
                .child(header)
                .children(rows)
                .into_any_element(),
        )
    }

    fn step_row(&self, i: usize, step: Step, current: bool, cx: &mut Context<Self>) -> AnyElement {
        let k = native_colors(cx);
        let owner = crate::views::settings::i_am_owner(&self.services);
        let (title, line) = match (step.kind, step.done) {
            (StepKind::Profile, false) => (c::DESK_START_PROFILE, c::DESK_START_PROFILE_TODO),
            (StepKind::Profile, true) => (c::DESK_START_PROFILE, c::DESK_START_PROFILE_DONE),
            (StepKind::Computer, false) => (c::DESK_START_COMPUTER, c::DESK_START_COMPUTER_TODO),
            (StepKind::Computer, true) => (c::DESK_START_COMPUTER, c::DESK_START_COMPUTER_DONE),
            (StepKind::AiMember, false) if owner => (c::DESK_START_AI, c::DESK_START_AI_TODO),
            (StepKind::AiMember, false) => (c::DESK_START_AI, c::DESK_START_AI_ASK),
            (StepKind::AiMember, true) => (c::DESK_START_AI, c::DESK_START_AI_DONE),
            (StepKind::FirstTask, _) => (c::DESK_START_TASK, c::DESK_START_TASK_TODO),
        };
        let style = if current {
            PushStyle::Default
        } else {
            PushStyle::Outline
        };
        let actions = if step.done {
            Vec::new()
        } else {
            self.step_actions(step.kind, style, owner, cx)
        };
        let glyph = if step.done {
            icons::sym(Sym::Done, STEP_ICON, k.label)
        } else {
            icons::sym(Sym::Circle, STEP_ICON, k.tertiary)
        };
        div()
            .id(SharedString::from(format!("desk-start-step-{i}")))
            .flex()
            .items_start()
            .gap(tsp::S12)
            .px(m::GUTTER)
            .py(tsp::S16)
            .border_t_1()
            .border_color(k.separator)
            .child(div().flex_none().pt(tsp::S2).child(glyph))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .gap(tsp::S4)
                    .child(
                        div()
                            .n_headline()
                            .text_color(if step.done { k.secondary } else { k.label })
                            .child(title),
                    )
                    .child(div().n_callout().text_color(k.secondary).child(line))
                    .when(step.kind == StepKind::FirstTask, |d| {
                        d.child(
                            div()
                                .n_footnote()
                                .text_color(k.tertiary)
                                .child(c::DESK_START_GROUPS),
                        )
                    }),
            )
            .when(!actions.is_empty(), |d| {
                d.child(
                    div()
                        .flex_none()
                        .flex()
                        .items_center()
                        .gap(tsp::S8)
                        .children(actions),
                )
            })
            .into_any_element()
    }

    fn step_actions(
        &self,
        kind: StepKind,
        style: PushStyle,
        owner: bool,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        let services = self.services.clone();
        match kind {
            StepKind::Profile => vec![
                push_button(
                    "desk-start-profile",
                    c::DESK_START_PROFILE_OPEN,
                    style,
                    PushSize::Small,
                    cx,
                )
                .on_click(move |_, _, cx| {
                    crate::views::settings::open_page(
                        services.clone(),
                        crate::views::settings::Page::Account,
                        cx,
                    )
                })
                .into_any_element(),
            ],
            StepKind::Computer => vec![
                push_button(
                    "desk-start-computer",
                    c::DESK_START_COMPUTER_OPEN,
                    style,
                    PushSize::Small,
                    cx,
                )
                .on_click(move |_, _, cx| {
                    crate::views::settings::open_page(
                        services.clone(),
                        crate::views::settings::Page::Machines,
                        cx,
                    )
                })
                .into_any_element(),
            ],
            StepKind::AiMember if owner => vec![
                push_button(
                    "desk-start-ai",
                    c::DESK_START_AI_OPEN,
                    style,
                    PushSize::Small,
                    cx,
                )
                .on_click(move |_, _, cx| {
                    crate::views::admin::open_at(
                        services.clone(),
                        crate::views::admin::AdminPage::Members,
                        cx,
                    )
                })
                .into_any_element(),
            ],
            StepKind::AiMember => Vec::new(),
            StepKind::FirstTask => self.task_actions(style, cx),
        }
    }

    /// 和 {bot} 聊聊 (the default AI 成员's 私聊) and 开个话题.
    fn task_actions(&self, style: PushStyle, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let p = &self.services.provider;
        let me = self.services.me();
        let bot = p
            .side_chat_default_member()
            .and_then(|id| p.member(&id))
            .filter(|m| Some(&m.id) != me.as_ref());
        let mut actions = Vec::new();
        if let Some(bot) = &bot {
            let id = bot.id.clone();
            actions.push(
                push_button(
                    "desk-start-dm",
                    fill(c::DESK_START_DM, &[("name", &bot.name)]),
                    PushStyle::Outline,
                    PushSize::Small,
                    cx,
                )
                .on_click(move |_, _, cx| crate::views::nav::open_dm_with(id.clone(), cx))
                .into_any_element(),
            );
        }
        actions.push(
            push_button(
                "desk-start-topic",
                c::DESK_NEW_TOPIC,
                style,
                PushSize::Small,
                cx,
            )
            .on_click(|_, window, cx| window.dispatch_action(Box::new(NewTopic), cx))
            .into_any_element(),
        );
        actions
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kinds(s: &[Step]) -> Vec<(StepKind, bool)> {
        s.iter().map(|s| (s.kind, s.done)).collect()
    }

    #[test]
    fn one_person_walks_all_four_steps() {
        let s = steps(&Facts {
            computer: Some(false),
            ..Facts::default()
        })
        .unwrap();
        assert_eq!(
            kinds(&s),
            vec![
                (StepKind::Profile, false),
                (StepKind::Computer, false),
                (StepKind::AiMember, false),
                (StepKind::FirstTask, false),
            ]
        );
        assert_eq!(current(&s), Some(StepKind::Profile));
    }

    #[test]
    fn a_team_member_goes_straight_to_the_first_task() {
        let s = steps(&Facts {
            profile: true,
            computer: Some(true),
            ai_member: true,
            involved: false,
        })
        .unwrap();
        assert_eq!(current(&s), Some(StepKind::FirstTask));
    }

    #[test]
    fn the_computer_step_waits_for_the_list() {
        let s = steps(&Facts::default()).unwrap();
        assert!(!s.iter().any(|s| s.kind == StepKind::Computer));
    }

    #[test]
    fn gone_once_a_topic_involves_me() {
        assert!(
            steps(&Facts {
                involved: true,
                ..Facts::default()
            })
            .is_none()
        );
    }

    #[test]
    fn a_working_service_or_an_online_computer_counts() {
        assert!(Facts::computer_of(&[], ThisMac::Working));
        assert!(!Facts::computer_of(&[], ThisMac::NotSetUp));
    }
}
