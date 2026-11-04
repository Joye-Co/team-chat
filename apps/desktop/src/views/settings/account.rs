//! 账号 (NATIVE.md §8): a header row (avatar 64, name, 岗位 · level) with
//! `编辑…` that opens the 编辑资料 sheet (like Messages › Name and Photo);
//! the group 登录的设备 with the `−` square button under it (an alert signs
//! the selected device out); `退出登录…` at the bottom-leading edge opens an
//! alert that can also delete this Mac's local copy.

use gpui::{
    App, AppContext as _, Context, ElementId, Entity, FocusHandle, Focusable as _,
    InteractiveElement as _, IntoElement, KeyDownEvent, ParentElement, PathPromptOptions, Render,
    Styled, Subscription, Window, div,
    prelude::FluentBuilder as _, px,
};
use gpui_component::{
    Disableable as _, WindowExt as _,
    input::{InputEvent, InputState},
    menu::{ContextMenuExt as _, PopupMenuItem},
};

use super::widgets::{self, Status, answer};
use crate::views::nav::dialogs::{sheet_footnote, sheet_form, sheet_form_row};
use crate::{
    app::{Changes, Services},
    copy::{common, fill, overlays as o, settings as s},
    data::{Change, DeviceInfo, DeviceKind, Member, SessionState},
    ui::{
        self, Sym, SymSize,
        native::{
            AlertLayout, GroupedForm, ListRow, NativeCheckbox, NativeTextField, NativeType as _,
            PushSize, PushStyle, SheetLayout, SheetWidth, alert_button, disabled_push, emphasized,
            metrics, native_colors, push_button, square_button,
        },
    },
    views::topic_list::relative_time,
};

pub const NAME_MAX: usize = 64;
pub const ROLE_MAX: usize = 64;

/// The form field a check failed on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Field {
    Name,
    Role,
}

/// A failed form check: the field and its message.
pub type FieldError = (Field, String);

/// Checks the profile form. Returns the trimmed name and 岗位, or the field
/// that is wrong and why.
pub fn validate(name: &str, role: &str) -> Result<(String, Option<String>), FieldError> {
    let name = name.trim();
    let role = role.trim();
    if name.is_empty() {
        return Err((Field::Name, s::NAME_REQUIRED.to_string()));
    }
    widgets::check_len(name, NAME_MAX).map_err(|e| (Field::Name, e))?;
    widgets::check_len(role, ROLE_MAX).map_err(|e| (Field::Role, e))?;
    Ok((
        name.to_string(),
        (!role.is_empty()).then(|| role.to_string()),
    ))
}

/// The name a device row shows.
fn device_name(d: &DeviceInfo) -> String {
    let n = d.name.split_whitespace().collect::<Vec<_>>().join(" ");
    if n.is_empty() {
        s::DEVICE_UNNAMED.to_string()
    } else {
        n
    }
}

fn device_symbol(kind: DeviceKind) -> Sym {
    match kind {
        DeviceKind::Laptop => Sym::Laptop,
        DeviceKind::Desktop => Sym::Computer,
        DeviceKind::Phone => Sym::Phone,
    }
}

pub struct AccountPage {
    services: Services,
    name: Entity<InputState>,
    role: Entity<InputState>,
    /// The values the fields were last filled from, to keep edits on refresh.
    shown: Option<(String, String)>,
    save: Status,
    field_error: Option<FieldError>,
    avatar: Status,
    devices: Option<Vec<DeviceInfo>>,
    devices_status: Status,
    selected_device: Option<String>,
    device_list: FocusHandle,
    /// The 退出登录 alert's checkbox.
    wipe: bool,
    _subscriptions: Vec<Subscription>,
}

impl AccountPage {
    pub fn new(services: Services, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let name = cx.new(|cx| InputState::new(window, cx).placeholder(s::NAME));
        let role = cx.new(|cx| InputState::new(window, cx).placeholder(s::ROLE));
        let subscriptions = vec![
            cx.subscribe_in(
                &services.hub,
                window,
                |this, _, changes: &Changes, window, cx| {
                    if changes.any(|c| matches!(c, Change::Members | Change::Session)) {
                        this.fill_from_me(window, cx);
                        cx.notify();
                    }
                },
            ),
            cx.subscribe_in(&name, window, Self::on_input),
            cx.subscribe_in(&role, window, Self::on_input),
        ];
        let mut this = Self {
            services,
            name,
            role,
            shown: None,
            save: Status::Idle,
            field_error: None,
            avatar: Status::Idle,
            devices: None,
            devices_status: Status::Idle,
            selected_device: None,
            device_list: cx.focus_handle(),
            wipe: false,
            _subscriptions: subscriptions,
        };
        this.fill_from_me(window, cx);
        this
    }

    fn on_input(
        &mut self,
        _: &Entity<InputState>,
        e: &InputEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match e {
            // Return in a field runs 完成.
            InputEvent::PressEnter { .. } => self.save(window, cx),
            // The error goes once the field is fixed.
            InputEvent::Change if self.field_error.is_some() => {
                let name = self.name.read(cx).value().to_string();
                let role = self.role.read(cx).value().to_string();
                let still = validate(&name, &role).err();
                if still.as_ref().map(|e| e.0) != self.field_error.as_ref().map(|e| e.0) {
                    self.field_error = still;
                    cx.notify();
                }
            }
            _ => {}
        }
    }

    fn me(&self) -> Option<Member> {
        match self.services.provider.session() {
            SessionState::SignedIn { me, .. } => Some(me),
            _ => None,
        }
    }

    /// Fills the fields from my member row unless they were edited.
    fn fill_from_me(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(me) = self.me() else { return };
        let fresh = (me.name.clone(), me.role.clone().unwrap_or_default());
        let current = (
            self.name.read(cx).value().to_string(),
            self.role.read(cx).value().to_string(),
        );
        let untouched = match &self.shown {
            None => true,
            Some(shown) => *shown == current,
        };
        if untouched && self.shown.as_ref() != Some(&fresh) {
            self.set_values(&fresh.0, &fresh.1, window, cx);
            self.shown = Some(fresh);
        }
    }

    /// Puts my saved name and 岗位 back into the fields (取消).
    fn reset_fields(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.shown = None;
        self.field_error = None;
        self.save = Status::Idle;
        self.fill_from_me(window, cx);
        cx.notify();
    }

    pub fn set_values(
        &mut self,
        name: &str,
        role: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.name
            .update(cx, |st, cx| st.set_value(name.to_string(), window, cx));
        self.role
            .update(cx, |st, cx| st.set_value(role.to_string(), window, cx));
    }

    pub fn status(&self) -> &Status {
        &self.save
    }

    pub fn field_error(&self) -> Option<&FieldError> {
        self.field_error.as_ref()
    }

    pub fn devices(&self) -> Option<&[DeviceInfo]> {
        self.devices.as_deref()
    }

    /// Saves the name and 岗位; on success the 编辑资料 sheet closes.
    pub fn save(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.save.busy() {
            return;
        }
        let name = self.name.read(cx).value().to_string();
        let role = self.role.read(cx).value().to_string();
        let (name, role) = match validate(&name, &role) {
            Ok(v) => v,
            Err(e) => {
                self.save = Status::Idle;
                self.field_error = Some(e);
                cx.notify();
                return;
            }
        };
        self.field_error = None;
        self.save = Status::Busy(s::SAVING.into());
        cx.notify();
        let reply = self.services.provider.update_profile(name, role);
        let window_handle = window.window_handle();
        cx.spawn(async move |this, cx| {
            let result = answer(reply).await;
            let refused = result.as_ref().err().copied();
            let _ = this.update(cx, |this, cx| {
                this.save = match result {
                    Ok(m) => {
                        this.shown = Some((m.name.clone(), m.role.clone().unwrap_or_default()));
                        Status::Done(s::PROFILE_SAVED.into())
                    }
                    Err(e) => Status::from_error(e),
                };
                cx.notify();
            });
            let _ = window_handle.update(cx, |_, window, cx| match refused {
                None => {
                    if window.has_active_dialog(cx) {
                        window.close_dialog(cx);
                    }
                }
                // A refusal blocks: an alert over the sheet, which stays.
                Some(e) => crate::views::nav::dialogs::error_alert(
                    o::PROFILE_REFUSED,
                    widgets::error_text(e),
                    |_, _| {},
                    window,
                    cx,
                ),
            });
        })
        .detach();
    }

    fn choose_avatar(&mut self, cx: &mut Context<Self>) {
        if self.avatar.busy() {
            return;
        }
        let paths = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: None,
        });
        let provider = self.services.provider.clone();
        cx.spawn(async move |this, cx| {
            let Ok(Ok(Some(paths))) = paths.await else {
                return;
            };
            let Some(path) = paths.into_iter().next() else {
                return;
            };
            let Ok(reply) = this.update(cx, |this, cx| {
                this.avatar = Status::Busy(s::AVATAR_UPLOADING.into());
                cx.notify();
                provider.upload_avatar(path)
            }) else {
                return;
            };
            let result = answer(reply).await;
            let _ = this.update(cx, |this, cx| {
                this.avatar = match result {
                    Ok(_) => Status::Done(s::AVATAR_DONE.into()),
                    Err(e) => Status::from_error(e),
                };
                cx.notify();
            });
        })
        .detach();
    }

    /// Loads 登录的设备.
    pub fn refresh_devices(&mut self, cx: &mut Context<Self>) {
        if self.devices_status.busy() {
            return;
        }
        self.devices_status = Status::Busy(s::LOADING.into());
        let reply = self.services.provider.devices();
        cx.spawn(async move |this, cx| {
            let result = answer(reply).await;
            let _ = this.update(cx, |this, cx| {
                match result {
                    Ok(list) => {
                        if let Some(sel) = &this.selected_device
                            && !list.iter().any(|d| &d.id == sel)
                        {
                            this.selected_device = None;
                        }
                        this.devices = Some(list);
                        this.devices_status = Status::Idle;
                    }
                    Err(e) => this.devices_status = Status::from_error(e),
                }
                cx.notify();
            });
        })
        .detach();
    }

    pub fn select_device(&mut self, id: Option<String>, cx: &mut Context<Self>) {
        self.selected_device = id;
        cx.notify();
    }

    fn selected(&self) -> Option<DeviceInfo> {
        let id = self.selected_device.as_ref()?;
        self.devices.as_ref()?.iter().find(|d| &d.id == id).cloned()
    }

    fn move_selection(&mut self, down: bool, cx: &mut Context<Self>) {
        let Some(list) = &self.devices else { return };
        if list.is_empty() {
            return;
        }
        let at = self
            .selected_device
            .as_ref()
            .and_then(|id| list.iter().position(|d| &d.id == id));
        let next = match (at, down) {
            (None, _) => 0,
            (Some(i), true) => (i + 1).min(list.len() - 1),
            (Some(i), false) => i.saturating_sub(1),
        };
        self.selected_device = Some(list[next].id.clone());
        cx.notify();
    }

    fn sign_out_device(&mut self, id: String, cx: &mut Context<Self>) {
        let reply = self.services.provider.sign_out_device(&id);
        self.devices_status = Status::Idle;
        cx.spawn(async move |this, cx| {
            let result = answer(reply).await;
            let _ = this.update(cx, |this, cx| {
                match result {
                    // Gone already (signed out elsewhere): the refresh shows it.
                    Ok(()) | Err(crate::data::AdminError::NotFound) => {
                        if let Some(list) = &mut this.devices {
                            list.retain(|d| d.id != id);
                        }
                        this.selected_device = None;
                        this.refresh_devices(cx);
                    }
                    Err(e) => this.devices_status = Status::from_error(e),
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// `−` or ⌫ on the selected device: an alert; this Mac signs out the
    /// whole app instead.
    pub fn confirm_remove_device(this: Entity<Self>, window: &mut Window, cx: &mut App) {
        let Some(device) = this.read(cx).selected() else {
            return;
        };
        if device.this_mac {
            Self::confirm_sign_out(this, window, cx);
            return;
        }
        // An alert title stays short: a long name is cut with an ellipsis.
        let full = device_name(&device);
        let name = if full.chars().count() > 24 {
            format!("{}…", full.chars().take(24).collect::<String>())
        } else {
            full
        };
        let id = device.id.clone();
        widgets::open_alert(window, cx, move |_, cx| {
            let page = this.clone();
            let id = id.clone();
            AlertLayout::new(fill(s::DEVICE_REMOVE_TITLE, &[("name", &name)]))
                .icon(widgets::app_icon(cx))
                .message(s::DEVICE_REMOVE_BODY)
                .buttons([
                    alert_button(
                        "device-remove-ok",
                        s::DEVICE_REMOVE_CONFIRM,
                        PushStyle::Standard,
                        true,
                        cx,
                    )
                    .on_click(move |_, window, cx| {
                        let id = id.clone();
                        page.update(cx, |p, cx| p.sign_out_device(id, cx));
                        window.close_dialog(cx);
                    }),
                    alert_button(
                        "device-remove-cancel",
                        common::CANCEL,
                        PushStyle::Standard,
                        false,
                        cx,
                    )
                    .on_click(|_, window, cx| window.close_dialog(cx)),
                ])
                .into_any_element()
        });
    }

    /// 退出登录… : the alert with 同时删除这台 Mac 上的聊天记录和下载的产出.
    pub fn confirm_sign_out(this: Entity<Self>, window: &mut Window, cx: &mut App) {
        this.update(cx, |p, _| p.wipe = false);
        widgets::open_alert(window, cx, move |_, cx| {
            let wipe = this.read(cx).wipe;
            let (toggle, ok) = (this.clone(), this.clone());
            AlertLayout::new(s::SIGN_OUT_TITLE)
                .icon(widgets::app_icon(cx))
                .message(s::SIGN_OUT_BODY)
                .checkbox(
                    NativeCheckbox::new("sign-out-wipe", wipe)
                        .label(s::SIGN_OUT_DELETE_LOCAL)
                        .on_change(move |on, window, cx| {
                            toggle.update(cx, |p, _| p.wipe = on);
                            window.refresh();
                        }),
                )
                .buttons([
                    alert_button(
                        "sign-out-ok",
                        s::DEVICE_REMOVE_CONFIRM,
                        PushStyle::Standard,
                        true,
                        cx,
                    )
                    .on_click(move |_, window, cx| {
                        let wipe = ok.read(cx).wipe;
                        window.close_dialog(cx);
                        ok.read(cx).services.provider.sign_out(wipe);
                    }),
                    alert_button(
                        "sign-out-cancel",
                        common::CANCEL,
                        PushStyle::Standard,
                        false,
                        cx,
                    )
                    .on_click(|_, window, cx| window.close_dialog(cx)),
                ])
                .into_any_element()
        });
    }

    /// 编辑… : the 编辑资料 sheet (avatar, name and 岗位, 取消 and 完成).
    pub fn open_edit(this: Entity<Self>, window: &mut Window, cx: &mut App) {
        this.update(cx, |p, cx| {
            p.save = Status::Idle;
            p.field_error = None;
            cx.notify();
        });
        let first = std::cell::Cell::new(true);
        widgets::open_sheet(window, cx, SheetWidth::Form, px(500.), move |dialog, window, cx| {
            if first.replace(false) {
                let handle = this.read(cx).name.read(cx).focus_handle(cx);
                handle.focus(window, cx);
            }
            let page = this.read(cx);
            let me = page.me();
            let busy = page.save.busy();
            // A field problem turns 完成 off and shows one quiet footnote;
            // an empty name only turns it off (NATIVE.md §7.4).
            let typed_name = page.name.read(cx).value().to_string();
            let problem = validate(&typed_name, &page.role.read(cx).value()).err();
            let footnote = problem
                .as_ref()
                .filter(|(f, _)| !(*f == Field::Name && typed_name.trim().is_empty()))
                .map(|(_, e)| e.clone());
            let invalid = problem.is_some();
            let avatar_status = widgets::native_status(&page.avatar, cx);
            let choose = this.clone();
            let (cancel, done, ok) = (this.clone(), this.clone(), this.clone());
            let photo = div()
                .w_full()
                .flex()
                .flex_col()
                .items_center()
                .gap(px(10.))
                .child(account_avatar(me.as_ref(), px(88.), cx))
                .child(
                    push_button(
                        "profile-photo",
                        s::CHANGE_PHOTO,
                        PushStyle::Standard,
                        PushSize::Small,
                        cx,
                    )
                    .disabled(page.avatar.busy())
                    .on_click(move |_, _, cx| choose.update(cx, |p, cx| p.choose_avatar(cx))),
                )
                .children(avatar_status);
            let fields = sheet_form()
                .pt(px(4.))
                .child(sheet_form_row(
                    s::NAME,
                    NativeTextField::new("profile-name", &page.name)
                        .width(metrics::FIELD_NAME_WIDTH),
                    cx,
                ))
                .child(sheet_form_row(
                    s::ROLE,
                    NativeTextField::new("profile-role", &page.role)
                        .width(metrics::FIELD_NAME_WIDTH),
                    cx,
                ))
                .children(footnote.map(|f| sheet_footnote(f, cx)));
            dialog
                .on_ok(move |_, window, cx| {
                    ok.update(cx, |p, cx| p.save(window, cx));
                    false
                })
                .on_cancel(move |_, window, cx| {
                    cancel.update(cx, |p, cx| p.reset_fields(window, cx));
                    true
                })
                .child(
                    SheetLayout::new(s::EDIT_PROFILE)
                        .explanation(s::EDIT_PROFILE_HINT)
                        .body(photo)
                        .body(fields)
                        .buttons([
                            push_button(
                                "profile-cancel",
                                common::CANCEL,
                                PushStyle::Standard,
                                PushSize::Large,
                                cx,
                            )
                            .on_click({
                                let page = this.clone();
                                move |_, window, cx| {
                                    page.update(cx, |p, cx| p.reset_fields(window, cx));
                                    window.close_dialog(cx);
                                }
                            }),
                            disabled_push(
                                push_button(
                                    "profile-done",
                                    s::DONE,
                                    PushStyle::Default,
                                    PushSize::Large,
                                    cx,
                                ),
                                busy || invalid,
                                cx,
                            )
                            .on_click(move |_, window, cx| {
                                done.update(cx, |p, cx| p.save(window, cx))
                            }),
                        ]),
                )
        });
    }

    fn header(&self, cx: &mut Context<Self>) -> gpui::Div {
        let k = native_colors(cx);
        let me = self.me();
        let name = me
            .as_ref()
            .map(|m| m.name.split_whitespace().collect::<Vec<_>>().join(" "))
            .unwrap_or_default();
        let role = me
            .as_ref()
            .and_then(|m| m.role.clone())
            .map(|r| r.split_whitespace().collect::<Vec<_>>().join(" "))
            .filter(|r| !r.is_empty())
            .unwrap_or_else(|| s::NO_ROLE.to_string());
        let level = me
            .as_ref()
            .map(|m| widgets::level_text(m.level))
            .unwrap_or_default();
        let page = cx.entity();
        div()
            .w_full()
            .px(metrics::FORM_ROW_INSET)
            .py(px(12.))
            .flex()
            .items_center()
            .gap(px(14.))
            .child(account_avatar(me.as_ref(), px(64.), cx))
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.))
                    .flex()
                    .flex_col()
                    .gap(px(2.))
                    .child(
                        div()
                            .n_title3()
                            .text_color(k.label)
                            .overflow_hidden()
                            .whitespace_nowrap()
                            .text_ellipsis()
                            .child(name),
                    )
                    .child(
                        div()
                            .n_body()
                            .text_color(k.secondary)
                            .overflow_hidden()
                            .whitespace_nowrap()
                            .text_ellipsis()
                            .child(format!("{role} · {level}")),
                    ),
            )
            .child(
                disabled_push(
                    push_button("profile-edit", s::EDIT, PushStyle::Standard, PushSize::Regular, cx),
                    me.is_none(),
                    cx,
                )
                .on_click(move |_, window, cx| Self::open_edit(page.clone(), window, cx)),
            )
    }

    fn device_row(&self, d: &DeviceInfo, window: &Window, cx: &mut Context<Self>) -> ListRow {
        let k = native_colors(cx);
        let selected = self.selected_device.as_deref() == Some(d.id.as_str());
        let focused = self.device_list.is_focused(window);
        let ink = ListRow::selection_ink(selected, focused, emphasized(window), cx);
        let text = ink.map(|i| i.1).unwrap_or(k.label);
        let quiet = if ink.is_some_and(|(fill, _)| fill == k.accent) {
            k.on_accent
        } else {
            k.secondary
        };
        let when = if d.this_mac {
            s::THIS_MAC.to_string()
        } else {
            match d.last_seen_at {
                Some(at) => fill(
                    s::DEVICE_SEEN,
                    &[(
                        "when",
                        &relative_time(at as i64, crate::selftest::clock::local_now()),
                    )],
                ),
                None => s::DEVICE_NEVER_USED.to_string(),
            }
        };
        let id = d.id.clone();
        let list = self.device_list.clone();
        ListRow::new(ElementId::Name(format!("device-{}", d.id).into()))
            .height(metrics::FORM_ROW - px(4.))
            .selected(selected)
            .list_focused(focused)
            .on_click(cx.listener(move |this, _, window, cx| {
                list.focus(window, cx);
                this.select_device(Some(id.clone()), cx);
            }))
            .child(ui::sym(device_symbol(d.kind), SymSize::SIDEBAR, quiet))
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.))
                    .n_body()
                    .text_color(text)
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .text_ellipsis()
                    .child(device_name(d)),
            )
            .child(
                div()
                    .flex_none()
                    .n_body()
                    .n_figures()
                    .text_color(quiet)
                    .child(when),
            )
    }
}

impl Render for AccountPage {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let k = native_colors(cx);
        let signed_in = self.me().is_some();
        let page = cx.entity();
        let rows: Vec<gpui::AnyElement> = match (&self.devices, &self.devices_status) {
            (Some(list), _) if !list.is_empty() => list
                .clone()
                .iter()
                .map(|d| {
                    // The context menu: 退出这台设备的登录… (this Mac: the
                    // 退出登录… alert).
                    let (page, id) = (page.clone(), d.id.clone());
                    div()
                        .id(gpui::ElementId::Name(format!("device-menu-{}", d.id).into()))
                        .child(self.device_row(d, window, cx))
                        .context_menu(move |menu, _, _| {
                            let (page, id) = (page.clone(), id.clone());
                            menu.item(PopupMenuItem::new(o::DEVICE_SIGN_OUT_MENU).on_click(
                                move |_, window, cx| {
                                    page.update(cx, |p, cx| p.select_device(Some(id.clone()), cx));
                                    Self::confirm_remove_device(page.clone(), window, cx);
                                },
                            ))
                        })
                        .into_any_element()
                })
                .enumerate()
                .flat_map(|(i, row)| {
                    // A hairline between rows, inset 12 pt from the group
                    // edges (the list sits 4 pt outside them).
                    let line = (i > 0).then(|| {
                        div()
                            .mx(metrics::FORM_ROW_INSET + px(4.))
                            .h(px(1.))
                            .bg(k.separator)
                            .into_any_element()
                    });
                    line.into_iter().chain(std::iter::once(row))
                })
                .collect(),
            (Some(_), _) => vec![
                div()
                    .px(metrics::FORM_ROW_INSET)
                    .h(metrics::FORM_ROW)
                    .flex()
                    .items_center()
                    .n_body()
                    .text_color(k.secondary)
                    .child(s::DEVICES_EMPTY)
                    .into_any_element(),
            ],
            (None, Status::Failed(t)) => vec![
                div()
                    .px(metrics::FORM_ROW_INSET)
                    .h(metrics::FORM_ROW)
                    .flex()
                    .items_center()
                    .n_body()
                    .text_color(k.secondary)
                    .child(t.clone())
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
        let can_remove = self.selected().is_some();
        let remove_page = page.clone();
        let remove = div()
            .flex_none()
            .h(metrics::SQUARE_BUTTON)
            .rounded(metrics::BUTTON_REGULAR_RADIUS)
            .bg(k.fill)
            .overflow_hidden()
            .when(!can_remove, |d| d.opacity(0.5))
            .child(
                square_button("device-remove", false, s::DEVICE_REMOVE, window, cx)
                    .disabled(!can_remove)
                    .on_click(move |_, window, cx| {
                        Self::confirm_remove_device(remove_page.clone(), window, cx)
                    }),
            );
        let key_page = page.clone();
        let devices = div()
            .id("device-list")
            .track_focus(&self.device_list)
            .on_key_down(move |e: &KeyDownEvent, window, cx| match e.keystroke.key.as_str() {
                "up" | "down" => {
                    let down = e.keystroke.key == "down";
                    key_page.update(cx, |p, cx| p.move_selection(down, cx));
                    cx.stop_propagation();
                }
                "backspace" | "delete" => {
                    Self::confirm_remove_device(key_page.clone(), window, cx);
                    cx.stop_propagation();
                }
                _ => {}
            })
            .child(
                GroupedForm::new()
                    .header(s::DEVICES)
                    // List rows inset their selection; the text lines up
                    // with the other groups' 12 pt.
                    .row(div().py(px(4.)).mx(px(-4.)).flex().flex_col().children(rows))
                    .below(div().flex().child(remove)),
            );
        let sign_out_page = page.clone();
        widgets::pane()
            .child(GroupedForm::new().row(self.header(cx)))
            .child(devices)
            // A failed sign-out of a device shows under the list.
            .children(
                (self.devices.is_some() && matches!(self.devices_status, Status::Failed(_)))
                    .then(|| widgets::native_status(&self.devices_status, cx))
                    .flatten(),
            )
            .child(
                div().pt(px(4.)).flex().child(
                    disabled_push(
                        push_button(
                            "sign-out",
                            s::SIGN_OUT_OPEN,
                            PushStyle::Destructive,
                            PushSize::Regular,
                            cx,
                        ),
                        !signed_in,
                        cx,
                    )
                    .on_click(move |_, window, cx| {
                        Self::confirm_sign_out(sign_out_page.clone(), window, cx)
                    }),
                ),
            )
    }
}

/// My avatar in the 账号 header: a photo, or a letter disk on the quiet
/// `quaternary` fill with `label` initials (never the bright ink disk).
fn account_avatar(me: Option<&Member>, size: gpui::Pixels, cx: &App) -> gpui::AnyElement {
    let k = native_colors(cx);
    match me {
        Some(m) if m.avatar_url.is_none() && !m.is_bot() => div()
            .flex_none()
            .size(size)
            .rounded_full()
            .bg(k.quaternary)
            .flex()
            .items_center()
            .justify_center()
            .text_size(size * 0.42)
            .font_weight(gpui::FontWeight::SEMIBOLD)
            .text_color(k.label)
            .child(ui::avatar::monogram(&m.name, size))
            .into_any_element(),
        _ => ui::MemberAvatar::new(me, size).on(k.window).into_any_element(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn profile_form_rules() {
        assert_eq!(validate("  小王 ", " "), Ok(("小王".to_string(), None)));
        assert_eq!(
            validate("小王", " 设计 "),
            Ok(("小王".to_string(), Some("设计".to_string())))
        );
        assert_eq!(validate("  ", "x").map_err(|e| e.0), Err(Field::Name));
        assert_eq!(
            validate(&"长".repeat(65), "").map_err(|e| e.0),
            Err(Field::Name)
        );
        assert_eq!(
            validate("a", &"长".repeat(65)).map_err(|e| e.0),
            Err(Field::Role)
        );
    }

    #[test]
    fn devices_get_their_symbol_and_a_name() {
        assert_eq!(device_symbol(DeviceKind::Phone), Sym::Phone);
        assert_eq!(device_symbol(DeviceKind::Desktop), Sym::Computer);
        let d = DeviceInfo {
            id: "a".into(),
            name: "  ".into(),
            kind: DeviceKind::Laptop,
            signed_in_at: 0,
            last_seen_at: None,
            this_mac: false,
        };
        assert_eq!(device_name(&d), s::DEVICE_UNNAMED);
    }
}
