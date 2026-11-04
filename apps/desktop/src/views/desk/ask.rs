//! One ask of 需要你拍板 as a row (NATIVE.md §9.4): the record words of an
//! answer, and the one answer a row can give in place (its default push
//! button). Everything else (options, forms, drafts) opens the 话题 with
//! 看看, where the card shows in full (DESIGN.md §7.10).

use std::collections::BTreeMap;

use chat_core::proto::{CardActionKind, CardV2, CardV2ActionStyle, CardV2Block};

use super::model::{Answer, AskCard};
use crate::{
    copy::{desk as c, fill},
    data::{
        card::{ActionStyle, Card},
        v2::AskKind,
    },
};

/// `你选了「批准合并」`.
pub fn button_record(label: &str) -> String {
    fill(c::DESK_ANSWERED, &[("label", label)])
}

/// `你选了 A：一个输入框，粘贴自动识别`.
pub fn option_record(key: &str, label: &str) -> String {
    fill(c::DESK_ANSWERED_OPTION, &[("key", key), ("label", label)])
}

/// The letter and label of option `ix`. A label written as `A 一个输入框`
/// keeps its own letter.
pub fn option_parts(ix: usize, label: &str) -> (String, String) {
    let mut chars = label.chars();
    if let (Some(first), Some(' ')) = (chars.next(), chars.next())
        && first.is_ascii_alphanumeric()
    {
        return (
            first.to_ascii_uppercase().to_string(),
            label[2..].trim().to_string(),
        );
    }
    let letter = (b'A' + (ix.min(25) as u8)) as char;
    (letter.to_string(), label.to_string())
}

/// The record of a v1 choice press, when `action_id` is one of its options.
pub fn v1_option_record(card: &Card, action_id: &str) -> Option<String> {
    let actions = card.actions.as_deref().unwrap_or(&[]);
    let ix = actions.iter().position(|a| a.id == action_id)?;
    let (key, label) = option_parts(ix, &actions[ix].label);
    Some(option_record(&key, &label))
}

/// The record of a v2 answer: the picked option, else the action's label.
pub fn v2_record(
    card: &CardV2,
    action_id: &str,
    inputs: &BTreeMap<String, serde_json::Value>,
) -> String {
    for b in all_blocks(&card.blocks) {
        if let CardV2Block::Decision { id, options, .. } = b
            && let Some(serde_json::Value::String(picked)) = inputs.get(id)
            && let Some(ix) = options.iter().position(|o| &o.id == picked)
        {
            let (key, label) = option_parts(ix, &options[ix].label);
            return option_record(&key, &label);
        }
    }
    let label = card
        .actions
        .as_deref()
        .unwrap_or(&[])
        .iter()
        .find(|a| a.id == action_id)
        .map(|a| a.label.clone())
        .unwrap_or_else(|| action_id.to_string());
    button_record(&label)
}

fn all_blocks(blocks: &[CardV2Block]) -> Vec<&CardV2Block> {
    let mut out = Vec::new();
    for b in blocks {
        out.push(b);
        if let CardV2Block::Group { blocks, .. } = b {
            out.extend(blocks.iter().flatten());
        }
    }
    out
}

/// The answer a row gives with its default button: the card's primary
/// action, when the card needs nothing else from me (no options to pick,
/// no required input, no text to type before an irreversible step).
#[derive(Clone, Debug)]
pub struct QuickAnswer {
    pub label: String,
    pub answer: Answer,
    /// The card asks to confirm first: (question, explanation).
    pub confirm: Option<(String, Option<String>)>,
}

impl QuickAnswer {
    pub fn record(&self) -> String {
        button_record(&self.label)
    }
}

pub fn quick_answer(a: &AskCard) -> Option<QuickAnswer> {
    if let Some(card) = a.card_v2.as_deref() {
        let blocks = all_blocks(&card.blocks);
        let needs_more = blocks.iter().any(|b| match b {
            CardV2Block::Decision { .. } => true,
            CardV2Block::Input { required, .. } => required.unwrap_or(false),
            _ => false,
        });
        let irreversible = card.ask.as_ref().and_then(|x| x.irreversible.as_ref());
        if needs_more || irreversible.is_some_and(|i| i.type_to_confirm.is_some()) {
            return None;
        }
        let act = card.actions.as_deref().unwrap_or(&[]).iter().find(|x| {
            x.style == Some(CardV2ActionStyle::Primary)
                && !matches!(
                    x.kind(),
                    CardActionKind::Link | CardActionKind::Cancel | CardActionKind::Unknown
                )
        })?;
        let consequence = irreversible.map(|i| i.consequence.clone());
        return Some(QuickAnswer {
            label: act.label.clone(),
            answer: Answer::V2 {
                rev: card.rev,
                action_id: act.id.clone(),
                inputs: BTreeMap::new(),
            },
            confirm: act
                .confirm
                .as_ref()
                .map(|cf| (cf.title.clone(), cf.body.clone().or(consequence))),
        });
    }
    let card = a.card.as_deref()?;
    let actions = card.actions.as_deref().unwrap_or(&[]);
    if a.item.kind == AskKind::Choice && actions.len() >= 2 {
        return None;
    }
    let act = actions
        .iter()
        .find(|x| x.style == Some(ActionStyle::Primary))?;
    Some(QuickAnswer {
        label: act.label.clone(),
        answer: Answer::V1 {
            action_id: act.id.clone(),
        },
        confirm: act.confirm.clone().map(|q| (q, None)),
    })
}
