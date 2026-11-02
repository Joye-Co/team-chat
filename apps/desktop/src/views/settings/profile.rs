//! 个人: my name, 岗位 and avatar, and sign out.

use gpui::{
    App, AppContext as _, Context, Entity, IntoElement, ParentElement, PathPromptOptions, Render,
    Styled, Subscription, Window, div, px,
};
use gpui_component::{
    Disableable as _, Icon, WindowExt as _,
    button::ButtonVariant,
    input::{InputEvent, InputState},
};

use super::widgets::{self, Status, answer};
use crate::{
    app::{Changes, Services},
    copy::{common, settings as s},
    data::{Change, Member, SessionState},
    theme::{self, Type as _, space},
    ui::{self, Lucide},
    views::avatar,
};

pub const NAME_MAX: usize = 64;
pub const ROLE_MAX: usize = 64;

pub struct ProfilePage {
    services: Services,
    name: Entity<InputState>,
    role: Entity<InputState>,
    /// The values the inputs were last filled from, to keep edits on refresh.
    shown: Option<(String, String)>,
    save: Status,
    /// A form check that failed, shown under its own field (§7.1).
    field_error: Option<FieldError>,
    avatar: Status,
    _subscriptions: Vec<Subscription>,
}

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

impl ProfilePage {
    pub fn new(services: Services, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let name = cx.new(|cx| InputState::new(window, cx).placeholder(s::NAME_PLACEHOLDER));
        let role = cx.new(|cx| InputState::new(window, cx).placeholder(s::ROLE_PLACEHOLDER));
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
            _subscriptions: subscriptions,
        };
        this.fill_from_me(window, cx);
        this
    }

    /// Return and leaving a changed field save it.
    fn on_input(
        &mut self,
        _: &Entity<InputState>,
        e: &InputEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match e {
            InputEvent::PressEnter { .. } => self.save(window, cx),
            InputEvent::Blur if self.edited(cx) => self.save(window, cx),
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

    /// True when the fields differ from the values they were filled from.
    fn edited(&self, cx: &App) -> bool {
        let current = (
            self.name.read(cx).value().to_string(),
            self.role.read(cx).value().to_string(),
        );
        self.shown.as_ref() != Some(&current)
    }

    fn me(&self) -> Option<Member> {
        match self.services.provider.session() {
            SessionState::SignedIn { me, .. } => Some(me),
            _ => None,
        }
    }

    /// Fills the inputs from my member row unless the user has edited them.
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
            self.name
                .update(cx, |st, cx| st.set_value(fresh.0.clone(), window, cx));
            self.role
                .update(cx, |st, cx| st.set_value(fresh.1.clone(), window, cx));
            self.shown = Some(fresh);
        }
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

    /// The form check that failed, if any.
    pub fn field_error(&self) -> Option<&FieldError> {
        self.field_error.as_ref()
    }

    pub fn save(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
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
        cx.spawn(async move |this, cx| {
            let result = answer(reply).await;
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

    fn confirm_wipe(&self, window: &mut Window, cx: &mut Context<Self>) {
        let provider = self.services.provider.clone();
        window.open_alert_dialog(cx, move |alert, _, cx| {
            let alert = crate::ui::SheetExt::sheet(alert, cx);
            let provider = provider.clone();
            alert
                .title(s::SIGN_OUT_WIPE_TITLE)
                .description(s::SIGN_OUT_WIPE_BODY)
                .ok_text(s::SIGN_OUT_WIPE)
                .ok_variant(ButtonVariant::Danger)
                .cancel_text(common::CANCEL)
                .show_cancel(true)
                .on_ok(move |_, _, _| {
                    provider.sign_out(true);
                    true
                })
        });
    }
}

impl Render for ProfilePage {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let colors = theme::colors(cx);
        let me = self.me();
        let signed_in = me.is_some();
        let error_of = |field: Field| {
            self.field_error
                .as_ref()
                .filter(|(f, _)| *f == field)
                .map(|(_, e)| e.clone())
        };
        let name_error = error_of(Field::Name);
        let role_error = error_of(Field::Role);
        div()
            .flex()
            .flex_col()
            // Avatar: the picture, 更换头像… and its help (§8.4).
            .child(widgets::section_label(s::AVATAR, true, cx))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(space::S16)
                    .child(div().flex_none().child(avatar(me.as_ref(), px(64.), cx)))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .items_start()
                            .gap(space::S6)
                            .child(
                                ui::secondary_button("avatar-choose", s::AVATAR_CHOOSE, cx)
                                    .loading(self.avatar.busy())
                                    .disabled(!signed_in || self.avatar.busy())
                                    .on_click(cx.listener(|this, _, _, cx| this.choose_avatar(cx))),
                            )
                            .child(
                                match widgets::status_line("avatar-status", &self.avatar, cx) {
                                    Some(line) => line,
                                    None => widgets::hint(s::AVATAR_HINT, cx),
                                },
                            ),
                    ),
            )
            // Name and 岗位 save on blur and on Return (§8.4).
            .child(
                div()
                    .pt(space::S24)
                    .flex()
                    .flex_col()
                    .gap(space::S16)
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .child(widgets::field(
                                s::NAME,
                                ui::TextField::new("profile-name", &self.name, ui::FieldKind::Form)
                                    .error(name_error.is_some())
                                    .disabled(!signed_in),
                                cx,
                            ))
                            // The error of a field sits 6 pt under it (§7.1).
                            .children(name_error.map(|e| ui::field_help(e, true, cx))),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .child(widgets::field(
                                s::ROLE,
                                ui::TextField::new("profile-role", &self.role, ui::FieldKind::Form)
                                    .error(role_error.is_some())
                                    .disabled(!signed_in),
                                cx,
                            ))
                            .child(match role_error {
                                Some(e) => ui::field_help(e, true, cx),
                                None => div().pt(space::S6).child(
                                    match widgets::status_line("save-status", &self.save, cx) {
                                        Some(line) => line,
                                        None => widgets::hint(s::PROFILE_SAVE_HINT, cx),
                                    },
                                ),
                            }),
                    ),
            )
            // Sign out, then the danger row on its own (U40).
            .child(widgets::section_label(s::ACCOUNT, false, cx))
            .child(widgets::table_of(
                None,
                vec![
                    widgets::table_row()
                        .child(widgets::title_cell(
                            s::SIGN_OUT,
                            Some(s::SIGN_OUT_HINT.into()),
                            cx,
                        ))
                        .child(
                            ui::secondary_button("sign-out", s::SIGN_OUT, cx)
                                .icon(Icon::new(Lucide::LogOut))
                                .disabled(!signed_in)
                                .on_click(cx.listener(|this, _, _, _| {
                                    this.services.provider.sign_out(false)
                                })),
                        ),
                ],
                cx,
            ))
            .child(div().h(space::S16))
            .child(widgets::table_of(
                None,
                vec![
                    widgets::table_row()
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .flex()
                                .flex_col()
                                .child(
                                    div()
                                        .t_ui()
                                        .medium()
                                        .text_color(colors.text_1)
                                        .child(s::SIGN_OUT_WIPE),
                                )
                                .child(
                                    div()
                                        .t_small()
                                        .text_color(colors.text_2)
                                        .child(s::SIGN_OUT_WIPE_HINT),
                                ),
                        )
                        .child(
                            // Secondary with a danger label: the alert that
                            // follows carries the filled danger button.
                            ui::secondary_button("sign-out-wipe", s::SIGN_OUT_WIPE_BUTTON, cx)
                                .text_color(colors.danger_text)
                                .disabled(!signed_in)
                                .on_click(
                                    cx.listener(|this, _, window, cx| {
                                        this.confirm_wipe(window, cx)
                                    }),
                                ),
                        ),
                ],
                cx,
            ))
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
}
