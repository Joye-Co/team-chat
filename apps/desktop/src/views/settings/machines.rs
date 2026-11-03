//! 机器: the machines that run bot members' work, with their health. Owners
//! add one: its credential and install command are shown once.

use std::time::Duration;

use gpui::{
    App, AppContext as _, Context, Entity, InteractiveElement as _, IntoElement, ParentElement,
    Render, SharedString, StatefulInteractiveElement as _, Styled, Task, Window, div,
    prelude::FluentBuilder as _, px,
};
use gpui_component::{Disableable as _, Icon, WindowExt as _, input::InputState, tooltip::Tooltip};

use super::{
    i_am_owner,
    widgets::{self, Status, answer},
};
use crate::ui::NamedButton as _;
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
        let now = crate::selftest::clock::local_now();
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
        let tags: Vec<String> = m.labels.iter().map(|l| tag_text(l)).collect();
        let (shown, more) = shown_tags(&tags);
        let shown: Vec<String> = shown.to_vec();
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
                        machine_glyph(m),
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
                            // Host names differ at the end: cut in the
                            // middle, the full name in a tooltip (§15).
                            .child(
                                ui::truncating(
                                    gpui::ElementId::Name(format!("machine-name-{}", m.id).into()),
                                    m.name.clone(),
                                )
                                .text_ellipsis_middle()
                                .t_ui()
                                .medium(),
                            )
                            .children(shown.into_iter().map(|t| tag_chip(t, cx)))
                            .when(more > 0, |el| {
                                let all: SharedString = tags.join("、").into();
                                el.child(
                                    tag_chip(
                                        fill(s::MACHINE_TAGS_MORE, &[("n", &more.to_string())]),
                                        cx,
                                    )
                                    .id(gpui::ElementId::Name(
                                        format!("machine-tags-{}", m.id).into(),
                                    ))
                                    .tooltip(
                                        move |window, cx| {
                                            Tooltip::new(all.clone()).build(window, cx)
                                        },
                                    ),
                                )
                            }),
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
            // Status centered in its column (U43).
            .child(
                widgets::status_cell(
                    widgets::state_dot(dot, cx),
                    state_text,
                    Some(seen.unwrap_or(activity).into()),
                    cx,
                )
                .justify_center(),
            )
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
                    .named(s::REFRESH)
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
                .child(
                    div()
                        .w(widgets::STATUS_COL)
                        .flex_none()
                        .flex()
                        .justify_center()
                        .child(s::STATUS),
                )
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
                    widgets::status_line("load-status", &Status::Failed(text), cx)
                        .map(|l| l.pb(space::S8))
                }))
            })
            .child(body)
    }
}

/// At most this many tags show on a row; the rest fold into `+N`.
const TAGS_SHOWN: usize = 3;

/// The tags a row shows and how many fold into `+N` (break-ui: a tag past
/// the third is never dropped without a sign).
fn shown_tags(tags: &[String]) -> (&[String], usize) {
    let n = tags.len().min(TAGS_SHOWN);
    (&tags[..n], tags.len() - n)
}

/// A machine tag as people read it: the common routing tags in plain words,
/// any other tag as its owner typed it.
fn tag_text(label: &str) -> String {
    let t = label.trim();
    match t.to_ascii_lowercase().as_str() {
        "mac" | "macos" | "darwin" | "osx" => s::MACHINE_TAG_MAC.into(),
        "linux" => s::MACHINE_TAG_LINUX.into(),
        "windows" | "win" => s::MACHINE_TAG_WINDOWS.into(),
        "office" => s::MACHINE_TAG_OFFICE.into(),
        "home" => s::MACHINE_TAG_HOME.into(),
        "gpu" => s::MACHINE_TAG_GPU.into(),
        "ios-simulator" | "ios-sim" | "simulator" => s::MACHINE_TAG_IOS_SIMULATOR.into(),
        _ => t.to_string(),
    }
}

/// The glyph of a machine by its kind, read from its name and tags: a
/// notebook gets `laptop`, a server `server`, anything else (a Mac mini, a
/// desktop) `monitor`.
fn machine_glyph(m: &MachineInfo) -> Lucide {
    let words: Vec<String> = std::iter::once(m.name.to_lowercase())
        .chain(m.labels.iter().map(|l| l.to_lowercase()))
        .collect();
    let has = |keys: &[&str]| words.iter().any(|w| keys.iter().any(|k| w.contains(k)));
    if has(&["laptop", "macbook", "notebook", "笔记本"]) {
        Lucide::Laptop
    } else if has(&["server", "服务器", "linux", "vps", "cloud", "ubuntu"]) {
        Lucide::Server
    } else {
        Lucide::Monitor
    }
}

/// One neutral tag chip.
fn tag_chip(text: impl Into<SharedString>, cx: &App) -> gpui::Div {
    let colors = theme::colors(cx);
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
        .child(text.into())
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
            .children(widgets::status_line("status", &self.status, cx))
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
    // The first field takes the focus once, as in the new-群 dialog.
    let first = std::cell::Cell::new(true);
    window.open_dialog(cx, move |dialog, window, cx| {
        let dialog = crate::ui::SheetExt::sheet(dialog, cx);
        if first.replace(false) {
            let handle = gpui::Focusable::focus_handle(shown.read(cx).name.read(cx), cx);
            handle.focus(window, cx);
        }
        dialog
            .w(theme::size::DIALOG_WIDTH)
            .title(SharedString::from(s::MACHINE_ADD))
            .overlay_closable(false)
            .close_button(false)
            .child(shown.clone())
    });
    form
}

#[cfg(test)]
mod tests {
    use super::*;

    fn machine(name: &str, labels: &[&str]) -> MachineInfo {
        MachineInfo {
            id: "m".into(),
            name: name.into(),
            labels: labels.iter().map(|l| l.to_string()).collect(),
            connected: true,
            healthy: true,
            last_heartbeat_at: None,
            version: None,
            disk_free_bytes: None,
            disk_total_bytes: None,
            active_runs: 0,
            max_parallel_runs: None,
            runtimes: Vec::new(),
        }
    }

    #[test]
    fn a_machine_gets_the_glyph_of_its_kind() {
        assert_eq!(
            machine_glyph(&machine("办公室 Mac mini", &["mac"])),
            Lucide::Monitor
        );
        assert_eq!(
            machine_glyph(&machine("旧笔记本", &["mac"])),
            Lucide::Laptop
        );
        assert_eq!(
            machine_glyph(&machine("joye-macbook-pro-m4-max", &[])),
            Lucide::Laptop
        );
        assert_eq!(
            machine_glyph(&machine("build-01", &["linux"])),
            Lucide::Server
        );
    }

    #[test]
    fn tags_read_as_words() {
        assert_eq!(tag_text("mac"), "macOS");
        assert_eq!(tag_text(" Office "), "办公室");
        assert_eq!(tag_text("ios-simulator"), "iOS 模拟器");
        // A tag the owner made up stays as typed.
        assert_eq!(tag_text("xcode-16"), "xcode-16");
    }

    #[test]
    fn tags_past_the_third_fold_into_a_count() {
        let tags: Vec<String> = ["a", "b", "c", "d", "e", "f", "g"]
            .iter()
            .map(|t| t.to_string())
            .collect();
        let (shown, more) = shown_tags(&tags);
        assert_eq!(shown.len(), 3);
        assert_eq!(more, 4);
        let (shown, more) = shown_tags(&tags[..2]);
        assert_eq!((shown.len(), more), (2, 0));
    }
}
