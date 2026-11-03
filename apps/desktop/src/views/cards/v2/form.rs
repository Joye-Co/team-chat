//! What the viewer has chosen and typed on one v2 card before answering:
//! decision choices, form fields, the draft edit mode, the open answer
//! record. One [`CardForm`] per card lives in the window as keyed element
//! state (`views::cards::v2::render`), so the desk, the topic and the pinned
//! card each keep their own.
//!
//! Validation and the answer payload are pure functions over [`Values`], so
//! they are unit tested without a window.

use std::collections::{BTreeMap, HashMap};

use gpui::{AppContext as _, Context, Entity, Focusable as _, Subscription, Window};
use gpui_component::input::{InputEvent, InputState, TextareaState};
use serde_json::{Number, Value};

use crate::data::cards_v2::{AnswerEdits, AnswerInputs, CardV2, CardV2Block, InputKind};

/// A typed or chosen value of one block.
#[derive(Clone, Debug, PartialEq)]
pub enum Val {
    Text(String),
    Choice(Vec<String>),
    Switch(bool),
    // --- inc-cards21: 2.1 inputs ---
    /// A rank: item ids in order.
    Order(Vec<String>),
    /// A rating, a scale or a slider.
    Num(f64),
    /// A matrix: row id → column id.
    Rows(BTreeMap<String, String>),
    // --- end inc-cards21 ---
}

/// The viewer's values by block id.
pub type Values = HashMap<String, Val>;

/// Why a field cannot be sent yet.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Problem {
    /// A required field is empty; carries its label.
    Missing(String),
    /// A value is not valid; carries its label.
    Invalid(String),
}

fn text_of<'a>(values: &'a Values, id: &str) -> &'a str {
    match values.get(id) {
        Some(Val::Text(t)) => t.trim(),
        _ => "",
    }
}

fn choice_of<'a>(values: &'a Values, id: &str) -> &'a [String] {
    match values.get(id) {
        Some(Val::Choice(c)) => c,
        _ => &[],
    }
}

/// `YYYY-MM-DD` that is a real date.
pub fn valid_date(text: &str) -> bool {
    chrono::NaiveDate::parse_from_str(text, "%Y-%m-%d").is_ok()
}

fn number(text: &str) -> Option<f64> {
    text.parse::<f64>().ok().filter(|v| v.is_finite())
}

fn f(n: &Option<Number>) -> Option<f64> {
    n.as_ref().and_then(Number::as_f64)
}

/// Every problem that blocks the submit action, in block order. Decisions
/// need a choice; required inputs need a value; typed values must parse and
/// fit `min`, `max` and `maxLength`.
pub fn problems(card: &CardV2, values: &Values) -> Vec<Problem> {
    let mut out = Vec::new();
    for block in card.all_blocks() {
        match block {
            CardV2Block::Decision { .. } if super::super::v21::is_21(card) => {
                out.extend(super::super::v21::form21::problem(card, block, values));
            }
            CardV2Block::Decision { id, .. } => {
                if choice_of(values, id).is_empty() {
                    out.push(Problem::Missing(String::new()));
                }
            }
            CardV2Block::Input {
                id,
                kind,
                label,
                required,
                min,
                max,
                max_length,
                ..
            } => {
                let required = required.unwrap_or(false);
                let missing = match kind {
                    InputKind::Chips | InputKind::Select => choice_of(values, id).is_empty(),
                    InputKind::Switch => false,
                    _ => text_of(values, id).is_empty(),
                };
                if missing {
                    if required {
                        out.push(Problem::Missing(label.clone()));
                    }
                    continue;
                }
                let text = text_of(values, id);
                let bad = match kind {
                    InputKind::Number | InputKind::Slider => match number(text) {
                        None => true,
                        Some(v) => f(min).is_some_and(|m| v < m) || f(max).is_some_and(|m| v > m),
                    },
                    InputKind::Date => !valid_date(text),
                    InputKind::Text | InputKind::Textarea | InputKind::Unknown => {
                        max_length.is_some_and(|m| text.chars().count() > m as usize)
                    }
                    InputKind::Chips => {
                        let n = choice_of(values, id).len() as f64;
                        f(min).is_some_and(|m| n < m) || f(max).is_some_and(|m| n > m)
                    }
                    InputKind::Select | InputKind::Switch => false,
                };
                if bad {
                    out.push(Problem::Invalid(label.clone()));
                }
            }
            // --- inc-cards21 ---
            b => out.extend(super::super::v21::form21::problem(card, b, values)),
            // --- end inc-cards21 ---
        }
    }
    out
}

/// The labels of the missing required fields (`还差名字、岗位、入职日期`).
pub fn missing_labels(problems: &[Problem]) -> Vec<String> {
    problems
        .iter()
        .filter_map(|p| match p {
            Problem::Missing(l) if !l.is_empty() => Some(l.clone()),
            _ => None,
        })
        .collect()
}

/// `inputs` of the answer: every decision and input block with a value.
/// Decisions send one option id (an array with `multiple`); chips an array;
/// select one id; switch a bool; number a number; the rest text.
pub fn to_inputs(card: &CardV2, values: &Values) -> Option<AnswerInputs> {
    // --- inc-cards21: 2.1 cards send the CARD21 §4 shapes ---
    if super::super::v21::is_21(card) {
        return super::super::v21::form21::to_inputs(card, values, None);
    }
    // --- end inc-cards21 ---
    let mut out = BTreeMap::new();
    for block in card.all_blocks() {
        match block {
            CardV2Block::Decision { id, multiple, .. } => {
                let c = choice_of(values, id);
                if c.is_empty() {
                    continue;
                }
                let v = if multiple.unwrap_or(false) {
                    Value::from(c.to_vec())
                } else {
                    Value::from(c[0].clone())
                };
                out.insert(id.clone(), v);
            }
            CardV2Block::Input { id, kind, .. } => {
                let v = match kind {
                    InputKind::Chips => {
                        let c = choice_of(values, id);
                        Some(Value::from(c.to_vec()))
                    }
                    InputKind::Select => choice_of(values, id).first().cloned().map(Value::from),
                    InputKind::Switch => match values.get(id) {
                        Some(Val::Switch(b)) => Some(Value::from(*b)),
                        _ => Some(Value::from(false)),
                    },
                    InputKind::Number => number(text_of(values, id))
                        .and_then(Number::from_f64)
                        .map(Value::Number),
                    _ => Some(text_of(values, id))
                        .filter(|t| !t.is_empty())
                        .map(|t| Value::from(t.to_string())),
                };
                if let Some(v) = v {
                    out.insert(id.clone(), v);
                }
            }
            _ => {}
        }
    }
    (!out.is_empty()).then_some(out)
}

/// `editedValues` of the answer: the editable drafts whose text changed.
pub fn to_edits(card: &CardV2, values: &Values) -> Option<AnswerEdits> {
    let mut out = BTreeMap::new();
    for block in card.all_blocks() {
        if let CardV2Block::Draft {
            id,
            text,
            editable: Some(true),
            ..
        } = block
            && let Some(Val::Text(t)) = values.get(id)
            && t != text
        {
            out.insert(id.clone(), t.clone());
        }
    }
    (!out.is_empty()).then_some(out)
}

/// The initial values of a card: decision and input defaults, draft texts.
pub fn defaults(card: &CardV2) -> Values {
    let mut v = Values::new();
    for block in card.all_blocks() {
        match block {
            CardV2Block::Decision { .. } => {
                // `default` marks the recommended options (推荐); the viewer
                // still chooses, so nothing is preselected.
            }
            CardV2Block::Input {
                id, kind, default, ..
            } => {
                let val = match (kind, default) {
                    (InputKind::Switch, Some(Value::Bool(b))) => Val::Switch(*b),
                    (InputKind::Chips, Some(Value::Array(a))) => Val::Choice(
                        a.iter()
                            .filter_map(|x| x.as_str().map(str::to_string))
                            .collect(),
                    ),
                    (InputKind::Select, Some(Value::String(s))) => Val::Choice(vec![s.clone()]),
                    (_, Some(Value::String(s))) => Val::Text(s.clone()),
                    (_, Some(Value::Number(n))) => Val::Text(n.to_string()),
                    _ => continue,
                };
                v.insert(id.clone(), val);
            }
            CardV2Block::Draft { id, text, .. } => {
                v.insert(id.clone(), Val::Text(text.clone()));
            }
            _ => {}
        }
    }
    v
}

/// One text field of the form.
#[derive(Clone)]
pub enum Field {
    Line(Entity<InputState>),
    Area(Entity<TextareaState>),
}

impl Field {
    pub fn text(&self, cx: &gpui::App) -> String {
        match self {
            Field::Line(s) => s.read(cx).value().to_string(),
            Field::Area(s) => s.read(cx).value().to_string(),
        }
    }

    pub fn is_focused(&self, window: &Window, cx: &gpui::App) -> bool {
        match self {
            Field::Line(s) => s.read(cx).focus_handle(cx).is_focused(window),
            Field::Area(s) => s.read(cx).focus_handle(cx).is_focused(window),
        }
    }
}

/// Values set before the first paint (screenshot scenes and the gallery).
#[derive(Clone, Debug, Default)]
pub struct Preset {
    pub values: Vec<(&'static str, Val)>,
    /// Open the draft edit mode.
    pub editing: bool,
    /// Expand the answer record.
    pub expanded: bool,
    /// inc-cards21: the rev the viewer saw before (screenshot scenes of a new rev).
    pub prev: Option<std::sync::Arc<CardV2>>,
}

/// The viewer's work on one card.
pub struct CardForm {
    /// The rev the values were made for.
    pub rev: u32,
    choices: HashMap<String, Vec<String>>,
    switches: HashMap<String, bool>,
    fields: HashMap<String, Field>,
    initial: Values,
    /// The draft edit mode is open, started at this rev.
    pub editing: Option<u32>,
    /// The answer record is expanded (展开).
    pub expanded: bool,
    /// inc-cards21: rank orders, numbers and matrix rows.
    other: HashMap<String, Val>,
    _subscriptions: Vec<Subscription>,
}

impl CardForm {
    pub fn new(card: &CardV2, preset: Option<&Preset>) -> Self {
        let mut initial = defaults(card);
        let mut editing = None;
        let mut expanded = false;
        if let Some(p) = preset {
            for (k, v) in &p.values {
                initial.insert((*k).to_string(), v.clone());
            }
            editing = p.editing.then_some(card.rev);
            expanded = p.expanded;
        }
        let mut choices = HashMap::new();
        let mut switches = HashMap::new();
        for (k, v) in &initial {
            match v {
                Val::Choice(c) => {
                    choices.insert(k.clone(), c.clone());
                }
                Val::Switch(b) => {
                    switches.insert(k.clone(), *b);
                }
                Val::Text(_) | Val::Order(_) | Val::Num(_) | Val::Rows(_) => {}
            }
        }
        let mut s = Self {
            rev: card.rev,
            choices,
            switches,
            fields: HashMap::new(),
            initial,
            editing,
            expanded,
            other: HashMap::new(),
            _subscriptions: Vec::new(),
        };
        for (k, v) in &s.initial {
            if let Val::Order(_) | Val::Num(_) | Val::Rows(_) = v {
                s.other.insert(k.clone(), v.clone());
            }
        }
        s
    }

    /// Starts over for a newer rev of the card.
    pub fn reset(&mut self, card: &CardV2) {
        *self = Self::new(card, None);
    }

    // --- inc-cards21: local values survive revs (INTERACT §4.4) ---

    /// Moves to a newer rev and keeps what the viewer chose or typed for every block that
    /// still exists (choices only when their option ids still exist). A block replaced by
    /// another type starts over.
    pub fn rebase(&mut self, old: &CardV2, card: &CardV2, cx: &mut Context<Self>) {
        let same_type = |id: &str| {
            matches!((old.block(id), card.block(id)), (Some(a), Some(b))
                if a.type_name() == b.type_name())
        };
        let ids = |id: &str| super::super::v21::form21::option_ids(card, id);
        self.choices.retain(|k, v| {
            let base = k.split('#').next().unwrap_or(k);
            same_type(base) && ids(base).is_none_or(|known| v.iter().all(|o| known.contains(o)))
        });
        self.switches.retain(|k, _| same_type(k));
        self.fields
            .retain(|k, _| same_type(k.split('#').next().unwrap_or(k)));
        self.other.retain(|k, _| same_type(k));
        let fresh = defaults(card);
        for (k, v) in fresh {
            if !self.initial.contains_key(&k) || !same_type(&k) {
                self.initial.insert(k, v);
            }
        }
        self.initial
            .retain(|k, _| card.block(k.split('#').next().unwrap_or(k)).is_some());
        self.rev = card.rev;
        cx.notify();
    }

    /// A 2.1 value (rank order, number, matrix rows).
    pub fn value21(&self, block: &str) -> Option<&Val> {
        self.other.get(block)
    }

    pub fn set_value21(&mut self, block: &str, v: Val, cx: &mut Context<Self>) {
        self.other.insert(block.to_string(), v);
        cx.notify();
    }

    /// Sets the whole choice of `block` (a matrix row, a table selection).
    pub fn set_choice(&mut self, block: &str, ids: Vec<String>, cx: &mut Context<Self>) {
        self.choices.insert(block.to_string(), ids);
        cx.notify();
    }
    // --- end inc-cards21 ---

    pub fn choice(&self, block: &str) -> &[String] {
        self.choices.get(block).map(Vec::as_slice).unwrap_or(&[])
    }

    /// Picks `option` of `block`: one option, or toggles it with `multiple`.
    pub fn choose(&mut self, block: &str, option: &str, multiple: bool, cx: &mut Context<Self>) {
        let list = self.choices.entry(block.to_string()).or_default();
        if multiple {
            if let Some(i) = list.iter().position(|o| o == option) {
                list.remove(i);
            } else {
                list.push(option.to_string());
            }
        } else {
            *list = vec![option.to_string()];
        }
        cx.notify();
    }

    pub fn switch(&self, block: &str) -> bool {
        self.switches.get(block).copied().unwrap_or(false)
    }

    /// inc-cards21: sets a switch (a confirm's yes or no).
    pub fn toggle_switch_to(&mut self, block: &str, value: bool, cx: &mut Context<Self>) {
        self.switches.insert(block.to_string(), value);
        cx.notify();
    }

    pub fn toggle_switch(&mut self, block: &str, cx: &mut Context<Self>) {
        let v = !self.switch(block);
        self.switches.insert(block.to_string(), v);
        cx.notify();
    }

    pub fn set_editing(&mut self, on: bool, cx: &mut Context<Self>) {
        self.editing = on.then_some(self.rev);
        if !on {
            // Cancel drops the edits: the fields start from the draft again.
            let drafts: Vec<String> = self
                .fields
                .keys()
                .filter(|k| self.initial.contains_key(*k))
                .cloned()
                .collect();
            for k in drafts {
                self.fields.remove(&k);
            }
        }
        cx.notify();
    }

    pub fn toggle_expanded(&mut self, cx: &mut Context<Self>) {
        self.expanded = !self.expanded;
        cx.notify();
    }

    /// The text field of `block`, made on first use.
    pub fn field(
        &mut self,
        block: &str,
        placeholder: Option<&str>,
        multi_line: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Field {
        if let Some(f) = self.fields.get(block) {
            return f.clone();
        }
        let text = match self.initial.get(block) {
            Some(Val::Text(t)) => t.clone(),
            _ => String::new(),
        };
        let placeholder = placeholder.unwrap_or("").to_string();
        let field = if multi_line {
            let s = cx.new(|cx| {
                TextareaState::new(window, cx)
                    .placeholder(placeholder)
                    .default_value(text)
            });
            self._subscriptions
                .push(cx.subscribe(&s, |_, _, _: &InputEvent, cx| cx.notify()));
            Field::Area(s)
        } else {
            let s = cx.new(|cx| {
                InputState::new(window, cx)
                    .placeholder(placeholder)
                    .default_value(text)
            });
            self._subscriptions
                .push(cx.subscribe(&s, |_, _, _: &InputEvent, cx| cx.notify()));
            Field::Line(s)
        };
        self.fields.insert(block.to_string(), field.clone());
        field
    }

    /// Every value now: chosen, switched and typed (a field not shown yet
    /// keeps its initial text).
    pub fn values(&self, cx: &gpui::App) -> Values {
        let mut v = self.initial.clone();
        for (k, c) in &self.choices {
            v.insert(k.clone(), Val::Choice(c.clone()));
        }
        for (k, b) in &self.switches {
            v.insert(k.clone(), Val::Switch(*b));
        }
        for (k, f) in &self.fields {
            v.insert(k.clone(), Val::Text(f.text(cx)));
        }
        for (k, x) in &self.other {
            v.insert(k.clone(), x.clone());
        }
        v
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn form_card() -> CardV2 {
        serde_json::from_value(serde_json::json!({
            "v": 2, "rev": 1, "intent": "ask", "title": "新实习生入职，要开哪些权限？", "fallback": "",
            "ask": { "from": ["m-joye"] },
            "blocks": [
                { "type": "input", "id": "name", "kind": "text", "label": "名字", "required": true, "maxLength": 20 },
                { "type": "input", "id": "role", "kind": "select", "label": "岗位", "required": true,
                  "options": [{ "id": "data", "label": "数据实习" }] },
                { "type": "input", "id": "start", "kind": "date", "label": "入职日期", "required": true },
                { "type": "input", "id": "days", "kind": "number", "label": "天数", "min": 1, "max": 90 },
                { "type": "input", "id": "groups", "kind": "chips", "label": "要进的群",
                  "options": [{ "id": "dev", "label": "开发" }, { "id": "all", "label": "全员" }],
                  "default": ["all"] },
                { "type": "input", "id": "laptop", "kind": "switch", "label": "要电脑" },
                { "type": "draft", "id": "note", "text": "欢迎", "editable": true }
            ],
            "actions": [{ "id": "submit", "label": "提交", "style": "primary" }]
        }))
        .unwrap()
    }

    #[test]
    fn missing_required_fields_are_listed_in_order() {
        let card = form_card();
        let v = defaults(&card);
        let p = problems(&card, &v);
        assert_eq!(missing_labels(&p), ["名字", "岗位", "入职日期"]);
    }

    #[test]
    fn values_are_checked_and_sent_with_their_types() {
        let card = form_card();
        let mut v = defaults(&card);
        v.insert("name".into(), Val::Text("周可".into()));
        v.insert("role".into(), Val::Choice(vec!["data".into()]));
        v.insert("start".into(), Val::Text("2026-02-30".into()));
        v.insert("days".into(), Val::Text("120".into()));
        let p = problems(&card, &v);
        assert_eq!(
            p,
            [
                Problem::Invalid("入职日期".into()),
                Problem::Invalid("天数".into())
            ]
        );
        v.insert("start".into(), Val::Text("2026-11-10".into()));
        v.insert("days".into(), Val::Text("30".into()));
        assert!(problems(&card, &v).is_empty());
        let inputs = to_inputs(&card, &v).unwrap();
        assert_eq!(inputs["name"], "周可");
        assert_eq!(inputs["role"], "data");
        assert_eq!(inputs["start"], "2026-11-10");
        assert_eq!(inputs["days"], 30.0);
        assert_eq!(inputs["groups"], serde_json::json!(["all"]));
        assert_eq!(inputs["laptop"], false);
        // An unchanged draft sends no edit; a changed one does.
        assert!(to_edits(&card, &v).is_none());
        v.insert("note".into(), Val::Text("欢迎加入".into()));
        assert_eq!(to_edits(&card, &v).unwrap()["note"], "欢迎加入");
    }

    #[test]
    fn too_long_text_is_invalid() {
        let card = form_card();
        let mut v = defaults(&card);
        v.insert("name".into(), Val::Text("字".repeat(21)));
        assert!(problems(&card, &v).contains(&Problem::Invalid("名字".into())));
    }

    #[test]
    fn a_decision_needs_a_choice_and_sends_one_id() {
        let card: CardV2 = serde_json::from_value(serde_json::json!({
            "v": 2, "rev": 1, "intent": "ask", "title": "t", "fallback": "",
            "blocks": [{ "type": "decision", "id": "pick", "default": ["a"], "options": [
                { "id": "a", "label": "周二" }, { "id": "b", "label": "周三" }
            ]}]
        }))
        .unwrap();
        let mut v = defaults(&card);
        assert_eq!(problems(&card, &v).len(), 1, "推荐 is not a choice");
        v.insert("pick".into(), Val::Choice(vec!["b".into()]));
        assert!(problems(&card, &v).is_empty());
        assert_eq!(to_inputs(&card, &v).unwrap()["pick"], "b");
    }

    #[test]
    fn dates_must_be_real() {
        assert!(valid_date("2026-11-10"));
        assert!(!valid_date("2026-13-01"));
        assert!(!valid_date("11/10"));
    }
}
