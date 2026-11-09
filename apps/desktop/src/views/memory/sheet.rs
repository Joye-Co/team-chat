//! The 记一条 and 编辑 sheet of 记得的事 and 团队约定 (NATIVE.md §7.1, COMPLETENESS §2.9): 类型
//! (a pop-up), 内容 (a textarea row) and 什么时候用 (one line). The default button is off
//! while nothing can be saved, shows its spinner while the server answers, and a refusal
//! opens an alert while the sheet stays open.

use std::cell::Cell;

use gpui::{
    App, AppContext as _, Context, Entity, Focusable as _, IntoElement, ParentElement as _, Render,
    SharedString, Styled as _, Subscription, WeakEntity, Window,
};
use gpui_component::{
    WindowExt as _,
    input::{InputEvent, InputState, TextareaState},
    menu::{DropdownMenu as _, PopupMenuItem},
};

use super::{Host, MemoryBoard, model};
use crate::{
    app::Services,
    copy::{common, fill, memory as c},
    data::{
        AdminError,
        memory::{
            MAX_TEXT_CHARS, MAX_WHEN_CHARS, Memory, MemoryEdit, MemoryKind, MemoryScope, NewMemory,
        },
    },
    ui::native::{
        NativeTextArea, NativeTextField, PushSize, SheetLayout, SheetWidth, disabled_push,
        native_sheet, popup_button, sheet_buttons,
    },
    views::nav::dialogs::{
        error_alert, form_field_width, sheet_footnote, sheet_form, sheet_form_row,
        sheet_form_row_top,
    },
};

/// The kinds a person picks from in a scope (and the edited memory's own kind).
pub fn kinds_for(scope: &MemoryScope, current: Option<MemoryKind>) -> Vec<MemoryKind> {
    let mut kinds = match scope {
        MemoryScope::Team => vec![MemoryKind::Convention, MemoryKind::Fact],
        MemoryScope::Member(_) => vec![
            MemoryKind::Lesson,
            MemoryKind::Playbook,
            MemoryKind::Preference,
            MemoryKind::Fact,
        ],
    };
    if let Some(k) = current.filter(|k| *k != MemoryKind::Other && !kinds.contains(k)) {
        kinds.push(k);
    }
    kinds
}

/// What a save sends, or why it cannot be sent (`Err(Some(note))` shows a footnote).
pub fn pending_edit(
    original: &Memory,
    kind: MemoryKind,
    text: &str,
    when: &str,
) -> Result<MemoryEdit, Option<String>> {
    let text = model::checked_text(text, MAX_TEXT_CHARS).map_err(too_long(MAX_TEXT_CHARS))?;
    let when = when.trim();
    if when.chars().count() > MAX_WHEN_CHARS {
        return Err(Some(fill(
            c::SHEET_TOO_LONG,
            &[("n", &MAX_WHEN_CHARS.to_string())],
        )));
    }
    let when = (!when.is_empty()).then(|| when.to_string());
    let edit = MemoryEdit {
        kind: (kind != original.kind).then_some(kind),
        text: (text != original.text).then_some(text),
        when: (when != original.when).then_some(when),
        status: None,
    };
    if edit.is_empty() { Err(None) } else { Ok(edit) }
}

fn too_long(max: usize) -> impl Fn(bool) -> Option<String> {
    move |long| long.then(|| fill(c::SHEET_TOO_LONG, &[("n", &max.to_string())]))
}

struct MemorySheet {
    services: Services,
    board: WeakEntity<MemoryBoard>,
    scope: MemoryScope,
    /// `None`: 记一条.
    original: Option<Memory>,
    member_name: String,
    kind: MemoryKind,
    text: Entity<TextareaState>,
    when: Entity<InputState>,
    busy: bool,
    _subscriptions: Vec<Subscription>,
}

enum Ready {
    New(NewMemory),
    Edit(String, MemoryEdit),
}

impl MemorySheet {
    fn check(&self, cx: &App) -> Result<Ready, Option<String>> {
        let text = self.text.read(cx).value().to_string();
        let when = self.when.read(cx).value().to_string();
        match &self.original {
            Some(m) => {
                pending_edit(m, self.kind, &text, &when).map(|e| Ready::Edit(m.id.clone(), e))
            }
            None => {
                let text =
                    model::checked_text(&text, MAX_TEXT_CHARS).map_err(too_long(MAX_TEXT_CHARS))?;
                let when = when.trim();
                if when.chars().count() > MAX_WHEN_CHARS {
                    return Err(Some(fill(
                        c::SHEET_TOO_LONG,
                        &[("n", &MAX_WHEN_CHARS.to_string())],
                    )));
                }
                Ok(Ready::New(NewMemory {
                    scope: self.scope.clone(),
                    kind: self.kind,
                    text,
                    when: (!when.is_empty()).then(|| when.to_string()),
                }))
            }
        }
    }

    fn submit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.busy {
            return;
        }
        let Ok(ready) = self.check(cx) else {
            return;
        };
        let p = &self.services.provider;
        let reply = match ready {
            Ready::New(new) => p.create_memory(new),
            Ready::Edit(id, edit) => p.update_memory(&id, edit),
        };
        self.busy = true;
        cx.notify();
        let board = self.board.clone();
        cx.spawn_in(window, async move |this, cx| {
            let result = reply.await.unwrap_or(Err(AdminError::Other));
            let _ = this.update_in(cx, |this, window, cx| {
                this.busy = false;
                match result {
                    Ok(_) => {
                        window.close_dialog(cx);
                        let _ = board.update(cx, |b, cx| b.load(cx));
                    }
                    Err(e) => {
                        error_alert(
                            c::SHEET_REFUSED,
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

impl Render for MemorySheet {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let check = self.check(cx);
        let ready = !self.busy && check.is_ok();
        let note = check.err().flatten();
        let field_w = form_field_width(SheetWidth::Form);
        let kinds = kinds_for(&self.scope, self.original.as_ref().map(|m| m.kind));
        let current = self.kind;
        let this = cx.entity().downgrade();
        let kind = popup_button(
            "memory-sheet-kind",
            model::kind_word(self.kind),
            PushSize::Regular,
            window,
            cx,
        )
        .w(field_w)
        .dropdown_menu(move |menu, _, _| {
            kinds.iter().fold(menu, |menu, k| {
                let this = this.clone();
                let pick = *k;
                menu.item(
                    PopupMenuItem::new(model::kind_word(*k))
                        .checked(current == *k)
                        .on_click(move |_, _, cx| {
                            let _ = this.update(cx, |this, cx| {
                                this.kind = pick;
                                cx.notify();
                            });
                        }),
                )
            })
        });
        let form = sheet_form()
            .child(sheet_form_row(c::SHEET_KIND, kind, cx))
            .child(sheet_form_row_top(
                c::SHEET_TEXT,
                NativeTextArea::new("memory-sheet-text", &self.text).rows(4),
                cx,
            ))
            .child(sheet_form_row(
                c::SHEET_WHEN,
                NativeTextField::new("memory-sheet-when", &self.when).width(field_w),
                cx,
            ))
            .children(note.map(|n| sheet_footnote(n, cx)));
        let editing = self.original.is_some();
        let (title, about, ok) = if editing {
            (
                c::SHEET_EDIT,
                c::SHEET_EDIT_ABOUT.to_string(),
                c::SHEET_SAVE,
            )
        } else {
            (
                c::SHEET_NEW,
                match &self.scope {
                    MemoryScope::Team => c::SHEET_NEW_TEAM.to_string(),
                    MemoryScope::Member(_) => {
                        fill(c::SHEET_NEW_MEMBER, &[("name", &self.member_name)])
                    }
                },
                c::SHEET_SAVE_NEW,
            )
        };
        let [cancel, save] = sheet_buttons("memory-sheet", common::CANCEL, ok, cx);
        SheetLayout::new(title)
            .explanation(SharedString::from(about))
            .body(form)
            .buttons([
                cancel.on_click(|_, window, cx| window.close_dialog(cx)),
                disabled_push(save, !ready, cx)
                    .loading(self.busy)
                    .on_click(cx.listener(|this, _, window, cx| this.submit(window, cx))),
            ])
    }
}

/// Who a sheet writes for: the board it reloads, its scope, its window and the member.
pub struct Target {
    pub services: Services,
    pub board: WeakEntity<MemoryBoard>,
    pub scope: MemoryScope,
    pub host: Host,
    pub member_name: String,
}

/// Opens 记一条 (`original: None`) or 编辑 over the window of `target.host`.
pub fn open(target: Target, original: Option<Memory>, window: &mut Window, cx: &mut App) {
    if window.has_active_dialog(cx) {
        return;
    }
    let Target {
        services,
        board,
        scope,
        host,
        member_name,
    } = target;
    let kind = original
        .as_ref()
        .map(|m| m.kind)
        .filter(|k| *k != MemoryKind::Other)
        .unwrap_or(kinds_for(&scope, None)[0]);
    let sheet = cx.new(|cx| {
        let text = cx.new(|cx| {
            TextareaState::new(window, cx)
                .placeholder(match scope {
                    MemoryScope::Team => c::SHEET_TEXT_PLACEHOLDER_TEAM,
                    MemoryScope::Member(_) => c::SHEET_TEXT_PLACEHOLDER_MEMBER,
                })
                .default_value(
                    original
                        .as_ref()
                        .map(|m| m.text.clone())
                        .unwrap_or_default(),
                )
        });
        let when = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder(c::SHEET_WHEN_PLACEHOLDER)
                .default_value(
                    original
                        .as_ref()
                        .and_then(|m| m.when.clone())
                        .unwrap_or_default(),
                )
        });
        let subscriptions = vec![
            cx.subscribe_in(
                &text,
                window,
                |_: &mut MemorySheet, _, event: &InputEvent, _, cx| {
                    if matches!(event, InputEvent::Change) {
                        cx.notify();
                    }
                },
            ),
            cx.subscribe_in(
                &when,
                window,
                |this: &mut MemorySheet, _, event: &InputEvent, window, cx| match event {
                    InputEvent::PressEnter { .. } => this.submit(window, cx),
                    InputEvent::Change => cx.notify(),
                    _ => {}
                },
            ),
        ];
        MemorySheet {
            services,
            board,
            scope,
            original,
            member_name,
            kind,
            text,
            when,
            busy: false,
            _subscriptions: subscriptions,
        }
    });
    let first = Cell::new(true);
    let build = move |dialog: gpui_component::dialog::Dialog, window: &mut Window, cx: &mut App| {
        if first.replace(false) {
            let text = sheet.read(cx).text.clone();
            text.read(cx).focus_handle(cx).focus(window, cx);
        }
        dialog.child(sheet.clone())
    };
    match host {
        Host::Settings => crate::views::settings::widgets::open_sheet(
            window,
            cx,
            SheetWidth::Form,
            gpui::px(560.),
            build,
        ),
        Host::Page => window.open_dialog(cx, move |dialog, window, cx| {
            build(
                native_sheet(dialog, SheetWidth::Form, cx).px_0(),
                window,
                cx,
            )
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::memory::{MemorySourceKind, MemoryStatus};

    fn original() -> Memory {
        Memory {
            id: "m1".into(),
            scope: "team".into(),
            kind: MemoryKind::Convention,
            text: "周五下午不发生产。".into(),
            when: None,
            source: MemorySourceKind::Manual,
            source_ids: Vec::new(),
            note: None,
            author_id: "m-joye".into(),
            human: true,
            status: MemoryStatus::Active,
            uses: 0,
            last_used_at: None,
            version: 1,
            created_at: 0,
            updated_at: 0,
        }
    }

    #[test]
    fn an_edit_sends_only_what_changed() {
        let m = original();
        assert_eq!(
            pending_edit(&m, MemoryKind::Convention, " 周五下午不发生产。 ", ""),
            Err(None),
            "nothing changed"
        );
        let e = pending_edit(&m, MemoryKind::Fact, "周五下午不发生产。", " 上线前 ").unwrap();
        assert_eq!(e.kind, Some(MemoryKind::Fact));
        assert_eq!(e.text, None);
        assert_eq!(e.when, Some(Some("上线前".into())));
        assert_eq!(pending_edit(&m, m.kind, "  ", ""), Err(None));
        assert_eq!(
            pending_edit(&m, m.kind, &"字".repeat(501), ""),
            Err(Some("最多 500 个字".into()))
        );
        assert_eq!(
            pending_edit(&m, m.kind, "x", &"字".repeat(301)),
            Err(Some("最多 300 个字".into()))
        );
    }

    #[test]
    fn each_scope_offers_its_own_kinds() {
        assert_eq!(
            kinds_for(&MemoryScope::Team, None),
            vec![MemoryKind::Convention, MemoryKind::Fact]
        );
        let member = kinds_for(
            &MemoryScope::Member("b1".into()),
            Some(MemoryKind::Decision),
        );
        assert_eq!(member[0], MemoryKind::Lesson);
        assert!(
            member.contains(&MemoryKind::Decision),
            "the edited kind stays pickable"
        );
    }
}
