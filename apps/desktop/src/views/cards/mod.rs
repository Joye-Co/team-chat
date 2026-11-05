//! Cards in the timeline (docs/protocol.md "Cards").
//!
//! [`CardExtra`] is a timeline extra: for a card message it draws the card
//! natively below the row header and hides the plain-text fallback body. The
//! card, its presses (`card.actioned`) and my unanswered or refused press come
//! from the provider on every frame (the provider keeps them in memory).
//! Buttons call [`ChatProvider::press_card_action`], after a confirm dialog
//! when the action has `confirm`. A card edit arrives as a new card for the
//! same message and is drawn in place.
//!
//! Files: `blocks.rs` (one renderer per block type), `actions.rs` (buttons,
//! confirm, press states and the "who pressed what" lines), `selftest.rs`
//! (`--selftest-cards`), `samples.rs` (sample cards for the fake provider).

mod actions;
mod blocks;
pub mod samples;
pub mod selftest;
// --- cards v2 track ---
pub mod v2;
// --- inc-cards21: Card v2.1 ---
pub mod v21;
// --- end inc-cards21 ---
// --- end cards v2 track ---

use std::{
    cell::RefCell,
    collections::{HashMap, HashSet},
    sync::Arc,
};

use gpui::{
    AnyElement, App, Global, IntoElement, ParentElement, Styled, Window, div,
    prelude::FluentBuilder as _, px,
};

use crate::{
    copy::cards as c,
    data::{
        Message,
        card::{Card, CardBlock, CardTone},
    },
    theme::{self, space},
    ui::{self, native::NativeType as _},
    views::timeline::extras::{ExtrasContext, MessageExtra},
};

/// The timeline extra that draws card messages.
pub struct CardExtra;

impl MessageExtra for CardExtra {
    /// The body shows above the card (`草稿好了…` over the draft card), except
    /// when it is only the fallback the server makes from the card title.
    fn hides_body(&self, message: &Message, ctx: &ExtrasContext) -> bool {
        if message.id.is_empty() {
            return false;
        }
        // A v2 card shows instead of its body when the body is only the
        // fallback or the title.
        if let Some(v2) = ctx.provider.card_v2(&message.group_id, &message.id) {
            let body = message.body.trim();
            return body.is_empty()
                || body == v2.card.title.trim()
                || body == v2.card.fallback.trim();
        }
        ctx.provider
            .card(&message.group_id, &message.id)
            .is_some_and(|card| {
                let body = message.body.trim();
                body.is_empty() || body == card.title.trim()
            })
    }

    fn render(
        &self,
        message: &Message,
        ctx: &ExtrasContext,
        window: &mut Window,
        cx: &mut App,
    ) -> Option<AnyElement> {
        if message.id.is_empty() {
            return None;
        }
        // Card v2 (1.1.0): the generative card replaces the derived v1 card.
        if v2::has_card(ctx.provider, &message.group_id, &message.id) {
            let env = v2::CardEnv {
                viewer: ctx.me.map(str::to_string),
                ..v2::CardEnv::new(ctx.provider.clone())
            };
            let card = v2::render(
                &env,
                &message.group_id,
                &message.id,
                v2::CardMode::Topic,
                window,
                cx,
            )?;
            return Some(
                div()
                    .mt(space::S6)
                    .w_full()
                    .max_w(crate::ui::native::tokens::layout::IN_THREAD)
                    .child(card)
                    .into_any_element(),
            );
        }
        let card = ctx.provider.card(&message.group_id, &message.id)?;
        Some(render_card(&card, message, ctx, window, cx))
    }
}

/// Which table and list blocks the user expanded, by element key. Only the
/// UI keeps it; it is lost on restart.
#[derive(Default)]
pub(crate) struct Expanded(HashSet<String>);

impl Global for Expanded {}

pub(crate) fn is_expanded(key: &str, cx: &App) -> bool {
    cx.try_global::<Expanded>()
        .is_some_and(|e| e.0.contains(key))
}

pub(crate) fn toggle_expanded(key: String, window: &mut Window, cx: &mut App) {
    let set = &mut cx.default_global::<Expanded>().0;
    if !set.remove(&key) {
        set.insert(key);
    }
    window.refresh();
}

/// The quiet label of a card status (§7.8): a glyph and the words, no fill.
/// A card that waits for the viewer shows no label: its 3 pt `you-bar`
/// already says it, so amber appears once per card (§7.10).
/// A card that waits for the viewer shows no label: its 3 pt `you-bar`
/// already says it, so amber appears once per card (§7.10).
fn status_label(tone: CardTone, label: impl Into<String>) -> ui::StatusChip {
    use crate::{data::TopicStatus as S, theme::ChipKind as K};
    let (status, kind) = match tone {
        CardTone::Neutral => (S::Open, K::Quiet),
        CardTone::Info => (S::InProgress, K::Quiet),
        CardTone::Success => (S::Done, K::Quiet),
        CardTone::Warning => (S::WaitingAnswer, K::You),
        CardTone::Danger => (S::Blocked, K::Blocked),
    };
    ui::StatusChip::labeled(label, status, kind).style(ui::ChipStyle::Label)
}

/// A card that waits for the viewer: the `warning` tone (§2.5 `you-bar`).
pub(crate) fn waits_for_viewer(card: &Card) -> bool {
    card.tone == Some(CardTone::Warning)
}

/// An ask card (§7.10, GrokBot): it waits for the viewer and its buttons are
/// plain answers (two or more, none primary, danger or confirmed). They draw
/// as lettered option rows instead of buttons.
pub(crate) fn is_ask(card: &Card) -> bool {
    let actions = card.actions.as_deref().unwrap_or(&[]);
    waits_for_viewer(card)
        && (2..=6).contains(&actions.len())
        && actions.iter().all(|a| {
            a.style.unwrap_or_default() == crate::data::card::ActionStyle::Default
                && a.confirm.is_none()
        })
}

// --- inc-refs ---
/// 就这张卡提问: the whole card (`blockId` `card`, no part).
fn card_target(card: &Arc<Card>, message: &Message) -> Option<crate::views::refs::PartTarget> {
    (!message.id.is_empty()).then(|| {
        crate::views::refs::parts::whole(&card.title).target(
            crate::views::refs::RefSource::Message {
                group_id: message.group_id.clone(),
                topic_id: message.topic_id.clone(),
                message_id: message.id.clone(),
            },
            &card.title,
            "card",
            None,
            None,
        )
    })
}
// --- end inc-refs ---

/// A stable element id inside a card: message id plus a part name.
pub(crate) fn part_id(message: &Message, part: impl std::fmt::Display) -> String {
    format!("card-{}-{part}", message.id)
}

/// Draws one card (§7.10): one 1 pt border, radius 12, padding 14 16 16; the
/// title at 15/22 600 with the status label at the right; blocks under it; the
/// actions 14 pt below. A card that waits for the viewer gets the 3 pt
/// `you-bar` on its left edge, inset 12 pt from the top and bottom.
pub fn render_card(
    card: &Arc<Card>,
    message: &Message,
    ctx: &ExtrasContext,
    window: &mut Window,
    cx: &mut App,
) -> AnyElement {
    let k = theme::colors(cx);
    let tone = card.tone.unwrap_or_default();

    // One header slot for every card (§2.4, C-08): the StatusBadge of §2.12 over the
    // title, as in Card v2; a card with a tone and no status word takes the tone's word.
    let n = ui::native::native_colors(cx);
    let state = {
        use ui::native::BadgeState as B;
        let (badge, word) = match tone {
            CardTone::Neutral => (B::Queued, None),
            CardTone::Info => (B::Working, Some(c::STATE_WORKING)),
            CardTone::Success => (B::Done, Some(c::STATE_DONE)),
            CardTone::Warning => (B::NeedsYou, Some(c::STATE_NEEDS_YOU)),
            CardTone::Danger => (B::Failed, Some(c::STATE_FAILED)),
        };
        card.status
            .as_ref()
            .map(|s| s.label.clone())
            .or_else(|| word.map(str::to_string))
            .map(|label| {
                div()
                    .flex()
                    .mb(space::S8)
                    .child(ui::native::StatusBadge::new(badge, label))
            })
    };
    // A title is one line of text: line breaks and runs of spaces collapse,
    // it wraps to at most 2 lines, and an empty title takes no room.
    let title = card.title.split_whitespace().collect::<Vec<_>>().join(" ");
    let header = div()
        .flex()
        .flex_col()
        .children(state)
        .when(!title.is_empty(), |el| {
            el.child(
                div()
                    .min_w(px(0.))
                    // A compact card's title: 14 semibold (§2.4).
                    .n_headline()
                    .font_weight(gpui::FontWeight::SEMIBOLD)
                    .text_color(n.label)
                    .line_clamp(2)
                    .child(title),
            )
        });

    let shown: Vec<(usize, &CardBlock)> = card
        .blocks
        .iter()
        .enumerate()
        .filter(|(_, b)| !matches!(b, CardBlock::Unknown))
        .collect();
    let mut body = div().flex().flex_col();
    for (n, (ix, block)) in shown.iter().enumerate() {
        // The first sentence sits 4 pt under the title in `text-2`; later
        // blocks are 12 pt apart.
        let lead = n == 0 && matches!(block, CardBlock::Text { .. });
        let el = blocks::render(block, *ix, message, ctx, window, cx);
        body = body.child(
            div()
                .mt(if n == 0 {
                    if lead { space::S4 } else { space::S12 }
                } else {
                    space::S12
                })
                .when(lead, |el| el.text_color(k.text_2))
                .child(el),
        );
    }
    record_render(message, card);

    let actions = actions::render(card, message, ctx, window, cx);

    ui::native::cards::card_frame(cx)
        .mt(space::S6)
        .max_w(crate::ui::native::tokens::layout::IN_THREAD)
        .child(
            div()
                .p(ui::native::cards::card::INSET)
                .min_w(px(0.))
                .flex()
                .flex_col()
                .child(crate::views::refs::affordance::whole_card(
                    header,
                    card_target(card, message),
                    cx,
                ))
                .child(body)
                .children(actions),
        )
        .into_any_element()
}

/// What the cards drew, for `--selftest-cards`. Only the main thread writes
/// it, and only while a self-test turned it on.
#[derive(Clone, Debug, Default)]
pub(crate) struct RenderStats {
    /// Card renders in total.
    pub renders: u64,
    /// Block type names drawn at least once.
    pub block_types: HashSet<&'static str>,
    /// Per message id: what its last render showed.
    pub cards: HashMap<String, CardDrawn>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct CardDrawn {
    pub title: String,
    pub status: Option<String>,
    pub buttons: usize,
    pub press_lines: usize,
    pub sending: bool,
    pub failed: bool,
}

thread_local! {
    pub(crate) static STATS: RefCell<Option<RenderStats>> = const { RefCell::new(None) };
}

pub(crate) fn record(message: &Message, f: impl FnOnce(&mut CardDrawn)) {
    STATS.with(|s| {
        if let Some(stats) = s.borrow_mut().as_mut() {
            f(stats.cards.entry(message.id.clone()).or_default());
        }
    });
}

fn record_render(message: &Message, card: &Card) {
    STATS.with(|s| {
        if let Some(stats) = s.borrow_mut().as_mut() {
            stats.renders += 1;
            for b in &card.blocks {
                stats.block_types.insert(blocks::type_name(b));
            }
            stats.cards.insert(
                message.id.clone(),
                CardDrawn {
                    title: card.title.clone(),
                    status: card.status.as_ref().map(|s| s.label.clone()),
                    ..Default::default()
                },
            );
        }
    });
}
