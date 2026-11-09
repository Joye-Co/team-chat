//! 通用 (NATIVE.md §8): 启动时, 登录时启动, 菜单栏里显示 and 声音; the
//! version with 检查更新; AI 服务 for owners (ai_service.rs); 开发者视图;
//! 发送诊断… opens a sheet.

use gpui::{
    App, AppContext as _, Context, Entity, Focusable as _, IntoElement, ParentElement, Render,
    SharedString, Styled, Subscription, Window, div, prelude::FluentBuilder as _, px,
};
use gpui_component::{
    Sizable as _, WindowExt as _,
    input::{InputEvent, TextareaState},
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
    theme::Type as _,
    ui::native::{
        BadgeState, FormRow, GroupedForm, IconSize, NativeSwitch, NativeType as _, PushSize,
        PushStyle, SectionTitle, SheetLayout, SheetWidth, StatusBadge, disabled_push, icon_button,
        metrics, native_colors, progress, push_button, spinner,
        tokens::{height, space},
    },
    updater::{self, Phase, banner::banner_text, version},
};

pub const DIAGNOSTICS_MAX: usize = 2000;

/// What the pane shows about my session, read when the session or the
/// members change rather than on every frame: `session()` reads the store.
#[derive(Default)]
struct Me {
    server: String,
    id: String,
    owner: bool,
}

impl Me {
    fn read(services: &Services) -> Self {
        match services.provider.session() {
            SessionState::SignedIn { me, server_url } => Me {
                owner: me.level == crate::data::MemberLevel::Owner,
                id: me.id,
                server: server_url,
            },
            _ => Me::default(),
        }
    }
}

pub struct GeneralPage {
    services: Services,
    me: Me,
    /// How many versions 更新记录 lists (fixed for this build).
    versions: usize,
    /// AI 服务, shown to owners only.
    ai: Entity<super::ai_service::AiServicePanel>,
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
            cx.subscribe(&services.hub, |this, _, changes: &Changes, cx| {
                if changes.any(|c| matches!(c, Change::Session | Change::Members)) {
                    this.me = Me::read(&this.services);
                    cx.notify();
                } else if changes.any(|c| matches!(c, Change::Connection)) {
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
        let ai = cx.new(|cx| super::ai_service::AiServicePanel::new(services.clone(), window, cx));
        Self {
            me: Me::read(&services),
            versions: updater::notes::history_len(),
            services,
            ai,
            updater,
            description,
            status: Status::Idle,
            _subscriptions: subscriptions,
        }
    }

    pub fn status(&self) -> &Status {
        &self.status
    }

    pub fn ai_service(&self) -> &Entity<super::ai_service::AiServicePanel> {
        &self.ai
    }

    pub fn set_description(&mut self, text: &str, window: &mut Window, cx: &mut Context<Self>) {
        self.description
            .update(cx, |st, cx| st.set_value(text.to_string(), window, cx));
    }

    fn server(&self) -> &str {
        &self.me.server
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
                // The §2.8 textarea (review 3 K-02): the field recipe of
                // every sheet, the error ring when sending failed.
                let field =
                    crate::ui::native::NativeTextArea::new("diagnostics-description", &description)
                        .rows(4)
                        .error(failed)
                        .show_focus(focused);
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
                                    PushSize::Regular,
                                    cx,
                                )
                                .on_click(|_, window, cx| window.close_dialog(cx)),
                                disabled_push(
                                    push_button(
                                        "diagnostics-send",
                                        s::DIAGNOSTICS_SUBMIT,
                                        PushStyle::Default,
                                        PushSize::Regular,
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

    /// The version row (2.19, F49): 当前版本 over the update state, and one
    /// control: 检查更新, 更新到 …, 取消下载, 重启安装 or 知道了. A download
    /// adds its Progress as a dependent row; a manual install adds its steps.
    fn update_rows(&self, window: &Window, cx: &mut Context<Self>) -> Vec<gpui::AnyElement> {
        let k = native_colors(cx);
        let phase = self.updater.as_ref().map(|u| u.read(cx).phase().clone());
        let product = env!("PRODUCT_NAME");
        let detail = match &phase {
            None | Some(Phase::Idle) => s::UPDATE_STATE_IDLE.to_string(),
            Some(Phase::Checking) => u::CHECKING.to_string(),
            Some(Phase::UpToDate) => u::UP_TO_DATE.to_string(),
            Some(Phase::Available(i)) => fill(u::NEW_VERSION, &[("version", &i.version)]),
            Some(Phase::Downloading { info, done, total }) => {
                let total = if *total == 0 { info.size } else { *total };
                fill(
                    u::DOWNLOAD_PROGRESS,
                    &[
                        ("percent", &version::percent(*done, total).to_string()),
                        ("done", &megabytes(*done)),
                        ("total", &megabytes(total)),
                    ],
                )
            }
            Some(Phase::Manual(_)) => u::BANNER_MANUAL.to_string(),
            Some(other) => banner_text(other).unwrap_or_default(),
        };
        let title = fill(u::VERSION_TITLE, &[("version", version::CURRENT)]);
        let share = phase
            .as_ref()
            .and_then(crate::updater::banner::download_share);
        let version_row = |control: Option<gpui::AnyElement>| {
            div()
                .w_full()
                .min_h(height::FORM_ROW_TWO)
                .px(metrics::FORM_ROW_INSET)
                .py(space::S12)
                .flex()
                .items_center()
                .gap(space::S12)
                .child(
                    div()
                        .flex_1()
                        .min_w(gpui::Pixels::ZERO)
                        .flex()
                        .flex_col()
                        .gap(space::S4)
                        .child(div().n_body().text_color(k.label).child(title.clone()))
                        .child(
                            div()
                                .n_subheadline()
                                .n_figures()
                                .text_color(k.secondary)
                                .child(detail.clone()),
                        )
                        // A download shows its Progress under the detail.
                        .children(share.map(|v| div().pt(space::S4).child(progress(v, cx)))),
                )
                .children(control.map(|c| div().flex_none().child(c)))
                .into_any_element()
        };
        let Some(updater) = self.updater.clone() else {
            return vec![version_row(None)];
        };
        let sm = |id: &'static str, label: SharedString, style: PushStyle, cx: &App| {
            push_button(id, label, style, PushSize::Small, cx)
        };
        let mut rows = Vec::new();
        let control: gpui::AnyElement = match phase.clone() {
            Some(Phase::Available(i)) => sm(
                "about-update",
                fill(u::UPDATE_TO, &[("version", &i.version)]).into(),
                PushStyle::Default,
                cx,
            )
            .on_click(move |_, _, cx| updater.update(cx, |u, cx| u.start_update(cx)))
            .into_any_element(),
            Some(Phase::Failed { info: Some(_), .. }) => {
                sm("about-retry", u::RETRY.into(), PushStyle::Outline, cx)
                    .on_click(move |_, _, cx| updater.update(cx, |u, cx| u.start_update(cx)))
                    .into_any_element()
            }
            Some(Phase::Downloading { .. }) => sm(
                "about-cancel-download",
                u::CANCEL_DOWNLOAD.into(),
                PushStyle::Outline,
                cx,
            )
            .on_click(move |_, _, cx| updater.update(cx, |u, cx| u.cancel_download(cx)))
            .into_any_element(),
            Some(Phase::Ready { .. }) => sm(
                "about-restart",
                o::UPDATE_RESTART.into(),
                PushStyle::Default,
                cx,
            )
            .on_click(move |_, _, cx| updater.update(cx, |u, cx| u.install_now(cx)))
            .into_any_element(),
            Some(Phase::WaitingForSends(_) | Phase::Installing(_)) => spinner()
                .with_size(metrics::SPINNER)
                .color(k.secondary)
                .into_any_element(),
            Some(Phase::Manual(_)) => {
                sm("about-dismiss", u::DISMISS.into(), PushStyle::Outline, cx)
                    .on_click(move |_, _, cx| updater.update(cx, |u, cx| u.dismiss(cx)))
                    .into_any_element()
            }
            Some(Phase::Checking) => disabled_push(
                sm("about-check", u::CHECK.into(), PushStyle::Outline, cx),
                true,
                cx,
            )
            .loading(true)
            .into_any_element(),
            Some(Phase::Idle | Phase::UpToDate | Phase::Failed { info: None, .. }) | None => {
                sm("about-check", u::CHECK.into(), PushStyle::Outline, cx)
                    .on_click(move |_, _, cx| updater.update(cx, |u, cx| u.check_now(cx)))
                    .into_any_element()
            }
        };
        rows.push(version_row(Some(control)));
        if let Some(Phase::Manual(_)) = &phase {
            // Each step wraps in the row, never runs past the group (2.21).
            let line = |t: String| {
                div()
                    .w_full()
                    .n_subheadline()
                    .text_color(k.secondary)
                    .child(t)
            };
            rows.push(
                dependent()
                    .flex_col()
                    .items_start()
                    .gap(space::S4)
                    .child(line(u::MANUAL_STEPS.to_string()))
                    .child(line(fill(u::MANUAL_STEP_1, &[("product", product)])))
                    .child(line(fill(u::MANUAL_STEP_2, &[("product", product)])))
                    .child(line(fill(u::MANUAL_STEP_3, &[("product", product)])))
                    .into_any_element(),
            );
        }
        // 查看更新内容: a navigation row when the update has notes.
        if let Some(info) = phase.as_ref().and_then(|p| p.info()).cloned()
            && !info.notes.trim().is_empty()
        {
            rows.push(nav_row(
                "about-notes",
                u::NOTES,
                None,
                window,
                cx,
                move |window, cx| {
                    updater::notes::open_notes(
                        info.clone(),
                        updater::notes::Place::Settings,
                        window,
                        cx,
                    )
                },
            ));
        }
        let p = prefs::get();
        rows.push(
            FormRow::new(u::AUTO_DOWNLOAD)
                .control(
                    NativeSwitch::new("general-auto-download", p.auto_download)
                        .name(u::AUTO_DOWNLOAD)
                        .on_change(|next, window, _| {
                            prefs::update(|p| p.auto_download = next);
                            window.refresh();
                        }),
                )
                .into_any_element(),
        );
        rows.push(
            FormRow::new(u::SHOW_WHATS_NEW)
                .detail(u::SHOW_WHATS_NEW_HINT)
                .min_height(height::FORM_ROW_TWO)
                .control(
                    NativeSwitch::new("general-whats-new", p.show_whats_new)
                        .name(u::SHOW_WHATS_NEW)
                        .on_change(|next, window, _| {
                            prefs::update(|p| p.show_whats_new = next);
                            window.refresh();
                        }),
                )
                .into_any_element(),
        );
        let versions = self.versions;
        if versions > 0 {
            rows.push(nav_row(
                "about-changelog",
                u::CHANGELOG,
                Some(fill(u::CHANGELOG_COUNT, &[("n", &versions.to_string())])),
                window,
                cx,
                |window, cx| {
                    updater::notes::open_changelog(updater::notes::Place::Settings, window, cx)
                },
            ));
        }
        rows
    }
}

/// Megabytes with one decimal under 10 MB (2.21: tabular, separators).
pub(crate) fn megabytes(bytes: u64) -> String {
    let mb = bytes as f64 / (1024. * 1024.);
    if mb < 10. {
        format!("{mb:.1}")
    } else {
        widgets::grouped(mb.round() as u64)
    }
}

/// A dependent row of a group (2.8): full width under its parent row.
fn dependent() -> gpui::Div {
    div()
        .w_full()
        .px(metrics::FORM_ROW_INSET)
        .py(space::S12)
        .flex()
        .items_center()
}

/// A navigation row (2.8): the label, an optional 14 muted value and a
/// chevron-right 16 muted; the whole row opens what it names.
pub fn nav_row(
    id: &'static str,
    label: &'static str,
    value: Option<String>,
    window: &Window,
    cx: &App,
    open: impl Fn(&mut Window, &mut App) + 'static,
) -> gpui::AnyElement {
    let k = native_colors(cx);
    // A ghost button across the row: a Tab stop with the focus ring, Space
    // and Return open it, muted on hover (E3, C1-05).
    crate::ui::native::group_row_button(id, cx)
        .h(metrics::FORM_ROW)
        .px(metrics::FORM_ROW_INSET)
        .accessibility_label(label)
        .on_click(move |_, window, cx| open(window, cx))
        .child(
            div()
                .w_full()
                .flex()
                .items_center()
                .gap(space::S8)
                .child(div().flex_1().n_body().text_color(k.label).child(label))
                .children(value.map(|v| {
                    div()
                        .flex_none()
                        .n_body()
                        .n_figures()
                        .text_color(k.secondary)
                        .child(v)
                }))
                .child(crate::ui::sf_symbols::symbol(
                    crate::ui::sf_symbols::Sf::ChevronRight,
                    crate::ui::native::tokens::icon::MD,
                    crate::ui::sf_symbols::Weight::Regular,
                    k.secondary,
                    window,
                )),
        )
        .into_any_element()
}

impl Render for GeneralPage {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let k = native_colors(cx);
        let p = prefs::get();
        let developer = self.services.nav.read(cx).developer_view;
        let server = self.server().to_string();
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
        let launch = widgets::popup_fixed(
            "general-launch",
            launch_title,
            crate::ui::native::tokens::size::SELECT_WIDTH,
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
        let startup = widgets::section(
            SectionTitle::new(s::SECTION_STARTUP),
            GroupedForm::new()
                .row(FormRow::new(s::ON_LAUNCH).control(launch))
                // The system owns the login item: show what it says.
                .row(
                    FormRow::new(s::LAUNCH_AT_LOGIN).control(
                        NativeSwitch::new(
                            "general-login",
                            super::login_item::shown_on(p.launch_at_login),
                        )
                        .name(s::LAUNCH_AT_LOGIN)
                        .on_change(|next, window, _| {
                            super::login_item::set(next);
                            window.refresh();
                        }),
                    ),
                )
                .row(
                    FormRow::new(s::MENU_BAR).control(
                        NativeSwitch::new("general-menu-bar", p.menu_bar)
                            .name(s::MENU_BAR)
                            .on_change(|next, window, cx| {
                                prefs::update(|p| p.menu_bar = next);
                                crate::views::nav::show_status_item(next, cx);
                                window.refresh();
                            }),
                    ),
                ),
        );
        let alerts = widgets::section(
            SectionTitle::new(s::SECTION_ALERTS),
            GroupedForm::new().row(FormRow::new(s::SOUND).control(switch(
                "general-sound",
                p.sound,
                s::SOUND,
                |p, v| p.sound = v,
            ))),
        );
        let mut about = GroupedForm::new().rows(self.update_rows(window, cx));
        if !server.is_empty() {
            about = about.row(FormRow::new(s::SERVER).control(value_with_copy(
                "general-server",
                server.clone(),
                s::COPY_SERVER,
                false,
                window,
                cx,
            )));
        }
        let version = widgets::section(SectionTitle::new(s::SECTION_VERSION), about);
        let nav = self.services.nav.clone();
        let mut dev_group = GroupedForm::new().row(
            FormRow::new(s::DEVELOPER_VIEW)
                .detail(s::DEVELOPER_VIEW_HINT)
                .min_height(height::FORM_ROW_TWO)
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
            // 连接 is a state, not a value to copy: a StatusBadge in
            // interface words (2.19, F5).
            let (state, word) = match provider.connection() {
                ConnectionState::Offline => (BadgeState::Offline, common::OFFLINE),
                ConnectionState::TokenUnreadable => {
                    (BadgeState::Failed, s::DEV_CONNECTION_TOKEN_UNREADABLE)
                }
                ConnectionState::Connecting => (BadgeState::Working, s::DEV_CONNECTION_CONNECTING),
                ConnectionState::Online => (BadgeState::Online, common::ONLINE),
                ConnectionState::Reconnecting { .. } => {
                    (BadgeState::Working, s::DEV_CONNECTION_RECONNECTING)
                }
            };
            dev_group = dev_group.row(dev_row(
                dev::LABEL_CONNECTION,
                StatusBadge::new(state, word).into_any_element(),
                cx,
            ));
            let me = self.me.id.clone();
            // 服务器 is already in 版本与服务器 with its copy button: one
            // place per fact on a screen (A5, review 3 C1-06).
            dev_group = dev_group.row(dev_row(
                dev::LABEL_MEMBER_ID,
                dev_value_with_copy("dev-member", me, window, cx),
                cx,
            ));
        }
        let page = cx.entity();
        let diagnostics = disabled_push(
            push_button(
                "diagnostics-open",
                s::DIAGNOSTICS_OPEN,
                PushStyle::Outline,
                PushSize::Small,
                cx,
            ),
            server.is_empty(),
            cx,
        )
        .on_click(move |_, window, cx| Self::open_diagnostics(page.clone(), window, cx));
        let retry_page = cx.entity();
        let developer_section = widgets::section(
            SectionTitle::new(s::SECTION_DEVELOPER)
                .description(s::SECTION_DEVELOPER_HINT)
                .action(diagnostics),
            div()
                .w_full()
                .flex()
                .flex_col()
                .gap(space::S12)
                .child(dev_group)
                // The answer of the last 发送诊断 (the sheet shows it while
                // it is open).
                .children(match &self.status {
                    Status::Done(t) => Some(widgets::quiet(t.clone(), cx).into_any_element()),
                    Status::Failed(t) => Some(widgets::failed_alert(
                        "diagnostics-failed",
                        s::DIAGNOSTICS_FAILED,
                        Some(t.clone()),
                        Some(widgets::retry_button("diagnostics-retry", cx).on_click(
                            move |_, window, cx| {
                                Self::open_diagnostics(retry_page.clone(), window, cx)
                            },
                        )),
                        cx,
                    )),
                    _ => None,
                }),
        );
        let _ = k;
        let owner = self.me.owner;
        widgets::pane()
            .when(owner, |p| p.child(admin_card(cx)))
            .child(startup)
            .child(alerts)
            .when(owner, |p| p.child(self.ai.clone()))
            .child(version)
            .child(developer_section)
    }
}

/// A trailing value with its copy button (2.19): 14 muted (13 mono for a
/// developer value), truncating with a tooltip, then a ghost 28 copy.
fn value_with_copy(
    id: &'static str,
    value: String,
    name: &'static str,
    mono: bool,
    window: &Window,
    cx: &App,
) -> gpui::Div {
    use gpui::{ClipboardItem, InteractiveElement as _, StatefulInteractiveElement as _};
    let k = native_colors(cx);
    let tip: SharedString = value.clone().into();
    let copy = value.clone();
    div()
        .min_w(gpui::Pixels::ZERO)
        .max_w(crate::ui::native::tokens::size::SETTINGS_VALUE_MAX)
        .flex()
        .items_center()
        .gap(space::S4)
        .child(
            div()
                .id(SharedString::from(format!("{id}-value")))
                .min_w(gpui::Pixels::ZERO)
                .map(|d| if mono { d.t_code() } else { d.n_body() })
                .text_color(if mono { k.label } else { k.secondary })
                .overflow_hidden()
                .whitespace_nowrap()
                .text_ellipsis()
                .tooltip(move |window, cx| {
                    gpui_component::tooltip::Tooltip::new(tip.clone()).build(window, cx)
                })
                .child(value),
        )
        .child(
            icon_button(
                SharedString::from(format!("{id}-copy")),
                crate::ui::Sym::Copy,
                name,
                IconSize::Small,
                window,
                cx,
            )
            .on_click(move |_, _, cx| {
                cx.write_to_clipboard(ClipboardItem::new_string(copy.clone()))
            }),
        )
}

/// A developer row (2.19 DescriptionList): the label 14 muted in the 112
/// column, then the value across the rest of the row.
fn dev_row(label: &'static str, value: gpui::AnyElement, cx: &App) -> gpui::Div {
    let k = native_colors(cx);
    div()
        .w_full()
        .h(metrics::FORM_ROW)
        .px(metrics::FORM_ROW_INSET)
        .flex()
        .items_center()
        .gap(metrics::LABEL_GAP)
        .child(
            div()
                .flex_none()
                .w(metrics::LABEL_COLUMN)
                .n_body()
                .text_color(k.secondary)
                .child(label),
        )
        .child(
            div()
                .flex_1()
                .min_w(gpui::Pixels::ZERO)
                .flex()
                .items_center()
                .child(value),
        )
}

/// A developer value: 13 mono from the value column, truncating with a
/// tooltip, and the ghost 28 copy pinned at the row's trailing edge, so
/// every copy button sits at one x.
fn dev_value_with_copy(
    id: &'static str,
    value: String,
    window: &Window,
    cx: &App,
) -> gpui::AnyElement {
    use gpui::ClipboardItem;
    let copy = value.clone();
    div()
        .w_full()
        .min_w(gpui::Pixels::ZERO)
        .flex()
        .items_center()
        .gap(space::S8)
        .child(
            crate::ui::truncate::TruncText::new(value)
                .id(SharedString::from(format!("{id}-value")))
                .flex_1()
                .t_code()
                .text_color(native_colors(cx).label),
        )
        .child(
            icon_button(
                SharedString::from(format!("{id}-copy")),
                crate::ui::Sym::Copy,
                s::COPY_VALUE,
                IconSize::Small,
                window,
                cx,
            )
            .on_click(move |_, _, cx| {
                cx.write_to_clipboard(ClipboardItem::new_string(copy.clone()))
            }),
        )
        .into_any_element()
}

/// Owners: the way from 设置 to 团队管理 (its own window), on top of 通用.
fn admin_card(cx: &App) -> impl IntoElement {
    use crate::copy::admin as a;
    use crate::ui::native::{PushSize, PushStyle, push_button, tokens::{monitor as t, radius}};
    let k = native_colors(cx);
    GroupedForm::new().row(
        FormRow::new(a::HEADER_TITLE)
            .detail(a::SETTINGS_CARD_DESC)
            .leading(
                div()
                    .flex_none()
                    .size(t::MACHINE_TILE)
                    .rounded(radius::CONTROL)
                    .bg(k.primary)
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(crate::ui::icons::sym(
                        crate::ui::Sym::Shield,
                        crate::ui::sf_symbols::SymSize::INLINE,
                        k.on_primary,
                    )),
            )
            .control(
                push_button("open-admin", a::SETTINGS_CARD_OPEN, PushStyle::Default, PushSize::Small, cx)
                    .on_click(|_, window, cx| {
                        window.dispatch_action(Box::new(crate::views::admin::OpenTeamAdmin), cx)
                    }),
            ),
    )
}
