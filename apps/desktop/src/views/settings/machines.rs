//! 机器: the machines that run bot members' work, with their health. Owners
//! add one: its credential and install command are shown once.

use std::time::Duration;

use gpui::{
    App, AppContext as _, Context, Entity, IntoElement, ParentElement, Render, SharedString,
    Styled, Task, Window, div, prelude::FluentBuilder as _, px,
};
use gpui_component::{Disableable as _, Icon, WindowExt as _, input::InputState};

use super::{
    i_am_owner,
    widgets::{self, Status, answer},
};
use crate::{
    app::Services,
    copy::{common, fill, settings as s},
    data::{MachineCreated, MachineInfo},
    theme::{self, Type as _, space},
    ui::{self, ButtonSpec as _, Lucide},
    views::topic_list::relative_time,
};

/// How often the open page reloads the list.
const REFRESH_EVERY: Duration = Duration::from_secs(15);

pub struct MachinesPage {
    services: Services,
    machines: Option<Vec<MachineInfo>>,
    status: Status,
    /// True once a load has run for the busy delay (DESIGN.md §12: busy
    /// indicators wait 400 ms), so a fast load never flashes a skeleton.
    busy_shown: bool,
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
            busy_shown: false,
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
        self.busy_shown = false;
        let reply = self.services.provider.machines();
        cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(theme::motion::BUSY_DELAY)
                .await;
            let _ = this.update(cx, |this, cx| {
                if this.status.busy() {
                    this.busy_shown = true;
                    cx.notify();
                }
            });
        })
        .detach();
        cx.spawn(async move |this, cx| {
            let result = answer(reply).await;
            let _ = this.update(cx, |this, cx| {
                this.busy_shown = false;
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

    /// One machine as a table row: a 36 pt tile with the machine glyph, the
    /// name with its facts and labels, and the status column (U43).
    fn row(&self, m: &MachineInfo, cx: &App) -> gpui::Div {
        let colors = theme::colors(cx);
        let now = chrono::Local::now();
        let (dot, state_text) = if !m.connected {
            (None, s::MACHINE_OFFLINE)
        } else if !m.healthy {
            (Some(colors.you_glyph), s::MACHINE_UNHEALTHY)
        } else {
            (Some(colors.online), s::MACHINE_ONLINE)
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
        let activity = if m.active_runs > 0 {
            fill(s::MACHINE_BUSY, &[("n", &m.active_runs.to_string())])
        } else {
            s::MACHINE_IDLE.to_string()
        };
        let seen = match m.last_heartbeat_at {
            _ if m.connected => None,
            Some(at) => Some(fill(s::MACHINE_SEEN, &[("when", &relative_time(at, now))])),
            None => Some(s::MACHINE_NEVER_SEEN.to_string()),
        };
        widgets::table_row()
            .items_center()
            .child(
                div()
                    .flex_none()
                    .size(theme::size::AVATAR_MESSAGE)
                    .rounded(theme::radius::MD)
                    .bg(colors.field)
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(ui::icon(
                        Lucide::Monitor,
                        theme::icon::HEADER,
                        colors.text_2,
                    )),
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
                            .child(
                                div()
                                    .min_w_0()
                                    .t_ui()
                                    .medium()
                                    .one_line()
                                    .child(m.name.clone()),
                            )
                            .children(m.labels.iter().take(3).map(|l| {
                                div()
                                    .flex_none()
                                    .h(theme::size::CHIP_COMPACT)
                                    .px(space::S6)
                                    .rounded(theme::radius::SM)
                                    .bg(colors.field)
                                    .flex()
                                    .items_center()
                                    .t_meta()
                                    .text_color(colors.text_2)
                                    .child(l.clone())
                            })),
                    )
                    .child(
                        div()
                            .t_small()
                            .tnum()
                            .text_color(colors.text_2)
                            .one_line()
                            .child(facts.join(" · ")),
                    ),
            )
            .child(widgets::status_cell(
                widgets::state_dot(dot, cx),
                state_text,
                Some(seen.unwrap_or(activity).into()),
                cx,
            ))
    }

    /// The page actions: 刷新 (icon button) and 添加机器 (36 pt page primary).
    pub fn header_actions(this: &Entity<Self>, cx: &App) -> gpui::AnyElement {
        let page = this.read(cx);
        let owner = i_am_owner(&page.services);
        let services = page.services.clone();
        let busy = page.busy_shown;
        let weak = this.downgrade();
        let weak2 = weak.clone();
        div()
            .flex()
            .items_center()
            .gap(space::S8)
            .child(
                ui::icon_button("machines-refresh", Lucide::RotateCcw)
                    .tooltip(s::REFRESH)
                    .loading(busy)
                    .on_click(move |_, _, cx| {
                        let _ = weak.update(cx, |this, cx| this.refresh(cx));
                    }),
            )
            .child(
                ui::button(
                    "machine-add",
                    s::MACHINE_ADD,
                    ui::ButtonKind::Primary,
                    ui::ButtonSize::Page,
                    cx,
                )
                .icon(Icon::new(Lucide::Plus))
                .disabled_fill(!owner, cx)
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
        let failed = match &self.status {
            Status::Failed(text) => Some(text.clone()),
            _ => None,
        };
        let header = |n: usize, cx: &App| {
            widgets::table_header(cx)
                .gap(space::S12)
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .tnum()
                        .child(fill(s::MACHINES_HEADER, &[("n", &n.to_string())])),
                )
                .child(div().w(widgets::STATUS_COL).flex_none().child(s::STATUS))
        };
        let body = match (&self.machines, failed.clone()) {
            // The first load failed: an inline error row with 重试 (§8.5).
            (None, Some(text)) => widgets::error_row(
                text,
                ui::button(
                    "machines-retry",
                    s::RETRY,
                    ui::ButtonKind::Secondary,
                    ui::ButtonSize::Sm,
                    cx,
                )
                .on_click(cx.listener(|this, _, _, cx| this.refresh(cx))),
                cx,
            ),
            // Skeleton rows while the first load runs.
            (None, None) if self.busy_shown => widgets::skeleton_table(3, cx),
            (None, None) => div(),
            (Some(list), _) if list.is_empty() => div().pt(px(72.)).child(
                ui::EmptyState::new(Lucide::Monitor, s::MACHINES_EMPTY)
                    .description(s::MACHINES_EMPTY_HINT),
            ),
            (Some(list), _) => {
                let rows = list.iter().map(|m| self.row(m, cx)).collect();
                widgets::table_of(Some(header(list.len(), cx)), rows, cx)
            }
        };
        div()
            .flex()
            .flex_col()
            .when(self.machines.is_some(), |el| {
                el.children(failed.and_then(|text| {
                    widgets::status_line(&Status::Failed(text), cx).map(|l| l.pb(space::S8))
                }))
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
                .pt(space::S8)
                .gap(space::S16)
                .child(
                    div()
                        .t_body()
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
                    div().pt(space::S4).flex().justify_end().child(
                        ui::primary_button("machine-done", s::DONE, cx)
                            .on_click(|_, window, cx| window.close_dialog(cx)),
                    ),
                )
                .into_any_element();
        }
        let busy = self.status.busy();
        div()
            .flex()
            .flex_col()
            .pt(space::S8)
            .gap(space::S16)
            .child(widgets::form_input(
                "machine-name",
                s::MACHINE_NAME,
                &self.name,
                cx,
            ))
            .child(widgets::form_input(
                "machine-labels",
                s::MACHINE_LABELS,
                &self.labels,
                cx,
            ))
            .children(widgets::status_line(&self.status, cx))
            .child(
                div()
                    .flex()
                    .justify_end()
                    .pt(space::S4)
                    .gap(space::S8)
                    .child(
                        ui::secondary_button("machine-cancel", common::CANCEL, cx)
                            .on_click(|_, window, cx| window.close_dialog(cx)),
                    )
                    .child(
                        ui::primary_button("machine-submit", s::MACHINE_CREATE, cx)
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
            .w(theme::size::DIALOG_WIDTH)
            .title(SharedString::from(s::MACHINE_ADD))
            .overlay_closable(false)
            .close_button(false)
            .child(shown.clone())
    });
    form
}
