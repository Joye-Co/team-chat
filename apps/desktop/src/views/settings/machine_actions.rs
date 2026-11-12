//! What can be done to one computer (design-roles 3.5, 机 1.5, 1.6, 3.1): rename it, remove it,
//! 交给团队 or 改成我的电脑, stop using this computer, pick the AI members whose work for me runs
//! on my own computer with my code folder there, and the screen that says what 用这台电脑 means
//! before it is done. Settings › 电脑 and 团队管理 › 电脑 open the same sheets and alerts; each
//! tells its page how it went through an [`After`].

use std::rc::Rc;

use gpui::{
    App, AppContext as _, Context, Entity, Focusable as _, IntoElement, ParentElement, Render,
    Styled, Window, div, prelude::FluentBuilder as _, px,
};
use gpui_component::{
    WindowExt as _,
    input::{InputEvent, InputState},
    menu::{PopupMenu, PopupMenuItem},
};

use super::widgets::{self, Status, answer};
use crate::views::nav::dialogs::{sheet_form, sheet_form_row};
use crate::{
    app::Services,
    copy::{common, fill, settings as s},
    data::{AdminError, MachineInfo, Member, Ownership, PersonalBot},
    ui::native::{
        AlertLayout, NativeSwitch, NativeTextField, NativeType as _, PushSize, PushStyle,
        SheetLayout, SheetWidth, alert_button, disabled_push, native_colors, push_button,
        sheet::sheet_status_error, tokens,
    },
};

/// Called once a change was answered: the page reloads, or shows why it failed.
pub type After = Rc<dyn Fn(Result<(), AdminError>, &mut App)>;

/// What I may do to one computer (design-roles 3.2, 3.5). The server checks every one again;
/// this only keeps the menu to what will work.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Allowed {
    /// 在这里做的事…: my own computer.
    pub my_bots: bool,
    /// 改名… (and 重新启动 on this computer).
    pub rename: bool,
    /// 交给团队: my own computer, and I am an admin.
    pub to_team: bool,
    /// 改成我的电脑: a team computer, and I am an admin.
    pub to_mine: bool,
    /// 不再用这台电脑…: this computer, when I manage it.
    pub stop_this: bool,
    /// 移除…: another computer I manage.
    pub remove: bool,
}

/// What `me` (an admin when `admin`) may do to `m`; `this` is this computer.
pub fn allowed(m: &MachineInfo, me: Option<&str>, admin: bool, this: bool) -> Allowed {
    let manage = m.manageable(me, admin);
    let ownership = m.ownership(me);
    Allowed {
        my_bots: ownership == Ownership::Mine,
        rename: manage,
        to_team: ownership == Ownership::Mine && admin,
        to_mine: ownership == Ownership::Team && admin,
        stop_this: this && manage,
        remove: !this && manage,
    }
}

/// The items of a computer's "⋯" after 复制名称: what `may` allows, in the order people look
/// for them, the ones that take it away last.
pub fn menu_items(
    menu: PopupMenu,
    services: &Services,
    m: &MachineInfo,
    may: Allowed,
    after: &After,
) -> PopupMenu {
    let mut menu = menu;
    let item = |label: &'static str,
                open: fn(Services, &MachineInfo, After, &mut Window, &mut App)| {
        let (services, m, after) = (services.clone(), m.clone(), after.clone());
        PopupMenuItem::new(label)
            .on_click(move |_, window, cx| open(services.clone(), &m, after.clone(), window, cx))
    };
    if may.my_bots {
        menu = menu.item(item(s::MACHINE_MY_BOTS_OPEN, |sv, m, a, w, cx| {
            open_my_bots(sv, m, a, w, cx);
        }));
    }
    if may.rename {
        menu = menu.item(item(s::MACHINE_RENAME, |sv, m, a, w, cx| {
            open_rename(sv, m, a, w, cx);
        }));
    }
    if may.to_team || may.to_mine {
        menu = menu.separator();
    }
    if may.to_team {
        menu = menu.item(item(s::MACHINE_TO_TEAM, |sv, m, a, w, cx| {
            confirm_owner(sv, m, false, a, w, cx)
        }));
    }
    if may.to_mine {
        menu = menu.item(item(s::MACHINE_TO_MINE, |sv, m, a, w, cx| {
            confirm_owner(sv, m, true, a, w, cx)
        }));
    }
    if may.stop_this || may.remove {
        menu = menu.separator();
    }
    if may.stop_this {
        menu = menu.item(item(s::THIS_STOP, |sv, _, a, w, cx| {
            confirm_stop_this(sv, a, w, cx)
        }));
    }
    if may.remove {
        menu = menu.item(item(s::MACHINE_REMOVE, confirm_remove));
    }
    menu
}

/// Runs `reply` and hands its outcome to `after` on the UI thread.
fn finish<T: 'static>(reply: crate::data::AdminReply<T>, after: After, cx: &mut App) {
    cx.spawn(async move |cx| {
        let result = answer(reply).await.map(|_| ());
        cx.update(|cx| after(result, cx));
    })
    .detach();
}

/// An alert with one action and 取消; `run` starts the change once the action is chosen.
#[allow(clippy::too_many_arguments)]
fn ask(
    id: &'static str,
    title: String,
    message: &'static str,
    action: &'static str,
    destructive: bool,
    run: impl Fn(&mut App) + 'static,
    window: &mut Window,
    cx: &mut App,
) {
    let run = Rc::new(run);
    widgets::open_alert(window, cx, move |_, cx| {
        let run = run.clone();
        AlertLayout::new(title.clone())
            .icon(widgets::app_icon(cx))
            .message(message)
            .buttons([
                alert_button((id, 0usize), action, PushStyle::Default, destructive, cx).on_click(
                    move |_, window, cx| {
                        window.close_dialog(cx);
                        run(cx);
                    },
                ),
                alert_button((id, 1usize), common::CANCEL, PushStyle::Outline, false, cx)
                    .on_click(|_, window, cx| window.close_dialog(cx)),
            ])
            .into_any_element()
    });
}

/// 移除…: the computer leaves the team; its daemon's token stops working.
pub fn confirm_remove(
    services: Services,
    m: &MachineInfo,
    after: After,
    window: &mut Window,
    cx: &mut App,
) {
    let id = m.id.clone();
    ask(
        "machine-remove",
        fill(s::MACHINE_REMOVE_TITLE, &[("name", &m.name)]),
        s::MACHINE_REMOVE_BODY,
        s::MACHINE_REMOVE_CONFIRM,
        true,
        move |cx| finish(services.provider.remove_machine(&id), after.clone(), cx),
        window,
        cx,
    );
}

/// 交给团队 (`mine` false) or 改成我的电脑 (`mine` true), after an alert that says what changes.
pub fn confirm_owner(
    services: Services,
    m: &MachineInfo,
    mine: bool,
    after: After,
    window: &mut Window,
    cx: &mut App,
) {
    let id = m.id.clone();
    let (key, title, body, action) = if mine {
        (
            "machine-to-mine",
            s::MACHINE_TO_MINE_TITLE,
            s::MACHINE_TO_MINE_CONFIRM,
            s::MACHINE_TO_MINE,
        )
    } else {
        (
            "machine-to-team",
            s::MACHINE_TO_TEAM_TITLE,
            s::MACHINE_TO_TEAM_BODY,
            s::MACHINE_TO_TEAM,
        )
    };
    ask(
        key,
        fill(title, &[("name", &m.name)]),
        body,
        action,
        false,
        move |cx| {
            finish(
                services.provider.set_machine_owner(&id, mine),
                after.clone(),
                cx,
            )
        },
        window,
        cx,
    );
}

/// 不再用这台电脑… (机 1.6): this computer leaves the team, and its background service and the
/// files that registered it are removed.
pub fn confirm_stop_this(services: Services, after: After, window: &mut Window, cx: &mut App) {
    ask(
        "machine-stop-this",
        s::THIS_STOP_TITLE.to_string(),
        s::THIS_STOP_BODY,
        s::THIS_STOP_CONFIRM,
        true,
        move |cx| {
            finish(
                services.provider.stop_using_this_computer(),
                after.clone(),
                cx,
            )
        },
        window,
        cx,
    );
}

/// 用这台电脑 says first what it means (机 3.1, verify note 9): the AI members act here with my
/// permissions, and my own Claude Code and Codex settings come along. `solo` hides the line
/// about whose computer it is (design-roles 3.9). `on_use` sets the computer up.
pub fn open_use_this(
    solo: bool,
    on_use: impl Fn(&mut App) + 'static,
    window: &mut Window,
    cx: &mut App,
) {
    let on_use = Rc::new(on_use);
    widgets::open_sheet(
        window,
        cx,
        SheetWidth::Form,
        px(380.),
        move |dialog, _, cx| {
            let on_use = on_use.clone();
            dialog.child(
                use_this_layout(solo, cx).buttons([
                    push_button(
                        "use-this-cancel",
                        common::CANCEL,
                        PushStyle::Outline,
                        PushSize::Regular,
                        cx,
                    )
                    .on_click(|_, window, cx| window.close_dialog(cx)),
                    push_button(
                        "use-this-ok",
                        s::THIS_INTRO_CONFIRM,
                        PushStyle::Default,
                        PushSize::Regular,
                        cx,
                    )
                    .on_click(move |_, window, cx| {
                        window.close_dialog(cx);
                        on_use(cx);
                    }),
                ]),
            )
        },
    );
}

/// The text of the 用这台电脑 screen.
fn use_this_layout(solo: bool, cx: &App) -> SheetLayout {
    let k = native_colors(cx);
    let para = |text: &'static str| div().n_body().text_color(k.label).child(text);
    SheetLayout::new(s::THIS_INTRO_TITLE).body(
        div()
            .w_full()
            .flex()
            .flex_col()
            .gap(tokens::space::S12)
            .child(para(s::THIS_INTRO_BODY))
            .child(para(s::THIS_INTRO_SETTINGS))
            .when(!solo, |d| {
                d.child(
                    div()
                        .n_footnote()
                        .text_color(k.secondary)
                        .child(s::THIS_INTRO_PERSONAL),
                )
            }),
    )
}

// --- 改名… ---

pub struct RenameForm {
    services: Services,
    id: String,
    before: String,
    name: Entity<InputState>,
    status: Status,
    after: After,
    _sub: gpui::Subscription,
}

impl RenameForm {
    fn submit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.status.busy() {
            return;
        }
        let name = self.name.read(cx).value().trim().to_string();
        if name.is_empty() || name == self.before {
            window.close_dialog(cx);
            return;
        }
        if let Err(e) = widgets::check_len(&name, 64) {
            self.status = Status::Failed(e.into());
            cx.notify();
            return;
        }
        let reply = self.services.provider.rename_machine(&self.id, name);
        self.status = Status::Busy(s::SAVING.into());
        cx.notify();
        let handle = window.window_handle();
        cx.spawn(async move |this, cx| {
            let result = answer(reply).await;
            let _ = this.update(cx, |this, cx| match result {
                Ok(_) => {
                    this.status = Status::Idle;
                    let after = this.after.clone();
                    let _ = handle.update(cx, |_, window, cx| window.close_dialog(cx));
                    after(Ok(()), cx);
                }
                Err(e) => {
                    this.status = Status::from_error(e);
                    cx.notify();
                }
            });
        })
        .detach();
    }
}

impl Render for RenameForm {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let busy = self.status.busy();
        let typed = self.name.read(cx).value().trim().to_string();
        let problem = (!typed.is_empty())
            .then(|| widgets::check_len(&typed, 64).err())
            .flatten();
        let off = busy || typed.is_empty() || problem.is_some();
        let save = cx.entity();
        let status = match (&problem, &self.status) {
            (Some(p), _) => Some(sheet_status_error(p.clone(), cx)),
            (None, Status::Failed(t)) => Some(sheet_status_error(
                format!("{}：{t}", s::MACHINE_ACTION_FAILED),
                cx,
            )),
            _ => None,
        };
        SheetLayout::new(fill(s::MACHINE_RENAME_TITLE, &[("name", &self.before)]))
            .body(
                sheet_form().child(sheet_form_row(
                    s::MACHINE_NAME,
                    NativeTextField::new("machine-rename", &self.name)
                        .full_width()
                        .error(problem.is_some()),
                    cx,
                )),
            )
            .when_some(status, |l, st| l.status(st))
            .buttons([
                push_button(
                    "machine-rename-cancel",
                    common::CANCEL,
                    PushStyle::Outline,
                    PushSize::Regular,
                    cx,
                )
                .on_click(|_, window, cx| window.close_dialog(cx)),
                disabled_push(
                    push_button(
                        "machine-rename-save",
                        common::SAVE,
                        PushStyle::Default,
                        PushSize::Regular,
                        cx,
                    ),
                    off,
                    cx,
                )
                .loading(busy)
                .on_click(move |_, window, cx| save.update(cx, |f, cx| f.submit(window, cx))),
            ])
    }
}

/// 改名…: a sheet with the name, saved on 保存 or Return.
pub fn open_rename(
    services: Services,
    m: &MachineInfo,
    after: After,
    window: &mut Window,
    cx: &mut App,
) -> Entity<RenameForm> {
    let (id, before) = (m.id.clone(), m.name.clone());
    let form = cx.new(|cx| {
        let name = cx.new(|cx| {
            let mut st = InputState::new(window, cx).placeholder(s::MACHINE_NAME_PLACEHOLDER);
            st.set_value(before.clone(), window, cx);
            st
        });
        let sub = cx.subscribe_in(
            &name,
            window,
            |this: &mut RenameForm, _, e: &InputEvent, window, cx| match e {
                InputEvent::PressEnter { .. } => this.submit(window, cx),
                InputEvent::Change => cx.notify(),
                _ => {}
            },
        );
        RenameForm {
            services,
            id,
            before,
            name,
            status: Status::Idle,
            after,
            _sub: sub,
        }
    });
    let shown = form.clone();
    let first = std::cell::Cell::new(true);
    widgets::open_sheet(
        window,
        cx,
        SheetWidth::Form,
        px(300.),
        move |dialog, window, cx| {
            if first.replace(false) {
                shown
                    .read(cx)
                    .name
                    .read(cx)
                    .focus_handle(cx)
                    .focus(window, cx);
            }
            dialog.child(shown.clone())
        },
    );
    form
}

// --- 在这里做的事… ---

/// One AI member of the sheet: checked or not, and my code folder for it.
struct BotRow {
    member: Member,
    on: bool,
    repo: Entity<InputState>,
    /// The folder as saved, to save only a change.
    saved: Option<String>,
}

/// 我交代给这些成员的事，在这台电脑上做 (design-roles 3.3, 3.9): a switch per AI member and,
/// once on, my code folder on this computer. Every change saves at once.
pub struct MyBotsForm {
    services: Services,
    machine: MachineInfo,
    rows: Vec<BotRow>,
    status: Status,
    after: After,
    _subs: Vec<gpui::Subscription>,
}

/// The AI members the sheet lists: active ones, those already checked first, then by name.
/// Members cannot read how an AI member runs (its runner is for admins), so the server and the
/// dispatch decide whether this computer can run it (design-roles 3.4).
pub fn bots_to_list(members: &[Member], checked: &[PersonalBot]) -> Vec<Member> {
    let mut list: Vec<Member> = members
        .iter()
        .filter(|m| m.is_bot() && !m.deactivated)
        .cloned()
        .collect();
    let on = |m: &Member| checked.iter().any(|b| b.bot_id == m.id);
    list.sort_by(|a, b| on(b).cmp(&on(a)).then_with(|| a.name.cmp(&b.name)));
    list
}

impl MyBotsForm {
    fn new(
        services: Services,
        machine: MachineInfo,
        after: After,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let members = services.provider.members();
        let mut subs = Vec::new();
        let rows = bots_to_list(&members, &machine.personal_bots)
            .into_iter()
            .map(|member| {
                let saved = machine
                    .personal_bots
                    .iter()
                    .find(|b| b.bot_id == member.id)
                    .map(|b| b.repo_path.clone());
                let on = saved.is_some();
                let saved = saved.flatten();
                let repo = cx.new(|cx| {
                    let mut st = InputState::new(window, cx)
                        .placeholder(s::MACHINE_MY_BOTS_REPO_PLACEHOLDER);
                    st.set_value(saved.clone().unwrap_or_default(), window, cx);
                    st
                });
                let id = member.id.clone();
                subs.push(cx.subscribe_in(
                    &repo,
                    window,
                    move |this: &mut MyBotsForm, _, e: &InputEvent, _, cx| {
                        if matches!(e, InputEvent::PressEnter { .. } | InputEvent::Blur) {
                            this.save_repo(&id, cx);
                        }
                    },
                ));
                BotRow {
                    member,
                    on,
                    repo,
                    saved,
                }
            })
            .collect();
        Self {
            services,
            machine,
            rows,
            status: Status::Idle,
            after,
            _subs: subs,
        }
    }

    /// The switch of `bot_id`: on sets it with the folder typed, off takes it away.
    fn toggle(&mut self, bot_id: &str, on: bool, cx: &mut Context<Self>) {
        let Some(row) = self.rows.iter_mut().find(|r| r.member.id == bot_id) else {
            return;
        };
        row.on = on;
        let reply = if on {
            let repo = row.repo.read(cx).value().trim().to_string();
            row.saved = (!repo.is_empty()).then_some(repo.clone());
            self.services.provider.set_machine_bot(
                &self.machine.id,
                bot_id,
                (!repo.is_empty()).then_some(repo),
            )
        } else {
            self.services
                .provider
                .unset_machine_bot(&self.machine.id, bot_id)
        };
        self.send(reply, bot_id.to_string(), Some(!on), cx);
    }

    /// My code folder for a checked AI member, when it changed.
    fn save_repo(&mut self, bot_id: &str, cx: &mut Context<Self>) {
        let Some(row) = self.rows.iter_mut().find(|r| r.member.id == bot_id && r.on) else {
            return;
        };
        let repo = row.repo.read(cx).value().trim().to_string();
        let repo = (!repo.is_empty()).then_some(repo);
        if repo == row.saved {
            return;
        }
        row.saved = repo.clone();
        let reply = self
            .services
            .provider
            .set_machine_bot(&self.machine.id, bot_id, repo);
        self.send(reply, bot_id.to_string(), None, cx);
    }

    /// Waits for a save; a failed switch goes back to `undo`.
    fn send(
        &mut self,
        reply: crate::data::AdminReply<MachineInfo>,
        bot_id: String,
        undo: Option<bool>,
        cx: &mut Context<Self>,
    ) {
        self.status = Status::Busy(s::SAVING.into());
        cx.notify();
        cx.spawn(async move |this, cx| {
            let result = answer(reply).await;
            let _ = this.update(cx, |this, cx| {
                match result {
                    Ok(m) => {
                        this.machine = m;
                        this.status = Status::Done(s::QUICK_SAVED.into());
                        (this.after)(Ok(()), cx);
                    }
                    Err(e) => {
                        if let (Some(was), Some(row)) =
                            (undo, this.rows.iter_mut().find(|r| r.member.id == bot_id))
                        {
                            row.on = was;
                        }
                        this.status = Status::from_error(e);
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    pub fn checked(&self) -> Vec<String> {
        self.rows
            .iter()
            .filter(|r| r.on)
            .map(|r| r.member.id.clone())
            .collect()
    }
}

impl Render for MyBotsForm {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let k = native_colors(cx);
        let page = cx.entity();
        let rows: Vec<gpui::AnyElement> = self
            .rows
            .iter()
            .map(|r| {
                let (id, page) = (r.member.id.clone(), page.clone());
                let switch = NativeSwitch::new(
                    gpui::ElementId::Name(format!("my-bot-{}", r.member.id).into()),
                    r.on,
                )
                .name(r.member.name.clone())
                .on_change(move |on, _, cx| {
                    let id = id.clone();
                    page.update(cx, |f, cx| f.toggle(&id, on, cx));
                });
                div()
                    .w_full()
                    .flex()
                    .flex_col()
                    .gap(tokens::space::S6)
                    .px(tokens::space::S12)
                    .py(tokens::space::S8)
                    .child(
                        div()
                            .w_full()
                            .flex()
                            .items_center()
                            .gap(tokens::space::S8)
                            .child(crate::ui::MemberAvatar::new(
                                Some(&r.member),
                                tokens::avatar::XS,
                            ))
                            .child(
                                div()
                                    .flex_1()
                                    .min_w(px(0.))
                                    .n_body()
                                    .text_color(k.label)
                                    .child(r.member.name.clone()),
                            )
                            .child(switch),
                    )
                    .when(r.on, |d| {
                        d.child(sheet_form_row(
                            s::RUNNER_REPO,
                            NativeTextField::new(
                                gpui::ElementId::Name(
                                    format!("my-bot-repo-{}", r.member.id).into(),
                                ),
                                &r.repo,
                            )
                            .full_width(),
                            cx,
                        ))
                    })
                    .into_any_element()
            })
            .collect();
        let list = if rows.is_empty() {
            widgets::quiet(s::MACHINE_MY_BOTS_EMPTY, cx).into_any_element()
        } else {
            sheet_form().children(rows).into_any_element()
        };
        let status = match &self.status {
            Status::Failed(t) => Some(
                sheet_status_error(format!("{}：{t}", s::MACHINE_MY_BOTS_FAILED), cx)
                    .into_any_element(),
            ),
            Status::Busy(t) | Status::Done(t) => {
                Some(widgets::quiet(t.clone(), cx).into_any_element())
            }
            Status::Idle => None,
        };
        SheetLayout::new(fill(
            s::MACHINE_MY_BOTS_TITLE,
            &[("name", &self.machine.name)],
        ))
        .explanation(s::MACHINE_MY_BOTS_HINT)
        .body(
            div()
                .w_full()
                .flex()
                .flex_col()
                .gap(tokens::space::S6)
                .child(div().n_headline().child(s::MACHINE_MY_BOTS))
                .child(list),
        )
        .when_some(status, |l, st| l.status(st))
        .buttons([push_button(
            "my-bots-done",
            s::DONE,
            PushStyle::Default,
            PushSize::Regular,
            cx,
        )
        .on_click(|_, window, cx| window.close_dialog(cx))])
    }
}

/// 在这里做的事…: the AI members whose work for me runs on my computer `m`.
pub fn open_my_bots(
    services: Services,
    m: &MachineInfo,
    after: After,
    window: &mut Window,
    cx: &mut App,
) -> Entity<MyBotsForm> {
    let machine = m.clone();
    let form = cx.new(|cx| MyBotsForm::new(services, machine, after, window, cx));
    let shown = form.clone();
    widgets::open_sheet(
        window,
        cx,
        SheetWidth::List,
        px(460.),
        move |dialog, _, _| dialog.child(shown.clone()),
    );
    form
}

/// The second line of my own computer: whose work it does for me.
pub fn my_bots_summary(m: &MachineInfo, members: &[Member]) -> Option<String> {
    let names: Vec<String> = m
        .personal_bots
        .iter()
        .filter_map(|b| members.iter().find(|x| x.id == b.bot_id))
        .map(|x| crate::ui::avatar::name_beside_avatar(&x.name).to_string())
        .collect();
    (!names.is_empty()).then(|| {
        fill(
            s::MACHINE_MY_BOTS_SUMMARY,
            &[("names", &names.join(s::MACHINE_MY_BOTS_AND))],
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::{MemberKind, MemberLevel};

    fn computer(owner: Option<&str>) -> MachineInfo {
        MachineInfo {
            id: "c".into(),
            owner_id: owner.map(str::to_string),
            ..Default::default()
        }
    }

    /// design-roles 3.2 and 3.5: a team computer is the admins'; my own computer is mine
    /// alone, also for an admin; somebody else's is nobody's but theirs.
    #[test]
    fn who_may_do_what_to_a_computer() {
        let me = Some("me");
        let team = computer(None);
        let mine = computer(Some("me"));
        let theirs = computer(Some("chen"));
        // An admin.
        let a = allowed(&team, me, true, false);
        assert!(a.rename && a.remove && a.to_mine && !a.to_team && !a.my_bots && !a.stop_this);
        let a = allowed(&mine, me, true, false);
        assert!(a.rename && a.remove && a.to_team && a.my_bots && !a.to_mine);
        assert_eq!(allowed(&theirs, me, true, false), Allowed::default());
        // A member.
        assert_eq!(allowed(&team, me, false, false), Allowed::default());
        let a = allowed(&mine, me, false, false);
        assert!(a.rename && a.remove && a.my_bots && !a.to_team && !a.to_mine);
        assert_eq!(allowed(&theirs, me, false, true), Allowed::default());
        // This computer: 不再用这台电脑 in place of 移除.
        let a = allowed(&mine, me, false, true);
        assert!(a.stop_this && !a.remove);
        let a = allowed(&team, me, false, true);
        assert!(!a.stop_this, "a member cannot take a team computer away");
    }

    fn member(id: &str, name: &str, kind: MemberKind, deactivated: bool) -> Member {
        Member {
            id: id.into(),
            kind,
            name: name.into(),
            role: None,
            level: MemberLevel::Member,
            avatar_url: None,
            deactivated,
            online: true,
            character: None,
            team_owner: false,
        }
    }

    #[test]
    fn the_sheet_lists_active_ai_members_checked_first() {
        let members = [
            member("b-1", "Forge", MemberKind::Bot, false),
            member("b-2", "Atlas", MemberKind::Bot, false),
            member("b-3", "Old", MemberKind::Bot, true),
            member("h-1", "小王", MemberKind::Human, false),
            member("b-4", "Jojo", MemberKind::Bot, false),
        ];
        let checked = [PersonalBot {
            bot_id: "b-4".into(),
            repo_path: None,
        }];
        let ids: Vec<String> = bots_to_list(&members, &checked)
            .into_iter()
            .map(|m| m.id)
            .collect();
        assert_eq!(ids, ["b-4", "b-2", "b-1"]);
    }

    #[test]
    fn my_computer_says_whose_work_it_does() {
        let members = [
            member("b-1", "Forge", MemberKind::Bot, false),
            member("b-2", "Jojo", MemberKind::Bot, false),
        ];
        let mut m = MachineInfo::default();
        assert_eq!(my_bots_summary(&m, &members), None);
        m.personal_bots = vec![
            PersonalBot {
                bot_id: "b-2".into(),
                repo_path: Some("~/code".into()),
            },
            PersonalBot {
                bot_id: "b-1".into(),
                repo_path: None,
            },
            // A member that is gone says nothing.
            PersonalBot {
                bot_id: "b-9".into(),
                repo_path: None,
            },
        ];
        assert_eq!(
            my_bots_summary(&m, &members).as_deref(),
            Some(fill(s::MACHINE_MY_BOTS_SUMMARY, &[("names", "Jojo、Forge")]).as_str())
        );
    }
}
