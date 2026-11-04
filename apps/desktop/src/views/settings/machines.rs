//! 机器 (NATIVE.md §8): where AI members work. Group 团队的电脑 lists the
//! team's computers (symbol, name, state dot and text, version and what it
//! is doing) with the `+`/`−` pair under it (`+` opens the add sheet; its
//! install command and credential are shown once). Group 云端 says that the
//! cloud is not open yet. Members do not run only on the team's computers.

use std::time::Duration;

use gpui::{
    App, AppContext as _, Context, Entity, Focusable as _, IntoElement, ParentElement, Render,
    Styled, Task, Window, div, prelude::FluentBuilder as _, px,
};
use gpui_component::{Disableable as _, WindowExt as _, input::InputState};

use super::{
    i_am_owner,
    widgets::{self, Status, answer},
};
use crate::{
    app::Services,
    copy::{common, fill, settings as s},
    data::{MachineCreated, MachineInfo},
    theme,
    ui::{
        self, Sym, SymSize,
        native::{
            FormRow, GroupedForm, NativeTextField, NativeType as _, PushSize, PushStyle,
            SheetLayout, SheetWidth, disabled_push, metrics, native_colors, push_button,
            square_button, square_pair,
        },
    },
    views::topic_list::relative_time,
};

/// How often the open pane reloads the list.
const REFRESH_EVERY: Duration = Duration::from_secs(15);

pub struct MachinesPage {
    services: Services,
    machines: Option<Vec<MachineInfo>>,
    status: Status,
    /// The periodic reload; dropped with the pane.
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

    /// One computer: its symbol, the name (cut in the middle: host names
    /// differ at the end), version, tags and what it does; the state dot.
    fn row(&self, m: &MachineInfo, cx: &App) -> FormRow {
        let k = native_colors(cx);
        let now = crate::selftest::clock::local_now();
        let (dot, state_text) = if !m.connected {
            (None, s::MACHINE_OFFLINE)
        } else if !m.healthy {
            (Some(k.amber), s::MACHINE_UNHEALTHY)
        } else {
            (Some(k.green), s::MACHINE_ONLINE)
        };
        let mut facts: Vec<String> = Vec::new();
        if let Some(v) = &m.version {
            facts.push(fill(s::MACHINE_VERSION, &[("v", v)]));
        }
        let tags: Vec<String> = m.labels.iter().map(|l| tag_text(l)).collect();
        if !tags.is_empty() {
            facts.push(tags.join("、"));
        }
        facts.push(match m.last_heartbeat_at {
            _ if m.connected && m.active_runs > 0 => {
                fill(s::MACHINE_BUSY, &[("n", &m.active_runs.to_string())])
            }
            _ if m.connected => s::MACHINE_IDLE.to_string(),
            Some(at) => fill(s::MACHINE_SEEN, &[("when", &relative_time(at, now))]),
            None => s::MACHINE_NEVER_SEEN.to_string(),
        });
        FormRow::new(m.name.clone())
            .detail(facts.join(" · "))
            .leading(ui::sym(machine_glyph(m), SymSize::SIDEBAR, k.secondary))
            .control(widgets::dot_text(dot, state_text, cx))
            .min_height(px(44.))
    }

    fn open_add(this: Entity<Self>, window: &mut Window, cx: &mut App) {
        let services = this.read(cx).services.clone();
        let form = open_add_machine(services, window, cx);
        // Reload the list once the computer exists.
        cx.observe(&form, move |form, cx| {
            if form.read(cx).created().is_some() {
                this.update(cx, |p, cx| p.refresh(cx));
            }
        })
        .detach();
    }
}

impl Render for MachinesPage {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let k = native_colors(cx);
        let owner = i_am_owner(&self.services);
        let note = |text: gpui::SharedString| {
            div()
                .h(metrics::FORM_ROW)
                .px(metrics::FORM_ROW_INSET)
                .flex()
                .items_center()
                .n_body()
                .text_color(k.secondary)
                .child(text)
                .into_any_element()
        };
        let rows: Vec<gpui::AnyElement> = match (&self.machines, &self.status) {
            (Some(list), _) if !list.is_empty() => list
                .iter()
                .map(|m| self.row(m, cx).into_any_element())
                .collect(),
            (Some(_), _) => vec![note(s::NO_COMPUTERS.into())],
            (None, Status::Failed(t)) => vec![
                FormRow::new(t.clone())
                    .control(
                        push_button(
                            "machines-retry",
                            s::RETRY,
                            PushStyle::Standard,
                            PushSize::Small,
                            cx,
                        )
                        .on_click(cx.listener(|this, _, _, cx| this.refresh(cx))),
                    )
                    .into_any_element(),
            ],
            (None, _) => vec![
                div()
                    .h(metrics::FORM_ROW)
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(ui::native::spinner())
                    .into_any_element(),
            ],
        };
        let page = cx.entity();
        let add = disabled_push(
            square_button("machine-add", true, s::COMPUTER_ADD, window, cx),
            !owner,
            cx,
        )
        .bg(gpui::transparent_black())
        .on_click(move |_, window, cx| Self::open_add(page.clone(), window, cx));
        // No computer can be removed from here yet: `−` stays dimmed.
        let remove = square_button("machine-remove", false, s::COMPUTER_REMOVE, window, cx)
            .disabled(true)
            .opacity(0.4);
        let computers = GroupedForm::new()
            .header(s::TEAM_COMPUTERS)
            .rows(rows)
            .below(widgets::list_controls(
                square_pair(add, remove, cx),
                s::TEAM_COMPUTERS_FOOTER,
                cx,
            ));
        let cloud = GroupedForm::new()
            .row(
                FormRow::new(s::CLOUD)
                    .leading(ui::sym(Sym::Cloud, SymSize::SIDEBAR, k.secondary))
                    .value(s::CLOUD_NOT_YET, cx),
            )
            .footer(s::CLOUD_FOOTER);
        widgets::pane()
            .child(computers)
            .children(
                (self.machines.is_some() && matches!(self.status, Status::Failed(_)))
                    .then(|| widgets::native_status(&self.status, cx))
                    .flatten(),
            )
            .child(cloud)
    }
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

/// The symbol of a computer by its kind, read from its name and tags: a
/// notebook `laptopcomputer`, a Mac mini `macmini`, a server `cloud`,
/// anything else `desktopcomputer`.
fn machine_glyph(m: &MachineInfo) -> Sym {
    let words: Vec<String> = std::iter::once(m.name.to_lowercase())
        .chain(m.labels.iter().map(|l| l.to_lowercase()))
        .collect();
    let has = |keys: &[&str]| words.iter().any(|w| keys.iter().any(|k| w.contains(k)));
    if has(&["laptop", "macbook", "notebook", "笔记本"]) {
        Sym::Laptop
    } else if has(&["mini"]) {
        Sym::MacMini
    } else if has(&["server", "服务器", "linux", "vps", "cloud", "云", "ubuntu"]) {
        Sym::Cloud
    } else {
        Sym::Computer
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
        let k = native_colors(cx);
        if let Some(created) = &self.created {
            let secret = |id: &'static str, label: &'static str, value: &str| {
                let value: gpui::SharedString = value.to_string().into();
                div()
                    .w_full()
                    .flex()
                    .flex_col()
                    .gap(px(4.))
                    .child(div().n_body().text_color(k.label).child(label))
                    .child(
                        div()
                            .w_full()
                            .min_h(metrics::FIELD)
                            .pl(metrics::FIELD_PAD)
                            .pr(px(2.))
                            .py(px(3.))
                            .rounded(metrics::FIELD_RADIUS)
                            .bg(k.content)
                            .border_1()
                            .border_color(k.separator)
                            .flex()
                            .items_start()
                            .gap(px(6.))
                            .child(
                                div()
                                    .flex_1()
                                    .min_w(px(0.))
                                    .font_family(theme::MONO_FONT_FAMILY)
                                    .text_size(px(12.))
                                    .line_height(px(18.))
                                    .text_color(k.label)
                                    .child(value.clone()),
                            )
                            .child(
                                gpui_component::clipboard::Clipboard::new(id)
                                    .tooltip(s::COPY)
                                    .value(value),
                            ),
                    )
            };
            return SheetLayout::new(fill(s::MACHINE_CREATED, &[("name", &created.machine.name)]))
                .explanation(s::SHOWN_ONCE)
                .body(secret(
                    "machine-install",
                    s::MACHINE_INSTALL,
                    created.install_command.expose(),
                ))
                .body(widgets::quiet(s::MACHINE_INSTALL_HINT, cx))
                .body(secret(
                    "machine-credential",
                    s::MACHINE_CREDENTIAL,
                    created.credential.expose(),
                ))
                .buttons([push_button(
                    "machine-done",
                    s::DONE,
                    PushStyle::Default,
                    PushSize::Large,
                    cx,
                )
                .on_click(|_, window, cx| window.close_dialog(cx))])
                .into_any_element();
        }
        let busy = self.status.busy();
        let submit = cx.entity();
        SheetLayout::new(s::COMPUTER_ADD)
            .explanation(s::COMPUTER_ADD_HINT)
            .body(
                GroupedForm::new()
                    .row(
                        FormRow::new(s::MACHINE_NAME).control(
                            NativeTextField::new("machine-name", &self.name)
                                .width(metrics::FIELD_NAME_WIDTH),
                        ),
                    )
                    .row(
                        FormRow::new(s::MACHINE_LABELS).control(
                            NativeTextField::new("machine-labels", &self.labels)
                                .width(metrics::FIELD_NAME_WIDTH),
                        ),
                    ),
            )
            .when_some(widgets::native_status(&self.status, cx), |l, st| l.body(st))
            .buttons([
                push_button(
                    "machine-cancel",
                    common::CANCEL,
                    PushStyle::Standard,
                    PushSize::Large,
                    cx,
                )
                .on_click(|_, window, cx| window.close_dialog(cx)),
                disabled_push(
                    push_button(
                        "machine-submit",
                        s::MACHINE_CREATE,
                        PushStyle::Default,
                        PushSize::Large,
                        cx,
                    ),
                    busy,
                    cx,
                )
                .on_click(move |_, _, cx| submit.update(cx, |f, cx| f.submit(cx))),
            ])
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
    let first = std::cell::Cell::new(true);
    widgets::open_sheet(
        window,
        cx,
        SheetWidth::Form,
        px(460.),
        move |dialog, window, cx| {
            if first.replace(false) {
                shown
                    .read(cx)
                    .name
                    .read(cx)
                    .focus_handle(cx)
                    .focus(window, cx);
            }
            let ok = shown.clone();
            dialog
                .on_ok(move |_, _, cx| {
                    ok.update(cx, |f, cx| f.submit(cx));
                    false
                })
                .child(shown.clone())
        },
    );
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
            Sym::MacMini
        );
        assert_eq!(
            machine_glyph(&machine("设计组 iMac", &["mac"])),
            Sym::Computer
        );
        assert_eq!(machine_glyph(&machine("旧笔记本", &["mac"])), Sym::Laptop);
        assert_eq!(
            machine_glyph(&machine("joye-macbook-pro-m4-max", &[])),
            Sym::Laptop
        );
        assert_eq!(machine_glyph(&machine("build-01", &["linux"])), Sym::Cloud);
    }

    #[test]
    fn tags_read_as_words() {
        assert_eq!(tag_text("mac"), "macOS");
        assert_eq!(tag_text(" Office "), "办公室");
        assert_eq!(tag_text("ios-simulator"), "iOS 模拟器");
        // A tag the owner made up stays as typed.
        assert_eq!(tag_text("xcode-16"), "xcode-16");
    }
}
