//! 沉淀 (memory) as the app sees it (docs/protocol.md, "Memory"): what the team and an AI
//! member keep, and the daily 记忆更新. The views read these; the provider maps the wire.

use super::{GroupId, MemberId, Millis};

/// Where a memory lives.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum MemoryScope {
    /// 团队约定: every member reads it; owners write it.
    Team,
    /// An AI member's own lessons and playbooks: owners and that member write them.
    Member(MemberId),
    /// 群里的约定: the group's members read and write it.
    Group(GroupId),
    /// 我的偏好: only that person reads and writes it.
    Person(MemberId),
}

impl MemoryScope {
    /// The wire form: `team`, `member:<id>`, `group:<id>` or `person:<id>`.
    pub fn wire(&self) -> String {
        match self {
            MemoryScope::Team => "team".into(),
            MemoryScope::Member(id) => format!("member:{id}"),
            MemoryScope::Group(id) => format!("group:{id}"),
            MemoryScope::Person(id) => format!("person:{id}"),
        }
    }

    /// The scope of a wire form; `None` for one this version does not know.
    pub fn parse(wire: &str) -> Option<MemoryScope> {
        if wire == "team" {
            return Some(MemoryScope::Team);
        }
        let (layer, id) = wire.split_once(':')?;
        if id.is_empty() {
            return None;
        }
        let id = id.to_string();
        match layer {
            "member" => Some(MemoryScope::Member(id)),
            "group" => Some(MemoryScope::Group(id)),
            "person" => Some(MemoryScope::Person(id)),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum MemoryKind {
    Convention,
    Fact,
    Decision,
    Preference,
    Lesson,
    Playbook,
    /// A kind this version does not know.
    Other,
}

impl MemoryKind {
    /// The kinds a person can pick when they write or edit one.
    pub const PICKABLE: [MemoryKind; 6] = [
        MemoryKind::Convention,
        MemoryKind::Fact,
        MemoryKind::Decision,
        MemoryKind::Preference,
        MemoryKind::Lesson,
        MemoryKind::Playbook,
    ];
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MemoryStatus {
    Active,
    /// 停用.
    Retired,
    /// Written by an AI today; the daily consolidation makes it active.
    Pending,
    Other,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MemorySourceKind {
    Message,
    Run,
    Card,
    Manual,
    Other,
}

/// One thing the team or a member keeps.
#[derive(Clone, Debug, PartialEq)]
pub struct Memory {
    pub id: String,
    pub scope: String,
    pub kind: MemoryKind,
    pub text: String,
    /// When it applies (not a time).
    pub when: Option<String>,
    pub source: MemorySourceKind,
    pub source_ids: Vec<String>,
    /// Evidence, what a merge came from, why it was retired.
    pub note: Option<String>,
    pub author_id: MemberId,
    /// A person wrote or last edited it.
    pub human: bool,
    pub status: MemoryStatus,
    pub uses: u64,
    pub last_used_at: Option<Millis>,
    pub version: u32,
    pub created_at: Millis,
    pub updated_at: Millis,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MemoryChange {
    Added,
    Changed,
    Retired,
    /// The consolidation thinks a person's memory is out of date; nothing changed.
    Flagged,
    Other,
}

/// One line of a day's 记忆更新.
#[derive(Clone, Debug, PartialEq)]
pub struct MemoryUpdate {
    pub id: String,
    pub day: String,
    pub memory_id: String,
    pub change: MemoryChange,
    pub before: Option<Memory>,
    pub after: Option<Memory>,
    pub note: Option<String>,
    pub undone_at: Option<Millis>,
    pub at: Millis,
}

/// A day's 记忆更新 of one scope: the newest day with any, oldest line first.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct MemoryDay {
    /// `YYYY-MM-DD`; `None` before the first consolidation.
    pub day: Option<String>,
    pub updates: Vec<MemoryUpdate>,
}

/// 记一条: a person's memory, active at once.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NewMemory {
    pub scope: MemoryScope,
    pub kind: MemoryKind,
    pub text: String,
    pub when: Option<String>,
}

/// An edit; a `None` field stays as it is (`when: Some(None)` clears it).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MemoryEdit {
    pub kind: Option<MemoryKind>,
    pub text: Option<String>,
    pub when: Option<Option<String>>,
    /// `Active` (恢复, or active at once) or `Retired` (停用).
    pub status: Option<MemoryStatus>,
}

impl MemoryEdit {
    pub fn is_empty(&self) -> bool {
        *self == MemoryEdit::default()
    }
}

/// The longest text and `when` the server takes.
pub const MAX_TEXT_CHARS: usize = 500;
pub const MAX_WHEN_CHARS: usize = 300;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scopes_round_trip_their_wire_form() {
        for scope in [
            MemoryScope::Team,
            MemoryScope::Member("b1".into()),
            MemoryScope::Group("g1".into()),
            MemoryScope::Person("m1".into()),
        ] {
            assert_eq!(MemoryScope::parse(&scope.wire()), Some(scope));
        }
        assert_eq!(MemoryScope::Group("g1".into()).wire(), "group:g1");
        assert_eq!(MemoryScope::Person("m1".into()).wire(), "person:m1");
        assert_eq!(MemoryScope::parse("topic:t1"), None);
        assert_eq!(MemoryScope::parse("group:"), None);
        assert_eq!(MemoryScope::parse("teams"), None);
    }
}
