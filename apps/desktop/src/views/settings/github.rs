//! Each person's GitHub (docs/plans/usable/design-github-identity.zh.md §3.7): the 连接 GitHub
//! sheet and the GitHub group of 设置 › 账号.
//!
//! The server runs GitHub's device code flow: the sheet shows the code and the address to
//! open, polls at the interval the server gives, and says when GitHub confirmed. The token
//! never reaches the app. A server without a client id (or without the routes) shows owners
//! one quiet line and everyone else nothing.

use std::time::Duration;

use gpui::{
    AnyWindowHandle, App, AppContext as _, ClipboardItem, Context, Entity, IntoElement,
    ParentElement, Render, Styled, Subscription, Window, div,
};
use gpui_component::WindowExt as _;

use super::widgets;
use crate::{
    app::{Changes, Services},
    copy::{common, fill, settings as s},
    data::{AdminError, Change, GithubDevice, GithubPoll, GithubState},
    ui::{
        Sym,
        native::{
            GroupedForm, NativeType as _, PushSize, PushStyle, SectionTitle, SheetWidth, metrics,
            native_colors, push_button,
            sheet::{AlertLayout, alert_button, sheet_status_error},
            tokens,
        },
    },
};

/// GitHub's page where a person withdraws an OAuth App's access.
const REVOKE_URL: &str = "https://github.com/settings/applications";
/// How long 已连接为 @login stays before the sheet closes.
const DONE_HOLD: Duration = Duration::from_secs(1);
/// What GitHub adds to the interval on `slow_down`.
const SLOW_DOWN_S: u32 = 5;

/// Where the sheet opens: under the settings toolbar, or over the main window.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Place {
    Settings,
    Main,
}

/// What the 连接 GitHub sheet shows.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Step {
    /// Asking the server for a code.
    Starting,
    /// The code is on screen; the sheet polls until GitHub answers.
    Code(GithubDevice),
    /// 已连接为 @login; the sheet closes after [`DONE_HOLD`].
    Done {
        login: String,
    },
    Expired,
    Denied,
    Taken,
    /// The server refused to start (not set up, offline, ...).
    Failed(AdminError),
}

/// The 连接 GitHub sheet.
pub struct GithubConnect {
    services: Services,
    step: Step,
    /// Bumped on every start and on close, so an older poll stops.
    run: u64,
    /// Opened by the one-time prompt after sign-in: it offers 以后再说.
    onboarding: bool,
    /// 复制验证码 was used for this code.
    copied: bool,
    window: Option<AnyWindowHandle>,
}

impl GithubConnect {
    pub fn new(services: Services, onboarding: bool) -> Self {
        Self {
            services,
            step: Step::Starting,
            run: 0,
            onboarding,
            copied: false,
            window: None,
        }
    }

    pub fn step(&self) -> &Step {
        &self.step
    }

    /// Screenshot scenes: show `step` without asking the server.
    pub fn preview(&mut self, step: Step, cx: &mut Context<Self>) {
        self.run += 1;
        self.step = step;
        cx.notify();
    }

    fn stop(&mut self) {
        self.run += 1;
    }

    /// Asks for a code and polls it until GitHub answers or the sheet closes.
    pub fn start(&mut self, cx: &mut Context<Self>) {
        self.run += 1;
        let run = self.run;
        self.step = Step::Starting;
        self.copied = false;
        cx.notify();
        let reply = self.services.provider.start_github();
        cx.spawn(async move |this, cx| {
            let device = match reply.await.unwrap_or(Err(AdminError::Other)) {
                Ok(d) => d,
                Err(e) => {
                    let _ = this.update(cx, |this, cx| {
                        if this.run == run {
                            this.step = Step::Failed(e);
                            cx.notify();
                        }
                    });
                    return;
                }
            };
            let (id, mut interval, expires_at) =
                (device.id.clone(), device.interval, device.expires_at);
            let live = this
                .update(cx, |this, cx| {
                    if this.run != run {
                        return false;
                    }
                    this.step = Step::Code(device);
                    cx.notify();
                    true
                })
                .unwrap_or(false);
            if !live {
                return;
            }
            loop {
                cx.background_executor()
                    .timer(Duration::from_secs(u64::from(interval)))
                    .await;
                if crate::selftest::clock::now_ms() >= expires_at {
                    let _ = this.update(cx, |this, cx| {
                        if this.run == run {
                            this.step = Step::Expired;
                            cx.notify();
                        }
                    });
                    return;
                }
                let Ok(Some(reply)) = this.update(cx, |this, _| {
                    (this.run == run).then(|| this.services.provider.poll_github(&id))
                }) else {
                    return;
                };
                let next = match reply.await {
                    // Offline for a moment: ask again at the next tick.
                    Ok(Err(AdminError::Offline)) | Err(_) => continue,
                    Ok(Err(e)) => Step::Failed(e),
                    Ok(Ok(GithubPoll::Pending)) => continue,
                    Ok(Ok(GithubPoll::SlowDown)) => {
                        interval += SLOW_DOWN_S;
                        continue;
                    }
                    Ok(Ok(GithubPoll::Done(a))) => Step::Done { login: a.login },
                    Ok(Ok(GithubPoll::Expired)) => Step::Expired,
                    Ok(Ok(GithubPoll::Denied)) => Step::Denied,
                    Ok(Ok(GithubPoll::Taken)) => Step::Taken,
                };
                let done = matches!(next, Step::Done { .. });
                let window = this
                    .update(cx, |this, cx| {
                        if this.run != run {
                            return None;
                        }
                        this.step = next;
                        cx.notify();
                        this.window
                    })
                    .ok()
                    .flatten();
                if done {
                    cx.background_executor().timer(DONE_HOLD).await;
                    let still = this.update(cx, |this, _| this.run == run).unwrap_or(false);
                    if let (true, Some(w)) = (still, window) {
                        let _ = cx.update_window(w, |_, window, cx| window.close_dialog(cx));
                    }
                }
                return;
            }
        })
        .detach();
    }

    fn copy_code(&mut self, cx: &mut Context<Self>) {
        if let Step::Code(d) = &self.step {
            cx.write_to_clipboard(ClipboardItem::new_string(d.user_code.clone()));
            self.copied = true;
            cx.notify();
        }
    }

    /// 打开 GitHub: copies the code too, so it only needs pasting there.
    fn open_github(&mut self, cx: &mut Context<Self>) {
        if let Step::Code(d) = &self.step {
            let url = d.verification_uri.clone();
            self.copy_code(cx);
            cx.open_url(&url);
        }
    }

    fn status_line(&self, cx: &App) -> Option<gpui::AnyElement> {
        let k = native_colors(cx);
        let text = |t: &'static str| {
            div()
                .n_body()
                .text_color(k.secondary)
                .text_center()
                .child(t)
                .into_any_element()
        };
        Some(match &self.step {
            Step::Starting => div()
                .flex()
                .items_center()
                .gap(tokens::space::S8)
                .n_body()
                .text_color(k.secondary)
                .child(crate::ui::motion::spinner())
                .child(s::GITHUB_STARTING)
                .into_any_element(),
            Step::Code(_) => div()
                .flex()
                .items_center()
                .gap(tokens::space::S8)
                .n_body()
                .text_color(k.secondary)
                .child(crate::ui::motion::spinner())
                .child(s::GITHUB_WAITING)
                .into_any_element(),
            Step::Done { login } => div()
                .flex()
                .items_center()
                .gap(tokens::space::S6)
                .n_body()
                .text_color(k.label)
                .child(crate::ui::icons::icon(Sym::Done, tokens::icon::MD, k.green))
                .child(fill(s::GITHUB_CONNECTED_AS, &[("login", login)]))
                .into_any_element(),
            Step::Expired => text(s::GITHUB_EXPIRED),
            Step::Denied => text(s::GITHUB_DENIED),
            Step::Taken => text(s::GITHUB_TAKEN),
            Step::Failed(e) => sheet_status_error(failed_text(*e), cx).into_any_element(),
        })
    }
}

/// What a refused start says.
fn failed_text(e: AdminError) -> &'static str {
    match e {
        AdminError::Unsupported => s::GITHUB_UNAVAILABLE,
        other => widgets::error_text(other),
    }
}

impl Render for GithubConnect {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let k = native_colors(cx);
        let this = cx.entity();
        let close = {
            let this = this.clone();
            crate::ui::native::icon_button(
                "github-close",
                crate::ui::sf_symbols::Sf::Close,
                common::CLOSE,
                crate::ui::native::IconSize::Small,
                window,
                cx,
            )
            .on_click(move |_, window, cx| {
                this.update(cx, |p, _| p.stop());
                window.close_dialog(cx)
            })
        };
        let code = match &self.step {
            Step::Code(d) => Some(d.user_code.clone()),
            _ => None,
        };
        let retry = matches!(
            self.step,
            Step::Expired | Step::Denied | Step::Taken | Step::Failed(_)
        ) && self.step != Step::Failed(AdminError::Unsupported);
        let buttons = code.as_ref().map(|_| {
            let (copy, open) = (this.clone(), this.clone());
            div()
                .flex()
                .items_center()
                .justify_center()
                .gap(tokens::space::S12)
                .child(
                    push_button(
                        "github-copy",
                        if self.copied {
                            s::GITHUB_CODE_COPIED
                        } else {
                            s::GITHUB_COPY_CODE
                        },
                        PushStyle::Outline,
                        PushSize::Regular,
                        cx,
                    )
                    .on_click(move |_, _, cx| copy.update(cx, |p, cx| p.copy_code(cx))),
                )
                .child(
                    push_button(
                        "github-open",
                        s::GITHUB_OPEN,
                        PushStyle::Default,
                        PushSize::Regular,
                        cx,
                    )
                    .on_click(move |_, _, cx| open.update(cx, |p, cx| p.open_github(cx))),
                )
        });
        let again = retry.then(|| {
            let this = this.clone();
            push_button(
                "github-retry",
                s::GITHUB_RETRY,
                PushStyle::Outline,
                PushSize::Regular,
                cx,
            )
            .on_click(move |_, _, cx| this.update(cx, |p, cx| p.start(cx)))
        });
        let later = (self.onboarding && !matches!(self.step, Step::Done { .. })).then(|| {
            let this = this.clone();
            push_button(
                "github-later",
                s::GITHUB_LATER,
                PushStyle::Ghost,
                PushSize::Regular,
                cx,
            )
            .on_click(move |_, window, cx| {
                this.update(cx, |p, _| p.stop());
                window.close_dialog(cx)
            })
        });
        // The code: large and monospaced, so 0/O and 1/I read apart. While it is being
        // asked for, its place stays the same height (nothing jumps when it arrives).
        let code_line = div()
            .h(metrics::CODE_SLOT.1)
            .flex()
            .items_center()
            .justify_center()
            .n_large_title()
            .font_family(crate::theme::mono_font_family())
            .text_color(k.label)
            .children(code);
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
            .child(crate::ui::native::feedback::icon_box(Sym::Branch, cx))
            .child(
                div()
                    .pt(tokens::space::S16)
                    .n_title2()
                    .text_color(k.label)
                    .child(s::GITHUB_CONNECT_TITLE),
            )
            .children(
                matches!(self.step, Step::Starting | Step::Code(_)).then(|| {
                    div()
                        .pt(tokens::space::S8)
                        .n_body()
                        .text_center()
                        .text_color(k.secondary)
                        .child(s::GITHUB_CODE_HINT)
                }),
            )
            // After an end (expired, cancelled, taken, refused) there is no code to show:
            // the line goes, so no empty band sits above the reason.
            .children(
                matches!(self.step, Step::Starting | Step::Code(_))
                    .then(|| div().pt(tokens::space::S16).child(code_line)),
            )
            .children(buttons.map(|b| div().pt(tokens::space::S16).child(b)))
            .children(
                self.status_line(cx)
                    .map(|l| div().pt(tokens::space::S16).child(l)),
            )
            .children(again.map(|b| div().pt(tokens::space::S12).child(b)))
            .child(
                div()
                    .pt(tokens::space::S24)
                    .n_footnote()
                    .text_center()
                    .text_color(k.tertiary)
                    .child(s::GITHUB_ORG_HINT),
            )
            .children(later.map(|b| div().pt(tokens::space::S12).child(b)))
    }
}

/// Opens the 连接 GitHub sheet and starts a sign-in. `onboarding`: the one-time prompt
/// after sign-in, with 以后再说.
pub fn open_connect(
    services: Services,
    place: Place,
    onboarding: bool,
    window: &mut Window,
    cx: &mut App,
) -> Entity<GithubConnect> {
    let view = cx.new(|_| GithubConnect::new(services, onboarding));
    let handle = window.window_handle();
    view.update(cx, |v, cx| {
        v.window = Some(handle);
        v.start(cx);
    });
    let sheet = view.clone();
    let build = move |dialog: gpui_component::dialog::Dialog| {
        let cancel = sheet.clone();
        dialog
            .on_cancel(move |_, _, cx| {
                cancel.update(cx, |p, _| p.stop());
                true
            })
            .child(sheet.clone())
    };
    match place {
        Place::Settings => widgets::open_sheet(
            window,
            cx,
            SheetWidth::Form,
            metrics::GITHUB_SHEET_FLOOR,
            move |dialog, _, _| build(dialog),
        ),
        Place::Main => window.open_dialog(cx, move |dialog, _, cx| {
            build(
                crate::ui::native::native_sheet(dialog, SheetWidth::Form, cx)
                    .px(gpui::Pixels::ZERO),
            )
        }),
    }
    view
}

/// 机器: the hint on a computer whose daemon has no feature `github` (it says nothing to
/// someone who has not connected GitHub, and the server tells owners only).
pub fn machine_hint(m: &crate::data::MachineInfo, services: &Services) -> Option<&'static str> {
    machine_needs_update(m.github, services.provider.github().as_ref())
        .then_some(s::GITHUB_MACHINE_OLD)
}

fn machine_needs_update(github: Option<bool>, state: Option<&GithubState>) -> bool {
    github == Some(false) && state.is_some_and(|s| s.account.is_some())
}

/// 设置 › 账号 › GitHub: who I am connected as, 连接…, 重新连接… or 断开….
pub struct GithubSection {
    services: Services,
    _subscriptions: Vec<Subscription>,
}

/// What the GitHub group shows.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SectionState {
    /// Nothing: not read yet, an older server, or not set up and I am not an owner.
    Hidden,
    /// Not set up on the server (owners only).
    Unavailable,
    NotConnected,
    Connected {
        login: String,
        name: Option<String>,
    },
    NeedsReconnect {
        login: String,
    },
}

/// The group's state from the server's answer.
pub fn section_state(state: Option<&GithubState>, owner: bool) -> SectionState {
    let Some(state) = state else {
        return SectionState::Hidden;
    };
    match &state.account {
        _ if !state.available && owner => SectionState::Unavailable,
        _ if !state.available => SectionState::Hidden,
        None => SectionState::NotConnected,
        Some(a) if a.ok => SectionState::Connected {
            login: a.login.clone(),
            name: a.name.clone().filter(|n| *n != a.login),
        },
        Some(a) => SectionState::NeedsReconnect {
            login: a.login.clone(),
        },
    }
}

impl GithubSection {
    pub fn new(services: Services, cx: &mut Context<Self>) -> Self {
        // 打开设置时取 (§3.7): the group is current when the page opens.
        services.provider.refresh_github();
        let subscriptions = vec![cx.subscribe(&services.hub, |_, _, changes: &Changes, cx| {
            if changes.any(|c| matches!(c, Change::Github | Change::Session)) {
                cx.notify();
            }
        })];
        Self {
            services,
            _subscriptions: subscriptions,
        }
    }

    /// 断开…: asks first, with the link to withdraw the app on GitHub.
    pub fn confirm_disconnect(services: Services, window: &mut Window, cx: &mut App) {
        widgets::open_alert(window, cx, move |_, cx| {
            let services = services.clone();
            AlertLayout::new(s::GITHUB_DISCONNECT_TITLE)
                .icon(widgets::app_icon(cx))
                .message(s::GITHUB_DISCONNECT_BODY)
                .buttons([
                    alert_button(
                        "github-disconnect-ok",
                        s::GITHUB_DISCONNECT_OK,
                        PushStyle::Outline,
                        true,
                        cx,
                    )
                    .on_click(move |_, window, cx| {
                        let reply = services.provider.disconnect_github();
                        window.close_dialog(cx);
                        cx.spawn(async move |_| {
                            if let Ok(Err(e)) = reply.await {
                                tracing::warn!(?e, "github disconnect");
                            }
                        })
                        .detach();
                    }),
                    alert_button(
                        "github-revoke",
                        s::GITHUB_REVOKE,
                        PushStyle::Outline,
                        false,
                        cx,
                    )
                    .on_click(|_, _, cx| cx.open_url(REVOKE_URL)),
                    alert_button(
                        "github-disconnect-cancel",
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
}

impl Render for GithubSection {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let state = section_state(
            self.services.provider.github().as_ref(),
            super::i_am_owner(&self.services),
        );
        let services = self.services.clone();
        let connect = |id: &'static str, label: &'static str, cx: &App| {
            let services = services.clone();
            push_button(id, label, PushStyle::Outline, PushSize::Small, cx).on_click(
                move |_, window, cx| {
                    open_connect(services.clone(), Place::Settings, false, window, cx);
                },
            )
        };
        let row = match &state {
            SectionState::Hidden => return div().into_any_element(),
            // Owners see why; nothing to press until the server is set up (INSTALL).
            SectionState::Unavailable => widgets::ItemRow::new("github-row", s::GITHUB_UNAVAILABLE)
                .media(widgets::icon_box_28(Sym::Branch, cx))
                .muted(true),
            SectionState::NotConnected => {
                widgets::ItemRow::new("github-row", s::GITHUB_NOT_CONNECTED)
                    .media(widgets::icon_box_28(Sym::Branch, cx))
                    .value(connect("github-connect", s::GITHUB_CONNECT, cx))
            }
            SectionState::Connected { login, name } => {
                let services = self.services.clone();
                widgets::ItemRow::new(
                    "github-row",
                    fill(s::GITHUB_CONNECTED_AS, &[("login", login)]),
                )
                .media(widgets::icon_box_28(Sym::Branch, cx))
                .description(name.clone().unwrap_or_default())
                .value(
                    push_button(
                        "github-disconnect",
                        s::GITHUB_DISCONNECT,
                        PushStyle::Outline,
                        PushSize::Small,
                        cx,
                    )
                    .on_click(move |_, window, cx| {
                        Self::confirm_disconnect(services.clone(), window, cx)
                    }),
                )
            }
            SectionState::NeedsReconnect { login } => {
                widgets::ItemRow::new("github-row", s::GITHUB_NEEDS_RECONNECT)
                    .media(widgets::icon_box_28(Sym::Warning, cx))
                    .description(format!("@{login}"))
                    .value(connect("github-reconnect", s::GITHUB_RECONNECT, cx))
            }
        };
        widgets::section(
            SectionTitle::new(s::GITHUB).description(s::GITHUB_HINT),
            GroupedForm::new().row(row),
        )
        .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::GithubAccount;

    fn state(available: bool, account: Option<(&str, bool)>) -> GithubState {
        GithubState {
            available,
            unavailable_reason: (!available).then(|| "no_client_id".into()),
            account: account.map(|(login, ok)| GithubAccount {
                login: login.into(),
                name: Some("Octo Cat".into()),
                ok,
                connected_at: 1,
            }),
            waits: Vec::new(),
        }
    }

    #[test]
    fn the_group_hides_until_read_and_from_members_when_not_set_up() {
        assert_eq!(section_state(None, true), SectionState::Hidden);
        assert_eq!(
            section_state(Some(&state(false, None)), false),
            SectionState::Hidden
        );
        assert_eq!(
            section_state(Some(&state(false, None)), true),
            SectionState::Unavailable
        );
    }

    #[test]
    fn the_group_says_how_i_am_connected() {
        assert_eq!(
            section_state(Some(&state(true, None)), false),
            SectionState::NotConnected
        );
        assert_eq!(
            section_state(Some(&state(true, Some(("octo", true)))), false),
            SectionState::Connected {
                login: "octo".into(),
                name: Some("Octo Cat".into())
            }
        );
        assert_eq!(
            section_state(Some(&state(true, Some(("octo", false)))), false),
            SectionState::NeedsReconnect {
                login: "octo".into()
            }
        );
    }

    #[test]
    fn only_an_older_computer_gets_the_hint_and_only_once_i_connected() {
        let connected = state(true, Some(("octo", true)));
        let not = state(true, None);
        assert!(machine_needs_update(Some(false), Some(&connected)));
        assert!(!machine_needs_update(Some(true), Some(&connected)));
        // Not told (not an owner): no guess.
        assert!(!machine_needs_update(None, Some(&connected)));
        assert!(!machine_needs_update(Some(false), Some(&not)));
        assert!(!machine_needs_update(Some(false), None));
    }

    #[test]
    fn not_set_up_never_offers_a_retry() {
        assert_eq!(failed_text(AdminError::Unsupported), s::GITHUB_UNAVAILABLE);
        assert_ne!(failed_text(AdminError::Offline), s::GITHUB_UNAVAILABLE);
    }
}
