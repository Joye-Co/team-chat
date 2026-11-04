//! 产出 page host types (inc-host track): what a version's page may do, and what comes
//! back from the server for it (INTERACT §5, R10).

use serde_json::Value;

use super::{ChatProvider, MemberId};

/// [`ChatProvider::page_bridge`] from the 产出, its 群's members and
/// [`ChatProvider::artifact_page_events`] (every declared name an `emit`).
pub fn default_bridge<P: ChatProvider + ?Sized>(
    p: &P,
    artifact_id: &str,
    version: u32,
) -> Option<PageBridge> {
    let detail = p.artifact(artifact_id)?;
    let author = detail
        .versions
        .iter()
        .find(|v| v.version == version)
        .map(|v| v.created_by.clone())
        .unwrap_or_else(|| detail.summary.created_by.clone());
    let in_group = p
        .groups()
        .into_iter()
        .find(|g| g.id == detail.group_id)
        .is_none_or(|g| g.member_ids.is_empty() || g.member_ids.contains(&author));
    let active_bot = p
        .member(&author)
        .is_some_and(|m| m.is_bot() && !m.deactivated);
    Some(PageBridge {
        author_active_bot: in_group && active_bot,
        runner: false,
        events: p
            .artifact_page_events(artifact_id, version)
            .into_iter()
            .map(|name| PageEventDecl {
                name,
                kind: PageEventKind::Emit,
                label: None,
            })
            .collect(),
        data_rev: 0,
        author_id: author,
    })
}

/// `emit` (fire and forget) or `ask` (the page waits for the member's reply).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PageEventKind {
    Emit,
    Ask,
}

/// One event a version declares.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PageEventDecl {
    pub name: String,
    pub kind: PageEventKind,
    /// Shown in the 产出 footer and in 页面里的回应 (≤ 40).
    pub label: Option<String>,
}

/// The bridge facts of one version: who made it and what its page may send.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct PageBridge {
    pub author_id: MemberId,
    /// A bot member of the 产出's 群 that is not deactivated (R10). Only then may the
    /// page `emit` or `ask`.
    pub author_active_bot: bool,
    /// The version came from a run of a runner member (the CLI on a machine): page `ask`
    /// replies go into that run's log (R7).
    pub runner: bool,
    pub events: Vec<PageEventDecl>,
    /// The latest data snapshot rev the server knows (0 = none).
    pub data_rev: u64,
}

/// How the member will answer, from the server's ack (`expect.mode`, INTERACT §6.4).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExpectMode {
    /// An SDK member is connected.
    Live,
    /// A runner member with an active run.
    Run,
    /// A runner member; a follow-up run starts.
    Starting,
    /// The member is not connected: nothing will answer now.
    Offline,
    /// Not interactive.
    None,
}

impl ExpectMode {
    /// `(soft, hard)` in milliseconds for the pending band (INTERACT §6.4).
    pub fn timings(self) -> Option<(u64, u64)> {
        match self {
            ExpectMode::Live => Some((8_000, 45_000)),
            ExpectMode::Run => Some((20_000, 120_000)),
            ExpectMode::Starting => Some((45_000, 180_000)),
            ExpectMode::Offline | ExpectMode::None => None,
        }
    }
}

/// Why a page event did not go out.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PageSendError {
    /// Not connected: page events are never queued.
    Offline,
    /// Refused here (bad name, too large) or not supported by this provider.
    Refused,
}

/// What the server sent back for pages, in order, on the main thread's channel.
#[derive(Clone, Debug, PartialEq)]
pub enum PageSignal {
    /// The server answered one of my page events.
    Acked {
        client_event_id: String,
        result: Result<PageAck, String>,
    },
    /// The member's reply to one of my page asks.
    Reply {
        event_id: String,
        ok: bool,
        payload: Option<Value>,
        /// `ok: false`: true for `declined`, false for `failed`.
        declined: bool,
    },
    /// A new data snapshot of a version (`data: None` when it is over 64 KiB: GET it).
    Data {
        artifact_id: String,
        version: u32,
        data_rev: u64,
        data: Option<Value>,
        cause: Option<String>,
    },
}

/// The ack of a page event.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PageAck {
    pub event_id: String,
    pub mode: ExpectMode,
}
