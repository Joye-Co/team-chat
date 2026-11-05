//! The login window (COMPLETENESS 2.20): a small window of its own (480 x
//! 520, not resizable) with one centered card 400 wide: the app icon 48, the
//! 24 hero title, a one-sentence description, the labelled h36 field across
//! the card, the error (12 destructive) or the custom server under it, the
//! full-width `继续` (lg) and a 14 muted link row (`使用其他服务器…`, also in
//! the app menu). Nothing sits at the window corners. A refused code or an
//! unreachable server marks the field and says why under it; no alert. A
//! pasted login credential (`tc_…`) or a legacy 12-character code is accepted
//! in the same place and shows as one locked field. The server URL defaults to
//! the production URL from product.json, then staging.

use gpui::{
    AppContext, Context, Entity, Focusable as _, InteractiveElement as _, IntoElement,
    ParentElement, Render, SharedString, Styled, Subscription, Window, div,
    prelude::FluentBuilder as _, px,
};
use gpui_component::input::{Input, InputEvent, InputState};

use crate::{
    app::{Changes, Services},
    copy::{common, login as l, overlays as o},
    data::{Change, Credential, LoginError, LoginRequest, SessionState},
    ui::{
        self, Sym,
        native::{
            FieldSize, NativeTextField, NativeType as _, PushSize, PushStyle, borderless_text,
            disabled_push, metrics, native_colors,
            overlays::{app_icon, spot},
            push_button,
        },
        sf_symbols::SymSize,
    },
};

pub fn default_server_url() -> &'static str {
    let production = env!("PRODUCT_PRODUCTION_URL");
    if production.is_empty() {
        env!("PRODUCT_STAGING_URL")
    } else {
        production
    }
}

/// Characters in an invite code (`XXXX-XXXX`, server `newInviteCode`).
pub const CODE_LEN: usize = 8;
/// Characters of a code made before the 8-character format
/// (`XXXX-XXXX-XXXX`); the server still redeems them.
pub const LEGACY_CODE_LEN: usize = 12;
/// Characters per group of a code (`XXXX-XXXX`).
const GROUP: usize = 4;
/// The login card (2.20): 400 wide.
const CARD_WIDTH: gpui::Pixels = px(400.);
/// The login window: small, fixed, not resizable.
pub const WINDOW_SIZE: gpui::Size<gpui::Pixels> = gpui::Size {
    width: px(480.),
    height: px(520.),
};

gpui::actions!(login, [UseOtherServer]);

/// What the code field holds.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CodeInput {
    /// The invite code characters typed so far, upper case, at most 8.
    Code(String),
    /// A pasted 12-character code of the earlier format, upper case.
    Legacy(String),
    /// A pasted login credential.
    Token(String),
}

/// The letters and digits of a code, upper case (dashes and spaces dropped).
fn code_chars(text: &str) -> String {
    text.chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .map(|c| c.to_ascii_uppercase())
        .collect()
}

impl CodeInput {
    /// Reads the field: a `tc_` credential, a pasted legacy code of exactly
    /// 12 characters, or the first 8 letters and digits of an invite code.
    pub fn parse(text: &str) -> Self {
        let text = text.trim();
        if text.starts_with("tc_") {
            return CodeInput::Token(text.to_string());
        }
        let chars = code_chars(text);
        if chars.len() == LEGACY_CODE_LEN {
            return CodeInput::Legacy(chars);
        }
        CodeInput::Code(chars.chars().take(CODE_LEN).collect())
    }

    /// The credential to send, once the field is complete.
    pub fn credential(&self) -> Option<Credential> {
        let grouped = |c: &str| {
            let groups: Vec<String> = c
                .as_bytes()
                .chunks(GROUP)
                .map(|g| String::from_utf8_lossy(g).into_owned())
                .collect();
            Credential::InviteCode(groups.join("-"))
        };
        match self {
            CodeInput::Token(t) => Credential::from_input(t),
            CodeInput::Legacy(c) => Some(grouped(c)),
            CodeInput::Code(c) if c.chars().count() == CODE_LEN => Some(grouped(c)),
            CodeInput::Code(_) => None,
        }
    }

    /// The field text to keep when typing went past 8 characters: typing
    /// stops at the eighth box, so only a paste reaches a legacy code.
    fn clamped(text: &str) -> Option<String> {
        let trimmed = text.trim();
        if trimmed.starts_with("tc_") {
            return None;
        }
        let chars = code_chars(trimmed);
        (chars.len() > CODE_LEN && chars.len() != LEGACY_CODE_LEN)
            .then(|| chars.chars().take(CODE_LEN).collect())
    }
}

/// The host part of a server URL, for the server line (`team.joye.co`).
pub fn server_host(url: &str) -> &str {
    let rest = url
        .trim()
        .trim_start_matches("https://")
        .trim_start_matches("http://");
    rest.split('/').next().unwrap_or(rest)
}

pub struct LoginView {
    services: Services,
    server: Entity<InputState>,
    code: Entity<InputState>,
    /// The server field is open (`更改`, or after an unreachable server).
    server_open: bool,
    /// Drawn inside a main window (signed out there): a sign-out moves to
    /// the login window.
    embedded: bool,
    /// The wrong code after a failed try: the next typed character or paste
    /// starts a new code from box 1, Backspace edits it from the end.
    rejected: Option<String>,
    /// The last refusal, shown under the field until the field changes.
    error: Option<LoginError>,
    /// The field text the refusal was about.
    error_for: String,
    /// Draws the signing-in state (a screenshot scene).
    preview_busy: bool,
    _subscriptions: Vec<Subscription>,
}

impl LoginView {
    /// One of the login fields has the keyboard focus (it took it when the
    /// view was made, drawn or not).
    pub fn has_focus(&self, window: &Window, cx: &gpui::App) -> bool {
        self.code.read(cx).focus_handle(cx).is_focused(window)
            || self.server.read(cx).focus_handle(cx).is_focused(window)
    }

    pub fn new(services: Services, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let server = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder(l::SERVER_PLACEHOLDER)
                .default_value(default_server_url())
        });
        let code = cx.new(|cx| InputState::new(window, cx).placeholder(l::CODE_PLACEHOLDER));
        // The first box has the caret when the view opens.
        code.read(cx).focus_handle(cx).focus(window, cx);
        let subscriptions = vec![
            cx.subscribe_in(
                &code,
                window,
                |this, _, event: &InputEvent, window, cx| match event {
                    InputEvent::PressEnter { .. } => this.submit(window, cx),
                    InputEvent::Change => {
                        let mut value = this.code.read(cx).value().to_string();
                        if let Some(old) = this.rejected.take()
                            && value.len() > old.len()
                            && let Some(fresh) = value.strip_prefix(old.as_str())
                        {
                            value = fresh.to_string();
                            this.code
                                .update(cx, |st, cx| st.set_value(value.clone(), window, cx));
                        }
                        if let Some(kept) = CodeInput::clamped(&value) {
                            this.code
                                .update(cx, |st, cx| st.set_value(kept, window, cx));
                        }
                        this.clear_stale_error(cx);
                        cx.notify();
                    }
                    _ => cx.notify(),
                },
            ),
            cx.subscribe_in(
                &server,
                window,
                |this, _, event: &InputEvent, window, cx| match event {
                    InputEvent::PressEnter { .. } => this.continue_(window, cx),
                    InputEvent::Change => {
                        this.clear_stale_error(cx);
                        cx.notify();
                    }
                    _ => {}
                },
            ),
            cx.subscribe_in(
                &services.hub,
                window,
                |this, _, changes: &Changes, window, cx| {
                    if !changes.any(|c| matches!(c, Change::Session)) {
                        return;
                    }
                    let session = this.services.provider.session();
                    match &session {
                        SessionState::SignedOut {
                            last_error: Some(LoginError::ServerUnreachable),
                        } => {
                            this.server_open = true;
                            this.server.read(cx).focus_handle(cx).focus(window, cx);
                        }
                        // A wrong code stays in the boxes with the caret
                        // after it (§8.3).
                        SessionState::SignedOut {
                            last_error: Some(LoginError::BadCredential),
                        } => this.reject_code(window, cx),
                        _ => {}
                    }
                    // A refusal marks the field and says why under it.
                    if let SessionState::SignedOut {
                        last_error: Some(e),
                    } = session
                    {
                        this.error = Some(e);
                        this.error_for = this.field_text(cx);
                    }
                    if this.embedded && matches!(session, SessionState::SignedOut { .. }) {
                        move_to_login_window(this.services.clone(), window, cx);
                    }
                    cx.notify();
                },
            ),
        ];
        Self {
            services,
            server,
            code,
            server_open: false,
            embedded: true,
            rejected: None,
            error: None,
            error_for: String::new(),
            preview_busy: false,
            _subscriptions: subscriptions,
        }
    }

    /// Keeps a wrong code in the boxes, focused, with the caret after its
    /// last character; the next character typed or pasted starts a new code.
    fn reject_code(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let value = self.code.read(cx).value().to_string();
        let end = value.len();
        self.code.update(cx, |st, cx| {
            st.focus(window, cx);
            st.set_selected_range(end..end, cx);
        });
        self.rejected = Some(value);
    }

    /// Opens the server field and moves the focus into it with the whole
    /// URL selected, as a macOS field does when it takes the focus.
    fn open_server_field(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.server_open = true;
        self.server.update(cx, |st, cx| {
            let end = st.value().len();
            st.focus(window, cx);
            st.set_selected_range(0..end, cx);
        });
        cx.notify();
    }

    /// The text of the field that is up: the server, or the code.
    fn field_text(&self, cx: &gpui::App) -> String {
        if self.server_open {
            self.server.read(cx).value().to_string()
        } else {
            self.code.read(cx).value().to_string()
        }
    }

    /// An edit of the field the refusal was about takes the error away.
    fn clear_stale_error(&mut self, cx: &gpui::App) {
        if self.error.is_some() && self.field_text(cx) != self.error_for {
            self.error = None;
        }
    }

    /// Draws the signing-in state with `code` typed, for a screenshot scene.
    pub fn preview_signing_in(&mut self, code: &str, window: &mut Window, cx: &mut Context<Self>) {
        self.preview(code, false, window, cx);
        self.preview_busy = true;
        cx.notify();
    }

    fn input(&self, cx: &gpui::App) -> CodeInput {
        CodeInput::parse(&self.code.read(cx).value())
    }

    /// Fills the form for a screenshot scene: the code typed so far, the
    /// server field open or not, and focus in the code boxes.
    pub fn preview(
        &mut self,
        code: &str,
        server_open: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.code
            .update(cx, |st, cx| st.set_value(code.to_string(), window, cx));
        // An open server field has the focus and its URL selected, as after
        // `使用其他服务器`.
        if server_open {
            self.open_server_field(window, cx);
        } else {
            self.server_open = false;
            self.code.read(cx).focus_handle(cx).focus(window, cx);
            cx.notify();
        }
    }

    fn submit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if matches!(self.services.provider.session(), SessionState::SigningIn) {
            return;
        }
        let server_url = self.server.read(cx).value().trim().to_string();
        let Some(credential) = self.input(cx).credential() else {
            return;
        };
        // No server: the server step, whose 继续 is off until it has one.
        if server_url.is_empty() {
            self.open_server_field(window, cx);
            return;
        }
        self.services.provider.login(LoginRequest {
            server_url,
            credential,
            device_name: crate::provider::session::device_name(),
        });
        // A wrong code stays in the boxes so it can be checked (§8.3); the
        // field lives only in this view and is never logged.
        let _ = window;
        cx.notify();
    }

    /// Sets a custom server, for a screenshot scene.
    pub fn preview_server(&mut self, url: &str, window: &mut Window, cx: &mut Context<Self>) {
        self.server
            .update(cx, |st, cx| st.set_value(url.to_string(), window, cx));
        cx.notify();
    }

    /// Fills the code and submits it, for the error screenshot scene.
    pub fn preview_submit(&mut self, code: &str, window: &mut Window, cx: &mut Context<Self>) {
        self.preview(code, false, window, cx);
        self.submit(window, cx);
    }

    /// The code field: a 28 pt standalone field (§5.6). A pasted credential
    /// or legacy code shows as a locked line (the secret is never drawn);
    /// the real input sits on top of it so paste and Backspace still work.
    fn render_code_field(
        &self,
        input: &CodeInput,
        focused: bool,
        error: bool,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let k = native_colors(cx);
        let locked = match input {
            CodeInput::Token(_) => Some(l::CODE_TOKEN),
            CodeInput::Legacy(_) => Some(l::CODE_LEGACY),
            CodeInput::Code(_) => None,
        };
        match locked {
            None => NativeTextField::new("login-code", &self.code)
                .size(FieldSize::Large)
                .full_width()
                .error(error)
                .into_any_element(),
            Some(text) => {
                let (edge, shadows) = crate::ui::native::fields::field_edge(&k, focused, error);
                div()
                    .id("login-code")
                    .relative()
                    .w_full()
                    .h(metrics::FIELD_LARGE)
                    .px(metrics::FIELD_PAD)
                    .rounded(metrics::FIELD_RADIUS)
                    // The fill of every field (§2.22: dark 37, review 4
                    // K-16), locked or not; the lock and muted text say it
                    // is read-only.
                    .bg(k.control)
                    .border_1()
                    .border_color(edge)
                    .shadow(shadows)
                    .flex()
                    .items_center()
                    .gap(crate::ui::native::tokens::space::S6)
                    .n_body()
                    .text_color(k.secondary)
                    .child(Sym::Lock.icon(SymSize::INLINE).text_color(k.secondary))
                    .child(text)
                    .child(
                        div()
                            .absolute()
                            .inset_0()
                            .opacity(0.)
                            .child(Input::new(&self.code).appearance(false).h_full()),
                    )
                    .into_any_element()
            }
        }
    }

    /// 继续: on the code step it joins; on the server step it goes back to
    /// the code, or joins when the code is complete.
    fn continue_(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.server_open && self.input(cx).credential().is_none() {
            self.close_server_field(window, cx);
        } else {
            self.submit(window, cx);
        }
    }

    fn close_server_field(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.server_open = false;
        self.code.read(cx).focus_handle(cx).focus(window, cx);
        cx.notify();
    }
}

impl Render for LoginView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        use crate::ui::native::tokens::{radius, shadow, space};
        let k = native_colors(cx);
        let session = self.services.provider.session();
        let signing_in = self.preview_busy || matches!(session, SessionState::SigningIn);
        let input = self.input(cx);
        let complete = input.credential().is_some();
        let focused = self.code.read(cx).focus_handle(cx).is_focused(window);
        let server_url = self.server.read(cx).value().to_string();
        let host: SharedString = server_host(&server_url).to_string().into();
        let custom =
            server_url.trim().trim_end_matches('/') != default_server_url().trim_end_matches('/');
        // One field at a time: the server, or the invite code.
        let server_step = self.server_open;
        let error = self.error.filter(|_| !signing_in);
        let field: gpui::AnyElement = if server_step {
            NativeTextField::new("login-server", &self.server)
                .size(FieldSize::Large)
                .full_width()
                .error(error.is_some())
                .into_any_element()
        } else {
            self.render_code_field(&input, focused, error.is_some(), cx)
        };
        // Under the field: the error, or on the code step the custom
        // server, quietly. The slot keeps its height, so nothing moves.
        let under: Option<gpui::AnyElement> = match error {
            // A field error (owner decision v15): neutral 12 text after a
            // 16 destructive icon, as in every sheet footer.
            Some(e) => Some(
                crate::ui::native::sheet::sheet_status_error(error_text(e), cx)
                    .id("login-error")
                    .w_full()
                    .into_any_element(),
            ),
            None => (!server_step && custom).then(|| {
                div()
                    .w_full()
                    .min_w_0()
                    .flex()
                    .items_center()
                    .gap(space::S4)
                    .n_subheadline()
                    .text_color(k.secondary)
                    .child(
                        Sym::MacMini
                            .icon(SymSize::INLINE_SMALL)
                            .text_color(k.secondary),
                    )
                    // A long host is cut in the middle (both ends name it),
                    // with the full host in a tooltip (2.21).
                    .child(
                        crate::ui::truncate::TruncText::new(host)
                            .id("login-host")
                            .middle()
                            .flex_1(),
                    )
                    .into_any_element()
            }),
        };
        let ready = if server_step {
            !server_url.trim().is_empty()
        } else {
            complete
        };
        let go = push_button(
            "login",
            if signing_in {
                l::SIGNING_IN
            } else {
                o::LOGIN_CONTINUE
            },
            PushStyle::Default,
            PushSize::Large,
            cx,
        )
        .w_full();
        // Busy: a 16 spinner before 正在验证…, at the same width.
        let go = disabled_push(go, !ready && !signing_in, cx)
            .when(signing_in, |b| b.icon(gpui_component::IconName::Loader))
            .loading(signing_in)
            .on_click(cx.listener(|this, _, window, cx| this.continue_(window, cx)));
        // The link row: the way to the server step, or back from it.
        let link = if server_step {
            borderless_text("login-server-cancel", common::CANCEL, cx)
                .text_color(k.secondary)
                .on_click(cx.listener(|this, _, window, cx| this.close_server_field(window, cx)))
        } else {
            borderless_text("login-server-other", l::OTHER_SERVER, cx)
                .text_color(k.secondary)
                .on_click(cx.listener(|this, _, window, cx| this.open_server_field(window, cx)))
        };
        let (title, description, label) = if server_step {
            (l::SERVER_TITLE, l::SERVER_LEAD, l::SERVER_LABEL)
        } else {
            (l::WELCOME, l::LEAD, l::CODE_LABEL)
        };
        let card = div()
            .w(CARD_WIDTH)
            .p(space::S24)
            .rounded(radius::CARD)
            .border_1()
            .border_color(k.separator)
            .bg(k.sheet)
            .shadow(shadow::sm())
            .flex()
            .flex_col()
            .gap(space::S16)
            .child(app_icon(spot::LOGIN_ICON))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(space::S4)
                    .child(div().n_large_title().text_color(k.label).child(title))
                    .child(div().n_body().text_color(k.secondary).child(description)),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(space::S8)
                    .child(div().n_headline().text_color(k.label).child(label))
                    .child(field)
                    .child(
                        div()
                            .h(crate::ui::native::tokens::text::META.1)
                            .w_full()
                            .min_w_0()
                            .flex()
                            .items_center()
                            .children(under),
                    ),
            )
            .child(go)
            .child(div().flex().justify_center().child(link));
        div()
            .size_full()
            .relative()
            .bg(k.window)
            .on_action(cx.listener(|this, _: &UseOtherServer, window, cx| {
                this.open_server_field(window, cx)
            }))
            .child(
                div()
                    .size_full()
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(card),
            )
            // The top band drags the window, as a title bar does.
            .child(
                ui::toolbar_band("login-toolbar")
                    .absolute()
                    .top_0()
                    .left_0(),
            )
    }
}

/// The text of a refused login.
fn error_text(e: LoginError) -> &'static str {
    match e {
        LoginError::BadCredential => l::ERROR_BAD_CREDENTIAL,
        LoginError::ServerUnreachable => l::ERROR_UNREACHABLE,
        LoginError::Unsupported => l::ERROR_UNSUPPORTED,
        LoginError::Other => l::ERROR_OTHER,
    }
}

// --- the login window ---

/// The login window while it is open.
struct LoginWindowGlobal(Option<gpui::AnyWindowHandle>);

impl gpui::Global for LoginWindowGlobal {}

/// Set by the app (not the self-tests): signing out in a main window moves
/// to the login window.
static STANDALONE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// The root of the login window: the form, and the move to the main window
/// once the session is signed in.
pub struct LoginWindow {
    login: Entity<LoginView>,
    _subscriptions: Vec<Subscription>,
}

impl LoginWindow {
    fn new(services: Services, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let login = cx.new(|cx| {
            let mut view = LoginView::new(services.clone(), window, cx);
            view.embedded = false;
            view
        });
        let hub = services.hub.clone();
        let subscriptions =
            vec![
                cx.subscribe_in(&hub, window, move |_, _, changes: &Changes, window, cx| {
                    if !changes.any(|c| matches!(c, Change::Session)) {
                        return;
                    }
                    if matches!(services.provider.session(), SessionState::SignedIn { .. }) {
                        let services = services.clone();
                        window.defer(cx, move |window, cx| {
                            if let Err(e) = crate::app::open_main_window(services, true, None, cx) {
                                eprintln!("failed to open the window: {e}");
                                return;
                            }
                            cx.set_global(LoginWindowGlobal(None));
                            window.remove_window();
                        });
                    }
                }),
            ];
        Self {
            login,
            _subscriptions: subscriptions,
        }
    }

    pub fn login(&self) -> &Entity<LoginView> {
        &self.login
    }

    /// The window of a screenshot scene.
    pub fn new_for_scene(services: Services, window: &mut Window, cx: &mut Context<Self>) -> Self {
        Self::new(services, window, cx)
    }
}

impl Render for LoginWindow {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().size_full().child(self.login.clone())
    }
}

/// The options of the login window: 480 x 520, centred, not resizable, a
/// transparent title bar (the content draws under it).
pub fn window_options(focus: bool, cx: &gpui::App) -> gpui::WindowOptions {
    gpui::WindowOptions {
        window_bounds: Some(gpui::WindowBounds::Windowed(gpui::Bounds::centered(
            None,
            WINDOW_SIZE,
            cx,
        ))),
        titlebar: Some(gpui::TitlebarOptions {
            title: Some(env!("PRODUCT_NAME").into()),
            appears_transparent: true,
            traffic_light_position: None,
        }),
        window_background: gpui::WindowBackgroundAppearance::Opaque,
        is_resizable: false,
        focus,
        show: true,
        ..Default::default()
    }
}

/// Opens the login window, or brings the open one to the front.
pub fn open_window(
    services: Services,
    focus: bool,
    cx: &mut gpui::App,
) -> Result<(gpui::AnyWindowHandle, Entity<LoginWindow>), String> {
    let (handle, view) = gpui_kit::open_window(window_options(focus, cx), cx, move |window, cx| {
        cx.new(|cx| LoginWindow::new(services, window, cx))
    })
    .map_err(|e| e.to_string())?;
    cx.set_global(LoginWindowGlobal(Some(handle)));
    Ok((handle, view))
}

/// The app's start and the Dock icon: while signed out, the login window
/// (brought to the front, or opened again). Returns false when signed in.
pub fn show_login_window(services: Services, cx: &mut gpui::App) -> bool {
    STANDALONE.store(true, std::sync::atomic::Ordering::Relaxed);
    if matches!(services.provider.session(), SessionState::SignedIn { .. }) {
        return false;
    }
    let open = cx
        .try_global::<LoginWindowGlobal>()
        .and_then(|g| g.0)
        .is_some_and(|h| {
            h.update(cx, |_, window, _| window.activate_window())
                .is_ok()
        });
    if !open && let Err(e) = open_window(services, true, cx) {
        eprintln!("failed to open the login window: {e}");
    }
    true
}

/// Signed out in a main window: the login window takes its place.
fn move_to_login_window(services: Services, window: &mut Window, cx: &mut gpui::App) {
    if !STANDALONE.load(std::sync::atomic::Ordering::Relaxed) {
        return;
    }
    window.defer(cx, move |window, cx| {
        show_login_window(services, cx);
        window.remove_window();
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn code_input_reads_codes_and_tokens() {
        for pasted in ["K7QM-2X9A", "K7QM2X9A", "k7qm-2x9a", " k7qm 2x9a "] {
            assert_eq!(CodeInput::parse(pasted), CodeInput::Code("K7QM2X9A".into()));
            assert_eq!(
                CodeInput::parse(pasted).credential(),
                Some(Credential::InviteCode("K7QM-2X9A".into()))
            );
        }
        assert_eq!(CodeInput::parse("K7QM-2X").credential(), None);
        assert_eq!(
            CodeInput::parse("K7QM2X9AB"),
            CodeInput::Code("K7QM2X9A".into())
        );
        // A legacy 12-character code is one locked field and still signs in.
        assert_eq!(
            CodeInput::parse(" k7qm-2x9a bc3d "),
            CodeInput::Legacy("K7QM2X9ABC3D".into())
        );
        assert_eq!(
            CodeInput::parse("k7qm2x9abc3d").credential(),
            Some(Credential::InviteCode("K7QM-2X9A-BC3D".into()))
        );
        assert!(matches!(
            CodeInput::parse(" tc_abc ").credential(),
            Some(Credential::Token(_))
        ));
    }

    #[test]
    fn typing_stops_at_the_eighth_box() {
        assert_eq!(CodeInput::clamped("K7QM2X9A"), None);
        assert_eq!(CodeInput::clamped("K7QM2X9AB"), Some("K7QM2X9A".into()));
        assert_eq!(CodeInput::clamped("K7QM-2X9A-BC3D"), None);
        assert_eq!(CodeInput::clamped("tc_abcdefghijklmnop"), None);
    }

    #[test]
    fn server_line_shows_the_host() {
        assert_eq!(server_host("https://team.joye.co/"), "team.joye.co");
        assert_eq!(server_host("http://127.0.0.1:8787"), "127.0.0.1:8787");
    }
}
