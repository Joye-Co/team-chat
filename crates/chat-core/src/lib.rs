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

pub mod backoff;
pub mod client;
mod engine;
pub mod http;
pub mod logging;
pub mod proto;
pub mod store;
pub mod token;
pub mod transient;

pub use client::{
    Client, ClientConfig, ClientError, ClientUpdate, ConnectionState, EditRequest, SearchRequest,
    SendFailure, SendRequest, TopicPatch, background,
};
pub use http::{ApiError, HttpClient};
pub use store::{
    GroupRow, LogEntry, Migration, OutboxItem, OutboxOp, OutboxState, PendingRead, Store, TopicRow,
};
#[cfg(target_os = "macos")]
pub use token::KeychainTokenStore;
pub use token::{FileTokenStore, TokenStore, TokenStoreError};

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
