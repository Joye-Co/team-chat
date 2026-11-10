//! Client core without UI: local cache, sync engine and the server connection.
//!
//! ```ignore
//! let cfg = ClientConfig::new(base_url, token, data_dir);   // data_dir holds cache.sqlite3
//! let client = Client::new(cfg)?;                          // opens the cache, builds the runtime
//! let mut updates = client.subscribe();                    // ClientUpdate stream
//! client.start();                                          // connects in the background
//! let groups = client.groups()?;                           // synchronous cache reads
//! let id = client.send_message(&group_id, &topic_id, "hi", vec![])?;   // queued, returns at once
//! ```
//!
//! Reads never wait for the network; writes go to the outbox and are resent with the
//! same `clientMsgId` until acked. On wake from sleep or a network change, call
//! [`Client::reconnect_now`]. Before a client exists (sign-in), run
//! [`HttpClient::redeem`] on [`background`] and keep the token in a [`TokenStore`].

pub mod artifacts;
pub mod backoff;
pub mod client;
mod client_ext;
mod client_v11;
pub mod diagnostics;
mod engine;
pub mod files;
pub mod http;
mod http_ext;
pub mod logging;
pub mod memory;
pub mod proto;
pub mod runs;
pub mod search;
pub mod settings;
pub mod store;
pub mod token;
pub mod transient;
pub mod updates;

pub use artifacts::{LocalArtifactFile, NewVersion};
pub use client::{
    Client, ClientConfig, ClientError, ClientUpdate, ConnectionState, DropLiveEvent, EditRequest,
    Notification, SearchRequest, SendFailure, SendRequest, TopicPatch, background,
};
pub use client_ext::{ArtifactFolder, CancelRun};
pub use client_v11::{CardEventRequest, CardEventSent, SideChatRequest, SideChatStarted};
pub use files::{Phase, Progress, ProgressFn, UploadRequest, no_progress};
pub use http::{ApiError, HttpClient};
pub use memory::MemoryStatusFilter;
pub use runs::{ProcessItem, ProcessView, TerminalLine, terminal_lines};
pub use search::SearchError;
pub use store::{
    ArtifactRow, CardActionRow, CardAnswerRow, DecisionRow, DeskNoteRow, GroupRow, LogEntry,
    Migration, OutboxItem, OutboxOp, OutboxState, PendingRead, Refetch, Store, TodoReason, TodoRow,
    TopicRow,
};
#[cfg(target_os = "windows")]
pub use token::CredentialTokenStore;
#[cfg(target_os = "macos")]
pub use token::KeychainTokenStore;
pub use token::{FileTokenStore, TokenStore, TokenStoreError, credential_target};

/// Milliseconds since the Unix epoch.
pub fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// A new client-made id (ULID) for `clientMsgId` and local outbox rows.
pub fn new_id() -> String {
    ulid::Ulid::new().to_string()
}
