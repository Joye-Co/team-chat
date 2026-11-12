//! The two lines of a 话题 about its members' computers (design-takeover 3.5, T6): while
//! someone holds a run, `小王 接管了 Forge`; once it is handed back and the member works on,
//! `小王 交还了 Forge · 12 分钟 · 跑了 3 条命令 · 改了 .env.example`. Drawn from the runs'
//! `holdKind` and `lastHandback`, at the foot of the timeline above the plan cards.

use gpui::{AnyElement, Context, IntoElement, ParentElement, Styled, div};

use super::{TimelineView, v2::GUTTER};
use crate::{
    data::{
        BotRunSummary,
        run_phase::{RunPhase, phase_of},
    },
    theme::space,
    ui::{
        Sym,
        native::{NativeType as _, native_colors},
        sf_symbols::SymSize,
    },
    views::live::phase::{handed_back_line, took_over_line},
};

/// The line a run shows now, when it shows one: (taken over, words).
pub fn hold_line(
    run: &BotRunSummary,
    member: &str,
    me: Option<&str>,
    name: impl Fn(&str) -> String,
) -> Option<(bool, String)> {
    let (phase, _) = phase_of(run);
    match phase {
        RunPhase::TakenOver => {
            let by = run.hold.paused_by.as_deref()?;
            Some((true, took_over_line(by, member, me, &name(by))))
        }
        // After a hand-back, while the member works on; the reply that follows carries on.
        RunPhase::Working
        | RunPhase::WaitingPerson
        | RunPhase::WaitingApproval
        | RunPhase::WaitingComputer => run
            .hold
            .last_handback
            .as_ref()
            .map(|h| (false, handed_back_line(h, member, me, &name(&h.by)))),
        _ => None,
    }
}

impl TimelineView {
    pub(super) fn hold_lines(&self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let name = |id: &str| {
            self.members
                .iter()
                .find(|m| m.id == id)
                .map(|m| crate::ui::avatar::name_beside_avatar(&m.name).to_string())
                .unwrap_or_default()
        };
        let n = native_colors(cx);
        self.runs
            .iter()
            .filter_map(|run| hold_line(run, &name(&run.bot_id), self.me.as_deref(), name))
            .map(|(held, text)| {
                div()
                    .mt(space::S12)
                    .px(space::S8)
                    .flex()
                    .gap(space::S12)
                    .child(div().flex_none().w(GUTTER))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .items_center()
                            .gap(space::S6)
                            .child(
                                if held { Sym::Pointer } else { Sym::Play }
                                    .icon(SymSize::INLINE)
                                    .text_color(n.secondary),
                            )
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .n_footnote()
                                    .text_color(n.secondary)
                                    .child(text),
                            ),
                    )
                    .into_any_element()
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::{
        RunStatus,
        run_phase::{Handback, RunHold},
    };

    fn run(hold: RunHold, status: RunStatus) -> BotRunSummary {
        BotRunSummary {
            id: "r1".into(),
            bot_id: "b1".into(),
            group_id: "g1".into(),
            topic_id: "t1".into(),
            machine_id: None,
            machine_name: None,
            status,
            requested_by: Some("me".into()),
            created_at: 0,
            started_at: None,
            ended_at: None,
            updated_at: 0,
            summary: None,
            plan: Vec::new(),
            hold,
        }
    }

    #[test]
    fn a_take_over_and_a_hand_back_show_once_each() {
        let name = |id: &str| {
            if id == "w" {
                "小王".to_string()
            } else {
                id.into()
            }
        };
        let held = run(
            RunHold {
                paused_by: Some("w".into()),
                hold_kind: Some("takeover".into()),
                ..Default::default()
            },
            RunStatus::Running,
        );
        assert_eq!(
            hold_line(&held, "Forge", Some("me"), name),
            Some((true, "小王 接管了 Forge".into()))
        );
        // A plain pause is no take-over: no line.
        let paused = run(
            RunHold {
                paused_by: Some("w".into()),
                hold_kind: Some("pause".into()),
                ..Default::default()
            },
            RunStatus::Running,
        );
        assert_eq!(hold_line(&paused, "Forge", Some("me"), name), None);
        let back = Handback {
            by: "w".into(),
            held_ms: 5 * 60_000,
            commands: 2,
            ..Default::default()
        };
        let working = run(
            RunHold {
                last_handback: Some(back.clone()),
                ..Default::default()
            },
            RunStatus::Running,
        );
        assert_eq!(
            hold_line(&working, "Forge", Some("me"), name),
            Some((false, "小王 交还了 Forge · 5 分钟 · 跑了 2 条命令".into()))
        );
        // Once the run is over its reply speaks for it.
        let done = run(
            RunHold {
                last_handback: Some(back),
                ..Default::default()
            },
            RunStatus::Done,
        );
        assert_eq!(hold_line(&done, "Forge", Some("me"), name), None);
    }
}
