//! Card v2 (docs/protocol.md, "Cards v2"): semantic blocks, intents, asks with inputs, and
//! answers. It travels in the optional `Message.cardV2` next to a server-derived v1 `card`.
//!
//! Readers are lenient so that later versions can add values: unknown block types, intents,
//! input kinds, chart kinds, ref kinds and action kinds read as `Unknown` (render nothing, or
//! the safe default), and a `cardV2` that does not parse at all is dropped from the message
//! (the v1 `card` stays). [`CardV2::validate`] (what the server runs) rejects every `Unknown`.

use std::collections::BTreeMap;

use serde::{Deserialize, Deserializer, Serialize};
use serde_json::{Number, Value};

use crate::bots::{RunPlanItem, check_plan};
use crate::card::{CardField, CardListItem, LinkTarget, check_action_id, check_web_url};
use crate::entities::Millis;
use crate::limits::{Validation, ValidationError, check_chars, check_id};
use crate::message::ArtifactRef;
use crate::serde_util::{any_value, non_null};

pub const CARD_V2_VERSION: u32 = 2;
/// Max size of a v2 card: UTF-8 bytes of its JSON text.
pub const MAX_CARD_V2_BYTES: usize = 65_536;
/// Max blocks of a v2 card, nested group children included.
pub const MAX_CARD_V2_BLOCKS: usize = 50;
pub const MAX_CARD_V2_ACTIONS: usize = 10;
pub const MAX_GROUP_BLOCKS: usize = 20;
pub const MAX_DECISION_OPTIONS: usize = 10;
pub const MAX_INPUT_OPTIONS: usize = 50;
pub const MAX_CHART_SERIES: usize = 8;
pub const MAX_CHART_POINTS: usize = 200;
pub const MAX_ASK_FROM: usize = 20;
/// Max UTF-8 bytes of `data` as JSON.
pub const MAX_CARD_DATA_BYTES: usize = 16_384;
/// Max UTF-8 bytes of `inputs` and `editedValues` of one answer, as JSON together.
pub const MAX_CARD_ANSWER_BYTES: usize = 65_536;
/// Max UTF-8 bytes of an input's `default` as JSON.
pub const MAX_INPUT_DEFAULT_BYTES: usize = 4096;
/// `ask.from` entry: the topic's creator (a bot that knows a run's requester lists the id).
pub const ASK_FROM_REQUESTER: &str = "requester";
/// `ask.from` entry: every owner.
pub const ASK_FROM_OWNERS: &str = "owners";

fn fail(field: &'static str, message: impl Into<String>) -> Validation {
    Err(ValidationError {
        field,
        message: message.into(),
    })
}

fn json_bytes(v: &impl Serialize) -> usize {
    serde_json::to_vec(v).map(|b| b.len()).unwrap_or(usize::MAX)
}

/// The JSON literal `2`, the card v2 schema version.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct CardV2Version;

impl Serialize for CardV2Version {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_u32(CARD_V2_VERSION)
    }
}

impl<'de> Deserialize<'de> for CardV2Version {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let v = u32::deserialize(d)?;
        if v == CARD_V2_VERSION {
            Ok(CardV2Version)
        } else {
            Err(serde::de::Error::custom(format!(
                "unsupported card v2 version {v}"
            )))
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CardIntent {
    /// Needs an answer (需要你拍板).
    Ask,
    /// A result with proof (做完了).
    Done,
    /// Work in progress (正在做).
    Working,
    #[default]
    Info,
    /// A value this version does not know; render as `info`.
    #[serde(other)]
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AskIrreversible {
    pub verb: String,
    pub target: String,
    pub consequence: String,
    /// When set, the answerer types this text before the submit action is enabled.
    #[serde(
        default,
        deserialize_with = "non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub type_to_confirm: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CardAsk {
    /// Member ids, [`ASK_FROM_REQUESTER`] or [`ASK_FROM_OWNERS`].
    pub from: Vec<String>,
    #[serde(
        default,
        deserialize_with = "non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub expires_at: Option<Millis>,
    /// An answer is a group decision (群里的决定).
    #[serde(
        default,
        deserialize_with = "non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub record: Option<bool>,
    #[serde(
        default,
        deserialize_with = "non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub irreversible: Option<AskIrreversible>,
}

impl CardAsk {
    /// True when `member` may answer: listed by id, an owner when `owners` is listed, or the
    /// requester when `requester` is listed.
    pub fn allows(&self, member: &str, is_owner: bool, requester: Option<&str>) -> bool {
        self.from.iter().any(|f| match f.as_str() {
            ASK_FROM_OWNERS => is_owner,
            ASK_FROM_REQUESTER => requester == Some(member),
            id => id == member,
        })
    }
    pub fn is_record(&self) -> bool {
        self.record.unwrap_or(false)
    }
}

/// Where a `ref` block or a `link` action points.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum RefTarget {
    Pr {
        url: String,
        #[serde(
            default,
            deserialize_with = "non_null",
            skip_serializing_if = "Option::is_none"
        )]
        repo: Option<String>,
        #[serde(
            default,
            deserialize_with = "non_null",
            skip_serializing_if = "Option::is_none"
        )]
        number: Option<u64>,
        #[serde(
            default,
            deserialize_with = "non_null",
            skip_serializing_if = "Option::is_none"
        )]
        state: Option<String>,
    },
    Artifact {
        #[serde(rename = "artifactId")]
        artifact_id: String,
        #[serde(
            default,
            deserialize_with = "non_null",
            skip_serializing_if = "Option::is_none"
        )]
        version: Option<u32>,
    },
    Topic {
        #[serde(rename = "groupId")]
        group_id: String,
        #[serde(rename = "topicId")]
        topic_id: String,
    },
    Member {
        #[serde(rename = "memberId")]
        member_id: String,
    },
    Url {
        url: String,
    },
    /// A kind this version does not know. Render the label only.
    #[serde(other)]
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InputKind {
    Text,
    Textarea,
    Number,
    Select,
    Date,
    Switch,
    Chips,
    /// A kind this version does not know. Render as `text`.
    #[serde(other)]
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChartKind {
    Line,
    Bar,
    Area,
    Spark,
    /// A kind this version does not know. Render as `line`.
    #[serde(other)]
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MetricBetter {
    Up,
    Down,
    #[serde(other)]
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EvidenceState {
    Pass,
    Fail,
    Info,
    #[serde(other)]
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DecisionOption {
    pub id: String,
    pub label: String,
    #[serde(
        default,
        deserialize_with = "non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub detail: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InputOption {
    pub id: String,
    pub label: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChartSeries {
    pub name: String,
    pub values: Vec<Number>,
}

/// One block of a v2 card. Every block has an `id`, unique in the card.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum CardV2Block {
    Text {
        id: String,
        text: String,
    },
    Fields {
        id: String,
        fields: Vec<CardField>,
    },
    Table {
        id: String,
        columns: Vec<String>,
        rows: Vec<Vec<String>>,
    },
    List {
        id: String,
        items: Vec<CardListItem>,
    },
    Progress {
        id: String,
        value: Number,
        #[serde(
            default,
            deserialize_with = "non_null",
            skip_serializing_if = "Option::is_none"
        )]
        label: Option<String>,
    },
    Code {
        id: String,
        #[serde(
            default,
            deserialize_with = "non_null",
            skip_serializing_if = "Option::is_none"
        )]
        lang: Option<String>,
        text: String,
    },
    Image {
        id: String,
        #[serde(rename = "fileId")]
        file_id: String,
        #[serde(
            default,
            deserialize_with = "non_null",
            skip_serializing_if = "Option::is_none"
        )]
        alt: Option<String>,
    },
    Divider {
        id: String,
    },
    Link {
        id: String,
        label: String,
        target: LinkTarget,
    },
    /// Choose one option (or several with `multiple`); the answer's `inputs[id]` is the option
    /// id, or an array of ids.
    Decision {
        id: String,
        options: Vec<DecisionOption>,
        #[serde(
            default,
            deserialize_with = "non_null",
            skip_serializing_if = "Option::is_none"
        )]
        multiple: Option<bool>,
        #[serde(
            default,
            deserialize_with = "non_null",
            skip_serializing_if = "Option::is_none"
        )]
        default: Option<Vec<String>>,
    },
    /// A form field; the answer's `inputs[id]` is its value.
    Input {
        id: String,
        kind: InputKind,
        label: String,
        #[serde(
            default,
            deserialize_with = "non_null",
            skip_serializing_if = "Option::is_none"
        )]
        placeholder: Option<String>,
        #[serde(
            default,
            deserialize_with = "non_null",
            skip_serializing_if = "Option::is_none"
        )]
        required: Option<bool>,
        #[serde(
            default,
            deserialize_with = "non_null",
            skip_serializing_if = "Option::is_none"
        )]
        options: Option<Vec<InputOption>>,
        #[serde(
            default,
            deserialize_with = "non_null",
            skip_serializing_if = "Option::is_none"
        )]
        min: Option<Number>,
        #[serde(
            default,
            deserialize_with = "non_null",
            skip_serializing_if = "Option::is_none"
        )]
        max: Option<Number>,
        #[serde(
            rename = "maxLength",
            default,
            deserialize_with = "non_null",
            skip_serializing_if = "Option::is_none"
        )]
        max_length: Option<u32>,
        #[serde(
            default,
            deserialize_with = "any_value",
            skip_serializing_if = "Option::is_none"
        )]
        default: Option<Value>,
    },
    /// Markdown the answerer may edit; the answer's `editedValues[id]` is the edited text.
    Draft {
        id: String,
        #[serde(
            default,
            deserialize_with = "non_null",
            skip_serializing_if = "Option::is_none"
        )]
        label: Option<String>,
        text: String,
        #[serde(
            default,
            deserialize_with = "non_null",
            skip_serializing_if = "Option::is_none"
        )]
        editable: Option<bool>,
    },
    /// The live plan of `runId`; `items` is a snapshot.
    Plan {
        id: String,
        #[serde(
            rename = "runId",
            default,
            deserialize_with = "non_null",
            skip_serializing_if = "Option::is_none"
        )]
        run_id: Option<String>,
        #[serde(
            default,
            deserialize_with = "non_null",
            skip_serializing_if = "Option::is_none"
        )]
        items: Option<Vec<RunPlanItem>>,
    },
    Metric {
        id: String,
        label: String,
        value: Number,
        #[serde(
            default,
            deserialize_with = "non_null",
            skip_serializing_if = "Option::is_none"
        )]
        unit: Option<String>,
        #[serde(
            default,
            deserialize_with = "non_null",
            skip_serializing_if = "Option::is_none"
        )]
        delta: Option<Number>,
        #[serde(
            default,
            deserialize_with = "non_null",
            skip_serializing_if = "Option::is_none"
        )]
        better: Option<MetricBetter>,
        #[serde(
            default,
            deserialize_with = "non_null",
            skip_serializing_if = "Option::is_none"
        )]
        note: Option<String>,
    },
    Chart {
        id: String,
        kind: ChartKind,
        #[serde(
            default,
            deserialize_with = "non_null",
            skip_serializing_if = "Option::is_none"
        )]
        label: Option<String>,
        #[serde(
            default,
            deserialize_with = "non_null",
            skip_serializing_if = "Option::is_none"
        )]
        unit: Option<String>,
        #[serde(
            default,
            deserialize_with = "non_null",
            skip_serializing_if = "Option::is_none"
        )]
        x: Option<Vec<String>>,
        series: Vec<ChartSeries>,
    },
    /// A unified diff.
    Diff {
        id: String,
        #[serde(
            default,
            deserialize_with = "non_null",
            skip_serializing_if = "Option::is_none"
        )]
        path: Option<String>,
        text: String,
        #[serde(
            default,
            deserialize_with = "non_null",
            skip_serializing_if = "Option::is_none"
        )]
        additions: Option<u64>,
        #[serde(
            default,
            deserialize_with = "non_null",
            skip_serializing_if = "Option::is_none"
        )]
        deletions: Option<u64>,
    },
    /// One proof chip: a test result, a screenshot, a link.
    Evidence {
        id: String,
        label: String,
        #[serde(
            default,
            deserialize_with = "non_null",
            skip_serializing_if = "Option::is_none"
        )]
        state: Option<EvidenceState>,
        #[serde(
            default,
            deserialize_with = "non_null",
            skip_serializing_if = "Option::is_none"
        )]
        detail: Option<String>,
        #[serde(
            rename = "fileId",
            default,
            deserialize_with = "non_null",
            skip_serializing_if = "Option::is_none"
        )]
        file_id: Option<String>,
        #[serde(
            default,
            deserialize_with = "non_null",
            skip_serializing_if = "Option::is_none"
        )]
        url: Option<String>,
        #[serde(
            default,
            deserialize_with = "non_null",
            skip_serializing_if = "Option::is_none"
        )]
        artifact: Option<ArtifactRef>,
    },
    Ref {
        id: String,
        label: String,
        target: RefTarget,
    },
    /// One level of grouping; children are not groups.
    Group {
        id: String,
        #[serde(
            default,
            deserialize_with = "non_null",
            skip_serializing_if = "Option::is_none"
        )]
        label: Option<String>,
        blocks: Vec<CardV2Block>,
    },
    /// A block type this version does not know. Render nothing.
    #[serde(other)]
    Unknown,
}

impl CardV2Block {
    /// The block id; `None` for [`CardV2Block::Unknown`].
    pub fn id(&self) -> Option<&str> {
        use CardV2Block::*;
        match self {
            Text { id, .. }
            | Fields { id, .. }
            | Table { id, .. }
            | List { id, .. }
            | Progress { id, .. }
            | Code { id, .. }
            | Image { id, .. }
            | Divider { id }
            | Link { id, .. }
            | Decision { id, .. }
            | Input { id, .. }
            | Draft { id, .. }
            | Plan { id, .. }
            | Metric { id, .. }
            | Chart { id, .. }
            | Diff { id, .. }
            | Evidence { id, .. }
            | Ref { id, .. }
            | Group { id, .. } => Some(id),
            Unknown => None,
        }
    }

    /// True for blocks whose value goes into an answer (`decision`, `input`, editable `draft`).
    pub fn takes_input(&self) -> bool {
        match self {
            Self::Decision { .. } | Self::Input { .. } => true,
            Self::Draft { editable, .. } => editable.unwrap_or(false),
            Self::Group { blocks, .. } => blocks.iter().any(Self::takes_input),
            _ => false,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CardActionKind {
    #[default]
    Submit,
    Cancel,
    Link,
    /// A kind this version does not know. Do not show the action.
    #[serde(other)]
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CardV2ActionStyle {
    Primary,
    #[default]
    Default,
    Danger,
    /// A style this version does not know. Render as `default`.
    #[serde(other)]
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CardConfirm {
    pub title: String,
    #[serde(
        default,
        deserialize_with = "non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub body: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CardV2Action {
    pub id: String,
    pub label: String,
    #[serde(
        default,
        deserialize_with = "non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub style: Option<CardV2ActionStyle>,
    /// Absent = submit; see [`CardV2Action::kind`].
    #[serde(
        default,
        deserialize_with = "non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub kind: Option<CardActionKind>,
    /// Required for `link`; absent otherwise.
    #[serde(
        default,
        deserialize_with = "non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub target: Option<RefTarget>,
    #[serde(
        default,
        deserialize_with = "non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub confirm: Option<CardConfirm>,
}

impl CardV2Action {
    pub fn kind(&self) -> CardActionKind {
        self.kind.unwrap_or_default()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CardV2 {
    /// Always 2.
    pub v: CardV2Version,
    /// 1 at first; every edit has a higher rev. Answers name the rev they answer.
    pub rev: u32,
    pub intent: CardIntent,
    pub title: String,
    /// Plain text for old clients, notifications and search.
    pub fallback: String,
    #[serde(
        default,
        deserialize_with = "non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub ask: Option<CardAsk>,
    pub blocks: Vec<CardV2Block>,
    #[serde(
        default,
        deserialize_with = "non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub actions: Option<Vec<CardV2Action>>,
    /// Bot state, at most 16 KiB.
    #[serde(
        default,
        deserialize_with = "any_value",
        skip_serializing_if = "Option::is_none"
    )]
    pub data: Option<Value>,
}

/// Reads an optional `cardV2` leniently: a value that does not parse is dropped.
pub(crate) fn lenient_card_v2<'de, D: Deserializer<'de>>(d: D) -> Result<Option<CardV2>, D::Error> {
    let v = Value::deserialize(d)?;
    Ok(serde_json::from_value(v).ok())
}

impl CardV2 {
    pub fn actions(&self) -> &[CardV2Action] {
        self.actions.as_deref().unwrap_or(&[])
    }
    pub fn is_ask(&self) -> bool {
        self.intent == CardIntent::Ask
    }
    /// Every block, group children after their group, in order.
    pub fn all_blocks(&self) -> Vec<&CardV2Block> {
        let mut out = Vec::new();
        for b in &self.blocks {
            out.push(b);
            if let CardV2Block::Group { blocks, .. } = b {
                out.extend(blocks.iter());
            }
        }
        out
    }
    /// True when the ask has expired at `now`.
    pub fn is_expired(&self, now: Millis) -> bool {
        self.ask
            .as_ref()
            .and_then(|a| a.expires_at)
            .is_some_and(|t| now >= t)
    }

    /// Checks every limit of docs/protocol.md "Cards v2", including the 64 KiB total.
    pub fn validate(&self) -> Validation {
        if self.rev == 0 {
            return fail("cardV2.rev", "must be positive");
        }
        if self.intent == CardIntent::Unknown {
            return fail("cardV2.intent", "unknown intent");
        }
        check_chars("cardV2.title", &self.title, 1, 200)?;
        check_chars("cardV2.fallback", &self.fallback, 0, 4000)?;
        if let Some(a) = &self.ask {
            if a.from.is_empty() || a.from.len() > MAX_ASK_FROM {
                return fail("cardV2.ask.from", format!("1..={MAX_ASK_FROM} entries"));
            }
            for f in &a.from {
                check_chars("cardV2.ask.from", f, 1, 64)?;
            }
            if let Some(i) = &a.irreversible {
                check_chars("cardV2.ask.irreversible.verb", &i.verb, 1, 40)?;
                check_chars("cardV2.ask.irreversible.target", &i.target, 1, 200)?;
                check_chars(
                    "cardV2.ask.irreversible.consequence",
                    &i.consequence,
                    1,
                    500,
                )?;
                if let Some(t) = &i.type_to_confirm {
                    check_chars("cardV2.ask.irreversible.typeToConfirm", t, 1, 100)?;
                }
            }
        }
        let all = self.all_blocks();
        if self.blocks.len() > MAX_CARD_V2_BLOCKS || all.len() > MAX_CARD_V2_BLOCKS {
            return fail(
                "cardV2.blocks",
                format!("at most {MAX_CARD_V2_BLOCKS} blocks"),
            );
        }
        let mut ids = std::collections::BTreeSet::new();
        for b in &all {
            if let Some(id) = b.id()
                && !ids.insert(id)
            {
                return fail("cardV2.blocks", "block ids are unique");
            }
        }
        for b in &self.blocks {
            b.validate(true)?;
        }
        let actions = self.actions();
        if actions.len() > MAX_CARD_V2_ACTIONS {
            return fail(
                "cardV2.actions",
                format!("at most {MAX_CARD_V2_ACTIONS} actions"),
            );
        }
        let mut action_ids = std::collections::BTreeSet::new();
        for a in actions {
            check_action_id("cardV2.actions.id", &a.id)?;
            if !action_ids.insert(a.id.as_str()) {
                return fail("cardV2.actions", "action ids are unique");
            }
            check_chars("cardV2.actions.label", &a.label, 1, 40)?;
            if a.style == Some(CardV2ActionStyle::Unknown) {
                return fail("cardV2.actions.style", "unknown style");
            }
            match (a.kind(), &a.target) {
                (CardActionKind::Unknown, _) => {
                    return fail("cardV2.actions.kind", "unknown kind");
                }
                (CardActionKind::Link, Some(t)) => check_ref("cardV2.actions.target", t)?,
                (CardActionKind::Link, None) | (_, Some(_)) => {
                    return fail(
                        "cardV2.actions",
                        "a link action has a target, other actions have none",
                    );
                }
                _ => {}
            }
            if let Some(c) = &a.confirm {
                check_chars("cardV2.actions.confirm.title", &c.title, 1, 200)?;
                if let Some(b) = &c.body {
                    check_chars("cardV2.actions.confirm.body", b, 0, 1000)?;
                }
            }
        }
        if let Some(d) = &self.data
            && json_bytes(d) > MAX_CARD_DATA_BYTES
        {
            return fail("cardV2.data", "data exceeds 16 KiB");
        }
        if json_bytes(self) > MAX_CARD_V2_BYTES {
            return fail("cardV2", "card exceeds 64 KiB");
        }
        Ok(())
    }
}

fn check_ref(field: &'static str, t: &RefTarget) -> Validation {
    match t {
        RefTarget::Pr {
            url,
            repo,
            number,
            state,
        } => {
            check_web_url(field, url)?;
            if let Some(r) = repo {
                check_chars(field, r, 0, 200)?;
            }
            if *number == Some(0) {
                return fail(field, "number must be positive");
            }
            state
                .as_deref()
                .map_or(Ok(()), |s| check_chars(field, s, 1, 32))
        }
        RefTarget::Artifact {
            artifact_id,
            version,
        } => {
            check_id(field, artifact_id)?;
            if *version == Some(0) {
                return fail(field, "version must be positive");
            }
            Ok(())
        }
        RefTarget::Topic { group_id, topic_id } => {
            check_id(field, group_id)?;
            check_id(field, topic_id)
        }
        RefTarget::Member { member_id } => check_id(field, member_id),
        RefTarget::Url { url } => check_web_url(field, url),
        RefTarget::Unknown => fail(field, "unknown ref kind"),
    }
}

fn finite(field: &'static str, n: &Number) -> Validation {
    match n.as_f64() {
        Some(v) if v.is_finite() => Ok(()),
        _ => fail(field, "must be a finite number"),
    }
}

fn opt_chars(field: &'static str, v: &Option<String>, max: usize) -> Validation {
    v.as_deref()
        .map_or(Ok(()), |s| check_chars(field, s, 0, max))
}

impl CardV2Block {
    fn validate(&self, top: bool) -> Validation {
        use CardV2Block::*;
        if let Some(id) = self.id() {
            check_action_id("cardV2.blocks.id", id)?;
        }
        // The v1 shapes share v1's limits.
        let v1 = |b: crate::card::CardBlock| b.validate();
        match self {
            Text { text, .. } => v1(crate::card::CardBlock::Text { text: text.clone() }),
            Fields { fields, .. } => v1(crate::card::CardBlock::Fields {
                fields: fields.clone(),
            }),
            Table { columns, rows, .. } => v1(crate::card::CardBlock::Table {
                columns: columns.clone(),
                rows: rows.clone(),
            }),
            List { items, .. } => v1(crate::card::CardBlock::List {
                items: items.clone(),
            }),
            Progress { value, label, .. } => v1(crate::card::CardBlock::Progress {
                value: value.clone(),
                label: label.clone(),
            }),
            Code { lang, text, .. } => v1(crate::card::CardBlock::Code {
                lang: lang.clone(),
                text: text.clone(),
            }),
            Image { file_id, alt, .. } => v1(crate::card::CardBlock::Image {
                file_id: file_id.clone(),
                alt: alt.clone(),
            }),
            Divider { .. } => Ok(()),
            Link { label, target, .. } => v1(crate::card::CardBlock::Link {
                label: label.clone(),
                target: target.clone(),
            }),
            Decision {
                options, default, ..
            } => {
                if options.len() < 2 || options.len() > MAX_DECISION_OPTIONS {
                    return fail(
                        "cardV2.blocks.options",
                        format!("2..={MAX_DECISION_OPTIONS} options"),
                    );
                }
                for o in options {
                    check_action_id("cardV2.blocks.options.id", &o.id)?;
                    check_chars("cardV2.blocks.options.label", &o.label, 1, 200)?;
                    opt_chars("cardV2.blocks.options.detail", &o.detail, 1000)?;
                }
                if let Some(d) = default {
                    if d.len() > MAX_DECISION_OPTIONS {
                        return fail("cardV2.blocks.default", "too many");
                    }
                    d.iter()
                        .try_for_each(|x| check_action_id("cardV2.blocks.default", x))?;
                }
                Ok(())
            }
            Input {
                kind,
                label,
                placeholder,
                options,
                min,
                max,
                max_length,
                default,
                ..
            } => {
                if *kind == InputKind::Unknown {
                    return fail("cardV2.blocks.kind", "unknown input kind");
                }
                check_chars("cardV2.blocks.label", label, 1, 200)?;
                opt_chars("cardV2.blocks.placeholder", placeholder, 200)?;
                if let Some(o) = options {
                    if o.len() > MAX_INPUT_OPTIONS {
                        return fail(
                            "cardV2.blocks.options",
                            format!("at most {MAX_INPUT_OPTIONS} options"),
                        );
                    }
                    for x in o {
                        check_action_id("cardV2.blocks.options.id", &x.id)?;
                        check_chars("cardV2.blocks.options.label", &x.label, 1, 200)?;
                    }
                }
                if let Some(n) = min {
                    finite("cardV2.blocks.min", n)?;
                }
                if let Some(n) = max {
                    finite("cardV2.blocks.max", n)?;
                }
                if matches!(max_length, Some(0)) || max_length.is_some_and(|m| m > 32_768) {
                    return fail("cardV2.blocks.maxLength", "1..=32768");
                }
                if let Some(d) = default
                    && json_bytes(d) > MAX_INPUT_DEFAULT_BYTES
                {
                    return fail("cardV2.blocks.default", "default exceeds 4 KiB");
                }
                Ok(())
            }
            Draft { label, text, .. } => {
                opt_chars("cardV2.blocks.label", label, 200)?;
                check_chars("cardV2.blocks.text", text, 0, 32_768)
            }
            Plan { run_id, items, .. } => {
                if let Some(r) = run_id {
                    check_id("cardV2.blocks.runId", r)?;
                }
                items
                    .as_deref()
                    .map_or(Ok(()), |i| check_plan("cardV2.blocks.items", i))
            }
            Metric {
                label,
                value,
                unit,
                delta,
                better,
                note,
                ..
            } => {
                check_chars("cardV2.blocks.label", label, 1, 100)?;
                finite("cardV2.blocks.value", value)?;
                opt_chars("cardV2.blocks.unit", unit, 16)?;
                if let Some(d) = delta {
                    finite("cardV2.blocks.delta", d)?;
                }
                if *better == Some(MetricBetter::Unknown) {
                    return fail("cardV2.blocks.better", "unknown value");
                }
                opt_chars("cardV2.blocks.note", note, 200)
            }
            Chart {
                kind,
                label,
                unit,
                x,
                series,
                ..
            } => {
                if *kind == ChartKind::Unknown {
                    return fail("cardV2.blocks.kind", "unknown chart kind");
                }
                opt_chars("cardV2.blocks.label", label, 200)?;
                opt_chars("cardV2.blocks.unit", unit, 16)?;
                if let Some(x) = x {
                    if x.len() > MAX_CHART_POINTS {
                        return fail("cardV2.blocks.x", format!("at most {MAX_CHART_POINTS}"));
                    }
                    x.iter()
                        .try_for_each(|l| check_chars("cardV2.blocks.x", l, 0, 40))?;
                }
                if series.is_empty() || series.len() > MAX_CHART_SERIES {
                    return fail(
                        "cardV2.blocks.series",
                        format!("1..={MAX_CHART_SERIES} series"),
                    );
                }
                for s in series {
                    check_chars("cardV2.blocks.series.name", &s.name, 1, 100)?;
                    if s.values.len() > MAX_CHART_POINTS {
                        return fail(
                            "cardV2.blocks.series.values",
                            format!("at most {MAX_CHART_POINTS}"),
                        );
                    }
                    s.values
                        .iter()
                        .try_for_each(|v| finite("cardV2.blocks.series.values", v))?;
                }
                Ok(())
            }
            Diff { path, text, .. } => {
                opt_chars("cardV2.blocks.path", path, 1024)?;
                check_chars("cardV2.blocks.text", text, 0, 32_768)
            }
            Evidence {
                label,
                state,
                detail,
                file_id,
                url,
                artifact,
                ..
            } => {
                check_chars("cardV2.blocks.label", label, 1, 200)?;
                if *state == Some(EvidenceState::Unknown) {
                    return fail("cardV2.blocks.state", "unknown state");
                }
                opt_chars("cardV2.blocks.detail", detail, 1000)?;
                if let Some(f) = file_id {
                    check_id("cardV2.blocks.fileId", f)?;
                }
                if let Some(u) = url {
                    check_web_url("cardV2.blocks.url", u)?;
                }
                if let Some(a) = artifact {
                    check_id("cardV2.blocks.artifact.artifactId", &a.artifact_id)?;
                    if a.version == 0 {
                        return fail("cardV2.blocks.artifact.version", "must be positive");
                    }
                }
                Ok(())
            }
            Ref { label, target, .. } => {
                check_chars("cardV2.blocks.label", label, 1, 200)?;
                check_ref("cardV2.blocks.target", target)
            }
            Group { label, blocks, .. } => {
                if !top {
                    return fail("cardV2.blocks", "no group inside a group");
                }
                opt_chars("cardV2.blocks.label", label, 200)?;
                if blocks.is_empty() || blocks.len() > MAX_GROUP_BLOCKS {
                    return fail(
                        "cardV2.blocks.blocks",
                        format!("1..={MAX_GROUP_BLOCKS} blocks"),
                    );
                }
                blocks.iter().try_for_each(|b| b.validate(false))
            }
            Unknown => fail("cardV2.blocks.type", "unknown block type"),
        }
    }
}

/// Values of `decision` and `input` blocks, by block id.
pub type AnswerInputs = BTreeMap<String, Value>;
/// Edited text of `draft` blocks, by block id.
pub type AnswerEdits = BTreeMap<String, String>;

/// `{ t: "card.answer" }`: answer a v2 card. `ack.result` is omitted.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CardAnswerFrame {
    pub rid: String,
    pub group_id: String,
    pub message_id: String,
    /// The `rev` of the card the answer was made on.
    pub rev: u32,
    pub action_id: String,
    #[serde(
        default,
        deserialize_with = "non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub inputs: Option<AnswerInputs>,
    #[serde(
        default,
        deserialize_with = "non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub edited_values: Option<AnswerEdits>,
}

/// Checks answer fields: ids, rev, block-id keys and the 64 KiB total.
pub fn check_answer(
    rev: u32,
    action_id: &str,
    inputs: Option<&AnswerInputs>,
    edited: Option<&AnswerEdits>,
) -> Validation {
    if rev == 0 {
        return fail("rev", "must be positive");
    }
    check_action_id("actionId", action_id)?;
    for k in inputs.into_iter().flat_map(|m| m.keys()) {
        check_action_id("inputs", k)?;
    }
    for (k, v) in edited.into_iter().flat_map(|m| m.iter()) {
        check_action_id("editedValues", k)?;
        check_chars("editedValues", v, 0, 32_768)?;
    }
    if json_bytes(&(inputs, edited)) > MAX_CARD_ANSWER_BYTES {
        return fail("inputs", "answer exceeds 64 KiB");
    }
    Ok(())
}

impl CardAnswerFrame {
    pub fn validate(&self) -> Validation {
        check_id("rid", &self.rid)?;
        check_id("groupId", &self.group_id)?;
        check_id("messageId", &self.message_id)?;
        check_answer(
            self.rev,
            &self.action_id,
            self.inputs.as_ref(),
            self.edited_values.as_ref(),
        )
    }
}

/// Data of `card.answered`. The event's `actorId` is the answerer.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CardAnsweredData {
    pub message_id: String,
    pub rev: u32,
    pub action_id: String,
    #[serde(
        default,
        deserialize_with = "non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub inputs: Option<AnswerInputs>,
    #[serde(
        default,
        deserialize_with = "non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub edited_values: Option<AnswerEdits>,
}
