//! Card v2 (docs/protocol.md, "Cards v2" and "Cards v2.1"): semantic blocks, intents, asks with
//! inputs, answers, and the 2.1 interaction loop (containers, live events, patches, card
//! state). It travels in the optional `Message.cardV2` next to a server-derived v1 `card`.
//!
//! Readers are lenient so that later versions can add values: unknown block types, intents,
//! input kinds, chart kinds, ref kinds and action kinds read as `Unknown` (render nothing, or
//! the safe default), and a `cardV2` that does not parse at all is dropped from the message
//! (the v1 `card` stays). [`CardV2::validate`] (what the server runs) rejects every `Unknown`
//! and checks the 2.1 write rules ([`CardV2::problem`]).

use std::collections::{BTreeMap, BTreeSet, HashMap};

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::{Number, Value};

use crate::bots::{RunPlanItem, check_plan};
use crate::card::{CardField, CardListItem, LinkTarget, check_action_id, check_web_url};
use crate::entities::{Message, Millis};
use crate::limits::{Validation, ValidationError, check_chars, check_id};
use crate::message::ArtifactRef;
use crate::serde_util::{any_value, non_null, nullable};

pub const CARD_V2_VERSION: u32 = 2;
/// Catalog of a card without `catalog`.
pub const CARD_CATALOG_20: &str = "tc.card/2.0";
/// Containers, choice variants, live events, `buttons`, `diagram` and `embed` (1.1.0).
pub const CARD_CATALOG_21: &str = "tc.card/2.1";
/// Catalogs this version reads and writes.
pub const CARD_CATALOGS: [&str; 2] = [CARD_CATALOG_20, CARD_CATALOG_21];
/// Max size of a v2 card: UTF-8 bytes of its JSON text.
pub const MAX_CARD_V2_BYTES: usize = 65_536;
/// Max blocks of a v2 card: leaves and containers, inline group children included.
pub const MAX_CARD_V2_BLOCKS: usize = 80;
pub const MAX_CARD_V2_ACTIONS: usize = 10;
pub const MAX_GROUP_BLOCKS: usize = 20;
pub const MAX_DECISION_OPTIONS: usize = 10;
pub const MAX_INPUT_OPTIONS: usize = 50;
pub const MAX_CHART_SERIES: usize = 5;
pub const MAX_CHART_POINTS: usize = 200;
pub const MAX_ASK_FROM: usize = 20;
/// Max UTF-8 bytes of `data` as JSON.
pub const MAX_CARD_DATA_BYTES: usize = 16_384;
/// Max UTF-8 bytes of `inputs` and `editedValues` of one answer, as JSON together.
pub const MAX_CARD_ANSWER_BYTES: usize = 65_536;
/// Max UTF-8 bytes of an input's `default` as JSON.
pub const MAX_INPUT_DEFAULT_BYTES: usize = 4096;
/// A container has depth <= 2 (a top-level block has depth 0), so a leaf has depth <= 3.
pub const MAX_CONTAINER_DEPTH: usize = 2;
/// Max input blocks of a card.
pub const MAX_CARD_V2_INPUT_BLOCKS: usize = 12;
/// Max blocks that declare live `events`.
pub const MAX_CARD_V2_EVENT_BLOCKS: usize = 20;
pub const MAX_EMPHASIS_HIGH: usize = 2;
pub const MAX_DIAGRAM_BLOCKS: usize = 2;
/// Max revs of one card message (`too_large`, reason `card.rev_limit`).
pub const MAX_CARD_V2_REVS: u32 = 300;
/// Max UTF-8 bytes of a `card.event` value, and of its `inputs`.
pub const MAX_CARD_EVENT_VALUE_BYTES: usize = 4096;
pub const MAX_CARD_EVENT_INPUTS_BYTES: usize = 16_384;
pub const MAX_PATCH_OPS: usize = 50;
pub const MAX_CARD_STATE_ANSWERS: usize = 500;
/// Max entries of `PartRef.keys`.
pub const MAX_PART_KEYS: usize = 4;
/// `ask.from` entry: the topic's creator (a bot that knows a run's requester lists the id).
pub const ASK_FROM_REQUESTER: &str = "requester";
/// `ask.from` entry: every owner.
pub const ASK_FROM_OWNERS: &str = "owners";
/// The event catalog of tc.card/2.1. Unknown names in `events` are kept and ignored.
pub const CARD_EVENT_NAMES: [&str; 15] = [
    "choice.change",
    "choice.submit",
    "input.change",
    "input.submit",
    "chart.select",
    "chart.clear",
    "metric.press",
    "table.rowSelect",
    "table.sort",
    "button.press",
    "draft.edit",
    "compare.pick",
    "tab.open",
    "action.press",
    "answer.retract",
];
/// Part kinds this version knows; the set is open (an unknown kind = the whole block).
pub const PART_KINDS: [&str; 14] = [
    "cell", "row", "column", "point", "bar", "series", "metric", "item", "step", "line", "hunk",
    "image", "node", "element",
];
/// Keys that style a block; rejected at any depth of a block or action
/// (`card.style_property`). `style` on actions and inline buttons and `height` on `embed`
/// are semantic and allowed.
pub const CARD_STYLE_KEYS: [&str; 21] = [
    "style",
    "class",
    "className",
    "css",
    "html",
    "color",
    "colour",
    "background",
    "bg",
    "font",
    "fontSize",
    "fontWeight",
    "padding",
    "margin",
    "gap",
    "width",
    "height",
    "border",
    "radius",
    "shadow",
    "icon",
];

fn fail(field: &'static str, message: impl Into<String>) -> Validation {
    Err(ValidationError {
        field,
        message: message.into(),
    })
}

pub(crate) fn json_bytes(v: &impl Serialize) -> usize {
    serde_json::to_vec(v).map(|b| b.len()).unwrap_or(usize::MAX)
}

/// `[a-z][A-Za-z0-9_.-]{0,63}`: card event names and declared page event names.
pub fn is_event_name(s: &str) -> bool {
    let mut c = s.chars();
    matches!(c.next(), Some('a'..='z'))
        && s.len() <= 64
        && c.all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '_' | '.' | '-'))
}

fn is_lower_word(s: &str) -> bool {
    (1..=16).contains(&s.len()) && s.bytes().all(|b| b.is_ascii_lowercase())
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

// ---------------------------------------------------------------------------
// Parts (引用提问 and part events)
// ---------------------------------------------------------------------------

/// A `PartRef.keys` value: an index (integer >= 0) or a stable key (<= 256 code points).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum PartKey {
    Index(u64),
    Key(String),
}

/// One part of a block: a table cell, a chart point, a plan step, a diagram node, …
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PartRef {
    /// 1..16 of `[a-z]`; open set ([`PART_KINDS`] are the known ones).
    pub kind: String,
    /// At most 4 entries; key names 1..16 of `[a-z]`.
    #[serde(
        default,
        deserialize_with = "non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub keys: Option<BTreeMap<String, PartKey>>,
}

impl PartRef {
    pub fn validate(&self) -> Validation {
        if !is_lower_word(&self.kind) {
            return fail("part.kind", "1..16 of [a-z]");
        }
        if let Some(k) = &self.keys {
            if k.len() > MAX_PART_KEYS {
                return fail("part.keys", format!("at most {MAX_PART_KEYS} keys"));
            }
            for (name, v) in k {
                if !is_lower_word(name) {
                    return fail("part.keys", "key names are 1..16 of [a-z]");
                }
                if let PartKey::Key(s) = v {
                    check_chars("part.keys", s, 0, 256)?;
                }
            }
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Block hints
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum EmitByKeyword {
    Ask,
    Requester,
    Members,
    Owners,
    #[serde(other)]
    Unknown,
}

/// Who may send a block's live events. Answers always follow `ask.from`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum EmitBy {
    Keyword(EmitByKeyword),
    Members {
        #[serde(rename = "memberIds")]
        member_ids: Vec<String>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Emphasis {
    High,
    Low,
    #[serde(other)]
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Deliver {
    /// The block's commit is the answer (no submit button).
    Immediate,
    /// The value goes with a submit or reject action (default).
    OnSubmit,
    #[serde(other)]
    Unknown,
}

/// Fields every block has besides `id` (2.1 hints; they never style).
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BlockBase {
    #[serde(
        default,
        deserialize_with = "non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub emphasis: Option<Emphasis>,
    /// Live events this block sends (`card.event`); absent = none.
    #[serde(
        default,
        deserialize_with = "non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub events: Option<Vec<String>>,
    #[serde(
        default,
        deserialize_with = "non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub deliver: Option<Deliver>,
    #[serde(
        rename = "emitBy",
        default,
        deserialize_with = "non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub emit_by: Option<EmitBy>,
    /// Plain text for readers that do not know the block type. Required on 2.1 leaves.
    #[serde(
        rename = "fallbackText",
        default,
        deserialize_with = "non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub fallback_text: Option<String>,
}

impl BlockBase {
    pub fn events(&self) -> &[String] {
        self.events.as_deref().unwrap_or(&[])
    }
    pub fn is_immediate(&self) -> bool {
        self.deliver == Some(Deliver::Immediate)
    }
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
    /// 2.1: the ask round; default "a1". A new id opens a new round.
    #[serde(
        default,
        deserialize_with = "non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub id: Option<String>,
    /// Member ids, [`ASK_FROM_REQUESTER`] or [`ASK_FROM_OWNERS`].
    pub from: Vec<String>,
    /// 2.1: a blocking ask sets the topic `waiting_answer`. Default: `intent == ask`.
    #[serde(
        default,
        deserialize_with = "non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub blocking: Option<bool>,
    #[serde(
        default,
        deserialize_with = "non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub expires_at: Option<Millis>,
    /// 2.1: what the member promises until the answer.
    #[serde(
        default,
        deserialize_with = "non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub promise: Option<String>,
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
    /// The round id ("a1" when absent).
    pub fn round(&self) -> &str {
        self.id.as_deref().unwrap_or("a1")
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
    /// 2.1: `min`, `max`, `step`, `unit`.
    Slider,
    /// A kind this version does not know (`secret` is refused in 1.1.0). Render as `text`.
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

/// 2.1: overrides the tone the client infers from `better` and `delta`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MetricTone {
    Good,
    Bad,
    Neutral,
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

/// 2.1: a table that is an input (value `{value}` / `{values}` of row keys).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TableSelect {
    Single,
    Multiple,
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
    /// 2.1
    #[serde(
        default,
        deserialize_with = "non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub recommended: Option<bool>,
}

/// 2.1: the "other" choice of a decision (value `{value: "other", other}`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AllowOther {
    pub label: String,
    #[serde(
        default,
        deserialize_with = "non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub placeholder: Option<String>,
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
    /// 2.1: the series id of `chart.select` and part refs; default the name.
    #[serde(
        default,
        deserialize_with = "non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub id: Option<String>,
    pub name: String,
    pub values: Vec<Number>,
}

impl ChartSeries {
    /// `id`, else `name`.
    pub fn key(&self) -> &str {
        self.id.as_deref().unwrap_or(&self.name)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RankItem {
    pub id: String,
    pub label: String,
    #[serde(
        default,
        deserialize_with = "non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub meta: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RankCut {
    pub after: u32,
    pub label: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImageOption {
    pub id: String,
    pub label: String,
    pub file_id: String,
    #[serde(
        default,
        deserialize_with = "non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub recommended: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MatrixRow {
    pub id: String,
    pub label: String,
    #[serde(
        default,
        deserialize_with = "non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub required: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Slot {
    pub id: String,
    pub start: Millis,
    pub end: Millis,
    #[serde(
        default,
        deserialize_with = "non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub conflicts: Option<Vec<String>>,
}

/// `"group"` (every member) or member ids.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum PollVoters {
    Members(Vec<String>),
    Keyword(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ButtonStyle {
    Default,
    Danger,
    #[serde(other)]
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InlineButton {
    pub id: String,
    pub label: String,
    #[serde(
        default,
        deserialize_with = "non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub style: Option<ButtonStyle>,
    #[serde(
        default,
        deserialize_with = "non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub confirm: Option<CardConfirm>,
}

/// A group file image or a 产出 snapshot.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ComparePreview {
    File {
        #[serde(rename = "fileId")]
        file_id: String,
    },
    Artifact {
        #[serde(rename = "artifactId")]
        artifact_id: String,
        version: u32,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CompareOption {
    pub id: String,
    pub label: String,
    #[serde(
        default,
        deserialize_with = "non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub pros: Option<Vec<String>>,
    #[serde(
        default,
        deserialize_with = "non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub cons: Option<Vec<String>>,
    #[serde(
        default,
        deserialize_with = "non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub preview: Option<ComparePreview>,
    #[serde(
        default,
        deserialize_with = "non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub recommended: Option<bool>,
    #[serde(
        default,
        deserialize_with = "non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub children: Option<Vec<String>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CompareNote {
    pub label: String,
    #[serde(
        default,
        deserialize_with = "non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub max_length: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Tab {
    pub id: String,
    pub label: String,
    pub children: Vec<String>,
}

/// One block of a v2 card. Every block has an `id`, unique in the card, and the 2.1 hints in
/// `base`. Containers (`group` with `children`, `stack`, `row`, `compare`, `tabs`,
/// `questionnaire`) name their children by id; the list stays flat.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum CardV2Block {
    Text {
        id: String,
        #[serde(flatten)]
        base: BlockBase,
        text: String,
    },
    Fields {
        id: String,
        #[serde(flatten)]
        base: BlockBase,
        fields: Vec<CardField>,
    },
    Table {
        id: String,
        #[serde(flatten)]
        base: BlockBase,
        columns: Vec<String>,
        rows: Vec<Vec<String>>,
        /// 2.1
        #[serde(
            default,
            deserialize_with = "non_null",
            skip_serializing_if = "Option::is_none"
        )]
        title: Option<String>,
        /// 2.1: one stable key per row; default the row index.
        #[serde(
            rename = "rowKeys",
            default,
            deserialize_with = "non_null",
            skip_serializing_if = "Option::is_none"
        )]
        row_keys: Option<Vec<String>>,
        /// 2.1: the table is an input.
        #[serde(
            default,
            deserialize_with = "non_null",
            skip_serializing_if = "Option::is_none"
        )]
        select: Option<TableSelect>,
        /// 2.1: local sort by header.
        #[serde(
            default,
            deserialize_with = "non_null",
            skip_serializing_if = "Option::is_none"
        )]
        sortable: Option<bool>,
    },
    List {
        id: String,
        #[serde(flatten)]
        base: BlockBase,
        items: Vec<CardListItem>,
    },
    Progress {
        id: String,
        #[serde(flatten)]
        base: BlockBase,
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
        #[serde(flatten)]
        base: BlockBase,
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
        #[serde(flatten)]
        base: BlockBase,
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
        #[serde(flatten)]
        base: BlockBase,
    },
    Link {
        id: String,
        #[serde(flatten)]
        base: BlockBase,
        label: String,
        target: LinkTarget,
    },
    /// Choose one option (or several with `multiple`): CARD21 `choice`. The answer's
    /// `inputs[id]` is the option id, or an array of ids (2.1: `{value}` / `{values}`).
    Decision {
        id: String,
        #[serde(flatten)]
        base: BlockBase,
        /// 2.1: the question.
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
        required: Option<bool>,
        #[serde(
            default,
            deserialize_with = "non_null",
            skip_serializing_if = "Option::is_none"
        )]
        help: Option<String>,
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
        /// 2.1, `multiple` only.
        #[serde(
            rename = "minSelect",
            default,
            deserialize_with = "non_null",
            skip_serializing_if = "Option::is_none"
        )]
        min_select: Option<u32>,
        #[serde(
            rename = "maxSelect",
            default,
            deserialize_with = "non_null",
            skip_serializing_if = "Option::is_none"
        )]
        max_select: Option<u32>,
        #[serde(
            rename = "allowOther",
            default,
            deserialize_with = "non_null",
            skip_serializing_if = "Option::is_none"
        )]
        allow_other: Option<AllowOther>,
    },
    /// A form field; the answer's `inputs[id]` is its value.
    Input {
        id: String,
        #[serde(flatten)]
        base: BlockBase,
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
        help: Option<String>,
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
        /// 2.1, `slider`.
        #[serde(
            default,
            deserialize_with = "non_null",
            skip_serializing_if = "Option::is_none"
        )]
        step: Option<Number>,
        /// 2.1, `slider` and `number`.
        #[serde(
            default,
            deserialize_with = "non_null",
            skip_serializing_if = "Option::is_none"
        )]
        unit: Option<String>,
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
        #[serde(flatten)]
        base: BlockBase,
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
        #[serde(flatten)]
        base: BlockBase,
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
        #[serde(flatten)]
        base: BlockBase,
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
        /// 2.1
        #[serde(
            default,
            deserialize_with = "non_null",
            skip_serializing_if = "Option::is_none"
        )]
        tone: Option<MetricTone>,
        #[serde(
            default,
            deserialize_with = "non_null",
            skip_serializing_if = "Option::is_none"
        )]
        note: Option<String>,
    },
    Chart {
        id: String,
        #[serde(flatten)]
        base: BlockBase,
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
        /// 2.1: one sentence that says what the chart shows.
        #[serde(
            default,
            deserialize_with = "non_null",
            skip_serializing_if = "Option::is_none"
        )]
        summary: Option<String>,
    },
    /// A unified diff.
    Diff {
        id: String,
        #[serde(flatten)]
        base: BlockBase,
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
        #[serde(flatten)]
        base: BlockBase,
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
        #[serde(flatten)]
        base: BlockBase,
        label: String,
        target: RefTarget,
    },
    // --- 2.1 leaves (catalog tc.card/2.1) ---------------------------------
    Rank {
        id: String,
        #[serde(flatten)]
        base: BlockBase,
        label: String,
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
        help: Option<String>,
        items: Vec<RankItem>,
        #[serde(
            default,
            deserialize_with = "non_null",
            skip_serializing_if = "Option::is_none"
        )]
        cut: Option<RankCut>,
        #[serde(
            rename = "rankTop",
            default,
            deserialize_with = "non_null",
            skip_serializing_if = "Option::is_none"
        )]
        rank_top: Option<u32>,
    },
    Rating {
        id: String,
        #[serde(flatten)]
        base: BlockBase,
        label: String,
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
        help: Option<String>,
        /// Always 5.
        max: u32,
        #[serde(
            default,
            deserialize_with = "non_null",
            skip_serializing_if = "Option::is_none"
        )]
        labels: Option<BTreeMap<String, String>>,
    },
    Scale {
        id: String,
        #[serde(flatten)]
        base: BlockBase,
        label: String,
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
        help: Option<String>,
        min: i64,
        max: i64,
        #[serde(
            default,
            deserialize_with = "non_null",
            skip_serializing_if = "Option::is_none"
        )]
        ends: Option<(String, String)>,
    },
    Confirm {
        id: String,
        #[serde(flatten)]
        base: BlockBase,
        label: String,
        yes: String,
        no: String,
        #[serde(
            default,
            deserialize_with = "non_null",
            skip_serializing_if = "Option::is_none"
        )]
        detail: Option<String>,
    },
    ImageChoice {
        id: String,
        #[serde(flatten)]
        base: BlockBase,
        label: String,
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
        help: Option<String>,
        #[serde(
            default,
            deserialize_with = "non_null",
            skip_serializing_if = "Option::is_none"
        )]
        multiple: Option<bool>,
        options: Vec<ImageOption>,
    },
    Matrix {
        id: String,
        #[serde(flatten)]
        base: BlockBase,
        label: String,
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
        help: Option<String>,
        rows: Vec<MatrixRow>,
        columns: Vec<InputOption>,
    },
    Slots {
        id: String,
        #[serde(flatten)]
        base: BlockBase,
        label: String,
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
        help: Option<String>,
        #[serde(
            default,
            deserialize_with = "non_null",
            skip_serializing_if = "Option::is_none"
        )]
        multiple: Option<bool>,
        tz: String,
        slots: Vec<Slot>,
    },
    Poll {
        id: String,
        #[serde(flatten)]
        base: BlockBase,
        label: String,
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
        help: Option<String>,
        options: Vec<InputOption>,
        voters: PollVoters,
        #[serde(rename = "closesAt")]
        closes_at: Millis,
        #[serde(
            default,
            deserialize_with = "non_null",
            skip_serializing_if = "Option::is_none"
        )]
        public: Option<bool>,
        #[serde(
            default,
            deserialize_with = "non_null",
            skip_serializing_if = "Option::is_none"
        )]
        changeable: Option<bool>,
    },
    /// Inline buttons; they send `button.press` and never answer an ask.
    Buttons {
        id: String,
        #[serde(flatten)]
        base: BlockBase,
        buttons: Vec<InlineButton>,
    },
    /// A Mermaid diagram. `syntax` is an open set on read (show `source` as code).
    Diagram {
        id: String,
        #[serde(flatten)]
        base: BlockBase,
        syntax: String,
        source: String,
        #[serde(
            default,
            deserialize_with = "non_null",
            skip_serializing_if = "Option::is_none"
        )]
        title: Option<String>,
        #[serde(
            default,
            deserialize_with = "non_null",
            skip_serializing_if = "Option::is_none"
        )]
        caption: Option<String>,
        #[serde(
            default,
            deserialize_with = "non_null",
            skip_serializing_if = "Option::is_none"
        )]
        alt: Option<String>,
    },
    /// A 产出 version shown live inline; `base.events` names its declared page events.
    Embed {
        id: String,
        #[serde(flatten)]
        base: BlockBase,
        #[serde(rename = "artifactId")]
        artifact_id: String,
        version: u32,
        #[serde(
            default,
            deserialize_with = "non_null",
            skip_serializing_if = "Option::is_none"
        )]
        height: Option<u32>,
        #[serde(
            default,
            deserialize_with = "non_null",
            skip_serializing_if = "Option::is_none"
        )]
        title: Option<String>,
    },
    // --- containers --------------------------------------------------------
    /// A titled section. 2.0: leaves inline in `blocks`. 2.1: children by id in `children`.
    Group {
        id: String,
        #[serde(flatten)]
        base: BlockBase,
        #[serde(
            default,
            deserialize_with = "non_null",
            skip_serializing_if = "Option::is_none"
        )]
        label: Option<String>,
        /// 2.0: inline leaves. `Some(vec![])` is kept so the write rules see it (TS does).
        #[serde(
            default,
            deserialize_with = "non_null",
            skip_serializing_if = "Option::is_none"
        )]
        blocks: Option<Vec<CardV2Block>>,
        #[serde(
            default,
            deserialize_with = "non_null",
            skip_serializing_if = "Option::is_none"
        )]
        children: Option<Vec<String>>,
        #[serde(
            default,
            deserialize_with = "non_null",
            skip_serializing_if = "Option::is_none"
        )]
        title: Option<String>,
        #[serde(
            default,
            deserialize_with = "non_null",
            skip_serializing_if = "Option::is_none"
        )]
        summary: Option<String>,
        #[serde(
            default,
            deserialize_with = "non_null",
            skip_serializing_if = "Option::is_none"
        )]
        collapsible: Option<bool>,
        #[serde(
            default,
            deserialize_with = "non_null",
            skip_serializing_if = "Option::is_none"
        )]
        collapsed: Option<bool>,
    },
    Stack {
        id: String,
        #[serde(flatten)]
        base: BlockBase,
        children: Vec<String>,
    },
    /// Side by side; folds to a stack under 560 pt.
    Row {
        id: String,
        #[serde(flatten)]
        base: BlockBase,
        children: Vec<String>,
        #[serde(
            default,
            deserialize_with = "non_null",
            skip_serializing_if = "Option::is_none"
        )]
        weights: Option<Vec<u8>>,
    },
    /// Options side by side; an input (one pick + note).
    Compare {
        id: String,
        #[serde(flatten)]
        base: BlockBase,
        #[serde(
            default,
            deserialize_with = "non_null",
            skip_serializing_if = "Option::is_none"
        )]
        question: Option<String>,
        options: Vec<CompareOption>,
        #[serde(
            default,
            deserialize_with = "non_null",
            skip_serializing_if = "Option::is_none"
        )]
        note: Option<CompareNote>,
    },
    Tabs {
        id: String,
        #[serde(flatten)]
        base: BlockBase,
        tabs: Vec<Tab>,
        #[serde(
            default,
            deserialize_with = "non_null",
            skip_serializing_if = "Option::is_none"
        )]
        selected: Option<String>,
    },
    /// Numbered questions and one submit.
    Questionnaire {
        id: String,
        #[serde(flatten)]
        base: BlockBase,
        children: Vec<String>,
        #[serde(
            rename = "submitLabel",
            default,
            deserialize_with = "non_null",
            skip_serializing_if = "Option::is_none"
        )]
        submit_label: Option<String>,
        #[serde(
            default,
            deserialize_with = "non_null",
            skip_serializing_if = "Option::is_none"
        )]
        anonymous: Option<bool>,
    },
    /// A block type this version does not know. Render nothing (or `fallbackText`).
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
            | Divider { id, .. }
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
            | Rank { id, .. }
            | Rating { id, .. }
            | Scale { id, .. }
            | Confirm { id, .. }
            | ImageChoice { id, .. }
            | Matrix { id, .. }
            | Slots { id, .. }
            | Poll { id, .. }
            | Buttons { id, .. }
            | Diagram { id, .. }
            | Embed { id, .. }
            | Group { id, .. }
            | Stack { id, .. }
            | Row { id, .. }
            | Compare { id, .. }
            | Tabs { id, .. }
            | Questionnaire { id, .. } => Some(id),
            Unknown => None,
        }
    }

    /// The 2.1 hints; `None` for [`CardV2Block::Unknown`].
    pub fn base(&self) -> Option<&BlockBase> {
        use CardV2Block::*;
        match self {
            Text { base, .. }
            | Fields { base, .. }
            | Table { base, .. }
            | List { base, .. }
            | Progress { base, .. }
            | Code { base, .. }
            | Image { base, .. }
            | Divider { base, .. }
            | Link { base, .. }
            | Decision { base, .. }
            | Input { base, .. }
            | Draft { base, .. }
            | Plan { base, .. }
            | Metric { base, .. }
            | Chart { base, .. }
            | Diff { base, .. }
            | Evidence { base, .. }
            | Ref { base, .. }
            | Rank { base, .. }
            | Rating { base, .. }
            | Scale { base, .. }
            | Confirm { base, .. }
            | ImageChoice { base, .. }
            | Matrix { base, .. }
            | Slots { base, .. }
            | Poll { base, .. }
            | Buttons { base, .. }
            | Diagram { base, .. }
            | Embed { base, .. }
            | Group { base, .. }
            | Stack { base, .. }
            | Row { base, .. }
            | Compare { base, .. }
            | Tabs { base, .. }
            | Questionnaire { base, .. } => Some(base),
            Unknown => None,
        }
    }

    /// The wire `type`; "unknown" for [`CardV2Block::Unknown`].
    pub fn type_name(&self) -> &'static str {
        use CardV2Block::*;
        match self {
            Text { .. } => "text",
            Fields { .. } => "fields",
            Table { .. } => "table",
            List { .. } => "list",
            Progress { .. } => "progress",
            Code { .. } => "code",
            Image { .. } => "image",
            Divider { .. } => "divider",
            Link { .. } => "link",
            Decision { .. } => "decision",
            Input { .. } => "input",
            Draft { .. } => "draft",
            Plan { .. } => "plan",
            Metric { .. } => "metric",
            Chart { .. } => "chart",
            Diff { .. } => "diff",
            Evidence { .. } => "evidence",
            Ref { .. } => "ref",
            Rank { .. } => "rank",
            Rating { .. } => "rating",
            Scale { .. } => "scale",
            Confirm { .. } => "confirm",
            ImageChoice { .. } => "imageChoice",
            Matrix { .. } => "matrix",
            Slots { .. } => "slots",
            Poll { .. } => "poll",
            Buttons { .. } => "buttons",
            Diagram { .. } => "diagram",
            Embed { .. } => "embed",
            Group { .. } => "group",
            Stack { .. } => "stack",
            Row { .. } => "row",
            Compare { .. } => "compare",
            Tabs { .. } => "tabs",
            Questionnaire { .. } => "questionnaire",
            Unknown => "unknown",
        }
    }

    /// True for the types of tc.card/2.1 (not in 2.0).
    pub fn is_21(&self) -> bool {
        matches!(
            self,
            Self::Rank { .. }
                | Self::Rating { .. }
                | Self::Scale { .. }
                | Self::Confirm { .. }
                | Self::ImageChoice { .. }
                | Self::Matrix { .. }
                | Self::Slots { .. }
                | Self::Poll { .. }
                | Self::Buttons { .. }
                | Self::Diagram { .. }
                | Self::Embed { .. }
                | Self::Stack { .. }
                | Self::Row { .. }
                | Self::Compare { .. }
                | Self::Tabs { .. }
                | Self::Questionnaire { .. }
        )
    }

    /// True for containers (a group, stack, row, compare, tabs or questionnaire).
    pub fn is_container(&self) -> bool {
        matches!(
            self,
            Self::Group { .. }
                | Self::Stack { .. }
                | Self::Row { .. }
                | Self::Compare { .. }
                | Self::Tabs { .. }
                | Self::Questionnaire { .. }
        )
    }

    /// Child ids a 2.1 container names (`children`, compare option and tab children).
    pub fn child_ids(&self) -> Vec<&str> {
        match self {
            Self::Group { children, .. } => children
                .as_deref()
                .unwrap_or(&[])
                .iter()
                .map(String::as_str)
                .collect(),
            Self::Stack { children, .. }
            | Self::Row { children, .. }
            | Self::Questionnaire { children, .. } => children.iter().map(String::as_str).collect(),
            Self::Compare { options, .. } => options
                .iter()
                .flat_map(|o| o.children.as_deref().unwrap_or(&[]))
                .map(String::as_str)
                .collect(),
            Self::Tabs { tabs, .. } => tabs
                .iter()
                .flat_map(|t| t.children.iter())
                .map(String::as_str)
                .collect(),
            _ => Vec::new(),
        }
    }

    /// True for blocks whose value goes into an answer or an input event.
    pub fn takes_input(&self) -> bool {
        match self {
            Self::Decision { .. }
            | Self::Input { .. }
            | Self::Rank { .. }
            | Self::Rating { .. }
            | Self::Scale { .. }
            | Self::Confirm { .. }
            | Self::ImageChoice { .. }
            | Self::Matrix { .. }
            | Self::Slots { .. }
            | Self::Poll { .. }
            | Self::Compare { .. } => true,
            Self::Draft { editable, .. } => editable.unwrap_or(false),
            Self::Table { select, .. } => select.is_some(),
            // A group is never an input itself (TS `takesInput`): its leaves are.
            _ => false,
        }
    }

    fn immediate_allowed(&self) -> bool {
        match self {
            Self::Decision { multiple, .. } | Self::ImageChoice { multiple, .. } => {
                !multiple.unwrap_or(false)
            }
            Self::Confirm { .. }
            | Self::Compare { .. }
            | Self::Rating { .. }
            | Self::Scale { .. } => true,
            Self::Input { kind, .. } => matches!(kind, InputKind::Text | InputKind::Number),
            _ => false,
        }
    }

    fn needs_fallback(&self) -> bool {
        self.is_21() && !self.is_container()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CardActionKind {
    #[default]
    Submit,
    Cancel,
    Link,
    /// 2.1: answers no (takes `inputs` like submit).
    Reject,
    /// 2.1: a live event (`action.press`), never an answer.
    Press,
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

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
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
    /// 2.1, `submit` and `reject`: the input blocks whose values go in `inputs`.
    #[serde(
        default,
        deserialize_with = "non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub inputs: Option<Vec<String>>,
}

impl CardV2Action {
    pub fn kind(&self) -> CardActionKind {
        self.kind.unwrap_or_default()
    }
}

/// A Card v2. Deserializing records the first style key of the raw JSON (`validate` refuses
/// it); unknown non-style keys are dropped, as everywhere.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", remote = "Self")]
pub struct CardV2 {
    /// Always 2.
    pub v: CardV2Version,
    /// 2.1: absent = tc.card/2.0.
    #[serde(
        default,
        deserialize_with = "non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub catalog: Option<String>,
    /// 1 at first; every edit has a higher rev. Answers name the rev they answer.
    pub rev: u32,
    pub intent: CardIntent,
    pub title: String,
    /// 2.1: one plain sentence under the title.
    #[serde(
        default,
        deserialize_with = "non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub summary: Option<String>,
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
    /// The first style key the raw JSON had (never on the wire).
    #[serde(skip)]
    pub style_key: Option<String>,
}

impl Serialize for CardV2 {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        CardV2::serialize(self, s)
    }
}

impl<'de> Deserialize<'de> for CardV2 {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let raw = Value::deserialize(d)?;
        let style_key = card_style_key(&raw);
        let mut card = CardV2::deserialize(raw).map_err(serde::de::Error::custom)?;
        card.style_key = style_key;
        Ok(card)
    }
}

fn style_key_in(v: &Value, allow: &[&str]) -> Option<String> {
    match v {
        Value::Array(a) => a.iter().find_map(|x| style_key_in(x, &[])),
        Value::Object(m) => m.iter().find_map(|(k, x)| {
            if CARD_STYLE_KEYS.contains(&k.as_str()) && !allow.contains(&k.as_str()) {
                return Some(k.clone());
            }
            match (k.as_str(), x) {
                // Inline buttons keep their semantic `style`.
                ("buttons", Value::Array(bs)) => {
                    bs.iter().find_map(|b| style_key_in(b, &["style"]))
                }
                _ => style_key_in(x, &[]),
            }
        }),
        _ => None,
    }
}

/// The first style key in a raw card (its blocks and actions), or `None`.
pub fn card_style_key(raw: &Value) -> Option<String> {
    let blocks = raw.get("blocks").and_then(Value::as_array);
    for b in blocks.into_iter().flatten() {
        let allow: &[&str] = if b.get("type").and_then(Value::as_str) == Some("embed") {
            &["height"]
        } else {
            &[]
        };
        if let Some(k) = style_key_in(b, allow) {
            return Some(k);
        }
    }
    let actions = raw.get("actions").and_then(Value::as_array);
    actions
        .into_iter()
        .flatten()
        .find_map(|a| style_key_in(a, &["style"]))
}

/// Reads an optional `cardV2` leniently: a value that does not parse is dropped.
pub(crate) fn lenient_card_v2<'de, D: Deserializer<'de>>(d: D) -> Result<Option<CardV2>, D::Error> {
    let v = Value::deserialize(d)?;
    Ok(serde_json::from_value(v).ok())
}

/// A write rule a card breaks: `reason` goes to `ack.error.reason`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CardProblem {
    pub reason: &'static str,
    pub message: String,
}

impl CardV2 {
    pub fn actions(&self) -> &[CardV2Action] {
        self.actions.as_deref().unwrap_or(&[])
    }
    pub fn is_ask(&self) -> bool {
        self.intent == CardIntent::Ask
    }
    /// `catalog`, else tc.card/2.0.
    pub fn catalog(&self) -> &str {
        self.catalog.as_deref().unwrap_or(CARD_CATALOG_20)
    }
    /// Every block: the top-level list, inline (2.0) group children after their group.
    pub fn all_blocks(&self) -> Vec<&CardV2Block> {
        let mut out = Vec::new();
        for b in &self.blocks {
            out.push(b);
            if let CardV2Block::Group {
                blocks: Some(blocks),
                ..
            } = b
            {
                out.extend(blocks.iter());
            }
        }
        out
    }
    /// A block by id (inline group children included).
    pub fn block(&self, id: &str) -> Option<&CardV2Block> {
        self.all_blocks().into_iter().find(|b| b.id() == Some(id))
    }
    /// Blocks in reading order: top-level blocks not named by a container, each container
    /// followed by its children.
    pub fn reading_order(&self) -> Vec<&CardV2Block> {
        let named: BTreeSet<&str> = self.blocks.iter().flat_map(|b| b.child_ids()).collect();
        let by_id: HashMap<&str, &CardV2Block> = self
            .blocks
            .iter()
            .filter_map(|b| b.id().map(|id| (id, b)))
            .collect();
        let mut out = Vec::new();
        let mut seen = BTreeSet::new();
        fn visit<'a>(
            b: &'a CardV2Block,
            by_id: &HashMap<&str, &'a CardV2Block>,
            seen: &mut BTreeSet<&'a str>,
            out: &mut Vec<&'a CardV2Block>,
        ) {
            if let Some(id) = b.id()
                && !seen.insert(id)
            {
                return;
            }
            out.push(b);
            for k in b.child_ids() {
                if let Some(c) = by_id.get(k) {
                    visit(c, by_id, seen, out);
                }
            }
        }
        for b in &self.blocks {
            if b.id().is_some_and(|id| named.contains(id)) {
                continue;
            }
            visit(b, &by_id, &mut seen, &mut out);
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

    /// Checks every limit of docs/protocol.md "Cards v2" and the 2.1 write rules, including
    /// the 64 KiB total. The message starts with the reason (`card.limits: …`).
    pub fn validate(&self) -> Validation {
        if let Some(p) = self.problem() {
            return fail("cardV2", format!("{}: {}", p.reason, p.message));
        }
        Ok(())
    }

    /// The first write rule the card breaks (shapes, then 2.1 structure), or `None`.
    pub fn problem(&self) -> Option<CardProblem> {
        let bad = |reason: &'static str, message: String| Some(CardProblem { reason, message });
        if let Some(k) = &self.style_key {
            return bad("card.style_property", format!("{k} is a style key"));
        }
        if let Err(e) = self.validate_shapes() {
            let reason = if e.message.contains("unknown block type") {
                "card.unknown_block"
            } else if e.message.contains("secret") {
                "card.secret_deferred"
            } else {
                "card.limits"
            };
            return bad(reason, e.to_string());
        }
        self.structure_problem()
    }

    fn validate_shapes(&self) -> Validation {
        if self.rev == 0 {
            return fail("cardV2.rev", "must be positive");
        }
        if self.intent == CardIntent::Unknown {
            return fail("cardV2.intent", "unknown intent");
        }
        if let Some(c) = &self.catalog {
            check_chars("cardV2.catalog", c, 1, 32)?;
        }
        check_chars("cardV2.title", &self.title, 1, 200)?;
        if let Some(s) = &self.summary {
            check_chars("cardV2.summary", s, 0, 280)?;
        }
        check_chars("cardV2.fallback", &self.fallback, 0, 4000)?;
        if let Some(a) = &self.ask {
            if let Some(id) = &a.id {
                check_action_id("cardV2.ask.id", id)?;
            }
            if a.from.is_empty() || a.from.len() > MAX_ASK_FROM {
                return fail("cardV2.ask.from", format!("1..={MAX_ASK_FROM} entries"));
            }
            for f in &a.from {
                check_chars("cardV2.ask.from", f, 1, 64)?;
            }
            if let Some(p) = &a.promise {
                check_chars("cardV2.ask.promise", p, 1, 200)?;
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
        if self.blocks.len() > MAX_CARD_V2_BLOCKS {
            return fail(
                "cardV2.blocks",
                format!("at most {MAX_CARD_V2_BLOCKS} blocks"),
            );
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
        for a in actions {
            check_action_id("cardV2.actions.id", &a.id)?;
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
            if let Some(i) = &a.inputs {
                if i.len() > MAX_CARD_V2_INPUT_BLOCKS {
                    return fail("cardV2.actions.inputs", "too many");
                }
                i.iter()
                    .try_for_each(|x| check_action_id("cardV2.actions.inputs", x))?;
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

    fn structure_problem(&self) -> Option<CardProblem> {
        let bad = |reason: &'static str, message: String| Some(CardProblem { reason, message });
        let catalog = self.catalog();
        if !CARD_CATALOGS.contains(&catalog) {
            return bad("card.unknown_catalog", format!("unknown catalog {catalog}"));
        }
        let all = self.all_blocks();
        if all.len() > MAX_CARD_V2_BLOCKS {
            return bad(
                "card.limits",
                format!("at most {MAX_CARD_V2_BLOCKS} blocks"),
            );
        }
        let mut by_id: HashMap<&str, &CardV2Block> = HashMap::new();
        for b in &all {
            let id = b.id().unwrap_or_default();
            if by_id.insert(id, b).is_some() {
                return bad("card.limits", "block ids are unique".into());
            }
            if id == "root" || id.starts_with('$') {
                return bad("card.limits", format!("block id {id} is reserved"));
            }
        }
        for b in &all {
            if catalog == CARD_CATALOG_20 && b.is_21() {
                return bad(
                    "card.unknown_block",
                    format!("{} needs catalog {CARD_CATALOG_21}", b.type_name()),
                );
            }
            if b.needs_fallback() && b.base().and_then(|x| x.fallback_text.as_ref()).is_none() {
                return bad(
                    "card.limits",
                    format!(
                        "{} {} needs fallbackText",
                        b.type_name(),
                        b.id().unwrap_or("")
                    ),
                );
            }
            if let Some(p) = b.leaf_problem() {
                return bad(
                    "card.limits",
                    format!("{} {}: {p}", b.type_name(), b.id().unwrap_or("")),
                );
            }
        }
        // Containers: children, parents, counts.
        let top: BTreeSet<&str> = self.blocks.iter().filter_map(CardV2Block::id).collect();
        let mut parent: HashMap<&str, &str> = HashMap::new();
        for b in &self.blocks {
            let id = b.id().unwrap_or_default();
            let kids = b.child_ids();
            let count = |lo: usize, hi: usize, n: usize, what: &str| {
                if n < lo || n > hi {
                    Some(CardProblem {
                        reason: "card.limits",
                        message: format!("{} {id}: {lo}..{hi} {what}", b.type_name()),
                    })
                } else {
                    None
                }
            };
            let p = match b {
                CardV2Block::Group {
                    blocks, children, ..
                } => {
                    if blocks.is_some() == children.is_some() {
                        bad(
                            "card.limits",
                            format!("group {id} has either blocks or children"),
                        )
                    } else if children.is_some() {
                        count(1, 12, kids.len(), "children")
                    } else {
                        count(
                            1,
                            MAX_GROUP_BLOCKS,
                            blocks.as_ref().map_or(0, Vec::len),
                            "blocks",
                        )
                    }
                }
                CardV2Block::Row { weights, .. } => {
                    count(2, 3, kids.len(), "children").or_else(|| {
                        weights
                            .as_ref()
                            .filter(|w| {
                                w.len() != kids.len() || w.iter().any(|x| !(1..=3).contains(x))
                            })
                            .and_then(|_| {
                                bad(
                                    "card.limits",
                                    format!("row {id}: one weight 1..3 per child"),
                                )
                            })
                    })
                }
                CardV2Block::Stack { .. } => count(1, 12, kids.len(), "children"),
                CardV2Block::Questionnaire { .. } => count(2, 5, kids.len(), "children"),
                CardV2Block::Compare { options, .. } => count(2, 4, options.len(), "options"),
                CardV2Block::Tabs { tabs, selected, .. } => count(2, 5, tabs.len(), "tabs")
                    .or_else(|| {
                        tabs.iter()
                            .any(|t| t.children.is_empty() || t.children.len() > 12)
                            .then(|| CardProblem {
                                reason: "card.limits",
                                message: format!("tabs {id}: 1..12 children per tab"),
                            })
                    })
                    .or_else(|| {
                        let ids: BTreeSet<&str> = tabs.iter().map(|t| t.id.as_str()).collect();
                        if ids.len() != tabs.len() {
                            return bad("card.limits", format!("tabs {id}: tab ids are unique"));
                        }
                        selected
                            .as_deref()
                            .filter(|s| !ids.contains(s))
                            .and_then(|_| {
                                bad("card.limits", format!("tabs {id}: selected names no tab"))
                            })
                    }),
                _ => None,
            };
            if p.is_some() {
                return p;
            }
            for k in kids {
                if !top.contains(k) {
                    return bad(
                        "card.limits",
                        format!("{} {id} names no block {k}", b.type_name()),
                    );
                }
                if k == id {
                    return bad("card.depth", format!("{id} contains itself"));
                }
                if parent.insert(k, id).is_some() {
                    return bad("card.limits", format!("block {k} has two parents"));
                }
            }
        }
        // Depth and nesting, walking down from the top-level blocks; a cycle is never reached.
        let mut depth: HashMap<&str, usize> = HashMap::new();
        let mut stack: Vec<(&str, usize, Vec<&str>)> = self
            .blocks
            .iter()
            .filter_map(CardV2Block::id)
            .filter(|id| !parent.contains_key(id))
            .map(|id| (id, 0, Vec::new()))
            .collect();
        stack.reverse();
        while let Some((bid, d, up)) = stack.pop() {
            if depth.contains_key(bid) {
                continue;
            }
            depth.insert(bid, d);
            let b = by_id[bid];
            if b.is_container() && d > MAX_CONTAINER_DEPTH {
                return bad(
                    "card.depth",
                    format!("{} {bid} is nested too deep", b.type_name()),
                );
            }
            let up_types: Vec<&str> = up.iter().map(|u| by_id[u].type_name()).collect();
            let direct = up_types.last().copied();
            if matches!(b, CardV2Block::Row { .. }) && direct == Some("row") {
                return bad("card.limits", format!("row {bid} is inside a row"));
            }
            if matches!(b, CardV2Block::Tabs { .. })
                && up_types
                    .iter()
                    .any(|t| matches!(*t, "tabs" | "row" | "compare"))
            {
                return bad(
                    "card.limits",
                    format!("tabs {bid} is inside tabs, a row or a compare"),
                );
            }
            if matches!(b, CardV2Block::Compare { .. })
                && up_types.iter().any(|t| matches!(*t, "row" | "compare"))
            {
                return bad(
                    "card.limits",
                    format!("compare {bid} is inside a row or a compare"),
                );
            }
            if direct == Some("questionnaire") && b.is_container() {
                return bad("card.limits", "questionnaire children are leaves".into());
            }
            for k in b.child_ids().into_iter().rev() {
                let mut next = up.clone();
                next.push(bid);
                stack.push((k, d + 1, next));
            }
        }
        if let Some(id) = self
            .blocks
            .iter()
            .filter_map(CardV2Block::id)
            .find(|id| !depth.contains_key(id))
        {
            return bad("card.depth", format!("block {id} is in a cycle"));
        }
        // Counts over the card.
        let leaf_inputs = all.iter().filter(|b| b.takes_input()).count();
        if leaf_inputs > MAX_CARD_V2_INPUT_BLOCKS {
            return bad(
                "card.limits",
                format!("at most {MAX_CARD_V2_INPUT_BLOCKS} input blocks"),
            );
        }
        let base = |b: &&CardV2Block| b.base().cloned().unwrap_or_default();
        if all.iter().filter(|b| !base(b).events().is_empty()).count() > MAX_CARD_V2_EVENT_BLOCKS {
            return bad(
                "card.limits",
                format!("at most {MAX_CARD_V2_EVENT_BLOCKS} blocks with events"),
            );
        }
        if all
            .iter()
            .filter(|b| base(b).emphasis == Some(Emphasis::High))
            .count()
            > MAX_EMPHASIS_HIGH
        {
            return bad(
                "card.limits",
                format!("at most {MAX_EMPHASIS_HIGH} blocks with emphasis high"),
            );
        }
        if all
            .iter()
            .filter(|b| matches!(b, CardV2Block::Diagram { .. }))
            .count()
            > MAX_DIAGRAM_BLOCKS
        {
            return bad(
                "card.limits",
                format!("at most {MAX_DIAGRAM_BLOCKS} diagrams"),
            );
        }
        for b in &all {
            let x = base(b);
            let id = b.id().unwrap_or_default();
            if x.deliver.is_some() && !b.takes_input() {
                return bad("card.limits", format!("{id}: deliver is for input blocks"));
            }
            if x.is_immediate() && !b.immediate_allowed() {
                return bad(
                    "card.limits",
                    format!(
                        "{id}: immediate is for single choices, confirm, compare, rating, scale and single-line inputs"
                    ),
                );
            }
        }
        // Actions.
        let actions = self.actions();
        let ids: BTreeSet<&str> = actions.iter().map(|a| a.id.as_str()).collect();
        if ids.len() != actions.len() {
            return bad("card.limits", "action ids are unique".into());
        }
        if actions
            .iter()
            .filter(|a| a.style == Some(CardV2ActionStyle::Primary))
            .count()
            > 1
        {
            return bad("card.limits", "at most one primary action".into());
        }
        for a in actions {
            if let Some(inputs) = &a.inputs {
                if !matches!(a.kind(), CardActionKind::Submit | CardActionKind::Reject) {
                    return bad(
                        "card.limits",
                        format!("action {}: only submit and reject take inputs", a.id),
                    );
                }
                for i in inputs {
                    if !by_id.get(i.as_str()).is_some_and(|b| b.takes_input()) {
                        return bad(
                            "card.limits",
                            format!("action {} names no input block {i}", a.id),
                        );
                    }
                }
            }
        }
        // One decision surface per open ask.
        if self.ask.is_some() {
            let mut surfaces = usize::from(
                actions
                    .iter()
                    .any(|a| a.inputs.as_ref().is_some_and(|i| !i.is_empty())),
            );
            for b in &all {
                if matches!(
                    b,
                    CardV2Block::Compare { .. } | CardV2Block::Questionnaire { .. }
                ) || base(b).is_immediate()
                {
                    surfaces += 1;
                }
            }
            if surfaces > 1 {
                return bad(
                    "card.decision_surfaces",
                    "one decision surface per ask".into(),
                );
            }
        }
        None
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

fn opt_short(field: &'static str, v: &Option<String>, max: usize) -> Validation {
    v.as_deref()
        .map_or(Ok(()), |s| check_chars(field, s, 1, max))
}

fn check_ids_list(field: &'static str, ids: &[String], max: usize) -> Validation {
    if ids.len() > max {
        return fail(field, format!("at most {max}"));
    }
    ids.iter().try_for_each(|x| check_action_id(field, x))
}

fn unique<'a>(ids: impl Iterator<Item = &'a str>) -> bool {
    let mut seen = BTreeSet::new();
    ids.into_iter().all(|x| seen.insert(x))
}

impl BlockBase {
    fn validate(&self) -> Validation {
        if self.emphasis == Some(Emphasis::Unknown) {
            return fail("cardV2.blocks.emphasis", "unknown value");
        }
        if let Some(e) = &self.events {
            if e.len() > 16 {
                return fail("cardV2.blocks.events", "at most 16");
            }
            if !e.iter().all(|n| is_event_name(n)) {
                return fail(
                    "cardV2.blocks.events",
                    "names are [a-z][A-Za-z0-9_.-]{0,63}",
                );
            }
        }
        if self.deliver == Some(Deliver::Unknown) {
            return fail("cardV2.blocks.deliver", "unknown value");
        }
        match &self.emit_by {
            Some(EmitBy::Keyword(EmitByKeyword::Unknown)) => {
                return fail("cardV2.blocks.emitBy", "unknown value");
            }
            Some(EmitBy::Members { member_ids }) => {
                if member_ids.is_empty() || member_ids.len() > MAX_ASK_FROM {
                    return fail(
                        "cardV2.blocks.emitBy",
                        format!("1..={MAX_ASK_FROM} member ids"),
                    );
                }
                member_ids
                    .iter()
                    .try_for_each(|m| check_id("cardV2.blocks.emitBy", m))?;
            }
            _ => {}
        }
        opt_short("cardV2.blocks.fallbackText", &self.fallback_text, 300)
    }
}

impl CardV2Block {
    fn validate(&self, top: bool) -> Validation {
        use CardV2Block::*;
        if let Some(id) = self.id() {
            check_action_id("cardV2.blocks.id", id)?;
        }
        if let Some(b) = self.base() {
            b.validate()?;
        }
        if !top && self.is_container() {
            return fail("cardV2.blocks", "a group's inline blocks are leaves");
        }
        // The v1 shapes share v1's limits.
        let v1 = |b: crate::card::CardBlock| b.validate();
        match self {
            Text { text, .. } => v1(crate::card::CardBlock::Text { text: text.clone() }),
            Fields { fields, .. } => v1(crate::card::CardBlock::Fields {
                fields: fields.clone(),
            }),
            Table {
                columns,
                rows,
                title,
                row_keys,
                select,
                ..
            } => {
                v1(crate::card::CardBlock::Table {
                    columns: columns.clone(),
                    rows: rows.clone(),
                })?;
                opt_short("cardV2.blocks.title", title, 80)?;
                if let Some(k) = row_keys {
                    if k.len() > 200 {
                        return fail("cardV2.blocks.rowKeys", "at most 200");
                    }
                    k.iter()
                        .try_for_each(|x| check_chars("cardV2.blocks.rowKeys", x, 1, 64))?;
                }
                if *select == Some(TableSelect::Unknown) {
                    return fail("cardV2.blocks.select", "unknown value");
                }
                Ok(())
            }
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
                label,
                help,
                options,
                default,
                min_select,
                max_select,
                allow_other,
                ..
            } => {
                opt_short("cardV2.blocks.label", label, 200)?;
                opt_chars("cardV2.blocks.help", help, 300)?;
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
                    check_ids_list("cardV2.blocks.default", d, MAX_DECISION_OPTIONS)?;
                }
                if *min_select == Some(0) || *max_select == Some(0) {
                    return fail("cardV2.blocks.minSelect", "must be positive");
                }
                if let Some(o) = allow_other {
                    check_chars("cardV2.blocks.allowOther.label", &o.label, 1, 40)?;
                    opt_chars("cardV2.blocks.allowOther.placeholder", &o.placeholder, 60)?;
                }
                Ok(())
            }
            Input {
                kind,
                label,
                placeholder,
                help,
                options,
                min,
                max,
                step,
                unit,
                max_length,
                default,
                ..
            } => {
                if *kind == InputKind::Unknown {
                    return fail(
                        "cardV2.blocks.kind",
                        "unknown input kind (secret inputs are not in 1.1.0)",
                    );
                }
                check_chars("cardV2.blocks.label", label, 1, 200)?;
                opt_chars("cardV2.blocks.placeholder", placeholder, 200)?;
                opt_chars("cardV2.blocks.help", help, 300)?;
                opt_chars("cardV2.blocks.unit", unit, 16)?;
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
                if let Some(n) = step {
                    finite("cardV2.blocks.step", n)?;
                    if n.as_f64().is_some_and(|v| v <= 0.0) {
                        return fail("cardV2.blocks.step", "must be positive");
                    }
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
                tone,
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
                if *tone == Some(MetricTone::Unknown) {
                    return fail("cardV2.blocks.tone", "unknown value");
                }
                opt_chars("cardV2.blocks.note", note, 200)
            }
            Chart {
                kind,
                label,
                unit,
                x,
                series,
                summary,
                ..
            } => {
                if *kind == ChartKind::Unknown {
                    return fail("cardV2.blocks.kind", "unknown chart kind");
                }
                opt_chars("cardV2.blocks.label", label, 200)?;
                opt_chars("cardV2.blocks.unit", unit, 16)?;
                opt_chars("cardV2.blocks.summary", summary, 280)?;
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
                    if let Some(id) = &s.id {
                        check_action_id("cardV2.blocks.series.id", id)?;
                    }
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
            Rank {
                label,
                help,
                items,
                cut,
                rank_top,
                ..
            } => {
                check_chars("cardV2.blocks.label", label, 1, 200)?;
                opt_chars("cardV2.blocks.help", help, 300)?;
                if items.len() < 2 || items.len() > 8 {
                    return fail("cardV2.blocks.items", "2..=8 items");
                }
                for i in items {
                    check_action_id("cardV2.blocks.items.id", &i.id)?;
                    check_chars("cardV2.blocks.items.label", &i.label, 1, 200)?;
                    opt_chars("cardV2.blocks.items.meta", &i.meta, 80)?;
                }
                if let Some(c) = cut {
                    if c.after == 0 {
                        return fail("cardV2.blocks.cut.after", "must be positive");
                    }
                    check_chars("cardV2.blocks.cut.label", &c.label, 1, 40)?;
                }
                if *rank_top == Some(0) {
                    return fail("cardV2.blocks.rankTop", "must be positive");
                }
                Ok(())
            }
            Rating {
                label,
                help,
                max,
                labels,
                ..
            } => {
                check_chars("cardV2.blocks.label", label, 1, 200)?;
                opt_chars("cardV2.blocks.help", help, 300)?;
                if *max != 5 {
                    return fail("cardV2.blocks.max", "always 5");
                }
                for (k, v) in labels.iter().flatten() {
                    if !matches!(k.as_str(), "1" | "2" | "3" | "4" | "5") {
                        return fail("cardV2.blocks.labels", "keys are 1..5");
                    }
                    check_chars("cardV2.blocks.labels", v, 1, 20)?;
                }
                Ok(())
            }
            Scale {
                label, help, ends, ..
            } => {
                check_chars("cardV2.blocks.label", label, 1, 200)?;
                opt_chars("cardV2.blocks.help", help, 300)?;
                if let Some((a, b)) = ends {
                    check_chars("cardV2.blocks.ends", a, 1, 20)?;
                    check_chars("cardV2.blocks.ends", b, 1, 20)?;
                }
                Ok(())
            }
            Confirm {
                label,
                yes,
                no,
                detail,
                ..
            } => {
                check_chars("cardV2.blocks.label", label, 1, 200)?;
                check_chars("cardV2.blocks.yes", yes, 1, 20)?;
                check_chars("cardV2.blocks.no", no, 1, 20)?;
                opt_chars("cardV2.blocks.detail", detail, 500)
            }
            ImageChoice {
                label,
                help,
                options,
                ..
            } => {
                check_chars("cardV2.blocks.label", label, 1, 200)?;
                opt_chars("cardV2.blocks.help", help, 300)?;
                if options.len() < 2 || options.len() > 4 {
                    return fail("cardV2.blocks.options", "2..=4 options");
                }
                for o in options {
                    check_action_id("cardV2.blocks.options.id", &o.id)?;
                    check_chars("cardV2.blocks.options.label", &o.label, 1, 80)?;
                    check_id("cardV2.blocks.options.fileId", &o.file_id)?;
                }
                Ok(())
            }
            Matrix {
                label,
                help,
                rows,
                columns,
                ..
            } => {
                check_chars("cardV2.blocks.label", label, 1, 200)?;
                opt_chars("cardV2.blocks.help", help, 300)?;
                if rows.is_empty() || rows.len() > 8 {
                    return fail("cardV2.blocks.rows", "1..=8 rows");
                }
                if columns.len() < 2 || columns.len() > 5 {
                    return fail("cardV2.blocks.columns", "2..=5 columns");
                }
                for r in rows {
                    check_action_id("cardV2.blocks.rows.id", &r.id)?;
                    check_chars("cardV2.blocks.rows.label", &r.label, 1, 80)?;
                }
                for c in columns {
                    check_action_id("cardV2.blocks.columns.id", &c.id)?;
                    check_chars("cardV2.blocks.columns.label", &c.label, 1, 40)?;
                }
                Ok(())
            }
            Slots {
                label,
                help,
                tz,
                slots,
                ..
            } => {
                check_chars("cardV2.blocks.label", label, 1, 200)?;
                opt_chars("cardV2.blocks.help", help, 300)?;
                check_chars("cardV2.blocks.tz", tz, 1, 64)?;
                if slots.is_empty() || slots.len() > 12 {
                    return fail("cardV2.blocks.slots", "1..=12 slots");
                }
                for s in slots {
                    check_action_id("cardV2.blocks.slots.id", &s.id)?;
                    if let Some(c) = &s.conflicts {
                        if c.len() > 50 {
                            return fail("cardV2.blocks.slots.conflicts", "at most 50");
                        }
                        c.iter()
                            .try_for_each(|m| check_id("cardV2.blocks.slots.conflicts", m))?;
                    }
                }
                Ok(())
            }
            Poll {
                label,
                help,
                options,
                voters,
                ..
            } => {
                check_chars("cardV2.blocks.label", label, 1, 200)?;
                opt_chars("cardV2.blocks.help", help, 300)?;
                if options.len() < 2 || options.len() > 6 {
                    return fail("cardV2.blocks.options", "2..=6 options");
                }
                for o in options {
                    check_action_id("cardV2.blocks.options.id", &o.id)?;
                    check_chars("cardV2.blocks.options.label", &o.label, 1, 200)?;
                }
                match voters {
                    PollVoters::Keyword(k) if k == "group" => Ok(()),
                    PollVoters::Keyword(_) => {
                        fail("cardV2.blocks.voters", "\"group\" or member ids")
                    }
                    PollVoters::Members(m) => {
                        if m.is_empty() || m.len() > 200 {
                            return fail("cardV2.blocks.voters", "1..=200 member ids");
                        }
                        m.iter()
                            .try_for_each(|x| check_id("cardV2.blocks.voters", x))
                    }
                }
            }
            Buttons { buttons, .. } => {
                if buttons.is_empty() || buttons.len() > 3 {
                    return fail("cardV2.blocks.buttons", "1..=3 buttons");
                }
                for b in buttons {
                    check_action_id("cardV2.blocks.buttons.id", &b.id)?;
                    check_chars("cardV2.blocks.buttons.label", &b.label, 1, 20)?;
                    if b.style == Some(ButtonStyle::Unknown) {
                        return fail("cardV2.blocks.buttons.style", "unknown style");
                    }
                    if let Some(c) = &b.confirm {
                        check_chars("cardV2.blocks.buttons.confirm.title", &c.title, 1, 200)?;
                        opt_chars("cardV2.blocks.buttons.confirm.body", &c.body, 1000)?;
                    }
                }
                Ok(())
            }
            Diagram {
                syntax,
                source,
                title,
                caption,
                alt,
                ..
            } => {
                check_chars("cardV2.blocks.syntax", syntax, 1, 32)?;
                if source.is_empty() {
                    return fail("cardV2.blocks.source", "must not be empty");
                }
                opt_short("cardV2.blocks.title", title, 80)?;
                opt_short("cardV2.blocks.caption", caption, 280)?;
                opt_short("cardV2.blocks.alt", alt, 1000)
            }
            Embed {
                artifact_id,
                version,
                height,
                title,
                ..
            } => {
                check_id("cardV2.blocks.artifactId", artifact_id)?;
                if *version == 0 {
                    return fail("cardV2.blocks.version", "must be positive");
                }
                if height.is_some_and(|h| !(160..=480).contains(&h)) {
                    return fail("cardV2.blocks.height", "160..=480");
                }
                opt_short("cardV2.blocks.title", title, 80)
            }
            Group {
                label,
                blocks,
                children,
                title,
                summary,
                ..
            } => {
                opt_chars("cardV2.blocks.label", label, 200)?;
                opt_short("cardV2.blocks.title", title, 40)?;
                opt_short("cardV2.blocks.summary", summary, 60)?;
                let blocks = blocks.as_deref().unwrap_or(&[]);
                if blocks.len() > MAX_GROUP_BLOCKS {
                    return fail(
                        "cardV2.blocks.blocks",
                        format!("at most {MAX_GROUP_BLOCKS} blocks"),
                    );
                }
                if let Some(c) = children {
                    check_ids_list("cardV2.blocks.children", c, 12)?;
                }
                blocks.iter().try_for_each(|b| b.validate(false))
            }
            Stack { children, .. } | Questionnaire { children, .. } => {
                check_ids_list("cardV2.blocks.children", children, 12)?;
                if let Questionnaire {
                    submit_label: Some(s),
                    ..
                } = self
                {
                    check_chars("cardV2.blocks.submitLabel", s, 1, 20)?;
                }
                Ok(())
            }
            Row {
                children, weights, ..
            } => {
                check_ids_list("cardV2.blocks.children", children, 12)?;
                if let Some(w) = weights
                    && (w.len() > 3 || w.iter().any(|x| !(1..=3).contains(x)))
                {
                    return fail("cardV2.blocks.weights", "at most 3 weights of 1..3");
                }
                Ok(())
            }
            Compare {
                question,
                options,
                note,
                ..
            } => {
                opt_short("cardV2.blocks.question", question, 200)?;
                if options.len() > 4 {
                    return fail("cardV2.blocks.options", "at most 4 options");
                }
                for o in options {
                    check_action_id("cardV2.blocks.options.id", &o.id)?;
                    check_chars("cardV2.blocks.options.label", &o.label, 1, 24)?;
                    for list in [&o.pros, &o.cons].into_iter().flatten() {
                        if list.len() > 2 {
                            return fail("cardV2.blocks.options.pros", "at most 2");
                        }
                        list.iter().try_for_each(|x| {
                            check_chars("cardV2.blocks.options.pros", x, 1, 40)
                        })?;
                    }
                    match &o.preview {
                        Some(ComparePreview::File { file_id }) => {
                            check_id("cardV2.blocks.options.preview", file_id)?
                        }
                        Some(ComparePreview::Artifact {
                            artifact_id,
                            version,
                        }) => {
                            check_id("cardV2.blocks.options.preview", artifact_id)?;
                            if *version == 0 {
                                return fail(
                                    "cardV2.blocks.options.preview",
                                    "version must be positive",
                                );
                            }
                        }
                        None => {}
                    }
                    if let Some(c) = &o.children {
                        check_ids_list("cardV2.blocks.options.children", c, 6)?;
                    }
                }
                if let Some(n) = note {
                    check_chars("cardV2.blocks.note.label", &n.label, 1, 40)?;
                    if n.max_length.is_some_and(|m| m == 0 || m > 1000) {
                        return fail("cardV2.blocks.note.maxLength", "1..=1000");
                    }
                }
                Ok(())
            }
            Tabs { tabs, selected, .. } => {
                if tabs.len() > 5 {
                    return fail("cardV2.blocks.tabs", "at most 5 tabs");
                }
                for t in tabs {
                    check_action_id("cardV2.blocks.tabs.id", &t.id)?;
                    check_chars("cardV2.blocks.tabs.label", &t.label, 1, 16)?;
                    check_ids_list("cardV2.blocks.tabs.children", &t.children, 12)?;
                }
                if let Some(s) = selected {
                    check_action_id("cardV2.blocks.selected", s)?;
                }
                Ok(())
            }
            Unknown => fail("cardV2.blocks.type", "unknown block type"),
        }
    }

    /// Variant rules of 2.1 leaves (CARD21 §4), checked after the shapes.
    fn leaf_problem(&self) -> Option<String> {
        use CardV2Block::*;
        let msg = |s: &str| Some(s.to_string());
        match self {
            Decision {
                options,
                default,
                multiple,
                min_select,
                max_select,
                ..
            } => {
                let ids: BTreeSet<&str> = options.iter().map(|o| o.id.as_str()).collect();
                if ids.len() != options.len() {
                    return msg("option ids are unique");
                }
                let d = default.as_deref().unwrap_or(&[]);
                if d.iter().any(|x| !ids.contains(x.as_str())) {
                    return msg("default names an unknown option");
                }
                let multiple = multiple.unwrap_or(false);
                if !multiple && d.len() > 1 {
                    return msg("a single choice has at most one default");
                }
                if (min_select.is_some() || max_select.is_some()) && !multiple {
                    return msg("minSelect and maxSelect need multiple");
                }
                let min = min_select.unwrap_or(1) as usize;
                let max = max_select.map_or(options.len(), |m| m as usize);
                (min > max || max > options.len())
                    .then(|| "1 <= minSelect <= maxSelect <= options".to_string())
            }
            Rank {
                items,
                cut,
                rank_top,
                ..
            } => {
                if !unique(items.iter().map(|i| i.id.as_str())) {
                    return msg("item ids are unique");
                }
                if cut
                    .as_ref()
                    .is_some_and(|c| c.after as usize >= items.len())
                {
                    return msg("cut.after is less than the items");
                }
                rank_top
                    .filter(|t| *t as usize > items.len())
                    .map(|_| "rankTop is at most the items".to_string())
            }
            Scale { min, max, .. } => (!(min < max && max - min <= 10))
                .then(|| "min < max and at most 11 steps".to_string()),
            ImageChoice { options, .. } => (!unique(options.iter().map(|o| o.id.as_str())))
                .then(|| "option ids are unique".to_string()),
            Matrix { rows, columns, .. } => {
                if !unique(rows.iter().map(|r| r.id.as_str())) {
                    return msg("row ids are unique");
                }
                (!unique(columns.iter().map(|c| c.id.as_str())))
                    .then(|| "column ids are unique".to_string())
            }
            Slots { slots, .. } => {
                if !unique(slots.iter().map(|s| s.id.as_str())) {
                    return msg("slot ids are unique");
                }
                slots
                    .iter()
                    .any(|s| s.start >= s.end)
                    .then(|| "a slot starts before it ends".to_string())
            }
            Poll { options, .. } => (!unique(options.iter().map(|o| o.id.as_str())))
                .then(|| "option ids are unique".to_string()),
            Buttons { buttons, .. } => (!unique(buttons.iter().map(|b| b.id.as_str())))
                .then(|| "button ids are unique".to_string()),
            Input {
                kind: InputKind::Slider,
                min,
                max,
                ..
            } => match (
                min.as_ref().and_then(Number::as_f64),
                max.as_ref().and_then(Number::as_f64),
            ) {
                (Some(a), Some(b)) if a < b => None,
                _ => msg("a slider has min < max"),
            },
            Table { rows, row_keys, .. } => {
                let k = row_keys.as_deref()?;
                if k.len() != rows.len() {
                    return msg("one row key per row");
                }
                (!unique(k.iter().map(String::as_str))).then(|| "row keys are unique".to_string())
            }
            Chart { series, .. } => (!unique(series.iter().map(ChartSeries::key)))
                .then(|| "series ids are unique".to_string()),
            Compare { options, .. } => (!unique(options.iter().map(|o| o.id.as_str())))
                .then(|| "option ids are unique".to_string()),
            Diagram { syntax, source, .. } => {
                if syntax != "mermaid" {
                    return msg("syntax must be mermaid");
                }
                if source.len() > crate::diagram::MAX_DIAGRAM_SOURCE_BYTES {
                    return Some(format!(
                        "source exceeds {} bytes",
                        crate::diagram::MAX_DIAGRAM_SOURCE_BYTES
                    ));
                }
                match crate::diagram::check_diagram_source(source) {
                    crate::diagram::DiagramCheck::Ok { .. } => None,
                    crate::diagram::DiagramCheck::Refused { reason } => {
                        Some(format!("diagram: {reason}"))
                    }
                }
            }
            _ => None,
        }
    }
}

/// Values of `decision` and `input` blocks, by block id.
pub type AnswerInputs = BTreeMap<String, Value>;
/// Edited text of `draft` blocks, by block id.
pub type AnswerEdits = BTreeMap<String, String>;

/// `{ t: "card.answer" }`: answer a v2 card. Short form of `card.event {event:
/// "action.press"}`; `ack.result` is a [`CardEventAck`] from a 1.1 server.
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
    /// 2.1: idempotency key (24 h).
    #[serde(
        default,
        deserialize_with = "non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub client_event_id: Option<String>,
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

fn check_client_event_id(field: &'static str, v: &str) -> Validation {
    check_chars(field, v, 1, 64)
}

impl CardAnswerFrame {
    pub fn validate(&self) -> Validation {
        check_id("rid", &self.rid)?;
        check_id("groupId", &self.group_id)?;
        check_id("messageId", &self.message_id)?;
        if let Some(c) = &self.client_event_id {
            check_client_event_id("clientEventId", c)?;
        }
        check_answer(
            self.rev,
            &self.action_id,
            self.inputs.as_ref(),
            self.edited_values.as_ref(),
        )
    }
}

/// `{ t: "card.answer.retract" }`: short form of `card.event {event: "answer.retract"}`.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CardAnswerRetractFrame {
    pub rid: String,
    pub group_id: String,
    pub message_id: String,
    pub answer_id: String,
}

impl CardAnswerRetractFrame {
    pub fn validate(&self) -> Validation {
        check_id("rid", &self.rid)?;
        check_id("groupId", &self.group_id)?;
        check_id("messageId", &self.message_id)?;
        check_id("answerId", &self.answer_id)
    }
}

/// `{ t: "card.event" }` (2.1): one interaction with a v2 card. Exactly one of `blockId` /
/// `actionId`, except `answer.retract` (neither); `inputs` only with `action.press`.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CardEventFrame {
    pub rid: String,
    pub group_id: String,
    pub message_id: String,
    /// The rev the person saw.
    pub rev: u32,
    /// A ULID made by the client: the idempotency key.
    pub client_event_id: String,
    #[serde(
        default,
        deserialize_with = "non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub block_id: Option<String>,
    #[serde(
        default,
        deserialize_with = "non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub action_id: Option<String>,
    pub event: String,
    #[serde(
        default,
        deserialize_with = "any_value",
        skip_serializing_if = "Option::is_none"
    )]
    pub value: Option<Value>,
    #[serde(
        default,
        deserialize_with = "non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub inputs: Option<AnswerInputs>,
    /// The part the event targets (`chart.select`: point or bar; `metric.press`: metric).
    #[serde(
        default,
        deserialize_with = "non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub part: Option<PartRef>,
}

/// The event name of a retract.
pub const EVENT_ANSWER_RETRACT: &str = "answer.retract";
/// The event name of a footer action press.
pub const EVENT_ACTION_PRESS: &str = "action.press";

impl CardEventFrame {
    pub fn validate(&self) -> Validation {
        check_id("rid", &self.rid)?;
        check_id("groupId", &self.group_id)?;
        check_id("messageId", &self.message_id)?;
        if self.rev == 0 {
            return fail("rev", "must be positive");
        }
        check_client_event_id("clientEventId", &self.client_event_id)?;
        if !is_event_name(&self.event) {
            return fail("event", "a card event name");
        }
        let one = if self.event == EVENT_ANSWER_RETRACT {
            self.block_id.is_none() && self.action_id.is_none()
        } else {
            self.block_id.is_some() != self.action_id.is_some()
        };
        if !one {
            return fail(
                "blockId",
                "exactly one of blockId and actionId (neither for answer.retract)",
            );
        }
        if let Some(b) = &self.block_id {
            check_action_id("blockId", b)?;
        }
        if let Some(a) = &self.action_id {
            check_action_id("actionId", a)?;
        }
        if self.inputs.is_some() && self.event != EVENT_ACTION_PRESS {
            return fail("inputs", "inputs only with action.press");
        }
        for k in self.inputs.iter().flat_map(|m| m.keys()) {
            check_action_id("inputs", k)?;
        }
        if self
            .value
            .as_ref()
            .is_some_and(|v| json_bytes(v) > MAX_CARD_EVENT_VALUE_BYTES)
        {
            return fail("value", "value exceeds 4 KiB");
        }
        if self
            .inputs
            .as_ref()
            .is_some_and(|v| json_bytes(v) > MAX_CARD_EVENT_INPUTS_BYTES)
        {
            return fail("inputs", "inputs exceed 16 KiB");
        }
        if let Some(p) = &self.part {
            if !part_allowed(&self.event, &p.kind) {
                return fail(
                    "part",
                    "part only with chart.select (point or bar) and metric.press (metric)",
                );
            }
            p.validate()?;
        }
        Ok(())
    }
}

/// V11-INC R3: only `chart.select` (a point or a bar) and `metric.press` (the metric) name a part.
pub fn part_allowed(event: &str, kind: &str) -> bool {
    (event == "chart.select" && (kind == "point" || kind == "bar"))
        || (event == "metric.press" && kind == "metric")
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExpectMode {
    /// An SDK member is connected.
    Live,
    /// A runner member with an active run in the topic.
    Run,
    /// A runner member; a follow-up run starts.
    Starting,
    /// The member is not connected.
    Offline,
    /// The block was not interactive (or a follow-up limit was reached).
    None,
    #[serde(other)]
    Unknown,
}

/// Who responds to an event, and how soon (the server's estimate).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Expect {
    pub by: String,
    pub mode: ExpectMode,
    #[serde(
        default,
        deserialize_with = "non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub eta_ms: Option<u64>,
}

/// `ack.result` of `card.event` (and of `card.answer` from a 1.1 server).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CardEventAck {
    pub event_id: String,
    pub stored: bool,
    #[serde(
        default,
        deserialize_with = "non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub answer_id: Option<String>,
    #[serde(
        default,
        deserialize_with = "non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub duplicate: Option<bool>,
    pub expect: Expect,
}

/// The props of a `card` patch op. `ask: Some(None)` (`null`) closes the open ask.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CardProps {
    #[serde(
        default,
        deserialize_with = "non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub title: Option<String>,
    #[serde(
        default,
        deserialize_with = "non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub summary: Option<String>,
    #[serde(
        default,
        deserialize_with = "non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub intent: Option<CardIntent>,
    #[serde(
        default,
        deserialize_with = "non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub fallback: Option<String>,
    #[serde(
        default,
        deserialize_with = "nullable",
        skip_serializing_if = "Option::is_none"
    )]
    pub ask: Option<Option<CardAsk>>,
    #[serde(
        default,
        deserialize_with = "non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub actions: Option<Vec<CardV2Action>>,
}

/// One op of a `card.patch`. The result is checked as a whole card.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "camelCase")]
pub enum PatchOp {
    /// Shallow merge; cannot change id, type, children, options or tabs.
    Set {
        id: String,
        props: BTreeMap<String, Value>,
    },
    /// Whole block, same id, any type.
    Replace {
        id: String,
        block: CardV2Block,
    },
    /// At most one of after / before / index; default: append. `parent` is a container,
    /// option or tab id.
    Insert {
        block: CardV2Block,
        #[serde(
            default,
            deserialize_with = "non_null",
            skip_serializing_if = "Option::is_none"
        )]
        parent: Option<String>,
        #[serde(
            default,
            deserialize_with = "non_null",
            skip_serializing_if = "Option::is_none"
        )]
        after: Option<String>,
        #[serde(
            default,
            deserialize_with = "non_null",
            skip_serializing_if = "Option::is_none"
        )]
        before: Option<String>,
        #[serde(
            default,
            deserialize_with = "non_null",
            skip_serializing_if = "Option::is_none"
        )]
        index: Option<u32>,
    },
    /// Also removes the id from its parent; children are removed too.
    Remove {
        id: String,
    },
    /// JSON Pointer into `data` ("" = whole); `None` deletes.
    Data {
        path: String,
        #[serde(
            default,
            deserialize_with = "any_value",
            skip_serializing_if = "Option::is_none"
        )]
        value: Option<Value>,
    },
    Card {
        props: CardProps,
    },
}

impl PatchOp {
    pub fn validate(&self) -> Validation {
        match self {
            Self::Set { id, props } => {
                check_action_id("ops.id", id)?;
                if ["id", "type", "children", "options", "tabs"]
                    .iter()
                    .any(|k| props.contains_key(*k))
                {
                    return fail(
                        "ops.props",
                        "set cannot change id, type, children, options or tabs",
                    );
                }
                Ok(())
            }
            Self::Replace { id, block } => {
                check_action_id("ops.id", id)?;
                block.validate(true)
            }
            Self::Insert {
                block,
                parent,
                after,
                before,
                index,
            } => {
                if [after.is_some(), before.is_some(), index.is_some()]
                    .iter()
                    .filter(|x| **x)
                    .count()
                    > 1
                {
                    return fail("ops", "at most one placement");
                }
                for x in [parent, after, before].into_iter().flatten() {
                    check_action_id("ops", x)?;
                }
                block.validate(true)
            }
            Self::Remove { id } => check_action_id("ops.id", id),
            Self::Data { path, .. } => check_chars("ops.path", path, 0, 512),
            Self::Card { props } => {
                opt_short("ops.props.title", &props.title, 200)?;
                opt_chars("ops.props.summary", &props.summary, 280)?;
                opt_chars("ops.props.fallback", &props.fallback, 4000)?;
                if props.intent == Some(CardIntent::Unknown) {
                    return fail("ops.props.intent", "unknown intent");
                }
                if props
                    .actions
                    .as_ref()
                    .is_some_and(|a| a.len() > MAX_CARD_V2_ACTIONS)
                {
                    return fail("ops.props.actions", "too many");
                }
                Ok(())
            }
        }
    }
}

/// `{ t: "card.patch" }` (2.1): the author changes its card in place. `ack.result` is a
/// [`CardPatchAck`].
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CardPatchFrame {
    pub rid: String,
    pub group_id: String,
    pub message_id: String,
    /// Must equal the current rev (`conflict`, `card.stale_rev`).
    pub base_rev: u32,
    pub ops: Vec<PatchOp>,
    /// The event this patch answers.
    #[serde(
        default,
        deserialize_with = "non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub cause: Option<String>,
    #[serde(
        default,
        deserialize_with = "non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub client_patch_id: Option<String>,
}

impl CardPatchFrame {
    pub fn validate(&self) -> Validation {
        check_id("rid", &self.rid)?;
        check_id("groupId", &self.group_id)?;
        check_id("messageId", &self.message_id)?;
        if self.base_rev == 0 {
            return fail("baseRev", "must be positive");
        }
        if self.ops.is_empty() || self.ops.len() > MAX_PATCH_OPS {
            return fail("ops", format!("1..={MAX_PATCH_OPS} ops"));
        }
        self.ops.iter().try_for_each(PatchOp::validate)?;
        if let Some(c) = &self.cause {
            check_id("cause", c)?;
        }
        if let Some(c) = &self.client_patch_id {
            check_client_event_id("clientPatchId", c)?;
        }
        Ok(())
    }
}

/// `ack.result` of `card.patch`: the message at its new rev.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CardPatchAck {
    pub message: Message,
    pub rev: u32,
    #[serde(
        default,
        deserialize_with = "non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub duplicate: Option<bool>,
}

/// How the author answered an event.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InteractionResult {
    Ignored,
    Failed,
    Patched,
    Posted,
    Replied,
    #[serde(other)]
    Unknown,
}

/// `{ t: "interaction.resolve" }` (2.1): the author answers an event without changing
/// anything. `result` is `ignored` or `failed`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InteractionResolveFrame {
    pub rid: String,
    pub group_id: String,
    pub event_id: String,
    pub result: InteractionResult,
    #[serde(
        default,
        deserialize_with = "non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub note: Option<String>,
}

impl InteractionResolveFrame {
    pub fn validate(&self) -> Validation {
        check_id("rid", &self.rid)?;
        check_id("groupId", &self.group_id)?;
        check_id("eventId", &self.event_id)?;
        if !matches!(
            self.result,
            InteractionResult::Ignored | InteractionResult::Failed
        ) {
            return fail("result", "ignored or failed");
        }
        opt_short("note", &self.note, 120)
    }
}

/// Data of `card.answered`. The event's `actorId` is the answerer. 2.1 adds the answer id,
/// the ask round and the event; the commit of an `immediate` block has no `actionId` (it is
/// then `""`) and names `blockId`.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CardAnsweredData {
    pub message_id: String,
    pub rev: u32,
    #[serde(default, skip_serializing_if = "String::is_empty")]
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
    #[serde(
        default,
        deserialize_with = "non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub answer_id: Option<String>,
    #[serde(
        default,
        deserialize_with = "non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub ask_id: Option<String>,
    #[serde(
        default,
        deserialize_with = "non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub event_id: Option<String>,
    #[serde(
        default,
        deserialize_with = "non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub event: Option<String>,
    #[serde(
        default,
        deserialize_with = "non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub block_id: Option<String>,
    #[serde(
        default,
        deserialize_with = "any_value",
        skip_serializing_if = "Option::is_none"
    )]
    pub value: Option<Value>,
}

/// Data of `card.retracted` (2.1): the answerer took an answer back; the ask is open again.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CardRetractedData {
    pub message_id: String,
    pub answer_id: String,
    #[serde(
        default,
        deserialize_with = "non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub ask_id: Option<String>,
    #[serde(
        default,
        deserialize_with = "non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub event_id: Option<String>,
}

/// What an answer or a rev was made for: the event, who sent it, when. Filled by the server.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Cause {
    pub event_id: String,
    pub actor_id: String,
    pub at: Millis,
    pub event: String,
    #[serde(
        default,
        deserialize_with = "non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub block_id: Option<String>,
    #[serde(
        default,
        deserialize_with = "non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub action_id: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AskStatus {
    #[default]
    Open,
    Answered,
    Expired,
    Closed,
    Superseded,
    #[serde(other)]
    Unknown,
}

/// One answer in `cardState.answers`.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CardAnswer {
    pub answer_id: String,
    pub ask_id: String,
    pub event_id: String,
    pub actor_id: String,
    pub at: Millis,
    pub rev: u32,
    pub event: String,
    #[serde(
        default,
        deserialize_with = "non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub block_id: Option<String>,
    #[serde(
        default,
        deserialize_with = "non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub action_id: Option<String>,
    #[serde(
        default,
        deserialize_with = "any_value",
        skip_serializing_if = "Option::is_none"
    )]
    pub value: Option<Value>,
    #[serde(
        default,
        deserialize_with = "non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub inputs: Option<AnswerInputs>,
    /// Draft field ids the answerer changed.
    #[serde(
        default,
        deserialize_with = "non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub edited: Option<Vec<String>>,
    /// sha256 of the answered draft text.
    #[serde(
        default,
        deserialize_with = "non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub content_hash: Option<String>,
    #[serde(
        default,
        deserialize_with = "non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub retracted_at: Option<Millis>,
}

/// `cardState.ask`: the server's view of the open (or last) ask round.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CardStateAsk {
    pub id: String,
    pub status: AskStatus,
    /// Resolved member ids.
    pub from: Vec<String>,
    pub blocking: bool,
    #[serde(
        default,
        deserialize_with = "non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub expires_at: Option<Millis>,
    #[serde(
        default,
        deserialize_with = "non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub answered_by: Option<String>,
    #[serde(
        default,
        deserialize_with = "non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub answered_at: Option<Millis>,
}

/// Server-owned state of a v2 card message (members never write it).
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CardState {
    #[serde(
        default,
        deserialize_with = "non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub ask: Option<CardStateAsk>,
    /// Oldest first.
    pub answers: Vec<CardAnswer>,
    /// Polls: votes per option id; `tallyBy` (public polls) the voters.
    #[serde(
        default,
        deserialize_with = "non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub tally: Option<BTreeMap<String, u64>>,
    #[serde(
        default,
        deserialize_with = "non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub tally_by: Option<BTreeMap<String, Vec<String>>>,
    /// `emitBy: "requester"` resolved on write, by block id.
    #[serde(
        default,
        deserialize_with = "non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub emitters: Option<BTreeMap<String, Vec<String>>>,
    /// The event the newest rev answered.
    #[serde(
        default,
        deserialize_with = "non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub last_cause: Option<Cause>,
}

impl CardState {
    /// The open ask round, if any.
    pub fn open_ask(&self) -> Option<&CardStateAsk> {
        self.ask.as_ref().filter(|a| a.status == AskStatus::Open)
    }
    /// Answers that were not taken back.
    pub fn live_answers(&self) -> impl Iterator<Item = &CardAnswer> {
        self.answers.iter().filter(|a| a.retracted_at.is_none())
    }
}

/// Transient (2.1, to the card's author): a live event of one of its cards.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CardEventTransient {
    pub group_id: String,
    pub topic_id: String,
    pub message_id: String,
    pub event_id: String,
    pub rev: u32,
    #[serde(
        default,
        deserialize_with = "non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub block_id: Option<String>,
    #[serde(
        default,
        deserialize_with = "non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub action_id: Option<String>,
    pub event: String,
    #[serde(
        default,
        deserialize_with = "any_value",
        skip_serializing_if = "Option::is_none"
    )]
    pub value: Option<Value>,
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
    pub part: Option<PartRef>,
    pub actor_id: String,
    pub at: Millis,
}

/// Transient (2.1, to the emitter's connections): the author answered an event.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InteractionResolvedTransient {
    pub group_id: String,
    pub event_id: String,
    pub result: InteractionResult,
    #[serde(
        default,
        deserialize_with = "non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub message_id: Option<String>,
    #[serde(
        default,
        deserialize_with = "non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub note: Option<String>,
}

/// Max decisions in one `GET /v1/groups/:id/decisions` page.
pub const MAX_DECISIONS_PAGE: usize = 200;

/// A group decision (群里的决定): an answer to a v2 ask whose `ask.record` was true at the
/// answered rev. `GET /v1/groups/:id/decisions`, newest first.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GroupDecision {
    /// Seq of the `card.answered` event.
    pub seq: crate::entities::Seq,
    pub message_id: String,
    pub topic_id: String,
    pub rev: u32,
    /// The card title at the answered rev: the question.
    pub title: String,
    pub action_id: String,
    pub action_label: String,
    /// Labels of the chosen options of the card's `decision` blocks, in block order.
    #[serde(
        default,
        deserialize_with = "non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub choices: Option<Vec<String>>,
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
    pub answered_by: String,
    pub answered_at: Millis,
}

/// `GET /v1/groups/:id/decisions?beforeSeq=&limit=`: newest first.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DecisionsResponse {
    pub decisions: Vec<GroupDecision>,
    pub has_more: bool,
}
