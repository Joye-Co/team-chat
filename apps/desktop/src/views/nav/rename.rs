//! The 重命名群 dialog (DESIGN.md §7.12), from a 群's menu in the sidebar.
//! The same sheet as 重命名话题: one prefilled field with its text
//! selected, 取消 and 保存; 保存 is off while the name is empty or
//! unchanged; a refusal shows under the field and the dialog stays open.

use std::cell::Cell;

use gpui::{
    App, AppContext as _, Context, Entity, Focusable as _, IntoElement, ParentElement, Render,
    SharedString, Styled, Subscription, Window, div, prelude::FluentBuilder as _,
};
use gpui_component::{
    WindowExt as _,
    input::{InputEvent, InputState},
};

use super::dialogs::validate_group_name;
use crate::{
    app::Services,
    copy::{common, nav},
    data::{AdminError, GroupId},
    theme::{Type as _, space},
    ui::{self, ButtonSpec as _, FieldKind},
};

/// Whether 保存 can rename: a valid name that differs from the current one.
/// A name that is too long stays savable, so 保存 can say why.
pub fn can_save(current: &str, typed: &str) -> bool {
    match validate_group_name(typed) {
        Ok(name) => name != current,
        Err(_) => !typed.trim().is_empty(),
    }
}

/// The error text of a refused rename.
pub fn refusal_text(e: AdminError) -> &'static str {
    match e {
        // Removed from the 群 while the dialog was open.
        AdminError::Forbidden => nav::RENAME_GROUP_FORBIDDEN,
        other => crate::views::settings::widgets::error_text(other),
    }
}

struct RenameGroupForm {
    services: Services,
    group_id: GroupId,
    current: String,
    name: Entity<InputState>,
    busy: bool,
    error: Option<SharedString>,
    _subscriptions: Vec<Subscription>,
}

impl RenameGroupForm {
    fn submit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let typed = self.name.read(cx).value().to_string();
        if self.busy || !can_save(&self.current, &typed) {
            return;
        }
        let name = match validate_group_name(&typed) {
            Ok(name) => name,
            Err(e) => {
                self.error = Some(e.into());
                cx.notify();
                return;
            }
        };
        let reply = self.services.provider.rename_group(&self.group_id, name);
        self.busy = true;
        self.error = None;
        cx.notify();
        // The dialog stays open until the server answers, so a refusal can be shown.
        cx.spawn_in(window, async move |this, cx| {
            let result = reply.await.unwrap_or(Err(AdminError::Other));
            let _ = this.update_in(cx, |this, window, cx| {
                this.busy = false;
                match result {
                    Ok(()) => window.close_dialog(cx),
                    Err(e) => {
                        this.error = Some(refusal_text(e).into());
                        cx.notify();
                    }
                }
            });
        })
        .detach();
    }
}

impl Render for RenameGroupForm {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let ready = !self.busy && can_save(&self.current, &self.name.read(cx).value());
        div()
            .flex()
            .flex_col()
            .t_ui()
            .child(ui::field_label(nav::GROUP_NAME_LABEL, cx))
            .child(
                ui::TextField::new("rename-group-name", &self.name, FieldKind::Form)
                    .error(self.error.is_some()),
            )
            .when_some(self.error.clone(), |el, e| {
                el.child(ui::field_help(e, true, cx))
            })
            .child(
                div()
                    .pt(space::S20)
                    .flex()
                    .justify_end()
                    .gap(space::S8)
                    .child(
                        ui::secondary_button("rename-group-cancel", common::CANCEL, cx)
                            .on_click(|_, window, cx| window.close_dialog(cx)),
                    )
                    .child(
                        ui::primary_button("rename-group-save", common::SAVE, cx)
                            .loading(self.busy)
                            .disabled_fill(!ready, cx)
                            .on_click(cx.listener(|this, _, window, cx| this.submit(window, cx))),
                    ),
            )
    }
}

/// Opens the dialog that renames a 群, its name prefilled and selected.
pub fn open_rename_group(
    services: Services,
    group_id: GroupId,
    current: String,
    window: &mut Window,
    cx: &mut App,
) {
    open_with(services, group_id, current, None, window, cx);
}

/// [`open_rename_group`] with the field set to `typed` and, for the
/// self-test, an error already shown.
pub fn open_with(
    services: Services,
    group_id: GroupId,
    current: String,
    preview: Option<(String, SharedString)>,
    window: &mut Window,
    cx: &mut App,
) {
    let typed = preview
        .as_ref()
        .map(|(t, _)| t.clone())
        .unwrap_or_else(|| current.clone());
    let form = cx.new(|cx| {
        let name = cx.new(|cx| InputState::new(window, cx).default_value(typed));
        let subscriptions = vec![cx.subscribe_in(
            &name,
            window,
            |this: &mut RenameGroupForm, _, event: &InputEvent, window, cx| match event {
                InputEvent::PressEnter { .. } => this.submit(window, cx),
                InputEvent::Change => {
                    this.error = None;
                    cx.notify();
                }
                _ => {}
            },
        )];
        RenameGroupForm {
            services,
            group_id,
            current,
            name,
            busy: false,
            error: preview.map(|(_, e)| e),
            _subscriptions: subscriptions,
        }
    });
    let first = Cell::new(true);
    window.open_dialog(cx, move |dialog, window, cx| {
        let dialog = crate::ui::SheetExt::sheet(dialog, cx);
        if first.replace(false) {
            let name = form.read(cx).name.clone();
            name.read(cx).focus_handle(cx).focus(window, cx);
            name.update(cx, |state, cx| state.select_all(window, cx));
        }
        dialog
            .close_button(false)
            .p(space::S20)
            .title(div().t_title().pb(space::S8).child(nav::RENAME_GROUP_TITLE))
            .child(form.clone())
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn save_needs_a_new_name() {
        assert!(!can_save("全员", "全员"));
        assert!(!can_save("全员", "  全员 "));
        assert!(!can_save("全员", "   "));
        assert!(can_save("General", "全员"));
        // Too long: 保存 stays on and says why.
        assert!(can_save("全员", &"长".repeat(65)));
        assert_eq!(
            refusal_text(AdminError::Forbidden),
            nav::RENAME_GROUP_FORBIDDEN
        );
        assert_eq!(
            refusal_text(AdminError::Offline),
            crate::copy::settings::ERROR_OFFLINE
        );
    }
}
