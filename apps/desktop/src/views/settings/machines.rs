//! 机器: the machines that run bot members' work, with their health. Owners
//! add one: its credential and install command are shown once.

use std::time::Duration;

use gpui::{
    App, AppContext as _, Context, Entity, IntoElement, ParentElement, Render, SharedString,
    Styled, Task, Window, div, prelude::FluentBuilder as _, px,
};
use gpui_component::{
    Disableable as _, Icon, Sizable as _, WindowExt as _,
    button::{Button, ButtonVariants as _},
    input::{Input, InputState},
    spinner::Spinner,
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
    theme::{self, Type as _, space},
    ui::{self, Lucide},
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

    /// One machine as a row of the grouped list: health, name, facts and labels.
    fn row(&self, m: &MachineInfo, cx: &App) -> gpui::Div {
        let colors = theme::colors(cx);
        let now = chrono::Local::now();
        let (state_icon, state_text): (gpui::AnyElement, &'static str) = if !m.connected {
            (presence_dot(false, cx).into_any_element(), s::MACHINE_OFFLINE)
        } else if !m.healthy {
            (
                ui::icon(Lucide::TriangleAlert, theme::icon::SMALL, colors.warning)
                    .into_any_element(),
                s::MACHINE_UNHEALTHY,
            )
        } else {
            (presence_dot(true, cx).into_any_element(), s::MACHINE_ONLINE)
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
        facts.push(if m.active_runs > 0 {
            fill(s::MACHINE_BUSY, &[("n", &m.active_runs.to_string())])
        } else {
            s::MACHINE_IDLE.to_string()
        });
        match m.last_heartbeat_at {
            _ if m.connected => {}
            Some(at) => facts.push(fill(s::MACHINE_SEEN, &[("when", &relative_time(at, now))])),
            None => facts.push(s::MACHINE_NEVER_SEEN.to_string()),
        }
        widgets::group_row()
            .items_start()
            .child(
                div()
                    .flex_none()
                    .size(px(28.))
                    .rounded(theme::radius::MD)
                    .bg(colors.fill_field)
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(ui::icon(Lucide::Monitor, theme::icon::TOOLBAR, colors.text_2)),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .gap(space::S2)
                    .child(div().t_ui().medium().one_line().child(m.name.clone()))
                    .child(
                        div()
                            .t_callout()
                            .tnum()
                            .text_color(colors.text_2)
                            .child(facts.join(" · ")),
                    )
                    .when(!m.labels.is_empty(), |el| {
                        el.child(
                            div()
                                .pt(space::S4)
                                .flex()
                                .flex_wrap()
                                .gap(space::S4)
                                .children(
                                    m.labels
                                        .iter()
                                        .map(|l| Tag::secondary().small().child(l.clone())),
                                ),
                        )
                    }),
            )
            .child(
                div()
                    .w(px(92.))
                    .h(px(18.))
                    .flex_none()
                    .flex()
                    .items_center()
                    .gap(space::S6)
                    .t_callout()
                    .text_color(colors.text_2)
                    .child(state_icon)
                    .child(state_text),
            )
    }

    /// The header buttons: 刷新 and 添加机器.
    pub fn header_actions(this: &Entity<Self>, cx: &App) -> gpui::AnyElement {
        let page = this.read(cx);
        let owner = i_am_owner(&page.services);
        let services = page.services.clone();
        let busy = page.status.busy();
        let weak = this.downgrade();
        let weak2 = weak.clone();
        div()
            .flex()
            .items_center()
            .gap(space::S8)
            .child(
                Button::new("machines-refresh")
                    .ghost()
                    .small()
                    .icon(Icon::new(Lucide::RotateCcw))
                    .label(s::REFRESH)
                    .loading(busy)
                    .on_click(move |_, _, cx| {
                        let _ = weak.update(cx, |this, cx| this.refresh(cx));
                    }),
            )
            .child(
                Button::new("machine-add")
                    .primary()
                    .small()
                    .icon(Icon::new(Lucide::Plus))
                    .label(s::MACHINE_ADD)
                    .disabled(!owner)
                    .on_click(move |_, window, cx| {
                        let form = open_add_machine(services.clone(), window, cx);
                        let page = weak2.clone();
                        // Reload the list once the machine exists.
                        cx.observe(&form, move |form, cx| {
                            if form.read(cx).created().is_some() {
                                let _ = page.update(cx, |p, cx| p.refresh(cx));
                            }
                        })
                        .detach();
                    }),
            )
            .into_any_element()
    }
}

impl Render for MachinesPage {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let colors = theme::colors(cx);
        let failed = match &self.status {
            Status::Failed(text) => Some(text.clone()),
            _ => None,
        };
        let body = match (&self.machines, failed.clone()) {
            // The first load failed: an error row with 重试 (§11 Error loading).
            (None, Some(text)) => widgets::group(cx).child(
                widgets::group_row()
                    .child(ui::icon(
                        Lucide::CircleAlert,
                        theme::icon::SMALL,
                        colors.danger,
                    ))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .t_callout()
                            .text_color(colors.text_1)
                            .child(text),
                    )
                    .child(
                        Button::new("machines-retry")
                            .small()
                            .label(s::RETRY)
                            .on_click(cx.listener(|this, _, _, cx| this.refresh(cx))),
                    ),
            ),
            (None, None) => widgets::group(cx).child(
                widgets::group_row()
                    .justify_center()
                    .child(Spinner::new().small().color(colors.text_3)),
            ),
            (Some(list), _) if list.is_empty() => div().pt(px(96.)).child(
                ui::EmptyState::new(Lucide::Server, s::MACHINES_EMPTY)
                    .description(s::MACHINES_EMPTY_HINT),
            ),
            (Some(list), _) => {
                let rows = list.iter().map(|m| self.row(m, cx)).collect();
                widgets::group_rows(rows, px(50.), cx)
            }
        };
        div()
            .flex()
            .flex_col()
            .when(self.machines.is_some(), |el| {
                el.children(failed.map(|text| {
                    widgets::status_line(&Status::Failed(text), cx)
                        .map(|l| l.px(space::S12).pb(space::S8))
                }).flatten())
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
