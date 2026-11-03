//! 引用提问 (REFS part A): the shared entry point that part affordances call. This is the stub
//! the tracks build against; the inc-refs track implements where the chip goes (§A3), the
//! addressee (§A5) and the label builder (§A2.2).

use gpui::{App, Window};

/// Where the referenced part lives.
#[derive(Clone, Debug, PartialEq)]
pub enum RefSource {
    Message {
        group_id: String,
        topic_id: String,
        message_id: String,
    },
    Artifact {
        group_id: String,
        topic_id: String,
        artifact_id: String,
        version: u32,
    },
}

/// One referenced part (or a whole block when `part` is `None`).
#[derive(Clone, Debug, PartialEq)]
pub struct PartTarget {
    pub source: RefSource,
    /// REFS §A2 grammar: a v2 block id, `v1:<i>`, `md:<i>`, `page` or `card`.
    pub block_id: String,
    pub part: Option<chat_core::proto::card2::PartRef>,
    /// Built with `refs::label` (REFS §A2.2), at most 120 code points.
    pub label: String,
    /// A snapshot of the part's value, at most 2048 bytes as JSON.
    pub value: Option<serde_json::Value>,
    pub card_rev: Option<u64>,
}

/// Ask in the topic (the composer chip) or in a side chat (单独问).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RefIntent {
    Topic,
    SideChat,
}

/// Adds a reference to a part. Stub: logs only (no labels or values in the log).
pub fn ask_about(target: PartTarget, intent: RefIntent, _window: &mut Window, _cx: &mut App) {
    let (kind, id) = match &target.source {
        RefSource::Message { message_id, .. } => ("message", message_id.as_str()),
        RefSource::Artifact { artifact_id, .. } => ("artifact", artifact_id.as_str()),
    };
    tracing::info!(
        target: "refs",
        source = kind,
        id,
        block = %target.block_id,
        part = ?target.part.as_ref().map(|p| p.kind.as_str()),
        intent = ?intent,
        label_chars = target.label.chars().count(),
        "ask about a part"
    );
}
