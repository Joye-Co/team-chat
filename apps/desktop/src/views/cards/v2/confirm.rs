//! The confirm sheet before an irreversible answer (DESIGN.md §7.10
//! "Approval"): the verb and target as the title, the consequence, and with
//! `typeToConfirm` a field `输入 qqbot-prod 确认发布`; the primary stays
//! disabled until the name matches. Esc and 取消 close it without sending.

use std::cell::Cell;

use gpui::{
    AnyElement, App, AppContext as _, Context, Entity, Focusable as _, FontWeight, IntoElement,
    ParentElement, Render, SharedString, Styled, Subscription, Window, div,
    prelude::FluentBuilder as _, px,
};
use gpui_component::{
    Sizable as _, WindowExt as _,
    input::{Input, InputEvent, InputState},
};

use super::{Answerer, button, disabled_button};
use crate::{
    copy::{cards_v2 as c, common, fill},
    data::cards_v2::{AskIrreversible, CardV2Action, CardV2ActionStyle},
    theme::{self, Type as _, space},
    ui::{self, ButtonKind},
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
    let k = theme::colors(cx);
    let ok_id = SharedString::from(format!("{id}-ok"));
    let ok = if ready {
        let b = button(
            ok_id,
            text.ok.clone(),
            if text.danger {
                ButtonKind::Danger
            } else {
                ButtonKind::Primary
            },
            cx,
        );
        match on_ok {
            Some(f) => b
                .on_click(move |_, window, cx| f(window, cx))
                .into_any_element(),
            None => b.into_any_element(),
        }
    } else {
        disabled_button(ok_id, text.ok.clone(), cx).into_any_element()
    };
    let cancel = button(
        SharedString::from(format!("{id}-cancel")),
        common::CANCEL,
        ButtonKind::Secondary,
        cx,
    );
    let cancel = match on_cancel {
        Some(f) => cancel
            .on_click(move |_, window, cx| f(window, cx))
            .into_any_element(),
        None => cancel.into_any_element(),
    };
    div()
        .w_full()
        .flex()
        .flex_col()
        .child(
            div()
                .pt(space::S20)
                .px(space::S20)
                .pb(space::S16)
                .flex()
                .flex_col()
                .child(
                    div()
                        .text_size(px(16.))
                        .line_height(px(24.))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(k.ink)
                        .child(text.title.clone()),
                )
                .when_some(text.body.clone(), |el, b| {
                    el.child(
                        div()
                            .mt(space::S6)
                            .text_size(px(13.))
                            .line_height(px(20.))
                            .text_color(k.ink_2)
                            .child(b),
                    )
                })
                .when_some(text.type_to_confirm.clone(), |el, (code, verb)| {
                    el.child(
                        div()
                            .mt(space::S16)
                            .flex()
                            .items_center()
                            .gap(space::S4)
                            .text_size(px(13.))
                            .line_height(px(20.))
                            .text_color(k.ink)
                            .child(c::CONFIRM_TYPE_BEFORE)
                            .child(
                                div()
                                    .px(px(5.))
                                    .rounded(px(5.))
                                    .bg(k.sunken)
                                    .text_size(px(12.))
                                    .mono()
                                    .child(code),
                            )
                            .child(fill(c::CONFIRM_TYPE_AFTER, &[("verb", &verb)])),
                    )
                    .children(field.map(|f| div().mt(space::S8).child(f)))
                }),
        )
        .child(
            div()
                .flex()
                .justify_end()
                .gap(space::S8)
                .px(space::S20)
                .py(space::S12)
                .border_t_1()
                .border_color(k.line)
                .bg(k.raised)
                .child(cancel)
                .child(ok),
        )
        .into_any_element()
}

/// The field box of the sheet: 36 pt, mono, `ink-3` edge.
pub(crate) fn field_box(child: AnyElement, cx: &App) -> AnyElement {
    let k = theme::colors(cx);
    div()
        .h(px(36.))
        .px(space::S10)
        .rounded(theme::radius::MD)
        .border_1()
        .border_color(k.ink_3)
        .bg(k.bg)
        .flex()
        .items_center()
        .text_size(px(13.))
        .mono()
        .text_color(k.ink)
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
        let dialog = crate::ui::SheetExt::sheet(dialog, cx);
        if first.replace(false) {
            let handle = form.read(cx).input.read(cx).focus_handle(cx);
            handle.focus(window, cx);
        }
        dialog
            .close_button(false)
            .p_0()
            .rounded(px(16.))
            .overflow_hidden()
            .child(form.clone())
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
