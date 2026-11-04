//! 通用 (NATIVE.md §8): 启动时, 登录时启动, 菜单栏里显示 and 声音; the
//! version with 检查更新; 开发者视图; 发送诊断… opens a sheet.

use gpui::{
    AppContext as _, Context, Entity, Focusable as _, IntoElement, ParentElement, Render, Styled,
    Subscription, Window, div, prelude::FluentBuilder as _, px,
};
use gpui_component::{
    Sizable as _, WindowExt as _,
    input::{InputEvent, Textarea, TextareaState},
    menu::{DropdownMenu as _, PopupMenuItem},
};

use super::{
    prefs,
    widgets::{self, Status, answer},
};
use crate::{
    app::{Changes, Services},
    copy::{common, dev, fill, overlays as o, settings as s, update as u},
    data::{Change, ConnectionState, SessionState},
    ui::native::{
        FormRow, GroupedForm, NativeSwitch, NativeType as _, PushSize, PushStyle, SheetLayout,
        SheetWidth, determinate_bar, disabled_push, metrics, native_colors, popup_button,
        push_button, spinner,
    },
    updater::{self, Phase, banner::banner_text, version},
};

pub const DIAGNOSTICS_MAX: usize = 2000;

pub struct GeneralPage {
    services: Services,
    updater: Option<Entity<updater::Updater>>,
    description: Entity<TextareaState>,
    status: Status,
    _subscriptions: Vec<Subscription>,
}

impl GeneralPage {
    pub fn new(services: Services, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let description = cx.new(|cx| {
            TextareaState::new(window, cx)
                .auto_grow(4, 8)
                .placeholder(s::DIAGNOSTICS_PLACEHOLDER)
        });
        let updater = updater::global(cx);
        let mut subscriptions = vec![
            cx.subscribe(&services.hub, |_, _, changes: &Changes, cx| {
                if changes.any(|c| matches!(c, Change::Session | Change::Connection)) {
                    cx.notify();
                }
            }),
            cx.observe(&services.nav, |_, _, cx| cx.notify()),
            cx.subscribe(&description, |this, _, e: &InputEvent, cx| {
                if matches!(e, InputEvent::Change) && matches!(this.status, Status::Failed(_)) {
                    this.status = Status::Idle;
                    cx.notify();
                }
            }),
        ];
        if let Some(u) = &updater {
            subscriptions.push(cx.observe(u, |_, _, cx| cx.notify()));
        }
        Self {
            services,
            updater,
            description,
            status: Status::Idle,
            _subscriptions: subscriptions,
        }
    }

    pub fn status(&self) -> &Status {
        &self.status
    }

    pub fn set_description(&mut self, text: &str, window: &mut Window, cx: &mut Context<Self>) {
        self.description
            .update(cx, |st, cx| st.set_value(text.to_string(), window, cx));
    }

    fn server(&self) -> String {
        match self.services.provider.session() {
            SessionState::SignedIn { server_url, .. } => server_url,
            _ => String::new(),
        }
    }

    pub fn send(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.status.busy() {
            return;
        }
        let text = self.description.read(cx).value().trim().to_string();
        if text.is_empty() {
            self.status = Status::Failed(s::DIAGNOSTICS_REQUIRED.into());
            cx.notify();
            return;
        }
        if let Err(e) = widgets::check_len(&text, DIAGNOSTICS_MAX) {
            self.status = Status::Failed(e.into());
            cx.notify();
            return;
        }
        self.status = Status::Busy(s::DIAGNOSTICS_SENDING.into());
        cx.notify();
        let reply = self.services.provider.send_diagnostics(text);
        let description = self.description.downgrade();
        let window_handle = window.window_handle();
        cx.spawn(async move |this, cx| {
            let result = answer(reply).await;
            let sent = result.is_ok();
            let _ = this.update(cx, |this, cx| {
                this.status = match result {
                    Ok(id) => Status::Done(fill(s::DIAGNOSTICS_SENT, &[("id", &id)]).into()),
                    Err(e) => Status::from_error(e),
                };
                cx.notify();
            });
            if sent {
                let _ = window_handle.update(cx, |_, window, cx| {
                    if let Some(d) = description.upgrade() {
                        d.update(cx, |st, cx| st.set_value(String::new(), window, cx));
                    }
                    if window.has_active_dialog(cx) {
                        window.close_dialog(cx);
                    }
                });
            }
        })
        .detach();
    }

    /// 发送诊断… : a sheet with the description and 发送.
    fn open_diagnostics(this: Entity<Self>, window: &mut Window, cx: &mut gpui::App) {
        this.update(cx, |p, _| {
            if !p.status.busy() {
                p.status = Status::Idle;
            }
        });
        let first = std::cell::Cell::new(true);
        widgets::open_sheet(
            window,
            cx,
            SheetWidth::Form,
            px(440.),
            move |dialog, window, cx| {
                let k = native_colors(cx);
                let description = this.read(cx).description.clone();
                let handle = description.read(cx).focus_handle(cx);
                if first.replace(false) {
                    handle.focus(window, cx);
                }
                let focused = handle.is_focused(window);
                let (busy, failed, no_server) = {
                    let page = this.read(cx);
                    (
                        page.status.busy(),
                        matches!(page.status, Status::Failed(_)),
                        page.server().is_empty(),
                    )
                };
                let field = div()
                    .w_full()
                    .px(metrics::FIELD_PAD)
                    .py(px(4.))
                    .rounded(metrics::FIELD_RADIUS)
                    .bg(k.content)
                    .border_1()
                    .border_color(if failed { k.red } else { k.separator })
                    .when(focused, |d| d.shadow(k.focus_ring()))
                    .child(crate::ui::placeholder_input(
                        Textarea::new(&description)
                            .appearance(false)
                            .n_body()
                            .text_color(k.label),
                        window,
                        cx,
                    ));
                let page = this.read(cx);
                let send_page = this.clone();
                let ok_page = this.clone();
                dialog
                    .on_ok(move |_, window, cx| {
                        ok_page.update(cx, |p, cx| p.send(window, cx));
                        false
                    })
                    .child(
                        SheetLayout::new(s::DIAGNOSTICS_TITLE)
                            .explanation(s::DIAGNOSTICS_ROW_HINT)
                            .body(field)
                            .when_some(widgets::native_status(&page.status, cx), |l, st| l.body(st))
                            .buttons([
                                push_button(
                                    "diagnostics-cancel",
                                    common::CANCEL,
                                    PushStyle::Outline,
                                    PushSize::Large,
                                    cx,
                                )
                                .on_click(|_, window, cx| window.close_dialog(cx)),
                                disabled_push(
                                    push_button(
                                        "diagnostics-send",
                                        s::DIAGNOSTICS_SUBMIT,
                                        PushStyle::Default,
                                        PushSize::Large,
                                        cx,
                                    ),
                                    busy || no_server,
                                    cx,
                                )
                                .on_click(move |_, window, cx| {
                                    send_page.update(cx, |p, cx| p.send(window, cx))
                                }),
                            ]),
                    )
            },
        );
    }

    /// The version row (NATIVE.md §8): the update state as its detail and
    /// one control: 检查更新, 更新到 …, the download's progress bar, or
    /// 重启安装 once the update is ready.
    fn update_row(&self, cx: &mut Context<Self>) -> FormRow {
        let k = native_colors(cx);
        let phase = self.updater.as_ref().map(|u| u.read(cx).phase().clone());
        let detail = match &phase {
            None | Some(Phase::Idle) => s::UPDATE_STATE_IDLE.to_string(),
            Some(Phase::Checking) => u::CHECKING.to_string(),
            Some(Phase::UpToDate) => u::UP_TO_DATE.to_string(),
            // The steps go in the group's footer: a detail is one line.
            Some(Phase::Manual(_)) => u::BANNER_MANUAL.to_string(),
            Some(other) => banner_text(other).unwrap_or_default(),
        };
        let row = FormRow::new(fill(u::VERSION, &[("version", version::CURRENT)])).detail(detail);
        let Some(updater) = self.updater.clone() else {
            return row;
        };
        match phase {
            Some(Phase::Available(i)) | Some(Phase::Failed { info: Some(i), .. }) => row.control(
                push_button(
                    "about-update",
                    fill(u::UPDATE_TO, &[("version", &i.version)]),
                    PushStyle::Default,
                    PushSize::Regular,
                    cx,
                )
                .on_click(move |_, _, cx| updater.update(cx, |u, cx| u.start_update(cx))),
            ),
            Some(ref p @ Phase::Downloading { .. }) => {
                row.control(div().w(px(120.)).child(determinate_bar(
                    crate::updater::banner::download_share(p).unwrap_or(0.),
                    None,
                    cx,
                )))
            }
            Some(Phase::Ready { .. }) => row.control(
                push_button(
                    "about-restart",
                    o::UPDATE_RESTART,
                    PushStyle::Default,
                    PushSize::Regular,
                    cx,
                )
                .on_click(move |_, _, cx| updater.update(cx, |u, cx| u.install_now(cx))),
            ),
            Some(Phase::WaitingForSends(_) | Phase::Installing(_) | Phase::Checking) => {
                row.control(spinner().with_size(metrics::SPINNER).color(k.secondary))
            }
            Some(Phase::Manual(_)) => row.control(
                push_button(
                    "about-dismiss",
                    u::DISMISS,
                    PushStyle::Outline,
                    PushSize::Regular,
                    cx,
                )
                .on_click(move |_, _, cx| updater.update(cx, |u, cx| u.dismiss(cx))),
            ),
            Some(Phase::Idle | Phase::UpToDate | Phase::Failed { info: None, .. }) | None => row
                .control(
                    push_button(
                        "about-check",
                        u::CHECK,
                        PushStyle::Outline,
                        PushSize::Regular,
                        cx,
                    )
                    .on_click(move |_, _, cx| updater.update(cx, |u, cx| u.check_now(cx))),
                ),
        }
    }
}

impl Render for GeneralPage {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = prefs::get();
        let developer = self.services.nav.read(cx).developer_view;
        let server = self.server();
        let launch_title = if p.launch_last_topic {
            s::ON_LAUNCH_LAST
        } else {
            s::ON_LAUNCH_HOME
        };
        let switch = |id: &'static str,
                      on: bool,
                      name: &'static str,
                      set: fn(&mut prefs::LocalPrefs, bool)| {
            NativeSwitch::new(id, on)
                .name(name)
                .on_change(move |next, window, _| {
                    prefs::update(|p| set(p, next));
                    window.refresh();
                })
        };
        let launch = popup_button(
            "general-launch",
            launch_title,
            PushSize::Regular,
            window,
            cx,
        )
        .dropdown_menu(move |menu, _, _| {
            [(false, s::ON_LAUNCH_HOME), (true, s::ON_LAUNCH_LAST)]
                .into_iter()
                .fold(menu, |menu, (last, label)| {
                    menu.item(
                        PopupMenuItem::new(label)
                            .checked(last == p.launch_last_topic)
                            .on_click(move |_, window, _| {
                                prefs::update(|p| p.launch_last_topic = last);
                                window.refresh();
                            }),
                    )
                })
        });
        let options = GroupedForm::new()
            .row(FormRow::new(s::ON_LAUNCH).control(launch))
            .row(FormRow::new(s::LAUNCH_AT_LOGIN).control(switch(
                "general-login",
                p.launch_at_login,
                s::LAUNCH_AT_LOGIN,
                |p, v| p.launch_at_login = v,
            )))
            .row(FormRow::new(s::MENU_BAR).control(switch(
                "general-menu-bar",
                p.menu_bar,
                s::MENU_BAR,
                |p, v| p.menu_bar = v,
            )))
            .row(FormRow::new(s::SOUND).control(switch(
                "general-sound",
                p.sound,
                s::SOUND,
                |p, v| p.sound = v,
            )));
        let nav = self.services.nav.clone();
        let mut about = GroupedForm::new().row(self.update_row(cx));
        if let Some(up) = &self.updater
            && matches!(up.read(cx).phase(), Phase::Manual(_))
        {
            about = about.footer(fill(u::MANUAL, &[("product", env!("PRODUCT_NAME"))]));
        }
        if !server.is_empty() {
            about = about.row(FormRow::new(s::SERVER).value(server.clone(), cx));
        }
        let mut dev_group = GroupedForm::new().row(
            FormRow::new(s::DEVELOPER_VIEW)
                .detail(s::DEVELOPER_VIEW_HINT)
                .control(
                    NativeSwitch::new("developer-view", developer)
                        .name(s::DEVELOPER_VIEW)
                        .on_change(move |on, _, cx| {
                            nav.update(cx, |nav, cx| nav.set_developer_view(on, cx))
                        }),
                ),
        );
        if developer {
            let provider = self.services.provider.clone();
            let connection = match provider.connection() {
                ConnectionState::Offline => dev::STATE_OFFLINE.to_string(),
                ConnectionState::TokenUnreadable => dev::STATE_TOKEN_UNREADABLE.to_string(),
                ConnectionState::Connecting => dev::STATE_CONNECTING.to_string(),
                ConnectionState::Online => dev::STATE_ONLINE.to_string(),
                ConnectionState::Reconnecting {
                    attempt,
                    retry_in_ms,
                } => fill(
                    dev::STATE_RECONNECTING,
                    &[
                        ("attempt", &attempt.to_string()),
                        ("ms", &retry_in_ms.to_string()),
                    ],
                ),
            };
            let me = match provider.session() {
                SessionState::SignedIn { me, .. } => me.id,
                _ => String::new(),
            };
            dev_group = dev_group
                .row(FormRow::new(fill(
                    dev::CONNECTION,
                    &[("state", &connection)],
                )))
                .row(FormRow::new(fill(dev::SERVER, &[("url", &server)])))
                .row(FormRow::new(fill(dev::MEMBER_ID, &[("id", &me)])));
        }
        let page = cx.entity();
        widgets::pane()
            .child(options)
            .child(about)
            .child(dev_group)
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(6.))
                    .child(
                        div().flex().child(
                            disabled_push(
                                push_button(
                                    "diagnostics-open",
                                    s::DIAGNOSTICS_OPEN,
                                    PushStyle::Outline,
                                    PushSize::Regular,
                                    cx,
                                ),
                                server.is_empty(),
                                cx,
                            )
                            .on_click(move |_, window, cx| {
                                Self::open_diagnostics(page.clone(), window, cx)
                            }),
                        ),
                    )
                    .children(
                        (!matches!(self.status, Status::Idle))
                            .then(|| widgets::native_status(&self.status, cx))
                            .flatten(),
                    ),
            )
    }
}
