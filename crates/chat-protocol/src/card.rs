//! Cards: a small declarative UI schema that bots and integrations post in messages
//! (docs/protocol.md, "Cards").
//!
//! Readers accept unknown block types as [`CardBlock::Unknown`] so that new block types can be
//! added later; [`Card::validate`] (what the server runs) rejects them.

use serde::{Deserialize, Serialize};
use serde_json::{Number, Value};

use crate::limits::{Validation, ValidationError, check_chars, check_id};
use crate::serde_util::{any_value, non_null};

/// Max size of a card: UTF-8 bytes of its JSON text.
pub const MAX_CARD_BYTES: usize = 65_536;
pub const MAX_CARD_BLOCKS: usize = 50;
pub const MAX_CARD_ACTIONS: usize = 10;
pub const MAX_TABLE_COLUMNS: usize = 20;
pub const MAX_TABLE_ROWS: usize = 200;
pub const MAX_LIST_ITEMS: usize = 200;
pub const MAX_FIELDS: usize = 50;
/// Max UTF-8 bytes of an action `value` as JSON.
pub const MAX_ACTION_VALUE_BYTES: usize = 4096;

fn fail(field: &'static str, message: impl Into<String>) -> Validation {
    Err(ValidationError {
        field,
        message: message.into(),
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CardTone {
    #[default]
    Neutral,
    Info,
    Success,
    Warning,
    Danger,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CardListStatus {
    Todo,
    InProgress,
    Done,
    Failed,
    Skipped,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ActionStyle {
    Primary,
    #[default]
    Default,
    Danger,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Card {
    /// Always 1.
    pub v: CardV1,
    pub title: String,
    #[serde(
        default,
        deserialize_with = "non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub tone: Option<CardTone>,
    #[serde(
        default,
        deserialize_with = "non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub status: Option<CardStatus>,
    pub blocks: Vec<CardBlock>,
    #[serde(
        default,
        deserialize_with = "non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub actions: Option<Vec<CardAction>>,
}

/// The JSON literal `1`, the card schema version.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct CardV1;

impl Serialize for CardV1 {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_u32(1)
    }
}

impl<'de> Deserialize<'de> for CardV1 {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let v = u32::deserialize(d)?;
        if v == 1 {
            Ok(CardV1)
        } else {
            Err(serde::de::Error::custom(format!(
                "unsupported card version {v}"
            )))
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CardStatus {
    pub label: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum CardBlock {
    /// Markdown.
    Text {
        text: String,
    },
    Fields {
        fields: Vec<CardField>,
    },
    /// Every row has one cell per column.
    Table {
        columns: Vec<String>,
        rows: Vec<Vec<String>>,
    },
    List {
        items: Vec<CardListItem>,
    },
    /// `value` is 0..=1; read it with [`CardBlock::progress`].
    Progress {
        value: Number,
        #[serde(
            default,
            deserialize_with = "non_null",
            skip_serializing_if = "Option::is_none"
        )]
        label: Option<String>,
    },
    Code {
        #[serde(
            default,
            deserialize_with = "non_null",
            skip_serializing_if = "Option::is_none"
        )]
        lang: Option<String>,
        text: String,
    },
    Image {
        #[serde(rename = "fileId")]
        file_id: String,
        #[serde(
            default,
            deserialize_with = "non_null",
            skip_serializing_if = "Option::is_none"
        )]
        alt: Option<String>,
    },
    Divider,
    Link {
        label: String,
        target: LinkTarget,
    },
    /// A block type this version does not know. Render nothing.
    #[serde(other)]
    Unknown,
}

impl CardBlock {
    /// The fraction of a progress block, clamped to 0..=1.
    pub fn progress(&self) -> Option<f64> {
        match self {
            Self::Progress { value, .. } => value.as_f64().map(|v| v.clamp(0.0, 1.0)),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CardField {
    pub label: String,
    pub value: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CardListItem {
    pub text: String,
    #[serde(
        default,
        deserialize_with = "non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub status: Option<CardListStatus>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum LinkTarget {
    /// http or https only.
    Url { url: String },
    Topic {
        #[serde(rename = "groupId")]
        group_id: String,
        #[serde(rename = "topicId")]
        topic_id: String,
    },
    Artifact {
        #[serde(rename = "artifactId")]
        artifact_id: String,
        /// Absent: the current version.
        #[serde(
            default,
            deserialize_with = "non_null",
            skip_serializing_if = "Option::is_none"
        )]
        version: Option<u32>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CardAction {
    /// Letters, digits and `_ . : -`, 1..=64.
    pub id: String,
    pub label: String,
    #[serde(
        default,
        deserialize_with = "non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub style: Option<ActionStyle>,
    /// When set, ask this question before sending the action.
    #[serde(
        default,
        deserialize_with = "non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub confirm: Option<String>,
    /// Any JSON, at most 4 KiB.
    #[serde(
        default,
        deserialize_with = "any_value",
        skip_serializing_if = "Option::is_none"
    )]
    pub value: Option<Value>,
}

/// Letters, digits and `_ . : -`, 1..=64 characters.
pub fn check_action_id(field: &'static str, id: &str) -> Validation {
    check_chars(field, id, 1, 64)?;
    if id
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | ':' | '-'))
    {
        Ok(())
    } else {
        fail(field, "only letters, digits and _ . : -")
    }
}

/// http or https URL without whitespace, at most 2048 characters.
pub fn check_web_url(field: &'static str, url: &str) -> Validation {
    check_chars(field, url, 1, 2048)?;
    let rest = url
        .strip_prefix("https://")
        .or_else(|| url.strip_prefix("http://"));
    match rest {
        Some(r) if !r.is_empty() && !r.chars().any(char::is_whitespace) => Ok(()),
        _ => fail(field, "must be an http or https URL"),
    }
}

fn json_bytes(v: &impl Serialize) -> usize {
    serde_json::to_vec(v).map(|b| b.len()).unwrap_or(usize::MAX)
}

impl Card {
    /// Checks every limit of docs/protocol.md "Cards", including the 64 KiB total.
    pub fn validate(&self) -> Validation {
        check_chars("card.title", &self.title, 1, 200)?;
        if let Some(s) = &self.status {
            check_chars("card.status.label", &s.label, 1, 40)?;
        }
        if self.blocks.len() > MAX_CARD_BLOCKS {
            return fail("card.blocks", format!("at most {MAX_CARD_BLOCKS} blocks"));
        }
        for b in &self.blocks {
            b.validate()?;
        }
        if let Some(actions) = &self.actions {
            if actions.len() > MAX_CARD_ACTIONS {
                return fail(
                    "card.actions",
                    format!("at most {MAX_CARD_ACTIONS} actions"),
                );
            }
            let mut ids = std::collections::BTreeSet::new();
            for a in actions {
                check_action_id("card.actions.id", &a.id)?;
                if !ids.insert(a.id.as_str()) {
                    return fail("card.actions", "action ids are unique");
                }
                check_chars("card.actions.label", &a.label, 1, 40)?;
                if let Some(c) = &a.confirm {
                    check_chars("card.actions.confirm", c, 1, 200)?;
                }
                if let Some(v) = &a.value
                    && json_bytes(v) > MAX_ACTION_VALUE_BYTES
                {
                    return fail("card.actions.value", "value exceeds 4 KiB");
                }
            }
        }
        if json_bytes(self) > MAX_CARD_BYTES {
            return fail("card", "card exceeds 64 KiB");
        }
        Ok(())
    }
}

impl CardBlock {
    pub fn validate(&self) -> Validation {
        match self {
            Self::Text { text } => check_chars("card.blocks.text", text, 0, 8000),
            Self::Fields { fields } => {
                if fields.is_empty() || fields.len() > MAX_FIELDS {
                    return fail("card.blocks.fields", format!("1..={MAX_FIELDS} fields"));
                }
                fields.iter().try_for_each(|f| {
                    check_chars("card.blocks.fields.label", &f.label, 1, 100)?;
                    check_chars("card.blocks.fields.value", &f.value, 0, 1000)
                })
            }
            Self::Table { columns, rows } => {
                if columns.is_empty() || columns.len() > MAX_TABLE_COLUMNS {
                    return fail(
                        "card.blocks.columns",
                        format!("1..={MAX_TABLE_COLUMNS} columns"),
                    );
                }
                columns
                    .iter()
                    .try_for_each(|c| check_chars("card.blocks.columns", c, 0, 100))?;
                if rows.len() > MAX_TABLE_ROWS {
                    return fail("card.blocks.rows", format!("at most {MAX_TABLE_ROWS} rows"));
                }
                for row in rows {
                    if row.len() != columns.len() {
                        return fail("card.blocks.rows", "every row has one cell per column");
                    }
                    row.iter()
                        .try_for_each(|c| check_chars("card.blocks.rows", c, 0, 1000))?;
                }
                Ok(())
            }
            Self::List { items } => {
                if items.is_empty() || items.len() > MAX_LIST_ITEMS {
                    return fail("card.blocks.items", format!("1..={MAX_LIST_ITEMS} items"));
                }
                items
                    .iter()
                    .try_for_each(|i| check_chars("card.blocks.items.text", &i.text, 1, 1000))
            }
            Self::Progress { value, label } => {
                match value.as_f64() {
                    Some(v) if (0.0..=1.0).contains(&v) => {}
                    _ => return fail("card.blocks.value", "must be 0..=1"),
                }
                label
                    .as_deref()
                    .map_or(Ok(()), |l| check_chars("card.blocks.label", l, 0, 200))
            }
            Self::Code { lang, text } => {
                if let Some(l) = lang {
                    check_chars("card.blocks.lang", l, 0, 32)?;
                }
                check_chars("card.blocks.text", text, 0, 32_768)
            }
            Self::Image { file_id, alt } => {
                check_id("card.blocks.fileId", file_id)?;
                alt.as_deref()
                    .map_or(Ok(()), |a| check_chars("card.blocks.alt", a, 0, 500))
            }
            Self::Divider => Ok(()),
            Self::Link { label, target } => {
                check_chars("card.blocks.label", label, 1, 200)?;
                match target {
                    LinkTarget::Url { url } => check_web_url("card.blocks.target.url", url),
                    LinkTarget::Topic { group_id, topic_id } => {
                        check_id("card.blocks.target.groupId", group_id)?;
                        check_id("card.blocks.target.topicId", topic_id)
                    }
                    LinkTarget::Artifact {
                        artifact_id,
                        version,
                    } => {
                        check_id("card.blocks.target.artifactId", artifact_id)?;
                        if *version == Some(0) {
                            return fail("card.blocks.target.version", "must be positive");
                        }
                        Ok(())
                    }
                }
            }
            Self::Unknown => fail("card.blocks.type", "unknown block type"),
        }
    }
}
