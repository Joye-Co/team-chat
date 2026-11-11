//! 账号 (NATIVE.md §8): a header row (avatar 64, name, 岗位 · level) with
//! `编辑…` that opens the 编辑资料 sheet (like Messages › Name and Photo);
//! the group 登录的设备 with the `−` square button under it (an alert signs
//! the selected device out); `退出登录…` at the bottom-leading edge opens an
//! alert that can also delete this Mac's local copy.

use gpui::{
    App, AppContext as _, Context, ElementId, Entity, Focusable as _, IntoElement, ParentElement,
    PathPromptOptions, Render, Styled, Subscription, Window, div, prelude::FluentBuilder as _, px,
};
use gpui_component::{
    Disableable as _, WindowExt as _,
    button::Button,
    input::{InputEvent, InputState},
    menu::{DropdownMenu as _, PopupMenuItem},
};

use super::widgets::{self, Status, answer};
use crate::ui::native::sheet::sheet_status_error;
use crate::views::nav::dialogs::{sheet_form, sheet_form_row};
use crate::{
    app::{Changes, Services},
    copy::{common, fill, settings as s},
    data::{Change, DeviceInfo, DeviceKind, EmailError, Member, SessionState},
    ui::{
        self, Sym,
        native::{
            AlertLayout, CodeInput, CodeInputEvent, GroupedForm, NativeCheckbox, NativeTextField,
            NativeType as _, PushSize, PushStyle, SectionTitle, SheetLayout, SheetWidth,
            alert_button, disabled_push, loading_push, metrics, native_colors, page::outline_badge,
            push_button, tokens,
        },
    },
    views::topic_list::relative_time,
};

/// Whole device rows shown before the list scrolls inside its group (2.9):
/// six, and half of the seventh says there is more.
const DEVICE_ROWS_SHOWN: usize = 6;

pub const NAME_MAX: usize = 64;

/// How far a step of the email sheet travels as it enters.
const EMAIL_STEP_TRAVEL: gpui::Pixels = gpui::px(24.);
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
    /// My member row, read when the session or the members change rather
    /// than on every frame (`session()` reads the store).
    me: Option<Member>,
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
    /// The device whose sign-out failed, for 重试.
    failed_device: Option<String>,
    /// The 退出登录 alert's checkbox.
    wipe: bool,
    /// The scroll of the capped device list.
    devices_scroll: gpui::ScrollHandle,
    /// 登录邮箱: `None` until loaded; `Some(None)` when I have none.
    email: Option<Option<String>>,
    email_status: Status,
    /// The 设置登录邮箱 sheet: the address field, the code slots, the
    /// address the code went to, and the last refusal.
    email_input: Entity<InputState>,
    email_code: Entity<CodeInput>,
    email_sent_to: Option<String>,
    email_busy: bool,
    email_error: Option<EmailError>,
    /// Bumped on every step change of the email sheet, so the new step
    /// plays its enter animation once.
    email_step: u32,
    /// The last step change went back (换个邮箱): the step slides in from
    /// the leading side instead of the trailing one.
    email_back: bool,
    /// 在手机上登录: show a QR code a phone scans, or approve a phone's code.
    phone: Entity<super::account_phone::PhoneSignIn>,
    _subscriptions: Vec<Subscription>,
}

fn read_me(services: &Services) -> Option<Member> {
    match services.provider.session() {
        SessionState::SignedIn { me, .. } => Some(me),
        _ => None,
    }
}

/// The text of a refused email request in 账号.
fn email_error_text(e: EmailError) -> &'static str {
    match e {
        EmailError::BadAddress => s::EMAIL_BAD_ADDRESS,
        EmailError::WrongCode => s::EMAIL_WRONG_CODE,
        EmailError::Taken => s::EMAIL_TAKEN,
        EmailError::RateLimited => s::EMAIL_RATE,
        EmailError::NotSetUp => s::EMAIL_NOT_SET_UP,
        EmailError::Offline => s::ERROR_OFFLINE,
        EmailError::Other => s::ERROR_OTHER,
    }
}

impl AccountPage {
    pub fn new(services: Services, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let name = cx.new(|cx| InputState::new(window, cx).placeholder(s::NAME_PLACEHOLDER));
        let role = cx.new(|cx| InputState::new(window, cx).placeholder(s::ROLE_PLACEHOLDER));
        let email_input = cx.new(|cx| {
            InputState::new(window, cx).placeholder(crate::copy::login::EMAIL_PLACEHOLDER)
        });
        let email_code =
            cx.new(|cx| CodeInput::new(crate::views::login::EMAIL_CODE_LEN, 3, window, cx));
        let phone =
            cx.new(|cx| super::account_phone::PhoneSignIn::new(services.clone(), window, cx));
        let subscriptions = vec![
            cx.subscribe(
                &phone,
                |this, _, _: &super::account_phone::PhoneSignedIn, cx| this.refresh_devices(cx),
            ),
            cx.subscribe_in(
                &services.hub,
                window,
                |this, _, changes: &Changes, window, cx| {
                    if changes.any(|c| matches!(c, Change::Members | Change::Session)) {
                        this.me = read_me(&this.services);
                        this.fill_from_me(window, cx);
                        cx.notify();
                    }
                },
            ),
            cx.subscribe_in(
                &email_input,
                window,
                |this, _, e: &InputEvent, window, cx| match e {
                    InputEvent::PressEnter { .. } => this.send_email_code(window, cx),
                    InputEvent::Change => {
                        this.email_error = None;
                        cx.notify();
                    }
                    _ => {}
                },
            ),
            cx.subscribe_in(
                &email_code,
                window,
                |this, _, e: &CodeInputEvent, window, cx| {
                    this.email_error = None;
                    if matches!(e, CodeInputEvent::Submit) || this.email_digits(cx).len() == 6 {
                        this.verify_email(window, cx);
                    }
                    cx.notify();
                },
            ),
            cx.subscribe_in(&name, window, Self::on_input),
            cx.subscribe_in(&role, window, Self::on_input),
        ];
        let mut this = Self {
            me: read_me(&services),
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
            failed_device: None,
            wipe: false,
            devices_scroll: gpui::ScrollHandle::new(),
            email: None,
            email_status: Status::Idle,
            email_input,
            email_code,
            email_sent_to: None,
            email_busy: false,
            email_error: None,
            email_step: 0,
            email_back: false,
            phone,
            _subscriptions: subscriptions,
        };
        this.fill_from_me(window, cx);
        this
    }

    // --- 登录邮箱 ---

    /// Loads my sign-in email.
    pub fn refresh_email(&mut self, cx: &mut Context<Self>) {
        let reply = self.services.provider.my_email();
        cx.spawn(async move |this, cx| {
            let result = reply.await.unwrap_or(Err(EmailError::Other));
            let _ = this.update(cx, |this, cx| {
                match result {
                    Ok(email) => {
                        this.email = Some(email);
                        this.email_status = Status::Idle;
                    }
                    Err(e) => this.email_status = Status::Failed(email_error_text(e).into()),
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn email_digits(&self, cx: &App) -> String {
        self.email_code
            .read(cx)
            .value(cx)
            .chars()
            .filter(char::is_ascii_digit)
            .collect()
    }

    /// 发送验证码 in the sheet.
    fn send_email_code(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.email_busy {
            return;
        }
        let email = self.email_input.read(cx).value().trim().to_lowercase();
        if !chat_core::proto::is_email_address(&email) {
            self.email_error = Some(EmailError::BadAddress);
            cx.notify();
            return;
        }
        self.email_busy = true;
        self.email_error = None;
        let reply = self.services.provider.add_email_code(&email);
        let started = std::time::Instant::now();
        cx.spawn_in(window, async move |this, cx| {
            let result = reply.await.unwrap_or(Err(EmailError::Other));
            // The spinner stays at least BUSY_MIN, so a fast reply reads as
            // "sent" rather than a flicker before the next step.
            if let Some(rest) = crate::theme::motion::BUSY_MIN.checked_sub(started.elapsed()) {
                cx.background_executor().timer(rest).await;
            }
            let _ = this.update_in(cx, |this, window, cx| {
                this.email_busy = false;
                match result {
                    Ok(()) => {
                        if this.email_sent_to.is_none() {
                            this.email_step += 1;
                            this.email_back = false;
                        }
                        this.email_sent_to = Some(email);
                        this.email_code
                            .update(cx, |c, cx| c.set_value("", window, cx));
                        let code = this.email_code.clone();
                        let handle = code.read(cx).focus_handle(cx);
                        handle.focus(window, cx);
                    }
                    Err(e) => this.email_error = Some(e),
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    /// 完成 (or the sixth digit): confirms the code; the sheet closes.
    fn verify_email(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(email) = self.email_sent_to.clone() else {
            return;
        };
        let code = self.email_digits(cx);
        if self.email_busy || code.len() != 6 {
            return;
        }
        self.email_busy = true;
        let reply = self.services.provider.verify_email(&email, &code);
        cx.spawn_in(window, async move |this, cx| {
            let result = reply.await.unwrap_or(Err(EmailError::Other));
            let _ = this.update_in(cx, |this, window, cx| {
                this.email_busy = false;
                match result {
                    Ok(saved) => {
                        this.email = Some(Some(saved));
                        this.email_sent_to = None;
                        window.close_dialog(cx);
                    }
                    Err(e) => {
                        this.email_error = Some(e);
                        this.email_code.update(cx, |c, cx| c.select_all(window, cx));
                    }
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    fn remove_email(&mut self, cx: &mut Context<Self>) {
        let reply = self.services.provider.remove_email();
        cx.spawn(async move |this, cx| {
            let result = reply.await.unwrap_or(Err(EmailError::Other));
            let _ = this.update(cx, |this, cx| {
                match result {
                    Ok(()) => this.email = Some(None),
                    Err(e) => this.email_status = Status::Failed(email_error_text(e).into()),
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// 换个邮箱: back to the address step, the address selected.
    fn edit_email_address(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.email_step += 1;
        self.email_back = true;
        self.email_sent_to = None;
        self.email_error = None;
        let handle = self.email_input.read(cx).focus_handle(cx);
        handle.focus(window, cx);
        cx.notify();
    }

    /// Self-test: the address step while 发送验证码 is on its way.
    pub fn preview_email_sending(&mut self, cx: &mut Context<Self>) {
        self.email_busy = true;
        cx.notify();
    }

    /// Self-test: the 设置登录邮箱 sheet after 发送验证码 to `to` (the code
    /// slots and the sent line), without a server round trip.
    pub fn preview_email_sent(&mut self, to: &str, window: &mut Window, cx: &mut Context<Self>) {
        self.email_input
            .update(cx, |st, cx| st.set_value(to.to_string(), window, cx));
        self.email_step += 1;
        self.email_sent_to = Some(to.to_string());
        cx.notify();
    }

    /// 添加… / 更改… : the 设置登录邮箱 sheet (the address, 发送验证码, then
    /// the six code slots; 取消 and 完成).
    pub fn open_email_sheet(this: Entity<Self>, window: &mut Window, cx: &mut App) {
        this.update(cx, |p, cx| {
            p.email_sent_to = None;
            p.email_error = None;
            p.email_busy = false;
            let current = p.email.clone().flatten().unwrap_or_default();
            p.email_input
                .update(cx, |st, cx| st.set_value(current, window, cx));
            cx.notify();
        });
        let first = std::cell::Cell::new(true);
        widgets::open_sheet(
            window,
            cx,
            SheetWidth::Form,
            px(500.),
            move |dialog, window, cx| {
                if first.replace(false) {
                    let handle = this.read(cx).email_input.read(cx).focus_handle(cx);
                    handle.focus(window, cx);
                }
                let page = this.read(cx);
                let busy = page.email_busy;
                let sent = page.email_sent_to.clone();
                let error = page.email_error.map(email_error_text);
                let k = native_colors(cx);
                // One centered column, one thing at a time (Stripe / Linear
                // verification): the icon, the title and one line; the
                // address or the six slots; one full-width button; quiet
                // links under it. A ✕ in the corner closes, so the footer
                // has no 取消 / OK pair.
                let full_button = |id: &'static str, label: &'static str, enabled: bool| {
                    loading_push(
                        disabled_push(
                            push_button(id, label, PushStyle::Default, PushSize::Large, cx),
                            !enabled && !busy,
                            cx,
                        ),
                        busy,
                        PushSize::Large,
                    )
                    .w_full()
                    .h(metrics::CODE_SLOT.1)
                    .rounded(metrics::FIELD_RADIUS)
                };
                let link = |id: &'static str, label: &'static str| {
                    push_button(id, label, PushStyle::Link, PushSize::Small, cx)
                        .px(tokens::space::S4)
                };
                let (title, hint, content, primary, links): (
                    &'static str,
                    String,
                    gpui::AnyElement,
                    Option<Button>,
                    Option<gpui::Div>,
                ) = match &sent {
                    None => {
                        let send = this.clone();
                        (
                            s::EMAIL_SHEET_TITLE,
                            s::EMAIL_SHEET_HINT.to_string(),
                            NativeTextField::new("email-field", &page.email_input)
                                .full_width()
                                .size(crate::ui::native::FieldSize::Hero)
                                .leading(crate::ui::icons::icon(
                                    Sym::Mention,
                                    tokens::icon::MD,
                                    k.secondary,
                                ))
                                .error(error.is_some())
                                .into_any_element(),
                            Some(
                                full_button(
                                    "email-send",
                                    if busy {
                                        s::EMAIL_SENDING
                                    } else {
                                        s::EMAIL_SEND
                                    },
                                    !busy,
                                )
                                .on_click(move |_, window, cx| {
                                    send.update(cx, |p, cx| p.send_email_code(window, cx))
                                }),
                            ),
                            None,
                        )
                    }
                    Some(to) => {
                        let (resend, back) = (this.clone(), this.clone());
                        // The sixth digit submits (no 完成 button); while it
                        // checks, the link row says so.
                        let links = if busy {
                            div()
                                .h(metrics::BUTTON_SMALL)
                                .flex()
                                .items_center()
                                .n_body()
                                .text_color(k.secondary)
                                .child(s::EMAIL_VERIFYING)
                        } else {
                            div()
                                .flex()
                                .items_center()
                                .justify_center()
                                .gap(tokens::space::S4)
                                .n_body()
                                .text_color(k.secondary)
                                .child(s::EMAIL_NOT_RECEIVED)
                                .child(
                                    disabled_push(link("email-resend", s::EMAIL_RESEND), busy, cx)
                                        .on_click(move |_, window, cx| {
                                            resend.update(cx, |p, cx| p.send_email_code(window, cx))
                                        }),
                                )
                                .child(crate::copy::native_kit::MIDDLE_DOT)
                                .child(link("email-other", s::EMAIL_OTHER_ADDRESS).on_click(
                                    move |_, window, cx| {
                                        back.update(cx, |p, cx| p.edit_email_address(window, cx))
                                    },
                                ))
                        };
                        (
                            s::EMAIL_CODE_TITLE,
                            fill(s::EMAIL_CODE_HINT, &[("email", to)]),
                            div()
                                .w_full()
                                .flex()
                                .justify_center()
                                .child(page.email_code.clone())
                                .into_any_element(),
                            None,
                            Some(links),
                        )
                    }
                };
                let close = crate::ui::native::icon_button(
                    "email-close",
                    crate::ui::sf_symbols::Sf::Close,
                    common::CLOSE,
                    crate::ui::native::IconSize::Small,
                    window,
                    cx,
                )
                .on_click(|_, window, cx| window.close_dialog(cx));
                let ok = this.clone();
                dialog
                    .on_ok(move |_, window, cx| {
                        ok.update(cx, |p, cx| {
                            if p.email_sent_to.is_some() {
                                p.verify_email(window, cx)
                            } else {
                                p.send_email_code(window, cx)
                            }
                        });
                        false
                    })
                    .on_cancel(|_, _, _| true)
                    .child({
                        // The step below the icon slides in (24 pt from the
                        // trailing side, or the leading side on 换个邮箱) and
                        // fades, once per step change; the icon and ✕ stay.
                        let step = div()
                            .w_full()
                            .flex()
                            .flex_col()
                            .items_center()
                            .child(
                                div()
                                    .pt(tokens::space::S16)
                                    .n_title2()
                                    .text_color(k.label)
                                    .child(title),
                            )
                            .child(
                                div()
                                    .pt(tokens::space::S8)
                                    .n_body()
                                    .text_center()
                                    .text_color(k.secondary)
                                    .child(hint),
                            )
                            .child(div().w_full().pt(tokens::space::S24).child(content))
                            .when_some(error, |d, e| {
                                d.child(
                                    div()
                                        .pt(tokens::space::S12)
                                        .flex()
                                        .justify_center()
                                        .child(sheet_status_error(e, cx)),
                                )
                            })
                            .when_some(primary, |d, b| {
                                d.child(div().w_full().pt(tokens::space::S24).child(b))
                            })
                            .when_some(links, |d, l| {
                                d.child(div().pt(tokens::space::S24).child(l))
                            });
                        let (seq, back) = (page.email_step, page.email_back);
                        let shift = if back {
                            -EMAIL_STEP_TRAVEL
                        } else {
                            EMAIL_STEP_TRAVEL
                        };
                        let step = if seq == 0 {
                            step.into_any_element()
                        } else {
                            crate::ui::motion::enter(
                                ElementId::Name(format!("email-step-{seq}").into()),
                                crate::theme::motion::SLOW,
                                step.relative(),
                                move |el, t| el.left(shift * (1. - t)),
                                cx,
                            )
                        };
                        div()
                            .relative()
                            .w_full()
                            .px(tokens::space::S32)
                            .pt(tokens::space::S32)
                            .pb(tokens::space::S24)
                            .flex()
                            .flex_col()
                            .items_center()
                            .overflow_hidden()
                            .child(
                                div()
                                    .absolute()
                                    .top(tokens::space::S12)
                                    .right(tokens::space::S12)
                                    .child(close),
                            )
                            .child(crate::ui::native::feedback::icon_box(Sym::Mention, cx))
                            .child(step)
                    })
            },
        );
    }

    /// 登录邮箱: my address (or 还没有添加) with 添加… / 更改…, and "⋯" ›
    /// 移除登录邮箱; a failed load shows the Alert with 重试.
    fn email_section(&self, window: &Window, cx: &mut Context<Self>) -> gpui::Div {
        let header = SectionTitle::new(s::EMAIL).description(s::EMAIL_HINT);
        let body: gpui::AnyElement = match (&self.email, &self.email_status) {
            (None, Status::Failed(t)) => widgets::failed_alert(
                "email-failed",
                s::EMAIL_FAILED,
                Some(t.clone()),
                Some(
                    widgets::retry_button("email-retry", cx)
                        .on_click(cx.listener(|this, _, _, cx| this.refresh_email(cx))),
                ),
                cx,
            ),
            (None, _) => GroupedForm::new()
                .row(widgets::list_loading("email-loading", 1, false, cx))
                .into_any_element(),
            (Some(email), _) => {
                let page = cx.entity();
                let has = email.is_some();
                let edit = push_button(
                    "email-edit",
                    if has { s::EMAIL_CHANGE } else { s::EMAIL_ADD },
                    PushStyle::Outline,
                    PushSize::Small,
                    cx,
                )
                .on_click({
                    let page = page.clone();
                    move |_, window, cx| Self::open_email_sheet(page.clone(), window, cx)
                });
                let mut row = widgets::ItemRow::new(
                    "email-row",
                    email.clone().unwrap_or_else(|| s::EMAIL_NONE.to_string()),
                )
                .media(widgets::icon_box_28(Sym::Mention, cx))
                .muted(!has)
                .value(edit);
                if has {
                    let more = widgets::more_button("email-more", window, cx).dropdown_menu(
                        move |menu, _, _| {
                            let page = page.clone();
                            menu.item(PopupMenuItem::new(s::EMAIL_REMOVE_ITEM).on_click(
                                move |_, _, cx| page.update(cx, |p, cx| p.remove_email(cx)),
                            ))
                        },
                    );
                    row = row.menu(more);
                }
                GroupedForm::new().row(row).into_any_element()
            }
        };
        widgets::section(header, body)
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
        self.me.clone()
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

    pub fn phone(&self) -> &Entity<super::account_phone::PhoneSignIn> {
        &self.phone
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
            // A refusal stays in the sheet as an Alert with 重试 (no
            // second modal over it).
            if refused.is_none() {
                let _ = window_handle.update(cx, |_, window, cx| {
                    if window.has_active_dialog(cx) {
                        window.close_dialog(cx);
                    }
                });
            }
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

    /// The failed state of 登录的设备 (a self-test scene).
    pub fn preview_devices_failed(&mut self, text: &str, cx: &mut Context<Self>) {
        self.devices = None;
        self.devices_status = Status::Failed(text.to_string().into());
        cx.notify();
    }

    pub fn select_device(&mut self, id: Option<String>, cx: &mut Context<Self>) {
        self.selected_device = id;
        cx.notify();
    }

    fn selected(&self) -> Option<DeviceInfo> {
        let id = self.selected_device.as_ref()?;
        self.devices.as_ref()?.iter().find(|d| &d.id == id).cloned()
    }

    fn sign_out_device(&mut self, id: String, cx: &mut Context<Self>) {
        let reply = self.services.provider.sign_out_device(&id);
        self.devices_status = Status::Idle;
        self.failed_device = None;
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
                    Err(e) => {
                        this.failed_device = Some(id.clone());
                        this.devices_status = Status::from_error(e);
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// "⋯" › 退出登录… on the selected device: an alert; this Mac signs out the
    /// whole app instead.
    pub fn confirm_remove_device(this: Entity<Self>, window: &mut Window, cx: &mut App) {
        let Some(device) = this.read(cx).selected() else {
            return;
        };
        if device.this_mac {
            Self::confirm_sign_out(this, window, cx);
            return;
        }
        // The title stays one line; the name goes to the body, cut at
        // two lines' worth.
        let full = device_name(&device);
        let name = if full.chars().count() > 40 {
            format!("{}…", full.chars().take(40).collect::<String>())
        } else {
            full
        };
        let id = device.id.clone();
        widgets::open_alert(window, cx, move |_, cx| {
            let page = this.clone();
            let id = id.clone();
            AlertLayout::new(s::DEVICE_REMOVE_TITLE)
                .icon(widgets::app_icon(cx))
                .message(fill(s::DEVICE_REMOVE_BODY, &[("name", &name)]))
                .buttons([
                    alert_button(
                        "device-remove-ok",
                        s::DEVICE_REMOVE_CONFIRM,
                        PushStyle::Outline,
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
                        PushStyle::Outline,
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
                        PushStyle::Outline,
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
                        PushStyle::Outline,
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
        widgets::open_sheet(
            window,
            cx,
            SheetWidth::Form,
            px(500.),
            move |dialog, window, cx| {
                if first.replace(false) {
                    let handle = this.read(cx).name.read(cx).focus_handle(cx);
                    handle.focus(window, cx);
                }
                let page = this.read(cx);
                let me = page.me();
                let busy = page.save.busy();
                // The error of the last 完成 (an empty name), or a field
                // that is too long while typing: the field turns
                // destructive, its label too, and the message sits under it.
                let typed_name = page.name.read(cx).value().to_string();
                let problem = validate(&typed_name, &page.role.read(cx).value()).err();
                let shown = page.field_error.clone().or_else(|| {
                    problem
                        .clone()
                        .filter(|(f, _)| !(*f == Field::Name && typed_name.trim().is_empty()))
                });
                let invalid = problem.is_some();
                let bad = |f: Field| shown.as_ref().is_some_and(|(g, _)| *g == f);
                let choose = this.clone();
                let (cancel, done, ok) = (this.clone(), this.clone(), this.clone());
                // Header row: avatar 64 and 更换头像… with its state.
                let avatar_note = match &page.avatar {
                    Status::Idle => None,
                    Status::Busy(t) | Status::Done(t) => Some(widgets::quiet(t.clone(), cx)),
                    // Foreground text after a red icon (§2.8 v15, K-03).
                    Status::Failed(t) => Some(crate::ui::native::sheet_status_error(t.clone(), cx)),
                };
                let photo = div()
                    .w_full()
                    .flex()
                    .items_center()
                    .gap(tokens::space::S16)
                    .child(account_avatar(me.as_ref(), tokens::avatar::XL, cx))
                    .child(
                        div()
                            .flex_1()
                            .min_w(gpui::Pixels::ZERO)
                            .flex()
                            .flex_col()
                            .items_start()
                            .gap(tokens::space::S8)
                            .child(
                                push_button(
                                    "profile-photo",
                                    s::CHANGE_PHOTO,
                                    PushStyle::Outline,
                                    PushSize::Small,
                                    cx,
                                )
                                .disabled(page.avatar.busy())
                                .on_click(move |_, _, cx| {
                                    choose.update(cx, |p, cx| p.choose_avatar(cx))
                                }),
                            )
                            .child(
                                avatar_note.unwrap_or_else(|| widgets::quiet(s::AVATAR_HINT, cx)),
                            ),
                    );
                // A long name or role ends with "…" at rest (C1-02).
                let fields = sheet_form()
                    .child(sheet_form_row(
                        s::NAME,
                        widgets::field_at_rest(
                            "profile-name-rest",
                            NativeTextField::new("profile-name", &page.name)
                                .full_width()
                                .error(bad(Field::Name)),
                            &page.name,
                            window,
                            cx,
                        ),
                        cx,
                    ))
                    .child(sheet_form_row(
                        s::ROLE,
                        widgets::field_at_rest(
                            "profile-role-rest",
                            NativeTextField::new("profile-role", &page.role)
                                .full_width()
                                .error(bad(Field::Role)),
                            &page.role,
                            window,
                            cx,
                        ),
                        cx,
                    ));
                let failed = match &page.save {
                    Status::Failed(t) => Some(widgets::failed_alert(
                        "profile-failed",
                        s::PROFILE_FAILED,
                        Some(t.clone()),
                        Some(widgets::retry_button("profile-retry", cx).on_click({
                            let page = this.clone();
                            move |_, window, cx| page.update(cx, |p, cx| p.save(window, cx))
                        })),
                        cx,
                    )),
                    _ => None,
                };
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
                            .when_some(failed, |l, f| l.body(f))
                            // The field error sits in the footer's left
                            // slot, so the sheet keeps its height (2.8).
                            .when_some(shown.clone(), |l, (_, e)| {
                                l.status(sheet_status_error(e, cx))
                            })
                            .buttons([
                                push_button(
                                    "profile-cancel",
                                    common::CANCEL,
                                    PushStyle::Outline,
                                    PushSize::Regular,
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
                                        PushSize::Regular,
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
            },
        );
    }

    fn header(&self, cx: &mut Context<Self>) -> gpui::Div {
        let k = native_colors(cx);
        let me = self.me();
        let name = me
            .as_ref()
            .map(|m| {
                // Beside the avatar a leading emoji is left out (X-01).
                crate::ui::avatar::name_beside_avatar(&m.name)
                    .split_whitespace()
                    .collect::<Vec<_>>()
                    .join(" ")
            })
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
            .gap(crate::ui::native::tokens::space::S16)
            .child(account_avatar(me.as_ref(), px(64.), cx))
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.))
                    .flex()
                    .flex_col()
                    .gap(crate::ui::native::tokens::space::S2)
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
                    push_button(
                        "profile-edit",
                        s::EDIT,
                        PushStyle::Outline,
                        PushSize::Regular,
                        cx,
                    ),
                    me.is_none(),
                    cx,
                )
                .on_click(move |_, window, cx| Self::open_edit(page.clone(), window, cx)),
            )
    }

    /// One device (2.19): a 28 icon box, the name, "这台 Mac" as an
    /// outline badge, when it was last used, and "⋯" › 退出登录….
    fn device_row(
        &self,
        d: &DeviceInfo,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let now = crate::selftest::clock::local_now();
        let when = if d.this_mac {
            s::DEVICE_IN_USE.to_string()
        } else {
            match d.last_seen_at {
                Some(at) if at > 0 => {
                    fill(s::DEVICE_SEEN, &[("when", &relative_time(at as i64, now))])
                }
                _ => s::DEVICE_NEVER_USED.to_string(),
            }
        };
        let (page, id) = (cx.entity(), d.id.clone());
        let more = widgets::more_button(
            ElementId::Name(format!("device-more-{}", d.id).into()),
            window,
            cx,
        )
        .dropdown_menu(move |menu, _, _| {
            let (page, id) = (page.clone(), id.clone());
            menu.item(
                PopupMenuItem::new(s::DEVICE_SIGN_OUT_ITEM).on_click(move |_, window, cx| {
                    page.update(cx, |p, cx| p.select_device(Some(id.clone()), cx));
                    Self::confirm_remove_device(page.clone(), window, cx);
                }),
            )
        });
        widgets::ItemRow::new(
            ElementId::Name(format!("device-{}", d.id).into()),
            device_name(d),
        )
        .media(widgets::icon_box_28(device_symbol(d.kind), cx))
        .when(d.this_mac, |r| r.title_lead(outline_badge(s::THIS_MAC, cx)))
        .meta(when)
        .menu(more)
        .into_any_element()
    }

    /// 登录的设备: the list in its group, capped with a scroll inside it,
    /// or its loading, empty or failed state.
    fn devices_section(&self, window: &Window, cx: &mut Context<Self>) -> gpui::Div {
        let mut header = SectionTitle::new(s::DEVICES).description(s::DEVICES_HINT);
        let body: gpui::AnyElement = match (&self.devices, &self.devices_status) {
            (Some(list), _) if !list.is_empty() => {
                header = header.count(list.len() as u32);
                let rows: Vec<gpui::AnyElement> = list
                    .clone()
                    .iter()
                    .map(|d| self.device_row(d, window, cx))
                    .collect();
                GroupedForm::new()
                    .row(widgets::capped_list(
                        "device-rows",
                        widgets::item_rows(rows, tokens::height::SM, cx),
                        widgets::half_row_cap(metrics::FORM_ROW, DEVICE_ROWS_SHOWN),
                        &self.devices_scroll,
                    ))
                    .into_any_element()
            }
            (Some(_), _) => GroupedForm::new()
                .row(widgets::list_empty(Sym::Laptop, s::DEVICES_EMPTY, None, cx))
                .into_any_element(),
            (None, Status::Failed(t)) => widgets::failed_alert(
                "devices-failed",
                s::DEVICES_FAILED,
                Some(t.clone()),
                Some(
                    widgets::retry_button("devices-retry", cx)
                        .on_click(cx.listener(|this, _, _, cx| this.refresh_devices(cx))),
                ),
                cx,
            ),
            (None, _) => GroupedForm::new()
                .row(widgets::list_loading("devices-loading", 2, false, cx))
                .into_any_element(),
        };
        // A device that did not sign out: the Alert under the list, 重试
        // signs it out again.
        let sign_out_failed = match (&self.devices, &self.devices_status, &self.failed_device) {
            (Some(_), Status::Failed(t), Some(id)) => {
                let id = id.clone();
                Some(widgets::failed_alert(
                    "device-sign-out-failed",
                    s::DEVICE_SIGN_OUT_FAILED,
                    Some(t.clone()),
                    Some(widgets::retry_button("device-sign-out-retry", cx).on_click(
                        cx.listener(move |this, _, _, cx| this.sign_out_device(id.clone(), cx)),
                    )),
                    cx,
                ))
            }
            _ => None,
        };
        widgets::section(
            header,
            div()
                .w_full()
                .flex()
                .flex_col()
                .gap(tokens::space::S12)
                .child(body)
                .children(sign_out_failed),
        )
    }
}

impl Render for AccountPage {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let signed_in = self.me.is_some();
        let sign_out_page = cx.entity();
        widgets::pane()
            .child(GroupedForm::new().row(self.header(cx)))
            .child(self.email_section(window, cx))
            .child(self.phone.clone())
            .child(self.devices_section(window, cx))
            .child(
                div().flex().child(
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
        _ => ui::MemberAvatar::new(me, size)
            .on(k.window)
            .into_any_element(),
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
