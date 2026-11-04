//! The 重命名群 sheet (NATIVE.md §7.1), from a 群's menu in the sidebar.
//! The same sheet as 重命名话题: one prefilled field with its text
//! selected, 取消 and 保存; 保存 is off while the name is empty, unchanged
//! or too long (one quiet footnote says why); a refusal from the server
//! opens an alert (NATIVE.md §7.4).

use std::cell::Cell;

use gpui::{
    App, AppContext as _, Context, Entity, Focusable as _, IntoElement, ParentElement, Render, Styled as _,
    SharedString, Subscription, Window, px,
};
use gpui_component::{
    WindowExt as _,
    input::{InputEvent, InputState},
};

use super::dialogs::{
    error_alert, form_field_width, sheet_footnote, sheet_form, sheet_form_row,
    validate_group_name,
};
use crate::{
    app::Services,
    copy::{common, nav, overlays as o},
    data::{AdminError, GroupId},
    ui::native::{
        NativeTextField, SheetLayout, SheetWidth, disabled_push, native_sheet, sheet_buttons,
    },
};

/// Whether 保存 can rename: a valid name that differs from the current one.
pub fn can_save(current: &str, typed: &str) -> bool {
    validate_group_name(typed).is_ok_and(|name| name != current)
}

/// The footnote of a typed name: why 保存 is off, when the name has a
/// problem other than being empty.
pub fn name_problem(typed: &str) -> Option<String> {
    if typed.trim().is_empty() {
        return None;
    }
    validate_group_name(typed).err()
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
    _subscriptions: Vec<Subscription>,
}

impl RenameGroupForm {
    fn submit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let typed = self.name.read(cx).value().to_string();
        if self.busy || !can_save(&self.current, &typed) {
            return;
        }
        let Ok(name) = validate_group_name(&typed) else {
            return;
        };
        let reply = self.services.provider.rename_group(&self.group_id, name);
        self.busy = true;
        cx.notify();
        // The dialog stays open until the server answers, so a refusal can be shown.
        cx.spawn_in(window, async move |this, cx| {
            let result = reply.await.unwrap_or(Err(AdminError::Other));
            let _ = this.update_in(cx, |this, window, cx| {
                this.busy = false;
                match result {
                    Ok(()) => window.close_dialog(cx),
                    Err(e) => {
                        refused(e, window, cx);
                        cx.notify();
                    }
                }
            });
        })
        .detach();
    }
}

/// A refused rename blocks: an alert over the sheet. Removed from the 群,
/// the sheet closes with the alert; otherwise it stays to try again.
fn refused(e: AdminError, window: &mut Window, cx: &mut gpui::App) {
    let gone = e == AdminError::Forbidden;
    error_alert(
        o::RENAME_GROUP_REFUSED,
        refusal_text(e),
        move |window, cx| {
            if gone {
                window.close_dialog(cx);
            }
        },
        window,
        cx,
    );
}

impl Render for RenameGroupForm {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let typed = self.name.read(cx).value().to_string();
        let ready = !self.busy && can_save(&self.current, &typed);
        let [cancel, save] = sheet_buttons("rename-group", common::CANCEL, common::SAVE, cx);
        rename_sheet(
            nav::RENAME_GROUP_TITLE,
            o::RENAME_GROUP_EXPLANATION,
            o::GROUP_NAME,
            NativeTextField::new("rename-group-name", &self.name)
                .width(form_field_width(SheetWidth::Form)),
            name_problem(&typed).map(SharedString::from),
            cancel.on_click(|_, window, cx| window.close_dialog(cx)),
            disabled_push(save, !ready, cx)
                .loading(self.busy)
                .on_click(cx.listener(|this, _, window, cx| this.submit(window, cx))),
            cx,
        )
    }
}

/// The rename sheet (§7.1) of a 群 or a 话题: Title 2, one line that says
/// who sees the change, the labelled field on the sheet background, an
/// optional quiet footnote under it, then 取消 and 保存.
#[allow(clippy::too_many_arguments)]
pub fn rename_sheet(
    title: &'static str,
    explanation: &'static str,
    label: &'static str,
    field: NativeTextField,
    footnote: Option<SharedString>,
    cancel: gpui_component::button::Button,
    save: gpui_component::button::Button,
    cx: &App,
) -> SheetLayout {
    SheetLayout::new(title)
        .explanation(explanation)
        .body(
            sheet_form()
                .child(sheet_form_row(label, field, cx))
                .children(footnote.map(|f| sheet_footnote(f, cx))),
        )
        .buttons([cancel, save])
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
/// self-test, the server's refusal `e` already shown in its alert.
pub fn open_with(
    services: Services,
    group_id: GroupId,
    current: String,
    preview: Option<(String, AdminError)>,
    window: &mut Window,
    cx: &mut App,
) {
    let typed = preview
        .as_ref()
        .map(|(t, _)| t.clone())
        .unwrap_or_else(|| current.clone());
    let refusal = preview.as_ref().map(|(_, e)| *e);
    let form = cx.new(|cx| {
        let name = cx.new(|cx| InputState::new(window, cx).default_value(typed));
        let subscriptions = vec![cx.subscribe_in(
            &name,
            window,
            |this: &mut RenameGroupForm, _, event: &InputEvent, window, cx| match event {
                InputEvent::PressEnter { .. } => this.submit(window, cx),
                InputEvent::Change => cx.notify(),
                _ => {}
            },
        )];
        RenameGroupForm {
            services,
            group_id,
            current,
            name,
            busy: false,
            _subscriptions: subscriptions,
        }
    });
    let first = Cell::new(true);
    window.open_dialog(cx, move |dialog, window, cx| {
        if first.replace(false) {
            let name = form.read(cx).name.clone();
            name.read(cx).focus_handle(cx).focus(window, cx);
            name.update(cx, |state, cx| state.select_all(window, cx));
        }
        native_sheet(dialog, SheetWidth::Form, cx).px(px(0.)).child(form.clone())
    });
    if let Some(e) = refusal {
        refused(e, window, cx);
    }
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
        // Too long: 保存 is off and the footnote says why.
        assert!(!can_save("全员", &"长".repeat(65)));
        assert_eq!(
            name_problem(&"长".repeat(65)).as_deref(),
            Some("群名称最多 64 个字")
        );
        assert_eq!(name_problem("  "), None);
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
