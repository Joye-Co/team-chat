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

use std::{
    cell::RefCell,
    collections::{HashMap, HashSet},
    sync::Arc,
};

use gpui::{
    AnyElement, App, FontWeight, Global, Hsla, IntoElement, ParentElement, Styled, Window, div,
    px,
};
use gpui_component::ActiveTheme as _;

use crate::{
    data::{
        Message,
        card::{Card, CardBlock, CardTone},
    },
    theme::{self, Type as _, space},
    ui::{self, Lucide},
    views::timeline::extras::{ExtrasContext, MessageExtra},
};

/// Widest a card grows in the timeline.
pub const CARD_MAX_WIDTH: f32 = 460.;

/// The timeline extra that draws card messages.
pub struct CardExtra;

impl MessageExtra for CardExtra {
    fn hides_body(&self, message: &Message, ctx: &ExtrasContext) -> bool {
        !message.id.is_empty() && ctx.provider.card(&message.group_id, &message.id).is_some()
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

/// The accent color of a card tone, from the theme.
pub(crate) fn tone_color(tone: CardTone, cx: &App) -> Option<Hsla> {
    let t = cx.theme();
    match tone {
        CardTone::Neutral => None,
        CardTone::Info => Some(t.blue),
        CardTone::Success => Some(t.green),
        CardTone::Warning => Some(t.yellow),
        CardTone::Danger => Some(t.red),
    }
}

/// The chip of a card status: three weights only (§2.5). A warning is
/// something that waits on the viewer.
fn status_chip(tone: CardTone, label: impl Into<String>) -> ui::StatusChip {
    use crate::{data::TopicStatus as S, theme::ChipKind as K};
    let (status, kind) = match tone {
        CardTone::Neutral => (S::Open, K::Quiet),
        CardTone::Info => (S::InProgress, K::Quiet),
        CardTone::Success => (S::Done, K::Quiet),
        CardTone::Warning => (S::WaitingAnswer, K::You),
        CardTone::Danger => (S::Blocked, K::Blocked),
    };
    ui::StatusChip::labeled(label, status, kind)
}

/// A stable element id inside a card: message id plus a part name.
pub(crate) fn part_id(message: &Message, part: impl std::fmt::Display) -> String {
    format!("card-{}-{part}", message.id)
}

/// Draws one card: tone stripe, title and status, blocks, buttons, presses.
pub fn render_card(
    card: &Arc<Card>,
    message: &Message,
    ctx: &ExtrasContext,
    window: &mut Window,
    cx: &mut App,
) -> AnyElement {
    let colors = theme::colors(cx);
    let tone = card.tone.unwrap_or_default();
    let stripe = tone_color(tone, cx);
    let radius = cx.theme().radius_lg;

    // §13.9: a status glyph, the title (one line) and the chip at the end.
    let k = theme::colors(cx);
    let glyph = match tone {
        CardTone::Danger => Some((Lucide::CircleAlert, k.danger)),
        CardTone::Success => Some((Lucide::CircleCheck, k.success)),
        CardTone::Warning => Some((Lucide::CircleQuestionMark, k.glyph_wait)),
        CardTone::Info => Some((Lucide::CircleDot, k.glyph_progress)),
        CardTone::Neutral => None,
    };
    let header = div()
        .flex()
        .items_center()
        .gap(space::S8)
        .children(glyph.map(|(g, c)| ui::icon(g, theme::icon::TOOLBAR, c)))
        .child(
            div()
                .flex_1()
                .min_w(px(0.))
                .t_ui()
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(colors.text_1)
                .one_line()
                .child(card.title.clone()),
        )
        .children(card.status.as_ref().map(|s| status_chip(tone, s.label.clone())));

    let blocks: Vec<AnyElement> = card
        .blocks
        .iter()
        .enumerate()
        .filter(|(_, b)| !matches!(b, CardBlock::Unknown))
        .map(|(ix, block)| blocks::render(block, ix, message, ctx, window, cx))
        .collect();
    record_render(message, card);

    let actions = actions::render(card, message, ctx, window, cx);

    let _ = (stripe, radius);
    ui::card(cx)
        .mt(space::S4)
        .w_full()
        .max_w(px(CARD_MAX_WIDTH))
        .flex()
        .overflow_hidden()
        .child(
            div()
                .flex_1()
                .min_w(px(0.))
                .px(space::S14)
                .py(space::S12)
                .flex()
                .flex_col()
                .gap(space::S10)
                .child(header)
                .children(blocks)
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
