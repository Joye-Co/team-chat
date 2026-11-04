//! The confirm sheet before an irreversible answer (DESIGN.md §7.10
//! "Approval"): the verb and target as the title, the consequence, and with
//! `typeToConfirm` a field `输入 qqbot-prod 确认发布`; the primary stays
//! disabled until the name matches. Esc and 取消 close it without sending.

use std::cell::Cell;

use gpui::{
    AnyElement, App, AppContext as _, Context, Entity, Focusable as _, IntoElement, ParentElement,
    Render, SharedString, Styled, Subscription, Window, div, px,
};
use gpui_component::{
    Sizable as _, WindowExt as _,
    input::{Input, InputEvent, InputState},
};

use super::Answerer;
use crate::{
    copy::{cards_v2 as c, common, fill},
    data::cards_v2::{AskIrreversible, CardV2Action, CardV2ActionStyle},
    theme::{self, space},
    ui,
};

/// What the sheet says.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct SheetText {
    pub title: String,
    pub body: Option<String>,
    /// The name to type, and the verb for `确认{verb}`.
    pub type_to_confirm: Option<(String, String)>,
    pub ok: String,
    pub danger: bool,
}

/// The sheet text for `action`, from the ask's `irreversible` or the
/// action's `confirm`.
pub(crate) fn sheet_text(
    action: &CardV2Action,
    irreversible: Option<&AskIrreversible>,
) -> SheetText {
    let danger = action.style == Some(CardV2ActionStyle::Danger);
    match irreversible {
        Some(irr) => SheetText {
            title: fill(
                c::CONFIRM_TITLE,
                &[("verb", &irr.verb), ("target", &irr.target)],
            ),
            body: Some(irr.consequence.clone()),
            type_to_confirm: irr.type_to_confirm.clone().map(|t| (t, irr.verb.clone())),
            ok: action.label.clone(),
            danger,
        },
        None => SheetText {
            title: action
                .confirm
                .as_ref()
                .map(|c| c.title.clone())
                .unwrap_or_else(|| action.label.clone()),
            body: action.confirm.as_ref().and_then(|c| c.body.clone()),
            type_to_confirm: None,
            ok: action.label.clone(),
            danger,
        },
    }
}

/// True when the typed text unlocks the primary.
pub(crate) fn matches(typed: &str, expected: Option<&str>) -> bool {
    expected.is_none_or(|e| typed.trim() == e)
}

/// A sheet button handler.
pub(crate) type Handler = Box<dyn Fn(&mut Window, &mut App)>;

/// The sheet's content: the title, the body, the field (as `field`) and
/// the footer. The gallery draws it inline with a static field.
pub(crate) fn sheet(
    text: &SheetText,
    field: Option<AnyElement>,
    ready: bool,
    id: &str,
    on_cancel: Option<Handler>,
    on_ok: Option<Handler>,
    cx: &App,
) -> AnyElement {
    use crate::ui::native::{self as nk, NativeType as _};
    let n = nk::native_colors(cx);
    // Large capsule buttons at the bottom trailing edge (NATIVE.md §7.1):
    // `取消`, then the default. A destructive verb is a standard button
    // whose title says what it does.
    let ok = nk::push_button(
        SharedString::from(format!("{id}-ok")),
        text.ok.clone(),
        if text.danger {
            nk::PushStyle::Destructive
        } else {
            nk::PushStyle::Default
        },
        nk::PushSize::Large,
        cx,
    );
    let ok = nk::disabled_push(ok, !ready, cx);
    let ok = match on_ok {
        Some(f) if ready => ok.on_click(move |_, window, cx| f(window, cx)),
        _ => ok,
    };
    let cancel = nk::push_button(
        SharedString::from(format!("{id}-cancel")),
        common::CANCEL,
        nk::PushStyle::Standard,
        nk::PushSize::Large,
        cx,
    );
    let cancel = match on_cancel {
        Some(f) => cancel.on_click(move |_, window, cx| f(window, cx)),
        None => cancel,
    };
    let mut layout = nk::SheetLayout::new(text.title.clone());
    if let Some(b) = text.body.clone() {
        layout = layout.explanation(b);
    }
    if let Some((code, verb)) = text.type_to_confirm.clone() {
        layout = layout.body(
            div()
                .flex()
                .flex_col()
                .gap(space::S6)
                .child(div().n_body().text_color(n.label).child(format!(
                    "{} {} {}",
                    c::CONFIRM_TYPE_BEFORE,
                    code,
                    fill(c::CONFIRM_TYPE_AFTER, &[("verb", &verb)])
                )))
                .children(field),
        );
    }
    layout.buttons([cancel, ok]).into_any_element()
}

/// The type-to-confirm field: a standalone 24 pt field (NATIVE.md §5.6),
/// as wide as a name.
pub(crate) fn field_box(child: AnyElement, cx: &App) -> AnyElement {
    let n = crate::ui::native::native_colors(cx);
    let m = crate::ui::native::metrics::FIELD;
    div()
        .w(crate::ui::native::metrics::FIELD_NAME_WIDTH)
        .h(m)
        .px(crate::ui::native::metrics::FIELD_PAD)
        .rounded(crate::ui::native::metrics::FIELD_RADIUS)
        .border_1()
        .border_color(n.separator)
        .bg(n.content)
        .shadow(n.focus_ring())
        .flex()
        .items_center()
        .text_size(px(13.))
        .text_color(n.label)
        .child(child)
        .into_any_element()
}

struct ConfirmForm {
    text: SheetText,
    input: Entity<InputState>,
    answerer: Answerer,
    action: CardV2Action,
    _subscriptions: Vec<Subscription>,
}

impl ConfirmForm {
    fn ready(&self, cx: &App) -> bool {
        matches(
            &self.input.read(cx).value(),
            self.text.type_to_confirm.as_ref().map(|(t, _)| t.as_str()),
        )
    }

    fn confirm(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.ready(cx) {
            return;
        }
        self.answerer.send(&self.action, cx);
        window.close_dialog(cx);
    }
}

impl Render for ConfirmForm {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let k = theme::colors(cx);
        let ready = self.ready(cx);
        let field = self.text.type_to_confirm.is_some().then(|| {
            field_box(
                ui::placeholder_input(
                    Input::new(&self.input)
                        .appearance(false)
                        .with_size(gpui_component::Size::Small)
                        .px_0()
                        .py_0()
                        .text_size(px(13.))
                        .text_color(k.ink),
                    window,
                    cx,
                ),
                cx,
            )
        });
        let this = cx.entity().downgrade();
        sheet(
            &self.text,
            field,
            ready,
            "card-confirm",
            Some(Box::new(|window, cx| window.close_dialog(cx))),
            Some(Box::new(move |window, cx| {
                let _ = this.update(cx, |f, cx| f.confirm(window, cx));
            })),
            cx,
        )
    }
}

/// Opens the confirm sheet for `action`.
pub(crate) fn open(
    answerer: Answerer,
    action: CardV2Action,
    irreversible: Option<AskIrreversible>,
    window: &mut Window,
    cx: &mut App,
) {
    let text = sheet_text(&action, irreversible.as_ref());
    let form = cx.new(|cx| {
        let input = cx.new(|cx| InputState::new(window, cx));
        let subscriptions = vec![cx.subscribe_in(
            &input,
            window,
            |this: &mut ConfirmForm, _, event: &InputEvent, window, cx| match event {
                InputEvent::PressEnter { .. } => this.confirm(window, cx),
                _ => cx.notify(),
            },
        )];
        ConfirmForm {
            text,
            input,
            answerer,
            action,
            _subscriptions: subscriptions,
        }
    });
    let first = Cell::new(true);
    window.open_dialog(cx, move |dialog, window, cx| {
        let dialog =
            crate::ui::native::native_sheet(dialog, crate::ui::native::SheetWidth::Form, cx);
        if first.replace(false) {
            let handle = form.read(cx).input.read(cx).focus_handle(cx);
            handle.focus(window, cx);
        }
        dialog.overflow_hidden().child(form.clone())
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_name_must_match_before_the_primary_unlocks() {
        assert!(!matches("qqbot-pr", Some("qqbot-prod")));
        assert!(matches(" qqbot-prod ", Some("qqbot-prod")));
        assert!(matches("", None));
    }

    #[test]
    fn the_sheet_says_verb_target_and_consequence() {
        let action = CardV2Action {
            role: None,
            id: "ship".into(),
            label: "发布到生产".into(),
            style: Some(CardV2ActionStyle::Primary),
            kind: None,
            target: None,
            confirm: None,
            inputs: None,
        };
        let irr = AskIrreversible {
            verb: "发布".into(),
            target: "qqbot v2.14 到生产".into(),
            consequence: "所有群会重启约 2 分钟。".into(),
            type_to_confirm: Some("qqbot-prod".into()),
        };
        let t = sheet_text(&action, Some(&irr));
        assert_eq!(t.title, "发布 qqbot v2.14 到生产");
        assert_eq!(
            t.type_to_confirm,
            Some(("qqbot-prod".into(), "发布".into()))
        );
        assert_eq!(t.ok, "发布到生产");
        let plain = sheet_text(&action, None);
        assert_eq!(plain.title, "发布到生产");
        assert!(plain.type_to_confirm.is_none());
    }
}
