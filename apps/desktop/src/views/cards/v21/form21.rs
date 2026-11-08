//! Card v2.1 values: what the viewer chose for the CARD21 §4 variants, their checks and the
//! answer payload in the CARD21 shapes (`{value}`, `{values}`, `{value:"other", other}`,
//! `{order}`, `{rows}`). Pure functions over [`Values`], unit tested without a window.

use std::collections::BTreeMap;

use serde_json::{Value, json};

use crate::data::cards_v2::{AnswerInputs, CardV2, CardV2Action, CardV2Block, InputKind};
use crate::views::cards::v2::form::{Problem, Val, Values};

/// The reserved option id of 其他 (CARD21 §4).
pub const OTHER: &str = "other";
/// The longest 其他 text.
pub const OTHER_MAX: usize = 60;

/// The key of the 其他 text of a choice block.
pub fn other_key(block: &str) -> String {
    format!("{block}#other")
}

/// The key of the note of a compare block.
pub fn note_key(block: &str) -> String {
    format!("{block}#note")
}

fn choice<'a>(values: &'a Values, id: &str) -> &'a [String] {
    match values.get(id) {
        Some(Val::Choice(c)) => c,
        _ => &[],
    }
}

fn text<'a>(values: &'a Values, id: &str) -> &'a str {
    match values.get(id) {
        Some(Val::Text(t)) => t.trim(),
        _ => "",
    }
}

fn num(values: &Values, id: &str) -> Option<f64> {
    match values.get(id) {
        Some(Val::Num(n)) => Some(*n),
        Some(Val::Text(t)) => t.trim().parse().ok().filter(|v: &f64| v.is_finite()),
        _ => None,
    }
}

/// The option ids a block offers, when it has a closed set.
pub fn option_ids(card: &CardV2, id: &str) -> Option<Vec<String>> {
    let b = card.block(id)?;
    Some(match b {
        CardV2Block::Decision {
            options,
            allow_other,
            ..
        } => {
            let mut v: Vec<String> = options.iter().map(|o| o.id.clone()).collect();
            if allow_other.is_some() {
                v.push(OTHER.into());
            }
            v
        }
        CardV2Block::ImageChoice { options, .. } => options.iter().map(|o| o.id.clone()).collect(),
        CardV2Block::Poll { options, .. } => options.iter().map(|o| o.id.clone()).collect(),
        CardV2Block::Slots { slots, .. } => slots.iter().map(|o| o.id.clone()).collect(),
        CardV2Block::Compare { options, .. } => options.iter().map(|o| o.id.clone()).collect(),
        CardV2Block::Table { rows, row_keys, .. } => row_keys
            .clone()
            .unwrap_or_else(|| (0..rows.len()).map(|i| i.to_string()).collect()),
        CardV2Block::Input {
            options: Some(o),
            kind: InputKind::Select | InputKind::Chips,
            ..
        } => o.iter().map(|o| o.id.clone()).collect(),
        _ => return None,
    })
}

fn missing(label: &str) -> Option<Problem> {
    Some(Problem::Missing(label.to_string()))
}

/// What keeps one 2.1 block from being sent, if anything.
pub fn problem(_card: &CardV2, b: &CardV2Block, values: &Values) -> Option<Problem> {
    match b {
        CardV2Block::Decision {
            id,
            label,
            required,
            multiple,
            min_select,
            max_select,
            options,
            ..
        } => {
            let picked = choice(values, id);
            let label = label.clone().unwrap_or_default();
            if picked.is_empty() {
                return required.unwrap_or(true).then_some(Problem::Missing(label));
            }
            if multiple.unwrap_or(false) {
                let n = picked.len() as u32;
                let min = min_select.unwrap_or(1);
                let max = max_select.unwrap_or(options.len() as u32);
                if n < min || n > max {
                    return Some(Problem::Invalid(label));
                }
            } else if picked[0] == OTHER {
                let t = text(values, &other_key(id));
                if t.is_empty() {
                    return Some(Problem::Missing(label));
                }
                if t.chars().count() > OTHER_MAX {
                    return Some(Problem::Invalid(label));
                }
            }
            None
        }
        CardV2Block::Rating {
            id,
            label,
            required,
            ..
        }
        | CardV2Block::Scale {
            id,
            label,
            required,
            ..
        } => (required.unwrap_or(true) && num(values, id).is_none())
            .then(|| missing(label))
            .flatten(),
        CardV2Block::ImageChoice {
            id,
            label,
            required,
            ..
        }
        | CardV2Block::Slots {
            id,
            label,
            required,
            ..
        }
        | CardV2Block::Poll {
            id,
            label,
            required,
            ..
        } => (required.unwrap_or(true) && choice(values, id).is_empty())
            .then(|| missing(label))
            .flatten(),
        CardV2Block::Matrix {
            id,
            label,
            required,
            rows,
            ..
        } => {
            let set = match values.get(id) {
                Some(Val::Rows(r)) => r.clone(),
                _ => BTreeMap::new(),
            };
            rows.iter()
                .any(|r| {
                    (r.required.unwrap_or(false) || required.unwrap_or(false))
                        && !set.contains_key(&r.id)
                })
                .then(|| missing(label))
                .flatten()
        }
        CardV2Block::Compare {
            id, question, note, ..
        } => {
            if choice(values, id).is_empty() {
                return missing(question.as_deref().unwrap_or(""));
            }
            let n = text(values, &note_key(id)).chars().count();
            let max = note.as_ref().and_then(|n| n.max_length).unwrap_or(1000) as usize;
            (n > max)
                .then(|| Some(Problem::Invalid(question.clone().unwrap_or_default())))
                .flatten()
        }
        _ => None,
    }
}

/// The value of one input block in the CARD21 shapes, or `None` when it has none.
pub fn value_of(card: &CardV2, b: &CardV2Block, values: &Values) -> Option<Value> {
    let id = b.id()?;
    let one_or_many = |multiple: bool| {
        let c = choice(values, id);
        if c.is_empty() {
            None
        } else if multiple {
            Some(json!({ "values": c }))
        } else {
            Some(json!({ "value": c[0] }))
        }
    };
    match b {
        CardV2Block::Decision { multiple, .. } => {
            let c = choice(values, id);
            if !multiple.unwrap_or(false) && c.first().map(String::as_str) == Some(OTHER) {
                return Some(json!({ "value": OTHER, "other": text(values, &other_key(id)) }));
            }
            one_or_many(multiple.unwrap_or(false))
        }
        CardV2Block::ImageChoice { multiple, .. } | CardV2Block::Slots { multiple, .. } => {
            one_or_many(multiple.unwrap_or(false))
        }
        CardV2Block::Poll { .. } => one_or_many(false),
        CardV2Block::Table { select, .. } => {
            let c = choice(values, id);
            match select {
                Some(crate::data::cards_v2::TableSelect::Multiple) => Some(json!({ "values": c })),
                Some(_) if !c.is_empty() => Some(json!({ "value": c[0] })),
                _ => None,
            }
        }
        CardV2Block::Compare { note, .. } => {
            let c = choice(values, id).first()?;
            let t = text(values, &note_key(id));
            Some(if note.is_some() && !t.is_empty() {
                json!({ "value": c, "note": t })
            } else {
                json!({ "value": c })
            })
        }
        CardV2Block::Rank { items, .. } => {
            let order = match values.get(id) {
                Some(Val::Order(o)) => o.clone(),
                _ => items.iter().map(|i| i.id.clone()).collect(),
            };
            Some(json!({ "order": order }))
        }
        CardV2Block::Rating { .. } | CardV2Block::Scale { .. } => {
            num(values, id).map(|n| json!({ "value": n.round() as i64 }))
        }
        CardV2Block::Confirm { .. } => match values.get(id) {
            Some(Val::Switch(b)) => Some(json!({ "value": b })),
            _ => None,
        },
        CardV2Block::Matrix { .. } => match values.get(id) {
            Some(Val::Rows(r)) if !r.is_empty() => Some(json!({ "rows": r })),
            _ => None,
        },
        CardV2Block::Input { kind, .. } => match kind {
            InputKind::Chips => Some(json!({ "values": choice(values, id) })),
            InputKind::Select => choice(values, id).first().map(|c| json!({ "value": c })),
            InputKind::Switch => Some(json!({
                "value": matches!(values.get(id), Some(Val::Switch(true)))
            })),
            InputKind::Number | InputKind::Slider => num(values, id)
                .and_then(serde_json::Number::from_f64)
                .map(|n| json!({ "value": n })),
            _ => Some(text(values, id))
                .filter(|t| !t.is_empty())
                .map(|t| json!({ "value": t })),
        },
        _ => {
            let _ = card;
            None
        }
    }
}

/// `inputs` of an answer: the blocks `action` names (2.1), or every input block (2.0
/// cards and actions that name none).
pub fn to_inputs(
    card: &CardV2,
    values: &Values,
    action: Option<&CardV2Action>,
) -> Option<AnswerInputs> {
    let named: Option<Vec<String>> = action.and_then(|a| a.inputs.clone()).map(|ids| {
        let mut all = Vec::new();
        for id in ids {
            if let Some(CardV2Block::Questionnaire { children, .. }) = card.block(&id) {
                all.extend(children.iter().cloned());
            }
            all.push(id);
        }
        all
    });
    let mut out = BTreeMap::new();
    for b in card.all_blocks() {
        let Some(id) = b.id() else { continue };
        if !b.takes_input() {
            continue;
        }
        if named.as_ref().is_some_and(|n| !n.iter().any(|x| x == id)) {
            continue;
        }
        if let Some(v) = value_of(card, b, values) {
            out.insert(id.to_string(), v);
        }
    }
    (!out.is_empty()).then_some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn card(v: Value) -> CardV2 {
        serde_json::from_value(v).expect("card parses")
    }

    fn choice_card() -> CardV2 {
        card(json!({
            "v": 2, "catalog": "tc.card/2.1", "rev": 1, "intent": "ask", "title": "先告诉谁？",
            "fallback": "f", "ask": { "from": ["me"] },
            "blocks": [
                { "type": "decision", "id": "who", "label": "先告诉谁？", "fallbackText": "f",
                  "options": [{ "id": "a", "label": "Watch" }, { "id": "b", "label": "小王" }],
                  "allowOther": { "label": "其他" } },
                { "type": "decision", "id": "where", "label": "发哪几个平台？", "multiple": true,
                  "minSelect": 2, "maxSelect": 3, "fallbackText": "f",
                  "options": [{ "id": "x", "label": "微博" }, { "id": "y", "label": "B 站" },
                              { "id": "z", "label": "抖音" }, { "id": "w", "label": "即刻" }] },
                { "type": "rank", "id": "order", "label": "排序", "fallbackText": "f",
                  "items": [{ "id": "p", "label": "照片" }, { "id": "s", "label": "分享" }] },
                { "type": "matrix", "id": "m", "label": "放进哪个版本", "fallbackText": "f",
                  "rows": [{ "id": "r1", "label": "照片", "required": true }, { "id": "r2", "label": "提醒" }],
                  "columns": [{ "id": "v40", "label": "4.0" }, { "id": "v41", "label": "4.1" }] }
            ],
            "actions": [{ "id": "send", "label": "提交", "style": "primary", "inputs": ["who", "where", "order", "m"] }]
        }))
    }

    #[test]
    fn other_needs_its_text_and_is_sent_with_it() {
        let c = choice_card();
        let mut v = Values::new();
        v.insert("who".into(), Val::Choice(vec![OTHER.into()]));
        let b = c.block("who").unwrap();
        assert!(matches!(problem(&c, b, &v), Some(Problem::Missing(_))));
        v.insert(
            other_key("who"),
            Val::Text("Alex Chen，下周轮到他值班".into()),
        );
        assert!(problem(&c, b, &v).is_none());
        assert_eq!(
            value_of(&c, b, &v).unwrap(),
            json!({ "value": "other", "other": "Alex Chen，下周轮到他值班" })
        );
        v.insert(other_key("who"), Val::Text("字".repeat(61)));
        assert!(matches!(problem(&c, b, &v), Some(Problem::Invalid(_))));
    }

    #[test]
    fn multiple_respects_min_and_max() {
        let c = choice_card();
        let b = c.block("where").unwrap();
        let mut v = Values::new();
        v.insert("where".into(), Val::Choice(vec!["x".into()]));
        assert!(matches!(problem(&c, b, &v), Some(Problem::Invalid(_))));
        v.insert("where".into(), Val::Choice(vec!["x".into(), "y".into()]));
        assert!(problem(&c, b, &v).is_none());
        assert_eq!(
            value_of(&c, b, &v).unwrap(),
            json!({ "values": ["x", "y"] })
        );
    }

    #[test]
    fn rank_defaults_to_the_item_order_and_matrix_needs_required_rows() {
        let c = choice_card();
        let v = Values::new();
        assert_eq!(
            value_of(&c, c.block("order").unwrap(), &v).unwrap(),
            json!({ "order": ["p", "s"] })
        );
        let m = c.block("m").unwrap();
        assert!(problem(&c, m, &v).is_some());
        let mut v = Values::new();
        v.insert(
            "m".into(),
            Val::Rows([("r1".to_string(), "v40".to_string())].into()),
        );
        assert!(problem(&c, m, &v).is_none());
        assert_eq!(
            value_of(&c, m, &v).unwrap(),
            json!({ "rows": { "r1": "v40" } })
        );
    }

    #[test]
    fn inputs_follow_the_action() {
        let c = choice_card();
        let mut v = Values::new();
        v.insert("who".into(), Val::Choice(vec!["a".into()]));
        let send = c.actions()[0].clone();
        let inputs = to_inputs(&c, &v, Some(&send)).unwrap();
        assert_eq!(inputs["who"], json!({ "value": "a" }));
        assert!(inputs.contains_key("order"));
        let only = CardV2Action {
            inputs: Some(vec!["who".into()]),
            ..send
        };
        let inputs = to_inputs(&c, &v, Some(&only)).unwrap();
        assert_eq!(inputs.len(), 1);
    }
}
