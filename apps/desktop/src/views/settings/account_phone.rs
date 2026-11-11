//! 账号 › 在手机上登录 (docs/protocol.md, "Code sign-in"): two rows. 显示二维码
//! opens a sheet with a one-time QR image a phone scans to sign in to my
//! account; it polls every 2 s and says when the phone is in. 输入 opens a
//! sheet for the code a phone shows (手机上选「让电脑帮我登录」): the code,
//! then which device asks, then 允许登录.

use std::time::Duration;

use gpui::{
    App, AppContext as _, Bounds, Context, Entity, EventEmitter, Focusable as _, Hsla, IntoElement,
    ParentElement, Render, Styled, Subscription, Window, canvas, div, prelude::FluentBuilder as _,
    px, rgba,
};
use gpui_component::{
    WindowExt as _,
    button::Button,
    input::{InputEvent, InputState},
};

use super::widgets::{self};
use crate::{
    app::Services,
    copy::{common, fill, settings as s},
    data::{EmailError, PhoneOffer, PhoneOfferState},
    ui::{
        Sym,
        native::{
            GroupedForm, NativeTextField, NativeType as _, PushSize, PushStyle, SectionTitle,
            SheetWidth, disabled_push, loading_push, metrics, native_colors, push_button,
            sheet::sheet_status_error, tokens,
        },
    },
};

/// How often the sheet asks whether the phone used the code.
const POLL: Duration = Duration::from_secs(2);
/// The side of the QR image.
const QR_SIDE: gpui::Pixels = px(184.);

/// A phone signed in; the device list should reload.
pub struct PhoneSignedIn;

/// The 显示二维码 sheet.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Offer {
    Loading,
    Ready {
        offer: PhoneOffer,
        state: PhoneOfferState,
    },
    Failed(EmailError),
}

/// The 输入手机上的码 sheet.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Approve {
    Code,
    Checking,
    Confirm { code: String, device: String },
    Approving { code: String, device: String },
    Done { device: String },
}

pub struct PhoneSignIn {
    services: Services,
    offer: Offer,
    /// Bumped when the QR sheet opens or closes, so an older poll stops.
    offer_run: u64,
    approve: Approve,
    approve_error: Option<EmailError>,
    code: Entity<InputState>,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<PhoneSignedIn> for PhoneSignIn {}

fn error_text(e: EmailError) -> &'static str {
    match e {
        EmailError::WrongCode | EmailError::BadAddress => s::PHONE_WRONG_CODE,
        EmailError::RateLimited => s::EMAIL_RATE,
        EmailError::Offline => s::ERROR_OFFLINE,
        _ => s::ERROR_OTHER,
    }
}

/// The dark modules of `text`'s QR code, as a square of `width` cells.
fn qr_modules(text: &str) -> Option<(usize, Vec<bool>)> {
    let code = qrcode::QrCode::with_error_correction_level(text, qrcode::EcLevel::M).ok()?;
    let width = code.width();
    let dark = code
        .to_colors()
        .into_iter()
        .map(|c| c == qrcode::Color::Dark)
        .collect();
    Some((width, dark))
}

/// The QR image: black modules on a white square with a 2-module quiet zone,
/// in both modes.
fn qr_image(text: &str) -> gpui::AnyElement {
    let Some((width, dark)) = qr_modules(text) else {
        return div().size(QR_SIDE).into_any_element();
    };
    let ink: Hsla = rgba(tokens::QR_INK).into();
    div()
        .size(QR_SIDE + tokens::space::S24)
        .p(tokens::space::S12)
        .rounded(tokens::radius::CARD)
        .bg(rgba(tokens::QR_PAPER))
        .child(
            canvas(
                |_, _, _| {},
                move |bounds, _, window, _| {
                    let cell = bounds.size.width / width as f32;
                    for (i, on) in dark.iter().enumerate() {
                        if !on {
                            continue;
                        }
                        let (x, y) = ((i % width) as f32, (i / width) as f32);
                        // A hair of overlap so neighbors leave no seams.
                        let origin = bounds.origin + gpui::point(cell * x, cell * y);
                        let size = gpui::size(cell + px(0.5), cell + px(0.5));
                        window.paint_quad(gpui::fill(Bounds { origin, size }, ink));
                    }
                },
            )
            .size(QR_SIDE),
        )
        .into_any_element()
}

impl PhoneSignIn {
    pub fn new(services: Services, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let code = cx.new(|cx| InputState::new(window, cx).placeholder(s::PHONE_CODE_PLACEHOLDER));
        let subscriptions =
            vec![
                cx.subscribe_in(&code, window, |this, _, e: &InputEvent, _, cx| match e {
                    InputEvent::PressEnter { .. } => this.check_code(cx),
                    InputEvent::Change => {
                        this.approve_error = None;
                        cx.notify();
                    }
                    _ => {}
                }),
            ];
        Self {
            services,
            offer: Offer::Loading,
            offer_run: 0,
            approve: Approve::Code,
            approve_error: None,
            code,
            _subscriptions: subscriptions,
        }
    }

    /// Asks for a new code and polls it while the sheet shows it.
    fn start_offer(&mut self, cx: &mut Context<Self>) {
        self.offer_run += 1;
        let run = self.offer_run;
        self.offer = Offer::Loading;
        cx.notify();
        let reply = self.services.provider.offer_phone_sign_in();
        cx.spawn(async move |this, cx| {
            let result = reply.await.unwrap_or(Err(EmailError::Other));
            let id = match &result {
                Ok(o) => Some(o.id.clone()),
                Err(_) => None,
            };
            let live = this
                .update(cx, |this, cx| {
                    if this.offer_run != run {
                        return false;
                    }
                    this.offer = match result {
                        Ok(offer) => Offer::Ready {
                            offer,
                            state: PhoneOfferState::Waiting,
                        },
                        Err(e) => Offer::Failed(e),
                    };
                    cx.notify();
                    true
                })
                .unwrap_or(false);
            let (Some(id), true) = (id, live) else {
                return;
            };
            loop {
                cx.background_executor().timer(POLL).await;
                let Ok(Some(reply)) = this.update(cx, |this, _| {
                    (this.offer_run == run).then(|| this.services.provider.phone_offer_state(&id))
                }) else {
                    return;
                };
                let Ok(Ok(state)) = reply.await else {
                    continue;
                };
                let done = state != PhoneOfferState::Waiting;
                let used = matches!(state, PhoneOfferState::Used { .. });
                let _ = this.update(cx, |this, cx| {
                    if this.offer_run != run {
                        return;
                    }
                    if let Offer::Ready { state: s, .. } = &mut this.offer {
                        *s = state;
                    }
                    if used {
                        cx.emit(PhoneSignedIn);
                    }
                    cx.notify();
                });
                if done {
                    return;
                }
            }
        })
        .detach();
    }

    fn stop_offer(&mut self) {
        self.offer_run += 1;
    }

    fn typed_code(&self, cx: &App) -> String {
        self.code.read(cx).value().trim().to_string()
    }

    /// 下一步: which device shows this code.
    fn check_code(&mut self, cx: &mut Context<Self>) {
        if self.approve != Approve::Code {
            return;
        }
        let code = self.typed_code(cx);
        if code.is_empty() {
            return;
        }
        self.approve = Approve::Checking;
        self.approve_error = None;
        cx.notify();
        let reply = self.services.provider.peek_phone_code(&code);
        cx.spawn(async move |this, cx| {
            let result = reply.await.unwrap_or(Err(EmailError::Other));
            let _ = this.update(cx, |this, cx| {
                match result {
                    Ok(device) => this.approve = Approve::Confirm { code, device },
                    Err(e) => {
                        this.approve = Approve::Code;
                        this.approve_error = Some(e);
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// 允许登录.
    fn approve(&mut self, cx: &mut Context<Self>) {
        let Approve::Confirm { code, device } = self.approve.clone() else {
            return;
        };
        self.approve = Approve::Approving {
            code: code.clone(),
            device: device.clone(),
        };
        self.approve_error = None;
        cx.notify();
        let reply = self.services.provider.approve_phone_code(&code);
        cx.spawn(async move |this, cx| {
            let result = reply.await.unwrap_or(Err(EmailError::Other));
            let _ = this.update(cx, |this, cx| {
                match result {
                    Ok(()) => {
                        this.approve = Approve::Done { device };
                        cx.emit(PhoneSignedIn);
                    }
                    Err(e) => {
                        this.approve = Approve::Confirm { code, device };
                        this.approve_error = Some(e);
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// Previews for the screenshot self-test.
    pub fn preview_offer(
        &mut self,
        offer: PhoneOffer,
        state: PhoneOfferState,
        cx: &mut Context<Self>,
    ) {
        self.offer = Offer::Ready { offer, state };
        cx.notify();
    }

    pub fn open_qr_sheet(this: Entity<Self>, window: &mut Window, cx: &mut App) {
        this.update(cx, |p, cx| p.start_offer(cx));
        widgets::open_sheet(
            window,
            cx,
            SheetWidth::Form,
            px(500.),
            move |dialog, window, cx| {
                let page = this.read(cx);
                let k = native_colors(cx);
                let status_line = |text: String, color: Hsla| {
                    div()
                        .pt(tokens::space::S16)
                        .n_body()
                        .text_color(color)
                        .child(text)
                };
                let body: gpui::AnyElement = match &page.offer {
                    Offer::Loading => div()
                        .size(QR_SIDE + tokens::space::S24)
                        .flex()
                        .items_center()
                        .justify_center()
                        .n_body()
                        .text_color(k.secondary)
                        .child(s::PHONE_QR_LOADING)
                        .into_any_element(),
                    Offer::Failed(e) => div()
                        .flex()
                        .flex_col()
                        .items_center()
                        .child(sheet_status_error(error_text(*e), cx))
                        .child(
                            div()
                                .pt(tokens::space::S16)
                                .child(Self::renew_button(&this, cx)),
                        )
                        .into_any_element(),
                    Offer::Ready { offer, state } => {
                        let expired = *state == PhoneOfferState::Expired;
                        let used = matches!(state, PhoneOfferState::Used { .. });
                        div()
                            .flex()
                            .flex_col()
                            .items_center()
                            .child(
                                div()
                                    .when(expired || used, |d| d.opacity(0.2))
                                    .child(qr_image(&offer.qr_text)),
                            )
                            .child(
                                div()
                                    .pt(tokens::space::S12)
                                    .n_title3()
                                    .font_family(crate::theme::mono_font_family())
                                    .text_color(if expired { k.tertiary } else { k.label })
                                    .child(offer.code.clone()),
                            )
                            .child(match state {
                                PhoneOfferState::Waiting => {
                                    status_line(s::PHONE_QR_WAITING.into(), k.secondary)
                                        .into_any_element()
                                }
                                PhoneOfferState::Used { device_name } => div()
                                    .pt(tokens::space::S16)
                                    .flex()
                                    .items_center()
                                    .gap(tokens::space::S6)
                                    .n_body()
                                    .text_color(k.label)
                                    .child(crate::ui::icons::icon(
                                        Sym::Done,
                                        tokens::icon::MD,
                                        k.green,
                                    ))
                                    .child(fill(s::PHONE_QR_USED, &[("name", device_name)]))
                                    .into_any_element(),
                                PhoneOfferState::Expired => div()
                                    .flex()
                                    .flex_col()
                                    .items_center()
                                    .child(status_line(s::PHONE_QR_EXPIRED.into(), k.secondary))
                                    .child(
                                        div()
                                            .pt(tokens::space::S12)
                                            .child(Self::renew_button(&this, cx)),
                                    )
                                    .into_any_element(),
                            })
                            .into_any_element()
                    }
                };
                let close_this = this.clone();
                let close = crate::ui::native::icon_button(
                    "phone-qr-close",
                    crate::ui::sf_symbols::Sf::Close,
                    common::CLOSE,
                    crate::ui::native::IconSize::Small,
                    window,
                    cx,
                )
                .on_click(move |_, window, cx| {
                    close_this.update(cx, |p, _| p.stop_offer());
                    window.close_dialog(cx)
                });
                let cancel_this = this.clone();
                dialog
                    .on_cancel(move |_, _, cx| {
                        cancel_this.update(cx, |p, _| p.stop_offer());
                        true
                    })
                    .child(sheet_column(
                        close,
                        Sym::Phone,
                        s::PHONE_QR_TITLE,
                        s::PHONE_QR_HINT.to_string(),
                        body,
                        cx,
                    ))
            },
        );
    }

    fn renew_button(this: &Entity<Self>, cx: &App) -> Button {
        let this = this.clone();
        push_button(
            "phone-qr-renew",
            s::PHONE_QR_RENEW,
            PushStyle::Outline,
            PushSize::Regular,
            cx,
        )
        .on_click(move |_, _, cx| this.update(cx, |p, cx| p.start_offer(cx)))
    }

    pub fn open_code_sheet(this: Entity<Self>, window: &mut Window, cx: &mut App) {
        this.update(cx, |p, cx| {
            p.approve = Approve::Code;
            p.approve_error = None;
            p.code.update(cx, |st, cx| st.set_value("", window, cx));
            cx.notify();
        });
        let first = std::cell::Cell::new(true);
        widgets::open_sheet(
            window,
            cx,
            SheetWidth::Form,
            px(460.),
            move |dialog, window, cx| {
                if first.replace(false) {
                    let handle = this.read(cx).code.read(cx).focus_handle(cx);
                    handle.focus(window, cx);
                }
                let page = this.read(cx);
                let k = native_colors(cx);
                let error = page.approve_error.map(error_text);
                let full_button = |id: &'static str, label: &'static str, busy: bool, cx: &App| {
                    loading_push(
                        push_button(id, label, PushStyle::Default, PushSize::Large, cx),
                        busy,
                        PushSize::Large,
                    )
                    .w_full()
                    .h(metrics::CODE_SLOT.1)
                    .rounded(metrics::FIELD_RADIUS)
                };
                let (title, hint, content): (&'static str, String, gpui::AnyElement) =
                    match &page.approve {
                        Approve::Code | Approve::Checking => {
                            let busy = page.approve == Approve::Checking;
                            let next = this.clone();
                            (
                                s::PHONE_CODE_TITLE,
                                s::PHONE_CODE_HINT.to_string(),
                                div()
                                    .w_full()
                                    .flex()
                                    .flex_col()
                                    .child(
                                        NativeTextField::new("phone-code-field", &page.code)
                                            .full_width()
                                            .size(crate::ui::native::FieldSize::Hero)
                                            .error(error.is_some()),
                                    )
                                    .child(
                                        div().pt(tokens::space::S24).child(
                                            disabled_push(
                                                full_button(
                                                    "phone-code-next",
                                                    if busy {
                                                        s::PHONE_CODE_CHECKING
                                                    } else {
                                                        s::PHONE_CODE_NEXT
                                                    },
                                                    busy,
                                                    cx,
                                                ),
                                                page.typed_code(cx).is_empty() && !busy,
                                                cx,
                                            )
                                            .on_click(
                                                move |_, _, cx| {
                                                    next.update(cx, |p, cx| p.check_code(cx))
                                                },
                                            ),
                                        ),
                                    )
                                    .into_any_element(),
                            )
                        }
                        Approve::Confirm { device, .. } | Approve::Approving { device, .. } => {
                            let busy = matches!(page.approve, Approve::Approving { .. });
                            let go = this.clone();
                            (
                                s::PHONE_APPROVE_TITLE,
                                fill(s::PHONE_APPROVE_BODY, &[("name", device)]),
                                full_button(
                                    "phone-approve",
                                    if busy {
                                        s::PHONE_APPROVING
                                    } else {
                                        s::PHONE_APPROVE
                                    },
                                    busy,
                                    cx,
                                )
                                .on_click(move |_, _, cx| go.update(cx, |p, cx| p.approve(cx)))
                                .into_any_element(),
                            )
                        }
                        Approve::Done { device } => (
                            s::PHONE_DONE,
                            fill(s::PHONE_APPROVED, &[("name", device)]),
                            full_button("phone-done", s::PHONE_DONE, false, cx)
                                .on_click(|_, window, cx| window.close_dialog(cx))
                                .into_any_element(),
                        ),
                    };
                let body = div()
                    .w_full()
                    .flex()
                    .flex_col()
                    .items_center()
                    .when_some(error, |d, e| {
                        d.child(
                            div()
                                .pb(tokens::space::S12)
                                .child(sheet_status_error(e, cx)),
                        )
                    })
                    .child(content);
                let close = crate::ui::native::icon_button(
                    "phone-code-close",
                    crate::ui::sf_symbols::Sf::Close,
                    common::CLOSE,
                    crate::ui::native::IconSize::Small,
                    window,
                    cx,
                )
                .on_click(|_, window, cx| window.close_dialog(cx));
                let _ = k;
                let ok = this.clone();
                dialog
                    .on_ok(move |_, _, cx| {
                        ok.update(cx, |p, cx| match p.approve {
                            Approve::Code => p.check_code(cx),
                            Approve::Confirm { .. } => p.approve(cx),
                            _ => {}
                        });
                        false
                    })
                    .on_cancel(|_, _, _| true)
                    .child(sheet_column(
                        close,
                        Sym::Phone,
                        title,
                        hint,
                        body.into_any_element(),
                        cx,
                    ))
            },
        );
    }
}

/// One centered column like the email sheet: ✕ in the corner, the icon, the
/// title, one line, then the content.
fn sheet_column(
    close: Button,
    sym: Sym,
    title: &'static str,
    hint: String,
    content: gpui::AnyElement,
    cx: &App,
) -> gpui::Div {
    let k = native_colors(cx);
    div()
        .relative()
        .w_full()
        .px(tokens::space::S32)
        .pt(tokens::space::S32)
        .pb(tokens::space::S24)
        .flex()
        .flex_col()
        .items_center()
        .child(
            div()
                .absolute()
                .top(tokens::space::S12)
                .right(tokens::space::S12)
                .child(close),
        )
        .child(crate::ui::native::feedback::icon_box(sym, cx))
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
        .child(
            div()
                .w_full()
                .pt(tokens::space::S24)
                .flex()
                .justify_center()
                .child(content),
        )
}

impl Render for PhoneSignIn {
    /// The section: 用手机扫码登录 (显示二维码) and 手机上显示了一个码 (输入).
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let page = cx.entity();
        let show = {
            let page = page.clone();
            push_button(
                "phone-show",
                s::PHONE_SHOW,
                PushStyle::Outline,
                PushSize::Small,
                cx,
            )
            .on_click(move |_, window, cx| Self::open_qr_sheet(page.clone(), window, cx))
        };
        let enter = push_button(
            "phone-enter",
            s::PHONE_ENTER,
            PushStyle::Outline,
            PushSize::Small,
            cx,
        )
        .on_click(move |_, window, cx| Self::open_code_sheet(page.clone(), window, cx));
        widgets::section(
            SectionTitle::new(s::PHONE).description(s::PHONE_HINT),
            GroupedForm::new()
                .row(
                    widgets::ItemRow::new("phone-show-row", s::PHONE_SHOW_ROW)
                        .media(widgets::icon_box_28(Sym::Phone, cx))
                        .value(show),
                )
                .row(
                    widgets::ItemRow::new("phone-enter-row", s::PHONE_ENTER_ROW)
                        .media(widgets::icon_box_28(Sym::Keyboard, cx))
                        .value(enter),
                ),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn qr_codes_encode_sign_in_links() {
        let (w, dark) =
            qr_modules("tc-sign-in:7K2M-9QPX?server=https%3A%2F%2Fchat.example.com").unwrap();
        assert_eq!(dark.len(), w * w);
        assert!(w >= 21);
        assert!(dark.iter().any(|d| *d) && dark.iter().any(|d| !*d));
    }
}
