//! Client core without UI: local cache, sync engine and the server connection.
//!
//! The UI builds a [`Client`] from a [`ClientConfig`], calls [`Client::start`], reads the
//! cache through the synchronous query methods, and listens to [`ClientUpdate`] values.
//! No method that the UI calls waits for the network.

pub mod backoff;
pub mod client;
mod engine;
pub mod http;
pub mod logging;
pub mod proto;
pub mod store;
pub mod token;
pub mod transient;

pub use client::{background, Client, ClientConfig, ClientError, ClientUpdate, ConnectionState, SendFailure};
pub use http::{ApiError, HttpClient};
pub use store::{GroupRow, OutboxItem, OutboxOp, OutboxState, Store, TopicRow};
pub use token::{FileTokenStore, TokenStore, TokenStoreError};
#[cfg(target_os = "macos")]
pub use token::KeychainTokenStore;

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
