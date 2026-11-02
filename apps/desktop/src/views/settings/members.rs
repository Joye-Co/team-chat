//! 成员: everyone with level and status. Owners also invite people, add bot
//! members, change levels and deactivate members.

use gpui::{
    App, AppContext as _, Context, ElementId, Entity, IntoElement, ParentElement, Render,
    SharedString, Styled, Subscription, Window, div, prelude::FluentBuilder as _, px,
};
use gpui_component::{
    Disableable as _, Icon, Sizable as _, WindowExt as _,
    button::{Button, ButtonVariant, ButtonVariants as _},
    input::{Input, InputState},
    menu::{DropdownMenu as _, PopupMenuItem},
};

use super::{
    i_am_owner,
    widgets::{self, Status, answer},
};
use crate::{
    app::{Changes, Services},
    copy::{common, fill, settings as s},
    data::{
        BotCreated, Change, InviteCreated, InviteRequest, Member, MemberKind, MemberLevel,
        RunnerSetup,
    },
    theme,
    theme::{Type as _, space},
    ui::{self, Lucide},
    views::presence_dot,
};

/// Members in list order: active people first, then active bot members, then
/// deactivated members; by name inside each part.
pub fn sorted(mut members: Vec<Member>) -> Vec<Member> {
    members.sort_by(|a, b| {
        (a.deactivated, a.is_bot(), a.name.to_lowercase()).cmp(&(
            b.deactivated,
            b.is_bot(),
            b.name.to_lowercase(),
        ))
    });
    members
}

pub struct MembersPage {
    services: Services,
    /// The last row request's result (level or deactivate).
    status: Status,
    _subscriptions: Vec<Subscription>,
}

impl MembersPage {
    pub fn new(services: Services, _window: &mut Window, cx: &mut Context<Self>) -> Self {
        let subscriptions = vec![cx.subscribe(&services.hub, |_, _, changes: &Changes, cx| {
            if changes.any(|c| matches!(c, Change::Members | Change::Session)) {
                cx.notify();
            }
        })];
        Self {
            services,
            status: Status::Idle,
            _subscriptions: subscriptions,
        }
    }

    pub fn status(&self) -> &Status {
        &self.status
    }

    pub fn set_level(&mut self, id: String, level: MemberLevel, cx: &mut Context<Self>) {
        let reply = self.services.provider.set_member_level(&id, level);
        self.run(reply, cx);
    }

    pub fn set_deactivated(&mut self, id: String, off: bool, cx: &mut Context<Self>) {
        let reply = self.services.provider.set_member_deactivated(&id, off);
        let reactivated = !off;
        self.status = Status::Idle;
        cx.spawn(async move |this, cx| {
            let result = answer(reply).await;
            let _ = this.update(cx, |this, cx| {
                this.status = match result {
                    Ok(_) if reactivated => Status::Done(s::REACTIVATE_HINT.into()),
                    Ok(_) => Status::Idle,
                    Err(e) => Status::from_error(e),
                };
                cx.notify();
            });
        })
        .detach();
    }

    fn run(&mut self, reply: crate::data::AdminReply<Member>, cx: &mut Context<Self>) {
        self.status = Status::Idle;
        cx.spawn(async move |this, cx| {
            let result = answer(reply).await;
            let _ = this.update(cx, |this, cx| {
                this.status = match result {
                    Ok(_) => Status::Idle,
                    Err(e) => Status::from_error(e),
                };
                cx.notify();
            });
        })
        .detach();
    }

    fn confirm_deactivate(&self, member: &Member, window: &mut Window, cx: &mut Context<Self>) {
        let page = cx.entity().downgrade();
        let id = member.id.clone();
        let name = member.name.clone();
        window.open_alert_dialog(cx, move |alert, _, _| {
            let alert = crate::ui::SheetExt::sheet(alert);
            let page = page.clone();
            let id = id.clone();
            alert
                .title(fill(s::DEACTIVATE_TITLE, &[("name", &name)]))
                .description(fill(s::DEACTIVATE_BODY, &[("name", &name)]))
                .ok_text(s::DEACTIVATE)
                .ok_variant(ButtonVariant::Danger)
                .cancel_text(common::CANCEL)
                .show_cancel(true)
                .on_ok(move |_, _, cx| {
                    let id = id.clone();
                    let _ = page.update(cx, |this, cx| this.set_deactivated(id, true, cx));
                    true
                })
        });
    }

    /// `8 位`: active members, for the header.
    pub fn count_text(&self) -> String {
        let n = self
            .services
            .provider
            .members()
            .iter()
            .filter(|m| !m.deactivated)
            .count();
        fill(s::MEMBERS_COUNT, &[("n", &n.to_string())])
    }

    /// The header buttons: 添加自动成员 (secondary) then 邀请成员 (primary).
    pub fn header_actions(&self, cx: &App) -> gpui::AnyElement {
        let _ = cx;
        let owner = i_am_owner(&self.services);
        let services = self.services.clone();
        let services2 = self.services.clone();
        div()
            .flex()
            .items_center()
            .gap(space::S8)
            .child(
                Button::new("add-bot")
                    .small()
                    .icon(Icon::new(Lucide::Bot))
                    .label(s::ADD_BOT_MEMBER)
                    .disabled(!owner)
                    .on_click(move |_, window, cx| {
                        open_add_bot(services2.clone(), window, cx);
                    }),
            )
            .child(
                Button::new("invite")
                    .primary()
                    .small()
                    .icon(Icon::new(Lucide::UserPlus))
                    .label(s::INVITE)
                    .disabled(!owner)
                    .on_click(move |_, window, cx| {
                        open_invite(services.clone(), window, cx);
                    }),
            )
            .into_any_element()
    }

    fn row(&self, m: &Member, me: Option<&str>, owner: bool, cx: &mut Context<Self>) -> gpui::Div {
        let colors = theme::colors(cx);
        let is_me = me == Some(m.id.as_str());
        let id = m.id.clone();
        let page = cx.entity().downgrade();
        let status_text = if m.deactivated {
            common::MEMBER_DEACTIVATED
        } else if m.online {
            s::MEMBER_ONLINE
        } else {
            s::MEMBER_OFFLINE
        };
        // 权限: a menu button for owners, plain text otherwise (§13.10).
        let level: gpui::AnyElement = if owner && !m.deactivated {
            let current = m.level;
            let target = id.clone();
            let page = page.clone();
            Button::new(ElementId::Name(format!("level-{id}").into()))
                .small()
                .w(px(92.))
                .label(widgets::level_text(m.level))
                .dropdown_caret(true)
                .dropdown_menu(move |menu, _, _| {
                    [MemberLevel::Owner, MemberLevel::Member]
                        .into_iter()
                        .fold(menu, |menu, level| {
                            let page = page.clone();
                            let target = target.clone();
                            menu.item(
                                PopupMenuItem::new(widgets::level_text(level))
                                    .checked(level == current)
                                    .on_click(move |_, _, cx| {
                                        if level != current {
                                            let target = target.clone();
                                            let _ = page.update(cx, |this, cx| {
                                                this.set_level(target, level, cx)
                                            });
                                        }
                                    }),
                            )
                        })
                })
                .into_any_element()
        } else {
            div()
                .w(px(92.))
                .px(space::S8)
                .t_callout()
                .text_color(colors.text_2)
                .child(widgets::level_text(m.level))
                .into_any_element()
        };
        // 停用… or 恢复 in the row's ellipsis menu; the slot stays to keep columns.
        let more: gpui::AnyElement = if owner && !is_me {
            let member = m.clone();
            let target = id.clone();
            let deactivated = m.deactivated;
            Button::new(ElementId::Name(format!("more-{id}").into()))
                .ghost()
                .small()
                .w(px(28.))
                .icon(Icon::new(Lucide::Ellipsis))
                .tooltip(s::MEMBER_MORE)
                .dropdown_menu_with_anchor(gpui::Anchor::TopRight, move |menu, _, _| {
                    let page = page.clone();
                    if deactivated {
                        let target = target.clone();
                        menu.item(PopupMenuItem::new(s::REACTIVATE).on_click(move |_, _, cx| {
                            let target = target.clone();
                            let _ = page
                                .update(cx, |this, cx| this.set_deactivated(target, false, cx));
                        }))
                    } else {
                        let member = member.clone();
                        menu.item(PopupMenuItem::new(s::DEACTIVATE_MENU).on_click(
                            move |_, window, cx| {
                                let _ = page.update(cx, |this, cx| {
                                    this.confirm_deactivate(&member, window, cx)
                                });
                            },
                        ))
                    }
                })
                .into_any_element()
        } else {
            div().w(px(28.)).flex_none().into_any_element()
        };
        widgets::group_row()
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .items_center()
                    .gap(space::S10)
                    .when(m.deactivated, |el| el.opacity(0.5))
                    .child(ui::MemberAvatar::new(Some(m), px(28.)).on(colors.group))
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
                                    .gap(space::S4)
                                    .min_w_0()
                                    .child(
                                        div()
                                            .min_w_0()
                                            .t_ui()
                                            .medium()
                                            .one_line()
                                            .child(m.name.clone()),
                                    )
                                    .when(is_me, |el| {
                                        el.child(
                                            div()
                                                .flex_none()
                                                .t_callout()
                                                .text_color(colors.text_2)
                                                .child(s::MEMBER_ME),
                                        )
                                    }),
                            )
                            .children(m.role.clone().filter(|r| !r.is_empty()).map(|role| {
                                div()
                                    .t_callout()
                                    .one_line()
                                    .text_color(colors.text_2)
                                    .child(role)
                            })),
                    )
                    .child(
                        div()
                            .w(px(92.))
                            .flex_none()
                            .flex()
                            .items_center()
                            .gap(space::S6)
                            .t_callout()
                            .text_color(colors.text_2)
                            .when(!m.deactivated, |el| {
                                el.when(m.online, |el| el.child(presence_dot(true, cx)))
                            })
                            .child(status_text),
                    ),
            )
            .child(
                div()
                    .flex_none()
                    .flex()
                    .items_center()
                    .gap(space::S6)
                    .when(m.deactivated, |el| el.opacity(0.5))
                    .child(level)
                    .child(more),
            )
    }
}

impl Render for MembersPage {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let colors = theme::colors(cx);
        let owner = i_am_owner(&self.services);
        let me = self.services.me();
        let members = sorted(self.services.provider.members());
        let rows: Vec<_> = members
            .iter()
            .map(|m| self.row(m, me.as_deref(), owner, cx))
            .collect();
        // Separators inset to the text: 12 padding + 28 avatar + 10 gap.
        let list = widgets::group_rows(rows, px(50.), cx);
        div()
            .flex()
            .flex_col()
            .child(
                div()
                    .flex()
                    .items_end()
                    .justify_between()
                    .gap(space::S12)
                    .px(space::S12)
                    .pb(space::S6)
                    .t_caption()
                    .text_color(colors.text_3)
                    .child(
                        div()
                            .min_w_0()
                            .one_line()
                            .when(!owner, |el| el.child(s::OWNER_ONLY)),
                    )
                    .child(div().flex_none().w(px(126.)).child(s::LEVEL)),
            )
            .children(widgets::status_line(&self.status, cx).map(|l| l.px(space::S12).pb(space::S8)))
            .child(list)
    }
}

// --- invite ---

/// The invite dialog: a form, then the code shown once.
pub struct InviteForm {
    services: Services,
    name: Entity<InputState>,
    role: Entity<InputState>,
    kind: MemberKind,
    level: MemberLevel,
    status: Status,
    created: Option<InviteCreated>,
}

impl InviteForm {
    pub fn new(services: Services, window: &mut Window, cx: &mut Context<Self>) -> Self {
        Self {
            services,
            name: cx.new(|cx| InputState::new(window, cx).placeholder(s::INVITE_NAME_PLACEHOLDER)),
            role: cx.new(|cx| InputState::new(window, cx).placeholder(s::ROLE_PLACEHOLDER)),
            kind: MemberKind::Human,
            level: MemberLevel::Member,
            status: Status::Idle,
            created: None,
        }
    }

    pub fn created(&self) -> Option<&InviteCreated> {
        self.created.as_ref()
    }

    pub fn status(&self) -> &Status {
        &self.status
    }

    pub fn fill(&mut self, name: &str, window: &mut Window, cx: &mut Context<Self>) {
        self.name
            .update(cx, |st, cx| st.set_value(name.to_string(), window, cx));
    }

    pub fn submit(&mut self, cx: &mut Context<Self>) {
        if self.status.busy() || self.created.is_some() {
            return;
        }
        let name = self.name.read(cx).value().trim().to_string();
        let role = self.role.read(cx).value().trim().to_string();
        if name.is_empty() {
            self.status = Status::Failed(s::NAME_REQUIRED.into());
            cx.notify();
            return;
        }
        if let Err(e) = widgets::check_len(&name, 64).and(widgets::check_len(&role, 64)) {
            self.status = Status::Failed(e.into());
            cx.notify();
            return;
        }
        let reply = self.services.provider.create_invite(InviteRequest {
            name,
            kind: self.kind,
            level: self.level,
            role: (!role.is_empty()).then_some(role),
        });
        self.status = Status::Busy(s::SAVING.into());
        cx.notify();
        cx.spawn(async move |this, cx| {
            let result = answer(reply).await;
            let _ = this.update(cx, |this, cx| {
                match result {
                    Ok(created) => {
                        this.created = Some(created);
                        this.status = Status::Idle;
                    }
                    Err(e) => this.status = Status::from_error(e),
                }
                cx.notify();
            });
        })
        .detach();
    }
}

impl Render for InviteForm {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if let Some(created) = &self.created {
            return div()
                .flex()
                .flex_col()
                .gap(px(12.))
                .child(widgets::hint(s::SHOWN_ONCE, cx))
                .child(widgets::secret_block(
                    "invite-code",
                    s::INVITE_CODE,
                    created.code.expose(),
                    cx,
                ))
                .child(widgets::hint(
                    fill(
                        s::INVITE_EXPIRES,
                        &[("date", &widgets::date_text(created.expires_at))],
                    ),
                    cx,
                ))
                .child(widgets::secret_block(
                    "invite-download",
                    s::INVITE_DOWNLOAD,
                    created.download_url.expose(),
                    cx,
                ))
                .child(widgets::hint(s::INVITE_DOWNLOAD_HINT, cx))
                .child(
                    div().flex().justify_end().child(
                        Button::new("invite-done")
                            .primary()
                            .label(s::DONE)
                            .on_click(|_, window, cx| window.close_dialog(cx)),
                    ),
                )
                .into_any_element();
        }
        let busy = self.status.busy();
        let kind_ix = usize::from(self.kind == MemberKind::Bot);
        let level_ix = usize::from(self.level == MemberLevel::Owner);
        let page = cx.entity().downgrade();
        let page2 = page.clone();
        div()
            .flex()
            .flex_col()
            .gap(px(10.))
            .child(widgets::field(s::NAME, Input::new(&self.name), cx))
            .child(widgets::field(s::ROLE, Input::new(&self.role), cx))
            .child(widgets::field(
                s::KIND,
                widgets::choice(
                    "invite-kind",
                    &[s::KIND_HUMAN, s::KIND_BOT],
                    kind_ix,
                    busy,
                    move |ix, _, cx| {
                        let _ = page.update(cx, |this, cx| {
                            this.kind = if ix == 1 {
                                MemberKind::Bot
                            } else {
                                MemberKind::Human
                            };
                            cx.notify();
                        });
                    },
                ),
                cx,
            ))
            .child(widgets::field(
                s::LEVEL,
                widgets::choice(
                    "invite-level",
                    &[s::LEVEL_MEMBER, s::LEVEL_OWNER],
                    level_ix,
                    busy,
                    move |ix, _, cx| {
                        let _ = page2.update(cx, |this, cx| {
                            this.level = if ix == 1 {
                                MemberLevel::Owner
                            } else {
                                MemberLevel::Member
                            };
                            cx.notify();
                        });
                    },
                ),
                cx,
            ))
            .children(widgets::status_line(&self.status, cx))
            .child(
                div()
                    .flex()
                    .justify_end()
                    .gap(px(8.))
                    .child(
                        Button::new("invite-cancel")
                            .ghost()
                            .label(common::CANCEL)
                            .on_click(|_, window, cx| window.close_dialog(cx)),
                    )
                    .child(
                        Button::new("invite-submit")
                            .primary()
                            .label(s::INVITE_CREATE)
                            .loading(busy)
                            .disabled(busy)
                            .on_click(cx.listener(|this, _, _, cx| this.submit(cx))),
                    ),
            )
            .into_any_element()
    }
}

pub fn open_invite(services: Services, window: &mut Window, cx: &mut App) -> Entity<InviteForm> {
    let form = cx.new(|cx| InviteForm::new(services, window, cx));
    let shown = form.clone();
    window.open_dialog(cx, move |dialog, _, _| {
        let dialog = crate::ui::SheetExt::sheet(dialog);
        dialog
            .w(px(460.))
            .title(SharedString::from(s::INVITE))
            .overlay_closable(false)
            .child(shown.clone())
    });
    form
}

// --- bot member ---

/// The add-bot-member dialog: name and 岗位, then either an outside
/// credential (shown once) or a runner setup.
pub struct BotForm {
    services: Services,
    name: Entity<InputState>,
    role: Entity<InputState>,
    runner: bool,
    labels: Entity<InputState>,
    runtime: Entity<InputState>,
    repo: Entity<InputState>,
    branch: Entity<InputState>,
    /// Runtime names that machines report.
    known_runtimes: Vec<String>,
    status: Status,
    created: Option<BotCreated>,
}

impl BotForm {
    pub fn new(services: Services, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let machines = services.provider.machines();
        cx.spawn(async move |this, cx| {
            if let Ok(list) = answer(machines).await {
                let mut known: Vec<String> = list.into_iter().flat_map(|m| m.runtimes).collect();
                known.sort();
                known.dedup();
                let _ = this.update(cx, |this, cx| {
                    this.known_runtimes = known;
                    cx.notify();
                });
            }
        })
        .detach();
        let input = |placeholder: &'static str, window: &mut Window, cx: &mut Context<Self>| {
            cx.new(|cx| InputState::new(window, cx).placeholder(placeholder))
        };
        Self {
            name: input(s::BOT_NAME_PLACEHOLDER, window, cx),
            role: input(s::ROLE_PLACEHOLDER, window, cx),
            labels: input(s::RUNNER_LABELS_PLACEHOLDER, window, cx),
            runtime: input(s::RUNNER_RUNTIME_PLACEHOLDER, window, cx),
            repo: input(s::RUNNER_REPO_PLACEHOLDER, window, cx),
            branch: input(s::RUNNER_BRANCH_PLACEHOLDER, window, cx),
            services,
            runner: false,
            known_runtimes: Vec::new(),
            status: Status::Idle,
            created: None,
        }
    }

    pub fn created(&self) -> Option<&BotCreated> {
        self.created.as_ref()
    }

    pub fn status(&self) -> &Status {
        &self.status
    }

    pub fn fill(
        &mut self,
        name: &str,
        runner: Option<&str>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.name
            .update(cx, |st, cx| st.set_value(name.to_string(), window, cx));
        self.runner = runner.is_some();
        if let Some(rt) = runner {
            self.runtime
                .update(cx, |st, cx| st.set_value(rt.to_string(), window, cx));
        }
    }

    fn value(&self, input: &Entity<InputState>, cx: &App) -> String {
        input.read(cx).value().trim().to_string()
    }

    pub fn submit(&mut self, cx: &mut Context<Self>) {
        if self.status.busy() || self.created.is_some() {
            return;
        }
        let name = self.value(&self.name, cx);
        let role = self.value(&self.role, cx);
        if name.is_empty() {
            self.status = Status::Failed(s::NAME_REQUIRED.into());
            cx.notify();
            return;
        }
        if let Err(e) = widgets::check_len(&name, 64).and(widgets::check_len(&role, 64)) {
            self.status = Status::Failed(e.into());
            cx.notify();
            return;
        }
        let runner = if self.runner {
            let runtime = self.value(&self.runtime, cx);
            if runtime.is_empty() {
                self.status = Status::Failed(s::RUNNER_RUNTIME_REQUIRED.into());
                cx.notify();
                return;
            }
            let repo = self.value(&self.repo, cx);
            let branch = self.value(&self.branch, cx);
            Some(RunnerSetup {
                runtime,
                labels: widgets::split_labels(&self.value(&self.labels, cx)),
                repo: (!repo.is_empty()).then_some(repo),
                base_branch: (!branch.is_empty()).then_some(branch),
            })
        } else {
            None
        };
        let reply = self.services.provider.create_bot_member(
            name,
            (!role.is_empty()).then_some(role),
            runner,
        );
        self.status = Status::Busy(s::SAVING.into());
        cx.notify();
        cx.spawn(async move |this, cx| {
            let result = answer(reply).await;
            let _ = this.update(cx, |this, cx| {
                match result {
                    Ok(created) => {
                        this.created = Some(created);
                        this.status = Status::Idle;
                    }
                    Err(e) => this.status = Status::from_error(e),
                }
                cx.notify();
            });
        })
        .detach();
    }
}

impl Render for BotForm {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if let Some(created) = &self.created {
            let name = created.member.name.clone();
            return div()
                .flex()
                .flex_col()
                .gap(px(12.))
                .child(
                    div()
                        .text_base()
                        .child(fill(s::BOT_CREATED, &[("name", &name)])),
                )
                .map(|el| match &created.credential {
                    Some(credential) => el
                        .child(widgets::hint(s::SHOWN_ONCE, cx))
                        .child(widgets::secret_block(
                            "bot-credential",
                            s::BOT_CREDENTIAL,
                            credential.expose(),
                            cx,
                        ))
                        .child(widgets::hint(s::BOT_CREDENTIAL_HINT, cx)),
                    None => el.child(widgets::hint(
                        fill(s::BOT_RUNNER_READY, &[("name", &name)]),
                        cx,
                    )),
                })
                .child(
                    div().flex().justify_end().child(
                        Button::new("bot-done")
                            .primary()
                            .label(s::DONE)
                            .on_click(|_, window, cx| window.close_dialog(cx)),
                    ),
                )
                .into_any_element();
        }
        let busy = self.status.busy();
        let page = cx.entity().downgrade();
        let runtime_input = self.runtime.clone();
        let known: Vec<_> = self
            .known_runtimes
            .iter()
            .enumerate()
            .map(|(ix, rt)| {
                let rt = rt.clone();
                let input = runtime_input.clone();
                Button::new(ElementId::NamedInteger("known-runtime".into(), ix as u64))
                    .ghost()
                    .xsmall()
                    .label(rt.clone())
                    .on_click(move |_, window, cx| {
                        input.update(cx, |st, cx| st.set_value(rt.clone(), window, cx));
                    })
            })
            .collect();
        div()
            .flex()
            .flex_col()
            .gap(px(10.))
            .child(widgets::field(s::NAME, Input::new(&self.name), cx))
            .child(widgets::field(s::ROLE, Input::new(&self.role), cx))
            .child(widgets::field(
                s::BOT_MODE,
                widgets::choice(
                    "bot-mode",
                    &[s::BOT_MODE_EXTERNAL, s::BOT_MODE_RUNNER],
                    usize::from(self.runner),
                    busy,
                    move |ix, _, cx| {
                        let _ = page.update(cx, |this, cx| {
                            this.runner = ix == 1;
                            this.status = Status::Idle;
                            cx.notify();
                        });
                    },
                ),
                cx,
            ))
            .child(widgets::hint(
                if self.runner {
                    s::BOT_MODE_RUNNER_HINT
                } else {
                    s::BOT_MODE_EXTERNAL_HINT
                },
                cx,
            ))
            .when(self.runner, |el| {
                el.child(widgets::field(
                    s::RUNNER_RUNTIME,
                    Input::new(&self.runtime),
                    cx,
                ))
                .when(!known.is_empty(), |el| {
                    el.child(
                        div()
                            .flex()
                            .flex_wrap()
                            .items_center()
                            .gap(px(4.))
                            .pl(px(100.))
                            .child(widgets::hint(s::RUNNER_RUNTIME_KNOWN, cx))
                            .children(known),
                    )
                })
                .child(widgets::field(
                    s::RUNNER_LABELS,
                    Input::new(&self.labels),
                    cx,
                ))
                .child(widgets::field(s::RUNNER_REPO, Input::new(&self.repo), cx))
                .child(widgets::field(
                    s::RUNNER_BRANCH,
                    Input::new(&self.branch),
                    cx,
                ))
            })
            .children(widgets::status_line(&self.status, cx))
            .child(
                div()
                    .flex()
                    .justify_end()
                    .gap(px(8.))
                    .child(
                        Button::new("bot-cancel")
                            .ghost()
                            .label(common::CANCEL)
                            .on_click(|_, window, cx| window.close_dialog(cx)),
                    )
                    .child(
                        Button::new("bot-submit")
                            .primary()
                            .label(s::BOT_CREATE)
                            .loading(busy)
                            .disabled(busy)
                            .on_click(cx.listener(|this, _, _, cx| this.submit(cx))),
                    ),
            )
            .into_any_element()
    }
}

pub fn open_add_bot(services: Services, window: &mut Window, cx: &mut App) -> Entity<BotForm> {
    let form = cx.new(|cx| BotForm::new(services, window, cx));
    let shown = form.clone();
    window.open_dialog(cx, move |dialog, _, _| {
        let dialog = crate::ui::SheetExt::sheet(dialog);
        dialog
            .w(px(500.))
            .title(SharedString::from(s::ADD_BOT_MEMBER))
            .overlay_closable(false)
            .child(shown.clone())
    });
    form
}

#[cfg(test)]
mod tests {
    use super::*;

    fn m(id: &str, name: &str, bot: bool, off: bool) -> Member {
        Member {
            id: id.into(),
            kind: if bot {
                MemberKind::Bot
            } else {
                MemberKind::Human
            },
            name: name.into(),
            role: None,
            level: MemberLevel::Member,
            avatar_url: None,
            deactivated: off,
            online: false,
            character: None,
        }
    }

    #[test]
    fn order_people_bots_then_deactivated() {
        let list = sorted(vec![
            m("1", "Zed", false, true),
            m("2", "bot b", true, false),
            m("3", "amy", false, false),
            m("4", "Bob", false, false),
        ]);
        let ids: Vec<_> = list.iter().map(|m| m.id.as_str()).collect();
        assert_eq!(ids, vec!["3", "4", "2", "1"]);
    }
}
