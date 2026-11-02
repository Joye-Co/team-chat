//! Wire protocol types, version 1. The contract is docs/protocol.md; the Zod schemas in
//! packages/protocol are the executable source of truth.
//!
//! Conventions that match the Zod schemas:
//! - Field names are camelCase. Unknown fields are ignored on read and dropped on write.
//! - Optional fields are `Option<T>`: omitted when `None`, and `null` is rejected on read.
//! - Optional nullable fields (`x?: T | null`) are `Option<Option<T>>`: `None` = omitted,
//!   `Some(None)` = `null`, `Some(Some(v))` = a value. This keeps `null` on a round trip.
//! - Frames are tagged on `t`, events on `type`. An unknown tag fails to parse.
//!
//! Parse a server frame with [`parse_server_frame`]; encode a client frame with
//! [`encode_client_frame`], which runs [`ClientFrame::validate`] first.

pub mod audit;
pub mod bots;
pub mod card;
pub mod diagnostics;
pub mod entities;
pub mod errors;
pub mod events;
pub mod files;
pub mod frames;
pub mod hooks;
pub mod http;
pub mod limits;
pub mod machine;
pub mod members;
pub mod message;
pub mod reads;
pub mod search;
mod serde_util;
pub mod topic;
pub mod updates;

pub use audit::*;
pub use bots::*;
pub use card::*;
pub use diagnostics::*;
pub use entities::*;
pub use errors::*;
pub use events::*;
pub use files::*;
pub use frames::*;
pub use hooks::*;
pub use http::*;
pub use limits::*;
pub use members::*;
pub use message::*;
pub use reads::*;
pub use search::*;
pub use serde_util::{ProtocolV1, True};
pub use topic::*;
pub use updates::*;

pub const PROTOCOL_VERSION: u32 = 1;
