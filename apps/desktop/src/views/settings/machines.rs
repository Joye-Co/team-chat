//! 电脑 (COMPLETENESS 2.19; design-roles 3.9, R5): where AI members work, for every member.
//! When more than one person uses the server the computers come in groups: 我的电脑 (mine: only
//! what I ask is done there, only I manage them), 团队的电脑 (an admin manages them; a member
//! sees the name and whether it is online) and 同事的电脑 (other people's own computers: the
//! name, the owner, online). One person alone sees one list, 电脑, and no 团队 or 我的. Each row:
//! a 28 icon box by OS, the name, what it does · version · OS · tags, a StatusBadge and "⋯"
//! with what I may do to it ([`super::machine_actions`]). 用这台电脑 and 添加电脑 are in the
//! first header. Section 云端 says the cloud is not open yet. Every list has its loading, empty
//! and failed state.

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
    machine_actions::{self as actions, After},
    my_id, solo,
    widgets::{self, Status, answer},
};
use crate::ui::native::sheet::sheet_status_error;
use crate::views::nav::dialogs::{sheet_form, sheet_form_row};
use crate::{
    app::Services,
    copy::{common, fill, settings as s},
    data::{MachineCreated, MachineInfo, Ownership, ThisMac},
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
    /// Whether I am an owner, read when the session or the members change
    /// rather than on every frame (`session()` reads the store).
    owner: bool,
    /// My member id, for whose computer is whose.
    me: Option<String>,
    /// One person uses the server: no groups (design-roles 3.9).
    solo: bool,
    /// The last change from a row's "⋯" that failed.
    action: Status,
    /// 不再用这台电脑 in progress or failed (its failure offers 重试).
    stopping: Status,
    /// The periodic reload; dropped with the pane.
    _poll: Task<()>,
    _owner: gpui::Subscription,
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
        let owner_sub = cx.subscribe(
            &services.hub,
            |this, _, changes: &crate::app::Changes, cx| {
                if changes.any(|c| {
                    matches!(
                        c,
                        crate::data::Change::Members | crate::data::Change::Session
                    )
                }) {
                    let owner = i_am_owner(&this.services);
                    let (me, alone) = (my_id(&this.services), solo(&this.services));
                    if owner != this.owner || me != this.me || alone != this.solo {
                        this.owner = owner;
                        this.me = me;
                        this.solo = alone;
                        cx.notify();
                    }
                }
            },
        );
        Self {
            owner: i_am_owner(&services),
            me: my_id(&services),
            solo: solo(&services),
            action: Status::Idle,
            stopping: Status::Idle,
            _owner: owner_sub,
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

    /// What a change from a row's "⋯" does next: reload, or say why it failed.
    pub fn after(&self, cx: &mut Context<Self>) -> After {
        let page = cx.entity().downgrade();
        std::rc::Rc::new(move |result, cx| {
            let _ = page.update(cx, |p, cx| {
                p.this_mac = p.services.provider.this_mac();
                p.this_machine = p.services.provider.this_machine_id();
                match result {
                    Ok(()) => {
                        p.action = Status::Idle;
                        p.using = Status::Idle;
                        p.refresh(cx);
                    }
                    Err(e) => p.action = Status::from_error(e),
                }
                cx.notify();
            });
        })
    }

    /// What 不再用这台电脑 does next: reload, or say it did not work and offer 重试.
    pub fn stop_after(&self, cx: &mut Context<Self>) -> After {
        let page = cx.entity().downgrade();
        std::rc::Rc::new(move |result, cx| {
            let _ = page.update(cx, |p, cx| {
                p.this_mac = p.services.provider.this_mac();
                p.this_machine = p.services.provider.this_machine_id();
                match result {
                    Ok(()) => {
                        p.stopping = Status::Idle;
                        p.using = Status::Idle;
                        p.refresh(cx);
                    }
                    Err(e) => p.stopping = Status::from_error(e),
                }
                cx.notify();
            });
        })
    }

    /// 不再用这台电脑 again after it failed (the confirmation was given already).
    pub fn stop_this(&mut self, cx: &mut Context<Self>) {
        if self.stopping.busy() {
            return;
        }
        self.stopping = Status::Busy(s::LOADING.into());
        let (reply, after) = (
            self.services.provider.stop_using_this_computer(),
            self.stop_after(cx),
        );
        cx.spawn(async move |_, cx| {
            let result = answer(reply).await.map(|_| ());
            cx.update(|cx| after(result, cx));
        })
        .detach();
        cx.notify();
    }

    /// 用这台电脑: first the screen that says what it means (机 3.1), then the set-up.
    pub fn ask_use_this_mac(this: Entity<Self>, window: &mut Window, cx: &mut App) {
        let alone = this.read(cx).solo;
        actions::open_use_this(
            alone,
            move |cx| this.update(cx, |p, cx| p.use_this_mac(cx)),
            window,
            cx,
        );
    }

    /// The computers in the order of the page: (mine, the team's, other people's); alone, all
    /// in the first.
    fn groups<'a>(
        &self,
        list: &'a [MachineInfo],
    ) -> (
        Vec<&'a MachineInfo>,
        Vec<&'a MachineInfo>,
        Vec<&'a MachineInfo>,
    ) {
        group_machines(list, self.me.as_deref(), self.solo)
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
        let this = self.this_machine.as_deref() == Some(m.id.as_str());
        let me = self.me.as_deref();
        let may = actions::allowed(m, me, self.owner, this);
        // This Mac's own daemon can be restarted here (an old one that does not restart
        // itself after an app update, or one that is stuck).
        let restart =
            (this && may.rename && self.this_mac == ThisMac::Working).then(|| cx.entity());
        let (services, after, stop) = (self.services.clone(), self.after(cx), self.stop_after(cx));
        let machine = m.clone();
        let more = crate::views::settings::widgets::more_button(
            ElementId::Name(format!("machine-more-{}", m.id).into()),
            window,
            cx,
        )
        .dropdown_menu(move |menu, _, _| {
            let name = machine.name.clone();
            let mut menu = menu.item(PopupMenuItem::new(s::MACHINE_COPY_NAME).on_click(
                move |_, _, cx| cx.write_to_clipboard(ClipboardItem::new_string(name.clone())),
            ));
            if let Some(page) = restart.clone() {
                menu = menu.item(
                    PopupMenuItem::new(s::MACHINE_RESTART)
                        .on_click(move |_, _, cx| page.update(cx, |p, cx| p.restart_this_mac(cx))),
                );
            }
            actions::menu_items(menu, &services, &machine, may, &after, &stop)
        });
        let facts = match m.ownership(me) {
            // Somebody else's computer: whose it is first (a member sees nothing more).
            Ownership::Theirs => {
                let owner = m
                    .owner_id
                    .as_deref()
                    .and_then(|id| self.services.provider.member(id))
                    .map(|o| crate::ui::avatar::name_beside_avatar(&o.name).to_string())
                    .unwrap_or_default();
                // Cut down for me: whose it is, nothing about its work.
                fill(s::MACHINE_OF, &[("name", &owner)])
            }
            // A member sees a team computer's name and whether it is online, nothing more.
            Ownership::Team if !self.owner => String::new(),
            Ownership::Mine => {
                let members = self.services.provider.members();
                match actions::my_bots_summary(m, &members) {
                    Some(work) => format!("{} · {work}", machine_facts(m, now)),
                    None => machine_facts(m, now),
                }
            }
            Ownership::Team => machine_facts(m, now),
        };
        let facts = match (this, facts.is_empty()) {
            (true, true) => s::MACHINE_THIS.to_string(),
            (true, false) => format!("{} · {facts}", s::MACHINE_THIS),
            (false, _) => facts,
        };
        // Usable (GitHub, §3.7): an older daemon cannot use the GitHub I connected.
        let facts = match super::github::machine_hint(m, &self.services) {
            Some(hint) => format!("{hint} · {facts}"),
            None => facts,
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
        let owner = self.owner;
        let page = cx.entity();
        // Every member adds their own computers (design-roles 3.3), once a personal computer
        // takes only its owner's work; until then only admins add computers.
        let may_add = crate::data::may_add_computers(owner);
        let add = may_add.then(|| {
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
            ThisMac::NotSetUp if may_add => Some(s::THIS_MAC_USE),
            ThisMac::NotSetUp => None,
            ThisMac::Elsewhere => Some(s::THIS_MAC_SWITCH),
            ThisMac::Unavailable | ThisMac::Working => None,
        };
        // This computer runs its service but the server no longer lists it (removed from
        // another device, or a 不再用这台电脑 that stopped halfway): its row and "⋯" are gone,
        // so 不再用这台电脑 is offered here.
        let stop_here = this_computer_unlisted(
            self.this_mac,
            self.this_machine.as_deref(),
            self.machines.as_deref(),
        )
        .then(|| {
            let (services, after) = (self.services.clone(), self.stop_after(cx));
            disabled_push(
                push_button(
                    "machine-stop-this",
                    s::THIS_STOP,
                    PushStyle::Outline,
                    PushSize::Small,
                    cx,
                ),
                self.stopping.busy(),
                cx,
            )
            .loading(self.stopping.busy())
            .on_click(move |_, window, cx| {
                actions::confirm_stop_this(services.clone(), after.clone(), window, cx)
            })
        });
        let use_here = use_label.map(|label| {
            let page = cx.entity();
            let first = self.this_mac == ThisMac::NotSetUp;
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
            .on_click(move |_, window, cx| {
                // Moving an existing set-up onto the app's daemon changes nothing it does.
                if first {
                    Self::ask_use_this_mac(page.clone(), window, cx)
                } else {
                    page.update(cx, |p, cx| p.use_this_mac(cx))
                }
            })
        });
        let actions_row = div()
            .flex()
            .items_center()
            .gap(tokens::space::S8)
            .children(stop_here)
            .children(use_here)
            .children(add);
        // Why this Mac is offered: a daemon copied by hand does not update with the app.
        let this_mac_note = (self.this_mac == ThisMac::Elsewhere && !self.using.busy())
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
        let stop_failed = match &self.stopping {
            Status::Failed(t) => Some(widgets::failed_alert(
                "machine-stop-failed",
                s::THIS_STOP_FAILED,
                Some(t.clone()),
                Some(
                    widgets::retry_button("machine-stop-retry", cx)
                        .on_click(cx.listener(|this, _, _, cx| this.stop_this(cx))),
                ),
                cx,
            )),
            _ => None,
        };
        let action_failed = match &self.action {
            Status::Failed(t) => Some(widgets::failed_alert(
                "machine-action-failed",
                s::MACHINE_ACTION_FAILED,
                Some(t.clone()),
                None,
                cx,
            )),
            _ => None,
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
        let rows = |list: &[&MachineInfo], window: &Window, cx: &mut Context<Self>| {
            let rows = list.iter().map(|m| self.row(m, window, cx)).collect();
            GroupedForm::new()
                .row(widgets::item_rows(rows, tokens::height::SM, cx))
                .into_any_element()
        };
        let empty = |text: &'static str, cx: &mut Context<Self>| {
            GroupedForm::new()
                .row(widgets::list_empty(Sym::Computer, text, None, cx))
                .into_any_element()
        };
        let (first_title, first_footer) = if self.solo {
            (s::COMPUTERS, s::TEAM_COMPUTERS_FOOTER)
        } else {
            (s::MY_COMPUTERS, s::MY_COMPUTERS_FOOTER)
        };
        let mut first = SectionTitle::new(first_title)
            .description(first_footer)
            .action(actions_row);
        let mut sections: Vec<gpui::AnyElement> = Vec::new();
        let first_body: gpui::AnyElement = match (&self.machines, &self.status) {
            (Some(list), _) => {
                let (mine, team, theirs) = self.groups(list);
                if !mine.is_empty() {
                    first = first.count(mine.len() as u32);
                }
                let body = match (mine.is_empty(), self.solo) {
                    (false, _) => rows(&mine, window, cx),
                    (true, true) => empty(s::NO_COMPUTERS, cx),
                    (true, false) => GroupedForm::new()
                        .row(widgets::list_empty(
                            Sym::Computer,
                            s::MY_COMPUTERS_EMPTY,
                            None,
                            cx,
                        ))
                        .footer(if may_add {
                            s::MY_COMPUTERS_EMPTY_HINT
                        } else {
                            s::MY_COMPUTERS_EMPTY_MEMBER_HINT
                        })
                        .into_any_element(),
                };
                if !self.solo {
                    let team_footer = if owner {
                        s::TEAM_COMPUTERS_FOOTER
                    } else {
                        s::TEAM_COMPUTERS_MEMBER_FOOTER
                    };
                    let mut header = SectionTitle::new(s::TEAM_COMPUTERS).description(team_footer);
                    let team_body = if team.is_empty() {
                        empty(s::NO_COMPUTERS, cx)
                    } else {
                        header = header.count(team.len() as u32);
                        rows(&team, window, cx)
                    };
                    sections.push(widgets::section(header, team_body).into_any_element());
                    if !theirs.is_empty() {
                        let header = SectionTitle::new(s::OTHERS_COMPUTERS)
                            .description(s::OTHERS_COMPUTERS_FOOTER)
                            .count(theirs.len() as u32);
                        sections.push(
                            widgets::section(header, rows(&theirs, window, cx)).into_any_element(),
                        );
                    }
                }
                body
            }
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
                first,
                div()
                    .w_full()
                    .flex()
                    .flex_col()
                    .gap(tokens::space::S12)
                    .children(this_mac_note)
                    .children(using)
                    .children(stop_failed)
                    .children(action_failed)
                    .child(first_body)
                    .children(reload_failed),
            ))
            .children(sections)
            .child(widgets::section(
                SectionTitle::new(s::CLOUD).description(s::CLOUD_FOOTER),
                cloud,
            ))
    }
}

/// This computer runs its service (`this_mac`) but the loaded list has no row for it.
pub fn this_computer_unlisted(
    this_mac: ThisMac,
    this_machine: Option<&str>,
    list: Option<&[MachineInfo]>,
) -> bool {
    let running = matches!(this_mac, ThisMac::Working | ThisMac::Elsewhere);
    match (running, list) {
        (true, Some(list)) => !this_machine.is_some_and(|id| list.iter().any(|m| m.id == id)),
        _ => false,
    }
}

/// The groups of the page (design-roles 3.9): (mine, the team's, other people's), each in the
/// server's order. One person alone gets every computer in the first list.
pub fn group_machines<'a>(
    list: &'a [MachineInfo],
    me: Option<&str>,
    solo: bool,
) -> (
    Vec<&'a MachineInfo>,
    Vec<&'a MachineInfo>,
    Vec<&'a MachineInfo>,
) {
    if solo {
        return (list.iter().collect(), Vec::new(), Vec::new());
    }
    let of = |o: Ownership| -> Vec<&MachineInfo> {
        list.iter().filter(|m| m.ownership(me) == o).collect()
    };
    (
        of(Ownership::Mine),
        of(Ownership::Team),
        of(Ownership::Theirs),
    )
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
    status: Status,
    created: Option<MachineCreated>,
    /// The system of the computer being added (it need not be this one's): its tag and
    /// which install command the sheet shows (机 1.8).
    target: InstallTarget,
    /// 谁用这台电脑 (admins, when more than one person uses the server): only me. Members
    /// always add their own computer.
    only_me: bool,
    _subscriptions: Vec<gpui::Subscription>,
}

/// The system of a computer that is being added as a machine.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InstallTarget {
    /// A Mac: shell lines that run the installed app's daemon.
    Mac,
    /// A Windows computer: PowerShell lines.
    Windows,
    /// A Linux computer: shell lines that download the released daemon.
    Linux,
}

impl InstallTarget {
    const ALL: [InstallTarget; 3] = [
        InstallTarget::Mac,
        InstallTarget::Windows,
        InstallTarget::Linux,
    ];

    /// The system this app runs on: the likeliest one for the next computer.
    pub fn of_this_computer() -> Self {
        if cfg!(target_os = "windows") {
            InstallTarget::Windows
        } else if cfg!(target_os = "linux") {
            InstallTarget::Linux
        } else {
            InstallTarget::Mac
        }
    }

    fn label(self) -> &'static str {
        match self {
            InstallTarget::Mac => s::MACHINE_TARGET_MAC,
            InstallTarget::Windows => s::MACHINE_TARGET_WINDOWS,
            InstallTarget::Linux => s::MACHINE_TARGET_LINUX,
        }
    }

    /// The tag a computer of this system gets: its glyph and its line say the system.
    pub fn tag(self) -> &'static str {
        match self {
            InstallTarget::Mac => "mac",
            InstallTarget::Windows => "windows",
            InstallTarget::Linux => "linux",
        }
    }

    /// The command for this system and the line under it.
    pub fn command(self, created: &MachineCreated) -> (String, &'static str) {
        match self {
            InstallTarget::Mac => (
                created.install_command.expose().to_string(),
                s::MACHINE_INSTALL_HINT,
            ),
            InstallTarget::Windows => (
                created.install_command_windows.expose().to_string(),
                s::MACHINE_INSTALL_HINT_WINDOWS,
            ),
            InstallTarget::Linux => (
                created.install_command_linux.expose().to_string(),
                s::MACHINE_INSTALL_HINT_LINUX,
            ),
        }
    }
}

/// What `POST /v1/machines` says about whose the new computer is (design-roles 3.3): a member's
/// is always his own; an admin's is the team's unless he chose 只有我 (one person alone never
/// chooses: the team's, as the server decides without a word).
pub fn personal_of(admin: bool, only_me: bool) -> Option<bool> {
    match (admin, only_me) {
        (false, _) => Some(true),
        (true, true) => Some(true),
        (true, false) => None,
    }
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
            status: Status::Idle,
            created: None,
            target: InstallTarget::of_this_computer(),
            only_me: false,
            _subscriptions: subscriptions,
        }
    }

    pub fn created(&self) -> Option<&MachineCreated> {
        self.created.as_ref()
    }

    pub fn status(&self) -> &Status {
        &self.status
    }

    /// Picks the system of the computer to add. It gives the computer its tag when it is
    /// created, so once it exists the system (and its install command) stays.
    pub fn set_target(&mut self, target: InstallTarget, cx: &mut Context<Self>) {
        if self.created.is_some() {
            return;
        }
        self.target = target;
        cx.notify();
    }

    pub fn target(&self) -> InstallTarget {
        self.target
    }

    /// Types the name and picks the system (self-tests and scenes).
    pub fn fill(
        &mut self,
        name: &str,
        target: InstallTarget,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.name
            .update(cx, |st, cx| st.set_value(name.to_string(), window, cx));
        self.target = target;
    }

    /// 谁用这台电脑 picked (admins): only me, or the team.
    pub fn set_only_me(&mut self, only_me: bool, cx: &mut Context<Self>) {
        self.only_me = only_me;
        cx.notify();
    }

    /// 谁用这台电脑 shows: I am an admin and other people use the server.
    fn asks_who(&self) -> bool {
        i_am_owner(&self.services) && !solo(&self.services)
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
        let labels = vec![self.target.tag().to_string()];
        let personal = personal_of(i_am_owner(&self.services), self.asks_who() && self.only_me);
        let reply = self
            .services
            .provider
            .create_machine_as(name, labels, personal);
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
            let (command, hint) = self.target.command(created);
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
                .body(widgets::field(
                    s::MACHINE_TARGET,
                    widgets::choice(
                        "machine-target",
                        &InstallTarget::ALL.map(InstallTarget::label),
                        InstallTarget::ALL
                            .iter()
                            .position(|t| *t == self.target)
                            .unwrap_or(0),
                        // The computer has its system's tag now: shown, not changed.
                        true,
                        {
                            let form = cx.entity().downgrade();
                            move |ix: usize, _: &mut Window, cx: &mut gpui::App| {
                                let _ = form.update(cx, |this, cx| {
                                    this.set_target(InstallTarget::ALL[ix.min(2)], cx)
                                });
                            }
                        },
                        cx,
                    ),
                    cx,
                ))
                .body(secret("machine-install", s::MACHINE_INSTALL, &command))
                .body(widgets::quiet(hint, cx))
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
        let form = cx.entity().downgrade();
        let system = widgets::choice(
            "machine-system",
            &InstallTarget::ALL.map(InstallTarget::label),
            InstallTarget::ALL
                .iter()
                .position(|t| *t == self.target)
                .unwrap_or(0),
            busy,
            move |ix: usize, _: &mut Window, cx: &mut gpui::App| {
                let _ = form.update(cx, |this, cx| {
                    this.set_target(InstallTarget::ALL[ix.min(2)], cx)
                });
            },
            cx,
        );
        let who = self.asks_who().then(|| {
            let form = cx.entity().downgrade();
            widgets::choice(
                "machine-who",
                &[s::MACHINE_WHO_USES_TEAM, s::MACHINE_WHO_USES_ONLY_ME],
                usize::from(self.only_me),
                busy,
                move |ix: usize, _: &mut Window, cx: &mut gpui::App| {
                    let _ = form.update(cx, |this, cx| this.set_only_me(ix == 1, cx));
                },
                cx,
            )
        });
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
                    .child(sheet_form_row(s::MACHINE_TARGET, system, cx))
                    .children(who.map(|w| sheet_form_row(s::MACHINE_WHO_USES, w, cx))),
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

    /// The computer being added may run another system than this one: each choice shows
    /// its own command and its own line about where to run it.
    #[test]
    fn each_system_gets_its_own_install_command() {
        let created = MachineCreated {
            machine: MachineInfo::default(),
            credential: crate::data::SecretText::new("tm_x"),
            install_command: crate::data::SecretText::new("mac"),
            install_command_windows: crate::data::SecretText::new("powershell"),
            install_command_linux: crate::data::SecretText::new("linux"),
        };
        assert_eq!(
            InstallTarget::Mac.command(&created),
            ("mac".to_string(), s::MACHINE_INSTALL_HINT)
        );
        assert_eq!(
            InstallTarget::Windows.command(&created),
            ("powershell".to_string(), s::MACHINE_INSTALL_HINT_WINDOWS)
        );
        assert_eq!(
            InstallTarget::Linux.command(&created),
            ("linux".to_string(), s::MACHINE_INSTALL_HINT_LINUX)
        );
        assert!(s::MACHINE_INSTALL_HINT_WINDOWS.contains("PowerShell"));
        assert!(s::MACHINE_INSTALL_HINT_LINUX.contains("Linux"));
        assert_eq!(
            InstallTarget::of_this_computer() == InstallTarget::Windows,
            cfg!(target_os = "windows")
        );
        // The system gives the computer its tag, so its row and glyph say what it is.
        let tags: Vec<&str> = InstallTarget::ALL.iter().map(|t| t.tag()).collect();
        assert_eq!(tags, ["mac", "windows", "linux"]);
        assert!(tags.iter().all(|t| os_tag(t)));
    }

    /// Until R4 sends a personal computer only its owner's work, only admins add computers.
    #[test]
    fn who_may_add_computers() {
        assert!(crate::data::may_add_computers(true));
        assert_eq!(
            crate::data::may_add_computers(false),
            crate::data::OWNERSHIP_DISPATCH
        );
    }

    /// 机 1.6: this computer's service runs but its row is gone (removed elsewhere, or a stop
    /// that ended halfway): the page offers 不再用这台电脑 itself.
    #[test]
    fn a_running_computer_the_server_forgot_can_still_be_stopped() {
        let listed = MachineInfo {
            id: "mc_here".into(),
            ..Default::default()
        };
        let list = [listed];
        let unlisted = |mac, id, list| this_computer_unlisted(mac, id, list);
        assert!(!unlisted(
            ThisMac::Working,
            Some("mc_here"),
            Some(&list[..])
        ));
        assert!(unlisted(ThisMac::Working, Some("mc_gone"), Some(&list[..])));
        assert!(unlisted(ThisMac::Elsewhere, Some("mc_gone"), Some(&[][..])));
        assert!(unlisted(ThisMac::Working, None, Some(&list[..])));
        // Not loaded yet, or nothing running here: nothing to offer.
        assert!(!unlisted(ThisMac::Working, Some("mc_gone"), None));
        assert!(!unlisted(
            ThisMac::NotSetUp,
            Some("mc_gone"),
            Some(&list[..])
        ));
        assert!(!unlisted(ThisMac::Unavailable, None, Some(&list[..])));
    }

    /// design-roles 3.3: a member's new computer is always his own; an admin's is the team's
    /// unless he picks 只有我.
    #[test]
    fn whose_a_new_computer_is() {
        assert_eq!(personal_of(false, false), Some(true));
        assert_eq!(personal_of(false, true), Some(true));
        assert_eq!(personal_of(true, false), None);
        assert_eq!(personal_of(true, true), Some(true));
    }

    fn owned(id: &str, owner: Option<&str>) -> MachineInfo {
        MachineInfo {
            id: id.into(),
            owner_id: owner.map(str::to_string),
            ..Default::default()
        }
    }

    /// design-roles 3.9: mine, the team's, other people's; one person alone sees one list.
    #[test]
    fn the_page_groups_computers_by_whose_they_are() {
        let list = [
            owned("t1", None),
            owned("m1", Some("me")),
            owned("o1", Some("chen")),
            owned("t2", None),
        ];
        let ids = |v: Vec<&MachineInfo>| v.iter().map(|m| m.id.clone()).collect::<Vec<_>>();
        let (mine, team, theirs) = group_machines(&list, Some("me"), false);
        assert_eq!(ids(mine), ["m1"]);
        assert_eq!(ids(team), ["t1", "t2"]);
        assert_eq!(ids(theirs), ["o1"]);
        let (all, team, theirs) = group_machines(&list, Some("me"), true);
        assert_eq!(ids(all), ["t1", "m1", "o1", "t2"]);
        assert!(team.is_empty() && theirs.is_empty());
    }

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
            ..Default::default()
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
