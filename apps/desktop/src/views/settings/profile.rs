//! 个人: my name, 岗位 and avatar, and sign out.

use gpui::{
    AppContext as _, Context, Entity, IntoElement, ParentElement, PathPromptOptions, Render,
    Styled, Subscription, Window, div, px,
};
use gpui_component::{
    Disableable as _, Icon, Sizable as _, WindowExt as _,
    button::{Button, ButtonVariant, ButtonVariants as _},
    input::{Input, InputEvent, InputState},
};

use super::widgets::{self, Status, answer};
use crate::{
    app::{Changes, Services},
    copy::{common, settings as s},
    data::{Change, Member, SessionState},
    theme::{self, Type as _, space},
    ui::Lucide,
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
    avatar: Status,
    _subscriptions: Vec<Subscription>,
}

/// Checks the profile form. Returns the trimmed name and 岗位 or an error.
pub fn validate(name: &str, role: &str) -> Result<(String, Option<String>), String> {
    let name = name.trim();
    let role = role.trim();
    if name.is_empty() {
        return Err(s::NAME_REQUIRED.to_string());
    }
    widgets::check_len(name, NAME_MAX)?;
    widgets::check_len(role, ROLE_MAX)?;
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
            cx.subscribe_in(&name, window, |this, _, e: &InputEvent, window, cx| {
                if matches!(e, InputEvent::PressEnter { .. }) {
                    this.save(window, cx);
                }
            }),
            cx.subscribe_in(&role, window, |this, _, e: &InputEvent, window, cx| {
                if matches!(e, InputEvent::PressEnter { .. }) {
                    this.save(window, cx);
                }
            }),
        ];
        let mut this = Self {
            services,
            name,
            role,
            shown: None,
            save: Status::Idle,
            avatar: Status::Idle,
            _subscriptions: subscriptions,
        };
        this.fill_from_me(window, cx);
        this
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

    pub fn save(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        if self.save.busy() {
            return;
        }
        let name = self.name.read(cx).value().to_string();
        let role = self.role.read(cx).value().to_string();
        let (name, role) = match validate(&name, &role) {
            Ok(v) => v,
            Err(e) => {
                self.save = Status::Failed(e.into());
                cx.notify();
                return;
            }
        };
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
        window.open_alert_dialog(cx, move |alert, _, _| {
            let alert = crate::ui::SheetExt::sheet(alert);
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
        let name = me.as_ref().map(|m| m.name.clone()).unwrap_or_default();
        div()
            .flex()
            .flex_col()
            // Avatar: the picture, my name, the file rule and 更换头像….
            .child(
                widgets::group(cx).child(
                    widgets::group_row()
                        .gap(space::S12)
                        .child(div().flex_none().child(avatar(me.as_ref(), px(48.), cx)))
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .flex()
                                .flex_col()
                                .gap(space::S2)
                                .child(div().t_ui().medium().one_line().child(name))
                                .child(
                                    match widgets::status_line(&self.avatar, cx) {
                                        Some(line) => line,
                                        None => widgets::hint(s::AVATAR_HINT, cx),
                                    }
                                    .one_line(),
                                ),
                        )
                        .child(
                            Button::new("avatar-choose")
                                .small()
                                .label(s::AVATAR_CHOOSE)
                                .loading(self.avatar.busy())
                                .disabled(!signed_in || self.avatar.busy())
                                .on_click(cx.listener(|this, _, _, cx| this.choose_avatar(cx))),
                        ),
                ),
            )
            .child(div().h(space::S20))
            .child(widgets::group_rows(
                vec![
                    widgets::group_field(s::NAME, Input::new(&self.name), cx),
                    widgets::group_field(s::ROLE, Input::new(&self.role), cx),
                ],
                space::S12,
                cx,
            ))
            .child(
                div()
                    .pt(space::S12)
                    .flex()
                    .items_center()
                    .gap(space::S12)
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .px(space::S12)
                            .children(widgets::status_line(&self.save, cx)),
                    )
                    .child(
                        Button::new("profile-save")
                            .primary()
                            .small()
                            .label(common::SAVE)
                            .loading(self.save.busy())
                            .disabled(!signed_in || self.save.busy())
                            .on_click(cx.listener(|this, _, window, cx| this.save(window, cx))),
                    ),
            )
            .child(widgets::section_header(s::ACCOUNT, cx))
            .child(
                widgets::group(cx).child(
                    widgets::group_row()
                        .gap(space::S8)
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .t_callout()
                                .text_color(colors.text_2)
                                .child(s::SIGN_OUT_HINT),
                        )
                        .child(
                            Button::new("sign-out-wipe")
                                .small()
                                .ghost()
                                .text_color(colors.danger_text)
                                .label(s::SIGN_OUT_WIPE)
                                .disabled(!signed_in)
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.confirm_wipe(window, cx)
                                })),
                        )
                        .child(
                            Button::new("sign-out")
                                .small()
                                .icon(Icon::new(Lucide::LogOut))
                                .label(s::SIGN_OUT)
                                .disabled(!signed_in)
                                .on_click(cx.listener(|this, _, _, _| {
                                    this.services.provider.sign_out(false)
                                })),
                        ),
                ),
            )
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
        assert!(validate("  ", "x").is_err());
        assert!(validate(&"长".repeat(65), "").is_err());
        assert!(validate("a", &"长".repeat(65)).is_err());
    }
}
