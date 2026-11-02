//! Card buttons, the confirm dialog, my press state, and the lines that say
//! who pressed what.
//!
//! While my press of a card is unanswered, its button shows a spinner and the
//! card's other buttons are disabled (the server allows one press per member
//! per message per second). A refused press shows one line in the danger
//! color; pressing again clears it.

use std::sync::Arc;

use gpui::{
    AnyElement, App, ElementId, IntoElement, ParentElement, Styled, Window, div,
    prelude::FluentBuilder as _, px,
};
use gpui_component::{
    Disableable as _, Sizable as _, WindowExt as _,
    button::{Button, ButtonVariant, ButtonVariants as _},
};

use super::part_id;
use crate::{
    copy::{cards as c, common, fill},
    data::{
        CardPress, CardPressError, CardPressPhase, ChatProvider, Message,
        card::{ActionStyle, Card, CardAction},
    },
    theme,
    views::{time_text, timeline::extras::ExtrasContext},
};

/// Press lines shown under a card; older presses are summed up in one line.
pub const SHOWN_PRESSES: usize = 3;

/// The buttons, the press state line and the press lines, or nothing.
pub fn render(
    card: &Card,
    message: &Message,
    ctx: &ExtrasContext,
    _window: &mut Window,
    cx: &mut App,
) -> Vec<AnyElement> {
    let colors = theme::colors(cx);
    let provider = ctx.provider;
    let actions = card.actions.as_deref().unwrap_or(&[]);
    let state = provider.card_press_state(&message.group_id, &message.id);
    let sending = state
        .as_ref()
        .filter(|s| s.phase == CardPressPhase::Sending)
        .map(|s| s.action_id.as_str());
    let mut out = Vec::new();

    if !actions.is_empty() {
        out.push(
            div()
                .flex()
                .flex_wrap()
                .gap(px(8.))
                .children(
                    actions
                        .iter()
                        .map(|action| button(action, message, provider, sending, cx)),
                )
                .into_any_element(),
        );
    }

    if let Some(state) = &state
        && let CardPressPhase::Failed(error) = state.phase
    {
        let label = action_label(card, &state.action_id, None);
        let template = match error {
            CardPressError::Gone => c::PRESS_GONE,
            CardPressError::Forbidden => c::PRESS_FORBIDDEN,
            CardPressError::Other => c::PRESS_FAILED,
        };
        out.push(
            div()
                .text_xs()
                .text_color(colors.danger)
                .child(fill(template, &[("action", &label)]))
                .into_any_element(),
        );
    }

    let presses = provider.card_presses(&message.group_id, &message.id);
    if !presses.is_empty() {
        out.push(press_lines(card, &presses, ctx, cx));
    }
    super::record(message, |d| {
        d.buttons = actions.len();
        d.press_lines = presses.len().min(SHOWN_PRESSES);
        d.sending = sending.is_some();
        d.failed = matches!(
            state.as_ref().map(|s| s.phase),
            Some(CardPressPhase::Failed(_))
        );
    });
    out
}

fn button(
    action: &CardAction,
    message: &Message,
    provider: &Arc<dyn ChatProvider>,
    sending: Option<&str>,
    _cx: &App,
) -> Button {
    let id = ElementId::Name(part_id(message, format_args!("action-{}", action.id)).into());
    let variant = match action.style.unwrap_or_default() {
        ActionStyle::Primary => ButtonVariant::Primary,
        ActionStyle::Default => ButtonVariant::Secondary,
        ActionStyle::Danger => ButtonVariant::Danger,
    };
    let loading = sending == Some(action.id.as_str());
    let press = Press {
        provider: provider.clone(),
        group_id: message.group_id.clone(),
        topic_id: message.topic_id.clone(),
        message_id: message.id.clone(),
        action_id: action.id.clone(),
        label: action.label.clone(),
        confirm: action.confirm.clone(),
        danger: action.style == Some(ActionStyle::Danger),
    };
    Button::new(id)
        .small()
        .with_variant(variant)
        .label(action.label.clone())
        .loading(loading)
        .disabled(sending.is_some() && !loading)
        .on_click(move |_, window, cx| press.run(window, cx))
}

/// Everything a button needs to send its action.
#[derive(Clone)]
struct Press {
    provider: Arc<dyn ChatProvider>,
    group_id: String,
    topic_id: String,
    message_id: String,
    action_id: String,
    label: String,
    confirm: Option<String>,
    danger: bool,
}

impl Press {
    fn send(&self) {
        self.provider.press_card_action(
            &self.group_id,
            &self.topic_id,
            &self.message_id,
            &self.action_id,
        );
    }

    /// Sends at once, or after the user agrees to the action's question.
    fn run(&self, window: &mut Window, cx: &mut App) {
        let Some(question) = self.confirm.clone() else {
            self.send();
            return;
        };
        if window.has_active_dialog(cx) {
            return;
        }
        let press = self.clone();
        window.open_alert_dialog(cx, move |dialog, _, _| {
            let press = press.clone();
            dialog
                .title(question.clone())
                .show_cancel(true)
                .ok_text(press.label.clone())
                .ok_variant(if press.danger {
                    ButtonVariant::Danger
                } else {
                    ButtonVariant::Primary
                })
                .cancel_text(common::CANCEL)
                .on_ok(move |_, _, _| {
                    press.send();
                    true
                })
        });
    }
}

/// The label of an action: from the press, else from the current card, else
/// `None`.
fn known_label(card: &Card, action_id: &str, press_label: Option<&str>) -> Option<String> {
    press_label.map(str::to_string).or_else(|| {
        card.actions
            .as_deref()
            .unwrap_or(&[])
            .iter()
            .find(|a| a.id == action_id)
            .map(|a| a.label.clone())
    })
}

fn action_label(card: &Card, action_id: &str, press_label: Option<&str>) -> String {
    known_label(card, action_id, press_label).unwrap_or_else(|| c::PRESS_REMOVED_ACTION.to_string())
}

/// "张三 点了「批准」 · 3 分钟前" for the newest presses, oldest first.
fn press_lines(card: &Card, presses: &[CardPress], ctx: &ExtrasContext, cx: &App) -> AnyElement {
    let colors = theme::colors(cx);
    let earlier = presses.len().saturating_sub(SHOWN_PRESSES);
    div()
        .flex()
        .flex_col()
        .gap(px(2.))
        .text_xs()
        .text_color(colors.muted_foreground)
        .when(earlier > 0, |el| {
            el.child(fill(c::PRESSED_EARLIER, &[("n", &earlier.to_string())]))
        })
        .children(presses[earlier..].iter().map(|p| {
            let name = ctx
                .member(&p.actor_id)
                .map(|m| m.name.clone())
                .unwrap_or_else(|| c::SOMEONE.to_string());
            let label = action_label(card, &p.action_id, p.label.as_deref());
            div().child(fill(
                c::PRESSED,
                &[
                    ("name", &name),
                    ("action", &label),
                    ("time", &time_text(p.at)),
                ],
            ))
        }))
        .into_any_element()
}
