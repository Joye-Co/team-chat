//! The machine monitor (开发者视图, owners only): its own window with a sidebar of pages and
//! the 概览 page of one machine (docs/protocol.md, "Machine monitor"). Only 概览 is live; the
//! other pages of the approved design are listed and disabled until they land.
//!
//! The entry point is one command, [`OpenMonitor`], offered to owners by the app menu
//! ([`menu_item`]); the admin area may move it, nothing else depends on where it sits.
//! The window asks the server every 5 seconds while it is open.

mod model;
mod overview;
mod spark;

use std::time::Duration;

use gpui::{
    AnyWindowHandle, App, AppContext as _, Bounds, Context, Entity, Global, InteractiveElement as _,
    IntoElement, MouseButton, ParentElement as _, Render, ScrollHandle, SharedString,
    StatefulInteractiveElement as _, Styled, Subscription, Task, TitlebarOptions, Window,
    WindowBounds, WindowOptions, div, point, prelude::FluentBuilder as _, size,
};
use gpui_component::Disableable as _;
use gpui_component::button::ButtonVariants as _;
use gpui_component::menu::{DropdownMenu as _, PopupMenuItem};

use chat_core::proto::MachineMonitor;

use crate::{
    app::Services,
    copy::{fill, monitor as m},
    app::Changes,
    data::{AdminError, Change, MachineInfo},
    theme,
    ui::{
        MemberAvatar, Sym,
        icons::sym as sym_icon,
        native::{
            Alert, AlertTone, Empty, NativeType as _, PushSize, PushStyle, SidebarFooter,
            SidebarRow, Segment, Segmented, TruncText, group_label, native_colors, push_button,
            sidebar_panel,
            tokens::{height, metrics, monitor as t, radius, space},
        },
        sf_symbols::SymSize,
    },
    views::settings::{i_am_owner, widgets::answer},
};

pub use model::Range;

gpui::actions!(
    monitor,
    [
        /// App menu › 机器监控… (owners): opens the monitor window, or brings it to the front.
        OpenMonitor
    ]
);

/// How often the open window asks for fresh numbers.
const REFRESH_EVERY: Duration = Duration::from_secs(5);

/// The app menu's item, for owners only; `None` for everyone else.
pub fn menu_item(owner: bool) -> Option<gpui::MenuItem> {
    owner.then(|| gpui::MenuItem::action(m::MENU_OPEN, OpenMonitor))
}

/// Binds [`OpenMonitor`]. The window itself checks again that I am an owner.
pub fn install(services: Services, cx: &mut App) {
    cx.on_action(move |_: &OpenMonitor, cx| open(services.clone(), cx));
}

/// The open monitor window, so the command never opens a second one.
#[derive(Default)]
struct MonitorWindow(Option<AnyWindowHandle>);

impl Global for MonitorWindow {}

/// Opens the monitor window, or brings the open one to the front.
pub fn open(services: Services, cx: &mut App) {
    if let Some(handle) = cx.try_global::<MonitorWindow>().and_then(|w| w.0)
        && handle
            .update(cx, |_, window, _| window.activate_window())
            .is_ok()
    {
        return;
    }
    if let Ok((handle, _)) = open_window(services, true, cx) {
        cx.set_global(MonitorWindow(Some(handle)));
    }
}

pub fn open_window(
    services: Services,
    focus: bool,
    cx: &mut App,
) -> Result<(AnyWindowHandle, Entity<MonitorView>), String> {
    let options = WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
            None,
            size(t::WINDOW_WIDTH, t::WINDOW_HEIGHT),
            cx,
        ))),
        window_min_size: Some(size(t::MIN_WIDTH, t::MIN_HEIGHT)),
        titlebar: Some(TitlebarOptions {
            title: Some(m::WINDOW_TITLE.into()),
            appears_transparent: true,
            traffic_light_position: Some(point(theme::TRAFFIC_LIGHTS.0, theme::TRAFFIC_LIGHTS.1)),
        }),
        window_background: gpui::WindowBackgroundAppearance::Opaque,
        focus,
        show: true,
        ..Default::default()
    };
    gpui_kit::open_window(options, cx, move |window, cx| {
        cx.new(|cx| MonitorView::new(services, window, cx))
    })
    .map_err(|e| e.to_string())
}

/// A load that has not answered, answered, or failed.
#[derive(Clone, Debug, PartialEq)]
enum Load {
    Idle,
    Busy,
    Failed(String),
}

pub struct MonitorView {
    services: Services,
    owner: bool,
    machines: Option<Vec<MachineInfo>>,
    selected: Option<String>,
    range: Range,
    sort: model::Sort,
    data: Option<MachineMonitor>,
    /// The machine and range `data` is for.
    data_for: Option<(String, Range)>,
    load: Load,
    scroll: ScrollHandle,
    _poll: Task<()>,
    _subs: Vec<Subscription>,
}

impl MonitorView {
    pub fn new(services: Services, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let owner = i_am_owner(&services);
        let poll = cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(REFRESH_EVERY).await;
                if this.update(cx, |this, cx| this.refresh(cx)).is_err() {
                    break;
                }
            }
        });
        let subs = vec![
            theme::follow_system(window, cx),
            cx.subscribe(&services.hub, |this: &mut Self, _, changes: &Changes, cx| {
                if changes.any(|c| matches!(c, Change::Members | Change::Session)) {
                    let owner = i_am_owner(&this.services);
                    if owner != this.owner {
                        this.owner = owner;
                        this.refresh(cx);
                    }
                    cx.notify();
                }
            }),
            cx.observe_window_activation(window, |_, _, cx| cx.notify()),
        ];
        let mut this = Self {
            services,
            owner,
            machines: None,
            selected: None,
            range: Range::default(),
            sort: model::Sort::default(),
            data: None,
            data_for: None,
            load: Load::Idle,
            scroll: ScrollHandle::new(),
            _poll: poll,
            _subs: subs,
        };
        this.refresh(cx);
        this
    }

    /// Asks for the machine list (the first time, or when it failed), then the monitor of the
    /// selected machine.
    fn refresh(&mut self, cx: &mut Context<Self>) {
        if !self.owner || self.load == Load::Busy {
            return;
        }
        self.load = Load::Busy;
        let list = self.services.provider.machines();
        cx.spawn(async move |this, cx| {
            let result = answer(list).await;
            let _ = this.update(cx, |this, cx| {
                match result {
                    Ok(list) => {
                        let keep = this
                            .selected
                            .as_ref()
                            .filter(|id| list.iter().any(|x| &x.id == *id))
                            .cloned();
                        this.selected = keep.or_else(|| {
                            list.iter()
                                .find(|x| x.connected)
                                .or(list.first())
                                .map(|x| x.id.clone())
                        });
                        this.machines = Some(list);
                        this.fetch(cx);
                    }
                    Err(e) => {
                        this.load = Load::Failed(error_text(e));
                        cx.notify();
                    }
                }
            });
        })
        .detach();
    }

    fn fetch(&mut self, cx: &mut Context<Self>) {
        let Some(id) = self.selected.clone() else {
            self.load = Load::Idle;
            cx.notify();
            return;
        };
        let range = self.range;
        let reply = self
            .services
            .provider
            .machine_monitor(&id, range.wire(), day_start());
        cx.spawn(async move |this, cx| {
            let result = answer(reply).await;
            let _ = this.update(cx, |this, cx| {
                // A reply for another machine or range is old: drop it.
                if this.selected.as_deref() != Some(id.as_str()) || this.range != range {
                    this.load = Load::Idle;
                    return;
                }
                match result {
                    Ok(d) => {
                        this.data = Some(d);
                        this.data_for = Some((id, range));
                        this.load = Load::Idle;
                    }
                    Err(e) => this.load = Load::Failed(error_text(e)),
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    fn select_machine(&mut self, id: String, cx: &mut Context<Self>) {
        if self.selected.as_deref() == Some(id.as_str()) {
            return;
        }
        self.selected = Some(id);
        self.data = None;
        self.data_for = None;
        self.load = Load::Idle;
        self.refresh(cx);
    }

    fn set_range(&mut self, range: Range, cx: &mut Context<Self>) {
        if self.range == range {
            return;
        }
        self.range = range;
        self.load = Load::Idle;
        self.refresh(cx);
    }

    pub(crate) fn sort_by(&mut self, key: model::SortKey, cx: &mut Context<Self>) {
        self.sort = self.sort.click(key);
        cx.notify();
    }

    fn retry(&mut self, cx: &mut Context<Self>) {
        self.load = Load::Idle;
        self.refresh(cx);
    }

    fn machine(&self) -> Option<&MachineInfo> {
        let id = self.selected.as_deref()?;
        self.machines.as_ref()?.iter().find(|x| x.id == id)
    }

    /// The data of the selected machine; data of the previous range stays shown while the new
    /// one loads.
    fn current(&self) -> Option<&MachineMonitor> {
        let (id, _) = self.data_for.as_ref()?;
        (self.selected.as_deref() == Some(id.as_str()))
            .then_some(self.data.as_ref())
            .flatten()
    }

    fn member_name(&self, id: &str) -> String {
        self.services
            .provider
            .member(id)
            .map(|x| x.name)
            .unwrap_or_else(|| m::UNKNOWN_MEMBER.to_string())
    }
}

/// Local midnight in ms: 今天 for the server.
fn day_start() -> u64 {
    crate::selftest::clock::local_now()
        .date_naive()
        .and_hms_opt(0, 0, 0)
        .and_then(|t| t.and_local_timezone(chrono::Local).single())
        .map(|t| t.timestamp_millis().max(0) as u64)
        .unwrap_or(0)
}

fn now_ms() -> u64 {
    crate::selftest::clock::local_now().timestamp_millis().max(0) as u64
}

/// "10:42:30" in local time.
fn clock(ms: u64) -> String {
    use chrono::TimeZone as _;
    chrono::Local
        .timestamp_millis_opt(ms as i64)
        .single()
        .map(|t| t.format("%H:%M:%S").to_string())
        .unwrap_or_default()
}

/// "10:31" in local time.
fn clock_short(ms: u64) -> String {
    use chrono::TimeZone as _;
    chrono::Local
        .timestamp_millis_opt(ms as i64)
        .single()
        .map(|t| t.format("%H:%M").to_string())
        .unwrap_or_default()
}

fn error_text(e: AdminError) -> String {
    crate::views::settings::widgets::error_text(e).to_string()
}

impl Render for MonitorView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let k = native_colors(cx);
        div()
            .id("monitor-root")
            .size_full()
            .flex()
            .bg(k.content)
            .text_color(k.label)
            .n_body()
            .child(self.sidebar(window, cx))
            .child(
                div()
                    .flex_1()
                    .min_w(gpui::Pixels::ZERO)
                    .h_full()
                    .flex()
                    .flex_col()
                    .child(self.toolbar(window, cx))
                    .child(
                        div()
                            .id("monitor-scroll")
                            .flex_1()
                            .min_h(gpui::Pixels::ZERO)
                            .overflow_y_scroll()
                            .track_scroll(&self.scroll)
                            .child(self.body(window, cx)),
                    ),
            )
    }
}

impl MonitorView {
    fn sidebar(&self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let k = native_colors(cx);
        let data = self.current();
        let logged_out = data
            .map(|d| {
                d.clis
                    .iter()
                    .filter(|c| c.available && c.login == "logged_out")
                    .count() as u32
            })
            .unwrap_or(0);
        let vm_count = data.map(model::running_now);
        let row = |id: &'static str, label: &'static str, s: Sym, selected: bool| {
            SidebarRow::new(id, label)
                .leading(sym_icon(
                    s,
                    SymSize::SIDEBAR,
                    if selected { k.label } else { k.secondary },
                ))
                .selected(selected)
        };
        // The other pages land in later changes: listed, not clickable.
        let soon = |id: &'static str, label: &'static str, s: Sym, trailing: Option<gpui::AnyElement>| {
            let row = row(id, label, s, false);
            let row = match trailing {
                Some(el) => row.trailing_element(el),
                None => row,
            };
            div()
                .id(id)
                .opacity(0.5)
                .tooltip(move |window, cx| {
                    gpui_component::tooltip::Tooltip::new(m::NAV_SOON).build(window, cx)
                })
                .child(row)
        };
        let count = |n: u32, warn: bool| -> gpui::AnyElement {
            let pill = crate::ui::native::CountPill::new(n);
            if warn { pill.tone(k.amber).into_any_element() } else { pill.into_any_element() }
        };
        let me = match self.services.provider.session() {
            crate::data::SessionState::SignedIn { me, .. } => Some(me),
            _ => None,
        };
        sidebar_panel(t::SIDEBAR_WIDTH, cx)
            .child(
                // The traffic lights' row; dragging it moves the window.
                div()
                    .id("monitor-lights")
                    .flex_none()
                    .h(height::TOOLBAR)
                    .w_full()
                    .on_mouse_down(MouseButton::Left, |e, window, _| {
                        if e.click_count >= 2 {
                            window.titlebar_double_click();
                        } else {
                            window.start_window_move();
                        }
                    }),
            )
            .child(self.switcher(window, cx))
            .children(self.capacity_row(cx))
            .child(group_label(m::NAV_WATCH, cx).mt(space::S12))
            .child(row("nav-overview", m::NAV_OVERVIEW, Sym::Grid, true))
            .child(soon("nav-vms", m::NAV_VMS, Sym::Server, vm_count.map(|n| count(n, false))))
            .child(soon("nav-history", m::NAV_HISTORY, Sym::History, None))
            .child(group_label(m::NAV_ACCOUNTS, cx).mt(space::S12))
            .child(soon("nav-subs", m::NAV_SUBSCRIPTIONS, Sym::Account, (logged_out > 0).then(|| count(logged_out, true))))
            .child(soon("nav-keys", m::NAV_KEYS, Sym::Key, None))
            .child(group_label(m::NAV_MACHINE, cx).mt(space::S12))
            .child(soon("nav-image", m::NAV_IMAGE, Sym::Layers, None))
            .child(soon("nav-network", m::NAV_NETWORK, Sym::Shield, None))
            .child(soon("nav-logs", m::NAV_LOGS, Sym::Logs, None))
            .child(div().flex_1())
            .children(me.map(|me| {
                SidebarFooter::new(
                    "monitor-me",
                    MemberAvatar::new(Some(&me), crate::ui::native::tokens::avatar::SM),
                    me.name.clone(),
                )
                .detail(m::ONLY_YOU, cx)
            }))
    }

    /// The machine card at the top of the sidebar: name and state; a menu when there are more.
    fn switcher(&self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let k = native_colors(cx);
        let machine = self.machine();
        let data = self.current();
        let connected = data
            .map(|d| d.machine.connected)
            .or(machine.map(|x| x.connected))
            .unwrap_or(false);
        let name: SharedString = machine
            .map(|x| x.name.clone())
            .or(data.map(|d| d.machine.name.clone()))
            .unwrap_or_default()
            .into();
        let detail = match data.and_then(|d| d.machine.os.clone()) {
            Some(os) if connected => format!("{} · {os}", m::MACHINE_ONLINE),
            _ if connected => m::MACHINE_ONLINE.to_string(),
            _ => m::MACHINE_OFFLINE.to_string(),
        };
        let many = self.machines.as_ref().is_some_and(|l| l.len() > 1);
        let card = div()
            .id("monitor-machine")
            .flex_none()
            .flex()
            .items_center()
            .gap(space::S12)
            .p(space::S8)
            .rounded(radius::CONTAINER)
            .bg(k.sheet)
            .border_1()
            .border_color(k.separator)
            .child(
                div()
                    .flex_none()
                    .size(t::MACHINE_TILE)
                    .rounded(radius::CONTROL)
                    .bg(k.primary)
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(sym_icon(Sym::Server, SymSize::INLINE, k.on_primary)),
            )
            .child(
                div()
                    .flex_1()
                    .min_w(gpui::Pixels::ZERO)
                    .flex()
                    .flex_col()
                    .child(TruncText::new(name).n_headline())
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(space::S6)
                            .n_footnote()
                            .text_color(k.secondary)
                            .child(dot(if connected { k.green } else { k.secondary }))
                            .child(TruncText::new(detail)),
                    ),
            )
            .when(many, |d| {
                d.child(sym_icon(Sym::PopUp, SymSize::INLINE_SMALL, k.secondary))
            });
        if !many {
            return card.into_any_element();
        }
        let list: Vec<(String, String, bool)> = self
            .machines
            .iter()
            .flatten()
            .map(|x| (x.id.clone(), x.name.clone(), x.connected))
            .collect();
        let selected = self.selected.clone();
        let view = cx.entity();
        let _ = window;
        gpui_component::button::Button::new("monitor-switch")
            .ghost()
            .p_0()
            .w_full()
            .h_auto()
            .tooltip(m::SWITCH_MACHINE)
            .child(card)
            .dropdown_menu(move |menu, _, _| {
                let mut menu = menu;
                for (id, name, _) in &list {
                    let (id, view) = (id.clone(), view.clone());
                    menu = menu.item(
                        PopupMenuItem::new(name.clone())
                            .checked(selected.as_deref() == Some(id.as_str()))
                            .on_click(move |_, _, cx| {
                                view.update(cx, |v, cx| v.select_machine(id.clone(), cx))
                            }),
                    );
                }
                menu
            })
            .into_any_element()
    }

    /// VM 容量 23 / 34 under the machine card.
    fn capacity_row(&self, cx: &App) -> Option<impl IntoElement> {
        let k = native_colors(cx);
        let d = self.current()?;
        let running = d.capacity.running;
        let total = running + d.capacity.can_take;
        Some(
            div()
                .px(space::S8)
                .pt(space::S12)
                .flex()
                .flex_col()
                .gap(space::S6)
                .child(
                    div()
                        .flex()
                        .justify_between()
                        .n_footnote()
                        .text_color(k.secondary)
                        .child(m::VM_CAPACITY)
                        .child(
                            div()
                                .n_figures()
                                .child(format!("{running} / {total}")),
                        ),
                )
                .child(crate::ui::native::progress(model::share(running as u64, total as u64), cx)),
        )
    }

    fn toolbar(&self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let k = native_colors(cx);
        let active = window.is_window_active();
        let subtitle = match self.current() {
            Some(d) => match d.sampled_at {
                Some(at) => fill(m::SAMPLED_AT, &[("time", &clock(at))]),
                None => m::WAITING_FIRST.to_string(),
            },
            None => String::new(),
        };
        let view = cx.entity();
        let range = Segmented::new(
            "monitor-range",
            Range::ALL.map(|r| Segment::Label(r.label().into())),
        )
        .selected(self.range.index())
        .disabled(!self.owner || self.selected.is_none())
        .on_change(move |ix, _, cx| {
            let r = Range::ALL[ix.min(2)];
            view.update(cx, |v, cx| v.set_range(r, cx));
        });
        let view = cx.entity();
        let busy = self.load == Load::Busy;
        let refresh = crate::ui::native::buttons::icon_button_outline(
            "monitor-refresh",
            Sym::Refresh,
            m::REFRESH,
            crate::ui::native::IconSize::Regular,
            cx,
        )
        .loading(busy)
        .disabled(!self.owner || self.selected.is_none())
        .on_click(move |_, _, cx| view.update(cx, |v, cx| v.retry(cx)));
        div()
            .id("monitor-toolbar")
            .flex_none()
            .h(height::TOOLBAR)
            .px(space::S32)
            .flex()
            .items_center()
            .gap(space::S12)
            .border_b(metrics::HAIRLINE)
            .border_color(k.separator)
            .on_mouse_down(MouseButton::Left, |e, window, _| {
                if e.click_count >= 2 {
                    window.titlebar_double_click();
                } else {
                    window.start_window_move();
                }
            })
            .child(
                div()
                    .flex_1()
                    .min_w(gpui::Pixels::ZERO)
                    .flex()
                    .flex_col()
                    .child(
                        div()
                            .n_title3()
                            .text_color(if active { k.label } else { k.secondary })
                            .child(m::TITLE),
                    )
                    .when(!subtitle.is_empty(), |d| {
                        d.child(
                            TruncText::new(subtitle)
                                .n_subheadline()
                                .n_figures()
                                .text_color(k.secondary),
                        )
                    }),
            )
            .child(range)
            .child(refresh)
    }

    fn body(&self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let pad = div()
            .w_full()
            .px(space::S32)
            .py(space::S24)
            .flex()
            .flex_col()
            .gap(space::S16);
        if !self.owner {
            return pad
                .child(
                    Empty::new(Sym::Lock, m::NOT_OWNER)
                        .description(m::NOT_OWNER_DESC)
                        .wide(),
                )
                .into_any_element();
        }
        let failed = match &self.load {
            Load::Failed(e) => Some(e.clone()),
            _ => None,
        };
        let retry = |cx: &mut Context<Self>| {
            let view = cx.entity();
            push_button("monitor-retry", m::RETRY, PushStyle::Outline, PushSize::Small, cx)
                .on_click(move |_, _, cx| view.update(cx, |v, cx| v.retry(cx)))
        };
        if let Some(list) = &self.machines
            && list.is_empty()
        {
            let services = self.services.clone();
            return pad
                .child(
                    Empty::new(Sym::Server, m::NO_MACHINES)
                        .description(m::NO_MACHINES_DESC)
                        .action(
                            push_button(
                                "monitor-open-settings",
                                m::OPEN_SETTINGS,
                                PushStyle::Default,
                                PushSize::Small,
                                cx,
                            )
                            .on_click(move |_, _, cx| {
                                crate::views::settings::open_page(
                                    services.clone(),
                                    crate::views::settings::Page::Machines,
                                    cx,
                                )
                            }),
                        )
                        .wide(),
                )
                .into_any_element();
        }
        let Some(d) = self.current().cloned() else {
            // Nothing to show yet: the error in place of the page, or the page's skeleton.
            return match failed {
                Some(e) => pad
                    .child(
                        Alert::new(Sym::Failed, m::ERROR_TITLE)
                            .tone(AlertTone::Destructive)
                            .description(e)
                            .action(retry(cx)),
                    )
                    .into_any_element(),
                None => pad.child(overview::page_skeleton(cx)).into_any_element(),
            };
        };
        let now = now_ms();
        let mut top: Vec<gpui::AnyElement> = vec![];
        if let Some(e) = failed {
            top.push(
                Alert::new(Sym::Failed, m::ERROR_TITLE)
                    .tone(AlertTone::Destructive)
                    .description(e)
                    .action(retry(cx))
                    .into_any_element(),
            );
        }
        if !d.machine.connected
            && let Some(at) = d.machine.last_heartbeat_at
        {
            top.push(
                Alert::new(Sym::Offline, m::OFFLINE_TITLE)
                    .tone(AlertTone::Warning)
                    .description(fill(
                        m::OFFLINE_DESC,
                        &[("ago", &model::ago(now.saturating_sub(at)))],
                    ))
                    .into_any_element(),
            );
        } else if d.machine.connected && d.sampled_at.is_none() {
            top.push(
                Alert::new(Sym::Info, m::NO_METRICS_TITLE)
                    .tone(AlertTone::Neutral)
                    .description(m::NO_METRICS_DESC)
                    .into_any_element(),
            );
        }
        let names = |id: &str| self.member_name(id);
        let provider = self.services.provider.clone();
        let member = move |id: &str| provider.member(id);
        pad.children(top)
            .child(overview::page(
                &d,
                self.range,
                self.sort,
                now,
                &names,
                &member,
                &clock_short,
                cx.entity(),
                window,
                cx,
            ))
            .into_any_element()
    }
}

/// An 8 pt state dot that sits next to its words (never alone, §2.12).
fn dot(color: gpui::Hsla) -> gpui::Div {
    div()
        .flex_none()
        .size(t::SWATCH)
        .rounded_full()
        .bg(color)
}
