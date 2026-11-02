//! 机器: the machines that run bot members' work, with their health. Owners
//! add one: its credential and install command are shown once.

use std::time::Duration;

use gpui::{
    App, AppContext as _, Context, Entity, IntoElement, ParentElement, Render, SharedString,
    Styled, Task, Window, div, prelude::FluentBuilder as _, px,
};
use gpui_component::{
    Disableable as _, Sizable as _, WindowExt as _,
    button::{Button, ButtonVariants as _},
    input::{Input, InputState},
    tag::Tag,
};

use super::{
    i_am_owner,
    widgets::{self, Status, answer},
};
use crate::{
    app::Services,
    copy::{common, fill, settings as s},
    data::{MachineCreated, MachineInfo},
    theme,
    views::{presence_dot, topic_list::relative_time},
};

/// How often the open page reloads the list.
const REFRESH_EVERY: Duration = Duration::from_secs(15);

pub struct MachinesPage {
    services: Services,
    machines: Option<Vec<MachineInfo>>,
    status: Status,
    /// The periodic reload; dropped with the page.
    _poll: Task<()>,
}

impl MachinesPage {
    pub fn new(services: Services, _window: &mut Window, cx: &mut Context<Self>) -> Self {
        let poll = cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(REFRESH_EVERY).await;
                let Ok(()) = this.update(cx, |this, cx| {
                    // Only while the window is open and has loaded once.
                    if this.machines.is_some() {
                        this.refresh(cx);
                    }
                }) else {
                    break;
                };
            }
        });
        Self {
            services,
            machines: None,
            status: Status::Idle,
            _poll: poll,
        }
    }

    pub fn machines(&self) -> Option<&[MachineInfo]> {
        self.machines.as_deref()
    }

    pub fn status(&self) -> &Status {
        &self.status
    }

    pub fn refresh(&mut self, cx: &mut Context<Self>) {
        if self.status.busy() {
            return;
        }
        self.status = Status::Busy(s::LOADING.into());
        let reply = self.services.provider.machines();
        cx.spawn(async move |this, cx| {
            let result = answer(reply).await;
            let _ = this.update(cx, |this, cx| {
                match result {
                    Ok(list) => {
                        this.machines = Some(list);
                        this.status = Status::Idle;
                    }
                    Err(e) => this.status = Status::from_error(e),
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    fn card(&self, m: &MachineInfo, cx: &App) -> gpui::Div {
        let colors = theme::colors(cx);
        let now = chrono::Local::now();
        let state = if !m.connected {
            Tag::secondary().small().child(s::MACHINE_OFFLINE)
        } else if !m.healthy {
            Tag::warning().small().child(s::MACHINE_UNHEALTHY)
        } else {
            Tag::success().small().child(s::MACHINE_ONLINE)
        };
        let mut facts: Vec<String> = Vec::new();
        if let (Some(free), Some(total)) = (m.disk_free_bytes, m.disk_total_bytes) {
            facts.push(fill(
                s::MACHINE_DISK,
                &[
                    ("free", &widgets::size_text(free)),
                    ("total", &widgets::size_text(total)),
                ],
            ));
        }
        if let Some(v) = &m.version {
            facts.push(fill(s::MACHINE_VERSION, &[("v", v)]));
        }
        let work = if m.active_runs > 0 {
            fill(s::MACHINE_BUSY, &[("n", &m.active_runs.to_string())])
        } else {
            s::MACHINE_IDLE.to_string()
        };
        let seen = match m.last_heartbeat_at {
            _ if m.connected => None,
            Some(at) => Some(fill(s::MACHINE_SEEN, &[("when", &relative_time(at, now))])),
            None => Some(s::MACHINE_NEVER_SEEN.to_string()),
        };
        div()
            .flex()
            .flex_col()
            .gap(px(6.))
            .p(px(12.))
            .rounded(px(8.))
            .border_1()
            .border_color(colors.border)
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .child(presence_dot(m.connected && m.healthy, cx))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .text_base()
                            .child(m.name.clone()),
                    )
                    .child(state),
            )
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .gap(px(12.))
                    .text_xs()
                    .text_color(colors.muted_foreground)
                    .children(facts)
                    .child(work)
                    .children(seen),
            )
            .when(!m.labels.is_empty(), |el| {
                el.child(
                    div()
                        .flex()
                        .flex_wrap()
                        .gap(px(4.))
                        .children(m.labels.iter().map(|l| Tag::new().small().child(l.clone()))),
                )
            })
    }
}

impl Render for MachinesPage {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let owner = i_am_owner(&self.services);
        let services = self.services.clone();
        let page = cx.entity().downgrade();
        let body = match &self.machines {
            None => div().child(widgets::hint(s::LOADING, cx)),
            Some(list) if list.is_empty() => div()
                .flex()
                .flex_col()
                .items_center()
                .gap(px(6.))
                .py(px(40.))
                .child(div().text_base().child(s::MACHINES_EMPTY))
                .child(widgets::hint(s::MACHINES_EMPTY_HINT, cx)),
            Some(list) => div()
                .flex()
                .flex_col()
                .gap(px(10.))
                .children(list.iter().map(|m| self.card(m, cx))),
        };
        div()
            .flex()
            .flex_col()
            .child(
                div()
                    .flex()
                    .items_start()
                    .justify_between()
                    .child(widgets::page_title(s::MACHINES, cx))
                    .child(
                        div()
                            .flex()
                            .gap(px(8.))
                            .child(
                                Button::new("machines-refresh")
                                    .ghost()
                                    .small()
                                    .label(s::REFRESH)
                                    .loading(self.status.busy())
                                    .on_click(cx.listener(|this, _, _, cx| this.refresh(cx))),
                            )
                            .child(
                                Button::new("machine-add")
                                    .primary()
                                    .small()
                                    .label(s::MACHINE_ADD)
                                    .disabled(!owner)
                                    .on_click(move |_, window, cx| {
                                        let form = open_add_machine(services.clone(), window, cx);
                                        let page = page.clone();
                                        // Reload the list once the machine exists.
                                        cx.observe(&form, move |form, cx| {
                                            if form.read(cx).created().is_some() {
                                                let _ = page.update(cx, |p, cx| p.refresh(cx));
                                            }
                                        })
                                        .detach();
                                    }),
                            ),
                    ),
            )
            .when(matches!(self.status, Status::Failed(_)), |el| {
                el.children(widgets::status_line(&self.status, cx))
            })
            .child(body)
    }
}

// --- add machine ---

pub struct MachineForm {
    services: Services,
    name: Entity<InputState>,
    labels: Entity<InputState>,
    status: Status,
    created: Option<MachineCreated>,
}

impl MachineForm {
    pub fn new(services: Services, window: &mut Window, cx: &mut Context<Self>) -> Self {
        Self {
            services,
            name: cx.new(|cx| InputState::new(window, cx).placeholder(s::MACHINE_NAME_PLACEHOLDER)),
            labels: cx
                .new(|cx| InputState::new(window, cx).placeholder(s::RUNNER_LABELS_PLACEHOLDER)),
            status: Status::Idle,
            created: None,
        }
    }

    pub fn created(&self) -> Option<&MachineCreated> {
        self.created.as_ref()
    }

    pub fn status(&self) -> &Status {
        &self.status
    }

    pub fn fill(&mut self, name: &str, labels: &str, window: &mut Window, cx: &mut Context<Self>) {
        self.name
            .update(cx, |st, cx| st.set_value(name.to_string(), window, cx));
        self.labels
            .update(cx, |st, cx| st.set_value(labels.to_string(), window, cx));
    }

    pub fn submit(&mut self, cx: &mut Context<Self>) {
        if self.status.busy() || self.created.is_some() {
            return;
        }
        let name = self.name.read(cx).value().trim().to_string();
        if name.is_empty() {
            self.status = Status::Failed(s::NAME_REQUIRED.into());
            cx.notify();
            return;
        }
        if let Err(e) = widgets::check_len(&name, 64) {
            self.status = Status::Failed(e.into());
            cx.notify();
            return;
        }
        let labels = widgets::split_labels(&self.labels.read(cx).value());
        let reply = self.services.provider.create_machine(name, labels);
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

impl Render for MachineForm {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if let Some(created) = &self.created {
            return div()
                .flex()
                .flex_col()
                .gap(px(12.))
                .child(
                    div()
                        .text_base()
                        .child(fill(s::MACHINE_CREATED, &[("name", &created.machine.name)])),
                )
                .child(widgets::hint(s::SHOWN_ONCE, cx))
                .child(widgets::secret_block(
                    "machine-install",
                    s::MACHINE_INSTALL,
                    created.install_command.expose(),
                    cx,
                ))
                .child(widgets::hint(s::MACHINE_INSTALL_HINT, cx))
                .child(widgets::secret_block(
                    "machine-credential",
                    s::MACHINE_CREDENTIAL,
                    created.credential.expose(),
                    cx,
                ))
                .child(
                    div().flex().justify_end().child(
                        Button::new("machine-done")
                            .primary()
                            .label(s::DONE)
                            .on_click(|_, window, cx| window.close_dialog(cx)),
                    ),
                )
                .into_any_element();
        }
        let busy = self.status.busy();
        div()
            .flex()
            .flex_col()
            .gap(px(10.))
            .child(widgets::field(s::MACHINE_NAME, Input::new(&self.name), cx))
            .child(widgets::field(
                s::MACHINE_LABELS,
                Input::new(&self.labels),
                cx,
            ))
            .children(widgets::status_line(&self.status, cx))
            .child(
                div()
                    .flex()
                    .justify_end()
                    .gap(px(8.))
                    .child(
                        Button::new("machine-cancel")
                            .ghost()
                            .label(common::CANCEL)
                            .on_click(|_, window, cx| window.close_dialog(cx)),
                    )
                    .child(
                        Button::new("machine-submit")
                            .primary()
                            .label(s::MACHINE_CREATE)
                            .loading(busy)
                            .disabled(busy)
                            .on_click(cx.listener(|this, _, _, cx| this.submit(cx))),
                    ),
            )
            .into_any_element()
    }
}

pub fn open_add_machine(
    services: Services,
    window: &mut Window,
    cx: &mut App,
) -> Entity<MachineForm> {
    let form = cx.new(|cx| MachineForm::new(services, window, cx));
    let shown = form.clone();
    window.open_dialog(cx, move |dialog, _, _| {
        let dialog = crate::ui::SheetExt::sheet(dialog);
        dialog
            .w(px(560.))
            .title(SharedString::from(s::MACHINE_ADD))
            .overlay_closable(false)
            .child(shown.clone())
    });
    form
}
