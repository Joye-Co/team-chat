//! 机器 (COMPLETENESS 2.19): where AI members work. Section 团队的电脑
//! (count, description, 添加电脑… in its header) lists the team's
//! computers: a 28 icon box by OS, the name, what it does · version · OS ·
//! tags, a StatusBadge and "⋯". Section 云端 says the cloud is not open
//! yet. Every list has its loading, empty and failed state.

use std::time::Duration;

use gpui::{
    App, AppContext as _, ClipboardItem, Context, ElementId, Entity, Focusable as _,
    InteractiveElement as _, IntoElement, ParentElement, Render, StatefulInteractiveElement as _,
    Styled, Task, Window, div, prelude::FluentBuilder as _, px,
};
use gpui_component::{
    WindowExt as _,
    input::{InputEvent, InputState},
    menu::{DropdownMenu as _, PopupMenuItem},
    tooltip::Tooltip,
};

use super::{
    i_am_owner,
    widgets::{self, Status, answer},
};
use crate::ui::native::sheet::sheet_status_error;
use crate::views::nav::dialogs::{sheet_form, sheet_form_row};
use crate::{
    app::Services,
    copy::{common, fill, settings as s},
    data::{MachineCreated, MachineInfo, ThisMac},
    theme,
    ui::{
        Sym,
        native::{
            BadgeState, GroupedForm, NativeTextField, NativeType as _, PushSize, PushStyle,
            SectionTitle, SheetLayout, SheetWidth, StatusBadge, disabled_push, metrics,
            native_colors, push_button, tokens,
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
    /// Whether this Mac runs the app's own daemon (用这台电脑).
    this_mac: ThisMac,
    /// This Mac's machine id: its row says 这台电脑 and can 重新启动.
    this_machine: Option<String>,
    /// 用这台电脑 in progress or failed.
    using: Status,
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
        let this_mac = services.provider.this_mac();
        let this_machine = services.provider.this_machine_id();
        Self {
            services,
            machines: None,
            status: Status::Idle,
            this_mac,
            this_machine,
            using: Status::Idle,
            _poll: poll,
        }
    }

    pub fn machines(&self) -> Option<&[MachineInfo]> {
        self.machines.as_deref()
    }

    pub fn status(&self) -> &Status {
        &self.status
    }

    /// The loading state of 团队的电脑 (a self-test scene).
    pub fn preview_loading(&mut self, cx: &mut Context<Self>) {
        self.machines = None;
        self.status = Status::Busy(s::LOADING.into());
        cx.notify();
    }

    pub fn this_mac(&self) -> ThisMac {
        self.this_mac
    }

    /// 用这台电脑: sets this Mac up with the app's own daemon, then reloads the list.
    pub fn use_this_mac(&mut self, cx: &mut Context<Self>) {
        if self.using.busy() {
            return;
        }
        self.using = Status::Busy(s::THIS_MAC_SETTING_UP.into());
        let reply = self.services.provider.use_this_mac();
        cx.spawn(async move |this, cx| {
            let result = answer(reply).await;
            let _ = this.update(cx, |this, cx| {
                this.this_mac = this.services.provider.this_mac();
                this.this_machine = this.services.provider.this_machine_id();
                match result {
                    Ok(()) => this.using = Status::Done(s::THIS_MAC_DONE.into()),
                    Err(e) => this.using = Status::from_error(e),
                }
                this.refresh(cx);
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    /// 重新启动 on this Mac's row: the daemon starts again from the app's current file.
    pub fn restart_this_mac(&mut self, cx: &mut Context<Self>) {
        if self.using.busy() {
            return;
        }
        self.using = Status::Busy(s::THIS_MAC_SETTING_UP.into());
        let reply = self.services.provider.restart_this_mac();
        cx.spawn(async move |this, cx| {
            let result = answer(reply).await;
            // The daemon reconnects within a few seconds; then the list shows its version.
            cx.background_executor().timer(Duration::from_secs(4)).await;
            let _ = this.update(cx, |this, cx| {
                this.using = match result {
                    Ok(()) => Status::Idle,
                    Err(_) => Status::Failed(s::MACHINE_RESTART_FAILED.into()),
                };
                this.refresh(cx);
                cx.notify();
            });
        })
        .detach();
        cx.notify();
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

    /// One computer (2.19): a 28 icon box by OS, the name, then what it
    /// does · version · OS · tags (the status first, tags last), a
    /// StatusBadge (异常 explains itself in a tooltip) and "⋯".
    fn row(&self, m: &MachineInfo, window: &Window, cx: &mut Context<Self>) -> gpui::AnyElement {
        let now = crate::selftest::clock::local_now();
        let badge = if !m.connected {
            StatusBadge::new(BadgeState::Offline, s::MACHINE_OFFLINE)
        } else if !m.healthy {
            StatusBadge::new(BadgeState::Abnormal, s::MACHINE_UNHEALTHY)
        } else {
            StatusBadge::new(BadgeState::Online, s::MACHINE_ONLINE)
        };
        let badge = div()
            .id(ElementId::Name(format!("machine-state-{}", m.id).into()))
            .child(badge)
            .when(m.connected && !m.healthy, |d| {
                d.tooltip(|window, cx| Tooltip::new(s::MACHINE_UNHEALTHY_TIP).build(window, cx))
            });
        let name: gpui::SharedString = m.name.clone().into();
        let this = self.this_machine.as_deref() == Some(m.id.as_str());
        // This Mac's own daemon can be restarted here (an old one that does not restart
        // itself after an app update, or one that is stuck).
        let restart = (this && i_am_owner(&self.services) && self.this_mac == ThisMac::Working)
            .then(|| cx.entity());
        let more = crate::views::settings::widgets::more_button(
            ElementId::Name(format!("machine-more-{}", m.id).into()),
            window,
            cx,
        )
        .dropdown_menu(move |menu, _, _| {
            let name = name.clone();
            let menu = menu.item(PopupMenuItem::new(s::MACHINE_COPY_NAME).on_click(
                move |_, _, cx| cx.write_to_clipboard(ClipboardItem::new_string(name.to_string())),
            ));
            match restart.clone() {
                Some(page) => menu.item(
                    PopupMenuItem::new(s::MACHINE_RESTART)
                        .on_click(move |_, _, cx| page.update(cx, |p, cx| p.restart_this_mac(cx))),
                ),
                None => menu,
            }
        });
        let facts = machine_facts(m, now);
        let facts = if this {
            format!("{} · {facts}", s::MACHINE_THIS)
        } else {
            facts
        };
        widgets::ItemRow::new(
            ElementId::Name(format!("machine-{}", m.id).into()),
            m.name.clone(),
        )
        .media(widgets::icon_box_28(machine_glyph(m), cx))
        .description(facts)
        .value(badge)
        .menu(more)
        .into_any_element()
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
        let owner = i_am_owner(&self.services);
        let page = cx.entity();
        let add = owner.then(|| {
            push_button(
                "machine-add",
                s::COMPUTER_ADD_OPEN,
                PushStyle::Outline,
                PushSize::Small,
                cx,
            )
            .on_click(move |_, window, cx| Self::open_add(page.clone(), window, cx))
        });
        // 用这台电脑: this Mac has no daemon yet, or runs one copied by hand that does not
        // update with the app.
        let use_label = match self.this_mac {
            ThisMac::NotSetUp => Some(s::THIS_MAC_USE),
            ThisMac::Elsewhere => Some(s::THIS_MAC_SWITCH),
            ThisMac::Unavailable | ThisMac::Working => None,
        };
        let use_here = use_label.filter(|_| owner).map(|label| {
            disabled_push(
                push_button(
                    "machine-use-this",
                    label,
                    PushStyle::Default,
                    PushSize::Small,
                    cx,
                ),
                self.using.busy(),
                cx,
            )
            .loading(self.using.busy())
            .on_click(cx.listener(|this, _, _, cx| this.use_this_mac(cx)))
        });
        let mut header = SectionTitle::new(s::TEAM_COMPUTERS).description(s::TEAM_COMPUTERS_FOOTER);
        if add.is_some() || use_here.is_some() {
            header = header.action(
                div()
                    .flex()
                    .items_center()
                    .gap(tokens::space::S8)
                    .children(use_here)
                    .children(add),
            );
        }
        // Why this Mac is offered: a daemon copied by hand does not update with the app.
        let this_mac_note = (owner && self.this_mac == ThisMac::Elsewhere && !self.using.busy())
            .then(|| widgets::quiet(s::THIS_MAC_ELSEWHERE_HINT, cx));
        let using = match &self.using {
            Status::Failed(t) => Some(widgets::failed_alert(
                "machine-use-this-failed",
                s::THIS_MAC_FAILED,
                Some(t.clone()),
                Some(
                    widgets::retry_button("machine-use-this-retry", cx)
                        .on_click(cx.listener(|this, _, _, cx| this.use_this_mac(cx))),
                ),
                cx,
            )),
            Status::Done(t) => Some(widgets::quiet(t.clone(), cx).into_any_element()),
            _ => None,
        };
        let body: gpui::AnyElement = match (&self.machines, &self.status) {
            (Some(list), _) if !list.is_empty() => {
                header = header.count(list.len() as u32);
                let rows = list.iter().map(|m| self.row(m, window, cx)).collect();
                GroupedForm::new()
                    .row(widgets::item_rows(rows, tokens::height::SM, cx))
                    .into_any_element()
            }
            (Some(_), _) => GroupedForm::new()
                .row(widgets::list_empty(
                    Sym::Computer,
                    s::NO_COMPUTERS,
                    None,
                    cx,
                ))
                .into_any_element(),
            (None, Status::Failed(t)) => widgets::failed_alert(
                "machines-failed",
                s::COMPUTERS_FAILED,
                Some(t.clone()),
                Some(
                    widgets::retry_button("machines-retry", cx)
                        .on_click(cx.listener(|this, _, _, cx| this.refresh(cx))),
                ),
                cx,
            ),
            (None, _) => GroupedForm::new()
                .row(widgets::list_loading("machines-loading", 2, true, cx))
                .into_any_element(),
        };
        // A reload that failed after the list was shown: the Alert under it.
        let reload_failed = match (&self.machines, &self.status) {
            (Some(_), Status::Failed(t)) => Some(widgets::failed_alert(
                "machines-reload-failed",
                s::COMPUTERS_FAILED,
                Some(t.clone()),
                Some(
                    widgets::retry_button("machines-reload-retry", cx)
                        .on_click(cx.listener(|this, _, _, cx| this.refresh(cx))),
                ),
                cx,
            )),
            _ => None,
        };
        // 云端 is not open to anyone yet: the row says so and an outline
        // badge marks it; no dead button (F5, C1-03).
        let cloud = GroupedForm::new().row(
            widgets::ItemRow::new("cloud", s::CLOUD)
                .media(widgets::icon_box_28(Sym::Cloud, cx))
                .description(s::CLOUD_NOT_YET)
                .value(crate::ui::native::Badge::new(s::CLOUD_COMING_SOON)),
        );
        widgets::pane()
            .child(widgets::section(
                header,
                div()
                    .w_full()
                    .flex()
                    .flex_col()
                    .gap(tokens::space::S12)
                    .children(this_mac_note)
                    .children(using)
                    .child(body)
                    .children(reload_failed),
            ))
            .child(widgets::section(
                SectionTitle::new(s::CLOUD).description(s::CLOUD_FOOTER),
                cloud,
            ))
    }
}

/// The second line of a computer row, most important first: what it does
/// (or when it was last seen; a time of 0 is never), the version, the OS,
/// then the other tags.
fn machine_facts(m: &MachineInfo, now: chrono::DateTime<chrono::Local>) -> String {
    let mut facts: Vec<String> = Vec::new();
    facts.push(match m.last_heartbeat_at {
        _ if m.connected && m.active_runs > 0 => {
            fill(s::MACHINE_BUSY, &[("n", &m.active_runs.to_string())])
        }
        _ if m.connected => s::MACHINE_IDLE.to_string(),
        Some(at) if at > 0 => fill(s::MACHINE_SEEN, &[("when", &relative_time(at, now))]),
        _ => s::MACHINE_NEVER_ONLINE.to_string(),
    });
    if let Some(v) = &m.version {
        facts.push(fill(s::MACHINE_VERSION, &[("v", v)]));
    }
    let (os, tags): (Vec<&String>, Vec<&String>) = m.labels.iter().partition(|l| os_tag(l));
    facts.extend(os.into_iter().map(|l| tag_text(l)));
    let tags: Vec<String> = tags.into_iter().map(|l| tag_text(l)).collect();
    if !tags.is_empty() {
        facts.push(tags.join("、"));
    }
    facts.join(" · ")
}

/// A tag that names the operating system.
fn os_tag(label: &str) -> bool {
    matches!(
        label.trim().to_ascii_lowercase().as_str(),
        "mac" | "macos" | "darwin" | "osx" | "linux" | "ubuntu" | "debian" | "windows" | "win"
    )
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

/// The symbol of a computer by its OS and kind (2.19): a Linux or other
/// server gets the server glyph (`Sym::Server` until the kit has
/// `Sym::Server`), a notebook `laptopcomputer`, a Mac mini `macmini`,
/// any other Mac `desktopcomputer`. The cloud glyph is only for 云端.
fn machine_glyph(m: &MachineInfo) -> Sym {
    let words: Vec<String> = std::iter::once(m.name.to_lowercase())
        .chain(m.labels.iter().map(|l| l.to_lowercase()))
        .collect();
    let has = |keys: &[&str]| words.iter().any(|w| keys.iter().any(|k| w.contains(k)));
    if has(&["laptop", "macbook", "notebook", "笔记本"]) {
        Sym::Laptop
    } else if has(&["mini"]) {
        Sym::MacMini
    } else if has(&[
        "server",
        "服务器",
        "linux",
        "ubuntu",
        "debian",
        "vps",
        "windows",
    ]) {
        Sym::Server
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
    _subscriptions: Vec<gpui::Subscription>,
}

impl MachineForm {
    pub fn new(services: Services, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let name =
            cx.new(|cx| InputState::new(window, cx).placeholder(s::MACHINE_NAME_PLACEHOLDER));
        // The default button follows the name as it is typed.
        let subscriptions = vec![cx.subscribe(&name, |_, _, e: &InputEvent, cx| {
            if matches!(e, InputEvent::Change) {
                cx.notify();
            }
        })];
        Self {
            services,
            name,
            labels: cx
                .new(|cx| InputState::new(window, cx).placeholder(s::RUNNER_LABELS_PLACEHOLDER)),
            status: Status::Idle,
            created: None,
            _subscriptions: subscriptions,
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
                    .gap(crate::ui::native::tokens::space::S4)
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
                            .gap(crate::ui::native::tokens::space::S6)
                            .child(
                                div()
                                    .flex_1()
                                    .min_w(px(0.))
                                    .font_family(theme::MONO_FONT_FAMILY)
                                    .text_size(crate::ui::native::tokens::text::META.0)
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
                    PushSize::Regular,
                    cx,
                )
                .on_click(|_, window, cx| window.close_dialog(cx))])
                .into_any_element();
        }
        let busy = self.status.busy();
        let typed = self.name.read(cx).value().trim().to_string();
        // An empty name only turns the default button off; a name that is
        // too long also shows why under the field.
        let problem = (!typed.is_empty())
            .then(|| widgets::check_len(&typed, 64).err())
            .flatten();
        let off = busy || typed.is_empty() || problem.is_some();
        let (submit, retry) = (cx.entity(), cx.entity());
        let failed = match &self.status {
            Status::Failed(t) => Some(widgets::failed_alert(
                "machine-add-failed",
                s::MACHINE_ADD_FAILED,
                Some(t.clone()),
                Some(
                    widgets::retry_button("machine-add-retry", cx)
                        .on_click(move |_, _, cx| retry.update(cx, |f, cx| f.submit(cx))),
                ),
                cx,
            )),
            _ => None,
        };
        SheetLayout::new(s::COMPUTER_ADD)
            .explanation(s::COMPUTER_ADD_HINT)
            .body(
                sheet_form()
                    .child(sheet_form_row(
                        s::MACHINE_NAME,
                        NativeTextField::new("machine-name", &self.name)
                            .full_width()
                            .error(problem.is_some()),
                        cx,
                    ))
                    .child(sheet_form_row(
                        s::MACHINE_LABELS,
                        NativeTextField::new("machine-labels", &self.labels).full_width(),
                        cx,
                    )),
            )
            .when_some(failed, |l, f| l.body(f))
            .when_some(problem, |l, p| l.status(sheet_status_error(p, cx)))
            .buttons([
                push_button(
                    "machine-cancel",
                    common::CANCEL,
                    PushStyle::Outline,
                    PushSize::Regular,
                    cx,
                )
                .on_click(|_, window, cx| window.close_dialog(cx)),
                disabled_push(
                    push_button(
                        "machine-submit",
                        s::MACHINE_CREATE,
                        PushStyle::Default,
                        PushSize::Regular,
                        cx,
                    ),
                    off,
                    cx,
                )
                .loading(busy)
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
        assert_eq!(machine_glyph(&machine("build-01", &["linux"])), Sym::Server);
        assert_eq!(machine_glyph(&machine("构建服务器", &[])), Sym::Server);
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
