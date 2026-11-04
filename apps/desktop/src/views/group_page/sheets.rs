//! Sheets of the 群 page (NATIVE.md §7.1): 记一条 (`POST /v1/groups/:id/decisions`) and
//! 群说明 (`PATCH /v1/groups/:id` with `purpose`). Each has one field; the default button
//! is off while the text cannot be sent, shows its spinner while the server answers, and a
//! refusal opens an alert while the sheet stays open to try again.

use std::cell::Cell;

use gpui::{
    App, AppContext as _, Context, Entity, Focusable as _, IntoElement, ParentElement as _, Render,
    SharedString, Styled as _, Subscription, Window,
};
use gpui_component::{
    WindowExt as _,
    input::{InputEvent, InputState},
};

use crate::{
    app::Services,
    copy::{common, group_page as c},
    data::{AdminError, AdminReply},
    ui::native::{
        NativeTextField, SheetLayout, SheetWidth, disabled_push, native_sheet, sheet_buttons,
    },
    views::nav::dialogs::{
        error_alert, form_field_width, sheet_footnote, sheet_form, sheet_form_row,
    },
};

/// Which field the sheet edits.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    /// 记一条: a decision, 1..=1000 characters after trimming.
    Decision,
    /// 群说明: the purpose line, 0..=200 characters after normalizing (`""` clears).
    Purpose,
}

/// The text to send, or why it cannot be sent (`Err(Some(note))` shows a footnote).
fn checked(kind: Kind, typed: &str, current: &str) -> Result<String, Option<&'static str>> {
    match kind {
        Kind::Decision => {
            let t = typed.trim();
            if t.is_empty() {
                Err(None)
            } else if t.chars().count() > chat_core::proto::MAX_RECORDED_DECISION_CHARS {
                Err(Some(c::GROUP_PAGE_RECORD_TOO_LONG))
            } else {
                Ok(t.to_string())
            }
        }
        Kind::Purpose => {
            let t = chat_core::proto::normalize_group_text(typed);
            if t.chars().count() > chat_core::proto::MAX_GROUP_PURPOSE_CHARS {
                Err(Some(c::GROUP_PAGE_PURPOSE_TOO_LONG))
            } else if t == chat_core::proto::normalize_group_text(current) {
                Err(None)
            } else {
                Ok(t)
            }
        }
    }
}

struct TextSheet {
    services: Services,
    group_id: String,
    kind: Kind,
    current: String,
    input: Entity<InputState>,
    busy: bool,
    _subscriptions: Vec<Subscription>,
}

impl TextSheet {
    fn submit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.busy {
            return;
        }
        let typed = self.input.read(cx).value().to_string();
        let Ok(text) = checked(self.kind, &typed, &self.current) else {
            return;
        };
        let p = &self.services.provider;
        let reply: AdminReply<()> = match self.kind {
            Kind::Decision => p.record_decision(&self.group_id, text),
            Kind::Purpose => p.set_group_purpose(&self.group_id, text),
        };
        let refused = match self.kind {
            Kind::Decision => c::GROUP_PAGE_RECORD_REFUSED,
            Kind::Purpose => c::GROUP_PAGE_PURPOSE_REFUSED,
        };
        self.busy = true;
        cx.notify();
        cx.spawn_in(window, async move |this, cx| {
            let result = reply.await.unwrap_or(Err(AdminError::Other));
            let _ = this.update_in(cx, |this, window, cx| {
                this.busy = false;
                match result {
                    Ok(()) => window.close_dialog(cx),
                    Err(e) => {
                        error_alert(
                            refused,
                            crate::views::settings::widgets::error_text(e),
                            |_, _| {},
                            window,
                            cx,
                        );
                        cx.notify();
                    }
                }
            });
        })
        .detach();
    }
}

impl Render for TextSheet {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let typed = self.input.read(cx).value().to_string();
        let check = checked(self.kind, &typed, &self.current);
        let ready = !self.busy && check.is_ok();
        let note = check.err().flatten();
        let (id, title, explanation, label, ok) = match self.kind {
            Kind::Decision => (
                "group-record",
                c::GROUP_PAGE_RECORD_TITLE,
                c::GROUP_PAGE_DECISIONS_NOTE,
                c::GROUP_PAGE_RECORD,
                c::GROUP_PAGE_SAVE,
            ),
            Kind::Purpose => (
                "group-purpose",
                c::GROUP_PAGE_PURPOSE_TITLE,
                c::GROUP_PAGE_PURPOSE_EXPLANATION,
                c::GROUP_PAGE_PURPOSE_LABEL,
                common::SAVE,
            ),
        };
        let [cancel, save] = sheet_buttons(id, common::CANCEL, ok, cx);
        SheetLayout::new(title)
            .explanation(explanation)
            .body(
                sheet_form()
                    .child(sheet_form_row(
                        label,
                        NativeTextField::new(
                            SharedString::from(format!("{id}-field")),
                            &self.input,
                        )
                        .width(form_field_width(SheetWidth::Form)),
                        cx,
                    ))
                    .children(note.map(|n| sheet_footnote(n, cx))),
            )
            .buttons([
                cancel.on_click(|_, window, cx| window.close_dialog(cx)),
                disabled_push(save, !ready, cx)
                    .loading(self.busy)
                    .on_click(cx.listener(|this, _, window, cx| this.submit(window, cx))),
            ])
    }
}

fn open(services: Services, group_id: String, kind: Kind, window: &mut Window, cx: &mut App) {
    if window.has_active_dialog(cx) {
        return;
    }
    let current = match kind {
        Kind::Decision => String::new(),
        Kind::Purpose => services
            .provider
            .group_purpose(&group_id)
            .unwrap_or_default(),
    };
    let sheet = cx.new(|cx| {
        let input = cx.new(|cx| {
            let state = InputState::new(window, cx).placeholder(match kind {
                Kind::Decision => c::GROUP_PAGE_RECORD_PLACEHOLDER,
                Kind::Purpose => c::GROUP_PAGE_PURPOSE_PLACEHOLDER,
            });
            state.default_value(current.clone())
        });
        let subscriptions = vec![cx.subscribe_in(
            &input,
            window,
            |this: &mut TextSheet, _, event: &InputEvent, window, cx| match event {
                InputEvent::PressEnter { .. } => this.submit(window, cx),
                InputEvent::Change => cx.notify(),
                _ => {}
            },
        )];
        TextSheet {
            services,
            group_id,
            kind,
            current,
            input,
            busy: false,
            _subscriptions: subscriptions,
        }
    });
    let first = Cell::new(true);
    window.open_dialog(cx, move |dialog, window, cx| {
        if first.replace(false) {
            let input = sheet.read(cx).input.clone();
            input.read(cx).focus_handle(cx).focus(window, cx);
            input.update(cx, |s, cx| s.select_all(window, cx));
        }
        native_sheet(dialog, SheetWidth::Form, cx)
            .px_0()
            .child(sheet.clone())
    });
}

/// 记一条…: records a decision of the 群 (members read it before they work).
pub fn open_record_sheet(services: Services, group: String, window: &mut Window, cx: &mut App) {
    open(services, group, Kind::Decision, window, cx)
}

/// 修改群说明…: the purpose line under the 群's name.
pub fn open_purpose_sheet(services: Services, group: String, window: &mut Window, cx: &mut App) {
    open(services, group, Kind::Purpose, window, cx)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_decision_needs_text_and_fits_the_server_limit() {
        assert_eq!(checked(Kind::Decision, "   ", ""), Err(None));
        assert_eq!(
            checked(Kind::Decision, "  先发 staging  ", ""),
            Ok("先发 staging".into())
        );
        let long = "决".repeat(1001);
        assert_eq!(
            checked(Kind::Decision, &long, ""),
            Err(Some(c::GROUP_PAGE_RECORD_TOO_LONG))
        );
        assert!(checked(Kind::Decision, &"决".repeat(1000), "").is_ok());
    }

    #[test]
    fn a_purpose_is_normalized_may_be_cleared_and_must_change() {
        assert_eq!(
            checked(Kind::Purpose, " 上线  和\t巡检 ", ""),
            Ok("上线 和 巡检".into())
        );
        assert_eq!(
            checked(Kind::Purpose, "上线 和 巡检", "上线  和 巡检"),
            Err(None)
        );
        assert_eq!(checked(Kind::Purpose, "  ", "上线"), Ok(String::new()));
        assert_eq!(checked(Kind::Purpose, "", ""), Err(None));
        assert_eq!(
            checked(Kind::Purpose, &"群".repeat(201), ""),
            Err(Some(c::GROUP_PAGE_PURPOSE_TOO_LONG))
        );
    }
}
