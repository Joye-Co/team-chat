//! Card buttons, the confirm dialog, my press state, and the lines that say
//! who pressed what.
//!
//! While my press of a card is unanswered, its button shows a spinner and the
//! card's other buttons are disabled (the server allows one press per member
//! per message per second). A refused press shows one line in the danger
//! color; pressing again clears it.

use std::sync::Arc;

use gpui::{
    AnyElement, App, ElementId, IntoElement, ParentElement,
    Styled, Window, div, prelude::FluentBuilder as _,
};
use gpui_component::{Disableable as _, WindowExt as _, button::ButtonVariant};

use super::part_id;
use crate::{
    copy::{cards as c, common, fill},
    data::{
        CardPress, CardPressError, CardPressPhase, ChatProvider, Message,
        card::{ActionStyle, Card, CardAction},
    },
    theme::{self, Type as _, space},
    ui,
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

    let mine = ctx.me.and_then(|me| {
        provider
            .card_presses(&message.group_id, &message.id)
            .into_iter()
            .rev()
            .find(|p| p.actor_id == me)
            .map(|p| p.action_id)
    });
    if super::is_ask(card) {
        out.push(
            div()
                .mt(space::S12)
                .flex()
                .flex_col()
                .gap(space::S6)
                .children(actions.iter().enumerate().map(|(ix, action)| {
                    option_row(ix, action, message, provider, sending, mine.as_deref(), cx)
                }))
                .into_any_element(),
        );
    } else if !actions.is_empty() {
        out.push(
            div()
                .mt(space::S14)
                .flex()
                .flex_wrap()
                .gap(space::S8)
                .children({
                    // §7.10: one primary; after it one secondary, then ghost
                    // buttons (发布 · 修改 · 放弃).
                    let has_primary = actions
                        .iter()
                        .any(|a| a.style == Some(ActionStyle::Primary));
                    let mut plain = 0;
                    let cx: &App = cx;
                    actions.iter().map(move |action| {
                        let quiet = has_primary
                            && action.style.unwrap_or_default() == ActionStyle::Default
                            && {
                                plain += 1;
                                plain > 1
                            };
                        button(action, message, provider, sending, quiet, cx)
                    })
                })
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
                .mt(space::S8)
                .t_meta()
                .text_color(colors.danger_text)
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

fn press_of(action: &CardAction, message: &Message, provider: &Arc<dyn ChatProvider>) -> Press {
    Press {
        provider: provider.clone(),
        group_id: message.group_id.clone(),
        topic_id: message.topic_id.clone(),
        message_id: message.id.clone(),
        action_id: action.id.clone(),
        label: action.label.clone(),
        confirm: action.confirm.clone(),
        danger: action.style == Some(ActionStyle::Danger),
    }
}

/// A card button: `md`, 32 pt (§7.10). Primary, secondary or danger by the
/// action's style. The other buttons are disabled at once; the spinner shows only when the answer takes longer
/// than 400 ms (§12).
fn button(
    action: &CardAction,
    message: &Message,
    provider: &Arc<dyn ChatProvider>,
    sending: Option<&str>,
    quiet: bool,
    cx: &App,
) -> AnyElement {
    let id = ElementId::Name(part_id(message, format_args!("action-{}", action.id)).into());
    let kind = match action.style.unwrap_or_default() {
        ActionStyle::Primary => ui::ButtonKind::Primary,
        // Every other action is a standard push button (NATIVE.md §5.2).
        ActionStyle::Default => {
            let _ = quiet;
            ui::ButtonKind::Quiet
        }
        ActionStyle::Danger => ui::ButtonKind::Danger,
    };
    let pressed = sending == Some(action.id.as_str());
    let make = |loading: bool| {
        let press = press_of(action, message, provider);
        ui::button(
            id.clone(),
            action.label.clone(),
            kind,
            ui::ButtonSize::Card,
            cx,
        )
        .loading(loading)
        .disabled(sending.is_some() && !pressed)
        // A second press while the first one is in flight does nothing.
        .when(!pressed, |b| {
            b.on_click(move |_, window, cx| press.run(window, cx))
        })
    };
    let (idle, busy) = (make(false), make(true));
    let gate = ElementId::Name(part_id(message, format_args!("busy-{}", action.id)).into());
    ui::motion::busy_gate(gate, pressed, move |shown| {
        if shown { busy } else { idle }.into_any_element()
    })
    .into_any_element()
}

/// One answer of an ask card (§7.10): a regular push button with the key
/// letter in `secondary` before the answer, stacked one under another; the
/// chosen answer is the default style (NATIVE.md §5.2). A press answers.
fn option_row(
    ix: usize,
    action: &CardAction,
    message: &Message,
    provider: &Arc<dyn ChatProvider>,
    sending: Option<&str>,
    mine: Option<&str>,
    cx: &App,
) -> AnyElement {
    use crate::ui::native as nk;
    let n = nk::native_colors(cx);
    let id = ElementId::Name(part_id(message, format_args!("option-{}", action.id)).into());
    let chosen = mine == Some(action.id.as_str()) || sending == Some(action.id.as_str());
    let blocked = sending.is_some();
    let letter = char::from(b'A' + ix as u8).to_string();
    let press = press_of(action, message, provider);
    let b = nk::push_button(
        id,
        "",
        if chosen {
            nk::PushStyle::Default
        } else {
            nk::PushStyle::Standard
        },
        nk::PushSize::Regular,
        cx,
    )
    .child(
        div()
            .flex()
            .items_center()
            .gap(space::S6)
            .child(
                div()
                    .text_color(if chosen { n.on_accent } else { n.secondary })
                    .child(letter),
            )
            .child(action.label.clone()),
    );
    let b = nk::disabled_push(b, blocked && !chosen, cx);
    div()
        .flex()
        .child(if blocked {
            b
        } else {
            b.on_click(move |_, window, cx| press.run(window, cx))
        })
        .into_any_element()
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
        window.open_alert_dialog(cx, move |dialog, _, cx| {
            let dialog = crate::ui::SheetExt::sheet(dialog, cx);
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
        .mt(space::S12)
        .flex()
        .flex_col()
        .gap(space::S2)
        .t_meta()
        .text_color(colors.text_3)
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
