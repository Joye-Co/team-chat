//! 沉淀 (memory): what the team, a group, an AI member and a person keep for later
//! (docs/protocol.md, "Memory"; docs/design/agent-harness.zh.md section 7).

use serde::{Deserialize, Serialize};

use crate::entities::Millis;
use crate::limits::{Validation, ValidationError, check_chars, check_id};
use crate::serde_util::nullable;

/// Max characters of a memory's text.
pub const MAX_MEMORY_CHARS: usize = 500;
/// Max characters of `when`.
pub const MAX_MEMORY_WHEN_CHARS: usize = 300;
/// Max characters of a memory's note.
pub const MAX_MEMORY_NOTE_CHARS: usize = 500;

fn invalid(field: &'static str, message: &str) -> Validation {
    Err(ValidationError {
        field,
        message: message.into(),
    })
}

/// `team`, `group:<id>`, `member:<id>` or `person:<id>` (ids of `[A-Za-z0-9_-]`).
pub fn check_memory_scope(scope: &str) -> Validation {
    if scope == "team" {
        return Ok(());
    }
    let ok = scope.len() <= 200
        && scope.split_once(':').is_some_and(|(layer, id)| {
            matches!(layer, "group" | "member" | "person")
                && !id.is_empty()
                && id
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
        });
    if ok {
        Ok(())
    } else {
        invalid("scope", "team, group:<id>, member:<id> or person:<id>")
    }
}

fn check_text(field: &'static str, text: &str, min: usize, max: usize) -> Validation {
    check_chars(field, text.trim(), min, max)
}

fn check_kind(kind: MemoryKind) -> Validation {
    if kind == MemoryKind::Unknown {
        return invalid(
            "kind",
            "convention, fact, decision, preference, lesson or playbook",
        );
    }
    Ok(())
}

fn check_day(day: &str) -> Validation {
    let b = day.as_bytes();
    let ok = b.len() == 10
        && b.iter().enumerate().all(|(i, c)| {
            if i == 4 || i == 7 {
                *c == b'-'
            } else {
                c.is_ascii_digit()
            }
        });
    if ok {
        Ok(())
    } else {
        invalid("day", "YYYY-MM-DD")
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MemoryKind {
    Convention,
    Fact,
    Decision,
    Preference,
    Lesson,
    Playbook,
    /// A value this version does not know.
    #[serde(other)]
    Unknown,
}

/// `pending`: written by an AI, waits for the daily consolidation. Kinds, statuses, source
/// kinds and changes may grow: an unknown value reads as `Unknown`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MemoryStatus {
    Active,
    Retired,
    Pending,
    /// A value this version does not know.
    #[serde(other)]
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MemorySourceKind {
    Message,
    Run,
    Card,
    Manual,
    /// A value this version does not know.
    #[serde(other)]
    Unknown,
}

/// Where a memory came from (来龙去脉).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MemorySource {
    pub kind: MemorySourceKind,
    pub ids: Vec<String>,
}

impl MemorySource {
    pub fn validate(&self) -> Validation {
        if self.kind == MemorySourceKind::Unknown {
            return invalid("source.kind", "message, run, card or manual");
        }
        if self.ids.len() > 20 {
            return invalid("source.ids", "at most 20");
        }
        self.ids
            .iter()
            .try_for_each(|id| check_id("source.ids", id))
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MemoryItem {
    pub id: String,
    pub scope: String,
    pub kind: MemoryKind,
    pub text: String,
    /// When it applies.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub when: Option<String>,
    pub source: MemorySource,
    /// Evidence, the items a promotion or merge came from, why it was retired.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
    pub author_id: String,
    /// A person wrote (or last edited) it: the daily consolidation never changes it.
    pub human: bool,
    pub status: MemoryStatus,
    pub confidence: f64,
    pub uses: u64,
    pub last_used_at: Option<Millis>,
    pub version: u32,
    pub created_at: Millis,
    pub updated_at: Millis,
    /// An AI member's memory learned in a group: the group's id (`"unknown"`: noted before the
    /// group was recorded). Only people in that group read it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub from_group: Option<String>,
}

impl MemoryItem {
    pub fn validate(&self) -> Validation {
        check_id("id", &self.id)?;
        check_memory_scope(&self.scope)?;
        check_chars("text", &self.text, 1, MAX_MEMORY_CHARS)?;
        if let Some(w) = &self.when {
            check_chars("when", w, 0, MAX_MEMORY_WHEN_CHARS)?;
        }
        if let Some(n) = &self.note {
            check_chars("note", n, 0, MAX_MEMORY_NOTE_CHARS)?;
        }
        self.source.validate()?;
        if !(0.0..=1.0).contains(&self.confidence) {
            return invalid("confidence", "0..1");
        }
        if self.version < 1 {
            return invalid("version", "at least 1");
        }
        if let Some(g) = &self.from_group {
            check_chars("fromGroup", g, 1, 64)?;
        }
        Ok(())
    }
}

/// `POST /v1/memories`: a person's memory is active at once; an AI member's waits as pending.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateMemoryRequest {
    pub scope: String,
    pub kind: MemoryKind,
    pub text: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub when: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<MemorySource>,
}

impl CreateMemoryRequest {
    pub fn validate(&self) -> Validation {
        check_memory_scope(&self.scope)?;
        check_kind(self.kind)?;
        check_text("text", &self.text, 1, MAX_MEMORY_CHARS)?;
        if let Some(w) = &self.when {
            check_text("when", w, 0, MAX_MEMORY_WHEN_CHARS)?;
        }
        if let Some(s) = &self.source {
            s.validate()?;
        }
        Ok(())
    }
}

/// `PATCH /v1/memories/:id`: a new version. `when: Some(None)` clears it.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateMemoryRequest {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<MemoryKind>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    #[serde(
        default,
        deserialize_with = "nullable",
        skip_serializing_if = "Option::is_none"
    )]
    pub when: Option<Option<String>>,
    /// `active` or `retired` (a person never makes a memory pending).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<MemoryStatus>,
}

impl UpdateMemoryRequest {
    pub fn validate(&self) -> Validation {
        if let Some(t) = &self.text {
            check_text("text", t, 1, MAX_MEMORY_CHARS)?;
        }
        if let Some(Some(w)) = &self.when {
            check_text("when", w, 0, MAX_MEMORY_WHEN_CHARS)?;
        }
        if let Some(k) = self.kind {
            check_kind(k)?;
        }
        if matches!(
            self.status,
            Some(MemoryStatus::Pending | MemoryStatus::Unknown)
        ) {
            return invalid("status", "active or retired");
        }
        Ok(())
    }
}

/// `GET /v1/memories?scope=&status=&q=&limit=`
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MemoriesResponse {
    pub memories: Vec<MemoryItem>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MemoryVersion {
    pub memory_id: String,
    pub version: u32,
    /// The memory as this version left it.
    pub item: MemoryItem,
    /// Who made the version; `None` = the daily consolidation.
    pub by: Option<String>,
    pub at: Millis,
}

/// `GET /v1/memories/:id/versions`, newest first.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MemoryVersionsResponse {
    pub versions: Vec<MemoryVersion>,
}

/// `flagged`: the consolidation thinks a person's memory is out of date; nothing changed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MemoryChange {
    Added,
    Changed,
    Retired,
    Flagged,
    /// A value this version does not know.
    #[serde(other)]
    Unknown,
}

/// One line of a day's 记忆更新.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MemoryUpdate {
    pub id: String,
    /// The team's local day of the consolidation, `YYYY-MM-DD`.
    pub day: String,
    pub scope: String,
    pub memory_id: String,
    pub change: MemoryChange,
    pub before: Option<MemoryItem>,
    pub after: Option<MemoryItem>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
    pub undone_at: Option<Millis>,
    pub at: Millis,
}

/// `GET /v1/memories/updates?scope=&day=`, oldest first.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MemoryUpdatesResponse {
    /// The day shown: the asked one, or the newest day with updates (`None` = none yet).
    pub day: Option<String>,
    pub updates: Vec<MemoryUpdate>,
}

impl MemoryUpdatesResponse {
    pub fn validate(&self) -> Validation {
        if let Some(d) = &self.day {
            check_day(d)?;
        }
        for u in &self.updates {
            check_day(&u.day)?;
            check_memory_scope(&u.scope)?;
        }
        Ok(())
    }
}

/// The layer a run writes to: its own member, the requester (person) or the run's group.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MemoryNoteScope {
    Member,
    Person,
    Group,
}

/// `POST /v1/machine/runs/:id/memories` (`chat-run memory note`): a pending memory with the
/// run as its source.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MachineMemoryNoteRequest {
    pub scope: MemoryNoteScope,
    pub kind: MemoryKind,
    pub text: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub when: Option<String>,
    /// `group`: the person's message the convention comes from; without one the note goes to
    /// the member's own memory.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_message_id: Option<String>,
}

impl MachineMemoryNoteRequest {
    pub fn validate(&self) -> Validation {
        check_kind(self.kind)?;
        check_text("text", &self.text, 1, MAX_MEMORY_CHARS)?;
        if let Some(w) = &self.when {
            check_text("when", w, 0, MAX_MEMORY_WHEN_CHARS)?;
        }
        if let Some(m) = &self.source_message_id {
            check_id("sourceMessageId", m)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scopes() {
        for ok in ["team", "group:01ABC", "member:a_b-c", "person:X"] {
            assert!(check_memory_scope(ok).is_ok(), "{ok}");
        }
        for bad in [
            "",
            "group:",
            "everyone",
            "team:1",
            "person:a b",
            "member:a:b",
        ] {
            assert!(check_memory_scope(bad).is_err(), "{bad}");
        }
        assert!(check_day("2026-11-09").is_ok());
        assert!(check_day("2026-1-09").is_err());
    }
}
