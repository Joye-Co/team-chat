//! Card buttons, the confirm dialog, my press state, and the lines that say
//! who pressed what.
//!
//! While my press of a card is unanswered, its button shows a spinner and the
//! card's other buttons are disabled (the server allows one press per member
//! per message per second). A refused press shows one line in the danger
//! color; pressing again clears it.

use std::sync::Arc;

use gpui::{
    AnyElement, App, ElementId, Hsla, IntoElement, ParentElement, Styled, Window, div,
    prelude::FluentBuilder as _, px,
};
use gpui_component::{
    ActiveTheme as _, Disableable as _, Sizable as _, WindowExt as _,
    button::{Button, ButtonCustomVariant, ButtonVariant, ButtonVariants as _},
    menu::{DropdownMenu as _, PopupMenuItem},
};

use super::part_id;
use crate::{
    copy::{cards as c, common, fill},
    data::{
        CardPress, CardPressError, CardPressPhase, ChatProvider, Message,
        card::{ActionStyle, Card, CardAction},
    },
    theme::{self, Elevation, Type as _, space},
    ui::{self, Lucide},
    views::{time_text, timeline::extras::ExtrasContext},
};

/// Press lines shown under a card; older presses are summed up in one line.
pub const SHOWN_PRESSES: usize = 3;
/// Buttons shown in a row; the rest go into an `ellipsis` menu (§12).
pub const SHOWN_ACTIONS: usize = 3;

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
        let (shown, more) = actions.split_at(actions.len().min(SHOWN_ACTIONS));
        out.push(
            div()
                .flex()
                .flex_wrap()
                .items_center()
                .gap(space::S8)
                .children(
                    shown
                        .iter()
                        .map(|action| button(action, message, provider, sending, cx)),
                )
                .when(!more.is_empty(), |el| {
                    el.child(more_menu(more, message, provider, sending))
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
            result_line(
                Lucide::CircleAlert,
                colors.danger,
                colors.danger_text,
                fill(template, &[("action", &label)]),
            )
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
    cx: &App,
) -> Button {
    let id = ElementId::Name(part_id(message, format_args!("action-{}", action.id)).into());
    // A destructive action is a secondary button with `danger-text`; the
    // filled red is kept for the confirm alert (§13.13).
    let k = theme::colors(cx);
    let variant = match action.style.unwrap_or_default() {
        ActionStyle::Primary => ButtonVariant::Primary,
        ActionStyle::Default => ButtonVariant::Secondary,
        ActionStyle::Danger => ButtonVariant::Custom(
            ButtonCustomVariant::new(cx)
                .color(k.control)
                .foreground(k.danger_text)
                .hover(cx.theme().secondary_hover)
                .active(cx.theme().secondary_active)
        ),
    };
    let loading = sending == Some(action.id.as_str());
    let press = Press::new(action, message, provider);
    let primary = matches!(variant, ButtonVariant::Primary);
    Button::new(id)
        .small()
        .with_variant(variant)
        // Secondary buttons carry the control ring and shadow (§7).
        .when(!primary, |b| b.shadow(k.shadow(Elevation::Control)))
        .label(action.label.clone())
        .loading(loading)
        .disabled(sending.is_some() && !loading)
        .on_click(move |_, window, cx| press.run(window, cx))
}

/// The `ellipsis` button with the actions after the third.
fn more_menu(
    more: &[CardAction],
    message: &Message,
    provider: &Arc<dyn ChatProvider>,
    sending: Option<&str>,
) -> AnyElement {
    let presses: Vec<Press> = more
        .iter()
        .map(|a| Press::new(a, message, provider))
        .collect();
    let busy = sending.is_some();
    Button::new(ElementId::Name(part_id(message, "actions-more").into()))
        .small()
        .ghost()
        .icon(Lucide::Ellipsis)
        .tooltip(c::MORE_ACTIONS)
        .disabled(busy)
        .dropdown_menu(move |mut menu, _, _| {
            for press in &presses {
                let press = press.clone();
                menu = menu.item(
                    PopupMenuItem::new(press.label.clone())
                        .on_click(move |_, window, cx| press.run(window, cx)),
                );
            }
            menu
        })
        .into_any_element()
}

impl Press {
    fn new(action: &CardAction, message: &Message, provider: &Arc<dyn ChatProvider>) -> Self {
        Self {
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
            let dialog = crate::ui::SheetExt::sheet(dialog);
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

/// A 12 pt line with a 13 pt glyph: a press result or a refused press.
fn result_line(glyph: Lucide, glyph_color: Hsla, text_color: Hsla, text: String) -> gpui::Div {
    div()
        .flex()
        .items_center()
        .gap(space::S6)
        .t_callout()
        .text_color(text_color)
        .child(ui::icon(glyph, theme::icon::SMALL, glyph_color).flex_none())
        .child(div().flex_1().min_w(px(0.)).one_line().child(text))
}

/// "张三 点了「批准」 · 3 分钟前" for the newest presses, oldest first, each
/// with a `circle-check` (§13.9).
fn press_lines(card: &Card, presses: &[CardPress], ctx: &ExtrasContext, cx: &App) -> AnyElement {
    let colors = theme::colors(cx);
    let earlier = presses.len().saturating_sub(SHOWN_PRESSES);
    div()
        .flex()
        .flex_col()
        .gap(space::S4)
        .when(earlier > 0, |el| {
            el.child(
                div()
                    .pl(px(19.))
                    .t_callout()
                    .text_color(colors.text_3)
                    .child(fill(c::PRESSED_EARLIER, &[("n", &earlier.to_string())])),
            )
        })
        .children(presses[earlier..].iter().map(|p| {
            let name = ctx
                .member(&p.actor_id)
                .map(|m| m.name.clone())
                .unwrap_or_else(|| c::SOMEONE.to_string());
            let label = action_label(card, &p.action_id, p.label.as_deref());
            result_line(
                Lucide::CircleCheck,
                colors.success,
                colors.text_2,
                fill(
                    c::PRESSED,
                    &[
                        ("name", &name),
                        ("action", &label),
                        ("time", &time_text(p.at)),
                    ],
                ),
            )
        }))
        .into_any_element()
}
