//! 常用设置 of an AI member: the sheet that changes what owners change most
//! often, which computer it works on (one of the team's, or any free one),
//! from wherever the member shows up in daily use: a right-click on its avatar
//! in a message, its page and ⋯ in 团队管理. A pick saves at once; 更多设置…
//! opens the full edit sheet. Models and 思考强度 are set once for the team,
//! so they are not here.

use gpui::{
    App, AppContext as _, Context, IntoElement, ParentElement, Render, Styled as _, Window, div,
    prelude::FluentBuilder as _, px,
};
use gpui_component::{
    WindowExt as _,
    menu::{DropdownMenu as _, PopupMenuItem},
};

use super::{
    i_am_owner,
    members::{PlaceView, machine_choice_text, open_edit_bot, placement_options},
    widgets::{self, Status, answer},
};
use crate::{
    app::Services,
    copy::{fill, settings as s},
    data::{BotMemberEdit, MachineInfo, Member, RunnerChange, RunnerSetup},
    ui::{
        avatar::name_beside_avatar,
        native::{PushSize, PushStyle, SheetLayout, SheetWidth, push_button, tokens},
    },
    views::nav::dialogs::{sheet_form, sheet_form_row},
};

/// The member's runner and the team's computers as they load.
enum Load {
    Loading,
    Failed(gpui::SharedString),
    /// `None`: the member is joined by an outside program, not run on a computer.
    Loaded(Option<RunnerSetup>),
}

pub struct BotQuick {
    services: Services,
    member: Member,
    /// The team's computers, in with the runner.
    machines: Vec<MachineInfo>,
    this_machine: Option<String>,
    runner: Load,
    status: Status,
}

/// Whether the 常用设置 entry shows for `member`: an active AI member, and I am an owner
/// (the server lets only owners change a runner).
pub fn can_open(services: &Services, member: &Member) -> bool {
    shows_for(member, i_am_owner(services))
}

fn shows_for(member: &Member, owner: bool) -> bool {
    owner && member.is_bot() && !member.deactivated
}

/// The choices of 在哪台电脑干活 (as in the edit sheet: [`placement_options`]).
fn machine_options(
    picked: Option<&str>,
    machines: &[MachineInfo],
    view: PlaceView,
) -> Vec<(Option<String>, String)> {
    placement_options(picked, machines, view)
}

/// The runner with computer `id` picked; `None` when nothing changes. A picked computer
/// (or 任意) replaces the old hand-typed labels, as in the edit sheet.
fn with_machine(r: &RunnerSetup, id: Option<String>) -> Option<RunnerSetup> {
    let mut after = r.clone();
    after.machine_id = id;
    after.labels.clear();
    (after != *r).then_some(after)
}

impl BotQuick {
    fn new(services: Services, member: Member, cx: &mut Context<Self>) -> Self {
        let this_machine = services.provider.this_machine_id();
        let mut quick = Self {
            services,
            member,
            machines: Vec::new(),
            this_machine,
            runner: Load::Loading,
            status: Status::Idle,
        };
        quick.load(cx);
        quick
    }

    /// Loads the runner and the computers together, so the picked computer never shows as
    /// removed while the list is still on its way.
    fn load(&mut self, cx: &mut Context<Self>) {
        self.runner = Load::Loading;
        let runner = self.services.provider.bot_runner(&self.member.id);
        let machines = self.services.provider.machines();
        cx.notify();
        cx.spawn(async move |this, cx| {
            let runner = answer(runner).await;
            let machines = answer(machines).await;
            let _ = this.update(cx, |this, cx| {
                this.runner = match (runner, machines) {
                    (Ok(r), Ok(list)) => {
                        this.machines = list;
                        Load::Loaded(r)
                    }
                    (Err(e), _) | (_, Err(e)) => Load::Failed(widgets::error_text(e).into()),
                };
                cx.notify();
            });
        })
        .detach();
    }

    /// Saves computer `id`; the sheet shows it at once and goes back to the old one when
    /// the save fails.
    fn pick(&mut self, id: Option<String>, cx: &mut Context<Self>) {
        if self.status.busy() {
            return;
        }
        let Load::Loaded(Some(before)) = &self.runner else {
            return;
        };
        let before = before.clone();
        let Some(after) = with_machine(&before, id) else {
            return;
        };
        let edit = BotMemberEdit {
            name: None,
            role: None,
            runner: Some(RunnerChange::Set(after.clone())),
        };
        let reply = self
            .services
            .provider
            .update_bot_member(&self.member.id, edit);
        self.runner = Load::Loaded(Some(after));
        self.status = Status::Busy(s::SAVING.into());
        cx.notify();
        cx.spawn(async move |this, cx| {
            let result = answer(reply).await;
            let _ = this.update(cx, |this, cx| {
                this.status = match result {
                    Ok(()) => Status::Done(s::QUICK_SAVED.into()),
                    Err(e) => {
                        this.runner = Load::Loaded(Some(before));
                        Status::from_error(e)
                    }
                };
                cx.notify();
            });
        })
        .detach();
    }

    fn fields(&self, r: &RunnerSetup, window: &Window, cx: &mut Context<Self>) -> gpui::Div {
        let me = super::my_id(&self.services);
        let view = PlaceView::of(&self.services, self.this_machine.as_deref(), me.as_deref());
        let picked = r.machine_id.clone();
        // On somebody else's own computer the choice is theirs: shown, not offered.
        if super::members::pinned_elsewhere(picked.as_deref(), &self.machines, me.as_deref())
            .is_some()
        {
            let hint =
                super::members::placement_hint(picked.as_deref(), &self.machines, view, |id| {
                    self.services
                        .provider
                        .member(id)
                        .map(|m| name_beside_avatar(&m.name).to_string())
                });
            return div().w_full().child(
                sheet_form().child(sheet_form_row(
                    s::RUNNER_MACHINE,
                    div()
                        .flex()
                        .flex_col()
                        .gap(tokens::space::S4)
                        .child(widgets::row_value(
                            machine_choice_text(picked.as_deref(), &self.machines, view),
                            cx,
                        ))
                        .children(hint.map(|h| widgets::quiet(h, cx))),
                    cx,
                )),
            );
        }
        let options = machine_options(picked.as_deref(), &self.machines, view);
        let shown = machine_choice_text(picked.as_deref(), &self.machines, view);
        let view = cx.entity();
        let select = widgets::popup_fixed(
            "quick-machine",
            shown,
            tokens::size::SELECT_WIDE,
            window,
            cx,
        )
        .dropdown_menu(move |menu, _, _| {
            options.iter().fold(menu, |menu, (id, text)| {
                let (id, view) = (id.clone(), view.clone());
                menu.item(
                    PopupMenuItem::new(text.clone())
                        .checked(id == picked)
                        .on_click(move |_, _, cx| {
                            let id = id.clone();
                            view.update(cx, |q, cx| q.pick(id, cx))
                        }),
                )
            })
        });
        div()
            .w_full()
            .child(sheet_form().child(sheet_form_row(s::RUNNER_MACHINE, select, cx)))
    }
}

impl Render for BotQuick {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let name = name_beside_avatar(&self.member.name);
        let title = fill(s::QUICK_TITLE, &[("name", name)]);
        let mut layout = SheetLayout::new(title);
        match &self.runner {
            Load::Loading => {
                layout = layout
                    .explanation(s::QUICK_HINT)
                    .body(widgets::list_loading("quick-loading", 1, false, cx));
            }
            Load::Failed(text) => {
                let again = cx.entity();
                layout = layout.body(widgets::failed_alert(
                    "quick-load-failed",
                    s::BOT_RUNNER_LOAD_FAILED,
                    Some(text.clone()),
                    Some(
                        widgets::retry_button("quick-retry", cx)
                            .on_click(move |_, _, cx| again.update(cx, |q, cx| q.load(cx))),
                    ),
                    cx,
                ));
            }
            Load::Loaded(None) => {
                layout = layout.explanation(fill(s::QUICK_EXTERNAL, &[("name", name)]));
            }
            Load::Loaded(Some(r)) => {
                let r = r.clone();
                layout = layout
                    .explanation(s::QUICK_HINT)
                    .body(self.fields(&r, window, cx));
            }
        }
        let status = match &self.status {
            Status::Failed(t) => Some(
                crate::ui::native::sheet::sheet_status_error(
                    format!("{}：{t}", s::BOT_EDIT_FAILED),
                    cx,
                )
                .into_any_element(),
            ),
            Status::Busy(t) | Status::Done(t) => {
                Some(widgets::quiet(t.clone(), cx).into_any_element())
            }
            Status::Idle => None,
        };
        let (services, member) = (self.services.clone(), self.member.clone());
        let more = push_button(
            "quick-more",
            s::QUICK_MORE,
            PushStyle::Outline,
            PushSize::Regular,
            cx,
        )
        .on_click(move |_, window, cx| {
            window.close_dialog(cx);
            open_edit_bot(services.clone(), &member, window, cx);
        });
        let done = push_button(
            "quick-done",
            s::DONE,
            PushStyle::Default,
            PushSize::Regular,
            cx,
        )
        .on_click(|_, window, cx| window.close_dialog(cx));
        layout
            .when_some(status, |l, st| l.status(st))
            .buttons([more, done])
    }
}

/// Opens 常用设置 of the AI member `member` in `window`.
pub fn open_bot_quick(services: Services, member: &Member, window: &mut Window, cx: &mut App) {
    let member = member.clone();
    let form = cx.new(|cx| BotQuick::new(services, member, cx));
    widgets::open_sheet(
        window,
        cx,
        SheetWidth::List,
        px(420.),
        move |dialog, _, _| dialog.child(form.clone()),
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::{MemberKind, MemberLevel};

    fn machine(id: &str, name: &str) -> MachineInfo {
        MachineInfo {
            id: id.into(),
            name: name.into(),
            ..Default::default()
        }
    }

    fn runner(machine: Option<&str>, labels: &[&str]) -> RunnerSetup {
        RunnerSetup {
            runtime: "claude".into(),
            labels: labels.iter().map(|l| l.to_string()).collect(),
            machine_id: machine.map(str::to_string),
            model: Some("m-1".into()),
            effort: Some("high".into()),
            ..Default::default()
        }
    }

    fn member(kind: MemberKind, deactivated: bool) -> Member {
        Member {
            id: "x-1".into(),
            kind,
            name: "小助手".into(),
            role: None,
            level: MemberLevel::Member,
            avatar_url: None,
            deactivated,
            online: false,
            character: None,
            team_owner: false,
        }
    }

    fn alone(this: Option<&str>) -> PlaceView<'_> {
        PlaceView {
            this,
            me: None,
            solo: true,
        }
    }

    #[test]
    fn options_start_with_any_and_mark_this_computer() {
        let list = [machine("m-a", "甲"), machine("m-b", "乙")];
        let options = machine_options(Some("m-b"), &list, alone(Some("m-a")));
        let ids: Vec<_> = options.iter().map(|(id, _)| id.as_deref()).collect();
        assert_eq!(ids, [None, Some("m-a"), Some("m-b")]);
        assert_eq!(options[0].1, s::RUNNER_MACHINE_ANY);
        assert_eq!(
            options[1].1,
            fill(s::RUNNER_MACHINE_THIS, &[("name", "甲")])
        );
        assert_eq!(options[2].1, "乙");
    }

    #[test]
    fn a_removed_computer_stays_listed_while_picked() {
        let list = [machine("m-a", "甲")];
        let options = machine_options(Some("m-gone"), &list, alone(None));
        assert_eq!(options.len(), 3);
        assert_eq!(
            options[1],
            (Some("m-gone".into()), s::RUNNER_MACHINE_GONE.into())
        );
    }

    #[test]
    fn picking_a_computer_keeps_the_rest_and_drops_labels() {
        let before = runner(None, &["gpu"]);
        let after = with_machine(&before, Some("m-a".into())).expect("a change");
        assert_eq!(after.machine_id.as_deref(), Some("m-a"));
        assert!(after.labels.is_empty());
        assert_eq!(after.model, before.model);
        assert_eq!(after.effort, before.effort);
        assert_eq!(after.runtime, before.runtime);
    }

    #[test]
    fn picking_the_same_computer_saves_nothing() {
        assert_eq!(
            with_machine(&runner(Some("m-a"), &[]), Some("m-a".into())),
            None
        );
        assert_eq!(with_machine(&runner(None, &[]), None), None);
        // 任意 over hand-typed labels still clears them.
        assert!(with_machine(&runner(None, &["gpu"]), None).is_some());
    }

    #[test]
    fn shows_only_for_active_ai_members_to_owners() {
        assert!(shows_for(&member(MemberKind::Bot, false), true));
        assert!(!shows_for(&member(MemberKind::Bot, false), false));
        assert!(!shows_for(&member(MemberKind::Bot, true), true));
        assert!(!shows_for(&member(MemberKind::Human, false), true));
    }
}
